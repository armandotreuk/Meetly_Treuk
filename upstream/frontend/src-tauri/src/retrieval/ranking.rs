//! Rank fusion, stable evidence deduplication, meeting aggregation, and local
//! reranking (Sprint 3 Task 3.2).
//!
//! Consumes the Task 3.1 channel-ranked candidates and produces the stable
//! meeting ranking and evidence ordering that Task 3.3 hydration and Task 3.4
//! broad-Chat rollout build on. The Sprint 1 policy is the runtime safety
//! baseline (`docs/hybrid-rag/task-1.3-final-selection.md` §3 and the
//! architecture "Approved Sprint 1 Bundle And Runtime Contract"): reciprocal
//! rank fusion over the lexical and semantic channels (raw BM25/cosine values
//! never enter fusion, and ties resolve deterministically), meeting
//! aggregation from the best fused evidence with a capped support
//! contribution, the title-overlap term, and a bounded local cross-encoder
//! head under the approved Chat runtime constants (depth 50, batch 1, ORT
//! intra-op 4, gamma 0, support cap 3). Task 3.2 adds one meeting-quality
//! term on the same score scale: the bounded authoritative-corroboration
//! credit ([`CORROBORATION_DELTA`]), which counts independently authored
//! document classes per meeting and corrects the measured thin-lookalike
//! failure without rewarding chunk volume. When reranking is available, scored
//! candidates qualify or reject their class; candidates outside the bounded
//! head are score-unknown and neutral, so tail volume cannot erase a class.
//!
//! Cross-channel identity: an FTS transcript segment and a semantic sliding
//! window covering it are the same evidence. Segment IDs are random UUIDs, so
//! identity is resolved by POSITION in the meeting's authoritative segment
//! chronology (the same ordering the chunker builds windows from) rather than
//! by lexicographic range comparison or any synthetic shared ID; FTS
//! whole-template summary rows and whole-blob note rows merge into the
//! semantic section documents of the same source. Merged candidates
//! accumulate both channels' provenance, so lexical/semantic and
//! original/rewritten/core-term origins survive fusion.
//!
//! Every failure below the request level degrades deterministically: a
//! reranker component error keeps the fused ordering, and only user/stream
//! cancellation aborts the request as a typed error (never a fallback).
//! Logs carry counts and fallback tags only - never query or candidate
//! text. Scores are rank-fusion ordering quantities, never calibrated
//! confidence.

use std::collections::{HashMap, HashSet};

use sha2::Digest;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::database::repositories::retrieval::RetrievalRepository;
use crate::retrieval::model::{cached_model, RetrievalModelError};
use crate::retrieval::service::{
    normalize_core_token, EvidenceProvenance, LexicalMode, QueryVariantKind, RetrievalChannel,
    RetrievalError, RetrievedEvidence, SourceAlias,
};
use crate::retrieval::worker::{RetrievalLifecycle, SchedulerRejection};

// -- Selected Task 3.2 constants ---------------------------------------------

/// Reciprocal-rank fusion constant `k` (Task 1.3 final selection §3).
pub const RRF_K: f64 = 5.0;
/// Semantic (vector) channel weight.
pub const W_VECTOR: f64 = 1.0;
/// Lexical (FTS) channel weight from the selected feasible configuration.
pub const W_LEXICAL: f64 = 0.0;
/// Meeting support contribution weight from the selected feasible
/// configuration; support remains capped and distinct-region based.
pub const SUPPORT_ALPHA: f64 = 1.5;
/// Meeting title-overlap weight from the selected feasible configuration.
pub const TITLE_BETA: f64 = 0.0;
/// Cross-encoder contribution to MEETING ranking, in rank space
/// (`gamma / (k + meeting_rank)`). Sprint 1 measured `gamma = 0` for the
/// approved candidate: the reranker orders the final evidence but must not
/// silently re-rank meetings. Any deviation needs a documented evidence
/// addendum, not silent tuning.
pub const RERANK_GAMMA: f64 = 0.0;
/// Approved capped support contribution: a meeting earns support for at most
/// this many of its evidence chunks, so long meetings cannot win by volume.
pub const SUPPORT_CAP: usize = 3;
/// Fused-order window inside which supporting evidence is counted (the
/// Sprint 1 aggregation policy's support window).
pub const SUPPORT_WINDOW: usize = 20;
/// Distinct-concept-coverage weight (`architecture.md:1271-1285`: meeting
/// ranking considers "Distinct meaningful query concepts covered across
/// evidence"). Sprint 1 measured no weight for this term, so its value is
/// produced by the Task 3.2 constants-isolation protocol like every other
/// tuned weight, never by judgement.
///
/// The selected feasible configuration returned `1.0`: distinct-concept
/// coverage is computed and exposed per meeting
/// ([`RankedMeeting::concept_coverage`]), and this weight was the winner's
/// value after full-gate eligibility and held-out objective ordering. The
/// measure remains reportable independently of its contribution to the score.
pub const CONCEPT_DELTA: f64 = 1.0;
/// Sprint 1 approved Chat rerank depth: `floor(900 ms / measured solo p95)`
/// capped at the measured 50-pair `RERANK_SET` ceiling (measured solo p95
/// 14.4 ms for the approved mmarco-quint8 session -> 720 ms at depth 50,
/// inside the 900 ms sub-budget; task-1.3-final-selection.md §6). Sprint 1
/// evidence contains no margin-based adaptive-depth policy, so the entire
/// approved policy is this deterministic input-clamped depth: identical
/// inputs always select identical depth and ordering, and wall-clock time is
/// never consulted.
pub const CHAT_RERANK_DEPTH: usize = 50;
pub const SEARCH_RERANK_DEPTH: usize = 25;

/// Authoritative-corroboration credit per additional document class
/// (transcript / summary / notes) whose evidence the request surfaced for a
/// meeting in the bounded fused candidate universe. This is the Task 3.2
/// meeting-quality policy (user-authorized 2026-08-31): fusion and the
/// reranker score CHUNKS, so a thin meeting whose single chunk tops both
/// channels earns the maximum fused score while a meeting whose answer is
/// spread across independently authored artifacts is demoted - the measured
/// shape of every fixture-distractor failure. The credit is grounded in
/// authorship independence, not volume: transcript, summary, and notes are
/// separately authored artifacts, so a query surfacing evidence from more of
/// them is stronger evidence that the meeting substantively concerns the
/// query. It is BOUNDED (at most two classes beyond the first, so the term
/// cannot grow with meeting length) and VOLUME-IMMUNE (duplicated or
/// overlapping transcript chunks all map to the one transcript class, so
/// chunk volume cannot buy corroboration).
///
/// Calibration (one interpretation, not a sweep): 0.6 is the smallest
/// 0.1-step above the largest measured corroboration deficit (0.566) between
/// a correct meeting and a winning lookalike decoy across the failing
/// distractor pairs at the approved baseline constants. It is expressed
/// relative to the same score scale as the support and title terms and is
/// deliberately larger than one full capped-support unit (0.5): a second
/// independent artifact agreeing with the query outweighs volume inside one
/// artifact.
///
/// ponytail: the class set is closed and the credit is class-count only -
/// it cannot tell a documented-but-irrelevant meeting from a documented
/// relevant one. Upgrade path: per-class relevance weighting once a
/// calibrated signal exists.
pub const CORROBORATION_DELTA: f64 = 0.6;

