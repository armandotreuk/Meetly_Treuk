-- Multi-Template Summaries: N summaries per meeting, keyed by (meeting_id, template_id).
-- Previously summary_processes had PK (meeting_id) alone, so a meeting could hold
-- at most one summary. We widen the key to (meeting_id, template_id) so each
-- template produces an independent row.
--
-- Strategy: SQLite cannot ALTER PRIMARY KEY in place, so we rebuild the table
-- (PRAGMA foreign_keys=off + CREATE new + INSERT copy + DROP old + RENAME).
-- Existing rows are backfilled with template_id='legacy' so users keep their
-- pre-migration summary under an explicit, honest sentinel id (frontend
-- renders it as "Summary (original)" and regenerating it creates a NEW row
-- rather than overwriting the legacy one).
--
-- result_backup / result_backup_timestamp are preserved (4 queries in
-- repositories/summary.rs depend on them for restore-on-fail/cancel).

PRAGMA foreign_keys=off;

CREATE TABLE IF NOT EXISTS summary_processes_new (
    meeting_id TEXT NOT NULL,
    template_id TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    error TEXT,
    result TEXT,
    start_time TEXT,
    end_time TEXT,
    chunk_count INTEGER DEFAULT 0,
    processing_time REAL DEFAULT 0.0,
    metadata TEXT,
    result_backup TEXT,
    result_backup_timestamp TEXT,
    PRIMARY KEY (meeting_id, template_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

-- Backfill: every pre-migration summary becomes 'legacy'.
-- 'legacy' is a reserved sentinel — templates loader never yields an id 'legacy',
-- so it cannot collide with a real template row.
INSERT INTO summary_processes_new
    (meeting_id, template_id, status, created_at, updated_at, error, result,
     start_time, end_time, chunk_count, processing_time, metadata,
     result_backup, result_backup_timestamp)
SELECT meeting_id, 'legacy', status, created_at, updated_at, error, result,
       start_time, end_time, chunk_count, processing_time, metadata,
       result_backup, result_backup_timestamp
FROM summary_processes;

DROP TABLE summary_processes;

ALTER TABLE summary_processes_new RENAME TO summary_processes;

-- Forward lookup by meeting is common (list summaries for a meeting).
CREATE INDEX IF NOT EXISTS idx_summary_processes_meeting
    ON summary_processes(meeting_id);

PRAGMA foreign_keys=on;
