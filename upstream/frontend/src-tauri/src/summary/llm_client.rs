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
            None,
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
        None,
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

/// Hard generation bounds for a single Deep planner call. Every provider
/// records support for these in [`planner_generation_capability`]; a provider
/// that cannot enforce a required bound is never called and the Deep loop
/// falls back to current Fast evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedGeneration {
    pub max_output_tokens: u32,
    pub max_response_bytes: usize,
}

/// One provider's recorded ability to enforce the required planner bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannerCapability {
    /// The provider request carries the output-token cap.
    pub output_limit: bool,
    /// The shared client enforces a hard response-byte/parser cap.
    pub response_byte_cap: bool,
    /// A scoped child cancellation token aborts in-flight generation.
    pub child_cancellation: bool,
}

impl PlannerCapability {
    pub const fn full() -> Self {
        Self {
            output_limit: true,
            response_byte_cap: true,
            child_cancellation: true,
        }
    }

    pub const fn enforces_all_bounds(&self) -> bool {
        self.output_limit && self.response_byte_cap && self.child_cancellation
    }
}

/// Capability/fallback matrix for every configured Chat provider (Sprint 4
/// Deep planner). OpenAI, Claude, Groq, Ollama, OpenRouter, and Custom OpenAI
/// take a native output limit - via the `max_tokens` field of the shared
/// OpenAI-compatible request, Claude via its required `max_tokens` field - so
/// they record full support. BuiltInAI cannot actually enforce the requested
/// bounds: its sidecar carries only the token cap, the response-byte cap is
/// checked only after the full response has been read, and sidecar
/// startup/shutdown can outlive the deadline - so it records the truthful
/// unsupported status and is never called for the planner: Deep falls back to
/// current Fast evidence without sidecar generation, while ordinary
/// non-planner BuiltInAI Chat is unaffected. The record is the single seam
/// where a provider's missing support is declared.
pub fn planner_generation_capability(provider: &LLMProvider) -> PlannerCapability {
    match provider {
        LLMProvider::OpenAI
        | LLMProvider::Claude
        | LLMProvider::Groq
        | LLMProvider::Ollama
        | LLMProvider::OpenRouter
        | LLMProvider::CustomOpenAI => PlannerCapability::full(),
        LLMProvider::BuiltInAI => PlannerCapability {
            output_limit: true,
            response_byte_cap: false,
            child_cancellation: false,
        },
    }
}

