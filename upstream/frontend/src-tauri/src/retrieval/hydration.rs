//! Authoritative multi-meeting hydration (Sprint 3 Task 3.3).
//!
//! Downstream of the Task 3.2 ranked result: converts ranked meetings and
//! evidence identities into current, budgeted authoritative context. Ranking
//! snippets are never published - every retained text is re-read from the
//! authoritative tables through [`RetrievalRepository::load_meeting_source`],
//! so a stale semantic window can only cost its identity, never publish its
//! stored text. Kept separate from Chat invocation until Task 3.4.
//!
//! Publication safety (architecture "Authoritative Hydration", "Context And
//! Source Parity", "Scope Semantics"):
//! - membership is revalidated after ranking (before any load) and again
//!   after each meeting's authoritative load, immediately before that
//!   meeting's retention/publication;
//! - a meeting moved out of scope, deleted, or whose source revision changed
//!   during the load is omitted together with every one of its sources;
//! - evidence identities (transcript segment ranges, summary templates) that
//!   no longer resolve against current content are omitted; the meeting stays
//!   eligible through its current authoritative data;
//! - each selected meeting receives a guaranteed minimum allocation before
//!   the remaining budget is distributed by ranked relevance, so the first
//!   meeting's summary/notes cannot starve the others;
//! - sources correspond exactly to the retained Markdown (a source's snippet
//!   is the text published for it), and summary/notes become sources when
//!   their text is retained;
//! - cancellation is the typed [`RetrievalError::Cancelled`], never a
//!   fallback. Logs carry counts only - never query or candidate text.

use std::collections::{HashMap, HashSet};

use sha2::Digest;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use tokio_util::sync::CancellationToken;

use crate::database::repositories::fts::FtsSearchResult;
use crate::database::repositories::retrieval::{MeetingSource, RetrievalRepository};
use crate::export::context::build_meeting_sections;

use super::ranking::{RankedEvidence, RankedMeeting};
use super::service::{PersistedRetrievalScope, RankedRetrieval, RetrievalError};

/// Meetings hydrated per request: the evaluation corpus's fixed retention
/// semantics (the admissibility protocol's hydration cap) applied so the
/// guaranteed minimum can actually cover every selected meeting.
pub(crate) const MAX_HYDRATED_MEETINGS: usize = 5;
/// Guaranteed per-meeting minimum: `1 / MIN_SHARE_DIVISOR` of the final
/// budget is reserved as a floor and split EVENLY across the selected
/// meetings, so each is guaranteed `budget / (MIN_SHARE_DIVISOR * count)` -
/// a twentieth of the budget at the [`MAX_HYDRATED_MEETINGS`] cap of 5, not
/// a quarter each (which would over-commit the budget 1.25x). The remaining
/// three quarters are distributed by ranked relevance, so one long
/// summary/notes block cannot consume the whole multi-meeting budget.
/// RAISING this divisor SHRINKS the guaranteed floor.
const MIN_SHARE_DIVISOR: usize = 4;

/// One retained source. `snippet` is exactly the text published in the final
/// Markdown for this source, so sources and prompt content can never diverge.
#[derive(Debug, Clone, PartialEq)]
pub struct HydratedSource {
    pub meeting_id: String,
    pub meeting_title: String,
    pub folder_name: String,
    /// `summary`, `notes`, or `transcript`.
    pub source_kind: String,
    pub snippet: String,
    pub source_start_id: Option<String>,
    pub source_end_id: Option<String>,
    pub source_template_id: Option<String>,
    /// Ranked candidate identities (including Task 3.2 source aliases)
    /// grounded by this source's retained text.
    pub evidence_ids: Vec<String>,
}

/// Per-meeting hydration coverage.
#[derive(Debug, Clone, PartialEq)]
pub struct HydratedMeeting {
    pub meeting_id: String,
    /// The Task 3.2 aggregated meeting rank.
    pub rank: usize,
    pub retained_evidence_ids: Vec<String>,
    pub transcript_segments_included: usize,
    pub transcript_segments_total: usize,
}

/// The hydration outcome: Markdown within the caller's final provider-derived
/// budget plus the exact retained evidence/source identities and per-meeting
/// coverage. Task 3.4 owns mapping [`HydratedSource`] onto the existing
/// `ChatSource` event/persistence contracts.
#[derive(Debug, Clone, PartialEq)]
pub struct HydratedContext {
    pub markdown: String,
    pub retained_evidence_ids: Vec<String>,
    pub sources: Vec<HydratedSource>,
    pub meetings: Vec<HydratedMeeting>,
}

/// Hydrates the ranked retrieval outcome into authoritative context. The
/// budget is the FINAL context character allowance (temporal, question, and
/// history overhead already subtracted by the caller).
pub async fn hydrate_context(
    pool: &SqlitePool,
    ranked: &RankedRetrieval,
    max_context_chars: usize,
    cancellation: Option<&CancellationToken>,
) -> Result<HydratedContext, RetrievalError> {
    #[cfg(test)]
    return hydrate(pool, ranked, max_context_chars, cancellation, None).await;
    #[cfg(not(test))]
    return hydrate(pool, ranked, max_context_chars, cancellation).await;
}

#[cfg(test)]
pub(crate) async fn hydrate_with_pause(
    pool: &SqlitePool,
    ranked: &RankedRetrieval,
    max_context_chars: usize,
    cancellation: Option<&CancellationToken>,
    pause: &HydrationPause,
) -> Result<HydratedContext, RetrievalError> {
    hydrate(pool, ranked, max_context_chars, cancellation, Some(pause)).await
}

/// Test-only publication gate, mirroring the service's semantic scan gate:
/// signaled after a meeting's authoritative load and awaited until released,
/// so a test can move or delete the meeting between loading and the final
/// retention recheck deterministically.
#[cfg(test)]
pub(crate) struct HydrationPause {
    loaded: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<()>>>,
    release: tokio::sync::Notify,
}

#[cfg(test)]
impl HydrationPause {
    fn new() -> Self {
        Self {
            loaded: std::sync::Mutex::new(None),
            release: tokio::sync::Notify::new(),
        }
    }

    fn arm(&self, sender: tokio::sync::mpsc::UnboundedSender<()>) {
        *self
            .loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sender);
    }

    /// Signals the test after one meeting's load and waits for release; a
    /// no-op once consumed (subsequent meetings publish without pausing).
    async fn after_load(&self) {
        let sender = self
            .loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(());
            self.release.notified().await;
        }
    }

    fn release(&self) {
        self.release.notify_one();
    }
}

fn db_error(error: sqlx::Error) -> RetrievalError {
    super::service::db_error(error)
}