/// Ranking-stage constants. [`RankingConfig::chat`] is the approved Sprint 1
/// safety baseline. The evaluation harness may execute other candidates against
/// the same production code path, but a candidate is not accepted until the
/// unchanged production gate passes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankingConfig {
    pub rrf_k: f64,
    pub w_vector: f64,
    pub w_lexical: f64,
    pub support_alpha: f64,
    pub title_beta: f64,
    pub rerank_gamma: f64,
    /// Distinct-concept-coverage weight (see [`CONCEPT_DELTA`]).
    pub concept_delta: f64,
    /// Authoritative-corroboration credit per additional document class
    /// (see [`CORROBORATION_DELTA`]).
    pub corroboration_delta: f64,
    pub support_cap: usize,
    pub support_window: usize,
    pub rerank_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RankingMode {
    Hybrid,
    LexicalOnly,
}

impl RankingConfig {
    /// The selected feasible Chat configuration from the Task 3.2
    /// constants-isolation protocol.
    pub const fn chat() -> Self {
        Self {
            rrf_k: RRF_K,
            w_vector: W_VECTOR,
            w_lexical: W_LEXICAL,
            support_alpha: SUPPORT_ALPHA,
            title_beta: TITLE_BETA,
            rerank_gamma: RERANK_GAMMA,
            concept_delta: CONCEPT_DELTA,
            corroboration_delta: CORROBORATION_DELTA,
            support_cap: SUPPORT_CAP,
            support_window: SUPPORT_WINDOW,
            rerank_depth: CHAT_RERANK_DEPTH,
        }
    }

    pub const fn search() -> Self {
        Self {
            rerank_depth: SEARCH_RERANK_DEPTH,
            ..Self::chat()
        }
    }

    pub const fn for_purpose(purpose: crate::retrieval::service::RetrievalPurpose) -> Self {
        match purpose {
            crate::retrieval::service::RetrievalPurpose::Search => Self::search(),
            crate::retrieval::service::RetrievalPurpose::Chat
            | crate::retrieval::service::RetrievalPurpose::Context => Self::chat(),
        }
    }
}

/// One candidate in fused order with its Task 3.1 provenance intact.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedEvidence {
    pub evidence: RetrievedEvidence,
    pub content_fingerprint: Option<Vec<u8>>,
    /// 1-based position in the fused evidence order.
    pub fused_rank: usize,
    /// Rank-fusion score (reciprocal-rank sum). A diagnostic ordering
    /// quantity, never a calibrated confidence.
    pub fused_score: f64,
    /// Cross-encoder logit when the candidate was reranked.
    pub reranker_score: Option<f32>,
}

/// One meeting in the aggregated ranking order.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedMeeting {
    pub meeting_id: String,
    /// 1-based meeting rank under the approved aggregation policy.
    pub rank: usize,
    /// Aggregation score:
    /// `k * best_fused + alpha * (min(support, cap) / cap) + beta * title
    ///  + delta * concept_coverage + corroboration * corroboration_delta`
    /// plus the gamma-weighted reranker rank term. The support term is
    /// NORMALIZED by `support_cap`, so `alpha` weights a 0..=1 fraction, not
    /// the raw capped count. A diagnostic ordering quantity, never a
    /// calibrated confidence.
    pub score: f64,
    pub best_fused_score: f64,
    /// DISTINCT supporting source regions inside the fused window, capped.
    /// Overlapping semantic windows over the same transcript span collapse
    /// to one region, so duplicate coverage cannot inflate this count.
    pub support: usize,
    /// Number of DISTINCT authoritative document classes (transcript /
    /// summary / notes) whose relevant evidence the request surfaced for this
    /// meeting. When reranking is unavailable, class presence is used;
    /// otherwise scored candidates qualify or reject a class; unscored bounded
    /// tail candidates are neutral. Profiles and title signals are not
    /// evidence and are not counted. See
    /// [`CORROBORATION_DELTA`].
    pub corroboration: usize,
    /// Fraction of query core terms present in the meeting title.
    pub title_overlap: f64,
    /// Fraction of DISTINCT query core terms covered across this meeting's
    /// evidence text - the architecture's separate diversity measure, scored
    /// independently of the capped support count.
    pub concept_coverage: f64,
}

/// Why the fused ordering was kept instead of a reranked one. Cancellation is
/// never a fallback; it is always the typed [`RetrievalError::Cancelled`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RerankFallback {
    Unavailable,
    SchedulerRejected,
    RerankerError,
}

impl RerankFallback {
    fn tag(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::SchedulerRejected => "scheduler_rejected",
            Self::RerankerError => "reranker_error",
        }
    }
}

/// The Task 3.2 ranking outcome: final evidence order, meeting order, and
/// whether the local cross-encoder served the request. The authoritative
/// effective-query/core-term pair and the title-overlap map computed from it
/// travel with the outcome, so downstream consumers never re-derive the
/// policy.
#[derive(Debug, Clone, PartialEq)]
pub struct RankingOutcome {
    pub evidence: Vec<RankedEvidence>,
    pub meetings: Vec<RankedMeeting>,
    pub reranker_used: bool,
    /// The deterministic depth the policy selected for this request.
    pub rerank_depth: usize,
    pub rerank_fallback: Option<RerankFallback>,
    /// Normalized core terms of the stripped original query (not the rewritten
    /// effective retrieval query).
    pub core_terms: Vec<String>,
    /// The aggregation terms this request computed, exposed so evaluation
    /// re-weights the SAME derived inputs instead of re-deriving them.
    pub terms: AggregationTerms,
    /// Title overlap per meeting, computed from [`Self::core_terms`].
    pub title_overlap: HashMap<String, f64>,
    /// The authoritative effective query this request reranked against
    /// (folder operators stripped, trimmed). Exposed so evaluation and
    /// downstream consumers reuse the exact production string instead of
    /// re-deriving it.
    pub effective_query: String,
    /// True when the authoritative segment-chronology read failed, so
    /// cross-channel transcript merging was skipped for this request. The
    /// output is still scope-correct, but an FTS segment and the semantic
    /// window covering it may appear as two candidates instead of one —
    /// callers must be able to tell that apart from a healthy merge.
    pub dedupe_degraded: bool,
    pub chronology_omitted_meetings: Vec<String>,
}

fn ensure_not_cancelled(cancel: &CancellationToken) -> Result<(), RetrievalError> {
    if cancel.is_cancelled() {
        Err(RetrievalError::Cancelled)
    } else {
        Ok(())
    }
}

// -- Cross-channel deduplication ---------------------------------------------

/// Chronological segment IDs per meeting, resolved from authoritative rows
/// through [`RetrievalRepository::ordered_transcript_segment_ids`].
pub type SegmentOrder = HashMap<String, Vec<String>>;