/// Generates one bounded, non-streaming completion. Used by the Deep planner:
/// the output limit is sent to the provider where supported, the response
/// body is hard-capped mid-read, the whole call is bounded by `deadline`,
/// and `cancellation_token` aborts generation in flight. Returns an error
/// (never a truncated result) when any bound is exceeded.
pub async fn generate_bounded(
    client: &Client,
    provider: &LLMProvider,
    model_name: &str,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    ollama_endpoint: Option<&str>,
    custom_openai_endpoint: Option<&str>,
    app_data_dir: Option<&PathBuf>,
    bounds: &BoundedGeneration,
    deadline: Duration,
    cancellation_token: &CancellationToken,
) -> Result<String, String> {
    if cancellation_token.is_cancelled() {
        return Err("Bounded generation was cancelled".to_string());
    }
    if !planner_generation_capability(provider).enforces_all_bounds() {
        return Err("Provider cannot enforce the required planner generation bounds".to_string());
    }

    if provider == &LLMProvider::BuiltInAI {
        // Currently unreachable from the Deep planner: the capability matrix
        // above records BuiltInAI as unable to enforce the planner bounds, so
        // the check refuses the call before this branch. The branch stays as
        // the seam for a future BuiltInAI caller that enforces the bounds for
        // real; ordinary non-planner BuiltInAI Chat does not pass here.
        let app_data_dir = app_data_dir
            .ok_or_else(|| "app_data_dir is required for BuiltInAI provider".to_string())?;
        // The deadline is carried by `cancellation_token`: the Deep agent's
        // watchdog cancels it at the per-call/total deadline while this
        // future is still alive, and `generate_with_builtin` races the
        // sidecar request against it and shuts the sidecar down.
        let text = crate::summary::summary_engine::generate_with_builtin(
            app_data_dir,
            model_name,
            system_prompt,
            user_prompt,
            Some(cancellation_token),
            Some(bounds.max_output_tokens),
        )
        .await
        .map_err(|e| e.to_string())?;
        if text.len() > bounds.max_response_bytes {
            return Err(format!(
                "Bounded generation output exceeded the {} byte cap",
                bounds.max_response_bytes
            ));
        }
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
        None,
        Some(bounds.max_output_tokens),
        None,
        None,
        None,
    )?;

    info!(
        "🐞 Bounded LLM request to {}: model={} cap={}B/{}t deadline={}s",
        provider_name(provider),
        model_name,
        bounds.max_response_bytes,
        bounds.max_output_tokens,
        deadline.as_secs()
    );

    let request_future = client
        .post(api_url)
        .headers(headers)
        .json(&request_body)
        .timeout(deadline)
        .send();

    let response = tokio::select! {
        result = request_future => {
            result.map_err(|e| {
                if e.is_timeout() {
                    format!("Bounded LLM request timed out after {} seconds", deadline.as_secs())
                } else {
                    format!("Failed to send request to LLM: {}", e)
                }
            })?
        }
        _ = cancellation_token.cancelled() => {
            return Err("Bounded generation was cancelled".to_string());
        }
    };

    if !response.status().is_success() {
        // ponytail: error bodies are truncated before inclusion so a large
        // provider error page cannot bypass the response cap.
        let error_body = capped_body(response, bounds.max_response_bytes, cancellation_token)
            .await
            .unwrap_or_else(|_| b"Unknown error".to_vec());
        return Err(format!(
            "LLM API request failed: {}",
            String::from_utf8_lossy(&error_body)
        ));
    }

    let body = capped_body(response, bounds.max_response_bytes, cancellation_token).await?;
    let parsed: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

    if provider == &LLMProvider::Claude {
        let content = parsed
            .get("content")
            .and_then(|content| content.get(0))
            .and_then(|entry| entry.get("text"))
            .and_then(|text| text.as_str())
            .ok_or("No content in LLM response")?;
        Ok(content.trim().to_string())
    } else {
        let content = parsed
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_str())
            .ok_or("No content in LLM response")?;
        Ok(content.trim().to_string())
    }
}

/// Reads the response body while enforcing a hard byte cap mid-stream, so an
/// oversized response is rejected before it is ever parsed, and selecting on
/// the cancellation token, so an in-flight body read aborts immediately.
async fn capped_body(
    response: reqwest::Response,
    max_response_bytes: usize,
    cancellation_token: &CancellationToken,
) -> Result<Vec<u8>, String> {
    use futures_util::StreamExt;

    let mut stream = response.bytes_stream();
    let mut body: Vec<u8> = Vec::new();
    loop {
        let chunk = tokio::select! {
            chunk = stream.next() => chunk
                .transpose()
                .map_err(|e| format!("Failed to read LLM response: {}", e))?,
            _ = cancellation_token.cancelled() => {
                return Err("Bounded generation was cancelled".to_string());
            }
        };
        let Some(chunk) = chunk else {
            return Ok(body);
        };
        if body.len() + chunk.len() > max_response_bytes {
            return Err(format!(
                "Bounded generation output exceeded the {} byte cap",
                max_response_bytes
            ));
        }
        body.extend_from_slice(&chunk);
    }
}

