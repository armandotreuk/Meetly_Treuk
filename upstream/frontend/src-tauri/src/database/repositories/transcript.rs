use crate::api::{TranscriptSearchResult, TranscriptSegment};
use crate::database::repositories::chat::ChatRepository;
use crate::database::repositories::fts::FtsRepository;
use crate::database::repositories::meeting_notes::MeetingNotesRepository;
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    /// If `folder_path` holds a non-empty `notes.md` (in-meeting notes mirror),
    /// it is imported into `meeting_notes` after the commit — and re-run on an
    /// idempotent retry so a failed import heals itself.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
        live_scope_key: Option<&str>,
    ) -> Result<String, SqlxError> {
        if let Some(live_scope_key) = live_scope_key {
            if let Some(meeting_id) = ChatRepository::get_promoted_meeting_id(pool, live_scope_key)
                .await
                .map_err(|error| SqlxError::Protocol(error.to_string()))?
            {
                // Idempotent retry: the first save committed but its post-commit
                // notes import may have failed — re-run it so the retry heals.
                Self::import_notes_and_refresh_fts(pool, &meeting_id, folder_path.as_deref())
                    .await?;
                return Ok(meeting_id);
            }
        }
        let meeting_id = format!("meeting-{}", Uuid::new_v4());

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();

        // Try to read created_at from metadata.json (recording start time)
        // to avoid using completion time as the meeting's created_at.
        let created_at = folder_path
            .as_ref()
            .and_then(|p| {
                std::fs::read_to_string(std::path::Path::new(p).join("metadata.json")).ok()
            })
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|v| v.get("created_at")?.as_str()?.to_string().parse().ok())
            .unwrap_or(now);

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at, folder_path) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(created_at)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting '{}': {}", meeting_title, e);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        for segment in transcripts {
            let transcript_id = format!("transcript-{}", Uuid::new_v4());
            let result = sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration)
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&transcript_id)
            .bind(&meeting_id)
            .bind(&segment.text)
            .bind(&segment.timestamp)
            .bind(segment.audio_start_time)
            .bind(segment.audio_end_time)
            .bind(segment.duration)
            .execute(&mut *transaction)
            .await;

            if let Err(e) = result {
                error!(
                    "Failed to save transcript segment for meeting {}: {}",
                    meeting_id, e
                );
                transaction.rollback().await?;
                return Err(e);
            }
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        if let Some(live_scope_key) = live_scope_key {
            ChatRepository::promote_live_recording_in_transaction(
                &mut transaction,
                live_scope_key,
                &meeting_id,
            )
            .await
            .map_err(|error| SqlxError::Protocol(error.to_string()))?;
        }

        // Commit the transaction
        transaction.commit().await?;

        Self::import_notes_and_refresh_fts(pool, &meeting_id, folder_path.as_deref()).await?;

        Ok(meeting_id)
    }

    /// Import in-meeting notes mirrored during recording, then refresh the FTS
    /// index. Notes import failures propagate (the meeting is already committed);
    /// the FTS refresh is best-effort. Shared by the first-save and idempotent
    /// retry paths so a retry heals whatever the earlier call left stale.
    async fn import_notes_and_refresh_fts(
        pool: &SqlitePool,
        meeting_id: &str,
        folder_path: Option<&str>,
    ) -> Result<(), SqlxError> {
        if let Some(path) = folder_path {
            if let Ok(notes) = std::fs::read_to_string(std::path::Path::new(path).join("notes.md"))
            {
                let notes_json =
                    std::fs::read_to_string(std::path::Path::new(path).join("notes.json")).ok();
                if !notes.trim().is_empty() || notes_json.is_some() {
                    MeetingNotesRepository::save_notes(
                        pool,
                        meeting_id,
                        Some(&notes),
                        notes_json.as_deref(),
                    )
                    .await
                    .map_err(|e| SqlxError::Protocol(e.to_string()))?;
                }
            }
        }

        // Update FTS index — best-effort; a failure here doesn't invalidate
        // the transcript data we just committed.
        if let Err(e) = FtsRepository::refresh_meeting(pool, meeting_id).await {
            error!("Failed to refresh FTS for meeting {}: {}", meeting_id, e);
        }

        Ok(())
    }

    /// Searches for a query string within the transcripts.
    /// It returns a list of matching transcripts with context.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let search_query = format!("%{}%", query.to_lowercase());

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT m.id, m.title, t.transcript, t.timestamp
             FROM meetings m
             JOIN transcripts t ON m.id = t.meeting_id
             WHERE LOWER(t.transcript) LIKE ?",
        )
        .bind(&search_query)
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|(id, title, transcript, timestamp)| {
                let match_context = Self::get_match_context(&transcript, query);
                TranscriptSearchResult {
                    id,
                    title,
                    match_context,
                    timestamp,
                }
            })
            .collect();

        Ok(results)
    }

    /// Helper function to extract a snippet of text around the first match of a query.
    fn get_match_context(transcript: &str, query: &str) -> String {
        let transcript_lower = transcript.to_lowercase();
        let query_lower = query.to_lowercase();

        match transcript_lower.find(&query_lower) {
            Some(match_index) => {
                let start_index = match_index.saturating_sub(100);
                let end_index = (match_index + query.len() + 100).min(transcript.len());

                let mut context = String::new();
                if start_index > 0 {
                    context.push_str("...");
                }
                context.push_str(&transcript[start_index..end_index]);
                if end_index < transcript.len() {
                    context.push_str("...");
                }
                context
            }
            None => transcript.chars().take(200).collect(), // Fallback to the start of the transcript
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    async fn test_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE meetings (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                saved_at TEXT,
                folder_path TEXT,
                folder_id TEXT
            );
            CREATE TABLE transcripts (
                id TEXT PRIMARY KEY,
                meeting_id TEXT NOT NULL,
                transcript TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                speaker TEXT,
                audio_start_time REAL,
                audio_end_time REAL,
                duration REAL
            );
            CREATE TABLE chat_conversations (
                id TEXT PRIMARY KEY NOT NULL,
                meeting_id TEXT REFERENCES meetings(id) ON DELETE SET NULL,
                origin TEXT NOT NULL DEFAULT 'meeting',
                scope_kind TEXT,
                scope_key TEXT,
                scope_data TEXT,
                promoted_from_live_scope_key TEXT UNIQUE,
                title TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE chat_messages (
                id TEXT PRIMARY KEY NOT NULL,
                conversation_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                sources_json TEXT,
                is_error INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES chat_conversations(id) ON DELETE CASCADE
            );
            CREATE TABLE meeting_notes (
                meeting_id TEXT PRIMARY KEY NOT NULL,
                notes_markdown TEXT,
                notes_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
                CHECK (length(notes_markdown) < 20)
            );
            CREATE TABLE meeting_folders (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                parent_id TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE summary_processes (
                meeting_id TEXT NOT NULL,
                template_id TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                result TEXT,
                PRIMARY KEY (meeting_id, template_id)
            );
            CREATE VIRTUAL TABLE meeting_fts USING fts5(
                meeting_id UNINDEXED,
                chunk_type UNINDEXED,
                chunk_id UNINDEXED,
                text,
                speaker UNINDEXED,
                timestamp_label UNINDEXED,
                folder_id UNINDEXED,
                folder_name,
                tokenize = 'unicode61'
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn idempotent_retry_heals_notes_import_and_fts() {
        let pool = test_pool().await;
        // A live conversation is required so the first save records promotion
        // lineage, which the retry early-returns on.
        sqlx::query(
            "INSERT INTO chat_conversations (id, origin, scope_kind, scope_key, created_at, updated_at)
             VALUES ('live-conv', 'live_recording', 'live_recording', 'live-1', '2026-08-17T00:00:00Z', '2026-08-17T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let folder = tempfile::tempdir().unwrap();
        // An oversized note trips the test schema's CHECK, so the first save
        // commits the meeting + promotion but fails the post-commit notes import.
        std::fs::write(folder.path().join("notes.md"), "x".repeat(40)).unwrap();

        let segment = TranscriptSegment {
            id: "seg-1".to_string(),
            text: "Migration decision recorded".to_string(),
            timestamp: "14:32".to_string(),
            audio_start_time: Some(0.0),
            audio_end_time: Some(3.0),
            duration: Some(3.0),
        };
        let first = TranscriptsRepository::save_transcript(
            &pool,
            "Planning",
            &[segment],
            Some(folder.path().to_string_lossy().into_owned()),
            Some("live-1"),
        )
        .await;
        assert!(
            first.is_err(),
            "first save must fail at the post-commit notes import"
        );

        let meeting_id: String = sqlx::query_scalar(
            "SELECT meeting_id FROM chat_conversations WHERE promoted_from_live_scope_key = 'live-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            MeetingNotesRepository::get_notes(&pool, &meeting_id)
                .await
                .unwrap()
                .is_none(),
            "failed notes import must leave no notes row"
        );
        let fts_rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM meeting_fts WHERE meeting_id = ?")
                .bind(&meeting_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            fts_rows, 0,
            "no FTS refresh may run when the notes import fails"
        );

        // Fix the note file; the retry must heal instead of early-returning
        // the promoted meeting untouched.
        std::fs::write(folder.path().join("notes.md"), "Budget approved").unwrap();
        let retried = TranscriptsRepository::save_transcript(
            &pool,
            "Planning",
            &[],
            Some(folder.path().to_string_lossy().into_owned()),
            Some("live-1"),
        )
        .await
        .unwrap();
        assert_eq!(
            retried, meeting_id,
            "retry must return the same promoted meeting"
        );

        let note = MeetingNotesRepository::get_notes(&pool, &meeting_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(note.notes_markdown.as_deref(), Some("Budget approved"));
        let fts_rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM meeting_fts WHERE meeting_id = ?")
                .bind(&meeting_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            fts_rows, 2,
            "retry must rebuild transcript + note FTS chunks"
        );
    }
}
