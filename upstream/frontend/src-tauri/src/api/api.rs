use log::{debug as log_debug, error as log_error, info as log_info, warn as log_warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_store::StoreExt;
use tokio_util::sync::CancellationToken;

use crate::{
    api::chat::{ChatRequestState, ChatRequestSurface, ChatRequestToken},
    database::{
        models::MeetingModel,
        repositories::{
            fts::{FtsRepository, FtsSearchResult},
            meeting::MeetingsRepository,
            retrieval::RetrievalRepository,
            setting::SettingsRepository,
            transcript::TranscriptsRepository,
        },
    },
    retrieval::{
        hydrate_context, hydrate_search_context, validate_hybrid_query, HybridContextResponse,
        HybridScope, HybridSearchResponse, PersistedRetrievalScope, RetrievalError,
        RetrievalLimits, RetrievalPurpose, RetrievalRequest, RetrievalService,
        MAX_HYBRID_CONTEXT_CHARS, MAX_HYBRID_SEARCH_MEETINGS, MAX_HYBRID_SEARCH_RESULTS,
        SEARCH_HYDRATION_BACKFILL,
    },
    state::AppState,
    summary::CustomOpenAIConfig,
};

// Hardcoded server URL
const APP_SERVER_URL: &str = "http://localhost:5167";

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub folder_id: Option<String>,
    pub has_notes: bool,
}

