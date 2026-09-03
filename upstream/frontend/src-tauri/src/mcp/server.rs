use axum::{
    extract::State as AxumState, http::StatusCode, response::IntoResponse, routing::post, Json,
    Router,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tracing::info;

use crate::api::chat::{
    finish_non_streaming_chat_request, prepare_chat_inputs_with_lifecycle, ChatRequestSurface,
    ChatRetrievalMode, CHAT_REQUEST_TIMEOUT, SYSTEM_PROMPT,
};
use crate::database::repositories::{folder::FolderRepository, fts::FtsRepository};
use crate::export::build_context_markdown;
use crate::summary::llm_client::generate_summary;

pub const DEFAULT_PORT: u16 = 5167;

pub(crate) fn serialize_chat_sources(
    sources: &[crate::api::chat::ChatSource],
) -> Result<Vec<serde_json::Value>, String> {
    sources
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()
        .map_err(|error| format!("Failed to serialize chat source: {error}"))
}

#[derive(Clone)]
pub struct McpState {
    pub pool: SqlitePool,
    pub app_data_dir: Option<std::path::PathBuf>,
    pub client: reqwest::Client,
    /// Clone of the process-wide retrieval lifecycle. MCP shares the Tauri
    /// runtime instead of constructing duplicate workers or model sessions.
    pub retrieval: crate::retrieval::worker::RetrievalLifecycle,
    /// Clone of the ONE shared chat request registry, so MCP chat requests
    /// participate in deletion invalidation and ownership cleanup.
    pub chat_requests: crate::api::chat::ChatRequestState,
}

// ---------- JSON-RPC request/response ----------

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

// ---------- Tool definitions ----------

#[derive(Debug, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "search_meetings".to_string(),
            description: "Full-text search across meeting transcripts, summaries, and notes. Returns ranked results with BM25 scoring. Supports folder:\"name\" filter syntax.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (supports folder:\"name\" prefix to filter by folder)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 20, max: 50)",
                        "default": 20
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "build_context".to_string(),
            description: "Build structured Markdown context from meeting search results, grouped by meeting. Use this when you need to prepare context for an LLM prompt.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query to find relevant meeting content"
                    },
                    "max_chunks": {
                        "type": "integer",
                        "description": "Maximum number of chunks to include (default: 20, max: 100)",
                        "default": 20
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "chat_with_meetings".to_string(),
            description: "Ask a question and get an AI-generated answer based on meeting transcripts, summaries, and notes. Returns answer with source citations.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The question to answer from meeting data"
                    },
                    "meetingId": {
                        "type": "string",
                        "description": "Optional meeting ID to scope the answer to one meeting"
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "list_folders".to_string(),
            description: "List all meeting folders. Returns flat list with id, name, parent_id for tree construction.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    ]
}

// ---------- Tool execution ----------

async fn execute_search_meetings(
    pool: &SqlitePool,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let query = params["query"]
        .as_str()
        .ok_or("Missing 'query' parameter")?
        .to_string();
    let limit = params["limit"].as_u64().unwrap_or(20).min(50) as u32;

    let results = FtsRepository::search(pool, &query, limit, None)
        .await
        .map_err(|e| format!("Search failed: {}", e))?;

    Ok(serde_json::json!({
        "results": results,
        "total": results.len()
    }))
}

async fn execute_build_context(
    pool: &SqlitePool,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let query = params["query"]
        .as_str()
        .ok_or("Missing 'query' parameter")?
        .to_string();
    let max_chunks = params["max_chunks"].as_u64().unwrap_or(20).min(100) as u32;

    let results = FtsRepository::search(pool, &query, max_chunks, None)
        .await
        .map_err(|e| format!("FTS search failed: {}", e))?;
    let context = build_context_markdown(&results);

    Ok(serde_json::json!({
        "context": context,
        "chunks_used": results.len()
    }))
}

/// Releases one request's registry entry when dropped unless ownership was
/// already released (idempotent): a panic or abort of the request future
/// cannot leak an admitted slot.
struct OwnershipGuard<'a> {
    state: &'a crate::api::chat::ChatRequestState,
    surface: ChatRequestSurface,
    request_id: &'a str,
    token: &'a crate::api::chat::ChatRequestToken,
}

