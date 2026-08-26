//! Immutable exact query index, journal publication, and generation
//! activation (Sprint 2B Task 2.5).
//!
//! SQLite vectors stay canonical. One process-wide [`QueryIndexService`]
//! serves queries from an immutable [`IndexSnapshot`] for the active
//! generation: a contiguous validated int8 base plus a bounded per-meeting
//! overlay (upsert replacements and deletion tombstones) replayed from the
//! durable `retrieval_index_changes` journal after the persisted published
//! bound. Readers clone the snapshot `Arc` and release all locks before
//! scanning, so they observe either the old complete snapshot or the new
//! complete one - never partial internals - and single-meeting updates never
//! copy the base.
//!
//! The publisher runs inside the Task 2.4 worker loop (`publish_tick`): it
//! loads canonical state through one consistent SQLite read snapshot, swaps
//! finished snapshots atomically, acknowledges durable journal bounds only
//! after their snapshot is installed (failed acknowledgements heal through
//! idempotent replay), replays canonical-ahead-of-published windows,
//! compacts at the approved 2% delta threshold (tombstones included),
//! activates completed shadow generations through the singleton pointer once
//! every gate passes, deactivates known-corrupt active generations to
//! FTS-only, and garbage-collects retired generations that survived one clean
//! restart plus one successful query. Status labels derived-disk payload
//! sums as estimates; the activation gate consumes a conservative SQLite
//! backing-store upper bound against the approved 3 GiB peak.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, PoisonError};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::database::repositories::retrieval::{
    CanonicalSnapshotRead, GenerationStatus, RetrievalRepository, SnapshotDocument,
};
use crate::retrieval::worker::{
    quantize_int8, RetrievalScheduler, APPROVED_INT8_DEQUANTIZATION_SCALE,
};

/// Approved operating limit: compact when the overlay reaches 2% of the base
/// (overlay documents x 50 >= base rows).
const COMPACTION_DENOMINATOR: usize = 50;
/// Journal rows applied per replay batch; sparse AUTOINCREMENT IDs are
/// absorbed naturally by reading `change_id > published`.
const JOURNAL_REPLAY_BATCH: i64 = 512;
/// Upper bound on replay batches per worker tick so crash catch-up stays
/// bounded and other work cannot starve.
const REPLAY_BATCHES_PER_TICK: usize = 16;
/// Approved candidate ceiling for one vector query.
pub const MAX_QUERY_LIMIT: usize = 150;
/// Approved envelope: 2 GiB steady-state derived-disk target.
pub const DERIVED_DISK_STEADY_TARGET_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Approved envelope: activation blocks above the 3 GiB shadow-rebuild peak.
pub const DERIVED_DISK_ACTIVATION_LIMIT_BYTES: u64 = 3 * 1024 * 1024 * 1024;
/// Approved transient RAM ceiling measured for the approved two-snapshot
/// e5-base int8 bundle (`architecture.md` "Approved Exact Backend Contract"):
/// any activation peak at or above it blocks until a user-approved remedy
/// exists. A third resident snapshot remains unapproved regardless.
pub const ACTIVATION_RAM_CEILING_BYTES: u64 = 1_073_741_824 * 13 / 10;

/// Measured process resident physical memory, or `None` when the platform
/// facility is unavailable (which itself blocks activation).
pub(crate) type RamProbe = Arc<dyn Fn() -> Option<u64> + Send + Sync>;

fn measure_resident_ram() -> Option<u64> {
    memory_stats::memory_stats().map(|stats| stats.physical_mem as u64)
}

/// Activation RAM gate over one real process measurement: below the ceiling
/// admits; at/above the ceiling blocks with the measured value; an
/// unavailable measurement blocks fail-closed rather than guessing.
fn ram_gate_blocker(measured_bytes: Option<u64>) -> Option<String> {
    match measured_bytes {
        None => Some(
            "resident memory measurement unavailable; refusing activation".to_string(),
        ),
        Some(bytes) if bytes >= ACTIVATION_RAM_CEILING_BYTES => Some(format!(
            "measured resident memory {bytes} bytes meets or exceeds the {ACTIVATION_RAM_CEILING_BYTES} byte activation ceiling"
        )),
        Some(_) => None,
    }
}

// ---------------------------------------------------------------------------
// Snapshot model
// ---------------------------------------------------------------------------

/// Identity/provenance retained per indexed document for later hydration and
/// reranking (Sprint 3 consumers).
#[derive(Debug, Clone)]
pub struct DocumentMeta {
    pub document_id: String,
    pub meeting_id: String,
    pub source_kind: String,
    pub source_start_id: Option<String>,
    pub source_end_id: Option<String>,
    pub source_template_id: Option<String>,
    /// Section-heading provenance persisted with the canonical row.
    pub heading: Option<String>,
    pub ordinal: i64,
}

/// Contiguous int8 base rows in load order. Row position is storage layout
/// only; every lookup goes through meeting identity, so deletes and
/// compaction need no document-id index.
#[derive(Default)]
struct BaseRows {
    metas: Vec<DocumentMeta>,
    /// Flat `metas.len() * dimensions` int8 bytes (stored as raw u8).
    vectors: Vec<u8>,
}

impl BaseRows {
    fn len(&self) -> usize {
        self.metas.len()
    }
}

#[derive(Clone)]
struct OverlayDoc {
    meta: DocumentMeta,
    vector: Vec<u8>,
}

impl OverlayDoc {
    fn from_snapshot(document: SnapshotDocument, vector: Vec<u8>) -> Self {
        Self {
            meta: DocumentMeta {
                document_id: document.document_id,
                meeting_id: document.meeting_id.clone(),
                source_kind: document.source_kind,
                source_start_id: document.source_start_id,
                source_end_id: document.source_end_id,
                source_template_id: document.source_template_id,
                heading: document.heading,
                ordinal: document.ordinal,
            },
            vector,
        }
    }
}

/// Per-meeting replacement delta. An upserted meeting owns its whole document
/// set (its base rows are shadowed); deleted meetings are tombstoned
/// everywhere until compaction drops their rows entirely.
#[derive(Default, Clone)]
struct Overlay {
    upserted: BTreeMap<String, Vec<OverlayDoc>>,
    deleted: BTreeSet<String>,
}

impl Overlay {
    fn document_count(&self) -> usize {
        self.upserted.values().map(Vec::len).sum()
    }

    fn shadows_meeting(&self, meeting_id: &str) -> bool {
        self.deleted.contains(meeting_id) || self.upserted.contains_key(meeting_id)
    }
}

/// Immutable reader-visible state for one generation. Every construction path
/// produces a complete snapshot (full canonical load, or overlay copy sharing
/// the unchanged base); publication swaps the finished `Arc` atomically, so a
/// reader always holds either the old or the new complete state.
pub struct IndexSnapshot {
    generation_id: String,
    model_id: String,
    dimensions: usize,
    base: Arc<BaseRows>,
    base_meeting_document_counts: Arc<BTreeMap<String, usize>>,
    overlay: Overlay,
    overlay_documents: usize,
}

impl IndexSnapshot {
    fn new(
        generation_id: String,
        model_id: String,
        dimensions: usize,
        base: BaseRows,
        overlay: Overlay,
    ) -> Self {
        let overlay_documents = overlay.document_count();
        let mut base_meeting_document_counts = BTreeMap::new();
        for meta in &base.metas {
            *base_meeting_document_counts
                .entry(meta.meeting_id.clone())
                .or_insert(0) += 1;
        }
        Self {
            generation_id,
            model_id,
            dimensions,
            base: Arc::new(base),
            base_meeting_document_counts: Arc::new(base_meeting_document_counts),
            overlay,
            overlay_documents,
        }
    }

    /// Overlay-sharing copy used by journal replay: the base allocation is
    /// shared, never copied, for single-meeting updates.
    fn with_overlay(&self, overlay: Overlay) -> Self {
        let overlay_documents = overlay.document_count();
        Self {
            generation_id: self.generation_id.clone(),
            model_id: self.model_id.clone(),
            dimensions: self.dimensions,
            base: Arc::clone(&self.base),
            base_meeting_document_counts: Arc::clone(&self.base_meeting_document_counts),
            overlay,
            overlay_documents,
        }
    }

    fn tombstoned_base_documents(&self) -> usize {
        self.overlay
            .deleted
            .iter()
            .map(|meeting_id| {
                self.base_meeting_document_counts
                    .get(meeting_id)
                    .copied()
                    .unwrap_or(0)
            })
            .sum()
    }

    pub fn generation_id(&self) -> &str {
        &self.generation_id
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn document_count(&self) -> usize {
        self.base.len() + self.overlay_documents
    }

    /// Resident vector bytes of this snapshot (base + overlay payloads).
    pub fn resident_vector_bytes(&self) -> u64 {
        (self.base.vectors.len()
            + self
                .overlay
                .upserted
                .values()
                .map(|docs| docs.iter().map(|d| d.vector.len()).sum::<usize>())
                .sum::<usize>()) as u64
    }
}

// ---------------------------------------------------------------------------
// Search contract
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    pub document_id: String,
    pub meeting_id: String,
    pub source_kind: String,
    pub source_start_id: Option<String>,
    pub source_end_id: Option<String>,
    pub source_template_id: Option<String>,
    /// Section-heading provenance for hydration/reranking consumers.
    pub heading: Option<String>,
    pub ordinal: i64,
    /// Approximate cosine from symmetric int8 dot products; an internal
    /// diagnostic score, never exposed as a public BM25-style `rank`.
    pub score: f32,
}

/// Authoritative allow-list scope resolved by the caller from current SQLite
/// state. Narrow scopes filter during the scan AND again afterwards, so an
/// out-of-scope meeting can never enter fusion, hydration, sources, or
/// prompts.
#[derive(Debug, Clone)]
pub enum ScopeFilter {
    All,
    Meetings(BTreeSet<String>),
}

impl ScopeFilter {
    /// Builds a meeting allow-list; duplicate IDs collapse and membership is
    /// order-free.
    pub fn meetings<I: IntoIterator<Item = S>, S: Into<String>>(ids: I) -> Self {
        ScopeFilter::Meetings(ids.into_iter().map(Into::into).collect())
    }

    fn allows(&self, meeting_id: &str) -> bool {
        match self {
            ScopeFilter::All => true,
            ScopeFilter::Meetings(ids) => ids.contains(meeting_id),
        }
    }
}

/// Typed semantic unavailability. Callers fall back to FTS5 (lexical-only
/// availability) whenever search fails typed; nothing here is fatal.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchFailure {
    Cancelled,
    NoActiveGeneration,
    /// Canonical state is ahead of the published snapshot; bounded catch-up
    /// is running and semantic queries pause meanwhile.
    CatchUpPending {
        behind: i64,
    },
    ModelMismatch,
    InvalidQuery(&'static str),
    ScanFailed(String),
}

/// Heap candidate borrowing directly from the immutable snapshot, so no
/// index bookkeeping can ever dangle or mismatch.
struct Candidate<'a> {
    score: f32,
    meta: &'a DocumentMeta,
}

impl Ord for Candidate<'_> {
    /// Ascending by quality (min-heap order under `Reverse`): lowest score
    /// first; among equal scores the lexicographically LARGER document id is
    /// the worse candidate, so bounded eviction keeps the smaller id and the
    /// final score-desc/document-id-asc output is insertion-order independent.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.meta.document_id.cmp(&self.meta.document_id))
    }
}

impl PartialOrd for Candidate<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Candidate<'_> {}

impl PartialEq for Candidate<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

/// Final ordering: score descending (`total_cmp`; NaN impossible for finite
/// int8 dots), ties broken by document id ascending.
fn candidate_order(a: &Candidate<'_>, b: &Candidate<'_>) -> std::cmp::Ordering {
    b.score
        .total_cmp(&a.score)
        .then_with(|| a.meta.document_id.cmp(&b.meta.document_id))
}

fn push_candidate<'a>(
    heap: &mut std::collections::BinaryHeap<std::cmp::Reverse<Candidate<'a>>>,
    k: usize,
    candidate: Candidate<'a>,
) {
    if heap.len() < k {
        heap.push(std::cmp::Reverse(candidate));
    } else if let Some(min) = heap.peek().map(|entry| &entry.0) {
        if candidate.cmp(min) == std::cmp::Ordering::Greater {
            heap.pop();
            heap.push(std::cmp::Reverse(candidate));
        }
    }
}

