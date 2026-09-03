use crate::api::{MeetingDetails, MeetingTranscript};
use crate::database::models::{MeetingModel, Transcript};
use crate::database::repositories::chat::ChatRepository;
#[cfg(debug_assertions)]
use crate::database::repositories::retrieval::RetrievalRepository;
use chrono::Utc;
use sqlx::{Acquire, Error as SqlxError, SqliteConnection, SqlitePool};
use tracing::{error, info};

pub struct MeetingsRepository;

impl MeetingsRepository {
    pub async fn get_meetings(pool: &SqlitePool) -> Result<Vec<MeetingModel>, sqlx::Error> {
        let meetings = sqlx::query_as::<_, MeetingModel>(
            "SELECT m.*,
                    CASE WHEN (n.notes_markdown IS NOT NULL AND n.notes_markdown != '')
                       OR (n.notes_json IS NOT NULL AND n.notes_json != '') THEN 1 ELSE 0 END AS has_notes
             FROM meetings m
             LEFT JOIN meeting_notes n ON n.meeting_id = m.id
             ORDER BY m.created_at DESC",
        )
        .fetch_all(pool)
        .await?;
        Ok(meetings)
    }

    /// Deletes a meeting and, while the deletion transaction is still open
    /// (before the meeting row disappears), runs `invalidate_requests` so
    /// active chat requests whose prepared evidence references the meeting
    /// are cancelled through the shared request registry. Invalidation inside
    /// the transaction, before commit, is what makes the ordering coherent:
    /// once the deletion commits, every later publication observes a
    /// cancelled ownership token and suppresses. If the transaction rolls
    /// back after invalidation, the affected request is cancelled without a
    /// deletion (the privacy-safe direction).
    pub async fn delete_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
        invalidate_requests: impl Fn(&str),
    ) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;

        match delete_meeting_with_transaction(&mut transaction, meeting_id, &invalidate_requests)
            .await
        {
            Ok((success, _affected_generations)) => {
                if success {
                    transaction.commit().await?;
                    #[cfg(debug_assertions)]
                    if !_affected_generations.is_empty() {
                        let generation_ids = _affected_generations
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>();
                        if let Err(error) =
                            RetrievalRepository::reconcile_document_counts(pool, &generation_ids)
                                .await
                        {
                            log::warn!(
                                "retrieval document count reconciliation failed after meeting deletion {}: {}",
                                meeting_id,
                                error
                            );
                        }
                    }
                    info!(
                        "Successfully deleted meeting {} and all associated data",
                        meeting_id
                    );
                    Ok(true)
                } else {
                    transaction.rollback().await?;
                    Ok(false)
                }
            }
            Err(e) => {
                let _ = transaction.rollback().await;
                error!("Failed to delete meeting {}: {}", meeting_id, e);
                Err(e)
            }
        }
    }

    pub async fn get_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingDetails>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        // Get meeting details
        let meeting: Option<MeetingModel> = sqlx::query_as(
            "SELECT id, title, created_at, updated_at, folder_path, folder_id FROM meetings WHERE id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(&mut *transaction)
        .await?;

        if meeting.is_none() {
            transaction.rollback().await?;
            return Err(SqlxError::RowNotFound);
        }

        if let Some(meeting) = meeting {
            // Get all transcripts for this meeting
            let transcripts =
                sqlx::query_as::<_, Transcript>("SELECT * FROM transcripts WHERE meeting_id = ?")
                    .bind(meeting_id)
                    .fetch_all(&mut *transaction)
                    .await?;

            transaction.commit().await?;

            // Convert Transcript to MeetingTranscript
            let meeting_transcripts = transcripts
                .into_iter()
                .map(|t| MeetingTranscript {
                    id: t.id,
                    text: t.transcript,
                    timestamp: t.timestamp,
                    audio_start_time: t.audio_start_time,
                    audio_end_time: t.audio_end_time,
                    duration: t.duration,
                })
                .collect::<Vec<_>>();

            Ok(Some(MeetingDetails {
                id: meeting.id,
                title: meeting.title,
                created_at: meeting.created_at.0.to_rfc3339(),
                updated_at: meeting.updated_at.0.to_rfc3339(),
                folder_id: meeting.folder_id,
                transcripts: meeting_transcripts,
            }))
        } else {
            transaction.rollback().await?;
            Ok(None)
        }
    }

    /// Get meeting metadata without transcripts (for pagination)
    pub async fn get_meeting_metadata(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingModel>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let meeting: Option<MeetingModel> = sqlx::query_as(
            "SELECT id, title, created_at, updated_at, folder_path, folder_id FROM meetings WHERE id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?;

        Ok(meeting)
    }

    /// Get meeting transcripts with pagination support
    pub async fn get_meeting_transcripts_paginated(
        pool: &SqlitePool,
        meeting_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Transcript>, i64), SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        // Get total count of transcripts for this meeting
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?")
            .bind(meeting_id)
            .fetch_one(pool)
            .await?;

        // Get paginated transcripts ordered by audio_start_time
        let transcripts = sqlx::query_as::<_, Transcript>(
            "SELECT * FROM transcripts
             WHERE meeting_id = ?
             ORDER BY audio_start_time ASC
             LIMIT ? OFFSET ?",
        )
        .bind(meeting_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok((transcripts, total.0))
    }

    pub async fn update_meeting_title(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now().naive_utc();

        let rows_affected =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;
        if rows_affected.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn update_meeting_name(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        let mut transaction = pool.begin().await?;
        let now = Utc::now();

        // Update meetings table
        let meeting_update =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;

        if meeting_update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false); // Meeting not found
        }

        // Update transcript_chunks table
        sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
            .bind(new_title)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;
        Ok(true)
    }
}

