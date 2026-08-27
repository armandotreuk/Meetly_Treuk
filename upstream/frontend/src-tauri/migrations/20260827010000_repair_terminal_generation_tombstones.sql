-- Forward-only repair for the original meeting-delete trigger. Terminal
-- generations cannot publish new journal entries, so any journal tail beyond
-- their durable published bound is obsolete terminal state. Remove those tails
-- without advancing `published_change_id`; the bound is not being
-- synthetically acknowledged.
DELETE FROM retrieval_index_changes
WHERE change_id > (
          SELECT s.published_change_id
          FROM retrieval_index_state s
          JOIN retrieval_generations g ON g.generation_id = s.generation_id
          WHERE s.generation_id = retrieval_index_changes.generation_id
            AND g.state IN ('failed', 'retired')
      )
  AND retrieval_index_changes.generation_id IN (
      SELECT g.generation_id
      FROM retrieval_generations g
      WHERE g.state IN ('failed', 'retired')
  );

UPDATE retrieval_index_state
SET canonical_change_id = published_change_id,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE canonical_change_id > published_change_id
  AND generation_id IN (
      SELECT generation_id
      FROM retrieval_generations
      WHERE state IN ('failed', 'retired')
  );

DROP TRIGGER IF EXISTS retrieval_tombstone_before_meeting_delete;

CREATE TRIGGER retrieval_tombstone_before_meeting_delete
BEFORE DELETE ON meetings
BEGIN
    INSERT INTO retrieval_index_changes (generation_id, meeting_id, operation, source_revision, created_at)
    SELECT g.generation_id, OLD.id, 'delete', NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    FROM retrieval_generations g
    JOIN retrieval_index_state s ON s.generation_id = g.generation_id
    WHERE g.state IN ('building', 'ready');

    UPDATE retrieval_index_state
    SET canonical_change_id = (
            SELECT MAX(c.change_id) FROM retrieval_index_changes c
            WHERE c.generation_id = retrieval_index_state.generation_id
        ),
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE generation_id IN (
        SELECT g.generation_id
        FROM retrieval_generations g
        WHERE g.state IN ('building', 'ready')
    );
END;
