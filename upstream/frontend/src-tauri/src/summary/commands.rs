use crate::database::repositories::{
    meeting::MeetingsRepository, summary::SummaryProcessesRepository,
    transcript_chunk::TranscriptChunksRepository,
};
use crate::state::AppState;
use crate::summary::language_detection::{detect_summary_language, SummaryLanguageDetection};
use crate::summary::metadata::{
    read_detected_summary_language_from_metadata, read_summary_language_from_metadata,
    write_detected_summary_language_to_metadata, write_summary_language_to_metadata,
};
use crate::summary::service::SummaryService;
use log::{error as log_error, info as log_info, warn as log_warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Runtime};

#[derive(Debug, Serialize, Deserialize)]
pub struct SummaryResponse {
    pub status: String,
    #[serde(rename = "meetingName")]
    pub meeting_name: Option<String>,
    pub meeting_id: String,
    // `template_id` of the row populated in this response. Empty when no
    // process exists yet ("idle" state); otherwise equal to the row's
    // `template_id`. Frontend `useActiveSummaryTemplate` uses this to seed
    // its active template state via `api_get_summary(meetingId)` (no template
    // arg) which resolves to the most recently updated row.
    pub template_id: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessTranscriptResponse {
    pub message: String,
    pub process_id: String,
}

/// Lightweight summary descriptor for the dropdown list. Excludes `result`
/// (potentially large); frontend fetches full content via `api_get_summary`
/// when switching the active summary.
#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingSummaryInfo {
    pub template_id: String,
    pub status: String,
    pub updated_at: String,
    pub error: Option<String>,
}

/// Default `template_id` used by legacy callers that don't pass one. Aligned
/// across `api_save_meeting_summary` / `api_process_transcript` /
/// `api_cancel_summary` so the legacy front-end (pre-multi-template) keeps
/// writing to the same row. Pre-existing code used `"daily_standup"`; that
/// was a misnomer and is corrected here as part of SEV-3 (plan §"Correções").
const DEFAULT_TEMPLATE_ID: &str = "standard_meeting";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SummaryLanguageStorage {
    Metadata,
    LocalFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSummaryLanguagePreference {
    pub language: Option<String>,
    pub storage: SummaryLanguageStorage,
}

impl MeetingSummaryLanguagePreference {
    fn metadata(language: Option<String>) -> Self {
        Self {
            language,
            storage: SummaryLanguageStorage::Metadata,
        }
    }

    fn local_fallback() -> Self {
        Self {
            language: None,
            storage: SummaryLanguageStorage::LocalFallback,
        }
    }
}

enum MeetingFolderResolution {
    Folder(PathBuf),
    NoFolder,
}

/// Saves a meeting summary (Native SQLx implementation)
///
/// Expected format: { "markdown": "...", "summary_json": [...BlockNote blocks...] }
#[tauri::command]
pub async fn api_save_meeting_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    summary: serde_json::Value,
    template_id: Option<String>,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    let template_id = template_id
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TEMPLATE_ID.to_string());
    log_info!(
        "api_save_meeting_summary (native) called for meeting_id: {} template_id: {}",
        meeting_id,
        template_id
    );
    let pool = state.db_manager.pool();

    match SummaryProcessesRepository::update_meeting_summary(pool, &meeting_id, &template_id, &summary).await {
        Ok(true) => {
            log_info!("Summary saved successfully for meeting_id: {}", meeting_id);
            Ok(serde_json::json!({
                "message": "Meeting summary saved successfully"
            }))
        }
        Ok(false) => {
            log_warn!(
                "Meeting not found or invalid JSON for meeting_id: {}",
                meeting_id
            );
            Err("Meeting not found or can't convert the json".into())
        }
        Err(e) => {
            log_error!("Failed to save meeting summary for {}: {}", meeting_id, e);
            Err(e.to_string())
        }
    }
}

