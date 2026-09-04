//! Concrete persisted-scope retrieval service (Sprint 3 Task 3.1).
//!
//! One shared service resolves an authoritative request-start scope boundary
//! and returns channel-ranked lexical (FTS + current-title) and semantic
//! evidence candidates for persisted all/folder/allowed-ID scopes. It stops
//! at candidates: rank fusion, meeting aggregation, reranking (Task 3.2),
//! authoritative hydration, and source publication (Task 3.3) live
//! downstream. Live-recording scope stays in `api/chat.rs`.
//!
//! Every semantic failure is a typed fallback to the lexical channels, never
//! a request failure; cancellation alone aborts the request as a typed error
//! and never degrades to lexical answer preparation. The service reuses the
//! shared [`RetrievalLifecycle`]: its interactive inference permit for query
//! embedding, its vector-scan permits inside [`QueryIndexService::search`],
//! and its bounded queue. It never logs query or candidate text - the single
//! outcome line carries counts and reasons only.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
#[cfg(test)]
use std::sync::{Mutex as StdMutex, PoisonError};

use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::database::repositories::folder::FolderRepository;
use crate::database::repositories::fts::{
    folder_operator_names, strip_folder_operators, FtsRepository, FtsSearchResult, MatchMode,
};
use crate::database::repositories::retrieval::RetrievalRepository;
use crate::database::repositories::setting::SettingsRepository;
use crate::retrieval::index::{ScopeFilter, SearchFailure, VectorHit, MAX_QUERY_LIMIT};
use crate::retrieval::model::RetrievalModelError;
use crate::retrieval::worker::{RetrievalLifecycle, SchedulerRejection, PAUSE_QUANTUM};

/// Approved bounded size for allowed-ID scopes, mirroring the approved
/// 100-meeting snapshot ceiling (`MAX_SEARCH_SNAPSHOT_RESULTS`).
pub const MAX_ALLOWED_MEETING_IDS: usize = 100;
pub const HYBRID_CANDIDATES_PER_VARIANT: usize = 100;
/// Approved ceiling on the folder membership accelerator: a folder subtree
/// with at most this many current meetings materializes its ID set once to
/// accelerate the vector scan; above it the scan runs as [`ScopeFilter::All`]
/// and the repository's recursive root-folder gate is the sole semantic
/// membership authority.
pub(crate) const MAX_FOLDER_SCAN_MEMBERSHIP: usize = 20_000;
/// Streaming page size of the bounded title scan; only one page plus the
/// bounded top-k heap is ever resident.
pub(crate) const TITLE_SCAN_PAGE: usize = 256;
/// Approved candidate ceiling per variant per channel (architecture:
/// FTS/vector candidates per variant 50-150; the index enforces the same
/// ceiling on vector scans).
const MAX_CANDIDATES_PER_VARIANT: usize = MAX_QUERY_LIMIT;
/// Extra search attempts while the snapshot journal catches up to canonical
/// state, one [`PAUSE_QUANTUM`] apart (~500 ms worst case), before falling
/// back to lexical for the request.
const CATCHUP_RETRIES: usize = 2;

/// Why retrieval runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalPurpose {
    Chat,
    Search,
    Context,
}

/// Exactly one tagged persisted scope. Combining scopes is unrepresentable by
/// construction; the `folder:"..."` query operator is normalized into
/// [`PersistedRetrievalScope::Folder`] by the service or the request is
/// rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistedRetrievalScope {
    All,
    Meeting(String),
    /// Selected folder plus descendants, resolved at request time.
    Folder(String),
    /// Explicit allow-list (snapshot/today semantics). Duplicate IDs are
    /// removed; the set is intersected with current meetings.
    AllowedMeetingIds(Vec<String>),
}

/// Candidate limits per channel. Provisional evaluation parameters from the
/// architecture's 50-150 band, clamped to the approved 150 ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrievalLimits {
    pub lexical_per_variant: usize,
    pub vector_per_variant: usize,
}

impl Default for RetrievalLimits {
    fn default() -> Self {
        Self::chat_default()
    }
}

impl RetrievalLimits {
    /// Provisional Fast defaults inside the approved candidate band; Task 3.2
    /// tunes them with threshold semantics.
    pub const fn chat_default() -> Self {
        Self {
            lexical_per_variant: HYBRID_CANDIDATES_PER_VARIANT,
            vector_per_variant: HYBRID_CANDIDATES_PER_VARIANT,
        }
    }

    pub const fn hybrid_default() -> Self {
        Self {
            lexical_per_variant: HYBRID_CANDIDATES_PER_VARIANT,
            vector_per_variant: HYBRID_CANDIDATES_PER_VARIANT,
        }
    }

    fn clamped(self) -> Self {
        Self {
            lexical_per_variant: self
                .lexical_per_variant
                .clamp(0, MAX_CANDIDATES_PER_VARIANT),
            vector_per_variant: self.vector_per_variant.clamp(0, MAX_CANDIDATES_PER_VARIANT),
        }
    }
}

/// Core-term language selection for the lexical channel. Mirrors the
/// evaluation corpus's explicit language field
/// (`tests/fixtures/corpus_types.rs`): the caller states the language
/// deliberately; there is no runtime auto-detection and no all-language
/// union. Unknown languages apply no stopword list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreTermLanguage {
    Portuguese,
    English,
    Unknown,
}

/// One retrieval request against persisted content, per the architecture's
/// retrieval contract. Cancellation travels with the request.
#[derive(Debug, Clone)]
pub struct RetrievalRequest {
    pub original_query: String,
    pub rewritten_query: Option<String>,
    pub scope: PersistedRetrievalScope,
    pub purpose: RetrievalPurpose,
    pub limits: RetrievalLimits,
    pub core_language: CoreTermLanguage,
    pub cancellation: Option<CancellationToken>,
}

/// Query variant provenance, preserved end to end so fusion and diagnostics
/// can tell the channels apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryVariantKind {
    Original,
    Rewritten,
    CoreTerms,
}

/// Match mode of a lexical channel hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LexicalMode {
    And,
    Or,
}

/// Candidate channel provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RetrievalChannel {
    Lexical,
    Title,
    Semantic,
}

/// One channel match behind a candidate: which channel, which query variant,
/// which lexical mode, and the 1-based rank inside that channel list.
/// `query_slot` separates independent queries inside one accumulated pool:
/// `0` is the request's own original/rewritten/core-terms namespaces, and
/// `1..=n` is the nth Deep planner query, so fusion keeps distinct per-query
/// rank lists instead of collapsing them all into one rewritten namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceProvenance {
    pub channel: RetrievalChannel,
    pub variant: QueryVariantKind,
    pub mode: Option<LexicalMode>,
    pub rank: usize,
    pub query_slot: u8,
}

