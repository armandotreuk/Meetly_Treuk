use futures_util::StreamExt;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::info;

const REQUEST_TIMEOUT_DURATION: Duration = Duration::from_secs(300);

// Generic structure for OpenAI-compatible API chat messages
#[derive(Debug, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

// Generic structure for OpenAI-compatible API chat requests
#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

// Generic structure for OpenAI-compatible API chat responses
#[derive(Deserialize, Debug)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
pub struct Choice {
    pub message: MessageContent,
}

#[derive(Deserialize, Debug)]
pub struct MessageContent {
    pub content: String,
}

// Claude-specific request structure
#[derive(Debug, Serialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub max_tokens: u32,
    pub system: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

// Claude-specific response structure
#[derive(Deserialize, Debug)]
pub struct ClaudeChatResponse {
    pub content: Vec<ClaudeChatContent>,
}

#[derive(Deserialize, Debug)]
pub struct ClaudeChatContent {
    pub text: String,
}

/// LLM Provider enumeration for multi-provider support
#[derive(Debug, Clone, PartialEq)]
pub enum LLMProvider {
    OpenAI,
    Claude,
    Groq,
    Ollama,
    OpenRouter,
    BuiltInAI,
    CustomOpenAI,
}

impl LLMProvider {
    /// Parse provider from string (case-insensitive)
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Self::OpenAI),
            "claude" => Ok(Self::Claude),
            "groq" => Ok(Self::Groq),
            "ollama" => Ok(Self::Ollama),
            "openrouter" => Ok(Self::OpenRouter),
            "builtin-ai" | "local-llama" | "localllama" => Ok(Self::BuiltInAI),
            "custom-openai" => Ok(Self::CustomOpenAI),
            _ => Err(format!("Unsupported LLM provider: {}", s)),
        }
    }
}

/// Generates a summary using the specified LLM provider
pub async fn generate_summary(
    client: &Client,
    provider: &LLMProvider,
    model_name: &str,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    ollama_endpoint: Option<&str>,
    custom_openai_endpoint: Option<&str>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    app_data_dir: Option<&PathBuf>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<String, String> {
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Err("Summary generation was cancelled".to_string());
        }
    }

    if provider == &LLMProvider::BuiltInAI {
        let app_data_dir = app_data_dir
            .ok_or_else(|| "app_data_dir is required for BuiltInAI provider".to_string())?;
        return crate::summary::summary_engine::generate_with_builtin(
            app_data_dir,
            model_name,
            system_prompt,
            user_prompt,
            cancellation_token,
        )
        .await
        .map_err(|e| e.to_string());
    }

    let (api_url, headers, request_body) = build_chat_request(
        provider,
        model_name,
        api_key,
        system_prompt,
        user_prompt,
        ollama_endpoint,
        custom_openai_endpoint,
        max_tokens,
        temperature,
        top_p,
        None,
    )?;

    info!(
        "🐞 LLM Request to {}: model={}",
        provider_name(provider),
        model_name
    );

    let request_future = client
        .post(api_url)
        .headers(headers)
        .json(&request_body)
        .timeout(REQUEST_TIMEOUT_DURATION)
        .send();

    let response = if let Some(token) = cancellation_token {
        tokio::select! {
            result = request_future => {
                result.map_err(|e| {
                    if e.is_timeout() {
                        format!(
                            "LLM request timed out after {} seconds",
                            REQUEST_TIMEOUT_DURATION.as_secs()
                        )
                    } else {
                        format!("Failed to send request to LLM: {}", e)
                    }
                })?
            }
            _ = token.cancelled() => {
                return Err("Summary generation was cancelled".to_string());
            }
        }
    } else {
        request_future.await.map_err(|e| {
            if e.is_timeout() {
                format!(
                    "LLM request timed out after {} seconds",
                    REQUEST_TIMEOUT_DURATION.as_secs()
                )
            } else {
                format!("Failed to send request to LLM: {}", e)
            }
        })?
    };

    if !response.status().is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("LLM API request failed: {}", error_body));
    }

    if provider == &LLMProvider::Claude {
        let chat_response = response
            .json::<ClaudeChatResponse>()
            .await
            .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

        info!("🐞 LLM Response received from Claude");

        let content = chat_response
            .content
            .get(0)
            .ok_or("No content in LLM response")?
            .text
            .trim();
        Ok(content.to_string())
    } else {
        let chat_response = response
            .json::<ChatResponse>()
            .await
            .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

        info!("🐞 LLM Response received from {}", provider_name(provider));

        let content = chat_response
            .choices
            .get(0)
            .ok_or("No content in LLM response")?
            .message
            .content
            .trim();
        Ok(content.to_string())
    }
}