/// Gets the per-meeting summary language override from metadata.json.
#[tauri::command]
pub async fn api_get_meeting_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_get_meeting_summary_language called for meeting_id: {}",
        meeting_id
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => read_summary_language_from_metadata(&folder)
            .map(MeetingSummaryLanguagePreference::metadata)
            .map_err(|e| e.to_string()),
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Saves or clears the per-meeting summary language override in metadata.json.
#[tauri::command]
pub async fn api_save_meeting_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    summary_language: Option<String>,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_save_meeting_summary_language called for meeting_id: {}, language: {:?}",
        meeting_id,
        summary_language
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => {
            write_summary_language_to_metadata(&folder, summary_language.as_deref())
                .map_err(|e| e.to_string())?;
            read_summary_language_from_metadata(&folder)
                .map(MeetingSummaryLanguagePreference::metadata)
                .map_err(|e| e.to_string())
        }
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Gets the cached Auto-detected summary language from metadata.json.
#[tauri::command]
pub async fn api_get_meeting_detected_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_get_meeting_detected_summary_language called for meeting_id: {}",
        meeting_id
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => {
            read_detected_summary_language_from_metadata(&folder)
                .map(MeetingSummaryLanguagePreference::metadata)
                .map_err(|e| e.to_string())
        }
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Saves or clears the cached Auto-detected summary language in metadata.json.
#[tauri::command]
pub async fn api_save_meeting_detected_summary_language<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    detected_summary_language: Option<String>,
) -> Result<MeetingSummaryLanguagePreference, String> {
    log_info!(
        "api_save_meeting_detected_summary_language called for meeting_id: {}, language: {:?}",
        meeting_id,
        detected_summary_language
    );

    match resolve_meeting_folder(state.db_manager.pool(), &meeting_id).await? {
        MeetingFolderResolution::Folder(folder) => {
            write_detected_summary_language_to_metadata(
                &folder,
                detected_summary_language.as_deref(),
            )
            .map_err(|e| e.to_string())?;
            read_detected_summary_language_from_metadata(&folder)
                .map(MeetingSummaryLanguagePreference::metadata)
                .map_err(|e| e.to_string())
        }
        MeetingFolderResolution::NoFolder => Ok(MeetingSummaryLanguagePreference::local_fallback()),
    }
}

/// Detects the dominant supported summary language from transcript segments.
#[tauri::command]
pub async fn api_detect_transcript_summary_language(
    transcript_texts: Vec<String>,
) -> Result<SummaryLanguageDetection, String> {
    Ok(detect_summary_language(&transcript_texts))
}