impl Drop for OwnershipGuard<'_> {
    fn drop(&mut self) {
        // Cancel BEFORE clearing: pending detached work (spawn_blocking
        // retrieval/ONNX holding a cloned token) must observe cancellation
        // before the slot can be reclaimed and reused. Idempotent on every
        // path — finish already cleared and cancelled where appropriate.
        self.token.cancel();
        self.state
            .clear_if_owner(self.surface, self.request_id, self.token);
    }
}

pub(crate) async fn execute_chat_with_meetings(
    pool: &SqlitePool,
    params: &serde_json::Value,
    app_data_dir: &Option<std::path::PathBuf>,
    client: &reqwest::Client,
    retrieval: crate::retrieval::worker::RetrievalLifecycle,
    chat_requests: &crate::api::chat::ChatRequestState,
    deadline: std::time::Duration,
) -> Result<serde_json::Value, String> {
    // Each MCP chat request owns one internal identity + token through the
    // SAME shared registry/mechanism as Chat: admission is capped
    // (MAX_CONCURRENT_MCP_REQUESTS, checked atomically before ANY work — a
    // rejected request never starts preparation or generation), deletion
    // invalidation cancels admitted requests, and every return/error/timeout
    // path releases ownership through `finish_non_streaming_chat_request`
    // (the final publication gate, which also closes the check-to-return
    // race). No public MCP cancel API exists.
    let request_id = format!("mcp-{}", uuid::Uuid::new_v4());
    let Some(token) = chat_requests.try_claim_request(ChatRequestSurface::Mcp, &request_id) else {
        return Err(crate::api::chat::MCP_CHAT_BUSY_ERROR.to_string());
    };
    // Releases this request's registry entry on EVERY exit — timeout, error,
    // deletion cancellation, and even a panic/abort of the request future —
    // idempotent after finish's own release.
    let guard = OwnershipGuard {
        state: chat_requests,
        surface: ChatRequestSurface::Mcp,
        request_id: &request_id,
        token: &token,
    };
    // One owned deadline lifecycle: the deadline sleep is driven inside the
    // same future as the request work, so expiry cancels the token BEFORE the
    // request future is dropped and BEFORE registry cleanup — there is no
    // detached watchdog task and the outcome is deterministic (biased polling
    // makes the deadline win an equal-deadline tie). Deletion invalidation
    // cancels the same token through the shared registry.
    let deadline_sleep = tokio::time::sleep(deadline);
    tokio::pin!(deadline_sleep);
    let work = async {
        // The SAME request token is passed through ALL shared preparation
        // work — retrieval, scheduler/ONNX queueing, and Deep eligibility —
        // so timeout or deletion cancellation stops it before generation.
        let (inputs, _sources) = prepare_mcp_chat_inputs(
            pool,
            params,
            app_data_dir,
            client,
            retrieval,
            Some(token.as_ref()),
        )
        .await?;
        // Deletion fence, in this exact order: bind the prepared evidence's
        // meeting identities BEFORE the authoritative rechecks, so a meeting
        // deleted afterwards cancels this visible registration from the real
        // deletion transaction.
        chat_requests.bind_request_meetings(
            ChatRequestSurface::Mcp,
            &request_id,
            &token,
            &crate::api::chat::prepared_meeting_ids(&inputs.sources),
        );
        crate::api::chat::ensure_prepared_meetings_exist(pool, &inputs.sources).await?;
        let answer = generate_summary(
            client,
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
            app_data_dir.as_ref(),
            Some(token.as_ref()),
        )
        .await
        .map_err(|e| format!("LLM call failed: {}", e))?;
        // Terminal invalidation fence: a meeting deleted during generation
        // must abort the response instead of returning an answer whose
        // sources can no longer exist (source/context parity).
        crate::api::chat::ensure_prepared_meetings_exist(pool, &inputs.sources).await?;
        let sources = serialize_chat_sources(&inputs.sources)?;
        Ok(serde_json::json!({ "answer": answer, "sources": sources }))
    };
    tokio::pin!(work);
    let inner = tokio::select! {
        biased;
        _ = &mut deadline_sleep => {
            // Deadline first: cancellation precedes dropping the request
            // future and any registry cleanup, so the outcome is
            // deterministically the cancelled one.
            token.cancel();
            Err("Chat request timed out".to_string())
        }
        result = &mut work => result,
    };
    // The final publication/ownership gate; the guard below is the backstop
    // for any path that bypassed it and is idempotent with this release.
    let outcome = finish_non_streaming_chat_request(
        chat_requests,
        ChatRequestSurface::Mcp,
        &request_id,
        &token,
        Ok(inner),
    );
    drop(guard);
    outcome
}