/// Segment-ID -> chronological position, per meeting. Built ONCE per pass so
/// every range lookup is O(1): resolving ranges by scanning the segment list
/// instead costs one full scan per candidate, and a meeting's chronology is
/// bounded at 10,000 segments, so a request with a couple of hundred
/// candidates over a long meeting would spend millions of string comparisons
/// on the interactive retrieval path rebuilding what one map already answers.
type SegmentPositions<'a> = HashMap<&'a str, HashMap<&'a str, usize>>;

fn segment_positions(segment_order: &SegmentOrder) -> SegmentPositions<'_> {
    segment_order
        .iter()
        .map(|(meeting_id, segments)| {
            (
                meeting_id.as_str(),
                segments
                    .iter()
                    .enumerate()
                    .map(|(position, segment)| (segment.as_str(), position))
                    .collect(),
            )
        })
        .collect()
}

/// Merges cross-channel duplicates into one stable identity per source
/// region, accumulating provenance:
///
/// - transcript: an FTS segment candidate absorbed by the semantic window
///   whose authoritative segment range covers it (positional resolution).
///   The range metadata must be complete and well ordered — missing,
///   unknown, or reversed ranges are non-mergeable, never a panic;
/// - summary and notes: FTS rows cover the whole template/blob while
///   semantic documents cover individual sections/windows, so there is no
///   authoritative matching region identity between them; the lexical
///   candidate is retained separately with its own citable text and
///   provenance rather than merged or dropped;
/// - title candidates are selection signals, not evidence: they feed the
///   title-overlap aggregation term and are not carried into the ranked
///   evidence output.
///
/// When no semantic counterpart exists the lexical candidate stands alone,
/// so lexical availability never depends on semantic health.
pub fn dedupe_candidates(
    candidates: Vec<RetrievedEvidence>,
    segment_order: &SegmentOrder,
) -> Vec<RetrievedEvidence> {
    // Positional window coverage: (meeting, segment) -> absorbing window,
    // chosen deterministically when overlapping windows cover one segment.
    // Windows with missing, unknown, or reversed ranges are non-mergeable.
    let positions = segment_positions(segment_order);
    let mut coverage: HashMap<(String, String), String> = HashMap::new();
    let mut window_quality: HashMap<String, (usize, u8, i64)> = HashMap::new();
    for candidate in candidates.iter() {
        if !is_semantic(candidate) || candidate.source_kind != "transcript" {
            continue;
        }
        let (Some(start), Some(end)) = (&candidate.source_start_id, &candidate.source_end_id)
        else {
            continue;
        };
        let Some(meeting_positions) = positions.get(candidate.meeting_id.as_str()) else {
            continue;
        };
        let (Some(start_position), Some(end_position)) = (
            meeting_positions.get(start.as_str()),
            meeting_positions.get(end.as_str()),
        ) else {
            continue;
        };
        if start_position > end_position {
            // Reversed (corrupt) range: non-mergeable, never a panic.
            continue;
        }
        let quality = semantic_quality(candidate);
        window_quality.insert(candidate.evidence_id.clone(), quality);
        let segments = &segment_order[candidate.meeting_id.as_str()];
        for segment in &segments[*start_position..=*end_position] {
            let key = (candidate.meeting_id.clone(), segment.clone());
            match coverage.get(&key) {
                Some(existing_id)
                    if window_quality.get(existing_id).copied().unwrap_or(quality) <= quality => {}
                _ => {
                    coverage.insert(key, candidate.evidence_id.clone());
                }
            }
        }
    }

    // Absorption decisions for transcript lexical candidates only; every
    // other lexical candidate is retained separately.
    let mut merged_aliases: HashMap<String, Vec<SourceAlias>> = HashMap::new();
    let mut output: Vec<RetrievedEvidence> = Vec::new();
    for candidate in candidates.into_iter() {
        if !is_semantic(&candidate) && candidate.source_kind == "transcript" {
            let target_id: Option<String> =
                candidate.source_start_id.as_deref().and_then(|segment| {
                    coverage
                        .get(&(candidate.meeting_id.clone(), segment.to_string()))
                        .cloned()
                });
            if let Some(target_id) =
                target_id.filter(|id| Some(id.as_str()) != Some(candidate.evidence_id.as_str()))
            {
                merged_aliases
                    .entry(target_id)
                    .or_default()
                    .push(SourceAlias {
                        evidence_id: candidate.evidence_id,
                        source_kind: candidate.source_kind,
                        source_start_id: candidate.source_start_id,
                        source_end_id: candidate.source_end_id,
                        text: candidate.text,
                        provenance: candidate.provenance,
                    });
                continue;
            }
        }
        if candidate.source_kind == "title" {
            // Title candidates are selection signals, not evidence: they
            // feed the title-overlap aggregation term only.
            continue;
        }
        output.push(candidate);
    }
    for candidate in &mut output {
        if let Some(aliases) = merged_aliases.remove(&candidate.evidence_id) {
            for alias in aliases {
                if !candidate
                    .source_aliases
                    .iter()
                    .any(|existing| existing == &alias)
                {
                    candidate.source_aliases.push(alias);
                }
            }
        }
    }
    output
}

/// Distinct source REGION per candidate, so diversity is measured over
/// covered source area rather than chunk count.
///
/// `architecture.md:1271-1285` requires per-meeting contribution to be capped
/// AND diversity to be measured separately. Counting chunks conflates the
/// two: the chunker emits overlapping transcript windows by design
/// (`overlap_tokens`), and [`dedupe_candidates`] merges lexical rows INTO
/// windows but never merges windows with each other, so two windows over
/// substantially the same span would otherwise earn two units of support for
/// one region of source.
///
/// Transcript candidates whose segment range resolves are grouped into merged
/// (transitively overlapping) intervals per meeting; every candidate in one
/// merged interval shares a region key. Anything else - summary sections,
/// note windows, unresolvable ranges - is its own region, keyed by evidence
/// ID, which preserves today's behavior for those kinds.
pub fn coverage_regions(
    candidates: &[RetrievedEvidence],
    segment_order: &SegmentOrder,
) -> HashMap<String, String> {
    let positions = segment_positions(segment_order);
    let mut spans: HashMap<&str, Vec<(usize, usize, &str)>> = HashMap::new();
    let mut regions: HashMap<String, String> = HashMap::new();
    for candidate in candidates {
        let resolved = resolve_span(candidate, &positions);
        match resolved {
            Some((start, end)) => spans
                .entry(candidate.meeting_id.as_str())
                .or_default()
                .push((start, end, candidate.evidence_id.as_str())),
            None => {
                regions.insert(
                    candidate.evidence_id.clone(),
                    format!(
                        "{}:evidence:{}",
                        candidate.meeting_id, candidate.evidence_id
                    ),
                );
            }
        }
    }
    for (meeting_id, mut meeting_spans) in spans {
        // Deterministic merge order; ties by evidence ID so the region key a
        // candidate receives never depends on candidate insertion order.
        meeting_spans.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(right.2))
        });
        let mut region_start = 0usize;
        let mut region_end = 0usize;
        let mut region_index = 0usize;
        for (position, (start, end, evidence_id)) in meeting_spans.iter().enumerate() {
            if position == 0 {
                region_start = *start;
                region_end = *end;
            } else if *start > region_end {
                // Disjoint from the region being accumulated: a new region.
                region_index += 1;
                region_start = *start;
                region_end = *end;
            } else {
                region_end = region_end.max(*end);
            }
            regions.insert(
                (*evidence_id).to_string(),
                format!("{meeting_id}:span:{region_index}:{region_start}"),
            );
        }
    }
    regions
}

