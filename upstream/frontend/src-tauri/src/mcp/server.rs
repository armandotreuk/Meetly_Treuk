use axum::{
    extract::State as AxumState, http::StatusCode, response::IntoResponse, routing::post, Json,
    Router,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tracing::info;

use crate::api::chat::{prepare_chat_inputs_with_lifecycle, ChatRetrievalMode, SYSTEM_PROMPT};
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

async fn execute_chat_with_meetings(
    pool: &SqlitePool,
    params: &serde_json::Value,
    app_data_dir: &Option<std::path::PathBuf>,
    client: &reqwest::Client,
    retrieval: crate::retrieval::worker::RetrievalLifecycle,
) -> Result<serde_json::Value, String> {
    let (inputs, sources) =
        prepare_mcp_chat_inputs(pool, params, app_data_dir, client, retrieval).await?;
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
        None,
    )
    .await
    .map_err(|e| format!("LLM call failed: {}", e))?;
    Ok(serde_json::json!({ "answer": answer, "sources": sources }))
}

async fn prepare_mcp_chat_inputs(
    pool: &SqlitePool,
    params: &serde_json::Value,
    app_data_dir: &Option<std::path::PathBuf>,
    client: &reqwest::Client,
    retrieval: crate::retrieval::worker::RetrievalLifecycle,
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
        None,
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
        let app_data_dir = app.path().app_data_dir().ok();
        if let Some(retrieval) = retrieval {
            tauri::async_runtime::spawn(async move {
                start_server(pool, app_data_dir, None, retrieval).await;
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
) {
    let port = port.unwrap_or(DEFAULT_PORT);
    let state = McpState {
        pool,
        app_data_dir,
        client: reqwest::Client::new(),
        retrieval,
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
        )
        .await
        {
            Ok(_) => panic!("Unknown MCP retrieval mode must be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("Invalid chat retrieval mode"));
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
