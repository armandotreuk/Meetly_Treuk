use chrono::{DateTime, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio_util::sync::CancellationToken;
use tracing::info;
use uuid::Uuid;

use crate::retrieval::worker::RetrievalLifecycle;
use crate::retrieval::{
    agent::DeepProgressCallback, hydrate_context, PersistedRetrievalScope, RetrievalLimits,
    RetrievalPurpose, RetrievalRequest, RetrievalService,
};
use crate::{
    database::repositories::{
        chat::{
            ChatConversation, ChatMessageRow, ChatRepository, ChatScope, ChatScopeData,
            ChatScopeKind,
        },
        folder::FolderRepository,
        fts::{FtsRepository, MatchMode},
        setting::SettingsRepository,
    },
    export::context::{build_meeting_context_markdown, lexical_evidence_id},
    export::{build_context_markdown, build_context_markdown_with_limit},
    state::AppState,
    summary::llm_client::{generate_summary, generate_summary_stream, LLMProvider},
};

#[tauri::command]
pub async fn api_chat_create_conversation(
    state: tauri::State<'_, AppState>,
    meeting_id: Option<String>,
    title: Option<String>,
) -> Result<String, String> {
    let origin = if meeting_id.is_some() {
        "meeting"
    } else {
        "global"
    };
    ChatRepository::create_conversation(
        state.db_manager.pool(),
        meeting_id.as_deref(),
        title.as_deref(),
        origin,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_chat_get_force_lexical_retrieval(
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    SettingsRepository::get_force_lexical_retrieval(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_chat_set_force_lexical_retrieval(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    SettingsRepository::set_force_lexical_retrieval(state.db_manager.pool(), enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_chat_get_conversation(
    state: tauri::State<'_, AppState>,
    meeting_id: Option<String>,
) -> Result<Option<ChatConversation>, String> {
    ChatRepository::get_latest_conversation(state.db_manager.pool(), meeting_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_chat_get_or_create_scoped_conversation(
    state: tauri::State<'_, AppState>,
    scope: ChatScope,
    title: Option<String>,
) -> Result<ChatConversation, String> {
    scope.validate().map_err(|e| e.to_string())?;
    if scope.kind == ChatScopeKind::SearchSnapshot {
        validate_search_snapshot_membership(state.db_manager.pool(), &scope).await?;
    }
    ChatRepository::get_or_create_scoped_conversation(
        state.db_manager.pool(),
        &scope,
        title.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Enforces snapshot membership on creation only: when the exact-scope thread
/// already exists (resume), members deleted since the snapshot was frozen are
/// tolerated so the thread stays reachable; retrieval simply skips them.
async fn validate_search_snapshot_membership(
    pool: &SqlitePool,
    scope: &ChatScope,
) -> Result<(), String> {
    if ChatRepository::get_latest_conversation_for_scope(pool, scope)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let result_ids = &scope
        .data
        .as_ref()
        .expect("validated search snapshot scope")
        .result_ids;
    let mut query = QueryBuilder::<Sqlite>::new("SELECT count(*) FROM meetings WHERE id IN (");
    let mut ids = query.separated(", ");
    for id in result_ids {
        ids.push_bind(id);
    }
    drop(ids);
    query.push(")");
    let count: i64 = query
        .build_query_scalar()
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    if count as usize != result_ids.len() {
        return Err("Search snapshot contains unknown result identifiers".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn api_chat_get_messages(
    state: tauri::State<'_, AppState>,
    conversation_id: String,
) -> Result<Vec<ChatMessageRow>, String> {
    ChatRepository::get_messages(state.db_manager.pool(), &conversation_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_chat_save_message(
    state: tauri::State<'_, AppState>,
    conversation_id: String,
    role: String,
    content: String,
    sources: Option<Vec<ChatSource>>,
    is_error: bool,
) -> Result<(), String> {
    let sources_json = sources
        .map(|sources| serde_json::to_string(&sources))
        .transpose()
        .map_err(|e| e.to_string())?;
    ChatRepository::save_message(
        state.db_manager.pool(),
        &conversation_id,
        &role,
        &content,
        sources_json.as_deref(),
        is_error,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_chat_clear_conversation(
    state: tauri::State<'_, AppState>,
    conversation_id: String,
) -> Result<(), String> {
    ChatRepository::delete_conversation(state.db_manager.pool(), &conversation_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_chat_promote_live_recording(
    state: tauri::State<'_, AppState>,
    live_scope_key: String,
    meeting_id: String,
) -> Result<Option<String>, String> {
    ChatRepository::promote_live_recording(state.db_manager.pool(), &live_scope_key, &meeting_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_chat_discard_live_recording(
    state: tauri::State<'_, AppState>,
    live_scope_key: String,
) -> Result<(), String> {
    ChatRepository::discard_live_recording(state.db_manager.pool(), &live_scope_key)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatResponse {
    pub answer: String,
    pub sources: Vec<ChatSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSource {
    #[serde(rename = "meetingId")]
    pub meeting_id: String,
    #[serde(rename = "meetingTitle")]
    pub meeting_title: String,
    #[serde(rename = "chunkType")]
    pub chunk_type: String,
    pub snippet: String,
    #[serde(rename = "folderName")]
    pub folder_name: String,
    #[serde(rename = "sourceKind", skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRetrievalMode {
    Fast,
    Deep,
}

impl Default for ChatRetrievalMode {
    fn default() -> Self {
        Self::Fast
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPreparationStage {
    InitialRetrieval,
    PlannerRound,
    AdditionalSearch,
    AnswerGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPreparationProgressPayload {
    #[serde(rename = "streamId")]
    pub stream_id: String,
    pub stage: ChatPreparationStage,
    pub completed: usize,
    pub total: usize,
}

pub const SYSTEM_PROMPT: &str = "You are a helpful meeting assistant. Answer the user's question based on the meeting context provided below. The application temporal reference is authoritative for current-date and latest-saved-meeting questions. If the context doesn't contain enough information, say so. If transcript coverage is marked partial, disclose that limitation in your answer. Be concise and cite specific meetings when relevant. Format your response in clear paragraphs.";

const QUERY_REWRITE_SYSTEM_PROMPT: &str = "You are a search query rewriter. Given a follow-up question and conversation history, rewrite it into a single standalone search query that would find relevant information in a meeting transcript database. Return ONLY the search query, nothing else. Keep it under 10 words. Do not add quotes or explanation.";

// ponytail: snapshot rehydration total cap mirrors the 100-meeting snapshot ceiling
// (repositories/chat.rs MAX_SEARCH_SNAPSHOT_RESULTS); per-meeting chunks are capped
// by chunk_limit in prepare_chat_inputs_for_scope. Upgrade: relevance-ranked chunk
// selection instead of the deterministic chunk_id order.
const SNAPSHOT_REHYDRATION_CHUNK_CAP: u32 = 100;
const CHAT_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Inputs shared by both the single-shot and the streaming chat commands.
pub struct ChatInputs {
    pub sources: Vec<ChatSource>,
    pub provider: LLMProvider,
    pub model_name: String,
    pub api_key: String,
    pub ollama_endpoint: Option<String>,
    pub custom_openai_endpoint: Option<String>,
    pub custom_openai_max_tokens: Option<u32>,
    pub custom_openai_temperature: Option<f32>,
    pub custom_openai_top_p: Option<f32>,
    pub app_data_dir: Option<PathBuf>,
    pub user_prompt: String,
    pub retrieval_diagnostic: RetrievalPreparationDiagnostic,
    pub retrieval_mode: ChatRetrievalMode,
    /// Provider round-trips made during preparation (follow-up query rewrite
    /// plus Deep planner calls), excluding the final answer generation.
    pub provider_round_trips: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalPreparationDiagnostic {
    Hybrid,
    ForcedLexical,
    SemanticFallback,
    LifecycleUnavailable,
}

#[derive(Clone)]
enum ChatRetrievalScope {
    All,
    Meeting(String),
    Folder(String),
    SearchSnapshot(Vec<String>),
    LiveRecording(String),
}

struct LiveTranscriptAuthorization {
    active_scope_key: Option<String>,
    consent: bool,
}

#[derive(sqlx::FromRow)]
struct LatestSavedMeeting {
    title: String,
    saved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ChatRequestSurface {
    Chat,
    Sidebar,
}

struct ChatRequestRegistry {
    active: HashMap<ChatRequestSurface, String>,
    requests: HashMap<(ChatRequestSurface, String), ChatRequestToken>,
}

type ChatRequestToken = Arc<CancellationToken>;

#[derive(Clone)]
pub struct ChatRequestState(Arc<Mutex<ChatRequestRegistry>>);

pub type ChatStreamState = ChatRequestState;

impl ChatRequestState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(ChatRequestRegistry {
            active: HashMap::new(),
            requests: HashMap::new(),
        })))
    }

    pub(crate) fn claim_request(
        &self,
        surface: ChatRequestSurface,
        request_id: &str,
    ) -> ChatRequestToken {
        let token = Arc::new(CancellationToken::new());
        let mut registry = self.0.lock().unwrap();
        if let Some(previous_id) = registry.active.insert(surface, request_id.to_string()) {
            if let Some(previous) = registry.requests.remove(&(surface, previous_id)) {
                previous.cancel();
            }
        }
        registry
            .requests
            .insert((surface, request_id.to_string()), token.clone());
        token
    }

    pub(crate) fn is_owner(
        &self,
        surface: ChatRequestSurface,
        request_id: &str,
        token: &ChatRequestToken,
    ) -> bool {
        let registry = self.0.lock().unwrap();
        registry.active.get(&surface).is_some_and(|active_id| {
            active_id == request_id
                && registry
                    .requests
                    .get(&(surface, active_id.clone()))
                    .is_some_and(|current| {
                        Arc::ptr_eq(current, token)
                            && !current.is_cancelled()
                            && !token.is_cancelled()
                    })
        })
    }

    pub(crate) fn clear_if_owner(
        &self,
        surface: ChatRequestSurface,
        request_id: &str,
        token: &ChatRequestToken,
    ) -> bool {
        let mut registry = self.0.lock().unwrap();
        if registry.active.get(&surface).map(String::as_str) != Some(request_id) {
            return false;
        }
        let key = (surface, request_id.to_string());
        if !registry
            .requests
            .get(&key)
            .is_some_and(|current| Arc::ptr_eq(current, token))
        {
            return false;
        }
        registry.active.remove(&surface);
        registry.requests.remove(&key);
        true
    }

    pub(crate) fn cancel_request(
        &self,
        surface: ChatRequestSurface,
        request_id: Option<&str>,
    ) -> bool {
        let mut registry = self.0.lock().unwrap();
        let Some(active_id) = registry.active.get(&surface).cloned() else {
            return false;
        };
        if request_id.is_some_and(|request_id| request_id != active_id) {
            return false;
        }
        registry.active.remove(&surface);
        if let Some(token) = registry.requests.remove(&(surface, active_id)) {
            token.cancel();
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    fn request_count(&self) -> usize {
        self.0.lock().unwrap().requests.len()
    }

    fn publish_if_owner<F: FnOnce(&str, serde_json::Value)>(
        &self,
        surface: ChatRequestSurface,
        request_id: &str,
        token: &ChatRequestToken,
        event: &str,
        payload: serde_json::Value,
        clear: bool,
        emit: F,
    ) -> bool {
        let mut registry = self.0.lock().unwrap();
        let key = (surface, request_id.to_string());
        if token.is_cancelled()
            || registry.active.get(&surface).map(String::as_str) != Some(request_id)
            || registry.requests.get(&key).map_or(true, |current| {
                !Arc::ptr_eq(current, token) || current.is_cancelled()
            })
        {
            return false;
        }
        if clear {
            registry.active.remove(&surface);
            registry.requests.remove(&key);
        }
        emit(event, payload);
        true
    }
}

/// Performs FTS search, source extraction, prompt building, and config/API-key
/// resolution so the two chat commands share exactly one setup path.
pub async fn prepare_chat_inputs_lexical_only(
    pool: &SqlitePool,
    app_data_dir: Option<PathBuf>,
    client: &reqwest::Client,
    query: &str,
    history: Option<&Vec<ChatMessage>>,
    meeting_id: Option<String>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<ChatInputs, String> {
    let retrieval_scope = meeting_id
        .map(ChatRetrievalScope::Meeting)
        .unwrap_or(ChatRetrievalScope::All);
    prepare_chat_inputs_for_scope(
        pool,
        app_data_dir,
        client,
        query,
        history,
        retrieval_scope,
        None,
        None,
        cancellation_token,
        Some(ChatRetrievalMode::Fast),
        None,
    )
    .await
}

pub async fn prepare_chat_inputs_with_lifecycle(
    pool: &SqlitePool,
    app_data_dir: Option<PathBuf>,
    client: &reqwest::Client,
    query: &str,
    history: Option<&Vec<ChatMessage>>,
    meeting_id: Option<String>,
    lifecycle: RetrievalLifecycle,
    cancellation_token: Option<&CancellationToken>,
    retrieval_mode: Option<ChatRetrievalMode>,
    deep_progress: Option<DeepProgressCallback<'_>>,
) -> Result<ChatInputs, String> {
    let retrieval_scope = meeting_id
        .map(ChatRetrievalScope::Meeting)
        .unwrap_or(ChatRetrievalScope::All);
    prepare_chat_inputs_for_scope(
        pool,
        app_data_dir,
        client,
        query,
        history,
        retrieval_scope,
        None,
        Some(lifecycle),
        cancellation_token,
        retrieval_mode,
        deep_progress,
    )
    .await
}

fn authorize_live_transcript(
    retrieval_scope: &ChatRetrievalScope,
    authorization: Option<&LiveTranscriptAuthorization>,
    provider: &str,
) -> Result<(), String> {
    let ChatRetrievalScope::LiveRecording(scope_key) = retrieval_scope else {
        return Ok(());
    };
    let authorization = authorization
        .ok_or_else(|| "Live transcript access requires recording authorization".to_string())?;
    if authorization.active_scope_key.as_deref() != Some(scope_key) {
        return Err("Live chat scope does not match the active recording".to_string());
    }
    if !is_local_chat_provider(provider) && !authorization.consent {
        return Err("Live transcript consent is required for the selected provider".to_string());
    }
    Ok(())
}

/// Re-checks the active native recording key at the transcript-read point, because the
/// recording may have stopped and restarted (new key) during the query rewrite above.
fn ensure_live_scope_matches_active_recording(scope_key: &str) -> Result<(), String> {
    if crate::audio::recording_commands::active_live_transcript_scope_key().as_deref()
        != Some(scope_key)
    {
        return Err("Live chat scope does not match the active recording".to_string());
    }
    Ok(())
}

fn is_local_chat_provider(provider: &str) -> bool {
    matches!(
        provider.to_ascii_lowercase().as_str(),
        "ollama" | "builtin-ai" | "local-llama" | "localllama"
    )
}

pub async fn prepare_scoped_chat_inputs(
    pool: &SqlitePool,
    app_data_dir: Option<PathBuf>,
    client: &reqwest::Client,
    query: &str,
    history: Option<&Vec<ChatMessage>>,
    conversation_id: &str,
    live_transcript_consent: bool,
    cancellation_token: Option<&CancellationToken>,
) -> Result<ChatInputs, String> {
    prepare_scoped_chat_inputs_with_authorization(
        pool,
        app_data_dir,
        client,
        query,
        history,
        conversation_id,
        LiveTranscriptAuthorization {
            active_scope_key: crate::audio::recording_commands::active_live_transcript_scope_key(),
            consent: live_transcript_consent,
        },
        cancellation_token,
        None,
        Some(ChatRetrievalMode::Fast),
        None,
    )
    .await
}

async fn prepare_scoped_chat_inputs_with_authorization(
    pool: &SqlitePool,
    app_data_dir: Option<PathBuf>,
    client: &reqwest::Client,
    query: &str,
    history: Option<&Vec<ChatMessage>>,
    conversation_id: &str,
    live_authorization: LiveTranscriptAuthorization,
    cancellation_token: Option<&CancellationToken>,
    lifecycle: Option<RetrievalLifecycle>,
    retrieval_mode: Option<ChatRetrievalMode>,
    deep_progress: Option<DeepProgressCallback<'_>>,
) -> Result<ChatInputs, String> {
    let conversation = ChatRepository::get_conversation(pool, conversation_id)
        .await
        .map_err(|e| format!("Failed to get conversation: {}", e))?
        .ok_or_else(|| "Chat conversation not found".to_string())?;
    ensure_not_cancelled(cancellation_token)?;
    prepare_chat_inputs_for_scope(
        pool,
        app_data_dir,
        client,
        query,
        history,
        retrieval_scope_from_conversation(&conversation)?,
        Some(live_authorization),
        lifecycle,
        cancellation_token,
        retrieval_mode,
        deep_progress,
    )
    .await
}

async fn prepare_chat_inputs_for_scope(
    pool: &SqlitePool,
    app_data_dir: Option<PathBuf>,
    client: &reqwest::Client,
    query: &str,
    history: Option<&Vec<ChatMessage>>,
    retrieval_scope: ChatRetrievalScope,
    live_authorization: Option<LiveTranscriptAuthorization>,
    lifecycle: Option<RetrievalLifecycle>,
    cancellation_token: Option<&CancellationToken>,
    retrieval_mode: Option<ChatRetrievalMode>,
    deep_progress: Option<DeepProgressCallback<'_>>,
) -> Result<ChatInputs, String> {
    let requested_retrieval_mode = retrieval_mode.unwrap_or_default();
    let model_config = SettingsRepository::get_chat_model_config(pool)
        .await
        .map_err(|e| format!("Failed to get model config: {}", e))?
        .ok_or_else(|| "No model configured. Please set a model in Settings.".to_string())?;
    ensure_not_cancelled(cancellation_token)?;

    let (model_provider_str, model_name, chat_ollama_endpoint) =
        SettingsRepository::resolve_chat_config(&model_config);

    authorize_live_transcript(
        &retrieval_scope,
        live_authorization.as_ref(),
        &model_provider_str,
    )?;

    let provider = LLMProvider::from_str(&model_provider_str)?;

    let (
        custom_openai_endpoint,
        custom_openai_api_key,
        custom_openai_max_tokens,
        custom_openai_temperature,
        custom_openai_top_p,
    ) = if provider == LLMProvider::CustomOpenAI {
        let config = SettingsRepository::get_custom_openai_config(pool).await;
        ensure_not_cancelled(cancellation_token)?;
        match config {
            Ok(Some(config)) => (
                Some(config.endpoint),
                config.api_key.unwrap_or_default(),
                config.max_tokens.map(|t| t as u32),
                config.temperature,
                config.top_p,
            ),
            _ => {
                return Err("Custom OpenAI provider selected but no configuration found".to_string())
            }
        }
    } else {
        (None, String::new(), None, None, None)
    };

    let api_key = if provider == LLMProvider::Ollama || provider == LLMProvider::BuiltInAI {
        String::new()
    } else if provider == LLMProvider::CustomOpenAI {
        custom_openai_api_key
    } else {
        // Get API key from the Setting struct's per-provider fields (global keys shared across features)
        let key = match model_provider_str.as_str() {
            "openai" => model_config.openai_api_key.as_deref(),
            "claude" | "anthropic" => model_config.anthropic_api_key.as_deref(),
            "groq" => model_config.groq_api_key.as_deref(),
            "openrouter" => model_config.open_router_api_key.as_deref(),
            "ollama" => model_config.ollama_api_key.as_deref(),
            _ => None,
        };
        match key {
            Some(k) if !k.is_empty() => k.to_string(),
            _ => {
                return Err(format!(
                    "API key not found for provider '{}'. Go to Settings → Chat tab and enter your API key for {}.",
                    model_provider_str, model_provider_str
                ));
            }
        }
    };

    let ollama_endpoint = if provider == LLMProvider::Ollama {
        chat_ollama_endpoint
    } else {
        None
    };

    let mut provider_round_trips = 0usize;
    let search_query = if should_rewrite_query(history, query) {
        provider_round_trips += 1;
        let rewrite_prompt = build_rewrite_prompt(history.unwrap(), query);
        // ponytail: 15s cap; retry with a shorter prompt or skip rewrite entirely on timeout.
        let rewritten = tokio::time::timeout(
            Duration::from_secs(15),
            generate_summary(
                client,
                &provider,
                &model_name,
                &api_key,
                QUERY_REWRITE_SYSTEM_PROMPT,
                &rewrite_prompt,
                ollama_endpoint.as_deref(),
                custom_openai_endpoint.as_deref(),
                custom_openai_max_tokens,
                custom_openai_temperature,
                custom_openai_top_p,
                app_data_dir.as_ref(),
                cancellation_token,
            ),
        )
        .await;
        ensure_not_cancelled(cancellation_token)?;
        match rewritten {
            Ok(Ok(rewritten)) if !rewritten.trim().is_empty() => rewritten.trim().to_string(),
            _ => query.to_string(),
        }
    } else {
        query.to_string()
    };

    // ponytail: provider class approximates model capacity until configured model context windows are available.
    let (chunk_limit, max_context_chars) = match &provider {
        LLMProvider::Ollama | LLMProvider::BuiltInAI | LLMProvider::CustomOpenAI => (10, 64_000),
        _ => (30, 100_000),
    };
    let today_date = if requests_todays_meetings(query) || requests_todays_meetings(&search_query) {
        Some(Local::now().date_naive())
    } else {
        None
    };
    let today_meeting_ids = if let Some(local_date) = today_date {
        Some(meeting_ids_for_local_date(pool, &retrieval_scope, local_date).await?)
    } else {
        None
    };
    let meeting_list_context = if requests_meeting_list(query) {
        Some(format_meeting_list_context(
            &meeting_titles_for_scope(pool, &retrieval_scope, query, today_date).await?,
        ))
    } else {
        None
    };
    let mut temporal_context = temporal_context_for_scope(pool, &retrieval_scope).await?;
    if let Some(meeting_ids) = &today_meeting_ids {
        temporal_context.push_str(&format!(
            "Meeting context is filtered to today's meetings in this scope ({} found).\n",
            meeting_ids.len()
        ));
    }
    let persisted_context_budget =
        context_budget_for_prompt(query, &search_query, &temporal_context, max_context_chars);
    let force_lexical = SettingsRepository::get_force_lexical_retrieval(pool)
        .await
        .map_err(|e| format!("Failed to get retrieval kill switch: {}", e))?;
    let mut retrieval_diagnostic = if force_lexical {
        RetrievalPreparationDiagnostic::ForcedLexical
    } else if lifecycle.is_none() {
        RetrievalPreparationDiagnostic::LifecycleUnavailable
    } else {
        RetrievalPreparationDiagnostic::Hybrid
    };
    let retrieval_mode = if force_lexical
        || lifecycle.is_none()
        || matches!(&retrieval_scope, ChatRetrievalScope::LiveRecording(_))
    {
        ChatRetrievalMode::Fast
    } else {
        requested_retrieval_mode
    };
    let (context, sources) = match retrieval_scope {
        ChatRetrievalScope::LiveRecording(scope_key) => {
            ensure_live_scope_matches_active_recording(&scope_key)?;
            let snapshot = crate::audio::recording_commands::get_transcript_history().await?;
            ensure_not_cancelled(cancellation_token)?;
            live_snapshot_context(&snapshot, &scope_key, max_context_chars)
        }
        scope => {
            if let Some(context) = meeting_list_context {
                (context, Vec::new())
            } else if matches!(scope, ChatRetrievalScope::Meeting(_)) && today_meeting_ids.is_none()
            {
                let ChatRetrievalScope::Meeting(meeting_id) = scope else {
                    unreachable!()
                };
                let meeting =
                    resolve_meeting_context(pool, &meeting_id, &search_query, query, chunk_limit)
                        .await?;
                ensure_not_cancelled(cancellation_token)?;
                let built = build_meeting_context_markdown(
                    &meeting.meeting_id,
                    &truncate_meeting_title(&meeting.meeting_title),
                    meeting.summary.as_deref(),
                    meeting.notes.as_deref(),
                    &meeting.transcripts,
                    meeting.total_transcript_segments,
                    persisted_context_budget,
                );
                let retained = built
                    .retained_transcript_ids
                    .into_iter()
                    .collect::<HashSet<_>>();
                let sources = meeting
                    .transcripts
                    .iter()
                    .filter(|transcript| retained.contains(&transcript.chunk_id))
                    .map(chat_source_from_result)
                    .collect();
                (built.markdown, sources)
            } else if !force_lexical
                && lifecycle.is_some()
                && matches!(
                    scope,
                    ChatRetrievalScope::All | ChatRetrievalScope::Folder(_)
                )
            {
                let lifecycle =
                    lifecycle.ok_or_else(|| "Retrieval lifecycle unavailable".to_string())?;
                // Task 4.2 keeps the Deep lifecycle scoped to the supported
                // persisted All/Folder paths; today's derived allow-list,
                // snapshot, saved-meeting, and live scopes stay Fast until
                // Tasks 4.3/4.4 extend them.
                let deep_eligible =
                    deep_preparation_eligible(retrieval_mode, today_meeting_ids.as_ref());
                let persisted_scope = match &scope {
                    ChatRetrievalScope::All => PersistedRetrievalScope::All,
                    ChatRetrievalScope::Folder(folder_id) => {
                        PersistedRetrievalScope::Folder(folder_id.clone())
                    }
                    _ => unreachable!(),
                };
                let persisted_scope = if deep_eligible {
                    persisted_scope
                } else {
                    today_meeting_ids
                        .as_ref()
                        .map(|ids| PersistedRetrievalScope::AllowedMeetingIds(ids.clone()))
                        .unwrap_or(persisted_scope)
                };
                if deep_eligible {
                    let request_cancellation = cancellation_token.cloned().unwrap_or_default();
                    let planner = crate::retrieval::agent::SharedClientPlanner {
                        client: client.clone(),
                        provider: provider.clone(),
                        model_name: model_name.clone(),
                        api_key: api_key.clone(),
                        ollama_endpoint: ollama_endpoint.clone(),
                        custom_openai_endpoint: custom_openai_endpoint.clone(),
                        app_data_dir: app_data_dir.clone(),
                    };
                    let deep = crate::retrieval::agent::run_deep_preparation(
                        crate::retrieval::agent::DeepPreparationInput {
                            pool,
                            lifecycle,
                            original_query: query,
                            effective_query: &search_query,
                            scope: persisted_scope,
                            limits: RetrievalLimits {
                                lexical_per_variant: chunk_limit as usize,
                                vector_per_variant: chunk_limit as usize,
                            },
                            core_language: crate::retrieval::service::CoreTermLanguage::Unknown,
                            context_budget: persisted_context_budget,
                            cancellation: cancellation_token.unwrap_or(&request_cancellation),
                            progress: deep_progress,
                            planner: &planner,
                            bounds: crate::retrieval::agent::DeepBounds::production(),
                        },
                    )
                    .await
                    .map_err(|error| match error {
                        crate::retrieval::agent::DeepPreparationError::Cancelled => {
                            "Chat preparation was cancelled".to_string()
                        }
                        crate::retrieval::agent::DeepPreparationError::BudgetExhausted => format!(
                            "Deep preparation exceeded the {} second budget",
                            crate::retrieval::agent::DEEP_PREPARATION_BUDGET.as_secs()
                        ),
                        crate::retrieval::agent::DeepPreparationError::InitialRetrieval(error) => {
                            format!("Retrieval failed: {}", error)
                        }
                        crate::retrieval::agent::DeepPreparationError::FinalValidation(error) => {
                            format!("Deep final validation failed: {}", error)
                        }
                    })?;
                    provider_round_trips += deep.planner_round_trips;
                    log::info!(
                        "Chat Deep preparation: additional_rounds={} planner_calls={} round_trips_before_generation={}",
                        deep.additional_rounds,
                        deep.planner_round_trips,
                        provider_round_trips
                    );
                    if deep.semantic_fallback.is_some() {
                        retrieval_diagnostic = RetrievalPreparationDiagnostic::SemanticFallback;
                    }
                    let sources = deep
                        .hydrated
                        .sources
                        .iter()
                        .map(chat_source_from_hydrated)
                        .collect();
                    (deep.hydrated.markdown, sources)
                } else {
                    let ranked = RetrievalService::new(lifecycle)
                        .retrieve_ranked(
                            pool,
                            RetrievalRequest {
                                original_query: query.to_string(),
                                rewritten_query: Some(search_query.clone()),
                                scope: persisted_scope,
                                purpose: RetrievalPurpose::Chat,
                                limits: RetrievalLimits {
                                    lexical_per_variant: chunk_limit as usize,
                                    vector_per_variant: chunk_limit as usize,
                                },
                                core_language: crate::retrieval::service::CoreTermLanguage::Unknown,
                                cancellation: cancellation_token.cloned(),
                            },
                        )
                        .await
                        .map_err(|e| format!("Retrieval failed: {}", e))?;
                    if ranked.semantic_fallback.is_some() {
                        retrieval_diagnostic = RetrievalPreparationDiagnostic::SemanticFallback;
                    }
                    let hydrated = hydrate_context(
                        pool,
                        &ranked,
                        persisted_context_budget,
                        cancellation_token,
                    )
                    .await
                    .map_err(|e| format!("Retrieval hydration failed: {}", e))?;
                    let sources = hydrated
                        .sources
                        .iter()
                        .map(chat_source_from_hydrated)
                        .collect();
                    (hydrated.markdown, sources)
                }
            } else {
                log::info!("Chat retrieval preparation mode: {retrieval_diagnostic:?}");
                let results = resolve_scope_results(
                    pool,
                    &search_query,
                    query,
                    chunk_limit,
                    scope,
                    today_meeting_ids.as_deref(),
                )
                .await?;
                ensure_not_cancelled(cancellation_token)?;
                let built = build_context_markdown_with_limit(&results, persisted_context_budget);
                let retained = built
                    .retained_evidence_ids
                    .into_iter()
                    .collect::<HashSet<_>>();
                let sources = results
                    .iter()
                    .filter(|result| retained.contains(&lexical_evidence_id(result)))
                    .map(chat_source_from_result)
                    .collect();
                (built.markdown, sources)
            }
        }
    };

    let user_prompt = assemble_prompt(
        &context,
        history.map_or(&[], Vec::as_slice),
        query,
        &search_query,
        &temporal_context,
        max_context_chars,
    );

    Ok(ChatInputs {
        sources,
        provider,
        model_name,
        api_key,
        ollama_endpoint,
        custom_openai_endpoint,
        custom_openai_max_tokens,
        custom_openai_temperature,
        custom_openai_top_p,
        app_data_dir,
        user_prompt,
        retrieval_diagnostic,
        retrieval_mode,
        provider_round_trips,
    })
}

async fn temporal_context_for_scope(
    pool: &SqlitePool,
    scope: &ChatRetrievalScope,
) -> Result<String, String> {
    let latest = match scope {
        ChatRetrievalScope::All => {
            sqlx::query_as::<_, LatestSavedMeeting>(
                "SELECT title, saved_at FROM meetings ORDER BY saved_at DESC, id DESC LIMIT 1",
            )
            .fetch_optional(pool)
            .await
        }
        ChatRetrievalScope::Meeting(meeting_id) => {
            sqlx::query_as::<_, LatestSavedMeeting>(
                "SELECT title, saved_at FROM meetings WHERE id = ? LIMIT 1",
            )
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
        }
        ChatRetrievalScope::Folder(folder_id) => {
            latest_saved_meeting_in_folder(pool, folder_id).await
        }
        ChatRetrievalScope::SearchSnapshot(meeting_ids) => {
            latest_saved_meeting_in_ids(pool, "id", meeting_ids).await
        }
        ChatRetrievalScope::LiveRecording(_) => Ok(None),
    }
    .map_err(|error| format!("Failed to resolve temporal meeting context: {}", error))?;

    let latest = if matches!(scope, ChatRetrievalScope::Meeting(_)) {
        latest.map(|mut meeting| {
            meeting.title = truncate_meeting_title(&meeting.title);
            meeting
        })
    } else {
        latest
    };
    Ok(format_temporal_context(Local::now(), latest.as_ref()))
}

fn truncate_meeting_title(title: &str) -> String {
    const MAX_TITLE_CHARS: usize = 512;
    if title.chars().count() <= MAX_TITLE_CHARS {
        title.to_string()
    } else {
        format!(
            "{}… [truncated]",
            title
                .chars()
                .take(MAX_TITLE_CHARS - "… [truncated]".chars().count())
                .collect::<String>()
        )
    }
}

async fn latest_saved_meeting_in_ids(
    pool: &SqlitePool,
    column: &str,
    ids: &[String],
) -> Result<Option<LatestSavedMeeting>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(None);
    }
    let mut query = QueryBuilder::<Sqlite>::new("SELECT title, saved_at FROM meetings WHERE ");
    query.push(column);
    query.push(" IN (");
    let mut values = query.separated(", ");
    for id in ids {
        values.push_bind(id);
    }
    drop(values);
    query.push(") ORDER BY saved_at DESC, id DESC LIMIT 1");
    query.build_query_as().fetch_optional(pool).await
}

async fn latest_saved_meeting_in_folder(
    pool: &SqlitePool,
    folder_id: &str,
) -> Result<Option<LatestSavedMeeting>, sqlx::Error> {
    sqlx::query_as(
        r#"
        WITH RECURSIVE folder_scope(id) AS (
            SELECT id FROM meeting_folders WHERE id = ?
            UNION ALL
            SELECT f.id FROM meeting_folders f JOIN folder_scope s ON f.parent_id = s.id
        )
        SELECT m.title, m.saved_at
        FROM meetings m
        WHERE m.folder_id IN (SELECT id FROM folder_scope)
        ORDER BY m.saved_at DESC, m.id DESC
        LIMIT 1
        "#,
    )
    .bind(folder_id)
    .fetch_optional(pool)
    .await
}

fn format_temporal_context(now: DateTime<Local>, latest: Option<&LatestSavedMeeting>) -> String {
    let mut context = format!(
        "Application temporal reference (authoritative):\nCurrent local date: {}\n",
        now.format("%Y-%m-%d")
    );
    if let Some(latest) = latest {
        context.push_str(&format!(
            "Most recently saved/imported meeting in this scope: {} (saved/imported local time: {}).\n",
            latest.title,
            latest.saved_at.with_timezone(&Local).format("%Y-%m-%d %H:%M")
        ));
    }
    context
}

fn requests_todays_meetings(query: &str) -> bool {
    let terms = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    terms.iter().any(|term| term == "today")
        && terms
            .iter()
            .any(|term| term == "meeting" || term == "meetings")
}

fn requests_meeting_list(query: &str) -> bool {
    let normalized = query.to_lowercase();
    let has_meeting_term = normalized.contains("meeting") || normalized.contains("reuni");
    let terms = normalized
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    let has_list_verb = terms.iter().any(|term| {
        matches!(
            *term,
            "list" | "show" | "listar" | "liste" | "mostre" | "quais"
        )
    });
    let has_content_predicate = terms.iter().any(|term| {
        matches!(
            *term,
            "about"
                | "where"
                | "with"
                | "contains"
                | "contain"
                | "discussed"
                | "discuss"
                | "mentioned"
                | "mention"
                | "decided"
                | "decision"
                | "sobre"
                | "onde"
                | "com"
                | "contem"
                | "discutiu"
                | "discutiram"
                | "mencionou"
                | "mencionaram"
                | "decidiu"
                | "decidiram"
        ) || term.ends_with("ed")
            || term.ends_with("ing")
            || term.ends_with("tion")
            || term.ends_with("sion")
    });
    has_meeting_term && has_list_verb && !has_content_predicate
}

async fn meeting_titles_for_scope(
    pool: &SqlitePool,
    scope: &ChatRetrievalScope,
    query_text: &str,
    local_date: Option<NaiveDate>,
) -> Result<Vec<String>, String> {
    if matches!(scope, ChatRetrievalScope::LiveRecording(_)) {
        return Ok(Vec::new());
    }

    let named_folder_id = if matches!(scope, ChatRetrievalScope::All) {
        let normalized_query = query_text.to_lowercase();
        let folder = FolderRepository::get_all(pool)
            .await
            .map_err(|error| format!("Failed to resolve named folder: {}", error))?
            .into_iter()
            .filter(|folder| normalized_query.contains(&folder.name.to_lowercase()))
            .max_by_key(|folder| folder.name.len());
        folder.map(|folder| folder.id)
    } else {
        None
    };

    let folder_scope_id = named_folder_id.or_else(|| match scope {
        ChatRetrievalScope::Folder(folder_id) => Some(folder_id.clone()),
        _ => None,
    });
    let mut query = QueryBuilder::<Sqlite>::new("");
    let mut has_filter = folder_scope_id.is_some();
    if let Some(folder_id) = folder_scope_id {
        query
            .push("WITH RECURSIVE folder_scope(id) AS (SELECT id FROM meeting_folders WHERE id = ");
        query.push_bind(folder_id);
        query.push(
            " UNION ALL SELECT f.id FROM meeting_folders f JOIN folder_scope s ON f.parent_id = s.id) SELECT title FROM meetings WHERE folder_id IN (SELECT id FROM folder_scope)",
        );
    } else {
        query.push("SELECT title FROM meetings");
    }
    match scope {
        ChatRetrievalScope::All | ChatRetrievalScope::Folder(_) => {}
        ChatRetrievalScope::Meeting(meeting_id) => {
            query.push(" WHERE id = ");
            query.push_bind(meeting_id);
            has_filter = true;
        }
        ChatRetrievalScope::SearchSnapshot(meeting_ids) => {
            if meeting_ids.is_empty() {
                return Ok(Vec::new());
            }
            query.push(" WHERE id IN (");
            has_filter = true;
            let mut values = query.separated(", ");
            for meeting_id in meeting_ids {
                values.push_bind(meeting_id);
            }
            drop(values);
            query.push(")");
        }
        ChatRetrievalScope::LiveRecording(_) => unreachable!(),
    }
    if let Some(local_date) = local_date {
        query.push(if has_filter {
            " AND date(created_at, 'localtime') = "
        } else {
            " WHERE date(created_at, 'localtime') = "
        });
        query.push_bind(local_date.format("%Y-%m-%d").to_string());
    }
    query.push(" ORDER BY datetime(created_at), id");
    query
        .build_query_scalar()
        .fetch_all(pool)
        .await
        .map_err(|error| format!("Failed to list meetings in this scope: {}", error))
}

fn format_meeting_list_context(titles: &[String]) -> String {
    let mut context = format!(
        "Authoritative meeting list for the current scope ({} total). Answer with every title below; do not say that meeting content was unavailable.\n",
        titles.len()
    );
    for title in titles {
        context.push_str("- ");
        context.push_str(title);
        context.push('\n');
    }
    context
}

async fn meeting_ids_for_local_date(
    pool: &SqlitePool,
    scope: &ChatRetrievalScope,
    local_date: NaiveDate,
) -> Result<Vec<String>, String> {
    if matches!(scope, ChatRetrievalScope::LiveRecording(_)) {
        return Ok(Vec::new());
    }

    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT id FROM meetings WHERE date(created_at, 'localtime') = ",
    );
    query.push_bind(local_date.format("%Y-%m-%d").to_string());
    match scope {
        ChatRetrievalScope::All => {}
        ChatRetrievalScope::Meeting(meeting_id) => {
            query.push(" AND id = ");
            query.push_bind(meeting_id.clone());
        }
        ChatRetrievalScope::Folder(folder_id) => {
            query.push(
                " AND folder_id IN (WITH RECURSIVE folder_scope(id) AS (SELECT id FROM meeting_folders WHERE id = ",
            );
            query.push_bind(folder_id);
            query.push(
                " UNION ALL SELECT f.id FROM meeting_folders f JOIN folder_scope s ON f.parent_id = s.id) SELECT id FROM folder_scope)",
            );
        }
        ChatRetrievalScope::SearchSnapshot(meeting_ids) => {
            if meeting_ids.is_empty() {
                return Ok(Vec::new());
            }
            query.push(" AND id IN (");
            let mut values = query.separated(", ");
            for meeting_id in meeting_ids {
                values.push_bind(meeting_id.clone());
            }
            drop(values);
            query.push(")");
        }
        ChatRetrievalScope::LiveRecording(_) => unreachable!(),
    }
    query.push(" ORDER BY datetime(created_at), id");
    query
        .build_query_scalar()
        .fetch_all(pool)
        .await
        .map_err(|error| format!("Failed to resolve meetings by local date: {}", error))
}

fn ensure_not_cancelled(token: Option<&CancellationToken>) -> Result<(), String> {
    if token.is_some_and(CancellationToken::is_cancelled) {
        Err("Chat preparation was cancelled".to_string())
    } else {
        Ok(())
    }
}

fn retrieval_scope_from_conversation(
    conversation: &ChatConversation,
) -> Result<ChatRetrievalScope, String> {
    let scope = match conversation.scope_kind.as_str() {
        "all" => ChatScope {
            kind: ChatScopeKind::All,
            key: conversation.scope_key.clone(),
            data: None,
        },
        "meeting" => ChatScope {
            kind: ChatScopeKind::Meeting,
            key: conversation.scope_key.clone(),
            data: None,
        },
        "folder" => ChatScope {
            kind: ChatScopeKind::Folder,
            key: conversation.scope_key.clone(),
            data: None,
        },
        "search_snapshot" => ChatScope {
            kind: ChatScopeKind::SearchSnapshot,
            key: conversation.scope_key.clone(),
            data: conversation
                .scope_data
                .as_deref()
                .map(serde_json::from_str::<ChatScopeData>)
                .transpose()
                .map_err(|e| format!("Invalid persisted chat scope: {}", e))?,
        },
        "live_recording" => ChatScope {
            kind: ChatScopeKind::LiveRecording,
            key: conversation.scope_key.clone(),
            data: None,
        },
        _ => {
            return Err("This chat scope is not available for saved-meeting retrieval".to_string())
        }
    };
    scope
        .validate()
        .map_err(|e| format!("Invalid persisted chat scope: {}", e))?;
    Ok(match scope.kind {
        ChatScopeKind::All => ChatRetrievalScope::All,
        ChatScopeKind::Meeting => ChatRetrievalScope::Meeting(scope.key),
        ChatScopeKind::Folder => ChatRetrievalScope::Folder(scope.key),
        ChatScopeKind::SearchSnapshot => ChatRetrievalScope::SearchSnapshot(
            scope
                .data
                .expect("validated search snapshot scope")
                .result_ids,
        ),
        ChatScopeKind::LiveRecording => ChatRetrievalScope::LiveRecording(scope.key),
        ChatScopeKind::OrphanedMeeting => unreachable!(),
    })
}

fn live_snapshot_context(
    segments: &[crate::audio::recording_saver::TranscriptSegment],
    scope_key: &str,
    max_context_chars: usize,
) -> (String, Vec<ChatSource>) {
    let transcript = segments
        .iter()
        .map(|segment| format!("{} {}", segment.display_time, segment.text))
        .collect::<Vec<_>>()
        .join("\n");
    // Live sessions: the most-recent speech matters more than the head, so the
    // budget keeps the transcript tail instead of the first N chars.
    let context = tail_at_char_boundary(&transcript, max_context_chars).to_string();
    let sources = if transcript.is_empty() {
        Vec::new()
    } else {
        vec![ChatSource {
            meeting_id: scope_key.to_string(),
            meeting_title: "Live recording".to_string(),
            chunk_type: "live_transcript".to_string(),
            snippet: segments
                .last()
                .map(|segment| segment.text.clone())
                .unwrap_or_default(),
            folder_name: String::new(),
            source_kind: Some("live_recording".to_string()),
        }]
    };
    (context, sources)
}

#[derive(Debug)]
struct MeetingChatContext {
    meeting_id: String,
    meeting_title: String,
    summary: Option<String>,
    notes: Option<String>,
    transcripts: Vec<crate::database::repositories::fts::FtsSearchResult>,
    total_transcript_segments: usize,
}

fn chat_source_from_result(
    result: &crate::database::repositories::fts::FtsSearchResult,
) -> ChatSource {
    ChatSource {
        meeting_id: result.meeting_id.clone(),
        meeting_title: result.meeting_title.clone(),
        chunk_type: result.chunk_type.clone(),
        snippet: result.snippet.clone(),
        folder_name: result.folder_name.clone(),
        source_kind: None,
    }
}

fn chat_source_from_hydrated(source: &crate::retrieval::HydratedSource) -> ChatSource {
    ChatSource {
        meeting_id: source.meeting_id.clone(),
        meeting_title: source.meeting_title.clone(),
        chunk_type: source.source_kind.clone(),
        snippet: source.snippet.clone(),
        folder_name: source.folder_name.clone(),
        source_kind: Some(source.source_kind.clone()),
    }
}

async fn resolve_meeting_context(
    pool: &SqlitePool,
    meeting_id: &str,
    search_query: &str,
    original_query: &str,
    chunk_limit: u32,
) -> Result<MeetingChatContext, String> {
    // Shared authoritative saved-meeting loader (same latest-summary policy
    // and transcript chronology as the retrieval worker and Task 3.3
    // hydration); the FTS hit selection below stays chat-specific.
    let hits =
        search_meeting_transcripts(pool, meeting_id, search_query, original_query, chunk_limit)
            .await?;
    let hit_ids = hits
        .iter()
        .map(|hit| hit.chunk_id.clone())
        .collect::<Vec<_>>();
    let mut source = crate::database::repositories::retrieval::RetrievalRepository::load_meeting_source_relevant(
        pool, meeting_id, &hit_ids,
    )
    .await
    .map_err(|error| format!("Failed to load meeting context: {}", error))?
    .ok_or_else(|| "Meeting not found".to_string())?;
    let total_transcript_segments = source.transcript_segments_total;
    let hit_ids = hit_ids.iter().map(String::as_str).collect::<HashSet<_>>();
    let has_mapped_hit = source
        .transcripts
        .iter()
        .filter(|row| hit_ids.contains(row.id.as_str()))
        .next()
        .is_some();
    if !has_mapped_hit {
        source = crate::database::repositories::retrieval::RetrievalRepository::load_meeting_source_relevant(
            pool, meeting_id, &[],
        )
        .await
        .map_err(|error| format!("Failed to load meeting context: {}", error))?
        .ok_or_else(|| "Meeting not found".to_string())?;
    }
    let mapped_hit_ids = source
        .transcripts
        .iter()
        .filter(|row| hit_ids.contains(row.id.as_str()))
        .map(|row| row.id.as_str())
        .collect::<HashSet<_>>();
    let included = if mapped_hit_ids.is_empty() {
        (0..total_transcript_segments.min(chunk_limit as usize)).collect::<HashSet<_>>()
    } else if source.transcripts.is_empty() {
        HashSet::new()
    } else {
        source
            .transcripts
            .iter()
            .zip(source.transcript_positions.iter().copied())
            .filter(|(row, _)| mapped_hit_ids.contains(row.id.as_str()))
            .flat_map(|(_, position)| {
                [
                    position.saturating_sub(1),
                    position,
                    (position + 1).min(total_transcript_segments - 1),
                ]
            })
            .collect::<HashSet<_>>()
    };
    let transcripts = source
        .transcripts
        .iter()
        .zip(source.transcript_positions.iter().copied())
        .filter(|(_, position)| included.contains(position))
        .map(
            |(segment, _)| crate::database::repositories::fts::FtsSearchResult {
                meeting_id: meeting_id.to_string(),
                meeting_title: source.title.clone(),
                chunk_type: "transcript".to_string(),
                chunk_id: segment.id.clone(),
                snippet: segment.text.clone(),
                speaker: segment.speaker.clone(),
                timestamp_label: Some(segment.timestamp.clone()),
                folder_id: None,
                folder_name: source.folder_name.clone(),
                rank: 0.0,
            },
        )
        .collect();
    Ok(MeetingChatContext {
        meeting_id: source.meeting_id,
        meeting_title: source.title,
        summary: source.latest_summary_markdown,
        notes: source
            .notes_markdown
            .filter(|notes| !notes.trim().is_empty()),
        transcripts,
        total_transcript_segments,
    })
}

async fn search_meeting_transcripts(
    pool: &SqlitePool,
    meeting_id: &str,
    search_query: &str,
    original_query: &str,
    chunk_limit: u32,
) -> Result<Vec<crate::database::repositories::fts::FtsSearchResult>, String> {
    let mut results = FtsRepository::search_transcripts_with_mode(
        pool,
        search_query,
        chunk_limit,
        meeting_id,
        MatchMode::And,
    )
    .await
    .map_err(|error| format!("Search failed: {}", error))?;
    if results.is_empty() {
        results = FtsRepository::search_transcripts_with_mode(
            pool,
            search_query,
            chunk_limit,
            meeting_id,
            MatchMode::Or,
        )
        .await
        .map_err(|error| format!("Search failed: {}", error))?;
    }
    if results.is_empty() && search_query != original_query {
        results = FtsRepository::search_transcripts_with_mode(
            pool,
            original_query,
            chunk_limit,
            meeting_id,
            MatchMode::And,
        )
        .await
        .map_err(|error| format!("Search failed: {}", error))?;
        if results.is_empty() {
            results = FtsRepository::search_transcripts_with_mode(
                pool,
                original_query,
                chunk_limit,
                meeting_id,
                MatchMode::Or,
            )
            .await
            .map_err(|error| format!("Search failed: {}", error))?;
        }
    }
    Ok(results)
}

async fn search_scope(
    pool: &SqlitePool,
    query: &str,
    chunk_limit: u32,
    scope: &ChatRetrievalScope,
    mode: MatchMode,
) -> Result<Vec<crate::database::repositories::fts::FtsSearchResult>, String> {
    match scope {
        ChatRetrievalScope::All => {
            FtsRepository::search_with_mode(pool, query, chunk_limit, None, mode).await
        }
        ChatRetrievalScope::Meeting(meeting_id) => {
            FtsRepository::search_with_mode(pool, query, chunk_limit, Some(meeting_id), mode).await
        }
        ChatRetrievalScope::Folder(folder_id) => {
            FtsRepository::search_with_folder_id(pool, query, chunk_limit, folder_id, mode).await
        }
        _ => unreachable!(),
    }
    .map_err(|error| {
        tracing::error!("FTS search failed for chat: {}", error);
        format!("Search failed: {}", error)
    })
}

async fn resolve_scope_results(
    pool: &SqlitePool,
    search_query: &str,
    original_query: &str,
    chunk_limit: u32,
    scope: ChatRetrievalScope,
    meeting_ids_override: Option<&[String]>,
) -> Result<Vec<crate::database::repositories::fts::FtsSearchResult>, String> {
    if let Some(meeting_ids) = meeting_ids_override {
        return FtsRepository::get_by_meeting_ids(
            pool,
            meeting_ids,
            chunk_limit,
            SNAPSHOT_REHYDRATION_CHUNK_CAP,
        )
        .await
        .map_err(|error| {
            tracing::error!("FTS search failed for chat: {}", error);
            format!("Search failed: {}", error)
        });
    }

    if let ChatRetrievalScope::SearchSnapshot(meeting_ids) = &scope {
        return FtsRepository::get_by_meeting_ids(
            pool,
            meeting_ids,
            chunk_limit,
            SNAPSHOT_REHYDRATION_CHUNK_CAP,
        )
        .await
        .map_err(|error| {
            tracing::error!("FTS search failed for chat: {}", error);
            format!("Search failed: {}", error)
        });
    }

    let mut attempts = vec![
        (search_query, MatchMode::And),
        (search_query, MatchMode::Or),
    ];
    if search_query != original_query {
        attempts.extend([
            (original_query, MatchMode::And),
            (original_query, MatchMode::Or),
        ]);
    }

    let mut attempt_results = Vec::with_capacity(attempts.len());
    let mut claimed = HashSet::new();
    for (index, (query, mode)) in attempts.into_iter().enumerate() {
        let attempt_limit = chunk_limit.saturating_mul(index as u32 + 1);
        let unique_results = search_scope(pool, query, attempt_limit, &scope, mode)
            .await?
            .into_iter()
            .filter(|result| {
                claimed.insert((
                    result.meeting_id.clone(),
                    result.chunk_type.clone(),
                    result.chunk_id.clone(),
                ))
            })
            .take(chunk_limit as usize)
            .collect::<Vec<_>>();
        attempt_results.push(unique_results.into_iter());
    }

    let mut results = Vec::new();
    while results.len() < chunk_limit as usize {
        let before = results.len();
        for attempt in &mut attempt_results {
            if let Some(result) = attempt.next() {
                results.push(result);
            }
            if results.len() >= chunk_limit as usize {
                break;
            }
        }
        if results.len() == before {
            break;
        }
    }
    Ok(results)
}

/// Deep runs only for the supported persisted All/Folder scopes in this
/// release: a derived today allow-list (snapshot/today/saved-meeting members)
/// keeps the request Fast until Tasks 4.3/4.4 extend the lifecycle.
fn deep_preparation_eligible(
    retrieval_mode: ChatRetrievalMode,
    today_meeting_ids: Option<&Vec<String>>,
) -> bool {
    retrieval_mode == ChatRetrievalMode::Deep && today_meeting_ids.is_none()
}

fn should_rewrite_query(history: Option<&Vec<ChatMessage>>, query: &str) -> bool {
    history.is_some_and(|messages| messages.len() >= 2) && query.chars().count() < 100
}

fn build_rewrite_prompt(history: &[ChatMessage], query: &str) -> String {
    let mut prompt = String::new();
    for message in history.iter().rev().take(10).rev() {
        prompt.push_str(&format!("{}: {}\n", message.role, message.content));
    }
    prompt.push_str(&format!("\nFollow-up question: {}", query));
    prompt
}

fn truncate_at_char_boundary(value: &str, cap: usize) -> &str {
    value
        .char_indices()
        .nth(cap)
        .map(|(index, _)| &value[..index])
        .unwrap_or(value)
}

fn tail_at_char_boundary(value: &str, max_chars: usize) -> &str {
    let char_count = value.chars().count();
    let start = char_count.saturating_sub(max_chars);
    value
        .char_indices()
        .nth(start)
        .map(|(index, _)| &value[index..])
        .unwrap_or("")
}

fn context_budget_for_prompt(
    query: &str,
    search_query: &str,
    temporal_context: &str,
    max_context_chars: usize,
) -> usize {
    max_context_chars.saturating_sub(
        format!("\n{}\nMeeting context:\n", temporal_context)
            .chars()
            .count()
            + format!(
                "\nUser question: {}\nSearch query: {}\n",
                query, search_query
            )
            .chars()
            .count(),
    )
}

fn assemble_prompt(
    context: &str,
    history: &[ChatMessage],
    query: &str,
    search_query: &str,
    temporal_context: &str,
    max_context_chars: usize,
) -> String {
    let question_block = format!(
        "\nUser question: {}\nSearch query: {}\n",
        query, search_query
    );
    let mut context_block = format!("\n{}\nMeeting context:\n{}", temporal_context, context);
    let mut history: Vec<String> = history
        .iter()
        .take(10)
        .map(|message| format!("{}: {}\n", message.role, message.content))
        .collect();
    let budget = max_context_chars.saturating_sub(question_block.chars().count());

    while context_block.chars().count()
        + history
            .iter()
            .map(|item| item.chars().count())
            .sum::<usize>()
        > budget
        && !history.is_empty()
    {
        history.remove(0);
    }
    if context_block.chars().count() > budget {
        context_block = truncate_at_char_boundary(&context_block, budget).to_string();
    }

    format!("{}{}{}", context_block, history.concat(), question_block)
}

fn finish_non_streaming_chat_request(
    state: &ChatRequestState,
    request_id: &str,
    token: &ChatRequestToken,
    result: Result<Result<ChatResponse, String>, tokio::time::error::Elapsed>,
) -> Result<ChatResponse, String> {
    let timed_out = result.is_err();
    if timed_out {
        token.cancel();
    }
    if !state.clear_if_owner(ChatRequestSurface::Chat, request_id, token) {
        return Err("Chat request was cancelled or superseded".to_string());
    }
    if timed_out {
        return Err("Chat request timed out".to_string());
    }
    if token.is_cancelled() {
        return Err("Chat request was cancelled".to_string());
    }
    result.expect("non-streaming chat timeout was already handled")
}

#[tauri::command]
pub async fn api_chat_with_meetings<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    retrieval: tauri::State<'_, RetrievalLifecycle>,
    request_state: tauri::State<'_, ChatRequestState>,
    query: String,
    history: Option<Vec<ChatMessage>>,
    auth_token: Option<String>,
    meeting_id: Option<String>,
    request_id: Option<String>,
    mode: Option<ChatRetrievalMode>,
) -> Result<ChatResponse, String> {
    info!(
        "api_chat_with_meetings called: query_len={}, history_len={:?}, auth_token={}",
        query.len(),
        history.as_ref().map(|h| h.len()),
        auth_token.is_some()
    );

    let request_id = request_id.unwrap_or_else(|| format!("chat-{}", Uuid::new_v4()));
    let request_state = request_state.inner().clone();
    let token = request_state.claim_request(ChatRequestSurface::Chat, &request_id);
    let pool = state.db_manager.pool().clone();
    let app_data_dir = app.path().app_data_dir().ok();
    let client = reqwest::Client::new();
    let lifecycle = retrieval.inner().clone();
    let result = tokio::time::timeout(CHAT_REQUEST_TIMEOUT, async {
        let inputs = prepare_chat_inputs_with_lifecycle(
            &pool,
            app_data_dir,
            &client,
            &query,
            history.as_ref(),
            meeting_id,
            lifecycle,
            Some(token.as_ref()),
            mode,
            None,
        )
        .await?;
        let answer = generate_summary(
            &client,
            &inputs.provider,
            &inputs.model_name,
            &inputs.api_key,
            SYSTEM_PROMPT,
            &inputs.user_prompt,
            inputs.ollama_endpoint.as_deref(),
            inputs.custom_openai_endpoint.as_deref(),
            inputs.custom_openai_max_tokens,
            inputs.custom_openai_temperature,
            inputs.custom_openai_top_p,
            inputs.app_data_dir.as_ref(),
            Some(token.as_ref()),
        )
        .await
        .map_err(|e| {
            tracing::error!("LLM call failed for chat: {}", e);
            format!("LLM error: {}", e)
        })?;

        info!(
            "Chat completed: {} sources, {} answer chars, {} provider round trips including final generation",
            inputs.sources.len(),
            answer.len(),
            inputs.provider_round_trips + 1
        );

        Ok(ChatResponse {
            answer,
            sources: inputs.sources,
        })
    })
    .await;

    finish_non_streaming_chat_request(&request_state, &request_id, &token, result)
}

#[tauri::command]
pub async fn api_chat_with_scoped_conversation<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    retrieval: tauri::State<'_, RetrievalLifecycle>,
    request_state: tauri::State<'_, ChatRequestState>,
    conversation_id: String,
    query: String,
    history: Option<Vec<ChatMessage>>,
    auth_token: Option<String>,
    live_transcript_consent: bool,
    request_id: Option<String>,
    mode: Option<ChatRetrievalMode>,
) -> Result<ChatResponse, String> {
    info!(
        "api_chat_with_scoped_conversation called: conversation_id={}, query_len={}, history_len={:?}, auth_token={}",
        conversation_id,
        query.len(),
        history.as_ref().map(|items| items.len()),
        auth_token.is_some()
    );

    let request_id = request_id.unwrap_or_else(|| format!("chat-{}", Uuid::new_v4()));
    let request_state = request_state.inner().clone();
    let token = request_state.claim_request(ChatRequestSurface::Chat, &request_id);
    let pool = state.db_manager.pool().clone();
    let app_data_dir = app.path().app_data_dir().ok();
    let client = reqwest::Client::new();
    let lifecycle = retrieval.inner().clone();
    let result = tokio::time::timeout(CHAT_REQUEST_TIMEOUT, async {
        let inputs = prepare_scoped_chat_inputs_with_authorization(
            &pool,
            app_data_dir,
            &client,
            &query,
            history.as_ref(),
            &conversation_id,
            LiveTranscriptAuthorization {
                active_scope_key:
                    crate::audio::recording_commands::active_live_transcript_scope_key(),
                consent: live_transcript_consent,
            },
            Some(token.as_ref()),
            Some(lifecycle),
            mode,
            None,
        )
        .await?;
        let answer = generate_summary(
            &client,
            &inputs.provider,
            &inputs.model_name,
            &inputs.api_key,
            SYSTEM_PROMPT,
            &inputs.user_prompt,
            inputs.ollama_endpoint.as_deref(),
            inputs.custom_openai_endpoint.as_deref(),
            inputs.custom_openai_max_tokens,
            inputs.custom_openai_temperature,
            inputs.custom_openai_top_p,
            inputs.app_data_dir.as_ref(),
            Some(token.as_ref()),
        )
        .await
        .map_err(|e| format!("LLM error: {}", e))?;

        Ok(ChatResponse {
            answer,
            sources: inputs.sources,
        })
    })
    .await;

    finish_non_streaming_chat_request(&request_state, &request_id, &token, result)
}

#[tauri::command]
pub async fn api_chat_with_meetings_stream<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    retrieval: tauri::State<'_, RetrievalLifecycle>,
    stream_state: tauri::State<'_, ChatRequestState>,
    query: String,
    history: Option<Vec<ChatMessage>>,
    auth_token: Option<String>,
    stream_id: String,
    meeting_id: Option<String>,
    mode: Option<ChatRetrievalMode>,
) -> Result<(), String> {
    info!("api_chat_with_meetings_stream called: query_len={}, history_len={:?}, auth_token={}, stream_id={}", query.len(), history.as_ref().map(|items| items.len()), auth_token.is_some(), stream_id);
    let token = claim_chat_stream(&stream_state, &stream_id).await;
    let request_state = stream_state.inner().clone();
    let request_stream_id = stream_id.clone();
    let client = reqwest::Client::new();
    let progress_sink = deep_progress_sink(&request_state, &app, &stream_id, &token);
    let result = tokio::time::timeout(CHAT_REQUEST_TIMEOUT, async {
        let inputs = match prepare_chat_inputs_with_lifecycle(
            state.db_manager.pool(),
            app.path().app_data_dir().ok(),
            &client,
            &query,
            history.as_ref(),
            meeting_id.clone(),
            retrieval.inner().clone(),
            Some(token.as_ref()),
            mode,
            Some(&progress_sink),
        )
        .await
        {
            Ok(inputs) => inputs,
            Err(error) => {
                if suppress_chat_preparation_error(&request_state, &stream_id, &token) {
                    return Ok(());
                }
                return Err(error);
            }
        };
        stream_chat(
            app,
            request_state.clone(),
            inputs,
            stream_id,
            meeting_id,
            token.clone(),
        )
        .await
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            token.cancel();
            request_state.clear_if_owner(ChatRequestSurface::Chat, &request_stream_id, &token);
            Err("Chat request timed out".to_string())
        }
    }
}

#[tauri::command]
pub async fn api_chat_with_scoped_conversation_stream<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    retrieval: tauri::State<'_, RetrievalLifecycle>,
    stream_state: tauri::State<'_, ChatRequestState>,
    conversation_id: String,
    query: String,
    history: Option<Vec<ChatMessage>>,
    auth_token: Option<String>,
    stream_id: String,
    live_transcript_consent: bool,
    mode: Option<ChatRetrievalMode>,
) -> Result<(), String> {
    info!("api_chat_with_scoped_conversation_stream called: conversation_id={}, query_len={}, history_len={:?}, auth_token={}, stream_id={}", conversation_id, query.len(), history.as_ref().map(|items| items.len()), auth_token.is_some(), stream_id);
    let token = claim_chat_stream(&stream_state, &stream_id).await;
    let request_state = stream_state.inner().clone();
    let request_stream_id = stream_id.clone();
    let client = reqwest::Client::new();
    let progress_sink = deep_progress_sink(&request_state, &app, &stream_id, &token);
    let result = tokio::time::timeout(CHAT_REQUEST_TIMEOUT, async {
        let inputs = match prepare_scoped_chat_inputs_with_authorization(
            state.db_manager.pool(),
            app.path().app_data_dir().ok(),
            &client,
            &query,
            history.as_ref(),
            &conversation_id,
            LiveTranscriptAuthorization {
                active_scope_key:
                    crate::audio::recording_commands::active_live_transcript_scope_key(),
                consent: live_transcript_consent,
            },
            Some(token.as_ref()),
            Some(retrieval.inner().clone()),
            mode,
            Some(&progress_sink),
        )
        .await
        {
            Ok(inputs) => inputs,
            Err(error) => {
                if suppress_chat_preparation_error(&request_state, &stream_id, &token) {
                    return Ok(());
                }
                return Err(error);
            }
        };
        stream_chat(
            app,
            request_state.clone(),
            inputs,
            stream_id,
            None,
            token.clone(),
        )
        .await
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            token.cancel();
            request_state.clear_if_owner(ChatRequestSurface::Chat, &request_stream_id, &token);
            Err("Chat request timed out".to_string())
        }
    }
}

async fn stream_chat<R: Runtime>(
    app: AppHandle<R>,
    stream_state: ChatRequestState,
    inputs: ChatInputs,
    stream_id: String,
    meeting_id: Option<String>,
    token: ChatRequestToken,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let sources = inputs.sources.clone();

    if token.is_cancelled() || !is_chat_stream_owner(&stream_state, &stream_id, &token).await {
        clear_chat_stream_if_owner(&stream_state, &stream_id, &token).await;
        return Ok(());
    }

    if inputs.retrieval_mode == ChatRetrievalMode::Deep {
        info!(
            "Chat Deep stream handoff: {} provider round trips before generation (rewrite + planner calls)",
            inputs.provider_round_trips
        );
    }

    if !emit_chat_stream_event_if_owner(
        &stream_state,
        &app,
        &stream_id,
        &token,
        "chat-stream-start",
        serde_json::json!({ "streamId": stream_id, "sources": sources, "meetingId": meeting_id }),
        false,
    ) {
        return Ok(());
    }

    let partial_text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let partial_for_chunk = partial_text.clone();
    let app_for_chunk = app.clone();
    let stream_id_for_chunk = stream_id.clone();
    let token_for_chunk = token.clone();
    let stream_state_for_chunk = stream_state.clone();
    let on_chunk = move |chunk: &str| {
        let chunk_text = chunk.to_string();
        let partial = partial_for_chunk.clone();
        let app = app_for_chunk.clone();
        publish_chat_stream_event_if_owner(
            &stream_state_for_chunk,
            &stream_id_for_chunk,
            &token_for_chunk,
            "chat-stream-chunk",
            serde_json::json!({ "streamId": stream_id_for_chunk.clone(), "text": chunk_text.clone() }),
            false,
            move |event, payload| {
                partial.lock().unwrap().push_str(&chunk_text);
                if let Err(error) = app.emit(event, payload) {
                    tracing::error!("Failed to emit {}: {}", event, error);
                }
            },
        );
    };

    let stream_result = generate_summary_stream(
        &client,
        &inputs.provider,
        &inputs.model_name,
        &inputs.api_key,
        SYSTEM_PROMPT,
        &inputs.user_prompt,
        inputs.ollama_endpoint.as_deref(),
        inputs.custom_openai_endpoint.as_deref(),
        inputs.custom_openai_max_tokens,
        inputs.custom_openai_temperature,
        inputs.custom_openai_top_p,
        inputs.app_data_dir.as_ref(),
        Some(token.as_ref()),
        on_chunk,
    )
    .await;

    match stream_result {
        Ok(answer) => {
            let Some(payload) =
                completed_chat_stream_payload(token.as_ref(), &stream_id, &answer, &sources)
            else {
                clear_chat_stream_if_owner(&stream_state, &stream_id, &token).await;
                return Ok(());
            };
            info!(
                "Chat stream completed: {} sources, {} answer chars, {} provider round trips including final generation",
                sources.len(),
                answer.len(),
                inputs.provider_round_trips + 1
            );
            emit_chat_stream_event_if_owner(
                &stream_state,
                &app,
                &stream_id,
                &token,
                "chat-stream-done",
                payload,
                true,
            );
            Ok(())
        }
        Err(e) if e.to_lowercase().contains("cancelled") => {
            let answer = partial_text.lock().unwrap().clone();
            tracing::info!("Chat stream cancelled after {} chars", answer.len());
            clear_chat_stream_if_owner(&stream_state, &stream_id, &token).await;
            Ok(())
        }
        Err(e) => {
            let answer = partial_text.lock().unwrap().clone();
            tracing::error!("LLM stream failed for chat: {}", e);
            // ponytail: preserve partial answers that already reached the UI.
            // If any chunk was emitted, finalize it instead of replacing it with an error bubble.
            let (event, payload) = if answer.is_empty() {
                (
                    "chat-stream-error",
                    serde_json::json!({ "streamId": stream_id, "error": e }),
                )
            } else {
                (
                    "chat-stream-done",
                    serde_json::json!({ "streamId": stream_id, "answer": answer, "sources": sources }),
                )
            };
            emit_chat_stream_event_if_owner(
                &stream_state,
                &app,
                &stream_id,
                &token,
                event,
                payload,
                true,
            );
            Ok(())
        }
    }
}

fn completed_chat_stream_payload(
    token: &CancellationToken,
    stream_id: &str,
    answer: &str,
    sources: &[ChatSource],
) -> Option<serde_json::Value> {
    (!token.is_cancelled())
        .then(|| serde_json::json!({ "streamId": stream_id, "answer": answer, "sources": sources }))
}

async fn claim_chat_stream(state: &ChatRequestState, stream_id: &str) -> ChatRequestToken {
    state.claim_request(ChatRequestSurface::Chat, stream_id)
}

async fn is_chat_stream_owner(
    state: &ChatRequestState,
    stream_id: &str,
    token: &ChatRequestToken,
) -> bool {
    state.is_owner(ChatRequestSurface::Chat, stream_id, token)
}

async fn clear_chat_stream_if_owner(
    state: &ChatRequestState,
    stream_id: &str,
    token: &ChatRequestToken,
) {
    state.clear_if_owner(ChatRequestSurface::Chat, stream_id, token);
}

fn suppress_chat_preparation_error(
    state: &ChatRequestState,
    stream_id: &str,
    token: &ChatRequestToken,
) -> bool {
    let owned = state.is_owner(ChatRequestSurface::Chat, stream_id, token);
    state.clear_if_owner(ChatRequestSurface::Chat, stream_id, token);
    !owned || token.is_cancelled()
}

fn emit_chat_stream_event_if_owner<R: Runtime>(
    state: &ChatRequestState,
    app: &AppHandle<R>,
    stream_id: &str,
    token: &ChatRequestToken,
    event: &str,
    payload: serde_json::Value,
    clear: bool,
) -> bool {
    publish_chat_stream_event_if_owner(
        state,
        stream_id,
        token,
        event,
        payload,
        clear,
        |event, payload| {
            if let Err(error) = app.emit(event, payload) {
                tracing::error!("Failed to emit {}: {}", event, error);
            }
        },
    )
}

fn emit_chat_preparation_progress_if_owner<R: Runtime>(
    state: &ChatRequestState,
    app: &AppHandle<R>,
    payload: ChatPreparationProgressPayload,
    token: &ChatRequestToken,
) -> bool {
    let stream_id = payload.stream_id.clone();
    emit_chat_stream_event_if_owner(
        state,
        app,
        &stream_id,
        token,
        "chat-preparation-progress",
        serde_json::to_value(payload).expect("chat preparation progress is serializable"),
        false,
    )
}

/// The privacy-safe Deep progress sink handed to the retrieval agent: it maps
/// stage identity and counts onto the established Chat publication fence.
/// The agent itself never touches Tauri events or the app handle.
fn deep_progress_sink<R: Runtime>(
    request_state: &ChatRequestState,
    app: &AppHandle<R>,
    stream_id: &str,
    token: &ChatRequestToken,
) -> impl Fn(crate::retrieval::agent::DeepProgressEvent) + Send + Sync + 'static {
    let request_state = request_state.clone();
    let app = app.clone();
    let stream_id = stream_id.to_string();
    let token = token.clone();
    move |event: crate::retrieval::agent::DeepProgressEvent| {
        let stage = match event.stage {
            crate::retrieval::agent::DeepProgressStage::InitialRetrieval => {
                ChatPreparationStage::InitialRetrieval
            }
            crate::retrieval::agent::DeepProgressStage::PlannerRound => {
                ChatPreparationStage::PlannerRound
            }
            crate::retrieval::agent::DeepProgressStage::AdditionalSearch => {
                ChatPreparationStage::AdditionalSearch
            }
            crate::retrieval::agent::DeepProgressStage::AnswerGeneration => {
                ChatPreparationStage::AnswerGeneration
            }
        };
        emit_chat_preparation_progress_if_owner(
            &request_state,
            &app,
            ChatPreparationProgressPayload {
                stream_id: stream_id.clone(),
                stage,
                completed: event.completed,
                total: event.total,
            },
            &token,
        );
    }
}

fn publish_chat_stream_event_if_owner<F: FnOnce(&str, serde_json::Value)>(
    state: &ChatRequestState,
    stream_id: &str,
    token: &ChatRequestToken,
    event: &str,
    payload: serde_json::Value,
    clear: bool,
    emit: F,
) -> bool {
    state.publish_if_owner(
        ChatRequestSurface::Chat,
        stream_id,
        token,
        event,
        payload,
        clear,
        emit,
    )
}

/// Cancels an active chat stream. `stream_id: None` cancels any active stream;
/// a specific id only cancels when it matches the currently active stream.
async fn cancel_chat_stream(state: &ChatRequestState, stream_id: Option<&str>) {
    state.cancel_request(ChatRequestSurface::Chat, stream_id);
}

#[tauri::command]
pub async fn api_cancel_chat_stream(
    stream_state: tauri::State<'_, ChatRequestState>,
    stream_id: Option<String>,
) -> Result<(), String> {
    cancel_chat_stream(&stream_state, stream_id.as_deref()).await;
    Ok(())
}

#[tauri::command]
pub async fn api_cancel_chat_request(
    request_state: tauri::State<'_, ChatRequestState>,
    request_id: String,
) -> Result<(), String> {
    request_state.cancel_request(ChatRequestSurface::Chat, Some(&request_id));
    Ok(())
}

#[tauri::command]
pub async fn api_build_context<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    query: String,
    limit: Option<u32>,
    auth_token: Option<String>,
) -> Result<String, String> {
    info!(
        "api_build_context called: query_len={}, limit={:?}, auth_token={}",
        query.len(),
        limit,
        auth_token.is_some()
    );
    let pool = state.db_manager.pool();
    let results = FtsRepository::search(pool, &query, limit.unwrap_or(10), None)
        .await
        .map_err(|e| format!("Search failed: {}", e))?;
    Ok(build_context_markdown(&results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::fts::FtsSearchResult;
    use sqlx::SqlitePool;

    fn make_search_result(
        meeting_id: &str,
        title: &str,
        chunk_type: &str,
        snippet: &str,
    ) -> FtsSearchResult {
        FtsSearchResult {
            meeting_id: meeting_id.to_string(),
            meeting_title: title.to_string(),
            chunk_type: chunk_type.to_string(),
            chunk_id: format!("{}-{}", chunk_type, meeting_id),
            snippet: snippet.to_string(),
            speaker: None,
            timestamp_label: None,
            folder_id: None,
            folder_name: "General".to_string(),
            rank: 0.0,
        }
    }

    #[test]
    fn chat_retrieval_mode_serializes_lowercase_and_rejects_unknown_values() {
        assert_eq!(ChatRetrievalMode::default(), ChatRetrievalMode::Fast);
        assert_eq!(
            serde_json::to_string(&ChatRetrievalMode::Deep).unwrap(),
            "\"deep\""
        );
        assert_eq!(
            serde_json::from_str::<ChatRetrievalMode>("\"fast\"").unwrap(),
            ChatRetrievalMode::Fast
        );
        assert!(serde_json::from_str::<ChatRetrievalMode>("\"turbo\"").is_err());
    }

    #[test]
    fn preparation_progress_payload_contains_only_stage_identity_and_counts() {
        for (stage, name) in [
            (ChatPreparationStage::InitialRetrieval, "initial_retrieval"),
            (ChatPreparationStage::PlannerRound, "planner_round"),
            (ChatPreparationStage::AdditionalSearch, "additional_search"),
            (ChatPreparationStage::AnswerGeneration, "answer_generation"),
        ] {
            let value = serde_json::to_value(ChatPreparationProgressPayload {
                stream_id: "stream".to_string(),
                stage,
                completed: 2,
                total: 3,
            })
            .unwrap();
            assert_eq!(
                value,
                serde_json::json!({
                    "streamId": "stream",
                    "stage": name,
                    "completed": 2,
                    "total": 3,
                })
            );
            for forbidden in ["planner", "query", "evidence", "text"] {
                assert!(value.get(forbidden).is_none());
            }
        }
    }

    async fn scope_pool() -> SqlitePool {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::query(
            r#"
            CREATE TABLE meetings (id TEXT PRIMARY KEY, title TEXT NOT NULL, folder_id TEXT, created_at TEXT, saved_at TEXT);
            CREATE TABLE meeting_folders (id TEXT PRIMARY KEY, name TEXT NOT NULL, parent_id TEXT, created_at TEXT NOT NULL);
            CREATE TABLE transcripts (id TEXT PRIMARY KEY, meeting_id TEXT NOT NULL, transcript TEXT NOT NULL, timestamp TEXT NOT NULL, speaker TEXT, audio_start_time REAL, audio_end_time REAL);
            CREATE TABLE meeting_notes (meeting_id TEXT PRIMARY KEY, notes_markdown TEXT);
            CREATE TABLE summary_processes (meeting_id TEXT NOT NULL, template_id TEXT NOT NULL, updated_at TEXT NOT NULL, result TEXT, PRIMARY KEY (meeting_id, template_id));
            CREATE TABLE search_source_state (meeting_id TEXT PRIMARY KEY, source_revision INTEGER);
            CREATE VIRTUAL TABLE meeting_fts USING fts5(
                meeting_id UNINDEXED, chunk_type UNINDEXED, chunk_id UNINDEXED,
                text, speaker UNINDEXED, timestamp_label UNINDEXED,
                folder_id UNINDEXED, folder_name
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        for (meeting_id, title) in [("m1", "Root"), ("m2", "Child"), ("m3", "Other")] {
            sqlx::query("INSERT INTO meetings (id, title) VALUES (?, ?)")
                .bind(meeting_id)
                .bind(title)
                .execute(&pool)
                .await
                .unwrap();
        }
        for (id, name, parent_id) in [
            ("root", "Same name", None),
            ("child", "Child", Some("root")),
            ("other", "Same name", None),
        ] {
            sqlx::query(
                "INSERT INTO meeting_folders (id, name, parent_id, created_at) VALUES (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(name)
            .bind(parent_id)
            .bind("2026-08-19T00:00:00Z")
            .execute(&pool)
            .await
            .unwrap();
        }
        for (meeting_id, chunk_id, folder_id) in [
            ("m1", "c1", "root"),
            ("m2", "c2", "child"),
            ("m3", "c3", "other"),
        ] {
            sqlx::query("UPDATE meetings SET folder_id = ? WHERE id = ?")
                .bind(folder_id)
                .bind(meeting_id)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, folder_id, folder_name) VALUES (?, 'note', ?, 'alpha', ?, 'Same name')")
                .bind(meeting_id)
                .bind(chunk_id)
                .bind(folder_id)
                .execute(&pool)
                .await
                .unwrap();
            if meeting_id == "m2" {
                sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, 'alpha', '10:00')")
                    .bind(format!("t-{}", meeting_id))
                    .bind(meeting_id)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        }
        pool
    }

    #[tokio::test]
    async fn scope_resolution_keeps_legacy_all_and_meeting_behavior() {
        let pool = scope_pool().await;
        let all = resolve_scope_results(&pool, "alpha", "alpha", 10, ChatRetrievalScope::All, None)
            .await
            .unwrap();
        let meeting = resolve_scope_results(
            &pool,
            "alpha",
            "alpha",
            10,
            ChatRetrievalScope::Meeting("m2".to_string()),
            None,
        )
        .await
        .unwrap();

        assert_eq!(all.len(), 3);
        assert_eq!(meeting.len(), 1);
        assert_eq!(meeting[0].meeting_id, "m2");
    }

    #[tokio::test]
    async fn meeting_context_uses_authoritative_sections_and_transcript_windows() {
        let pool = scope_pool().await;
        sqlx::query(
            "INSERT INTO meeting_notes (meeting_id, notes_markdown) VALUES ('m2', 'Current notes')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO summary_processes (meeting_id, template_id, updated_at, result) VALUES ('m2', 'old', '2026-01-01', '{\"markdown\":\"Authoritative summary\"}'), ('m2', 'new', '2026-01-02', '{\"markdown\":\"\"}'), ('m2', 'a', '2026-01-03', '{\"markdown\":\"Earlier tie\"}'), ('m2', 'z', '2026-01-03', '{\"markdown\":\"Latest tie\"}')")
            .execute(&pool).await.unwrap();
        for (id, text, time) in [
            ("before", "before", 1.0),
            ("hit", "needle", 2.0),
            ("after", "after", 3.0),
        ] {
            sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, 'm2', ?, ?, ?)")
                .bind(id).bind(text).bind(format!("{:02}:00", time as u32)).bind(time).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m2', 'summary', 's', 'needle'), ('m2', 'note', 'n', 'needle'), ('m2', 'transcript', 'hit', 'needle')")
            .execute(&pool).await.unwrap();

        let context = resolve_meeting_context(&pool, "m2", "needle", "needle", 10)
            .await
            .unwrap();
        assert_eq!(context.summary.as_deref(), Some("Latest tie"));
        assert_eq!(context.notes.as_deref(), Some("Current notes"));
        assert_eq!(
            context
                .transcripts
                .iter()
                .map(|row| row.chunk_id.as_str())
                .collect::<Vec<_>>(),
            vec!["before", "hit", "after"]
        );
    }

    #[tokio::test]
    async fn meeting_windows_deduplicate_and_use_stable_timing_order() {
        let pool = scope_pool().await;
        sqlx::query("DELETE FROM transcripts WHERE id = 't-m2'")
            .execute(&pool)
            .await
            .unwrap();
        for (id, text, time) in [
            ("a", "anchor", Some(1.0)),
            ("b", "anchor", Some(2.0)),
            ("c", "anchor", Some(2.0)),
            ("z", "last", None),
        ] {
            sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, 'm2', ?, '10:00', ?)")
                .bind(id).bind(text).bind(time).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m2', 'transcript', ?, ?)")
                .bind(id).bind(text).execute(&pool).await.unwrap();
        }
        let context = resolve_meeting_context(&pool, "m2", "anchor", "anchor", 10)
            .await
            .unwrap();
        assert_eq!(
            context
                .transcripts
                .iter()
                .map(|row| row.chunk_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c", "z"]
        );
    }

    #[tokio::test]
    async fn meeting_zero_hits_uses_bounded_chronological_fallback() {
        let pool = scope_pool().await;
        for (id, time) in [("first", 1.0), ("second", 2.0), ("third", 3.0)] {
            sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, 'm2', ?, '10:00', ?)")
                .bind(id).bind(id).bind(time).execute(&pool).await.unwrap();
        }
        let context = resolve_meeting_context(&pool, "m2", "missing", "missing", 2)
            .await
            .unwrap();
        assert_eq!(
            context
                .transcripts
                .iter()
                .map(|row| row.chunk_id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        let built = build_meeting_context_markdown(
            "m2",
            "Child",
            None,
            None,
            &context.transcripts,
            context.total_transcript_segments,
            1_000,
        );
        assert!(built.markdown.contains(&format!(
            "Partial transcript coverage: 2/{}",
            context.total_transcript_segments
        )));
    }

    #[tokio::test]
    async fn meeting_fts_failure_is_not_converted_to_fallback() {
        let pool = scope_pool().await;
        sqlx::query("DROP TABLE meeting_fts")
            .execute(&pool)
            .await
            .unwrap();
        let error = resolve_meeting_context(&pool, "m2", "missing", "missing", 2)
            .await
            .unwrap_err();
        assert!(error.contains("Search failed"));
    }

    #[tokio::test]
    async fn stale_fts_hits_use_the_bounded_chronological_fallback() {
        let pool = scope_pool().await;
        for (id, time) in [("first", 1.0), ("second", 2.0), ("third", 3.0)] {
            sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, 'm2', ?, '10:00', ?)")
                .bind(id).bind(id).bind(time).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m2', 'transcript', 'stale', 'needle')")
            .execute(&pool).await.unwrap();
        let context = resolve_meeting_context(&pool, "m2", "needle", "needle", 2)
            .await
            .unwrap();
        assert_eq!(
            context
                .transcripts
                .iter()
                .map(|row| row.chunk_id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
    }

    #[tokio::test]
    async fn ordinary_meeting_preparation_keeps_authoritative_context_and_source_parity() {
        let pool = scope_pool().await;
        sqlx::query("CREATE TABLE settings (id TEXT PRIMARY KEY, provider TEXT NOT NULL, model TEXT NOT NULL, whisperModel TEXT NOT NULL, groqApiKey TEXT, openaiApiKey TEXT, anthropicApiKey TEXT, ollamaApiKey TEXT, openRouterApiKey TEXT, ollamaEndpoint TEXT, customOpenAIConfig TEXT, customVocabulary TEXT, chatProvider TEXT, chatModel TEXT, chatOllamaEndpoint TEXT, force_lexical_retrieval BOOLEAN NOT NULL DEFAULT FALSE)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO settings (id, provider, model, whisperModel, chatProvider, chatModel) VALUES ('1', 'ollama', 'local', 'whisper', 'ollama', 'local')")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "UPDATE meetings SET title = ?, saved_at = '2026-01-01T00:00:00Z' WHERE id = 'm2'",
        )
        .bind("🦀".repeat(10_000))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO meeting_notes (meeting_id, notes_markdown) VALUES ('m2', 'Current notes')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO summary_processes (meeting_id, template_id, updated_at, result) VALUES ('m2', 'summary', '2026-01-01', '{\"markdown\":\"Authoritative summary\"}')")
            .execute(&pool).await.unwrap();
        let before = format!("before-{}", "b".repeat(30_000));
        let anchor = format!("needle-anchor-{}", "a".repeat(30_000));
        let after = format!("budget-excluded-{}", "c".repeat(30_000));
        for (id, transcript, start) in [
            ("before", before.as_str(), 1.0),
            ("anchor", anchor.as_str(), 2.0),
            ("after", after.as_str(), 3.0),
        ] {
            sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, 'm2', ?, '10:00', ?)")
                .bind(id).bind(transcript).bind(start).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m2', 'transcript', 'anchor', 'needle')")
            .execute(&pool).await.unwrap();

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "needle",
            None,
            ChatRetrievalScope::Meeting("m2".to_string()),
            None,
            None,
            None,
            Some(ChatRetrievalMode::Deep),
            None,
        )
        .await
        .unwrap();
        assert!(inputs.user_prompt.contains("Authoritative summary"));
        assert!(inputs.user_prompt.contains("Current notes"));
        assert!(inputs.user_prompt.contains("Partial transcript coverage"));
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.snippet.as_str())
                .collect::<Vec<_>>(),
            vec![before.as_str(), anchor.as_str()]
        );
        assert!(inputs.user_prompt.contains(&before));
        assert!(inputs.user_prompt.contains(&anchor));
        assert!(!inputs.user_prompt.contains(&after));
        assert!(inputs.sources.iter().all(|source| source.snippet != after));
    }

    #[test]
    fn meeting_prompt_keeps_retained_sources_and_coverage_under_unicode_budget() {
        let mut transcripts = vec![
            make_search_result("m1", "Meeting", "transcript", "🦀 first"),
            make_search_result("m1", "Meeting", "transcript", "é second"),
        ];
        transcripts[0].chunk_id = "first".to_string();
        transcripts[1].chunk_id = "second".to_string();
        let built = build_meeting_context_markdown(
            "m1",
            "Meeting",
            Some(&"🦀".repeat(100)),
            Some(&"é".repeat(100)),
            &transcripts,
            10,
            1_000,
        );
        let retained = built.retained_transcript_ids.iter().collect::<HashSet<_>>();
        let sources = transcripts
            .iter()
            .filter(|row| retained.contains(&row.chunk_id))
            .map(chat_source_from_result)
            .collect::<Vec<_>>();
        let prompt = assemble_prompt(&built.markdown, &[], "🦀?", "🦀?", "", 1_040);
        assert!(prompt.contains("Partial transcript coverage"));
        assert!(!sources.is_empty());
        assert!(sources
            .iter()
            .all(|source| prompt.contains(&source.snippet)));
    }

    #[tokio::test]
    async fn scope_resolution_retries_or_when_and_finds_nothing() {
        let pool = scope_pool().await;

        let results = resolve_scope_results(
            &pool,
            "alpha missing",
            "alpha missing",
            10,
            ChatRetrievalScope::All,
            None,
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn scope_resolution_nonempty_and_does_not_suppress_or_hits() {
        let pool = scope_pool().await;
        sqlx::query("UPDATE meeting_fts SET text = 'alpha beta' WHERE meeting_id = 'm1'")
            .execute(&pool)
            .await
            .unwrap();

        let all = resolve_scope_results(
            &pool,
            "alpha beta",
            "alpha beta",
            10,
            ChatRetrievalScope::All,
            None,
        )
        .await
        .unwrap();
        let folder = resolve_scope_results(
            &pool,
            "alpha beta",
            "alpha beta",
            10,
            ChatRetrievalScope::Folder("root".to_string()),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            all.iter()
                .map(|result| result.meeting_id.as_str())
                .collect::<HashSet<_>>(),
            HashSet::from(["m1", "m2", "m3"])
        );
        assert_eq!(all.len(), 3);
        assert_eq!(
            folder
                .iter()
                .map(|result| result.meeting_id.as_str())
                .collect::<HashSet<_>>(),
            HashSet::from(["m1", "m2"])
        );
        assert_eq!(folder.len(), 2);
    }

    #[tokio::test]
    async fn scope_resolution_overfetches_past_strict_or_overlap() {
        let pool = scope_pool().await;
        sqlx::query("DELETE FROM meeting_fts")
            .execute(&pool)
            .await
            .unwrap();
        for index in 0..4 {
            sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m1', 'note', ?, 'alpha beta')")
                .bind(format!("strict-{index}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m2', 'note', 'or-only', 'alpha')")
            .execute(&pool)
            .await
            .unwrap();

        let capped_or = search_scope(
            &pool,
            "alpha beta",
            4,
            &ChatRetrievalScope::All,
            MatchMode::Or,
        )
        .await
        .unwrap();
        assert!(capped_or.iter().all(|result| result.meeting_id == "m1"));

        let results = resolve_scope_results(
            &pool,
            "alpha beta",
            "alpha beta",
            4,
            ChatRetrievalScope::All,
            None,
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 4);
        assert!(results.iter().any(|result| result.meeting_id == "m2"));
    }

    #[tokio::test]
    async fn scope_resolution_full_rewrite_results_do_not_suppress_original_query() {
        let pool = scope_pool().await;
        sqlx::query("DELETE FROM meeting_fts")
            .execute(&pool)
            .await
            .unwrap();
        for index in 0..4 {
            sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m1', 'note', ?, 'alpha beta')")
                .bind(format!("strict-{index}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m2', 'note', 'original', 'gamma')")
            .execute(&pool)
            .await
            .unwrap();

        let results = resolve_scope_results(
            &pool,
            "alpha beta",
            "gamma",
            4,
            ChatRetrievalScope::All,
            None,
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 4);
        assert!(results.iter().any(|result| result.meeting_id == "m2"));
    }

    #[tokio::test]
    async fn folder_scope_includes_stable_id_descendants() {
        let pool = scope_pool().await;
        let results = resolve_scope_results(
            &pool,
            "alpha",
            "alpha",
            10,
            ChatRetrievalScope::Folder("root".to_string()),
            None,
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(
            results
                .iter()
                .map(|result| result.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m1", "m2"]
        );
    }

    #[tokio::test]
    async fn folder_scope_uses_meeting_assignment_when_fts_metadata_is_stale() {
        let pool = scope_pool().await;
        sqlx::query("UPDATE meeting_fts SET folder_id = 'other' WHERE meeting_id = 'm1'")
            .execute(&pool)
            .await
            .unwrap();

        let results = resolve_scope_results(
            &pool,
            "alpha",
            "alpha",
            10,
            ChatRetrievalScope::Folder("root".to_string()),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            results
                .iter()
                .map(|result| result.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m1", "m2"]
        );
    }

    #[tokio::test]
    async fn folder_meeting_list_uses_metadata_without_fts_content() {
        let pool = scope_pool().await;

        let titles = meeting_titles_for_scope(
            &pool,
            &ChatRetrievalScope::Folder("root".to_string()),
            "listar reuniões existentes nesta pasta",
            None,
        )
        .await
        .unwrap();

        assert_eq!(titles, vec!["Root", "Child"]);
        let context = format_meeting_list_context(&titles);
        assert!(context.contains("2 total"));
        assert!(context.contains("- Root"));
        assert!(context.contains("- Child"));
        assert!(!context.contains("Other"));
        assert!(requests_meeting_list(
            "listar reuniões existentes nesta pasta"
        ));
        assert!(!requests_meeting_list(
            "mostre a reunião onde discutimos retenção"
        ));
        assert!(!requests_meeting_list("which meeting discussed retention"));
    }

    #[tokio::test]
    async fn all_meetings_list_uses_folder_named_in_query() {
        let pool = scope_pool().await;

        let titles = meeting_titles_for_scope(
            &pool,
            &ChatRetrievalScope::All,
            "listar reuniões na pasta Child",
            None,
        )
        .await
        .unwrap();

        assert_eq!(titles, vec!["Child"]);
    }

    #[tokio::test]
    async fn frozen_snapshot_rehydrates_displayed_meetings_without_searching_query() {
        let pool = scope_pool().await;
        let results = resolve_scope_results(
            &pool,
            "missing",
            "missing",
            10,
            ChatRetrievalScope::SearchSnapshot(vec!["m2".to_string(), "m1".to_string()]),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            results
                .iter()
                .map(|result| result.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m2", "m1"]
        );
    }

    #[tokio::test]
    async fn snapshot_resume_tolerates_deleted_member_and_retrieves_survivors() {
        let pool = scope_pool().await;
        sqlx::query(
            r#"
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
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE UNIQUE INDEX idx_chat_conversations_scope_identity ON chat_conversations(scope_kind, scope_key, COALESCE(scope_data, ''))",
        )
        .execute(&pool)
        .await
        .unwrap();

        let scope = ChatScope {
            kind: ChatScopeKind::SearchSnapshot,
            key: "snapshot".to_string(),
            data: Some(ChatScopeData {
                result_ids: vec!["m1".to_string(), "m2".to_string(), "m3".to_string()],
            }),
        };

        // Creation: all members exist, so strict membership validation passes.
        validate_search_snapshot_membership(&pool, &scope)
            .await
            .unwrap();
        let conversation = ChatRepository::get_or_create_scoped_conversation(&pool, &scope, None)
            .await
            .unwrap();

        // A member meeting is deleted (along with its FTS chunks).
        sqlx::query("DELETE FROM meetings WHERE id = 'm2'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM meeting_fts WHERE meeting_id = 'm2'")
            .execute(&pool)
            .await
            .unwrap();

        // Resume: the exact-scope thread still exists, so the deleted member is tolerated.
        validate_search_snapshot_membership(&pool, &scope)
            .await
            .unwrap();
        let resumed = ChatRepository::get_or_create_scoped_conversation(&pool, &scope, None)
            .await
            .unwrap();
        assert_eq!(resumed.id, conversation.id);

        // Retrieval rehydrates surviving members only.
        let results = resolve_scope_results(
            &pool,
            "unused",
            "unused",
            10,
            ChatRetrievalScope::SearchSnapshot(scope.data.unwrap().result_ids),
            None,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| result.meeting_id != "m2"));
    }

    #[tokio::test]
    async fn todays_meetings_are_date_filtered_and_intersected_with_scope() {
        let pool = scope_pool().await;
        let now = Utc::now();
        for (meeting_id, created_at) in [
            ("m1", now),
            ("m2", now),
            ("m3", now - chrono::Duration::days(2)),
        ] {
            sqlx::query("UPDATE meetings SET created_at = ? WHERE id = ?")
                .bind(created_at)
                .bind(meeting_id)
                .execute(&pool)
                .await
                .unwrap();
        }

        assert!(requests_todays_meetings("Summarize today's meetings"));
        assert!(requests_meeting_list("list today's meetings"));
        assert!(!requests_todays_meetings(
            "Which action items are due today?"
        ));

        let all_ids =
            meeting_ids_for_local_date(&pool, &ChatRetrievalScope::All, Local::now().date_naive())
                .await
                .unwrap();
        let folder_ids = meeting_ids_for_local_date(
            &pool,
            &ChatRetrievalScope::Folder("root".to_string()),
            Local::now().date_naive(),
        )
        .await
        .unwrap();
        let snapshot_ids = meeting_ids_for_local_date(
            &pool,
            &ChatRetrievalScope::SearchSnapshot(vec!["m2".to_string(), "m3".to_string()]),
            Local::now().date_naive(),
        )
        .await
        .unwrap();

        assert_eq!(all_ids, vec!["m1", "m2"]);
        assert_eq!(folder_ids, vec!["m1", "m2"]);
        assert_eq!(snapshot_ids, vec!["m2"]);

        let all_titles = meeting_titles_for_scope(
            &pool,
            &ChatRetrievalScope::All,
            "list today's meetings",
            Some(Local::now().date_naive()),
        )
        .await
        .unwrap();
        let folder_titles = meeting_titles_for_scope(
            &pool,
            &ChatRetrievalScope::Folder("root".to_string()),
            "list today's meetings",
            Some(Local::now().date_naive()),
        )
        .await
        .unwrap();
        let snapshot_titles = meeting_titles_for_scope(
            &pool,
            &ChatRetrievalScope::SearchSnapshot(vec!["m2".to_string(), "m3".to_string()]),
            "list today's meetings",
            Some(Local::now().date_naive()),
        )
        .await
        .unwrap();

        assert_eq!(all_titles, vec!["Root", "Child"]);
        assert_eq!(folder_titles, vec!["Root", "Child"]);
        assert_eq!(snapshot_titles, vec!["Child"]);

        let results = resolve_scope_results(
            &pool,
            "words do not need to match",
            "words do not need to match",
            10,
            ChatRetrievalScope::All,
            Some(&all_ids),
        )
        .await
        .unwrap();
        assert_eq!(
            results
                .iter()
                .map(|result| result.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m1", "m2"]
        );
    }

    #[test]
    fn persisted_snapshot_rejects_invalid_or_oversized_ids() {
        let conversation = ChatConversation {
            id: "conversation".to_string(),
            meeting_id: None,
            origin: "search_snapshot".to_string(),
            scope_kind: "search_snapshot".to_string(),
            scope_key: "snapshot".to_string(),
            scope_data: Some(
                serde_json::to_string(&ChatScopeData {
                    result_ids: vec!["bad id".to_string()],
                })
                .unwrap(),
            ),
            promoted_from_live_scope_key: None,
            title: None,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };
        assert!(retrieval_scope_from_conversation(&conversation).is_err());

        let oversized = ChatScope {
            kind: ChatScopeKind::SearchSnapshot,
            key: "snapshot".to_string(),
            data: Some(ChatScopeData {
                result_ids: (0..101).map(|index| format!("c{}", index)).collect(),
            }),
        };
        assert!(oversized.validate().is_err());

        let duplicate = ChatScope {
            kind: ChatScopeKind::SearchSnapshot,
            key: "snapshot".to_string(),
            data: Some(ChatScopeData {
                result_ids: vec!["c1".to_string(), "c1".to_string()],
            }),
        };
        assert!(duplicate.validate().is_err());
    }

    #[test]
    fn broad_context_produces_sources_only_from_retained_results() {
        let results = vec![
            make_search_result(
                "m1",
                "Sprint Planning",
                "transcript",
                "We decided to ship FTS5.",
            ),
            make_search_result("m1", "Sprint Planning", "summary", "Summary of sprint."),
            make_search_result("m2", "Retro", "transcript", "Team velocity is improving."),
        ];

        let built = build_context_markdown_with_limit(&results, 250);
        let retained = built
            .retained_evidence_ids
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let sources: Vec<ChatSource> = results
            .iter()
            .filter(|result| retained.contains(&lexical_evidence_id(result)))
            .map(chat_source_from_result)
            .collect();

        assert!(!sources.is_empty());
        assert!(sources.len() < results.len());
        assert_eq!(sources[0].meeting_title, "Sprint Planning");
        assert!(sources
            .iter()
            .all(|source| built.markdown.contains(&source.snippet)));
    }

    #[tokio::test]
    async fn broad_preparation_emits_only_sources_delivered_to_model() {
        let pool = scope_pool().await;
        sqlx::query("CREATE TABLE settings (id TEXT PRIMARY KEY, provider TEXT NOT NULL, model TEXT NOT NULL, whisperModel TEXT NOT NULL, groqApiKey TEXT, openaiApiKey TEXT, anthropicApiKey TEXT, ollamaApiKey TEXT, openRouterApiKey TEXT, ollamaEndpoint TEXT, customOpenAIConfig TEXT, customVocabulary TEXT, chatProvider TEXT, chatModel TEXT, chatOllamaEndpoint TEXT, force_lexical_retrieval BOOLEAN NOT NULL DEFAULT FALSE)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO settings (id, provider, model, whisperModel, chatProvider, chatModel) VALUES ('1', 'ollama', 'local', 'whisper', 'ollama', 'local')")
            .execute(&pool).await.unwrap();
        sqlx::query("UPDATE settings SET force_lexical_retrieval = TRUE WHERE id = '1'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE meetings SET saved_at = '2026-08-22T00:00:00Z'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE meetings SET title = ? WHERE id = 'm1'")
            .bind("🦀".repeat(64_000))
            .execute(&pool)
            .await
            .unwrap();

        let progress: Arc<Mutex<Vec<crate::retrieval::agent::DeepProgressEvent>>> =
            Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let sink_events = Arc::clone(&progress);
            move |event: crate::retrieval::agent::DeepProgressEvent| {
                sink_events.lock().unwrap().push(event);
            }
        };

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "alpha",
            None,
            ChatRetrievalScope::All,
            None,
            None,
            None,
            Some(ChatRetrievalMode::Deep),
            Some(&sink),
        )
        .await
        .unwrap();

        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::ForcedLexical
        );
        assert_eq!(inputs.retrieval_mode, ChatRetrievalMode::Fast);
        // Forced lexical suppresses Deep: no planner runs, no progress emits.
        assert!(progress.lock().unwrap().is_empty());
        assert_eq!(inputs.provider_round_trips, 0);
        assert!(!inputs.sources.is_empty());
        assert!(inputs
            .sources
            .iter()
            .all(|source| source.meeting_id != "m1"));
        assert!(inputs
            .sources
            .iter()
            .all(|source| inputs.user_prompt.contains(&source.snippet)));
        assert!(inputs.user_prompt.chars().count() <= 64_000);
        let serialized = crate::mcp::server::serialize_chat_sources(&inputs.sources).unwrap();
        assert!(serialized
            .iter()
            .all(|source| source.get("sourceKind").is_none()));
    }

    #[test]
    fn deep_preparation_eligibility_requires_deep_mode_and_no_today_allow_list() {
        assert!(deep_preparation_eligible(ChatRetrievalMode::Deep, None));
        // A derived today allow-list (snapshot/today/saved-meeting members)
        // stays Fast until Tasks 4.3/4.4 extend the Deep lifecycle.
        assert!(!deep_preparation_eligible(
            ChatRetrievalMode::Deep,
            Some(&vec!["m1".to_string()])
        ));
        assert!(!deep_preparation_eligible(ChatRetrievalMode::Fast, None));
        // Omitted mode resolves to Fast.
        assert!(!deep_preparation_eligible(
            ChatRetrievalMode::default(),
            None
        ));
    }

    #[tokio::test]
    async fn fast_preparation_makes_no_deep_progress_and_counts_no_round_trips() {
        let pool = scope_pool().await;
        sqlx::query("CREATE TABLE settings (id TEXT PRIMARY KEY, provider TEXT NOT NULL, model TEXT NOT NULL, whisperModel TEXT NOT NULL, groqApiKey TEXT, openaiApiKey TEXT, anthropicApiKey TEXT, ollamaApiKey TEXT, openRouterApiKey TEXT, ollamaEndpoint TEXT, customOpenAIConfig TEXT, customVocabulary TEXT, chatProvider TEXT, chatModel TEXT, chatOllamaEndpoint TEXT, force_lexical_retrieval BOOLEAN NOT NULL DEFAULT FALSE)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO settings (id, provider, model, whisperModel, chatProvider, chatModel) VALUES ('1', 'ollama', 'local', 'whisper', 'ollama', 'local')")
            .execute(&pool).await.unwrap();
        sqlx::query("UPDATE meetings SET saved_at = '2026-08-22T00:00:00Z'")
            .execute(&pool)
            .await
            .unwrap();

        let progress: Arc<Mutex<Vec<crate::retrieval::agent::DeepProgressEvent>>> =
            Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let sink_events = Arc::clone(&progress);
            move |event: crate::retrieval::agent::DeepProgressEvent| {
                sink_events.lock().unwrap().push(event);
            }
        };

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "alpha",
            None,
            ChatRetrievalScope::All,
            None,
            None,
            None,
            None,
            Some(&sink),
        )
        .await
        .unwrap();

        assert_eq!(inputs.retrieval_mode, ChatRetrievalMode::Fast);
        // Fast never constructs the planner path or emits preparation progress.
        assert!(progress.lock().unwrap().is_empty());
        assert_eq!(inputs.provider_round_trips, 0);
        assert!(!inputs.sources.is_empty());
    }

    #[test]
    fn user_prompt_includes_history_and_query() {
        let history = vec![
            ChatMessage {
                role: "user".into(),
                content: "What did we discuss?".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "You discussed FTS5.".into(),
            },
        ];

        let context = "# Meeting Context\n\nSome context here.\n";
        let query = "What about next steps?";

        let user_prompt = assemble_prompt(context, &history, query, query, "", 64_000);

        assert!(user_prompt.contains("user: What did we discuss?"));
        assert!(user_prompt.contains("assistant: You discussed FTS5."));
        assert!(user_prompt.contains("What about next steps?"));
        assert!(user_prompt.contains("Some context here."));
    }

    #[test]
    fn user_prompt_preserves_question_when_history_exceeds_budget() {
        let history = vec![ChatMessage {
            role: "assistant".into(),
            content: "x".repeat(200_000),
        }];
        let query = "What did we decide?";
        let question_block = format!("\nUser question: {}\nSearch query: {}\n", query, query);

        let user_prompt = assemble_prompt("context", &history, query, query, "", 64_000);

        assert!(user_prompt.ends_with(&question_block));
        assert!(user_prompt.contains(query));
    }

    #[tokio::test]
    async fn temporal_context_uses_latest_saved_meeting_within_scope() {
        let pool = scope_pool().await;
        for (id, saved_at) in [
            ("m1", "2026-08-16T10:00:00Z"),
            ("m2", "2026-08-18T09:00:00Z"),
            ("m3", "2026-08-17T12:00:00Z"),
        ] {
            sqlx::query("UPDATE meetings SET saved_at = ? WHERE id = ?")
                .bind(saved_at)
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }

        let all = temporal_context_for_scope(&pool, &ChatRetrievalScope::All)
            .await
            .unwrap();
        let folder =
            temporal_context_for_scope(&pool, &ChatRetrievalScope::Folder("root".to_string()))
                .await
                .unwrap();
        let snapshot = temporal_context_for_scope(
            &pool,
            &ChatRetrievalScope::SearchSnapshot(vec!["m1".to_string(), "m3".to_string()]),
        )
        .await
        .unwrap();

        assert!(all.contains("Child"));
        assert!(folder.contains("Child"));
        assert!(snapshot.contains("Other"));
    }

    #[test]
    fn temporal_context_injects_an_authoritative_local_date() {
        let now = DateTime::parse_from_rfc3339("2026-08-18T14:30:00+00:00")
            .unwrap()
            .with_timezone(&Local);
        let latest = LatestSavedMeeting {
            title: "Imported notes".to_string(),
            saved_at: DateTime::parse_from_rfc3339("2026-08-18T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        };

        let context = format_temporal_context(now, Some(&latest));

        assert!(context.contains("Current local date: 2026-08-18"));
        assert!(
            context.contains("Most recently saved/imported meeting in this scope: Imported notes")
        );
    }

    #[test]
    fn rewrite_gate_requires_short_query_and_history() {
        let history = vec![
            ChatMessage {
                role: "user".into(),
                content: "What did we decide?".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "We decided to ship FTS5.".into(),
            },
        ];

        assert!(!should_rewrite_query(None, "What about next steps?"));
        assert!(should_rewrite_query(
            Some(&history),
            "What about next steps?"
        ));
        assert!(!should_rewrite_query(Some(&history), &"x".repeat(100)));
    }

    #[test]
    fn chat_source_serialization() {
        let source = ChatSource {
            meeting_id: "m1".to_string(),
            meeting_title: "Planning".to_string(),
            chunk_type: "transcript".to_string(),
            snippet: "Hello world".to_string(),
            folder_name: "Alpha".to_string(),
            source_kind: None,
        };
        let json = serde_json::to_string(&source).unwrap();
        assert!(json.contains("\"meetingId\":\"m1\""));
        assert!(json.contains("\"chunkType\":\"transcript\""));
        assert!(json.contains("\"folderName\":\"Alpha\""));
    }

    #[test]
    fn chat_response_serialization() {
        let response = ChatResponse {
            answer: "The team decided to ship FTS5 first.".to_string(),
            sources: vec![ChatSource {
                meeting_id: "m1".to_string(),
                meeting_title: "Planning".to_string(),
                chunk_type: "transcript".to_string(),
                snippet: "FTS5 decision".to_string(),
                folder_name: "General".to_string(),
                source_kind: None,
            }],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("FTS5 first"));
        assert!(json.contains("\"sources\""));
    }

    #[test]
    fn live_snapshot_uses_current_transcript_and_non_navigable_source() {
        let segments = vec![crate::audio::recording_saver::TranscriptSegment {
            id: "segment-1".into(),
            text: "We will ship on Friday.".into(),
            audio_start_time: 1.0,
            audio_end_time: 2.0,
            duration: 1.0,
            display_time: "[00:01]".into(),
            confidence: 0.9,
            sequence_id: 1,
        }];

        let (context, sources) = live_snapshot_context(&segments, "live-1", 64_000);

        assert!(context.contains("[00:01] We will ship on Friday."));
        assert_eq!(sources[0].meeting_id, "live-1");
        assert_eq!(sources[0].source_kind.as_deref(), Some("live_recording"));
        assert_eq!(sources[0].chunk_type, "live_transcript");
    }

    #[test]
    fn live_snapshot_budget_keeps_the_most_recent_tail() {
        let segments = ["one", "two", "three"]
            .into_iter()
            .enumerate()
            .map(
                |(index, text)| crate::audio::recording_saver::TranscriptSegment {
                    id: format!("segment-{}", index),
                    text: text.to_string(),
                    audio_start_time: index as f64,
                    audio_end_time: index as f64 + 1.0,
                    duration: 1.0,
                    display_time: format!("[00:0{}]", index + 1),
                    confidence: 0.9,
                    sequence_id: index as u64,
                },
            )
            .collect::<Vec<_>>();

        // Joined transcript "[00:01] one\n[00:02] two\n[00:03] three" is 37
        // chars; a 14-char budget fits only the last segment.
        let (context, _) = live_snapshot_context(&segments, "live-1", 14);

        assert!(context.contains("three"));
        assert!(!context.contains("two"));
        assert!(!context.contains("one"));
    }

    fn live_authorization(active_scope_key: &str, consent: bool) -> LiveTranscriptAuthorization {
        LiveTranscriptAuthorization {
            active_scope_key: Some(active_scope_key.to_string()),
            consent,
        }
    }

    #[test]
    fn live_scope_must_match_active_native_recording() {
        let error = authorize_live_transcript(
            &ChatRetrievalScope::LiveRecording("persisted-live".to_string()),
            Some(&live_authorization("active-live", true)),
            "ollama",
        )
        .unwrap_err();

        assert!(error.contains("does not match"));
    }

    #[test]
    fn local_live_provider_is_allowed_without_consent() {
        assert!(authorize_live_transcript(
            &ChatRetrievalScope::LiveRecording("live-1".to_string()),
            Some(&live_authorization("live-1", false)),
            "ollama",
        )
        .is_ok());
    }

    #[test]
    fn remote_live_provider_is_rejected_without_consent() {
        let error = authorize_live_transcript(
            &ChatRetrievalScope::LiveRecording("live-1".to_string()),
            Some(&live_authorization("live-1", false)),
            "openai",
        )
        .unwrap_err();

        assert!(error.contains("consent is required"));
        assert!(authorize_live_transcript(
            &ChatRetrievalScope::LiveRecording("live-1".to_string()),
            Some(&live_authorization("live-1", false)),
            "unknown-future-provider",
        )
        .is_err());
    }

    #[test]
    fn remote_live_provider_is_allowed_with_consent() {
        assert!(authorize_live_transcript(
            &ChatRetrievalScope::LiveRecording("live-1".to_string()),
            Some(&live_authorization("live-1", true)),
            "custom-openai",
        )
        .is_ok());
    }

    #[test]
    fn restart_issued_live_key_cannot_serve_prior_thread() {
        let first_key = crate::audio::recording_commands::issue_live_transcript_scope_key();
        // The prior thread's authorization passes while its key is still active.
        assert!(authorize_live_transcript(
            &ChatRetrievalScope::LiveRecording(first_key.clone()),
            Some(&live_authorization(&first_key, true)),
            "ollama",
        )
        .is_ok());

        // Recording restarts before the transcript read: a fresh key replaces the old one.
        crate::audio::recording_commands::issue_live_transcript_scope_key();

        let error = ensure_live_scope_matches_active_recording(&first_key).unwrap_err();
        assert!(error.contains("does not match"));
    }

    #[tokio::test]
    async fn non_streaming_scoped_preparation_enforces_live_consent() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::query(
            r#"
            CREATE TABLE chat_conversations (
                id TEXT PRIMARY KEY, meeting_id TEXT, origin TEXT NOT NULL,
                scope_kind TEXT NOT NULL, scope_key TEXT NOT NULL, scope_data TEXT,
                promoted_from_live_scope_key TEXT,
                title TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE settings (
                id TEXT PRIMARY KEY, provider TEXT NOT NULL, model TEXT NOT NULL,
                whisperModel TEXT NOT NULL, groqApiKey TEXT, openaiApiKey TEXT,
                anthropicApiKey TEXT, ollamaApiKey TEXT, openRouterApiKey TEXT,
                ollamaEndpoint TEXT, customOpenAIConfig TEXT, customVocabulary TEXT,
                chatProvider TEXT, chatModel TEXT, chatOllamaEndpoint TEXT
            );
            INSERT INTO chat_conversations VALUES
                ('conversation', NULL, 'live_recording', 'live_recording', 'live-1', NULL, NULL, NULL, 'now', 'now');
            INSERT INTO settings (id, provider, model, whisperModel, chatProvider, chatModel)
                VALUES ('1', 'ollama', 'local', 'whisper', 'openai', 'remote');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let error = prepare_scoped_chat_inputs_with_authorization(
            &pool,
            None,
            &reqwest::Client::new(),
            "question",
            None,
            "conversation",
            live_authorization("live-1", false),
            None,
            None,
            None,
            None,
        )
        .await
        .err()
        .unwrap();

        assert!(error.contains("consent is required"));
    }

    #[tokio::test]
    async fn cancel_chat_stream_cancels_matching_id() {
        let state = ChatStreamState::new();
        let token = claim_chat_stream(&state, "s1").await;
        cancel_chat_stream(&state, Some("s1")).await;
        assert!(token.is_cancelled());
        assert_eq!(state.request_count(), 0);
    }

    #[tokio::test]
    async fn cancel_chat_stream_ignores_mismatched_id() {
        let state = ChatStreamState::new();
        let token = claim_chat_stream(&state, "s1").await;
        cancel_chat_stream(&state, Some("s2")).await;
        assert!(!token.is_cancelled());
        assert_eq!(state.request_count(), 1);
    }

    #[tokio::test]
    async fn cancel_chat_stream_any_cancels_active_stream() {
        let state = ChatStreamState::new();
        let token = claim_chat_stream(&state, "s1").await;
        cancel_chat_stream(&state, None).await;
        assert!(token.is_cancelled());
        assert_eq!(state.request_count(), 0);
    }

    #[tokio::test]
    async fn cancelled_stream_publication_boundary_emits_no_start_source_done_or_answer() {
        let state = ChatStreamState::new();
        let token = claim_chat_stream(&state, "s1").await;
        let mut events = Vec::new();
        token.cancel();
        assert!(!publish_chat_stream_event_if_owner(
            &state,
            "s1",
            &token,
            "chat-stream-start",
            serde_json::json!({"sources": [{"sourceKind": "transcript"}]}),
            false,
            |event, payload| events.push((event.to_string(), payload)),
        ));
        let done_token = claim_chat_stream(&state, "s2").await;
        done_token.cancel();
        assert!(!publish_chat_stream_event_if_owner(
            &state,
            "s2",
            &done_token,
            "chat-stream-done",
            serde_json::json!({"answer": "partial", "sources": [{"sourceKind": "transcript"}]}),
            true,
            |event, payload| events.push((event.to_string(), payload)),
        ));
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn delayed_old_preparation_cannot_reclaim_or_clear_new_stream() {
        let state = ChatStreamState::new();
        let old_token = claim_chat_stream(&state, "old").await;
        let preparation_done = Arc::new(tokio::sync::Notify::new());
        let delayed_state = state.clone();
        let delayed_done = preparation_done.clone();
        let delayed_old_token = old_token.clone();
        let old_work = tokio::spawn(async move {
            delayed_done.notified().await;
            !delayed_old_token.is_cancelled()
                && is_chat_stream_owner(&delayed_state, "old", &delayed_old_token).await
        });

        let new_token = claim_chat_stream(&state, "new").await;
        preparation_done.notify_one();

        assert!(old_token.is_cancelled());
        assert!(!old_work.await.unwrap());
        clear_chat_stream_if_owner(&state, "old", &old_token).await;
        assert!(is_chat_stream_owner(&state, "new", &new_token).await);
        assert!(!new_token.is_cancelled());
    }

    #[tokio::test]
    async fn reused_stream_id_cannot_clear_replacement() {
        let state = ChatStreamState::new();
        let old_token = claim_chat_stream(&state, "same").await;
        let new_token = claim_chat_stream(&state, "same").await;

        assert!(old_token.is_cancelled());
        assert!(!is_chat_stream_owner(&state, "same", &old_token).await);
        clear_chat_stream_if_owner(&state, "same", &old_token).await;
        assert!(is_chat_stream_owner(&state, "same", &new_token).await);
        assert_eq!(state.request_count(), 1);
    }

    #[test]
    fn request_ownership_allows_chat_and_sidebar_to_coexist() {
        let state = ChatRequestState::new();
        let old_chat = state.claim_request(ChatRequestSurface::Chat, "chat-old");
        let sidebar = state.claim_request(ChatRequestSurface::Sidebar, "sidebar");
        let new_chat = state.claim_request(ChatRequestSurface::Chat, "chat-new");

        assert!(old_chat.is_cancelled());
        assert!(!sidebar.is_cancelled());
        assert!(!state.is_owner(ChatRequestSurface::Chat, "chat-old", &old_chat));
        assert!(state.is_owner(ChatRequestSurface::Chat, "chat-new", &new_chat));
        assert!(state.is_owner(ChatRequestSurface::Sidebar, "sidebar", &sidebar));
        assert_eq!(state.request_count(), 2);
    }

    #[test]
    fn replaced_or_cancelled_progress_cannot_publish_and_cleanup_stays_bounded() {
        let state = ChatRequestState::new();
        let old = state.claim_request(ChatRequestSurface::Chat, "old");
        let current = state.claim_request(ChatRequestSurface::Chat, "current");
        let mut events = Vec::new();

        assert!(!publish_chat_stream_event_if_owner(
            &state,
            "old",
            &old,
            "chat-preparation-progress",
            serde_json::json!({"stage": "initial_retrieval", "completed": 1, "total": 1}),
            false,
            |event, payload| events.push((event.to_string(), payload)),
        ));
        assert!(publish_chat_stream_event_if_owner(
            &state,
            "current",
            &current,
            "chat-preparation-progress",
            serde_json::json!({"stage": "initial_retrieval", "completed": 1, "total": 1}),
            false,
            |event, payload| events.push((event.to_string(), payload)),
        ));
        let foreign = Arc::new(CancellationToken::new());
        assert!(!publish_chat_stream_event_if_owner(
            &state,
            "current",
            &foreign,
            "chat-preparation-progress",
            serde_json::json!({"stage": "answer_generation", "completed": 0, "total": 1}),
            false,
            |event, payload| events.push((event.to_string(), payload)),
        ));
        assert!(state.is_owner(ChatRequestSurface::Chat, "current", &current));
        state.cancel_request(ChatRequestSurface::Chat, Some("current"));
        assert!(!publish_chat_stream_event_if_owner(
            &state,
            "current",
            &current,
            "chat-preparation-progress",
            serde_json::json!({"stage": "answer_generation", "completed": 0, "total": 1}),
            false,
            |event, payload| events.push((event.to_string(), payload)),
        ));
        assert_eq!(events.len(), 1);
        assert_eq!(state.request_count(), 0);

        for index in 0..64 {
            let id = format!("terminal-{index}");
            let token = state.claim_request(ChatRequestSurface::Chat, &id);
            assert!(state.clear_if_owner(ChatRequestSurface::Chat, &id, &token));
        }
        assert_eq!(state.request_count(), 0);
    }

    #[test]
    fn non_streaming_error_and_timeout_cleanup_release_request_ownership() {
        let state = ChatRequestState::new();
        let error_token = state.claim_request(ChatRequestSurface::Chat, "error");
        let error = finish_non_streaming_chat_request(
            &state,
            "error",
            &error_token,
            Ok(Err("provider failed".to_string())),
        )
        .unwrap_err();
        assert_eq!(error, "provider failed");
        assert_eq!(state.request_count(), 0);

        let success_token = state.claim_request(ChatRequestSurface::Chat, "success");
        let response = finish_non_streaming_chat_request(
            &state,
            "success",
            &success_token,
            Ok(Ok(ChatResponse {
                answer: "answer".to_string(),
                sources: Vec::new(),
            })),
        )
        .unwrap();
        assert_eq!(response.answer, "answer");
        assert_eq!(state.request_count(), 0);

        let old_token = state.claim_request(ChatRequestSurface::Chat, "old");
        let new_token = state.claim_request(ChatRequestSurface::Chat, "new");
        let superseded = finish_non_streaming_chat_request(
            &state,
            "old",
            &old_token,
            Ok(Ok(ChatResponse {
                answer: "stale".to_string(),
                sources: Vec::new(),
            })),
        )
        .unwrap_err();
        assert!(superseded.contains("superseded"));
        assert!(state.is_owner(ChatRequestSurface::Chat, "new", &new_token));
        assert!(state.clear_if_owner(ChatRequestSurface::Chat, "new", &new_token));
        assert_eq!(state.request_count(), 0);

        let timeout_token = state.claim_request(ChatRequestSurface::Chat, "timeout");
        timeout_token.cancel();
        assert!(state.clear_if_owner(ChatRequestSurface::Chat, "timeout", &timeout_token));
        assert_eq!(state.request_count(), 0);
    }
}