/// The candidate's authoritative segment-position span, when it resolves,
/// against the prebuilt [`segment_positions`] index.
fn resolve_span(
    candidate: &RetrievedEvidence,
    positions: &SegmentPositions<'_>,
) -> Option<(usize, usize)> {
    if candidate.source_kind != "transcript" {
        return None;
    }
    let segments = positions.get(candidate.meeting_id.as_str())?;
    let start = *segments.get(candidate.source_start_id.as_deref()?)?;
    let end = match candidate.source_end_id.as_deref() {
        Some(end) => *segments.get(end)?,
        None => start,
    };
    (start <= end).then_some((start, end))
}

/// Distinct query concepts covered across a meeting's evidence text: the
/// fraction of core terms that appear somewhere in the meeting's retained
/// evidence. This is the architecture's "Distinct meaningful query concepts
/// covered across evidence", measured over CONTENT and therefore independent
/// of how many chunks carry it - unlike the capped support count.
pub fn concept_coverage(
    candidates: &[RetrievedEvidence],
    core_terms: &[String],
) -> HashMap<String, f64> {
    let mut tokens_by_meeting: HashMap<&str, HashSet<String>> = HashMap::new();
    for candidate in candidates {
        let entry = tokens_by_meeting
            .entry(candidate.meeting_id.as_str())
            .or_default();
        for token in candidate
            .text
            .split(|character: char| !character.is_alphanumeric())
            .filter(|token| !token.is_empty())
        {
            entry.insert(normalize_core_token(token));
        }
    }
    tokens_by_meeting
        .into_iter()
        .map(|(meeting_id, tokens)| {
            let hits = core_terms
                .iter()
                .filter(|term| tokens.contains(term.as_str()))
                .count();
            (
                meeting_id.to_string(),
                hits as f64 / core_terms.len().max(1) as f64,
            )
        })
        .collect()
}

fn is_semantic(candidate: &RetrievedEvidence) -> bool {
    all_provenance(candidate).any(|provenance| provenance.channel == RetrievalChannel::Semantic)
}

fn all_provenance<'a>(
    candidate: &'a RetrievedEvidence,
) -> impl Iterator<Item = &'a EvidenceProvenance> {
    candidate.provenance.iter().chain(
        candidate
            .source_aliases
            .iter()
            .flat_map(|alias| alias.provenance.iter()),
    )
}

/// The closed set of independently authored document classes corroboration
/// counts. Lexical note rows (`note`) and semantic note-section windows
/// (`notes`) are the same authored artifact and map to one class;
/// `meeting_profile` and `title` are selection signals, not evidence, and
/// count for nothing. Unknown kinds count for nothing.
fn authoritative_class(kind: &str) -> Option<&'static str> {
    match kind {
        "transcript" => Some("transcript"),
        "summary" => Some("summary"),
        "note" | "notes" => Some("notes"),
        _ => None,
    }
}

/// Deterministic absorption priority: best semantic provenance first (best
/// variant rank, then variant order), then window ordinal, then stable
/// document id.
fn semantic_quality(candidate: &RetrievedEvidence) -> (usize, u8, i64) {
    let provenance = all_provenance(candidate)
        .filter(|provenance| provenance.channel == RetrievalChannel::Semantic)
        .map(|provenance| (provenance.rank, variant_order(provenance.variant)))
        .min()
        .unwrap_or((usize::MAX, u8::MAX));
    (provenance.0, provenance.1, candidate.ordinal)
}

fn variant_order(variant: QueryVariantKind) -> u8 {
    match variant {
        QueryVariantKind::Original => 0,
        QueryVariantKind::Rewritten => 1,
        QueryVariantKind::CoreTerms => 2,
    }
}

fn mode_order(mode: Option<LexicalMode>) -> u8 {
    match mode {
        Some(LexicalMode::And) => 0,
        Some(LexicalMode::Or) => 1,
        None => 0,
    }
}

// -- Rank fusion -------------------------------------------------------------

/// The per-meeting aggregation inputs computed once per request: title
/// overlap, distinct-concept coverage, and the region key each candidate
/// belongs to. Bundled so every caller (production and the evaluation
/// constants grid) feeds aggregation the SAME derived terms and only the
/// weights vary.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AggregationTerms {
    /// Fraction of query core terms present in each meeting's title.
    pub title_overlap: HashMap<String, f64>,
    /// Fraction of distinct query core terms covered across each meeting's
    /// evidence text (the architecture's separate diversity measure).
    pub concept_coverage: HashMap<String, f64>,
    /// Evidence ID -> distinct source region key (see [`coverage_regions`]).
    pub regions: HashMap<String, String>,
}

/// One candidate in fused order.
#[derive(Debug, Clone, PartialEq)]
pub struct FusedEvidence {
    pub evidence: RetrievedEvidence,
    /// 1-based position in the fused evidence order.
    pub fused_rank: usize,
    pub fused_score: f64,
}

fn is_semantic_provenance(provenance: &EvidenceProvenance) -> bool {
    matches!(provenance.channel, RetrievalChannel::Semantic)
}

/// One merged rank list for one (channel, query-slot) pair: each candidate
/// enters at the position its best provenance earns, ties resolved by
/// evidence ID. The 1-based position is what RRF consumes.
fn channel_positions<K: Ord>(
    candidates: &[RetrievedEvidence],
    select: impl Fn(&EvidenceProvenance) -> bool,
    key: impl Fn(&EvidenceProvenance) -> K,
) -> Vec<(&str, usize)> {
    let mut entries: Vec<(&str, K)> = candidates
        .iter()
        .filter_map(|candidate| {
            all_provenance(candidate)
                .filter(|provenance| select(provenance))
                .map(&key)
                .min()
                .map(|best| (candidate.evidence_id.as_str(), best))
        })
        .collect();
    entries.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    entries
        .into_iter()
        .zip(1..)
        .map(|((id, _), position)| (id, position))
        .collect()
}

/// One merged rank list per query slot. Slot 0 (the request's own
/// original/rewritten/core-terms variants) is exactly the Fast list, and
/// each planner query slot `1..=n` is an independent list under the same
/// key, so a later query's rank-1 candidate is not dominated by the
/// earliest slot's ordering.
fn slot_channel_lists<K: Ord>(
    candidates: &[RetrievedEvidence],
    select: impl Fn(&EvidenceProvenance) -> bool + Copy,
    key: impl Fn(&EvidenceProvenance) -> K + Copy,
) -> Vec<Vec<(&str, usize)>> {
    let mut slots: Vec<u8> = candidates
        .iter()
        .flat_map(all_provenance)
        .filter(|provenance| select(provenance))
        .map(|provenance| provenance.query_slot)
        .collect();
    slots.sort_unstable();
    slots.dedup();
    slots
        .into_iter()
        .map(|slot| {
            channel_positions(
                candidates,
                |provenance| select(provenance) && provenance.query_slot == slot,
                key,
            )
        })
        .collect()
}

