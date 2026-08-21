use anyhow::{bail, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ChatConversation {
    pub id: String,
    pub meeting_id: Option<String>,
    pub origin: String,
    pub scope_kind: String,
    pub scope_key: String,
    pub scope_data: Option<String>,
    pub promoted_from_live_scope_key: Option<String>,
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatScopeKind {
    All,
    Meeting,
    Folder,
    SearchSnapshot,
    LiveRecording,
    OrphanedMeeting,
}

impl ChatScopeKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Meeting => "meeting",
            Self::Folder => "folder",
            Self::SearchSnapshot => "search_snapshot",
            Self::LiveRecording => "live_recording",
            Self::OrphanedMeeting => "orphaned_meeting",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub struct ChatScopeData {
    pub result_ids: Vec<String>,
}

const MAX_SEARCH_SNAPSHOT_RESULTS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChatScope {
    pub kind: ChatScopeKind,
    pub key: String,
    pub data: Option<ChatScopeData>,
}

impl ChatScope {
    pub fn validate(&self) -> Result<()> {
        if self.key.trim().is_empty() {
            bail!("Chat scope key must not be empty");
        }

        match (&self.kind, &self.data) {
            (ChatScopeKind::All, None) if self.key == "all" => Ok(()),
            (
                ChatScopeKind::Meeting | ChatScopeKind::Folder | ChatScopeKind::LiveRecording,
                None,
            ) => Ok(()),
            (ChatScopeKind::SearchSnapshot, Some(ChatScopeData { result_ids }))
                if result_ids.len() <= MAX_SEARCH_SNAPSHOT_RESULTS
                    && result_ids
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == result_ids.len()
                    && result_ids.iter().all(|id| {
                        !id.trim().is_empty()
                            && id.len() <= 512
                            && id.chars().all(|character| {
                                character.is_ascii_alphanumeric() || "_-:.".contains(character)
                            })
                    }) =>
            {
                Ok(())
            }
            (ChatScopeKind::OrphanedMeeting, _) => {
                bail!("Orphaned meeting scopes cannot be created")
            }
            _ => bail!("Invalid chat scope key or data"),
        }
    }

    fn lineage(&self) -> (Option<&str>, &'static str) {
        match self.kind {
            ChatScopeKind::All => (None, "global"),
            ChatScopeKind::Meeting => (Some(&self.key), "meeting"),
            ChatScopeKind::Folder => (None, "folder"),
            ChatScopeKind::SearchSnapshot => (None, "search_snapshot"),
            ChatScopeKind::LiveRecording => (None, "live_recording"),
            ChatScopeKind::OrphanedMeeting => (None, "meeting"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ChatMessageRow {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub sources_json: Option<String>,
    pub is_error: bool,
    pub created_at: String,
}

pub struct ChatRepository;

impl ChatRepository {
    pub async fn get_promoted_meeting_id(
        pool: &SqlitePool,
        live_scope_key: &str,
    ) -> Result<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT meeting_id FROM chat_conversations WHERE promoted_from_live_scope_key = $1",
        )
        .bind(live_scope_key)
        .fetch_optional(pool)
        .await?)
    }

    pub async fn create_conversation(
        pool: &SqlitePool,
        meeting_id: Option<&str>,
        title: Option<&str>,
        origin: &'static str,
    ) -> Result<String> {
        let scope = match (meeting_id, origin) {
            (Some(meeting_id), _) => ChatScope {
                kind: ChatScopeKind::Meeting,
                key: meeting_id.to_string(),
                data: None,
            },
            (None, "global") => ChatScope {
                kind: ChatScopeKind::All,
                key: "all".to_string(),
                data: None,
            },
            (None, _) => ChatScope {
                kind: ChatScopeKind::OrphanedMeeting,
                key: Uuid::new_v4().to_string(),
                data: None,
            },
        };
        Ok(Self::create_scoped_conversation(pool, &scope, title)
            .await?
            .id)
    }

    pub async fn get_or_create_scoped_conversation(
        pool: &SqlitePool,
        scope: &ChatScope,
        title: Option<&str>,
    ) -> Result<ChatConversation> {
        scope.validate()?;
        Self::upsert_scoped_conversation(pool, scope, title).await
    }

    pub async fn get_latest_conversation_for_scope(
        pool: &SqlitePool,
        scope: &ChatScope,
    ) -> Result<Option<ChatConversation>> {
        scope.validate()?;
        let scope_data = scope.data.as_ref().map(serde_json::to_string).transpose()?;
        Ok(sqlx::query_as::<_, ChatConversation>(
            "SELECT * FROM chat_conversations WHERE scope_kind = $1 AND scope_key = $2 AND scope_data IS $3 ORDER BY updated_at DESC, created_at DESC LIMIT 1",
        )
        .bind(scope.kind.as_str())
        .bind(&scope.key)
        .bind(&scope_data)
        .fetch_optional(pool)
        .await?)
    }

    async fn create_scoped_conversation(
        pool: &SqlitePool,
        scope: &ChatScope,
        title: Option<&str>,
    ) -> Result<ChatConversation> {
        Self::upsert_scoped_conversation(pool, scope, title).await
    }

    async fn upsert_scoped_conversation(
        pool: &SqlitePool,
        scope: &ChatScope,
        title: Option<&str>,
    ) -> Result<ChatConversation> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let scope_data = scope.data.as_ref().map(serde_json::to_string).transpose()?;
        let (meeting_id, origin) = scope.lineage();

        Ok(sqlx::query_as::<_, ChatConversation>(
            "INSERT INTO chat_conversations (id, meeting_id, title, origin, scope_kind, scope_key, scope_data, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8) ON CONFLICT(scope_kind, scope_key, COALESCE(scope_data, '')) DO UPDATE SET id = chat_conversations.id RETURNING *",
        )
        .bind(&id)
        .bind(meeting_id)
        .bind(title)
        .bind(origin)
        .bind(scope.kind.as_str())
        .bind(&scope.key)
        .bind(&scope_data)
        .bind(&now)
        .fetch_one(pool)
        .await?)
    }

    pub async fn get_latest_conversation(
        pool: &SqlitePool,
        meeting_id: Option<&str>,
    ) -> Result<Option<ChatConversation>> {
        let conversation = match meeting_id {
            Some(meeting_id) => sqlx::query_as::<_, ChatConversation>(
                "SELECT * FROM chat_conversations WHERE meeting_id = $1 ORDER BY updated_at DESC, created_at DESC LIMIT 1",
            )
            .bind(meeting_id)
            .fetch_optional(pool)
            .await?,
            None => sqlx::query_as::<_, ChatConversation>(
                "SELECT * FROM chat_conversations WHERE meeting_id IS NULL AND origin = 'global' ORDER BY updated_at DESC, created_at DESC LIMIT 1",
            )
            .fetch_optional(pool)
            .await?,
        };

        Ok(conversation)
    }

    pub async fn get_conversation(
        pool: &SqlitePool,
        conversation_id: &str,
    ) -> Result<Option<ChatConversation>> {
        Ok(
            sqlx::query_as::<_, ChatConversation>("SELECT * FROM chat_conversations WHERE id = $1")
                .bind(conversation_id)
                .fetch_optional(pool)
                .await?,
        )
    }

    // ponytail: list, rename, and delete-by-list UI are deferred; MVP only resumes and clears a thread.
    pub async fn get_conversations_by_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<ChatConversation>> {
        Ok(sqlx::query_as::<_, ChatConversation>(
            "SELECT * FROM chat_conversations WHERE meeting_id = $1 ORDER BY updated_at DESC, created_at DESC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?)
    }

    pub async fn save_message(
        pool: &SqlitePool,
        conversation_id: &str,
        role: &str,
        content: &str,
        sources_json: Option<&str>,
        is_error: bool,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let conversation_context: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT scope_kind, scope_key, promoted_from_live_scope_key, meetings.title FROM chat_conversations LEFT JOIN meetings ON meetings.id = chat_conversations.meeting_id WHERE chat_conversations.id = $1",
        )
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await?;
        let normalized_sources = match sources_json {
            Some(sources) => {
                normalize_sources_for_persistence(&mut tx, sources, conversation_context.as_ref())
                    .await?
            }
            None => None,
        };

        sqlx::query(
            "INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, is_error, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(conversation_id)
        .bind(role)
        .bind(content)
        .bind(normalized_sources.as_deref())
        .bind(is_error)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE chat_conversations SET title = CASE WHEN title IS NULL AND $2 = 'user' THEN substr($3, 1, 50) ELSE title END, updated_at = CASE WHEN updated_at < $4 THEN $4 ELSE updated_at END WHERE id = $1",
        )
        .bind(conversation_id)
        .bind(role)
        .bind(content)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    pub async fn get_messages(
        pool: &SqlitePool,
        conversation_id: &str,
    ) -> Result<Vec<ChatMessageRow>> {
        Ok(sqlx::query_as::<_, ChatMessageRow>(
            "SELECT * FROM chat_messages WHERE conversation_id = $1 ORDER BY created_at ASC, id ASC",
        )
        .bind(conversation_id)
        .fetch_all(pool)
        .await?)
    }

    pub async fn promote_live_recording(
        pool: &SqlitePool,
        live_scope_key: &str,
        meeting_id: &str,
    ) -> Result<Option<String>> {
        let mut tx = pool.begin().await?;
        let result =
            Self::promote_live_recording_in_transaction(&mut tx, live_scope_key, meeting_id)
                .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn promote_live_recording_in_transaction(
        connection: &mut SqliteConnection,
        live_scope_key: &str,
        meeting_id: &str,
    ) -> Result<Option<String>> {
        if let Some(id) = sqlx::query_scalar::<_, String>(
            "SELECT id FROM chat_conversations WHERE promoted_from_live_scope_key = $1",
        )
        .bind(live_scope_key)
        .fetch_optional(&mut *connection)
        .await?
        {
            if sqlx::query_scalar::<_, String>(
                "SELECT scope_key FROM chat_conversations WHERE id = $1",
            )
            .bind(&id)
            .fetch_one(&mut *connection)
            .await?
                != meeting_id
            {
                bail!("Live conversation was already promoted to another meeting");
            }
            return Ok(Some(id));
        }

        let conversations = sqlx::query_as::<_, ChatConversation>(
            "SELECT * FROM chat_conversations WHERE scope_kind = 'live_recording' AND scope_key = $1 ORDER BY updated_at DESC, created_at DESC",
        )
        .bind(live_scope_key)
        .fetch_all(&mut *connection)
        .await?;
        let Some(conversation) = conversations.first().cloned() else {
            return Ok(None);
        };
        let meeting_title: String = sqlx::query_scalar("SELECT title FROM meetings WHERE id = $1")
            .bind(meeting_id)
            .fetch_one(&mut *connection)
            .await?;

        let mut merged_ids: Vec<String> = conversations.into_iter().map(|item| item.id).collect();
        merged_ids.extend(
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM chat_conversations WHERE scope_kind = 'meeting' AND scope_key = $1 AND id != $2",
            )
            .bind(meeting_id)
            .bind(&conversation.id)
            .fetch_all(&mut *connection)
            .await?,
        );
        for merged_id in merged_ids.iter().filter(|id| *id != &conversation.id) {
            sqlx::query("UPDATE chat_messages SET conversation_id = $1 WHERE conversation_id = $2")
                .bind(&conversation.id)
                .bind(merged_id)
                .execute(&mut *connection)
                .await?;
            sqlx::query("DELETE FROM chat_conversations WHERE id = $1")
                .bind(merged_id)
                .execute(&mut *connection)
                .await?;
        }

        let messages: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, sources_json FROM chat_messages WHERE conversation_id = $1 AND sources_json IS NOT NULL",
        )
        .bind(&conversation.id)
        .fetch_all(&mut *connection)
        .await?;
        for (message_id, sources_json) in messages {
            sqlx::query("UPDATE chat_messages SET sources_json = $2 WHERE id = $1")
                .bind(message_id)
                .bind(rewrite_live_sources(
                    &sources_json,
                    live_scope_key,
                    meeting_id,
                    &meeting_title,
                )?)
                .execute(&mut *connection)
                .await?;
        }

        sqlx::query("UPDATE chat_conversations SET meeting_id = $2, origin = 'meeting', scope_kind = 'meeting', scope_key = $2, scope_data = NULL, promoted_from_live_scope_key = $3 WHERE id = $1")
            .bind(&conversation.id)
            .bind(meeting_id)
            .bind(live_scope_key)
            .execute(&mut *connection)
            .await?;
        Ok(Some(conversation.id.clone()))
    }

    pub async fn delete_conversation(pool: &SqlitePool, conversation_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM chat_conversations WHERE id = $1")
            .bind(conversation_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// GC for discarded (never-saved) live recordings: removes the unreachable
    /// live thread and its messages (cascade) only when the scope key matches
    /// and the thread was never promoted. Promoted threads changed scope_kind/
    /// scope_key at promotion, and the lineage guard fences any residual rows.
    pub async fn discard_live_recording(pool: &SqlitePool, live_scope_key: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM chat_conversations WHERE scope_kind = 'live_recording' AND scope_key = $1 AND promoted_from_live_scope_key IS NULL",
        )
        .bind(live_scope_key)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn remove_meeting_sources_in_transaction(
        connection: &mut SqliteConnection,
        meeting_id: &str,
    ) -> Result<()> {
        // ponytail: source metadata is denormalized JSON, so deletion scans only
        // source-bearing messages. Normalize sources if this becomes measurable.
        let messages: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, sources_json FROM chat_messages WHERE sources_json IS NOT NULL",
        )
        .fetch_all(&mut *connection)
        .await?;

        for (message_id, sources_json) in messages {
            let Ok(mut sources) = serde_json::from_str::<serde_json::Value>(&sources_json) else {
                sqlx::query("UPDATE chat_messages SET sources_json = NULL WHERE id = $1")
                    .bind(&message_id)
                    .execute(&mut *connection)
                    .await?;
                continue;
            };
            let Some(items) = sources.as_array_mut() else {
                sqlx::query("UPDATE chat_messages SET sources_json = NULL WHERE id = $1")
                    .bind(&message_id)
                    .execute(&mut *connection)
                    .await?;
                continue;
            };
            let original_len = items.len();
            items.retain(|source| {
                source.get("meetingId").and_then(|value| value.as_str()) != Some(meeting_id)
            });
            if items.len() == original_len {
                continue;
            }

            let rewritten = if items.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&sources)?)
            };
            sqlx::query("UPDATE chat_messages SET sources_json = $2 WHERE id = $1")
                .bind(&message_id)
                .bind(rewritten.as_deref())
                .execute(&mut *connection)
                .await?;
        }

        Ok(())
    }
}

async fn normalize_sources_for_persistence(
    connection: &mut SqliteConnection,
    sources_json: &str,
    conversation_context: Option<&(String, String, Option<String>, Option<String>)>,
) -> Result<Option<String>> {
    let mut sources: serde_json::Value = serde_json::from_str(sources_json)?;
    let Some(items) = sources.as_array_mut() else {
        bail!("Chat sources must be a JSON array");
    };

    let active_live_key = conversation_context.and_then(
        |(scope_kind, scope_key, promoted_from_live_scope_key, _)| {
            (scope_kind == "live_recording" && promoted_from_live_scope_key.is_none())
                .then_some(scope_key.as_str())
        },
    );
    if let Some((_, meeting_id, Some(live_scope_key), meeting_title)) = conversation_context {
        if let Some(meeting_title) = meeting_title {
            for source in items.iter_mut() {
                if source.get("sourceKind").and_then(|value| value.as_str())
                    == Some("live_recording")
                    && source.get("meetingId").and_then(|value| value.as_str())
                        == Some(live_scope_key)
                {
                    source["meetingId"] = meeting_id.as_str().into();
                    source["meetingTitle"] = meeting_title.as_str().into();
                    source["sourceKind"] = "meeting".into();
                    source["chunkType"] = "transcript".into();
                }
            }
        } else {
            items.retain(|source| {
                source.get("sourceKind").and_then(|value| value.as_str()) != Some("live_recording")
                    || source.get("meetingId").and_then(|value| value.as_str())
                        != Some(live_scope_key)
            });
        }
    }

    let mut meeting_availability = HashMap::new();
    let mut index = 0;
    while index < items.len() {
        let source = &items[index];
        let source_kind = source.get("sourceKind").and_then(|value| value.as_str());
        let source_meeting_id = source.get("meetingId").and_then(|value| value.as_str());
        let keep = if source_kind == Some("live_recording") {
            source_meeting_id == active_live_key
        } else if let Some(source_meeting_id) = source_meeting_id {
            if let Some(exists) = meeting_availability.get(source_meeting_id) {
                *exists
            } else {
                let exists: i64 =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE id = $1)")
                        .bind(source_meeting_id)
                        .fetch_one(&mut *connection)
                        .await?;
                let exists = exists != 0;
                meeting_availability.insert(source_meeting_id.to_string(), exists);
                exists
            }
        } else {
            false
        };

        if keep {
            index += 1;
        } else {
            items.remove(index);
        }
    }

    if items.is_empty() {
        Ok(None)
    } else {
        Ok(Some(serde_json::to_string(&sources)?))
    }
}

