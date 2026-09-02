use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::cmp::Ordering;
use std::sync::LazyLock;
use tracing::info;

use crate::database::repositories::folder::FolderRepository;

static FOLDER_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"(?i)folder:"([^"]*)""#).unwrap());

pub(crate) const SQLITE_BIND_CHUNK: usize = 400;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FtsSearchResult {
    pub meeting_id: String,
    pub meeting_title: String,
    #[serde(rename = "chunkType")]
    pub chunk_type: String,
    #[serde(rename = "chunkId")]
    pub chunk_id: String,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    #[serde(rename = "timestampLabel", skip_serializing_if = "Option::is_none")]
    pub timestamp_label: Option<String>,
    #[serde(rename = "folderId", skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(rename = "folderName")]
    pub folder_name: String,
    pub rank: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum MatchMode {
    #[default]
    Or,
    And,
    Phrase,
}

/// Parsed search query with optional folder filter extracted from
/// `folder:"Name"` prefix syntax.
struct ParsedQuery {
    fts_query: String,
    folder_id: Option<String>,
    invalid_folder: bool,
}

#[derive(Clone, Copy)]
enum SnippetMode {
    Highlighted,
    Plain,
}

/// Extract `folder:"..."` from the raw query, resolve it to a folder_id,
/// and return the cleaned FTS query + optional folder_id filter.
///
/// ponytail: naive parse — finds the first `folder:"` pair. If the user
/// writes multiple folder operators, only the first one takes effect.
/// FTS5 column filters (`folder_name:"..."`) would be more natural but
/// require the column to be indexed (it is), so this is a convenience
/// alias that also resolves the folder name to its id for a WHERE filter.
async fn parse_query(pool: &SqlitePool, raw: &str) -> Result<ParsedQuery, sqlx::Error> {
    let fts_query = strip_folder_operators(raw.to_string());
    let folder_names = folder_operator_names(raw);
    if folder_names.is_empty() {
        return Ok(ParsedQuery {
            fts_query,
            folder_id: None,
            invalid_folder: false,
        });
    }
    let mut folder_id = None;
    let mut invalid_folder = false;
    for (index, name) in folder_names.into_iter().enumerate() {
        let resolved = FolderRepository::get_by_name(pool, &name)
            .await?
            .map(|folder| folder.id);
        if resolved.is_none() {
            invalid_folder = true;
        }
        if index == 0 {
            folder_id = resolved;
        }
    }
    Ok(ParsedQuery {
        fts_query,
        invalid_folder,
        folder_id,
    })
}

/// Splits the first `folder:"..."` operator out of a raw query, returning the
/// remaining query text (text before and after the operator, joined) and the
/// operator's folder name. Direct FTS search methods keep parsing the
/// operator themselves; this helper exists for scope normalization in the
/// retrieval service.
pub(crate) fn split_folder_operator(raw: &str) -> (String, Option<String>) {
    if let Some(caps) = FOLDER_RE.captures(raw) {
        let whole = caps.get(0).unwrap();
        let name = caps.get(1).unwrap().as_str().to_string();
        let before = raw[..whole.start()].trim();
        let after = raw[whole.end()..].trim();
        let rest = match (before.is_empty(), after.is_empty()) {
            (true, _) => after.to_string(),
            (false, true) => before.to_string(),
            (false, false) => format!("{before} {after}"),
        };
        (rest, Some(name))
    } else {
        (raw.trim().to_string(), None)
    }
}

pub(crate) fn folder_operator_names(raw: &str) -> Vec<String> {
    FOLDER_RE
        .captures_iter(raw)
        .filter_map(|caps| caps.get(1).map(|name| name.as_str().to_string()))
        .collect()
}

pub(crate) fn strip_folder_operators(mut raw: String) -> String {
    loop {
        let (rest, folder_name) = split_folder_operator(&raw);
        if folder_name.is_none() {
            return rest;
        }
        raw = rest;
    }
}

pub struct FtsRepository;

impl FtsRepository {
    /// Full-text search across transcripts, summaries, and notes.
    /// Supports `folder:"name"` operator in the query string.
    pub async fn search(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        meeting_id: Option<&str>,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        Self::search_with_mode(pool, raw_query, limit, meeting_id, MatchMode::Or).await
    }

    pub async fn search_with_mode(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        meeting_id: Option<&str>,
        match_mode: MatchMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        Self::search_with_mode_inner(
            pool,
            raw_query,
            limit,
            meeting_id,
            match_mode,
            SnippetMode::Highlighted,
        )
        .await
    }

    pub async fn search_with_mode_plain(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        meeting_id: Option<&str>,
        match_mode: MatchMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        Self::search_with_mode_inner(
            pool,
            raw_query,
            limit,
            meeting_id,
            match_mode,
            SnippetMode::Plain,
        )
        .await
    }

    async fn search_with_mode_inner(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        meeting_id: Option<&str>,
        match_mode: MatchMode,
        snippet_mode: SnippetMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        if raw_query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let parsed = parse_query(pool, raw_query).await?;

        if parsed.invalid_folder || parsed.fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = fetch_search_rows(
            pool,
            &parsed.fts_query,
            limit,
            meeting_id,
            parsed.folder_id.as_deref(),
            None,
            match_mode,
            snippet_mode,
        )
        .await?;
        expand_transcript_segments(
            pool,
            &mut results,
            200,
            matches!(snippet_mode, SnippetMode::Highlighted),
        )
        .await?;

        info!(
            "FTS search: query_len={} mode={:?} folder_scoped={} results={}",
            parsed.fts_query.chars().count(),
            match_mode,
            parsed.folder_id.is_some(),
            results.len()
        );
        Ok(results)
    }

    pub async fn search_transcripts_with_mode(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        meeting_id: &str,
        match_mode: MatchMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        Self::search_transcripts_with_mode_inner(
            pool,
            raw_query,
            limit,
            meeting_id,
            match_mode,
            SnippetMode::Highlighted,
        )
        .await
    }

    async fn search_transcripts_with_mode_inner(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        meeting_id: &str,
        match_mode: MatchMode,
        snippet_mode: SnippetMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        if raw_query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let parsed = parse_query(pool, raw_query).await?;
        if parsed.invalid_folder || parsed.fts_query.is_empty() {
            return Ok(Vec::new());
        }
        fetch_search_rows(
            pool,
            &parsed.fts_query,
            limit,
            Some(meeting_id),
            parsed.folder_id.as_deref(),
            Some("transcript"),
            match_mode,
            snippet_mode,
        )
        .await
    }

    pub async fn search_with_folder_id(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        folder_id: &str,
        match_mode: MatchMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        Self::search_with_folder_id_inner(
            pool,
            raw_query,
            limit,
            folder_id,
            match_mode,
            SnippetMode::Highlighted,
        )
        .await
    }

    pub async fn search_with_folder_id_plain(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        folder_id: &str,
        match_mode: MatchMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        Self::search_with_folder_id_inner(
            pool,
            raw_query,
            limit,
            folder_id,
            match_mode,
            SnippetMode::Plain,
        )
        .await
    }

    async fn search_with_folder_id_inner(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        folder_id: &str,
        match_mode: MatchMode,
        snippet_mode: SnippetMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        if raw_query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let parsed = parse_query(pool, raw_query).await?;
        if parsed.invalid_folder
            || parsed
                .folder_id
                .as_deref()
                .is_some_and(|query_folder| query_folder != folder_id)
            || parsed.fts_query.is_empty()
        {
            return Ok(Vec::new());
        }
        let mut results = fetch_search_rows(
            pool,
            &parsed.fts_query,
            limit,
            None,
            Some(folder_id),
            None,
            match_mode,
            snippet_mode,
        )
        .await?;
        expand_transcript_segments(
            pool,
            &mut results,
            200,
            matches!(snippet_mode, SnippetMode::Highlighted),
        )
        .await?;
        Ok(results)
    }

    pub async fn search_with_folder_ids(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        folder_ids: &[String],
        match_mode: MatchMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        Self::search_with_folder_ids_inner(
            pool,
            raw_query,
            limit,
            folder_ids,
            match_mode,
            SnippetMode::Highlighted,
        )
        .await
    }

    async fn search_with_folder_ids_inner(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        folder_ids: &[String],
        match_mode: MatchMode,
        snippet_mode: SnippetMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        if raw_query.trim().is_empty() || folder_ids.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let parsed = parse_query(pool, raw_query).await?;
        if parsed.invalid_folder {
            return Ok(Vec::new());
        }
        if let Some(query_folder) = parsed.folder_id.as_deref() {
            if !folder_ids.iter().any(|folder_id| folder_id == query_folder) {
                return Ok(Vec::new());
            }
        }
        let safe_query = sanitize_fts_query(&parsed.fts_query, match_mode);
        if safe_query.is_empty() {
            return Ok(Vec::new());
        }

        let cap = limit as usize;
        let mut results = Vec::with_capacity(cap.min(SQLITE_BIND_CHUNK));
        for folder_chunk in folder_ids.chunks(SQLITE_BIND_CHUNK) {
            let rows =
                fetch_folder_rows(pool, &safe_query, limit, folder_chunk, snippet_mode).await?;
            for result in rows_to_results(rows) {
                retain_best_result(&mut results, result, cap);
            }
        }
        results.sort_by(compare_results);
        expand_transcript_segments(
            pool,
            &mut results,
            200,
            matches!(snippet_mode, SnippetMode::Highlighted),
        )
        .await?;
        Ok(results)
    }

    /// FTS search restricted to an explicit meeting-ID allow-list. Mirror of
    /// [`Self::search_with_folder_ids`] for allowed-ID scopes (snapshots,
    /// today, and the retrieval service's `AllowedMeetingIds` scope).
    pub async fn search_with_meeting_ids(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        meeting_ids: &[String],
        match_mode: MatchMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        Self::search_with_meeting_ids_inner(
            pool,
            raw_query,
            limit,
            meeting_ids,
            match_mode,
            SnippetMode::Highlighted,
        )
        .await
    }

    pub async fn search_with_meeting_ids_plain(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        meeting_ids: &[String],
        match_mode: MatchMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        Self::search_with_meeting_ids_inner(
            pool,
            raw_query,
            limit,
            meeting_ids,
            match_mode,
            SnippetMode::Plain,
        )
        .await
    }

    async fn search_with_meeting_ids_inner(
        pool: &SqlitePool,
        raw_query: &str,
        limit: u32,
        meeting_ids: &[String],
        match_mode: MatchMode,
        snippet_mode: SnippetMode,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        if raw_query.trim().is_empty() || meeting_ids.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let parsed = parse_query(pool, raw_query).await?;
        if parsed.invalid_folder || parsed.fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let safe_query = sanitize_fts_query(&parsed.fts_query, match_mode);
        if safe_query.is_empty() {
            return Ok(Vec::new());
        }
        let mut results = Vec::with_capacity(limit as usize);
        for meeting_chunk in meeting_ids.chunks(SQLITE_BIND_CHUNK) {
            let rows = fetch_meeting_search_rows(
                pool,
                &safe_query,
                limit,
                meeting_chunk,
                parsed.folder_id.as_deref(),
                snippet_mode,
            )
            .await?;
            for result in rows_to_results(rows) {
                retain_best_result(&mut results, result, limit as usize);
            }
        }
        results.sort_by(compare_results);
        expand_transcript_segments(
            pool,
            &mut results,
            200,
            matches!(snippet_mode, SnippetMode::Highlighted),
        )
        .await?;
        Ok(results)
    }

    pub async fn get_by_meeting_ids(
        pool: &SqlitePool,
        meeting_ids: &[String],
        per_meeting_limit: u32,
        total_limit: u32,
    ) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
        if meeting_ids.is_empty() || per_meeting_limit == 0 || total_limit == 0 {
            return Ok(Vec::new());
        }
        let mut results = Vec::with_capacity(total_limit as usize);
        for meeting_chunk in meeting_ids.chunks(SQLITE_BIND_CHUNK) {
            for result in rows_to_results(
                fetch_meeting_rows(pool, meeting_chunk, per_meeting_limit, total_limit).await?,
            ) {
                retain_earliest_result(&mut results, result, total_limit as usize);
            }
        }
        results.sort_by(compare_hydration_results);
        let mut by_meeting: std::collections::HashMap<String, Vec<FtsSearchResult>> =
            std::collections::HashMap::new();
        for result in results {
            by_meeting
                .entry(result.meeting_id.clone())
                .or_default()
                .push(result);
        }
        Ok(meeting_ids
            .iter()
            .flat_map(|meeting_id| by_meeting.remove(meeting_id).unwrap_or_default())
            .collect())
    }

    /// Delete all FTS rows for a meeting and re-insert from current data.
    /// Called after transcript save or summary completion.
    pub async fn refresh_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<(), sqlx::Error> {
        Self::refresh_meeting_inner(pool, meeting_id, true).await
    }

    pub(crate) async fn refresh_meeting_unmarked(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<(), sqlx::Error> {
        Self::refresh_meeting_inner(pool, meeting_id, false).await
    }

    async fn refresh_meeting_inner(
        pool: &SqlitePool,
        meeting_id: &str,
        mark_indexed: bool,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM meeting_fts WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(&mut *tx)
            .await?;

        // Re-insert transcripts
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                t.meeting_id, 'transcript', t.id, t.transcript, t.speaker, t.timestamp,
                m.folder_id, COALESCE(f.name, '')
            FROM transcripts t
            JOIN meetings m ON t.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE t.meeting_id = ?1
              AND t.transcript IS NOT NULL AND t.transcript != ''
            "#,
        )
        .bind(meeting_id)
        .execute(&mut *tx)
        .await?;

        // Re-insert summaries
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                sp.meeting_id, 'summary', sp.meeting_id || ':' || sp.template_id,
                json_extract(sp.result, '$.markdown'), NULL, NULL,
                m.folder_id, COALESCE(f.name, '')
            FROM summary_processes sp
            JOIN meetings m ON sp.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE sp.meeting_id = ?1
               AND sp.result IS NOT NULL AND json_valid(sp.result)
              AND json_extract(sp.result, '$.markdown') IS NOT NULL
              AND json_extract(sp.result, '$.markdown') != ''
            "#,
        )
        .bind(meeting_id)
        .execute(&mut *tx)
        .await?;

        // Re-insert notes
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                mn.meeting_id, 'note', mn.meeting_id,
                mn.notes_markdown, NULL, NULL,
                m.folder_id, COALESCE(f.name, '')
            FROM meeting_notes mn
            JOIN meetings m ON mn.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE mn.meeting_id = ?1
              AND mn.notes_markdown IS NOT NULL AND mn.notes_markdown != ''
            "#,
        )
        .bind(meeting_id)
        .execute(&mut *tx)
        .await?;

        if mark_indexed {
            let revision_columns: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM pragma_table_info('search_source_state') WHERE name IN ('fts_projection_revision', 'fts_indexed_revision')",
            )
            .fetch_one(&mut *tx)
            .await?;
            if revision_columns == 2 {
                sqlx::query(
                    "UPDATE search_source_state SET fts_indexed_revision = fts_projection_revision, fts_attempt_count = 0, fts_next_attempt_at = NULL, fts_last_error = NULL WHERE meeting_id = ?",
                )
                .bind(meeting_id)
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        info!("Refreshed FTS index for meeting {}", meeting_id);
        Ok(())
    }

    /// Remove all FTS rows for a meeting.
    pub async fn remove_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM meeting_fts WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Full rebuild: delete everything and re-populate from source tables.
    pub async fn rebuild_index(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
        sqlx::query("DELETE FROM meeting_fts").execute(pool).await?;

        // Re-insert all transcripts
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                t.meeting_id, 'transcript', t.id, t.transcript, t.speaker, t.timestamp,
                m.folder_id, COALESCE(f.name, '')
            FROM transcripts t
            JOIN meetings m ON t.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE t.transcript IS NOT NULL AND t.transcript != ''
            "#,
        )
        .execute(pool)
        .await?;

        // Re-insert all summaries
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                sp.meeting_id, 'summary', sp.meeting_id || ':' || sp.template_id,
                json_extract(sp.result, '$.markdown'), NULL, NULL,
                m.folder_id, COALESCE(f.name, '')
            FROM summary_processes sp
            JOIN meetings m ON sp.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE sp.result IS NOT NULL
              AND json_extract(sp.result, '$.markdown') IS NOT NULL
              AND json_extract(sp.result, '$.markdown') != ''
            "#,
        )
        .execute(pool)
        .await?;

        // Re-insert all notes
        sqlx::query(
            r#"
            INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, speaker, timestamp_label, folder_id, folder_name)
            SELECT
                mn.meeting_id, 'note', mn.meeting_id,
                mn.notes_markdown, NULL, NULL,
                m.folder_id, COALESCE(f.name, '')
            FROM meeting_notes mn
            JOIN meetings m ON mn.meeting_id = m.id
            LEFT JOIN meeting_folders f ON m.folder_id = f.id
            WHERE mn.notes_markdown IS NOT NULL AND mn.notes_markdown != ''
            "#,
        )
        .execute(pool)
        .await?;

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meeting_fts")
            .fetch_one(pool)
            .await?;
        info!("FTS index rebuilt: {} rows", count.0);
        Ok(count.0 as u64)
    }

    /// Update folder_name in all FTS rows for a given folder_id.
    /// Called after folder rename.
    pub async fn sync_folder(pool: &SqlitePool, folder_id: &str) -> Result<(), sqlx::Error> {
        // Fetch new folder name
        let folder = FolderRepository::get_by_id(pool, folder_id).await?;
        let new_name = match folder {
            Some(f) => f.name,
            None => {
                // Folder was deleted — clear the name in FTS rows
                sqlx::query("UPDATE meeting_fts SET folder_name = '' WHERE folder_id = ?")
                    .bind(folder_id)
                    .execute(pool)
                    .await?;
                return Ok(());
            }
        };

        sqlx::query("UPDATE meeting_fts SET folder_name = ? WHERE folder_id = ?")
            .bind(&new_name)
            .bind(folder_id)
            .execute(pool)
            .await?;

        info!(
            "Synced FTS folder_name for folder {} -> '{}'",
            folder_id, new_name
        );
        Ok(())
    }
}

type FtsRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    f64,
);

fn snippet_expression(mode: SnippetMode) -> &'static str {
    match mode {
        SnippetMode::Highlighted => "snippet(meeting_fts, 3, '<mark>', '</mark>', '...', 48)",
        SnippetMode::Plain => "snippet(meeting_fts, 3, '', '', '...', 48)",
    }
}

fn append_folder_scope_cte<'args>(
    query: &mut QueryBuilder<'args, Sqlite>,
    folder_id: Option<&'args str>,
) {
    if let Some(folder_id) = folder_id {
        query
            .push("WITH RECURSIVE folder_scope(id) AS (SELECT id FROM meeting_folders WHERE id = ");
        query.push_bind(folder_id);
        query.push(
            " UNION ALL SELECT f.id FROM meeting_folders f JOIN folder_scope s ON f.parent_id = s.id) ",
        );
    }
}

fn append_result_select(
    query: &mut QueryBuilder<'_, Sqlite>,
    snippet_mode: SnippetMode,
    current_folder_metadata: bool,
) {
    query.push("SELECT fts.meeting_id, m.title, fts.chunk_type, fts.chunk_id, ");
    query.push(snippet_expression(snippet_mode));
    query.push(", fts.speaker, fts.timestamp_label, ");
    if current_folder_metadata {
        query.push("m.folder_id, COALESCE(folder.name, ''), ");
    } else {
        query.push("fts.folder_id, COALESCE(fts.folder_name, ''), ");
    }
    query.push("bm25(meeting_fts, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0, 0.5, 0.5)");
}