/// Reciprocal-rank fusion over the two approved channels with the Sprint 1
/// constants: `score = sum(weight / (k + channel_position))`. Each channel is
/// fused as independent per-query-slot rank lists built from the candidates'
/// per-variant provenance — semantic: best variant rank first (the
/// benchmark's vector channel ranks by best similarity across query
/// variants, so a doc's best rank leads, with the variant order as the
/// deterministic tie-break); lexical: AND mode before OR, then variant rank —
/// so raw BM25/cosine values never enter fusion. Slot 0 is the request's own
/// list exactly as Fast produced it; each Deep planner query (`1..=n`)
/// contributes its own list, and a candidate matched by several planner
/// queries accumulates one bounded contribution per slot instead of
/// collapsing into one rewritten namespace where the earliest slot dominates.
/// Equal fused scores resolve by evidence ID, making the result
/// deterministic for ties.
pub fn fuse(candidates: &[RetrievedEvidence], config: &RankingConfig) -> Vec<FusedEvidence> {
    let mut scores: HashMap<&str, f64> = HashMap::new();
    for (weight, lists) in [
        (
            config.w_vector,
            slot_channel_lists(candidates, is_semantic_provenance, |provenance| {
                (provenance.rank, variant_order(provenance.variant))
            }),
        ),
        (
            config.w_lexical,
            slot_channel_lists(
                candidates,
                |provenance| matches!(provenance.channel, RetrievalChannel::Lexical),
                |provenance| {
                    (
                        mode_order(provenance.mode),
                        variant_order(provenance.variant),
                        provenance.rank,
                    )
                },
            ),
        ),
    ] {
        for list in lists {
            for (id, position) in list {
                // Positions are 1-based, so k + position equals the Sprint 1
                // harness's k + rank0 + 1 over 0-based indices.
                *scores.entry(id).or_insert(0.0) += weight / (config.rrf_k + position as f64);
            }
        }
    }
    let mut fused: Vec<(&RetrievedEvidence, f64)> = candidates
        .iter()
        .filter_map(|candidate| {
            // Membership is channel PRESENCE, never score magnitude: an
            // entry exists for every candidate that appeared in a channel
            // list, so a zero-weighted channel deweights its candidates to
            // 0.0 and ranks them last instead of deleting them.
            scores
                .get(candidate.evidence_id.as_str())
                .map(|score| (candidate, *score))
        })
        .collect();
    fused.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| a.0.evidence_id.cmp(&b.0.evidence_id))
    });
    fused
        .into_iter()
        .enumerate()
        .map(|(index, (evidence, fused_score))| FusedEvidence {
            evidence: evidence.clone(),
            fused_rank: index + 1,
            fused_score,
        })
        .collect()
}

/// Preserve the repository's BM25 ordering when semantic retrieval is not
/// available or is deliberately disabled. The lexical channel rank remains
/// the relevance signal; the selected hybrid weights are not changed. Each
/// query slot contributes one independent bounded term, so additional
/// planner queries accumulate support instead of being dominated by slot 0's
/// ordering.
pub fn fuse_lexical_only(
    candidates: &[RetrievedEvidence],
    config: &RankingConfig,
) -> Vec<FusedEvidence> {
    let lists = slot_channel_lists(
        candidates,
        |provenance| matches!(provenance.channel, RetrievalChannel::Lexical),
        |provenance| {
            (
                mode_order(provenance.mode),
                variant_order(provenance.variant),
                provenance.rank,
            )
        },
    );
    // Slot 0 alone reproduces the repository's BM25 ordering exactly (the
    // score is strictly decreasing in position); additional planner slots
    // contribute one independent bounded term per slot, ordered by score.
    let mut scores: HashMap<&str, f64> = HashMap::new();
    for list in lists {
        for (id, position) in list {
            *scores.entry(id).or_insert(0.0) += config.rrf_k / (config.rrf_k + position as f64);
        }
    }
    let mut fused: Vec<(&RetrievedEvidence, f64)> = candidates
        .iter()
        .filter_map(|candidate| {
            scores
                .get(candidate.evidence_id.as_str())
                .map(|score| (candidate, *score))
        })
        .collect();
    fused.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| a.0.evidence_id.cmp(&b.0.evidence_id))
    });
    fused
        .into_iter()
        .enumerate()
        .map(|(index, (evidence, fused_score))| FusedEvidence {
            evidence: evidence.clone(),
            fused_rank: index + 1,
            fused_score,
        })
        .collect()
}

// -- Meeting aggregation ------------------------------------------------------