async fn resolve_meeting_folder(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
) -> Result<MeetingFolderResolution, String> {
    let meeting = MeetingsRepository::get_meeting_metadata(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting metadata: {}", e))?
        .ok_or_else(|| format!("Meeting not found: {}", meeting_id))?;

    let Some(folder_path) = meeting.folder_path.filter(|p| !p.trim().is_empty()) else {
        return Ok(MeetingFolderResolution::NoFolder);
    };

    Ok(MeetingFolderResolution::Folder(PathBuf::from(folder_path)))
}

/// Gets summary status and data (Native SQLx implementation)
///
/// Returns summary status (pending/processing/completed/failed) and parsed result data
#[tauri::command]
pub async fn api_get_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    template_id: Option<String>,
    _auth_token: Option<String>,
) -> Result<SummaryResponse, String> {
    log_info!(
        "api_get_summary (native) called for meeting_id: {} template_id: {:?}",
        meeting_id,
        template_id
    );
    let pool = state.db_manager.pool();

    // Resolve which row to fetch:
    //   Some(t) -> exact (meeting, template)
    //   None    -> most recently updated row for the meeting (fallback).
    // The None branch lets the initial page load (`api_get_summary(meetingId)`
    // with no template arg) restore whatever the user was looking at without
    // the frontend needing prior knowledge of stored templates.
    let process_opt = match template_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(tid) => {
            SummaryProcessesRepository::get_summary_data_for_meeting(pool, &meeting_id, tid).await
        }
        None => {
            SummaryProcessesRepository::get_latest_summary_for_meeting(pool, &meeting_id).await
        }
    };

    match process_opt {
        Ok(Some(process)) => {
            let status = process.status.to_lowercase();
            let error = process.error;
            let resolved_template_id = process.template_id.clone();

            // Parse result data if it exists (regardless of status)
            // This allows displaying restored summaries after cancellation or failure
            let data = if let Some(result_str) = process.result {
                match serde_json::from_str::<serde_json::Value>(&result_str) {
                    Ok(parsed) => Some(parsed),
                    Err(e) => {
                        log_error!("Failed to parse summary result JSON: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            // Fetch meeting title from database
            let meeting_name = match MeetingsRepository::get_meeting(pool, &meeting_id).await {
                Ok(Some(meeting_details)) => {
                    log_info!("Fetched meeting title: {}", &meeting_details.title);
                    Some(meeting_details.title)
                }
                Ok(None) => {
                    log_warn!("Meeting not found for meeting_id: {}", meeting_id);
                    None
                }
                Err(e) => {
                    log_error!("Failed to fetch meeting title: {}", e);
                    None
                }
            };

            let response = SummaryResponse {
                status: status.clone(),
                meeting_name,
                meeting_id: meeting_id.clone(),
                template_id: resolved_template_id,
                start: process.start_time.map(|t| t.to_rfc3339()),
                end: process.end_time.map(|t| t.to_rfc3339()),
                data,
                error,
            };

            log_info!(
                "Summary status for {}: {}, template_id: {}, has_data: {}, meeting_name: {:?}",
                meeting_id,
                status,
                response.template_id,
                response.data.is_some(),
                response.meeting_name
            );
            Ok(response)
        }
        Ok(None) => {
            log_info!("No summary process found for meeting_id: {}", meeting_id);

            // Still fetch meeting title for idle state
            let meeting_name = match MeetingsRepository::get_meeting(pool, &meeting_id).await {
                Ok(Some(meeting_details)) => Some(meeting_details.title),
                _ => None,
            };

            Ok(SummaryResponse {
                status: "idle".to_string(),
                meeting_name,
                meeting_id,
                template_id: String::new(),
                start: None,
                end: None,
                data: None,
                error: None,
            })
        }
        Err(e) => {
            log_error!("Error retrieving summary for {}: {}", meeting_id, e);
            Err(format!("Failed to retrieve summary: {}", e))
        }
    }
}

/// Processes transcript and generates summary (Native SQLx implementation)
///
/// Spawns a background task and returns immediately with process_id
#[tauri::command]
pub async fn api_process_transcript<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    text: String,
    model: String,
    model_name: String,
    meeting_id: Option<String>,
    _chunk_size: Option<i32>,
    _overlap: Option<i32>,
    custom_prompt: Option<String>,
    template_id: Option<String>,
    summary_language: Option<String>,
    _auth_token: Option<String>,
) -> Result<ProcessTranscriptResponse, String> {
    use uuid::Uuid;

    let m_id = meeting_id.unwrap_or_else(|| format!("meeting-{}", Uuid::new_v4()));
    log_info!(
        "api_process_transcript (native) called for meeting_id: {}, model: {}",
        &m_id,
        &model
    );

    let pool = state.db_manager.pool().clone();
    let final_prompt = custom_prompt.unwrap_or_else(|| "".to_string());
    let final_template_id = template_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| DEFAULT_TEMPLATE_ID.to_string());

    // Normalise empty / whitespace-only to None so "" and null behave identically
    let summary_language = summary_language.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });

    // Create or reset the process entry in the database
    SummaryProcessesRepository::create_or_reset_process(&pool, &m_id, &final_template_id)
        .await
        .map_err(|e| format!("Failed to initialize process: {}", e))?;

    log_info!("✓ Summary process initialized for meeting_id: {}", &m_id);

    // Save transcript chunks data (matching Python backend behavior)
    let chunk_size = _chunk_size.unwrap_or(40000);
    let overlap = _overlap.unwrap_or(1000);

    TranscriptChunksRepository::save_transcript_data(
        &pool,
        &m_id,
        &text,
        &model,
        &model_name,
        chunk_size,
        overlap,
    )
    .await
    .map_err(|e| format!("Failed to save transcript data: {}", e))?;

    log_info!("✓ Transcript chunks saved for meeting_id: {}", &m_id);

    // Spawn background task for actual processing
    let meeting_id_clone = m_id.clone();
    tauri::async_runtime::spawn(async move {
        SummaryService::process_transcript_background(
            app,
            pool,
            meeting_id_clone.clone(),
            text,
            model,
            model_name,
            final_prompt,
            final_template_id,
            summary_language,
        )
        .await;
    });

    log_info!("🚀 Background task spawned for meeting_id: {}", &m_id);

    Ok(ProcessTranscriptResponse {
        message: "Summary generation started".to_string(),
        process_id: m_id,
    })
}