impl From<MeetingModel> for Meeting {
    fn from(meeting: MeetingModel) -> Self {
        Self {
            id: meeting.id,
            title: meeting.title,
            created_at: meeting.created_at.0.to_rfc3339(),
            folder_id: meeting.folder_id,
            has_notes: meeting.has_notes,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptSearchResult {
    pub id: String,
    pub title: String,
    #[serde(rename = "matchContext")]
    pub match_context: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileRequest {
    pub email: String,
    pub license_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveProfileRequest {
    pub id: String,
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateProfileRequest {
    pub email: String,
    pub license_key: String,
    pub company: String,
    pub position: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveModelConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatModelConfig {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey", skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint", skip_serializing_if = "Option::is_none")]
    pub ollama_endpoint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetApiKeyRequest {
    pub provider: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptConfig {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveTranscriptConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteMeetingRequest {
    pub meeting_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingDetails {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub folder_id: Option<String>,
    pub transcripts: Vec<MeetingTranscript>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingTranscript {
    pub id: String,
    pub text: String,
    pub timestamp: String,
    // Recording-relative timestamps for audio-transcript synchronization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_start_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_end_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

/// Meeting metadata without transcripts (for pagination)
#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingMetadata {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub folder_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
}

/// Paginated transcripts response with total count
#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedTranscriptsResponse {
    pub transcripts: Vec<MeetingTranscript>,
    pub total_count: i64,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveMeetingTitleRequest {
    pub meeting_id: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveMeetingSummaryRequest {
    pub meeting_id: String,
    pub summary: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveTranscriptRequest {
    pub meeting_title: String,
    pub transcripts: Vec<TranscriptSegment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub text: String,
    pub timestamp: String,
    // NEW: Recording-relative timestamps for playback synchronization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_start_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_end_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: Option<String>,
    pub email: String,
    pub license_key: String,
    pub company: Option<String>,
    pub position: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub is_licensed: bool,
}

// Helper function to get auth token from store (optional)
#[allow(dead_code)]
async fn get_auth_token<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let store = match app.store("store.json") {
        Ok(store) => store,
        Err(_) => return None,
    };

    match store.get("authToken") {
        Some(token) => {
            if let Some(token_str) = token.as_str() {
                let truncated = token_str.chars().take(20).collect::<String>();
                log_info!("Found auth token: {}", truncated);
                Some(token_str.to_string())
            } else {
                log_warn!("Auth token is not a string");
                None
            }
        }
        None => {
            log_warn!("No auth token found in store");
            None
        }
    }
}

// Helper function to get server address - now hardcoded
async fn get_server_address<R: Runtime>(_app: &AppHandle<R>) -> Result<String, String> {
    log_info!("Using hardcoded server URL: {}", APP_SERVER_URL);
    Ok(APP_SERVER_URL.to_string())
}

// Generic API call function with optional authentication
async fn make_api_request<R: Runtime, T: for<'de> Deserialize<'de>>(
    app: &AppHandle<R>,
    endpoint: &str,
    method: &str,
    body: Option<&str>,
    additional_headers: Option<HashMap<String, String>>,
    auth_token: Option<String>, // Pass auth token from frontend
) -> Result<T, String> {
    let client = reqwest::Client::new();
    let server_url = get_server_address(app).await?;

    let url = format!("{}{}", server_url, endpoint);
    log_info!("Making {} request to: {}", method, url);

    let mut request = match method.to_uppercase().as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        _ => return Err(format!("Unsupported HTTP method: {}", method)),
    };

    // Add authorization header if auth token is provided
    if let Some(token) = auth_token {
        log_info!("Adding authorization header");
        request = request.header("Authorization", format!("Bearer {}", token));
    } else {
        log_warn!("No auth token provided, making unauthenticated request");
    }

    request = request.header("Content-Type", "application/json");

    // Add additional headers if provided
    if let Some(headers) = additional_headers {
        for (key, value) in headers {
            request = request.header(&key, &value);
        }
    }

    // Add body if provided
    if let Some(body_str) = body {
        request = request.body(body_str.to_string());
    }

    let response = request.send().await.map_err(|e| {
        let error_msg = format!("Request failed: {}", e);
        log_error!("{}", error_msg);
        error_msg
    })?;

    let status = response.status();
    log_info!("Response status: {}", status);

    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        let error_msg = format!("HTTP {}: {}", status, error_text);
        log_error!("{}", error_msg);
        return Err(error_msg);
    }

    let response_text = response.text().await.map_err(|e| {
        let error_msg = format!("Failed to read response: {}", e);
        log_error!("{}", error_msg);
        error_msg
    })?;

    // Safely truncate response for logging, respecting UTF-8 character boundaries
    let truncated = response_text.chars().take(200).collect::<String>();
    log_info!("Response body: {}", truncated);

    serde_json::from_str(&response_text).map_err(|e| {
        let error_msg = format!("Failed to parse JSON: {}", e);
        log_error!("{}", error_msg);
        error_msg
    })
}

// API Commands for Tauri

fn lexical_search_log_fields(query: &str, mode: &str, result_count: usize) -> String {
    format!(
        "query_len={}, mode={}, results={}",
        query.chars().count(),
        mode,
        result_count
    )
}

pub(crate) const HYBRID_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const HYBRID_SEARCH_DEFAULT_LIMIT: u32 = 20;
pub(crate) const HYBRID_CONTEXT_DEFAULT_CHARS: u32 = 100_000;
pub(crate) const MCP_HYBRID_BUSY_ERROR: &str =
    "MCP hybrid request is at its concurrent request limit; retry shortly";
const HYBRID_INVALIDATED_ERROR: &str = "Hybrid result was invalidated";

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct HybridPublicationGate {
    entered: std::sync::Arc<tokio::sync::Notify>,
    release: std::sync::Arc<tokio::sync::Notify>,
}

#[cfg(test)]
impl HybridPublicationGate {
    pub(crate) fn new() -> Self {
        Self {
            entered: std::sync::Arc::new(tokio::sync::Notify::new()),
            release: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub(crate) async fn wait(&self) {
        self.entered.notified().await;
    }

    pub(crate) fn release(&self) {
        self.release.notify_one();
    }
}

#[cfg(test)]
static HYBRID_PUBLICATION_GATE: std::sync::Mutex<Option<(String, HybridPublicationGate)>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn set_hybrid_publication_gate(request_id: &str, gate: Option<HybridPublicationGate>) {
    *HYBRID_PUBLICATION_GATE.lock().unwrap() = gate.map(|gate| (request_id.to_string(), gate));
}

#[cfg(test)]
async fn wait_for_hybrid_publication_gate(request_id: &str) {
    let gate = HYBRID_PUBLICATION_GATE
        .lock()
        .unwrap()
        .as_ref()
        .filter(|(id, _)| id == request_id)
        .map(|(_, gate)| gate.clone());
    if let Some(gate) = gate {
        gate.entered.notify_one();
        gate.release.notified().await;
    }
}

fn validate_hybrid_request_id(request_id: &str) -> Result<(), String> {
    if request_id.is_empty()
        || request_id.chars().count() > 128
        || !request_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-:.".contains(character))
    {
        return Err("Invalid hybrid request ID".to_string());
    }
    Ok(())
}

fn validate_hybrid_limit(limit: Option<u32>) -> Result<usize, String> {
    let limit = limit.unwrap_or(HYBRID_SEARCH_DEFAULT_LIMIT);
    if limit == 0 || limit as usize > MAX_HYBRID_SEARCH_RESULTS {
        return Err(format!(
            "Hybrid result limit must be between 1 and {}",
            MAX_HYBRID_SEARCH_RESULTS
        ));
    }
    Ok(limit as usize)
}

fn validate_hybrid_context_chars(max_context_chars: Option<u32>) -> Result<usize, String> {
    let max_context_chars = max_context_chars.unwrap_or(HYBRID_CONTEXT_DEFAULT_CHARS);
    if max_context_chars == 0 || max_context_chars as usize > MAX_HYBRID_CONTEXT_CHARS {
        return Err(format!(
            "Hybrid context limit must be between 1 and {}",
            MAX_HYBRID_CONTEXT_CHARS
        ));
    }
    Ok(max_context_chars as usize)
}

fn map_hybrid_error(error: RetrievalError) -> String {
    match error {
        RetrievalError::Cancelled => "Hybrid request was cancelled or superseded".to_string(),
        RetrievalError::InvalidQuery(_) => "Invalid hybrid query".to_string(),
        RetrievalError::InvalidScope(_) => "Invalid hybrid scope".to_string(),
        RetrievalError::UnsupportedPurpose(_) => "Hybrid request is not supported".to_string(),
        RetrievalError::Database(_) => "Hybrid retrieval is unavailable".to_string(),
    }
}

struct HybridOwnershipGuard<'a> {
    state: &'a ChatRequestState,
    surface: ChatRequestSurface,
    request_id: &'a str,
    token: &'a ChatRequestToken,
}

impl Drop for HybridOwnershipGuard<'_> {
    fn drop(&mut self) {
        self.token.cancel();
        self.state
            .clear_if_owner(self.surface, self.request_id, self.token);
    }
}

fn finish_hybrid_request<T>(
    state: &ChatRequestState,
    surface: ChatRequestSurface,
    request_id: &str,
    token: &ChatRequestToken,
    result: Result<T, String>,
    timed_out: bool,
) -> Result<T, String> {
    if timed_out {
        token.cancel();
    }
    if !state.clear_if_owner(surface, request_id, token) {
        return Err("Hybrid request was cancelled or superseded".to_string());
    }
    if timed_out {
        return Err("Hybrid request timed out".to_string());
    }
    if token.is_cancelled() {
        return Err("Hybrid request was cancelled".to_string());
    }
    result
}

pub(crate) async fn with_hybrid_request<T, F, Fut>(
    state: &ChatRequestState,
    surface: ChatRequestSurface,
    request_id: String,
    timeout: Duration,
    work: F,
) -> Result<T, String>
where
    F: FnOnce(ChatRequestToken) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    validate_hybrid_request_id(&request_id)?;
    let Some(token) = state.try_claim_request(surface, &request_id) else {
        return Err(if matches!(surface, ChatRequestSurface::Mcp) {
            MCP_HYBRID_BUSY_ERROR.to_string()
        } else {
            "Hybrid request could not be started".to_string()
        });
    };
    let guard = HybridOwnershipGuard {
        state,
        surface,
        request_id: &request_id,
        token: &token,
    };
    let work = work(token.clone());
    tokio::pin!(work);
    let deadline_sleep = tokio::time::sleep(timeout);
    tokio::pin!(deadline_sleep);
    let mut timed_out = false;
    let result = tokio::select! {
        biased;
        _ = &mut deadline_sleep => {
            token.cancel();
            timed_out = true;
            Err("Hybrid request timed out".to_string())
        }
        result = &mut work => result,
    };
    let outcome = finish_hybrid_request(state, surface, &request_id, &token, result, timed_out);
    drop(guard);
    outcome
}

fn hybrid_retrieval_request(
    query: String,
    scope: PersistedRetrievalScope,
    purpose: RetrievalPurpose,
    limits: RetrievalLimits,
    token: &CancellationToken,
) -> RetrievalRequest {
    RetrievalRequest {
        original_query: query,
        rewritten_query: None,
        scope,
        purpose,
        limits,
        core_language: crate::retrieval::CoreTermLanguage::Unknown,
        cancellation: Some(token.clone()),
    }
}

/// What a surface does when revalidation finds a hydrated meeting is no
/// longer current (deleted, or moved out of scope, since retrieval ran).
#[derive(Clone, Copy, PartialEq, Eq)]
enum StaleMeetingPolicy {
    /// Search publishes one independent row per meeting, so a stale meeting
    /// is dropped and the surviving rows are still returned.
    Drop,
    /// Context publishes ONE Markdown blob assembled from every meeting. It
    /// cannot be pruned after the fact without leaving removed content in the
    /// text, so any stale meeting fails the whole request closed.
    FailClosed,
}

/// Binds the hydrated meetings to the request, then rechecks them against
/// current existence and scope. Returns the meeting IDs that are still
/// current; under [`StaleMeetingPolicy::FailClosed`] any loss is an error
/// instead.
async fn bind_and_revalidate_hybrid(
    service: &RetrievalService,
    pool: &sqlx::SqlitePool,
    request_state: &ChatRequestState,
    surface: ChatRequestSurface,
    request_id: &str,
    token: &ChatRequestToken,
    scope: &PersistedRetrievalScope,
    hydrated: &crate::retrieval::HydratedContext,
    stale_policy: StaleMeetingPolicy,
) -> Result<HashSet<String>, String> {
    let meeting_ids = hydrated
        .meetings
        .iter()
        .map(|meeting| meeting.meeting_id.clone())
        .collect::<HashSet<_>>();
    if !request_state.bind_request_meetings(surface, request_id, token, &meeting_ids) {
        return Err("Hybrid request was cancelled or superseded".to_string());
    }
    let requested_ids = meeting_ids.iter().cloned().collect::<Vec<_>>();
    let current_ids = service
        .revalidate_ids_in_scope(pool, scope, &requested_ids, token)
        .await
        .map_err(map_hybrid_error)?
        .into_iter()
        .collect::<HashSet<_>>();
    if current_ids != meeting_ids && stale_policy == StaleMeetingPolicy::FailClosed {
        token.cancel();
        return Err(HYBRID_INVALIDATED_ERROR.to_string());
    }
    if !request_state.is_owner(surface, request_id, token) {
        return Err("Hybrid request was cancelled or superseded".to_string());
    }
    Ok(current_ids)
}

/// Removes every meeting the recheck did not confirm, so a meeting deleted or
/// moved out of scope since retrieval ran can never reach a published result.
fn retain_current_meetings(
    hydrated: &mut crate::retrieval::HydratedContext,
    current_ids: &HashSet<String>,
) {
    hydrated
        .meetings
        .retain(|meeting| current_ids.contains(&meeting.meeting_id));
    hydrated
        .sources
        .retain(|source| current_ids.contains(&source.meeting_id));
    let published: HashSet<&str> = hydrated
        .meetings
        .iter()
        .flat_map(|meeting| meeting.retained_evidence_ids.iter().map(String::as_str))
        .collect();
    hydrated
        .retained_evidence_ids
        .retain(|evidence_id| published.contains(evidence_id.as_str()));
}

/// Per-meeting hydration budget for the search surface. A search result needs
/// one snippet per meeting, not a Chat-sized context: the budget scales with
/// the caller's own result limit instead of always spending the contract
/// maximum (and the assembled Markdown is discarded by the search contract).
const SEARCH_CONTEXT_CHARS_PER_MEETING: usize = 1_200;

pub(crate) async fn execute_hybrid_search(
    pool: &sqlx::SqlitePool,
    retrieval: crate::retrieval::worker::RetrievalLifecycle,
    request_state: &ChatRequestState,
    surface: ChatRequestSurface,
    request_id: String,
    query: String,
    scope: HybridScope,
    limit: Option<u32>,
    timeout: Duration,
) -> Result<HybridSearchResponse, String> {
    validate_hybrid_query(&query)?;
    let scope = scope.into_persisted()?;
    let limit = validate_hybrid_limit(limit)?;
    let work_request_id = request_id.clone();
    with_hybrid_request(request_state, surface, request_id, timeout, move |token| {
        let request_id = work_request_id;
        async move {
            let service = RetrievalService::new(retrieval);
            let ranked = service
                .retrieve_ranked(
                    pool,
                    hybrid_retrieval_request(
                        query,
                        scope.clone(),
                        RetrievalPurpose::Search,
                        RetrievalLimits::chat_default(),
                        &token,
                    ),
                )
                .await
                .map_err(map_hybrid_error)?;
            let max_meetings = limit
                .saturating_add(SEARCH_HYDRATION_BACKFILL)
                .min(MAX_HYBRID_SEARCH_MEETINGS);
            let mut hydrated = hydrate_search_context(
                pool,
                &ranked,
                max_meetings
                    .saturating_mul(SEARCH_CONTEXT_CHARS_PER_MEETING)
                    .min(MAX_HYBRID_CONTEXT_CHARS),
                max_meetings,
                Some(&token),
            )
            .await
            .map_err(map_hybrid_error)?;
            let current_ids = bind_and_revalidate_hybrid(
                &service,
                pool,
                request_state,
                surface,
                &request_id,
                &token,
                &ranked.scope.scope,
                &hydrated,
                StaleMeetingPolicy::Drop,
            )
            .await?;
            retain_current_meetings(&mut hydrated, &current_ids);
            #[cfg(test)]
            wait_for_hybrid_publication_gate(&request_id).await;
            request_state
                .publish_hybrid_if_current(surface, &request_id, &token, || {
                    HybridSearchResponse::from_outputs(&ranked, &hydrated, limit)
                })
                .ok_or_else(|| "Hybrid request was cancelled or superseded".to_string())
        }
    })
    .await
}

pub(crate) async fn execute_hybrid_context(
    pool: &sqlx::SqlitePool,
    retrieval: crate::retrieval::worker::RetrievalLifecycle,
    request_state: &ChatRequestState,
    surface: ChatRequestSurface,
    request_id: String,
    query: String,
    scope: HybridScope,
    max_context_chars: Option<u32>,
    timeout: Duration,
) -> Result<HybridContextResponse, String> {
    validate_hybrid_query(&query)?;
    let scope = scope.into_persisted()?;
    let max_context_chars = validate_hybrid_context_chars(max_context_chars)?;
    let work_request_id = request_id.clone();
    with_hybrid_request(request_state, surface, request_id, timeout, move |token| {
        let request_id = work_request_id;
        async move {
            let service = RetrievalService::new(retrieval);
            let ranked = service
                .retrieve_ranked(
                    pool,
                    hybrid_retrieval_request(
                        query,
                        scope.clone(),
                        RetrievalPurpose::Context,
                        RetrievalLimits::chat_default(),
                        &token,
                    ),
                )
                .await
                .map_err(map_hybrid_error)?;
            let hydrated = hydrate_context(pool, &ranked, max_context_chars, Some(&token))
                .await
                .map_err(map_hybrid_error)?;
            bind_and_revalidate_hybrid(
                &service,
                pool,
                request_state,
                surface,
                &request_id,
                &token,
                &ranked.scope.scope,
                &hydrated,
                StaleMeetingPolicy::FailClosed,
            )
            .await?;
            #[cfg(test)]
            wait_for_hybrid_publication_gate(&request_id).await;
            request_state
                .publish_hybrid_if_current(surface, &request_id, &token, || {
                    HybridContextResponse::from_outputs(&ranked, &hydrated)
                })
                .ok_or_else(|| "Hybrid request was cancelled or superseded".to_string())
        }
    })
    .await
}

#[tauri::command]
pub async fn api_get_meetings<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    auth_token: Option<String>,
) -> Result<Vec<Meeting>, String> {
    log_info!(
        "api_get_meetings called with auth_token(native) : {}",
        auth_token.is_some()
    );
    let pool = state.db_manager.pool();
    let meetings: Result<Vec<MeetingModel>, sqlx::Error> =
        MeetingsRepository::get_meetings(pool).await;

    match meetings {
        Ok(meeting_models) => {
            log_info!("Successfully got {} meetings", meeting_models.len());

            let result: Vec<Meeting> = meeting_models.into_iter().map(Meeting::from).collect();
            Ok(result)
        }
        Err(e) => {
            log_error!("Error getting meetings: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_search_transcripts<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    query: String,
    auth_token: Option<String>,
) -> Result<Vec<TranscriptSearchResult>, String> {
    let pool = state.db_manager.pool();

    match TranscriptsRepository::search_transcripts(pool, &query).await {
        Ok(results) => {
            log_info!(
                "api_search_transcripts completed: {}, auth_token={}",
                lexical_search_log_fields(&query, "legacy_transcript", results.len()),
                auth_token.is_some()
            );
            Ok(results)
        }
        Err(e) => {
            log_error!(
                "api_search_transcripts failed: query_len={}, mode=legacy_transcript, error={}",
                query.chars().count(),
                e
            );
            Err(format!("Failed to search transcripts: {}", e))
        }
    }
}

#[tauri::command]
pub async fn api_get_profile<R: Runtime>(
    app: AppHandle<R>,
    email: String,
    license_key: String,
    auth_token: Option<String>,
) -> Result<Profile, String> {
    log_info!(
        "api_get_profile called for email: {}, auth_token: {}",
        email,
        auth_token.is_some()
    );

    let profile_request = ProfileRequest { email, license_key };
    let body = serde_json::to_string(&profile_request).map_err(|e| e.to_string())?;

    make_api_request::<R, Profile>(&app, "/get-profile", "POST", Some(&body), None, auth_token)
        .await
}

#[tauri::command]
pub async fn api_save_profile<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    email: String,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_profile called for email: {}, auth_token: {}",
        email,
        auth_token.is_some()
    );

    let save_request = SaveProfileRequest { id, email };
    let body = serde_json::to_string(&save_request).map_err(|e| e.to_string())?;

    make_api_request::<R, serde_json::Value>(
        &app,
        "/save-profile",
        "POST",
        Some(&body),
        None,
        auth_token,
    )
    .await
}

#[tauri::command]
pub async fn api_update_profile<R: Runtime>(
    app: AppHandle<R>,
    email: String,
    license_key: String,
    company: String,
    position: String,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_update_profile called for email: {}, auth_token: {}",
        email,
        auth_token.is_some()
    );

    let update_request = UpdateProfileRequest {
        email,
        license_key,
        company,
        position,
    };
    let body = serde_json::to_string(&update_request).map_err(|e| e.to_string())?;

    make_api_request::<R, serde_json::Value>(
        &app,
        "/update-profile",
        "POST",
        Some(&body),
        None,
        auth_token,
    )
    .await
}

#[tauri::command]
pub async fn api_get_model_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    _auth_token: Option<String>,
) -> Result<Option<ModelConfig>, String> {
    log_info!("api_get_model_config called (native)");
    let pool = state.db_manager.pool();

    match SettingsRepository::get_model_config(pool).await {
        Ok(Some(config)) => {
            log_info!(
                "✅ Found model config in database: provider={}, model={}, whisperModel={}, ollamaEndpoint={:?}",
                &config.provider,
                &config.model,
                &config.whisper_model,
                &config.ollama_endpoint
            );
            match SettingsRepository::get_api_key(pool, &config.provider).await {
                Ok(api_key) => {
                    log_info!("Successfully retrieved model config and API key.");
                    Ok(Some(ModelConfig {
                        provider: config.provider,
                        model: config.model,
                        whisper_model: config.whisper_model,
                        api_key,
                        ollama_endpoint: config.ollama_endpoint,
                    }))
                }
                Err(e) => {
                    log_error!(
                        "Failed to get API key for provider {}: {}",
                        &config.provider,
                        e
                    );
                    Err(e.to_string())
                }
            }
        }
        Ok(None) => {
            log_warn!("⚠️ No model config found in database - database may be empty or settings table not initialized");
            Ok(None)
        }
        Err(e) => {
            log_error!("❌ Failed to get model config from database: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_save_model_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    model: String,
    whisper_model: String,
    api_key: Option<String>,
    ollama_endpoint: Option<String>,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "💾 api_save_model_config called (native): provider='{}', model='{}', whisperModel='{}', ollamaEndpoint={:?}",
        &provider,
        &model,
        &whisper_model,
        &ollama_endpoint
    );
    let pool = state.db_manager.pool();

    if let Err(e) = SettingsRepository::save_model_config(
        pool,
        &provider,
        &model,
        &whisper_model,
        ollama_endpoint.as_deref(),
    )
    .await
    {
        log_error!("❌ Failed to save model config to database: {}", e);
        return Err(e.to_string());
    }

    // Skip API key saving for custom-openai provider (it uses customOpenAIConfig JSON instead)
    if let Some(key) = api_key {
        if !key.is_empty() && provider != "custom-openai" {
            log_info!("🔑 API key provided, saving...");
            if let Err(e) = SettingsRepository::save_api_key(pool, &provider, &key).await {
                log_error!("❌ Failed to save API key: {}", e);
                return Err(e.to_string());
            }
        }
    }

    // Trigger graceful shutdown of built-in AI sidecar if it's running
    // This ensures that if the user switched models/providers, the old one is cleaned up
    // The shutdown happens in the background, so it won't block the UI
    if let Err(e) = crate::summary::summary_engine::client::shutdown_sidecar_gracefully().await {
        log_warn!("Failed to initiate graceful sidecar shutdown: {}", e);
    }

    log_info!("✅ Successfully saved model configuration to database");
    Ok(
        serde_json::json!({ "status": "success", "message": "Model configuration saved successfully" }),
    )
}

#[tauri::command]
pub async fn api_search_fts<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    query: String,
    limit: Option<u32>,
    auth_token: Option<String>,
) -> Result<Vec<FtsSearchResult>, String> {
    let pool = state.db_manager.pool();
    let limit = limit.unwrap_or(20);
    let results = FtsRepository::search(pool, &query, limit, None)
        .await
        .map_err(|e| format!("Failed to search FTS index: {}", e))?;
    log_info!(
        "api_search_fts completed: {}, limit={}, auth_token={}",
        lexical_search_log_fields(&query, "fts_or", results.len()),
        limit,
        auth_token.is_some()
    );
    Ok(results)
}

#[tauri::command]
pub async fn api_search_hybrid<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    retrieval: tauri::State<'_, crate::retrieval::worker::RetrievalLifecycle>,
    request_state: tauri::State<'_, ChatRequestState>,
    query: String,
    scope: HybridScope,
    limit: Option<u32>,
    request_id: String,
) -> Result<HybridSearchResponse, String> {
    execute_hybrid_search(
        state.db_manager.pool(),
        retrieval.inner().clone(),
        request_state.inner(),
        ChatRequestSurface::Sidebar,
        request_id,
        query,
        scope,
        limit,
        HYBRID_REQUEST_TIMEOUT,
    )
    .await
}

#[tauri::command]
pub async fn api_build_hybrid_context<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    retrieval: tauri::State<'_, crate::retrieval::worker::RetrievalLifecycle>,
    request_state: tauri::State<'_, ChatRequestState>,
    query: String,
    scope: HybridScope,
    max_context_chars: Option<u32>,
    request_id: String,
) -> Result<HybridContextResponse, String> {
    execute_hybrid_context(
        state.db_manager.pool(),
        retrieval.inner().clone(),
        request_state.inner(),
        ChatRequestSurface::Sidebar,
        request_id,
        query,
        scope,
        max_context_chars,
        HYBRID_REQUEST_TIMEOUT,
    )
    .await
}

#[tauri::command]
pub async fn api_cancel_hybrid_request(
    request_state: tauri::State<'_, ChatRequestState>,
    request_id: String,
) -> Result<(), String> {
    validate_hybrid_request_id(&request_id)?;
    request_state.cancel_request(ChatRequestSurface::Sidebar, Some(&request_id));
    Ok(())
}

#[tauri::command]
pub async fn api_rebuild_fts_index<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    auth_token: Option<String>,
) -> Result<u64, String> {
    log_info!(
        "api_rebuild_fts_index called, auth_token: {}",
        auth_token.is_some()
    );
    let pool = state.db_manager.pool();
    FtsRepository::rebuild_index(pool)
        .await
        .map_err(|e| format!("Failed to rebuild FTS index: {}", e))
}

#[tauri::command]
pub async fn api_get_chat_model_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    _auth_token: Option<String>,
) -> Result<Option<ChatModelConfig>, String> {
    log_info!("api_get_chat_model_config called (native)");
    let pool = state.db_manager.pool();

    match SettingsRepository::get_chat_model_config(pool).await {
        Ok(Some(config)) => {
            let (provider, model, ollama_endpoint) =
                SettingsRepository::resolve_chat_config(&config);
            let api_key = match SettingsRepository::get_api_key(pool, &provider).await {
                Ok(k) => k,
                Err(e) => {
                    log_error!(
                        "Failed to get API key for chat provider {}: {}",
                        provider,
                        e
                    );
                    None
                }
            };
            log_info!(
                "✅ Found chat model config: provider={}, model={}, ollamaEndpoint={:?}",
                provider,
                model,
                ollama_endpoint
            );
            Ok(Some(ChatModelConfig {
                provider,
                model,
                api_key,
                ollama_endpoint,
            }))
        }
        Ok(None) => {
            log_warn!("⚠️ No chat model config found in database");
            Ok(None)
        }
        Err(e) => {
            log_error!("❌ Failed to get chat model config from database: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_save_chat_model_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    model: String,
    api_key: Option<String>,
    ollama_endpoint: Option<String>,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "💾 api_save_chat_model_config called: provider='{}', model='{}', ollamaEndpoint={:?}",
        provider,
        model,
        ollama_endpoint
    );
    let pool = state.db_manager.pool();

    if let Err(e) = SettingsRepository::save_chat_model_config(
        pool,
        &provider,
        &model,
        ollama_endpoint.as_deref(),
    )
    .await
    {
        log_error!("❌ Failed to save chat model config to database: {}", e);
        return Err(e.to_string());
    }

    if let Some(key) = api_key {
        if !key.is_empty() && provider != "custom-openai" {
            log_info!("🔑 Chat API key provided, saving...");
            if let Err(e) = SettingsRepository::save_api_key(pool, &provider, &key).await {
                log_error!("❌ Failed to save chat API key: {}", e);
                return Err(e.to_string());
            }
        }
    }

    log_info!("✅ Successfully saved chat model configuration to database");
    Ok(serde_json::json!({
        "status": "success",
        "message": "Chat model configuration saved successfully"
    }))
}

#[tauri::command]
pub async fn api_get_api_key<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    _auth_token: Option<String>,
) -> Result<String, String> {
    log_info!(
        "api_get_api_key called (native) for provider '{}'",
        &provider
    );
    match SettingsRepository::get_api_key(&state.db_manager.pool(), &provider).await {
        Ok(key) => {
            log_info!(
                "Successfully retrieved API key for provider '{}'.",
                &provider
            );
            Ok(key.unwrap_or_default())
        }
        Err(e) => {
            log_error!("Failed to get API key for provider '{}': {}", &provider, e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_get_transcript_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    _auth_token: Option<String>,
) -> Result<Option<TranscriptConfig>, String> {
    log_info!("api_get_transcript_config called (native)");
    let pool = state.db_manager.pool();

    match SettingsRepository::get_transcript_config(pool).await {
        Ok(Some(config)) => {
            log_info!(
                "Found transcript config: provider={}, model={}",
                &config.provider,
                &config.model
            );
            match SettingsRepository::get_transcript_api_key(pool, &config.provider).await {
                Ok(api_key) => {
                    log_info!("Successfully retrieved transcript config and API key.");
                    Ok(Some(TranscriptConfig {
                        provider: config.provider,
                        model: config.model,
                        api_key,
                    }))
                }
                Err(e) => {
                    log_error!(
                        "Failed to get transcript API key for provider {}: {}",
                        &config.provider,
                        e
                    );
                    Err(e.to_string())
                }
            }
        }
        Ok(None) => {
            log_info!("No transcript config found, returning default.");
            Ok(Some(TranscriptConfig {
                provider: "parakeet".to_string(),
                model: crate::config::DEFAULT_PARAKEET_MODEL.to_string(),
                api_key: None,
            }))
        }
        Err(e) => {
            log_error!("Failed to get transcript config: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_save_transcript_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    model: String,
    api_key: Option<String>,
    _auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_transcript_config called (native) for provider '{}'",
        &provider
    );
    let pool = state.db_manager.pool();

    if let Err(e) = SettingsRepository::save_transcript_config(pool, &provider, &model).await {
        log_error!("Failed to save transcript config: {}", e);
        return Err(e.to_string());
    }

    if let Some(key) = api_key {
        if !key.is_empty() {
            log_info!("API key provided, saving for transcript provider...");
            if let Err(e) = SettingsRepository::save_transcript_api_key(pool, &provider, &key).await
            {
                log_error!("Failed to save transcript API key: {}", e);
                return Err(e.to_string());
            }
        }
    }

    log_info!("Successfully saved transcript configuration.");
    Ok(
        serde_json::json!({ "status": "success", "message": "Transcript configuration saved successfully" }),
    )
}

#[tauri::command]
pub async fn api_get_transcript_api_key<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    _auth_token: Option<String>,
) -> Result<String, String> {
    log_info!(
        "api_get_transcript_api_key called (native) for provider '{}'",
        &provider
    );
    match SettingsRepository::get_transcript_api_key(&state.db_manager.pool(), &provider).await {
        Ok(key) => {
            log_info!(
                "Successfully retrieved transcript API key for provider '{}'.",
                &provider
            );
            Ok(key.unwrap_or_default())
        }
        Err(e) => {
            log_error!(
                "Failed to get transcript API key for provider '{}': {}",
                &provider,
                e
            );
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_delete_api_key<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: String,
    _auth_token: Option<String>,
) -> Result<(), String> {
    log_info!(
        "log_api_delete_api_key called (native) for provider '{}'",
        &provider
    );
    match SettingsRepository::delete_api_key(&state.db_manager.pool(), &provider).await {
        Ok(_) => {
            log_info!("Successfully deleted API key for provider '{}'.", &provider);
            Ok(())
        }
        Err(e) => {
            log_error!(
                "Failed to delete API key for provider '{}': {}",
                &provider,
                e
            );
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn api_delete_meeting<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    retrieval: tauri::State<'_, crate::retrieval::worker::RetrievalLifecycle>,
    chat_requests: tauri::State<'_, crate::api::chat::ChatRequestState>,
    meeting_id: String,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_delete_meeting called for meeting_id(native): {}, auth_token: {}",
        meeting_id,
        auth_token.is_some()
    );

    let pool = state.db_manager.pool();

    let index = retrieval.index_service();
    let active_generation = index.active_generation();
    let stale_epoch = index.mark_stale();
    // The invalidation hook runs inside the deletion transaction, before the
    // meeting row disappears: active chat requests whose prepared evidence
    // references this meeting are cancelled through the one shared request
    // registry, so no later chunk/source/done publication can occur.
    let chat_requests = chat_requests.inner().clone();
    let deletion_guard = chat_requests.begin_meeting_deletion(&meeting_id);
    let deletion = MeetingsRepository::delete_meeting(pool, &meeting_id, |deleted_meeting_id| {
        chat_requests.invalidate_meeting(deleted_meeting_id);
    })
    .await;
    drop(deletion_guard);
    // One identity-only local notification AFTER the transaction committed:
    // never before commit, on rollback, or with content/sources. The renderer
    // uses it to drop the deleted meeting's retained sources from already
    // loaded chat messages.
    crate::api::chat::emit_chat_meeting_deleted_if_committed(
        |event, payload| {
            if let Err(error) = app.emit(event, payload) {
                log_error!("Failed to emit {}: {}", event, error);
            }
        },
        &meeting_id,
        &deletion,
    );
    match deletion {
        Ok(true) => {
            match active_generation {
                Some(generation_id) => {
                    match RetrievalRepository::publication_lag(pool, &generation_id).await {
                        Ok(Some((canonical_change_id, published_change_id))) => {
                            index.commit_stale(
                                stale_epoch,
                                &generation_id,
                                canonical_change_id,
                                Some(published_change_id),
                            );
                        }
                        _ => index.commit_stale(stale_epoch, &generation_id, i64::MAX, None),
                    }
                }
                _ => index.restore_stale(stale_epoch),
            }
            log_info!("Successfully deleted meeting {}", meeting_id);
            Ok(serde_json::json!({
                "status": "success",
                "message": "Meeting deleted successfully"
            }))
        }
        Ok(false) => {
            index.restore_stale(stale_epoch);
            log_warn!("Meeting not found or already deleted: {}", meeting_id);
            Err(format!(
                "Meeting not found or could not be deleted: {}",
                meeting_id
            ))
        }
        Err(e) => {
            index.restore_stale(stale_epoch);
            log_error!("Error deleting meeting {}: {}", meeting_id, e);
            Err(format!("Failed to delete meeting: {}", e))
        }
    }
}

#[tauri::command]
pub async fn api_get_meeting<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    state: tauri::State<'_, AppState>,
    auth_token: Option<String>,
) -> Result<MeetingDetails, String> {
    log_info!(
        "api_get_meeting called(native) for meeting_id: {}, auth_token: {}",
        meeting_id,
        auth_token.is_some()
    );

    let pool = state.db_manager.pool();

    match MeetingsRepository::get_meeting(pool, &meeting_id).await {
        Ok(Some(meeting)) => {
            log_info!("Successfully retrieved meeting {}", meeting_id);
            Ok(meeting)
        }
        Ok(None) => {
            log_warn!("Meeting not found: {}", meeting_id);
            Err(format!("Meeting not found: {}", meeting_id))
        }
        Err(e) => {
            log_error!("Error retrieving meeting {}: {}", meeting_id, e);
            Err(format!("Failed to retrieve meeting: {}", e))
        }
    }
}

/// Get meeting metadata without transcripts (for pagination)
#[tauri::command]
pub async fn api_get_meeting_metadata<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<MeetingMetadata, String> {
    log_info!(
        "api_get_meeting_metadata called for meeting_id: {}",
        meeting_id
    );

    let pool = state.db_manager.pool();

    match MeetingsRepository::get_meeting_metadata(pool, &meeting_id).await {
        Ok(Some(meeting)) => {
            log_info!("Successfully retrieved meeting metadata {}", meeting_id);
            Ok(MeetingMetadata {
                id: meeting.id,
                title: meeting.title,
                created_at: meeting.created_at.0.to_rfc3339(),
                updated_at: meeting.updated_at.0.to_rfc3339(),
                folder_id: meeting.folder_id,
                folder_path: meeting.folder_path,
            })
        }
        Ok(None) => {
            log_warn!("Meeting not found: {}", meeting_id);
            Err(format!("Meeting not found: {}", meeting_id))
        }
        Err(e) => {
            log_error!("Error retrieving meeting metadata {}: {}", meeting_id, e);
            Err(format!("Failed to retrieve meeting metadata: {}", e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::models::DateTimeUtc;

    #[test]
    fn meeting_mapper_preserves_null_folder_id() {
        let now = chrono::Utc::now();
        let meeting = Meeting::from(MeetingModel {
            id: "meeting-1".to_string(),
            title: "Test meeting".to_string(),
            created_at: DateTimeUtc(now),
            updated_at: DateTimeUtc(now),
            folder_path: None,
            folder_id: None,
            has_notes: false,
        });

        let json = serde_json::to_value(meeting).expect("serialize meeting");
        assert!(json.get("folder_id").expect("folder_id field").is_null());
    }

    #[test]
    fn meeting_mapper_preserves_logical_folder_id() {
        let now = chrono::Utc::now();
        let meeting = Meeting::from(MeetingModel {
            id: "meeting-1".to_string(),
            title: "Test meeting".to_string(),
            created_at: DateTimeUtc(now),
            updated_at: DateTimeUtc(now),
            folder_path: None,
            folder_id: Some("folder-1".to_string()),
            has_notes: false,
        });

        assert_eq!(meeting.folder_id.as_deref(), Some("folder-1"));
    }

    #[test]
    fn lexical_search_info_fields_never_contain_raw_query_text() {
        for query in [
            "quais reuniões falaram sobre retenção",
            "which meetings discussed retention",
        ] {
            for mode in ["fts_or", "legacy_transcript"] {
                let fields = lexical_search_log_fields(query, mode, 3);
                assert_eq!(
                    fields,
                    format!(
                        "query_len={}, mode={}, results=3",
                        query.chars().count(),
                        mode
                    )
                );
                assert!(!fields.contains(query));
            }
        }
    }

    #[test]
    fn hybrid_request_bounds_and_ids_are_validated() {
        assert_eq!(validate_hybrid_limit(None).unwrap(), 20);
        assert_eq!(validate_hybrid_limit(Some(50)).unwrap(), 50);
        assert!(validate_hybrid_limit(Some(0)).is_err());
        assert!(validate_hybrid_limit(Some(51)).is_err());
        assert_eq!(validate_hybrid_context_chars(None).unwrap(), 100_000);
        assert!(validate_hybrid_context_chars(Some(100_001)).is_err());
        assert!(validate_hybrid_request_id("").is_err());
        assert!(validate_hybrid_request_id("request/id").is_err());
        assert!(validate_hybrid_request_id("sidebar-search-1").is_ok());
    }

    #[tokio::test]
    async fn hybrid_cancellation_reclaims_sidebar_ownership() {
        let state = ChatRequestState::new();
        let work_state = state.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            with_hybrid_request(
                &work_state,
                ChatRequestSurface::Sidebar,
                "hybrid-cancel".to_string(),
                Duration::from_secs(30),
                move |token| async move {
                    let _ = started_tx.send(());
                    token.cancelled().await;
                    Ok::<_, String>(())
                },
            )
            .await
        });
        started_rx.await.unwrap();
        assert!(state.cancel_request(ChatRequestSurface::Sidebar, Some("hybrid-cancel")));
        assert_eq!(
            task.await.unwrap(),
            Err("Hybrid request was cancelled or superseded".to_string())
        );
        assert_eq!(state.request_count(), 0);
    }

    #[tokio::test]
    async fn hybrid_mcp_timeout_cancels_the_shared_request_token() {
        let state = ChatRequestState::new();
        let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
        let task_state = state.clone();
        let task_observed = observed.clone();
        let result = with_hybrid_request(
            &task_state,
            ChatRequestSurface::Mcp,
            "mcp-hybrid-timeout".to_string(),
            Duration::from_millis(10),
            move |token| async move {
                *task_observed.lock().unwrap() = Some(token.clone());
                token.cancelled().await;
                Ok::<_, String>(())
            },
        )
        .await;
        assert_eq!(result, Err("Hybrid request timed out".to_string()));
        assert!(observed
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|token| token.is_cancelled()));
        assert_eq!(state.request_count(), 0);
    }
}

/// Get paginated transcripts for a meeting
#[tauri::command]
pub async fn api_get_meeting_transcripts<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    limit: i64,
    offset: i64,
    state: tauri::State<'_, AppState>,
) -> Result<PaginatedTranscriptsResponse, String> {
    log_info!(
        "api_get_meeting_transcripts called for meeting_id: {}, limit: {}, offset: {}",
        meeting_id,
        limit,
        offset
    );

    let pool = state.db_manager.pool();

    match MeetingsRepository::get_meeting_transcripts_paginated(pool, &meeting_id, limit, offset)
        .await
    {
        Ok((transcripts, total_count)) => {
            log_info!(
                "Successfully retrieved {} transcripts for meeting {} (total: {})",
                transcripts.len(),
                meeting_id,
                total_count
            );

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

            let has_more = (offset + meeting_transcripts.len() as i64) < total_count;

            Ok(PaginatedTranscriptsResponse {
                transcripts: meeting_transcripts,
                total_count,
                has_more,
            })
        }
        Err(e) => {
            log_error!(
                "Error retrieving transcripts for meeting {}: {}",
                meeting_id,
                e
            );
            Err(format!("Failed to retrieve transcripts: {}", e))
        }
    }
}

#[tauri::command]
pub async fn api_save_meeting_title<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    title: String,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_meeting_title called for meeting_id: {}, auth_token: {}",
        meeting_id,
        auth_token.is_some()
    );
    let pool = state.db_manager.pool();
    match MeetingsRepository::update_meeting_title(pool, &meeting_id, &title).await {
        Ok(true) => {
            log_info!("Successfully saved meeting title");
            Ok(serde_json::json!({"message": "Meeting title saved successfully"}))
        }
        Ok(false) => {
            log_error!("No meeting found with id {}", meeting_id);
            Err(format!("No meeting found with id {}", meeting_id))
        }
        Err(e) => {
            log_error!("Failed to update meeting {}", e);
            Err(format!("Failed to update meeting: {}", e))
        }
    }
}

#[tauri::command]
pub async fn api_save_transcript<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_title: String,
    transcripts: Vec<serde_json::Value>,
    folder_path: Option<String>,
    live_scope_key: Option<String>,
    auth_token: Option<String>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_transcript called for meeting: {}, transcripts: {}, folder_path: {:?}, auth_token: {}",
        meeting_title,
        transcripts.len(),
        folder_path,
        auth_token.is_some()
    );

    // Log first transcript for debugging
    if let Some(first) = transcripts.first() {
        log_debug!(
            "First transcript data: {}",
            serde_json::to_string_pretty(first).unwrap_or_default()
        );
    }

    // Convert serde_json::Value to TranscriptSegment
    let transcripts_to_save: Vec<TranscriptSegment> = transcripts
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            log_error!("Failed to parse transcript segments: {}", e);
            format!(
                "Invalid transcript data format: {}. Please check the data structure.",
                e
            )
        })?;

    // Log parsed segments count and first segment details
    if let Some(first_seg) = transcripts_to_save.first() {
        log_debug!("First parsed segment: text='{}', audio_start_time={:?}, audio_end_time={:?}, duration={:?}",
                   first_seg.text.chars().take(50).collect::<String>(),
                   first_seg.audio_start_time,
                   first_seg.audio_end_time,
                   first_seg.duration);
    }

    let pool = state.db_manager.pool();

    // Now, call the repository with the correctly typed data.
    match TranscriptsRepository::save_transcript(
        pool,
        &meeting_title,
        &transcripts_to_save,
        folder_path,
        live_scope_key.as_deref(),
    )
    .await
    {
        Ok(meeting_id) => {
            log_info!(
                "Successfully saved transcript and created meeting with id: {}",
                meeting_id
            );
            Ok(serde_json::json!({
                "status": "success",
                "message": "Transcript saved successfully",
                "meeting_id": meeting_id
            }))
        }
        Err(e) => {
            log_error!(
                "Error saving transcript for meeting '{}': {}",
                meeting_title,
                e
            );
            Err(format!("Failed to save transcript: {}", e))
        }
    }
}

/// Opens the meeting's recording folder in the system file explorer
#[tauri::command]
pub async fn open_meeting_folder<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<(), String> {
    log_info!("open_meeting_folder called for meeting_id: {}", meeting_id);

    let pool = state.db_manager.pool();

    // Get both disk and logical folder metadata. `MeetingModel` includes the
    // logical folder so this query must stay in sync with it.
    let meeting: Option<MeetingModel> = sqlx::query_as(
        "SELECT id, title, created_at, updated_at, folder_path, folder_id FROM meetings WHERE id = ?",
    )
    .bind(&meeting_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    match meeting {
        Some(m) => {
            if let Some(folder_path) = m.folder_path {
                log_info!("Opening meeting folder: {}", folder_path);

                // Verify folder exists
                let path = std::path::Path::new(&folder_path);
                if !path.exists() {
                    log_warn!("Folder path does not exist: {}", folder_path);
                    return Err(format!("Recording folder not found: {}", folder_path));
                }

                // Open folder based on OS
                #[cfg(target_os = "macos")]
                {
                    std::process::Command::new("open")
                        .arg(&folder_path)
                        .spawn()
                        .map_err(|e| format!("Failed to open folder: {}", e))?;
                }

                #[cfg(target_os = "windows")]
                {
                    std::process::Command::new("explorer")
                        .arg(&folder_path)
                        .spawn()
                        .map_err(|e| format!("Failed to open folder: {}", e))?;
                }

                #[cfg(target_os = "linux")]
                {
                    std::process::Command::new("xdg-open")
                        .arg(&folder_path)
                        .spawn()
                        .map_err(|e| format!("Failed to open folder: {}", e))?;
                }

                log_info!("Successfully opened folder: {}", folder_path);
                Ok(())
            } else {
                log_warn!("Meeting {} has no folder_path set", meeting_id);
                Err("Recording folder path not available for this meeting".to_string())
            }
        }
        None => {
            log_warn!("Meeting not found: {}", meeting_id);
            Err("Meeting not found".to_string())
        }
    }
}

// Simple test command to check backend connectivity
#[tauri::command]
pub async fn test_backend_connection<R: Runtime>(
    app: AppHandle<R>,
    auth_token: Option<String>,
) -> Result<String, String> {
    log_debug!("Testing backend connection...");

    let client = reqwest::Client::new();
    let server_url = get_server_address(&app).await?;

    log_debug!("Testing connection to: {}", server_url);

    let mut request = client.get(&format!("{}/docs", server_url));

    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    match request.send().await {
        Ok(response) => {
            let status = response.status();
            log_debug!("Backend responded with status: {}", status);
            Ok(format!("Backend is reachable. Status: {}", status))
        }
        Err(e) => {
            let error_msg = format!("Failed to connect to backend: {}", e);
            log_debug!("{}", error_msg);
            Err(error_msg)
        }
    }
}

#[tauri::command]
pub async fn debug_backend_connection<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    log_debug!("=== DEBUG: Testing backend connection ===");

    // Test 1: Check server address from store
    let server_url = match get_server_address(&app).await {
        Ok(url) => {
            log_debug!("✓ Server URL from store: {}", url);
            url
        }
        Err(e) => {
            log_error!("✗ Failed to get server URL: {}", e);
            return Err(format!("Failed to get server URL: {}", e));
        }
    };

    // Test 2: Make a simple HTTP request to the backend
    let client = reqwest::Client::new();
    let test_url = format!("{}/docs", server_url); // Try the docs endpoint which should be public

    log_debug!("Testing connection to: {}", test_url);

    match client.get(&test_url).send().await {
        Ok(response) => {
            let status = response.status();
            log_debug!("✓ Backend responded with status: {}", status);
            Ok(format!(
                "Backend connection successful! Status: {}, URL: {}",
                status, server_url
            ))
        }
        Err(e) => {
            log_error!("✗ Backend connection failed: {}", e);
            Err(format!("Backend connection failed: {}", e))
        }
    }
}

#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    use std::process::Command;

    let result = if cfg!(target_os = "windows") {
        Command::new("cmd").args(&["/C", "start", &url]).output()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(&url).output()
    } else {
        // Linux and other Unix-like systems
        Command::new("xdg-open").arg(&url).output()
    };

    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to open URL: {}", e)),
    }
}

// ===== CUSTOM OPENAI API COMMANDS =====

/// Saves the custom OpenAI configuration
/// This configuration is stored as JSON and includes endpoint, apiKey, model, and optional parameters
#[tauri::command]
pub async fn api_save_custom_openai_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    endpoint: String,
    api_key: Option<String>,
    model: String,
    max_tokens: Option<i32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_save_custom_openai_config called: endpoint='{}', model='{}'",
        &endpoint,
        &model
    );

