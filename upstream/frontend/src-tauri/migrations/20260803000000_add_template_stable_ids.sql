-- Keep the file/template registry identity separate from the database row ID.
-- Display names and schemas can change without changing a built-in template's
-- stable file ID.
ALTER TABLE templates ADD COLUMN stable_id TEXT;

CREATE INDEX IF NOT EXISTS idx_templates_stable_id
    ON templates(stable_id)
    WHERE stable_id IS NOT NULL;

-- Existing synchronized rows used the built-in display name as their only
-- identity. Backfill the names shipped before stable_id was introduced; rows
-- with a custom/unknown name remain unpaired instead of being guessed.
UPDATE templates SET stable_id = 'daily_standup'
 WHERE is_builtin = 1 AND stable_id IS NULL AND name = 'Daily Standup';
UPDATE templates SET stable_id = 'standard_meeting'
 WHERE is_builtin = 1 AND stable_id IS NULL AND name = 'Standard Meeting';
UPDATE templates SET stable_id = 'project_sync'
 WHERE is_builtin = 1 AND stable_id IS NULL AND name = 'Project Sync / Status Update';
UPDATE templates SET stable_id = 'retrospective'
 WHERE is_builtin = 1 AND stable_id IS NULL AND name = 'Retrospective (Agile)';
UPDATE templates SET stable_id = 'psychatric_session'
 WHERE is_builtin = 1 AND stable_id IS NULL AND name = 'Psychiatric Session Note (SOAP + AI Hybrid)';
UPDATE templates SET stable_id = 'sales_marketing_client_call'
 WHERE is_builtin = 1 AND stable_id IS NULL AND name = 'Client / Sales Meeting';