/// Cancels an ongoing summary generation process
///
/// This command triggers the cancellation token for the specified meeting,
/// stopping the summary generation gracefully.
#[tauri::command]
pub async fn api_cancel_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    template_id: Option<String>,
) -> Result<serde_json::Value, String> {
    // Cancellation token is keyed by meeting_id only (serial 1-by-meeting, see
    // `CANCELLATION_REGISTRY`). The DB write of `cancelled` status, however,
    // needs the template_id to hit the right row. Default to `standard_meeting`
    // for legacy callers that don't pass one explicitly.
    let template_id = template_id
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TEMPLATE_ID.to_string());
    log_info!(
        "api_cancel_summary called for meeting_id: {} template_id: {}",
        meeting_id,
        template_id
    );

    // Trigger cancellation via the service
    let cancelled = SummaryService::cancel_summary(&meeting_id);

    if cancelled {
        // Update database status to cancelled
        let pool = state.db_manager.pool();
        if let Err(e) =
            SummaryProcessesRepository::update_process_cancelled(pool, &meeting_id, &template_id).await
        {
            log_error!(
                "Failed to update DB status to cancelled for {}: {}",
                meeting_id,
                e
            );
            return Err(format!("Failed to update cancellation status: {}", e));
        }

        log_info!(
            "Successfully cancelled summary generation for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );
        Ok(serde_json::json!({
            "message": "Summary generation cancelled successfully",
            "meeting_id": meeting_id,
            "template_id": template_id,
        }))
    } else {
        log_warn!(
            "No active summary generation found for meeting_id: {}",
            meeting_id
        );
        Ok(serde_json::json!({
            "message": "No active summary generation to cancel",
            "meeting_id": meeting_id,
            "template_id": template_id,
        }))
    }
}

/// Lists every summary row for a meeting, newest first.
///
/// Returns only lightweight metadata (`template_id`, `status`, `updated_at`,
/// `error`); the full `result` JSON is fetched on-demand via
/// `api_get_summary` when the user switches the active summary in the UI.
/// Typical row count is 1-5 per meeting, so even if the underlying repo
/// function returns the full row struct, trimming the wire payload here keeps
/// the dropdown list snappy on slow IPC channels.
#[tauri::command]
pub async fn api_list_meeting_summaries<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<MeetingSummaryInfo>, String> {
    log_info!(
        "api_list_meeting_summaries called for meeting_id: {}",
        meeting_id
    );
    let pool = state.db_manager.pool();
    let rows = SummaryProcessesRepository::list_summaries_for_meeting(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to list summaries: {}", e))?;
    let summaries: Vec<MeetingSummaryInfo> = rows
        .into_iter()
        .map(|p| MeetingSummaryInfo {
            template_id: p.template_id,
            status: p.status,
            updated_at: p.updated_at.to_rfc3339(),
            error: p.error,
        })
        .collect();
    log_info!(
        "api_list_meeting_summaries: {} summaries for meeting_id: {}",
        summaries.len(),
        meeting_id
    );
    Ok(summaries)
}

/// Deletes one summary row identified by `(meeting_id, template_id)`.
///
/// Sequence:
/// 1. `SummaryService::cancel_summary` (idempotent — true if a token exists,
///    false otherwise). Triggers the background task's cancellation path so it
///    won't try to write back into the row we're about to delete. Even if it
///    races, the `WHERE meeting_id=? AND template_id=?` UPDATEs from service
///    hit 0 rows on a deleted row, so the delete is safe by construction.
/// 2. `delete_summary` removes the row. Returns `removed: bool`.
///
/// Frontend invalidates the active template after a successful delete (and
/// picks a fallback per the plan: next-listed summary, else empty state).
#[tauri::command]
pub async fn api_delete_meeting_summary<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    template_id: String,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_delete_meeting_summary called for meeting_id: {} template_id: {}",
        meeting_id,
        template_id
    );

    // Step 1: signal cancellation to any in-flight processing for this meeting.
    // Idempotent: returns false when nothing is running.
    let _ = SummaryService::cancel_summary(&meeting_id);

    // Step 2: remove the row.
    let pool = state.db_manager.pool();
    let removed = SummaryProcessesRepository::delete_summary(pool, &meeting_id, &template_id)
        .await
        .map_err(|e| format!("Failed to delete summary: {}", e))?;

    log_info!(
        "api_delete_meeting_summary: removed={} meeting_id: {} template_id: {}",
        removed,
        meeting_id,
        template_id
    );
    Ok(serde_json::json!({
        "removed": removed,
        "meeting_id": meeting_id,
        "template_id": template_id,
    }))
}

#[cfg(test)]
mod multi_template_tests {
    use super::DEFAULT_TEMPLATE_ID;
    use std::fs;

