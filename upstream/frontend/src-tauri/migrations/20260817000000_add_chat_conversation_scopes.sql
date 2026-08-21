ALTER TABLE chat_conversations ADD COLUMN scope_kind TEXT;
ALTER TABLE chat_conversations ADD COLUMN scope_key TEXT;
ALTER TABLE chat_conversations ADD COLUMN scope_data TEXT;

UPDATE chat_conversations
SET
    scope_kind = CASE
        WHEN meeting_id IS NOT NULL THEN 'meeting'
        WHEN origin = 'global' THEN 'all'
        ELSE 'orphaned_meeting'
    END,
    scope_key = CASE
        WHEN meeting_id IS NOT NULL THEN meeting_id
        WHEN origin = 'global' THEN 'all'
        ELSE id
    END;

CREATE INDEX IF NOT EXISTS idx_chat_conversations_scope_lookup
    ON chat_conversations(scope_kind, scope_key, updated_at DESC, created_at DESC);

CREATE TRIGGER IF NOT EXISTS chat_conversations_orphan_deleted_meeting
AFTER UPDATE OF meeting_id ON chat_conversations
WHEN OLD.meeting_id IS NOT NULL AND NEW.meeting_id IS NULL AND NEW.origin != 'global'
BEGIN
    UPDATE chat_conversations
    SET scope_kind = 'orphaned_meeting'
    WHERE id = NEW.id;
END;