    // Validate required fields
    if endpoint.trim().is_empty() {
        return Err("Endpoint URL is required".to_string());
    }
    if model.trim().is_empty() {
        return Err("Model name is required".to_string());
    }

    // Validate endpoint URL format
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        return Err("Endpoint must start with http:// or https://".to_string());
    }

    // Validate optional numeric parameters
    if let Some(temp) = temperature {
        if !(0.0..=2.0).contains(&temp) {
            return Err("Temperature must be between 0.0 and 2.0".to_string());
        }
    }
    if let Some(top) = top_p {
        if !(0.0..=1.0).contains(&top) {
            return Err("Top P must be between 0.0 and 1.0".to_string());
        }
    }
    if let Some(tokens) = max_tokens {
        if tokens < 1 {
            return Err("Max tokens must be at least 1".to_string());
        }
    }

    let config = CustomOpenAIConfig {
        endpoint: endpoint.trim().to_string(),
        api_key: api_key.filter(|k| !k.trim().is_empty()),
        model: model.trim().to_string(),
        max_tokens,
        temperature,
        top_p,
    };

    let pool = state.db_manager.pool();

    match SettingsRepository::save_custom_openai_config(pool, &config).await {
        Ok(()) => {
            log_info!(
                "✅ Successfully saved custom OpenAI config for endpoint: {}",
                config.endpoint
            );
            Ok(serde_json::json!({
                "status": "success",
                "message": "Custom OpenAI configuration saved successfully"
            }))
        }
        Err(e) => {
            log_error!("❌ Failed to save custom OpenAI config: {}", e);
            Err(format!("Failed to save custom OpenAI configuration: {}", e))
        }
    }
}

