use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MeetingNote {
    pub meeting_id: String,
    pub notes_markdown: Option<String>,
    pub notes_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct MeetingNotesRepository;

impl MeetingNotesRepository {
    pub async fn get_notes(pool: &SqlitePool, meeting_id: &str) -> Result<Option<MeetingNote>> {
        let note = sqlx::query_as::<_, MeetingNote>(
            "SELECT * FROM meeting_notes WHERE meeting_id = $1 LIMIT 1",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?;
        Ok(note)
    }

    pub async fn save_notes(
        pool: &SqlitePool,
        meeting_id: &str,
        notes_markdown: Option<&str>,
        notes_json: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO meeting_notes (meeting_id, notes_markdown, notes_json, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $4)
            ON CONFLICT(meeting_id) DO UPDATE SET
                notes_markdown = excluded.notes_markdown,
                notes_json = excluded.notes_json,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(meeting_id)
        .bind(notes_markdown)
        .bind(notes_json)
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn delete_notes(pool: &SqlitePool, meeting_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM meeting_notes WHERE meeting_id = $1")
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    #[tokio::test]
    async fn saves_and_reads_markdown_and_json() {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE meetings (id TEXT PRIMARY KEY NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE meeting_notes (meeting_id TEXT PRIMARY KEY NOT NULL, notes_markdown TEXT, notes_json TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meetings (id) VALUES (?)")
            .bind("meeting-1")
            .execute(&pool)
            .await
            .unwrap();
        let blocks = r#"[{"id":"block-1","type":"paragraph"}]"#;

        MeetingNotesRepository::save_notes(&pool, "meeting-1", Some("hello"), Some(blocks))
            .await
            .unwrap();
        let note = MeetingNotesRepository::get_notes(&pool, "meeting-1")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(note.notes_markdown.as_deref(), Some("hello"));
        assert_eq!(note.notes_json.as_deref(), Some(blocks));
    }

    #[tokio::test]
    async fn updates_markdown_and_json_on_conflict() {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE meetings (id TEXT PRIMARY KEY NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE meeting_notes (meeting_id TEXT PRIMARY KEY NOT NULL, notes_markdown TEXT, notes_json TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meetings (id) VALUES (?)")
            .bind("meeting-1")
            .execute(&pool)
            .await
            .unwrap();

        MeetingNotesRepository::save_notes(&pool, "meeting-1", Some("first"), Some("[]"))
            .await
            .unwrap();
        MeetingNotesRepository::save_notes(
            &pool,
            "meeting-1",
            Some("second"),
            Some(r#"[{"id":"block-2"}]"#),
        )
        .await
        .unwrap();
        let note = MeetingNotesRepository::get_notes(&pool, "meeting-1")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(note.notes_markdown.as_deref(), Some("second"));
        assert_eq!(note.notes_json.as_deref(), Some(r#"[{"id":"block-2"}]"#));
    }

    #[tokio::test]
    async fn awaited_save_then_delete_leaves_no_notes() {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE meetings (id TEXT PRIMARY KEY NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE meeting_notes (meeting_id TEXT PRIMARY KEY NOT NULL, notes_markdown TEXT, notes_json TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meetings (id) VALUES (?)")
            .bind("meeting-1")
            .execute(&pool)
            .await
            .unwrap();

        let save_pool = pool.clone();
        let save = tokio::spawn(async move {
            MeetingNotesRepository::save_notes(
                &save_pool,
                "meeting-1",
                Some("markdown-A"),
                Some("json-A"),
            )
            .await
        });
        save.await.unwrap().unwrap();
        let delete_pool = pool.clone();
        let delete = tokio::spawn(async move {
            MeetingNotesRepository::delete_notes(&delete_pool, "meeting-1").await
        });
        delete.await.unwrap().unwrap();

        // ponytail: A deliberately concurrent race test would be nondeterministic; this covers the awaited frontend contract.
        assert!(MeetingNotesRepository::get_notes(&pool, "meeting-1")
            .await
            .unwrap()
            .is_none());
    }
}