/// The approved Sprint 1 meeting aggregation (task-1.3-final-selection.md
/// `aggregate_meetings`): each meeting's best fused evidence score scaled by
/// `k`, plus the capped support contribution inside the fused window (the
/// approved diversity control — a meeting earns support for at most `cap`
/// chunks regardless of length, so long meetings cannot win by volume; the
/// architecture's "Per-meeting contribution is capped and diversity is
/// measured separately"), plus the title-overlap term, plus the
/// gamma-weighted reranker rank-space channel. On top of the approved terms
/// sits the Task 3.2 authoritative-corroboration credit
/// ([`CORROBORATION_DELTA`]): one bounded, volume-immune unit per additional
/// independently authored document class the request surfaced for the
/// meeting in the bounded fused candidate universe, which corrects the measured thin-lookalike failure where a
/// single chunk topping both channels outranks a meeting whose answer is
/// spread across its transcript, summary, and notes. Ordering is
/// score-descending, then profile rank, then meeting ID — exactly the
/// evaluated policy.
///
/// The `architecture.md:1271-1285` requirements are MET here, each by a
/// distinct mechanism rather than by the capped support count standing in
/// for all of them:
/// - "diversity is measured separately" from the per-meeting cap: support
///   counts DISTINCT covered regions ([`coverage_regions`]), so overlapping
///   semantic windows over one span collapse to one unit and cannot inflate
///   it by volume;
/// - "Distinct meaningful query concepts covered across evidence": the
///   separate [`concept_coverage`] term, weighted by
///   [`RankingConfig::concept_delta`];
/// - "Meeting-profile rank": the deterministic tie-break below, applied
///   among meetings that tie on score.
///
/// The weights carried by the first two are outputs of the Task 3.2
/// constants-isolation protocol, not judgement — see [`CONCEPT_DELTA`] and
/// [`SUPPORT_ALPHA`] for the value each axis was measured at.
pub fn aggregate_meetings(
    fused: &[FusedEvidence],
    terms: &AggregationTerms,
    rerank_scores: Option<&HashMap<String, f32>>,
    config: &RankingConfig,
) -> Vec<RankedMeeting> {
    let title_overlap = &terms.title_overlap;
    let window = config.support_window.min(fused.len());
    let mut best_fused: HashMap<&str, f64> = HashMap::new();
    // Diversity is measured over DISTINCT covered regions, not chunk count,
    // so overlapping windows over one span cannot inflate support.
    let mut support_regions: HashMap<&str, HashSet<&str>> = HashMap::new();
    // Corroboration is measured over DISTINCT authoritative document classes,
    // so transcript volume cannot inflate it either. When the reranker serves
    // the request, scored candidates qualify or reject a class while bounded
    // unscored tail candidates are neutral; this prevents tail volume from
    // erasing a qualifying class.
    let mut corroboration_classes: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut rejected_classes: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (rank0, entry) in fused.iter().enumerate() {
        let meeting_id = entry.evidence.meeting_id.as_str();
        let slot = best_fused.entry(meeting_id).or_insert(0.0);
        if entry.fused_score > *slot {
            *slot = entry.fused_score;
        }
        if rank0 < window {
            let region = terms
                .regions
                .get(entry.evidence.evidence_id.as_str())
                .map(String::as_str)
                .unwrap_or(entry.evidence.evidence_id.as_str());
            support_regions
                .entry(meeting_id)
                .or_default()
                .insert(region);
        }
        // Corroboration is global over the bounded fused candidate universe,
        // unlike support, which intentionally remains in the support window.
        if terms
            .concept_coverage
            .get(meeting_id)
            .is_some_and(|coverage| *coverage >= 1.0)
        {
            if let Some(class) = authoritative_class(&entry.evidence.source_kind) {
                match rerank_scores {
                    None => {
                        corroboration_classes
                            .entry(meeting_id)
                            .or_default()
                            .insert(class);
                    }
                    Some(scores) => match scores.get(&entry.evidence.evidence_id) {
                        Some(score) if *score < 0.0 => {
                            rejected_classes
                                .entry(meeting_id)
                                .or_default()
                                .insert(class);
                        }
                        Some(_) => {
                            corroboration_classes
                                .entry(meeting_id)
                                .or_default()
                                .insert(class);
                        }
                        None => {}
                    },
                }
            }
        }
    }
    let support: HashMap<&str, usize> = support_regions
        .into_iter()
        .map(|(meeting_id, regions)| (meeting_id, regions.len()))
        .collect();

    // The cross-encoder contributes as a third rank-space RRF channel at
    // meeting level (rank of the meeting by its best reranked evidence), so
    // its signal cannot be drowned out by the fused-score scale. With the
    // approved gamma = 0 this term is zero and the reranker never re-ranks
    // meetings; the code path stays because it is the approved policy.
    let mut rr_channel: HashMap<&str, usize> = HashMap::new();
    if let Some(scores) = rerank_scores {
        let mut best_per_meeting: HashMap<&str, f32> = HashMap::new();
        for entry in fused {
            if let Some(score) = scores.get(&entry.evidence.evidence_id) {
                let slot = best_per_meeting
                    .entry(entry.evidence.meeting_id.as_str())
                    .or_insert(f32::MIN);
                if *score > *slot {
                    *slot = *score;
                }
            }
        }
        let mut ranked: Vec<(&str, f32)> = best_per_meeting.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for (position, (meeting_id, _)) in ranked.iter().enumerate() {
            rr_channel.insert(*meeting_id, position + 1);
        }
    }

    // Title-only meetings: Task 3.1's title channel exists so a title match
    // never depends on lexical/semantic health, and its candidates are
    // selection signals folded into `title_overlap` rather than evidence
    // (see [`dedupe_candidates`]). A meeting carrying only a title signal has
    // no fused evidence, so seed it at `best_fused = 0.0` to keep it in the
    // ranking on its title term alone. Every meeting with fused evidence
    // scores strictly higher (`k * best_fused > 0`), so ordering is unchanged.
    for (meeting_id, overlap) in title_overlap {
        if *overlap > 0.0 {
            best_fused.entry(meeting_id.as_str()).or_insert(0.0);
        }
    }

    let mut profile_rank: HashMap<&str, usize> = HashMap::new();
    let mut profiles: Vec<(&str, usize)> = fused
        .iter()
        .filter(|entry| entry.evidence.source_kind == "meeting_profile")
        .map(|entry| (entry.evidence.meeting_id.as_str(), entry.fused_rank))
        .collect();
    profiles.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));
    for (rank, (meeting_id, _)) in profiles.into_iter().enumerate() {
        profile_rank.insert(meeting_id, rank + 1);
    }
    let mut rows: Vec<RankedMeeting> = best_fused
        .into_iter()
        .map(|(meeting_id, best_fused)| {
            let support = support
                .get(meeting_id)
                .copied()
                .unwrap_or(0)
                .min(config.support_cap);
            let title = title_overlap.get(meeting_id).copied().unwrap_or(0.0);
            let concept = terms
                .concept_coverage
                .get(meeting_id)
                .copied()
                .unwrap_or(0.0);
            let rr = rr_channel
                .get(meeting_id)
                .map(|rank| config.rerank_gamma / (config.rrf_k + *rank as f64))
                .unwrap_or(0.0);
            let corroboration = corroboration_classes.get(meeting_id).map_or(0, |classes| {
                classes
                    .iter()
                    .filter(|class| {
                        !rejected_classes
                            .get(meeting_id)
                            .is_some_and(|rejected| rejected.contains(*class))
                    })
                    .count()
            });
            RankedMeeting {
                meeting_id: meeting_id.to_string(),
                rank: 0,
                score: config.rrf_k * best_fused
                    + config.support_alpha * (support as f64 / config.support_cap.max(1) as f64)
                    + config.title_beta * title
                    + config.concept_delta * concept
                    + config.corroboration_delta * corroboration.saturating_sub(1) as f64
                    + rr,
                best_fused_score: best_fused,
                support,
                corroboration,
                title_overlap: title,
                concept_coverage: concept,
            }
        })
        .collect();
    // A meeting with NO profile signal sorts LAST among equals, so absence is
    // never a ranking advantage. `Option` ordering would do the opposite
    // (`None < Some(_)`), placing a meeting whose profile never matched ahead
    // of one whose profile fused at rank 1.
    let profile_key =
        |meeting_id: &str| profile_rank.get(meeting_id).copied().unwrap_or(usize::MAX);
    // Title-only meetings have no fused evidence and remain behind meetings
    // carrying evidence, even when their title term produces a larger score.
    rows.sort_by(|a, b| {
        (a.best_fused_score == 0.0)
            .cmp(&(b.best_fused_score == 0.0))
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| {
                profile_key(a.meeting_id.as_str()).cmp(&profile_key(b.meeting_id.as_str()))
            })
            .then_with(|| a.meeting_id.cmp(&b.meeting_id))
    });
    for (index, meeting) in rows.iter_mut().enumerate() {
        meeting.rank = index + 1;
    }
    rows
}

// -- Aggregation terms --------------------------------------------------------