/// Builds the URL, headers, and JSON body shared by streaming and
/// non-streaming chat requests. The only caller-controlled difference is the
/// `stream` field injected into the request body.
fn build_chat_request(
    provider: &LLMProvider,
    model_name: &str,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    ollama_endpoint: Option<&str>,
    custom_openai_endpoint: Option<&str>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    stream: Option<bool>,
) -> Result<(String, header::HeaderMap, serde_json::Value), String> {
    let (api_url, mut headers) = match provider {
        LLMProvider::OpenAI => (
            "https://api.openai.com/v1/chat/completions".to_string(),
            header::HeaderMap::new(),
        ),
        LLMProvider::Groq => (
            "https://api.groq.com/openai/v1/chat/completions".to_string(),
            header::HeaderMap::new(),
        ),
        LLMProvider::OpenRouter => (
            "https://openrouter.ai/api/v1/chat/completions".to_string(),
            header::HeaderMap::new(),
        ),
        LLMProvider::Ollama => {
            let host = ollama_endpoint
                .map(|s| s.to_string())
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            (
                format!("{}/v1/chat/completions", host),
                header::HeaderMap::new(),
            )
        }
        LLMProvider::CustomOpenAI => {
            let endpoint = custom_openai_endpoint
                .ok_or_else(|| "Custom OpenAI endpoint not configured".to_string())?;
            (
                format!("{}/chat/completions", endpoint.trim_end_matches('/')),
                header::HeaderMap::new(),
            )
        }
        LLMProvider::Claude => {
            let mut header_map = header::HeaderMap::new();
            header_map.insert(
                "x-api-key",
                api_key
                    .parse()
                    .map_err(|_| "Invalid API key format".to_string())?,
            );
            header_map.insert(
                "anthropic-version",
                "2023-06-01"
                    .parse()
                    .map_err(|_| "Invalid anthropic version".to_string())?,
            );
            (
                "https://api.anthropic.com/v1/messages".to_string(),
                header_map,
            )
        }
        LLMProvider::BuiltInAI => {
            unreachable!("BuiltInAI is handled before this match statement")
        }
    };

    if provider != &LLMProvider::Claude {
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", api_key)
                .parse()
                .map_err(|_| "Invalid authorization header".to_string())?,
        );
    }
    headers.insert(
        header::CONTENT_TYPE,
        "application/json"
            .parse()
            .map_err(|_| "Invalid content type".to_string())?,
    );

    let request_body = if provider != &LLMProvider::Claude {
        let (max_tokens_val, temperature_val, top_p_val) = if provider == &LLMProvider::CustomOpenAI
        {
            (max_tokens, temperature, top_p)
        } else {
            (None, None, None)
        };

        serde_json::json!(ChatRequest {
            model: model_name.to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                }
            ],
            max_tokens: max_tokens_val,
            temperature: temperature_val,
            top_p: top_p_val,
            stream,
        })
    } else {
        serde_json::json!(ClaudeRequest {
            system: system_prompt.to_string(),
            model: model_name.to_string(),
            max_tokens: 2048,
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            }],
            stream,
        })
    };

    Ok((api_url, headers, request_body))
}

/// Decoded SSE event classification.
#[derive(Debug, PartialEq)]
enum SseEvent {
    Delta(String),
    Done,
    Ignore,
}

/// Parses a single SSE `data:` payload into a decoded event.
fn parse_sse_line(provider: &LLMProvider, data: &str) -> SseEvent {
    if provider == &LLMProvider::Claude {
        let parsed: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return SseEvent::Ignore,
        };

        match parsed.get("type").and_then(|t| t.as_str()) {
            Some("message_stop") => return SseEvent::Done,
            Some("content_block_delta") => {}
            _ => return SseEvent::Ignore,
        }

        let delta = match parsed.get("delta") {
            Some(d) => d,
            None => return SseEvent::Ignore,
        };

        if delta.get("type").and_then(|t| t.as_str()) != Some("text_delta") {
            return SseEvent::Ignore;
        }

        match delta.get("text").and_then(|t| t.as_str()) {
            Some(text) => SseEvent::Delta(text.to_string()),
            None => SseEvent::Ignore,
        }
    } else {
        let trimmed = data.trim();
        if trimmed == "[DONE]" {
            return SseEvent::Done;
        }

        let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => return SseEvent::Ignore,
        };

        match parsed
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c0| c0.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
        {
            Some(text) => SseEvent::Delta(text.to_string()),
            None => SseEvent::Ignore,
        }
    }
}

/// Byte buffer that reassembles HTTP chunks into complete SSE lines before
/// converting to UTF-8, so a multi-byte codepoint split across chunks is not
/// corrupted by lossy per-chunk decoding.
struct SseLineBuffer {
    bytes: Vec<u8>,
}