async fn prepare_mcp_chat_inputs(
    pool: &SqlitePool,
    params: &serde_json::Value,
    app_data_dir: &Option<std::path::PathBuf>,
    client: &reqwest::Client,
    retrieval: crate::retrieval::worker::RetrievalLifecycle,
    cancellation: Option<&tokio_util::sync::CancellationToken>,
) -> Result<(crate::api::chat::ChatInputs, Vec<serde_json::Value>), String> {
    let query = params["query"]
        .as_str()
        .ok_or("Missing 'query' parameter")?
        .to_string();
    let meeting_id = params["meetingId"].as_str().map(str::to_string);
    let mode = params
        .get("mode")
        .filter(|value| !value.is_null())
        .map(|value| {
            serde_json::from_value::<ChatRetrievalMode>(value.clone())
                .map_err(|_| "Invalid chat retrieval mode; expected 'fast' or 'deep'".to_string())
        })
        .transpose()?;
    if mode == Some(ChatRetrievalMode::Deep) {
        return Err("MCP Chat supports Fast mode only".to_string());
    }
    let inputs = prepare_chat_inputs_with_lifecycle(
        pool,
        app_data_dir.clone(),
        client,
        &query,
        None,
        meeting_id,
        retrieval,
        cancellation,
        mode,
        None,
    )
    .await?;
    let sources = serialize_chat_sources(&inputs.sources)?;
    Ok((inputs, sources))
}

async fn execute_list_folders(pool: &SqlitePool) -> Result<serde_json::Value, String> {
    let folders = FolderRepository::get_all(pool)
        .await
        .map_err(|e| format!("Failed to list folders: {}", e))?;

    let folders_json: Vec<serde_json::Value> = folders
        .iter()
        .map(|f| {
            serde_json::json!({
                "id": f.id,
                "name": f.name,
                "parentId": f.parent_id,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "folders": folders_json
    }))
}

// ---------- Router ----------

pub fn app(state: McpState) -> Router {
    Router::new()
        .route("/", post(handle_jsonrpc))
        .with_state(state)
}

async fn handle_jsonrpc(
    AxumState(state): AxumState<McpState>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    info!("MCP request: method={}", request.method);

    let response = match request.method.as_str() {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(serde_json::json!({
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": "meetily-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            error: None,
        },
        "notifications/initialized" => {
            return (StatusCode::OK, Json(serde_json::json!({}))).into_response();
        }
        "tools/list" => {
            let tools = tool_definitions();
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(serde_json::json!({ "tools": tools })),
                error: None,
            }
        }
        "tools/call" => {
            let tool_name = request.params["name"].as_str().unwrap_or("");
            let tool_args = &request.params["arguments"];

            let result = match tool_name {
                "search_meetings" => execute_search_meetings(&state.pool, tool_args).await,
                "build_context" => execute_build_context(&state.pool, tool_args).await,
                "chat_with_meetings" => {
                    execute_chat_with_meetings(
                        &state.pool,
                        tool_args,
                        &state.app_data_dir,
                        &state.client,
                        state.retrieval.clone(),
                        &state.chat_requests,
                        CHAT_REQUEST_TIMEOUT,
                    )
                    .await
                }
                "list_folders" => execute_list_folders(&state.pool).await,
                _ => Err(format!("Unknown tool: {}", tool_name)),
            };

            match result {
                Ok(value) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: Some(serde_json::json!({
                        "content": [{ "type": "text", "text": value.to_string() }],
                        "isError": false
                    })),
                    error: None,
                },
                Err(e) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: Some(serde_json::json!({
                        "content": [{ "type": "text", "text": e }],
                        "isError": true
                    })),
                    error: None,
                },
            }
        }
        "ping" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(serde_json::json!({})),
            error: None,
        },
        _ => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
            }),
        },
    };

    (StatusCode::OK, Json(response)).into_response()
}

