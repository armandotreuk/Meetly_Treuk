-- Additive FTS5 mirror of meeting titles for the hybrid title channel
-- (HR-5.R8). Identity is the stable meeting string ID stored in the mirror,
-- never `meetings.rowid` and never a derived/hash key: the lookup joins live
-- `meetings` on `meeting_id` for current identity, title, and folder
-- metadata within the caller's read snapshot, and no meeting ID can
-- fail insertion, upgrade, or backfill (every schema-valid TEXT ID is
-- accepted verbatim).
--
-- Scope lives INSIDE the index: the `scope` column carries one token per
-- identity and one per direct folder ('m'/'f' + lowercase hex of the stable
-- string ID, a bijection that cannot collide or inject FTS5 syntax). A
-- constrained query ANDs column-filtered `{title}` terms with column-filtered
-- `{scope}` terms, so only scoped matches enter candidate ranking. This
-- constrains membership, not total query work: all in-scope matches are
-- scored before the caller retains its candidate limit. Descendant folders are handled by
-- expanding the subtree's direct folder IDs from the LIVE meeting_folders
-- table at query time, so folder re-parenting needs no mirror maintenance.
-- The scope column always carries exactly three tokens (identity, direct
-- folder when present, padding) so the ranking document length is uniform
-- across rows; ranking is additionally computed with an explicit per-query
-- bm25 weight of zero for the scope column, so scope-token document
-- frequencies can never influence title relevance or order.
--
-- Rowids are FTS5-assigned and carry NO meaning: deterministic title
-- selection/order is enforced by the caller, which ranks candidates by
-- (bm25 title score, meeting ID) over bounded rowid-window batches, making
-- the result independent of insertion history and collision-free by
-- construction (the meeting ID itself is the tie-break - a bijective,
-- collision-free identity for arbitrary TEXT IDs).
--
-- The triggers keep the mirror transactionally in sync with every meetings
-- insert, title update, folder move, and delete, whatever the writer, and
-- every rewrite writes the complete row so combined updates are
-- order-independent. Deleting a folder SETs its meetings' folder_id to NULL
-- through a foreign-key action that does not fire triggers, so the
-- BEFORE DELETE trigger on meeting_folders rewrites those rows first. A
-- meeting deleted mid-flight leaves zero mirror rows and can never be served
-- through a later read snapshot. A request already reading an older snapshot
-- remains internally consistent; downstream hydration/publication rechecks
-- current authority. This table is deliberately NOT part of meeting_fts:
-- no title rows enter the public lexical commands, the semantic document
-- set, or the vectors.
--
-- Amended in place during the uncommitted HR-5.R5/R6/R7/R8 working-tree
-- lifetime: no database outside this tree has applied an earlier revision,
-- so no repair migration is stacked.
CREATE VIRTUAL TABLE IF NOT EXISTS retrieval_title_fts USING fts5(
    meeting_id UNINDEXED,
    title,
    scope,
    tokenize = 'unicode61'
);

-- Backfill from the meetings that exist at migration time; `meetings.id` is
-- the primary key, so the backfill seeds exactly one row per meeting.
INSERT INTO retrieval_title_fts (meeting_id, title, scope)
SELECT id,
       title,
       'm' || lower(hex(id)) || CASE WHEN folder_id IS NULL THEN ' pad pad' ELSE ' f' || lower(hex(folder_id)) || ' pad' END
FROM meetings;

CREATE TRIGGER IF NOT EXISTS retrieval_title_fts_ad AFTER DELETE ON meetings BEGIN
    DELETE FROM retrieval_title_fts WHERE meeting_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS retrieval_title_fts_ai AFTER INSERT ON meetings BEGIN
    INSERT INTO retrieval_title_fts (meeting_id, title, scope)
    VALUES (NEW.id,
            NEW.title,
            'm' || lower(hex(NEW.id)) || CASE WHEN NEW.folder_id IS NULL THEN ' pad pad' ELSE ' f' || lower(hex(NEW.folder_id)) || ' pad' END);
END;

CREATE TRIGGER IF NOT EXISTS retrieval_title_fts_au AFTER UPDATE OF title ON meetings
WHEN OLD.title IS NOT NEW.title
BEGIN
    DELETE FROM retrieval_title_fts WHERE meeting_id = NEW.id;
    INSERT INTO retrieval_title_fts (meeting_id, title, scope)
    VALUES (NEW.id,
            NEW.title,
            'm' || lower(hex(NEW.id)) || CASE WHEN NEW.folder_id IS NULL THEN ' pad pad' ELSE ' f' || lower(hex(NEW.folder_id)) || ' pad' END);
END;

CREATE TRIGGER IF NOT EXISTS retrieval_title_fts_af AFTER UPDATE OF folder_id ON meetings
WHEN OLD.folder_id IS NOT NEW.folder_id
BEGIN
    DELETE FROM retrieval_title_fts WHERE meeting_id = NEW.id;
    INSERT INTO retrieval_title_fts (meeting_id, title, scope)
    VALUES (NEW.id,
            NEW.title,
            'm' || lower(hex(NEW.id)) || CASE WHEN NEW.folder_id IS NULL THEN ' pad pad' ELSE ' f' || lower(hex(NEW.folder_id)) || ' pad' END);
END;

CREATE TRIGGER IF NOT EXISTS retrieval_title_folder_ad BEFORE DELETE ON meeting_folders BEGIN
    -- The folder's direct meetings lose their folder_id through the ON DELETE
    -- SET NULL action, which does not fire triggers; rewrite their scope
    -- tokens first so the mirror never carries a stale folder token.
    DELETE FROM retrieval_title_fts
     WHERE meeting_id IN (SELECT id FROM meetings WHERE folder_id = OLD.id);
    INSERT INTO retrieval_title_fts (meeting_id, title, scope)
    SELECT m.id,
           m.title,
           'm' || lower(hex(m.id)) || ' pad pad'
    FROM meetings m WHERE m.folder_id = OLD.id;
END;