impl SseLineBuffer {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
    }

    /// Returns the next complete line (without the trailing newline) once a
    /// `\n` byte is present. Handles both `\n` and `\r\n` line endings.
    fn next_line(&mut self) -> Option<String> {
        let newline_pos = self.bytes.iter().position(|&b| b == b'\n')?;
        let remaining = self.bytes.split_off(newline_pos + 1);
        let mut line_bytes = std::mem::replace(&mut self.bytes, remaining);
        line_bytes.pop(); // remove '\n'
        if line_bytes.last() == Some(&b'\r') {
            line_bytes.pop();
        }
        Some(String::from_utf8_lossy(&line_bytes).into_owned())
    }
}

/// Strips the SSE `data:` prefix with an optional single space, per the SSE
/// spec (`data:` and `data: ` are both valid). Returns `None` for non-data lines.
fn sse_data_payload(line: &str) -> Option<&str> {
    line.strip_prefix("data:")
        .map(|data| data.strip_prefix(' ').unwrap_or(data))
}

/// Generates a summary in streaming mode, invoking `on_chunk` for each
/// content delta received from the LLM. Returns the full accumulated text.
pub async fn generate_summary_stream<F>(
    client: &Client,
    provider: &LLMProvider,
    model_name: &str,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    ollama_endpoint: Option<&str>,
    custom_openai_endpoint: Option<&str>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    app_data_dir: Option<&PathBuf>,
    cancellation_token: Option<&CancellationToken>,
    mut on_chunk: F,
) -> Result<String, String>
where
    F: FnMut(&str) + Send,
{
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Err("Summary generation was cancelled".to_string());
        }
    }

    // ponytail: BuiltInAI uses the sidecar's JSON-RPC protocol, which today
    // does not expose token-by-token streaming. Fall back to the non-streaming
    // path and emit the full answer as one chunk. Upgrade path: extend the
    // sidecar protocol with streaming events.
    if provider == &LLMProvider::BuiltInAI {
        let text = generate_summary(
            client,
            provider,
            model_name,
            api_key,
            system_prompt,
            user_prompt,
            ollama_endpoint,
            custom_openai_endpoint,
            max_tokens,
            temperature,
            top_p,
            app_data_dir,
            cancellation_token,
        )
        .await?;
        on_chunk(&text);
        return Ok(text);
    }

    let (api_url, headers, request_body) = build_chat_request(
        provider,
        model_name,
        api_key,
        system_prompt,
        user_prompt,
        ollama_endpoint,
        custom_openai_endpoint,
        max_tokens,
        temperature,
        top_p,
        Some(true),
    )?;

    info!(
        "🐞 LLM streaming request to {}: model={}",
        provider_name(provider),
        model_name
    );

    let request_future = client
        .post(api_url)
        .headers(headers)
        .json(&request_body)
        .timeout(REQUEST_TIMEOUT_DURATION)
        .send();

    let response = if let Some(token) = cancellation_token {
        tokio::select! {
            result = request_future => {
                result.map_err(|e| {
                    if e.is_timeout() {
                        format!(
                            "LLM request timed out after {} seconds",
                            REQUEST_TIMEOUT_DURATION.as_secs()
                        )
                    } else {
                        format!("Failed to send request to LLM: {}", e)
                    }
                })?
            }
            _ = token.cancelled() => {
                return Err("Summary generation was cancelled".to_string());
            }
        }
    } else {
        request_future.await.map_err(|e| {
            if e.is_timeout() {
                format!(
                    "LLM request timed out after {} seconds",
                    REQUEST_TIMEOUT_DURATION.as_secs()
                )
            } else {
                format!("Failed to send request to LLM: {}", e)
            }
        })?
    };

    if !response.status().is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("LLM API request failed: {}", error_body));
    }

    let mut stream = response.bytes_stream();
    let mut full_text = String::new();
    let mut line_buffer = SseLineBuffer::new();

    loop {
        let chunk_result = if let Some(token) = cancellation_token {
            tokio::select! {
                result = stream.next() => result,
                _ = token.cancelled() => {
                    return Err("Summary generation was cancelled".to_string());
                }
            }
        } else {
            stream.next().await
        };

        match chunk_result {
            Some(Ok(bytes)) => {
                line_buffer.push(&bytes);
                while let Some(line) = line_buffer.next_line() {
                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }
                    if let Some(data) = sse_data_payload(&line) {
                        match parse_sse_line(provider, data) {
                            SseEvent::Delta(delta) => {
                                on_chunk(&delta);
                                full_text.push_str(&delta);
                            }
                            SseEvent::Done => {
                                info!(
                                    "🐞 LLM streaming response completed from {}: {} chars",
                                    provider_name(provider),
                                    full_text.len()
                                );
                                return Ok(full_text);
                            }
                            SseEvent::Ignore => {}
                        }
                    }
                }
            }
            Some(Err(e)) => {
                return Err(format!("Stream error: {}", e));
            }
            None => {
                info!(
                    "🐞 LLM streaming response completed from {}: {} chars",
                    provider_name(provider),
                    full_text.len()
                );
                return Ok(full_text);
            }
        }
    }
}