/// Exact top-k over base + overlay. Runs on a blocking thread with shared
/// references only - no lock is ever held across the scan - and honors
/// cancellation in bounded chunks.
fn scan_snapshot(
    snapshot: &IndexSnapshot,
    query_i8: &[i8],
    scope: &ScopeFilter,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<VectorHit>, SearchFailure> {
    const CANCEL_CHECK_ROWS: usize = 4096;
    let mut heap =
        std::collections::BinaryHeap::<std::cmp::Reverse<Candidate<'_>>>::with_capacity(limit + 1);
    let dimensions = snapshot.dimensions;

    for (row, meta) in snapshot.base.metas.iter().enumerate() {
        if row % CANCEL_CHECK_ROWS == 0 && cancel.is_cancelled() {
            return Err(SearchFailure::Cancelled);
        }
        if snapshot.overlay.shadows_meeting(&meta.meeting_id) || !scope.allows(&meta.meeting_id) {
            continue;
        }
        let start = row * dimensions;
        push_candidate(
            &mut heap,
            limit,
            Candidate {
                score: dot_i8(&snapshot.base.vectors[start..start + dimensions], query_i8),
                meta,
            },
        );
    }

    for (meeting_id, docs) in snapshot.overlay.upserted.iter() {
        if cancel.is_cancelled() {
            return Err(SearchFailure::Cancelled);
        }
        if snapshot.overlay.deleted.contains(meeting_id) {
            continue;
        }
        for doc in docs.iter() {
            if !scope.allows(&doc.meta.meeting_id) {
                continue;
            }
            push_candidate(
                &mut heap,
                limit,
                Candidate {
                    score: dot_i8(&doc.vector, query_i8),
                    meta: &doc.meta,
                },
            );
        }
    }

    // Post-search authoritative scope re-filter (defense in depth for later
    // fusion/hydration consumers).
    let mut candidates: Vec<Candidate<'_>> = heap
        .into_iter()
        .map(|entry| entry.0)
        .filter(|candidate| scope.allows(candidate.meta.meeting_id.as_str()))
        .collect();
    candidates.sort_by(candidate_order);
    Ok(candidates
        .into_iter()
        .map(|candidate| VectorHit {
            document_id: candidate.meta.document_id.clone(),
            meeting_id: candidate.meta.meeting_id.clone(),
            source_kind: candidate.meta.source_kind.clone(),
            source_start_id: candidate.meta.source_start_id.clone(),
            source_end_id: candidate.meta.source_end_id.clone(),
            source_template_id: candidate.meta.source_template_id.clone(),
            heading: candidate.meta.heading.clone(),
            ordinal: candidate.meta.ordinal,
            score: candidate.score,
        })
        .collect())
}

/// Symmetric int8 cosine approximation under the approved storage contract:
/// both sides store `round(v * 127)` of unit vectors, so
/// `dot_i8(q, d) / 127^2` approximates cosine. Ranking ignores the constant
/// factor; it is applied so scores stay interpretable.
fn dot_i8(row: &[u8], query: &[i8]) -> f32 {
    let dot: i32 = row
        .iter()
        .zip(query)
        .map(|(a, b)| (*a as i8 as i32) * (*b as i32))
        .sum();
    dot as f32 * (APPROVED_INT8_DEQUANTIZATION_SCALE * APPROVED_INT8_DEQUANTIZATION_SCALE) as f32
}

/// Defensively normalizes the caller's query embedding and quantizes it under
/// the approved symmetric int8 contract before any scan.
fn prepare_query(query: &[f32]) -> Result<Vec<i8>, SearchFailure> {
    if query.is_empty() {
        return Err(SearchFailure::InvalidQuery("query embedding is empty"));
    }
    let mut norm_sq = 0.0_f64;
    for value in query {
        if !value.is_finite() {
            return Err(SearchFailure::InvalidQuery("query embedding is not finite"));
        }
        norm_sq += (*value as f64) * (*value as f64);
    }
    let norm = norm_sq.sqrt();
    if norm <= f64::EPSILON {
        return Err(SearchFailure::InvalidQuery("query embedding has zero norm"));
    }
    let normalized: Vec<f32> = query.iter().map(|value| value / norm as f32).collect();
    quantize_int8(&normalized)
        .map(|bytes| bytes.into_iter().map(|byte| byte as i8).collect())
        .map_err(|_| SearchFailure::InvalidQuery("query quantization failed"))
}

// ---------------------------------------------------------------------------
// Process-wide service
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ServiceState {
    active: Option<Arc<IndexSnapshot>>,
    lag: i64,
    epoch: u64,
    model_mismatch: bool,
    loaded_model_ids: BTreeSet<String>,
    published_bounds: BTreeMap<String, i64>,
    model_load_failure: Option<String>,
    pending_stale: BTreeSet<u64>,
    committed_stale: BTreeMap<u64, (String, i64)>,
    activation_transition: bool,
    pending_blockers: Vec<String>,
}

/// The one retrieval query-index service. Owned by the shared Task 2.4
/// lifecycle (Tauri and MCP receive the same instance through lifecycle
/// clones); mutations happen only inside the single worker task while reads
/// may arrive from any thread.
pub struct QueryIndexService {
    state: StdMutex<ServiceState>,
    scheduler: RetrievalScheduler,
    acknowledged_fast_hybrid_queries: AtomicU64,
    ram_probe: StdMutex<RamProbe>,
    created_at: DateTime<Utc>,
}

impl QueryIndexService {
    pub(crate) fn new(scheduler: RetrievalScheduler) -> Self {
        Self {
            state: StdMutex::new(ServiceState::default()),
            scheduler,
            acknowledged_fast_hybrid_queries: AtomicU64::new(0),
            ram_probe: StdMutex::new(Arc::new(measure_resident_ram) as RamProbe),
            created_at: Utc::now(),
        }
    }

    fn measured_ram(&self) -> Option<u64> {
        (self
            .ram_probe
            .lock()
            .unwrap_or_else(PoisonError::into_inner))()
    }

    /// The shared scheduler consumed by query-side vector scans (Task 2.4
    /// policy: at most two concurrent scans, cancellable while waiting).
    #[cfg(test)]
    pub(crate) fn scheduler(&self) -> &RetrievalScheduler {
        &self.scheduler
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, ServiceState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn active_snapshot(&self) -> Option<Arc<IndexSnapshot>> {
        self.lock_state().active.clone()
    }

    pub(crate) fn active_generation(&self) -> Option<String> {
        self.lock_state()
            .active
            .as_ref()
            .map(|snapshot| snapshot.generation_id.clone())
    }

    /// Installs a fully constructed snapshot as the queryable active state.
    fn install_active(&self, snapshot: Arc<IndexSnapshot>) {
        let mut state = self.lock_state();
        let generation_id = snapshot.generation_id().to_string();
        state.model_mismatch = !state.loaded_model_ids.contains(snapshot.model_id());
        state.active = Some(snapshot);
        state.lag = 0;
        state.activation_transition = false;
        state
            .committed_stale
            .retain(|_, (generation, _)| generation == &generation_id);
        state.epoch = state.epoch.wrapping_add(1);
    }

    fn replace_snapshot(&self, snapshot: Arc<IndexSnapshot>) {
        self.lock_state().active = Some(snapshot);
    }

    fn set_lag(&self, lag: i64) {
        self.lock_state().lag = lag.max(0);
    }

    pub(crate) fn mark_stale(&self) -> u64 {
        let mut state = self.lock_state();
        state.epoch = state.epoch.wrapping_add(1);
        state.lag = state.lag.max(1);
        let token = state.epoch;
        state.pending_stale.insert(token);
        token
    }

    pub(crate) fn restore_stale(&self, epoch: u64) {
        let mut state = self.lock_state();
        state.pending_stale.remove(&epoch);
        if state.pending_stale.is_empty()
            && state.committed_stale.is_empty()
            && !state.model_mismatch
        {
            state.lag = 0;
        }
    }

    pub(crate) fn commit_stale(
        &self,
        token: u64,
        generation_id: &str,
        change_id: i64,
        published_change_id: Option<i64>,
    ) {
        let mut state = self.lock_state();
        if state.pending_stale.remove(&token) {
            let published = published_change_id.map(|published_change_id| {
                let published = state
                    .published_bounds
                    .entry(generation_id.to_string())
                    .or_insert(published_change_id);
                *published = (*published).max(published_change_id);
                *published
            });
            if state
                .active
                .as_ref()
                .is_none_or(|active| active.generation_id() == generation_id)
                && published.is_none_or(|published| published < change_id)
            {
                state
                    .committed_stale
                    .insert(token, (generation_id.to_string(), change_id));
            }
            if state.pending_stale.is_empty()
                && state.committed_stale.is_empty()
                && !state.model_mismatch
            {
                state.lag = 0;
            }
        }
    }

    fn clear_published_stale(&self, generation_id: &str, published_change_id: i64) {
        let mut state = self.lock_state();
        let published = state
            .published_bounds
            .entry(generation_id.to_string())
            .or_insert(published_change_id);
        *published = (*published).max(published_change_id);
        state.committed_stale.retain(|_, (generation, bound)| {
            generation != generation_id || *bound > published_change_id
        });
        if state.pending_stale.is_empty() && !state.model_mismatch {
            state.lag = 0;
        }
    }

    fn clear_active_after_deactivation(&self, generation_id: &str) {
        let mut state = self.lock_state();
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.generation_id() == generation_id)
        {
            state.active = None;
            state.lag = 0;
            state.model_mismatch = false;
            state.epoch = state.epoch.wrapping_add(1);
        }
    }

    fn begin_activation_transition(&self) {
        self.lock_state().activation_transition = true;
    }

    fn cancel_activation_transition(&self) {
        self.lock_state().activation_transition = false;
    }

    fn semantic_unavailable_state(&self) -> (bool, bool) {
        let state = self.lock_state();
        (state.model_mismatch, state.activation_transition)
    }

    pub(crate) fn suppress_terminal_failure(&self, generation_id: &str, meeting_id: &str) {
        let mut state = self.lock_state();
        if let Some(snapshot) = state
            .active
            .as_ref()
            .filter(|snapshot| snapshot.generation_id() == generation_id)
        {
            let mut overlay = snapshot.overlay.clone();
            overlay.upserted.remove(meeting_id);
            overlay.deleted.insert(meeting_id.to_string());
            state.active = Some(Arc::new(snapshot.with_overlay(overlay)));
            state.epoch = state.epoch.wrapping_add(1);
        }
    }

    pub(crate) fn set_loaded_model(&self, model_id: &str) {
        let mut state = self.lock_state();
        state.loaded_model_ids.insert(model_id.to_string());
        state.model_mismatch = state
            .active
            .as_ref()
            .is_some_and(|snapshot| !state.loaded_model_ids.contains(snapshot.model_id()));
        state.model_load_failure = None;
    }

    fn has_loaded_model(&self, model_id: &str) -> bool {
        self.lock_state().loaded_model_ids.contains(model_id)
    }

    pub(crate) fn set_model_load_failure(&self, reason: String) {
        self.lock_state().model_load_failure = Some(reason);
    }

    fn model_load_failure(&self) -> Option<String> {
        self.lock_state().model_load_failure.clone()
    }

    pub fn publication_lag(&self) -> i64 {
        self.lock_state().lag
    }

    fn set_pending_blockers(&self, blockers: Vec<String>) {
        self.lock_state().pending_blockers = blockers;
    }

    pub fn pending_activation_blockers(&self) -> Vec<String> {
        self.lock_state().pending_blockers.clone()
    }

    pub fn resident_vector_bytes(&self) -> u64 {
        self.lock_state()
            .active
            .as_ref()
            .map(|snapshot| snapshot.resident_vector_bytes())
            .unwrap_or(0)
    }

    #[allow(dead_code)] // Invoked by the Sprint 3 Fast hybrid consumer.
    pub(crate) fn acknowledge_fast_hybrid_query(&self) {
        self.acknowledged_fast_hybrid_queries
            .fetch_add(1, Ordering::Relaxed);
    }

    fn acknowledged_fast_hybrid_queries(&self) -> u64 {
        self.acknowledged_fast_hybrid_queries
            .load(Ordering::Relaxed)
    }

    /// Injects a deterministic RAM measurement for gate tests.
    #[cfg(test)]
    pub(crate) fn set_ram_probe(&self, probe: RamProbe) {
        *self
            .ram_probe
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = probe;
    }

    /// Process-start proxy for garbage-collection eligibility: anything
    /// retired after this instant was retired inside the current process and
    /// has not yet survived a restart.
    ///
    /// ponytail: lifecycle construction approximates process start; exact
    /// boot timestamps would need plumbing from Tauri setup for no observed
    /// benefit.
    fn process_start(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Deterministic normalized-vector nearest-neighbor search over the
    /// active snapshot. Cancellable at every boundary; scoped hits carry the
    /// metadata later hydration/reranking needs.
    pub async fn search(
        &self,
        query: &[f32],
        scope: ScopeFilter,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<VectorHit>, SearchFailure> {
        if cancel.is_cancelled() {
            return Err(SearchFailure::Cancelled);
        }
        let (snapshot, lag, epoch, model_mismatch, activation_transition, stale) = {
            let state = self.lock_state();
            let Some(snapshot) = state.active.clone() else {
                return Err(SearchFailure::NoActiveGeneration);
            };
            (
                snapshot,
                state.lag,
                state.epoch,
                state.model_mismatch,
                state.activation_transition,
                !state.pending_stale.is_empty() || !state.committed_stale.is_empty(),
            )
        };
        if model_mismatch {
            return Err(SearchFailure::ModelMismatch);
        }
        if activation_transition {
            return Err(SearchFailure::CatchUpPending { behind: lag.max(1) });
        }
        if stale || lag > 0 {
            return Err(SearchFailure::CatchUpPending { behind: lag });
        }
        let query_i8 = prepare_query(query)?;
        // Shape must match the index exactly: `zip` in the dot product would
        // otherwise silently truncate short or long queries.
        if query_i8.len() != snapshot.dimensions {
            return Err(SearchFailure::InvalidQuery(
                "query embedding dimension does not match the index",
            ));
        }
        let Some(scan_permit) = self.scheduler.acquire_vector_scan(cancel).await else {
            return Err(SearchFailure::Cancelled);
        };
        let scan_cancel = cancel.clone();
        let scanned = tokio::task::spawn_blocking(move || {
            scan_snapshot(
                &snapshot,
                &query_i8,
                &scope,
                limit.min(MAX_QUERY_LIMIT),
                &scan_cancel,
            )
        })
        .await
        .map_err(|error| SearchFailure::ScanFailed(format!("scan task join failed: {error}")))?;
        drop(scan_permit);
        let hits = scanned?;
        if cancel.is_cancelled() {
            return Err(SearchFailure::Cancelled);
        }
        let state = self.lock_state();
        if state.epoch != epoch || state.model_mismatch || state.activation_transition {
            return Err(if state.model_mismatch {
                SearchFailure::ModelMismatch
            } else {
                SearchFailure::CatchUpPending {
                    behind: state.lag.max(1),
                }
            });
        }
        if !state.pending_stale.is_empty() || !state.committed_stale.is_empty() {
            return Err(SearchFailure::CatchUpPending {
                behind: state.lag.max(1),
            });
        }
        Ok(hits)
    }
}

// ---------------------------------------------------------------------------
// Publisher: journal replay, activation, garbage collection
// ---------------------------------------------------------------------------

/// One publisher pass inside the worker loop. Errors are returned to the
/// caller (which logs them); every step leaves prior durable and in-memory
/// state intact on failure. Cancellation stops the pass at every bounded
/// DB/page/batch boundary and before any acknowledgement or swap, leaving the
/// durable bounds and the prior reader snapshot valid.
#[cfg(test)]
pub(crate) async fn publish_tick(
    pool: &SqlitePool,
    service: &QueryIndexService,
) -> Result<(), String> {
    publish_tick_with(pool, service, &CancellationToken::new()).await
}

pub(crate) async fn publish_tick_with(
    pool: &SqlitePool,
    service: &QueryIndexService,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let result = publish_tick_inner(pool, service, cancel).await;
    // Refreshed even after failures so an aborted or erroring pass can never
    // leave queries believing catch-up completed.
    update_publication_lag(pool, service).await;
    result
}

async fn publish_tick_inner(
    pool: &SqlitePool,
    service: &QueryIndexService,
    cancel: &CancellationToken,
) -> Result<(), String> {
    if cancel.is_cancelled() {
        return Ok(());
    }

    drain_retired_journals_and_gc(pool, service, cancel).await;

    let pointer = RetrievalRepository::active_generation_id(pool)
        .await
        .map_err(|error| format!("reading active generation failed: {error}"))?;

    if let Some(generation_id) = &pointer {
        if service.active_generation().as_deref() != Some(generation_id.as_str()) {
            catch_up_active_generation(pool, service, generation_id, cancel).await?;
        } else if !cancel.is_cancelled() {
            replay_steady_state(pool, service, cancel).await?;
            compact_if_needed(service, cancel).await;
        }
    }

    if cancel.is_cancelled() {
        return Ok(());
    }
    try_activate_shadow_generation(pool, service, cancel).await?;
    Ok(())
}

/// Startup/attach path: full canonical load plus bounded journal catch-up
/// from one consistent database snapshot, then the atomic in-memory install,
/// and only afterwards the durable acknowledgement of the installed bound.
/// Quarantined rows mean known-corrupt active retrieval: deactivate to
/// FTS-only and requeue the affected meetings instead of serving holes.
async fn catch_up_active_generation(
    pool: &SqlitePool,
    service: &QueryIndexService,
    generation_id: &str,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let Some(read) = RetrievalRepository::read_canonical_snapshot(pool, generation_id, cancel)
        .await
        .map_err(|error| format!("canonical snapshot load failed: {error}"))?
    else {
        // Cancelled between pages: nothing was fully loaded, so nothing may
        // be installed or acknowledged from this pass.
        return Ok(());
    };
    if !read.page.rejected.is_empty() {
        let quarantined: Vec<String> = read
            .page
            .rejected
            .iter()
            .map(|rejected| rejected.meeting_id.clone())
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        requeue_quarantined(pool, generation_id, &quarantined).await;
        log::warn!(
            "Active generation {generation_id} has {} malformed derived document(s); \
             deactivating semantic retrieval to FTS-only",
            quarantined.len()
        );
        let deactivated = RetrievalRepository::deactivate_active_generation(pool)
            .await
            .map_err(|error| format!("deactivating corrupt active generation failed: {error}"))?;
        if let Some(deactivated) = deactivated {
            service.clear_active_after_deactivation(&deactivated);
        }
        return Ok(());
    }

    // Install the fully built immutable snapshot BEFORE acknowledging its
    // bound, with a cancellation recheck at each boundary: a durable
    // acknowledgement must never advance ahead of reader state.
    publish_catch_up_snapshot(pool, service, generation_id, read, cancel).await?;
    replay_steady_state(pool, service, cancel).await
}

/// Installs the fully built immutable snapshot BEFORE acknowledging its
/// bound: a durable acknowledgement must never advance ahead of reader
/// state. Because rows and bound came from one consistent read snapshot,
/// the acknowledged bound exactly describes what readers now hold.
/// Cancellation at either boundary installs or acknowledges nothing from
/// this pass; the next pass repeats the catch-up and converges normally.
async fn publish_catch_up_snapshot(
    pool: &SqlitePool,
    service: &QueryIndexService,
    generation_id: &str,
    read: CanonicalSnapshotRead,
    cancel: &CancellationToken,
) -> Result<(), String> {
    if cancel.is_cancelled() {
        // Cancelled after the canonical read completed but before
        // publication: nothing may be installed or acknowledged from this
        // pass, so the prior reader state and durable bounds stay untouched.
        return Ok(());
    }
    let bound = read.canonical_change_id;
    service.install_active(Arc::new(base_snapshot(generation_id, read)));
    if cancel.is_cancelled() {
        // Installed ahead of its durable bound; idempotent journal replay
        // resynchronizes the acknowledgement on a later pass.
        return Ok(());
    }
    if let Err(error) = RetrievalRepository::acknowledge_journal(pool, generation_id, bound).await {
        // Safe to retry later: replay restarts from the durable bound and
        // re-applying already-installed changes is idempotent (upserts fold
        // per meeting over their own rows; tombstones remove absent or
        // identical rows), so nothing is duplicated or lost by the gap.
        log::warn!(
            "Acknowledging loaded bound failed for generation {generation_id}; \
             journal replay will resynchronize: {error}"
        );
    } else {
        service.clear_published_stale(generation_id, bound);
    }
    // Surface the true lag immediately so queries pause while the journal
    // beyond the installed bound replays.
    if let Ok(Some((canonical, published))) =
        RetrievalRepository::publication_lag(pool, generation_id).await
    {
        service.set_lag(canonical - published);
    }
    log::info!("Published semantic snapshot for generation {generation_id}");
    Ok(())
}

async fn replay_steady_state(
    pool: &SqlitePool,
    service: &QueryIndexService,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let mut current: Arc<IndexSnapshot> = match service.active_snapshot() {
        Some(snapshot) => snapshot,
        None => return Ok(()),
    };
    for _ in 0..REPLAY_BATCHES_PER_TICK {
        if cancel.is_cancelled() {
            // Durable bounds untouched; the prior reader snapshot stays valid.
            return Ok(());
        }
        let Some((next, last_change_id)) = apply_journal_batch(pool, &current, cancel).await?
        else {
            break;
        };
        let next = Arc::new(next);
        if !publish_replayed_batch(pool, service, &current, &next, last_change_id, cancel).await? {
            // Cancelled after the batch was fully built but before it could
            // be published: the candidate is dropped whole.
            return Ok(());
        }
        current = next;
    }
    Ok(())
}

/// Publishes one fully built replay batch to readers FIRST and acknowledges
/// durably only afterwards: a failure in between replays the same batch next
/// tick onto a snapshot that already contains it (the fold is idempotent).
/// Returns false when cancellation fired at either boundary, leaving the
/// swap or the acknowledgement for that batch unperformed.
async fn publish_replayed_batch(
    pool: &SqlitePool,
    service: &QueryIndexService,
    current: &Arc<IndexSnapshot>,
    next: &Arc<IndexSnapshot>,
    last_change_id: i64,
    cancel: &CancellationToken,
) -> Result<bool, String> {
    if cancel.is_cancelled() {
        return Ok(false);
    }
    service.replace_snapshot(Arc::clone(next));
    if cancel.is_cancelled() {
        // Installed ahead of the durable bound; replay resynchronizes the
        // acknowledgement next pass.
        return Ok(false);
    }
    RetrievalRepository::acknowledge_journal(pool, current.generation_id(), last_change_id)
        .await
        .map_err(|error| format!("acknowledging journal failed: {error}"))?;
    service.clear_published_stale(current.generation_id(), last_change_id);
    Ok(true)
}

/// Applies up to [`JOURNAL_REPLAY_BATCH`] canonical changes after the
/// DURABLE published bound onto `base`, returning the new snapshot together
/// with the highest applied change ID; the caller publishes the snapshot and
/// only then acknowledges that ID.
///
/// Returns `None` when the generation is fully caught up OR when cancellation
/// fired mid-build (durable bounds stay untouched either way); a reload
/// failure surfaces as `Err` and the caller simply retries next tick with
/// prior state intact.
async fn apply_journal_batch(
    pool: &SqlitePool,
    base: &IndexSnapshot,
    cancel: &CancellationToken,
) -> Result<Option<(IndexSnapshot, i64)>, String> {
    let generation_id = base.generation_id().to_string();
    let Some((canonical, published)) = RetrievalRepository::publication_lag(pool, &generation_id)
        .await
        .map_err(|error| format!("reading publication lag failed: {error}"))?
    else {
        return Ok(None);
    };
    if canonical <= published {
        return Ok(None);
    }
    apply_journal_changes(pool, base, published, cancel).await
}

/// Core journal application over an explicit lower bound. Operations fold
/// last-writer-wins per meeting, so sparse IDs, repeated upserts,
/// upsert/delete ordering, AND re-application of already-installed changes
/// all converge: an upsert reloads the meeting's current canonical rows which
/// shadow their identical base/overlay copies, and a delete removes rows that
/// are absent or identical.
///
/// Cancellation between individual meeting reloads abandons the candidate and
/// returns `None`; callers observe the same token before swapping or
/// acknowledging anything built from it.
async fn apply_journal_changes(
    pool: &SqlitePool,
    base: &IndexSnapshot,
    after_change_id: i64,
    cancel: &CancellationToken,
) -> Result<Option<(IndexSnapshot, i64)>, String> {
    let generation_id = base.generation_id().to_string();
    let changes = RetrievalRepository::read_journal_since(
        pool,
        &generation_id,
        after_change_id,
        JOURNAL_REPLAY_BATCH,
    )
    .await
    .map_err(|error| format!("reading journal failed: {error}"))?;
    if changes.is_empty() {
        return Ok(None);
    }

    let mut folded: BTreeMap<String, bool> = BTreeMap::new();
    for change in &changes {
        folded.insert(change.meeting_id.clone(), change.operation == "upsert");
    }

    let mut overlay = base.overlay.clone();
    for (meeting_id, is_upsert) in &folded {
        if cancel.is_cancelled() {
            // Candidate abandoned mid-build; nothing built from it may be
            // swapped or acknowledged this pass.
            return Ok(None);
        }
        if *is_upsert {
            let page = RetrievalRepository::read_snapshot_documents(
                pool,
                &generation_id,
                Some(meeting_id),
            )
            .await
            .map_err(|error| format!("reloading documents for one meeting failed: {error}"))?;
            if !page.rejected.is_empty() {
                // A partially unreadable canonical meeting must never enter
                // the delta as a subset: requeue it and defer this batch
                // without acknowledging, so publication lag stays visible
                // and semantic queries pause until the worker rebuilds.
                requeue_quarantined(
                    pool,
                    &generation_id,
                    &page
                        .rejected
                        .iter()
                        .map(|rejected| rejected.meeting_id.clone())
                        .collect::<BTreeSet<String>>()
                        .into_iter()
                        .collect::<Vec<_>>(),
                )
                .await;
                log::warn!(
                    "Delta reload for generation {generation_id} hit malformed documents; publication deferred"
                );
                return Ok(None);
            }
            overlay.upserted.insert(
                meeting_id.clone(),
                page.documents
                    .into_iter()
                    .zip(page.vectors.chunks_exact(base.dimensions))
                    .map(|(document, vector)| OverlayDoc::from_snapshot(document, vector.to_vec()))
                    .collect(),
            );
            overlay.deleted.remove(meeting_id);
        } else {
            overlay.upserted.remove(meeting_id);
            overlay.deleted.insert(meeting_id.clone());
        }
    }

    let last_change_id = changes[changes.len() - 1].change_id;
    Ok(Some((base.with_overlay(overlay), last_change_id)))
}

/// Rebuilds the base from base-minus-tombstones-plus-overlay once the overlay
/// crosses the approved 2% threshold. Tombstoned meetings count alongside
/// overlay documents, so deletion-only churn also compacts and deleted base
/// vectors cannot be retained indefinitely. Runs on a blocking thread with no
/// database transaction held; readers keep querying the old snapshot until
/// the compacted one swaps in (the accounted two-snapshot overlap moment),
/// the blocking loops observe cancellation at a bounded cadence, and a
/// cancelled compaction keeps the prior snapshot and overlay installed for a
/// later retry.
async fn compact_if_needed(service: &QueryIndexService, cancel: &CancellationToken) {
    let Some(snapshot) = service.active_snapshot() else {
        return;
    };
    let overlay_units = snapshot.overlay_documents + snapshot.tombstoned_base_documents();
    if overlay_units == 0 || overlay_units * COMPACTION_DENOMINATOR < snapshot.base.len().max(1) {
        return;
    }
    if cancel.is_cancelled() {
        return;
    }
    let compact_cancel = cancel.clone();
    let compacted =
        match tokio::task::spawn_blocking(move || compact_snapshot(&snapshot, &compact_cancel))
            .await
        {
            Ok(Some(compacted)) => Arc::new(compacted),
            Ok(None) => return,
            Err(error) => {
                log::warn!("Compaction task join failed; keeping overlay: {error}");
                return;
            }
        };
    if cancel.is_cancelled() {
        return;
    }
    service.replace_snapshot(compacted);
}

/// Compacts synchronously on a blocking thread, observing cancellation at a
/// bounded cadence in both the base and the overlay loop; a cancelled pass
/// returns `None` so no partially built result can ever be swapped in.
fn compact_snapshot(snapshot: &IndexSnapshot, cancel: &CancellationToken) -> Option<IndexSnapshot> {
    const CANCEL_CHECK_ROWS: usize = 4096;
    let dimensions = snapshot.dimensions;
    let mut metas = Vec::with_capacity(snapshot.document_count());
    let mut vectors = Vec::with_capacity(snapshot.document_count() * dimensions);
    for (row, meta) in snapshot.base.metas.iter().enumerate() {
        if row % CANCEL_CHECK_ROWS == 0 && cancel.is_cancelled() {
            return None;
        }
        if snapshot.overlay.shadows_meeting(&meta.meeting_id) {
            continue;
        }
        let start = row * dimensions;
        metas.push(meta.clone());
        vectors.extend_from_slice(&snapshot.base.vectors[start..start + dimensions]);
    }
    for docs in snapshot.overlay.upserted.values() {
        if cancel.is_cancelled() {
            return None;
        }
        for doc in docs {
            metas.push(doc.meta.clone());
            vectors.extend_from_slice(&doc.vector);
        }
    }
    Some(IndexSnapshot::new(
        snapshot.generation_id.clone(),
        snapshot.model_id.clone(),
        dimensions,
        BaseRows { metas, vectors },
        Overlay::default(),
    ))
}

/// Builds a fresh immutable base snapshot from one consistent canonical
/// read. Malformed rows never reach this point: they are quarantined by the
/// caller from the same read's rejection list.
fn base_snapshot(generation_id: &str, read: CanonicalSnapshotRead) -> IndexSnapshot {
    let mut metas = Vec::with_capacity(read.page.documents.len());
    for document in read.page.documents {
        metas.push(DocumentMeta {
            document_id: document.document_id,
            meeting_id: document.meeting_id,
            source_kind: document.source_kind,
            source_start_id: document.source_start_id,
            source_end_id: document.source_end_id,
            source_template_id: document.source_template_id,
            heading: document.heading,
            ordinal: document.ordinal,
        });
    }
    IndexSnapshot::new(
        generation_id.to_string(),
        read.model_id,
        read.dimensions,
        BaseRows {
            metas,
            vectors: read.page.vectors,
        },
        Overlay::default(),
    )
}

async fn requeue_quarantined(pool: &SqlitePool, generation_id: &str, meetings: &[String]) {
    let unique: BTreeSet<&str> = meetings.iter().map(String::as_str).collect();
    for meeting in unique {
        if let Err(error) = RetrievalRepository::requeue_meeting_work(
            pool,
            generation_id,
            meeting,
            "malformed derived vector quarantined during snapshot load",
        )
        .await
        {
            log::warn!("Requeueing quarantined meeting failed: {error}");
        }
    }
}
async fn generation_identity(
    pool: &SqlitePool,
    generation_id: &str,
) -> Result<(usize, String), String> {
    sqlx::query_as::<_, (i64, String)>(
        "SELECT m.dimensions, m.model_id
         FROM retrieval_generations g JOIN retrieval_models m ON m.model_id = g.model_id
         WHERE g.generation_id = ?",
    )
    .bind(generation_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?
    .map(|(dimensions, model_id)| (dimensions.max(0) as usize, model_id))
    .ok_or_else(|| format!("generation '{generation_id}' has no model"))
}

// ---------------------------------------------------------------------------
// Activation gates
// ---------------------------------------------------------------------------

fn coverage_blockers(status: &GenerationStatus) -> Vec<String> {
    let mut blockers = Vec::new();
    if status.tracked_meetings == 0 {
        blockers.push("no meetings indexed yet".to_string());
    }
    if status.current_meetings < status.tracked_meetings {
        blockers.push(format!(
            "coverage incomplete: {}/{} meetings at current revision",
            status.current_meetings, status.tracked_meetings
        ));
    }
    if status.failed_meetings > 0 {
        blockers.push(format!(
            "{} permanent item failure(s)",
            status.failed_meetings
        ));
    }
    blockers
}

/// Activation disk gate: usage at or below the approved 3 GiB shadow-rebuild
/// peak is admissible; anything above blocks activation (and is reported,
/// never auto-deleted).
fn disk_envelope_blocker(usage_bytes: u64) -> Option<String> {
    (usage_bytes > DERIVED_DISK_ACTIVATION_LIMIT_BYTES).then(|| {
        format!(
            "derived disk usage {usage_bytes} bytes exceeds the {DERIVED_DISK_ACTIVATION_LIMIT_BYTES} byte activation limit"
        )
    })
}

/// Conservative envelope usage feeding the activation disk gate: the SQLite
/// backing-store upper bound (allocated pages plus uncheckpointed WAL frames,
/// which counts primary storage and therefore can overcount but never
/// undercount), building shadows' persisted vector bytes, and the resident
/// active snapshot.
async fn envelope_usage_bytes(
    pool: &SqlitePool,
    service: &QueryIndexService,
) -> Result<u64, String> {
    let backing_store = RetrievalRepository::derived_backing_store_upper_bound_bytes(pool)
        .await
        .map_err(|error| format!("measuring derived disk failed: {error}"))?;
    let shadow = RetrievalRepository::estimated_shadow_snapshot_bytes(pool)
        .await
        .map_err(|error| format!("estimating shadow snapshot size failed: {error}"))?;
    Ok(backing_store + shadow + service.resident_vector_bytes())
}

/// Promotes completed shadow generations (initial build, manual rebuild, or
/// model upgrade) through the singleton pointer. The prior active generation
/// stays resident and queryable until the pointer transaction succeeds; the
/// in-memory swap follows atomically, and only then is the installed bound
/// acknowledged durably (a failed acknowledgement heals through steady-state
/// replay).
async fn try_activate_shadow_generation(
    pool: &SqlitePool,
    service: &QueryIndexService,
    cancel: &CancellationToken,
) -> Result<(), String> {
    if cancel.is_cancelled() {
        return Ok(());
    }
    let pointer = RetrievalRepository::active_generation_id(pool)
        .await
        .map_err(|error| format!("reading active generation failed: {error}"))?;
    let candidates = RetrievalRepository::list_live_generations(pool)
        .await
        .map_err(|error| format!("listing live generations failed: {error}"))?;

    let mut reported_blockers = Vec::new();
    for (generation_id, _model_id) in candidates {
        if cancel.is_cancelled() {
            return Ok(());
        }
        if pointer.as_deref() == Some(generation_id.as_str()) {
            continue;
        }
        if !service.has_loaded_model(&_model_id) {
            reported_blockers.push(format!(
                "generation {generation_id} awaits its matching approved model runtime"
            ));
            continue;
        }
        let Some(mut status) = RetrievalRepository::generation_status(pool, &generation_id)
            .await
            .map_err(|error| format!("reading generation status failed: {error}"))?
        else {
            continue;
        };

        // Cheap gates first; the expensive snapshot load runs only for a
        // generation that already looks complete. Journal acknowledgement is
        // deliberately excluded here: shadows catch up during validation and
        // acknowledge only after installation.
        let usage = envelope_usage_bytes(pool, service).await?;
        let mut blockers = coverage_blockers(&status);
        if let Some(disk_blocker) = disk_envelope_blocker(usage) {
            blockers.push(disk_blocker);
        }
        if !blockers.is_empty() {
            reported_blockers.extend(blockers);
            continue;
        }
        if cancel.is_cancelled() {
            return Ok(());
        }

        // Expensive validation: consistent full load plus journal catch-up.
        // Quarantine blocks activation (never activates a hole-y snapshot)
        // and heals via requeued work.
        let Some(read) = RetrievalRepository::read_canonical_snapshot(pool, &generation_id, cancel)
            .await
            .map_err(|error| format!("canonical snapshot load failed: {error}"))?
        else {
            return Ok(());
        };
        if !read.page.rejected.is_empty() {
            let quarantined = read.page.rejected.len();
            requeue_quarantined(
                pool,
                &generation_id,
                &read
                    .page
                    .rejected
                    .iter()
                    .map(|rejected| rejected.meeting_id.clone())
                    .collect::<BTreeSet<String>>()
                    .into_iter()
                    .collect::<Vec<_>>(),
            )
            .await;
            reported_blockers.push(format!(
                "generation {generation_id} snapshot validation failed ({quarantined} malformed document(s))"
            ));
            continue;
        }
        let mut caught_up_to = read.canonical_change_id;
        let mut current = base_snapshot(&generation_id, read);
        for _ in 0..REPLAY_BATCHES_PER_TICK {
            if cancel.is_cancelled() {
                return Ok(());
            }
            // Replay only what was committed beyond the consistently loaded
            // bound, so already-loaded changes are never re-applied into the
            // overlay before activation.
            match apply_journal_changes(pool, &current, caught_up_to, cancel).await? {
                Some((next, change_id)) => {
                    current = next;
                    caught_up_to = change_id;
                }
                None => break,
            }
        }

        // Measured RAM gate at the actual activation peak: the caught-up
        // shadow snapshot is resident here alongside the still-installed
        // active snapshot, immediately before pointer switch and memory swap.
        if let Some(blocker) = ram_gate_blocker(service.measured_ram()) {
            reported_blockers.push(format!("generation {generation_id}: {blocker}"));
            continue;
        }
        if cancel.is_cancelled() {
            return Ok(());
        }

        // Re-check coverage after load/catch-up; new work may have appeared.
        // The publication gate compares current canonical state against the
        // CAUGHT-UP LOCAL bound - durable acknowledgement happens only after
        // activation below, so it cannot run ahead of any reader snapshot.
        status = RetrievalRepository::generation_status(pool, &generation_id)
            .await
            .map_err(|error| format!("reading generation status failed: {error}"))?
            .ok_or_else(|| format!("generation '{generation_id}' vanished during activation"))?;
        let mut remaining = coverage_blockers(&status);
        match RetrievalRepository::publication_lag(pool, &generation_id).await {
            Ok(Some((canonical_now, _))) if canonical_now > caught_up_to => {
                remaining.push("publication journal not fully caught up".to_string());
            }
            Ok(_) => {}
            Err(error) => return Err(format!("reading publication lag failed: {error}")),
        }
        if !remaining.is_empty() {
            reported_blockers.extend(remaining);
            continue;
        }

        if !promote_shadow_generation(pool, service, &generation_id, current, caught_up_to, cancel)
            .await?
        {
            // Cancelled after validation but before promotion: nothing was
            // promoted, installed, or acknowledged from this candidate.
            return Ok(());
        }
        log::info!(
            "Activated semantic generation {} (model {}) with {} documents",
            generation_id,
            status.model_id,
            status.document_count
        );
        reported_blockers.clear();
        break;
    }
    service.set_pending_blockers(reported_blockers);
    Ok(())
}

/// Promotes one fully validated shadow candidate: readiness, singleton
/// pointer, memory install, and only then the durable acknowledgement of the
/// installed bound (a failed acknowledgement heals through steady-state
/// replay). Cancellation immediately before promotion discards the validated
/// candidate without any state change; cancellation after installation leaves
/// the bound for replay to resynchronize.
async fn promote_shadow_generation(
    pool: &SqlitePool,
    service: &QueryIndexService,
    generation_id: &str,
    snapshot: IndexSnapshot,
    caught_up_to: i64,
    cancel: &CancellationToken,
) -> Result<bool, String> {
    if cancel.is_cancelled() {
        // Validated candidate discarded before promotion: no readiness flip,
        // no pointer move, no memory swap, no acknowledgement.
        return Ok(false);
    }
    service.begin_activation_transition();
    if !RetrievalRepository::activate_generation_if_ready(pool, generation_id, caught_up_to)
        .await
        .map_err(|error| {
            service.cancel_activation_transition();
            format!("activating generation failed: {error}")
        })?
    {
        service.cancel_activation_transition();
        return Ok(false);
    }
    service.install_active(Arc::new(snapshot));
    if !cancel.is_cancelled() {
        if let Err(error) =
            RetrievalRepository::acknowledge_journal(pool, generation_id, caught_up_to).await
        {
            log::warn!(
                "Acknowledging activated bound failed for generation {generation_id}; \
              steady-state replay will resynchronize: {error}"
            );
        } else {
            service.clear_published_stale(generation_id, caught_up_to);
        }
    }
    Ok(true)
}

async fn update_publication_lag(pool: &SqlitePool, service: &QueryIndexService) {
    let generation_id = match RetrievalRepository::active_generation_id(pool).await {
        Ok(Some(generation_id)) => generation_id,
        _ => {
            service.set_lag(1);
            return;
        }
    };
    let lag = match RetrievalRepository::publication_lag(pool, &generation_id).await {
        Ok(Some((canonical, published))) => (canonical - published).max(0),
        _ => 0,
    };
    service.set_lag(lag);
}

/// Terminal generations stop being served, so their journal rows exist only
/// for historical publishers; draining them to canonical records exactly
/// that fact before cleanup, letting [`RetrievalRepository::delete_generation`]
/// enforce its own unacknowledged-changes guard honestly. Deletion itself
/// additionally waits for one clean restart plus one successful query.
async fn drain_retired_journals_and_gc(
    pool: &SqlitePool,
    service: &QueryIndexService,
    cancel: &CancellationToken,
) {
    let retired = match RetrievalRepository::retired_generations(pool).await {
        Ok(retired) => retired,
        Err(error) => {
            log::warn!("Listing retired generations failed: {error}");
            return;
        }
    };
    for (generation_id, retired_at) in retired {
        if cancel.is_cancelled() {
            return;
        }
        if let Ok(Some((canonical, published))) =
            RetrievalRepository::publication_lag(pool, &generation_id).await
        {
            if canonical > published && !cancel.is_cancelled() {
                if let Err(error) =
                    RetrievalRepository::acknowledge_journal(pool, &generation_id, canonical).await
                {
                    log::warn!("Draining retired journal failed for {generation_id}: {error}");
                }
            }
        }
        let survived_restart = retired_at
            .parse::<DateTime<Utc>>()
            .map(|retired| retired < service.process_start())
            .unwrap_or(false);
        if survived_restart
            && service.acknowledged_fast_hybrid_queries() > 0
            && !cancel.is_cancelled()
        {
            match RetrievalRepository::delete_generation(pool, &generation_id).await {
                Ok(true) => log::info!("Garbage-collected retired generation {generation_id}"),
                Ok(false) => {}
                Err(error) => log::debug!("GC refused generation {generation_id}: {error}"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rebuild requests and status reporting
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum RebuildRequestError {
    /// At most two complete generations are retained; a third is refused.
    RetentionLimit,
    NoModelAvailable,
    Database(sqlx::Error),
}

impl std::fmt::Display for RebuildRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RebuildRequestError::RetentionLimit => write!(
                formatter,
                "two generations are already retained; garbage collection must reclaim one first"
            ),
            RebuildRequestError::NoModelAvailable => {
                write!(formatter, "no registered model is available to rebuild")
            }
            RebuildRequestError::Database(error) => write!(formatter, "{error}"),
        }
    }
}

/// Registers a distinct shadow generation for the active model (manual
/// rebuild). Healthy active retrieval continues while the worker indexes the
/// shadow; cancellation removes only the shadow's derived state.
pub async fn request_rebuild(pool: &SqlitePool) -> Result<String, RebuildRequestError> {
    let model_id = match RetrievalRepository::active_generation_id(pool).await {
        Ok(Some(generation_id)) => {
            generation_identity(pool, &generation_id)
                .await
                .map_err(|_| RebuildRequestError::NoModelAvailable)?
                .1
        }
        _ => {
            let models: Vec<(String,)> =
                sqlx::query_as("SELECT model_id FROM retrieval_models ORDER BY created_at LIMIT 2")
                    .fetch_all(pool)
                    .await
                    .map_err(RebuildRequestError::Database)?;
            match models.len() {
                1 => models[0].0.clone(),
                _ => return Err(RebuildRequestError::NoModelAvailable),
            }
        }
    };
    let generation_id = format!("gen-rebuild-{}", Utc::now().timestamp_millis());
    RetrievalRepository::register_generation(pool, &generation_id, &model_id)
        .await
        .map_err(|error| match &error {
            sqlx::Error::Protocol(message) if message == "generation retention limit reached" => {
                RebuildRequestError::RetentionLimit
            }
            _ => RebuildRequestError::Database(error),
        })?;
    log::info!("Registered shadow rebuild generation {generation_id} for model {model_id}");
    Ok(generation_id)
}

/// Measured/derived status data backing the later Settings UI (no UI here).
#[derive(Debug, Serialize)]
pub struct RetrievalStatusReport {
    pub backend: &'static str,
    /// `paused`, `unavailable`, `building`, `loading`, `catching_up`, `ready`.
    pub semantic_state: String,
    pub model_load_failure: Option<String>,
    pub paused: bool,
    pub active_generation_id: Option<String>,
    pub active_model_id: Option<String>,
    pub building_generations: Vec<GenerationStatus>,
    pub document_count: i64,
    pub tracked_meetings: i64,
    pub current_meetings: i64,
    pub retry_meetings: i64,
    pub failed_meetings: i64,
    pub canonical_change_id: Option<i64>,
    pub published_change_id: Option<i64>,
    pub activation_blockers: Vec<String>,
    pub resident_index_bytes: u64,
    /// Measured process resident physical memory at status time (`None` when
    /// the platform facility is unavailable).
    pub resident_process_bytes: Option<u64>,
    /// Approved transient activation ceiling (1.30 GiB); activation blocks at
    /// or above it.
    pub activation_ram_ceiling_bytes: u64,
    /// Payload-row ESTIMATE of derived bytes (excludes indexes, journal/state
    /// rows, page fragmentation/free pages, and WAL). Always labeled by
    /// [`Self::derived_disk_is_estimate`]; never presented as exact derived
    /// bytes and never used as the activation gate.
    pub derived_disk_bytes: u64,
    /// True while [`Self::derived_disk_bytes`] is an estimate rather than a
    /// measured exact value.
    pub derived_disk_is_estimate: bool,
    /// Conservative SQLite backing-store upper bound (allocated pages plus
    /// uncheckpointed WAL frames). It counts primary storage too, so it can
    /// overcount - safe for the block-only gate - but cannot undercount
    /// material data.
    pub derived_disk_backing_store_upper_bound_bytes: u64,
    pub derived_disk_steady_target_bytes: u64,
    pub derived_disk_activation_limit_bytes: u64,
}

pub async fn index_status(
    pool: &SqlitePool,
    service: &QueryIndexService,
    paused: bool,
) -> Result<RetrievalStatusReport, String> {
    let pointer = RetrievalRepository::active_generation_id(pool)
        .await
        .map_err(|error| error.to_string())?;
    let status = match &pointer {
        Some(generation_id) => RetrievalRepository::generation_status(pool, generation_id)
            .await
            .map_err(|error| error.to_string())?,
        None => None,
    };
    let building = RetrievalRepository::list_live_generations(pool)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .any(|(generation_id, _model_id)| pointer.as_deref() != Some(generation_id.as_str()));
    let building_generations = RetrievalRepository::building_generation_statuses(pool)
        .await
        .map_err(|error| error.to_string())?;
    let lag = service.publication_lag();
    let (model_mismatch, activation_transition) = service.semantic_unavailable_state();

    let semantic_state = if paused {
        "paused".to_string()
    } else if service.model_load_failure().is_some() {
        "model_unavailable".to_string()
    } else if pointer.is_none() {
        if building {
            "building".to_string()
        } else {
            "unavailable".to_string()
        }
    } else if activation_transition {
        "transitioning".to_string()
    } else if model_mismatch {
        "model_mismatch".to_string()
    } else if service.active_snapshot().is_none() {
        "loading".to_string()
    } else if lag > 0 {
        "catching_up".to_string()
    } else {
        "ready".to_string()
    };

    Ok(RetrievalStatusReport {
        backend: crate::database::repositories::retrieval::EXACT_INDEX_BACKEND,
        semantic_state,
        model_load_failure: service.model_load_failure(),
        paused,
        active_generation_id: pointer,
        active_model_id: status.as_ref().map(|status| status.model_id.clone()),
        building_generations,
        document_count: status.as_ref().map_or(0, |status| status.document_count),
        tracked_meetings: status.as_ref().map_or(0, |status| status.tracked_meetings),
        current_meetings: status.as_ref().map_or(0, |status| status.current_meetings),
        retry_meetings: status.as_ref().map_or(0, |status| status.retry_meetings),
        failed_meetings: status.as_ref().map_or(0, |status| status.failed_meetings),
        canonical_change_id: status
            .as_ref()
            .and_then(|status| status.canonical_change_id),
        published_change_id: status
            .as_ref()
            .and_then(|status| status.published_change_id),
        activation_blockers: service.pending_activation_blockers(),
        resident_index_bytes: service.resident_vector_bytes(),
        resident_process_bytes: measure_resident_ram(),
        activation_ram_ceiling_bytes: ACTIVATION_RAM_CEILING_BYTES,
        derived_disk_bytes: RetrievalRepository::derived_disk_bytes(pool)
            .await
            .map_err(|error| error.to_string())?,
        derived_disk_is_estimate: true,
        derived_disk_backing_store_upper_bound_bytes:
            RetrievalRepository::derived_backing_store_upper_bound_bytes(pool)
                .await
                .map_err(|error| error.to_string())?,
        derived_disk_steady_target_bytes: DERIVED_DISK_STEADY_TARGET_BYTES,
        derived_disk_activation_limit_bytes: DERIVED_DISK_ACTIVATION_LIMIT_BYTES,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::retrieval::{
        ModelSpec, ReplacementJob, ReplacementOutcome, StagedDocument, VectorEncoding,
    };
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    use std::time::Duration;

    const DIMS: usize = 4;
    const MODEL_ID: &str = "test-e5-int8";
    const SCALE: f64 = 1.0 / 127.0;

    // -- Harness -------------------------------------------------------------

    async fn migrated_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
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

    async fn insert_meeting(pool: &SqlitePool, id: &str, title: &str) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(title)
            .bind("2026-08-26T00:00:00Z")
            .bind("2026-08-26T00:00:00Z")
            .execute(pool)
            .await
            .unwrap();
    }

    async fn add_transcript(pool: &SqlitePool, id: &str, meeting_id: &str, text: &str) {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind(id)
        .bind(meeting_id)
        .bind(text)
        .bind("10:00")
        .execute(pool)
        .await
        .unwrap();
    }

    async fn register_test_model(pool: &SqlitePool) {
        assert!(RetrievalRepository::ensure_model(
            pool,
            &ModelSpec {
                model_id: MODEL_ID.to_string(),
                dimensions: DIMS as u32,
                vector_encoding: VectorEncoding::Int8,
                chunker_version: 1,
                dequantization_scale: Some(SCALE),
                dequantization_zero_point: Some(0),
            }
        )
        .await
        .unwrap());
    }

    /// Deterministic test embedding: a pure one-hot unit vector whose axis is
    /// picked from the text, so distinct-axis documents are exactly orthogonal.
    fn vector_for(text: &str) -> Vec<f32> {
        let axis = text.as_bytes().first().copied().unwrap_or(0) as usize % DIMS;
        let mut vector = vec![0.0_f32; DIMS];
        vector[axis] = 1.0;
        vector
    }

    fn query_for(text: &str) -> Vec<f32> {
        vector_for(text)
    }

    fn contains_hit(hits: &[VectorHit], document_id: &str) -> bool {
        hits.iter().any(|hit| hit.document_id == document_id)
    }

    async fn current_revision(pool: &SqlitePool, meeting: &str) -> i64 {
        RetrievalRepository::current_source_revision(pool, meeting)
            .await
            .unwrap()
            .unwrap()
    }

    /// Publishes canonical documents for one meeting through the Task 2.4
    /// repository transaction (staging + revision-fenced replacement), so the
    /// journal/canonical state matches production byte-for-byte.
    async fn publish_meeting(pool: &SqlitePool, generation: &str, meeting: &str, texts: &[&str]) {
        let revision = current_revision(pool, meeting).await;
        let documents: Vec<StagedDocument> = texts
            .iter()
            .enumerate()
            .map(|(ordinal, text)| StagedDocument {
                document_id: format!("doc-{meeting}-{ordinal}"),
                source_kind: "transcript".to_string(),
                source_start_id: None,
                source_end_id: None,
                source_template_id: None,
                heading: None,
                ordinal: ordinal as i64,
                content: (*text).to_string(),
                content_hash: vec![ordinal as u8; 32],
                dimensions: DIMS as i64,
                vector_encoding: VectorEncoding::Int8,
                vector: quantize_int8(&vector_for(text)).unwrap(),
            })
            .collect();
        RetrievalRepository::stage_documents(
            pool,
            &format!("job-{generation}-{meeting}-{revision}"),
            generation,
            meeting,
            revision,
            &documents,
        )
        .await
        .unwrap();
        assert!(matches!(
            RetrievalRepository::replace_meeting_documents(
                pool,
                ReplacementJob {
                    generation_id: generation,
                    meeting_id: meeting,
                    expected_source_revision: revision,
                    job_id: &format!("job-{generation}-{meeting}-{revision}"),
                },
            )
            .await
            .unwrap(),
            ReplacementOutcome::Published { .. }
        ));
    }

    fn fresh_service() -> QueryIndexService {
        let service = QueryIndexService::new(RetrievalScheduler::new());
        service.set_loaded_model(MODEL_ID);
        service
    }

    async fn search_all(
        service: &QueryIndexService,
        text: &str,
    ) -> Result<Vec<VectorHit>, SearchFailure> {
        service
            .search(
                &query_for(text),
                ScopeFilter::All,
                10,
                &CancellationToken::new(),
            )
            .await
    }

    fn hit_ids(hits: &[VectorHit]) -> Vec<String> {
        hits.iter().map(|hit| hit.document_id.clone()).collect()
    }

    #[tokio::test]
    async fn exact_search_orders_neighbors_and_breaks_ties_by_document_id() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Ordering").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-order", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-order", "m", &["alpha", "beta"]).await;

        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let hits = search_all(&service, "alpha").await.unwrap();

        // The exact-match document must win; the orthogonal-ish second
        // document follows with a strictly smaller score.
        assert_eq!(hits[0].document_id, "doc-m-0");
        assert!(hits[0].score > hits[1].score);
        assert!(hits[0].score <= 1.0 + 1e-5);

        // Identical vectors tie on score and are ordered by document id.
        let snapshot = IndexSnapshot::new(
            "gen-tie".to_string(),
            MODEL_ID.to_string(),
            2,
            BaseRows {
                metas: ["zz", "aa"]
                    .into_iter()
                    .map(|id| DocumentMeta {
                        document_id: format!("doc-{id}"),
                        meeting_id: "m".to_string(),
                        source_kind: "transcript".to_string(),
                        source_start_id: None,
                        source_end_id: None,
                        source_template_id: None,
                        heading: None,
                        ordinal: 0,
                    })
                    .collect(),
                vectors: [[127i8 as u8, 0], [127i8 as u8, 0]].concat(),
            },
            Overlay::default(),
        );
        let hits = scan_snapshot(
            &snapshot,
            &[127, 0],
            &ScopeFilter::All,
            10,
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            hit_ids(&hits),
            vec!["doc-aa".to_string(), "doc-zz".to_string()]
        );
        assert_eq!(hits[0].score, hits[1].score);
    }

    #[test]
    fn bounded_heap_keeps_the_smaller_document_id_among_equal_scores_regardless_of_insertion_order()
    {
        let make_snapshot = |first: &str, second: &str| {
            IndexSnapshot::new(
                "gen-tie".to_string(),
                MODEL_ID.to_string(),
                2,
                BaseRows {
                    metas: [first, second]
                        .into_iter()
                        .map(|id| DocumentMeta {
                            document_id: format!("doc-{id}"),
                            meeting_id: "m".to_string(),
                            source_kind: "transcript".to_string(),
                            source_start_id: None,
                            source_end_id: None,
                            source_template_id: None,
                            heading: None,
                            ordinal: 0,
                        })
                        .collect(),
                    vectors: [[127i8 as u8, 0u8], [127i8 as u8, 0u8]].concat(),
                },
                Overlay::default(),
            )
        };
        let query = [127i8, 0];

        // Reverse insertion with limit=1: the lexicographically larger id
        // enters the bounded heap first; the equally-scored smaller id must
        // still win the slot.
        let hits = scan_snapshot(
            &make_snapshot("zz", "aa"),
            &query,
            &ScopeFilter::All,
            1,
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(hit_ids(&hits), vec!["doc-aa".to_string()]);

        // Forward insertion sanity: same winner.
        let hits = scan_snapshot(
            &make_snapshot("aa", "zz"),
            &query,
            &ScopeFilter::All,
            1,
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(hit_ids(&hits), vec!["doc-aa".to_string()]);
    }

    #[tokio::test]
    async fn narrow_allow_list_never_emits_other_meetings() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "in-scope", "In").await;
        insert_meeting(&pool, "out-of-scope", "Out").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-scope", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-scope", "in-scope", &["shared topic"]).await;
        publish_meeting(&pool, "gen-scope", "out-of-scope", &["shared topic"]).await;

        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();

        let hits = service
            .search(
                &query_for("shared"),
                ScopeFilter::meetings(["out-of-scope"]),
                10,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|hit| hit.meeting_id == "out-of-scope"));

        // An empty allow-list fails closed, and post-filtering re-checks every hit.
        let hits = service
            .search(
                &query_for("shared"),
                ScopeFilter::meetings(Vec::<String>::new()),
                10,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn steady_state_replay_uses_delta_without_copying_the_base() {
        let pool = migrated_pool().await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-delta", MODEL_ID)
            .await
            .unwrap();
        // A 120-row base keeps the overlay below the 3-doc compaction floor,
        // so delta behavior is observable without compaction interfering.
        for index in 0..60 {
            let meeting = format!("m{index}");
            insert_meeting(&pool, &meeting, &meeting).await;
        }
        for index in 0..60 {
            let meeting = format!("m{index}");
            publish_meeting(
                &pool,
                "gen-delta",
                &meeting,
                &["stable body text", "filler body text"],
            )
            .await;
        }
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let before = service.active_snapshot().unwrap();

        // A single-meeting update replays onto the same base allocation.
        add_transcript(&pool, "extra", "m7", "echo content").await;
        publish_meeting(
            &pool,
            "gen-delta",
            "m7",
            &["stable body text", "echo content"],
        )
        .await;
        publish_tick(&pool, &service).await.unwrap();
        let after = service.active_snapshot().unwrap();
        assert!(
            Arc::ptr_eq(&before.base, &after.base),
            "single-meeting updates must not rebuild or copy the base"
        );
        assert_eq!(after.overlay_documents, 2);

        // Readers holding the old Arc still observe the complete old state.
        assert_eq!(before.document_count(), 120);
        assert_eq!(after.document_count(), 122);
        let updated_hits = service
            .search(
                &query_for("echo"),
                ScopeFilter::All,
                MAX_QUERY_LIMIT,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        // Only the updated meeting uses this axis; every other document sits
        // at exactly zero similarity below it.
        let top = &updated_hits[0];
        assert_eq!(
            (top.meeting_id.as_str(), top.document_id.as_str()),
            ("m7", "doc-m7-1")
        );
        assert!(updated_hits.iter().skip(1).all(|hit| hit.score < top.score));
    }

    #[tokio::test]
    async fn compaction_merges_overlay_at_the_approved_threshold_preserving_results() {
        let pool = migrated_pool().await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-compaction", MODEL_ID)
            .await
            .unwrap();
        // 60 meetings x 2 docs = 120 base rows; the 2% threshold is ceil(120/50)=3 docs.
        for index in 0..60 {
            let meeting = format!("m{index}");
            insert_meeting(&pool, &meeting, &meeting).await;
        }
        for index in 0..60 {
            let meeting = format!("m{index}");
            publish_meeting(
                &pool,
                "gen-compaction",
                &meeting,
                &["stable body text", "filler body text"],
            )
            .await;
        }
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let baseline_hits = service
            .search(
                &query_for("stable"),
                ScopeFilter::All,
                MAX_QUERY_LIMIT,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(baseline_hits.len(), 120);
        let pre_base = Arc::clone(&service.lock_state().active.as_ref().unwrap().base);

        // Two overlay docs stay below the threshold...
        add_transcript(&pool, "t-x0", "m0", "delta one").await;
        publish_meeting(
            &pool,
            "gen-compaction",
            "m0",
            &["delta one", "stable body text"],
        )
        .await;
        publish_tick(&pool, &service).await.unwrap();
        {
            let state = service.lock_state();
            let snapshot = state.active.as_ref().unwrap();
            assert_eq!(snapshot.overlay_documents, 2);
            assert!(Arc::ptr_eq(&pre_base, &snapshot.base));
        }

        // ...the next update crosses it and compacts without changing results.
        add_transcript(&pool, "t-x1", "m1", "delta two").await;
        publish_meeting(
            &pool,
            "gen-compaction",
            "m1",
            &["delta two", "stable body text"],
        )
        .await;
        publish_tick(&pool, &service).await.unwrap();
        {
            let state = service.lock_state();
            let snapshot = state.active.as_ref().unwrap();
            assert_eq!(
                snapshot.overlay_documents, 0,
                "compaction drained the overlay"
            );
            assert!(!Arc::ptr_eq(&pre_base, &snapshot.base));
        }
        let after_hits = service
            .search(
                &query_for("stable"),
                ScopeFilter::All,
                MAX_QUERY_LIMIT,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        // Compaction preserves the candidate SET; ordering within score ties
        // may legitimately change because two meetings were re-indexed.
        let to_set = |hits: &[VectorHit]| -> BTreeSet<String> {
            hits.iter().map(|hit| hit.document_id.clone()).collect()
        };
        assert_eq!(to_set(&baseline_hits), to_set(&after_hits));
        let delta_hits = service
            .search(
                &query_for("delta"),
                ScopeFilter::All,
                MAX_QUERY_LIMIT,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(contains_hit(&delta_hits, "doc-m1-0"));
    }

    #[tokio::test]
    async fn journal_crash_replay_publishes_missing_changes_on_restart() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "crashed", "Crashed").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-crash", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-crash", "crashed", &["pre crash content"]).await;
        // Simulate the crash window: canonical advanced but nothing was
        // acknowledged (published stays behind canonical).
        let (canonical, published) = RetrievalRepository::publication_lag(&pool, "gen-crash")
            .await
            .unwrap()
            .unwrap();
        assert!(canonical > published);

        // Restart: a fresh service (empty memory) catches up from the durable
        // published bound.
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let hits = search_all(&service, "crash").await.unwrap();
        assert!(contains_hit(&hits, "doc-crashed-0"));
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen-crash")
                .await
                .unwrap(),
            Some((canonical, canonical))
        );

        // A change committed while serving is replayed through the delta on
        // the next tick without dropping availability in between.
        add_transcript(&pool, "t2", "crashed", "post restart content").await;
        publish_meeting(
            &pool,
            "gen-crash",
            "crashed",
            &["pre crash content", "post restart content"],
        )
        .await;
        publish_tick(&pool, &service).await.unwrap();
        let hits = search_all(&service, "restart").await.unwrap();
        assert!(contains_hit(&hits, "doc-crashed-1"));
    }

    #[tokio::test]
    async fn sparse_ids_repeated_upserts_and_upsert_delete_ordering_converge() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "folded", "Folded").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-fold", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-fold", "folded", &["original content"]).await;

        // Force genuinely sparse journal IDs the way rolled-back transactions
        // do: bump AUTOINCREMENT past a gap.
        sqlx::query(
            "INSERT INTO retrieval_index_changes (change_id, generation_id, meeting_id, operation, created_at)
             VALUES (500, 'gen-fold', 'folded', 'upsert', '2026-01-01T00:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM retrieval_index_changes WHERE change_id = 500")
            .execute(&pool)
            .await
            .unwrap();

        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        assert!(contains_hit(
            &search_all(&service, "original").await.unwrap(),
            "doc-folded-0"
        ));

        // Repeated upserts: the last writer wins and is served after replay.
        add_transcript(&pool, "t-a", "folded", "second revision content").await;
        publish_meeting(
            &pool,
            "gen-fold",
            "folded",
            &["original content", "second revision content"],
        )
        .await;
        add_transcript(&pool, "t-b", "folded", "third revision content").await;
        publish_meeting(
            &pool,
            "gen-fold",
            "folded",
            &[
                "original content",
                "second revision content",
                "third revision content",
            ],
        )
        .await;
        publish_tick(&pool, &service).await.unwrap();
        let hits = search_all(&service, "revision").await.unwrap();
        assert!(contains_hit(&hits, "doc-folded-2"));
        let (_, published) = RetrievalRepository::publication_lag(&pool, "gen-fold")
            .await
            .unwrap()
            .unwrap();
        assert!(
            published > 500,
            "natural IDs continue past the forced gap (sparse IDs absorbed)"
        );

        // Upsert then delete: the tombstone cascade removes every vector and
        // cannot leak through any scope.
        sqlx::query("DELETE FROM meetings WHERE id = 'folded'")
            .execute(&pool)
            .await
            .unwrap();
        publish_tick(&pool, &service).await.unwrap();
        assert!(search_all(&service, "content").await.unwrap().is_empty());

        // Delete then upsert (meeting re-created under the same ID): the
        // later upsert resurrects the documents.
        insert_meeting(&pool, "folded", "Folded Again").await;
        publish_meeting(&pool, "gen-fold", "folded", &["resurrected content"]).await;
        publish_tick(&pool, &service).await.unwrap();
        let hits = search_all(&service, "resurrected").await.unwrap();
        assert!(contains_hit(&hits, "doc-folded-0"));
    }

    #[tokio::test]
    async fn deletion_tombstones_remove_vectors_before_queries_resume() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "doomed", "Doomed").await;
        insert_meeting(&pool, "survivor", "Survivor").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-delete", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-delete", "doomed", &["unique doomed wording"]).await;
        publish_meeting(&pool, "gen-delete", "survivor", &["survivor content here"]).await;

        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        assert!(contains_hit(
            &search_all(&service, "doomed").await.unwrap(),
            "doc-doomed-0"
        ));

        // Meeting deletion cascades derived rows and journals tombstones; the
        // publisher applies them before semantic queries can serve the stale
        // vectors again.
        sqlx::query("DELETE FROM meetings WHERE id = 'doomed'")
            .execute(&pool)
            .await
            .unwrap();
        publish_tick(&pool, &service).await.unwrap();

        let remaining = search_all(&service, "unique").await.unwrap();
        assert!(!remaining.iter().any(|hit| hit.meeting_id == "doomed"));
        assert!(contains_hit(
            &search_all(&service, "survivor").await.unwrap(),
            "doc-survivor-0"
        ));
        // The deleted meeting can never leak through an explicit allow-list.
        let scoped = service
            .search(
                &query_for("unique"),
                ScopeFilter::meetings(["doomed"]),
                10,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(scoped.is_empty());
    }

    #[tokio::test]
    async fn deferred_publication_pauses_semantic_queries_until_caught_up() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "deferred", "Deferred").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-defer", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-defer", "deferred", &["first published wording"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        assert!(contains_hit(
            &search_all(&service, "published").await.unwrap(),
            "doc-deferred-0"
        ));

        // Publish a second revision, then corrupt its canonical blob so the
        // delta reload fails typed: the batch defers, lag stays positive, and
        // semantic queries pause instead of serving a stale snapshot.
        add_transcript(&pool, "t-defer", "deferred", "second wording arrives").await;
        publish_meeting(
            &pool,
            "gen-defer",
            "deferred",
            &["first published wording", "second wording arrives"],
        )
        .await;
        sqlx::query("UPDATE retrieval_documents SET vector = x'0102'")
            .execute(&pool)
            .await
            .unwrap();

        // The quarantine requeued the work; until it heals, queries are typed
        // as catch-up-pending (lexical-only availability preserved).
        publish_tick(&pool, &service).await.unwrap();
        let failure = search_all(&service, "wording").await.unwrap_err();
        assert!(
            matches!(failure, SearchFailure::CatchUpPending { behind } if behind > 0),
            "expected CatchUpPending, got {failure:?}"
        );
    }

    #[tokio::test]
    async fn readers_hold_complete_snapshots_while_publication_proceeds() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "reader", "Reader").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-reader", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-reader", "reader", &["initial content"]).await;
        let service = Arc::new(fresh_service());
        publish_tick(&pool, &service).await.unwrap();
        let known_ids: BTreeSet<String> = service
            .active_snapshot()
            .unwrap()
            .base
            .metas
            .iter()
            .map(|meta| meta.document_id.clone())
            .collect();
        let old = service.active_snapshot().unwrap();

        // Concurrent reader hammers active_snapshot while updates land; every
        // observed snapshot must be internally complete (document count within
        // the known bounds, never a partial mixture).
        let stop = Arc::new(AtomicU64::new(0));
        let reader_stop = Arc::clone(&stop);
        let reader_service = Arc::clone(&service);
        let reader = std::thread::spawn(move || {
            let mut observations = 0_u64;
            while reader_stop.load(Ordering::Relaxed) == 0 {
                if let Some(snapshot) = reader_service.active_snapshot() {
                    let count = snapshot.document_count();
                    assert!(
                        (1..=21).contains(&count),
                        "impossible partial observation: {count}"
                    );
                    observations += 1;
                }
            }
            observations
        });

        for round in 1..=10u32 {
            add_transcript(
                &pool,
                &format!("r{round}"),
                "reader",
                &format!("round {round}"),
            )
            .await;
            let mut texts: Vec<String> = (0..round).map(|index| format!("word{index}")).collect();
            texts.push("initial content".to_string());
            let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            publish_meeting(&pool, "gen-reader", "reader", &refs).await;
            publish_tick(&pool, &service).await.unwrap();
        }
        stop.store(1, Ordering::Relaxed);
        let observations = reader.join().unwrap();
        assert!(observations > 0);

        // The old Arc remains complete and unchanged after every swap.
        assert_eq!(old.document_count(), 1);
        assert_eq!(
            old.base
                .metas
                .iter()
                .map(|meta| meta.document_id.clone())
                .collect::<BTreeSet<String>>(),
            known_ids
        );
        assert_eq!(service.active_snapshot().unwrap().document_count(), 11);
    }

    #[tokio::test]
    async fn activation_blocked_until_coverage_completes_then_promotes_atomically() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "first", "First").await;
        insert_meeting(&pool, "second", "Second").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-partial", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-partial", "first", &["first content"]).await;

        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();

        // Partial initial coverage must not activate: pointer unset, semantic
        // queries unavailable, blocker reported.
        assert!(RetrievalRepository::active_generation_id(&pool)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            search_all(&service, "first").await.unwrap_err(),
            SearchFailure::NoActiveGeneration
        );
        assert!(service
            .pending_activation_blockers()
            .iter()
            .any(|blocker| blocker.contains("coverage")));

        publish_meeting(&pool, "gen-partial", "second", &["second content"]).await;
        publish_tick(&pool, &service).await.unwrap();

        // Complete coverage promotes atomically: pointer + memory swap together.
        assert_eq!(
            RetrievalRepository::active_generation_id(&pool)
                .await
                .unwrap(),
            Some("gen-partial".to_string())
        );
        let hits = search_all(&service, "second").await.unwrap();
        assert!(contains_hit(&hits, "doc-second-0"));
        assert!(service.pending_activation_blockers().is_empty());
        let report = index_status(&pool, &service, false).await.unwrap();
        assert_eq!(report.semantic_state, "ready");
        assert_eq!(report.backend, "exact");
        assert_eq!(
            report.derived_disk_steady_target_bytes,
            DERIVED_DISK_STEADY_TARGET_BYTES
        );
    }

    #[tokio::test]
    async fn activation_transaction_rejects_a_meeting_mutation_after_preflight() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Meeting").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-fenced", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-fenced", "m", &["complete"]).await;
        let caught_up_to = RetrievalRepository::publication_lag(&pool, "gen-fenced")
            .await
            .unwrap()
            .unwrap()
            .0;

        add_transcript(&pool, "after-preflight", "m", "changed").await;
        assert!(!RetrievalRepository::activate_generation_if_ready(
            &pool,
            "gen-fenced",
            caught_up_to
        )
        .await
        .unwrap());
        assert!(RetrievalRepository::active_generation_id(&pool)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn third_generation_is_refused_and_shadow_cancel_keeps_active_queryable() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "solo", "Solo").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-active", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-active", "solo", &["active generation content"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        assert_eq!(
            search_all(&service, "active").await.unwrap().len(),
            1,
            "active retrieval keeps working"
        );

        // First shadow is accepted (two retained generations).
        let shadow = request_rebuild(&pool).await.unwrap();
        assert_ne!(shadow, "gen-active");

        // A third is refused safely.
        assert!(matches!(
            request_rebuild(&pool).await,
            Err(RebuildRequestError::RetentionLimit)
        ));

        // Cancelling the shadow removes only its derived state; the active
        // generation and its snapshot remain fully usable.
        assert!(RetrievalRepository::delete_generation(&pool, &shadow)
            .await
            .unwrap());
        assert_eq!(
            search_all(&service, "active").await.unwrap().len(),
            1,
            "cancellation must leave the active generation intact"
        );
        // Retention freed: another rebuild is accepted.
        assert!(request_rebuild(&pool).await.is_ok());
    }

    #[tokio::test]
    async fn completed_shadow_promotes_and_retires_previous_active() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "one", "One").await;
        insert_meeting(&pool, "two", "Two").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-old", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-old", "one", &["one content"]).await;
        publish_meeting(&pool, "gen-old", "two", &["two content"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let old_active = service.active_snapshot().unwrap();

        // Manual rebuild shadows the SAME model; the old generation stays
        // queryable while the shadow builds.
        let shadow = request_rebuild(&pool).await.unwrap();
        publish_tick(&pool, &service).await.unwrap();
        assert_eq!(
            RetrievalRepository::active_generation_id(&pool)
                .await
                .unwrap(),
            Some("gen-old".to_string()),
            "an incomplete shadow must not displace the active pointer"
        );
        assert_eq!(search_all(&service, "content").await.unwrap().len(), 2);

        // Complete the shadow for both meetings, then promote.
        publish_meeting(&pool, &shadow, "one", &["one content rebuilt"]).await;
        publish_meeting(&pool, &shadow, "two", &["two content rebuilt"]).await;
        publish_tick(&pool, &service).await.unwrap();

        assert_eq!(
            RetrievalRepository::active_generation_id(&pool)
                .await
                .unwrap(),
            Some(shadow.clone())
        );
        let promoted = service.active_snapshot().unwrap();
        assert_eq!(promoted.generation_id(), shadow);
        assert_eq!(promoted.document_count(), old_active.document_count());
        assert_eq!(search_all(&service, "rebuilt").await.unwrap().len(), 2);

        let previous_state: (String,) = sqlx::query_as(
            "SELECT state FROM retrieval_generations WHERE generation_id = 'gen-old'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(previous_state.0, "retired");

        // The retired generation survives this process; GC requires one clean
        // restart plus a successful query, so it is still present now.
        assert!(RetrievalRepository::retired_generations(&pool)
            .await
            .unwrap()
            .iter()
            .any(|(id, _)| id == "gen-old"));
    }

    #[tokio::test]
    async fn corrupt_active_generation_deactivates_to_fts_only_and_requeues() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "victim", "Victim").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-corrupt", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(
            &pool,
            "gen-corrupt",
            "victim",
            &["healthy before corruption"],
        )
        .await;
        let healthy_service = fresh_service();
        publish_tick(&pool, &healthy_service).await.unwrap();
        assert_eq!(
            search_all(&healthy_service, "healthy").await.unwrap().len(),
            1
        );

        // Post-install corruption of the canonical blob: the NEXT load must
        // quarantine instead of admitting the bad vector or panicking.
        sqlx::query("UPDATE retrieval_documents SET vector = x'0102'")
            .execute(&pool)
            .await
            .unwrap();

        // Restart semantics: a fresh service catches up the pointed-at active
        // generation, detects corruption, deactivates to FTS-only, and
        // requeues the affected work for rebuild.
        let restarted = fresh_service();
        publish_tick(&pool, &restarted).await.unwrap();
        assert!(RetrievalRepository::active_generation_id(&pool)
            .await
            .unwrap()
            .is_none());
        assert!(restarted.active_snapshot().is_none());
        assert_eq!(
            restarted
                .search(
                    &query_for("healthy"),
                    ScopeFilter::All,
                    10,
                    &CancellationToken::new()
                )
                .await,
            Err(SearchFailure::NoActiveGeneration)
        );
        let (state, indexed): (String, i64) = sqlx::query_as(
            "SELECT state, indexed_source_revision FROM retrieval_meeting_state
             WHERE generation_id = 'gen-corrupt' AND meeting_id = 'victim'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((state.as_str(), indexed), ("pending", 0));

        // The previously served (clean) snapshot Arc is untouched by all of
        // this - readers never see corrupted or partial state.
        assert_eq!(
            search_all(&healthy_service, "healthy").await.unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn canonical_commit_pauses_queries_before_the_next_publisher_tick() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Meeting").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen", "m", &["before"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();

        let epoch = service.mark_stale();
        add_transcript(&pool, "changed", "m", "changed").await;
        publish_meeting(&pool, "gen", "m", &["changed"]).await;
        service.commit_stale(
            epoch,
            "gen",
            RetrievalRepository::publication_lag(&pool, "gen")
                .await
                .unwrap()
                .unwrap()
                .0,
            Some(0),
        );
        assert_eq!(
            search_all(&service, "before").await.unwrap_err(),
            SearchFailure::CatchUpPending { behind: 1 }
        );
        publish_tick(&pool, &service).await.unwrap();
        assert!(contains_hit(
            &search_all(&service, "changed").await.unwrap(),
            "doc-m-0"
        ));
    }

    #[tokio::test]
    async fn failed_overlapping_mutation_cannot_clear_a_committed_publication_barrier() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Meeting").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen", "m", &["before"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();

        let mutation_a = service.mark_stale();
        let mutation_b = service.mark_stale();
        service.restore_stale(mutation_b);
        add_transcript(&pool, "changed", "m", "changed").await;
        publish_meeting(&pool, "gen", "m", &["changed"]).await;
        service.commit_stale(
            mutation_a,
            "gen",
            RetrievalRepository::publication_lag(&pool, "gen")
                .await
                .unwrap()
                .unwrap()
                .0,
            Some(0),
        );

        assert_eq!(
            search_all(&service, "before").await.unwrap_err(),
            SearchFailure::CatchUpPending { behind: 1 }
        );
        publish_tick(&pool, &service).await.unwrap();
        assert!(contains_hit(
            &search_all(&service, "changed").await.unwrap(),
            "doc-m-0"
        ));
    }

    #[test]
    fn stale_tokens_clear_only_at_their_durable_bound_and_activation_drops_old_generation_barriers()
    {
        let service = fresh_service();
        let first = service.mark_stale();
        let pending = service.mark_stale();
        let second = service.mark_stale();
        service.commit_stale(first, "old", 10, Some(0));
        service.commit_stale(second, "old", 20, Some(0));
        service.clear_published_stale("old", 10);
        let state = service.lock_state();
        assert!(state.pending_stale.contains(&pending));
        assert!(!state.committed_stale.contains_key(&first));
        assert!(state.committed_stale.contains_key(&second));
        drop(state);

        service.install_active(Arc::new(IndexSnapshot::new(
            "new".to_string(),
            MODEL_ID.to_string(),
            DIMS,
            BaseRows::default(),
            Overlay::default(),
        )));
        let state = service.lock_state();
        assert!(state.pending_stale.contains(&pending));
        assert!(state.committed_stale.is_empty());
        drop(state);
        service.commit_stale(pending, "old", 30, Some(0));
        let state = service.lock_state();
        assert!(state.pending_stale.is_empty());
        assert!(state.committed_stale.is_empty());
    }

    #[tokio::test]
    async fn acknowledged_delete_before_stale_binding_does_not_strand_queries() {
        let service = fresh_service();
        service.install_active(Arc::new(IndexSnapshot::new(
            "gen".to_string(),
            MODEL_ID.to_string(),
            1,
            BaseRows {
                metas: vec![DocumentMeta {
                    document_id: "doc".to_string(),
                    meeting_id: "meeting".to_string(),
                    source_kind: "transcript".to_string(),
                    source_start_id: None,
                    source_end_id: None,
                    source_template_id: None,
                    heading: None,
                    ordinal: 0,
                }],
                vectors: vec![127],
            },
            Overlay::default(),
        )));
        let token = service.mark_stale();

        service.clear_published_stale("gen", 7);
        service.commit_stale(token, "gen", 7, Some(0));

        assert!(service.lock_state().committed_stale.is_empty());
        assert_eq!(
            service
                .search(&[1.0], ScopeFilter::All, 1, &CancellationToken::new(),)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn metadata_read_error_after_commit_keeps_queries_blocked_until_reconciliation() {
        let service = fresh_service();
        service.install_active(Arc::new(IndexSnapshot::new(
            "gen".to_string(),
            MODEL_ID.to_string(),
            1,
            BaseRows::default(),
            Overlay::default(),
        )));
        let token = service.mark_stale();
        service.commit_stale(token, "gen", 7, None);

        assert!(matches!(
            service
                .search(&[1.0], ScopeFilter::All, 1, &CancellationToken::new())
                .await,
            Err(SearchFailure::CatchUpPending { .. })
        ));
        service.clear_published_stale("gen", 7);
        assert!(service.lock_state().committed_stale.is_empty());
    }

    #[tokio::test]
    async fn terminal_failure_suppression_survives_snapshot_reload() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "failed", "Failed").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-failed", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-failed", "failed", &["prior vector"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        assert!(contains_hit(
            &search_all(&service, "prior").await.unwrap(),
            "doc-failed-0"
        ));

        RetrievalRepository::record_work_failure(
            &pool,
            "gen-failed",
            "failed",
            true,
            "terminal test failure",
            "2099-01-01T00:00:00Z",
        )
        .await
        .unwrap();
        service.suppress_terminal_failure("gen-failed", "failed");
        assert!(search_all(&service, "prior").await.unwrap().is_empty());

        let restarted = fresh_service();
        publish_tick(&pool, &restarted).await.unwrap();
        assert!(search_all(&restarted, "prior").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn activation_transition_blocks_old_snapshot_hits_and_status() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Meeting").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-transition", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-transition", "m", &["old generation hit"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();

        service.begin_activation_transition();
        assert!(matches!(
            search_all(&service, "old").await,
            Err(SearchFailure::CatchUpPending { .. })
        ));
        assert_eq!(
            index_status(&pool, &service, false)
                .await
                .unwrap()
                .semantic_state,
            "transitioning"
        );
        service.cancel_activation_transition();
    }

    #[tokio::test]
    async fn same_process_deactivation_removes_installed_vectors() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Meeting").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen", "m", &["healthy"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        sqlx::query("UPDATE retrieval_documents SET vector = x'01'")
            .execute(&pool)
            .await
            .unwrap();

        catch_up_active_generation(&pool, &service, "gen", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            search_all(&service, "healthy").await.unwrap_err(),
            SearchFailure::NoActiveGeneration
        );
    }

    #[tokio::test]
    async fn additional_shadow_runtime_keeps_active_generation_queryable() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Meeting").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen", "m", &["old model content"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();

        service.set_loaded_model("new-model-runtime");
        assert!(search_all(&service, "old").await.is_ok());
        assert_eq!(
            index_status(&pool, &service, false)
                .await
                .unwrap()
                .semantic_state,
            "ready"
        );
        service.acknowledge_fast_hybrid_query();
    }

    #[tokio::test]
    async fn retired_generation_requires_explicit_fast_hybrid_acknowledgement() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Meeting").await;
        register_test_model(&pool).await;
        for generation in ["old", "active"] {
            RetrievalRepository::ensure_generation(&pool, generation, MODEL_ID)
                .await
                .unwrap();
            publish_meeting(&pool, generation, "m", &["content"]).await;
        }
        RetrievalRepository::set_generation_state(&pool, "old", "retired")
            .await
            .unwrap();
        sqlx::query("UPDATE retrieval_generations SET retired_at = '2000-01-01T00:00:00Z' WHERE generation_id = 'old'")
            .execute(&pool)
            .await
            .unwrap();
        RetrievalRepository::set_generation_state(&pool, "active", "ready")
            .await
            .unwrap();
        RetrievalRepository::switch_active_generation(&pool, "active")
            .await
            .unwrap();

        let restarted = fresh_service();
        publish_tick(&pool, &restarted).await.unwrap();
        assert!(search_all(&restarted, "content").await.is_ok());
        publish_tick(&pool, &restarted).await.unwrap();
        assert!(RetrievalRepository::retired_generations(&pool)
            .await
            .unwrap()
            .iter()
            .any(|(id, _)| id == "old"));

        restarted.acknowledge_fast_hybrid_query();
        publish_tick(&pool, &restarted).await.unwrap();
        assert!(!RetrievalRepository::retired_generations(&pool)
            .await
            .unwrap()
            .iter()
            .any(|(id, _)| id == "old"));
    }

    #[tokio::test]
    async fn failed_non_active_generation_counts_and_is_gc_eligible() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Meeting").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "failed", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "failed", "m", &["content"]).await;
        RetrievalRepository::set_generation_state(&pool, "failed", "failed")
            .await
            .unwrap();
        sqlx::query("UPDATE retrieval_generations SET retired_at = '2000-01-01T00:00:00Z' WHERE generation_id = 'failed'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            RetrievalRepository::count_retained_generations(&pool)
                .await
                .unwrap(),
            1
        );

        let restarted = fresh_service();
        restarted.acknowledge_fast_hybrid_query();
        publish_tick(&pool, &restarted).await.unwrap();
        assert!(RetrievalRepository::retired_generations(&pool)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn distinct_shadow_model_keeps_the_old_runtime_queryable_until_activation() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Meeting").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "old", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "old", "m", &["old content"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();

        let new_model = "test-e5-new";
        RetrievalRepository::register_model(
            &pool,
            &ModelSpec {
                model_id: new_model.to_string(),
                dimensions: DIMS as u32,
                vector_encoding: VectorEncoding::Int8,
                chunker_version: 1,
                dequantization_scale: Some(SCALE),
                dequantization_zero_point: Some(0),
            },
        )
        .await
        .unwrap();
        RetrievalRepository::ensure_generation(&pool, "shadow", new_model)
            .await
            .unwrap();
        service.set_loaded_model(new_model);
        publish_tick(&pool, &service).await.unwrap();
        assert_eq!(service.active_generation().as_deref(), Some("old"));
        assert!(search_all(&service, "old").await.is_ok());

        publish_meeting(&pool, "shadow", "m", &["new content"]).await;
        publish_tick(&pool, &service).await.unwrap();
        assert_eq!(service.active_generation().as_deref(), Some("shadow"));
    }

    #[tokio::test]
    async fn status_reports_safe_model_load_failures_and_clears_after_load() {
        let pool = migrated_pool().await;
        let service = fresh_service();
        for reason in ["bundle manifest missing", "bundle artifact corrupt"] {
            service.set_model_load_failure(reason.to_string());
            let status = index_status(&pool, &service, false).await.unwrap();
            assert_eq!(status.semantic_state, "model_unavailable");
            assert_eq!(status.model_load_failure.as_deref(), Some(reason));
        }
        service.set_loaded_model(MODEL_ID);
        assert!(index_status(&pool, &service, false)
            .await
            .unwrap()
            .model_load_failure
            .is_none());
    }

    #[test]
    fn tombstone_threshold_counts_hidden_base_vectors_not_meetings() {
        let base = BaseRows {
            metas: (0..100)
                .map(|ordinal| DocumentMeta {
                    document_id: ordinal.to_string(),
                    meeting_id: "large".to_string(),
                    source_kind: "transcript".to_string(),
                    source_start_id: None,
                    source_end_id: None,
                    source_template_id: None,
                    heading: None,
                    ordinal,
                })
                .collect(),
            vectors: vec![0; 100],
        };
        let snapshot = IndexSnapshot::new(
            "gen".to_string(),
            MODEL_ID.to_string(),
            1,
            base,
            Overlay {
                upserted: BTreeMap::new(),
                deleted: ["large".to_string()].into_iter().collect(),
            },
        );
        assert_eq!(snapshot.tombstoned_base_documents(), 100);
    }

    #[tokio::test]
    async fn quarantined_shadow_blocks_activation_but_active_serves_on() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "live", "Live").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-live", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-live", "live", &["live queryable content"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();

        let shadow = request_rebuild(&pool).await.unwrap();
        publish_meeting(&pool, &shadow, "live", &["shadow rebuilt content"]).await;
        sqlx::query("UPDATE retrieval_documents SET vector = x'0909' WHERE generation_id = ?")
            .bind(&shadow)
            .execute(&pool)
            .await
            .unwrap();

        publish_tick(&pool, &service).await.unwrap();
        // Activation blocked on validation; active generation untouched.
        assert_eq!(
            RetrievalRepository::active_generation_id(&pool)
                .await
                .unwrap(),
            Some("gen-live".to_string())
        );
        assert!(service
            .pending_activation_blockers()
            .iter()
            .any(|blocker| blocker.contains("snapshot validation")));
        assert_eq!(search_all(&service, "queryable").await.unwrap().len(), 1);
        // Quarantined work was requeued for healing.
        let (state,): (String,) = sqlx::query_as(
            "SELECT state FROM retrieval_meeting_state WHERE generation_id = ? AND meeting_id = 'live'",
        )
        .bind(&shadow)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, "pending");
    }

    #[test]
    fn disk_envelope_gate_blocks_only_above_the_approved_peak() {
        assert!(disk_envelope_blocker(DERIVED_DISK_ACTIVATION_LIMIT_BYTES).is_none());
        let blocker = disk_envelope_blocker(DERIVED_DISK_ACTIVATION_LIMIT_BYTES + 1).unwrap();
        assert!(blocker.contains("activation limit"));
        // Steady target sits below the activation ceiling by construction.
        assert!(DERIVED_DISK_STEADY_TARGET_BYTES < DERIVED_DISK_ACTIVATION_LIMIT_BYTES);
    }

    #[tokio::test]
    async fn envelope_accounting_is_measured_and_reported() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "measured", "Measured").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-disk", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-disk", "measured", &["measured content"]).await;

        // SQLite derived data and the building shadow both count before activation.
        let sqlite_derived = RetrievalRepository::derived_disk_bytes(&pool)
            .await
            .unwrap();
        assert!(sqlite_derived > 0);
        assert!(
            RetrievalRepository::estimated_shadow_snapshot_bytes(&pool)
                .await
                .unwrap()
                >= DIMS as u64,
            "building generations count their persisted vector bytes"
        );

        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let report = index_status(&pool, &service, false).await.unwrap();
        assert_eq!(report.derived_disk_bytes, sqlite_derived);
        assert_eq!(report.resident_index_bytes, service.resident_vector_bytes());
        assert!(report.resident_index_bytes >= DIMS as u64);
        assert_eq!(
            report.activation_ram_ceiling_bytes,
            ACTIVATION_RAM_CEILING_BYTES
        );
    }

    #[test]
    fn ram_gate_admits_below_and_blocks_at_above_or_unavailable() {
        // Below the approved 1.30 GiB transient ceiling admits.
        assert!(ram_gate_blocker(Some(ACTIVATION_RAM_CEILING_BYTES - 1)).is_none());
        // At and above the ceiling block with the measured value named.
        let at_limit = ram_gate_blocker(Some(ACTIVATION_RAM_CEILING_BYTES)).unwrap();
        assert!(at_limit.contains("activation ceiling"));
        assert!(ram_gate_blocker(Some(ACTIVATION_RAM_CEILING_BYTES + 1)).is_some());
        // An unavailable measurement blocks fail-closed.
        let unavailable = ram_gate_blocker(None).unwrap();
        assert!(unavailable.contains("unavailable"));
    }

    #[tokio::test]
    async fn measured_ram_gate_blocks_activation_until_measurement_admits() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "ram", "Ram").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-ram", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-ram", "ram", &["ram gate content"]).await;

        let service = fresh_service();
        // Measurement unavailable: refuse to activate.
        let unavailable: RamProbe = Arc::new(|| None);
        service.set_ram_probe(unavailable);
        publish_tick(&pool, &service).await.unwrap();
        assert!(RetrievalRepository::active_generation_id(&pool)
            .await
            .unwrap()
            .is_none());
        assert!(service
            .pending_activation_blockers()
            .iter()
            .any(|blocker| blocker.contains("unavailable")));

        // At the approved ceiling: still blocked.
        let at_limit: RamProbe = Arc::new(|| Some(ACTIVATION_RAM_CEILING_BYTES));
        service.set_ram_probe(at_limit);
        publish_tick(&pool, &service).await.unwrap();
        assert!(RetrievalRepository::active_generation_id(&pool)
            .await
            .unwrap()
            .is_none());
        assert!(service
            .pending_activation_blockers()
            .iter()
            .any(|blocker| blocker.contains("activation ceiling")));

        // Below the ceiling: activation proceeds through pointer + memory swap.
        let below: RamProbe = Arc::new(|| Some(ACTIVATION_RAM_CEILING_BYTES - 1));
        service.set_ram_probe(below);
        publish_tick(&pool, &service).await.unwrap();
        assert_eq!(
            RetrievalRepository::active_generation_id(&pool)
                .await
                .unwrap(),
            Some("gen-ram".to_string())
        );
        let hits = search_all(&service, "gate").await.unwrap();
        assert!(hits.iter().any(|hit| hit.meeting_id == "ram"));
    }

    #[tokio::test]
    async fn query_cancellation_is_typed_at_every_boundary() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "cancel", "Cancel").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-cancel", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-cancel", "cancel", &["cancellation target"]).await;

        let service = Arc::new(fresh_service());
        publish_tick(&pool, &service).await.unwrap();

        // Pre-cancelled token fails fast without scanning.
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            service
                .search(&[1.0, 0.0, 0.0, 0.0], ScopeFilter::All, 10, &cancelled)
                .await,
            Err(SearchFailure::Cancelled)
        );

        // Waiting for a vector-scan permit honors queued cancellation: with
        // both approved permits held on the shared scheduler, a queued search
        // cancels instead of scanning.
        let keep_alive = CancellationToken::new();
        let held_a = service
            .scheduler()
            .acquire_vector_scan(&keep_alive)
            .await
            .unwrap();
        let held_b = service
            .scheduler()
            .acquire_vector_scan(&keep_alive)
            .await
            .unwrap();

        let query_token = CancellationToken::new();
        let queued_search = tokio::spawn({
            let service = Arc::clone(&service);
            let token = query_token.clone();
            async move {
                service
                    .search(&query_for("target"), ScopeFilter::All, 10, &token)
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        query_token.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), queued_search)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result, Err(SearchFailure::Cancelled));

        // Releasing the permits lets a fresh query through normally.
        drop((held_a, held_b));
        assert!(contains_hit(
            &search_all(&service, "target").await.unwrap(),
            "doc-cancel-0"
        ));
    }

    #[tokio::test]
    async fn query_shape_mismatch_fails_typed_before_any_scan() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "shape", "Shape").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-shape", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-shape", "shape", &["shape target content"]).await;
        let service = Arc::new(fresh_service());
        publish_tick(&pool, &service).await.unwrap();

        // Short and long queries are rejected typed; nothing is scanned and
        // no result is produced.
        let short = service
            .search(
                &[1.0, 0.0, 0.0],
                ScopeFilter::All,
                10,
                &CancellationToken::new(),
            )
            .await;
        assert!(
            matches!(short, Err(SearchFailure::InvalidQuery(reason)) if reason.contains("dimension")),
            "short query must fail typed: {short:?}"
        );
        let long = service
            .search(
                &[1.0, 0.0, 0.0, 0.0, 0.25],
                ScopeFilter::All,
                10,
                &CancellationToken::new(),
            )
            .await;
        assert!(
            matches!(long, Err(SearchFailure::InvalidQuery(reason)) if reason.contains("dimension")),
            "long query must fail typed: {long:?}"
        );

        // Proof no scan occurs: with every vector-scan permit held, a
        // mis-shaped query still fails immediately instead of queueing behind
        // the scanner budget.
        let keep_alive = CancellationToken::new();
        let held_a = service
            .scheduler()
            .acquire_vector_scan(&keep_alive)
            .await
            .unwrap();
        let held_b = service
            .scheduler()
            .acquire_vector_scan(&keep_alive)
            .await
            .unwrap();
        let raced = tokio::time::timeout(
            Duration::from_millis(200),
            service.search(
                &[1.0, 0.0, 0.0],
                ScopeFilter::All,
                10,
                &CancellationToken::new(),
            ),
        )
        .await;
        assert!(
            matches!(raced, Ok(Err(SearchFailure::InvalidQuery(_)))),
            "mis-shaped query must not wait for a scan permit: {raced:?}"
        );
        drop((held_a, held_b));
    }

    #[tokio::test]
    async fn lifecycle_shares_one_index_service_and_shutdown_joins_publication() {
        use crate::retrieval::worker::LifecycleConfig;
        let lifecycle =
            crate::retrieval::worker::RetrievalLifecycle::new(LifecycleConfig::production(None));
        let clone = lifecycle.clone();
        let other =
            crate::retrieval::worker::RetrievalLifecycle::new(LifecycleConfig::production(None));

        assert!(
            Arc::ptr_eq(&lifecycle.index_service(), &clone.index_service()),
            "MCP and Tauri clones must share the same query-index service"
        );
        assert!(!Arc::ptr_eq(
            &lifecycle.index_service(),
            &other.index_service()
        ));

        lifecycle.attach_database(migrated_pool().await);
        assert!(lifecycle.is_running());
        lifecycle.shutdown().await;
        assert!(!lifecycle.is_running());
    }

    #[tokio::test]
    async fn manual_pause_stops_index_work_without_disabling_queries_or_publication() {
        use crate::retrieval::worker::{EngineLoader, LifecycleConfig, PressureProbe};

        let pool = migrated_pool().await;
        insert_meeting(&pool, "paused", "Paused").await;
        add_transcript(&pool, "p1", "paused", "conteudo durante pausa manual").await;

        struct FakeEmbedder;
        impl crate::retrieval::worker::DocumentEmbedder for FakeEmbedder {
            fn model_id(&self) -> String {
                MODEL_ID.to_string()
            }
            fn dimensions(&self) -> usize {
                DIMS
            }
            fn count_tokens(&self, text: &str) -> usize {
                text.split_whitespace().count()
            }
            fn embed_documents_blocking(
                &self,
                texts: &[String],
                _cancel: &CancellationToken,
            ) -> Result<Vec<Vec<f32>>, crate::retrieval::model::RetrievalModelError> {
                Ok(texts.iter().map(|text| vector_for(text)).collect())
            }
        }
        let embedder: Arc<dyn crate::retrieval::worker::DocumentEmbedder> = Arc::new(FakeEmbedder);
        let loader: EngineLoader = {
            let embedder = Arc::clone(&embedder);
            Arc::new(move || Ok(Arc::clone(&embedder)))
        };
        let pressure: PressureProbe = Arc::new(|| false);

        let lifecycle = crate::retrieval::worker::RetrievalLifecycle::new(
            LifecycleConfig::testing(pressure, loader),
        );
        lifecycle.set_index_paused(true);
        lifecycle.attach_database(pool.clone());

        // While paused, no semantic indexing happens.
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM retrieval_documents")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );

        // Resuming lets the worker index, publish, and activate end to end.
        lifecycle.set_index_paused(false);
        let service = lifecycle.index_service();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while service.active_snapshot().is_none() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "activation did not complete after resume"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let hits = service
            .search(
                &query_for("pausa"),
                ScopeFilter::All,
                10,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|hit| hit.meeting_id == "paused"));
        lifecycle.shutdown().await;
    }

    /// Injects a durable acknowledgement failure: every attempt to advance
    /// any published bound aborts until the test drops the trigger.
    async fn fail_acknowledgements(pool: &SqlitePool) {
        sqlx::query(
            "CREATE TRIGGER fail_acknowledgement_until_removed
             BEFORE UPDATE ON retrieval_index_state
             WHEN NEW.published_change_id > OLD.published_change_id
             BEGIN SELECT RAISE(ABORT, 'synthetic acknowledgement failure'); END;",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn publisher_installs_before_acknowledging_and_retries_failed_acks() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Ack").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-ack", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-ack", "m", &["ack order content"]).await;

        // Healthy pass: the generation activates and converges.
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let (baseline_canonical, baseline_published) =
            RetrievalRepository::publication_lag(&pool, "gen-ack")
                .await
                .unwrap()
                .unwrap();
        assert_eq!(baseline_canonical, baseline_published);

        // Inject a durable acknowledgement failure, then commit new work.
        fail_acknowledgements(&pool).await;
        add_transcript(&pool, "t2", "m", "second ack content").await;
        publish_meeting(
            &pool,
            "gen-ack",
            "m",
            &["ack order content", "second ack content"],
        )
        .await;

        // Steady-state replay swaps the new snapshot to readers BEFORE
        // acknowledging durably - so the failed acknowledgement leaves the
        // installed snapshot ahead of, never behind, the durable bound...
        assert!(
            publish_tick(&pool, &service).await.is_err(),
            "the injected acknowledgement failure surfaces as a typed pass error"
        );
        let (_, published) = RetrievalRepository::publication_lag(&pool, "gen-ack")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            published, baseline_published,
            "a failed acknowledgement must not advance the durable bound"
        );
        // The freshly replayed meeting lives in the installed overlay even
        // though its journal change is not acknowledged yet.
        let snapshot = service.active_snapshot().unwrap();
        assert!(snapshot
            .overlay
            .upserted
            .values()
            .flatten()
            .any(|doc| doc.meta.document_id == "doc-m-1"));
        let failure = search_all(&service, "ack").await.unwrap_err();
        assert!(
            matches!(failure, SearchFailure::CatchUpPending { behind } if behind > 0),
            "expected CatchUpPending while unacknowledged, got {failure:?}"
        );

        // ...and once acknowledgements work again, replay resynchronizes
        // WITHOUT duplicating the already-installed journal change.
        sqlx::query("DROP TRIGGER fail_acknowledgement_until_removed")
            .execute(&pool)
            .await
            .unwrap();
        publish_tick(&pool, &service).await.unwrap();
        let (canonical, published) = RetrievalRepository::publication_lag(&pool, "gen-ack")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(published, canonical);
        let snapshot = service.active_snapshot().unwrap();
        assert_eq!(
            snapshot.document_count(),
            2,
            "re-applied installed changes must not duplicate documents"
        );
        let hits = search_all(&service, "second").await.unwrap();
        assert!(
            contains_hit(&hits, "doc-m-1"),
            "the converged snapshot serves the replayed revision"
        );
        assert_eq!(
            hits.len(),
            2,
            "re-applied installed changes must not duplicate documents"
        );
    }

    #[tokio::test]
    async fn deletion_only_tombstone_churn_reaches_the_compaction_threshold() {
        let pool = migrated_pool().await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-tomb", MODEL_ID)
            .await
            .unwrap();
        // 60 meetings x 1 doc = 60 base rows; the 2% threshold is 2 overlay
        // units, reachable by tombstoning alone.
        for index in 0..60 {
            let meeting = format!("m{index}");
            insert_meeting(&pool, &meeting, &meeting).await;
            publish_meeting(&pool, "gen-tomb", &meeting, &[&format!("body {index}")]).await;
        }
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        {
            let state = service.lock_state();
            let snapshot = state.active.as_ref().unwrap();
            assert_eq!(snapshot.base.len(), 60);
            assert_eq!(snapshot.overlay_documents, 0);
        }

        // Delete-only churn: no upserts ever re-enter the overlay.
        for index in 0..2 {
            sqlx::query("DELETE FROM meetings WHERE id = ?")
                .bind(format!("m{index}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        publish_tick(&pool, &service).await.unwrap();

        let state = service.lock_state();
        let snapshot = state.active.as_ref().unwrap();
        assert_eq!(
            (snapshot.overlay_documents, snapshot.overlay.deleted.len()),
            (0, 0),
            "tombstone churn alone must drain through compaction"
        );
        assert_eq!(
            snapshot.base.len(),
            58,
            "deleted base vectors must leave the retained base"
        );
        drop(state);

        // Deleted content cannot leak and survivors keep serving: exactly the
        // 58 undeleted documents remain queryable.
        let hits = service
            .search(
                &query_for("body"),
                ScopeFilter::All,
                MAX_QUERY_LIMIT,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(hits.len(), 58);
        assert!(
            hits.iter()
                .all(|hit| hit.document_id != "doc-m0-0" && hit.document_id != "doc-m1-0"),
            "compacted deleted vectors must not be served"
        );
    }

    #[tokio::test]
    async fn cancelled_publisher_leaves_durable_bounds_and_prior_snapshot_valid() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m1", "First").await;
        insert_meeting(&pool, "m2", "Second").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-cancel-pub", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-cancel-pub", "m1", &["first published"]).await;
        publish_meeting(&pool, "gen-cancel-pub", "m2", &["second meeting body"]).await;

        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let installed = service.active_snapshot().unwrap();

        // Canonical moves ahead of published (crash-style backlog)...
        publish_meeting(
            &pool,
            "gen-cancel-pub",
            "m1",
            &["first published", "extra queued wording"],
        )
        .await;
        let (canonical, published) = RetrievalRepository::publication_lag(&pool, "gen-cancel-pub")
            .await
            .unwrap()
            .unwrap();
        assert!(canonical > published);

        // ...and a cancelled pass performs no work at all: bounds untouched,
        // the prior reader snapshot stays installed and valid.
        let cancel = CancellationToken::new();
        cancel.cancel();
        publish_tick_with(&pool, &service, &cancel).await.unwrap();
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen-cancel-pub")
                .await
                .unwrap(),
            Some((canonical, published))
        );
        let after = service.active_snapshot().unwrap();
        assert!(
            Arc::ptr_eq(&installed, &after),
            "a cancelled pass must not swap the reader snapshot"
        );
        assert_eq!(after.document_count(), 2);
        // While durable state trails the backlog, queries stay typed-paused;
        // the retained snapshot itself remains complete and unchanged.
        let failure = search_all(&service, "first").await.unwrap_err();
        assert!(
            matches!(failure, SearchFailure::CatchUpPending { .. }),
            "expected CatchUpPending while publication lags, got {failure:?}"
        );

        // An uncancelled pass converges normally afterwards.
        publish_tick(&pool, &service).await.unwrap();
        assert!(contains_hit(
            &search_all(&service, "first").await.unwrap(),
            "doc-m1-0"
        ));
        assert!(contains_hit(
            &search_all(&service, "extra").await.unwrap(),
            "doc-m1-1"
        ));
    }

    fn meta_for(document_id: &str, meeting_id: &str, ordinal: i64) -> DocumentMeta {
        DocumentMeta {
            document_id: document_id.to_string(),
            meeting_id: meeting_id.to_string(),
            source_kind: "transcript".to_string(),
            source_start_id: None,
            source_end_id: None,
            source_template_id: None,
            heading: None,
            ordinal,
        }
    }

    #[tokio::test]
    async fn cancelled_after_canonical_read_installs_and_acknowledges_nothing() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Cut").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-cut", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-cut", "m", &["cut boundary content"]).await;
        let bounds = RetrievalRepository::publication_lag(&pool, "gen-cut")
            .await
            .unwrap()
            .unwrap();

        // Complete the canonical read successfully, THEN cancel: the exact
        // race window between load success and install/acknowledgement.
        let cancel = CancellationToken::new();
        let read = RetrievalRepository::read_canonical_snapshot(&pool, "gen-cut", &cancel)
            .await
            .unwrap()
            .unwrap();
        assert!(read.page.rejected.is_empty());
        cancel.cancel();

        let service = fresh_service();
        catch_up_active_generation(&pool, &service, "gen-cut", &cancel)
            .await
            .unwrap();
        assert!(
            service.active_snapshot().is_none(),
            "a cancelled catch-up must not install a snapshot"
        );
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen-cut")
                .await
                .unwrap(),
            Some(bounds),
            "a cancelled catch-up must not acknowledge"
        );
        assert_eq!(
            search_all(&service, "cut").await.unwrap_err(),
            SearchFailure::NoActiveGeneration
        );

        // The subsequent pass converges normally.
        publish_tick(&pool, &service).await.unwrap();
        assert!(contains_hit(
            &search_all(&service, "cut").await.unwrap(),
            "doc-m-0"
        ));
    }

    #[tokio::test]
    async fn cancelled_replay_batch_build_is_discarded_without_swap_or_acknowledgement() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Replay").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-replay-cut", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-replay-cut", "m", &["first body"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let installed = service.active_snapshot().unwrap();

        // Canonical moves ahead of published, then the replay batch builds
        // fully with a live token...
        add_transcript(&pool, "t2", "m", "second body").await;
        publish_meeting(&pool, "gen-replay-cut", "m", &["first body", "second body"]).await;
        let bounds = RetrievalRepository::publication_lag(&pool, "gen-replay-cut")
            .await
            .unwrap()
            .unwrap();
        assert!(bounds.0 > bounds.1);
        let cancel = CancellationToken::new();
        let (candidate, change_id) = apply_journal_batch(&pool, &installed, &cancel)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(candidate.overlay_documents, 2);

        // ...and cancellation fires only before publication: no swap, no ack.
        cancel.cancel();
        let published = publish_replayed_batch(
            &pool,
            &service,
            &installed,
            &Arc::new(candidate),
            change_id,
            &cancel,
        )
        .await
        .unwrap();
        assert!(!published);
        assert!(
            Arc::ptr_eq(&installed, &service.active_snapshot().unwrap()),
            "a cancelled batch must not swap the reader snapshot"
        );
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen-replay-cut")
                .await
                .unwrap(),
            Some(bounds),
            "a cancelled batch must not advance the durable bound"
        );
        // The prior snapshot stays queryable; the discarded candidate's new
        // document never became visible.
        assert_eq!(search_all(&service, "first").await.unwrap().len(), 1);

        // The next pass replays the same batch and converges.
        publish_tick(&pool, &service).await.unwrap();
        assert!(contains_hit(
            &search_all(&service, "second").await.unwrap(),
            "doc-m-1"
        ));
    }

    #[test]
    fn compact_snapshot_observes_cancellation_in_both_blocking_loops() {
        // Base larger than the bounded cancellation cadence so the base-loop
        // guard sits mid-loop, plus overlay churn for the overlay loop.
        let rows = 4200_usize;
        let metas: Vec<DocumentMeta> = (0..rows)
            .map(|row| {
                let meeting = if row == 0 { "gone" } else { "kept" };
                meta_for(&format!("doc-{row}"), meeting, row as i64)
            })
            .collect();
        let overlay_churn = || Overlay {
            upserted: BTreeMap::from([(
                "fresh".to_string(),
                vec![OverlayDoc {
                    meta: meta_for("doc-fresh", "fresh", 0),
                    vector: vec![127_u8; DIMS],
                }],
            )]),
            deleted: BTreeSet::from(["gone".to_string()]),
        };
        let snapshot = IndexSnapshot::new(
            "gen-compact-cut".to_string(),
            MODEL_ID.to_string(),
            DIMS,
            BaseRows {
                metas,
                vectors: vec![127_u8; rows * DIMS],
            },
            overlay_churn(),
        );

        // Live token: the tombstoned meeting drops and the overlay folds in.
        let compacted = compact_snapshot(&snapshot, &CancellationToken::new()).unwrap();
        assert_eq!(compacted.base.len(), rows);
        assert_eq!(compacted.overlay_documents, 0);

        // Cancelled token: the base loop bails before emitting anything.
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(compact_snapshot(&snapshot, &cancelled).is_none());

        // The overlay-loop guard: empty base so the first overlay meeting is
        // where cancellation fires.
        let overlay_only = IndexSnapshot::new(
            "gen-compact-cut".to_string(),
            MODEL_ID.to_string(),
            DIMS,
            BaseRows::default(),
            overlay_churn(),
        );
        assert!(compact_snapshot(&overlay_only, &cancelled).is_none());
    }

    #[tokio::test]
    async fn cancelled_compaction_keeps_the_prior_overlay_snapshot_installed() {
        let service = fresh_service();
        let base_len = 10_usize;
        let snapshot = Arc::new(IndexSnapshot::new(
            "gen-compact-svc".to_string(),
            MODEL_ID.to_string(),
            DIMS,
            BaseRows {
                metas: (0..base_len)
                    .map(|row| meta_for(&format!("doc-{row}"), &format!("m{row}"), row as i64))
                    .collect(),
                vectors: vec![127_u8; base_len * DIMS],
            },
            Overlay {
                upserted: BTreeMap::from([(
                    "m0".to_string(),
                    vec![OverlayDoc {
                        meta: meta_for("doc-m0-new", "m0", 0),
                        vector: vec![127_u8; DIMS],
                    }],
                )]),
                deleted: BTreeSet::from(["m9".to_string()]),
            },
        ));
        service.install_active(Arc::clone(&snapshot));

        // 2 overlay units x 50 >= 10 base rows crosses the approved threshold;
        // a cancelled pass builds nowhere and swaps nothing.
        let cancel = CancellationToken::new();
        cancel.cancel();
        compact_if_needed(&service, &cancel).await;
        assert!(Arc::ptr_eq(&snapshot, &service.active_snapshot().unwrap()));

        // An uncancelled pass compacts and drains the overlay.
        compact_if_needed(&service, &CancellationToken::new()).await;
        let compacted = service.active_snapshot().unwrap();
        assert!(!Arc::ptr_eq(&snapshot, &compacted));
        assert_eq!(compacted.overlay_documents, 0);
        assert_eq!(compacted.document_count(), 9);
    }

    #[tokio::test]
    async fn cancelled_shadow_promotion_flips_no_pointer_state_or_memory() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "one", "One").await;
        insert_meeting(&pool, "two", "Two").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-old", MODEL_ID)
            .await
            .unwrap();
        publish_meeting(&pool, "gen-old", "one", &["one content"]).await;
        publish_meeting(&pool, "gen-old", "two", &["two content"]).await;
        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let active_before = service.active_snapshot().unwrap();

        // Complete shadow build WITHOUT ticking: a fully prepared candidate
        // whose promotion is still pending.
        let shadow = request_rebuild(&pool).await.unwrap();
        publish_meeting(&pool, &shadow, "one", &["one rebuilt"]).await;
        publish_meeting(&pool, &shadow, "two", &["two rebuilt"]).await;
        let read =
            RetrievalRepository::read_canonical_snapshot(&pool, &shadow, &CancellationToken::new())
                .await
                .unwrap()
                .unwrap();
        let caught_up_to = read.canonical_change_id;
        let shadow_bounds = RetrievalRepository::publication_lag(&pool, &shadow)
            .await
            .unwrap()
            .unwrap();

        // Cancellation between validation and promotion: no readiness flip,
        // no pointer move, no memory swap, no acknowledgement.
        let cancel = CancellationToken::new();
        cancel.cancel();
        let promoted = promote_shadow_generation(
            &pool,
            &service,
            &shadow,
            base_snapshot(&shadow, read.clone()),
            caught_up_to,
            &cancel,
        )
        .await
        .unwrap();
        assert!(!promoted);
        assert_eq!(
            RetrievalRepository::active_generation_id(&pool)
                .await
                .unwrap(),
            Some("gen-old".to_string())
        );
        assert!(Arc::ptr_eq(
            &active_before,
            &service.active_snapshot().unwrap()
        ));
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, &shadow)
                .await
                .unwrap(),
            Some(shadow_bounds)
        );
        let (state,): (String,) =
            sqlx::query_as("SELECT state FROM retrieval_generations WHERE generation_id = ?")
                .bind(&shadow)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "building");

        // A live token promotes through the approved order: readiness, then
        // pointer + memory swap together, then the durable acknowledgement.
        let promoted = promote_shadow_generation(
            &pool,
            &service,
            &shadow,
            base_snapshot(&shadow, read),
            caught_up_to,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(promoted);
        assert_eq!(
            RetrievalRepository::active_generation_id(&pool)
                .await
                .unwrap(),
            Some(shadow.clone())
        );
        assert_eq!(service.active_snapshot().unwrap().generation_id(), shadow);
        assert!(contains_hit(
            &search_all(&service, "one").await.unwrap(),
            "doc-one-0"
        ));
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, &shadow)
                .await
                .unwrap(),
            Some((caught_up_to, caught_up_to)),
            "the acknowledged bound advances only after installation"
        );
        let (old_state,): (String,) = sqlx::query_as(
            "SELECT state FROM retrieval_generations WHERE generation_id = 'gen-old'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(old_state, "retired");
    }

    #[tokio::test]
    async fn heading_provenance_survives_publication_and_reaches_hits() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Headed").await;
        register_test_model(&pool).await;
        RetrievalRepository::ensure_generation(&pool, "gen-head", MODEL_ID)
            .await
            .unwrap();
        let revision = current_revision(&pool, "m").await;
        let mut document = StagedDocument {
            document_id: "doc-headed-0".to_string(),
            source_kind: "summary".to_string(),
            source_start_id: None,
            source_end_id: None,
            source_template_id: Some("tpl".to_string()),
            heading: Some("Decisões finais".to_string()),
            ordinal: 0,
            content: "headed body text".to_string(),
            content_hash: vec![7; 32],
            dimensions: DIMS as i64,
            vector_encoding: VectorEncoding::Int8,
            vector: quantize_int8(&vector_for("headed body text")).unwrap(),
        };
        document.vector = quantize_int8(&vector_for(&document.content)).unwrap();
        RetrievalRepository::stage_documents(
            &pool,
            &format!("job-gen-head-m-{revision}"),
            "gen-head",
            "m",
            revision,
            &[document],
        )
        .await
        .unwrap();
        assert!(matches!(
            RetrievalRepository::replace_meeting_documents(
                &pool,
                ReplacementJob {
                    generation_id: "gen-head",
                    meeting_id: "m",
                    expected_source_revision: revision,
                    job_id: &format!("job-gen-head-m-{revision}"),
                },
            )
            .await
            .unwrap(),
            ReplacementOutcome::Published { .. }
        ));

        let service = fresh_service();
        publish_tick(&pool, &service).await.unwrap();
        let hits = search_all(&service, "headed").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].heading.as_deref(), Some("Decisões finais"));
    }
}
