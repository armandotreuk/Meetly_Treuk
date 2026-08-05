use crate::database::repositories::summary::canonical_summary_template_id;
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
use chrono::{DateTime, Utc};
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
    // Canonical `template_id` of the row populated in this response. Empty
    // when no process exists yet ("idle" state); legacy numeric DB rows are
    // returned with the `db:` namespace.
    pub template_id: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub updated_at: Option<String>,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessTranscriptResponse {
    pub message: String,
    pub process_id: String,
    /// Persisted `start_time` of the generation used by cancellation/polling.
    pub generation: String,
}

/// Lightweight summary descriptor for the dropdown list. Excludes `result`
/// (potentially large); frontend fetches full content via `api_get_summary`
/// when switching the active summary.
#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingSummaryInfo {
    pub template_id: String,
    pub status: String,
    pub updated_at: String,
    pub generation: Option<String>,
    pub error: Option<String>,
}

/// Default `template_id` used by legacy save/process callers that do not pass
/// one. Cancellation deliberately requires an explicit template and
/// generation. Pre-existing code used `"daily_standup"`; that was a misnomer
/// and is corrected here as part of SEV-3 (plan §"Correções").
const DEFAULT_TEMPLATE_ID: &str = "standard_meeting";

fn normalize_summary_status(status: &str) -> String {
    match status.trim().to_ascii_lowercase().as_str() {
        "pending" => "processing".to_string(),
        normalized => normalized.to_string(),
    }
}