/// Helper function to get provider name for logging
fn provider_name(provider: &LLMProvider) -> &str {
    match provider {
        LLMProvider::OpenAI => "OpenAI",
        LLMProvider::Claude => "Claude",
        LLMProvider::Groq => "Groq",
        LLMProvider::Ollama => "Ollama",
        LLMProvider::BuiltInAI => "Built-in AI",
        LLMProvider::OpenRouter => "OpenRouter",
        LLMProvider::CustomOpenAI => "Custom OpenAI",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_line_openai_compatible_delta() {
        let data = r#"{"choices":[{"delta":{"content":"hello"}}]}"#;
        assert_eq!(
            parse_sse_line(&LLMProvider::OpenAI, data),
            SseEvent::Delta("hello".to_string())
        );
        assert_eq!(
            parse_sse_line(&LLMProvider::Groq, data),
            SseEvent::Delta("hello".to_string())
        );
        assert_eq!(
            parse_sse_line(&LLMProvider::Ollama, data),
            SseEvent::Delta("hello".to_string())
        );
    }

    #[test]
    fn parse_sse_line_done_is_terminal() {
        assert_eq!(
            parse_sse_line(&LLMProvider::OpenAI, "[DONE]"),
            SseEvent::Done
        );
        assert_eq!(
            parse_sse_line(&LLMProvider::OpenAI, "  [DONE]  "),
            SseEvent::Done
        );
    }

    #[test]
    fn parse_sse_line_empty_openai_delta_is_ignore() {
        let data = r#"{"choices":[{"delta":{}}]}"#;
        assert_eq!(parse_sse_line(&LLMProvider::OpenAI, data), SseEvent::Ignore);
    }

    #[test]
    fn parse_sse_line_claude_text_delta() {
        let data = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"world"}}"#;
        assert_eq!(
            parse_sse_line(&LLMProvider::Claude, data),
            SseEvent::Delta("world".to_string())
        );
    }

    #[test]
    fn parse_sse_line_claude_non_text_delta_is_ignore() {
        let data = r#"{"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{}"}}"#;
        assert_eq!(parse_sse_line(&LLMProvider::Claude, data), SseEvent::Ignore);
    }

    #[test]
    fn parse_sse_line_claude_message_stop_is_done() {
        let data = r#"{"type":"message_stop"}"#;
        assert_eq!(parse_sse_line(&LLMProvider::Claude, data), SseEvent::Done);
    }

    #[test]
    fn parse_sse_line_claude_ping_is_ignore() {
        let data = r#"{"type":"ping"}"#;
        assert_eq!(parse_sse_line(&LLMProvider::Claude, data), SseEvent::Ignore);
    }

    #[test]
    fn sse_line_buffer_splits_lines_across_chunks() {
        let mut buf = SseLineBuffer::new();
        buf.push(b"data: hel");
        assert_eq!(buf.next_line(), None);
        buf.push(b"lo\ndata: world\n");
        assert_eq!(buf.next_line(), Some("data: hello".to_string()));
        assert_eq!(buf.next_line(), Some("data: world".to_string()));
        assert_eq!(buf.next_line(), None);
    }

    #[test]
    fn sse_line_buffer_handles_crlf() {
        let mut buf = SseLineBuffer::new();
        buf.push(b"data: hello\r\n: comment\r\n");
        assert_eq!(buf.next_line(), Some("data: hello".to_string()));
        assert_eq!(buf.next_line(), Some(": comment".to_string()));
    }

    #[test]
    fn sse_line_buffer_reassembles_split_utf8() {
        // "é" is encoded as [0xc3, 0xa9].
        let mut buf = SseLineBuffer::new();
        buf.push(b"data: \xc3");
        assert_eq!(buf.next_line(), None);
        buf.push(b"\xa9\n");
        assert_eq!(buf.next_line(), Some("data: é".to_string()));
    }

    #[test]
    fn sse_data_payload_with_space() {
        assert_eq!(sse_data_payload("data: hello"), Some("hello"));
    }

    #[test]
    fn sse_data_payload_without_space() {
        assert_eq!(sse_data_payload("data:hello"), Some("hello"));
    }

    #[test]
    fn sse_data_payload_non_data_line_returns_none() {
        assert_eq!(sse_data_payload(": comment"), None);
        assert_eq!(sse_data_payload("event: ping"), None);
    }
}