fn append_folder_filter(query: &mut QueryBuilder<'_, Sqlite>, folder_id: Option<&str>) {
    if folder_id.is_some() {
        query.push(" AND EXISTS (SELECT 1 FROM folder_scope WHERE id = m.folder_id)");
    }
}

async fn fts_revision_schema_available(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    let columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('search_source_state') WHERE name IN ('fts_projection_revision', 'fts_indexed_revision')",
    )
    .fetch_one(pool)
    .await?;
    Ok(columns == 2)
}

fn append_fts_freshness(query: &mut QueryBuilder<'_, Sqlite>, revision_schema: bool) {
    if revision_schema {
        query.push(
            " LEFT JOIN search_source_state ss ON ss.meeting_id = m.id WHERE (ss.meeting_id IS NULL OR ss.fts_indexed_revision = ss.fts_projection_revision)",
        );
    } else {
        query.push(" WHERE 1 = 1");
    }
}

async fn fetch_search_rows(
    pool: &SqlitePool,
    query_text: &str,
    limit: u32,
    meeting_id: Option<&str>,
    folder_id: Option<&str>,
    chunk_type: Option<&str>,
    match_mode: MatchMode,
    snippet_mode: SnippetMode,
) -> Result<Vec<FtsSearchResult>, sqlx::Error> {
    let safe_query = sanitize_fts_query(query_text, match_mode);
    if safe_query.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new("");
    let revision_schema = fts_revision_schema_available(pool).await?;
    append_folder_scope_cte(&mut query, folder_id);
    append_result_select(&mut query, snippet_mode, false);
    query.push(" FROM meeting_fts fts JOIN meetings m ON fts.meeting_id = m.id");
    append_fts_freshness(&mut query, revision_schema);
    query.push(" AND meeting_fts MATCH ");
    query.push_bind(safe_query);
    append_folder_filter(&mut query, folder_id);
    if let Some(meeting_id) = meeting_id {
        query.push(" AND fts.meeting_id = ");
        query.push_bind(meeting_id);
    }
    if let Some(chunk_type) = chunk_type {
        query.push(" AND fts.chunk_type = ");
        query.push_bind(chunk_type);
    }
    query.push(" ORDER BY 10, fts.meeting_id, fts.chunk_type, fts.chunk_id LIMIT ");
    query.push_bind(limit);
    Ok(rows_to_results(
        query.build_query_as().fetch_all(pool).await?,
    ))
}