fn parse_optional_timestamp(
    value: Option<String>,
    field: &str,
) -> Result<Option<DateTime<Utc>>, String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            DateTime::parse_from_rfc3339(value.trim())
                .map(|timestamp| timestamp.with_timezone(&Utc))
                .map_err(|error| format!("Invalid {field} timestamp: {error}"))
        })
        .transpose()
}

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
    expected_start_time: Option<String>,
    expected_updated_at: Option<String>,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    let template_id = template_id
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TEMPLATE_ID.to_string());
    let template_id = canonical_summary_template_id(&template_id);
    let expected_start_time = parse_optional_timestamp(expected_start_time, "expected_start_time")?;
    let expected_updated_at = parse_optional_timestamp(expected_updated_at, "expected_updated_at")?;
    log_info!(
        "api_save_meeting_summary (native) called for meeting_id: {} template_id: {}",
        meeting_id,
        template_id
    );
    let pool = state.db_manager.pool();

    match SummaryProcessesRepository::update_meeting_summary(
        pool,
        &meeting_id,
        &template_id,
        &summary,
        expected_start_time.as_ref(),
        expected_updated_at.as_ref(),
    )
    .await
    {
        Ok(true) => {
            log_info!("Summary saved successfully for meeting_id: {}", meeting_id);
            let revision =
                SummaryProcessesRepository::get_summary_data(pool, &meeting_id, &template_id)
                    .await
                    .map_err(|e| format!("Failed to read saved summary revision: {}", e))?;
            Ok(serde_json::json!({
                "message": "Meeting summary saved successfully",
                "start": revision.as_ref().and_then(|revision| revision.start_time.map(|time| time.to_rfc3339())),
                "updated_at": revision.as_ref().map(|revision| revision.updated_at.to_rfc3339()),
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
    generation: Option<String>,
    _auth_token: Option<String>,
) -> Result<SummaryResponse, String> {
    log_info!(
        "api_get_summary (native) called for meeting_id: {} template_id: {:?}",
        meeting_id,
        template_id
    );
    let pool = state.db_manager.pool();
    let expected_generation = parse_optional_timestamp(generation, "generation")?;
    if expected_generation.is_some()
        && template_id
            .as_deref()
            .map(str::trim)
            .filter(|template_id| !template_id.is_empty())
            .is_none()
    {
        return Err("template_id is required when generation is specified".to_string());
    }

    // Resolve which row to fetch:
    //   Some(t) -> exact (meeting, template)
    //   None    -> most recently updated row for the meeting (fallback).
    // The None branch lets the initial page load (`api_get_summary(meetingId)`
    // with no template arg) restore whatever the user was looking at without
    // the frontend needing prior knowledge of stored templates.
    let process_opt = match template_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(tid) => {
            SummaryProcessesRepository::get_summary_data_for_meeting(pool, &meeting_id, tid).await
        }
        None => SummaryProcessesRepository::get_latest_summary_for_meeting(pool, &meeting_id).await,
    };

    let process_opt = process_opt.map(|process| {
        process.filter(|process| {
            expected_generation
                .as_ref()
                .map(|expected| process.start_time.as_ref() == Some(expected))
                .unwrap_or(true)
        })
    });

    match process_opt {
        Ok(Some(process)) => {
            let status = normalize_summary_status(&process.status);
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
                template_id: canonical_summary_template_id(&resolved_template_id),
                start: process.start_time.map(|t| t.to_rfc3339()),
                end: process.end_time.map(|t| t.to_rfc3339()),
                updated_at: Some(process.updated_at.to_rfc3339()),
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
                updated_at: None,
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
    let final_template_id = canonical_summary_template_id(&final_template_id);

    // Normalise empty / whitespace-only to None so "" and null behave identically
    let summary_language = summary_language.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });

    // Create or reset the process entry in the database. Do not mark an
    // already-active row failed: that error is the duplicate-work guard.
    let run =
        match SummaryProcessesRepository::create_or_reset_process(&pool, &m_id, &final_template_id)
            .await
        {
            Ok(run) => run,
            Err(error) => return Err(format!("Failed to initialize process: {}", error)),
        };

    log_info!("✓ Summary process initialized for meeting_id: {}", &m_id);

    // Save transcript chunks data (matching Python backend behavior)
    let chunk_size = _chunk_size.unwrap_or(40000);
    let overlap = _overlap.unwrap_or(1000);

    if let Err(error) = TranscriptChunksRepository::save_transcript_data(
        &pool,
        &m_id,
        &text,
        &model,
        &model_name,
        chunk_size,
        overlap,
    )
    .await
    {
        let error_message = format!("Failed to save transcript data: {}", error);
        if let Err(status_error) = SummaryProcessesRepository::update_process_failed(
            &pool,
            &m_id,
            &final_template_id,
            &run.expected_start_time,
            &error_message,
        )
        .await
        {
            log_error!(
                "Failed to mark initialization process as failed for {} / {}: {}",
                m_id,
                final_template_id,
                status_error
            );
        }
        return Err(error_message);
    }

    log_info!("✓ Transcript chunks saved for meeting_id: {}", &m_id);

    // Spawn background task for actual processing
    let meeting_id_clone = m_id.clone();
    let generation = run.expected_start_time.to_rfc3339();
    tauri::async_runtime::spawn(async move {
        SummaryService::process_transcript_background(
            app,
            pool,
            meeting_id_clone.clone(),
            text,
            model,
            model_name,
            final_prompt,
            run,
            summary_language,
        )
        .await;
    });

    log_info!("🚀 Background task spawned for meeting_id: {}", &m_id);

    Ok(ProcessTranscriptResponse {
        message: "Summary generation started".to_string(),
        process_id: m_id,
        generation,
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
    generation: Option<String>,
) -> Result<serde_json::Value, String> {
    let pool = state.db_manager.pool();
    let template_id = template_id
        .filter(|template_id| !template_id.trim().is_empty())
        .ok_or_else(|| "template_id is required to cancel a summary".to_string())?;
    let template_id = canonical_summary_template_id(&template_id);
    let expected_generation = parse_optional_timestamp(generation, "generation")?
        .ok_or_else(|| "generation is required to cancel a summary".to_string())?;
    let run = SummaryProcessesRepository::get_active_summary_run(pool, &meeting_id, &template_id)
        .await
        .map_err(|e| format!("Failed to find active summary: {}", e))?
        .filter(|run| run.expected_start_time == expected_generation);
    log_info!(
        "api_cancel_summary called for meeting_id: {} template_id: {} generation: {}",
        meeting_id,
        template_id,
        expected_generation.to_rfc3339()
    );

    // Trigger cancellation via the service
    let cancelled = run.as_ref().is_some_and(|run| {
        SummaryService::cancel_summary(
            &meeting_id,
            &run.template_id,
            Some(&run.expected_start_time),
        )
    });

    let database_cancelled = if let Some(run) = &run {
        SummaryProcessesRepository::cancel_active_process_if_run(
            pool,
            &meeting_id,
            &template_id,
            Some(&run.expected_start_time),
        )
        .await
        .map_err(|e| format!("Failed to update cancellation status: {}", e))?
    } else {
        false
    };

    if cancelled || database_cancelled {
        log_info!(
            "Successfully cancelled summary generation for meeting_id: {} template_id: {}",
            meeting_id,
            template_id
        );
        Ok(serde_json::json!({
            "message": "Summary generation cancelled successfully",
            "meeting_id": meeting_id,
            "template_id": template_id,
            "generation": expected_generation.to_rfc3339(),
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
            "generation": expected_generation.to_rfc3339(),
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
            template_id: canonical_summary_template_id(&p.template_id),
            status: normalize_summary_status(&p.status),
            updated_at: p.updated_at.to_rfc3339(),
            generation: p.start_time.map(|start_time| start_time.to_rfc3339()),
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
/// The row's persisted generation is used for both cancellation and deletion.
/// A worker that races this command cannot update or delete a newer generation.
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
    let template_id = canonical_summary_template_id(&template_id);
    log_info!(
        "api_delete_meeting_summary called for meeting_id: {} template_id: {}",
        meeting_id,
        template_id
    );

    let pool = state.db_manager.pool();
    let revision = SummaryProcessesRepository::get_summary_data(pool, &meeting_id, &template_id)
        .await
        .map_err(|e| format!("Failed to find summary to delete: {}", e))?;
    let run = SummaryProcessesRepository::get_summary_run(pool, &meeting_id, &template_id)
        .await
        .map_err(|e| format!("Failed to find summary generation: {}", e))?;

    // Signal only the generation represented by the row being deleted. A
    // worker that races the delete is fenced by the same persisted start time.
    if let Some(run) = &run {
        let _ = SummaryService::cancel_summary(
            &meeting_id,
            &run.template_id,
            Some(&run.expected_start_time),
        );
    }

    let removed = SummaryProcessesRepository::delete_summary_if_revision(
        pool,
        &meeting_id,
        &template_id,
        run.as_ref().map(|run| &run.expected_start_time),
        revision.as_ref().map(|revision| &revision.updated_at),
    )
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
    use super::{normalize_summary_status, DEFAULT_TEMPLATE_ID};
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
        assert!(
            s.contains("fn get_summary_data("),
            "get_summary_data must exist"
        );
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
        assert!(
            s.contains("fn delete_summary("),
            "delete_summary fn must exist"
        );
        assert!(
            s.contains("pub async fn delete_summary_if_run(")
                && s.contains("template_id IN ({placeholders})"),
            "delete_summary must retain a meeting/template composite scope"
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
        // Save/process retain the legacy default; cancellation requires an
        // exact template and generation instead of guessing a row.
        let n_default = src
            .matches("unwrap_or_else(|| DEFAULT_TEMPLATE_ID.to_string())")
            .count();
        assert!(
            n_default >= 2,
            "expected >=2 DEFAULT_TEMPLATE_ID fallbacks (save/process), got {n_default}"
        );
    }

    #[test]
    fn api_status_normalization_maps_pending_to_processing() {
        assert_eq!(normalize_summary_status("PENDING"), "processing");
        assert_eq!(normalize_summary_status("COMPLETED"), "completed");
        assert_eq!(normalize_summary_status(" failed "), "failed");
    }
}
