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
        // Meeting scopes for a deleted meeting are a failed lookup, not a
        // foreign-key jargon error: the disclosure tells the user the thread
        // is gone and that earlier answers may still quote deleted content.
        if scope.kind == ChatScopeKind::Meeting {
            let exists: i64 =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE id = ?)")
                    .bind(&scope.key)
                    .fetch_one(pool)
                    .await?;
            if exists == 0 {
                bail!(
                    "{}|{}",
                    DELETED_MEETING_THREAD_CODE,
                    DELETED_MEETING_THREAD_ERROR
                );
            }
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let scope_data = scope.data.as_ref().map(serde_json::to_string).transpose()?;
        let (meeting_id, origin) = scope.lineage();

        let inserted = sqlx::query_as::<_, ChatConversation>(
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
        .await;
        match inserted {
            Ok(conversation) => Ok(conversation),
            Err(error) => {
                // The existence pre-check above can lose a race with a
                // concurrent deletion: the foreign-key failure then means the
                // meeting disappeared, so surface the same typed disclosure
                // instead of raw database jargon.
                if scope.kind == ChatScopeKind::Meeting {
                    let exists: i64 =
                        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE id = ?)")
                            .bind(&scope.key)
                            .fetch_one(pool)
                            .await
                            .unwrap_or(0);
                    if exists == 0 {
                        bail!(
                            "{}|{}",
                            DELETED_MEETING_THREAD_CODE,
                            DELETED_MEETING_THREAD_ERROR
                        );
                    }
                }
                Err(error.into())
            }
        }
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
            let mut sources = match serde_json::from_str::<serde_json::Value>(&sources_json) {
                Ok(sources) => sources,
                // Malformed legacy payload: clear it only when the raw text
                // actually carries the deleted meeting as a meetingId value;
                // an unrelated malformed payload is preserved verbatim.
                Err(_) => {
                    if raw_sources_reference_meeting(&sources_json, meeting_id) {
                        sqlx::query("UPDATE chat_messages SET sources_json = NULL WHERE id = $1")
                            .bind(&message_id)
                            .execute(&mut *connection)
                            .await?;
                    }
                    continue;
                }
            };
            let Some(items) = sources.as_array_mut() else {
                // Non-array payload: same containment rule as malformed JSON,
                // decided structurally over the parsed value.
                if value_references_meeting(&sources, meeting_id) {
                    sqlx::query("UPDATE chat_messages SET sources_json = NULL WHERE id = $1")
                        .bind(&message_id)
                        .execute(&mut *connection)
                        .await?;
                }
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

/// Stable machine-readable condition code surfaced for a deleted-meeting chat
/// thread. The frontend maps the condition by exact code equality, not
/// substring; the human disclosure follows the separator.
pub const DELETED_MEETING_THREAD_CODE: &str = "deleted_meeting_thread";

/// Human-readable part of the typed deleted-meeting condition; carries no
/// meeting content.
pub const DELETED_MEETING_THREAD_ERROR: &str = "This meeting's chat thread is no longer available because the meeting was deleted. Earlier answers may still quote deleted content.";

/// Advances past JSON whitespace (space, tab, CR, LF) from a byte index,
/// metering EVERY probed byte — whitespace bytes and the terminating
/// non-whitespace probe — against the shared budget. Returns `None` only when
/// the budget is exhausted (the caller fails closed); end of input and
/// non-whitespace bytes return the index.
fn skip_json_whitespace(raw: &str, mut index: usize, budget: &mut ScanBudget) -> Option<usize> {
    loop {
        // The whitespace skip has no per-token cap (it is not inside a
        // token); the global budget is the only bound here.
        match probe_byte(raw, index, 0, usize::MAX, budget) {
            Probe::Byte(byte) if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') => index += 1,
            Probe::Byte(_) => return Some(index),
            Probe::Absent => return Some(index),
            Probe::OverTokenCap => return None,
            Probe::Exhausted => return None,
        }
    }
}

fn hex_digit_value(byte: u8) -> Option<u16> {
    match byte {
        b'0'..=b'9' => Some((byte - b'0') as u16),
        b'a'..=b'f' => Some((byte - b'a') as u16 + 10),
        b'A'..=b'F' => Some((byte - b'A') as u16 + 10),
        _ => None,
    }
}

fn push_utf8(decoded: &mut Vec<u8>, code: u32) {
    let mut buffer = [0u8; 4];
    decoded.extend_from_slice(
        char::from_u32(code)
            .unwrap_or('\u{FFFD}')
            .encode_utf8(&mut buffer)
            .as_bytes(),
    );
}

/// One attempted string-token scan: the decoded result (token text and byte
/// index just past the closing quote) when valid, and whether the shared work
/// budget ran out during the scan (the caller fails closed in that case).
struct ScanOutcome {
    token: Option<(String, usize)>,
    budget_exhausted: bool,
}

impl ScanOutcome {
    fn failed() -> Self {
        Self {
            token: None,
            budget_exhausted: false,
        }
    }

    fn exhausted() -> Self {
        Self {
            token: None,
            budget_exhausted: true,
        }
    }
}

/// Meters every inspected byte of a malformed-payload scan BEFORE it is read:
/// traversal stops immediately when the budget is spent, so no path can walk
/// unbounded and charge afterwards. One byte per `take`; exhausted means the
/// caller must fail closed (clear) before any additional traversal.
pub(crate) struct ScanBudget {
    remaining: usize,
}

impl ScanBudget {
    pub(crate) fn new() -> Self {
        Self {
            remaining: MAX_SCAN_WORK_BYTES,
        }
    }

    pub(crate) fn exhausted(&self) -> bool {
        self.remaining == 0
    }

    /// Reserves one byte of budget. Always paired with exactly one byte read
    /// (bounds pre-checked separately), so the charge equals actual reads.
    fn take(&mut self) {
        self.remaining -= 1;
    }
}

/// One metered byte probe: the byte is read only after the budget is checked
/// and charged. `Absent` is end-of-input (nothing read, nothing charged);
/// `Exhausted` means the budget was spent before the probe (the caller fails
/// closed). Every direct byte inspection in the scanner routes through here,
/// so no probe can bypass the budget.
enum Probe {
    Byte(u8),
    Absent,
    /// The per-token cap (max_bytes after the opening quote) fired before
    /// this probe: the token is over-long and must be rejected, while the
    /// global budget remains unspent by this probe.
    OverTokenCap,
    Exhausted,
}

/// One metered, capped byte probe: EVERY direct byte inspection in the
/// scanner routes through here, so no probe can bypass the budget or the
/// per-token cap. The byte is read only after both limits are checked and
/// the budget charged. Absent is end-of-input (nothing read, nothing
/// charged); OverCap means the probe would be past after-opening byte
/// max_bytes (the token is over-long: the caller fails the token, scans
/// on); Exhausted means the budget was spent before the probe (the caller
/// fails closed).
fn probe_byte(
    raw: &str,
    index: usize,
    quote_pos: usize,
    max_bytes: usize,
    budget: &mut ScanBudget,
) -> Probe {
    if index >= raw.len() {
        return Probe::Absent;
    }
    if index - quote_pos > max_bytes {
        return Probe::OverTokenCap;
    }
    if budget.exhausted() {
        return Probe::Exhausted;
    }
    budget.take();
    Probe::Byte(raw.as_bytes()[index])
}
/// Decodes one JSON string token beginning at the opening quote `quote_pos`
/// (a byte index into `raw`). Returns the decoded text and the byte index just
/// past the closing quote, or `None` when the token is not a valid JSON
/// string. All escape forms are decoded (\" \\ \/ \b \f \n \r \t \uXXXX with
/// uppercase/lowercase hex and surrogate pairs), so decoded-equivalent
/// `meetingId` keys/values compare exactly. A token longer than `max_bytes`
/// cannot be a target key/value (real meeting ids are <= 512 bytes and
/// escapes only expand the raw form) and is rejected, keeping every scan
/// bounded. Single forward pass, no recursion, no panic.
///
/// Accounting and bounding are centralized: EVERY byte read — the opening
/// quote, content, escape bytes, `\u` digits, surrogate lookahead, and the
/// closing quote — goes through [`probe_byte`], so each probe is metered
/// against the shared budget before it reads, and budget exhaustion is
/// reported as `budget_exhausted` (the caller fails closed).
fn scan_json_string(
    raw: &str,
    quote_pos: usize,
    max_bytes: usize,
    budget: &mut ScanBudget,
) -> ScanOutcome {
    match probe_byte(raw, quote_pos, quote_pos, max_bytes, budget) {
        Probe::Byte(b'"') => {}
        Probe::Byte(_) | Probe::Absent | Probe::OverTokenCap => return ScanOutcome::failed(),
        Probe::Exhausted => return ScanOutcome::exhausted(),
    }
    let mut decoded: Vec<u8> = Vec::new();
    let mut index = quote_pos + 1;
    loop {
        // Meter BEFORE the read: token cap, bounds, then budget. The token
        // cap is DERIVED from the probe position, so it counts EVERY probed
        // raw token byte by construction: bytes AFTER the opening quote
        // (the closing quote included, the opening quote excluded),
        // including the escape code byte, all `\u` digits, and surrogate
        // lookahead. Raw probed bytes, not decoded size: a fully escaped
        // token probes more raw bytes than it decodes, while a real meeting
        // id (<= 512 decoded bytes, <= ~3074 fully escaped) always fits.
        // After-opening byte #4097 is refused before it is read.
        let byte = match probe_byte(raw, index, quote_pos, max_bytes, budget) {
            Probe::Byte(byte) => byte,
            Probe::OverTokenCap | Probe::Absent => return ScanOutcome::failed(),
            Probe::Exhausted => return ScanOutcome::exhausted(),
        };
        index += 1;
        match byte {
            b'"' => {
                let token = String::from_utf8(decoded).ok().map(|text| (text, index));
                return ScanOutcome {
                    token,
                    budget_exhausted: false,
                };
            }
            b'\\' => {
                // Escape character: same meter-before-read, capped probe.
                let escape = match probe_byte(raw, index, quote_pos, max_bytes, budget) {
                    Probe::Byte(escape) => escape,
                    Probe::OverTokenCap | Probe::Absent => return ScanOutcome::failed(),
                    Probe::Exhausted => return ScanOutcome::exhausted(),
                };
                index += 1;

                match escape {
                    b'"' | b'\\' | b'/' => decoded.push(escape),
                    b'b' => decoded.push(0x08),
                    b'f' => decoded.push(0x0C),
                    b'n' => decoded.push(b'\n'),
                    b'r' => decoded.push(b'\r'),
                    b't' => decoded.push(b'\t'),
                    b'u' => {
                        let mut value: u16 = 0;
                        for _ in 0..4 {
                            let digit = match probe_byte(raw, index, quote_pos, max_bytes, budget) {
                                Probe::Byte(byte) => match hex_digit_value(byte) {
                                    Some(digit) => digit,
                                    None => return ScanOutcome::failed(),
                                },
                                Probe::OverTokenCap | Probe::Absent => {
                                    return ScanOutcome::failed()
                                }
                                Probe::Exhausted => return ScanOutcome::exhausted(),
                            };
                            index += 1;
                            value = match value
                                .checked_mul(16)
                                .and_then(|current| current.checked_add(digit))
                            {
                                Some(current) => current,
                                None => return ScanOutcome::failed(),
                            };
                        }
                        let code = if (0xD800..0xDC00).contains(&value) {
                            // High surrogate: the low surrogate escape must
                            // follow immediately; both lookahead bytes are
                            // metered and charged when present.
                            let first = match probe_byte(raw, index, quote_pos, max_bytes, budget) {
                                Probe::Byte(byte) => byte,
                                Probe::OverTokenCap | Probe::Absent => {
                                    return ScanOutcome::failed()
                                }
                                Probe::Exhausted => return ScanOutcome::exhausted(),
                            };
                            index += 1;

                            if first != b'\\' {
                                return ScanOutcome::failed();
                            }
                            let second = match probe_byte(raw, index, quote_pos, max_bytes, budget)
                            {
                                Probe::Byte(byte) => byte,
                                Probe::OverTokenCap | Probe::Absent => {
                                    return ScanOutcome::failed()
                                }
                                Probe::Exhausted => return ScanOutcome::exhausted(),
                            };
                            index += 1;

                            if second != b'u' {
                                return ScanOutcome::failed();
                            }
                            let mut low: u16 = 0;
                            for _ in 0..4 {
                                let digit =
                                    match probe_byte(raw, index, quote_pos, max_bytes, budget) {
                                        Probe::Byte(byte) => match hex_digit_value(byte) {
                                            Some(digit) => digit,
                                            None => return ScanOutcome::failed(),
                                        },
                                        Probe::OverTokenCap | Probe::Absent => {
                                            return ScanOutcome::failed()
                                        }
                                        Probe::Exhausted => return ScanOutcome::exhausted(),
                                    };
                                index += 1;
                                low = match low
                                    .checked_mul(16)
                                    .and_then(|current| current.checked_add(digit))
                                {
                                    Some(current) => current,
                                    None => return ScanOutcome::failed(),
                                };
                            }
                            0x10000 + (((value as u32) - 0xD800) << 10) + ((low as u32) - 0xDC00)
                        } else if (0xDC00..0xE000).contains(&value) {
                            return ScanOutcome::failed();
                        } else {
                            value as u32
                        };
                        push_utf8(&mut decoded, code);
                    }
                    _ => return ScanOutcome::failed(),
                }
            }
            _ => {
                if byte < 0x20 {
                    // Raw control characters must be escaped in JSON.
                    return ScanOutcome::failed();
                }
                decoded.push(byte);
            }
        }
    }
}
/// Upper bound for one scanned string token, counted in PROBED RAW bytes
/// read AFTER the opening quote: the opening quote is EXCLUDED; every
/// content byte, the escape code byte, all `\u` hex digits, surrogate
/// lookahead bytes, and the closing quote are INCLUDED — so the cap is on
/// raw in-token bytes read, not the decoded size (decoded length is always
/// <= probed length, and a real meeting id of <= 512 decoded bytes
/// serializes to at most ~3074 fully escaped raw bytes, so a real target
/// always fits while an over-cap target-like string cannot alter scan
/// behavior or evade the scrub).
const MAX_SCAN_TOKEN_BYTES: usize = 4096;

/// Explicit total-work budget for one malformed-payload scan. EVERY byte
/// traversal — quote search (including the tail after the last quote), token
/// walks on every failure path, whitespace/inter-token gaps, and nested value
/// scans — is METERED against this budget before the byte is read, and
/// traversal stops immediately when it is spent. A genuinely oversized or
/// adversarial payload that exhausts the budget is treated as source-bearing
/// (fail-closed): the whole payload is cleared rather than silently
/// preserved, so unscanned deleted source metadata cannot survive. The
/// deliberate data-preservation tradeoff — an unrelated oversized malformed
/// payload is also cleared — is accepted because real persisted
/// `sources_json` serializations are far smaller than the budget.
pub(crate) const MAX_SCAN_WORK_BYTES: usize = 1024 * 1024;

/// Decides whether an UNPARSABLE legacy `sources_json` payload carries the
/// deleted meeting in a source-bearing form: a decoded-equivalent string key
/// `meetingId` (any JSON escape form), a real colon, and a decoded string
/// value exactly equal to the deleted ID. Whole-document corruption is
/// tolerated, but the pair structure is required — a missing colon, an
/// unquoted value, a longer ID, or a bare-ID mention inside snippet text is
/// never treated as a source field. Only such payloads are cleared; other
/// malformed payloads within the work budget are preserved.
///
/// Recovery is bounded and fully accounted: every quote position is tried
/// exactly once as a token start (a failed or non-key token never consumes
/// later bytes, so an unmatched prefix cannot hide a later target pair), and
/// every byte traversal — quote search including the tail after the last
/// quote, token-failure paths, whitespace gaps, and nested value scans — is
/// metered before it reads against [`MAX_SCAN_WORK_BYTES`]; exhausting the
/// budget fails closed (clear) before any additional traversal.
fn raw_sources_reference_meeting(raw: &str, meeting_id: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut budget = ScanBudget::new();
    let mut cursor = 0usize;
    while cursor < raw.len() {
        // Meter BEFORE the read: budget exhaustion fails closed immediately,
        // before any additional traversal.
        if budget.exhausted() {
            return true;
        }
        budget.take();
        let is_quote = bytes[cursor] == b'"';
        cursor += 1;
        if !is_quote {
            continue;
        }
        let ScanOutcome {
            token,
            budget_exhausted,
            ..
        } = scan_json_string(raw, cursor - 1, MAX_SCAN_TOKEN_BYTES, &mut budget);
        if budget_exhausted {
            // Fail closed before any additional traversal.
            return true;
        }
        let Some((key, after_key)) = token else {
            continue;
        };
        if key == "meetingId" {
            let Some(after_whitespace) = skip_json_whitespace(raw, after_key, &mut budget) else {
                return true;
            };
            // Metered colon probe (the whitespace skip already probed and
            // charged the terminator byte; this re-probe is a second actual
            // read and is charged as such).
            match probe_byte(raw, after_whitespace, 0, usize::MAX, &mut budget) {
                Probe::Byte(b':') => {}
                Probe::Byte(_) | Probe::Absent => continue,
                Probe::OverTokenCap | Probe::Exhausted => return true,
            }
            let after_colon = after_whitespace + 1;
            let Some(after_colon_ws) = skip_json_whitespace(raw, after_colon, &mut budget) else {
                return true;
            };
            // Metered value opening-quote probe.
            match probe_byte(raw, after_colon_ws, 0, usize::MAX, &mut budget) {
                Probe::Byte(b'"') => {}
                Probe::Byte(_) | Probe::Absent => continue,
                Probe::OverTokenCap | Probe::Exhausted => return true,
            }
            let ScanOutcome {
                token: value_token,
                budget_exhausted,
                ..
            } = scan_json_string(raw, after_colon_ws, MAX_SCAN_TOKEN_BYTES, &mut budget);
            if budget_exhausted {
                return true;
            }
            if let Some((value, _)) = value_token {
                if value == meeting_id {
                    return true;
                }
            }
        }
    }
    false
}

/// Structural containment for a parseable but non-array payload: true when
/// any object in the value carries `meetingId == meeting_id`.
fn value_references_meeting(value: &serde_json::Value, meeting_id: &str) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.get("meetingId").and_then(|value| value.as_str()) == Some(meeting_id)
                || map
                    .values()
                    .any(|value| value_references_meeting(value, meeting_id))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|value| value_references_meeting(value, meeting_id)),
        _ => false,
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

    /// Exact per-form accounting: every present byte probe is metered against
    /// the budget, so a failure path cannot under-report the work the caller
    /// budgeted. Verified as the budget delta (the authoritative meter).
    #[test]
    fn scan_json_string_meters_every_probe_against_the_budget() {
        let consumed = |raw: &str, max: usize| {
            let mut budget = ScanBudget::new();
            let outcome = scan_json_string(raw, 0, max, &mut budget);
            (
                outcome.token.is_some(),
                outcome.budget_exhausted,
                MAX_SCAN_WORK_BYTES - budget.remaining,
            )
        };
        // Successes: opening quote + content + closing quote probes.
        assert_eq!(consumed(r#""""#, 4096), (true, false, 2));
        assert_eq!(consumed(r#""ab""#, 4096), (true, false, 4));
        assert_eq!(consumed(r#""\u0041""#, 4096), (true, false, 8));
        // Surrogate pair success: quote + \u + 4 + \u + 4 + close = 14 probes.
        assert_eq!(consumed(r#""\ud83e\udd80""#, 4096), (true, false, 14));
        // Trailing backslash: the backslash itself is metered.
        assert_eq!(consumed(r#""abc\"#, 4096), (false, false, 5));
        // Invalid escape: quote + backslash + escape char.
        assert_eq!(consumed(r#""\q x"#, 4096), (false, false, 3));
        // Truncated \u digits: quote + backslash + u + present digits.
        assert_eq!(consumed(r#""\u12"#, 4096), (false, false, 5));
        // Invalid hex digit: metered through the failing byte.
        assert_eq!(consumed(r#""\uZZZZ""#, 4096), (false, false, 4));
        // High surrogate without a low escape: the lookahead byte is metered.
        assert_eq!(consumed(r#""\ud800""#, 4096), (false, false, 8));
        assert_eq!(consumed(r#""\ud800x""#, 4096), (false, false, 8));
        // Low surrogate digits truncated inside the pair: metered so far.
        assert_eq!(consumed(r#""\ud800\u12"#, 4096), (false, false, 11));
        // Lone low surrogate: rejected after the four digits.
        assert_eq!(consumed(r#""\udc00"#, 4096), (false, false, 7));
        // Raw control byte: rejected immediately after being read.
        assert_eq!(consumed("\"\u{1}x", 4096), (false, false, 2));
        // Per-token cap (strict): every raw token byte read AFTER the
        // opening quote counts (closing quote included, opening quote
        // excluded); byte #4097 after the opening quote is refused before it
        // is read, so a token is accepted iff its raw length <= 4097.
        assert_eq!(consumed(r#""abcde"#, 4), (false, false, 5));
    }

    /// The per-token cap counts PROBED RAW bytes (escape code bytes, all
    /// `\u` digits, surrogate lookahead included), not decoded size.
    /// Boundary regressions with fully escaped content: below/at the cap the
    /// token is valid and decodes exactly; above the cap the scan fails
    /// without altering behavior or letting an over-cap target-like string
    /// act as a source field.
    #[test]
    fn token_cap_counts_probed_raw_bytes_of_fully_escaped_tokens() {
        let probe = |raw: &str| {
            let mut budget = ScanBudget::new();
            let outcome = scan_json_string(raw, 0, MAX_SCAN_TOKEN_BYTES, &mut budget);
            (
                outcome.token.is_some(),
                outcome.budget_exhausted,
                MAX_SCAN_WORK_BYTES - budget.remaining,
            )
        };
        let escaped_a = |count: usize, extra: usize| {
            format!(r#""{}{}""#, r#"\u0041"#.repeat(count), "a".repeat(extra))
        };
        // Each `\u0041` is 6 probed bytes decoding to one 'A'. Token raw
        // Raw length T = 1 + 6*count + extra + 1. The cap counts every raw
        // token byte read AFTER the opening quote (closing quote included,
        // opening quote excluded): accepted iff after-opening bytes <= 4096
        // (T <= 4097); after-opening byte #4097 is refused before it is
        // read.
        // Below/at cap: T = 4094 -> valid, exact decode; T = 4097 -> the
        // maximum valid token (its close is after-opening byte #4096).
        let (valid, exhausted, probed) = probe(&escaped_a(682, 0));
        assert!(valid && !exhausted && probed == 4094);
        let decoded = scan_json_string(
            &escaped_a(682, 0),
            0,
            MAX_SCAN_TOKEN_BYTES,
            &mut ScanBudget::new(),
        );
        assert_eq!(
            decoded.token.as_ref().map(|(text, _)| text.clone()),
            Some("A".repeat(682))
        );
        let (valid, exhausted, probed) = probe(&escaped_a(682, 2));
        assert!(valid && !exhausted && probed == 4096);
        let (valid, exhausted, probed) = probe(&escaped_a(682, 3));
        assert!(valid && !exhausted && probed == 4097);
        // Above cap: T = 4098 (the close would be after-opening byte #4097)
        // -> failed; byte #4097 is refused before it is read (4097 probes
        // charged: opening + 4096 after-opening bytes).
        let (valid, exhausted, probed) = probe(&escaped_a(682, 4));
        assert!(!valid && !exhausted && probed == 4097);
        // Surrogate pattern: each `\ud83e\udd80` pair is 12 probed bytes
        // decoding to one astral char; T = 4094 -> below cap and exactly
        // decoded; T = 4106 -> over cap -> failed.
        let surrogate_token = |count: usize| format!(r#""{}""#, r#"\ud83e\udd80"#.repeat(count));
        let (valid, exhausted, probed) = probe(&surrogate_token(341));
        assert!(valid && !exhausted && probed == 4094);
        let decoded = scan_json_string(
            &surrogate_token(341),
            0,
            MAX_SCAN_TOKEN_BYTES,
            &mut ScanBudget::new(),
        );
        assert_eq!(
            decoded.token.as_ref().map(|(text, _)| text.chars().count()),
            Some(341)
        );
        let (valid, exhausted, probed) = probe(&surrogate_token(342));
        assert!(!valid && !exhausted && probed == 4097);
    }

    /// The budget meters exactly the documented cap and reports exhaustion at
    /// it, so traversal is bounded before any read.
    #[test]
    fn scan_budget_meters_exactly_to_the_documented_cap() {
        let mut budget = ScanBudget::new();
        assert_eq!(MAX_SCAN_WORK_BYTES, 1024 * 1024);
        assert!(!budget.exhausted());
        for _ in 0..MAX_SCAN_WORK_BYTES {
            budget.take();
        }
        assert!(budget.exhausted());
    }

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
    async fn meeting_scope_for_a_deleted_meeting_is_rejected_with_disclosure() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO meetings (id) VALUES (?)")
            .bind("meeting-1")
            .execute(&pool)
            .await
            .unwrap();

        let meeting = ChatRepository::get_or_create_scoped_conversation(
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
        assert_eq!(meeting.scope_kind, "meeting");

        let error = ChatRepository::get_or_create_scoped_conversation(
            &pool,
            &ChatScope {
                kind: ChatScopeKind::Meeting,
                key: "deleted-meeting".into(),
                data: None,
            },
            None,
        )
        .await
        .unwrap_err()
        .to_string();
        // The typed condition: exact machine code first, human disclosure
        // after the separator (the frontend matches the code by equality).
        assert!(
            error.starts_with(&format!("{DELETED_MEETING_THREAD_CODE}|")),
            "unexpected condition format: {error}"
        );
        assert!(error.contains("no longer available"));
        assert!(error.contains("may still quote deleted content"));
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_conversations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
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
