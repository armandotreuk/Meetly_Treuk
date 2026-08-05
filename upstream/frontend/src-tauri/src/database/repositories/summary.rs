use crate::database::models::SummaryProcess;
use crate::database::repositories::fts::FtsRepository;
use crate::summary::templates::{database_template_id, parse_database_template_id};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::SqlitePool;
use std::cmp::Ordering;
use tracing::{error, info as log_info};

pub struct SummaryProcessesRepository;

/// Identity of one summary generation. `start_time` is already persisted on
/// every newly-created/reset row, so it can fence workers without another
/// destructive table rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryRun {
    pub template_id: String,
    pub expected_start_time: DateTime<Utc>,
}

struct SummaryStorageCandidate {
    template_id: String,
    updated_at: String,
    start_time: Option<String>,
}

fn compare_storage_time(left: &str, right: &str) -> Ordering {
    match (
        DateTime::parse_from_rfc3339(left),
        DateTime::parse_from_rfc3339(right),
    ) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

/// Convert the two persisted database-template spellings into the one API key.
/// Numeric rows predate the `db:` namespace and must remain addressable.
pub fn canonical_summary_template_id(template_id: &str) -> String {
    let template_id = template_id.trim();
    if let Some(id) = parse_database_template_id(template_id) {
        return database_template_id(id);
    }

    template_id
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .map(database_template_id)
        .unwrap_or_else(|| template_id.to_string())
}

fn summary_template_storage_candidates(template_id: &str) -> Vec<String> {
    let template_id = template_id.trim();
    let canonical_id = canonical_summary_template_id(template_id);

    if let Some(id) = parse_database_template_id(&canonical_id) {
        let mut candidates = vec![canonical_id, id.to_string()];
        if template_id != candidates[1]
            && !candidates.iter().any(|candidate| candidate == template_id)
        {
            candidates.push(template_id.to_string());
        }
        return candidates;
    }

    vec![template_id.to_string()]
}

/// Resolve a requested summary identity to the actual key stored in SQLite.
///
/// `db:N` and legacy numeric `N` deliberately share this lookup. If both
/// spellings exist, the newest generation/version wins using the same rule as
/// latest-summary and list queries; ties prefer the canonical spelling.
pub async fn resolve_summary_storage_template_id(
    pool: &SqlitePool,
    meeting_id: &str,
    template_id: &str,
) -> Result<String, sqlx::Error> {
    let candidates = summary_template_storage_candidates(template_id);

    let mut existing = Vec::new();
    for candidate in &candidates {
        if let Some((template_id, updated_at, start_time)) = sqlx::query_as::<
            _,
            (String, String, Option<String>),
        >(
            "SELECT template_id, updated_at, start_time FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind(meeting_id)
        .bind(candidate)
        .fetch_optional(pool)
        .await?
        {
            existing.push(SummaryStorageCandidate {
                template_id,
                updated_at,
                start_time,
            });
        }
    }

    Ok(existing
        .into_iter()
        .max_by(|left, right| {
            compare_storage_time(
                left.start_time.as_deref().unwrap_or(&left.updated_at),
                right.start_time.as_deref().unwrap_or(&right.updated_at),
            )
            .then_with(|| left.updated_at.cmp(&right.updated_at))
            .then_with(|| {
                let left_is_canonical =
                    canonical_summary_template_id(&left.template_id) == left.template_id.trim();
                let right_is_canonical =
                    canonical_summary_template_id(&right.template_id) == right.template_id.trim();
                left_is_canonical.cmp(&right_is_canonical)
            })
        })
        .map(|candidate| candidate.template_id)
        .or_else(|| candidates.into_iter().next())
        .unwrap_or_else(|| template_id.trim().to_string()))
}

fn canonicalize_process(mut process: SummaryProcess) -> SummaryProcess {
    process.template_id = canonical_summary_template_id(&process.template_id);
    process
}

fn summary_generation_time(process: &SummaryProcess) -> DateTime<Utc> {
    process.start_time.unwrap_or(process.updated_at)
}

fn compare_summary_newness(left: &SummaryProcess, right: &SummaryProcess) -> Ordering {
    summary_generation_time(left)
        .cmp(&summary_generation_time(right))
        .then_with(|| left.updated_at.cmp(&right.updated_at))
        .then_with(|| {
            let left_is_canonical =
                canonical_summary_template_id(&left.template_id) == left.template_id.trim();
            let right_is_canonical =
                canonical_summary_template_id(&right.template_id) == right.template_id.trim();
            left_is_canonical.cmp(&right_is_canonical)
        })
}

fn collapse_summary_aliases(rows: Vec<SummaryProcess>) -> Vec<SummaryProcess> {
    // ponytail: a meeting normally has only a handful of rows; the explicit
    // scan makes the legacy numeric/db alias rule easier to audit than a
    // second SQL query with subtly different ordering semantics.
    let mut grouped = Vec::new();
    for process in rows {
        let canonical_id = canonical_summary_template_id(&process.template_id);
        if let Some(existing) = grouped.iter_mut().find(|existing: &&mut SummaryProcess| {
            canonical_summary_template_id(&existing.template_id) == canonical_id
        }) {
            if compare_summary_newness(&process, existing) == Ordering::Greater {
                *existing = process;
            }
        } else {
            grouped.push(process);
        }
    }
    grouped
}

fn sort_summaries_newest_first(rows: &mut [SummaryProcess]) {
    rows.sort_by(|left, right| compare_summary_newness(right, left));
}

impl SummaryProcessesRepository {
    /// Retrieves the summary process for a given (meeting_id, template_id).
    pub async fn get_summary_data(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        let storage_template_id =
            resolve_summary_storage_template_id(pool, meeting_id, template_id).await?;
        let process = sqlx::query_as::<_, SummaryProcess>(
            "SELECT * FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind(meeting_id)
        .bind(storage_template_id)
        .fetch_optional(pool)
        .await?;

        Ok(process.map(canonicalize_process))
    }

    /// Retrieves the most recently updated summary process for a meeting,
    /// regardless of template. Used when the caller does not specify a template
    /// (e.g. initial page load, restore-on-open).
    pub async fn get_latest_summary_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        let rows = sqlx::query_as::<_, SummaryProcess>(
            "SELECT * FROM summary_processes WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        Ok(collapse_summary_aliases(rows)
            .into_iter()
            .max_by(compare_summary_newness)
            .map(canonicalize_process))
    }

    /// Lightweight list of (template_id, status, updated_at, error) for a meeting.
    /// Does not return `result` (which can be large); frontend fetches full content
    /// via `get_summary_data` when switching the active summary.
    pub async fn list_summaries_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<SummaryProcess>, sqlx::Error> {
        let rows = sqlx::query_as::<_, SummaryProcess>(
            "SELECT * FROM summary_processes WHERE meeting_id = ? ORDER BY updated_at DESC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        let mut grouped = collapse_summary_aliases(rows);
        sort_summaries_newest_first(&mut grouped);
        Ok(grouped.into_iter().map(canonicalize_process).collect())
    }

    pub async fn update_meeting_summary(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
        summary: &Value,
        expected_start_time: Option<&DateTime<Utc>>,
        expected_updated_at: Option<&DateTime<Utc>>,
    ) -> Result<bool, sqlx::Error> {
        let storage_template_id =
            resolve_summary_storage_template_id(pool, meeting_id, template_id).await?;
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

        let result_json = result_json.unwrap();
        let summary_update = if let Some(expected_start_time) = expected_start_time {
            sqlx::query(
                "UPDATE summary_processes SET result = ?, updated_at = ? WHERE meeting_id = ? AND template_id = ? AND start_time = ?",
            )
            .bind(&result_json)
            .bind(now)
            .bind(meeting_id)
            .bind(&storage_template_id)
            .bind(expected_start_time)
            .execute(&mut *transaction)
            .await?
        } else if let Some(expected_updated_at) = expected_updated_at {
            // Legacy rows may not have a start_time. Their last-write time is
            // the available revision fence and is sufficient to reject a
            // newer generation, which always receives a non-null start_time.
            sqlx::query(
                "UPDATE summary_processes SET result = ?, updated_at = ? WHERE meeting_id = ? AND template_id = ? AND start_time IS NULL AND updated_at = ?",
            )
            .bind(&result_json)
            .bind(now)
            .bind(meeting_id)
            .bind(&storage_template_id)
            .bind(expected_updated_at)
            .execute(&mut *transaction)
            .await?
        } else {
            transaction.rollback().await?;
            return Err(sqlx::Error::Protocol(
                "A summary revision is required for manual saves".to_string(),
            ));
        };

        if summary_update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }

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
        let storage_template_id =
            resolve_summary_storage_template_id(pool, meeting_id, template_id).await?;
        let process = sqlx::query_as::<_, SummaryProcess>(
            "SELECT p.* FROM summary_processes p JOIN transcript_chunks t ON p.meeting_id = t.meeting_id WHERE p.meeting_id = ? AND p.template_id = ?",
        )
        .bind(meeting_id)
        .bind(storage_template_id)
        .fetch_optional(pool)
        .await?;

        Ok(process.map(canonicalize_process))
    }

    /// Returns the persisted generation identity for a summary row, if its
    /// start time is available. Older completed rows may legitimately have a
    /// null start time and are handled by the null-safe delete/cancel paths.
    pub async fn get_summary_run(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<Option<SummaryRun>, sqlx::Error> {
        let storage_template_id =
            resolve_summary_storage_template_id(pool, meeting_id, template_id).await?;
        let row = sqlx::query_as::<_, (Option<DateTime<Utc>>,)>(
            "SELECT start_time FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind(meeting_id)
        .bind(storage_template_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.and_then(|(expected_start_time,)| {
            expected_start_time.map(|expected_start_time| SummaryRun {
                template_id: canonical_summary_template_id(template_id),
                expected_start_time,
            })
        }))
    }

    pub async fn get_active_summary_run(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<Option<SummaryRun>, sqlx::Error> {
        let mut runs = Vec::new();
        for storage_template_id in summary_template_storage_candidates(template_id) {
            if let Some((expected_start_time,)) = sqlx::query_as::<_, (Option<DateTime<Utc>>,)>(
                "SELECT start_time FROM summary_processes WHERE meeting_id = ? AND template_id = ? AND LOWER(status) IN ('pending', 'processing', 'summarizing', 'regenerating')",
            )
            .bind(meeting_id)
            .bind(storage_template_id)
            .fetch_optional(pool)
            .await?
            {
                if let Some(expected_start_time) = expected_start_time {
                    runs.push(SummaryRun {
                        template_id: canonical_summary_template_id(template_id),
                        expected_start_time,
                    });
                }
            }
        }

        Ok(runs
            .into_iter()
            .max_by(|left, right| left.expected_start_time.cmp(&right.expected_start_time)))
    }

    pub async fn create_or_reset_process(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<SummaryRun, sqlx::Error> {
        let storage_template_id =
            resolve_summary_storage_template_id(pool, meeting_id, template_id).await?;
        log_info!(
            "Creating or resetting summary process for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );
        let now = Utc::now();
        let result = sqlx::query(
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
                WHERE LOWER(summary_processes.status) NOT IN
                    ('pending', 'processing', 'summarizing', 'regenerating')
                    OR (
                        LOWER(summary_processes.status) IN
                            ('pending', 'processing', 'summarizing', 'regenerating')
                        AND julianday(summary_processes.updated_at) < julianday('now', '-15 minutes')
                    )
            "#,
        )
        .bind(meeting_id)
        .bind(storage_template_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(sqlx::Error::Protocol(
                "Summary generation is already in progress".to_string(),
            ));
        }

        log_info!(
            "Backed up existing summary before regeneration for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );
        Ok(SummaryRun {
            template_id: canonical_summary_template_id(template_id),
            expected_start_time: now,
        })
    }

    pub async fn update_process_completed(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
        expected_start_time: &DateTime<Utc>,
        result: Value,
        chunk_count: i64,
        processing_time: f64,
    ) -> Result<bool, sqlx::Error> {
        let storage_template_id =
            resolve_summary_storage_template_id(pool, meeting_id, template_id).await?;
        let now = Utc::now();
        let result_str = serde_json::to_string(&result)
            .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize result: {}", e)))?;

        let update_result = sqlx::query(
            r#"
            UPDATE summary_processes
            SET status = 'completed', result = ?, updated_at = ?, end_time = ?, chunk_count = ?, processing_time = ?, error = NULL, result_backup = NULL, result_backup_timestamp = NULL
            WHERE meeting_id = ? AND template_id = ? AND start_time = ?
              AND LOWER(status) IN ('pending', 'processing', 'summarizing', 'regenerating')
            "#,
        )
        .bind(result_str)
        .bind(now)
        .bind(now)
        .bind(chunk_count)
        .bind(processing_time)
        .bind(meeting_id)
        .bind(storage_template_id)
        .bind(expected_start_time)
        .execute(pool)
        .await?;
        let updated = update_result.rows_affected() > 0;
        log_info!(
            "Summary completed and backup cleared for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );

        // Update FTS index — best-effort; summary is already saved.
        if updated {
            if let Err(e) = FtsRepository::refresh_meeting(pool, meeting_id).await {
                error!("Failed to refresh FTS for meeting {}: {}", meeting_id, e);
            }
        }

        if !updated {
            log_info!(
                "Skipped stale summary completion for meeting_id: {} template_id: {}",
                meeting_id,
                template_id
            );
        }

        Ok(updated)
    }

    pub async fn update_process_failed(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
        expected_start_time: &DateTime<Utc>,
        error: &str,
    ) -> Result<bool, sqlx::Error> {
        let storage_template_id =
            resolve_summary_storage_template_id(pool, meeting_id, template_id).await?;
        let now = Utc::now();

        // Restore from backup if it exists, but only for this generation.
        let result = sqlx::query(
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
            WHERE meeting_id = ? AND template_id = ? AND start_time = ?
              AND LOWER(status) IN ('pending', 'processing', 'summarizing', 'regenerating')
            "#,
        )
        .bind(error)
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(storage_template_id)
        .bind(expected_start_time)
        .execute(pool)
        .await?;
        let updated = result.rows_affected() > 0;
        log_info!(
            "Summary generation failed for meeting_id: {} template_id: {} updated={}",
            meeting_id,
            template_id,
            updated
        );
        Ok(updated)
    }

    pub async fn update_process_cancelled(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
        expected_start_time: &DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let storage_template_id =
            resolve_summary_storage_template_id(pool, meeting_id, template_id).await?;
        let now = Utc::now();

        // Restore from backup if it exists, but never overwrite a newer run.
        let result = sqlx::query(
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
            WHERE meeting_id = ? AND template_id = ? AND start_time = ?
              AND LOWER(status) IN ('pending', 'processing', 'summarizing', 'regenerating')
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(storage_template_id)
        .bind(expected_start_time)
        .execute(pool)
        .await?;
        let updated = result.rows_affected() > 0;
        log_info!(
            "Marked summary process as cancelled for meeting_id: {} template_id: {} updated={}",
            meeting_id,
            template_id,
            updated
        );
        Ok(updated)
    }

    /// Compatibility wrapper for callers that cancel the current active row.
    /// New callers should use `cancel_active_process_if_run` when they have a
    /// generation identity.
    pub async fn cancel_active_process(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<bool, sqlx::Error> {
        Self::cancel_active_process_if_run(pool, meeting_id, template_id, None).await
    }

    pub async fn cancel_active_process_if_run(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
        expected_start_time: Option<&DateTime<Utc>>,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        let result = match expected_start_time {
            Some(expected_start_time) => {
                let candidates = summary_template_storage_candidates(template_id);
                let placeholders = std::iter::repeat("?")
                    .take(candidates.len())
                    .collect::<Vec<_>>()
                    .join(", ");
                let query = format!(
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
                WHERE meeting_id = ? AND template_id IN ({placeholders}) AND start_time = ?
                  AND LOWER(status) IN ('pending', 'processing', 'summarizing', 'regenerating')
                "#
                );
                let mut query = sqlx::query(&query).bind(now).bind(now).bind(meeting_id);
                for candidate in &candidates {
                    query = query.bind(candidate);
                }
                query.bind(expected_start_time).execute(pool).await?
            }
            None => {
                let storage_template_id =
                    resolve_summary_storage_template_id(pool, meeting_id, template_id).await?;
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
                WHERE meeting_id = ? AND template_id = ? AND start_time IS NULL
                  AND LOWER(status) IN ('pending', 'processing', 'summarizing', 'regenerating')
                "#,
                )
                .bind(now)
                .bind(now)
                .bind(meeting_id)
                .bind(storage_template_id)
                .execute(pool)
                .await?
            }
        };

        Ok(result.rows_affected() > 0)
    }

    /// Deletes a logical summary row and, for a DB template, only aliases that
    /// represent the same persisted generation. The generation guard is part
    /// of the DELETE, so a newer alias cannot be removed after the caller read
    /// the old row.
    pub async fn delete_summary_if_run(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
        expected_start_time: Option<&DateTime<Utc>>,
    ) -> Result<bool, sqlx::Error> {
        Self::delete_summary_if_revision(pool, meeting_id, template_id, expected_start_time, None)
            .await
    }

    pub async fn delete_summary_if_revision(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
        expected_start_time: Option<&DateTime<Utc>>,
        expected_updated_at: Option<&DateTime<Utc>>,
    ) -> Result<bool, sqlx::Error> {
        let candidates = if expected_start_time.is_none() && expected_updated_at.is_none() {
            // Without a persisted revision, delete only the row selected by
            // the resolver. Never remove an unverified alias alongside it.
            vec![resolve_summary_storage_template_id(pool, meeting_id, template_id).await?]
        } else {
            summary_template_storage_candidates(template_id)
        };
        let placeholders = std::iter::repeat("?")
            .take(candidates.len())
            .collect::<Vec<_>>()
            .join(", ");
        let generation_guard = if expected_start_time.is_some() {
            "AND start_time = ?"
        } else if expected_updated_at.is_some() {
            "AND start_time IS NULL AND updated_at = ?"
        } else {
            "AND start_time IS NULL"
        };
        let query = format!(
            "DELETE FROM summary_processes WHERE meeting_id = ? AND template_id IN ({placeholders}) {generation_guard}"
        );
        let mut query = sqlx::query(&query).bind(meeting_id);
        for candidate in &candidates {
            query = query.bind(candidate);
        }
        if let Some(expected_start_time) = expected_start_time {
            query = query.bind(expected_start_time);
        } else if let Some(expected_updated_at) = expected_updated_at {
            query = query.bind(expected_updated_at);
        }
        let result = query.execute(pool).await?;
        let removed = result.rows_affected() > 0;
        if removed {
            log_info!(
                "Deleted summary aliases for meeting_id: {} template_id: {}",
                meeting_id,
                template_id
            );
        }
        Ok(removed)
    }

    /// Deletes an old/null-generation row. Kept for repository callers that
    /// do not have a run identity; generation-aware callers use the method
    /// above.
    pub async fn delete_summary(
        pool: &SqlitePool,
        meeting_id: &str,
        template_id: &str,
    ) -> Result<bool, sqlx::Error> {
        Self::delete_summary_if_run(pool, meeting_id, template_id, None).await
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
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT template_id FROM summary_processes WHERE meeting_id = ? AND LOWER(status) = 'completed'",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
        let except_template_id = canonical_summary_template_id(except_template_id);
        Ok(rows.into_iter().any(|(template_id,)| {
            canonical_summary_template_id(&template_id) != except_template_id
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_summary_template_id, resolve_summary_storage_template_id,
        SummaryProcessesRepository,
    };
    use chrono::{DateTime, Duration, Utc};
    use serde_json::json;
    use sqlx::SqlitePool;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect(":memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query(
            "CREATE TABLE meetings (id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .expect("create meetings test schema");
        sqlx::query(
            "CREATE TABLE transcript_chunks (meeting_id TEXT PRIMARY KEY, meeting_name TEXT, transcript_text TEXT NOT NULL, model TEXT NOT NULL, model_name TEXT NOT NULL, chunk_size INTEGER, overlap INTEGER, created_at TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .expect("create transcript test schema");
        sqlx::query(
            "CREATE TABLE summary_processes (meeting_id TEXT NOT NULL, template_id TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, error TEXT, result TEXT, start_time TEXT, end_time TEXT, chunk_count INTEGER DEFAULT 0, processing_time REAL DEFAULT 0.0, metadata TEXT, result_backup TEXT, result_backup_timestamp TEXT, PRIMARY KEY (meeting_id, template_id))",
        )
        .execute(&pool)
        .await
        .expect("create summary test schema");

        let now = Utc::now();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("meeting-1")
            .bind("Meeting")
            .bind(now)
            .bind(now)
            .execute(&pool)
            .await
            .expect("insert test meeting");
        sqlx::query(
            "INSERT INTO transcript_chunks (meeting_id, transcript_text, model, model_name, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("meeting-1")
        .bind("Transcript")
        .bind("ollama")
        .bind("model")
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert test transcript chunk");

        pool
    }

    async fn insert_summary(
        pool: &SqlitePool,
        template_id: &str,
        status: &str,
        updated_at: chrono::DateTime<Utc>,
    ) {
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind("meeting-1")
        .bind(template_id)
        .bind(status)
        .bind(updated_at)
        .bind(updated_at)
        .bind(r#"{"markdown":"old"}"#)
        .execute(pool)
        .await
        .expect("insert summary row");
    }

    #[test]
    fn canonical_summary_ids_keep_database_and_file_names_separate() {
        assert_eq!(canonical_summary_template_id("42"), "db:42");
        assert_eq!(canonical_summary_template_id("db:42"), "db:42");
        assert_eq!(canonical_summary_template_id("file:42"), "file:42");
        assert_eq!(
            canonical_summary_template_id("standard_meeting"),
            "standard_meeting"
        );
    }

    #[tokio::test]
    async fn canonical_db_get_list_and_polling_bridge_numeric_legacy_row() {
        let pool = test_pool().await;
        insert_summary(&pool, "42", "completed", Utc::now()).await;

        let exact = SummaryProcessesRepository::get_summary_data(&pool, "meeting-1", "db:42")
            .await
            .expect("get summary")
            .expect("legacy row");
        assert_eq!(exact.template_id, "db:42");

        let joined =
            SummaryProcessesRepository::get_summary_data_for_meeting(&pool, "meeting-1", "db:42")
                .await
                .expect("get joined summary")
                .expect("legacy joined row");
        assert_eq!(joined.template_id, "db:42");

        let listed = SummaryProcessesRepository::list_summaries_for_meeting(&pool, "meeting-1")
            .await
            .expect("list summaries");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].template_id, "db:42");
    }

    #[tokio::test]
    async fn list_deduplicates_canonical_and_numeric_alias_rows() {
        let pool = test_pool().await;
        insert_summary(&pool, "42", "completed", Utc::now()).await;
        insert_summary(&pool, "db:42", "failed", Utc::now()).await;

        let listed = SummaryProcessesRepository::list_summaries_for_meeting(&pool, "meeting-1")
            .await
            .expect("list aliased summaries");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].template_id, "db:42");
        assert_eq!(listed[0].status, "failed");
    }

    #[tokio::test]
    async fn latest_lookup_and_listing_choose_the_same_newest_alias_generation() {
        let pool = test_pool().await;
        let canonical_time = Utc::now() - Duration::minutes(2);
        let numeric_time = Utc::now();
        insert_summary(&pool, "db:42", "completed", canonical_time).await;
        insert_summary(&pool, "42", "processing", numeric_time).await;

        let latest = SummaryProcessesRepository::get_latest_summary_for_meeting(&pool, "meeting-1")
            .await
            .expect("get latest summary")
            .expect("latest alias");
        let exact = SummaryProcessesRepository::get_summary_data(&pool, "meeting-1", "db:42")
            .await
            .expect("get exact logical summary")
            .expect("exact alias");
        let listed = SummaryProcessesRepository::list_summaries_for_meeting(&pool, "meeting-1")
            .await
            .expect("list summaries");

        assert_eq!(latest.template_id, listed[0].template_id);
        assert_eq!(latest.status, listed[0].status);
        assert_eq!(latest.start_time, listed[0].start_time);
        assert_eq!(latest.updated_at, listed[0].updated_at);
        assert_eq!(exact.status, listed[0].status);
        assert_eq!(exact.updated_at, listed[0].updated_at);
        assert_eq!(latest.status, "processing");
    }

    #[tokio::test]
    async fn canonical_generation_and_save_reuse_numeric_legacy_row() {
        let pool = test_pool().await;
        insert_summary(&pool, "42", "completed", Utc::now()).await;

        assert_eq!(
            resolve_summary_storage_template_id(&pool, "meeting-1", "db:42")
                .await
                .expect("resolve storage key"),
            "42"
        );
        let run = SummaryProcessesRepository::create_or_reset_process(&pool, "meeting-1", "db:42")
            .await
            .expect("reset legacy row");

        let saved = SummaryProcessesRepository::update_meeting_summary(
            &pool,
            "meeting-1",
            "db:42",
            &json!({"markdown": "new"}),
            Some(&run.expected_start_time),
            None,
        )
        .await
        .expect("save summary");
        assert!(saved);

        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT template_id, result FROM summary_processes WHERE meeting_id = ?",
        )
        .bind("meeting-1")
        .fetch_all(&pool)
        .await
        .expect("read saved row");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "42");
        assert!(rows[0].1.contains("new"));
    }

    #[tokio::test]
    async fn canonical_cancel_and_delete_reuse_numeric_legacy_row() {
        let pool = test_pool().await;
        insert_summary(&pool, "42", "PENDING", Utc::now()).await;

        assert!(
            SummaryProcessesRepository::cancel_active_process(&pool, "meeting-1", "db:42")
                .await
                .expect("cancel active row")
        );
        assert!(
            SummaryProcessesRepository::delete_summary(&pool, "meeting-1", "db:42")
                .await
                .expect("delete legacy row")
        );
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM summary_processes WHERE meeting_id = ?")
                .bind("meeting-1")
                .fetch_one(&pool)
                .await
                .expect("count rows");
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn file_and_database_numeric_summary_rows_do_not_alias() {
        let pool = test_pool().await;
        insert_summary(&pool, "file:42", "completed", Utc::now()).await;

        assert!(
            SummaryProcessesRepository::get_summary_data(&pool, "meeting-1", "db:42")
                .await
                .expect("get DB row")
                .is_none()
        );
        assert!(
            SummaryProcessesRepository::get_summary_data(&pool, "meeting-1", "file:42")
                .await
                .expect("get file row")
                .is_some()
        );

        SummaryProcessesRepository::create_or_reset_process(&pool, "meeting-1", "db:42")
            .await
            .expect("create DB row beside file row");
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM summary_processes WHERE meeting_id = ?")
                .bind("meeting-1")
                .fetch_one(&pool)
                .await
                .expect("count distinct source rows");
        assert_eq!(count.0, 2);
    }

    #[tokio::test]
    async fn stale_pending_process_can_be_retried_without_duplicate_active_work() {
        let pool = test_pool().await;
        insert_summary(&pool, "42", "PENDING", Utc::now() - Duration::minutes(16)).await;

        SummaryProcessesRepository::create_or_reset_process(&pool, "meeting-1", "db:42")
            .await
            .expect("stale process should be replaceable");

        let row: (String, String) = sqlx::query_as(
            "SELECT template_id, status FROM summary_processes WHERE meeting_id = ?",
        )
        .bind("meeting-1")
        .fetch_one(&pool)
        .await
        .expect("read retried process");
        assert_eq!(row.0, "42");
        assert_eq!(row.1, "PENDING");
    }

    #[tokio::test]
    async fn create_or_reset_does_not_reset_an_active_process() {
        let pool = SqlitePool::connect(":memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE summary_processes (
                meeting_id TEXT NOT NULL,
                template_id TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                start_time TEXT,
                result TEXT,
                error TEXT,
                result_backup TEXT,
                result_backup_timestamp TEXT,
                PRIMARY KEY (meeting_id, template_id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create summary schema");

        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind("meeting-1")
        .bind("standard_meeting")
        .bind("PENDING")
        .bind("now")
        .bind("now")
        .bind("old result")
        .execute(&pool)
        .await
        .expect("insert active process");

        let error = SummaryProcessesRepository::create_or_reset_process(
            &pool,
            "meeting-1",
            "standard_meeting",
        )
        .await
        .expect_err("active process must not be reset");
        assert!(error.to_string().contains("already in progress"));

        let row: (String, Option<String>) = sqlx::query_as(
            "SELECT status, result FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("standard_meeting")
        .fetch_one(&pool)
        .await
        .expect("read active process");
        assert_eq!(row.0, "PENDING");
        assert_eq!(row.1.as_deref(), Some("old result"));
    }

    #[tokio::test]
    async fn stale_worker_cannot_complete_a_newer_generation() {
        let pool = test_pool().await;
        insert_summary(&pool, "standard_meeting", "completed", Utc::now()).await;

        let first_run = SummaryProcessesRepository::create_or_reset_process(
            &pool,
            "meeting-1",
            "standard_meeting",
        )
        .await
        .expect("create first generation");
        sqlx::query(
            "UPDATE summary_processes SET status = 'completed' WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("standard_meeting")
        .execute(&pool)
        .await
        .expect("finish first generation");

        let second_run = SummaryProcessesRepository::create_or_reset_process(
            &pool,
            "meeting-1",
            "standard_meeting",
        )
        .await
        .expect("create second generation");

        assert!(!SummaryProcessesRepository::update_process_completed(
            &pool,
            "meeting-1",
            "standard_meeting",
            &first_run.expected_start_time,
            json!({"markdown": "stale"}),
            1,
            1.0,
        )
        .await
        .expect("stale completion should be ignored"));

        let current: (String, Option<String>) = sqlx::query_as(
            "SELECT status, result FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("standard_meeting")
        .fetch_one(&pool)
        .await
        .expect("read current generation");
        assert_eq!(current.0, "PENDING");
        assert!(!current.1.as_deref().unwrap_or_default().contains("stale"));

        assert!(SummaryProcessesRepository::update_process_completed(
            &pool,
            "meeting-1",
            "standard_meeting",
            &second_run.expected_start_time,
            json!({"markdown": "current"}),
            1,
            1.0,
        )
        .await
        .expect("current completion should apply"));
    }

    #[tokio::test]
    async fn generation_fenced_cancel_cannot_cancel_a_newer_run() {
        let pool = test_pool().await;
        insert_summary(&pool, "standard_meeting", "completed", Utc::now()).await;
        let first_run = SummaryProcessesRepository::create_or_reset_process(
            &pool,
            "meeting-1",
            "standard_meeting",
        )
        .await
        .expect("create first generation");
        sqlx::query(
            "UPDATE summary_processes SET status = 'completed' WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("standard_meeting")
        .execute(&pool)
        .await
        .expect("complete first generation");
        let second_run = SummaryProcessesRepository::create_or_reset_process(
            &pool,
            "meeting-1",
            "standard_meeting",
        )
        .await
        .expect("create second generation");

        assert!(!SummaryProcessesRepository::cancel_active_process_if_run(
            &pool,
            "meeting-1",
            "standard_meeting",
            Some(&first_run.expected_start_time),
        )
        .await
        .expect("stale cancellation should be ignored"));
        assert!(SummaryProcessesRepository::cancel_active_process_if_run(
            &pool,
            "meeting-1",
            "standard_meeting",
            Some(&second_run.expected_start_time),
        )
        .await
        .expect("current cancellation should apply"));
    }

    #[tokio::test]
    async fn active_cancel_searches_aliases_before_preferring_newer_completed_row() {
        let pool = test_pool().await;
        let active_generation = Utc::now() - Duration::minutes(1);
        let completed_generation = Utc::now();
        for (template_id, status, generation) in [
            ("42", "PENDING", active_generation),
            ("db:42", "completed", completed_generation),
        ] {
            sqlx::query(
                "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, start_time, result) VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind("meeting-1")
            .bind(template_id)
            .bind(status)
            .bind(generation)
            .bind(generation)
            .bind(generation)
            .bind(r#"{"markdown":"alias"}"#)
            .execute(&pool)
            .await
            .expect("insert active alias");
        }

        let run = SummaryProcessesRepository::get_active_summary_run(&pool, "meeting-1", "db:42")
            .await
            .expect("find active alias")
            .expect("active alias run");
        assert_eq!(run.expected_start_time, active_generation);
        assert!(SummaryProcessesRepository::cancel_active_process_if_run(
            &pool,
            "meeting-1",
            "db:42",
            Some(&run.expected_start_time),
        )
        .await
        .expect("cancel active alias"));

        let status: (String,) = sqlx::query_as(
            "SELECT status FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("42")
        .fetch_one(&pool)
        .await
        .expect("read cancelled alias");
        assert_eq!(status.0, "cancelled");
    }

    #[tokio::test]
    async fn manual_save_accepts_current_generation_and_rejects_stale_generation() {
        let pool = test_pool().await;
        insert_summary(&pool, "standard_meeting", "completed", Utc::now()).await;

        let first_run = SummaryProcessesRepository::create_or_reset_process(
            &pool,
            "meeting-1",
            "standard_meeting",
        )
        .await
        .expect("create first generation");
        sqlx::query(
            "UPDATE summary_processes SET status = 'completed' WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("standard_meeting")
        .execute(&pool)
        .await
        .expect("complete first generation");

        assert!(SummaryProcessesRepository::update_meeting_summary(
            &pool,
            "meeting-1",
            "standard_meeting",
            &json!({"markdown": "current manual edit"}),
            Some(&first_run.expected_start_time),
            None,
        )
        .await
        .expect("current manual save should succeed"));

        let second_run = SummaryProcessesRepository::create_or_reset_process(
            &pool,
            "meeting-1",
            "standard_meeting",
        )
        .await
        .expect("create second generation");

        assert!(!SummaryProcessesRepository::update_meeting_summary(
            &pool,
            "meeting-1",
            "standard_meeting",
            &json!({"markdown": "stale manual edit"}),
            Some(&first_run.expected_start_time),
            None,
        )
        .await
        .expect("stale manual save should be fenced"));

        let current: (String, Option<String>) = sqlx::query_as(
            "SELECT status, result FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("standard_meeting")
        .fetch_one(&pool)
        .await
        .expect("read current generation");
        assert_eq!(current.0, "PENDING");
        assert!(!current
            .1
            .as_deref()
            .unwrap_or_default()
            .contains("stale manual edit"));
        assert!(second_run.expected_start_time > first_run.expected_start_time);
    }

    #[tokio::test]
    async fn legacy_manual_save_uses_updated_at_revision_when_start_time_is_null() {
        let pool = test_pool().await;
        let initial = Utc::now();
        insert_summary(&pool, "legacy", "completed", initial).await;
        let expected: (DateTime<Utc>,) = sqlx::query_as(
            "SELECT updated_at FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("legacy")
        .fetch_one(&pool)
        .await
        .expect("read legacy revision");

        assert!(SummaryProcessesRepository::update_meeting_summary(
            &pool,
            "meeting-1",
            "legacy",
            &json!({"markdown": "first edit"}),
            None,
            Some(&expected.0),
        )
        .await
        .expect("legacy save should succeed"));

        let newer: (DateTime<Utc>,) = sqlx::query_as(
            "SELECT updated_at FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("legacy")
        .fetch_one(&pool)
        .await
        .expect("read updated legacy revision");
        assert!(!SummaryProcessesRepository::update_meeting_summary(
            &pool,
            "meeting-1",
            "legacy",
            &json!({"markdown": "stale edit"}),
            None,
            Some(&expected.0),
        )
        .await
        .expect("stale legacy save should be fenced"));
        assert!(newer.0 > expected.0);
    }

    #[tokio::test]
    async fn stale_delete_cannot_remove_a_newer_generation() {
        let pool = test_pool().await;
        insert_summary(&pool, "standard_meeting", "completed", Utc::now()).await;

        let first_run = SummaryProcessesRepository::create_or_reset_process(
            &pool,
            "meeting-1",
            "standard_meeting",
        )
        .await
        .expect("create first generation");
        sqlx::query(
            "UPDATE summary_processes SET status = 'completed' WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("standard_meeting")
        .execute(&pool)
        .await
        .expect("finish first generation");
        SummaryProcessesRepository::create_or_reset_process(&pool, "meeting-1", "standard_meeting")
            .await
            .expect("create newer generation");

        assert!(!SummaryProcessesRepository::delete_summary_if_run(
            &pool,
            "meeting-1",
            "standard_meeting",
            Some(&first_run.expected_start_time),
        )
        .await
        .expect("stale delete should be ignored"));

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM summary_processes WHERE meeting_id = ? AND template_id = ?",
        )
        .bind("meeting-1")
        .bind("standard_meeting")
        .fetch_one(&pool)
        .await
        .expect("count current generation");
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn deleting_database_alias_removes_only_database_aliases() {
        let pool = test_pool().await;
        let generation = Utc::now();
        for template_id in ["42", "db:42", "file:42"] {
            sqlx::query(
                "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES (?, ?, 'completed', ?, ?, ?)",
            )
            .bind("meeting-1")
            .bind(template_id)
            .bind(generation)
            .bind(generation)
            .bind(r#"{"markdown":"same generation"}"#)
            .execute(&pool)
            .await
            .expect("insert summary alias");
        }

        assert!(SummaryProcessesRepository::delete_summary_if_revision(
            &pool,
            "meeting-1",
            "db:42",
            None,
            Some(&generation),
        )
        .await
        .expect("delete database aliases"));

        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT template_id FROM summary_processes WHERE meeting_id = ? ORDER BY template_id",
        )
        .bind("meeting-1")
        .fetch_all(&pool)
        .await
        .expect("read remaining summaries");
        assert_eq!(rows, vec![("file:42".to_string(),)]);
    }

    #[tokio::test]
    async fn deleting_database_alias_does_not_remove_a_newer_alias_generation() {
        let pool = test_pool().await;
        let old_generation = Utc::now() - Duration::minutes(2);
        let new_generation = Utc::now();
        for (template_id, generation) in [("db:42", old_generation), ("42", new_generation)] {
            sqlx::query(
                "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, start_time, result) VALUES (?, ?, 'completed', ?, ?, ?, ?)",
            )
            .bind("meeting-1")
            .bind(template_id)
            .bind(generation)
            .bind(generation)
            .bind(generation)
            .bind(format!("{{\"markdown\":\"{template_id}\"}}"))
            .execute(&pool)
            .await
            .expect("insert aliased generation");
        }

        assert!(SummaryProcessesRepository::delete_summary_if_run(
            &pool,
            "meeting-1",
            "db:42",
            Some(&old_generation),
        )
        .await
        .expect("delete old generation"));

        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT template_id FROM summary_processes WHERE meeting_id = ? ORDER BY template_id",
        )
        .bind("meeting-1")
        .fetch_all(&pool)
        .await
        .expect("read remaining alias");
        assert_eq!(rows, vec![("42".to_string(),)]);
    }
}
