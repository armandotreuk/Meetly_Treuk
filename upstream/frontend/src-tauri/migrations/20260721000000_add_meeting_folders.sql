-- Folders (logical grouping, multi-level). folder_path on disk (audio.mp4 / notes.md / transcripts.json)
-- remains untouched; folders exist only in DB.
CREATE TABLE IF NOT EXISTS meeting_folders (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    parent_id TEXT REFERENCES meeting_folders(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL
);

ALTER TABLE meetings ADD COLUMN folder_id TEXT REFERENCES meeting_folders(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_meetings_folder_id ON meetings(folder_id);
CREATE INDEX IF NOT EXISTS idx_folders_parent ON meeting_folders(parent_id);