async fn delete_meeting_with_transaction(
    transaction: &mut SqliteConnection,
    meeting_id: &str,
    invalidate_requests: &impl Fn(&str),
) -> Result<(bool, Vec<String>), SqlxError> {
    // Check if meeting exists
    let meeting_exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(&mut *transaction)
        .await?;

    if meeting_exists.is_none() {
        error!("Meeting {} not found for deletion", meeting_id);
        return Ok((false, Vec::new()));
    }

    #[cfg(debug_assertions)]
    let affected_generations: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT generation_id
         FROM retrieval_documents
         WHERE meeting_id = ?
         ORDER BY generation_id",
    )
    .bind(meeting_id)
    .fetch_all(&mut *transaction)
    .await?;
    #[cfg(not(debug_assertions))]
    let affected_generations = Vec::new();

    // Delete from related tables in proper order
    // 1. Delete from FTS index
    sqlx::query("DELETE FROM meeting_fts WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 2. Delete from transcript_chunks
    sqlx::query("DELETE FROM transcript_chunks WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 3. Delete from summary_processes
    sqlx::query("DELETE FROM summary_processes WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 4. Delete from meeting_notes
    sqlx::query("DELETE FROM meeting_notes WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 5. Delete from transcripts
    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 6. Keep per-generation document_count exact across the derived-row
    // cascade below: canonical replacements move that counter by incremental
    // deltas (never an O(corpus) recount inside their write transaction), so
    // a deletion performed outside that path applies its own decrement. The
    // MAX(...) clamp only guards pre-existing legacy drift from going
    // negative; steady state equals the true row count.
    sqlx::query(
        "UPDATE retrieval_generations
         SET document_count = MAX(document_count - (
                 SELECT COUNT(*) FROM retrieval_documents d
                  WHERE d.meeting_id = ?1
                    AND d.generation_id = retrieval_generations.generation_id), 0)
         WHERE EXISTS (
             SELECT 1 FROM retrieval_documents d2
              WHERE d2.meeting_id = ?1
                AND d2.generation_id = retrieval_generations.generation_id)",
    )
    .bind(meeting_id)
    .execute(&mut *transaction)
    .await?;

    // 7. Remove denormalized snippets/navigation while preserving answer text.
    ChatRepository::remove_meeting_sources_in_transaction(transaction, meeting_id)
        .await
        .map_err(|error| SqlxError::Protocol(error.to_string()))?;

    // 8. Cancel/invalidate active chat requests whose prepared evidence
    // references this meeting, inside the transaction and before the meeting
    // row disappears: once the deletion commits, any later publication sees
    // the cancelled ownership token and suppresses. After the source scrub so
    // a scrub failure (rollback) cannot cancel spuriously.
    invalidate_requests(meeting_id);

    // 9. Finally, delete the meeting
    let result = sqlx::query("DELETE FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    Ok((result.rows_affected() > 0, affected_generations))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::chat::ChatMessageRow;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    async fn deletion_test_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        for statement in [
            "CREATE TABLE meetings (id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL)",
            "CREATE TABLE meeting_fts (meeting_id TEXT)",
            "CREATE TABLE transcript_chunks (meeting_id TEXT)",
            "CREATE TABLE summary_processes (meeting_id TEXT)",
            "CREATE TABLE meeting_notes (meeting_id TEXT)",
            "CREATE TABLE transcripts (meeting_id TEXT)",
            // Minimal derived-retrieval subset mirroring the production
            // schema, since deletion now adjusts generation document counts
            // before the meeting cascade removes those rows.
            "CREATE TABLE retrieval_generations (generation_id TEXT PRIMARY KEY NOT NULL, model_id TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'building', document_count INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT '')",
            "CREATE TABLE retrieval_documents (id INTEGER PRIMARY KEY AUTOINCREMENT, generation_id TEXT NOT NULL REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE, document_id TEXT NOT NULL, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE, source_kind TEXT NOT NULL DEFAULT '', ordinal INTEGER NOT NULL DEFAULT 0, content TEXT NOT NULL DEFAULT '', content_hash BLOB NOT NULL DEFAULT x'', dimensions INTEGER NOT NULL DEFAULT 2 CHECK (dimensions > 0), vector_encoding TEXT NOT NULL DEFAULT 'int8', vector BLOB NOT NULL DEFAULT x'', source_revision INTEGER NOT NULL DEFAULT 0, updated_at TEXT NOT NULL DEFAULT '', UNIQUE (generation_id, document_id))",
            "CREATE INDEX retrieval_documents_by_meeting ON retrieval_documents(generation_id, meeting_id)",
            "CREATE INDEX retrieval_documents_by_meeting_lookup ON retrieval_documents(meeting_id, generation_id)",
            "CREATE TABLE chat_conversations (id TEXT PRIMARY KEY NOT NULL, meeting_id TEXT REFERENCES meetings(id) ON DELETE SET NULL, origin TEXT NOT NULL, scope_kind TEXT NOT NULL, scope_key TEXT NOT NULL, scope_data TEXT, promoted_from_live_scope_key TEXT, title TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
            "CREATE TABLE chat_messages (id TEXT PRIMARY KEY NOT NULL, conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE, role TEXT NOT NULL, content TEXT NOT NULL, sources_json TEXT, is_error INTEGER DEFAULT 0, created_at TEXT NOT NULL)",
            "CREATE TRIGGER chat_conversations_orphan_deleted_meeting AFTER UPDATE OF meeting_id ON chat_conversations WHEN OLD.meeting_id IS NOT NULL AND NEW.meeting_id IS NULL AND NEW.origin != 'global' BEGIN UPDATE chat_conversations SET scope_kind = 'orphaned_meeting' WHERE id = NEW.id; END",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn deletion_scrubs_chat_sources_and_fences_late_persistence() {
        let pool = deletion_test_pool().await;
        for (id, title) in [("delete-me", "Deleted"), ("keep-me", "Survivor")] {
            sqlx::query("INSERT INTO meetings (id, title) VALUES ($1, $2)")
                .bind(id)
                .bind(title)
                .execute(&pool)
                .await
                .unwrap();
        }
        for (id, meeting_id, origin, scope_kind, scope_key, promoted_key) in [
            ("global", None, "global", "all", "all", None),
            (
                "promoted",
                Some("delete-me"),
                "meeting",
                "meeting",
                "delete-me",
                Some("live-1"),
            ),
        ] {
            sqlx::query("INSERT INTO chat_conversations (id, meeting_id, origin, scope_kind, scope_key, promoted_from_live_scope_key, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
                .bind(id)
                .bind(meeting_id)
                .bind(origin)
                .bind(scope_kind)
                .bind(scope_key)
                .bind(promoted_key)
                .execute(&pool)
                .await
                .unwrap();
        }

        let mixed_sources = serde_json::json!([
            {"meetingId":"delete-me","meetingTitle":"Deleted","chunkType":"transcript","snippet":"private","sourceKind":"meeting"},
            {"meetingId":"keep-me","meetingTitle":"Survivor","chunkType":"note","snippet":"keep","sourceKind":"meeting"}
        ])
        .to_string();
        ChatRepository::save_message(
            &pool,
            "global",
            "assistant",
            "answer survives",
            Some(&mixed_sources),
            false,
        )
        .await
        .unwrap();
        let original_sources: String = sqlx::query_scalar(
            "SELECT sources_json FROM chat_messages WHERE content = 'answer survives'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::query("CREATE TRIGGER block_meeting_delete BEFORE DELETE ON meetings WHEN OLD.id = 'delete-me' BEGIN SELECT RAISE(ABORT, 'blocked'); END")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT sources_json FROM chat_messages WHERE content = 'answer survives'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            original_sources
        );
        sqlx::query("DROP TRIGGER block_meeting_delete")
            .execute(&pool)
            .await
            .unwrap();

        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .unwrap()
        );
        let retained: ChatMessageRow =
            sqlx::query_as("SELECT * FROM chat_messages WHERE content = 'answer survives'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(retained.content, "answer survives");
        let retained_sources: serde_json::Value =
            serde_json::from_str(retained.sources_json.as_deref().unwrap()).unwrap();
        assert_eq!(retained_sources.as_array().unwrap().len(), 1);
        assert_eq!(retained_sources[0]["meetingId"], "keep-me");

        ChatRepository::save_message(
            &pool,
            "global",
            "assistant",
            "late answer",
            Some(&mixed_sources),
            false,
        )
        .await
        .unwrap();
        let late_sources: String = sqlx::query_scalar(
            "SELECT sources_json FROM chat_messages WHERE content = 'late answer'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let late_sources: serde_json::Value = serde_json::from_str(&late_sources).unwrap();
        assert_eq!(late_sources.as_array().unwrap().len(), 1);
        assert_eq!(late_sources[0]["meetingId"], "keep-me");

        let late_live = serde_json::json!([{
            "meetingId":"live-1",
            "meetingTitle":"Live recording",
            "chunkType":"live_transcript",
            "snippet":"private live text",
            "sourceKind":"live_recording"
        }])
        .to_string();
        ChatRepository::save_message(
            &pool,
            "promoted",
            "assistant",
            "late promoted answer",
            Some(&late_live),
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT sources_json FROM chat_messages WHERE content = 'late promoted answer'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            None
        );
    }

    /// Source-scrub precision through the real deletion transaction: a
    /// malformed legacy payload is cleared only when it actually carries the
    /// deleted meeting as a meetingId value; unrelated malformed payloads and
    /// valid unrelated source arrays are preserved, and no deleted source can
    /// survive any shape.
    #[tokio::test]
    async fn deletion_scrubs_only_source_bearing_payloads_of_the_deleted_meeting() {
        let pool = deletion_test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO chat_conversations (id, meeting_id, origin, scope_kind, scope_key, created_at, updated_at) VALUES ('global', NULL, 'global', 'all', 'all', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        for (id, sources_json) in [
            // Malformed JSON that carries the deleted ID as a meetingId value: cleared.
            (
                "malformed-with-target",
                r#"not json at all "meetingId":"delete-me" broken"#,
            ),
            // High surrogate followed by a syntactically complete NON-low
            // code unit: rejected before arithmetic (no debug underflow), and
            // the payload carries no real pair: preserved.
            (
                "high-surrogate-non-low-pair",
                r#"broken {"meetingId":"\ud800\u0000"} tail"#,
            ),
            // High surrogate followed by a valid low surrogate: decodes to a
            // non-identifier character: preserved.
            (
                "high-surrogate-valid-pair",
                r#"broken {"meetingId":"\ud83e\udd80" tail"#,
            ),
            // Malformed JSON with the ID in a longer ID: preserved.
            (
                "malformed-longer-id",
                r#"broken "meetingId":"other-delete-me" payload"#,
            ),
            // Malformed JSON that only mentions the bare ID in snippet text: preserved.
            (
                "malformed-snippet-mention",
                r#"garbage {"snippet":"we discussed delete-me earlier""#,
            ),
            // Malformed JSON whose escaped quote form must not match: preserved.
            (
                "malformed-escaped",
                r#"{"snippet":"x \"meetingId\":\"delete-me\" y""#,
            ),
            // Escaped KEY spelling (\u006d = 'm'): cleared.
            (
                "escaped-key",
                r#"broken {"\u006deetingId":"delete-me" tail"#,
            ),
            // Escaped VALUE (\u002d = '-'): cleared.
            (
                "escaped-value",
                r#"garbage {"meetingId":"delete\u002dme" tail"#,
            ),
            // Both escaped with uppercase hex digits: cleared.
            (
                "escaped-both-uppercase-hex",
                r#"junk {"\u006DeetingId":"delete\u002Dme" end"#,
            ),
            // Surrogate-pair value (decodes to an emoji, not the ID): preserved.
            (
                "surrogate-value",
                r#"broken {"meetingId":"\ud83e\udd80" tail"#,
            ),
            // Missing colon: a key string followed by a value string is not a
            // key/value pair: preserved.
            ("missing-colon", r#"broken "meetingId" "delete-me" text"#),
            // Unquoted value: not a JSON string value: preserved.
            ("unquoted-value", r#"broken {"meetingId": delete-me }"#),
            // Invalid escape inside the key: the token cannot be decoded, so
            // no source-bearing pair is proven: preserved.
            (
                "invalid-escape-in-key",
                r#"broken "meetingId\uZZZZ" : "delete-me" tail"#,
            ),
            // Truncated document that still contains a real pair: cleared.
            ("truncated-with-pair", r#"[{"meetingId":"delete-me""#),
            // Unmatched quote/prefix BEFORE a later raw target pair: the
            // failed/stolen token must not hide the later pair: cleared.
            (
                "unmatched-prefix-raw-target",
                r#"broken "prefix {"meetingId":"delete-me""#,
            ),
            // Unmatched prefix before a later escaped VALUE: cleared.
            (
                "unmatched-prefix-escaped-value",
                r#"broken "prefix {"meetingId":"delete\u002dme""#,
            ),
            // Unmatched prefix before a later escaped KEY: cleared.
            (
                "unmatched-prefix-escaped-key",
                r#"broken "prefix {"\u006deetingId":"delete-me""#,
            ),
            // Unmatched prefix with a missing colon at the later key and an
            // unmatched prefix with an unquoted later value: preserved.
            (
                "unmatched-prefix-missing-colon",
                r#"broken "prefix {"meetingId" "delete-me""#,
            ),
            (
                "unmatched-prefix-unquoted-value",
                r#"broken "prefix {"meetingId": delete-me"#,
            ),
            // Whitespace forms around the colon: cleared.
            (
                "malformed-whitespace",
                "{ broken \"meetingId\" : \"delete-me\" junk",
            ),
            // Parseable non-array payload referencing the deleted meeting: cleared.
            (
                "non-array-with-target",
                r#"{"meetingId":"delete-me","snippet":"private"}"#,
            ),
            // Parseable nested non-array payload referencing the deleted
            // meeting regardless of field order: cleared.
            (
                "non-array-nested-target",
                r#"{"note":"unrelated","inner":{"other":1,"meetingId":"delete-me"}}"#,
            ),
            // Parseable non-array payload without the deleted meeting: preserved.
            ("non-array-without-target", r#"{"note":"unrelated"}"#),
            // Valid unrelated arrays (broad and meeting-scoped): preserved byte-identically.
            (
                "valid-array-unrelated",
                r#"[{"meetingId":"keep-me","meetingTitle":"Survivor","chunkType":"note","snippet":"keep","folderName":"","sourceKind":"meeting"}]"#,
            ),
        ] {
            sqlx::query(
                "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES (?, 'global', 'assistant', ?, ?, '2026-01-01T00:00:00Z')",
            )
            .bind(id)
            .bind(id)
            .bind(sources_json)
            .execute(&pool)
            .await
            .unwrap();
        }

        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .unwrap()
        );

        for (id, expected) in [
            ("malformed-with-target", None),
            (
                "high-surrogate-non-low-pair",
                Some(r#"broken {"meetingId":"\ud800\u0000"} tail"#),
            ),
            (
                "high-surrogate-valid-pair",
                Some(r#"broken {"meetingId":"\ud83e\udd80" tail"#),
            ),
            (
                "malformed-longer-id",
                Some(r#"broken "meetingId":"other-delete-me" payload"#),
            ),
            (
                "malformed-snippet-mention",
                Some(r#"garbage {"snippet":"we discussed delete-me earlier""#),
            ),
            (
                "malformed-escaped",
                Some(r#"{"snippet":"x \"meetingId\":\"delete-me\" y""#),
            ),
            ("escaped-key", None),
            ("escaped-value", None),
            ("escaped-both-uppercase-hex", None),
            (
                "surrogate-value",
                Some(r#"broken {"meetingId":"\ud83e\udd80" tail"#),
            ),
            (
                "missing-colon",
                Some(r#"broken "meetingId" "delete-me" text"#),
            ),
            (
                "unquoted-value",
                Some(r#"broken {"meetingId": delete-me }"#),
            ),
            (
                "invalid-escape-in-key",
                Some(r#"broken "meetingId\uZZZZ" : "delete-me" tail"#),
            ),
            ("truncated-with-pair", None),
            ("unmatched-prefix-raw-target", None),
            ("unmatched-prefix-escaped-value", None),
            ("unmatched-prefix-escaped-key", None),
            (
                "unmatched-prefix-missing-colon",
                Some(r#"broken "prefix {"meetingId" "delete-me""#),
            ),
            (
                "unmatched-prefix-unquoted-value",
                Some(r#"broken "prefix {"meetingId": delete-me"#),
            ),
            ("malformed-whitespace", None),
            ("non-array-with-target", None),
            ("non-array-nested-target", None),
            ("non-array-without-target", Some(r#"{"note":"unrelated"}"#)),
            (
                "valid-array-unrelated",
                Some(
                    r#"[{"meetingId":"keep-me","meetingTitle":"Survivor","chunkType":"note","snippet":"keep","folderName":"","sourceKind":"meeting"}]"#,
                ),
            ),
        ] {
            let persisted: Option<String> =
                sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                    .bind(id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(
                persisted.as_deref(),
                expected,
                "unexpected scrub outcome for {id}"
            );
        }

        // Adversarial small payloads: many short invalid quote fragments
        // before a later valid target. Actual-byte accounting must not let
        // the fragments exhaust the budget and hide the target; the same
        // fragment shape without any target stays inside the budget and is
        // preserved (no false positive / data loss).
        // (The main loop deleted the meeting; restore it for this section.)
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
            .execute(&pool)
            .await
            .unwrap();
        let fragments = r#""\q "\q "\q "\q "\q "#.repeat(64);
        for (id, sources_json, expect_cleared) in [
            (
                "adversarial-fragments-then-raw-target",
                format!(r#"{fragments}{{"meetingId":"delete-me""#),
                true,
            ),
            (
                "adversarial-fragments-then-escaped-value",
                format!(r#"{fragments}{{"meetingId":"delete\u002dme""#),
                true,
            ),
            (
                "adversarial-fragments-then-escaped-key",
                format!(r#"{fragments}{{"\u006deetingId":"delete-me""#),
                true,
            ),
        ] {
            sqlx::query(
                "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES (?, 'global', 'assistant', ?, ?, '2026-01-01T00:00:00Z')",
            )
            .bind(&id)
            .bind(&id)
            .bind(&sources_json)
            .execute(&pool)
            .await
            .unwrap();
            assert!(
                MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                    .await
                    .unwrap(),
                "deletion must succeed for {id}"
            );
            let persisted: Option<String> =
                sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                    .bind(&id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(
                persisted.is_none(),
                expect_cleared,
                "unexpected scrub outcome for {id}"
            );
            // Restore the deleted meeting and the message for the next case.
            sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM chat_messages WHERE content = ?")
                .bind(&id)
                .execute(&pool)
                .await
                .unwrap();
        }

        // Fragment shape without any target: well inside the budget, so the
        // payload must be preserved byte-identically (no false positive).
        let no_target = r#""\q "\q "\q "\q "\q "#.repeat(256);
        sqlx::query(
            "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES ('adversarial-fragments-without-target', 'global', 'assistant', 'adversarial-fragments-without-target', ?, '2026-01-01T00:00:00Z')",
        )
        .bind(&no_target)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .unwrap()
        );
        let persisted: Option<String> =
            sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                .bind("adversarial-fragments-without-target")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(persisted.as_deref(), Some(no_target.as_str()));

        // Budget boundary with a giant QUOTE-FREE tail: the tail walk is
        // metered per byte and stops at the cap — under budget the payload
        // (and any target inside it) is preserved/scanned, at/over budget it
        // clears fail-closed without traversing beyond the cap.
        for (id, tail_len, expect_cleared) in [
            (
                "tail-under-cap",
                crate::database::repositories::chat::MAX_SCAN_WORK_BYTES - 64,
                false,
            ),
            (
                "tail-at-cap",
                crate::database::repositories::chat::MAX_SCAN_WORK_BYTES,
                true,
            ),
            (
                "tail-over-cap",
                crate::database::repositories::chat::MAX_SCAN_WORK_BYTES + 64,
                true,
            ),
        ] {
            sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
                .execute(&pool)
                .await
                .unwrap();
            let payload = format!(r#""\q {}"#, "a".repeat(tail_len));
            sqlx::query(
                "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES (?, 'global', 'assistant', ?, ?, '2026-01-01T00:00:00Z')",
            )
            .bind(&id)
            .bind(&id)
            .bind(&payload)
            .execute(&pool)
            .await
            .unwrap();
            assert!(
                MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                    .await
                    .unwrap()
            );
            let persisted: Option<String> =
                sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                    .bind(&id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(
                persisted.is_none(),
                expect_cleared,
                "unexpected boundary outcome for {id}"
            );
        }

        // Giant WHITESPACE/inter-token gap under the budget: a later target
        // pair after the gap is still detected (resynchronization preserved).
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
            .execute(&pool)
            .await
            .unwrap();
        let gap_under = format!(r#""\q {}{{"meetingId":"delete-me""#, " ".repeat(100_000));
        sqlx::query(
            "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES ('gap-under-then-target', 'global', 'assistant', 'gap-under-then-target', ?, '2026-01-01T00:00:00Z')",
        )
        .bind(&gap_under)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .unwrap()
        );
        let persisted: Option<String> =
            sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                .bind("gap-under-then-target")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(persisted, None, "target after a giant gap must be cleared");

        // Giant whitespace gap OVER the budget: the metered walk stops at the
        // cap and clears fail-closed WITHOUT reaching the later target (the
        // payload is cleared, never silently preserved).
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
            .execute(&pool)
            .await
            .unwrap();
        let gap_over = format!(
            r#""\q {}{{"meetingId":"delete-me""#,
            " ".repeat(crate::database::repositories::chat::MAX_SCAN_WORK_BYTES + 64)
        );
        sqlx::query(
            "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES ('gap-over-then-target', 'global', 'assistant', 'gap-over-then-target', ?, '2026-01-01T00:00:00Z')",
        )
        .bind(&gap_over)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .unwrap()
        );
        let persisted: Option<String> =
            sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                .bind("gap-over-then-target")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            persisted, None,
            "an over-budget gap must clear fail-closed without unbounded traversal"
        );

        // Giant whitespace gap between the KEY and its value, over budget:
        // skip_json_whitespace exhaustion clears fail-closed too.
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
            .execute(&pool)
            .await
            .unwrap();
        let key_value_gap = format!(
            r#"broken {{"meetingId":{}"delete-me""#,
            " ".repeat(crate::database::repositories::chat::MAX_SCAN_WORK_BYTES + 64)
        );
        sqlx::query(
            "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES ('gap-over-key-value', 'global', 'assistant', 'gap-over-key-value', ?, '2026-01-01T00:00:00Z')",
        )
        .bind(&key_value_gap)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .unwrap()
        );
        let persisted: Option<String> =
            sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                .bind("gap-over-key-value")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            persisted, None,
            "an over-budget key/value whitespace gap must clear fail-closed"
        );

        // Every token failure form followed by a later valid target pair: the
        // target is still reached well below the actual budget (no failure
        // path hides or exhausts it).
        for (id, sources_json) in [
            (
                "failure-trailing-backslash-then-target",
                format!(r#"broken "abc\ {{"meetingId":"delete-me""#),
            ),
            (
                "failure-lone-low-surrogate-then-target",
                format!(r#"broken "\udc00" {{"meetingId":"delete-me""#),
            ),
            (
                "failure-control-byte-then-target",
                format!("broken \"\u{1}x {{\"meetingId\":\"delete-me\""),
            ),
            (
                "failure-overlong-token-then-target",
                format!(
                    r#"broken "{}x" {{"meetingId":"delete-me""#,
                    "x".repeat(5000)
                ),
            ),
        ] {
            sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES (?, 'global', 'assistant', ?, ?, '2026-01-01T00:00:00Z')",
            )
            .bind(&id)
            .bind(&id)
            .bind(&sources_json)
            .execute(&pool)
            .await
            .unwrap();
            assert!(
                MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                    .await
                    .unwrap()
            );
            let persisted: Option<String> =
                sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                    .bind(&id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(
                persisted, None,
                "a later target must survive every failure form: {id}"
            );
        }

        // A fully \u-escaped REAL-size meeting id is still decoded, matched,
        // and scrubbed (the cap counts probed raw bytes; a real id always
        // fits).
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
            .execute(&pool)
            .await
            .unwrap();
        let fully_escaped_value =
            r#"{"meetingId":"\u0064\u0065\u006c\u0065\u0074\u0065\u002d\u006d\u0065"}"#;
        sqlx::query(
            "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES ('escaped-real-target', 'global', 'assistant', 'escaped-real-target', ?, '2026-01-01T00:00:00Z')",
        )
        .bind(fully_escaped_value)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .unwrap()
        );
        let persisted: Option<String> =
            sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                .bind("escaped-real-target")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            persisted, None,
            "a fully escaped real target must be scrubbed"
        );

        // An over-cap target-like escaped token cannot alter scan behavior:
        // alone it is preserved (it cannot be a source field for a real
        // meeting id), while a real pair elsewhere in the same payload is
        // still found and scrubbed (no evasion).
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
            .execute(&pool)
            .await
            .unwrap();
        let over_cap_token = format!(
            r#"broken "{}" real {{"meetingId":"delete-me""#,
            r#"\u0041"#.repeat(700)
        );
        sqlx::query(
            "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES ('over-cap-token-with-real-pair', 'global', 'assistant', 'over-cap-token-with-real-pair', ?, '2026-01-01T00:00:00Z')",
        )
        .bind(&over_cap_token)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .unwrap()
        );
        let persisted: Option<String> =
            sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                .bind("over-cap-token-with-real-pair")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            persisted, None,
            "the real pair after an over-cap token must still be scrubbed"
        );

        // The over-cap target-like token ALONE (no real pair): preserved —
        // it cannot be a source field for any real meeting id.
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('delete-me', 'Deleted')")
            .execute(&pool)
            .await
            .unwrap();
        let over_cap_alone = format!(r#"broken "{}" tail"#, r#"\u0064"#.repeat(700));
        sqlx::query(
            "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES ('over-cap-token-alone', 'global', 'assistant', 'over-cap-token-alone', ?, '2026-01-01T00:00:00Z')",
        )
        .bind(&over_cap_alone)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .unwrap()
        );
        let persisted: Option<String> =
            sqlx::query_scalar("SELECT sources_json FROM chat_messages WHERE content = ?")
                .bind("over-cap-token-alone")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            persisted.as_deref(),
            Some(over_cap_alone.as_str()),
            "an over-cap token without a real pair must be preserved"
        );
    }
    /// Covers the deletion path end to end: the DISTINCT generation lookup must
    /// pick up exactly the generations that held the deleted meeting's rows, the
    /// decrement must leave those counters matching the true row count, and
    /// generations that held nothing must be left untouched (pre-existing drift
    /// on them is still reported, proving the lookup did not over-collect).
    #[tokio::test]
    async fn deletion_reconciles_document_counts_for_affected_generations_only() {
        use crate::database::repositories::retrieval::RetrievalRepository;

        let pool = deletion_test_pool().await;
        for (id, title) in [("delete-me", "Deleted"), ("keep-me", "Survivor")] {
            sqlx::query("INSERT INTO meetings (id, title) VALUES ($1, $2)")
                .bind(id)
                .bind(title)
                .execute(&pool)
                .await
                .unwrap();
        }
        // `holds-docs` owns two rows for the deleted meeting and one for the
        // survivor; `no-docs` owns nothing and carries deliberate drift.
        for (generation_id, document_count) in [("holds-docs", 3), ("no-docs", 9)] {
            sqlx::query(
                "INSERT INTO retrieval_generations (generation_id, document_count) VALUES ($1, $2)",
            )
            .bind(generation_id)
            .bind(document_count)
            .execute(&pool)
            .await
            .unwrap();
        }
        for (document_id, meeting_id) in [
            ("d-1", "delete-me"),
            ("d-2", "delete-me"),
            ("d-3", "keep-me"),
        ] {
            sqlx::query(
                "INSERT INTO retrieval_documents (generation_id, document_id, meeting_id)
                 VALUES ('holds-docs', $1, $2)",
            )
            .bind(document_id)
            .bind(meeting_id)
            .execute(&pool)
            .await
            .unwrap();
        }

        assert!(
            MeetingsRepository::delete_meeting(&pool, "delete-me", |_| {})
                .await
                .unwrap()
        );

        // The affected generation's counter tracked the cascade exactly, so
        // reconciliation reports nothing for it.
        let tracked: i64 = sqlx::query_scalar(
            "SELECT document_count FROM retrieval_generations WHERE generation_id = 'holds-docs'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(tracked, 1);
        assert!(
            RetrievalRepository::reconcile_document_counts(&pool, &["holds-docs"])
                .await
                .unwrap()
                .is_empty()
        );

        // The generation that held no rows for this meeting was not decremented,
        // so its pre-existing drift survives and is still detected.
        let untouched: i64 = sqlx::query_scalar(
            "SELECT document_count FROM retrieval_generations WHERE generation_id = 'no-docs'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(untouched, 9);
        assert_eq!(
            RetrievalRepository::reconcile_document_counts(&pool, &["no-docs"])
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
