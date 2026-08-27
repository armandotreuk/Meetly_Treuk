-- Forward-only timestamp normalization (hybrid RAG Sprint 2 remediation).
--
-- 20260826000000_add_semantic_document_heading.sql wrote CURRENT_TIMESTAMP
-- ('YYYY-MM-DD HH:MM:SS') into retrieval_meeting_state.updated_at, while every
-- Rust writer uses RFC 3339. That migration has shipped, so its text is
-- immutable: editing it changes the sqlx checksum and fails startup with
-- VersionMismatch on any database that already applied it. The value is
-- corrected here instead.
--
-- Nothing parses this column today, so this is a consistency fix rather than a
-- behavior change. The predicate matches only the exact CURRENT_TIMESTAMP
-- shape, so the statement is idempotent and leaves RFC 3339 values untouched.

UPDATE retrieval_meeting_state
SET updated_at = replace(updated_at, ' ', 'T') || '.000Z'
WHERE updated_at LIKE '____-__-__ __:__:__';