/// Builds the URL, headers, and JSON body shared by streaming and
/// non-streaming chat requests. The only caller-controlled difference is the
/// `stream` field injected into the request body. `bounded_max_tokens` is the
/// Deep planner's hard output cap: when present it overrides every provider's
/// output limit (including the Custom OpenAI user preference and Claude's
/// default 2048).
#[allow(clippy::too_many_arguments)]
fn build_chat_request(
    provider: &LLMProvider,
    model_name: &str,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    ollama_endpoint: Option<&str>,
    custom_openai_endpoint: Option<&str>,
    max_tokens: Option<u32>,
    bounded_max_tokens: Option<u32>,
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
            (bounded_max_tokens.or(max_tokens), temperature, top_p)
        } else {
            (bounded_max_tokens, None, None)
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
            max_tokens: bounded_max_tokens.unwrap_or(2048),
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
        None,
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
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const PLANNER_BOUNDS: BoundedGeneration = BoundedGeneration {
        max_output_tokens: 512,
        max_response_bytes: 8 * 1024,
    };

    fn openai_compatible_response(content: &str) -> String {
        let body = serde_json::json!({
            "choices": [{ "message": { "content": content } }]
        })
        .to_string();
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    /// Serves exactly one pre-encoded HTTP response on a loopback port and
    /// returns the endpoint URL. The first read captures the request bytes so
    /// tests can assert on the outgoing body.
    async fn serve_once(response: &[u8]) -> (String, Arc<std::sync::Mutex<Vec<u8>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let response = response.to_vec();
        let request_bytes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = Arc::clone(&request_bytes);
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0u8; 64 * 1024];
            if let Ok(read) = socket.read(&mut buffer).await {
                *captured.lock().unwrap() = buffer[..read].to_vec();
            }
            let _ = socket.write_all(&response).await;
            let _ = socket.flush().await;
        });
        (format!("http://{}", addr), request_bytes)
    }

    #[test]
    fn planner_capability_matrix_reflects_actual_provider_bounds() {
        let full = [
            LLMProvider::OpenAI,
            LLMProvider::Claude,
            LLMProvider::Groq,
            LLMProvider::Ollama,
            LLMProvider::OpenRouter,
            LLMProvider::CustomOpenAI,
        ];
        for provider in full {
            assert!(
                planner_generation_capability(&provider).enforces_all_bounds(),
                "{provider:?} must enforce the planner output limit, byte cap, and child cancellation"
            );
        }
        // BuiltInAI cannot actually enforce the requested bounds: the sidecar
        // carries only the token cap, the byte cap is checked after the full
        // response has been read, and sidecar startup/shutdown can outlive
        // the deadline. The matrix must record that truthfully so Deep falls
        // back to Fast evidence instead of calling the sidecar.
        let builtin = planner_generation_capability(&LLMProvider::BuiltInAI);
        assert!(builtin.output_limit);
        assert!(!builtin.response_byte_cap);
        assert!(!builtin.child_cancellation);
        assert!(!builtin.enforces_all_bounds());
    }

    #[tokio::test]
    async fn builtin_ai_planner_generation_fails_closed_before_sidecar_startup() {
        // The matrix marks BuiltInAI unsupported for the Deep planner: with a
        // live token the call must fail closed BEFORE the sidecar manager is
        // ever touched (no spawn, no model load, no sidecar generation).
        let result = generate_bounded(
            &Client::new(),
            &LLMProvider::BuiltInAI,
            "gemma3:1b",
            "",
            "system",
            "user",
            None,
            None,
            Some(&PathBuf::from("unused-app-data")),
            &PLANNER_BOUNDS,
            Duration::from_secs(5),
            &CancellationToken::new(),
        )
        .await;
        let error = result.unwrap_err();
        assert!(
            error.contains("cannot enforce"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn bounded_max_tokens_overrides_every_provider_output_limit() {
        let body_for = |provider: &LLMProvider, bounded: Option<u32>| {
            let (_, _, body) = build_chat_request(
                provider,
                "model",
                "key",
                "system",
                "user",
                Some("http://ollama.local"),
                Some("http://custom.local"),
                Some(4_096),
                bounded,
                None,
                None,
                None,
            )
            .unwrap();
            body
        };

        let providers = [
            LLMProvider::OpenAI,
            LLMProvider::Groq,
            LLMProvider::OpenRouter,
            LLMProvider::Ollama,
        ];
        for provider in providers {
            let body = body_for(&provider, Some(512));
            assert_eq!(body["max_tokens"], serde_json::json!(512));
        }
        let custom = body_for(&LLMProvider::CustomOpenAI, Some(512));
        assert_eq!(custom["max_tokens"], serde_json::json!(512));
        let claude = body_for(&LLMProvider::Claude, Some(512));
        assert_eq!(claude["max_tokens"], serde_json::json!(512));

        // Without the planner bound the existing defaults are unchanged.
        let openai_default = body_for(&LLMProvider::OpenAI, None);
        assert!(openai_default.get("max_tokens").is_none());
        let claude_default = body_for(&LLMProvider::Claude, None);
        assert_eq!(claude_default["max_tokens"], serde_json::json!(2048));
        let custom_default = body_for(&LLMProvider::CustomOpenAI, None);
        assert_eq!(custom_default["max_tokens"], serde_json::json!(4_096));
    }

    #[tokio::test]
    async fn generate_bounded_caps_the_response_body_before_parsing() {
        let oversized = "x".repeat(20 * 1024);
        let (endpoint, _) = serve_once(openai_compatible_response(&oversized).as_bytes()).await;
        let result = generate_bounded(
            &Client::new(),
            &LLMProvider::Ollama,
            "local",
            "",
            "system",
            "user",
            Some(endpoint.as_str()),
            None,
            None,
            &PLANNER_BOUNDS,
            Duration::from_secs(5),
            &CancellationToken::new(),
        )
        .await;
        let error = result.unwrap_err();
        assert!(error.contains("byte cap"), "unexpected error: {error}");
    }

    #[tokio::test]
    async fn generate_bounded_enforces_the_call_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            // Accept, hold the socket, and never respond: the request can
            // only end via deadline.
            let Ok((_socket, _)) = listener.accept().await else {
                return;
            };
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let started = std::time::Instant::now();
        let result = generate_bounded(
            &Client::new(),
            &LLMProvider::Ollama,
            "local",
            "",
            "system",
            "user",
            Some(format!("http://{}", addr).as_str()),
            None,
            None,
            &PLANNER_BOUNDS,
            Duration::from_millis(300),
            &CancellationToken::new(),
        )
        .await;
        let error = result.unwrap_err();
        assert!(error.contains("timed out"), "unexpected error: {error}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// Serves headers plus a partial JSON body and then stalls forever, so an
    /// in-flight body read can only end through cancellation.
    async fn serve_stalled_body(headers_sent: &[u8], partial_body: &[u8]) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let headers_sent = headers_sent.to_vec();
        let partial_body = partial_body.to_vec();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let _ = socket.write_all(&headers_sent).await;
            let _ = socket.write_all(&partial_body).await;
            let _ = socket.flush().await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        format!("http://{}", addr)
    }

    #[tokio::test]
    async fn generate_bounded_cancels_an_in_flight_response_body() {
        let endpoint = serve_stalled_body(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
            b"{\"choices\":[{\"message\":{\"content\":\"partial",
        )
        .await;
        let token = CancellationToken::new();
        let cancel_task = {
            let token = token.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(150)).await;
                token.cancel();
            })
        };
        let started = std::time::Instant::now();
        let result = generate_bounded(
            &Client::new(),
            &LLMProvider::Ollama,
            "local",
            "",
            "system",
            "user",
            Some(endpoint.as_str()),
            None,
            None,
            &PLANNER_BOUNDS,
            Duration::from_secs(20),
            &token,
        )
        .await;
        cancel_task.abort();
        let error = result.unwrap_err();
        assert!(error.contains("cancelled"), "unexpected error: {error}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cancellation must abort the stalled body read promptly"
        );
    }

    #[tokio::test]
    async fn generate_bounded_sends_the_output_limit_and_cancellation() {
        let (endpoint, request_bytes) =
            serve_once(openai_compatible_response("{\"schemaVersion\":1}").as_bytes()).await;
        let token = CancellationToken::new();
        let result = generate_bounded(
            &Client::new(),
            &LLMProvider::Ollama,
            "local",
            "",
            "system",
            "user",
            Some(endpoint.as_str()),
            None,
            None,
            &PLANNER_BOUNDS,
            Duration::from_secs(5),
            &token,
        )
        .await
        .unwrap();
        assert_eq!(result, "{\"schemaVersion\":1}");
        let request = String::from_utf8(request_bytes.lock().unwrap().clone()).unwrap();
        assert!(request.contains("\"max_tokens\":512"), "request: {request}");

        token.cancel();
        let cancelled = generate_bounded(
            &Client::new(),
            &LLMProvider::Ollama,
            "local",
            "",
            "system",
            "user",
            Some(endpoint.as_str()),
            None,
            None,
            &PLANNER_BOUNDS,
            Duration::from_secs(5),
            &token,
        )
        .await
        .unwrap_err();
        assert!(cancelled.contains("cancelled"));
    }

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
    }

    #[tokio::test]
    async fn builtin_ai_generation_observes_cancellation_before_sidecar_startup() {
        // A cancelled deadline must abort before the sidecar manager is ever
        // touched, so no model load or sidecar spawn starts for a dead call.
        let token = CancellationToken::new();
        token.cancel();
        let result = generate_bounded(
            &Client::new(),
            &LLMProvider::BuiltInAI,
            "gemma3:1b",
            "",
            "system",
            "user",
            None,
            None,
            Some(&PathBuf::from("unused-app-data")),
            &PLANNER_BOUNDS,
            Duration::from_secs(5),
            &token,
        )
        .await;
        let error = result.unwrap_err();
        assert!(error.contains("cancelled"), "unexpected error: {error}");
    }
}
