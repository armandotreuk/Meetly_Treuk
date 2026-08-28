CREATE INDEX IF NOT EXISTS retrieval_documents_by_meeting_lookup
    ON retrieval_documents(meeting_id, generation_id);