/// Title overlap per meeting: the fraction of stripped-original query core
/// terms present in the meeting title (the evaluated Sprint 1 title term,
/// computed from the meeting title every candidate already carries).
pub fn title_overlap(
    candidates: &[RetrievedEvidence],
    core_terms: &[String],
) -> HashMap<String, f64> {
    let mut titles: HashMap<&str, &str> = HashMap::new();
    for candidate in candidates {
        titles
            .entry(candidate.meeting_id.as_str())
            .or_insert(candidate.meeting_title.as_str());
    }
    titles
        .into_iter()
        .map(|(meeting_id, title)| {
            let tokens: HashSet<String> = title
                .split(|character: char| !character.is_alphanumeric())
                .filter(|token| !token.is_empty())
                .map(normalize_core_token)
                .collect();
            let hits = core_terms
                .iter()
                .filter(|term| tokens.contains(term.as_str()))
                .count();
            (
                meeting_id.to_string(),
                hits as f64 / core_terms.len().max(1) as f64,
            )
        })
        .collect()
}

// -- Local cross-encoder reranking -------------------------------------------

/// The cross-encoder reranks citable evidence only: meeting profiles and
/// title signals support selection and are never cited ("Profiles do not
/// replace evidence chunks and are not cited").
fn is_citable(kind: &str) -> bool {
    kind != "meeting_profile" && kind != "title"
}

/// The deterministic approved adaptive-depth policy: the fused order's
/// citable evidence, truncated at the approved Chat depth. Identical inputs
/// always produce the identical head.
pub fn select_rerank_head<'a>(
    fused: &'a [FusedEvidence],
    config: &RankingConfig,
) -> Vec<&'a FusedEvidence> {
    fused
        .iter()
        .filter(|entry| is_citable(&entry.evidence.source_kind))
        .take(config.rerank_depth)
        .collect()
}

/// Final evidence and meeting order under the approved policy: scored
/// evidence heads the order (reranker score descending, fused-rank
/// tie-break), every other candidate follows in fused order, and the meeting
/// order is recomputed under the aggregation policy with the reranker rank
/// channel (a no-op at the approved gamma 0).
pub fn apply_rerank(
    fused: &[FusedEvidence],
    rerank_scores: &HashMap<String, f32>,
    terms: &AggregationTerms,
    config: &RankingConfig,
) -> (Vec<RankedEvidence>, Vec<RankedMeeting>) {
    let mut head: Vec<(&FusedEvidence, f32)> = fused
        .iter()
        .filter_map(|entry| {
            rerank_scores
                .get(&entry.evidence.evidence_id)
                .map(|score| (entry, *score))
        })
        .collect();
    head.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| a.0.fused_rank.cmp(&b.0.fused_rank))
    });
    let head_ids: HashSet<&str> = head
        .iter()
        .map(|(entry, _)| entry.evidence.evidence_id.as_str())
        .collect();
    let mut evidence: Vec<RankedEvidence> = head
        .into_iter()
        .map(|(entry, score)| RankedEvidence {
            evidence: entry.evidence.clone(),
            content_fingerprint: Some(
                sha2::Sha256::digest(entry.evidence.text.as_bytes()).to_vec(),
            ),
            fused_rank: entry.fused_rank,
            fused_score: entry.fused_score,
            reranker_score: Some(score),
        })
        .collect();
    evidence.extend(
        fused
            .iter()
            .filter(|entry| !head_ids.contains(entry.evidence.evidence_id.as_str()))
            .map(|entry| RankedEvidence {
                evidence: entry.evidence.clone(),
                content_fingerprint: Some(
                    sha2::Sha256::digest(entry.evidence.text.as_bytes()).to_vec(),
                ),
                fused_rank: entry.fused_rank,
                fused_score: entry.fused_score,
                reranker_score: None,
            }),
    );
    let meetings = aggregate_meetings(fused, terms, Some(rerank_scores), config);
    (evidence, meetings)
}

fn fused_only(fused: Vec<FusedEvidence>) -> Vec<RankedEvidence> {
    fused
        .into_iter()
        .map(|entry| RankedEvidence {
            content_fingerprint: Some(
                sha2::Sha256::digest(entry.evidence.text.as_bytes()).to_vec(),
            ),
            evidence: entry.evidence,
            fused_rank: entry.fused_rank,
            fused_score: entry.fused_score,
            reranker_score: None,
        })
        .collect()
}

// -- Pipeline ----------------------------------------------------------------

enum RerankResult {
    Scores(Vec<f32>),
    Fallback(RerankFallback),
}

/// The full Task 3.2 stage: dedupe, fuse, aggregate, and (when available)
/// local cross-encode the bounded head. Consumes the Task 3.1 candidates.
/// `effective_query` and `core_terms` are the request's one authoritative
/// effective-query/core-term pair, computed once by
/// [`RetrievalService::retrieve_ranked`](crate::retrieval::service::RetrievalService::retrieve_ranked)
/// and carried immutably on the outcome so no downstream consumer re-derives
/// the policy. They come from DIFFERENT queries by design:
/// - `effective_query` (the reranker question) is the rewritten query when
///   present and distinct, else the original, folder operators stripped;
/// - `core_terms` are derived from the stripped ORIGINAL query — the exact
///   set that drove the CoreTerms lexical variant and Task 3.1's title
///   top-k. Deriving them from the rewritten text would score the
///   title-overlap and concept-coverage terms against different terms than
///   the title channel selected on.
#[cfg(test)]
pub(crate) async fn rank(
    lifecycle: &RetrievalLifecycle,
    pool: &SqlitePool,
    candidates: Vec<RetrievedEvidence>,
    effective_query: &str,
    core_terms: Vec<String>,
    config: &RankingConfig,
    cancel: &CancellationToken,
) -> Result<RankingOutcome, RetrievalError> {
    rank_with_mode(
        lifecycle,
        pool,
        candidates,
        effective_query,
        core_terms,
        config,
        RankingMode::Hybrid,
        cancel,
    )
    .await
}

