-- Semantic retrieval persistence foundation (hybrid RAG, Sprint 2 Task 2.1).
-- Strictly additive: primary meeting tables and the FTS5 definition/refresh
-- behavior are untouched. This migration only enqueues work; it never
-- tokenizes or runs model inference.

CREATE TABLE IF NOT EXISTS retrieval_models (
    -- Immutable identity of one bundled embedding model. Quantized encodings
    -- MUST record their dequantization parameters here so a vector is never
    -- interpreted under the wrong scale. There is deliberately no fixed
    -- byte-width CHECK on vectors anywhere in this schema; validation is
    -- encoding-aware and lives at the repository boundary.
    model_id TEXT PRIMARY KEY,
    dimensions INTEGER NOT NULL CHECK (dimensions > 0),
    vector_encoding TEXT NOT NULL,
    chunker_version INTEGER NOT NULL,
    dequantization_scale REAL,
    dequantization_zero_point INTEGER,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS retrieval_generations (
    generation_id TEXT PRIMARY KEY,
    model_id TEXT NOT NULL REFERENCES retrieval_models(model_id),
    state TEXT NOT NULL CHECK (state IN ('building', 'ready', 'failed', 'retired')),
    document_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    activated_at TEXT,
    retired_at TEXT
);

CREATE TABLE IF NOT EXISTS retrieval_active_model (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    generation_id TEXT NOT NULL REFERENCES retrieval_generations(generation_id),
    activated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS search_source_state (
    meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
    source_revision INTEGER NOT NULL DEFAULT 1,
    fts_projection_revision INTEGER NOT NULL DEFAULT 1,
    fts_indexed_revision INTEGER NOT NULL DEFAULT 0,
    changed_at TEXT NOT NULL,
    fts_attempt_count INTEGER NOT NULL DEFAULT 0,
    fts_next_attempt_at TEXT,
    fts_last_error TEXT
);

CREATE INDEX IF NOT EXISTS search_source_state_fts_due
    ON search_source_state(fts_next_attempt_at)
    WHERE fts_indexed_revision < fts_projection_revision;

CREATE TABLE IF NOT EXISTS retrieval_documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    generation_id TEXT NOT NULL REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE,
    document_id TEXT NOT NULL,
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL,
    source_start_id TEXT,
    source_end_id TEXT,
    source_template_id TEXT,
    ordinal INTEGER NOT NULL,
    content TEXT NOT NULL,
    content_hash BLOB NOT NULL,
    dimensions INTEGER NOT NULL CHECK (dimensions > 0),
    vector_encoding TEXT NOT NULL,
    vector BLOB NOT NULL,
    source_revision INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (generation_id, document_id)
);

CREATE INDEX IF NOT EXISTS retrieval_documents_by_meeting
    ON retrieval_documents(generation_id, meeting_id);

CREATE TABLE IF NOT EXISTS retrieval_document_staging (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL,
    generation_id TEXT NOT NULL REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE,
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    source_revision INTEGER NOT NULL,
    document_id TEXT NOT NULL,
    payload BLOB NOT NULL,
    UNIQUE (job_id, document_id)
);

CREATE INDEX IF NOT EXISTS retrieval_document_staging_by_generation
    ON retrieval_document_staging(generation_id, meeting_id);

CREATE TABLE IF NOT EXISTS retrieval_meeting_state (
    generation_id TEXT NOT NULL REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE,
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    indexed_source_revision INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL CHECK (state IN ('pending', 'ready', 'retry', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT,
    last_error TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (generation_id, meeting_id)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS retrieval_meeting_state_due
    ON retrieval_meeting_state(generation_id, state, next_attempt_at);

CREATE TABLE IF NOT EXISTS retrieval_index_state (
    generation_id TEXT PRIMARY KEY REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE,
    backend TEXT NOT NULL,
    state TEXT NOT NULL,
    document_count INTEGER NOT NULL,
    canonical_change_id INTEGER NOT NULL DEFAULT 0,
    published_change_id INTEGER NOT NULL DEFAULT 0,
    sidecar_hash BLOB,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS retrieval_index_changes (
    -- Deliberately free of foreign keys: deletion tombstones must survive the
    -- meeting delete cascade until every publisher acknowledges them.
    change_id INTEGER PRIMARY KEY AUTOINCREMENT,
    generation_id TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
    source_revision INTEGER,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS retrieval_index_changes_replay
    ON retrieval_index_changes(generation_id, change_id);

-- Seed durable state for every meeting that exists at migration time.
-- indexed revision starts behind the projection so the first worker pass
-- durably verifies (and heals if needed) the previously best-effort FTS data.
INSERT OR IGNORE INTO search_source_state (meeting_id, changed_at)
SELECT id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM meetings;

-- ---------------------------------------------------------------------------
-- Durable mutation tracking. Content mutations advance BOTH the semantic
-- source revision and the FTS projection revision; folder metadata changes
-- advance ONLY the FTS projection revision (no re-embedding). Repeated
-- changes coalesce into one row per meeting with a monotonically advancing
-- revision. The EXISTS guard makes the bump a safe no-op while a meeting
-- delete cascade is removing its dependent rows.
-- ---------------------------------------------------------------------------

CREATE TRIGGER IF NOT EXISTS retrieval_meeting_insert
AFTER INSERT ON meetings
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    VALUES (NEW.id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;

    -- A meeting created after a generation was registered owes that
    -- generation independent work, exactly like registration seeding does for
    -- pre-existing meetings. Only live generations are seeded: 'building'
    -- shadows and 'ready' active generations process work; 'failed' and
    -- 'retired' are terminal and never run a worker again.
    INSERT INTO retrieval_meeting_state (generation_id, meeting_id, state, updated_at)
    SELECT g.generation_id, NEW.id, 'pending', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    FROM retrieval_generations g
    WHERE g.state IN ('building', 'ready');
END;

CREATE TRIGGER IF NOT EXISTS retrieval_meeting_title_update
AFTER UPDATE OF title ON meetings
WHEN OLD.title IS NOT NEW.title
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    VALUES (NEW.id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

CREATE TRIGGER IF NOT EXISTS retrieval_transcript_insert
AFTER INSERT ON transcripts
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT NEW.meeting_id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE EXISTS (SELECT 1 FROM meetings WHERE id = NEW.meeting_id)
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

-- Any transcript update can change chunk windows (audio timing orders them),
-- so it always dirties both revisions.
CREATE TRIGGER IF NOT EXISTS retrieval_transcript_update
AFTER UPDATE ON transcripts
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT NEW.meeting_id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE EXISTS (SELECT 1 FROM meetings WHERE id = NEW.meeting_id)
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

CREATE TRIGGER IF NOT EXISTS retrieval_transcript_delete
AFTER DELETE ON transcripts
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT OLD.meeting_id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE EXISTS (SELECT 1 FROM meetings WHERE id = OLD.meeting_id)
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

CREATE TRIGGER IF NOT EXISTS retrieval_summary_insert
AFTER INSERT ON summary_processes
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT NEW.meeting_id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE EXISTS (SELECT 1 FROM meetings WHERE id = NEW.meeting_id)
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

-- Only an actual result change counts; status/error churn on the same result
-- (reset, failed restore identical to current) does not dirty anything.
CREATE TRIGGER IF NOT EXISTS retrieval_summary_result_update
AFTER UPDATE OF result ON summary_processes
WHEN OLD.result IS NOT NEW.result
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT NEW.meeting_id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE EXISTS (SELECT 1 FROM meetings WHERE id = NEW.meeting_id)
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

CREATE TRIGGER IF NOT EXISTS retrieval_summary_delete
AFTER DELETE ON summary_processes
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT OLD.meeting_id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE EXISTS (SELECT 1 FROM meetings WHERE id = OLD.meeting_id)
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

CREATE TRIGGER IF NOT EXISTS retrieval_notes_insert
AFTER INSERT ON meeting_notes
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT NEW.meeting_id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE EXISTS (SELECT 1 FROM meetings WHERE id = NEW.meeting_id)
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

CREATE TRIGGER IF NOT EXISTS retrieval_notes_update
AFTER UPDATE OF notes_markdown, notes_json ON meeting_notes
WHEN OLD.notes_markdown IS NOT NEW.notes_markdown
    OR OLD.notes_json IS NOT NEW.notes_json
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT NEW.meeting_id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE EXISTS (SELECT 1 FROM meetings WHERE id = NEW.meeting_id)
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

CREATE TRIGGER IF NOT EXISTS retrieval_notes_delete
AFTER DELETE ON meeting_notes
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT OLD.meeting_id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE EXISTS (SELECT 1 FROM meetings WHERE id = OLD.meeting_id)
    ON CONFLICT(meeting_id) DO UPDATE SET
        source_revision = source_revision + 1,
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

-- Folder metadata never requires re-embedding: FTS stores folder ID/name, so
-- these advance ONLY the FTS projection revision. A folder parent move keeps
-- immediate folder IDs/names unchanged and therefore touches nothing.

CREATE TRIGGER IF NOT EXISTS retrieval_meeting_folder_update
AFTER UPDATE OF folder_id ON meetings
WHEN OLD.folder_id IS NOT NEW.folder_id
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    VALUES (NEW.id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    ON CONFLICT(meeting_id) DO UPDATE SET
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

CREATE TRIGGER IF NOT EXISTS retrieval_folder_rename
AFTER UPDATE OF name ON meeting_folders
WHEN OLD.name IS NOT NEW.name
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    FROM meetings
    WHERE folder_id = OLD.id
    ON CONFLICT(meeting_id) DO UPDATE SET
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

CREATE TRIGGER IF NOT EXISTS retrieval_folder_delete
AFTER DELETE ON meeting_folders
BEGIN
    INSERT INTO search_source_state (meeting_id, changed_at)
    SELECT id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    FROM meetings
    WHERE folder_id = OLD.id
    ON CONFLICT(meeting_id) DO UPDATE SET
        fts_projection_revision = fts_projection_revision + 1,
        changed_at = excluded.changed_at;
END;

-- Deletion tombstones: before the meeting disappears and its derived rows
-- cascade away, append a non-FK journal entry for every generation holding
-- durable index state -- the generation/index-state relationship is the source
-- of truth for "can retain or replay derived state", not the lifecycle state,
-- so a retired generation awaiting GC keeps receiving tombstones until its
-- publisher acknowledges them -- and advance each affected generation's
-- canonical change ID to its tombstone atomically. This runs inside the
-- caller's delete transaction, so semantic publication is observably behind
-- canonical immediately after commit.
CREATE TRIGGER IF NOT EXISTS retrieval_tombstone_before_meeting_delete
BEFORE DELETE ON meetings
BEGIN
    INSERT INTO retrieval_index_changes (generation_id, meeting_id, operation, source_revision, created_at)
    SELECT g.generation_id, OLD.id, 'delete', NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    FROM retrieval_generations g
    WHERE EXISTS (
        SELECT 1 FROM retrieval_index_state s WHERE s.generation_id = g.generation_id
    );

    UPDATE retrieval_index_state
    SET canonical_change_id = (
            SELECT MAX(c.change_id) FROM retrieval_index_changes c
            WHERE c.generation_id = retrieval_index_state.generation_id
        ),
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;
