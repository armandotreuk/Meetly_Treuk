use axum::{
    extract::State as AxumState,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tracing::info;

use crate::database::repositories::{
    fts::FtsRepository,
    folder::FolderRepository,
    setting::SettingsRepository,
};
use crate::export::build_context_markdown;
use crate::summary::llm_client::{generate_summary, LLMProvider};

pub const DEFAULT_PORT: u16 = 5167;

#[derive(Clone)]
pub struct McpState {
    pub pool: SqlitePool,
    pub app_data_dir: Option<std::path::PathBuf>,
    pub client: reqwest::Client,
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

    let results = FtsRepository::search(pool, &query, limit)
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

    let results = FtsRepository::search(pool, &query, max_chunks)
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
) -> Result<serde_json::Value, String> {
    let query = params["query"]
        .as_str()
        .ok_or("Missing 'query' parameter")?
        .to_string();

    // 1. FTS search
    let results = FtsRepository::search(pool, &query, 10)
        .await
        .map_err(|e| format!("FTS search failed: {}", e))?;
    let context = build_context_markdown(&results);

    // 2. Build sources
    let sources: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "meetingId": r.meeting_id,
                "meetingTitle": r.meeting_title,
                "chunkType": r.chunk_type,
                "snippet": r.snippet,
                "folderName": r.folder_name,
            })
        })
        .collect();

    // 3. Get LLM config (chat-specific, falls back to summary config)
    let model_config = SettingsRepository::get_chat_model_config(pool)
        .await
        .map_err(|e| format!("Failed to get model config: {}", e))?
        .ok_or_else(|| "No model configured. Please set a model in Settings.".to_string())?;

    let (model_provider_str, model_name, chat_ollama_endpoint) =
        SettingsRepository::resolve_chat_config(&model_config);

    let provider = LLMProvider::from_str(&model_provider_str)?;

    // CustomOpenAI config (has its own API key + endpoint)
    let (custom_openai_endpoint, custom_openai_api_key, custom_openai_max_tokens, custom_openai_temperature, custom_openai_top_p) =
        if provider == LLMProvider::CustomOpenAI {
            match SettingsRepository::get_custom_openai_config(pool).await {
                Ok(Some(config)) => (
                    Some(config.endpoint),
                    config.api_key.unwrap_or_default(),
                    config.max_tokens.map(|t| t as u32),
                    config.temperature,
                    config.top_p,
                ),
                _ => return Err("Custom OpenAI selected but no config found".to_string()),
            }
        } else {
            (None, String::new(), None, None, None)
        };

    let api_key = if provider == LLMProvider::Ollama
        || provider == LLMProvider::BuiltInAI
    {
        String::new()
    } else if provider == LLMProvider::CustomOpenAI {
        custom_openai_api_key
    } else {
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

    let system_prompt = "You are a helpful meeting assistant. Answer the user's question based on the meeting context provided below. If the context doesn't contain enough information, say so. Be concise and cite specific meetings when relevant.";
    let user_prompt = format!(
        "User question: {}\n\nMeeting context:\n{}",
        query, context
    );

    let answer = generate_summary(
        client,
        &provider,
        &model_name,
        &api_key,
        system_prompt,
        &user_prompt,
        ollama_endpoint.as_deref(),
        custom_openai_endpoint.as_deref(),
        custom_openai_max_tokens,
        custom_openai_temperature,
        custom_openai_top_p,
        app_data_dir.as_ref(),
        None,
    )
    .await
    .map_err(|e| format!("LLM call failed: {}", e))?;

    Ok(serde_json::json!({
        "answer": answer,
        "sources": sources
    }))
}

async fn execute_list_folders(
    pool: &SqlitePool,
) -> Result<serde_json::Value, String> {
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
        "initialize" => {
            JsonRpcResponse {
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
            }
        }
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
            let tool_name = request.params["name"]
                .as_str()
                .unwrap_or("");
            let tool_args = &request.params["arguments"];

            let result = match tool_name {
                "search_meetings" => execute_search_meetings(&state.pool, tool_args).await,
                "build_context" => execute_build_context(&state.pool, tool_args).await,
                "chat_with_meetings" => {
                    execute_chat_with_meetings(&state.pool, tool_args, &state.app_data_dir, &state.client).await
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
        let app_data_dir = app.path().app_data_dir().ok();
        tauri::async_runtime::spawn(async move {
            start_server(pool, app_data_dir, None).await;
        });
    } else {
        tracing::warn!("AppState not available, MCP server not started");
    }
}

pub async fn start_server(pool: SqlitePool, app_data_dir: Option<std::path::PathBuf>, port: Option<u16>) {
    let port = port.unwrap_or(DEFAULT_PORT);
    let state = McpState {
        pool,
        app_data_dir,
        client: reqwest::Client::new(),
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
}
