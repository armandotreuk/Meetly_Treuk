-- Add separate Chat LLM config columns to settings table.
-- Chat and Summary now have independent provider/model selections.
-- API keys remain global (per-provider, shared across features).
ALTER TABLE settings ADD COLUMN chatProvider TEXT;
ALTER TABLE settings ADD COLUMN chatModel TEXT;
ALTER TABLE settings ADD COLUMN chatOllamaEndpoint TEXT;