fn ensure_not_cancelled(cancel: &CancellationToken) -> Result<(), RetrievalError> {
    if cancel.is_cancelled() {
        Err(RetrievalError::Cancelled)
    } else {
        Ok(())
    }
}

fn blank(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

async fn hydrate(
    pool: &SqlitePool,
    ranked: &RankedRetrieval,
    max_context_chars: usize,
    cancellation: Option<&CancellationToken>,
    #[cfg(test)] pause: Option<&HydrationPause>,
) -> Result<HydratedContext, RetrievalError> {
    let cancel = cancellation.cloned().unwrap_or_default();
    ensure_not_cancelled(&cancel)?;
    let scope = &ranked.scope.scope;

    // Citable ranked evidence per meeting (title/meeting-profile candidates
    // are selection signals, never evidence - the same rule the reranker
    // applies).
    let mut items_by_meeting: HashMap<String, Vec<&RankedEvidence>> = HashMap::new();
    for entry in &ranked.ranking.evidence {
        let kind = entry.evidence.source_kind.as_str();
        if kind == "meeting_profile" || kind == "title" {
            continue;
        }
        items_by_meeting
            .entry(entry.evidence.meeting_id.clone())
            .or_default()
            .push(entry);
    }
    let selected: Vec<&RankedMeeting> = ranked
        .ranking
        .meetings
        .iter()
        .filter(|meeting| items_by_meeting.contains_key(&meeting.meeting_id))
        .take(MAX_HYDRATED_MEETINGS)
        .collect();
    if selected.is_empty() {
        return Ok(empty_context(max_context_chars));
    }

    // Checkpoint 1: current scope membership, after ranking, before loading.
    let ids: Vec<String> = selected
        .iter()
        .map(|meeting| meeting.meeting_id.clone())
        .collect();
    let alive = current_scope_members(pool, scope, &ids, &cancel).await?;
    let selected: Vec<&RankedMeeting> = selected
        .into_iter()
        .filter(|meeting| alive.contains(&meeting.meeting_id))
        .collect();
    if selected.is_empty() {
        return Ok(empty_context(max_context_chars));
    }

    // Budget: the document prefix is reserved first, then a guaranteed
    // minimum per selected meeting, then the remaining budget distributed by
    // ranked relevance (reciprocal rank).
    const GLOBAL_PREFIX: &str = "# Meeting Context\n\n";
    let count = selected.len();
    let budget = max_context_chars.saturating_sub(GLOBAL_PREFIX.chars().count());
    let minimum = budget / (MIN_SHARE_DIVISOR * count);
    let remaining = budget - minimum * count;
    let weight_sum: f64 = selected.iter().map(|m| 1.0 / m.rank as f64).sum();
    let shares: Vec<usize> = selected
        .iter()
        .map(|meeting| {
            minimum
                + ((remaining as f64) * (1.0 / meeting.rank as f64) / weight_sum).floor() as usize
        })
        .collect();

    let mut markdown = String::from(GLOBAL_PREFIX);
    let mut sources: Vec<HydratedSource> = Vec::new();
    let mut retained_evidence_ids: Vec<String> = Vec::new();
    let mut coverage: Vec<HydratedMeeting> = Vec::new();
    let mut omitted = 0usize;
    let mut meeting_ordinal = 0usize;

    for (meeting, share) in selected.into_iter().zip(shares) {
        ensure_not_cancelled(&cancel)?;
        let mut transcript_ids = Vec::new();
        for item in &items_by_meeting[&meeting.meeting_id] {
            let evidence = &item.evidence;
            if evidence.source_kind == "transcript" {
                transcript_ids.extend(
                    [
                        evidence.source_start_id.as_ref(),
                        evidence.source_end_id.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .cloned(),
                );
            }
            for alias in &evidence.source_aliases {
                if alias.source_kind == "transcript" {
                    transcript_ids.extend(
                        [alias.source_start_id.as_ref(), alias.source_end_id.as_ref()]
                            .into_iter()
                            .flatten()
                            .cloned(),
                    );
                }
            }
        }
        let Some(source) = RetrievalRepository::load_meeting_source_relevant_with_cancellation(
            pool,
            &meeting.meeting_id,
            &transcript_ids,
            &cancel,
        )
        .await
        .map_err(db_error)?
        else {
            omitted += 1;
            continue;
        };
        #[cfg(test)]
        {
            if let Some(pause) = pause {
                pause.after_load().await;
            }
        }
        ensure_not_cancelled(&cancel)?;
        // Checkpoint 2: one authoritative scope/revision snapshot immediately
        // before retaining/publishing this meeting. A move, deletion, or
        // mid-load content change omits the meeting and every source.
        let current = current_scope_and_revision(
            pool,
            scope,
            &meeting.meeting_id,
            source.source_revision,
            &cancel,
        )
        .await?;
        ensure_not_cancelled(&cancel)?;
        if !current {
            omitted += 1;
            continue;
        }

        let plan = plan_meeting_evidence(&source, &items_by_meeting[&meeting.meeting_id]);
        let mut header = format!(
            "## Meeting {} — {}\n\n**ID:** `{}`\n",
            meeting_ordinal + 1,
            source.title,
            source.meeting_id
        );
        if !source.folder_name.is_empty() {
            header.push_str(&format!("**Folder:** {}\n", source.folder_name));
        }
        header.push('\n');
        let used_chars = markdown.chars().count();
        // The header must leave room for content: this meeting's whole
        // allowance is `share`, so a header that fills it publishes a bare
        // heading and nothing else.
        if share == 0 || header.chars().count() >= share {
            omitted += 1;
            continue;
        }
        let sections = build_meeting_sections(
            source.latest_summary_markdown.as_deref(),
            source.notes_markdown.as_deref(),
            &plan.segments,
            source.transcript_segments_total,
            used_chars.saturating_add(share),
            used_chars + header.chars().count(),
        );
        ensure_not_cancelled(&cancel)?;

        let retained_segments: HashSet<&str> = sections
            .retained_transcript_ids
            .iter()
            .map(String::as_str)
            .collect();
        let summary_retained = blank(sections.summary.as_deref()).is_some();
        let note_retained = blank(sections.notes.as_deref()).is_some();
        let mut meeting_sources: Vec<HydratedSource> = Vec::new();
        let mut meeting_evidence: Vec<String> = Vec::new();
        if let Some(snippet) = blank(sections.summary.as_deref()) {
            let evidence_ids = plan.retained_summary_ids();
            meeting_evidence.extend(evidence_ids.iter().cloned());
            meeting_sources.push(HydratedSource {
                meeting_id: source.meeting_id.clone(),
                meeting_title: source.title.clone(),
                folder_name: source.folder_name.clone(),
                source_kind: "summary".to_string(),
                snippet: snippet.to_string(),
                source_start_id: None,
                source_end_id: None,
                source_template_id: source.latest_summary_template_id.clone(),
                evidence_ids,
            });
        }
        if let Some(snippet) = blank(sections.notes.as_deref()) {
            let evidence_ids = plan.retained_note_ids();
            meeting_evidence.extend(evidence_ids.iter().cloned());
            meeting_sources.push(HydratedSource {
                meeting_id: source.meeting_id.clone(),
                meeting_title: source.title.clone(),
                folder_name: source.folder_name.clone(),
                source_kind: "notes".to_string(),
                snippet: snippet.to_string(),
                source_start_id: None,
                source_end_id: None,
                source_template_id: None,
                evidence_ids,
            });
        }
        // Transcript sources: one per contiguous retained segment run, with
        // the run's identity and every grounded ranked/alias candidate.
        for group in plan.retained_groups(&retained_segments) {
            ensure_not_cancelled(&cancel)?;
            meeting_evidence.extend(group.evidence_ids.iter().cloned());
            meeting_sources.push(HydratedSource {
                meeting_id: source.meeting_id.clone(),
                meeting_title: source.title.clone(),
                folder_name: source.folder_name.clone(),
                source_kind: "transcript".to_string(),
                snippet: group.snippet.clone(),
                source_start_id: Some(group.start_id.clone()),
                source_end_id: Some(group.end_id.clone()),
                source_template_id: None,
                evidence_ids: group.evidence_ids,
            });
        }
        // A meeting with no retained content publishes nothing (no bare
        // headers, no coverage notice without content).
        if meeting_sources.is_empty() {
            omitted += 1;
            continue;
        }
        meeting_ordinal += 1;
        markdown.push_str(&header);
        markdown.push_str(&sections.markdown);
        retained_evidence_ids.extend(meeting_evidence);
        sources.extend(meeting_sources);
        coverage.push(HydratedMeeting {
            meeting_id: source.meeting_id,
            rank: meeting.rank,
            retained_evidence_ids: plan.retained_evidence_ids(
                summary_retained,
                note_retained,
                &retained_segments,
            ),
            transcript_segments_included: sections.retained_transcript_ids.len(),
            transcript_segments_total: source.transcript_segments_total,
        });
    }

    // Every selected meeting was omitted (moved, deleted, or without any
    // retained content): publish the empty outcome, never a bare header.
    if coverage.is_empty() {
        return Ok(empty_context(max_context_chars));
    }

    log::info!(
        "Hydration: meetings={} sources={} retained={} omitted={} scope_tag={}",
        coverage.len(),
        sources.len(),
        retained_evidence_ids.len(),
        omitted,
        scope_tag(scope)
    );
    ensure_not_cancelled(&cancel)?;
    Ok(HydratedContext {
        markdown,
        retained_evidence_ids,
        sources,
        meetings: coverage,
    })
}

fn empty_context(max_context_chars: usize) -> HydratedContext {
    HydratedContext {
        markdown: "No relevant meeting content found.\n"
            .chars()
            .take(max_context_chars)
            .collect(),
        retained_evidence_ids: Vec::new(),
        sources: Vec::new(),
        meetings: Vec::new(),
    }
}

fn scope_tag(scope: &PersistedRetrievalScope) -> &'static str {
    match scope {
        PersistedRetrievalScope::All => "all",
        PersistedRetrievalScope::Meeting(_) => "meeting",
        PersistedRetrievalScope::Folder(_) => "folder",
        PersistedRetrievalScope::AllowedMeetingIds(_) => "allowed_ids",
    }
}

/// Current membership for the selected meetings: existence for every scope,
/// plus the authoritative recursive folder gate for folder scopes. Snapshot
/// and today allow-lists stay frozen; current existence is rechecked.
async fn current_scope_members(
    pool: &SqlitePool,
    scope: &PersistedRetrievalScope,
    meeting_ids: &[String],
    cancel: &CancellationToken,
) -> Result<HashSet<String>, RetrievalError> {
    if meeting_ids.is_empty() {
        return Ok(HashSet::new());
    }
    ensure_not_cancelled(cancel)?;
    let mut query = QueryBuilder::<Sqlite>::new(match scope {
        PersistedRetrievalScope::Folder(_) => {
            "WITH RECURSIVE folder_scope(id) AS (SELECT id FROM meeting_folders WHERE id = "
        }
        _ => "",
    });
    if let PersistedRetrievalScope::Folder(folder_id) = scope {
        query.push_bind(folder_id);
        query.push(
            " UNION ALL SELECT f.id FROM meeting_folders f JOIN folder_scope s ON f.parent_id = s.id) ",
        );
    }
    query.push("SELECT m.id FROM meetings m WHERE m.id IN (");
    let mut binds = query.separated(", ");
    for meeting_id in meeting_ids {
        binds.push_bind(meeting_id);
    }
    drop(binds);
    query.push(")");
    match scope {
        PersistedRetrievalScope::Folder(_) => {
            query.push(" AND m.folder_id IN (SELECT id FROM folder_scope)");
        }
        PersistedRetrievalScope::Meeting(scope_id) => {
            query.push(" AND m.id = ").push_bind(scope_id);
        }
        PersistedRetrievalScope::AllowedMeetingIds(allowed_ids) => {
            query.push(" AND m.id IN (");
            let mut allowed = query.separated(", ");
            for id in allowed_ids {
                allowed.push_bind(id);
            }
            drop(allowed);
            query.push(")");
        }
        PersistedRetrievalScope::All => {}
    }
    let rows: Vec<(String,)> = query
        .build_query_as()
        .fetch_all(pool)
        .await
        .map_err(db_error)?;
    ensure_not_cancelled(cancel)?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

async fn current_scope_and_revision(
    pool: &SqlitePool,
    scope: &PersistedRetrievalScope,
    meeting_id: &str,
    expected_revision: Option<i64>,
    cancel: &CancellationToken,
) -> Result<bool, RetrievalError> {
    ensure_not_cancelled(cancel)?;
    let mut query = QueryBuilder::<Sqlite>::new(match scope {
        PersistedRetrievalScope::Folder(_) => {
            "WITH RECURSIVE folder_scope(id) AS (SELECT id FROM meeting_folders WHERE id = "
        }
        _ => "",
    });
    if let PersistedRetrievalScope::Folder(folder_id) = scope {
        query.push_bind(folder_id);
        query.push(
            " UNION ALL SELECT f.id FROM meeting_folders f JOIN folder_scope s ON f.parent_id = s.id) ",
        );
    }
    query.push(
        "SELECT EXISTS(SELECT 1 FROM meetings m LEFT JOIN search_source_state s ON s.meeting_id = m.id WHERE m.id = ",
    );
    query.push_bind(meeting_id);
    query.push(" AND s.source_revision IS ");
    query.push_bind(expected_revision);
    match scope {
        PersistedRetrievalScope::Folder(_) => {
            query.push(" AND m.folder_id IN (SELECT id FROM folder_scope)");
        }
        PersistedRetrievalScope::Meeting(scope_id) => {
            query.push(" AND m.id = ").push_bind(scope_id);
        }
        PersistedRetrievalScope::AllowedMeetingIds(allowed_ids) => {
            query.push(" AND m.id IN (");
            let mut allowed = query.separated(", ");
            for id in allowed_ids {
                allowed.push_bind(id);
            }
            drop(allowed);
            query.push(")");
        }
        PersistedRetrievalScope::All => {}
    }
    query.push(")");
    let present: (bool,) = query
        .build_query_as()
        .fetch_one(pool)
        .await
        .map_err(db_error)?;
    ensure_not_cancelled(cancel)?;
    Ok(present.0)
}

// -- Evidence identity planning ----------------------------------------------

/// One ranked candidate's validated identity against current content.
#[derive(Debug)]
struct PlannedItem {
    evidence_id: String,
    kind: String,
    /// Transcript segment ids the validated range covers (empty for
    /// summary/note items).
    segment_ids: Vec<String>,
    /// True when the identity resolved against the authoritative read.
    valid: bool,
    /// Task 3.2 aliases: (alias evidence id, current segment ids of the alias
    /// range). A missing alias segment omits the alias, never the meeting.
    aliases: Vec<(String, Vec<String>)>,
}

/// The authoritative publication plan for one meeting: current segment rows
/// (matched ranges plus the approved one-segment adjacent neighborhood) and
/// every ranked candidate identity validated against that content.
struct MeetingPlan {
    segments: Vec<FtsSearchResult>,
    /// Chronological position of `segments[i]` in the meeting's current
    /// transcript list (for contiguous-run grouping).
    positions: Vec<usize>,
    items: Vec<PlannedItem>,
}

/// One contiguous retained transcript run: its identity, its exact published
/// text, and every grounded ranked/alias candidate identity.
struct TranscriptGroup {
    start_id: String,
    end_id: String,
    snippet: String,
    evidence_ids: Vec<String>,
}

/// A transcript identity is grounded only when its range is non-empty and
/// EVERY segment it covers survived publication. Both retention lists ask
/// exactly this question - the per-meeting coverage list against everything
/// retained, the per-source list against one contiguous run - so they ask it
/// through one predicate and cannot drift apart.
fn grounded(segment_ids: &[String], segments: &HashSet<&str>) -> bool {
    !segment_ids.is_empty()
        && segment_ids
            .iter()
            .all(|segment| segments.contains(segment.as_str()))
}

/// [`grounded`] refined to a single run: the run every segment of the range
/// belongs to, or `None` when the range is empty, partly unretained, or
/// split across runs. `plan_meeting_evidence` marks a matched range's whole
/// neighborhood, so a fully retained range always lands in one run and this
/// agrees with [`grounded`]; returning `None` rather than a guess is what
/// keeps the two consistent if that ever stops holding.
fn grounded_run(segment_ids: &[String], run_of: &HashMap<&str, usize>) -> Option<usize> {
    let mut runs = segment_ids
        .iter()
        .map(|segment| run_of.get(segment.as_str()).copied());
    let first = runs.next()??;
    runs.all(|run| run == Some(first)).then_some(first)
}

impl MeetingPlan {
    fn retained_summary_ids(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.valid && item.kind == "summary")
            .map(|item| item.evidence_id.clone())
            .collect()
    }

    fn retained_note_ids(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.valid && (item.kind == "note" || item.kind == "notes"))
            .map(|item| item.evidence_id.clone())
            .collect()
    }

    /// Every retained candidate identity for the meeting: ranked items whose
    /// validated identity survived publication plus their Task 3.2 aliases,
    /// in ranked order.
    fn retained_evidence_ids(
        &self,
        summary_retained: bool,
        note_retained: bool,
        retained_segments: &HashSet<&str>,
    ) -> Vec<String> {
        let mut ids = Vec::new();
        for item in &self.items {
            if !item.valid {
                continue;
            }
            match item.kind.as_str() {
                "summary" if summary_retained => ids.push(item.evidence_id.clone()),
                "note" | "notes" if note_retained => ids.push(item.evidence_id.clone()),
                "transcript" if grounded(&item.segment_ids, retained_segments) => {
                    ids.push(item.evidence_id.clone());
                }
                _ => {}
            }
            for (alias_id, alias_segments) in &item.aliases {
                if grounded(alias_segments, retained_segments) {
                    ids.push(alias_id.clone());
                }
            }
        }
        ids
    }

    /// Contiguous retained transcript runs with their exact published text
    /// and grounded candidate identities.
    ///
    /// Identities are assigned in ONE pass over the items against a
    /// segment -> run index, not by rescanning every item for every run.
    fn retained_groups(&self, retained: &HashSet<&str>) -> Vec<TranscriptGroup> {
        let mut runs: Vec<Vec<&FtsSearchResult>> = Vec::new();
        let mut previous: Option<usize> = None;
        for (index, segment) in self.segments.iter().enumerate() {
            if !retained.contains(segment.chunk_id.as_str()) {
                continue;
            }
            let position = self.positions[index];
            match previous {
                Some(last) if last + 1 == position => {
                    runs.last_mut().expect("run in progress").push(segment)
                }
                _ => runs.push(vec![segment]),
            }
            previous = Some(position);
        }
        let mut run_of: HashMap<&str, usize> = HashMap::new();
        for (index, run) in runs.iter().enumerate() {
            for segment in run {
                run_of.insert(segment.chunk_id.as_str(), index);
            }
        }
        let mut evidence_ids: Vec<Vec<String>> = vec![Vec::new(); runs.len()];
        for item in &self.items {
            if !item.valid || item.kind != "transcript" {
                continue;
            }
            let identities = std::iter::once((&item.evidence_id, &item.segment_ids))
                .chain(item.aliases.iter().map(|(id, segments)| (id, segments)));
            for (evidence_id, segment_ids) in identities {
                if let Some(index) = grounded_run(segment_ids, &run_of) {
                    evidence_ids[index].push(evidence_id.clone());
                }
            }
        }
        runs.into_iter()
            .zip(evidence_ids)
            .map(|(run, evidence_ids)| TranscriptGroup {
                start_id: run.first().expect("non-empty run").chunk_id.clone(),
                end_id: run.last().expect("non-empty run").chunk_id.clone(),
                snippet: run
                    .iter()
                    .map(|row| row.snippet.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                evidence_ids,
            })
            .collect()
    }
}

/// Validates every ranked candidate identity for one meeting against the
/// authoritative read and rehydrates transcript windows (plus the approved
/// one-segment adjacent neighborhood) to current segments. Stale identities
/// are marked invalid and never contribute text; the meeting's published text
/// is always the current authoritative content.
fn plan_meeting_evidence(source: &MeetingSource, items: &[&RankedEvidence]) -> MeetingPlan {
    let positions: HashMap<&str, usize> = source
        .transcripts
        .iter()
        .zip(source.transcript_positions.iter().copied())
        .map(|(segment, position)| (segment.id.as_str(), position))
        .collect();
    // Matched positions expanded by the approved one-segment adjacent
    // neighborhood.
    let mut included: HashSet<usize> = HashSet::new();
    let mut planned: Vec<PlannedItem> = Vec::with_capacity(items.len());
    let total = source.transcripts.len();
    for item in items {
        let evidence = &item.evidence;
        let mut plan = PlannedItem {
            evidence_id: evidence.evidence_id.clone(),
            kind: evidence.source_kind.clone(),
            segment_ids: Vec::new(),
            valid: false,
            aliases: Vec::new(),
        };
        match evidence.source_kind.as_str() {
            "summary" => {
                plan.valid = evidence.source_template_id.is_some()
                    && evidence.source_template_id == source.latest_summary_template_id;
            }
            "note" | "notes" => {
                plan.valid = blank(source.notes_markdown.as_deref()).is_some();
            }
            "transcript" => {
                if let Some((start, end)) = resolve_span(
                    evidence.source_start_id.as_deref(),
                    evidence
                        .source_end_id
                        .as_deref()
                        .or(evidence.source_start_id.as_deref()),
                    &positions,
                ) {
                    let authoritative = source.transcripts[start..=end]
                        .iter()
                        .map(|segment| segment.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    // Semantic identities are content-bound at ranking time;
                    // an ID surviving a source edit is not sufficient.
                    plan.valid = evidence.evidence_id.starts_with("fts:")
                        || item.content_fingerprint.is_none()
                        || item.content_fingerprint.as_deref()
                            == Some(sha2::Sha256::digest(authoritative.as_bytes()).as_slice());
                    plan.segment_ids = source.transcripts[start..=end]
                        .iter()
                        .map(|segment| segment.id.clone())
                        .collect();
                    mark_neighborhood(&mut included, start, end, total);
                }
            }
            _ => {}
        }
        // Task 3.2 source aliases (absorbed lexical candidates) are separate
        // identities over the same source; a missing alias segment omits the
        // alias, never the meeting.
        for alias in &evidence.source_aliases {
            if alias.source_kind == "transcript" {
                if let Some((start, end)) = resolve_span(
                    alias.source_start_id.as_deref(),
                    alias
                        .source_end_id
                        .as_deref()
                        .or(alias.source_start_id.as_deref()),
                    &positions,
                ) {
                    plan.aliases.push((
                        alias.evidence_id.clone(),
                        source.transcripts[start..=end]
                            .iter()
                            .map(|segment| segment.id.clone())
                            .collect(),
                    ));
                    mark_neighborhood(&mut included, start, end, total);
                }
            }
        }
        planned.push(plan);
    }
    let mut segments = Vec::new();
    let mut positions = Vec::new();
    for (position, segment) in source.transcripts.iter().enumerate() {
        if !included.contains(&position) {
            continue;
        }
        positions.push(position);
        segments.push(FtsSearchResult {
            meeting_id: source.meeting_id.clone(),
            meeting_title: source.title.clone(),
            chunk_type: "transcript".to_string(),
            chunk_id: segment.id.clone(),
            snippet: segment.text.clone(),
            speaker: segment.speaker.clone(),
            timestamp_label: Some(segment.timestamp.clone()),
            folder_id: None,
            folder_name: source.folder_name.clone(),
            rank: 0.0,
        });
    }
    MeetingPlan {
        segments,
        positions,
        items: planned,
    }
}

/// Positional span of a segment range in the current chronology. Missing,
/// unknown, or reversed ranges are unresolvable (the same rule as ranking's
/// cross-channel merge), never a panic.
fn resolve_span(
    start: Option<&str>,
    end: Option<&str>,
    positions: &HashMap<&str, usize>,
) -> Option<(usize, usize)> {
    let start = start?;
    let end = end.unwrap_or(start);
    let (start_position, end_position) = (positions.get(start)?, positions.get(end)?);
    if start_position > end_position {
        return None;
    }
    Some((*start_position, *end_position))
}

/// The approved adjacent neighborhood: one segment on each side of a matched
/// range, clamped to the current transcript list.
fn mark_neighborhood(included: &mut HashSet<usize>, start: usize, end: usize, total: usize) {
    for position in start.saturating_sub(1)..=(end + 1).min(total.saturating_sub(1)) {
        included.insert(position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::service::{ResolvedScope, RetrievedEvidence, SourceAlias};
    use sqlx::sqlite::SqlitePoolOptions;
    use std::str::FromStr;
    use std::sync::Arc;

    const BUDGET: usize = 12_000;

    async fn pool() -> SqlitePool {
        let options = sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn insert_meeting(pool: &SqlitePool, id: &str, title: &str, folder: Option<&str>) {
        sqlx::query(
            "INSERT INTO meetings (id, title, folder_id, created_at, updated_at) VALUES (?, ?, ?, '2026-08-29', '2026-08-29')",
        )
        .bind(id)
        .bind(title)
        .bind(folder)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn add_transcript(pool: &SqlitePool, id: &str, meeting_id: &str, text: &str, time: f64) {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time) VALUES (?, ?, ?, '10:00', ?)",
        )
        .bind(id)
        .bind(meeting_id)
        .bind(text)
        .bind(time)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn set_notes(pool: &SqlitePool, meeting_id: &str, notes: &str) {
        sqlx::query(
            "INSERT INTO meeting_notes (meeting_id, notes_markdown, created_at, updated_at) VALUES (?, ?, '2026-08-29', '2026-08-29')",
        )
        .bind(meeting_id)
        .bind(notes)
        .execute(pool)
        .await
        .unwrap();
    }

    fn summary_json(markdown: &str) -> String {
        serde_json::json!({ "markdown": markdown }).to_string()
    }

    async fn set_summary(pool: &SqlitePool, meeting_id: &str, template: &str, markdown: &str) {
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES (?, ?, 'completed', '2026-08-29', '2026-08-29', ?)",
        )
        .bind(meeting_id)
        .bind(template)
        .bind(summary_json(markdown))
        .execute(pool)
        .await
        .unwrap();
    }

    fn evidence(
        evidence_id: &str,
        meeting_id: &str,
        kind: &str,
        text: &str,
        start: Option<&str>,
        end: Option<&str>,
        template: Option<&str>,
    ) -> RetrievedEvidence {
        RetrievedEvidence {
            evidence_id: evidence_id.to_string(),
            meeting_id: meeting_id.to_string(),
            meeting_title: "ranked title".to_string(),
            source_kind: kind.to_string(),
            source_start_id: start.map(str::to_string),
            source_end_id: end.map(str::to_string),
            source_template_id: template.map(str::to_string),
            heading: None,
            ordinal: 0,
            text: text.to_string(),
            speaker: None,
            timestamp_label: None,
            provenance: Vec::new(),
            source_aliases: Vec::new(),
        }
    }

    fn ranked(meeting_id: &str, rank: usize) -> RankedMeeting {
        RankedMeeting {
            meeting_id: meeting_id.to_string(),
            rank,
            score: 0.0,
            best_fused_score: 0.0,
            support: 0,
            corroboration: 0,
            title_overlap: 0.0,
            concept_coverage: 0.0,
        }
    }

    fn outcome(
        scope: PersistedRetrievalScope,
        evidence: Vec<RetrievedEvidence>,
        meetings: Vec<RankedMeeting>,
    ) -> RankedRetrieval {
        RankedRetrieval {
            scope: ResolvedScope { scope },
            ranking: crate::retrieval::RankingOutcome {
                evidence: evidence
                    .into_iter()
                    .map(|evidence| crate::retrieval::RankedEvidence {
                        evidence,
                        content_fingerprint: None,
                        fused_rank: 1,
                        fused_score: 1.0,
                        reranker_score: None,
                    })
                    .collect(),
                meetings,
                reranker_used: false,
                rerank_depth: 0,
                rerank_fallback: Some(crate::retrieval::RerankFallback::Unavailable),
                core_terms: Vec::new(),
                terms: crate::retrieval::AggregationTerms::default(),
                title_overlap: HashMap::new(),
                effective_query: String::new(),
                dedupe_degraded: false,
                chronology_omitted_meetings: Vec::new(),
            },
            semantic_fallback: None,
        }
    }

    /// Meeting with three chronological transcript segments (`{id}-s1..s3`).
    async fn seeded_meeting(pool: &SqlitePool, id: &str, title: &str, folder: Option<&str>) {
        insert_meeting(pool, id, title, folder).await;
        for (index, position) in ["first", "middle", "final"].iter().enumerate() {
            add_transcript(
                pool,
                &format!("{id}-s{}", index + 1),
                id,
                &format!("{id} segment {position}"),
                index as f64 + 1.0,
            )
            .await;
        }
    }

    #[tokio::test]
    async fn multi_meeting_budget_retains_every_meeting_in_ranked_order() {
        let pool = pool().await;
        for id in ["m1", "m2", "m3"] {
            seeded_meeting(&pool, id, &id.to_uppercase(), None).await;
        }
        let ranked = outcome(
            PersistedRetrievalScope::All,
            vec![
                evidence("w1", "m1", "transcript", "STALE", Some("m1-s2"), None, None),
                evidence(
                    "w2",
                    "m2",
                    "transcript",
                    "snippet",
                    Some("m2-s2"),
                    None,
                    None,
                ),
                evidence(
                    "w3",
                    "m3",
                    "transcript",
                    "snippet",
                    Some("m3-s2"),
                    None,
                    None,
                ),
            ],
            vec![ranked("m1", 1), ranked("m2", 2), ranked("m3", 3)],
        );
        let context = hydrate_context(&pool, &ranked, 6_000, None).await.unwrap();

        assert_eq!(
            context
                .meetings
                .iter()
                .map(|meeting| meeting.meeting_id.as_str())
                .collect::<Vec<_>>(),
            ["m1", "m2", "m3"],
            "all meetings hydrate in ranked order"
        );
        assert!(context.markdown.chars().count() <= 6_000);
        for meeting_id in ["m1", "m2", "m3"] {
            assert!(
                context.sources.iter().any(
                    |source| source.meeting_id == meeting_id && !source.evidence_ids.is_empty()
                ),
                "{meeting_id} retains evidence-backed sources"
            );
            assert!(context
                .markdown
                .contains(&format!("{meeting_id} segment middle")));
        }
        // Source parity: every source's snippet is published verbatim.
        for source in &context.sources {
            for line in source.snippet.lines() {
                assert!(
                    context.markdown.contains(line),
                    "source snippet line {line:?} must be published"
                );
            }
        }
    }

    #[tokio::test]
    async fn long_first_summary_cannot_starve_other_meetings() {
        let pool = pool().await;
        seeded_meeting(&pool, "m1", "Top", None).await;
        seeded_meeting(&pool, "m2", "Beta", None).await;
        set_notes(&pool, "m1", &"x".repeat(40_000)).await;
        let ranked = outcome(
            PersistedRetrievalScope::All,
            vec![
                evidence("w1", "m1", "note", "ranked note", None, None, None),
                evidence(
                    "w2",
                    "m2",
                    "transcript",
                    "snippet",
                    Some("m2-s2"),
                    None,
                    None,
                ),
            ],
            vec![ranked("m1", 1), ranked("m2", 2)],
        );
        let context = hydrate_context(&pool, &ranked, 8_000, None).await.unwrap();

        // The guaranteed minimum keeps m2's transcript in the context even
        // though the first meeting's notes are enormous.
        assert!(context.markdown.contains("m2 segment middle"));
        assert!(context.markdown.contains("\u{2026} [truncated]"));
        assert!(context.markdown.chars().count() <= 8_000);
        assert_eq!(context.meetings.len(), 2);
    }

    #[tokio::test]
    async fn stale_semantic_identity_is_omitted_and_current_text_published() {
        let pool = pool().await;
        seeded_meeting(&pool, "m1", "Current", None).await;
        // The window's end segment no longer resolves: stale identity.
        let stale_window = evidence(
            "w-stale",
            "m1",
            "transcript",
            "STALE WINDOW TEXT",
            Some("m1-s2"),
            Some("m1-gone"),
            None,
        );
        let good = evidence(
            "w-good",
            "m1",
            "transcript",
            "STALE TOO",
            Some("m1-s2"),
            None,
            None,
        );
        let ranked = outcome(
            PersistedRetrievalScope::All,
            vec![stale_window, good],
            vec![ranked("m1", 1)],
        );
        let context = hydrate_context(&pool, &ranked, 4_000, None).await.unwrap();

        // Current authoritative text replaces the stale semantic text.
        assert!(context.markdown.contains("m1 segment middle"));
        assert!(!context.markdown.contains("STALE"));
        assert!(!context
            .retained_evidence_ids
            .contains(&"w-stale".to_string()));
        assert!(context
            .retained_evidence_ids
            .contains(&"w-good".to_string()));
    }

    #[tokio::test]
    async fn stale_summary_template_is_omitted_and_latest_published() {
        let pool = pool().await;
        seeded_meeting(&pool, "m1", "Current", None).await;
        set_summary(&pool, "m1", "eval", "current summary body").await;
        // The ranked candidate claims the superseded template.
        let stale = evidence(
            "sum-old",
            "m1",
            "summary",
            "OLD SUMMARY TEXT",
            None,
            None,
            Some("superseded-template"),
        );
        let ranked = outcome(
            PersistedRetrievalScope::All,
            vec![stale],
            vec![ranked("m1", 1)],
        );
        let context = hydrate_context(&pool, &ranked, 4_000, None).await.unwrap();

        // The latest non-empty summary still publishes (from the current
        // template); the stale-template evidence id is omitted.
        assert!(context.markdown.contains("current summary body"));
        assert!(!context.markdown.contains("OLD SUMMARY"));
        assert!(!context
            .retained_evidence_ids
            .contains(&"sum-old".to_string()));
        let summary = context
            .sources
            .iter()
            .find(|source| source.source_kind == "summary")
            .expect("published summary has a source");
        assert_eq!(summary.source_template_id.as_deref(), Some("eval"));
    }

    #[tokio::test]
    async fn moved_out_of_folder_after_ranking_is_omitted() {
        let pool = pool().await;
        sqlx::query("INSERT INTO meeting_folders (id, name, created_at) VALUES ('f1', 'In', '2026-08-29'), ('f2', 'Outside', '2026-08-29')")
            .execute(&pool)
            .await
            .unwrap();
        seeded_meeting(&pool, "m1", "Moved", Some("f1")).await;
        seeded_meeting(&pool, "m2", "Kept", Some("f1")).await;
        let ranked = outcome(
            PersistedRetrievalScope::Folder("f1".to_string()),
            vec![
                evidence(
                    "w1",
                    "m1",
                    "transcript",
                    "snippet",
                    Some("m1-s2"),
                    None,
                    None,
                ),
                evidence(
                    "w2",
                    "m2",
                    "transcript",
                    "snippet",
                    Some("m2-s2"),
                    None,
                    None,
                ),
            ],
            vec![ranked("m1", 1), ranked("m2", 2)],
        );
        // Controlled move outside the folder after ranking, before hydration.
        sqlx::query("UPDATE meetings SET folder_id = 'f2' WHERE id = 'm1'")
            .execute(&pool)
            .await
            .unwrap();

        let context = hydrate_context(&pool, &ranked, 4_000, None).await.unwrap();

        assert_eq!(
            context
                .meetings
                .iter()
                .map(|meeting| meeting.meeting_id.as_str())
                .collect::<Vec<_>>(),
            ["m2"],
            "the moved meeting is omitted; the in-scope one stays"
        );
        assert!(!context
            .sources
            .iter()
            .any(|source| source.meeting_id == "m1"));
        assert!(context.markdown.contains("m2 segment middle"));
        assert!(!context.markdown.contains("Moved"));
    }

    #[tokio::test]
    async fn exact_and_allowed_scopes_fence_defective_existing_ranked_meetings() {
        for scope in [
            PersistedRetrievalScope::Meeting("m1".to_string()),
            PersistedRetrievalScope::AllowedMeetingIds(vec!["m1".to_string()]),
        ] {
            let pool = pool().await;
            seeded_meeting(&pool, "m1", "Allowed", None).await;
            seeded_meeting(&pool, "m2", "Unauthorized", None).await;
            let ranked = outcome(
                scope,
                vec![
                    evidence(
                        "allowed",
                        "m1",
                        "transcript",
                        "stale",
                        Some("m1-s2"),
                        None,
                        None,
                    ),
                    evidence(
                        "unauthorized",
                        "m2",
                        "transcript",
                        "stale",
                        Some("m2-s2"),
                        None,
                        None,
                    ),
                ],
                vec![ranked("m1", 1), ranked("m2", 2)],
            );

            let context = hydrate_context(&pool, &ranked, BUDGET, None).await.unwrap();
            assert_eq!(
                context
                    .meetings
                    .iter()
                    .map(|meeting| meeting.meeting_id.as_str())
                    .collect::<Vec<_>>(),
                ["m1"]
            );
            assert!(context.markdown.contains("m1 segment middle"));
            assert!(!context.markdown.contains("m2 segment middle"));
            assert!(!context
                .sources
                .iter()
                .any(|source| source.meeting_id == "m2"));
        }
    }

    #[tokio::test]
    async fn cancelled_hydration_returns_typed_error() {
        let pool = pool().await;
        seeded_meeting(&pool, "m1", "M", None).await;
        let ranked = outcome(
            PersistedRetrievalScope::All,
            vec![evidence(
                "w1",
                "m1",
                "transcript",
                "s",
                Some("m1-s2"),
                None,
                None,
            )],
            vec![ranked("m1", 1)],
        );
        let token = CancellationToken::new();
        token.cancel();
        let error = hydrate_context(&pool, &ranked, 1_000, Some(&token))
            .await
            .unwrap_err();
        assert!(matches!(error, RetrievalError::Cancelled));
    }

    #[tokio::test]
    async fn cancellation_at_final_fence_returns_typed_error_without_context() {
        let pool = pool().await;
        seeded_meeting(&pool, "m1", "M", None).await;
        let ranked = outcome(
            PersistedRetrievalScope::All,
            vec![evidence(
                "w1",
                "m1",
                "transcript",
                "s",
                Some("m1-s2"),
                None,
                None,
            )],
            vec![ranked("m1", 1)],
        );
        let token = CancellationToken::new();
        let pause = Arc::new(HydrationPause::new());
        let (sender, mut loaded) = tokio::sync::mpsc::unbounded_channel();
        pause.arm(sender);
        let task_pool = pool.clone();
        let task_pause = Arc::clone(&pause);
        let task_token = token.clone();
        let task = tokio::spawn(async move {
            hydrate_with_pause(&task_pool, &ranked, BUDGET, Some(&task_token), &task_pause).await
        });
        loaded.recv().await.expect("meeting loaded");
        token.cancel();
        pause.release();

        assert!(matches!(
            task.await.unwrap(),
            Err(RetrievalError::Cancelled)
        ));
    }

    #[tokio::test]
    async fn sources_exactly_match_retained_markdown_and_identities() {
        let pool = pool().await;
        seeded_meeting(&pool, "m1", "Full", None).await;
        set_summary(&pool, "m1", "eval", "Authoritative summary body").await;
        set_notes(&pool, "m1", "Authoritative notes body").await;
        let note_item = evidence("n1", "m1", "note", "ranked note", None, None, None);
        let mut window = evidence(
            "w1",
            "m1",
            "transcript",
            "STALE WINDOW",
            Some("m1-s2"),
            Some("m1-s3"),
            None,
        );
        window.source_aliases.push(SourceAlias {
            evidence_id: "alias-1".to_string(),
            source_kind: "transcript".to_string(),
            source_start_id: Some("m1-s2".to_string()),
            source_end_id: None,
            text: "lexical alias".to_string(),
        });
        let ranked = outcome(
            PersistedRetrievalScope::All,
            vec![note_item, window],
            vec![ranked("m1", 1)],
        );
        let context = hydrate_context(&pool, &ranked, 8_000, None).await.unwrap();

        let kinds: Vec<&str> = context
            .sources
            .iter()
            .map(|source| source.source_kind.as_str())
            .collect();
        assert_eq!(kinds, ["summary", "notes", "transcript"]);
        let summary = &context.sources[0];
        assert_eq!(summary.source_template_id.as_deref(), Some("eval"));
        assert!(context.markdown.contains("Authoritative summary body"));
        let transcript = &context.sources[2];
        // The window (s2..s3) plus the approved adjacent neighborhood covers
        // the whole run s1..s3.
        assert_eq!(transcript.source_start_id.as_deref(), Some("m1-s1"));
        assert_eq!(transcript.source_end_id.as_deref(), Some("m1-s3"));
        // Complete coverage: no partial-coverage notice.
        assert!(!context.markdown.contains("Partial transcript coverage"));
        assert_eq!(
            context.retained_evidence_ids,
            ["n1".to_string(), "w1".to_string(), "alias-1".to_string()]
        );
        assert!(context.markdown.chars().count() <= 8_000);
    }

    #[tokio::test]
    async fn unicode_budget_is_respected_with_safe_truncation() {
        let pool = pool().await;
        let crab = "\u{1F980}";
        insert_meeting(&pool, "m1", &format!("{crab} Emoji"), None).await;
        set_summary(&pool, "m1", "eval", &crab.repeat(1000)).await;
        let ranked = outcome(
            PersistedRetrievalScope::All,
            vec![evidence(
                "w1",
                "m1",
                "transcript",
                "snippet",
                Some("m1-s2"),
                None,
                None,
            )],
            vec![ranked("m1", 1)],
        );
        let context = hydrate_context(&pool, &ranked, 500, None).await.unwrap();
        assert!(context.markdown.chars().count() <= 500);
        assert!(context.markdown.contains("\u{2026} [truncated]"));
        assert!(std::str::from_utf8(context.markdown.as_bytes()).is_ok());
    }

    /// Spawns hydration armed at the post-load pause and returns the load
    /// signal receiver, the gate, and the task.
    fn paused_hydration(
        pool: &SqlitePool,
        ranked: RankedRetrieval,
    ) -> (
        tokio::sync::mpsc::UnboundedReceiver<()>,
        Arc<HydrationPause>,
        tokio::task::JoinHandle<Result<HydratedContext, RetrievalError>>,
    ) {
        let pause = Arc::new(HydrationPause::new());
        let (sender, loaded) = tokio::sync::mpsc::unbounded_channel();
        pause.arm(sender);
        let task_pool = pool.clone();
        let pause_task = Arc::clone(&pause);
        let task = tokio::spawn(async move {
            hydrate_with_pause(&task_pool, &ranked, BUDGET, None, &pause_task).await
        });
        (loaded, pause, task)
    }

    #[tokio::test]
    async fn moved_after_load_is_omitted_before_publication() {
        let pool = pool().await;
        sqlx::query(
            "INSERT INTO meeting_folders (id, name, created_at) VALUES ('f1', 'In', '2026-08-29'), ('f2', 'Outside', '2026-08-29')",
        )
        .execute(&pool)
        .await
        .unwrap();
        seeded_meeting(&pool, "m1", "Moved", Some("f1")).await;
        let ranked = outcome(
            PersistedRetrievalScope::Folder("f1".to_string()),
            vec![evidence(
                "w1",
                "m1",
                "transcript",
                "STALE",
                Some("m1-s2"),
                None,
                None,
            )],
            vec![ranked("m1", 1)],
        );
        let (mut loaded, pause, task) = paused_hydration(&pool, ranked);
        loaded.recv().await.expect("meeting loaded");
        // Controlled move outside the folder after loading, before the final
        // retention recheck.
        sqlx::query("UPDATE meetings SET folder_id = 'f2' WHERE id = 'm1'")
            .execute(&pool)
            .await
            .unwrap();
        pause.release();
        let context = task.await.unwrap().unwrap();

        assert!(context.meetings.is_empty(), "moved meeting is omitted");
        assert!(context.sources.is_empty());
        assert!(context.retained_evidence_ids.is_empty());
        assert!(!context.markdown.contains("Moved"));
    }

    #[tokio::test]
    async fn deleted_after_load_is_omitted_before_publication() {
        let pool = pool().await;
        seeded_meeting(&pool, "m1", "Deleted", None).await;
        let ranked = outcome(
            PersistedRetrievalScope::All,
            vec![evidence(
                "w1",
                "m1",
                "transcript",
                "STALE",
                Some("m1-s2"),
                None,
                None,
            )],
            vec![ranked("m1", 1)],
        );
        let (mut loaded, pause, task) = paused_hydration(&pool, ranked);
        loaded.recv().await.expect("meeting loaded");
        sqlx::query("DELETE FROM meetings WHERE id = 'm1'")
            .execute(&pool)
            .await
            .unwrap();
        pause.release();
        let context = task.await.unwrap().unwrap();

        assert!(context.meetings.is_empty(), "deleted meeting is omitted");
        assert!(context.sources.is_empty());
        assert!(context.retained_evidence_ids.is_empty());
        assert!(!context.markdown.contains("Deleted"));
    }

    #[tokio::test]
    async fn changed_source_revision_during_load_is_omitted() {
        let pool = pool().await;
        seeded_meeting(&pool, "m1", "Changed", None).await;
        let ranked = outcome(
            PersistedRetrievalScope::All,
            vec![evidence(
                "w1",
                "m1",
                "transcript",
                "snippet",
                Some("m1-s2"),
                None,
                None,
            )],
            vec![ranked("m1", 1)],
        );
        let (mut loaded, pause, task) = paused_hydration(&pool, ranked);
        loaded.recv().await.expect("meeting loaded");
        // A real content change during the load: the source revision moves
        // forward before the retention recheck.
        sqlx::query(
            "UPDATE search_source_state SET source_revision = source_revision + 1 WHERE meeting_id = 'm1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        pause.release();
        let context = task.await.unwrap().unwrap();

        assert!(context.meetings.is_empty(), "changed meeting is omitted");
        assert!(context.sources.is_empty());
        assert!(context.retained_evidence_ids.is_empty());
    }
}