fn rewrite_live_sources(
    sources_json: &str,
    live_scope_key: &str,
    meeting_id: &str,
    meeting_title: &str,
) -> Result<String> {
    let mut sources: serde_json::Value = serde_json::from_str(sources_json)?;
    if let Some(items) = sources.as_array_mut() {
        for source in items {
            if source.get("sourceKind").and_then(|value| value.as_str()) == Some("live_recording")
                && source.get("meetingId").and_then(|value| value.as_str()) == Some(live_scope_key)
            {
                source["meetingId"] = meeting_id.into();
                source["meetingTitle"] = meeting_title.into();
                source["sourceKind"] = "meeting".into();
                source["chunkType"] = "transcript".into();
            }
        }
    }
    Ok(serde_json::to_string(&sources)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::migrate::Migrator;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::fs;
    use std::str::FromStr;

    async fn test_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE meetings (id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL DEFAULT 'Saved meeting')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE chat_conversations (id TEXT PRIMARY KEY NOT NULL, meeting_id TEXT REFERENCES meetings(id) ON DELETE SET NULL, origin TEXT NOT NULL DEFAULT 'meeting', scope_kind TEXT, scope_key TEXT, scope_data TEXT, promoted_from_live_scope_key TEXT UNIQUE, title TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE UNIQUE INDEX idx_chat_conversations_scope_identity ON chat_conversations(scope_kind, scope_key, COALESCE(scope_data, ''))")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TRIGGER chat_conversations_orphan_deleted_meeting AFTER UPDATE OF meeting_id ON chat_conversations WHEN OLD.meeting_id IS NOT NULL AND NEW.meeting_id IS NULL AND NEW.origin != 'global' BEGIN UPDATE chat_conversations SET scope_kind = 'orphaned_meeting' WHERE id = NEW.id; END")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE chat_messages (id TEXT PRIMARY KEY NOT NULL, conversation_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, sources_json TEXT, is_error INTEGER DEFAULT 0, created_at TEXT NOT NULL, FOREIGN KEY (conversation_id) REFERENCES chat_conversations(id) ON DELETE CASCADE)")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn scoped_lookup_is_exact_and_resumes_its_scope() {
        let pool = test_pool().await;
        let all = ChatScope {
            kind: ChatScopeKind::All,
            key: "all".to_string(),
            data: None,
        };
        let folder = ChatScope {
            kind: ChatScopeKind::Folder,
            key: "all".to_string(),
            data: None,
        };

        let all_conversation = ChatRepository::get_or_create_scoped_conversation(&pool, &all, None)
            .await
            .unwrap();
        let folder_conversation =
            ChatRepository::get_or_create_scoped_conversation(&pool, &folder, None)
                .await
                .unwrap();
        let resumed = ChatRepository::get_or_create_scoped_conversation(&pool, &all, None)
            .await
            .unwrap();

        assert_ne!(all_conversation.id, folder_conversation.id);
        assert_eq!(resumed.id, all_conversation.id);
    }

    #[tokio::test]
    async fn concurrent_scoped_get_or_create_returns_one_exact_conversation() {
        let pool = test_pool().await;
        let scope = ChatScope {
            kind: ChatScopeKind::Folder,
            key: "folder-1".to_string(),
            data: None,
        };
        let mut callers = tokio::task::JoinSet::new();
        for _ in 0..16 {
            let pool = pool.clone();
            let scope = scope.clone();
            callers.spawn(async move {
                ChatRepository::get_or_create_scoped_conversation(&pool, &scope, None)
                    .await
                    .unwrap()
            });
        }
        let mut ids = std::collections::HashSet::new();
        while let Some(result) = callers.join_next().await {
            let conversation = result.unwrap();
            assert_eq!(conversation.scope_kind, "folder");
            assert_eq!(conversation.scope_key, "folder-1");
            assert_eq!(conversation.scope_data, None);
            ids.insert(conversation.id);
        }
        assert_eq!(ids.len(), 1);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_conversations WHERE scope_kind = 'folder' AND scope_key = 'folder-1' AND scope_data IS NULL").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn existing_global_lookup_keeps_global_lineage() {
        let pool = test_pool().await;
        let conversation_id = ChatRepository::create_conversation(&pool, None, None, "global")
            .await
            .unwrap();
        let conversation = ChatRepository::get_latest_conversation(&pool, None)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(conversation.id, conversation_id);
        assert_eq!(conversation.origin, "global");
        assert_eq!(conversation.scope_kind, "all");
        assert_eq!(conversation.scope_key, "all");
    }

    #[tokio::test]
    async fn meeting_lookup_keeps_meeting_lineage() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id) VALUES (?)")
            .bind("meeting-1")
            .execute(&pool)
            .await
            .unwrap();

        let conversation_id =
            ChatRepository::create_conversation(&pool, Some("meeting-1"), None, "meeting")
                .await
                .unwrap();
        ChatRepository::save_message(
            &pool,
            &conversation_id,
            "user",
            "What did we decide?",
            None,
            false,
        )
        .await
        .unwrap();
        ChatRepository::save_message(
            &pool,
            &conversation_id,
            "assistant",
            "Ship it.",
            Some("[]"),
            false,
        )
        .await
        .unwrap();

        let conversation = ChatRepository::get_latest_conversation(&pool, Some("meeting-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(conversation.id, conversation_id);
        assert_eq!(conversation.scope_kind, "meeting");
        assert_eq!(conversation.scope_key, "meeting-1");
    }

    #[tokio::test]
    async fn deleted_meeting_orphan_is_excluded_from_global_lookup() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id) VALUES (?)")
            .bind("meeting-1")
            .execute(&pool)
            .await
            .unwrap();

        let conversation_id =
            ChatRepository::create_conversation(&pool, Some("meeting-1"), None, "meeting")
                .await
                .unwrap();
        sqlx::query("DELETE FROM meetings WHERE id = ?")
            .bind("meeting-1")
            .execute(&pool)
            .await
            .unwrap();

        let conversation =
            sqlx::query_as::<_, ChatConversation>("SELECT * FROM chat_conversations WHERE id = ?")
                .bind(conversation_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(conversation.meeting_id, None);
        assert_eq!(conversation.origin, "meeting");
        assert_eq!(conversation.scope_kind, "orphaned_meeting");
        assert!(ChatRepository::get_latest_conversation(&pool, None)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn live_promotion_is_atomic_and_rewrites_live_sources() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('meeting-1', 'Planning')")
            .execute(&pool)
            .await
            .unwrap();
        let scope = ChatScope {
            kind: ChatScopeKind::LiveRecording,
            key: "live-1".into(),
            data: None,
        };
        let conversation = ChatRepository::get_or_create_scoped_conversation(&pool, &scope, None)
            .await
            .unwrap();
        ChatRepository::save_message(&pool, &conversation.id, "assistant", "Answer", Some(r#"[{"meetingId":"live-1","meetingTitle":"Live recording","chunkType":"live_transcript","snippet":"Now","folderName":"","sourceKind":"live_recording"}]"#), false).await.unwrap();

        ChatRepository::promote_live_recording(&pool, "live-1", "meeting-1")
            .await
            .unwrap();

        let promoted = ChatRepository::get_conversation(&pool, &conversation.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(promoted.meeting_id.as_deref(), Some("meeting-1"));
        assert_eq!(promoted.origin, "meeting");
        assert_eq!(promoted.scope_kind, "meeting");
        let messages = ChatRepository::get_messages(&pool, &conversation.id)
            .await
            .unwrap();
        assert!(messages[0]
            .sources_json
            .as_deref()
            .unwrap()
            .contains(r#""sourceKind":"meeting""#));
    }

    #[tokio::test]
    async fn failed_live_promotion_retains_live_conversation() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('meeting-1', 'Planning')")
            .execute(&pool)
            .await
            .unwrap();
        let scope = ChatScope {
            kind: ChatScopeKind::LiveRecording,
            key: "live-1".into(),
            data: None,
        };
        let conversation = ChatRepository::get_or_create_scoped_conversation(&pool, &scope, None)
            .await
            .unwrap();
        sqlx::query("INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES ('corrupt', $1, 'assistant', 'Answer', 'not-json', '2026-01-01T00:00:00Z')")
            .bind(&conversation.id)
            .execute(&pool)
            .await
            .unwrap();

        assert!(
            ChatRepository::promote_live_recording(&pool, "live-1", "meeting-1")
                .await
                .is_err()
        );
        let retained = ChatRepository::get_conversation(&pool, &conversation.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retained.scope_kind, "live_recording");
        assert_eq!(retained.scope_key, "live-1");
    }

    #[tokio::test]
    async fn scope_identity_repair_backfills_and_merges_before_unique_index() {
        let pool = test_pool().await;
        sqlx::query("DROP INDEX idx_chat_conversations_scope_identity")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meetings (id) VALUES ('meeting-1'), ('meeting-2')")
            .execute(&pool)
            .await
            .unwrap();
        for (id, meeting_id, origin, scope_kind, scope_key, scope_data, lineage, updated_at) in [
            (
                "legacy-global",
                None,
                "global",
                None,
                None,
                None,
                None,
                "2026-01-01T00:00:00Z",
            ),
            (
                "legacy-meeting",
                Some("meeting-1"),
                "meeting",
                None,
                None,
                None,
                None,
                "2026-01-01T00:00:00Z",
            ),
            (
                "legacy-orphan",
                None,
                "meeting",
                None,
                None,
                None,
                None,
                "2026-01-01T00:00:00Z",
            ),
            (
                "null-scope",
                None,
                "meeting",
                Some("meeting"),
                None,
                None,
                None,
                "2026-01-01T00:00:00Z",
            ),
            (
                "duplicate-global-old",
                None,
                "global",
                Some("all"),
                Some("all"),
                None,
                None,
                "2026-01-01T00:00:00Z",
            ),
            (
                "duplicate-global-new",
                None,
                "global",
                Some("all"),
                Some("all"),
                Some(""),
                None,
                "2026-01-02T00:00:00Z",
            ),
            (
                "duplicate-meeting-lineage",
                Some("meeting-2"),
                "meeting",
                Some("meeting"),
                Some("meeting-2"),
                None,
                Some("live-1"),
                "2026-01-01T00:00:00Z",
            ),
            (
                "duplicate-meeting-new",
                Some("meeting-2"),
                "meeting",
                Some("meeting"),
                Some("meeting-2"),
                None,
                None,
                "2026-01-02T00:00:00Z",
            ),
        ] {
            sqlx::query("INSERT INTO chat_conversations (id, meeting_id, origin, scope_kind, scope_key, scope_data, promoted_from_live_scope_key, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, '2026-01-01T00:00:00Z', ?)")
                .bind(id)
                .bind(meeting_id)
                .bind(origin)
                .bind(scope_kind)
                .bind(scope_key)
                .bind(scope_data)
                .bind(lineage)
                .bind(updated_at)
                .execute(&pool)
                .await
                .unwrap();
        }
        for (id, conversation_id, sources_json) in [
            (
                "global-message",
                "duplicate-global-old",
                r#"[{"source":"global"}]"#,
            ),
            (
                "meeting-message",
                "duplicate-meeting-new",
                r#"[{"source":"meeting"}]"#,
            ),
        ] {
            sqlx::query("INSERT INTO chat_messages (id, conversation_id, role, content, sources_json, created_at) VALUES (?, ?, 'assistant', 'answer', ?, '2026-01-01T00:00:00Z')")
                .bind(id)
                .bind(conversation_id)
                .bind(sources_json)
                .execute(&pool)
                .await
                .unwrap();
        }

        let migrations = tempfile::tempdir().unwrap();
        fs::write(
            migrations
                .path()
                .join("20260817115000_repair_chat_scope_identities.sql"),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/migrations/20260817115000_repair_chat_scope_identities.sql"
            )),
        )
        .unwrap();
        fs::write(
            migrations
                .path()
                .join("20260817120000_add_chat_scope_identity.sql"),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/migrations/20260817120000_add_chat_scope_identity.sql"
            )),
        )
        .unwrap();
        Migrator::new(migrations.path())
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();

        for (id, scope_kind, scope_key) in [
            ("legacy-meeting", "meeting", "meeting-1"),
            ("legacy-orphan", "orphaned_meeting", "legacy-orphan"),
            ("null-scope", "orphaned_meeting", "null-scope"),
        ] {
            let conversation = ChatRepository::get_conversation(&pool, id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(conversation.scope_kind, scope_kind);
            assert_eq!(conversation.scope_key, scope_key);
            assert_eq!(conversation.scope_data, None);
        }
        assert_eq!(
            ChatRepository::get_latest_conversation(&pool, None)
                .await
                .unwrap()
                .unwrap()
                .id,
            "duplicate-global-new"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT conversation_id FROM chat_messages WHERE id = 'global-message'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "duplicate-global-new"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT sources_json FROM chat_messages WHERE id = 'global-message'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            r#"[{"source":"global"}]"#
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT conversation_id FROM chat_messages WHERE id = 'meeting-message'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "duplicate-meeting-lineage"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT promoted_from_live_scope_key FROM chat_conversations WHERE id = 'duplicate-meeting-lineage'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            "live-1"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM chat_conversations GROUP BY scope_kind, scope_key, COALESCE(scope_data, '') HAVING count(*) > 1")
                .fetch_optional(&pool)
                .await
                .unwrap(),
            None
        );
        assert!(sqlx::query("INSERT INTO chat_conversations (id, origin, scope_kind, scope_key, created_at, updated_at) VALUES ('duplicate', 'global', 'all', 'all', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(&pool)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn repeated_promotion_merges_target_and_fences_late_sources() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('meeting-1', 'Planning')")
            .execute(&pool)
            .await
            .unwrap();
        let live = ChatRepository::get_or_create_scoped_conversation(
            &pool,
            &ChatScope {
                kind: ChatScopeKind::LiveRecording,
                key: "live-1".into(),
                data: None,
            },
            None,
        )
        .await
        .unwrap();
        let target = ChatRepository::get_or_create_scoped_conversation(
            &pool,
            &ChatScope {
                kind: ChatScopeKind::Meeting,
                key: "meeting-1".into(),
                data: None,
            },
            None,
        )
        .await
        .unwrap();
        ChatRepository::save_message(&pool, &live.id, "user", "Live question", None, false)
            .await
            .unwrap();
        ChatRepository::save_message(&pool, &target.id, "user", "Earlier question", None, false)
            .await
            .unwrap();

        let first = ChatRepository::promote_live_recording(&pool, "live-1", "meeting-1")
            .await
            .unwrap();
        let repeated = ChatRepository::promote_live_recording(&pool, "live-1", "meeting-1")
            .await
            .unwrap();
        ChatRepository::save_message(&pool, &live.id, "assistant", "Partial", Some(r#"[{"meetingId":"live-1","meetingTitle":"Live recording","chunkType":"live_transcript","snippet":"Now","folderName":"","sourceKind":"live_recording"}]"#), false).await.unwrap();

        assert_eq!(first, repeated);
        assert_eq!(first.as_deref(), Some(live.id.as_str()));
        assert_eq!(
            ChatRepository::get_promoted_meeting_id(&pool, "live-1")
                .await
                .unwrap()
                .as_deref(),
            Some("meeting-1")
        );
        let conversations: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_conversations WHERE scope_kind = 'meeting' AND scope_key = 'meeting-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(conversations, 1);
        let messages = ChatRepository::get_messages(&pool, &live.id).await.unwrap();
        assert_eq!(messages.len(), 3);
        assert!(messages
            .last()
            .unwrap()
            .sources_json
            .as_deref()
            .unwrap()
            .contains(r#""meetingId":"meeting-1""#));
        assert!(ChatRepository::get_conversation(&pool, &target.id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn discard_removes_only_the_unpromoted_live_thread() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('meeting-1', 'Planning'), ('meeting-2', 'Retro')")
            .execute(&pool)
            .await
            .unwrap();
        let discarded = ChatRepository::get_or_create_scoped_conversation(
            &pool,
            &ChatScope {
                kind: ChatScopeKind::LiveRecording,
                key: "live-discarded".into(),
                data: None,
            },
            None,
        )
        .await
        .unwrap();
        ChatRepository::save_message(
            &pool,
            &discarded.id,
            "user",
            "Asked while recording",
            None,
            false,
        )
        .await
        .unwrap();

        // Saved recording: promoted to a meeting thread.
        let promoted = ChatRepository::get_or_create_scoped_conversation(
            &pool,
            &ChatScope {
                kind: ChatScopeKind::LiveRecording,
                key: "live-saved".into(),
                data: None,
            },
            None,
        )
        .await
        .unwrap();
        ChatRepository::promote_live_recording(&pool, "live-saved", "meeting-1")
            .await
            .unwrap();
        // Other scope: an ordinary meeting thread on a different meeting.
        let meeting = ChatRepository::get_or_create_scoped_conversation(
            &pool,
            &ChatScope {
                kind: ChatScopeKind::Meeting,
                key: "meeting-2".into(),
                data: None,
            },
            None,
        )
        .await
        .unwrap();

        ChatRepository::discard_live_recording(&pool, "live-discarded")
            .await
            .unwrap();
        // Idempotent: discarding again is a no-op.
        ChatRepository::discard_live_recording(&pool, "live-discarded")
            .await
            .unwrap();

        assert!(ChatRepository::get_conversation(&pool, &discarded.id)
            .await
            .unwrap()
            .is_none());
        let messages: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_messages WHERE conversation_id = $1")
                .bind(&discarded.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(messages, 0);
        let promoted_row = ChatRepository::get_conversation(&pool, &promoted.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(promoted_row.scope_kind, "meeting");
        assert_eq!(promoted_row.scope_key, "meeting-1");
        assert!(ChatRepository::get_conversation(&pool, &meeting.id)
            .await
            .unwrap()
            .is_some());
    }
}
