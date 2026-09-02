use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

pub fn serialize_id_as_string<S>(id: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format!("db:{}", id))
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MeetingModel {
    pub id: String,
    pub title: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub folder_path: Option<String>,
    pub folder_id: Option<String>,
    // ponytail: only the meetings-list query produces this column; the other
    // MeetingModel queries select explicit columns, so #[sqlx(default)] maps the
    // missing column to false instead of a decode error.
    #[sqlx(default)]
    pub has_notes: bool,
}

/// Logical folder for grouping meetings (multi-level, in-DB only; disk layout is unaffected).
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MeetingFolderModel {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(transparent)]
pub struct DateTimeUtc(pub DateTime<Utc>);

impl From<NaiveDateTime> for DateTimeUtc {
    fn from(naive: NaiveDateTime) -> Self {
        DateTimeUtc(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
    }
}

// Renamed from TranscriptSegment to Transcript to match the table name
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Transcript {
    pub id: String,
    pub meeting_id: String,
    pub transcript: String,
    pub timestamp: String,
    pub summary: Option<String>,
    pub action_items: Option<String>,
    pub key_points: Option<String>,
    // Recording-relative timestamps for audio-transcript synchronization
    pub audio_start_time: Option<f64>,
    pub audio_end_time: Option<f64>,
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SummaryProcess {
    pub meeting_id: String,
    pub template_id: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub error: Option<String>,
    pub result: Option<String>, // JSON
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    pub chunk_count: i64,
    pub processing_time: f64,
    pub metadata: Option<String>,      // JSON
    pub result_backup: Option<String>, // Backup of result before regeneration
    pub result_backup_timestamp: Option<chrono::DateTime<chrono::Utc>>, // When backup was created
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TranscriptChunk {
    pub meeting_id: String,
    pub meeting_name: Option<String>,
    pub transcript_text: String,
    pub model: String,
    pub model_name: String,
    pub chunk_size: Option<i64>,
    pub overlap: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Setting {
    pub id: String,
    pub provider: String,
    pub model: String,
    #[sqlx(rename = "whisperModel")]
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[sqlx(rename = "groqApiKey")]
    #[serde(rename = "groqApiKey")]
    pub groq_api_key: Option<String>,
    #[sqlx(rename = "openaiApiKey")]
    #[serde(rename = "openaiApiKey")]
    pub openai_api_key: Option<String>,
    #[sqlx(rename = "anthropicApiKey")]
    #[serde(rename = "anthropicApiKey")]
    pub anthropic_api_key: Option<String>,
    #[sqlx(rename = "ollamaApiKey")]
    #[serde(rename = "ollamaApiKey")]
    pub ollama_api_key: Option<String>,
    #[sqlx(rename = "openRouterApiKey")]
    #[serde(rename = "openRouterApiKey")]
    pub open_router_api_key: Option<String>,
    #[sqlx(rename = "ollamaEndpoint")]
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
    /// Custom OpenAI-compatible endpoint configuration stored as JSON
    #[sqlx(rename = "customOpenAIConfig")]
    #[serde(rename = "customOpenAIConfig")]
    pub custom_openai_config: Option<String>,
    /// Global vocabulary list (newline-separated words/phrases) biasing Whisper + summary LLM
    #[sqlx(rename = "customVocabulary")]
    #[serde(rename = "customVocabulary")]
    pub custom_vocabulary: Option<String>,
    /// Chat with Meetings: separate LLM provider (falls back to summary provider if NULL)
    #[sqlx(rename = "chatProvider")]
    #[serde(rename = "chatProvider")]
    pub chat_provider: Option<String>,
    /// Chat with Meetings: separate model name (falls back to summary model if NULL)
    #[sqlx(rename = "chatModel")]
    #[serde(rename = "chatModel")]
    pub chat_model: Option<String>,
    /// Chat with Meetings: separate Ollama endpoint (falls back to summary endpoint if NULL)
    #[sqlx(rename = "chatOllamaEndpoint")]
    #[serde(rename = "chatOllamaEndpoint")]
    pub chat_ollama_endpoint: Option<String>,
    #[sqlx(rename = "force_lexical_retrieval")]
    #[serde(rename = "forceLexicalRetrieval")]
    #[sqlx(default)]
    pub force_lexical_retrieval: bool,
}

impl Setting {
    /// Parse the custom OpenAI config from JSON string
    pub fn get_custom_openai_config(&self) -> Option<crate::summary::CustomOpenAIConfig> {
        self.custom_openai_config
            .as_ref()
            .and_then(|json| serde_json::from_str(json).ok())
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TranscriptSetting {
    pub id: String,
    pub provider: String,
    pub model: String,
    #[sqlx(rename = "whisperApiKey")]
    #[serde(rename = "whisperApiKey")]
    pub whisper_api_key: Option<String>,
    #[sqlx(rename = "deepgramApiKey")]
    #[serde(rename = "deepgramApiKey")]
    pub deepgram_api_key: Option<String>,
    #[sqlx(rename = "elevenLabsApiKey")]
    #[serde(rename = "elevenLabsApiKey")]
    pub eleven_labs_api_key: Option<String>,
    #[sqlx(rename = "groqApiKey")]
    #[serde(rename = "groqApiKey")]
    pub groq_api_key: Option<String>,
    #[sqlx(rename = "openaiApiKey")]
    #[serde(rename = "openaiApiKey")]
    pub openai_api_key: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Template {
    #[serde(serialize_with = "serialize_id_as_string")]
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub stable_id: Option<String>,
    #[sqlx(rename = "schema_json")]
    #[serde(rename = "schema_json")]
    pub schema_json: String,
    #[sqlx(rename = "is_builtin")]
    #[serde(rename = "is_builtin")]
    pub is_builtin: i64,
    #[sqlx(rename = "created_at")]
    #[serde(rename = "created_at")]
    pub created_at: String,
    #[sqlx(rename = "updated_at")]
    #[serde(rename = "updated_at")]
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::Template;

    #[test]
    fn template_id_serializes_as_string() {
        let template = Template {
            id: 42,
            name: "Custom".to_string(),
            description: Some("Description".to_string()),
            stable_id: None,
            schema_json: "{}".to_string(),
            is_builtin: 0,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };

        let json = serde_json::to_value(template).expect("serialize template");
        assert_eq!(json["id"], "db:42");
    }
}