async fn fetch_folder_rows(
    pool: &SqlitePool,
    safe_query: &str,
    limit: u32,
    folder_ids: &[String],
    snippet_mode: SnippetMode,
) -> Result<Vec<FtsRow>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new("");
    let revision_schema = fts_revision_schema_available(pool).await?;
    append_result_select(&mut query, snippet_mode, true);
    query.push(" FROM meeting_fts fts JOIN meetings m ON fts.meeting_id = m.id LEFT JOIN meeting_folders folder ON m.folder_id = folder.id");
    append_fts_freshness(&mut query, revision_schema);
    query.push(" AND meeting_fts MATCH ");
    query.push_bind(safe_query);
    query.push(" AND m.folder_id IN (");
    let mut ids = query.separated(", ");
    for folder_id in folder_ids {
        ids.push_bind(folder_id);
    }
    drop(ids);
    query.push(" ) ORDER BY 10, fts.meeting_id, fts.chunk_type, fts.chunk_id LIMIT ");
    query.push_bind(limit);
    query.build_query_as().fetch_all(pool).await
}

async fn fetch_meeting_search_rows(
    pool: &SqlitePool,
    safe_query: &str,
    limit: u32,
    meeting_ids: &[String],
    folder_id: Option<&str>,
    snippet_mode: SnippetMode,
) -> Result<Vec<FtsRow>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new("");
    let revision_schema = fts_revision_schema_available(pool).await?;
    append_folder_scope_cte(&mut query, folder_id);
    append_result_select(&mut query, snippet_mode, false);
    query.push(" FROM meeting_fts fts JOIN meetings m ON fts.meeting_id = m.id");
    append_fts_freshness(&mut query, revision_schema);
    query.push(" AND meeting_fts MATCH ");
    query.push_bind(safe_query);
    append_folder_filter(&mut query, folder_id);
    query.push(" AND fts.meeting_id IN (");
    let mut ids = query.separated(", ");
    for meeting_id in meeting_ids {
        ids.push_bind(meeting_id);
    }
    drop(ids);
    query.push(" ) ORDER BY 10, fts.meeting_id, fts.chunk_type, fts.chunk_id LIMIT ");
    query.push_bind(limit);
    query.build_query_as().fetch_all(pool).await
}

async fn fetch_meeting_rows(
    pool: &SqlitePool,
    meeting_ids: &[String],
    per_meeting_limit: u32,
    total_limit: u32,
) -> Result<Vec<FtsRow>, sqlx::Error> {
    let revision_schema = fts_revision_schema_available(pool).await?;
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT meeting_id, title, chunk_type, chunk_id, snippet, speaker, timestamp_label, folder_id, folder_name, rank FROM (SELECT fts.meeting_id, m.title, fts.chunk_type, fts.chunk_id, substr(fts.text, 1, 400) AS snippet, fts.speaker, fts.timestamp_label, fts.folder_id, COALESCE(fts.folder_name, '') AS folder_name, 0.0 AS rank, ROW_NUMBER() OVER (PARTITION BY fts.meeting_id ORDER BY CASE fts.chunk_type WHEN 'summary' THEN 0 WHEN 'note' THEN 1 ELSE 2 END, fts.chunk_id) AS rn FROM meeting_fts fts JOIN meetings m ON fts.meeting_id = m.id",
    );
    append_fts_freshness(&mut query, revision_schema);
    query.push(" AND fts.meeting_id IN (");
    let mut ids = query.separated(", ");
    for meeting_id in meeting_ids {
        ids.push_bind(meeting_id);
    }
    drop(ids);
    query.push(")) WHERE rn <= ");
    query.push_bind(per_meeting_limit);
    query.push(" ORDER BY meeting_id, CASE chunk_type WHEN 'summary' THEN 0 WHEN 'note' THEN 1 ELSE 2 END, chunk_id LIMIT ");
    query.push_bind(total_limit);
    query.build_query_as().fetch_all(pool).await
}

fn compare_results(left: &FtsSearchResult, right: &FtsSearchResult) -> Ordering {
    left.rank
        .total_cmp(&right.rank)
        .then_with(|| left.meeting_id.cmp(&right.meeting_id))
        .then_with(|| left.chunk_type.cmp(&right.chunk_type))
        .then_with(|| left.chunk_id.cmp(&right.chunk_id))
}

