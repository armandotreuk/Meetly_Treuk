-- SQLx runs this SQLite migration in one transaction. On failure it rolls back;
-- after success, restore a pre-upgrade database backup rather than attempting to split merged threads.
UPDATE chat_conversations
SET scope_kind = 'meeting', scope_key = meeting_id, scope_data = NULL
WHERE meeting_id IS NOT NULL;

UPDATE chat_conversations
SET scope_kind = 'all', scope_key = 'all', scope_data = NULL
WHERE meeting_id IS NULL AND origin = 'global';

UPDATE chat_conversations
SET scope_kind = 'orphaned_meeting', scope_key = id, scope_data = NULL
WHERE meeting_id IS NULL
  AND origin != 'global'
  AND (
      scope_kind IS NULL
      OR trim(scope_kind) = ''
      OR scope_key IS NULL
      OR trim(scope_key) = ''
      OR scope_kind = 'orphaned_meeting'
  );

WITH ranked AS (
    SELECT
        id,
        FIRST_VALUE(id) OVER (
            PARTITION BY scope_kind, scope_key, COALESCE(scope_data, '')
            ORDER BY promoted_from_live_scope_key IS NULL, updated_at DESC, created_at DESC, id ASC
        ) AS canonical_id,
        ROW_NUMBER() OVER (
            PARTITION BY scope_kind, scope_key, COALESCE(scope_data, '')
            ORDER BY promoted_from_live_scope_key IS NULL, updated_at DESC, created_at DESC, id ASC
        ) AS position
    FROM chat_conversations
)
UPDATE chat_messages
SET conversation_id = (
    SELECT canonical_id FROM ranked WHERE ranked.id = chat_messages.conversation_id
)
WHERE conversation_id IN (SELECT id FROM ranked WHERE position > 1);

WITH ranked AS (
    SELECT
        id,
        FIRST_VALUE(id) OVER (
            PARTITION BY scope_kind, scope_key, COALESCE(scope_data, '')
            ORDER BY promoted_from_live_scope_key IS NULL, updated_at DESC, created_at DESC, id ASC
        ) AS canonical_id,
        ROW_NUMBER() OVER (
            PARTITION BY scope_kind, scope_key, COALESCE(scope_data, '')
            ORDER BY promoted_from_live_scope_key IS NULL, updated_at DESC, created_at DESC, id ASC
        ) AS position
    FROM chat_conversations
)
UPDATE chat_conversations
SET updated_at = (
    SELECT MAX(duplicate.updated_at)
    FROM chat_conversations AS duplicate
    WHERE duplicate.scope_kind = chat_conversations.scope_kind
      AND duplicate.scope_key = chat_conversations.scope_key
      AND COALESCE(duplicate.scope_data, '') = COALESCE(chat_conversations.scope_data, '')
)
WHERE id IN (SELECT canonical_id FROM ranked WHERE position > 1);

WITH ranked AS (
    SELECT
        id,
        ROW_NUMBER() OVER (
            PARTITION BY scope_kind, scope_key, COALESCE(scope_data, '')
            ORDER BY promoted_from_live_scope_key IS NULL, updated_at DESC, created_at DESC, id ASC
        ) AS position
    FROM chat_conversations
)
DELETE FROM chat_conversations
WHERE id IN (SELECT id FROM ranked WHERE position > 1);
