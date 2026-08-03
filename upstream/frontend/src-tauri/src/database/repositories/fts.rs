use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::LazyLock;
use tracing::info;

use crate::database::repositories::folder::FolderRepository;

static FOLDER_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"(?i)folder:"([^"]*)""#).unwrap());

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FtsSearchResult {
    pub meeting_id: String,
    pub meeting_title: String,
    #[serde(rename = "chunkType")]
    pub chunk_type: String,
    #[serde(rename = "chunkId")]
    pub chunk_id: String,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    #[serde(rename = "timestampLabel", skip_serializing_if = "Option::is_none")]
    pub timestamp_label: Option<String>,
    #[serde(rename = "folderId", skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(rename = "folderName")]
    pub folder_name: String,
    pub rank: f64,
}

/// Parsed search query with optional folder filter extracted from
/// `folder:"Name"` prefix syntax.
struct ParsedQuery {
    fts_query: String,
    folder_id: Option<String>,
}

/// Extract `folder:"..."` from the raw query, resolve it to a folder_id,
/// and return the cleaned FTS query + optional folder_id filter.
///
/// ponytail: naive parse — finds the last `folder:"` pair. If the user
/// writes multiple folder operators, only the last one takes effect.
/// FTS5 column filters (`folder_name:"..."`) would be more natural but
/// require the column to be indexed (it is), so this is a convenience
/// alias that also resolves the folder name to its id for a WHERE filter.
async fn parse_query(pool: &SqlitePool, raw: &str) -> ParsedQuery {
    if let Some(caps) = FOLDER_RE.captures(raw) {
        let folder_name = caps.get(1).unwrap().as_str();
        let match_end = caps.get(0).unwrap().end();
        let fts_query = raw[match_end..].trim().to_string();
        // Resolve folder name to id
        match FolderRepository::get_all(pool).await {
            Ok(folders) => {
                if let Some(folder) = folders.iter().find(|f| {
                    f.name.eq_ignore_ascii_case(folder_name)
                }) {
                    return ParsedQuery {
                        fts_query,
                        folder_id: Some(folder.id.clone()),
                    };
                }
                // Folder name not found — return query without folder filter
                ParsedQuery {
                    fts_query,
                    folder_id: None,
                }
            }
            Err(_) => ParsedQuery {
                fts_query,
                folder_id: None,
            },
        }
    } else {
        ParsedQuery {
            fts_query: raw.trim().to_string(),
            folder_id: None,
        }
    }
}

pub struct FtsRepository;

impl FtsRepository {
    /// Full-text search across transcripts, summaries, and notes.
    /// Supports `folder:"name"` operator in the query string.
    pub async fn search(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        if raw_query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let parsed = parse_query(pool, raw_query).await;

        if parsed.fts_query.is_empty() {
            return Ok(Vec::new());
        }

        // Escape FTS5 special chars in the user query to prevent syntax errors.
        // Only allow simple term searches; no boolean operators or column filters.
        let safe_query = sanitize_fts_query(&parsed.fts_query);

        let rows: Vec<(
            String, String, String, String, String,
            Option<String>, Option<String>, Option<String>,
            String, f64,
        )> = if let Some(ref folder_id) = parsed.folder_id {
            sqlx::query_as(
                r#"
                SELECT
                    fts.meeting_id,
                    m.title,
                    fts.chunk_type,
                    fts.chunk_id,
                    snippet(meeting_fts, 3, '<mark>', '</mark>', '...', 48) AS snippet,
                    fts.speaker,
                    fts.timestamp_label,
                    fts.folder_id,
                    COALESCE(fts.folder_name, '') AS folder_name,
                    bm25(meeting_fts, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0, 0.5, 0.5) AS rank
                FROM meeting_fts fts
                JOIN meetings m ON fts.meeting_id = m.id
                WHERE meeting_fts MATCH ?1
                  AND fts.folder_id = ?2
                ORDER BY rank
                LIMIT ?3
                "#,
            )
            .bind(&safe_query)
            .bind(folder_id)
            .bind(limit)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT
                    fts.meeting_id,
                    m.title,
                    fts.chunk_type,
                    fts.chunk_id,
                    snippet(meeting_fts, 3, '<mark>', '</mark>', '...', 48) AS snippet,
                    fts.speaker,
                    fts.timestamp_label,
                    fts.folder_id,
                    COALESCE(fts.folder_name, '') AS folder_name,
                    bm25(meeting_fts, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0, 0.5, 0.5) AS rank
                FROM meeting_fts fts
                JOIN meetings m ON fts.meeting_id = m.id
                WHERE meeting_fts MATCH ?1
                ORDER BY rank
                LIMIT ?2
                "#,
            )
            .bind(&safe_query)
            .bind(limit)
            .fetch_all(pool)
            .await?
        };

        let results: Vec<FtsSearchResult> = rows
            .into_iter()
            .map(
                |(meeting_id, title, chunk_type, chunk_id, snippet, speaker, timestamp_label, folder_id, folder_name, rank)| {
                    FtsSearchResult {
                        meeting_id,
                        meeting_title: title,
                        chunk_type,
                        chunk_id,
                        snippet,
                        speaker,
                        timestamp_label,
                        folder_id,
                        folder_name,
                        rank,
                    }
                },
            )
            .collect();

        info!(
            "FTS search for '{}' (folder={:?}) returned {} results",
            parsed.fts_query, parsed.folder_id, results.len()
        );
        Ok(results)
    }

    /// Delete all FTS rows for a meeting and re-insert from current data.
    /// Called after transcript save or summary completion.
    pub async fn refresh_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<(), sqlx::Error> {
        // Remove old rows
        Self::remove_meeting(pool, meeting_id).await?;

        // Re-insert transcripts
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                t.meeting_id, 'transcript', t.id, t.transcript, t.speaker, t.timestamp,
                m.folder_id, COALESCE(f.name, '')
            FROM transcripts t
            JOIN meetings m ON t.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE t.meeting_id = ?1
              AND t.transcript IS NOT NULL AND t.transcript != ''
            "#,
        )
        .bind(meeting_id)
        .execute(pool)
        .await?;

        // Re-insert summaries
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                sp.meeting_id, 'summary', sp.meeting_id || ':' || sp.template_id,
                json_extract(sp.result, '$.markdown'), NULL, NULL,
                m.folder_id, COALESCE(f.name, '')
            FROM summary_processes sp
            JOIN meetings m ON sp.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE sp.meeting_id = ?1
              AND sp.result IS NOT NULL
              AND json_extract(sp.result, '$.markdown') IS NOT NULL
              AND json_extract(sp.result, '$.markdown') != ''
            "#,
        )
        .bind(meeting_id)
        .execute(pool)
        .await?;

        // Re-insert notes
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                mn.meeting_id, 'note', mn.meeting_id,
                mn.notes_markdown, NULL, NULL,
                m.folder_id, COALESCE(f.name, '')
            FROM meeting_notes mn
            JOIN meetings m ON mn.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE mn.meeting_id = ?1
              AND mn.notes_markdown IS NOT NULL AND mn.notes_markdown != ''
            "#,
        )
        .bind(meeting_id)
        .execute(pool)
        .await?;

        info!("Refreshed FTS index for meeting {}", meeting_id);
        Ok(())
    }

    /// Remove all FTS rows for a meeting.
    pub async fn remove_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM meeting_fts WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Full rebuild: delete everything and re-populate from source tables.
    pub async fn rebuild_index(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
        sqlx::query("DELETE FROM meeting_fts").execute(pool).await?;

        // Re-insert all transcripts
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                t.meeting_id, 'transcript', t.id, t.transcript, t.speaker, t.timestamp,
                m.folder_id, COALESCE(f.name, '')
            FROM transcripts t
            JOIN meetings m ON t.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE t.transcript IS NOT NULL AND t.transcript != ''
            "#,
        )
        .execute(pool)
        .await?;

        // Re-insert all summaries
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                sp.meeting_id, 'summary', sp.meeting_id || ':' || sp.template_id,
                json_extract(sp.result, '$.markdown'), NULL, NULL,
                m.folder_id, COALESCE(f.name, '')
            FROM summary_processes sp
            JOIN meetings m ON sp.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE sp.result IS NOT NULL
              AND json_extract(sp.result, '$.markdown') IS NOT NULL
              AND json_extract(sp.result, '$.markdown') != ''
            "#,
        )
        .execute(pool)
        .await?;

        // Re-insert all notes
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                mn.meeting_id, 'note', mn.meeting_id,
                mn.notes_markdown, NULL, NULL,
                m.folder_id, COALESCE(f.name, '')
            FROM meeting_notes mn
            JOIN meetings m ON mn.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE mn.notes_markdown IS NOT NULL AND mn.notes_markdown != ''
            "#,
        )
        .execute(pool)
        .await?;

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meeting_fts")
            .fetch_one(pool)
            .await?;
        info!("FTS index rebuilt: {} rows", count.0);
        Ok(count.0 as u64)
    }

    /// Update folder_name in all FTS rows for a given folder_id.
    /// Called after folder rename.
    pub async fn sync_folder(
        pool: &SqlitePool,
        folder_id: &str,
    ) -> Result<(), sqlx::Error> {
        // Fetch new folder name
        let folder = FolderRepository::get_by_id(pool, folder_id).await?;
        let new_name = match folder {
            Some(f) => f.name,
            None => {
                // Folder was deleted — clear the name in FTS rows
                sqlx::query("UPDATE meeting_fts SET folder_name = '' WHERE folder_id = ?")
                    .bind(folder_id)
                    .execute(pool)
                    .await?;
                return Ok(());
            }
        };

        sqlx::query("UPDATE meeting_fts SET folder_name = ? WHERE folder_id = ?")
            .bind(&new_name)
            .bind(folder_id)
            .execute(pool)
            .await?;

        info!("Synced FTS folder_name for folder {} -> '{}'", folder_id, new_name);
        Ok(())
    }
}

