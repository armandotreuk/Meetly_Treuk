use chrono::{DateTime, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio_util::sync::CancellationToken;
use tracing::info;
use uuid::Uuid;

use crate::retrieval::worker::RetrievalLifecycle;
use crate::retrieval::{
    agent::DeepProgressCallback, hydrate_broad_scope_context, hydrate_context,
    hydrate_context_with_broad_coverage, PersistedRetrievalScope, RetrievalChannel,
    RetrievalLimits, RetrievalPurpose, RetrievalRequest, RetrievalService, SemanticFallbackReason,
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
// (repositories/chat.rs MAX_SEARCH_SNAPSHOT_RESULTS); query-aware paths use bounded
// FTS ranking while broad/no-hit fallback keeps deterministic per-meeting coverage.
const SNAPSHOT_REHYDRATION_CHUNK_CAP: u32 = 100;
pub(crate) const CHAT_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const MEETING_LIST_HEADER: &str = "Bounded rendered meeting-list context for the current scope. Answer only with the titles shown below; do not say that meeting content was unavailable.\n";
const CHAT_CONTEXT_REVALIDATION_ERROR: &str = "The chat context could not be revalidated safely.";

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
    /// Every meeting whose data/metadata is retained anywhere in the final
    /// prompt (sources, retained meeting context, meeting-list titles, or
    /// temporal latest-meeting titles). Deletion invalidation
    /// binds THIS set — not only the source IDs — so a meeting deleted after
    /// preparation can never answer from or disclose its retained metadata.
    pub prompt_meeting_ids: HashSet<String>,
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
    id: String,
    title: String,
    saved_at: DateTime<Utc>,
}

struct TemporalPromptContext {
    context: String,
    meeting_id: Option<String>,
}

struct ListedMeeting {
    id: String,
    title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ChatRequestSurface {
    Chat,
    Sidebar,
    /// Unauthenticated localhost MCP chat requests: independent internal
    /// identities through the SAME registry/mechanism (no public cancel API,
    /// no second registry). MCP claims are admitted up to
    /// [`MAX_CONCURRENT_MCP_REQUESTS`] concurrently — a request beyond the
    /// cap is rejected before any work — and each admitted request cleans up
    /// only its own entry on every terminal path.
    Mcp,
}

/// Internal admission cap for concurrent MCP chat requests (architecture:
/// MCP is strictly bounded and has no public client cancellation). Rejected
/// excess requests receive [`MCP_CHAT_BUSY_ERROR`] before any preparation or
/// provider work; admitted requests reclaim capacity on success, error,
/// timeout, and deletion cancellation. Deliberately small: Fast-only,
/// unauthenticated localhost surface.
pub(crate) const MAX_CONCURRENT_MCP_REQUESTS: usize = 4;

/// Stable error returned when an MCP chat request is rejected by the
/// concurrent-admission cap.
pub(crate) const MCP_CHAT_BUSY_ERROR: &str =
    "MCP chat is at its concurrent request limit; retry shortly";

/// One registered request: its ownership token plus the meeting identities
/// whose evidence it prepared. Binding happens once after preparation, so the
/// real meeting-deletion path can invalidate the request through this same
/// registry — no second registry or tombstone store exists. Registrations are
/// removed on replacement, cancellation, and completion. An invalidated
/// stream remains only until its ownership-fenced abort is published.
struct ChatRequestRegistration {
    token: ChatRequestToken,
    meetings: HashSet<String>,
    retain_deletion_abort: bool,
    deletion_invalidated: bool,
    stream_started: bool,
}

struct ChatRequestRegistry {
    active: HashMap<ChatRequestSurface, String>,
    requests: HashMap<(ChatRequestSurface, String), ChatRequestRegistration>,
}

pub(crate) type ChatRequestToken = Arc<CancellationToken>;

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

    /// Chat and Sidebar keep the approved replacement semantics: one active
    /// request per surface, a newer claim cancels the previous one. MCP chat
    /// requests are INDEPENDENT: each internal unique identity owns its own
    /// registration/token, a concurrent MCP request never cancels another,
    /// and every entry lives only until its own success/error/timeout/
    /// invalidation cleanup (bounded per-entry lifetime).
    fn supersedes(surface: ChatRequestSurface) -> bool {
        !matches!(surface, ChatRequestSurface::Mcp)
    }

    /// Ownership check shared by is_owner/bind/publish: the registration must
    /// exist, hold this exact token, and be alive; superseding surfaces must
    /// additionally still be the surface's active request.
    fn owns(
        registry: &ChatRequestRegistry,
        surface: ChatRequestSurface,
        request_id: &str,
        token: &ChatRequestToken,
    ) -> bool {
        registry
            .requests
            .get(&(surface, request_id.to_string()))
            .is_some_and(|registration| {
                Arc::ptr_eq(&registration.token, token)
                    && !token.is_cancelled()
                    && (matches!(surface, ChatRequestSurface::Mcp)
                        || registry.active.get(&surface).map(String::as_str) == Some(request_id))
            })
    }

    /// THE single admission API for every surface. Chat/Sidebar keep the
    /// approved replacement semantics (one active request per surface, a
    /// newer claim cancels the previous one) and are always admitted; MCP
    /// requests are INDEPENDENT, keyed by their internal unique identities,
    /// and admitted only up to [`MAX_CONCURRENT_MCP_REQUESTS`] — a saturated
    /// claim returns `None` before any caller-side work. There is no other
    /// way to construct a registry entry, so the MCP cap is a state
    /// invariant, not an adapter convention.
    pub(crate) fn try_claim_request(
        &self,
        surface: ChatRequestSurface,
        request_id: &str,
    ) -> Option<ChatRequestToken> {
        let token = Arc::new(CancellationToken::new());
        let mut registry = self.0.lock().unwrap();
        if Self::supersedes(surface) {
            if let Some(previous_id) = registry.active.insert(surface, request_id.to_string()) {
                if let Some(previous) = registry.requests.remove(&(surface, previous_id)) {
                    previous.token.cancel();
                }
            }
        } else {
            // Independent surface: admission is capped atomically, and an id
            // collision (unique UUIDs in practice) replaces only its own
            // entry; no other request is touched.
            if registry
                .requests
                .keys()
                .filter(|(claim_surface, _)| *claim_surface == surface)
                .count()
                >= MAX_CONCURRENT_MCP_REQUESTS
            {
                return None;
            }
            registry.requests.remove(&(surface, request_id.to_string()));
        }
        registry.requests.insert(
            (surface, request_id.to_string()),
            ChatRequestRegistration {
                token: token.clone(),
                meetings: HashSet::new(),
                retain_deletion_abort: false,
                deletion_invalidated: false,
                stream_started: false,
            },
        );
        Some(token)
    }

    /// Claim for the superseding surfaces (Chat/Sidebar), which are always
    /// admitted. MCP has no uncapped path: independent surfaces MUST go
    /// through [`Self::try_claim_request`].
    fn claim_superseding_request(
        &self,
        surface: ChatRequestSurface,
        request_id: &str,
    ) -> ChatRequestToken {
        assert!(
            Self::supersedes(surface),
            "independent surfaces must use the cap-enforcing try_claim_request"
        );
        self.try_claim_request(surface, request_id)
            .expect("superseding surfaces are always admitted")
    }

    /// Binds the prepared evidence's meeting identities to a still-owned
    /// request. Returns false (caller aborts) when the request was superseded
    /// or cancelled. Binding MUST precede the post-preparation existence
    /// recheck: any deletion that commits afterwards invalidates this visible
    /// registration, closing the check-to-emit race.
    pub(crate) fn bind_request_meetings(
        &self,
        surface: ChatRequestSurface,
        request_id: &str,
        token: &ChatRequestToken,
        meetings: &HashSet<String>,
    ) -> bool {
        let mut registry = self.0.lock().unwrap();
        if !Self::owns(&registry, surface, request_id, token) {
            return false;
        }
        let registration = registry
            .requests
            .get_mut(&(surface, request_id.to_string()))
            .expect("ownership re-checked above");
        registration.meetings = meetings.clone();
        true
    }

    fn bind_chat_stream_meetings(
        &self,
        request_id: &str,
        token: &ChatRequestToken,
        meetings: &HashSet<String>,
    ) -> bool {
        let mut registry = self.0.lock().unwrap();
        if !Self::owns(&registry, ChatRequestSurface::Chat, request_id, token) {
            return false;
        }
        let registration = registry
            .requests
            .get_mut(&(ChatRequestSurface::Chat, request_id.to_string()))
            .expect("ownership re-checked above");
        registration.meetings = meetings.clone();
        registration.retain_deletion_abort = true;
        true
    }

    /// Cancels and removes every registered request whose prepared evidence
    /// references `meeting_id`. Called from the real meeting-deletion
    /// transaction before the meeting row disappears. A retained current
    /// stream atomically converts its next publication into an abort; every
    /// other request suppresses normal publication.
    /// Surface-independent: cancels Chat, Sidebar, and every affected
    /// independent MCP request.
    pub(crate) fn invalidate_meeting(&self, meeting_id: &str) -> usize {
        let mut registry = self.0.lock().unwrap();
        let invalidated: Vec<(ChatRequestSurface, String)> = registry
            .requests
            .iter()
            .filter(|(_, registration)| registration.meetings.contains(meeting_id))
            .map(|(key, _)| key.clone())
            .collect();
        for (surface, request_id) in &invalidated {
            let key = (*surface, request_id.clone());
            if registry
                .requests
                .get(&key)
                .is_some_and(|registration| registration.retain_deletion_abort)
            {
                let registration = registry
                    .requests
                    .get_mut(&key)
                    .expect("registration was checked above");
                registration.deletion_invalidated = true;
                registration.token.cancel();
                continue;
            }
            registry.active.remove(surface);
            if let Some(registration) = registry.requests.remove(&key) {
                registration.token.cancel();
            }
        }
        invalidated.len()
    }

    pub(crate) fn is_owner(
        &self,
        surface: ChatRequestSurface,
        request_id: &str,
        token: &ChatRequestToken,
    ) -> bool {
        let registry = self.0.lock().unwrap();
        Self::owns(&registry, surface, request_id, token)
    }

    pub(crate) fn clear_if_owner(
        &self,
        surface: ChatRequestSurface,
        request_id: &str,
        token: &ChatRequestToken,
    ) -> bool {
        let mut registry = self.0.lock().unwrap();
        // Cancellation-tolerant by design: the timeout path cancels the token
        // and THEN clears its own entry.
        let owned = registry
            .requests
            .get(&(surface, request_id.to_string()))
            .is_some_and(|registration| {
                Arc::ptr_eq(&registration.token, token)
                    && (matches!(surface, ChatRequestSurface::Mcp)
                        || registry.active.get(&surface).map(String::as_str) == Some(request_id))
            });
        if !owned {
            return false;
        }
        let key = (surface, request_id.to_string());
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
        if let Some(registration) = registry.requests.remove(&(surface, active_id)) {
            registration.token.cancel();
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub(crate) fn request_count(&self) -> usize {
        self.0.lock().unwrap().requests.len()
    }

    fn publish_chat_stream_event_if_current<F: FnOnce(&str, serde_json::Value)>(
        &self,
        request_id: &str,
        token: &ChatRequestToken,
        event: &str,
        payload: serde_json::Value,
        clear: bool,
        emit: F,
    ) -> bool {
        let mut registry = self.0.lock().unwrap();
        let key = (ChatRequestSurface::Chat, request_id.to_string());
        let current = registry.requests.get(&key).is_some_and(|registration| {
            Arc::ptr_eq(&registration.token, token)
                && registry
                    .active
                    .get(&ChatRequestSurface::Chat)
                    .map(String::as_str)
                    == Some(request_id)
        });
        if !current {
            return false;
        }
        if registry
            .requests
            .get(&key)
            .is_some_and(|registration| registration.deletion_invalidated)
        {
            registry.active.remove(&ChatRequestSurface::Chat);
            registry.requests.remove(&key);
            emit(
                "chat-stream-abort",
                serde_json::json!({
                    "streamId": request_id,
                    "reason": "referenced_meeting_deleted",
                }),
            );
            return false;
        }
        if token.is_cancelled() {
            return false;
        }
        if event == "chat-stream-start" {
            registry
                .requests
                .get_mut(&key)
                .expect("ownership re-checked above")
                .stream_started = true;
        }
        if clear {
            registry.active.remove(&ChatRequestSurface::Chat);
            registry.requests.remove(&key);
        }
        emit(event, payload);
        true
    }

    fn finish_chat_stream_if_current<F: FnOnce(&str, serde_json::Value)>(
        &self,
        request_id: &str,
        token: &ChatRequestToken,
        cancel: bool,
        cleanup_event: Option<(&str, serde_json::Value)>,
        emit: F,
    ) -> bool {
        let mut registry = self.0.lock().unwrap();
        let key = (ChatRequestSurface::Chat, request_id.to_string());
        let current = registry.requests.get(&key).is_some_and(|registration| {
            Arc::ptr_eq(&registration.token, token)
                && registry
                    .active
                    .get(&ChatRequestSurface::Chat)
                    .map(String::as_str)
                    == Some(request_id)
        });
        if !current {
            return false;
        }
        let invalidated = registry
            .requests
            .get(&key)
            .is_some_and(|registration| registration.deletion_invalidated);
        let stream_started = registry
            .requests
            .get(&key)
            .is_some_and(|registration| registration.stream_started);
        if cancel {
            token.cancel();
        }
        registry.active.remove(&ChatRequestSurface::Chat);
        registry.requests.remove(&key);
        if invalidated {
            emit(
                "chat-stream-abort",
                serde_json::json!({
                    "streamId": request_id,
                    "reason": "referenced_meeting_deleted",
                }),
            );
            return true;
        }
        if stream_started {
            if let Some((event, payload)) = cleanup_event {
                emit(event, payload);
                return true;
            }
        }
        false
    }

    #[cfg(test)]
    fn publish_deletion_abort_if_current<F: FnOnce(&str, serde_json::Value)>(
        &self,
        request_id: &str,
        token: &ChatRequestToken,
        emit: F,
    ) -> bool {
        let mut registry = self.0.lock().unwrap();
        let key = (ChatRequestSurface::Chat, request_id.to_string());
        let invalidated = registry.requests.get(&key).is_some_and(|registration| {
            Arc::ptr_eq(&registration.token, token)
                && registration.deletion_invalidated
                && registry
                    .active
                    .get(&ChatRequestSurface::Chat)
                    .map(String::as_str)
                    == Some(request_id)
        });
        if !invalidated {
            return false;
        }
        registry.active.remove(&ChatRequestSurface::Chat);
        registry.requests.remove(&key);
        emit(
            "chat-stream-abort",
            serde_json::json!({
                "streamId": request_id,
                "reason": "referenced_meeting_deleted",
            }),
        );
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
    let broad_intent = requests_broad_retrieval(query) || requests_broad_retrieval(&search_query);
    let today_context = today_meeting_ids
        .as_ref()
        .map(|_| "Meeting context is filtered to today's meetings in this scope.\n".to_string());
    let temporal = temporal_context_for_scope(
        pool,
        &retrieval_scope,
        temporal_context_budget(query, &search_query, &today_context, max_context_chars),
    )
    .await?;
    let mut temporal_context = temporal.context;
    if let Some(today_context) = today_context {
        temporal_context.push_str(&today_context);
    }
    let persisted_context_budget =
        context_budget_for_prompt(query, &search_query, &temporal_context, max_context_chars);
    let (meeting_list_context, mut prompt_meeting_ids) = meeting_list_context_for_scope(
        pool,
        &retrieval_scope,
        query,
        today_date,
        persisted_context_budget,
    )
    .await?;
    if let Some(meeting_id) = temporal.meeting_id {
        prompt_meeting_ids.insert(meeting_id);
    }
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
    let (mut context, mut sources, context_meeting_ids) = match retrieval_scope {
        ChatRetrievalScope::LiveRecording(scope_key) => {
            ensure_live_scope_matches_active_recording(&scope_key)?;
            let snapshot = crate::audio::recording_commands::get_transcript_history().await?;
            ensure_not_cancelled(cancellation_token)?;
            let (context, sources) =
                live_snapshot_context(&snapshot, &scope_key, persisted_context_budget);
            (context, sources, HashSet::new())
        }
        scope => {
            if let Some(context) = meeting_list_context {
                (context, Vec::new(), HashSet::new())
            } else if matches!(scope, ChatRetrievalScope::Meeting(_)) && today_meeting_ids.is_none()
            {
                let ChatRetrievalScope::Meeting(meeting_id) = scope else {
                    unreachable!()
                };
                let (meeting, semantic_fallback) = if !force_lexical {
                    if let Some(lifecycle) = lifecycle.as_ref() {
                        resolve_meeting_context_hybrid(
                            pool,
                            &meeting_id,
                            &search_query,
                            query,
                            chunk_limit,
                            lifecycle,
                            cancellation_token,
                        )
                        .await?
                    } else {
                        (
                            resolve_meeting_context(
                                pool,
                                &meeting_id,
                                &search_query,
                                query,
                                chunk_limit,
                            )
                            .await?,
                            None,
                        )
                    }
                } else {
                    (
                        resolve_meeting_context(
                            pool,
                            &meeting_id,
                            &search_query,
                            query,
                            chunk_limit,
                        )
                        .await?,
                        None,
                    )
                };
                if semantic_fallback.is_some() {
                    retrieval_diagnostic = RetrievalPreparationDiagnostic::SemanticFallback;
                }
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
                let sources: Vec<ChatSource> = meeting
                    .transcripts
                    .iter()
                    .filter(|transcript| retained.contains(&transcript.chunk_id))
                    .map(chat_source_from_result)
                    .collect();
                if deep_preparation_eligible(retrieval_mode)
                    && !force_lexical
                    && lifecycle.is_some()
                {
                    // Saved-meeting Deep: the authoritative one-pass anchors
                    // above are the request's Fast baseline; the bounded
                    // planner runs with a strict one-meeting allow-list so it
                    // can only retrieve more of THIS meeting. The
                    // authoritative summary/notes and R10 fallback above are
                    // retained verbatim; planner rounds may only add
                    // additional in-scope transcript evidence, which hydration
                    // re-loads and re-fences under the same parity contracts.
                    let lifecycle = lifecycle
                        .as_ref()
                        .ok_or_else(|| "Retrieval lifecycle unavailable".to_string())?;
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
                            lifecycle: lifecycle.clone(),
                            original_query: query,
                            effective_query: &search_query,
                            // Strict one-meeting allow-list: the planner can
                            // search/open/expand only this meeting; opening
                            // another meeting is outside the capability set.
                            scope: crate::retrieval::service::PersistedRetrievalScope::
                                AllowedMeetingIds(vec![meeting_id.clone()]),
                            broad_intent: false,
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
                        "Chat saved-meeting Deep preparation: additional_rounds={} planner_calls={} round_trips_before_generation={}",
                        deep.additional_rounds,
                        deep.planner_round_trips,
                        provider_round_trips
                    );
                    if deep.semantic_fallback.is_some() {
                        retrieval_diagnostic = RetrievalPreparationDiagnostic::SemanticFallback;
                    }
                    // Merge planner-added evidence INTO the authoritative
                    // builder output: mandatory summary/notes and R10
                    // transcript anchors stay first; only bounded in-meeting
                    // evidence whose text is NOT already retained in the
                    // authoritative context is appended (its snippet comes
                    // from the fenced hydration and is guaranteed to appear
                    // verbatim in the markdown once appended), keeping exact
                    // prompt/source parity.
                    let deep_sources = deep
                        .hydrated
                        .sources
                        .iter()
                        .map(chat_source_from_hydrated)
                        .collect::<Vec<_>>();
                    let (markdown, merged_sources) = merge_saved_meeting_deep_context(
                        built.markdown,
                        sources,
                        deep_sources,
                        persisted_context_budget,
                    );
                    (
                        markdown,
                        merged_sources,
                        HashSet::from([meeting.meeting_id]),
                    )
                } else {
                    (built.markdown, sources, HashSet::from([meeting.meeting_id]))
                }
            } else if !force_lexical
                && lifecycle.is_some()
                && matches!(
                    scope,
                    ChatRetrievalScope::All
                        | ChatRetrievalScope::Folder(_)
                        | ChatRetrievalScope::SearchSnapshot(_)
                )
            {
                let lifecycle =
                    lifecycle.ok_or_else(|| "Retrieval lifecycle unavailable".to_string())?;
                // Saved meetings use shared hybrid anchors without the planner;
                // persisted broad scopes use the planner only after their
                // authoritative allow-list is resolved.
                let deep_eligible = deep_preparation_eligible(retrieval_mode);
                let persisted_scope = match &scope {
                    ChatRetrievalScope::All => PersistedRetrievalScope::All,
                    ChatRetrievalScope::Folder(folder_id) => {
                        PersistedRetrievalScope::Folder(folder_id.clone())
                    }
                    ChatRetrievalScope::SearchSnapshot(meeting_ids) => {
                        PersistedRetrievalScope::AllowedMeetingIds(meeting_ids.clone())
                    }
                    _ => unreachable!(),
                };
                let persisted_scope = today_meeting_ids
                    .as_ref()
                    .map(|ids| PersistedRetrievalScope::AllowedMeetingIds(ids.clone()))
                    .unwrap_or(persisted_scope);
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
                            broad_intent,
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
                    (deep.hydrated.markdown, sources, HashSet::new())
                } else {
                    let request = RetrievalRequest {
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
                    };
                    let service = RetrievalService::new(lifecycle);
                    let ranked = if broad_intent {
                        service
                            .retrieve_ranked_with_broad_coverage(pool, request)
                            .await
                    } else {
                        service.retrieve_ranked(pool, request).await
                    }
                    .map_err(|e| format!("Retrieval failed: {}", e))?;
                    if ranked.semantic_fallback.is_some() {
                        retrieval_diagnostic = RetrievalPreparationDiagnostic::SemanticFallback;
                    }
                    let hydrated = if broad_intent {
                        hydrate_context_with_broad_coverage(
                            pool,
                            &ranked,
                            persisted_context_budget,
                            cancellation_token,
                        )
                        .await
                    } else {
                        hydrate_context(pool, &ranked, persisted_context_budget, cancellation_token)
                            .await
                    }
                    .map_err(|e| format!("Retrieval hydration failed: {}", e))?;
                    let sources = hydrated
                        .sources
                        .iter()
                        .map(chat_source_from_hydrated)
                        .collect();
                    (hydrated.markdown, sources, HashSet::new())
                }
            } else {
                log::info!("Chat retrieval preparation mode: {retrieval_diagnostic:?}");
                let broad_scope_ids = today_meeting_ids.as_deref().or_else(|| match &scope {
                    ChatRetrievalScope::SearchSnapshot(meeting_ids) => Some(meeting_ids.as_slice()),
                    _ => None,
                });
                if broad_intent {
                    if let Some(meeting_ids) = broad_scope_ids {
                        let hydrated = hydrate_broad_scope_context(
                            pool,
                            &PersistedRetrievalScope::AllowedMeetingIds(meeting_ids.to_vec()),
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
                        (hydrated.markdown, sources, HashSet::new())
                    } else {
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
                        let built =
                            build_context_markdown_with_limit(&results, persisted_context_budget);
                        let retained = built
                            .retained_evidence_ids
                            .into_iter()
                            .collect::<HashSet<_>>();
                        let sources = results
                            .iter()
                            .filter(|result| retained.contains(&lexical_evidence_id(result)))
                            .map(chat_source_from_result)
                            .collect();
                        (built.markdown, sources, HashSet::new())
                    }
                } else {
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
                    let built =
                        build_context_markdown_with_limit(&results, persisted_context_budget);
                    let retained = built
                        .retained_evidence_ids
                        .into_iter()
                        .collect::<HashSet<_>>();
                    let sources = results
                        .iter()
                        .filter(|result| retained.contains(&lexical_evidence_id(result)))
                        .map(chat_source_from_result)
                        .collect();
                    (built.markdown, sources, HashSet::new())
                }
            }
        }
    };

    // Every context builder above is already bounded by
    // `persisted_context_budget`, so this is an assertion in debug and a
    // fail-closed backstop in release: if a builder ever overshoots, the
    // context is cut AND every source whose snippet no longer appears in it is
    // dropped, so a published source can never reference text the model did
    // not receive. Silently truncating while keeping the sources would break
    // exactly the prompt/source parity the scope contracts rely on.
    debug_assert!(
        context.chars().count() <= persisted_context_budget,
        "assembled chat context exceeded the persisted context budget"
    );
    if context.chars().count() > persisted_context_budget {
        log::warn!(
            "Chat context exceeded its budget ({} > {}); truncating and reconciling sources",
            context.chars().count(),
            persisted_context_budget
        );
        context = truncate_at_char_boundary(&context, persisted_context_budget).to_string();
        sources.retain(|source| context.contains(&source.snippet));
    }
    let user_prompt = assemble_prompt(
        &context,
        history.map_or(&[], Vec::as_slice),
        query,
        &search_query,
        &temporal_context,
        max_context_chars,
    );

    if !context.is_empty() {
        prompt_meeting_ids.extend(context_meeting_ids);
    }
    for source in &sources {
        prompt_meeting_ids.insert(source.meeting_id.clone());
    }
    ensure_prompt_meetings_exist(pool, &prompt_meeting_ids).await?;

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
        prompt_meeting_ids,
        retrieval_diagnostic,
        retrieval_mode,
        provider_round_trips,
    })
}

async fn temporal_context_for_scope(
    pool: &SqlitePool,
    scope: &ChatRetrievalScope,
    max_context_chars: usize,
) -> Result<TemporalPromptContext, String> {
    let latest =
        match scope {
            ChatRetrievalScope::All => sqlx::query_as::<_, LatestSavedMeeting>(
                "SELECT id, title, saved_at FROM meetings ORDER BY saved_at DESC, id DESC LIMIT 1",
            )
            .fetch_optional(pool)
            .await,
            ChatRetrievalScope::Meeting(meeting_id) => {
                sqlx::query_as::<_, LatestSavedMeeting>(
                    "SELECT id, title, saved_at FROM meetings WHERE id = ? LIMIT 1",
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

    let latest = latest.map(|mut meeting| {
        meeting.title = truncate_meeting_title(&meeting.title);
        meeting
    });
    let context = format_temporal_context(Local::now(), latest.as_ref());
    if context.chars().count() <= max_context_chars {
        Ok(TemporalPromptContext {
            context,
            meeting_id: latest.map(|meeting| meeting.id),
        })
    } else {
        Ok(TemporalPromptContext {
            context: truncate_at_char_boundary(
                &format_temporal_context(Local::now(), None),
                max_context_chars,
            )
            .to_string(),
            meeting_id: None,
        })
    }
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
    let mut query = QueryBuilder::<Sqlite>::new("SELECT id, title, saved_at FROM meetings WHERE ");
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
        SELECT m.id, m.title, m.saved_at
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

fn requests_broad_retrieval(query: &str) -> bool {
    let terms = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    terms.iter().any(|term| {
        term.starts_with("summar")
            || (term.starts_with("resum") && !term.starts_with("resume"))
            || term.starts_with("compar")
            || term.starts_with("differ")
            || term.starts_with("contrast")
            || matches!(
                term.as_str(),
                "overview"
                    | "recap"
                    | "sintese"
                    | "síntese"
                    | "panorama"
                    | "consolidate"
                    | "consolidar"
            )
    }) || requests_meeting_list(query)
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
) -> Result<Vec<ListedMeeting>, String> {
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
            " UNION ALL SELECT f.id FROM meeting_folders f JOIN folder_scope s ON f.parent_id = s.id) SELECT id, title FROM meetings WHERE folder_id IN (SELECT id FROM folder_scope)",
        );
    } else {
        query.push("SELECT id, title FROM meetings");
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
        .build_query_as()
        .fetch_all(pool)
        .await
        .map(|rows: Vec<(String, String)>| {
            rows.into_iter()
                .map(|(id, title)| ListedMeeting { id, title })
                .collect()
        })
        .map_err(|error| format!("Failed to list meetings in this scope: {}", error))
}

async fn meeting_list_context_for_scope(
    pool: &SqlitePool,
    scope: &ChatRetrievalScope,
    query_text: &str,
    today_date: Option<NaiveDate>,
    context_budget: usize,
) -> Result<(Option<String>, HashSet<String>), String> {
    if requests_meeting_list(query_text) {
        let meetings = meeting_titles_for_scope(pool, scope, query_text, today_date).await?;
        let (context, ids) = format_meeting_list_context(&meetings, context_budget);
        Ok((Some(context), ids))
    } else {
        Ok((None, HashSet::new()))
    }
}

fn format_meeting_list_context(
    meetings: &[ListedMeeting],
    max_context_chars: usize,
) -> (String, HashSet<String>) {
    let mut context = MEETING_LIST_HEADER.to_string();
    if context.chars().count() > max_context_chars {
        return (
            truncate_at_char_boundary(&context, max_context_chars).to_string(),
            HashSet::new(),
        );
    }
    let mut ids = HashSet::new();
    for meeting in meetings {
        let line = format!("- {}\n", truncate_meeting_title(&meeting.title));
        if context.chars().count() + line.chars().count() > max_context_chars {
            break;
        }
        context.push_str(&line);
        ids.insert(meeting.id.clone());
    }
    (context, ids)
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

async fn resolve_meeting_context_hybrid(
    pool: &SqlitePool,
    meeting_id: &str,
    search_query: &str,
    original_query: &str,
    chunk_limit: u32,
    lifecycle: &RetrievalLifecycle,
    cancellation_token: Option<&CancellationToken>,
) -> Result<(MeetingChatContext, Option<SemanticFallbackReason>), String> {
    let cancel = cancellation_token.cloned().unwrap_or_default();
    let ranked = RetrievalService::new(lifecycle.clone())
        .retrieve_ranked(
            pool,
            RetrievalRequest {
                original_query: original_query.to_string(),
                rewritten_query: Some(search_query.to_string()),
                scope: PersistedRetrievalScope::Meeting(meeting_id.to_string()),
                purpose: RetrievalPurpose::Chat,
                limits: RetrievalLimits {
                    lexical_per_variant: chunk_limit as usize,
                    vector_per_variant: chunk_limit as usize,
                },
                core_language: crate::retrieval::service::CoreTermLanguage::Unknown,
                cancellation: Some(cancel.clone()),
            },
        )
        .await
        .map_err(|error| format!("Retrieval failed: {}", error))?;
    ensure_not_cancelled(Some(&cancel))?;
    let mut transcript_ids = BTreeSet::new();
    let mut transcript_ranges = BTreeSet::new();
    let mut add_range = |start: Option<&String>, end: Option<&String>| {
        let Some(start) = start else {
            return;
        };
        let end = end.unwrap_or(start);
        transcript_ids.insert(start.clone());
        transcript_ids.insert(end.clone());
        transcript_ranges.insert((start.clone(), end.clone()));
    };
    for item in ranked.ranking.evidence.iter().filter(|item| {
        item.evidence.meeting_id == meeting_id && item.evidence.source_kind == "transcript"
    }) {
        add_range(
            item.evidence.source_start_id.as_ref(),
            item.evidence.source_end_id.as_ref(),
        );
        for alias in item
            .evidence
            .source_aliases
            .iter()
            .filter(|alias| alias.source_kind == "transcript")
        {
            add_range(alias.source_start_id.as_ref(), alias.source_end_id.as_ref());
        }
    }
    let transcript_ids = transcript_ids.into_iter().collect::<Vec<_>>();
    let transcript_ranges = transcript_ranges.into_iter().collect::<Vec<_>>();
    let mut source = crate::database::repositories::retrieval::RetrievalRepository::
        load_meeting_source_relevant_ranges_with_cancellation(
            pool,
            meeting_id,
            &transcript_ids,
            &transcript_ranges,
            &cancel,
        )
        .await
        .map_err(|error| format!("Failed to load meeting context: {}", error))?
        .ok_or_else(|| "Meeting not found".to_string())?;
    let mut included =
        meeting_transcript_positions(&source, &ranked.ranking.evidence, chunk_limit as usize);
    if included.is_empty() && !transcript_ids.is_empty() {
        source = crate::database::repositories::retrieval::RetrievalRepository::
            load_meeting_source_relevant_with_cancellation(pool, meeting_id, &[], &cancel)
                .await
                .map_err(|error| format!("Failed to load meeting context: {}", error))?
                .ok_or_else(|| "Meeting not found".to_string())?;
    }
    if included.is_empty() {
        included.extend(
            source
                .transcript_positions
                .iter()
                .take(chunk_limit as usize)
                .copied(),
        );
    }
    let transcripts = meeting_transcripts_for_positions(&source, &included);
    let context = MeetingChatContext {
        meeting_id: source.meeting_id,
        meeting_title: source.title,
        summary: source.latest_summary_markdown,
        notes: source
            .notes_markdown
            .filter(|notes| !notes.trim().is_empty()),
        transcripts,
        total_transcript_segments: source.transcript_segments_total,
    };
    Ok((context, ranked.semantic_fallback))
}

fn meeting_transcript_positions(
    source: &crate::database::repositories::retrieval::MeetingSource,
    evidence: &[crate::retrieval::RankedEvidence],
    limit: usize,
) -> BTreeSet<usize> {
    let positions: HashMap<&str, (usize, usize)> = source
        .transcripts
        .iter()
        .zip(source.transcript_positions.iter().copied())
        .enumerate()
        .map(|(index, (segment, position))| (segment.id.as_str(), (position, index)))
        .collect();
    let range_for = |start_id: Option<&str>, end_id: Option<&str>| {
        let start_id = start_id?;
        let end_id = end_id.unwrap_or(start_id);
        let &(start_position, start_index) = positions.get(start_id)?;
        let &(end_position, end_index) = positions.get(end_id)?;
        (start_position <= end_position && start_index <= end_index).then_some((
            start_position,
            end_position,
            start_index,
            end_index,
        ))
    };
    let mut included = BTreeSet::new();
    let mut anchors = 0;
    for item in evidence.iter().filter(|item| {
        item.evidence.meeting_id == source.meeting_id && item.evidence.source_kind == "transcript"
    }) {
        if anchors >= limit {
            break;
        }
        let is_semantic = item
            .evidence
            .provenance
            .iter()
            .any(|provenance| provenance.channel == RetrievalChannel::Semantic);
        let mut accepted = false;
        if let Some((start, end, start_index, end_index)) = range_for(
            item.evidence.source_start_id.as_deref(),
            item.evidence.source_end_id.as_deref(),
        ) {
            let hash_matches = !is_semantic || {
                let authoritative = source.transcripts[start_index..=end_index]
                    .iter()
                    .map(|segment| segment.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                let expected = item.content_fingerprint.clone().unwrap_or_else(|| {
                    sha2::Sha256::digest(item.evidence.text.as_bytes()).to_vec()
                });
                sha2::Sha256::digest(authoritative.as_bytes()).as_slice() == expected.as_slice()
            };
            if hash_matches {
                accepted = true;
                include_transcript_range(
                    &mut included,
                    start,
                    end,
                    source.transcript_segments_total,
                );
            }
        }
        for alias in &item.evidence.source_aliases {
            if alias.source_kind != "transcript" {
                continue;
            }
            if let Some((start, end, _, _)) = range_for(
                alias.source_start_id.as_deref(),
                alias.source_end_id.as_deref(),
            ) {
                accepted = true;
                include_transcript_range(
                    &mut included,
                    start,
                    end,
                    source.transcript_segments_total,
                );
            }
        }
        if accepted {
            anchors += 1;
        }
    }
    included
}

fn include_transcript_range(
    included: &mut BTreeSet<usize>,
    start: usize,
    end: usize,
    total: usize,
) {
    if total == 0 {
        return;
    }
    included.extend(start.saturating_sub(1)..=end.saturating_add(1).min(total - 1));
}

fn meeting_transcripts_for_positions(
    source: &crate::database::repositories::retrieval::MeetingSource,
    included: &BTreeSet<usize>,
) -> Vec<crate::database::repositories::fts::FtsSearchResult> {
    source
        .transcripts
        .iter()
        .zip(source.transcript_positions.iter().copied())
        .filter(|(_, position)| included.contains(position))
        .map(
            |(segment, _)| crate::database::repositories::fts::FtsSearchResult {
                meeting_id: source.meeting_id.clone(),
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
        .collect()
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
    let allowed_ids = meeting_ids_override.or_else(|| match &scope {
        ChatRetrievalScope::SearchSnapshot(meeting_ids) => Some(meeting_ids.as_slice()),
        _ => None,
    });
    if let Some(meeting_ids) = allowed_ids {
        if requests_broad_retrieval(search_query) || requests_broad_retrieval(original_query) {
            return FtsRepository::get_by_meeting_ids(
                pool,
                meeting_ids,
                1,
                SNAPSHOT_REHYDRATION_CHUNK_CAP,
            )
            .await
            .map_err(|error| {
                tracing::error!("FTS search failed for chat: {}", error);
                format!("Search failed: {}", error)
            });
        }
    }

    if let Some(meeting_ids) = allowed_ids {
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
            let results = FtsRepository::search_with_meeting_ids(
                pool,
                query,
                attempt_limit,
                meeting_ids,
                mode,
            )
            .await
            .map_err(|error| {
                tracing::error!("FTS search failed for chat: {}", error);
                format!("Search failed: {}", error)
            })?;
            let unique_results = results
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
        if !results.is_empty() {
            return Ok(results);
        }
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

/// Whether this request runs the bounded Deep planner. The surface-level
/// refusals happen earlier, where `retrieval_mode` is resolved: forced-lexical,
/// an unavailable retrieval lifecycle, and live-recording scope all downgrade
/// to Fast before this is consulted, and MCP rejects Deep before preparation.
/// Every remaining persisted scope resolves its authoritative membership
/// before preparation, so the mode is the only thing left to decide.
fn deep_preparation_eligible(retrieval_mode: ChatRetrievalMode) -> bool {
    retrieval_mode == ChatRetrievalMode::Deep
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

fn temporal_context_budget(
    query: &str,
    search_query: &str,
    today_context: &Option<String>,
    max_context_chars: usize,
) -> usize {
    max_context_chars.saturating_sub(
        "\n\nMeeting context:\n".chars().count()
            + today_context.as_deref().unwrap_or_default().chars().count()
            + format!(
                "\nUser question: {}\nSearch query: {}\n",
                query, search_query
            )
            .chars()
            .count(),
    )
}

fn merge_saved_meeting_deep_context(
    mut markdown: String,
    mut sources: Vec<ChatSource>,
    deep_sources: Vec<ChatSource>,
    context_budget: usize,
) -> (String, Vec<ChatSource>) {
    for source in deep_sources {
        if markdown.contains(&source.snippet)
            || sources
                .iter()
                .any(|existing| existing.snippet == source.snippet)
        {
            continue;
        }
        let addition = format!("\n\n### Additional retrieved context\n{}", source.snippet);
        if markdown.chars().count() + addition.chars().count() > context_budget {
            continue;
        }
        markdown.push_str(&addition);
        sources.push(source);
    }
    (markdown, sources)
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

pub(crate) fn finish_non_streaming_chat_request<T>(
    state: &ChatRequestState,
    surface: ChatRequestSurface,
    request_id: &str,
    token: &ChatRequestToken,
    result: Result<Result<T, String>, tokio::time::error::Elapsed>,
) -> Result<T, String> {
    let timed_out = result.is_err();
    if timed_out {
        token.cancel();
    }
    if !state.clear_if_owner(surface, request_id, token) {
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
    let token = request_state.claim_superseding_request(ChatRequestSurface::Chat, &request_id);
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
        // Deletion fence: bind prepared evidence identities, then recheck
        // existence before spending the final provider call.
        if !request_state.bind_request_meetings(
            ChatRequestSurface::Chat,
            &request_id,
            &token,
            &inputs.prompt_meeting_ids,
        ) {
            return Err("Chat request was cancelled or superseded".to_string());
        }
        ensure_prompt_meetings_exist(&pool, &inputs.prompt_meeting_ids).await?;
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
        // Terminal invalidation fence: a deletion during generation
        // invalidated this request through the registry; recheck existence
        // before returning any final answer/source payload.
        ensure_prompt_meetings_exist(&pool, &inputs.prompt_meeting_ids).await?;

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

    finish_non_streaming_chat_request(
        &request_state,
        ChatRequestSurface::Chat,
        &request_id,
        &token,
        result,
    )
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
    let token = request_state.claim_superseding_request(ChatRequestSurface::Chat, &request_id);
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
        // Deletion fence: bind prepared evidence identities, then recheck
        // existence before spending the final provider call.
        if !request_state.bind_request_meetings(
            ChatRequestSurface::Chat,
            &request_id,
            &token,
            &inputs.prompt_meeting_ids,
        ) {
            return Err("Chat request was cancelled or superseded".to_string());
        }
        ensure_prompt_meetings_exist(&pool, &inputs.prompt_meeting_ids).await?;
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
        // Terminal invalidation fence before any final answer/source payload.
        ensure_prompt_meetings_exist(&pool, &inputs.prompt_meeting_ids).await?;

        Ok(ChatResponse {
            answer,
            sources: inputs.sources,
        })
    })
    .await;

    finish_non_streaming_chat_request(
        &request_state,
        ChatRequestSurface::Chat,
        &request_id,
        &token,
        result,
    )
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
    let timeout_sink = AppEventSink(app.clone());
    await_chat_stream_with_timeout(
        CHAT_REQUEST_TIMEOUT,
        &request_state,
        &timeout_sink,
        &request_stream_id,
        &token,
        async {
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
                state.db_manager.pool().clone(),
                AppEventSink(app.clone()),
                request_state.clone(),
                inputs,
                stream_id,
                meeting_id,
                token.clone(),
            )
            .await
        },
    )
    .await
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
    let timeout_sink = AppEventSink(app.clone());
    await_chat_stream_with_timeout(
        CHAT_REQUEST_TIMEOUT,
        &request_state,
        &timeout_sink,
        &request_stream_id,
        &token,
        async {
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
                state.db_manager.pool().clone(),
                AppEventSink(app.clone()),
                request_state.clone(),
                inputs,
                stream_id,
                None,
                token.clone(),
            )
            .await
        },
    )
    .await
}

/// Emits chat stream events. Production wraps the Tauri app handle; tests
/// capture events so the real publication fence can be exercised without a
/// Tauri runtime.
trait ChatEventSink: Clone + Send + Sync + 'static {
    fn emit(&self, event: &str, payload: serde_json::Value);
}

struct AppEventSink<R: Runtime>(AppHandle<R>);

impl<R: Runtime> std::clone::Clone for AppEventSink<R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: Runtime> ChatEventSink for AppEventSink<R> {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        if let Err(error) = self.0.emit(event, payload) {
            tracing::error!("Failed to emit {}: {}", event, error);
        }
    }
}

async fn stream_chat<S: ChatEventSink>(
    pool: SqlitePool,
    sink: S,
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

    // Deletion privacy fence, in this exact order: atomically bind the prepared
    // evidence identities and retain an abort marker, then recheck existence.
    // A deletion that commits afterwards leaves a durable marker for the next
    // publication; one that committed before the recheck fails here.
    if !stream_state.bind_chat_stream_meetings(&stream_id, &token, &inputs.prompt_meeting_ids) {
        clear_chat_stream_if_owner(&stream_state, &stream_id, &token).await;
        return Ok(());
    }
    if let Err(error) = ensure_prompt_meetings_exist(&pool, &inputs.prompt_meeting_ids).await {
        clear_chat_stream_if_owner(&stream_state, &stream_id, &token).await;
        return Err(error);
    }

    if inputs.retrieval_mode == ChatRetrievalMode::Deep {
        info!(
            "Chat Deep stream handoff: {} provider round trips before generation (rewrite + planner calls)",
            inputs.provider_round_trips
        );
    }

    if !emit_chat_stream_event_if_sink(
        &stream_state,
        &sink,
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
    let sink_for_chunk = sink.clone();
    let stream_id_for_chunk = stream_id.clone();
    let token_for_chunk = token.clone();
    let stream_state_for_chunk = stream_state.clone();
    let on_chunk = move |chunk: &str| {
        let chunk_text = chunk.to_string();
        let partial = partial_for_chunk.clone();
        let sink = sink_for_chunk.clone();
        publish_chat_stream_event_if_owner(
            &stream_state_for_chunk,
            &stream_id_for_chunk,
            &token_for_chunk,
            "chat-stream-chunk",
            serde_json::json!({ "streamId": stream_id_for_chunk.clone(), "text": chunk_text.clone() }),
            false,
            move |event, payload| {
                partial.lock().unwrap().push_str(&chunk_text);
                sink.emit(event, payload);
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

    let terminal_fence = ensure_prompt_meetings_exist(&pool, &inputs.prompt_meeting_ids).await;
    if terminal_fence.is_err() {
        stream_state.finish_chat_stream_if_current(
            &stream_id,
            &token,
            false,
            Some((
                "chat-stream-error",
                serde_json::json!({
                    "streamId": stream_id,
                    "error": CHAT_CONTEXT_REVALIDATION_ERROR,
                    "safeCleanup": true,
                }),
            )),
            |event, payload| sink.emit(event, payload),
        );
        return Ok(());
    }

    match stream_result {
        Ok(answer) => {
            info!(
                "Chat stream completed: {} sources, {} answer chars, {} provider round trips including final generation",
                sources.len(),
                answer.len(),
                inputs.provider_round_trips + 1
            );
            emit_chat_stream_event_if_sink(
                &stream_state,
                &sink,
                &stream_id,
                &token,
                "chat-stream-done",
                completed_chat_stream_payload(&stream_id, &answer, &sources),
                true,
            );
            Ok(())
        }
        // Cancellation is decided by the request's OWN token, never by the
        // provider's message text: `generate_summary_stream` embeds the raw
        // provider body verbatim, so an upstream error that merely contains
        // "cancelled" used to take this arm, publish no terminal event, and
        // leave a started stream's row rendering forever. Anything the token
        // does not confirm as cancelled falls through to the error arm below,
        // which always publishes a terminal event.
        Err(error) if token.is_cancelled() => {
            let answer = partial_text.lock().unwrap().clone();
            tracing::info!(
                "Chat stream cancelled after {} chars ({error})",
                answer.len()
            );
            stream_state.finish_chat_stream_if_current(
                &stream_id,
                &token,
                false,
                None,
                |event, payload| sink.emit(event, payload),
            );
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
            emit_chat_stream_event_if_sink(
                &stream_state,
                &sink,
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

/// Typed abort used whenever prepared evidence references a meeting that no
/// longer exists. The request is aborted — never answered from degrading
/// evidence — so retained context and published sources keep exact parity.
pub(crate) const DELETED_MEETING_EVIDENCE_ERROR: &str =
    "Chat was aborted because a referenced meeting was deleted";

pub(crate) async fn ensure_prompt_meetings_exist(
    pool: &SqlitePool,
    meeting_ids: &HashSet<String>,
) -> Result<(), String> {
    if meeting_ids.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::<Sqlite>::new("SELECT id FROM meetings WHERE id IN (");
    let mut ids = query.separated(", ");
    for meeting_id in meeting_ids {
        ids.push_bind(meeting_id);
    }
    drop(ids);
    query.push(")");
    match query.build_query_scalar::<String>().fetch_all(pool).await {
        Ok(existing) if existing.len() == meeting_ids.len() => Ok(()),
        Ok(_) => Err(DELETED_MEETING_EVIDENCE_ERROR.to_string()),
        Err(error) => {
            // Fail closed WITHOUT disclosing database internals: the raw error
            // is logged, the caller (and through it the UI) receives the same
            // stable, content-free revalidation message the post-start
            // terminal paths already publish.
            tracing::error!("Chat prompt existence check failed: {error}");
            Err(CHAT_CONTEXT_REVALIDATION_ERROR.to_string())
        }
    }
}

fn completed_chat_stream_payload(
    stream_id: &str,
    answer: &str,
    sources: &[ChatSource],
) -> serde_json::Value {
    serde_json::json!({ "streamId": stream_id, "answer": answer, "sources": sources })
}

async fn claim_chat_stream(state: &ChatRequestState, stream_id: &str) -> ChatRequestToken {
    state.claim_superseding_request(ChatRequestSurface::Chat, stream_id)
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

async fn await_chat_stream_with_timeout<S: ChatEventSink>(
    timeout: Duration,
    state: &ChatRequestState,
    sink: &S,
    stream_id: &str,
    token: &ChatRequestToken,
    stream: impl std::future::Future<Output = Result<(), String>>,
) -> Result<(), String> {
    match tokio::time::timeout(timeout, stream).await {
        Ok(result) => result,
        Err(_) if finish_timed_out_chat_stream(state, sink, stream_id, token) => Ok(()),
        Err(_) => Err("Chat request timed out".to_string()),
    }
}

fn finish_timed_out_chat_stream<S: ChatEventSink>(
    state: &ChatRequestState,
    sink: &S,
    stream_id: &str,
    token: &ChatRequestToken,
) -> bool {
    state.finish_chat_stream_if_current(
        stream_id,
        token,
        true,
        Some((
            "chat-stream-error",
            serde_json::json!({
                "streamId": stream_id,
                "error": CHAT_CONTEXT_REVALIDATION_ERROR,
                "safeCleanup": true,
            }),
        )),
        |event, payload| {
            sink.emit(event, payload);
        },
    )
}

fn emit_chat_stream_event_if_sink<S: ChatEventSink>(
    state: &ChatRequestState,
    sink: &S,
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
        |event, payload| sink.emit(event, payload),
    )
}

/// Local deletion notification (R72): emitted once AFTER a committed meeting
/// deletion so the renderer can drop that meeting's retained sources from
/// already-loaded chat messages. The payload carries the stable meeting
/// identity only — never content, snippets, or sources.
pub(crate) const CHAT_MEETING_DELETED_EVENT: &str = "chat-meeting-deleted";

/// Emits [`CHAT_MEETING_DELETED_EVENT`] exactly when `deletion` is a
/// committed deletion (`Ok(true)`); `Ok(false)` and every error/rollback emit
/// nothing, so the notification can never precede the commit it describes.
pub(crate) fn emit_chat_meeting_deleted_if_committed<E>(
    emit: impl FnOnce(&str, serde_json::Value),
    meeting_id: &str,
    deletion: &Result<bool, E>,
) -> bool {
    if !matches!(deletion, Ok(true)) {
        return false;
    }
    emit(
        CHAT_MEETING_DELETED_EVENT,
        serde_json::json!({ "meetingId": meeting_id }),
    );
    true
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
    state.publish_chat_stream_event_if_current(stream_id, token, event, payload, clear, emit)
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

    async fn configure_scope_chat(
        pool: &SqlitePool,
        force_lexical: bool,
        ollama_endpoint: Option<&str>,
    ) {
        sqlx::query("CREATE TABLE settings (id TEXT PRIMARY KEY, provider TEXT NOT NULL, model TEXT NOT NULL, whisperModel TEXT NOT NULL, groqApiKey TEXT, openaiApiKey TEXT, anthropicApiKey TEXT, ollamaApiKey TEXT, openRouterApiKey TEXT, ollamaEndpoint TEXT, customOpenAIConfig TEXT, customVocabulary TEXT, chatProvider TEXT, chatModel TEXT, chatOllamaEndpoint TEXT, force_lexical_retrieval BOOLEAN NOT NULL DEFAULT FALSE)")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO settings (id, provider, model, whisperModel, chatProvider, chatModel, chatOllamaEndpoint, force_lexical_retrieval) VALUES ('1', 'ollama', 'local', 'whisper', 'ollama', 'local', ?, ?)")
            .bind(ollama_endpoint)
            .bind(force_lexical)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("UPDATE meetings SET saved_at = '2026-09-01T00:00:00Z'")
            .execute(pool)
            .await
            .unwrap();
    }

    async fn set_scope_note(pool: &SqlitePool, meeting_id: &str, text: &str) {
        sqlx::query(
            "INSERT OR REPLACE INTO meeting_notes (meeting_id, notes_markdown) VALUES (?, ?)",
        )
        .bind(meeting_id)
        .bind(text)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn serve_deep_finish() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 64 * 1024];
            let _ = socket.read(&mut request).await;
            let body = r#"{"choices":[{"message":{"content":"{\"schemaVersion\":1,\"status\":\"finish\"}"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
        });
        format!("http://{}", address)
    }

    /// Serves one rewrite response and then one planner finish action, so a
    /// Deep request with rewrite-eligible history exercises both provider
    /// round trips against one fake endpoint.
    async fn serve_rewrite_then_planner_finish() -> String {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let served = calls.fetch_add(1, Ordering::SeqCst);
                let mut request = vec![0_u8; 64 * 1024];
                let _ = socket.read(&mut request).await;
                let content = if served == 0 {
                    "rewritten words"
                } else {
                    "{\"schemaVersion\":1,\"status\":\"finish\"}"
                };
                let escaped = content.replace('\\', "\\\\").replace('"', "\\\"");
                let body =
                    format!("{{\"choices\":[{{\"message\":{{\"content\":\"{escaped}\"}}}}]}}");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
            }
        });
        format!("http://{}", address)
    }

    const HYBRID_MODEL_ID: &str = "chat-test-model";
    const HYBRID_DIMENSIONS: usize = 4;
    const HYBRID_BEFORE: &str = "Opening remarks.";
    const HYBRID_TARGET: &str = "The budget was approved.";
    const HYBRID_AFTER: &str = "Closing remarks.";

    struct HybridTestEmbedder;

    impl crate::retrieval::worker::DocumentEmbedder for HybridTestEmbedder {
        fn model_id(&self) -> String {
            HYBRID_MODEL_ID.to_string()
        }

        fn dimensions(&self) -> usize {
            HYBRID_DIMENSIONS
        }

        fn count_tokens(&self, text: &str) -> usize {
            text.split_whitespace().count()
        }

        fn embed_documents_blocking(
            &self,
            texts: &[String],
            _cancel: &tokio_util::sync::CancellationToken,
        ) -> Result<Vec<Vec<f32>>, crate::retrieval::model::RetrievalModelError> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }

        fn embed_queries_blocking(
            &self,
            texts: &[String],
            _cancel: &tokio_util::sync::CancellationToken,
        ) -> Result<Vec<Vec<f32>>, crate::retrieval::model::RetrievalModelError> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }
    }

    fn hybrid_test_lifecycle() -> crate::retrieval::worker::RetrievalLifecycle {
        let embedder: Arc<dyn crate::retrieval::worker::DocumentEmbedder> =
            Arc::new(HybridTestEmbedder);
        let loader: crate::retrieval::worker::EngineLoader =
            Arc::new(move || Ok(Arc::clone(&embedder)));
        crate::retrieval::worker::RetrievalLifecycle::new(
            crate::retrieval::worker::LifecycleConfig::testing(Arc::new(|| false), loader),
        )
    }

    fn semantic_unavailable_lifecycle() -> crate::retrieval::worker::RetrievalLifecycle {
        crate::retrieval::worker::RetrievalLifecycle::new(
            crate::retrieval::worker::LifecycleConfig::testing(
                Arc::new(|| false),
                Arc::new(|| Err("semantic unavailable".to_string())),
            ),
        )
    }

    async fn hybrid_test_pool() -> SqlitePool {
        use std::str::FromStr;

        let options = sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn configure_hybrid_chat(pool: &SqlitePool) {
        sqlx::query(
            "INSERT INTO settings (id, provider, model, whisperModel, chatProvider, chatModel) VALUES ('1', 'ollama', 'local', 'whisper', 'ollama', 'local')",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    fn hybrid_transcript_text(index: usize) -> String {
        match index {
            0 => HYBRID_BEFORE.to_string(),
            1 => HYBRID_TARGET.to_string(),
            2 => HYBRID_AFTER.to_string(),
            index => format!("Chronological filler {index:02}"),
        }
    }

    async fn insert_hybrid_meeting(pool: &SqlitePool, transcript_count: usize) {
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m2', 'Hybrid meeting', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO meeting_notes (meeting_id, notes_markdown, created_at, updated_at) VALUES ('m2', 'Current notes', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES ('m2', 'summary', 'completed', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '{\"markdown\":\"Authoritative summary\"}')",
        )
        .execute(pool)
        .await
        .unwrap();
        for index in 0..transcript_count {
            let id = match index {
                0 => "before".to_string(),
                1 => "target".to_string(),
                2 => "after".to_string(),
                index => format!("filler-{index:02}"),
            };
            sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, 'm2', ?, '10:00', ?)",
            )
            .bind(id)
            .bind(hybrid_transcript_text(index))
            .bind(index as f64)
            .execute(pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m2', 'transcript', 'target', ?)",
        )
        .bind(HYBRID_TARGET)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE search_source_state SET fts_indexed_revision = fts_projection_revision WHERE meeting_id = 'm2'",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn publish_hybrid_document(
        pool: &SqlitePool,
        lifecycle: &crate::retrieval::worker::RetrievalLifecycle,
        meeting_id: &str,
        document_id: &str,
        start_id: &str,
        end_id: &str,
        content: &str,
    ) {
        use crate::database::repositories::retrieval::{
            ModelSpec, ReplacementJob, ReplacementOutcome, RetrievalRepository, StagedDocument,
            VectorEncoding,
        };

        assert!(RetrievalRepository::ensure_model(
            pool,
            &ModelSpec {
                model_id: HYBRID_MODEL_ID.to_string(),
                dimensions: HYBRID_DIMENSIONS as u32,
                vector_encoding: VectorEncoding::Int8,
                chunker_version: 1,
                dequantization_scale: Some(1.0 / 127.0),
                dequantization_zero_point: Some(0),
            },
        )
        .await
        .unwrap());
        assert!(
            RetrievalRepository::ensure_generation(pool, "gen-chat-hybrid", HYBRID_MODEL_ID,)
                .await
                .unwrap()
        );
        let revision = RetrievalRepository::current_source_revision(pool, meeting_id)
            .await
            .unwrap()
            .unwrap();
        let job_id = format!("job-chat-hybrid-{meeting_id}");
        RetrievalRepository::stage_documents(
            pool,
            &job_id,
            "gen-chat-hybrid",
            meeting_id,
            revision,
            &[StagedDocument {
                document_id: document_id.to_string(),
                source_kind: "transcript".to_string(),
                source_start_id: Some(start_id.to_string()),
                source_end_id: Some(end_id.to_string()),
                source_template_id: None,
                heading: None,
                ordinal: 0,
                content: content.to_string(),
                content_hash: vec![0; 32],
                dimensions: HYBRID_DIMENSIONS as i64,
                vector_encoding: VectorEncoding::Int8,
                vector: vec![127, 0, 0, 0],
            }],
        )
        .await
        .unwrap();
        assert!(matches!(
            RetrievalRepository::replace_meeting_documents(
                pool,
                ReplacementJob {
                    generation_id: "gen-chat-hybrid",
                    meeting_id,
                    expected_source_revision: revision,
                    job_id: &job_id,
                },
            )
            .await
            .unwrap(),
            ReplacementOutcome::Published { .. }
        ));
        lifecycle.index_service().set_loaded_model(HYBRID_MODEL_ID);
        crate::retrieval::index::publish_tick(pool, lifecycle.index_service().as_ref())
            .await
            .unwrap();
    }

    async fn mark_hybrid_meeting_ready(pool: &SqlitePool, meeting_id: &str) {
        use crate::database::repositories::retrieval::{
            ReplacementJob, ReplacementOutcome, RetrievalRepository, StagedDocument,
        };

        let revision = RetrievalRepository::current_source_revision(pool, meeting_id)
            .await
            .unwrap()
            .unwrap();
        let job_id = format!("job-chat-hybrid-empty-{meeting_id}");
        RetrievalRepository::stage_documents(
            pool,
            &job_id,
            "gen-chat-hybrid",
            meeting_id,
            revision,
            &[] as &[StagedDocument],
        )
        .await
        .unwrap();
        assert!(matches!(
            RetrievalRepository::replace_meeting_documents(
                pool,
                ReplacementJob {
                    generation_id: "gen-chat-hybrid",
                    meeting_id,
                    expected_source_revision: revision,
                    job_id: &job_id,
                },
            )
            .await
            .unwrap(),
            ReplacementOutcome::Published { .. }
        ));
    }

    async fn hybrid_test_fixture(
        semantic_target: bool,
        transcript_count: usize,
    ) -> (SqlitePool, crate::retrieval::worker::RetrievalLifecycle) {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        insert_hybrid_meeting(&pool, transcript_count).await;
        let lifecycle = hybrid_test_lifecycle();
        if semantic_target {
            publish_hybrid_document(
                &pool,
                &lifecycle,
                "m2",
                "doc-m2-target",
                "target",
                "target",
                HYBRID_TARGET,
            )
            .await;
        } else {
            sqlx::query(
                "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m3', 'Other meeting', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('other-target', 'm3', 'Other meeting', '10:00')",
            )
            .execute(&pool)
            .await
            .unwrap();
            publish_hybrid_document(
                &pool,
                &lifecycle,
                "m3",
                "doc-m3-target",
                "other-target",
                "other-target",
                "Other meeting",
            )
            .await;
            mark_hybrid_meeting_ready(&pool, "m2").await;
            crate::retrieval::index::publish_tick(&pool, lifecycle.index_service().as_ref())
                .await
                .unwrap();
            assert!(lifecycle.index_service().active_snapshot().is_some());
        }
        (pool, lifecycle)
    }

    async fn semantic_range_hybrid_test_fixture(
    ) -> (SqlitePool, crate::retrieval::worker::RetrievalLifecycle) {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        insert_hybrid_meeting(&pool, 12).await;
        let lifecycle = hybrid_test_lifecycle();
        let content = (4..=8)
            .map(hybrid_transcript_text)
            .collect::<Vec<_>>()
            .join("\n");
        publish_hybrid_document(
            &pool,
            &lifecycle,
            "m2",
            "doc-m2-range",
            "filler-04",
            "filler-08",
            &content,
        )
        .await;
        (pool, lifecycle)
    }

    async fn resume_scope_hybrid_test_fixture(
    ) -> (SqlitePool, crate::retrieval::worker::RetrievalLifecycle) {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        for (meeting_id, title, transcript) in [
            (
                "resume-meeting",
                "Resume discussion",
                "resume parsing decision",
            ),
            ("roadmap-meeting", "Roadmap discussion", "roadmap planning"),
        ] {
            sqlx::query(
                "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES (?, ?, '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
            )
            .bind(meeting_id)
            .bind(title)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, ?, ?, '10:00', 1.0)",
            )
            .bind(format!("{meeting_id}-transcript"))
            .bind(meeting_id)
            .bind(transcript)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES (?, 'transcript', ?, ?)",
            )
            .bind(meeting_id)
            .bind(format!("{meeting_id}-transcript"))
            .bind(transcript)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "UPDATE search_source_state SET fts_indexed_revision = fts_projection_revision",
        )
        .execute(&pool)
        .await
        .unwrap();
        let lifecycle = hybrid_test_lifecycle();
        publish_hybrid_document(
            &pool,
            &lifecycle,
            "resume-meeting",
            "doc-resume-meeting",
            "resume-meeting-transcript",
            "resume-meeting-transcript",
            "resume parsing decision",
        )
        .await;
        (pool, lifecycle)
    }

    async fn today_deep_test_fixture() -> (SqlitePool, crate::retrieval::worker::RetrievalLifecycle)
    {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        let today = Local::now().format("%Y-%m-%dT12:00:00").to_string();
        for (meeting_id, created_at) in [
            ("today-one", today.clone()),
            ("today-two", today),
            ("old-meeting", "2020-01-01T12:00:00".to_string()),
        ] {
            sqlx::query(
                "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(meeting_id)
            .bind(format!("Meeting {meeting_id}"))
            .bind(&created_at)
            .bind(&created_at)
            .bind(&created_at)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO meeting_notes (meeting_id, notes_markdown, created_at, updated_at) VALUES (?, ?, ?, ?)",
            )
            .bind(meeting_id)
            .bind(format!("Authoritative notes for {meeting_id}"))
            .bind(&created_at)
            .bind(&created_at)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES (?, 'note', ?, 'agenda')",
            )
            .bind(meeting_id)
            .bind(format!("note-{meeting_id}"))
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "UPDATE search_source_state SET fts_indexed_revision = fts_projection_revision",
        )
        .execute(&pool)
        .await
        .unwrap();
        (pool, semantic_unavailable_lifecycle())
    }

    async fn prepare_hybrid(
        pool: &SqlitePool,
        lifecycle: &crate::retrieval::worker::RetrievalLifecycle,
        query: &str,
    ) -> ChatInputs {
        prepare_chat_inputs_for_scope(
            pool,
            None,
            &reqwest::Client::new(),
            query,
            None,
            ChatRetrievalScope::Meeting("m2".to_string()),
            None,
            Some(lifecycle.clone()),
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap()
    }

    fn assert_hybrid_sources(inputs: &ChatInputs, expected: &[String]) {
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.snippet.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(inputs
            .sources
            .iter()
            .all(|source| source.meeting_id == "m2" && source.chunk_type == "transcript"));
        for snippet in expected {
            assert!(inputs.user_prompt.contains(snippet));
        }
        assert!(inputs.user_prompt.contains("Authoritative summary"));
        assert!(inputs.user_prompt.contains("Current notes"));
    }

    async fn long_hybrid_test_fixture() -> (SqlitePool, crate::retrieval::worker::RetrievalLifecycle)
    {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m2', 'Long meeting', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO meeting_notes (meeting_id, notes_markdown, created_at, updated_at) VALUES ('m2', 'Current notes', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES ('m2', 'summary', 'completed', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '{\"markdown\":\"Authoritative summary\"}')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            WITH RECURSIVE sequence(n) AS (
                SELECT 0
                UNION ALL
                SELECT n + 1 FROM sequence WHERE n < 10000
            )
            INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time)
            SELECT printf('long-%05d', n), 'm2',
                   CASE WHEN n = 10000 THEN 'needle after cap'
                        ELSE printf('Long filler %05d', n) END,
                   '10:00', n
            FROM sequence
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m2', 'transcript', 'long-10000', 'needle after cap')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE search_source_state SET fts_indexed_revision = fts_projection_revision WHERE meeting_id = 'm2'",
        )
        .execute(&pool)
        .await
        .unwrap();
        (pool, semantic_unavailable_lifecycle())
    }

    #[tokio::test]
    async fn saved_meeting_hybrid_semantic_paraphrase_reaches_final_prompt() {
        let (pool, lifecycle) = hybrid_test_fixture(true, 3).await;
        let inputs = prepare_hybrid(&pool, &lifecycle, "funding outlook").await;
        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::Hybrid
        );
        assert_hybrid_sources(
            &inputs,
            &vec![
                HYBRID_BEFORE.to_string(),
                HYBRID_TARGET.to_string(),
                HYBRID_AFTER.to_string(),
            ],
        );
    }

    #[tokio::test]
    async fn saved_meeting_hybrid_semantic_range_reaches_final_prompt() {
        let (pool, lifecycle) = semantic_range_hybrid_test_fixture().await;
        let inputs = prepare_hybrid(&pool, &lifecycle, "funding outlook").await;
        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::Hybrid
        );
        let expected = (3..=9).map(hybrid_transcript_text).collect::<Vec<_>>();
        assert_hybrid_sources(&inputs, &expected);
        assert!(inputs
            .user_prompt
            .contains("Partial transcript coverage: 7/12 segments"));
    }

    #[tokio::test]
    async fn snapshot_broad_active_semantic_range_reaches_final_prompt() {
        let (pool, lifecycle) = semantic_range_hybrid_test_fixture().await;
        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Summarize these meetings",
            None,
            ChatRetrievalScope::SearchSnapshot(vec!["m2".to_string()]),
            None,
            Some(lifecycle),
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::Hybrid
        );
        let transcript = inputs
            .sources
            .iter()
            .find(|source| source.chunk_type == "transcript")
            .expect("semantic transcript source");
        for index in 3..=9 {
            assert!(transcript.snippet.contains(&hybrid_transcript_text(index)));
        }
        for source in &inputs.sources {
            for line in source
                .snippet
                .lines()
                .filter(|line| !line.trim().is_empty())
            {
                assert!(inputs.user_prompt.contains(line));
            }
        }
    }

    #[tokio::test]
    async fn saved_meeting_hybrid_exact_query_preserves_final_sources() {
        let (pool, lifecycle) = hybrid_test_fixture(true, 3).await;
        let inputs = prepare_hybrid(&pool, &lifecycle, "budget approved").await;
        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::Hybrid
        );
        assert_hybrid_sources(
            &inputs,
            &vec![
                HYBRID_BEFORE.to_string(),
                HYBRID_TARGET.to_string(),
                HYBRID_AFTER.to_string(),
            ],
        );
    }

    #[tokio::test]
    async fn saved_meeting_hybrid_zero_hit_uses_chronological_head() {
        let (pool, lifecycle) = hybrid_test_fixture(false, 12).await;
        let inputs = prepare_hybrid(&pool, &lifecycle, "nothing matches").await;
        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::Hybrid
        );
        let expected = (0..10).map(hybrid_transcript_text).collect::<Vec<_>>();
        assert_hybrid_sources(&inputs, &expected);
        assert!(inputs
            .user_prompt
            .contains("Partial transcript coverage: 10/12 segments"));
    }

    #[tokio::test]
    async fn saved_meeting_hybrid_rehydrates_long_meeting_hits_with_true_coverage() {
        let (pool, lifecycle) = long_hybrid_test_fixture().await;
        let relevant = crate::database::repositories::retrieval::RetrievalRepository::
            load_meeting_source_relevant(&pool, "m2", &["long-10000".to_string()])
                .await
                .unwrap()
                .unwrap();
        assert_eq!(relevant.transcript_segments_total, 10001);
        assert!(relevant
            .transcripts
            .iter()
            .any(|row| row.id == "long-10000"));
        let hits =
            FtsRepository::search_transcripts_with_mode(&pool, "needle", 10, "m2", MatchMode::And)
                .await
                .unwrap();
        assert_eq!(
            hits.iter()
                .map(|hit| hit.chunk_id.as_str())
                .collect::<Vec<_>>(),
            ["long-10000"]
        );
        let inputs = prepare_hybrid(&pool, &lifecycle, "needle").await;
        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::SemanticFallback
        );
        assert_hybrid_sources(
            &inputs,
            &vec![
                "Long filler 09999".to_string(),
                "needle after cap".to_string(),
            ],
        );
        assert!(inputs
            .user_prompt
            .contains("Partial transcript coverage: 2/10001 segments"));
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

    #[test]
    fn hybrid_meeting_anchors_verify_semantic_ranges_and_ignore_metadata_limits() {
        use sha2::Digest;

        let source = crate::database::repositories::retrieval::MeetingSource {
            meeting_id: "m2".to_string(),
            title: "Child".to_string(),
            folder_name: String::new(),
            source_revision: None,
            latest_summary_template_id: None,
            latest_summary_markdown: None,
            notes_markdown: None,
            transcripts: [
                ("first", "first"),
                ("target", "semantic target"),
                ("after", "semantic after"),
                ("last", "last"),
            ]
            .into_iter()
            .map(
                |(id, text)| crate::database::repositories::retrieval::SourceTranscript {
                    id: id.to_string(),
                    text: text.to_string(),
                    speaker: None,
                    timestamp: "10:00".to_string(),
                    audio_start_time: None,
                    audio_end_time: None,
                },
            )
            .collect(),
            transcript_positions: vec![0, 1, 2, 3],
            transcript_segments_total: 4,
            complete: true,
        };
        let evidence =
            |id: &str,
             kind: &str,
             start: Option<&str>,
             end: Option<&str>,
             text: &str,
             provenance: Vec<crate::retrieval::EvidenceProvenance>,
             fingerprint: Option<Vec<u8>>| crate::retrieval::RankedEvidence {
                evidence: crate::retrieval::RetrievedEvidence {
                    evidence_id: id.to_string(),
                    meeting_id: "m2".to_string(),
                    meeting_title: "Child".to_string(),
                    source_kind: kind.to_string(),
                    source_start_id: start.map(str::to_string),
                    source_end_id: end.map(str::to_string),
                    source_template_id: None,
                    heading: None,
                    ordinal: 0,
                    text: text.to_string(),
                    speaker: None,
                    timestamp_label: None,
                    provenance,
                    source_aliases: Vec::new(),
                },
                content_fingerprint: fingerprint,
                fused_rank: 1,
                fused_score: 1.0,
                reranker_score: None,
            };
        let semantic_text = "semantic target\nsemantic after";
        let semantic = evidence(
            "semantic-window",
            "transcript",
            Some("target"),
            Some("after"),
            semantic_text,
            vec![crate::retrieval::EvidenceProvenance {
                channel: crate::retrieval::RetrievalChannel::Semantic,
                variant: crate::retrieval::QueryVariantKind::Original,
                mode: None,
                rank: 1,
                query_slot: 0,
            }],
            Some(sha2::Sha256::digest(semantic_text.as_bytes()).to_vec()),
        );
        let summary = evidence(
            "summary",
            "summary",
            None,
            None,
            "metadata",
            Vec::new(),
            None,
        );
        assert_eq!(
            meeting_transcript_positions(&source, &[summary, semantic], 1)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );

        let stale = evidence(
            "stale-window",
            "transcript",
            Some("target"),
            Some("after"),
            semantic_text,
            vec![crate::retrieval::EvidenceProvenance {
                channel: crate::retrieval::RetrievalChannel::Semantic,
                variant: crate::retrieval::QueryVariantKind::Original,
                mode: None,
                rank: 1,
                query_slot: 0,
            }],
            Some(vec![0; 32]),
        );
        assert_eq!(
            meeting_transcript_positions(&source, std::slice::from_ref(&stale), 1)
                .into_iter()
                .collect::<Vec<_>>(),
            Vec::<usize>::new()
        );
        let lexical = evidence(
            "lexical-hit",
            "transcript",
            Some("target"),
            None,
            "semantic target",
            vec![crate::retrieval::EvidenceProvenance {
                channel: crate::retrieval::RetrievalChannel::Lexical,
                variant: crate::retrieval::QueryVariantKind::Original,
                mode: Some(crate::retrieval::LexicalMode::And),
                rank: 1,
                query_slot: 0,
            }],
            None,
        );
        assert_eq!(
            meeting_transcript_positions(&source, &[stale, lexical], 1)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
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
        let lifecycle = crate::retrieval::worker::RetrievalLifecycle::new(
            crate::retrieval::worker::LifecycleConfig::testing(
                std::sync::Arc::new(|| false),
                std::sync::Arc::new(|| Err("semantic unavailable".to_string())),
            ),
        );

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "needle",
            None,
            ChatRetrievalScope::Meeting("m2".to_string()),
            None,
            Some(lifecycle),
            None,
            Some(ChatRetrievalMode::Deep),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::SemanticFallback
        );
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
            Some(&"🦀".repeat(100)),
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

        assert_eq!(
            titles
                .iter()
                .map(|meeting| meeting.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Root", "Child"]
        );
        let (context, ids) = format_meeting_list_context(&titles, 64_000);
        assert!(!context.contains("total"));
        assert!(context.contains("- Root"));
        assert!(context.contains("- Child"));
        assert!(!context.contains("Other"));
        assert_eq!(ids, HashSet::from(["m1".to_string(), "m2".to_string()]));
        assert!(requests_meeting_list(
            "listar reuniões existentes nesta pasta",
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

        assert_eq!(
            titles
                .iter()
                .map(|meeting| meeting.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Child"]
        );
    }

    #[tokio::test]
    async fn duplicate_titles_bind_only_the_rendered_scope_rows() {
        let pool = scope_pool().await;
        sqlx::query("UPDATE meetings SET title = 'Duplicate' WHERE id IN ('m2', 'm3')")
            .execute(&pool)
            .await
            .unwrap();

        let (context, prompt_meeting_ids) = meeting_list_context_for_scope(
            &pool,
            &ChatRetrievalScope::Folder("root".to_string()),
            "list the meetings",
            None,
            64_000,
        )
        .await
        .unwrap();

        assert!(context.unwrap().contains("- Duplicate"));
        assert!(prompt_meeting_ids.contains("m1"));
        assert!(prompt_meeting_ids.contains("m2"));
        assert!(!prompt_meeting_ids.contains("m3"));
    }

    #[tokio::test]
    async fn truncated_meeting_list_binds_only_retained_titles_without_an_aggregate() {
        use crate::database::repositories::meeting::MeetingsRepository;

        let pool = hybrid_test_pool().await;
        for (id, parent_id) in [("root", None), ("child", Some("root")), ("other", None)] {
            sqlx::query(
                "INSERT INTO meeting_folders (id, name, parent_id, created_at) VALUES (?, ?, ?, '2026-09-02T00:00:00Z')",
            )
            .bind(id)
            .bind(id)
            .bind(parent_id)
            .execute(&pool)
            .await
            .unwrap();
        }
        for (id, folder_id) in [("m1", "root"), ("m2", "child"), ("m3", "other")] {
            sqlx::query(
                "INSERT INTO meetings (id, title, folder_id, created_at, updated_at, saved_at) VALUES (?, 'Duplicate', ?, '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
            )
            .bind(id)
            .bind(folder_id)
            .execute(&pool)
            .await
            .unwrap();
        }
        assert_eq!(
            meeting_titles_for_scope(
                &pool,
                &ChatRetrievalScope::Folder("root".to_string()),
                "list the meetings",
                None,
            )
            .await
            .unwrap()
            .into_iter()
            .map(|meeting| meeting.id)
            .collect::<Vec<_>>(),
            vec!["m1", "m2"]
        );
        let context_budget = MEETING_LIST_HEADER.chars().count() + "- Duplicate\n".chars().count();
        let (context, prompt_meeting_ids) = meeting_list_context_for_scope(
            &pool,
            &ChatRetrievalScope::Folder("root".to_string()),
            "list the meetings",
            None,
            context_budget,
        )
        .await
        .unwrap();
        let context = context.unwrap();

        assert_eq!(context, format!("{MEETING_LIST_HEADER}- Duplicate\n"));
        assert!(context.starts_with("Bounded rendered meeting-list context"));
        assert!(!context.contains("Authoritative meeting list"));
        assert!(!context.contains("total"));
        assert_eq!(prompt_meeting_ids, HashSet::from(["m1".to_string()]));

        let request_state = ChatRequestState::new();
        let token = request_state.claim_superseding_request(ChatRequestSurface::Chat, "short-list");
        assert!(request_state.bind_request_meetings(
            ChatRequestSurface::Chat,
            "short-list",
            &token,
            &prompt_meeting_ids,
        ));
        let invalidated = request_state.clone();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "m2", |meeting_id| {
                invalidated.invalidate_meeting(meeting_id);
            })
            .await
            .unwrap()
        );

        assert!(!token.is_cancelled());
        ensure_prompt_meetings_exist(&pool, &prompt_meeting_ids)
            .await
            .unwrap();
        assert!(!prompt_meeting_ids.contains("m3"));
        request_state.clear_if_owner(ChatRequestSurface::Chat, "short-list", &token);
    }

    #[test]
    fn broad_retrieval_intent_covers_summary_compare_and_list_requests() {
        assert!(requests_broad_retrieval("Summarize all meetings"));
        assert!(requests_broad_retrieval("Compare the decisions"));
        assert!(requests_broad_retrieval("Síntese das reuniões"));
        assert!(requests_broad_retrieval("list today's meetings"));
        assert!(!requests_broad_retrieval(
            "Which meeting discussed retention?"
        ));
        assert!(!requests_broad_retrieval(
            "Which meeting discussed resume parsing?"
        ));
    }

    #[tokio::test]
    async fn snapshot_factual_resume_query_reaches_final_prompt_and_sources() {
        let pool = scope_pool().await;
        configure_scope_chat(&pool, true, None).await;
        sqlx::query(
            "UPDATE meeting_fts SET text = CASE meeting_id WHEN 'm1' THEN 'resume parsing decision' WHEN 'm2' THEN 'roadmap planning' ELSE 'unrelated' END",
        )
        .execute(&pool)
        .await
        .unwrap();

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Which meeting discussed resume parsing?",
            None,
            ChatRetrievalScope::SearchSnapshot(vec!["m2".to_string(), "m1".to_string()]),
            None,
            None,
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::ForcedLexical
        );
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m1"]
        );
        assert!(inputs.sources[0].snippet.contains("resume"));
        assert!(inputs.user_prompt.contains(&inputs.sources[0].snippet));
        assert!(!inputs.user_prompt.contains("roadmap planning"));
    }

    #[tokio::test]
    async fn snapshot_factual_resume_query_with_available_lifecycle_stays_relevant() {
        let (pool, lifecycle) = resume_scope_hybrid_test_fixture().await;
        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Which meeting discussed resume parsing?",
            None,
            ChatRetrievalScope::SearchSnapshot(vec![
                "roadmap-meeting".to_string(),
                "resume-meeting".to_string(),
            ]),
            None,
            Some(lifecycle),
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        assert_ne!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::LifecycleUnavailable
        );
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["resume-meeting"]
        );
        assert!(inputs.sources[0].snippet.contains("resume"));
        assert!(inputs.sources[0].snippet.contains("parsing"));
        assert!(inputs.user_prompt.contains(&inputs.sources[0].snippet));
        assert!(!inputs.user_prompt.contains("roadmap planning"));
    }

    #[tokio::test]
    async fn snapshot_factual_resume_query_with_unavailable_lifecycle_stays_relevant() {
        let (pool, _) = resume_scope_hybrid_test_fixture().await;
        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Which meeting discussed resume parsing?",
            None,
            ChatRetrievalScope::SearchSnapshot(vec![
                "roadmap-meeting".to_string(),
                "resume-meeting".to_string(),
            ]),
            None,
            None,
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::LifecycleUnavailable
        );
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["resume-meeting"]
        );
        assert!(inputs.sources[0].snippet.contains("resume"));
        assert!(inputs.sources[0].snippet.contains("parsing"));
        assert!(inputs.user_prompt.contains(&inputs.sources[0].snippet));
        assert!(!inputs.user_prompt.contains("roadmap planning"));
    }

    #[tokio::test]
    async fn today_factual_resume_query_reaches_only_relevant_today_member() {
        let pool = scope_pool().await;
        configure_scope_chat(&pool, true, None).await;
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
        sqlx::query(
            "UPDATE meeting_fts SET text = CASE meeting_id WHEN 'm1' THEN 'resume parsing decision' WHEN 'm2' THEN 'roadmap planning' ELSE 'resume parsing old' END",
        )
        .execute(&pool)
        .await
        .unwrap();

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Which meeting discussed resume parsing today?",
            None,
            ChatRetrievalScope::All,
            None,
            None,
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m1"]
        );
        assert!(inputs.sources[0].snippet.contains("resume"));
        assert!(inputs.user_prompt.contains(&inputs.sources[0].snippet));
        assert!(!inputs
            .sources
            .iter()
            .any(|source| source.snippet.contains("old")));
        assert!(inputs.user_prompt.contains("Current local date:"));
    }

    #[tokio::test]
    async fn today_broad_lifecycle_unavailable_reaches_all_live_today_members() {
        let pool = scope_pool().await;
        configure_scope_chat(&pool, false, None).await;
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
            set_scope_note(
                &pool,
                meeting_id,
                &format!("Authoritative notes for {meeting_id}"),
            )
            .await;
        }

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Summarize today's meetings",
            None,
            ChatRetrievalScope::All,
            None,
            None,
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::LifecycleUnavailable
        );
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["m1", "m2"])
        );
        assert!(inputs
            .sources
            .iter()
            .all(|source| inputs.user_prompt.contains(&source.snippet)));
        assert!(!inputs.user_prompt.contains("Authoritative notes for m3"));
    }

    #[tokio::test]
    async fn snapshot_broad_lifecycle_unavailable_reaches_all_live_members() {
        let pool = scope_pool().await;
        configure_scope_chat(&pool, false, None).await;
        for meeting_id in ["m1", "m2", "m3"] {
            set_scope_note(
                &pool,
                meeting_id,
                &format!("Authoritative notes for {meeting_id}"),
            )
            .await;
        }
        sqlx::query("DELETE FROM meetings WHERE id = 'm2'")
            .execute(&pool)
            .await
            .unwrap();

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Summarize these meetings",
            None,
            ChatRetrievalScope::SearchSnapshot(vec![
                "m1".to_string(),
                "m2".to_string(),
                "m3".to_string(),
            ]),
            None,
            None,
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::LifecycleUnavailable
        );
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m1", "m3"]
        );
        assert!(inputs
            .sources
            .iter()
            .all(|source| inputs.user_prompt.contains(&source.snippet)));
        assert!(!inputs.user_prompt.contains("m2"));
    }

    #[tokio::test]
    async fn snapshot_broad_hybrid_reaches_six_members_after_ranked_retrieval() {
        let pool = scope_pool().await;
        configure_scope_chat(&pool, false, None).await;
        let mut meeting_ids = vec!["m1".to_string(), "m2".to_string(), "m3".to_string()];
        for index in 4..=6 {
            let meeting_id = format!("m{index}");
            sqlx::query(
                "INSERT INTO meetings (id, title, saved_at) VALUES (?, ?, '2026-09-01T00:00:00Z')",
            )
            .bind(&meeting_id)
            .bind(format!("Meeting {meeting_id}"))
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES (?, 'note', ?, 'alpha')",
            )
            .bind(&meeting_id)
            .bind(format!("note-{meeting_id}"))
            .execute(&pool)
            .await
            .unwrap();
            set_scope_note(
                &pool,
                &meeting_id,
                &format!("Authoritative notes for {meeting_id}"),
            )
            .await;
            meeting_ids.push(meeting_id);
        }
        for meeting_id in ["m1", "m2", "m3"] {
            set_scope_note(
                &pool,
                meeting_id,
                &format!("Authoritative notes for {meeting_id}"),
            )
            .await;
        }

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Summarize these meetings",
            None,
            ChatRetrievalScope::SearchSnapshot(meeting_ids.clone()),
            None,
            Some(semantic_unavailable_lifecycle()),
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        assert_eq!(inputs.provider_round_trips, 0);
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.as_str())
                .collect::<BTreeSet<_>>(),
            meeting_ids.iter().map(String::as_str).collect()
        );
        assert!(inputs
            .sources
            .iter()
            .all(|source| inputs.user_prompt.contains(&source.snippet)));
    }

    #[tokio::test]
    async fn snapshot_broad_hundred_member_scope_stays_bounded_and_ordered() {
        let pool = scope_pool().await;
        configure_scope_chat(&pool, false, None).await;
        let meeting_ids = (0..100)
            .map(|index| format!("r50-member-{index:03}"))
            .collect::<Vec<_>>();
        for meeting_id in &meeting_ids {
            sqlx::query(
                "INSERT INTO meetings (id, title, created_at, saved_at) VALUES (?, ?, '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
            )
            .bind(meeting_id)
            .bind(format!("Meeting {meeting_id}"))
            .execute(&pool)
            .await
            .unwrap();
            set_scope_note(
                &pool,
                meeting_id,
                &format!("Authoritative notes for {meeting_id}"),
            )
            .await;
        }

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Summarize all meetings",
            None,
            ChatRetrievalScope::SearchSnapshot(meeting_ids.clone()),
            None,
            None,
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        assert_eq!(inputs.sources.len(), 100);
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.clone())
                .collect::<Vec<_>>(),
            meeting_ids
        );
        assert!(inputs.user_prompt.chars().count() <= 64_000);
        assert!(inputs
            .sources
            .iter()
            .all(|source| inputs.user_prompt.contains(&source.snippet)));
    }

    #[tokio::test]
    async fn snapshot_broad_max_length_ids_stay_within_prompt_and_source_budget() {
        let pool = scope_pool().await;
        configure_scope_chat(&pool, false, None).await;
        let meeting_ids = (0..100)
            .map(|index| {
                let prefix = format!("r51-member-{index:03}-");
                format!("{prefix}{}", "x".repeat(512 - prefix.len()))
            })
            .collect::<Vec<_>>();
        assert!(meeting_ids.iter().all(|meeting_id| meeting_id.len() == 512));
        ChatScope {
            kind: ChatScopeKind::SearchSnapshot,
            key: "r51".to_string(),
            data: Some(ChatScopeData {
                result_ids: meeting_ids.clone(),
            }),
        }
        .validate()
        .unwrap();
        for (index, meeting_id) in meeting_ids.iter().enumerate() {
            sqlx::query(
                "INSERT INTO meetings (id, title, created_at, saved_at) VALUES (?, ?, '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
            )
            .bind(meeting_id)
            .bind(format!("Meeting {index}"))
            .execute(&pool)
            .await
            .unwrap();
            set_scope_note(
                &pool,
                meeting_id,
                &format!("Authoritative notes for {index}"),
            )
            .await;
        }

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Summarize all meetings",
            None,
            ChatRetrievalScope::SearchSnapshot(meeting_ids.clone()),
            None,
            None,
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        assert_eq!(inputs.sources.len(), 100);
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.clone())
                .collect::<Vec<_>>(),
            meeting_ids
        );
        assert!(inputs.user_prompt.chars().count() <= 64_000);
        assert!(inputs
            .sources
            .iter()
            .all(|source| inputs.user_prompt.contains(&source.snippet)));
    }

    #[tokio::test]
    async fn snapshot_broad_deep_supported_path_reaches_final_prompt_and_sources() {
        let pool = scope_pool().await;
        let endpoint = serve_deep_finish().await;
        configure_scope_chat(&pool, false, Some(&endpoint)).await;
        set_scope_note(&pool, "m1", "Authoritative notes for m1").await;
        set_scope_note(&pool, "m2", "Authoritative notes for m2").await;

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Summarize these meetings",
            None,
            ChatRetrievalScope::SearchSnapshot(vec!["m1".to_string(), "m2".to_string()]),
            None,
            Some(semantic_unavailable_lifecycle()),
            None,
            Some(ChatRetrievalMode::Deep),
            None,
        )
        .await
        .unwrap();

        assert_eq!(inputs.retrieval_mode, ChatRetrievalMode::Deep);
        assert_eq!(
            inputs.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::SemanticFallback
        );
        assert_eq!(inputs.provider_round_trips, 1);
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["m1", "m2"])
        );
        assert!(inputs
            .sources
            .iter()
            .all(|source| inputs.user_prompt.contains(&source.snippet)));
    }

    #[tokio::test]
    async fn today_deep_preparation_reaches_final_prompt_for_today_members() {
        let (pool, lifecycle) = today_deep_test_fixture().await;
        let endpoint = serve_deep_finish().await;
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(endpoint)
            .execute(&pool)
            .await
            .unwrap();

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Summarize today's meetings",
            None,
            ChatRetrievalScope::All,
            None,
            Some(lifecycle),
            None,
            Some(ChatRetrievalMode::Deep),
            None,
        )
        .await
        .unwrap();

        assert_eq!(inputs.retrieval_mode, ChatRetrievalMode::Deep);
        assert_eq!(inputs.provider_round_trips, 1);
        assert_eq!(
            inputs
                .sources
                .iter()
                .map(|source| source.meeting_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["today-one", "today-two"])
        );
        assert!(inputs
            .sources
            .iter()
            .all(|source| inputs.user_prompt.contains(&source.snippet)));
        assert!(!inputs.user_prompt.contains("old-meeting"));
    }

    #[tokio::test]
    async fn force_lexical_governs_the_shared_boundary_and_restores_hybrid() {
        let (pool, lifecycle) = resume_scope_hybrid_test_fixture().await;
        let endpoint = serve_deep_finish().await;
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(endpoint)
            .execute(&pool)
            .await
            .unwrap();

        // Unforced Deep request: the planner runs at the shared boundary.
        let unforced = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Which meeting discussed resume parsing?",
            None,
            ChatRetrievalScope::SearchSnapshot(vec![
                "roadmap-meeting".to_string(),
                "resume-meeting".to_string(),
            ]),
            None,
            Some(lifecycle.clone()),
            None,
            Some(ChatRetrievalMode::Deep),
            None,
        )
        .await
        .unwrap();
        assert_eq!(unforced.retrieval_mode, ChatRetrievalMode::Deep);
        assert_ne!(
            unforced.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::ForcedLexical
        );
        assert_eq!(unforced.provider_round_trips, 1);

        // Enable: the very next request resolves to forced lexical Fast with
        // the typed reason, no planner round trip, and no progress events.
        sqlx::query("UPDATE settings SET force_lexical_retrieval = TRUE")
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
        let forced = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Which meeting discussed resume parsing?",
            None,
            ChatRetrievalScope::SearchSnapshot(vec![
                "roadmap-meeting".to_string(),
                "resume-meeting".to_string(),
            ]),
            None,
            Some(lifecycle.clone()),
            None,
            Some(ChatRetrievalMode::Deep),
            Some(&sink),
        )
        .await
        .unwrap();
        assert_eq!(
            forced.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::ForcedLexical
        );
        assert_eq!(forced.retrieval_mode, ChatRetrievalMode::Fast);
        assert_eq!(forced.provider_round_trips, 0);
        assert!(progress.lock().unwrap().is_empty());

        // Disable: hybrid Deep is restored on the next request.
        sqlx::query("UPDATE settings SET force_lexical_retrieval = FALSE")
            .execute(&pool)
            .await
            .unwrap();
        let restored = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Which meeting discussed resume parsing?",
            None,
            ChatRetrievalScope::SearchSnapshot(vec![
                "roadmap-meeting".to_string(),
                "resume-meeting".to_string(),
            ]),
            None,
            Some(lifecycle),
            None,
            Some(ChatRetrievalMode::Deep),
            None,
        )
        .await
        .unwrap();
        assert_eq!(restored.retrieval_mode, ChatRetrievalMode::Deep);
        assert_ne!(
            restored.retrieval_diagnostic,
            RetrievalPreparationDiagnostic::ForcedLexical
        );
        assert_eq!(restored.provider_round_trips, 1);
    }

    #[tokio::test]
    async fn saved_meeting_deep_runs_bounded_planner_and_keeps_authoritative_anchors() {
        let (pool, lifecycle) = hybrid_test_fixture(true, 3).await;
        let endpoint = serve_saved_meeting_deep_two_rounds().await;
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(endpoint)
            .execute(&pool)
            .await
            .unwrap();
        let fast = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "approved budget",
            None,
            ChatRetrievalScope::Meeting("m2".to_string()),
            None,
            Some(lifecycle.clone()),
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
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
        let deep = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "approved budget",
            None,
            ChatRetrievalScope::Meeting("m2".to_string()),
            None,
            Some(lifecycle),
            None,
            Some(ChatRetrievalMode::Deep),
            Some(&sink),
        )
        .await
        .unwrap();

        // Deep actually runs: the planner is invoked (two provider round
        // trips), stage-level progress is emitted, and the authoritative
        // mandatory summary/notes anchors are retained verbatim.
        assert_eq!(deep.retrieval_mode, ChatRetrievalMode::Deep);
        assert_eq!(deep.provider_round_trips, 2);
        assert!(!progress.lock().unwrap().is_empty());
        assert!(deep.user_prompt.contains("Authoritative summary"));
        assert!(deep.user_prompt.contains("Current notes"));
        assert!(deep.user_prompt.contains(HYBRID_TARGET));
        // Fast remains one-pass with no planner.
        assert_eq!(fast.provider_round_trips, 0);
        // Final sources keep transcript parity for the saved meeting.
        assert!(!deep.sources.is_empty());
        assert!(deep.sources.iter().all(|source| source.meeting_id == "m2"));
        // Every published source's snippet is present in the final prompt.
        assert!(deep
            .sources
            .iter()
            .all(|source| deep.user_prompt.contains(&source.snippet)));
    }

    #[test]
    fn saved_meeting_deep_merge_drops_evidence_that_cannot_survive_the_final_budget() {
        let mut transcript = make_search_result("m2", "Saved", "transcript", HYBRID_TARGET);
        transcript.chunk_id = "anchor".to_string();
        let built = build_meeting_context_markdown(
            "m2",
            "Saved",
            Some("Authoritative summary"),
            Some("Current notes"),
            &[transcript],
            1,
            4_000,
        );
        let authoritative = ChatSource {
            meeting_id: "m2".to_string(),
            meeting_title: "Saved".to_string(),
            chunk_type: "transcript".to_string(),
            snippet: HYBRID_TARGET.to_string(),
            folder_name: String::new(),
            source_kind: Some("transcript".to_string()),
        };
        let omitted = ChatSource {
            meeting_id: "m2".to_string(),
            meeting_title: "Saved".to_string(),
            chunk_type: "transcript".to_string(),
            snippet: "planner evidence that exceeds the remaining saved-meeting context budget"
                .to_string(),
            folder_name: String::new(),
            source_kind: Some("transcript".to_string()),
        };
        let budget = built.markdown.chars().count();
        let (context, sources) = merge_saved_meeting_deep_context(
            built.markdown,
            vec![authoritative],
            vec![omitted.clone()],
            budget,
        );
        let prompt = assemble_prompt(
            &context,
            &[],
            "question",
            "question",
            "",
            budget + "\n\nMeeting context:\n".chars().count() + 64,
        );

        assert!(context.contains("Authoritative summary"));
        assert!(context.contains("Current notes"));
        assert!(context.contains(HYBRID_TARGET));
        assert!(!context.contains(&omitted.snippet));
        assert!(!sources
            .iter()
            .any(|source| source.snippet == omitted.snippet));
        assert!(sources
            .iter()
            .all(|source| prompt.contains(&source.snippet)));
    }

    #[tokio::test]
    async fn saved_meeting_deep_cannot_open_another_meeting() {
        // Fixture where "m3" holds a matching document but is NOT the
        // requested meeting: the planner's card set is one-meeting, so an
        // open/search action can never pull "m3" content into the prompt or
        // sources.
        let (pool, lifecycle) = hybrid_test_fixture(false, 3).await;
        let endpoint = serve_saved_meeting_deep_two_rounds().await;
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(endpoint)
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
        let deep = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "approved budget",
            None,
            ChatRetrievalScope::Meeting("m2".to_string()),
            None,
            Some(lifecycle),
            None,
            Some(ChatRetrievalMode::Deep),
            Some(&sink),
        )
        .await
        .unwrap();

        // The planner ran, but no cross-meeting action escaped: sources and
        // retained context are strictly within the one-meeting allow-list.
        assert_eq!(deep.retrieval_mode, ChatRetrievalMode::Deep);
        assert_eq!(deep.provider_round_trips, 2);
        assert!(deep.sources.iter().all(|source| source.meeting_id == "m2"));
        assert!(!deep.user_prompt.contains("Other meeting"));
        assert!(deep.user_prompt.contains("Authoritative summary"));
        assert!(deep.user_prompt.contains("Current notes"));
    }

    #[tokio::test]
    async fn folder_deep_preparation_stays_inside_folder_membership() {
        let (pool, lifecycle) = hybrid_test_fixture(true, 3).await;
        let endpoint = serve_deep_finish().await;
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(endpoint)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO meeting_folders (id, name, created_at) VALUES ('f1', 'Folder', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE meetings SET folder_id = 'f1' WHERE id = 'm2'")
            .execute(&pool)
            .await
            .unwrap();
        // A matching meeting outside the folder: any appearance in the Deep
        // context or sources would be a scope leak.
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m3', 'Outsider', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('outsider', 'm3', 'Exclusive out-of-folder decision', '10:00')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m3', 'transcript', 'outsider', 'Exclusive out-of-folder decision')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "exclusive decision",
            None,
            ChatRetrievalScope::Folder("f1".to_string()),
            None,
            Some(lifecycle),
            None,
            Some(ChatRetrievalMode::Deep),
            None,
        )
        .await
        .unwrap();

        assert_eq!(inputs.retrieval_mode, ChatRetrievalMode::Deep);
        assert_eq!(inputs.provider_round_trips, 1);
        assert!(inputs
            .sources
            .iter()
            .all(|source| source.meeting_id == "m2"));
        assert!(!inputs.user_prompt.contains("Exclusive out-of-folder"));
    }

    #[tokio::test]
    async fn deep_with_rewritten_query_keeps_the_original_user_question() {
        let (pool, lifecycle) = resume_scope_hybrid_test_fixture().await;
        let endpoint = serve_rewrite_then_planner_finish().await;
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(endpoint)
            .execute(&pool)
            .await
            .unwrap();

        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "Which meeting discussed resume parsing?",
            Some(&vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: "What did we discuss last time?".to_string(),
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: "The resume parsing decision.".to_string(),
                },
            ]),
            ChatRetrievalScope::All,
            None,
            Some(lifecycle),
            None,
            Some(ChatRetrievalMode::Deep),
            None,
        )
        .await
        .unwrap();

        // Rewrite (1) plus the Deep planner call (2): the rewritten query may
        // drive retrieval, but the final prompt retains the original question.
        assert_eq!(inputs.retrieval_mode, ChatRetrievalMode::Deep);
        assert_eq!(inputs.provider_round_trips, 2);
        assert!(inputs
            .user_prompt
            .contains("User question: Which meeting discussed resume parsing?"));
        assert!(inputs.user_prompt.contains("Search query: rewritten words"));
    }

    #[tokio::test]
    async fn prepared_evidence_for_a_deleted_meeting_aborts_instead_of_filtering() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        for (meeting_id, title) in [("gone", "Deleted"), ("kept", "Survivor")] {
            sqlx::query(
                "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES (?, ?, '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
            )
            .bind(meeting_id)
            .bind(title)
            .execute(&pool)
            .await
            .unwrap();
        }
        let prepared_ids = HashSet::from(["gone".to_string(), "kept".to_string()]);
        ensure_prompt_meetings_exist(&pool, &prepared_ids)
            .await
            .unwrap();

        sqlx::query("DELETE FROM meetings WHERE id = 'gone'")
            .execute(&pool)
            .await
            .unwrap();

        // Abort (typed), never a filtered partial response: publishing a
        // source-less answer over retained deleted evidence would break
        // source/context parity.
        let error = ensure_prompt_meetings_exist(&pool, &prepared_ids)
            .await
            .unwrap_err();
        assert_eq!(error, DELETED_MEETING_EVIDENCE_ERROR);
        // A surviving meeting's sources still pass.
        ensure_prompt_meetings_exist(&pool, &HashSet::from(["kept".to_string()]))
            .await
            .unwrap();

        // Live-recording sources carry a scope key, not a meeting id, and are
        // exempt from the meeting existence check.
        ensure_prompt_meetings_exist(&pool, &HashSet::new())
            .await
            .unwrap();
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
    async fn snapshot_factual_query_is_relevant_and_stays_inside_membership() {
        let pool = scope_pool().await;
        sqlx::query(
            "UPDATE meeting_fts SET text = CASE meeting_id WHEN 'm1' THEN 'budget approved' WHEN 'm2' THEN 'roadmap planned' ELSE 'budget outside' END",
        )
        .execute(&pool)
        .await
        .unwrap();

        let results = resolve_scope_results(
            &pool,
            "budget",
            "budget",
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
            vec!["m1"]
        );
        assert!(results.iter().all(|result| result.meeting_id != "m3"));

        let fallback = resolve_scope_results(
            &pool,
            "unmatched",
            "unmatched",
            10,
            ChatRetrievalScope::SearchSnapshot(vec!["m2".to_string(), "m1".to_string()]),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            fallback
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

        assert_eq!(
            all_titles
                .iter()
                .map(|meeting| meeting.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Root", "Child"]
        );
        assert_eq!(
            folder_titles
                .iter()
                .map(|meeting| meeting.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Root", "Child"]
        );
        assert_eq!(
            snapshot_titles
                .iter()
                .map(|meeting| meeting.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Child"]
        );

        sqlx::query(
            "UPDATE meeting_fts SET text = CASE meeting_id WHEN 'm1' THEN 'budget decision' WHEN 'm2' THEN 'roadmap decision' ELSE 'budget decision' END",
        )
        .execute(&pool)
        .await
        .unwrap();
        let relevant = resolve_scope_results(
            &pool,
            "budget",
            "budget",
            10,
            ChatRetrievalScope::All,
            Some(&all_ids),
        )
        .await
        .unwrap();
        assert_eq!(
            relevant
                .iter()
                .map(|result| result.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["m1"]
        );
        assert!(relevant.iter().all(|result| result.meeting_id != "m3"));

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
    fn deep_preparation_eligibility_requires_deep_mode() {
        assert!(deep_preparation_eligible(ChatRetrievalMode::Deep));
        assert!(!deep_preparation_eligible(ChatRetrievalMode::Fast));
        // Omitted mode resolves to Fast.
        assert!(!deep_preparation_eligible(ChatRetrievalMode::default()));
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

        let all = temporal_context_for_scope(&pool, &ChatRetrievalScope::All, 64_000)
            .await
            .unwrap();
        let folder = temporal_context_for_scope(
            &pool,
            &ChatRetrievalScope::Folder("root".to_string()),
            64_000,
        )
        .await
        .unwrap();
        let snapshot = temporal_context_for_scope(
            &pool,
            &ChatRetrievalScope::SearchSnapshot(vec!["m1".to_string(), "m3".to_string()]),
            64_000,
        )
        .await
        .unwrap();

        assert!(all.context.contains("Child"));
        assert_eq!(all.meeting_id.as_deref(), Some("m2"));
        assert!(folder.context.contains("Child"));
        assert_eq!(folder.meeting_id.as_deref(), Some("m2"));
        assert!(snapshot.context.contains("Other"));
        assert_eq!(snapshot.meeting_id.as_deref(), Some("m3"));
    }

    #[test]
    fn temporal_context_injects_an_authoritative_local_date() {
        let now = DateTime::parse_from_rfc3339("2026-08-18T14:30:00+00:00")
            .unwrap()
            .with_timezone(&Local);
        let latest = LatestSavedMeeting {
            id: "imported-notes".to_string(),
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
        let old_chat = state.claim_superseding_request(ChatRequestSurface::Chat, "chat-old");
        let sidebar = state.claim_superseding_request(ChatRequestSurface::Sidebar, "sidebar");
        let new_chat = state.claim_superseding_request(ChatRequestSurface::Chat, "chat-new");

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
        let old = state.claim_superseding_request(ChatRequestSurface::Chat, "old");
        let current = state.claim_superseding_request(ChatRequestSurface::Chat, "current");
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
            let token = state.claim_superseding_request(ChatRequestSurface::Chat, &id);
            assert!(state.clear_if_owner(ChatRequestSurface::Chat, &id, &token));
        }
        assert_eq!(state.request_count(), 0);
    }

    #[test]
    fn non_streaming_error_and_timeout_cleanup_release_request_ownership() {
        let state = ChatRequestState::new();
        let error_token = state.claim_superseding_request(ChatRequestSurface::Chat, "error");
        let error = finish_non_streaming_chat_request(
            &state,
            ChatRequestSurface::Chat,
            "error",
            &error_token,
            Ok(Err::<ChatResponse, _>("provider failed".to_string())),
        )
        .unwrap_err();
        assert_eq!(error, "provider failed");
        assert_eq!(state.request_count(), 0);

        let success_token = state.claim_superseding_request(ChatRequestSurface::Chat, "success");
        let response = finish_non_streaming_chat_request(
            &state,
            ChatRequestSurface::Chat,
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

        let old_token = state.claim_superseding_request(ChatRequestSurface::Chat, "old");
        let new_token = state.claim_superseding_request(ChatRequestSurface::Chat, "new");
        let superseded = finish_non_streaming_chat_request(
            &state,
            ChatRequestSurface::Chat,
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

        let timeout_token = state.claim_superseding_request(ChatRequestSurface::Chat, "timeout");
        timeout_token.cancel();
        assert!(state.clear_if_owner(ChatRequestSurface::Chat, "timeout", &timeout_token));
        assert_eq!(state.request_count(), 0);
    }

    #[test]
    fn mcp_admission_is_capped_and_reclaims_capacity() {
        let state = ChatRequestState::new();
        // Saturate: up to the cap each MCP claim is admitted atomically.
        let mut admitted = Vec::new();
        for index in 0..MAX_CONCURRENT_MCP_REQUESTS {
            let token = state.try_claim_request(ChatRequestSurface::Mcp, &format!("mcp-{index}"));
            assert!(
                token.is_some(),
                "claim {index} below the cap must be admitted"
            );
            admitted.push(token.unwrap());
        }
        assert_eq!(state.request_count(), MAX_CONCURRENT_MCP_REQUESTS);

        // At the cap, further MCP requests are rejected without disturbing
        // the admitted ones.
        assert!(state
            .try_claim_request(ChatRequestSurface::Mcp, "mcp-saturated")
            .is_none());
        for (index, token) in admitted.iter().enumerate() {
            assert!(
                !token.is_cancelled(),
                "admitted request {index} was disturbed"
            );
            assert!(state.is_owner(ChatRequestSurface::Mcp, &format!("mcp-{index}"), token));
        }

        // Capacity reclaims after a success path cleans its own entry.
        state.clear_if_owner(ChatRequestSurface::Mcp, "mcp-0", &admitted[0]);
        assert!(
            state
                .try_claim_request(ChatRequestSurface::Mcp, "mcp-new")
                .is_some(),
            "reclaimed capacity must admit a new MCP request"
        );
        assert_eq!(state.request_count(), MAX_CONCURRENT_MCP_REQUESTS);

        // Deletion cancellation also reclaims capacity (invalidate the
        // remaining bound requests and count the freed slots).
        for (index, token) in admitted.iter().enumerate().skip(1) {
            assert!(state.bind_request_meetings(
                ChatRequestSurface::Mcp,
                &format!("mcp-{index}"),
                token,
                &HashSet::from(["m1".to_string()])
            ));
        }
        let freed = state.invalidate_meeting("m1");
        assert_eq!(freed, MAX_CONCURRENT_MCP_REQUESTS - 1);
        for index in 0..(MAX_CONCURRENT_MCP_REQUESTS - 1) {
            assert!(
                state
                    .try_claim_request(ChatRequestSurface::Mcp, &format!("mcp-next-{index}"))
                    .is_some(),
                "deletion cancellation must reclaim admission capacity"
            );
        }
        assert_eq!(state.request_count(), MAX_CONCURRENT_MCP_REQUESTS);
    }

    #[test]
    fn chat_and_sidebar_admission_is_unchanged_by_the_mcp_cap() {
        let state = ChatRequestState::new();
        // Fill the registry with MAX_CONCURRENT_MCP_REQUESTS MCP claims.
        for index in 0..MAX_CONCURRENT_MCP_REQUESTS {
            assert!(state
                .try_claim_request(ChatRequestSurface::Mcp, &format!("mcp-{index}"))
                .is_some());
        }
        // Chat/Sidebar claims are never admission-capped: supersession keeps
        // their cardinality at one per surface.
        let chat = state.claim_superseding_request(ChatRequestSurface::Chat, "chat-1");
        let sidebar = state.claim_superseding_request(ChatRequestSurface::Sidebar, "sidebar-1");
        let newer_chat = state.claim_superseding_request(ChatRequestSurface::Chat, "chat-2");
        assert!(chat.is_cancelled());
        assert!(!sidebar.is_cancelled());
        assert!(state.is_owner(ChatRequestSurface::Chat, "chat-2", &newer_chat));
        assert!(state.is_owner(ChatRequestSurface::Sidebar, "sidebar-1", &sidebar));
        // Sidebar claims remain admittable while MCP is saturated, and each
        // newer Sidebar claim supersedes only its own surface.
        let another_sidebar =
            state.claim_superseding_request(ChatRequestSurface::Sidebar, "sidebar-2");
        assert!(!another_sidebar.is_cancelled());
        assert!(sidebar.is_cancelled());
        assert!(state.is_owner(ChatRequestSurface::Sidebar, "sidebar-2", &another_sidebar));
    }

    /// Embedder whose query embedding captures the cancellation token the
    /// retrieval layer handed it, then parks until released or cancelled
    /// (bounded), proving the MCP deadline/deletion cancellation reaches the
    /// ONNX/retrieval layer while preparation is in flight. `exited` flips
    /// when the parked work actually stops, so tests can prove pending work
    /// aborts only after cancellation.
    struct ParkingCapturingEmbedder {
        captured: Arc<Mutex<Option<tokio_util::sync::CancellationToken>>>,
        exited: Arc<std::sync::atomic::AtomicBool>,
        release: Arc<std::sync::atomic::AtomicBool>,
    }

    impl crate::retrieval::worker::DocumentEmbedder for ParkingCapturingEmbedder {
        fn model_id(&self) -> String {
            HYBRID_MODEL_ID.to_string()
        }

        fn dimensions(&self) -> usize {
            HYBRID_DIMENSIONS
        }

        fn count_tokens(&self, text: &str) -> usize {
            text.split_whitespace().count()
        }

        fn embed_documents_blocking(
            &self,
            texts: &[String],
            _cancel: &tokio_util::sync::CancellationToken,
        ) -> Result<Vec<Vec<f32>>, crate::retrieval::model::RetrievalModelError> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }

        fn embed_queries_blocking(
            &self,
            _texts: &[String],
            cancel: &tokio_util::sync::CancellationToken,
        ) -> Result<Vec<Vec<f32>>, crate::retrieval::model::RetrievalModelError> {
            *self.captured.lock().unwrap() = Some(cancel.clone());
            for _ in 0..2000 {
                if cancel.is_cancelled() {
                    self.exited.store(true, std::sync::atomic::Ordering::SeqCst);
                    return Err(crate::retrieval::model::RetrievalModelError::Cancelled);
                }
                if self.release.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            self.exited.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(_texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }
    }

    /// Embedder that ignores cancellation and releases after a fixed delay,
    /// letting a test interleave a deletion with gated preparation.
    struct DelayedOkEmbedder {
        park_ms: u64,
    }

    impl crate::retrieval::worker::DocumentEmbedder for DelayedOkEmbedder {
        fn model_id(&self) -> String {
            HYBRID_MODEL_ID.to_string()
        }

        fn dimensions(&self) -> usize {
            HYBRID_DIMENSIONS
        }

        fn count_tokens(&self, text: &str) -> usize {
            text.split_whitespace().count()
        }

        fn embed_documents_blocking(
            &self,
            texts: &[String],
            _cancel: &tokio_util::sync::CancellationToken,
        ) -> Result<Vec<Vec<f32>>, crate::retrieval::model::RetrievalModelError> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }

        fn embed_queries_blocking(
            &self,
            texts: &[String],
            _cancel: &tokio_util::sync::CancellationToken,
        ) -> Result<Vec<Vec<f32>>, crate::retrieval::model::RetrievalModelError> {
            std::thread::sleep(std::time::Duration::from_millis(self.park_ms));
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }
    }

    fn mcp_parking_lifecycle(
        captured: Arc<Mutex<Option<tokio_util::sync::CancellationToken>>>,
        exited: Arc<std::sync::atomic::AtomicBool>,
        release: Arc<std::sync::atomic::AtomicBool>,
    ) -> crate::retrieval::worker::RetrievalLifecycle {
        let embedder: Arc<dyn crate::retrieval::worker::DocumentEmbedder> =
            Arc::new(ParkingCapturingEmbedder {
                captured,
                exited,
                release,
            });
        let loader: crate::retrieval::worker::EngineLoader =
            Arc::new(move || Ok(Arc::clone(&embedder)));
        crate::retrieval::worker::RetrievalLifecycle::new(
            crate::retrieval::worker::LifecycleConfig::testing(Arc::new(|| false), loader),
        )
    }

    fn mcp_delayed_lifecycle(park_ms: u64) -> crate::retrieval::worker::RetrievalLifecycle {
        let embedder: Arc<dyn crate::retrieval::worker::DocumentEmbedder> =
            Arc::new(DelayedOkEmbedder { park_ms });
        let loader: crate::retrieval::worker::EngineLoader =
            Arc::new(move || Ok(Arc::clone(&embedder)));
        crate::retrieval::worker::RetrievalLifecycle::new(
            crate::retrieval::worker::LifecycleConfig::testing(Arc::new(|| false), loader),
        )
    }

    /// The MCP deadline owns the request token through PREPARATION: while a
    /// gated retrieval/ONNX embedding holds the token, expiry cancels it, the
    /// gated preparation aborts, no provider/generation connection is ever
    /// attempted, and ownership is released (capacity reclaimed).
    #[tokio::test]
    async fn mcp_timeout_cancellation_reaches_gated_preparation_and_stops_before_generation() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        insert_hybrid_meeting(&pool, 3).await;
        let captured: Arc<Mutex<Option<tokio_util::sync::CancellationToken>>> =
            Arc::new(Mutex::new(None));
        let exited = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let never_release = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let lifecycle =
            mcp_parking_lifecycle(Arc::clone(&captured), Arc::clone(&exited), never_release);
        // Publish the semantic document against the SAME lifecycle the MCP
        // request will use, so the gated embedding actually runs.
        publish_hybrid_document(
            &pool,
            &lifecycle,
            "m2",
            "doc-m2-target",
            "target",
            "target",
            HYBRID_TARGET,
        )
        .await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (called_tx, mut called_rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            // Any connection would mean generation started: serve and report.
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut request = vec![0_u8; 64 * 1024];
            let _ = socket.read(&mut request).await;
            let _ = called_tx.send(()).await;
            let body = r#"{"choices":[{"message":{"content":"answer"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
        });
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(format!("http://{}", address))
            .execute(&pool)
            .await
            .unwrap();

        let chat_requests = crate::api::chat::ChatRequestState::new();
        let task_pool = pool.clone();
        let task_chat_requests = chat_requests.clone();
        let task = tokio::spawn(async move {
            crate::mcp::server::execute_chat_with_meetings(
                &task_pool,
                &serde_json::json!({"query": "approved budget"}),
                &None,
                &reqwest::Client::new(),
                lifecycle,
                &task_chat_requests,
                std::time::Duration::from_millis(150),
            )
            .await
        });

        // The request token reaches the retrieval/ONNX layer: the gated
        // embedding captured it while preparation was in flight.
        for _ in 0..400 {
            if captured.lock().unwrap().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let captured_token = captured
            .lock()
            .unwrap()
            .clone()
            .expect("the request token must reach gated preparation");
        assert!(!captured_token.is_cancelled());

        // The deadline fires while preparation is gated: cancellation reaches
        // the parked embedding through the same request token.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !captured_token.is_cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("deadline cancellation must reach gated preparation");

        let result = tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        // Deterministic timeout result: the deadline cancelled the token
        // BEFORE the request future was dropped and before registry cleanup,
        // so the outcome is never an Elapsed/cancelled race. The surfaced
        // cause is the deadline itself — the token cancellation is HOW the
        // deadline stops the work, not a separate condition an MCP client
        // should have to guess at (it cannot tell a deadline from deletion
        // invalidation otherwise).
        assert_eq!(
            result.unwrap_err(),
            crate::mcp::server::MCP_CHAT_TIMEOUT_ERROR
        );
        // No provider/generation connection was ever attempted.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(300), called_rx.recv())
                .await
                .is_err(),
            "a timed-out MCP request must never start generation"
        );
        // Ownership released: capacity reclaimed for the next request.
        assert_eq!(chat_requests.request_count(), 0);
        let reclaimed = chat_requests
            .try_claim_request(ChatRequestSurface::Mcp, "mcp-reclaimed")
            .is_some();
        assert!(reclaimed);
    }

    /// Deletion during gated MCP preparation: the meeting is deleted (real
    /// transaction) while the gated embedding still parks, and the finished
    /// preparation cannot publish any deleted-meeting source or content —
    /// the response is either aborted or free of deleted evidence, ownership
    /// is released, and the provider answer is only produced for surviving
    /// evidence.
    #[tokio::test]
    async fn mcp_deletion_during_gated_preparation_publishes_no_deleted_evidence() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        insert_hybrid_meeting(&pool, 3).await;
        let lifecycle = mcp_delayed_lifecycle(250);
        publish_hybrid_document(
            &pool,
            &lifecycle,
            "m2",
            "doc-m2-target",
            "target",
            "target",
            HYBRID_TARGET,
        )
        .await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            // Serve generation immediately when (and if) it starts.
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut request = vec![0_u8; 64 * 1024];
            let _ = socket.read(&mut request).await;
            let body = r#"{"choices":[{"message":{"content":"answer"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
        });
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(format!("http://{}", address))
            .execute(&pool)
            .await
            .unwrap();

        let chat_requests = crate::api::chat::ChatRequestState::new();
        let task_pool = pool.clone();
        let task_chat_requests = chat_requests.clone();
        let task = tokio::spawn(async move {
            crate::mcp::server::execute_chat_with_meetings(
                &task_pool,
                &serde_json::json!({"query": "approved budget"}),
                &None,
                &reqwest::Client::new(),
                lifecycle,
                &task_chat_requests,
                CHAT_REQUEST_TIMEOUT,
            )
            .await
        });

        // Delete the meeting while the gated embedding still parks (well
        // before preparation finishes), through the real transaction.
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        let deleted = crate::database::repositories::meeting::MeetingsRepository::delete_meeting(
            &pool,
            "m2",
            |_| {},
        )
        .await
        .unwrap();
        assert!(deleted);

        let result = tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        // The response never carries deleted-meeting sources or snippets.
        match result {
            Ok(value) => {
                let sources = value["sources"].as_array().cloned().unwrap_or_default();
                assert!(
                    sources.iter().all(|source| source["meetingId"] != "m2"),
                    "deleted-meeting sources must not be published"
                );
                assert!(
                    !value.to_string().contains("The budget was approved"),
                    "deleted-meeting content must not reach the response"
                );
            }
            Err(error) => assert!(!error.contains("m2"), "no meeting id may leak: {error}"),
        }
        // Ownership released on every path.
        assert_eq!(chat_requests.request_count(), 0);
    }

    /// Aborting the MCP owner future AFTER it has claimed AND bound its
    /// token (gated generation in flight) drops the ownership guard, which
    /// CANCELS the request token before clearing the registry entry: the
    /// same token captured during preparation is cancelled, the blocked
    /// generation connection stops, the slot is reclaimed only after that,
    /// capacity becomes reusable, and no response is ever published.
    #[tokio::test]
    async fn mcp_owner_future_abort_cancels_token_before_reclaiming_the_slot() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        insert_hybrid_meeting(&pool, 3).await;
        let captured: Arc<Mutex<Option<tokio_util::sync::CancellationToken>>> =
            Arc::new(Mutex::new(None));
        let exited = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let lifecycle = mcp_parking_lifecycle(
            Arc::clone(&captured),
            Arc::clone(&exited),
            Arc::clone(&release),
        );
        publish_hybrid_document(
            &pool,
            &lifecycle,
            "m2",
            "doc-m2-target",
            "target",
            "target",
            HYBRID_TARGET,
        )
        .await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (called_tx, mut called_rx) = tokio::sync::mpsc::channel(4);
        let (closed_tx, mut closed_rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            use tokio::io::AsyncReadExt;
            let mut request = vec![0_u8; 64 * 1024];
            let _ = socket.read(&mut request).await;
            let _ = called_tx.send(()).await;
            // Park without responding; detect the client dropping after the
            // abort (EOF) and report the blocked generation as stopped.
            let mut eof = vec![0_u8; 64];
            let closed = socket.read(&mut eof).await.unwrap_or(0) == 0;
            let _ = closed_tx.send(closed).await;
        });
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(format!("http://{}", address))
            .execute(&pool)
            .await
            .unwrap();

        let chat_requests = crate::api::chat::ChatRequestState::new();
        let task_pool = pool.clone();
        let task_chat_requests = chat_requests.clone();
        let task = tokio::spawn(async move {
            crate::mcp::server::execute_chat_with_meetings(
                &task_pool,
                &serde_json::json!({"query": "approved budget"}),
                &None,
                &reqwest::Client::new(),
                lifecycle,
                &task_chat_requests,
                CHAT_REQUEST_TIMEOUT,
            )
            .await
        });

        // Wait until the gated embedding holds the request token, then let
        // preparation complete so the evidence is bound and generation starts.
        for _ in 0..400 {
            if captured.lock().unwrap().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let captured_token = captured
            .lock()
            .unwrap()
            .clone()
            .expect("the request token must reach gated preparation");
        assert!(!captured_token.is_cancelled());
        release.store(true, std::sync::atomic::Ordering::SeqCst);
        tokio::time::timeout(std::time::Duration::from_secs(10), called_rx.recv())
            .await
            .expect("generation must start while the request owns the slot");
        // Claimed AND bound, generation in flight.
        assert_eq!(chat_requests.request_count(), 1);

        // Abort the owner future: the ownership guard drops and cancels the
        // token BEFORE clearing the registry entry.
        task.abort();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !captured_token.is_cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("abort must cancel the request token");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while chat_requests.request_count() != 0 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the registry slot must be reclaimed after cancellation");
        // The blocked generation work stopped (connection closed by the
        // abort); no response was ever published.
        let generation_stopped =
            tokio::time::timeout(std::time::Duration::from_secs(5), closed_rx.recv())
                .await
                .unwrap()
                .unwrap();
        assert!(generation_stopped);
        // Capacity is reusable after the cancellation-ordered reclaim.
        assert!(chat_requests
            .try_claim_request(ChatRequestSurface::Mcp, "mcp-reclaimed")
            .is_some());
    }

    #[test]
    fn concurrent_mcp_requests_are_independent_and_deletion_cancels_only_affected() {
        let state = ChatRequestState::new();
        let first = state
            .try_claim_request(ChatRequestSurface::Mcp, "mcp-1")
            .unwrap();
        let second = state
            .try_claim_request(ChatRequestSurface::Mcp, "mcp-2")
            .unwrap();
        assert!(state.bind_request_meetings(
            ChatRequestSurface::Mcp,
            "mcp-1",
            &first,
            &HashSet::from(["m1".to_string()])
        ));
        assert!(state.bind_request_meetings(
            ChatRequestSurface::Mcp,
            "mcp-2",
            &second,
            &HashSet::from(["m2".to_string()])
        ));

        // A concurrent MCP claim does not cancel or replace the other.
        let third = state
            .try_claim_request(ChatRequestSurface::Mcp, "mcp-3")
            .unwrap();
        assert!(!first.is_cancelled());
        assert!(!second.is_cancelled());
        assert!(state.is_owner(ChatRequestSurface::Mcp, "mcp-1", &first));
        assert!(state.is_owner(ChatRequestSurface::Mcp, "mcp-2", &second));
        assert_eq!(state.request_count(), 3);

        // Deletion cancels ONLY the requests whose prepared evidence
        // references the deleted meeting (deletion-one).
        assert_eq!(state.invalidate_meeting("m1"), 1);
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
        assert!(!state.is_owner(ChatRequestSurface::Mcp, "mcp-1", &first));
        assert!(state.is_owner(ChatRequestSurface::Mcp, "mcp-2", &second));
        assert_eq!(state.request_count(), 2);

        // Deletion-all: both remaining requests reference the same meeting.
        assert!(state.bind_request_meetings(
            ChatRequestSurface::Mcp,
            "mcp-3",
            &third,
            &HashSet::from(["m2".to_string()])
        ));
        assert_eq!(state.invalidate_meeting("m2"), 2);
        assert!(second.is_cancelled());
        assert!(third.is_cancelled());
        assert_eq!(state.request_count(), 0);

        // Each request cleans only its own entry; a stale cleanup is a no-op
        // for the others.
        let stale = state
            .try_claim_request(ChatRequestSurface::Mcp, "mcp-a")
            .unwrap();
        let kept = state
            .try_claim_request(ChatRequestSurface::Mcp, "mcp-b")
            .unwrap();
        assert!(state.clear_if_owner(ChatRequestSurface::Mcp, "mcp-a", &stale));
        assert!(!state.clear_if_owner(ChatRequestSurface::Mcp, "mcp-b", &stale));
        assert!(state.is_owner(ChatRequestSurface::Mcp, "mcp-b", &kept));
        assert_eq!(state.request_count(), 1);
        state.clear_if_owner(ChatRequestSurface::Mcp, "mcp-b", &kept);
        assert_eq!(state.request_count(), 0);
    }

    #[test]
    fn chat_and_sidebar_replacement_semantics_are_unchanged() {
        let state = ChatRequestState::new();
        let old_chat = state.claim_superseding_request(ChatRequestSurface::Chat, "chat-1");
        let old_sidebar = state.claim_superseding_request(ChatRequestSurface::Sidebar, "sidebar-1");
        let new_chat = state.claim_superseding_request(ChatRequestSurface::Chat, "chat-2");
        // A newer same-surface claim supersedes the previous Chat request but
        // not the other surface, and independent MCP requests are untouched
        // by Chat supersession.
        let mcp = state
            .try_claim_request(ChatRequestSurface::Mcp, "mcp-1")
            .unwrap();
        assert!(old_chat.is_cancelled());
        assert!(!old_sidebar.is_cancelled());
        assert!(!mcp.is_cancelled());
        assert!(!state.is_owner(ChatRequestSurface::Chat, "chat-1", &old_chat));
        assert!(state.is_owner(ChatRequestSurface::Chat, "chat-2", &new_chat));
        assert!(state.is_owner(ChatRequestSurface::Sidebar, "sidebar-1", &old_sidebar));
        assert!(state.is_owner(ChatRequestSurface::Mcp, "mcp-1", &mcp));
        // A superseded request can no longer bind or publish.
        assert!(!state.bind_request_meetings(
            ChatRequestSurface::Chat,
            "chat-1",
            &old_chat,
            &HashSet::from(["m9".to_string()])
        ));
        assert!(state.bind_request_meetings(
            ChatRequestSurface::Mcp,
            "mcp-1",
            &mcp,
            &HashSet::from(["m9".to_string()])
        ));
    }

    #[test]
    fn deletion_invalidation_cancels_bound_requests_and_cleans_up() {
        let state = ChatRequestState::new();
        let chat_token = state.claim_superseding_request(ChatRequestSurface::Chat, "chat-1");
        let sidebar_token =
            state.claim_superseding_request(ChatRequestSurface::Sidebar, "sidebar-1");
        assert!(state.bind_request_meetings(
            ChatRequestSurface::Chat,
            "chat-1",
            &chat_token,
            &HashSet::from(["m1".to_string()])
        ));
        assert!(state.bind_request_meetings(
            ChatRequestSurface::Sidebar,
            "sidebar-1",
            &sidebar_token,
            &HashSet::from(["m1".to_string(), "m2".to_string()])
        ));
        // A newer request supersedes the chat request and removes its
        // registration; binding for it must fail afterward.
        let unbound_token = state.claim_superseding_request(ChatRequestSurface::Chat, "chat-2");
        assert!(chat_token.is_cancelled());
        assert!(!state.bind_request_meetings(
            ChatRequestSurface::Chat,
            "chat-1",
            &chat_token,
            &HashSet::from(["m9".to_string()])
        ));
        // Binding guards: wrong token and cancelled tokens fail.
        assert!(!state.bind_request_meetings(
            ChatRequestSurface::Chat,
            "chat-2",
            &sidebar_token,
            &HashSet::from(["m9".to_string()])
        ));

        // Only the registered request whose prepared evidence references the
        // meeting is cancelled; the unbound chat request is spared.
        assert_eq!(state.invalidate_meeting("m2"), 1);
        assert!(sidebar_token.is_cancelled());
        assert!(!unbound_token.is_cancelled());
        assert!(state.is_owner(ChatRequestSurface::Chat, "chat-2", &unbound_token));

        assert_eq!(state.invalidate_meeting("m1"), 0);
        assert_eq!(state.invalidate_meeting("missing"), 0);
        // Invalidated registrations are removed immediately (bounded lifetime).
        assert_eq!(state.request_count(), 1);
        state.clear_if_owner(ChatRequestSurface::Chat, "chat-2", &unbound_token);
        assert_eq!(state.request_count(), 0);
    }

    #[derive(Clone, Default)]
    struct CapturingSink(Arc<Mutex<Vec<(String, serde_json::Value)>>>);

    impl ChatEventSink for CapturingSink {
        fn emit(&self, event: &str, payload: serde_json::Value) {
            self.0.lock().unwrap().push((event.to_string(), payload));
        }
    }

    impl CapturingSink {
        fn events(&self) -> Vec<(String, serde_json::Value)> {
            self.0.lock().unwrap().clone()
        }

        fn names(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .map(|(name, _)| name.clone())
                .collect()
        }

        async fn wait_for_event(&self, event: &str) {
            for _ in 0..2000 {
                if self.0.lock().unwrap().iter().any(|(name, _)| name == event) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            panic!("event {event} was never emitted");
        }
    }

    /// SSE server that writes one delta, then parks until released, so a test
    /// can deterministically interleave a deletion with an in-flight stream.
    async fn serve_streaming_chunks_with_barrier() -> (String, tokio::sync::oneshot::Sender<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 64 * 1024];
            let _ = socket.read(&mut request).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await;
            let _ = socket
                .write_all(br#"data: {"choices":[{"delta":{"content":"before "}}]}"#)
                .await;
            let _ = socket.write_all(b"\n\n").await;
            let _ = socket.flush().await;
            let _ = release_rx.await;
            let _ = socket
                .write_all(br#"data: {"choices":[{"delta":{"content":"after"}}]}"#)
                .await;
            let _ = socket.write_all(b"\n\ndata: [DONE]\n\n").await;
            let _ = socket.flush().await;
        });
        (format!("http://{}", address), release_tx)
    }

    fn stream_race_inputs(endpoint: &str) -> ChatInputs {
        ChatInputs {
            sources: vec![ChatSource {
                meeting_id: "m1".to_string(),
                meeting_title: "Deleted".to_string(),
                chunk_type: "transcript".to_string(),
                snippet: "private deleted snippet".to_string(),
                folder_name: "General".to_string(),
                source_kind: Some("transcript".to_string()),
            }],
            provider: LLMProvider::Ollama,
            model_name: "local".to_string(),
            api_key: String::new(),
            ollama_endpoint: Some(endpoint.to_string()),
            custom_openai_endpoint: None,
            custom_openai_max_tokens: None,
            custom_openai_temperature: None,
            custom_openai_top_p: None,
            app_data_dir: None,
            user_prompt: "Meeting context:\nprivate deleted snippet\n".to_string(),
            prompt_meeting_ids: HashSet::from(["m1".to_string()]),
            retrieval_diagnostic: RetrievalPreparationDiagnostic::Hybrid,
            retrieval_mode: ChatRetrievalMode::Fast,
            provider_round_trips: 0,
        }
    }

    fn source_less_stream_race_inputs(endpoint: &str, meeting_id: &str) -> ChatInputs {
        ChatInputs {
            sources: Vec::new(),
            provider: LLMProvider::Ollama,
            model_name: "local".to_string(),
            api_key: String::new(),
            ollama_endpoint: Some(endpoint.to_string()),
            custom_openai_endpoint: None,
            custom_openai_max_tokens: None,
            custom_openai_temperature: None,
            custom_openai_top_p: None,
            app_data_dir: None,
            user_prompt: "Authoritative meeting card: Deleted\nMost recently saved/imported meeting in this scope: Deleted.\n".to_string(),
            prompt_meeting_ids: HashSet::from([meeting_id.to_string()]),
            retrieval_diagnostic: RetrievalPreparationDiagnostic::Hybrid,
            retrieval_mode: ChatRetrievalMode::Fast,
            provider_round_trips: 0,
        }
    }

    /// The real-stream deletion race: the stream starts and publishes its
    /// first chunk while the meeting exists; the meeting is then deleted
    /// through the real transaction with the real invalidation hook; the
    /// in-flight generation is cancelled and no post-deletion chunk, source,
    /// done, or error event is published, and the registry is cleaned up.
    #[tokio::test]
    async fn deletion_invalidates_the_active_stream_and_publishes_nothing_afterward() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (endpoint, release_tx) = serve_streaming_chunks_with_barrier().await;
        let stream_state = ChatRequestState::new();
        let token = stream_state.claim_superseding_request(ChatRequestSurface::Chat, "stream-race");
        let sink = CapturingSink::default();
        let task = tokio::spawn(stream_chat(
            pool.clone(),
            sink.clone(),
            stream_state.clone(),
            stream_race_inputs(&endpoint),
            "stream-race".to_string(),
            None,
            token.clone(),
        ));
        sink.wait_for_event("chat-stream-chunk").await;
        // While the meeting exists the stream published the full prepared
        // source set: source/context parity before any deletion.
        let start = sink
            .events()
            .into_iter()
            .find(|(name, _)| name == "chat-stream-start")
            .unwrap();
        assert_eq!(start.1["sources"][0]["meetingId"], "m1");

        // Real deletion path with the real invalidation hook: the deletion
        // transaction itself cancels this stream's ownership token.
        let invalidated = stream_state.clone();
        let deleted = crate::database::repositories::meeting::MeetingsRepository::delete_meeting(
            &pool,
            "m1",
            |meeting_id| {
                invalidated.invalidate_meeting(meeting_id);
            },
        )
        .await
        .unwrap();
        assert!(deleted);
        assert!(token.is_cancelled());

        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let names = sink.names();
        assert_eq!(
            names
                .iter()
                .filter(|name| *name == "chat-stream-chunk")
                .count(),
            1,
            "no post-deletion chunk may be published"
        );
        assert!(!names.contains(&"chat-stream-done".to_string()));
        assert!(!names.contains(&"chat-stream-error".to_string()));
        assert_eq!(
            names
                .iter()
                .filter(|name| *name == "chat-stream-abort")
                .count(),
            1,
            "an invalidated started stream must publish one safe abort"
        );
        // Terminal cleanup: the invalidated registration was removed.
        assert_eq!(stream_state.request_count(), 0);

        // A delayed save of the already-emitted source set cannot re-persist
        // the deleted meeting's snippet: save-time sanitization keeps only
        // currently existing meetings.
        let conversation = ChatRepository::get_or_create_scoped_conversation(
            &pool,
            &ChatScope {
                kind: ChatScopeKind::All,
                key: "all".to_string(),
                data: None,
            },
            None,
        )
        .await
        .unwrap();
        ChatRepository::save_message(
            &pool,
            &conversation.id,
            "assistant",
            "delayed answer",
            Some(r#"[{"meetingId":"m1","meetingTitle":"Deleted","chunkType":"transcript","snippet":"private deleted snippet","folderName":"General","sourceKind":"transcript"}]"#),
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT sources_json FROM chat_messages WHERE content = 'delayed answer'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            None
        );
    }

    /// Post-start deletion UI contract: sources are rendered at
    /// `chat-stream-start`, the meeting is then deleted (real transaction),
    /// and `stream_chat` publishes a privacy-safe abort event carrying only
    /// the stream identity and stable reason — no source/done/error payload,
    /// no delayed save.
    #[tokio::test]
    async fn post_start_deletion_publishes_privacy_safe_abort_event() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (endpoint, release_tx) = serve_streaming_chunks_with_barrier().await;
        let stream_state = ChatRequestState::new();
        let token =
            stream_state.claim_superseding_request(ChatRequestSurface::Chat, "stream-abort");
        let sink = CapturingSink::default();
        let task = tokio::spawn(stream_chat(
            pool.clone(),
            sink.clone(),
            stream_state.clone(),
            stream_race_inputs(&endpoint),
            "stream-abort".to_string(),
            None,
            token.clone(),
        ));
        sink.wait_for_event("chat-stream-chunk").await;

        // Real deletion path while the stream is open after start.
        let invalidated = stream_state.clone();
        let deleted = crate::database::repositories::meeting::MeetingsRepository::delete_meeting(
            &pool,
            "m1",
            |meeting_id| {
                invalidated.invalidate_meeting(meeting_id);
            },
        )
        .await
        .unwrap();
        assert!(deleted);
        assert!(token.is_cancelled());

        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        // The abort event carries ONLY stable identity + reason.
        let names = sink.names();
        assert!(names.contains(&"chat-stream-abort".to_string()));
        let abort = sink
            .events()
            .into_iter()
            .find(|(name, _)| name == "chat-stream-abort")
            .unwrap();
        assert_eq!(abort.1["streamId"], "stream-abort");
        assert_eq!(abort.1["reason"], "referenced_meeting_deleted");
        let abort_text = abort.1.to_string();
        assert!(!abort_text.contains("private deleted snippet"));
        assert!(!abort_text.contains("Deleted"));
        // No terminal source/done/error publication.
        assert!(!names.contains(&"chat-stream-done".to_string()));
        assert!(!names.contains(&"chat-stream-error".to_string()));
        assert_eq!(stream_state.request_count(), 0);
        // The delayed save cannot re-persist the deleted snippet.
        let conversation = ChatRepository::get_or_create_scoped_conversation(
            &pool,
            &ChatScope {
                kind: ChatScopeKind::All,
                key: "all".to_string(),
                data: None,
            },
            None,
        )
        .await
        .unwrap();
        ChatRepository::save_message(
            &pool,
            &conversation.id,
            "assistant",
            "delayed answer",
            Some(r#"[{"meetingId":"m1","meetingTitle":"Deleted","chunkType":"transcript","snippet":"private deleted snippet","folderName":"General","sourceKind":"transcript"}]"#),
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT sources_json FROM chat_messages WHERE content = 'delayed answer'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn source_less_prompt_deletion_before_stream_binding_publishes_nothing() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let stream_state = ChatRequestState::new();
        let token = stream_state
            .claim_superseding_request(ChatRequestSurface::Chat, "source-less-before-bind");
        let invalidated = stream_state.clone();
        assert!(
            crate::database::repositories::meeting::MeetingsRepository::delete_meeting(
                &pool,
                "m1",
                |meeting_id| {
                    invalidated.invalidate_meeting(meeting_id);
                },
            )
            .await
            .unwrap()
        );
        assert!(!token.is_cancelled());

        let sink = CapturingSink::default();
        let result = stream_chat(
            pool,
            sink.clone(),
            stream_state.clone(),
            source_less_stream_race_inputs("http://127.0.0.1:9", "m1"),
            "source-less-before-bind".to_string(),
            None,
            token,
        )
        .await;

        assert_eq!(result.unwrap_err(), DELETED_MEETING_EVIDENCE_ERROR);
        assert!(sink.events().is_empty());
        assert_eq!(stream_state.request_count(), 0);
    }

    /// Serves one non-2xx response whose body contains the word "cancelled",
    /// exactly as a proxying provider reports an aborted upstream request.
    /// `generate_summary_stream` embeds that body verbatim in its error.
    async fn serve_upstream_cancelled_error() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 64 * 1024];
            let _ = socket.read(&mut request).await;
            let body = r#"{"error":{"message":"Upstream request was cancelled by the gateway"}}"#;
            let _ = socket
                .write_all(
                    format!(
                        "HTTP/1.1 502 Bad Gateway\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .await;
            let _ = socket.flush().await;
        });
        format!("http://{}", address)
    }

    /// A started stream must ALWAYS receive exactly one terminal event.
    /// Classifying cancellation by substring-matching the provider's message
    /// let an upstream body containing "cancelled" take the cancellation arm,
    /// which publishes nothing: the panel kept an assistant row rendering its
    /// sources forever. Cancellation is now decided by the request's own
    /// token, so an uncancelled provider failure falls through to the error
    /// arm and terminates the stream.
    #[tokio::test]
    async fn provider_error_naming_cancellation_still_terminates_a_started_stream() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Kept', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let endpoint = serve_upstream_cancelled_error().await;
        let stream_state = ChatRequestState::new();
        let token =
            stream_state.claim_superseding_request(ChatRequestSurface::Chat, "upstream-cancelled");
        let sink = CapturingSink::default();

        let result = stream_chat(
            pool,
            sink.clone(),
            stream_state.clone(),
            source_less_stream_race_inputs(&endpoint, "m1"),
            "upstream-cancelled".to_string(),
            None,
            token.clone(),
        )
        .await;

        assert!(result.is_ok());
        // The request itself was never cancelled: only the provider's text
        // said so.
        assert!(!token.is_cancelled());
        let events = sink.events();
        let names: Vec<&str> = events.iter().map(|(event, _)| event.as_str()).collect();
        assert_eq!(names, vec!["chat-stream-start", "chat-stream-error"]);
        // Exactly one terminal event, and ownership is released so the panel
        // is not left waiting on a stream that can never finish.
        assert_eq!(stream_state.request_count(), 0);
    }

    #[tokio::test]
    async fn source_less_prompt_deletion_after_start_emits_only_the_abort() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (endpoint, release_tx) = serve_streaming_chunks_with_barrier().await;
        let stream_state = ChatRequestState::new();
        let token = stream_state
            .claim_superseding_request(ChatRequestSurface::Chat, "source-less-after-start");
        let sink = CapturingSink::default();
        let task = tokio::spawn(stream_chat(
            pool.clone(),
            sink.clone(),
            stream_state.clone(),
            source_less_stream_race_inputs(&endpoint, "m1"),
            "source-less-after-start".to_string(),
            None,
            token.clone(),
        ));
        sink.wait_for_event("chat-stream-chunk").await;
        let start = sink
            .events()
            .into_iter()
            .find(|(name, _)| name == "chat-stream-start")
            .unwrap();
        assert_eq!(start.1["sources"], serde_json::json!([]));

        let invalidated = stream_state.clone();
        assert!(
            crate::database::repositories::meeting::MeetingsRepository::delete_meeting(
                &pool,
                "m1",
                |meeting_id| {
                    invalidated.invalidate_meeting(meeting_id);
                },
            )
            .await
            .unwrap()
        );
        assert!(token.is_cancelled());
        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let events = sink.events();
        assert_eq!(
            events
                .iter()
                .filter(|(name, _)| name == "chat-stream-abort")
                .count(),
            1
        );
        assert!(events.iter().any(|(name, payload)| {
            name == "chat-stream-abort"
                && payload["streamId"] == "source-less-after-start"
                && payload["reason"] == "referenced_meeting_deleted"
        }));
        assert!(!events.iter().any(|(name, _)| {
            matches!(name.as_str(), "chat-stream-done" | "chat-stream-error")
        }));
        assert_eq!(stream_state.request_count(), 0);
    }

    #[test]
    fn replaced_stream_cannot_publish_a_stale_deletion_abort() {
        let state = ChatRequestState::new();
        let old = state.claim_superseding_request(ChatRequestSurface::Chat, "old");
        assert!(state.bind_chat_stream_meetings("old", &old, &HashSet::from(["m1".to_string()]),));
        let _new = state.claim_superseding_request(ChatRequestSurface::Chat, "new");
        state.invalidate_meeting("m1");

        assert!(
            !state.publish_deletion_abort_if_current("old", &old, |_, _| {
                panic!("a replaced stream must not publish an abort")
            })
        );
    }

    #[tokio::test]
    async fn deletion_between_post_bind_fence_and_start_publishes_exactly_one_abort() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let state = ChatRequestState::new();
        let token = state.claim_superseding_request(ChatRequestSurface::Chat, "start-race");
        let meeting_ids = HashSet::from(["m1".to_string()]);
        assert!(state.bind_chat_stream_meetings("start-race", &token, &meeting_ids));
        ensure_prompt_meetings_exist(&pool, &meeting_ids)
            .await
            .unwrap();

        let invalidated = state.clone();
        assert!(
            crate::database::repositories::meeting::MeetingsRepository::delete_meeting(
                &pool,
                "m1",
                |meeting_id| {
                    invalidated.invalidate_meeting(meeting_id);
                },
            )
            .await
            .unwrap()
        );

        let sink = CapturingSink::default();
        assert!(!emit_chat_stream_event_if_sink(
            &state,
            &sink,
            "start-race",
            &token,
            "chat-stream-start",
            serde_json::json!({ "streamId": "start-race", "sources": [] }),
            false,
        ));
        let events = sink.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "chat-stream-abort");
        assert_eq!(events[0].1["streamId"], "start-race");
        assert_eq!(events[0].1["reason"], "referenced_meeting_deleted");
        assert_eq!(state.request_count(), 0);
    }

    #[tokio::test]
    async fn deletion_between_terminal_fence_and_done_publishes_exactly_one_abort() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let state = ChatRequestState::new();
        let token = state.claim_superseding_request(ChatRequestSurface::Chat, "terminal-race");
        let meeting_ids = HashSet::from(["m1".to_string()]);
        assert!(state.bind_chat_stream_meetings("terminal-race", &token, &meeting_ids));
        let sink = CapturingSink::default();
        assert!(emit_chat_stream_event_if_sink(
            &state,
            &sink,
            "terminal-race",
            &token,
            "chat-stream-start",
            serde_json::json!({ "streamId": "terminal-race", "sources": [] }),
            false,
        ));
        ensure_prompt_meetings_exist(&pool, &meeting_ids)
            .await
            .unwrap();

        let invalidated = state.clone();
        assert!(
            crate::database::repositories::meeting::MeetingsRepository::delete_meeting(
                &pool,
                "m1",
                |meeting_id| {
                    invalidated.invalidate_meeting(meeting_id);
                },
            )
            .await
            .unwrap()
        );

        assert!(!emit_chat_stream_event_if_sink(
            &state,
            &sink,
            "terminal-race",
            &token,
            "chat-stream-done",
            serde_json::json!({ "streamId": "terminal-race", "answer": "deleted", "sources": [] }),
            true,
        ));
        let events = sink.events();
        assert_eq!(
            events
                .iter()
                .filter(|(event, _)| event == "chat-stream-start")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|(event, _)| event == "chat-stream-abort")
                .count(),
            1
        );
        assert!(!events.iter().any(|(event, _)| event == "chat-stream-done"));
        assert_eq!(state.request_count(), 0);
    }

    #[tokio::test]
    async fn deletion_between_timeout_observation_and_cleanup_publishes_exactly_one_abort() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let state = ChatRequestState::new();
        let token = state.claim_superseding_request(ChatRequestSurface::Chat, "timeout-race");
        let meeting_ids = HashSet::from(["m1".to_string()]);
        assert!(state.bind_chat_stream_meetings("timeout-race", &token, &meeting_ids));
        let sink = CapturingSink::default();
        assert!(emit_chat_stream_event_if_sink(
            &state,
            &sink,
            "timeout-race",
            &token,
            "chat-stream-start",
            serde_json::json!({ "streamId": "timeout-race", "sources": [] }),
            false,
        ));

        assert!(
            !state.publish_deletion_abort_if_current("timeout-race", &token, |_, _| {
                panic!("the timeout observation precedes deletion")
            })
        );
        let invalidated = state.clone();
        assert!(
            crate::database::repositories::meeting::MeetingsRepository::delete_meeting(
                &pool,
                "m1",
                |meeting_id| {
                    invalidated.invalidate_meeting(meeting_id);
                },
            )
            .await
            .unwrap()
        );

        assert!(state.finish_chat_stream_if_current(
            "timeout-race",
            &token,
            true,
            Some((
                "chat-stream-error",
                serde_json::json!({
                    "streamId": "timeout-race",
                    "error": CHAT_CONTEXT_REVALIDATION_ERROR,
                    "safeCleanup": true,
                }),
            )),
            |event, payload| { sink.emit(event, payload) }
        ));
        let events = sink.events();
        assert_eq!(
            events
                .iter()
                .filter(|(event, _)| event == "chat-stream-abort")
                .count(),
            1
        );
        assert!(events.iter().any(|(event, payload)| {
            event == "chat-stream-abort"
                && payload
                    == &serde_json::json!({
                        "streamId": "timeout-race",
                        "reason": "referenced_meeting_deleted",
                    })
        }));
        assert!(!events.iter().any(|(event, _)| {
            matches!(event.as_str(), "chat-stream-done" | "chat-stream-error")
        }));
        assert!(token.is_cancelled());
        assert_eq!(state.request_count(), 0);
    }

    #[tokio::test]
    async fn committed_deletion_notification_is_identity_only_and_failures_emit_none() {
        use crate::database::repositories::meeting::MeetingsRepository;

        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let events = Arc::new(Mutex::new(Vec::<(String, serde_json::Value)>::new()));
        let record = |events: Arc<Mutex<Vec<(String, serde_json::Value)>>>| {
            move |event: &str, payload: serde_json::Value| {
                events.lock().unwrap().push((event.to_string(), payload));
            }
        };

        // Nothing deleted (Ok(false)) and a failure/rollback shape: no event.
        let missing = MeetingsRepository::delete_meeting(&pool, "missing", |_| {}).await;
        assert!(!emit_chat_meeting_deleted_if_committed(
            record(events.clone()),
            "missing",
            &missing
        ));
        let failure: Result<bool, sqlx::Error> = Err(sqlx::Error::Protocol("rolled back".into()));
        assert!(!emit_chat_meeting_deleted_if_committed(
            record(events.clone()),
            "m1",
            &failure
        ));

        // Committed deletion through the real transaction: exactly one
        // identity-only event, published only once the row is actually gone.
        let committed = MeetingsRepository::delete_meeting(&pool, "m1", |_| {}).await;
        assert!(emit_chat_meeting_deleted_if_committed(
            record(events.clone()),
            "m1",
            &committed
        ));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings WHERE id = ?")
                .bind("m1")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0,
            "the notification must follow the committed row removal"
        );

        let events = events.lock().unwrap().clone();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, CHAT_MEETING_DELETED_EVENT);
        assert_eq!(events[0].1, serde_json::json!({ "meetingId": "m1" }));
    }

    #[tokio::test]
    async fn terminal_revalidation_timeout_publishes_one_safe_cleanup_error() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (endpoint, release_tx) = serve_streaming_chunks_with_barrier().await;
        let state = ChatRequestState::new();
        let token =
            state.claim_superseding_request(ChatRequestSurface::Chat, "revalidation-timeout");
        let sink = CapturingSink::default();
        let task = tokio::spawn({
            let task_pool = pool.clone();
            let task_sink = sink.clone();
            let task_state = state.clone();
            let task_token = token.clone();
            async move {
                await_chat_stream_with_timeout(
                    Duration::from_secs(1),
                    &task_state,
                    &task_sink,
                    "revalidation-timeout",
                    &task_token,
                    stream_chat(
                        task_pool,
                        task_sink.clone(),
                        task_state.clone(),
                        stream_race_inputs(&endpoint),
                        "revalidation-timeout".to_string(),
                        None,
                        task_token.clone(),
                    ),
                )
                .await
            }
        });
        sink.wait_for_event("chat-stream-chunk").await;
        let connection = pool.acquire().await.unwrap();
        release_tx.send(()).unwrap();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), task)
                .await
                .unwrap()
                .unwrap(),
            Ok(())
        );
        drop(connection);

        let events = sink.events();
        assert_eq!(
            events
                .iter()
                .filter(|(event, _)| event == "chat-stream-error")
                .count(),
            1
        );
        assert!(!events.iter().any(|(event, _)| {
            matches!(event.as_str(), "chat-stream-abort" | "chat-stream-done")
        }));
        let cleanup = events
            .into_iter()
            .find(|(event, _)| event == "chat-stream-error")
            .unwrap();
        assert_eq!(
            cleanup.1,
            serde_json::json!({
                "streamId": "revalidation-timeout",
                "error": CHAT_CONTEXT_REVALIDATION_ERROR,
                "safeCleanup": true,
            })
        );
        assert!(token.is_cancelled());
        assert_eq!(state.request_count(), 0);
    }

    #[tokio::test]
    async fn terminal_revalidation_db_error_publishes_one_safe_cleanup_error() {
        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (endpoint, release_tx) = serve_streaming_chunks_with_barrier().await;
        let state = ChatRequestState::new();
        let token = state.claim_superseding_request(ChatRequestSurface::Chat, "revalidation-error");
        let sink = CapturingSink::default();
        let task = tokio::spawn(stream_chat(
            pool.clone(),
            sink.clone(),
            state.clone(),
            stream_race_inputs(&endpoint),
            "revalidation-error".to_string(),
            None,
            token,
        ));
        sink.wait_for_event("chat-stream-chunk").await;
        pool.close().await;
        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let events = sink.events();
        assert_eq!(
            events
                .iter()
                .filter(|(event, _)| event == "chat-stream-error")
                .count(),
            1
        );
        assert!(!events.iter().any(|(event, _)| {
            matches!(event.as_str(), "chat-stream-abort" | "chat-stream-done")
        }));
        let cleanup = events
            .into_iter()
            .find(|(event, _)| event == "chat-stream-error")
            .unwrap();
        assert_eq!(
            cleanup.1,
            serde_json::json!({
                "streamId": "revalidation-error",
                "error": CHAT_CONTEXT_REVALIDATION_ERROR,
                "safeCleanup": true,
            })
        );
        assert_eq!(state.request_count(), 0);
    }

    #[test]
    fn replaced_stream_cannot_publish_a_stale_revalidation_cleanup() {
        let state = ChatRequestState::new();
        let old = state.claim_superseding_request(ChatRequestSurface::Chat, "old");
        let _new = state.claim_superseding_request(ChatRequestSurface::Chat, "new");
        let sink = CapturingSink::default();

        assert!(!state.finish_chat_stream_if_current(
            "old",
            &old,
            false,
            Some((
                "chat-stream-error",
                serde_json::json!({
                    "streamId": "old",
                    "error": CHAT_CONTEXT_REVALIDATION_ERROR,
                    "safeCleanup": true,
                }),
            )),
            |event, payload| sink.emit(event, payload),
        ));
        assert!(sink.events().is_empty());
        assert_eq!(state.request_count(), 1);
    }

    /// Deletion during Deep preparation: evidence captured before the
    /// deletion must never reach the final prompt or sources (the agent's
    /// final validation fence), and the prepared output keeps exact parity.
    #[tokio::test]
    async fn deletion_during_deep_preparation_publishes_no_deleted_evidence() {
        let (pool, lifecycle) = resume_scope_hybrid_test_fixture().await;
        let (endpoint, planner_called_rx, release_tx) = serve_planner_with_deletion_barrier().await;
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(endpoint)
            .execute(&pool)
            .await
            .unwrap();

        let task_pool = pool.clone();
        let task = tokio::spawn(async move {
            prepare_chat_inputs_for_scope(
                &task_pool,
                None,
                &reqwest::Client::new(),
                "Which meeting discussed resume parsing?",
                None,
                ChatRetrievalScope::SearchSnapshot(vec![
                    "roadmap-meeting".to_string(),
                    "resume-meeting".to_string(),
                ]),
                None,
                Some(lifecycle),
                None,
                Some(ChatRetrievalMode::Deep),
                None,
            )
            .await
        });

        // The planner call is in flight when the meeting is deleted through
        // the real transaction.
        tokio::time::timeout(Duration::from_secs(10), planner_called_rx)
            .await
            .unwrap()
            .unwrap();
        let deleted = crate::database::repositories::meeting::MeetingsRepository::delete_meeting(
            &pool,
            "resume-meeting",
            |_| {},
        )
        .await
        .unwrap();
        assert!(deleted);
        let _ = release_tx.send(());

        let inputs = tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        // No deleted evidence may be published: neither as a source nor in
        // the retained context, and published sources keep prompt parity.
        assert_eq!(inputs.retrieval_mode, ChatRetrievalMode::Deep);
        assert!(inputs
            .sources
            .iter()
            .all(|source| source.meeting_id != "resume-meeting"));
        assert!(!inputs.user_prompt.contains("resume parsing decision"));
        assert!(inputs
            .sources
            .iter()
            .all(|source| inputs.user_prompt.contains(&source.snippet)));
    }

    /// Serves the Deep planner: signals when the planner request arrives,
    /// then waits for the test to release it before answering finish.
    async fn serve_planner_with_deletion_barrier() -> (
        String,
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (called_tx, called_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 64 * 1024];
            let _ = socket.read(&mut request).await;
            let _ = called_tx.send(());
            let _ = release_rx.await;
            let body = r#"{"choices":[{"message":{"content":"{\"schemaVersion\":1,\"status\":\"finish\"}"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
        });
        (format!("http://{}", address), called_rx, release_tx)
    }

    /// Serves a two-round Deep planner for the saved-meeting scope: round one
    /// requests an additional same-meeting search, round two finishes. The
    /// extra retrieval is bounded to the one-meeting allow-list.
    async fn serve_saved_meeting_deep_two_rounds() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 128 * 1024];
            let _ = socket.read(&mut request).await;
            let round_one = r#"{"choices":[{"message":{"content":"{\"schemaVersion\":1,\"status\":\"search_more\",\"queries\":[\"approved budget details\"]}"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                round_one.len(),
                round_one
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
            // Round two: finish.
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 128 * 1024];
            let _ = socket.read(&mut request).await;
            let body = r#"{"choices":[{"message":{"content":"{\"schemaVersion\":1,\"status\":\"finish\"}"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
        });
        format!("http://{}", address)
    }

    /// The non-streaming terminal fence through the real deletion path: the
    /// deletion transaction invalidates the bound request, the ownership
    /// mechanism refuses the final payload, and the existence fence aborts.
    #[tokio::test]
    async fn deletion_invalidates_non_streaming_requests_before_final_response() {
        use crate::database::repositories::meeting::MeetingsRepository;

        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES ('m1', 'Deleted', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let request_state = ChatRequestState::new();
        let token = request_state.claim_superseding_request(ChatRequestSurface::Chat, "request-1");
        let sources = vec![ChatSource {
            meeting_id: "m1".to_string(),
            meeting_title: "Deleted".to_string(),
            chunk_type: "transcript".to_string(),
            snippet: "private deleted snippet".to_string(),
            folder_name: "General".to_string(),
            source_kind: Some("transcript".to_string()),
        }];
        assert!(request_state.bind_request_meetings(
            ChatRequestSurface::Chat,
            "request-1",
            &token,
            &HashSet::from(["m1".to_string()])
        ));

        let invalidated = request_state.clone();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "m1", |meeting_id| {
                invalidated.invalidate_meeting(meeting_id);
            })
            .await
            .unwrap()
        );
        assert!(token.is_cancelled());
        assert!(!request_state.is_owner(ChatRequestSurface::Chat, "request-1", &token));

        // The ownership mechanism refuses the final answer/source payload and
        // cleans up.
        let err = finish_non_streaming_chat_request(
            &request_state,
            ChatRequestSurface::Chat,
            "request-1",
            &token,
            Ok(Ok(ChatResponse {
                answer: "answer quoting deleted evidence".to_string(),
                sources,
            })),
        )
        .unwrap_err();
        assert!(err.contains("superseded"));
        assert_eq!(request_state.request_count(), 0);

        // The delayed save of the already-emitted source set cannot re-persist
        // the deleted meeting's snippet: save-time sanitization keeps only
        // currently existing meetings.
        let conversation = ChatRepository::get_or_create_scoped_conversation(
            &pool,
            &ChatScope {
                kind: ChatScopeKind::All,
                key: "all".to_string(),
                data: None,
            },
            None,
        )
        .await
        .unwrap();
        ChatRepository::save_message(
            &pool,
            &conversation.id,
            "assistant",
            "delayed answer",
            Some(r#"[{"meetingId":"m1","meetingTitle":"Deleted","chunkType":"transcript","snippet":"private deleted snippet","folderName":"General","sourceKind":"transcript"}]"#),
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT sources_json FROM chat_messages WHERE content = 'delayed answer'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn meeting_list_deletion_between_preparation_and_binding_is_fenced() {
        use crate::database::repositories::meeting::MeetingsRepository;

        let pool = hybrid_test_pool().await;
        configure_hybrid_chat(&pool).await;
        for (id, title) in [("m1", "Root"), ("m2", "Child"), ("m3", "Other")] {
            sqlx::query(
                "INSERT INTO meetings (id, title, created_at, updated_at, saved_at) VALUES (?, ?, '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z')",
            )
            .bind(id)
            .bind(title)
            .execute(&pool)
            .await
            .unwrap();
        }
        let request_state = ChatRequestState::new();
        let token =
            request_state.claim_superseding_request(ChatRequestSurface::Chat, "list-request");
        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "list the meetings",
            None,
            ChatRetrievalScope::All,
            None,
            None,
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        // The listed titles entered the prompt without producing sources.
        assert!(inputs.sources.is_empty());
        assert!(inputs.user_prompt.contains("Root"));
        assert!(inputs.user_prompt.contains("Child"));
        assert!(inputs.user_prompt.contains("Other"));
        assert!(inputs.prompt_meeting_ids.contains("m1"));
        assert!(inputs.prompt_meeting_ids.contains("m2"));
        assert!(inputs.prompt_meeting_ids.contains("m3"));

        let invalidated = request_state.clone();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "m2", |meeting_id| {
                invalidated.invalidate_meeting(meeting_id);
            })
            .await
            .unwrap()
        );
        assert!(!token.is_cancelled());
        let sink = CapturingSink::default();
        let err = stream_chat(
            pool,
            sink.clone(),
            request_state.clone(),
            inputs,
            "list-request".to_string(),
            None,
            token,
        )
        .await
        .unwrap_err();
        assert_eq!(err, DELETED_MEETING_EVIDENCE_ERROR);
        assert!(sink.events().is_empty());
        assert_eq!(request_state.request_count(), 0);
    }

    #[tokio::test]
    async fn temporal_latest_deletion_at_terminal_fence_aborts_a_source_less_request() {
        use crate::database::repositories::meeting::MeetingsRepository;

        let (pool, _lifecycle) = hybrid_test_fixture(true, 3).await;
        let inputs = prepare_chat_inputs_for_scope(
            &pool,
            None,
            &reqwest::Client::new(),
            "what did we discuss",
            None,
            ChatRetrievalScope::SearchSnapshot(vec!["m2".to_string()]),
            None,
            Some(crate::retrieval::worker::RetrievalLifecycle::new(
                crate::retrieval::worker::LifecycleConfig::production(None),
            )),
            None,
            Some(ChatRetrievalMode::Fast),
            None,
        )
        .await
        .unwrap();

        // The temporal latest-meeting metadata (here: m2) entered the prompt.
        assert!(inputs.user_prompt.contains("Most recently saved"));
        assert!(
            inputs.prompt_meeting_ids.contains("m2"),
            "the temporal latest meeting must be bound"
        );

        let request_state = ChatRequestState::new();
        let token =
            request_state.claim_superseding_request(ChatRequestSurface::Chat, "temporal-request");
        assert!(request_state.bind_request_meetings(
            ChatRequestSurface::Chat,
            "temporal-request",
            &token,
            &inputs.prompt_meeting_ids
        ));
        let invalidated = request_state.clone();
        assert!(
            MeetingsRepository::delete_meeting(&pool, "m2", |meeting_id| {
                invalidated.invalidate_meeting(meeting_id);
            })
            .await
            .unwrap()
        );
        assert!(token.is_cancelled());
        let terminal_error = ensure_prompt_meetings_exist(&pool, &inputs.prompt_meeting_ids)
            .await
            .unwrap_err();
        assert_eq!(terminal_error, DELETED_MEETING_EVIDENCE_ERROR);
        let err = finish_non_streaming_chat_request(
            &request_state,
            ChatRequestSurface::Chat,
            "temporal-request",
            &token,
            Ok(Err::<ChatResponse, _>(terminal_error)),
        )
        .unwrap_err();
        assert!(err.contains("superseded"));
        assert_eq!(request_state.request_count(), 0);
    }
}
