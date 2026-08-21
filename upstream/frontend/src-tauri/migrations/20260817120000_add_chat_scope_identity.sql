CREATE UNIQUE INDEX IF NOT EXISTS idx_chat_conversations_scope_identity
ON chat_conversations(scope_kind, scope_key, COALESCE(scope_data, ''));
