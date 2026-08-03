use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use tracing::info;

use crate::{
    database::repositories::{fts::FtsRepository, setting::SettingsRepository},
    export::build_context_markdown,
    state::AppState,
    summary::llm_client::{generate_summary, LLMProvider},
};

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

#[derive(Debug, Serialize, Deserialize)]
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
}

const SYSTEM_PROMPT: &str = "You are a helpful meeting assistant. Answer the user's question based on the meeting context provided below. If the context doesn't contain enough information, say so. Be concise and cite specific meetings when relevant. Format your response in clear paragraphs.";

#[tauri::command]
pub async fn api_chat_with_meetings<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    query: String,
    history: Option<Vec<ChatMessage>>,
    auth_token: Option<String>,
) -> Result<ChatResponse, String> {
    info!(
        "api_chat_with_meetings called with query: '{}', history_len: {:?}, auth_token: {}",
        query,
        history.as_ref().map(|h| h.len()),
        auth_token.is_some()
    );

    let pool = state.db_manager.pool();

    // 1. FTS search for relevant content
    let results = FtsRepository::search(pool, &query, 10).await.map_err(|e| {
        tracing::error!("FTS search failed for chat: {}", e);
        format!("Search failed: {}", e)
    })?;

    // 2. Build context from search results
    let context = build_context_markdown(&results);

    // 3. Build source list for response
    let sources: Vec<ChatSource> = results
        .iter()
        .map(|r| ChatSource {
            meeting_id: r.meeting_id.clone(),
            meeting_title: r.meeting_title.clone(),
            chunk_type: r.chunk_type.clone(),
            snippet: r.snippet.clone(),
            folder_name: r.folder_name.clone(),
        })
        .collect();

    // 4. Get LLM config from database (chat-specific, falls back to summary config)
    let model_config = SettingsRepository::get_chat_model_config(pool)
        .await
        .map_err(|e| format!("Failed to get model config: {}", e))?
        .ok_or_else(|| "No model configured. Please set a model in Settings.".to_string())?;

    let (model_provider_str, model_name, chat_ollama_endpoint) =
        SettingsRepository::resolve_chat_config(&model_config);

    let provider = LLMProvider::from_str(&model_provider_str)?;

    // 5. Get CustomOpenAI config (has its own API key + endpoint)
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
                _ => return Err("Custom OpenAI provider selected but no configuration found".to_string()),
            }
        } else {
            (None, String::new(), None, None, None)
        };

    // 6. Get API key for cloud providers
    let api_key = if provider == LLMProvider::Ollama
        || provider == LLMProvider::BuiltInAI
    {
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

    // 7. Get Ollama endpoint (chat-specific, falls back to summary endpoint)
    let ollama_endpoint = if provider == LLMProvider::Ollama {
        chat_ollama_endpoint
    } else {
        None
    };

    // 8. Build user prompt with conversation history
    let mut user_prompt = String::new();

    if let Some(ref msgs) = history {
        for msg in msgs.iter().take(10) {
            user_prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }
    }

    user_prompt.push_str(&format!("\nUser question: {}\n\nMeeting context:\n{}", query, context));

    // 9. Get app data dir for BuiltInAI
    let app_data_dir = app.path().app_data_dir().ok();

    // 10. Call LLM
    let client = reqwest::Client::new();
    let answer = generate_summary(
        &client,
        &provider,
        &model_name,
        &api_key,
        SYSTEM_PROMPT,
        &user_prompt,
        ollama_endpoint.as_deref(),
        custom_openai_endpoint.as_deref(),
        custom_openai_max_tokens,
        custom_openai_temperature,
        custom_openai_top_p,
        app_data_dir.as_ref(),
        None, // no cancellation token for chat
    )
    .await
    .map_err(|e| {
        tracing::error!("LLM call failed for chat: {}", e);
        format!("LLM error: {}", e)
    })?;

    info!(
        "Chat completed: {} sources, {} answer chars",
        sources.len(),
        answer.len()
    );

    Ok(ChatResponse { answer, sources })
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
        "api_build_context called with query: '{}', limit: {:?}, auth_token: {}",
        query,
        limit,
        auth_token.is_some()
    );
    let pool = state.db_manager.pool();
    let results = FtsRepository::search(pool, &query, limit.unwrap_or(10))
        .await
        .map_err(|e| format!("Search failed: {}", e))?;
    Ok(build_context_markdown(&results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::fts::FtsSearchResult;

    fn make_search_result(meeting_id: &str, title: &str, chunk_type: &str, snippet: &str) -> FtsSearchResult {
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
    fn build_context_produces_sources_from_search_results() {
        let results = vec![
            make_search_result("m1", "Sprint Planning", "transcript", "We decided to ship FTS5."),
            make_search_result("m1", "Sprint Planning", "summary", "Summary of sprint."),
            make_search_result("m2", "Retro", "transcript", "Team velocity is improving."),
        ];

        let sources: Vec<ChatSource> = results
            .iter()
            .map(|r| ChatSource {
                meeting_id: r.meeting_id.clone(),
                meeting_title: r.meeting_title.clone(),
                chunk_type: r.chunk_type.clone(),
                snippet: r.snippet.clone(),
                folder_name: r.folder_name.clone(),
            })
            .collect();

        assert_eq!(sources.len(), 3);
        assert_eq!(sources[0].meeting_title, "Sprint Planning");
        assert_eq!(sources[1].chunk_type, "summary");
        assert_eq!(sources[2].meeting_id, "m2");
    }

    #[test]
    fn user_prompt_includes_history_and_query() {
        let history = vec![
            ChatMessage { role: "user".into(), content: "What did we discuss?".into() },
            ChatMessage { role: "assistant".into(), content: "You discussed FTS5.".into() },
        ];

        let context = "# Meeting Context\n\nSome context here.\n";
        let query = "What about next steps?";

        let mut user_prompt = String::new();
        for msg in history.iter().take(10) {
            user_prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }
        user_prompt.push_str(&format!("\nUser question: {}\n\nMeeting context:\n{}", query, context));

        assert!(user_prompt.contains("user: What did we discuss?"));
        assert!(user_prompt.contains("assistant: You discussed FTS5."));
        assert!(user_prompt.contains("What about next steps?"));
        assert!(user_prompt.contains("Some context here."));
    }

    #[test]
    fn chat_source_serialization() {
        let source = ChatSource {
            meeting_id: "m1".to_string(),
            meeting_title: "Planning".to_string(),
            chunk_type: "transcript".to_string(),
            snippet: "Hello world".to_string(),
            folder_name: "Alpha".to_string(),
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
            }],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("FTS5 first"));
        assert!(json.contains("\"sources\""));
    }
}
