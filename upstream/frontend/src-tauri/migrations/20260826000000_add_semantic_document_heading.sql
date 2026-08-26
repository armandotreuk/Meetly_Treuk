-- Forward-only follow-up to 20260825000000_add_semantic_retrieval.sql:
-- persists per-document section-heading provenance on canonical derived
-- documents. The column is nullable because this migration can run on user
-- databases that already hold published rows; those rows keep NULL (never
-- reconstructed heuristically) and regain headings when their meetings are
-- re-indexed. Staged payloads already carry the heading in their JSON.
ALTER TABLE retrieval_documents ADD COLUMN heading TEXT;

UPDATE retrieval_meeting_state
SET indexed_source_revision = 0,
    state = 'pending',
    next_attempt_at = NULL,
    updated_at = CURRENT_TIMESTAMP
WHERE state != 'failed'
  AND generation_id IN (
      SELECT generation_id FROM retrieval_generations WHERE state IN ('building', 'ready')
  )
  AND EXISTS (
      SELECT 1 FROM retrieval_documents d
      WHERE d.generation_id = retrieval_meeting_state.generation_id
        AND d.meeting_id = retrieval_meeting_state.meeting_id
        AND d.heading IS NULL
  );
