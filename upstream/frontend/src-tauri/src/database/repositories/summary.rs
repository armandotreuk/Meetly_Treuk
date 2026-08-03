use crate::database::models::SummaryProcess;
use crate::database::repositories::fts::FtsRepository;
use chrono::Utc;
use serde_json::Value;
use sqlx::SqlitePool;
use tracing::{error, info as log_info};

pub struct SummaryProcessesRepository;

impl SummaryProcessesRepository {
    /// Retrieves the summary process for a given (meeting_id, template_id).
    pub async fn get_summary_data(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>(
            "SELECT * FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind(meeting_id)
        .bind(template_id)
        .fetch_optional(pool)
        .await
    }

    /// Retrieves the most recently updated summary process for a meeting,
    /// regardless of template. Used when the caller does not specify a template
    /// (e.g. initial page load, restore-on-open).
    pub async fn get_latest_summary_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>(
            "SELECT * FROM summary_processes WHERE meeting_id = ? ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
    }

    /// Lightweight list of (template_id, status, updated_at, error) for a meeting.
    /// Does not return `result` (which can be large); frontend fetches full content
    /// via `get_summary_data` when switching the active summary.
    pub async fn list_summaries_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>(
            "SELECT * FROM summary_processes WHERE meeting_id = ? ORDER BY updated_at DESC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await
    }

    pub async fn update_meeting_summary(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
        summary: &Value,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = pool.begin().await?;

        let meeting_exists: bool = sqlx::query("SELECT 1 FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(&mut *transaction)
            .await?
            .is_some();

        if !meeting_exists {
            log_info!(
                "Attempted to save summary for a non-existent meeting_id: {}",
                meeting_id
            );
            transaction.rollback().await?;
            return Ok(false);
        }

        let result_json = serde_json::to_string(summary);
        if result_json.is_err() {
            error!("Can't convert the json to string for saving to Database");
            transaction.rollback().await?;
            return Ok(false);
        }
        let now = Utc::now();

        sqlx::query(
            "UPDATE summary_processes SET result = ?, updated_at = ? WHERE meeting_id = ? AND template_id = ?",
        )
        .bind(&result_json.unwrap())
        .bind(now)
        .bind(meeting_id)
        .bind(template_id)
        .execute(&mut *transaction)
        .await?;

        sqlx::query("UPDATE meetings SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;

        log_info!(
            "Successfully updated summary and timestamp for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );
        Ok(true)
    }

    /// Fetches summary joined with transcript_chunks to ensure a transcript exists.
    /// template_id filter keeps per-template isolation.
    pub async fn get_summary_data_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>(
            "SELECT p.* FROM summary_processes p JOIN transcript_chunks t ON p.meeting_id = t.meeting_id WHERE p.meeting_id = ? AND p.template_id = ?",
        )
        .bind(meeting_id)
        .bind(template_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn create_or_reset_process(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<(), sqlx::Error> {
        log_info!(
            "Creating or resetting summary process for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, start_time, result, error)
            VALUES (?, ?, 'PENDING', ?, ?, ?, NULL, NULL)
            ON CONFLICT(meeting_id, template_id) DO UPDATE SET
                status = 'PENDING',
                updated_at = excluded.updated_at,
                start_time = excluded.start_time,
                result_backup = result,
                result_backup_timestamp = excluded.updated_at,
                result = result,
                error = NULL
            "#,
        )
        .bind(meeting_id)
        .bind(template_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
        log_info!(
            "Backed up existing summary before regeneration for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );
        Ok(())
    }

    pub async fn update_process_completed(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
        result: Value,
        chunk_count: i64,
        processing_time: f64,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        let result_str = serde_json::to_string(&result)
            .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize result: {}", e)))?;

        sqlx::query(
            r#"
            UPDATE summary_processes
            SET status = 'completed', result = ?, updated_at = ?, end_time = ?, chunk_count = ?, processing_time = ?, error = NULL, result_backup = NULL, result_backup_timestamp = NULL
            WHERE meeting_id = ? AND template_id = ?
            "#,
        )
        .bind(result_str)
        .bind(now)
        .bind(now)
        .bind(chunk_count)
        .bind(processing_time)
        .bind(meeting_id)
        .bind(template_id)
        .execute(pool)
        .await?;
        log_info!(
            "Summary completed and backup cleared for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );

        // Update FTS index — best-effort; summary is already saved.
        if let Err(e) = FtsRepository::refresh_meeting(pool, meeting_id).await {
            error!("Failed to refresh FTS for meeting {}: {}", meeting_id, e);
        }

        Ok(())
    }

    pub async fn update_process_failed(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
        error: &str,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now();

        // Restore from backup if it exists, otherwise keep current result
        sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'failed',
                error = ?,
                updated_at = ?,
                end_time = ?,
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ? AND template_id = ?
            "#,
        )
        .bind(error)
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(template_id)
        .execute(pool)
        .await?;
        log_info!(
            "Summary generation failed and backup restored for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );
        Ok(())
    }

    pub async fn update_process_cancelled(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now();

        // Restore from backup if it exists, otherwise keep current result
        sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'cancelled',
                updated_at = ?,
                end_time = ?,
                error = 'Generation was cancelled by user',
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ? AND template_id = ?
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(template_id)
        .execute(pool)
        .await?;
        log_info!(
            "Marked summary process as cancelled and restored backup for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );
        Ok(())
    }

    /// Deletes one summary row. Returns true if a row was removed.
    /// The caller is responsible for cancelling any in-flight PENDING process
    /// before calling this (see `api_delete_meeting_summary`).
    pub async fn delete_summary(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query(
            "DELETE FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind(meeting_id)
        .bind(template_id)
        .execute(pool)
        .await?;
        let removed = res.rows_affected() > 0;
        if removed {
            log_info!(
                "Deleted summary for meeting_id: {} template_id: {}",
                meeting_id,
                template_id
            );
        }
        Ok(removed)
    }

    /// Returns true if at least one `completed` summary exists for `meeting_id`
    /// with a `template_id` different from `except_template_id`. Used by
    /// `SummaryService::process_transcript_background` to decide whether to
    /// rename the meeting: the first completed summary of a meeting sets the
    /// title; later completions (from other templates) leave it alone. Errors
    /// are propagated (the caller falls back to `false` so a transient DB
    /// error doesn't suppress a legitimate first-time rename — the safe
    /// direction).
    pub async fn has_other_completed_summaries(
        pool: &SqlitePool,
        meeting_id: &str,
        except_template_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM summary_processes WHERE meeting_id = ? AND status = 'completed' AND template_id != ?",
        )
        .bind(meeting_id)
        .bind(except_template_id)
        .fetch_one(pool)
        .await?;
        Ok(row.0 > 0)
    }
}