/// Sanitize a user query string for FTS5 MATCH.
/// Removes characters that could break FTS5 query syntax (quotes, operators,
/// colons). AND/OR/NOT are preserved as literal search terms — they will be
/// matched as regular words, not as FTS5 boolean operators.
/// ponytail: intentionally naive — simple term matching only.
fn sanitize_fts_query(query: &str) -> String {
    // Remove FTS5 double-quote syntax to prevent injection
    let cleaned = query.replace('"', "");
    // Remove FTS5 prefix operators, colons, and parameter markers
    let cleaned = cleaned
        .replace('-', " ")
        .replace('+', " ")
        .replace('*', " ")
        .replace(':', " ")
        .replace('?', " ");
    // Collapse whitespace, then join with OR so FTS5 matches any term
    // instead of requiring all terms (AND semantics).
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    parts
        .iter()
        .map(|w| format!("\"{}\"", w))
        .collect::<Vec<_>>()
        .join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    #[test]
    fn sanitize_removes_fts_operators() {
        assert_eq!(
            sanitize_fts_query(r#"risk AND migration"#),
            "\"risk\" OR \"AND\" OR \"migration\""
        );
        assert_eq!(
            sanitize_fts_query(r#"risk "quoted""#),
            "\"risk\" OR \"quoted\""
        );
        assert_eq!(
            sanitize_fts_query(r#"-risk +migration"#),
            "\"risk\" OR \"migration\""
        );
        assert_eq!(
            sanitize_fts_query(r#"folder:"Sprint 14" risk"#),
            "\"folder\" OR \"Sprint\" OR \"14\" OR \"risk\""
        );
    }

    #[test]
    fn sanitize_collapses_whitespace() {
        assert_eq!(sanitize_fts_query("  hello   world  "), "\"hello\" OR \"world\"");
    }

    async fn setup_fts_db() -> SqlitePool {
        let pool = SqlitePool::connect(":memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE meetings (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                folder_path TEXT,
                folder_id TEXT
            );
            CREATE TABLE meeting_folders (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                parent_id TEXT,
                created_at TEXT NOT NULL
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
            CREATE TABLE summary_processes (
                meeting_id TEXT NOT NULL,
                template_id TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                result TEXT,
                PRIMARY KEY (meeting_id, template_id)
            );
            CREATE TABLE meeting_notes (
                meeting_id TEXT PRIMARY KEY NOT NULL,
                notes_markdown TEXT
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
        .expect("create schema");
        pool
    }

    #[tokio::test]
    async fn search_finds_transcript_text() {
        let pool = setup_fts_db().await;

        // Seed meeting
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m1")
            .bind("Sprint Planning")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        // Seed transcript
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)")
            .bind("t1")
            .bind("m1")
            .bind("We discussed the migration risk and decided to use the outbox pattern for Kafka")
            .bind("14:32")
            .execute(&pool)
            .await
            .unwrap();

        // Populate FTS
        FtsRepository::refresh_meeting(&pool, "m1").await.unwrap();

        // Search
        let results = FtsRepository::search(&pool, "migration risk", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meeting_id, "m1");
        assert_eq!(results[0].chunk_type, "transcript");
        assert!(results[0].snippet.contains("<mark>"));
    }

    #[tokio::test]
    async fn search_finds_summary_text() {
        let pool = setup_fts_db().await;

        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m2")
            .bind("Architecture Review")
            .bind("2026-07-27T11:00:00Z")
            .bind("2026-07-27T11:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        let result_json = r#"{"markdown":"Decision: migrate to event-driven architecture with CQRS"}"#;
        sqlx::query("INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES (?, ?, ?, ?, ?, ?)")
            .bind("m2")
            .bind("standard_meeting")
            .bind("completed")
            .bind("2026-07-27T11:30:00Z")
            .bind("2026-07-27T11:30:00Z")
            .bind(result_json)
            .execute(&pool)
            .await
            .unwrap();

        FtsRepository::refresh_meeting(&pool, "m2").await.unwrap();

        let results = FtsRepository::search(&pool, "event-driven CQRS", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_type, "summary");
    }

    #[tokio::test]
    async fn search_finds_note_text() {
        let pool = setup_fts_db().await;

        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m3")
            .bind("1:1 with manager")
            .bind("2026-07-27T12:00:00Z")
            .bind("2026-07-27T12:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO meeting_notes (meeting_id, notes_markdown) VALUES (?, ?)")
            .bind("m3")
            .bind("Action item: follow up on budget approval next week")
            .execute(&pool)
            .await
            .unwrap();

        FtsRepository::refresh_meeting(&pool, "m3").await.unwrap();

        let results = FtsRepository::search(&pool, "budget approval", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_type, "note");
    }

    #[tokio::test]
    async fn search_with_folder_filter() {
        let pool = setup_fts_db().await;

        // Create folder
        sqlx::query("INSERT INTO meeting_folders (id, name, created_at) VALUES (?, ?, ?)")
            .bind("f1")
            .bind("Sprint 14")
            .bind("2026-07-27T09:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        // Meeting in folder
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_id) VALUES (?, ?, ?, ?, ?)")
            .bind("m4")
            .bind("Sprint 14 Planning")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .bind("f1")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)")
            .bind("t4")
            .bind("m4")
            .bind("Discussing migration strategy for the new microservice")
            .bind("10:15")
            .execute(&pool)
            .await
            .unwrap();

        // Meeting NOT in folder
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m5")
            .bind("Unrelated meeting")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)")
            .bind("t5")
            .bind("m5")
            .bind("Also discussing migration strategy for another project")
            .bind("10:30")
            .execute(&pool)
            .await
            .unwrap();

        FtsRepository::refresh_meeting(&pool, "m4").await.unwrap();
        FtsRepository::refresh_meeting(&pool, "m5").await.unwrap();

        // Search with folder filter
        let results = FtsRepository::search(&pool, r#"folder:"Sprint 14" migration"#, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meeting_id, "m4");
        assert_eq!(results[0].folder_name, "Sprint 14");
    }

    #[tokio::test]
    async fn search_empty_query_returns_empty() {
        let pool = setup_fts_db().await;
        let results = FtsRepository::search(&pool, "", 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn remove_meeting_clears_fts() {
        let pool = setup_fts_db().await;

        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m6")
            .bind("Test meeting")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)")
            .bind("t6")
            .bind("m6")
            .bind("Some important text to search for")
            .bind("10:00")
            .execute(&pool)
            .await
            .unwrap();

        FtsRepository::refresh_meeting(&pool, "m6").await.unwrap();
        assert_eq!(FtsRepository::search(&pool, "important", 10).await.unwrap().len(), 1);

        FtsRepository::remove_meeting(&pool, "m6").await.unwrap();
        assert!(FtsRepository::search(&pool, "important", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rebuild_index_repopulates_from_source() {
        let pool = setup_fts_db().await;

        // Seed two meetings with transcripts
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m7")
            .bind("Meeting Alpha")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)")
            .bind("t7")
            .bind("m7")
            .bind("Discussion about deployment strategy")
            .bind("10:00")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m8")
            .bind("Meeting Beta")
            .bind("2026-07-27T11:00:00Z")
            .bind("2026-07-27T11:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)")
            .bind("t8")
            .bind("m8")
            .bind("Review of deployment strategy for production")
            .bind("11:00")
            .execute(&pool)
            .await
            .unwrap();

        // Clear FTS and rebuild
        sqlx::query("DELETE FROM meeting_fts").execute(&pool).await.unwrap();
        assert!(FtsRepository::search(&pool, "deployment", 10).await.unwrap().is_empty());

        let count = FtsRepository::rebuild_index(&pool).await.unwrap();
        assert_eq!(count, 2);

        let results = FtsRepository::search(&pool, "deployment", 10).await.unwrap();
        assert_eq!(results.len(), 2);
    }
}