fn retain_best_result(results: &mut Vec<FtsSearchResult>, candidate: FtsSearchResult, cap: usize) {
    if cap == 0
        || results.iter().any(|result| {
            result.meeting_id == candidate.meeting_id
                && result.chunk_type == candidate.chunk_type
                && result.chunk_id == candidate.chunk_id
        })
    {
        return;
    }
    if results.len() < cap {
        results.push(candidate);
        return;
    }
    let Some((worst_index, worst)) = results
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| compare_results(left, right))
    else {
        return;
    };
    if compare_results(&candidate, worst) == Ordering::Less {
        results[worst_index] = candidate;
    }
}

fn compare_hydration_results(left: &FtsSearchResult, right: &FtsSearchResult) -> Ordering {
    left.meeting_id
        .cmp(&right.meeting_id)
        .then_with(|| {
            hydration_chunk_order(&left.chunk_type).cmp(&hydration_chunk_order(&right.chunk_type))
        })
        .then_with(|| left.chunk_id.cmp(&right.chunk_id))
}

fn hydration_chunk_order(chunk_type: &str) -> u8 {
    match chunk_type {
        "summary" => 0,
        "note" => 1,
        _ => 2,
    }
}

fn retain_earliest_result(
    results: &mut Vec<FtsSearchResult>,
    candidate: FtsSearchResult,
    cap: usize,
) {
    if cap == 0
        || results.iter().any(|result| {
            result.meeting_id == candidate.meeting_id
                && result.chunk_type == candidate.chunk_type
                && result.chunk_id == candidate.chunk_id
        })
    {
        return;
    }
    if results.len() < cap {
        results.push(candidate);
        return;
    }
    let Some((worst_index, worst)) = results
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| compare_hydration_results(left, right))
    else {
        return;
    };
    if compare_hydration_results(&candidate, worst) == Ordering::Less {
        results[worst_index] = candidate;
    }
}

fn rows_to_results(rows: Vec<FtsRow>) -> Vec<FtsSearchResult> {
    rows.into_iter()
        .map(
            |(
                meeting_id,
                meeting_title,
                chunk_type,
                chunk_id,
                snippet,
                speaker,
                timestamp_label,
                folder_id,
                folder_name,
                rank,
            )| FtsSearchResult {
                meeting_id,
                meeting_title,
                chunk_type,
                chunk_id,
                snippet,
                speaker,
                timestamp_label,
                folder_id,
                folder_name,
                rank,
            },
        )
        .collect()
}

/// Sanitize a user query string for FTS5 MATCH.
/// ponytail: terms are escaped before applying the selected OR, AND, or phrase mode; richer FTS5 syntax remains unsupported.
fn sanitize_fts_query(query: &str, match_mode: MatchMode) -> String {
    // Remove FTS5 double-quote syntax to prevent injection
    let cleaned = query.replace('"', "");
    // Remove FTS5 prefix operators, colons, and parameter markers
    let cleaned = cleaned
        .replace('-', " ")
        .replace('+', " ")
        .replace('*', " ")
        .replace(':', " ")
        .replace('?', " ");
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    let terms = parts
        .iter()
        .map(|w| format!("\"{}\"", w))
        .collect::<Vec<_>>();
    match match_mode {
        MatchMode::Or => terms.join(" OR "),
        MatchMode::And => terms.join(" AND "),
        MatchMode::Phrase if !parts.is_empty() => format!("\"{}\"", parts.join(" ")),
        MatchMode::Phrase => String::new(),
    }
}

