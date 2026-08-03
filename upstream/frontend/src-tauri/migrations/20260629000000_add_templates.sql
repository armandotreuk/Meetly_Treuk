-- F1: Custom summary templates
CREATE TABLE IF NOT EXISTS templates (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  name         TEXT NOT NULL,
  description  TEXT,
  schema_json  TEXT NOT NULL,          -- validated against upstream template schema
  is_builtin   INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for faster lookups
CREATE INDEX IF NOT EXISTS idx_templates_is_builtin ON templates(is_builtin);

-- Seed builtin templates from bundled JSON files
-- Note: These are inserted with is_builtin=1 so they appear read-only in UI
-- The JSON content is read from Tauri resources at runtime and synced on startup

-- Trigger to auto-update updated_at
CREATE TRIGGER IF NOT EXISTS update_templates_updated_at
AFTER UPDATE ON templates
FOR EACH ROW
BEGIN
  UPDATE templates SET updated_at = datetime('now') WHERE id = NEW.id;
END;