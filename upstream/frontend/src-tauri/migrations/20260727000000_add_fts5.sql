-- FTS5 full-text search index over transcripts, summaries, and notes.
-- One row per searchable text chunk; folder_name is indexed so the
-- search operator folder:"name" works via FTS5 MATCH.

CREATE VIRTUAL TABLE IF NOT EXISTS meeting_fts USING fts5(
    meeting_id UNINDEXED,
    chunk_type UNINDEXED,      -- 'transcript' | 'summary' | 'note'
    chunk_id UNINDEXED,
    text,
    speaker UNINDEXED,
    timestamp_label UNINDEXED,
    folder_id UNINDEXED,
    folder_name,
    tokenize = 'unicode61'
);

-- Populate from existing transcripts
INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
SELECT
    t.meeting_id,
    'transcript',
    t.id,
    t.transcript,
    t.speaker,
    t.timestamp,
    m.folder_id,
    COALESCE(f.name, '')
FROM transcripts t
JOIN meetings m ON t.meeting_id = m.id
LEFT JOIN meeting_folders f ON m.folder_id = f.id
WHERE t.transcript IS NOT NULL AND t.transcript != '';

-- Populate from existing summaries
INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
SELECT
    sp.meeting_id,
    'summary',
    sp.meeting_id || ':' || sp.template_id,
    json_extract(sp.result, '$.markdown'),
    NULL,
    NULL,
    m.folder_id,
    COALESCE(f.name, '')
FROM summary_processes sp
JOIN meetings m ON sp.meeting_id = m.id
LEFT JOIN meeting_folders f ON m.folder_id = f.id
WHERE sp.result IS NOT NULL
  AND json_extract(sp.result, '$.markdown') IS NOT NULL
  AND json_extract(sp.result, '$.markdown') != '';

-- Populate from existing notes
INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
SELECT
    mn.meeting_id,
    'note',
    mn.meeting_id,
    mn.notes_markdown,
    NULL,
    NULL,
    m.folder_id,
    COALESCE(f.name, '')
FROM meeting_notes mn
JOIN meetings m ON mn.meeting_id = m.id
LEFT JOIN meeting_folders f ON m.folder_id = f.id
WHERE mn.notes_markdown IS NOT NULL AND mn.notes_markdown != '';