// ---------- Server startup ----------

/// Spawn the MCP server from a Tauri AppHandle.
/// Call this after AppState is managed (either normal startup or first-launch init).
pub fn spawn_from_app<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        let pool = state.db_manager.pool().clone();
        let retrieval = app
            .try_state::<crate::retrieval::worker::RetrievalLifecycle>()
            .map(|lifecycle| lifecycle.inner().clone());
        let chat_requests = app
            .try_state::<crate::api::chat::ChatRequestState>()
            .map(|requests| requests.inner().clone())
            .unwrap_or_else(crate::api::chat::ChatRequestState::new);
        let app_data_dir = app.path().app_data_dir().ok();
        if let Some(retrieval) = retrieval {
            tauri::async_runtime::spawn(async move {
                start_server(pool, app_data_dir, None, retrieval, chat_requests).await;
            });
        } else {
            tracing::error!("RetrievalLifecycle not available, MCP server not started");
        }
    } else {
        tracing::warn!("AppState not available, MCP server not started");
    }
}

pub async fn start_server(
    pool: SqlitePool,
    app_data_dir: Option<std::path::PathBuf>,
    port: Option<u16>,
    retrieval: crate::retrieval::worker::RetrievalLifecycle,
    chat_requests: crate::api::chat::ChatRequestState,
) {
    let port = port.unwrap_or(DEFAULT_PORT);
    let state = McpState {
        pool,
        app_data_dir,
        client: reqwest::Client::new(),
        retrieval,
        chat_requests,
    };

    let listener = match TcpListener::bind(format!("127.0.0.1:{}", port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("MCP server failed to bind to port {}: {}", port, e);
            return;
        }
    };

    info!("MCP server starting on http://127.0.0.1:{}", port);

    let app = app(state);
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| tracing::error!("MCP server error: {}", e));
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_definitions_returns_4_tools() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 4);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"search_meetings"));
        assert!(names.contains(&"build_context"));
        assert!(names.contains(&"chat_with_meetings"));
        assert!(names.contains(&"list_folders"));
    }

    #[test]
    fn jsonrpc_response_serializes() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            result: Some(json!({"tools": []})),
            error: None,
        };
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains("tools"));
        assert!(!s.contains("error"));
    }

    #[test]
    fn jsonrpc_error_response_serializes() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
            }),
        };
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains("error"));
        assert!(!s.contains("result"));
    }

    #[test]
    fn shared_chat_source_serialization_preserves_source_kind() {
        let source = crate::api::chat::ChatSource {
            meeting_id: "meeting".into(),
            meeting_title: "Title".into(),
            chunk_type: "transcript".into(),
            snippet: "text".into(),
            folder_name: "Folder".into(),
            source_kind: Some("transcript".into()),
        };
        let value = serialize_chat_sources(&[source]).unwrap();
        assert_eq!(value[0]["sourceKind"], "transcript");
        assert_eq!(value[0]["snippet"], "text");
    }

    #[tokio::test]
    async fn chat_preparation_uses_managed_forced_lexical_boundary() {
        let pool = chat_pool(true).await;
        let lifecycle = crate::retrieval::worker::RetrievalLifecycle::new(
            crate::retrieval::worker::LifecycleConfig::production(None),
        );
        let (inputs, sources) = prepare_mcp_chat_inputs(
            &pool,
            &json!({"query": "alpha"}),
            &None,
            &reqwest::Client::new(),
            lifecycle.clone(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            inputs.retrieval_diagnostic,
            crate::api::chat::RetrievalPreparationDiagnostic::ForcedLexical
        );
        assert_eq!(inputs.retrieval_mode, ChatRetrievalMode::Fast);
        assert!(inputs.sources[0].source_kind.is_none());
        assert!(sources[0].get("sourceKind").is_none());
        assert_eq!(sources[0]["meetingId"], "m1");

        let error = match prepare_mcp_chat_inputs(
            &pool,
            &json!({"query": "alpha", "mode": "deep"}),
            &None,
            &reqwest::Client::new(),
            lifecycle.clone(),
            None,
        )
        .await
        {
            Ok(_) => panic!("Deep MCP mode must be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("Fast mode only"));

        let error = match prepare_mcp_chat_inputs(
            &pool,
            &json!({"query": "alpha", "mode": "unknown"}),
            &None,
            &reqwest::Client::new(),
            lifecycle,
            None,
        )
        .await
        {
            Ok(_) => panic!("Unknown MCP retrieval mode must be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("Invalid chat retrieval mode"));
    }

    /// The MCP terminal fence end to end: a meeting deleted (through the real
    /// deletion transaction with the real invalidation hook) while the answer
    /// is being generated cancels the MCP request's ownership token and
    /// aborts the response instead of returning an answer whose sources can
    /// no longer exist. Ownership is released on the abort path.
    #[tokio::test]
    async fn deleted_meeting_cancels_the_mcp_request_and_aborts_the_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let pool = chat_pool(false).await;
        let chat_requests = crate::api::chat::ChatRequestState::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (generation_called_tx, generation_called_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 64 * 1024];
            let _ = socket.read(&mut request).await;
            let _ = generation_called_tx.send(());
            let _ = release_rx.await;
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

        let task_pool = pool.clone();
        let task_requests = chat_requests.clone();
        let task = tokio::spawn(async move {
            execute_chat_with_meetings(
                &task_pool,
                &json!({"query": "alpha", "meetingId": "m1"}),
                &None,
                &reqwest::Client::new(),
                crate::retrieval::worker::RetrievalLifecycle::new(
                    crate::retrieval::worker::LifecycleConfig::production(None),
                ),
                &task_requests,
                CHAT_REQUEST_TIMEOUT,
            )
            .await
        });

        // The answer generation is in flight when the meeting is deleted
        // through the real deletion transaction with the real hook: the MCP
        // request's bound registration is invalidated and its token cancelled.
        tokio::time::timeout(std::time::Duration::from_secs(10), generation_called_rx)
            .await
            .unwrap()
            .unwrap();
        let invalidated = chat_requests.clone();
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
        let _ = release_tx.send(());

        let result = tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        // Fail-closed abort: no answer/source payload can be returned.
        let error = match result {
            Ok(_) => panic!("deleted meeting must not return an MCP answer"),
            Err(error) => error,
        };
        assert!(
            error.contains("cancelled or superseded")
                || error.contains("referenced meeting was deleted"),
            "unexpected abort error: {error}"
        );
        // Cleanup proof: the invalidated MCP registration was removed.
        assert_eq!(chat_requests.request_count(), 0);
    }

    /// The check-to-return race, closed by the ownership gate: the terminal
    /// existence recheck PASSES while the meeting exists, the meeting is then
    /// deleted through the real deletion transaction (invalidating the bound
    /// request inside the transaction, before commit), and the final
    /// publication gate refuses the already-built answer/source payload.
    #[tokio::test]
    async fn deletion_after_the_terminal_recheck_cannot_return_the_mcp_response() {
        let pool = chat_pool(false).await;
        let chat_requests = crate::api::chat::ChatRequestState::new();
        let lifecycle = crate::retrieval::worker::RetrievalLifecycle::new(
            crate::retrieval::worker::LifecycleConfig::production(None),
        );
        let (inputs, _sources) = prepare_mcp_chat_inputs(
            &pool,
            &json!({"query": "alpha", "meetingId": "m1"}),
            &None,
            &reqwest::Client::new(),
            lifecycle,
            None,
        )
        .await
        .unwrap();

        // Production ordering: bind the prepared evidence identities, then
        // recheck existence.
        let request_id = "mcp-check-to-return".to_string();
        let token = chat_requests
            .try_claim_request(ChatRequestSurface::Mcp, &request_id)
            .expect("below the admission cap");
        assert!(chat_requests.bind_request_meetings(
            ChatRequestSurface::Mcp,
            &request_id,
            &token,
            &crate::api::chat::prepared_meeting_ids(&inputs.sources),
        ));
        crate::api::chat::ensure_prepared_meetings_exist(&pool, &inputs.sources)
            .await
            .unwrap();
        // The terminal existence recheck PASSES here (the meeting exists).
        crate::api::chat::ensure_prepared_meetings_exist(&pool, &inputs.sources)
            .await
            .unwrap();

        // Deletion commits strictly AFTER the passing terminal recheck and
        // strictly BEFORE the return/publication gate, with the real
        // invalidation hook inside the real transaction.
        let invalidated = chat_requests.clone();
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

        // The publication gate refuses the already-built answer/source
        // payload and releases ownership.
        let err = finish_non_streaming_chat_request(
            &chat_requests,
            ChatRequestSurface::Mcp,
            &request_id,
            &token,
            Ok(Ok(json!({
                "answer": "answer quoting deleted evidence",
                "sources": [{"meetingId": "m1"}],
            }))),
        )
        .unwrap_err();
        assert!(err.contains("superseded"));
        assert_eq!(chat_requests.request_count(), 0);
    }

    /// Two concurrent MCP chats are independent: neither claim supersedes the
    /// other, both are in flight simultaneously, both publish normally, and
    /// both release their own ownership entries.
    #[tokio::test]
    async fn concurrent_mcp_chats_coexist_without_superseding_each_other() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::sync::mpsc;

        let pool = chat_pool(false).await;
        let chat_requests = crate::api::chat::ChatRequestState::new();
        // One endpoint serves both requests; every connection is barriered
        // until the test has proven both requests are in flight together.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (called_tx, mut called_rx) = mpsc::channel(4);
        let (release_one_tx, release_one_rx) = tokio::sync::oneshot::channel::<()>();
        let (release_two_tx, release_two_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            // Accept BOTH connections up front and handle them concurrently,
            // so the second request's generation is not blocked behind the
            // first one's barrier.
            for release_rx in [release_one_rx, release_two_rx] {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let called_tx = called_tx.clone();
                tokio::spawn(async move {
                    let mut request = vec![0_u8; 64 * 1024];
                    let _ = socket.read(&mut request).await;
                    let _ = called_tx.send(()).await;
                    let _ = release_rx.await;
                    let body = r#"{"choices":[{"message":{"content":"answer"}}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        });
        sqlx::query("UPDATE settings SET chatOllamaEndpoint = ?")
            .bind(format!("http://{}", address))
            .execute(&pool)
            .await
            .unwrap();

        let spawn_request = |chat_requests: crate::api::chat::ChatRequestState| {
            let task_pool = pool.clone();
            tokio::spawn(async move {
                execute_chat_with_meetings(
                    &task_pool,
                    &json!({"query": "alpha", "meetingId": "m1"}),
                    &None,
                    &reqwest::Client::new(),
                    crate::retrieval::worker::RetrievalLifecycle::new(
                        crate::retrieval::worker::LifecycleConfig::production(None),
                    ),
                    &chat_requests,
                    CHAT_REQUEST_TIMEOUT,
                )
                .await
            })
        };
        let requests_one = chat_requests.clone();
        let requests_two = chat_requests.clone();
        let task_one = spawn_request(requests_one);
        let task_two = spawn_request(requests_two);

        // Both requests reach generation concurrently: neither superseded
        // the other, so both registrations are alive at once.
        tokio::time::timeout(std::time::Duration::from_secs(10), called_rx.recv())
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), called_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(chat_requests.request_count(), 2);

        let _ = release_one_tx.send(());
        let _ = release_two_tx.send(());
        let result_one = tokio::time::timeout(std::time::Duration::from_secs(10), task_one)
            .await
            .unwrap()
            .unwrap();
        let result_two = tokio::time::timeout(std::time::Duration::from_secs(10), task_two)
            .await
            .unwrap()
            .unwrap();
        // Both publish normally and release their own entries.
        assert!(result_one.is_ok());
        assert!(result_two.is_ok());
        assert_eq!(chat_requests.request_count(), 0);
    }

    /// The MCP admission cap: with the shared registry saturated, an excess
    /// MCP chat request is rejected with the stable busy error BEFORE any
    /// provider work (no generation connection is ever attempted), admitted
    /// requests are untouched, and reclaimed capacity admits a new request.
    #[tokio::test]
    async fn saturated_mcp_admission_rejects_before_generation_and_reclaims() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::sync::mpsc;

        let pool = chat_pool(false).await;
        let chat_requests = crate::api::chat::ChatRequestState::new();
        // Simulate admitted in-flight requests saturating the cap.
        let mut admitted = Vec::new();
        for index in 0..crate::api::chat::MAX_CONCURRENT_MCP_REQUESTS {
            let token = chat_requests
                .try_claim_request(ChatRequestSurface::Mcp, &format!("mcp-admitted-{index}"))
                .unwrap();
            admitted.push(token);
        }

        // One fake provider: if the rejected request were to start
        // generation, a connection would arrive; the test proves none does.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (called_tx, mut called_rx) = mpsc::channel(4);
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 64 * 1024];
            let _ = socket.read(&mut request).await;
            let _ = called_tx.send(()).await;
            let _ = release_rx.await;
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

        let task_pool = pool.clone();
        let result = execute_chat_with_meetings(
            &task_pool,
            &json!({"query": "alpha", "meetingId": "m1"}),
            &None,
            &reqwest::Client::new(),
            crate::retrieval::worker::RetrievalLifecycle::new(
                crate::retrieval::worker::LifecycleConfig::production(None),
            ),
            &chat_requests,
            CHAT_REQUEST_TIMEOUT,
        )
        .await;
        // Rejected with the stable busy error before any work.
        match result {
            Ok(_) => panic!("saturated MCP admission must reject the request"),
            Err(error) => assert_eq!(error, crate::api::chat::MCP_CHAT_BUSY_ERROR),
        }
        // No generation connection was attempted.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), called_rx.recv())
                .await
                .is_err(),
            "a capped request must never start generation"
        );
        // Admitted requests are untouched and capacity is still saturated.
        for (index, token) in admitted.iter().enumerate() {
            assert!(
                !token.is_cancelled(),
                "admitted request {index} was disturbed"
            );
        }
        assert_eq!(
            chat_requests.request_count(),
            crate::api::chat::MAX_CONCURRENT_MCP_REQUESTS
        );

        // Reclaim: one admitted request completes (success cleanup), and the
        // freed capacity admits a new request that reaches generation.
        chat_requests.clear_if_owner(ChatRequestSurface::Mcp, "mcp-admitted-0", &admitted[0]);
        let task_pool = pool.clone();
        let task = tokio::spawn(async move {
            execute_chat_with_meetings(
                &task_pool,
                &json!({"query": "alpha", "meetingId": "m1"}),
                &None,
                &reqwest::Client::new(),
                crate::retrieval::worker::RetrievalLifecycle::new(
                    crate::retrieval::worker::LifecycleConfig::production(None),
                ),
                &chat_requests,
                CHAT_REQUEST_TIMEOUT,
            )
            .await
        });
        let _ = release_tx.send(());
        let admitted_result = tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            admitted_result.is_ok(),
            "reclaimed capacity must be admittable"
        );
        // The new request started generation (the connection arrived).
        let generation_ran =
            tokio::time::timeout(std::time::Duration::from_secs(10), called_rx.recv())
                .await
                .unwrap()
                .is_some();
        assert!(generation_ran, "the admitted request must reach generation");
    }

    #[tokio::test]
    async fn omitted_mode_resolves_to_fast_without_the_forced_lexical_mask() {
        let pool = chat_pool(false).await;
        let lifecycle = crate::retrieval::worker::RetrievalLifecycle::new(
            crate::retrieval::worker::LifecycleConfig::production(None),
        );
        // No mode field, and the kill switch is OFF: the ordinary default
        // must still resolve to Fast through shared preparation, never Deep.
        let (inputs, _sources) = prepare_mcp_chat_inputs(
            &pool,
            &json!({"query": "alpha"}),
            &None,
            &reqwest::Client::new(),
            lifecycle.clone(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(inputs.retrieval_mode, ChatRetrievalMode::Fast);
        assert_ne!(
            inputs.retrieval_diagnostic,
            crate::api::chat::RetrievalPreparationDiagnostic::ForcedLexical
        );
        // Omitted mode makes no planner round trips.
        assert_eq!(inputs.provider_round_trips, 0);
        assert!(!inputs.sources.is_empty());

        // An explicit fast request behaves identically.
        let (inputs, _sources) = prepare_mcp_chat_inputs(
            &pool,
            &json!({"query": "alpha", "mode": "fast"}),
            &None,
            &reqwest::Client::new(),
            lifecycle,
            None,
        )
        .await
        .unwrap();
        assert_eq!(inputs.retrieval_mode, ChatRetrievalMode::Fast);
        assert_eq!(inputs.provider_round_trips, 0);
    }

    /// Minimal shared-preparation schema: one meeting with an indexed note
    /// and transcript, and a configurable force-lexical switch.
    async fn chat_pool(force_lexical: bool) -> SqlitePool {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::query(r#"CREATE TABLE meetings (id TEXT PRIMARY KEY, title TEXT NOT NULL, folder_id TEXT, created_at TEXT, saved_at TEXT);
            CREATE TABLE meeting_folders (id TEXT PRIMARY KEY, name TEXT NOT NULL, parent_id TEXT, created_at TEXT NOT NULL);
            CREATE TABLE transcripts (id TEXT PRIMARY KEY, meeting_id TEXT NOT NULL, transcript TEXT NOT NULL, timestamp TEXT NOT NULL, speaker TEXT, audio_start_time REAL, audio_end_time REAL);
            CREATE TABLE meeting_notes (meeting_id TEXT PRIMARY KEY, notes_markdown TEXT);
            CREATE TABLE summary_processes (meeting_id TEXT NOT NULL, template_id TEXT NOT NULL, updated_at TEXT NOT NULL, result TEXT, PRIMARY KEY (meeting_id, template_id));
            CREATE TABLE search_source_state (meeting_id TEXT PRIMARY KEY, source_revision INTEGER);
            CREATE TABLE settings (id TEXT PRIMARY KEY, provider TEXT NOT NULL, model TEXT NOT NULL, whisperModel TEXT NOT NULL, groqApiKey TEXT, openaiApiKey TEXT, anthropicApiKey TEXT, ollamaApiKey TEXT, openRouterApiKey TEXT, ollamaEndpoint TEXT, customOpenAIConfig TEXT, customVocabulary TEXT, chatProvider TEXT, chatModel TEXT, chatOllamaEndpoint TEXT, force_lexical_retrieval BOOLEAN NOT NULL DEFAULT FALSE);
            CREATE VIRTUAL TABLE meeting_fts USING fts5(meeting_id UNINDEXED, chunk_type UNINDEXED, chunk_id UNINDEXED, text, speaker UNINDEXED, timestamp_label UNINDEXED, folder_id UNINDEXED, folder_name);
            CREATE TABLE transcript_chunks (meeting_id TEXT);
            CREATE TABLE retrieval_generations (generation_id TEXT PRIMARY KEY NOT NULL, model_id TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'building', document_count INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT '');
            CREATE TABLE retrieval_documents (id INTEGER PRIMARY KEY AUTOINCREMENT, generation_id TEXT NOT NULL REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE, document_id TEXT NOT NULL, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE, source_kind TEXT NOT NULL DEFAULT '', ordinal INTEGER NOT NULL DEFAULT 0, content TEXT NOT NULL DEFAULT '', content_hash BLOB NOT NULL DEFAULT x'', dimensions INTEGER NOT NULL DEFAULT 2 CHECK (dimensions > 0), vector_encoding TEXT NOT NULL DEFAULT 'int8', vector BLOB NOT NULL DEFAULT x'', source_revision INTEGER NOT NULL DEFAULT 0, updated_at TEXT NOT NULL DEFAULT '', UNIQUE (generation_id, document_id));
            CREATE TABLE chat_messages (id TEXT PRIMARY KEY NOT NULL, conversation_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, sources_json TEXT, is_error INTEGER DEFAULT 0, created_at TEXT NOT NULL);
            INSERT INTO meetings (id, title, created_at, saved_at) VALUES ('m1', 'Title', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z');
            INSERT INTO settings (id, provider, model, whisperModel, chatProvider, chatModel, force_lexical_retrieval) VALUES ('1', 'ollama', 'local', 'whisper', 'ollama', 'local', ?);
            INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('t1', 'm1', 'alpha', '10:00');
            INSERT INTO meeting_notes (meeting_id, notes_markdown) VALUES ('m1', 'alpha note text');
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, folder_name) VALUES ('m1', 'note', 'n1', 'alpha', 'General');"#)
            .bind(force_lexical)
            .execute(&pool).await.unwrap();
        pool
    }
}