async fn expand_transcript_segments(
    pool: &SqlitePool,
    results: &mut [FtsSearchResult],
    radius_chars: usize,
    strip_injected_markers: bool,
) -> Result<(), sqlx::Error> {
    let mut expanded_chars = 0;
    for result in results
        .iter_mut()
        .filter(|result| result.chunk_type == "transcript")
    {
        let Some(transcript) =
            sqlx::query_scalar::<_, String>("SELECT transcript FROM transcripts WHERE id = ?1")
                .bind(&result.chunk_id)
                .fetch_optional(pool)
                .await?
        else {
            continue;
        };
        let needle = if strip_injected_markers {
            result.snippet.replace("<mark>", "").replace("</mark>", "")
        } else {
            result.snippet.clone()
        };
        let needle = needle.trim_matches('.').trim().to_string();
        let Some(start) = transcript.find(&needle) else {
            continue;
        };
        let end = start + needle.len();
        let window_start = transcript[..start]
            .char_indices()
            .rev()
            .nth(radius_chars.saturating_sub(1))
            .map(|(index, _)| index)
            .unwrap_or(0);
        let window_end = transcript[end..]
            .char_indices()
            .nth(radius_chars)
            .map(|(index, _)| end + index)
            .unwrap_or(transcript.len());
        let expanded = format!(
            "{}{}{}",
            &transcript[window_start..start],
            result.snippet.trim_matches('.'),
            &transcript[end..window_end]
        );
        let char_count = expanded.chars().count();
        // ponytail: 8K expanded characters bounds retrieval globally; model metadata can replace this fixed ceiling.
        if expanded_chars + char_count > 8_000 {
            break;
        }
        expanded_chars += char_count;
        result.snippet = expanded;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    #[test]
    fn sanitize_removes_fts_operators() {
        assert_eq!(
            sanitize_fts_query(r#"risk AND migration"#, MatchMode::Or),
            "\"risk\" OR \"AND\" OR \"migration\""
        );
        assert_eq!(
            sanitize_fts_query(r#"risk "quoted""#, MatchMode::Or),
            "\"risk\" OR \"quoted\""
        );
        assert_eq!(
            sanitize_fts_query(r#"-risk +migration"#, MatchMode::Or),
            "\"risk\" OR \"migration\""
        );
        assert_eq!(
            sanitize_fts_query(r#"folder:"Sprint 14" risk"#, MatchMode::Or),
            "\"folder\" OR \"Sprint\" OR \"14\" OR \"risk\""
        );
    }

    #[test]
    fn sanitize_collapses_whitespace() {
        assert_eq!(
            sanitize_fts_query("  hello   world  ", MatchMode::Or),
            "\"hello\" OR \"world\""
        );
    }

    #[test]
    fn sanitize_supports_match_modes() {
        assert_eq!(
            sanitize_fts_query("quick brown fox", MatchMode::Or),
            "\"quick\" OR \"brown\" OR \"fox\""
        );
        assert_eq!(
            sanitize_fts_query("quick brown fox", MatchMode::And),
            "\"quick\" AND \"brown\" AND \"fox\""
        );
        assert_eq!(
            sanitize_fts_query("quick brown fox", MatchMode::Phrase),
            "\"quick brown fox\""
        );
    }

    #[test]
    fn split_folder_operator_extracts_name_and_rest() {
        let (rest, name) = split_folder_operator(r#"folder:"Sprint 14" migration"#);
        assert_eq!(rest, "migration");
        assert_eq!(name.as_deref(), Some("Sprint 14"));

        let (rest, name) = split_folder_operator("  plain query  ");
        assert_eq!(rest, "plain query");
        assert!(name.is_none());
    }

    #[test]
    fn split_folder_operator_preserves_text_before_the_operator() {
        let (rest, name) = split_folder_operator(r#"migration risks folder:"Sales""#);
        assert_eq!(rest, "migration risks");
        assert_eq!(name.as_deref(), Some("Sales"));

        let (rest, name) = split_folder_operator(r#"before folder:"Sales" after"#);
        assert_eq!(rest, "before after");
        assert_eq!(name.as_deref(), Some("Sales"));
    }

    #[test]
    fn strip_folder_operators_removes_repeated_operators_without_losing_text() {
        assert_eq!(
            strip_folder_operators(
                r#"before folder:"Sales" after folder:"Planning" needle"#.to_string(),
            ),
            "before after needle"
        );
        assert_eq!(
            folder_operator_names(r#"folder:"Sales" folder:"Planning" needle"#),
            vec!["Sales", "Planning"]
        );
    }

    #[tokio::test]
    async fn folder_operator_failures_never_widen_public_searches() {
        let pool = setup_fts_db().await;
        let query = r#"folder:"Missing" needle"#;

        assert!(FtsRepository::search(&pool, query, 10, None)
            .await
            .unwrap()
            .is_empty());
        assert!(FtsRepository::search_transcripts_with_mode(
            &pool,
            query,
            10,
            "meeting",
            MatchMode::Or,
        )
        .await
        .unwrap()
        .is_empty());
        assert!(
            FtsRepository::search_with_folder_id(&pool, query, 10, "folder", MatchMode::Or,)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(FtsRepository::search_with_folder_ids(
            &pool,
            query,
            10,
            &["folder".to_string()],
            MatchMode::Or,
        )
        .await
        .unwrap()
        .is_empty());
        assert!(FtsRepository::search_with_meeting_ids(
            &pool,
            query,
            10,
            &["meeting".to_string()],
            MatchMode::Or,
        )
        .await
        .unwrap()
        .is_empty());
    }

    #[tokio::test]
    async fn folder_lookup_failures_are_returned_as_errors() {
        let pool = setup_fts_db().await;
        sqlx::query("DROP TABLE meeting_folders")
            .execute(&pool)
            .await
            .unwrap();

        let error = FtsRepository::search(&pool, r#"folder:"Missing" needle"#, 10, None)
            .await
            .expect_err("folder lookup failure must not become an unscoped search");
        assert!(matches!(error, sqlx::Error::Database(_)));
    }

    #[tokio::test]
    async fn repeated_folder_operators_keep_surrounding_text() {
        let pool = setup_fts_db().await;
        sqlx::query(
            "INSERT INTO meeting_folders (id, name, created_at) VALUES ('folder', 'Sales', '2026-08-29T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_id) VALUES ('meeting', 'Sales', 'now', 'now', 'folder')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('transcript', 'meeting', 'before after needle', '10:00')")
            .execute(&pool)
            .await
            .unwrap();
        FtsRepository::refresh_meeting(&pool, "meeting")
            .await
            .unwrap();

        let results = FtsRepository::search(
            &pool,
            r#"before folder:"Sales" after folder:"Sales" needle"#,
            10,
            None,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meeting_id, "meeting");

        let transcript_results = FtsRepository::search_transcripts_with_mode(
            &pool,
            r#"before folder:"Sales" after folder:"Sales" needle"#,
            10,
            "meeting",
            MatchMode::And,
        )
        .await
        .unwrap();
        assert_eq!(transcript_results.len(), 1);
    }

    #[tokio::test]
    async fn folder_id_search_chunks_large_allow_lists() {
        let pool = setup_fts_db().await;
        let folder_ids: Vec<String> = (0..1200).map(|index| format!("folder-{index}")).collect();
        let results =
            FtsRepository::search_with_folder_ids(&pool, "needle", 10, &folder_ids, MatchMode::Or)
                .await
                .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn folder_id_search_uses_current_folder_metadata() {
        let pool = setup_fts_db().await;
        sqlx::query(
            "INSERT INTO meeting_folders (id, name, created_at) VALUES ('current-folder', 'Current', 'now')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_id) VALUES ('meeting', 'Meeting', 'now', 'now', 'current-folder')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, folder_id, folder_name) VALUES ('meeting', 'transcript', 'chunk', 'needle', 'stale-folder', 'Stale')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let results = FtsRepository::search_with_folder_ids(
            &pool,
            "needle",
            10,
            &["current-folder".to_string()],
            MatchMode::Or,
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].folder_id.as_deref(), Some("current-folder"));
        assert_eq!(results[0].folder_name, "Current");
    }

    #[tokio::test]
    async fn root_folder_search_scopes_large_recursive_descendants() {
        let pool = setup_fts_db().await;
        sqlx::query(
            "INSERT INTO meeting_folders (id, name, created_at) VALUES ('root', 'Root', '2026-08-29T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for index in 0..450 {
            sqlx::query(
                "INSERT INTO meeting_folders (id, name, parent_id, created_at) VALUES (?, ?, 'root', '2026-08-29T00:00:00Z')",
            )
            .bind(format!("child-{index}"))
            .bind(format!("Child {index}"))
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_id) VALUES ('descendant', 'Descendant', '2026-08-29T00:00:00Z', '2026-08-29T00:00:00Z', 'child-449')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('descendant-transcript', 'descendant', 'recursive needle', '10:00')")
            .execute(&pool)
            .await
            .unwrap();
        FtsRepository::refresh_meeting(&pool, "descendant")
            .await
            .unwrap();

        let results =
            FtsRepository::search_with_folder_id(&pool, "needle", 10, "root", MatchMode::Or)
                .await
                .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meeting_id, "descendant");
    }

    #[tokio::test]
    async fn plain_search_preserves_literal_mark_tags() {
        let pool = setup_fts_db().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('marked', 'Marked', 'now', 'now')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('marked-transcript', 'marked', 'literal <mark> needle </mark> text', '10:00')")
            .execute(&pool)
            .await
            .unwrap();
        FtsRepository::refresh_meeting(&pool, "marked")
            .await
            .unwrap();

        let results =
            FtsRepository::search_with_mode_plain(&pool, "needle", 10, None, MatchMode::Or)
                .await
                .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].snippet, "literal <mark> needle </mark> text");
    }

    #[tokio::test]
    async fn search_with_meeting_ids_scopes_to_allow_list() {
        let pool = setup_fts_db().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m-in', 'In', 'now', 'now'), ('m-out', 'Out', 'now', 'now')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind("t-in")
        .bind("m-in")
        .bind("allowlisted lexical needle")
        .bind("10:00")
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind("t-out")
        .bind("m-out")
        .bind("allowlisted lexical needle")
        .bind("10:00")
        .execute(&pool)
        .await
        .unwrap();
        FtsRepository::refresh_meeting(&pool, "m-in").await.unwrap();
        FtsRepository::refresh_meeting(&pool, "m-out")
            .await
            .unwrap();

        let results = FtsRepository::search_with_meeting_ids(
            &pool,
            "needle",
            10,
            &["m-in".to_string()],
            MatchMode::Or,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meeting_id, "m-in");

        // Empty allow-list stays empty without querying.
        let results =
            FtsRepository::search_with_meeting_ids(&pool, "needle", 10, &[], MatchMode::Or)
                .await
                .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn meeting_id_search_chunks_large_allow_lists() {
        let pool = setup_fts_db().await;
        let meeting_ids: Vec<String> = (0..1200).map(|index| format!("meeting-{index}")).collect();
        let results = FtsRepository::search_with_meeting_ids(
            &pool,
            "needle",
            10,
            &meeting_ids,
            MatchMode::Or,
        )
        .await
        .unwrap();
        assert!(results.is_empty());
    }

    async fn setup_fts_db() -> SqlitePool {
        let pool = SqlitePool::connect(":memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE meetings (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                folder_path TEXT,
                folder_id TEXT
            );
            CREATE TABLE meeting_folders (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                parent_id TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE transcripts (
                id TEXT PRIMARY KEY,
                meeting_id TEXT NOT NULL,
                transcript TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                speaker TEXT,
                audio_start_time REAL,
                audio_end_time REAL,
                duration REAL
            );
            CREATE TABLE summary_processes (
                meeting_id TEXT NOT NULL,
                template_id TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                result TEXT,
                PRIMARY KEY (meeting_id, template_id)
            );
            CREATE TABLE meeting_notes (
                meeting_id TEXT PRIMARY KEY NOT NULL,
                notes_markdown TEXT
            );
            CREATE VIRTUAL TABLE meeting_fts USING fts5(
                meeting_id UNINDEXED,
                chunk_type UNINDEXED,
                chunk_id UNINDEXED,
                text,
                speaker UNINDEXED,
                timestamp_label UNINDEXED,
                folder_id UNINDEXED,
                folder_name,
                tokenize = 'unicode61'
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("create schema");
        pool
    }

    #[tokio::test]
    async fn search_finds_transcript_text() {
        let pool = setup_fts_db().await;

        // Seed meeting
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m1")
            .bind("Sprint Planning")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        // Seed transcript
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind("t1")
        .bind("m1")
        .bind("We discussed the migration risk and decided to use the outbox pattern for Kafka")
        .bind("14:32")
        .execute(&pool)
        .await
        .unwrap();

        // Populate FTS
        FtsRepository::refresh_meeting(&pool, "m1").await.unwrap();

        // Search
        let results = FtsRepository::search(&pool, "migration risk", 10, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meeting_id, "m1");
        assert_eq!(results[0].chunk_type, "transcript");
        assert!(results[0].snippet.contains("<mark>"));
    }

    #[tokio::test]
    async fn search_excludes_stale_fts_projection() {
        let pool = setup_fts_db().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'Meeting', 'now', 'now')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m1', 'transcript', 't1', 'stale needle')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE search_source_state (meeting_id TEXT PRIMARY KEY, source_revision INTEGER NOT NULL, fts_projection_revision INTEGER NOT NULL, fts_indexed_revision INTEGER NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO search_source_state VALUES ('m1', 7, 2, 2)")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            FtsRepository::search(&pool, "needle", 10, None)
                .await
                .unwrap()
                .len(),
            1
        );

        sqlx::query("UPDATE search_source_state SET fts_projection_revision = 3")
            .execute(&pool)
            .await
            .unwrap();
        assert!(FtsRepository::search(&pool, "needle", 10, None)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn transcript_search_does_not_spend_its_limit_on_notes_or_summaries() {
        let pool = setup_fts_db().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'Meeting', 'now', 'now')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m1', 'summary', 's', 'needle'), ('m1', 'note', 'n', 'needle'), ('m1', 'transcript', 't', 'needle')").execute(&pool).await.unwrap();

        let results =
            FtsRepository::search_transcripts_with_mode(&pool, "needle", 1, "m1", MatchMode::Or)
                .await
                .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_type, "transcript");
        assert_eq!(results[0].chunk_id, "t");
    }

    #[tokio::test]
    async fn transcript_hit_expands_around_highlight() {
        let pool = setup_fts_db().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("expanded")
            .bind("Expanded context")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .execute(&pool)
            .await
            .unwrap();
        let transcript = format!(
            "{} migration risk {}",
            "context before ".repeat(40),
            "context after ".repeat(40)
        );
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind("expanded-transcript")
        .bind("expanded")
        .bind(&transcript)
        .bind("14:32")
        .execute(&pool)
        .await
        .unwrap();
        FtsRepository::refresh_meeting(&pool, "expanded")
            .await
            .unwrap();

        let results =
            FtsRepository::search_with_mode(&pool, "migration risk", 10, None, MatchMode::And)
                .await
                .unwrap();

        assert!(results[0].snippet.contains("<mark>migration</mark>"));
        assert!(results[0].snippet.contains("context before"));
        assert!(results[0].snippet.contains("context after"));
        assert!(results[0].snippet.chars().count() > 300);
    }

    #[tokio::test]
    async fn search_finds_summary_text() {
        let pool = setup_fts_db().await;

        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m2")
            .bind("Architecture Review")
            .bind("2026-07-27T11:00:00Z")
            .bind("2026-07-27T11:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        let result_json =
            r#"{"markdown":"Decision: migrate to event-driven architecture with CQRS"}"#;
        sqlx::query("INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES (?, ?, ?, ?, ?, ?)")
            .bind("m2")
            .bind("standard_meeting")
            .bind("completed")
            .bind("2026-07-27T11:30:00Z")
            .bind("2026-07-27T11:30:00Z")
            .bind(result_json)
            .execute(&pool)
            .await
            .unwrap();

        FtsRepository::refresh_meeting(&pool, "m2").await.unwrap();

        let results = FtsRepository::search(&pool, "event-driven CQRS", 10, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_type, "summary");
    }

    #[tokio::test]
    async fn malformed_summary_json_does_not_erase_the_meeting_projection() {
        let pool = setup_fts_db().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m-json', 'Meeting', 'now', 'now')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('t-json', 'm-json', 'durable lexical text', '10:00')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES ('m-json', 'summary', 'completed', 'now', 'now', '{not json')")
            .execute(&pool)
            .await
            .unwrap();

        FtsRepository::refresh_meeting(&pool, "m-json")
            .await
            .unwrap();
        assert_eq!(
            FtsRepository::search(&pool, "durable lexical", 10, Some("m-json"))
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn search_finds_note_text() {
        let pool = setup_fts_db().await;

        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m3")
            .bind("1:1 with manager")
            .bind("2026-07-27T12:00:00Z")
            .bind("2026-07-27T12:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO meeting_notes (meeting_id, notes_markdown) VALUES (?, ?)")
            .bind("m3")
            .bind("Action item: follow up on budget approval next week")
            .execute(&pool)
            .await
            .unwrap();

        FtsRepository::refresh_meeting(&pool, "m3").await.unwrap();

        let results = FtsRepository::search(&pool, "budget approval", 10, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_type, "note");
    }

    #[tokio::test]
    async fn search_with_folder_filter() {
        let pool = setup_fts_db().await;

        // Create folder
        sqlx::query("INSERT INTO meeting_folders (id, name, created_at) VALUES (?, ?, ?)")
            .bind("f1")
            .bind("Sprint 14")
            .bind("2026-07-27T09:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        // Meeting in folder
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_id) VALUES (?, ?, ?, ?, ?)")
            .bind("m4")
            .bind("Sprint 14 Planning")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .bind("f1")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind("t4")
        .bind("m4")
        .bind("Discussing migration strategy for the new microservice")
        .bind("10:15")
        .execute(&pool)
        .await
        .unwrap();

        // Meeting NOT in folder
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m5")
            .bind("Unrelated meeting")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind("t5")
        .bind("m5")
        .bind("Also discussing migration strategy for another project")
        .bind("10:30")
        .execute(&pool)
        .await
        .unwrap();

        FtsRepository::refresh_meeting(&pool, "m4").await.unwrap();
        FtsRepository::refresh_meeting(&pool, "m5").await.unwrap();

        // Search with folder filter
        let results = FtsRepository::search(&pool, r#"folder:"Sprint 14" migration"#, 10, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meeting_id, "m4");
        assert_eq!(results[0].folder_name, "Sprint 14");

        let scoped_results =
            FtsRepository::search(&pool, r#"folder:"Sprint 14" migration"#, 10, Some("m5"))
                .await
                .unwrap();
        assert!(scoped_results.is_empty());
    }

    #[tokio::test]
    async fn search_empty_query_returns_empty() {
        let pool = setup_fts_db().await;
        let results = FtsRepository::search(&pool, "", 10, None).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn get_by_meeting_ids_respects_per_meeting_and_total_caps() {
        let pool = setup_fts_db().await;
        for meeting_id in ["m1", "m2", "m3"] {
            sqlx::query(
                "INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)",
            )
            .bind(meeting_id)
            .bind(format!("Meeting {meeting_id}"))
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .execute(&pool)
            .await
            .unwrap();
            for chunk_index in 0..5 {
                sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES (?, 'transcript', ?, ?)")
                    .bind(meeting_id)
                    .bind(format!("{meeting_id}-{chunk_index}"))
                    .bind(format!("content {chunk_index}"))
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        }
        sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text) VALUES ('m1', 'summary', 'm1-summary', 'meeting summary')")
            .execute(&pool)
            .await
            .unwrap();
        let ids =
            || -> Vec<String> { ["m1", "m2", "m3"].iter().map(|id| id.to_string()).collect() };

        let preferred = FtsRepository::get_by_meeting_ids(&pool, &["m1".to_string()], 1, 1)
            .await
            .unwrap();
        assert_eq!(preferred[0].chunk_type, "summary");

        // Total cap dominates: 3 meetings × per-meeting 2 = 6 > 4.
        let results = FtsRepository::get_by_meeting_ids(&pool, &ids(), 2, 4)
            .await
            .unwrap();
        assert_eq!(results.len(), 4);

        // Per-meeting cap: each meeting contributes at most 2 chunks.
        let results = FtsRepository::get_by_meeting_ids(&pool, &ids(), 2, 100)
            .await
            .unwrap();
        assert_eq!(results.len(), 6);
        let mut per_meeting: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for result in &results {
            *per_meeting.entry(result.meeting_id.as_str()).or_default() += 1;
        }
        assert!(per_meeting.values().all(|count| *count <= 2));

        // Missing meetings (e.g. deleted snapshot members) are skipped.
        let results = FtsRepository::get_by_meeting_ids(
            &pool,
            &["m1".to_string(), "missing".to_string(), "m2".to_string()],
            10,
            100,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 11);
        assert!(results.iter().all(|result| result.meeting_id != "missing"));
    }

    #[tokio::test]
    async fn get_by_meeting_ids_chunks_large_allow_lists() {
        let pool = setup_fts_db().await;
        let meeting_ids: Vec<String> = (0..1200).map(|index| format!("meeting-{index}")).collect();
        let results = FtsRepository::get_by_meeting_ids(&pool, &meeting_ids, 1, 10)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn remove_meeting_clears_fts() {
        let pool = setup_fts_db().await;

        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m6")
            .bind("Test meeting")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind("t6")
        .bind("m6")
        .bind("Some important text to search for")
        .bind("10:00")
        .execute(&pool)
        .await
        .unwrap();

        FtsRepository::refresh_meeting(&pool, "m6").await.unwrap();
        assert_eq!(
            FtsRepository::search(&pool, "important", 10, None)
                .await
                .unwrap()
                .len(),
            1
        );

        FtsRepository::remove_meeting(&pool, "m6").await.unwrap();
        assert!(FtsRepository::search(&pool, "important", 10, None)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn rebuild_index_repopulates_from_source() {
        let pool = setup_fts_db().await;

        // Seed two meetings with transcripts
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m7")
            .bind("Meeting Alpha")
            .bind("2026-07-27T10:00:00Z")
            .bind("2026-07-27T10:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind("t7")
        .bind("m7")
        .bind("Discussion about deployment strategy")
        .bind("10:00")
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind("m8")
            .bind("Meeting Beta")
            .bind("2026-07-27T11:00:00Z")
            .bind("2026-07-27T11:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind("t8")
        .bind("m8")
        .bind("Review of deployment strategy for production")
        .bind("11:00")
        .execute(&pool)
        .await
        .unwrap();

        // Clear FTS and rebuild
        sqlx::query("DELETE FROM meeting_fts")
            .execute(&pool)
            .await
            .unwrap();
        assert!(FtsRepository::search(&pool, "deployment", 10, None)
            .await
            .unwrap()
            .is_empty());

        let count = FtsRepository::rebuild_index(&pool).await.unwrap();
        assert_eq!(count, 2);

        let scoped_results = FtsRepository::search(&pool, "deployment", 10, Some("m7"))
            .await
            .unwrap();
        assert_eq!(scoped_results.len(), 1);
        assert_eq!(scoped_results[0].meeting_id, "m7");

        let results = FtsRepository::search(&pool, "deployment", 10, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }
}