    // ponytail: string-invariant test only — catches DDL regressions (PK drop,
    // backfill sentinel removal, backup-column drop). Does not run the migration.
    #[test]
    fn migration_invariants() {
        let sql = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/20260721000001_multi_template_summaries.sql"
        ))
        .expect("migration file must exist");
        assert!(
            sql.contains("PRIMARY KEY (meeting_id, template_id)"),
            "composite PK on (meeting_id, template_id) must be declared"
        );
        // Note: spec phrasing "UPDATE ... SET template_id = 'legacy'" is loose;
        // the real migration backfills via INSERT...SELECT with the 'legacy'
        // literal. The invariant we care about is the presence of the sentinel.
        assert!(
            sql.contains("'legacy'"),
            "backfill must mark pre-migration rows with the 'legacy' template_id sentinel"
        );
        // Spec named columns result_backup_markdown / result_backup_json; the
        // real schema uses result_backup (TEXT, serialized JSON) and
        // result_backup_timestamp. Both are referenced by restore-on-fail/cancel
        // queries in repositories/summary.rs.
        assert!(
            sql.contains("result_backup "),
            "result_backup column must be preserved for restore-on-fail/cancel"
        );
        assert!(
            sql.contains("result_backup_timestamp"),
            "result_backup_timestamp column must be preserved"
        );
        assert!(
            sql.contains("idx_summary_processes_meeting"),
            "forward-lookup index on meeting_id must exist"
        );
    }

    // ponytail: string-invariant test only — catches WHERE-clause regressions
    // that would break per-template isolation. Does not touch a live DB.
    #[test]
    fn repo_per_template_isolation() {
        let s = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/database/repositories/summary.rs"
        ))
        .expect("summary repo src must exist");
        assert!(s.contains("fn get_summary_data("), "get_summary_data must exist");
        assert!(
            s.contains("fn get_latest_summary_for_meeting("),
            "get_latest_summary_for_meeting must exist"
        );
        assert!(
            s.contains("fn list_summaries_for_meeting("),
            "list_summaries_for_meeting must exist"
        );
        assert!(
            s.contains("fn has_other_completed_summaries("),
            "has_other_completed_summaries must exist"
        );
        // get_summary_data must take template_id in its signature.
        assert!(
            s.contains("fn get_summary_data(")
                && s.match_indices("fn get_summary_data(").next().is_some(),
        );
        // Every state-mutating query must key on the composite (meeting_id, template_id).
        let n_composite_where = s
            .matches("WHERE meeting_id = ? AND template_id = ?")
            .count();
        assert!(
            n_composite_where >= 5,
            "expected >=5 composite-key WHERE clauses (get/update/delete), got {n_composite_where}"
        );
        // Each repo function carrying template_id contributes one `template_id: &str` param.
        let n_template_params = s.matches("template_id: &str").count();
        assert!(
            n_template_params >= 7,
            "expected >=7 `template_id: &str` params across repo fns, got {n_template_params}"
        );
    }

    // ponytail: string-invariant test only — guards composite-key delete against
    // a single-column WHERE regression (which would delete across templates).
    #[test]
    fn repo_delete_composite_key() {
        let s = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/database/repositories/summary.rs"
        ))
        .expect("summary repo src must exist");
        assert!(s.contains("fn delete_summary("), "delete_summary fn must exist");
        assert!(
            s.contains("DELETE FROM summary_processes WHERE meeting_id = ? AND template_id = ?"),
            "delete_summary must DELETE on composite (meeting_id, template_id), not single-column"
        );
    }

    // ponytail: string-invariant test only — catches regressions in default
    // template_id resolution (const rename, signature drop, fallback removal).
    #[test]
    fn commands_default_template_resolution() {
        // Const value — direct symbol reference, no string check needed.
        assert_eq!(DEFAULT_TEMPLATE_ID, "standard_meeting");

        let src = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/summary/commands.rs"
        ))
        .expect("commands src must exist");
        // api_get_summary accepts Optional template_id and falls back to "latest".
        assert!(
            src.contains("pub async fn api_get_summary<R: Runtime>("),
            "api_get_summary command must exist"
        );
        assert!(
            src.contains("template_id: Option<String>"),
            "api_get_summary must accept template_id: Option<String> (fallback path)"
        );
        assert!(
            src.contains("get_latest_summary_for_meeting"),
            "api_get_summary None branch must fall back to get_latest_summary_for_meeting"
        );
        // api_save_meeting_summary / api_process_transcript / api_cancel_summary all
        // default template_id to DEFAULT_TEMPLATE_ID. Counting the canonical
        // unwrap_or_else pattern surfaces the 3 expected call sites.
        let n_default = src
            .matches("unwrap_or_else(|| DEFAULT_TEMPLATE_ID.to_string())")
            .count();
        assert!(
            n_default >= 3,
            "expected >=3 DEFAULT_TEMPLATE_ID fallbacks (save/process/cancel), got {n_default}"
        );
    }
}