/// One stable-identity evidence candidate. Field names follow the
/// architecture's conceptual `RetrievedEvidence`; per-channel ranks live in
/// `provenance` and fused rank/reranker score are added by Task 3.2.
///
/// `evidence_id` is stable only within the channel that produced it (each
/// channel mints its own namespace: `fts:{chunk_type}:{chunk_id}`,
/// `title:{meeting_id}`, or the semantic `document_id`). A lexical chunk and
/// a semantic chunk covering the same source text are therefore two distinct
/// candidates at this stage; the same source range can also legitimately
/// back several overlapping semantic documents (384-token sliding windows).
/// Recognizing that overlap and fusing it into one ranked identity is
/// Task 3.2's job, not this task's.
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedEvidence {
    pub evidence_id: String,
    pub meeting_id: String,
    pub meeting_title: String,
    pub source_kind: String,
    pub source_start_id: Option<String>,
    pub source_end_id: Option<String>,
    pub source_template_id: Option<String>,
    pub heading: Option<String>,
    pub ordinal: i64,
    pub text: String,
    pub speaker: Option<String>,
    pub timestamp_label: Option<String>,
    pub provenance: Vec<EvidenceProvenance>,
    /// Bounded source identities absorbed during cross-channel dedupe. These
    /// aliases remain available to the authoritative hydration stage.
    pub source_aliases: Vec<SourceAlias>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceAlias {
    pub evidence_id: String,
    pub source_kind: String,
    pub source_start_id: Option<String>,
    pub source_end_id: Option<String>,
    pub text: String,
    pub provenance: Vec<EvidenceProvenance>,
}

/// The normalized scope: the tagged scope after `folder:"..."` normalization.
/// The request-start membership stays internal to the service; downstream
/// stages must revalidate current membership before publication (Task 3.3).
#[derive(Debug, Clone)]
pub struct ResolvedScope {
    pub scope: PersistedRetrievalScope,
}

/// Documented semantic-stage fallback: the request degrades to lexical
/// candidates with the reason attached. Cancellation is never one of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticFallbackReason {
    NoActiveGeneration,
    ModelMismatch,
    /// The pinned generation was superseded by an activation/snapshot swap
    /// mid-request (a routine, self-healing rotation) - distinct from
    /// [`Self::ModelMismatch`], a genuine embedder/index model divergence.
    GenerationChanged,
    EmbeddingUnavailable,
    SchedulerRejected,
    ForcedLexical,
    CatchUpTimeout {
        behind: i64,
    },
    SemanticScanFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RetrievalError {
    #[error("retrieval cancelled")]
    Cancelled,
    #[error("retrieval purpose {0:?} is not supported")]
    UnsupportedPurpose(RetrievalPurpose),
    #[error("invalid retrieval query: {0}")]
    InvalidQuery(&'static str),
    #[error("invalid retrieval scope: {0}")]
    InvalidScope(String),
    #[error("retrieval database read failed: {0}")]
    Database(String),
}

/// Maps a repository SQL error onto the retrieval error type. The
/// repositories' typed cancellation marker arrives wrapped in
/// `SqlxError::Protocol`, whose Display text is not the marker itself, so the
/// variant is matched instead of the formatted string.
pub(crate) fn db_error(error: sqlx::Error) -> RetrievalError {
    match &error {
        sqlx::Error::Protocol(message) if message == "retrieval cancelled" => {
            RetrievalError::Cancelled
        }
        _ => RetrievalError::Database(error.to_string()),
    }
}

fn ensure_not_cancelled(cancel: &CancellationToken) -> Result<(), RetrievalError> {
    if cancel.is_cancelled() {
        Err(RetrievalError::Cancelled)
    } else {
        Ok(())
    }
}

/// One retrieval outcome.
#[derive(Debug, Clone)]
pub struct RetrievalResult {
    pub scope: ResolvedScope,
    pub candidates: Vec<RetrievedEvidence>,
    /// `None` when the semantic channel ran (whether or not it produced
    /// candidates); otherwise the documented reason it degraded to lexical.
    pub semantic_fallback: Option<SemanticFallbackReason>,
}

/// The ranked Fast-retrieval result: the request scope plus the Task 3.2
/// ranking outcome (final evidence order, meeting order, reranker state).
#[derive(Debug, Clone)]
pub struct RankedRetrieval {
    pub scope: ResolvedScope,
    pub ranking: crate::retrieval::ranking::RankingOutcome,
    /// The candidate stage's typed semantic availability: `None` when the
    /// semantic channel ran, otherwise the documented reason it degraded to
    /// lexical-only. Carried through ranking so a caller can never mistake
    /// lexical-only output for healthy hybrid output.
    pub semantic_fallback: Option<SemanticFallbackReason>,
}

/// Internal normalized request: lexical texts with folder operators stripped,
/// core terms, and request-start membership.
struct NormalizedRequest {
    purpose: RetrievalPurpose,
    scope: PersistedRetrievalScope,
    membership: ScopeFilter,
    transcript_only: bool,
    /// Folder membership exceeded [`MAX_FOLDER_SCAN_MEMBERSHIP`]: the
    /// semantic scan runs unscoped and the repository's recursive root-folder
    /// gate is the sole semantic admission authority.
    folder_over_cap: bool,
    lexical_original: String,
    lexical_rewritten: Option<String>,
    core_terms: Vec<String>,
    core_language: CoreTermLanguage,
}

/// The one shared retrieval service. Holds a clone of the process-wide
/// [`RetrievalLifecycle`] - the same scheduler, queue, and model sessions the
/// index worker uses - and never builds a second runtime.
#[derive(Clone)]
pub struct RetrievalService {
    lifecycle: RetrievalLifecycle,
    #[cfg(test)]
    scan_gate: Arc<SemanticScanGate>,
}

/// Minimal test-only synchronization point for pinned-generation regressions:
/// when armed, the semantic loop signals after one successful variant scan
/// and waits for release, so a mid-request generation swap is deterministic.
/// Instance-scoped, so concurrent tests never observe each other's gate.
#[cfg(test)]
struct SemanticScanGate {
    armed: StdMutex<Option<tokio::sync::mpsc::UnboundedSender<()>>>,
    release: tokio::sync::Notify,
}

#[cfg(test)]
impl SemanticScanGate {
    fn new() -> Self {
        Self {
            armed: StdMutex::new(None),
            release: tokio::sync::Notify::new(),
        }
    }
}

impl RetrievalService {
    pub fn new(lifecycle: RetrievalLifecycle) -> Self {
        Self {
            lifecycle,
            #[cfg(test)]
            scan_gate: Arc::new(SemanticScanGate::new()),
        }
    }

    /// Arms the test-only scan gate with a signal channel.
    #[cfg(test)]
    pub(crate) fn arm_scan_gate(&self, sender: tokio::sync::mpsc::UnboundedSender<()>) {
        *self
            .scan_gate
            .armed
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(sender);
    }

    /// Releases the test-only scan gate after a deterministic swap.
    #[cfg(test)]
    pub(crate) fn release_scan_gate(&self) {
        self.scan_gate.release.notify_one();
    }

    /// Resolves the scope once, then runs the lexical, current-title, and
    /// semantic candidate channels, returning channel-ranked candidates.
    pub async fn retrieve(
        &self,
        pool: &SqlitePool,
        request: RetrievalRequest,
    ) -> Result<RetrievalResult, RetrievalError> {
        let cancel = request.cancellation.clone().unwrap_or_default();
        ensure_not_cancelled(&cancel)?;
        let force_lexical = SettingsRepository::get_force_lexical_retrieval(pool)
            .await
            .map_err(db_error)?;
        let normalized = self.normalize_request(pool, &request, &cancel).await?;
        ensure_not_cancelled(&cancel)?;
        if let ScopeFilter::Meetings(ids) = &normalized.membership {
            if ids.is_empty() {
                return Ok(RetrievalResult {
                    scope: normalized.resolved(),
                    candidates: Vec::new(),
                    semantic_fallback: if force_lexical {
                        Some(SemanticFallbackReason::ForcedLexical)
                    } else {
                        None
                    },
                });
            }
        }
        let limits = request.limits.clamped();
        let mut candidates: HashMap<String, RetrievedEvidence> = HashMap::new();

        self.lexical_channel(pool, &normalized, limits, &cancel, &mut candidates)
            .await?;
        ensure_not_cancelled(&cancel)?;
        self.title_channel(pool, &normalized, limits, &cancel, &mut candidates)
            .await?;
        ensure_not_cancelled(&cancel)?;
        let semantic_fallback = if force_lexical {
            Some(SemanticFallbackReason::ForcedLexical)
        } else {
            self.semantic_channel(pool, &normalized, limits, &cancel, &mut candidates)
                .await?
        };
        ensure_not_cancelled(&cancel)?;

        let candidates = order_candidates(candidates);
        let outcome_line = outcome_line(&normalized.scope, candidates.len(), &semantic_fallback);
        log::info!("{outcome_line}");
        Ok(RetrievalResult {
            scope: normalized.resolved(),
            candidates,
            semantic_fallback,
        })
    }

    /// Resolves the scope once, runs the lexical, current-title, and semantic
    /// candidate channels, then the Task 3.2 ranking stage (cross-channel
    /// deduplication, reciprocal-rank fusion, meeting aggregation, and the
    /// bounded local cross-encoder head) under the approved Chat
    /// configuration. Broad-Chat caller wiring is Task 3.4; hydration and
    /// source publication are Task 3.3.
    ///
    /// Query policy, derived here with the SAME rules
    /// [`Self::normalize_request`] applied to the candidate channels, so one
    /// request never carries two disagreeing derivations:
    /// - the effective query (the reranker question) is the folder-operator
    ///   stripped rewritten query when it is non-empty and differs from the
    ///   stripped original, else the stripped original;
    /// - the core terms are derived from the stripped ORIGINAL query, which
    ///   is the exact set that already drove the CoreTerms lexical variant
    ///   and Task 3.1's title top-k selection. Deriving them from the
    ///   rewritten text instead would score the Task 3.2 title-overlap term
    ///   against different terms than the title channel selected on.
    ///
    /// Both values are carried on the outcome so no consumer re-derives them.
    pub async fn retrieve_ranked(
        &self,
        pool: &SqlitePool,
        request: RetrievalRequest,
    ) -> Result<RankedRetrieval, RetrievalError> {
        let cancel = request.cancellation.clone().unwrap_or_default();
        let purpose = request.purpose;
        let result = self.retrieve(pool, request.clone()).await?;
        ensure_not_cancelled(&cancel)?;
        let lexical_original = strip_folder_operators(request.original_query.trim().to_string());
        let effective_query = request
            .rewritten_query
            .as_deref()
            .map(|rewritten| strip_folder_operators(rewritten.to_string()))
            .filter(|rewritten| !rewritten.trim().is_empty() && rewritten != &lexical_original)
            .unwrap_or_else(|| lexical_original.clone());
        let core_terms = core_terms(&lexical_original, request.core_language);
        let ranking_mode = if result.semantic_fallback.is_some() {
            crate::retrieval::ranking::RankingMode::LexicalOnly
        } else {
            crate::retrieval::ranking::RankingMode::Hybrid
        };
        let ranking = crate::retrieval::ranking::rank_with_mode(
            &self.lifecycle,
            pool,
            result.candidates,
            effective_query.trim(),
            core_terms,
            &crate::retrieval::ranking::RankingConfig::for_purpose(purpose),
            ranking_mode,
            &cancel,
        )
        .await?;
        Ok(RankedRetrieval {
            scope: result.scope,
            ranking,
            semantic_fallback: result.semantic_fallback,
        })
    }

    pub(crate) async fn retrieve_ranked_with_broad_coverage(
        &self,
        pool: &SqlitePool,
        request: RetrievalRequest,
    ) -> Result<RankedRetrieval, RetrievalError> {
        let purpose = request.purpose;
        let cancel = request.cancellation.clone().unwrap_or_default();
        let mut ranked = self.retrieve_ranked(pool, request).await?;
        let allowed_ids = match &ranked.scope.scope {
            PersistedRetrievalScope::AllowedMeetingIds(ids) => {
                let mut unique = Vec::with_capacity(ids.len());
                let mut seen = HashSet::new();
                for id in ids {
                    if seen.insert(id) {
                        unique.push(id.clone());
                    }
                }
                unique
            }
            _ => return Ok(ranked),
        };
        if allowed_ids.is_empty() {
            return Ok(ranked);
        }

        ensure_not_cancelled(&cancel)?;
        let coverage =
            FtsRepository::get_by_meeting_ids(pool, &allowed_ids, 1, allowed_ids.len() as u32)
                .await
                .map_err(db_error)?;
        ensure_not_cancelled(&cancel)?;

        let mut candidates: Vec<RetrievedEvidence> = ranked
            .ranking
            .evidence
            .iter()
            .map(|entry| entry.evidence.clone())
            .collect();
        let mut seen = candidates
            .iter()
            .map(|candidate| candidate.evidence_id.clone())
            .collect::<HashSet<_>>();
        for result in coverage {
            let mut evidence = lexical_evidence(result);
            evidence.provenance.push(EvidenceProvenance {
                channel: RetrievalChannel::Lexical,
                variant: QueryVariantKind::Original,
                mode: Some(LexicalMode::Or),
                rank: MAX_CANDIDATES_PER_VARIANT,
                query_slot: 0,
            });
            if seen.insert(evidence.evidence_id.clone()) {
                candidates.push(evidence);
            }
        }
        if candidates.len() == ranked.ranking.evidence.len() {
            return Ok(ranked);
        }

        let ranking_mode = if ranked.semantic_fallback.is_some() {
            crate::retrieval::ranking::RankingMode::LexicalOnly
        } else {
            crate::retrieval::ranking::RankingMode::Hybrid
        };
        ranked.ranking = crate::retrieval::ranking::rank_with_mode(
            &self.lifecycle,
            pool,
            candidates,
            &ranked.ranking.effective_query,
            ranked.ranking.core_terms.clone(),
            &crate::retrieval::ranking::RankingConfig::for_purpose(purpose),
            ranking_mode,
            &cancel,
        )
        .await?;
        Ok(ranked)
    }

    /// Revalidates current meeting existence and ORIGINAL authoritative scope,
    /// returning supplied IDs still inside it (input order kept). Deep
    /// retrieval rounds call this after every planner round; hybrid callers
    /// also use it immediately before publication.
    pub async fn revalidate_ids_in_scope(
        &self,
        pool: &SqlitePool,
        scope: &PersistedRetrievalScope,
        meeting_ids: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, RetrievalError> {
        if meeting_ids.is_empty() {
            return Ok(Vec::new());
        }
        ensure_not_cancelled(cancel)?;
        let mut query = QueryBuilder::<Sqlite>::new("");
        if let PersistedRetrievalScope::Folder(folder_id) = scope {
            query.push(
                "WITH RECURSIVE folder_scope(id) AS (SELECT id FROM meeting_folders WHERE id = ",
            );
            query.push_bind(folder_id);
            query.push(
                " UNION ALL SELECT f.id FROM meeting_folders f JOIN folder_scope s ON f.parent_id = s.id) ",
            );
        }
        query.push("SELECT m.id FROM meetings m WHERE m.id IN (");
        let mut ids = query.separated(", ");
        for meeting_id in meeting_ids {
            ids.push_bind(meeting_id);
        }
        drop(ids);
        query.push(")");
        match scope {
            PersistedRetrievalScope::All => {}
            PersistedRetrievalScope::Meeting(scope_id) => {
                query.push(" AND m.id = ").push_bind(scope_id);
            }
            PersistedRetrievalScope::Folder(_) => {
                query.push(" AND m.folder_id IN (SELECT id FROM folder_scope)");
            }
            PersistedRetrievalScope::AllowedMeetingIds(allowed_ids) => {
                if allowed_ids.len() > MAX_ALLOWED_MEETING_IDS {
                    return Err(RetrievalError::InvalidScope(format!(
                        "allowed-ID scope exceeds the approved {} id bound",
                        MAX_ALLOWED_MEETING_IDS
                    )));
                }
                if allowed_ids.is_empty() {
                    return Ok(Vec::new());
                }
                query.push(" AND m.id IN (");
                let mut allowed = query.separated(", ");
                for id in allowed_ids {
                    allowed.push_bind(id);
                }
                drop(allowed);
                query.push(")");
            }
        }
        let rows: Vec<(String,)> = query
            .build_query_as()
            .fetch_all(pool)
            .await
            .map_err(db_error)?;
        ensure_not_cancelled(cancel)?;
        let current = rows.into_iter().map(|(id,)| id).collect::<HashSet<_>>();
        Ok(meeting_ids
            .iter()
            .filter(|id| current.contains(id.as_str()))
            .cloned()
            .collect())
    }

    // -- Scope validation and normalization ----------------------------------

    async fn normalize_request(
        &self,
        pool: &SqlitePool,
        request: &RetrievalRequest,
        cancel: &CancellationToken,
    ) -> Result<NormalizedRequest, RetrievalError> {
        ensure_not_cancelled(cancel)?;
        let original = request.original_query.trim();
        if original.is_empty() {
            return Err(RetrievalError::InvalidQuery("original query is empty"));
        }
        let lexical_original = strip_folder_operators(original.to_string());
        let operator_names = folder_operator_names(original);
        let mut folder_operator = None;
        for (index, name) in operator_names.into_iter().enumerate() {
            let resolved = self.resolve_folder_name(pool, &name, cancel).await?;
            ensure_not_cancelled(cancel)?;
            if index == 0 {
                folder_operator = Some(resolved);
            } else if folder_operator.as_deref() != Some(resolved.as_str()) {
                return Err(RetrievalError::InvalidScope(
                    "folder query operators resolve to conflicting folders".to_string(),
                ));
            }
        }
        // Only the first `folder:"..."` operator determines scope; any
        // further occurrence in the remaining text must not leak into the
        // FTS MATCH as literal search terms.
        let lexical_rewritten = request
            .rewritten_query
            .as_deref()
            .map(|rewritten| strip_folder_operators(rewritten.to_string()))
            .filter(|rewritten| !rewritten.is_empty() && rewritten != &lexical_original);

        let scope = match (&request.scope, folder_operator.as_ref()) {
            (PersistedRetrievalScope::All, None) => PersistedRetrievalScope::All,
            // Normalized into folder scope only from All.
            (PersistedRetrievalScope::All, Some(resolved)) => {
                PersistedRetrievalScope::Folder(resolved.clone())
            }
            (PersistedRetrievalScope::Folder(id), Some(resolved)) => {
                if resolved != id {
                    return Err(RetrievalError::InvalidScope(format!(
                        "folder operator resolves to folder {resolved}, conflicting with folder scope {id}"
                    )));
                }
                PersistedRetrievalScope::Folder(id.clone())
            }
            (_, Some(_)) => {
                return Err(RetrievalError::InvalidScope(
                    "folder query operator conflicts with an explicit meeting or allowed-ID scope"
                        .to_string(),
                ));
            }
            (scope, None) => scope.clone(),
        };

        let scope = match scope {
            PersistedRetrievalScope::AllowedMeetingIds(ids) => {
                if ids.len() > MAX_ALLOWED_MEETING_IDS {
                    return Err(RetrievalError::InvalidScope(format!(
                        "allowed-ID scope exceeds the approved {} id bound",
                        MAX_ALLOWED_MEETING_IDS
                    )));
                }
                let mut deduped = Vec::with_capacity(ids.len());
                let mut seen = HashSet::new();
                for id in ids {
                    if seen.insert(id.clone()) {
                        deduped.push(id);
                    }
                }
                PersistedRetrievalScope::AllowedMeetingIds(deduped)
            }
            scope => scope,
        };
        let (membership, folder_over_cap) = self.resolve_membership(pool, &scope, cancel).await?;
        ensure_not_cancelled(cancel)?;
        let core_terms = core_terms(&lexical_original, request.core_language);
        Ok(NormalizedRequest {
            purpose: request.purpose,
            transcript_only: matches!(&scope, PersistedRetrievalScope::Meeting(_)),
            scope,
            membership,
            folder_over_cap,
            lexical_original,
            lexical_rewritten,
            core_terms,
            core_language: request.core_language,
        })
    }

    /// Resolves a `folder:"..."` name to its current folder ID with the same
    /// case-insensitive match as the direct FTS operator. An unknown name
    /// fails closed instead of widening the scope.
    async fn resolve_folder_name(
        &self,
        pool: &SqlitePool,
        name: &str,
        cancel: &CancellationToken,
    ) -> Result<String, RetrievalError> {
        ensure_not_cancelled(cancel)?;
        let folder = FolderRepository::get_by_name(pool, name)
            .await
            .map_err(db_error)?;
        ensure_not_cancelled(cancel)?;
        folder.map(|folder| folder.id).ok_or_else(|| {
            RetrievalError::InvalidScope("folder operator names no current folder".to_string())
        })
    }

    /// Authoritative request-start membership, resolved once per request from
    /// current SQLite state, together with the folder over-cap flag. Folder
    /// scopes use one recursive query capped at [`MAX_FOLDER_SCAN_MEMBERSHIP`]
    /// plus one row: at or below the cap the IDs are a vector-scan
    /// accelerator; above it the semantic scan runs as [`ScopeFilter::All`]
    /// and the repository's recursive root-folder gate is the sole semantic
    /// admission authority (lexical FTS and title scans stay root-scoped
    /// SQL). `All` stays `All`: the FTS and title queries already join
    /// current meetings, and semantic candidates are verified against
    /// current existence per candidate.
    async fn resolve_membership(
        &self,
        pool: &SqlitePool,
        scope: &PersistedRetrievalScope,
        cancel: &CancellationToken,
    ) -> Result<(ScopeFilter, bool), RetrievalError> {
        match scope {
            PersistedRetrievalScope::All => Ok((ScopeFilter::All, false)),
            PersistedRetrievalScope::Meeting(meeting_id) => {
                ensure_not_cancelled(cancel)?;
                let exists: Option<(String,)> =
                    sqlx::query_as("SELECT id FROM meetings WHERE id = ?")
                        .bind(meeting_id)
                        .fetch_optional(pool)
                        .await
                        .map_err(db_error)?;
                ensure_not_cancelled(cancel)?;
                exists
                    .map(|(id,)| (ScopeFilter::meetings([id]), false))
                    .ok_or_else(|| {
                        RetrievalError::InvalidScope(
                            "meeting scope names no current meeting".to_string(),
                        )
                    })
            }
            PersistedRetrievalScope::Folder(folder_id) => {
                ensure_not_cancelled(cancel)?;
                let folder = FolderRepository::get_by_id(pool, folder_id)
                    .await
                    .map_err(db_error)?;
                ensure_not_cancelled(cancel)?;
                if folder.is_none() {
                    return Err(RetrievalError::InvalidScope(
                        "folder scope names no current folder".to_string(),
                    ));
                }
                // At most cap + 1 rows decide whether the subtree fits the
                // approved accelerator cap; the complete membership list is
                // never materialized.
                let meetings: Vec<(String,)> = sqlx::query_as(
                    r#"
                    WITH RECURSIVE folder_scope(id) AS (
                        SELECT id FROM meeting_folders WHERE id = ?
                        UNION ALL
                        SELECT f.id FROM meeting_folders f JOIN folder_scope s ON f.parent_id = s.id
                    )
                    SELECT id FROM meetings
                    WHERE folder_id IN (SELECT id FROM folder_scope)
                    LIMIT ?
                    "#,
                )
                .bind(folder_id)
                .bind(MAX_FOLDER_SCAN_MEMBERSHIP as i64 + 1)
                .fetch_all(pool)
                .await
                .map_err(db_error)?;
                ensure_not_cancelled(cancel)?;
                if meetings.len() > MAX_FOLDER_SCAN_MEMBERSHIP {
                    return Ok((ScopeFilter::All, true));
                }
                Ok((
                    ScopeFilter::meetings(meetings.into_iter().map(|(id,)| id)),
                    false,
                ))
            }
            PersistedRetrievalScope::AllowedMeetingIds(ids) => {
                if ids.len() > MAX_ALLOWED_MEETING_IDS {
                    return Err(RetrievalError::InvalidScope(format!(
                        "allowed-ID scope exceeds the approved {} id bound",
                        MAX_ALLOWED_MEETING_IDS
                    )));
                }
                let mut deduped: Vec<String> = Vec::with_capacity(ids.len());
                let mut seen = HashSet::new();
                for id in ids {
                    if seen.insert(id) {
                        deduped.push(id.clone());
                    }
                }
                if deduped.is_empty() {
                    return Ok((ScopeFilter::meetings(Vec::<String>::new()), false));
                }
                ensure_not_cancelled(cancel)?;
                let mut query =
                    QueryBuilder::<Sqlite>::new("SELECT id FROM meetings WHERE id IN (");
                let mut binds = query.separated(", ");
                for id in &deduped {
                    binds.push_bind(id);
                }
                drop(binds);
                query.push(")");
                let meetings: Vec<(String,)> = query
                    .build_query_as()
                    .fetch_all(pool)
                    .await
                    .map_err(db_error)?;
                ensure_not_cancelled(cancel)?;
                Ok((
                    ScopeFilter::meetings(meetings.into_iter().map(|(id,)| id)),
                    false,
                ))
            }
        }
    }

    // -- Lexical channels ------------------------------------------------------

    async fn lexical_channel(
        &self,
        pool: &SqlitePool,
        normalized: &NormalizedRequest,
        limits: RetrievalLimits,
        cancel: &CancellationToken,
        candidates: &mut HashMap<String, RetrievedEvidence>,
    ) -> Result<(), RetrievalError> {
        if normalized.transcript_only {
            return self
                .meeting_transcript_lexical_channel(pool, normalized, limits, cancel, candidates)
                .await;
        }
        let mut variants: Vec<(QueryVariantKind, String)> = vec![(
            QueryVariantKind::Original,
            normalized.lexical_original.clone(),
        )];
        if let Some(rewritten) = &normalized.lexical_rewritten {
            variants.push((QueryVariantKind::Rewritten, rewritten.clone()));
        }
        let core_text = normalized.core_terms.join(" ");
        if !core_text.is_empty() {
            variants.push((QueryVariantKind::CoreTerms, core_text));
        }
        for (variant, text) in variants {
            ensure_not_cancelled(cancel)?;
            let and_results = self
                .fts_for_scope(
                    pool,
                    &text,
                    limits.lexical_per_variant,
                    normalized,
                    MatchMode::And,
                )
                .await?;
            ensure_not_cancelled(cancel)?;
            // The AND pass alone can already fill the per-variant bound; the
            // OR pass exists only to fill what AND left short, so skip the
            // extra FTS round-trip (and its snippet-expansion cost) when
            // there is nothing left to fill.
            let or_results = if and_results.len() >= limits.lexical_per_variant {
                Vec::new()
            } else {
                self.fts_for_scope(
                    pool,
                    &text,
                    limits.lexical_per_variant,
                    normalized,
                    MatchMode::Or,
                )
                .await?
            };
            ensure_not_cancelled(cancel)?;
            // AND ranks first, OR fills the remainder of the per-variant
            // bound; a chunk already claimed keeps its first (mode, rank).
            let mut claimed = HashSet::new();
            let mut rank = 0usize;
            for (mode, results) in [
                (LexicalMode::And, and_results),
                (LexicalMode::Or, or_results),
            ] {
                for result in results {
                    if rank >= limits.lexical_per_variant {
                        break;
                    }
                    let key = (
                        result.meeting_id.clone(),
                        result.chunk_type.clone(),
                        result.chunk_id.clone(),
                    );
                    if !claimed.insert(key) {
                        continue;
                    }
                    rank += 1;
                    record_candidate(
                        candidates,
                        lexical_evidence(result),
                        EvidenceProvenance {
                            channel: RetrievalChannel::Lexical,
                            variant,
                            mode: Some(mode),
                            rank,
                            query_slot: 0,
                        },
                    );
                }
                if rank >= limits.lexical_per_variant {
                    break;
                }
            }
        }
        Ok(())
    }

    async fn meeting_transcript_lexical_channel(
        &self,
        pool: &SqlitePool,
        normalized: &NormalizedRequest,
        limits: RetrievalLimits,
        cancel: &CancellationToken,
        candidates: &mut HashMap<String, RetrievedEvidence>,
    ) -> Result<(), RetrievalError> {
        if limits.lexical_per_variant == 0 {
            return Ok(());
        }
        let mut queries = Vec::with_capacity(2);
        if let Some(rewritten) = &normalized.lexical_rewritten {
            queries.push((QueryVariantKind::Rewritten, rewritten.clone()));
        }
        queries.push((
            QueryVariantKind::Original,
            normalized.lexical_original.clone(),
        ));
        for (variant, text) in queries {
            ensure_not_cancelled(cancel)?;
            let mut mode = MatchMode::And;
            let mut results = self
                .fts_for_scope(pool, &text, limits.lexical_per_variant, normalized, mode)
                .await?;
            if results.is_empty() {
                mode = MatchMode::Or;
                results = self
                    .fts_for_scope(
                        pool,
                        &text,
                        limits.lexical_per_variant,
                        normalized,
                        MatchMode::Or,
                    )
                    .await?;
            }
            ensure_not_cancelled(cancel)?;
            if results.is_empty() {
                continue;
            }
            for (rank, result) in results.into_iter().enumerate() {
                record_candidate(
                    candidates,
                    lexical_evidence(result),
                    EvidenceProvenance {
                        channel: RetrievalChannel::Lexical,
                        variant,
                        mode: Some(match mode {
                            MatchMode::And => LexicalMode::And,
                            _ => LexicalMode::Or,
                        }),
                        rank: rank + 1,
                        query_slot: 0,
                    },
                );
            }
            break;
        }
        Ok(())
    }

    async fn fts_for_scope(
        &self,
        pool: &SqlitePool,
        text: &str,
        limit: usize,
        normalized: &NormalizedRequest,
        mode: MatchMode,
    ) -> Result<Vec<FtsSearchResult>, RetrievalError> {
        if text.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let limit = limit as u32;
        let results = match &normalized.scope {
            PersistedRetrievalScope::All => {
                FtsRepository::search_with_mode_plain(pool, text, limit, None, mode).await
            }
            PersistedRetrievalScope::Meeting(meeting_id) => {
                FtsRepository::search_transcripts_with_mode(pool, text, limit, meeting_id, mode)
                    .await
            }
            PersistedRetrievalScope::Folder(folder_id) => {
                FtsRepository::search_with_folder_id_plain(pool, text, limit, folder_id, mode).await
            }
            PersistedRetrievalScope::AllowedMeetingIds(_) => {
                let ScopeFilter::Meetings(ids) = &normalized.membership else {
                    return Ok(Vec::new());
                };
                let ids: Vec<String> = ids.iter().cloned().collect();
                FtsRepository::search_with_meeting_ids_plain(pool, text, limit, &ids, mode).await
            }
        };
        let results = results.map_err(db_error)?;
        // Current membership is authoritative; stale FTS folder metadata can
        // never admit an out-of-scope meeting.
        Ok(results
            .into_iter()
            .filter(|result| normalized.membership.allows(&result.meeting_id))
            .collect())
    }

    /// Authoritative current-title candidates. FTS does not index titles, so
    /// title-only search must not depend on semantic availability.
    ///
    /// Streaming with bounded top-k: meetings are read in keyset-paged
    /// batches and only the current best `lexical_per_variant` candidates are
    /// ever held, so a large corpus is never fetched, stored, or sorted whole
    /// before truncation. Scope safety, the overlap score, and the
    /// (overlap desc, meeting id asc) ordering are identical to a full sort.
    ///
    /// [`CoreTermLanguage::Unknown`] deliberately keeps every normalized query
    /// token in `core_terms` for Search, whose contract requires title search
    /// without an explicit language. Other purposes keep the guard against
    /// function-word title signals outranking content evidence.
    async fn title_channel(
        &self,
        pool: &SqlitePool,
        normalized: &NormalizedRequest,
        limits: RetrievalLimits,
        cancel: &CancellationToken,
        candidates: &mut HashMap<String, RetrievedEvidence>,
    ) -> Result<(), RetrievalError> {
        if normalized.core_terms.is_empty()
            || limits.lexical_per_variant == 0
            || (normalized.purpose != RetrievalPurpose::Search
                && normalized.core_language == CoreTermLanguage::Unknown)
        {
            return Ok(());
        }
        let mut top: std::collections::BinaryHeap<std::cmp::Reverse<TitleCandidate>> =
            std::collections::BinaryHeap::with_capacity(limits.lexical_per_variant + 1);
        match &normalized.scope {
            PersistedRetrievalScope::Folder(folder_id) => {
                let mut cursor = String::new();
                loop {
                    ensure_not_cancelled(cancel)?;
                    let mut query = QueryBuilder::<Sqlite>::new(
                        "WITH RECURSIVE folder_scope(id) AS (SELECT id FROM meeting_folders WHERE id = ",
                    );
                    query.push_bind(folder_id);
                    query.push(
                        " UNION ALL SELECT f.id FROM meeting_folders f JOIN folder_scope s ON f.parent_id = s.id) SELECT m.id, m.title FROM meetings m WHERE m.id > ",
                    );
                    query.push_bind(&cursor);
                    query.push(
                        " AND m.folder_id IN (SELECT id FROM folder_scope) ORDER BY m.id LIMIT ",
                    );
                    query.push_bind(TITLE_SCAN_PAGE as i64);
                    let rows: Vec<(String, String)> = query
                        .build_query_as()
                        .fetch_all(pool)
                        .await
                        .map_err(db_error)?;
                    ensure_not_cancelled(cancel)?;
                    let next_cursor = rows.last().map(|(id, _)| id.clone());
                    let complete_page = rows.len() == TITLE_SCAN_PAGE;
                    push_title_candidates(&mut top, rows, normalized, limits.lexical_per_variant);
                    match next_cursor {
                        Some(id) if complete_page => cursor = id,
                        _ => break,
                    }
                }
            }
            PersistedRetrievalScope::Meeting(_) | PersistedRetrievalScope::AllowedMeetingIds(_) => {
                let ScopeFilter::Meetings(ids) = &normalized.membership else {
                    return Ok(());
                };
                if ids.is_empty() {
                    return Ok(());
                }
                ensure_not_cancelled(cancel)?;
                let mut query =
                    QueryBuilder::<Sqlite>::new("SELECT id, title FROM meetings WHERE id IN (");
                let mut binds = query.separated(", ");
                for id in ids.iter() {
                    binds.push_bind(id);
                }
                drop(binds);
                query.push(")");
                let rows: Vec<(String, String)> = query
                    .build_query_as()
                    .fetch_all(pool)
                    .await
                    .map_err(db_error)?;
                ensure_not_cancelled(cancel)?;
                push_title_candidates(&mut top, rows, normalized, limits.lexical_per_variant);
            }
            PersistedRetrievalScope::All => {
                let mut cursor = String::new();
                loop {
                    // A cancelled request must not continue to later SQL pages.
                    ensure_not_cancelled(cancel)?;
                    let mut query =
                        QueryBuilder::<Sqlite>::new("SELECT id, title FROM meetings WHERE id > ");
                    query.push_bind(&cursor);
                    query.push(" ORDER BY id LIMIT ");
                    query.push_bind(TITLE_SCAN_PAGE as i64);
                    let rows: Vec<(String, String)> = query
                        .build_query_as()
                        .fetch_all(pool)
                        .await
                        .map_err(db_error)?;
                    ensure_not_cancelled(cancel)?;
                    let next_cursor = rows.last().map(|(id, _)| id.clone());
                    let complete_page = rows.len() == TITLE_SCAN_PAGE;
                    push_title_candidates(&mut top, rows, normalized, limits.lexical_per_variant);
                    match next_cursor {
                        Some(id) if complete_page => cursor = id,
                        _ => break,
                    }
                }
            }
        }
        let mut ranked: Vec<(usize, String, String)> = top
            .into_iter()
            .map(|entry| (entry.0.overlap, entry.0.meeting_id, entry.0.title))
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        for (rank, (_, meeting_id, title)) in ranked.into_iter().enumerate() {
            record_candidate(
                candidates,
                title_evidence(meeting_id, title),
                EvidenceProvenance {
                    channel: RetrievalChannel::Title,
                    variant: QueryVariantKind::CoreTerms,
                    mode: None,
                    rank: rank + 1,
                    query_slot: 0,
                },
            );
        }
        Ok(())
    }

    // -- Semantic channel -------------------------------------------------------

    /// Embeds the approved variants and searches the active semantic
    /// generation. Every failure degrades to the lexical channels with a
    /// documented reason; only cancellation aborts the request.
    async fn semantic_channel(
        &self,
        pool: &SqlitePool,
        normalized: &NormalizedRequest,
        limits: RetrievalLimits,
        cancel: &CancellationToken,
        candidates: &mut HashMap<String, RetrievedEvidence>,
    ) -> Result<Option<SemanticFallbackReason>, RetrievalError> {
        if limits.vector_per_variant == 0 {
            return Ok(None);
        }
        let index = self.lifecycle.index_service();
        let Some(snapshot) = index.active_snapshot() else {
            return Ok(Some(SemanticFallbackReason::NoActiveGeneration));
        };
        // The generation/model the snapshot was read under. Candidate
        // verification and content reads below are fenced to this generation,
        // so a snapshot swap mid-request can only drop hits, never admit
        // another generation's rows.
        let generation_id = snapshot.generation_id().to_string();
        let model_id = snapshot.model_id().to_string();
        drop(snapshot);
        ensure_not_cancelled(cancel)?;
        let mut variants: Vec<(QueryVariantKind, String)> = vec![(
            QueryVariantKind::Original,
            normalized.lexical_original.clone(),
        )];
        if let Some(rewritten) = &normalized.lexical_rewritten {
            variants.push((QueryVariantKind::Rewritten, rewritten.clone()));
        }
        // A folder-operator-only request has no text to embed.
        variants.retain(|(_, text)| !text.trim().is_empty());
        if variants.is_empty() {
            return Ok(None);
        }

        let ticket = match self.lifecycle.scheduler().enqueue_interactive() {
            Ok(ticket) => ticket,
            Err(_) => return Ok(Some(SemanticFallbackReason::SchedulerRejected)),
        };
        // The request token aborts the queued wait immediately; the ticket is
        // removed from the bounded queue instead of waiting for (or later
        // consuming) the single inference permit.
        let _lease = match ticket.wait_for_permit_with(cancel).await {
            Ok(lease) => lease,
            Err(SchedulerRejection::CancelledWhileQueued) => return Err(RetrievalError::Cancelled),
            Err(_) => return Ok(Some(SemanticFallbackReason::SchedulerRejected)),
        };
        // Same loader as the worker, so the same process-wide cached session
        // set - never a second model-session owner.
        let embedder = match self.lifecycle.load_embedder().await {
            Ok(embedder) => embedder,
            Err(_) => return Ok(Some(SemanticFallbackReason::EmbeddingUnavailable)),
        };
        // Prior-generation vectors must never be scored with another model's
        // query embeddings.
        if embedder.model_id() != model_id {
            return Ok(Some(SemanticFallbackReason::ModelMismatch));
        }
        let query_texts: Vec<String> = variants.iter().map(|(_, text)| text.clone()).collect();
        let embedded = {
            let embedder = Arc::clone(&embedder);
            let cancel = cancel.clone();
            tokio::task::spawn_blocking(move || {
                embedder.embed_queries_blocking(&query_texts, &cancel)
            })
            .await
        };
        let vectors = match embedded {
            Ok(Ok(vectors)) => vectors,
            Ok(Err(RetrievalModelError::Cancelled)) => return Err(RetrievalError::Cancelled),
            Ok(Err(_)) | Err(_) => {
                return Ok(Some(SemanticFallbackReason::EmbeddingUnavailable));
            }
        };
        drop(_lease);
        ensure_not_cancelled(cancel)?;
        if vectors.len() != variants.len() {
            return Ok(Some(SemanticFallbackReason::EmbeddingUnavailable));
        }

        let scope_filter = normalized.membership.clone();
        // An over-cap folder scans the bounded global over-fetch up to the
        // approved ceiling; the root gate below then retains the in-scope
        // per-variant top-k. Every other scope scans exactly its per-variant
        // bound (the index clamps to the same ceiling).
        let scan_limit = if normalized.folder_over_cap {
            MAX_QUERY_LIMIT
        } else {
            limits.vector_per_variant
        };
        let mut pending: Vec<(QueryVariantKind, Vec<f32>)> = variants
            .into_iter()
            .zip(vectors)
            .map(|((variant, _), vector)| (variant, vector))
            .collect();
        let mut hits: Vec<(QueryVariantKind, usize, VectorHit)> = Vec::new();
        for attempt in 0..=CATCHUP_RETRIES {
            let mut retried = Vec::new();
            for (variant, vector) in pending {
                match index
                    .search_pinned_for_source(
                        &vector,
                        scope_filter.clone(),
                        scan_limit,
                        cancel,
                        &generation_id,
                        normalized.transcript_only.then_some("transcript"),
                    )
                    .await
                {
                    Ok(vector_hits) => {
                        hits.extend(
                            vector_hits
                                .into_iter()
                                .enumerate()
                                .map(|(index, hit)| (variant, index + 1, hit)),
                        );
                        #[cfg(test)]
                        self.after_variant_scan().await;
                    }
                    Err(SearchFailure::Cancelled) => return Err(RetrievalError::Cancelled),
                    Err(SearchFailure::CatchUpPending { behind }) => {
                        if attempt < CATCHUP_RETRIES {
                            retried.push((variant, vector));
                        } else {
                            return Ok(Some(SemanticFallbackReason::CatchUpTimeout { behind }));
                        }
                    }
                    Err(SearchFailure::NoActiveGeneration) => {
                        return Ok(Some(SemanticFallbackReason::NoActiveGeneration));
                    }
                    Err(SearchFailure::ModelMismatch | SearchFailure::InvalidQuery(_)) => {
                        // Any identity/shape failure invalidates the whole
                        // semantic stage: no partial hits survive.
                        return Ok(Some(SemanticFallbackReason::ModelMismatch));
                    }
                    Err(SearchFailure::GenerationChanged) => {
                        // A benign mid-request activation swap, not a model
                        // divergence: no partial hits survive either way.
                        return Ok(Some(SemanticFallbackReason::GenerationChanged));
                    }
                    Err(SearchFailure::ScanFailed(_)) => {
                        return Ok(Some(SemanticFallbackReason::SemanticScanFailed));
                    }
                }
            }
            if retried.is_empty() {
                break;
            }
            pending = retried;
            tokio::select! {
                _ = cancel.cancelled() => return Err(RetrievalError::Cancelled),
                _ = tokio::time::sleep(PAUSE_QUANTUM) => {}
            }
        }
        // Fence the active generation again before any accumulated hit is
        // used: an activation between variant scans cannot publish partial
        // semantic candidates.
        if index.active_generation().as_deref() != Some(generation_id.as_str()) {
            return Ok(Some(SemanticFallbackReason::GenerationChanged));
        }

        // Current-membership re-filter (defense in depth on top of the scan
        // filter), then authoritative existence/dirty verification.
        let filtered: Vec<&(QueryVariantKind, usize, VectorHit)> = hits
            .iter()
            .filter(|(_, _, hit)| normalized.membership.allows(&hit.meeting_id))
            .collect();
        if filtered.is_empty() {
            // Explicit tested rule: a zero-hit semantic stage that completed
            // without a typed failure still proves the generation served a
            // Fast hybrid query, so it counts.
            index.acknowledge_fast_hybrid_query();
            return Ok(None);
        }
        // Folder scopes verify inside the recursive root-folder gate of the
        // same authoritative read: it is the definitive membership check for
        // both under-cap and over-cap requests.
        let folder_root = match &normalized.scope {
            PersistedRetrievalScope::Folder(folder_id) => Some(folder_id.clone()),
            _ => None,
        };
        let meeting_ids: Vec<String> = filtered
            .iter()
            .map(|(_, _, hit)| hit.meeting_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        ensure_not_cancelled(cancel)?;
        let verified: HashMap<String, String> =
            match RetrievalRepository::verified_semantic_meetings(
                pool,
                &generation_id,
                &meeting_ids,
                folder_root.as_deref(),
            )
            .await
            {
                Ok(verified) => verified.into_iter().collect(),
                // A failed candidate-gate read degrades only the semantic
                // channel; already collected lexical candidates stay, and the
                // Fast hybrid query is never acknowledged.
                Err(_) => return Ok(Some(SemanticFallbackReason::SemanticScanFailed)),
            };
        ensure_not_cancelled(cancel)?;
        // Re-fence after the awaited candidate-gate SQL read: an activation
        // during validation cannot publish partial semantic candidates.
        if index.active_generation().as_deref() != Some(generation_id.as_str()) {
            return Ok(Some(SemanticFallbackReason::GenerationChanged));
        }
        // The over-cap scan over-fetched globally, so nothing beyond the
        // per-variant bound may be published: after the authoritative root
        // gate, retain the first bound entries per variant in the scan's
        // score order and rank 1..=n among the retained in-scope candidates.
        let selected: Vec<(QueryVariantKind, usize, &VectorHit)> = if normalized.folder_over_cap {
            let mut retained: HashMap<QueryVariantKind, usize> = HashMap::new();
            filtered
                .into_iter()
                .filter(|(_, _, hit)| verified.contains_key(&hit.meeting_id))
                .filter_map(|(variant, _, hit)| {
                    let count = retained.entry(*variant).or_insert(0);
                    if *count >= limits.vector_per_variant {
                        return None;
                    }
                    *count += 1;
                    Some((*variant, *count, hit))
                })
                .collect()
        } else {
            filtered
                .into_iter()
                .map(|(variant, rank, hit)| (*variant, *rank, hit))
                .collect()
        };
        let document_ids: Vec<String> = selected
            .iter()
            .map(|(_, _, hit)| hit.document_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        ensure_not_cancelled(cancel)?;
        let contents: HashMap<String, String> =
            match RetrievalRepository::document_contents(pool, &generation_id, &document_ids).await
            {
                Ok(contents) => contents.into_iter().collect(),
                Err(_) => return Ok(Some(SemanticFallbackReason::SemanticScanFailed)),
            };
        ensure_not_cancelled(cancel)?;
        // A swap while canonical content was read invalidates the whole
        // semantic stage before any stale-generation evidence is published.
        if index.active_generation().as_deref() != Some(generation_id.as_str()) {
            return Ok(Some(SemanticFallbackReason::GenerationChanged));
        }
        for (variant, rank, hit) in selected {
            let Some(title) = verified.get(&hit.meeting_id) else {
                continue;
            };
            let Some(content) = contents.get(&hit.document_id) else {
                continue;
            };
            record_candidate(
                candidates,
                semantic_evidence(hit, title.clone(), content.clone()),
                EvidenceProvenance {
                    channel: RetrievalChannel::Semantic,
                    variant,
                    mode: None,
                    rank,
                    query_slot: 0,
                },
            );
        }
        // The Fast hybrid query counts only when the semantic stage validated
        // successfully end to end - after the candidate-gate/content reads and
        // the final fence, with or without hits (explicit tested rule). Fence
        // and SQL failures never reach this line.
        index.acknowledge_fast_hybrid_query();
        Ok(None)
    }

    /// Test-only gate point (see [`SemanticScanGate`]): no-op when unarmed.
    #[cfg(test)]
    async fn after_variant_scan(&self) {
        let sender = self
            .scan_gate
            .armed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(());
            self.scan_gate.release.notified().await;
        }
    }
}

impl NormalizedRequest {
    fn resolved(&self) -> ResolvedScope {
        ResolvedScope {
            scope: self.scope.clone(),
        }
    }
}

/// Deduplicates repeat matches within one channel's evidence-ID namespace
/// (e.g. the same FTS chunk hit by two query variants) while accumulating
/// their provenance. Different channels never share an evidence-ID
/// namespace (see [`RetrievedEvidence`]), so this cannot and does not unify
/// a lexical and a semantic candidate covering the same source text - that
/// cross-channel fusion is Task 3.2's job.
fn record_candidate(
    candidates: &mut HashMap<String, RetrievedEvidence>,
    evidence: RetrievedEvidence,
    provenance: EvidenceProvenance,
) {
    candidates
        .entry(evidence.evidence_id.clone())
        .or_insert(evidence)
        .provenance
        .push(provenance);
}

fn lexical_evidence(result: FtsSearchResult) -> RetrievedEvidence {
    // Mirrors the semantic chunker's source identity: transcripts carry their
    // segment row, summaries their template, notes their meeting row.
    let (source_start_id, source_template_id) = match result.chunk_type.as_str() {
        "transcript" => (Some(result.chunk_id.clone()), None),
        "summary" => (
            None,
            result
                .chunk_id
                .rsplit_once(':')
                .map(|(_, template)| template.to_string()),
        ),
        _ => (None, None),
    };
    RetrievedEvidence {
        evidence_id: format!("fts:{}:{}", result.chunk_type, result.chunk_id),
        meeting_id: result.meeting_id,
        meeting_title: result.meeting_title,
        source_kind: result.chunk_type,
        source_start_id,
        source_end_id: None,
        source_template_id,
        heading: None,
        ordinal: 0,
        text: result.snippet,
        speaker: result.speaker,
        timestamp_label: result.timestamp_label,
        provenance: Vec::new(),
        source_aliases: Vec::new(),
    }
}

fn title_evidence(meeting_id: String, title: String) -> RetrievedEvidence {
    RetrievedEvidence {
        evidence_id: format!("title:{meeting_id}"),
        source_kind: "title".to_string(),
        text: title.clone(),
        meeting_id,
        meeting_title: title,
        source_start_id: None,
        source_end_id: None,
        source_template_id: None,
        heading: None,
        ordinal: 0,
        speaker: None,
        timestamp_label: None,
        provenance: Vec::new(),
        source_aliases: Vec::new(),
    }
}

fn semantic_evidence(hit: &VectorHit, meeting_title: String, content: String) -> RetrievedEvidence {
    RetrievedEvidence {
        evidence_id: hit.document_id.clone(),
        meeting_id: hit.meeting_id.clone(),
        meeting_title,
        source_kind: hit.source_kind.clone(),
        source_start_id: hit.source_start_id.clone(),
        source_end_id: hit.source_end_id.clone(),
        source_template_id: hit.source_template_id.clone(),
        heading: hit.heading.clone(),
        ordinal: hit.ordinal,
        text: content,
        speaker: None,
        timestamp_label: None,
        provenance: Vec::new(),
        source_aliases: Vec::new(),
    }
}

/// Deterministic channel-ranked presentation: candidates sort by their best
/// (channel, variant, mode, rank) provenance, then evidence ID. Rank fusion
/// itself is Task 3.2.
fn order_candidates(candidates: HashMap<String, RetrievedEvidence>) -> Vec<RetrievedEvidence> {
    let mut ordered: Vec<RetrievedEvidence> = candidates.into_values().collect();
    ordered.sort_by(|a, b| {
        best_provenance(a)
            .cmp(&best_provenance(b))
            .then_with(|| a.evidence_id.cmp(&b.evidence_id))
    });
    ordered
}

fn best_provenance(evidence: &RetrievedEvidence) -> (u8, u8, u8, usize) {
    evidence
        .provenance
        .iter()
        .map(|provenance| {
            (
                channel_order(provenance.channel),
                variant_order(provenance.variant),
                mode_order(provenance.mode),
                provenance.rank,
            )
        })
        .min()
        .unwrap_or((u8::MAX, u8::MAX, u8::MAX, usize::MAX))
}

fn channel_order(channel: RetrievalChannel) -> u8 {
    match channel {
        RetrievalChannel::Lexical => 0,
        RetrievalChannel::Title => 1,
        RetrievalChannel::Semantic => 2,
    }
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

/// The evaluated lexical core-term policy (Task 1.2
/// `evaluation_policy.json`): split on non-alphanumeric Unicode characters,
/// lowercase, fold the listed Portuguese diacritics, then remove only the
/// fixed language-specific high-frequency list. Fixed closed lists only -
/// never per-corpus inference, never an all-language union as a language
/// substitute. Names and numeric tokens survive unless they exactly equal a
/// listed function word.
pub(crate) const PORTUGUESE_HIGH_FREQUENCY: &[&str] = &[
    "a", "as", "o", "os", "de", "da", "das", "do", "dos", "e", "em", "no", "na", "nos", "nas",
    "para", "por", "que", "qual", "quais", "como", "foi",
];

pub(crate) const ENGLISH_HIGH_FREQUENCY: &[&str] = &[
    "a", "an", "the", "of", "to", "and", "in", "on", "for", "by", "that", "what", "which", "how",
    "was", "were", "is", "are",
];

/// Identical character-level normalization to the evaluation harness
/// (`retrieval_evaluation::normalize_core_token`), so production and
/// evaluation tokenize identically, including the ASCII-only lowercasing.
pub(crate) fn normalize_core_token(token: &str) -> String {
    token
        .chars()
        .map(|character| match character.to_ascii_lowercase() {
            'á' | 'à' | 'â' | 'ã' | 'Á' | 'À' | 'Â' | 'Ã' => 'a',
            'é' | 'ê' | 'É' | 'Ê' => 'e',
            'í' | 'Í' => 'i',
            'ó' | 'ô' | 'õ' | 'Ó' | 'Ô' | 'Õ' => 'o',
            'ú' | 'ü' | 'Ú' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            other => other,
        })
        .collect()
}

/// Evaluated core terms of the original question: normalization is
/// language-independent, removal uses only the fixed list of the language the
/// caller stated on the request (the evaluated harness's explicit language
/// field - no runtime detection, no all-language union), and an all-stopword
/// result falls back to the untouched normalized tokens. The answer question
/// itself is never changed - this only feeds the additional core-term variant
/// and the title channel. For an unknown language nothing is removed, so the
/// core variant may equal the original while keeping its distinct provenance.
pub(crate) fn core_terms(query: &str, language: CoreTermLanguage) -> Vec<String> {
    let high_frequency: &[&str] = match language {
        CoreTermLanguage::Portuguese => PORTUGUESE_HIGH_FREQUENCY,
        CoreTermLanguage::English => ENGLISH_HIGH_FREQUENCY,
        CoreTermLanguage::Unknown => &[],
    };
    let normalized: Vec<String> = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(normalize_core_token)
        .collect();
    let core: Vec<String> = normalized
        .iter()
        .cloned()
        .filter(|token| !high_frequency.contains(&token.as_str()))
        .collect();
    if core.is_empty() {
        normalized
    } else {
        core
    }
}

/// Scores one page/batch of `(id, title)` rows against the bounded top-k
/// heap. Shared by the full-corpus streamed scan and the bounded-scope
/// direct read so both apply identical scope, overlap, and eviction rules.
fn push_title_candidates(
    top: &mut std::collections::BinaryHeap<std::cmp::Reverse<TitleCandidate>>,
    rows: Vec<(String, String)>,
    normalized: &NormalizedRequest,
    cap: usize,
) {
    for (meeting_id, title) in rows {
        if !normalized.membership.allows(&meeting_id) {
            continue;
        }
        let overlap = title_term_overlap(&normalized.core_terms, &title);
        if overlap == 0 {
            continue;
        }
        let candidate = TitleCandidate {
            overlap,
            meeting_id,
            title,
        };
        if top.len() < cap {
            top.push(std::cmp::Reverse(candidate));
        } else if let Some(worst) = top.peek().map(|entry| &entry.0) {
            if candidate.cmp(worst) == std::cmp::Ordering::Greater {
                top.pop();
                top.push(std::cmp::Reverse(candidate));
            }
        }
    }
}

fn title_term_overlap(terms: &[String], title: &str) -> usize {
    let title_terms: HashSet<String> = title
        .split(|character: char| !character.is_alphanumeric())
        .map(normalize_core_token)
        .collect();
    let terms: HashSet<&String> = terms.iter().collect();
    terms
        .iter()
        .filter(|term| title_terms.contains(**term))
        .count()
}

/// One in-flight title candidate. Ascending quality (min-heap under
/// `Reverse`): lower overlap first, and among equal overlaps the
/// lexicographically LARGER meeting id is worse, so bounded eviction keeps
/// the smaller id and the final order is overlap desc, meeting id asc.
///
/// `Eq`/`Ord` are both keyed on `(overlap, meeting_id)` only, matching each
/// other exactly (`meetings.id` is a primary key, so `title` never varies
/// for a fixed `meeting_id` and dropping it from equality changes nothing
/// observable today - but it keeps the `BinaryHeap` invariant that equal
/// `Ord` implies equal `Eq` true by construction rather than by coincidence).
#[derive(Debug, Clone)]
struct TitleCandidate {
    overlap: usize,
    meeting_id: String,
    title: String,
}

impl PartialEq for TitleCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.overlap == other.overlap && self.meeting_id == other.meeting_id
    }
}

impl Eq for TitleCandidate {}

impl Ord for TitleCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.overlap
            .cmp(&other.overlap)
            .then_with(|| other.meeting_id.cmp(&self.meeting_id))
    }
}

impl PartialOrd for TitleCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The service's single log line: counts, scope tag, and fallback reason.
/// Never query text, candidate text, or raw identifiers.
fn outcome_line(
    scope: &PersistedRetrievalScope,
    candidate_count: usize,
    fallback: &Option<SemanticFallbackReason>,
) -> String {
    let scope_tag = match scope {
        PersistedRetrievalScope::All => "all".to_string(),
        PersistedRetrievalScope::Meeting(_) => "meeting".to_string(),
        PersistedRetrievalScope::Folder(_) => "folder".to_string(),
        PersistedRetrievalScope::AllowedMeetingIds(ids) => format!("allowed_ids({})", ids.len()),
    };
    format!(
        "Retrieval: scope={scope_tag} candidates={candidate_count} semantic_fallback={}",
        fallback.as_ref().map_or("none", |reason| reason.tag())
    )
}

impl SemanticFallbackReason {
    fn tag(&self) -> &'static str {
        match self {
            Self::NoActiveGeneration => "no_active_generation",
            Self::ModelMismatch => "model_mismatch",
            Self::GenerationChanged => "generation_changed",
            Self::EmbeddingUnavailable => "embedding_unavailable",
            Self::SchedulerRejected => "scheduler_rejected",
            Self::ForcedLexical => "forced_lexical",
            Self::CatchUpTimeout { .. } => "catch_up_timeout",
            Self::SemanticScanFailed => "semantic_scan_failed",
        }
    }
}