pub(crate) async fn rank_with_mode(
    lifecycle: &RetrievalLifecycle,
    pool: &SqlitePool,
    candidates: Vec<RetrievedEvidence>,
    effective_query: &str,
    core_terms: Vec<String>,
    config: &RankingConfig,
    mode: RankingMode,
    cancel: &CancellationToken,
) -> Result<RankingOutcome, RetrievalError> {
    ensure_not_cancelled(cancel)?;
    let mut meeting_ids: Vec<String> = candidates
        .iter()
        .map(|candidate| candidate.meeting_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    meeting_ids.sort();
    // A failed coverage read degrades to no cross-channel transcript merge;
    // it is a ranking-quality read, never a request failure. The degradation
    // is TYPED onto the outcome rather than silent, so a caller can tell a
    // healthy merge from a skipped one.
    let (segment_order, chronology_omitted_meetings, dedupe_degraded) =
        match RetrievalRepository::ordered_transcript_segment_ids(pool, &meeting_ids, cancel).await
        {
            Ok(chronology) => {
                let degraded = !chronology.complete();
                (chronology.segments, chronology.omitted_meetings, degraded)
            }
            Err(error) => {
                log::warn!("Ranking: segment chronology read failed; cross-channel transcript merge skipped: {error}");
                (SegmentOrder::new(), meeting_ids, true)
            }
        };
    ensure_not_cancelled(cancel)?;
    // Title overlap is computed BEFORE deduplication: `dedupe_candidates`
    // drops title candidates (they are selection signals, not evidence), so
    // a meeting matched ONLY by its title would otherwise contribute no
    // title term and disappear from the ranking entirely.
    let title = title_overlap(&candidates, &core_terms);
    let deduped = dedupe_candidates(candidates, &segment_order);
    // Aggregation inputs computed once from the deduplicated candidates:
    // distinct-concept coverage over evidence text, and the region key that
    // makes diversity a measure of covered source area rather than chunk
    // count. Both travel on the outcome so nothing re-derives them.
    let terms = AggregationTerms {
        title_overlap: title.clone(),
        concept_coverage: concept_coverage(&deduped, &core_terms),
        regions: coverage_regions(&deduped, &segment_order),
    };
    let fused = match mode {
        RankingMode::Hybrid => fuse(&deduped, config),
        RankingMode::LexicalOnly => fuse_lexical_only(&deduped, config),
    };
    let head = select_rerank_head(&fused, config);
    let rerank_depth = config.rerank_depth.min(head.len());
    let outcome = if rerank_depth == 0 {
        let meetings = aggregate_meetings(&fused, &terms, None, config);
        RankingOutcome {
            evidence: fused_only(fused),
            meetings,
            reranker_used: false,
            rerank_depth,
            rerank_fallback: None,
            core_terms,
            title_overlap: title,
            terms,
            effective_query: effective_query.to_string(),
            dedupe_degraded,
            chronology_omitted_meetings,
        }
    } else {
        let texts: Vec<String> = head
            .iter()
            .map(|entry| entry.evidence.text.clone())
            .collect();
        match rerank_with_lifecycle(lifecycle, effective_query, texts, cancel).await? {
            // One score per pair or nothing: a short vector would silently
            // score a prefix and reorder the tail while still reporting
            // `reranker_used`, so a count mismatch takes the documented
            // deterministic fallback instead.
            RerankResult::Scores(scores) if scores.len() == head.len() => assemble_scored_outcome(
                &fused,
                &head,
                &scores,
                &terms,
                config,
                cancel,
                core_terms,
                effective_query,
                dedupe_degraded,
                chronology_omitted_meetings.clone(),
            )?,
            RerankResult::Scores(scores) => {
                log::warn!(
                    "Ranking: reranker returned {} scores for {} pairs; keeping fused order",
                    scores.len(),
                    head.len()
                );
                let meetings = aggregate_meetings(&fused, &terms, None, config);
                RankingOutcome {
                    evidence: fused_only(fused),
                    meetings,
                    reranker_used: false,
                    rerank_depth,
                    rerank_fallback: Some(RerankFallback::RerankerError),
                    core_terms,
                    title_overlap: title,
                    terms: terms.clone(),
                    effective_query: effective_query.to_string(),
                    dedupe_degraded,
                    chronology_omitted_meetings: chronology_omitted_meetings.clone(),
                }
            }
            RerankResult::Fallback(fallback) => {
                let meetings = aggregate_meetings(&fused, &terms, None, config);
                RankingOutcome {
                    evidence: fused_only(fused),
                    meetings,
                    reranker_used: false,
                    rerank_depth,
                    rerank_fallback: Some(fallback),
                    core_terms,
                    title_overlap: title,
                    terms: terms.clone(),
                    effective_query: effective_query.to_string(),
                    dedupe_degraded,
                    chronology_omitted_meetings: chronology_omitted_meetings.clone(),
                }
            }
        }
    };
    // Cancellation is checked on EVERY path that can return an outcome, not
    // only the scored one: the fallback and score-mismatch branches also run
    // after model work, and the documented rule is that user/stream
    // cancellation is always the typed error and never a fallback result.
    ensure_not_cancelled(cancel)?;
    log::info!(
        "Ranking: evidence={} meetings={} reranker={} depth={} fallback={}",
        outcome.evidence.len(),
        outcome.meetings.len(),
        if outcome.reranker_used { "on" } else { "off" },
        outcome.rerank_depth,
        outcome.rerank_fallback.map_or("none", RerankFallback::tag),
    );
    Ok(outcome)
}

/// Applies the reranker scores to the fused order. Cancellation is checked
/// FIRST — after the final inference has returned but before anything is
/// applied or returned — so a late cancellation can never produce an
/// outcome.
fn assemble_scored_outcome(
    fused: &[FusedEvidence],
    head: &[&FusedEvidence],
    scores: &[f32],
    terms: &AggregationTerms,
    config: &RankingConfig,
    cancel: &CancellationToken,
    core_terms: Vec<String>,
    effective_query: &str,
    dedupe_degraded: bool,
    chronology_omitted_meetings: Vec<String>,
) -> Result<RankingOutcome, RetrievalError> {
    ensure_not_cancelled(cancel)?;
    let score_map: HashMap<String, f32> = head
        .iter()
        .zip(scores)
        .filter(|(_, score)| score.is_finite())
        .map(|(entry, score)| (entry.evidence.evidence_id.clone(), *score))
        .collect();
    let (evidence, meetings) = apply_rerank(fused, &score_map, terms, config);
    Ok(RankingOutcome {
        evidence,
        meetings,
        reranker_used: true,
        rerank_depth: config.rerank_depth.min(head.len()),
        rerank_fallback: None,
        core_terms,
        title_overlap: terms.title_overlap.clone(),
        terms: terms.clone(),
        effective_query: effective_query.to_string(),
        dedupe_degraded,
        chronology_omitted_meetings,
    })
}
async fn rerank_with_lifecycle(
    lifecycle: &RetrievalLifecycle,
    question: &str,
    texts: Vec<String>,
    cancel: &CancellationToken,
) -> Result<RerankResult, RetrievalError> {
    let ticket = match lifecycle.scheduler().enqueue_interactive() {
        Ok(ticket) => ticket,
        Err(_) => return Ok(RerankResult::Fallback(RerankFallback::SchedulerRejected)),
    };
    let _lease = match ticket.wait_for_permit_with(cancel).await {
        Ok(lease) => lease,
        Err(SchedulerRejection::CancelledWhileQueued) => return Err(RetrievalError::Cancelled),
        Err(_) => return Ok(RerankResult::Fallback(RerankFallback::SchedulerRejected)),
    };
    let embedder = match lifecycle.load_embedder().await {
        Ok(embedder) => embedder,
        Err(_) => return Ok(RerankResult::Fallback(RerankFallback::Unavailable)),
    };
    // The reranker rides the same process-wide session cache the loader just
    // warmed - never a second model-session owner. Engines without the
    // bundled cross-encoder (test embedders) take the documented fallback.
    let Some(models) = cached_model(&embedder.model_id()) else {
        return Ok(RerankResult::Fallback(RerankFallback::Unavailable));
    };
    ensure_not_cancelled(cancel)?;
    let pairs: Vec<(String, String)> = texts
        .into_iter()
        .map(|text| (question.to_string(), text))
        .collect();
    match models.rerank(pairs, cancel.clone()).await {
        Ok(scores) => Ok(RerankResult::Scores(scores)),
        Err(RetrievalModelError::Cancelled) => Err(RetrievalError::Cancelled),
        Err(_) => Ok(RerankResult::Fallback(RerankFallback::RerankerError)),
    }
}

#[cfg(test)]
mod tests;
