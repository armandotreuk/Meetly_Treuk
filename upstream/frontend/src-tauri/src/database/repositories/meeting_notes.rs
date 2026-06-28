use sqlx::SqlitePool;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use anyhow::Result;

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