/// Gets the custom OpenAI configuration
#[tauri::command]
pub async fn api_get_custom_openai_config<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CustomOpenAIConfig>, String> {
    log_info!("api_get_custom_openai_config called");

    let pool = state.db_manager.pool();

    match SettingsRepository::get_custom_openai_config(pool).await {
        Ok(config) => {
            if let Some(ref c) = config {
                log_info!(
                    "✅ Found custom OpenAI config: endpoint='{}', model='{}'",
                    c.endpoint,
                    c.model
                );
            } else {
                log_info!("No custom OpenAI config found");
            }
            Ok(config)
        }
        Err(e) => {
            log_error!("❌ Failed to get custom OpenAI config: {}", e);
            Err(format!("Failed to get custom OpenAI configuration: {}", e))
        }
    }
}

/// Tests the connection to a custom OpenAI-compatible endpoint
/// Makes a minimal request to verify the endpoint is reachable and responds correctly
#[tauri::command]
pub async fn api_test_custom_openai_connection<R: Runtime>(
    _app: AppHandle<R>,
    endpoint: String,
    api_key: Option<String>,
    model: String,
) -> Result<serde_json::Value, String> {
    log_info!(
        "api_test_custom_openai_connection called: endpoint='{}', model='{}'",
        &endpoint,
        &model
    );

    // Validate endpoint URL format
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        return Err("Endpoint must start with http:// or https://".to_string());
    }

    // Build the URL - append /chat/completions to the base endpoint
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));

    // Create a minimal test request
    let test_request = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": "Hi"
            }
        ],
        "max_tokens": 5
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let mut request = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&test_request);

    // Add authorization if API key provided
    if let Some(key) = api_key.filter(|k| !k.trim().is_empty()) {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    match request.send().await {
        Ok(response) => {
            let status = response.status();
            let response_text = response.text().await.unwrap_or_default();

            if status.is_success() {
                // Parse response as JSON to verify it's a valid OpenAI-compatible response
                match serde_json::from_str::<serde_json::Value>(&response_text) {
                    Ok(json) => {
                        // Verify the response has the expected OpenAI structure
                        if let Some(choices) = json.get("choices") {
                            if let Some(choices_array) = choices.as_array() {
                                if !choices_array.is_empty() {
                                    // Verify the first choice has the required message structure
                                    if let Some(first_choice) = choices_array.get(0) {
                                        // Check if message.content field exists (can be empty string)
                                        let has_message_structure = first_choice
                                            .get("message")
                                            .and_then(|m| {
                                                m.get("content")
                                                    .or_else(|| m.get("reasoning_content"))
                                            })
                                            .is_some();

                                        if has_message_structure {
                                            log_info!("✅ Custom OpenAI connection test successful - response validated");
                                            return Ok(serde_json::json!({
                                                "status": "success",
                                                "message": "Connection successful and response validated",
                                                "http_status": status.as_u16()
                                            }));
                                        }
                                    }
                                }
                            }
                        }

                        // Response was 200 but doesn't match OpenAI format
                        log_warn!(
                            "⚠️ Endpoint returned 200 but response doesn't match OpenAI format: {}",
                            response_text
                        );
                        Err("Endpoint is reachable but doesn't appear to be OpenAI-compatible. Response is missing 'choices' array or 'message.content' / 'message.reasoning_content' field.".to_string())
                    }
                    Err(e) => {
                        log_warn!(
                            "⚠️ Endpoint returned 200 but response is not valid JSON: {}",
                            e
                        );
                        Err(format!(
                            "Endpoint is reachable but returned invalid JSON: {}. Response: {}",
                            e, response_text
                        ))
                    }
                }
            } else {
                log_warn!(
                    "⚠️ Custom OpenAI connection test failed with status {}: {}",
                    status,
                    response_text
                );
                Err(format!(
                    "Connection failed with status {}: {}",
                    status, response_text
                ))
            }
        }
        Err(e) => {
            log_error!("❌ Custom OpenAI connection test failed: {}", e);
            if e.is_timeout() {
                Err("Connection timed out. Please check the endpoint URL.".to_string())
            } else if e.is_connect() {
                Err("Could not connect to endpoint. Please verify the URL is correct and the server is running.".to_string())
            } else {
                Err(format!("Connection failed: {}", e))
            }
        }
    }
}
