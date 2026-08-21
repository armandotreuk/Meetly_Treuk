ALTER TABLE chat_conversations ADD COLUMN promoted_from_live_scope_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_chat_conversations_promoted_live_scope
ON chat_conversations(promoted_from_live_scope_key)
WHERE promoted_from_live_scope_key IS NOT NULL;
