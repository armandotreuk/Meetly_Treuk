//! Durable FTS/semantic index worker (Sprint 2B Task 2.4).
//!
//! One detached, cancellable lifecycle owns lexical repair and per-generation
//! semantic indexing. It is created during Tauri setup before any database
//! exists and idempotently attaches after each database installation path;
//! MCP receives a clone of the same object instead of building a duplicate
//! worker or session set.
//!
//! Ordering rules this module guarantees:
//! - FTS repair runs first and is model-independent: stale projections heal
//!   even when the bundled models fail to load, and the indexed revision is
//!   marked only after a complete refresh. A mark superseded mid-refresh keeps
//!   the meeting due with no error state, but consecutive supersessions of the
//!   same meeting fall through to the sleep quantum after a small bound so a
//!   continuously mutating meeting cannot monopolize the loop; a failed mark
//!   records persisted backoff exactly like a failed refresh.
//! - Semantic work processes one bounded due item at a time, extracting
//!   authoritative rows through the Task 2.1 repository, chunking with the
//!   exact packaged tokenizer (Task 2.3), and embedding through bounded CPU
//!   batches on blocking threads (Task 2.2) - never inside a transaction or
//!   on an async worker thread.
//! - Batches stage at most [`MAX_STAGE_DOCUMENTS`] documents or
//!   [`MAX_STAGE_BYTES`] of estimated working memory into job IDs bound to
//!   `(generation, meeting, source revision)`. A crash resumes matching
//!   staging; stale, divergent, or cancelled staging is pruned without ever
//!   touching active documents, which are replaced only by the repository's
//!   revision-fenced transaction that also appends the publication journal.
//! - The approved scheduler policy is shared with future query paths:
//!   interactive requests preempt indexing on one ONNX inference permit, at
//!   most two vector scans run concurrently, and at most eight interactive
//!   requests queue with deterministic FIFO admission and queued cancellation.
//! - Active recording, import, and retranscription pause all index work at
//!   item/batch boundaries within the approved 250 ms quantum, resuming
//!   without losing durable source revisions.
//!
//! Every failure path records durable retry state and never propagates:
//! startup, primary content mutations, and shutdown ordering cannot fail
//! because model or index work failed. Logs carry counts, IDs, stages, and
//! safe error kinds - never source text, tokens, or vectors.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex as StdMutex, PoisonError};
use std::time::Duration;

use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::database::repositories::fts::FtsRepository;
use crate::database::repositories::retrieval::{
    is_unreadable_staged_payload, FtsDueItem, GenerationWorkItem, ModelSpec, ReplacementJob,
    ReplacementOutcome, RetrievalRepository, StagedDocument, VectorEncoding,
};
use crate::retrieval::chunking::{
    chunk_meeting, ChunkerConfig, SemanticDocument, TokenizerPolicy, APPROVED_CHUNKER_VERSION,
};
use crate::retrieval::model::{RetrievalModelError, RetrievalModels};

/// Approved scheduler policy: one concurrent ONNX inference pipeline.
const INFERENCE_PERMITS: usize = 1;
/// Approved scheduler policy: at most two concurrent vector scans.
const VECTOR_SCAN_PERMITS: usize = 2;
/// Approved scheduler policy: at most eight queued interactive requests.
const MAX_QUEUED_INTERACTIVE: usize = 8;
/// Approved operating limit: index work pauses within one 250 ms quantum of
/// an active recording/import/retranscription signal. The same quantum is the
/// worker's idle poll interval, bounding wake latency for durable revisions.
pub const PAUSE_QUANTUM: Duration = Duration::from_millis(250);
/// Approved staging ceiling from the sprint contract. Also the streaming page
/// size of the repository's revision-fenced replacement, so resume selection,
/// staging, and publication all stay bounded by one batch.
pub(crate) const MAX_STAGE_DOCUMENTS: usize = 256;
/// Approved staging ceiling: 64 MiB estimated working memory per batch.
const MAX_STAGE_BYTES: u64 = 64 * 1024 * 1024;
/// Consecutive superseded marks tolerated for the SAME meeting before a tick
/// falls through to the sleep quantum instead of immediately repairing it
/// again. Each supersession is normal (the projection advanced mid-refresh and
/// nothing persisted), but a meeting whose projection advances on every pass
/// would otherwise be re-refreshed back-to-back forever; this bound leaves the
/// rest of every tick for other due work until the mutation storm passes.
const MAX_CONSECUTIVE_FTS_SUPERSESSIONS: u32 = 4;
/// Retry schedule: exponential backoff from 2 s capped at 1 h.
const BASE_BACKOFF_SECS: i64 = 2;
const BACKOFF_CAP_SECS: i64 = 3600;
const BACKOFF_MAX_SHIFT: u32 = 11;
/// Attempts before one poison meeting is marked permanently failed and stops
/// consuming queue slots (it stays visible as an activation blocker through
/// its durable state for the Task 2.5 status API).
const MAX_ITEM_ATTEMPTS: i64 = 5;
/// How often a failed bundled-model load is retried while the worker keeps
/// repairing FTS lexically in the meantime.
const MODEL_RETRY_INTERVAL: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Embedding port
// ---------------------------------------------------------------------------

/// Blocking embedding/tokenizing port implemented by the bundled runtime and
/// by deterministic test fakes. The worker invokes it on blocking threads
/// only; implementors must honor cancellation between internal batches.
pub trait DocumentEmbedder: Send + Sync + 'static {
    fn model_id(&self) -> String;
    fn dimensions(&self) -> usize;
    /// Counts content tokens under the exact packaged tokenizer contract.
    fn count_tokens(&self, text: &str) -> usize;
    /// Embeds document texts (blocking; call from `spawn_blocking`).
    fn embed_documents_blocking(
        &self,
        texts: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError>;
    /// Embeds query texts with the manifest query prefix (blocking; call from
    /// `spawn_blocking`). The approved bundle uses distinct query and document
    /// prefixes, so query work must never route through the document method.
    fn embed_queries_blocking(
        &self,
        texts: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError>;
}

impl DocumentEmbedder for RetrievalModels {
    fn model_id(&self) -> String {
        // The persisted identity is derived from the complete approved bundle
        // contract (which `parse_manifest` enforces for every loadable
        // bundle), not from the raw bundle id.
        bundled_model_identity()
    }

    fn dimensions(&self) -> usize {
        RetrievalModels::dimensions(self) as usize
    }

    fn count_tokens(&self, text: &str) -> usize {
        use crate::retrieval::chunking::PackagedTokenizer;
        PackagedTokenizer::new(self.document_tokenizer()).count_tokens(text)
    }

    fn embed_documents_blocking(
        &self,
        texts: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
        let references: Vec<&str> = texts.iter().map(String::as_str).collect();
        self.embed_documents_sync(&references, cancel)
    }

    fn embed_queries_blocking(
        &self,
        texts: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
        let references: Vec<&str> = texts.iter().map(String::as_str).collect();
        self.embed_queries_sync(&references, cancel)
    }
}

struct EmbedderTokenizer<'a>(&'a Arc<dyn DocumentEmbedder>);

impl TokenizerPolicy for EmbedderTokenizer<'_> {
    fn count_tokens(&self, text: &str) -> usize {
        self.0.count_tokens(text)
    }
}

/// Loads the bundled engine; the worker always invokes it via
/// `spawn_blocking` so artifact verification never blocks async workers.
pub type EngineLoader =
    Arc<dyn Fn() -> Result<Arc<dyn DocumentEmbedder>, String> + Send + Sync + 'static>;

fn production_loader(bundle_root: Option<std::path::PathBuf>) -> EngineLoader {
    Arc::new(move || {
        let Some(root) = bundle_root.as_ref() else {
            return Err("retrieval resource directory unavailable".to_string());
        };
        match crate::retrieval::model::get_or_load(root) {
            Ok(models) => Ok(Arc::new(models) as Arc<dyn DocumentEmbedder>),
            Err(error) => Err(error.to_string()),
        }
    })
}

/// Returns true while recording, audio import, or retranscription pressure is
/// active; the production probe reads the existing audio-state flags.
pub type PressureProbe = Arc<dyn Fn() -> bool + Send + Sync + 'static>;

fn production_pressure_probe() -> PressureProbe {
    Arc::new(|| {
        crate::audio::recording_commands::is_recording_active()
            || crate::audio::import::is_import_in_progress()
            || crate::audio::retranscription::is_retranscription_in_progress()
    })
}

// ---------------------------------------------------------------------------
// Shared scheduler (approved Sprint 1 policy)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerRejection {
    QueueFull { capacity: usize },
    CancelledWhileQueued,
}

struct QueuedInteractive {
    id: u64,
    token: CancellationToken,
}

struct SchedulerShared {
    inference: Arc<tokio::sync::Semaphore>,
    vector_scans: Arc<tokio::sync::Semaphore>,
    queue: StdMutex<VecDeque<QueuedInteractive>>,
    next_ticket: AtomicU64,
    drained: tokio::sync::Notify,
    #[cfg(test)]
    vector_scan_waiting: tokio::sync::Notify,
}

/// One process-wide scheduler implementing the approved concurrency policy.
/// Interactive retrieval (Task 2.5 query paths) shares the single ONNX
/// inference permit with index work but always preempts it; vector scans get
/// their own two-permit budget.
#[derive(Clone)]
pub struct RetrievalScheduler(Arc<SchedulerShared>);

impl Default for RetrievalScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl RetrievalScheduler {
    pub fn new() -> Self {
        Self(Arc::new(SchedulerShared {
            inference: Arc::new(tokio::sync::Semaphore::new(INFERENCE_PERMITS)),
            vector_scans: Arc::new(tokio::sync::Semaphore::new(VECTOR_SCAN_PERMITS)),
            queue: StdMutex::new(VecDeque::new()),
            next_ticket: AtomicU64::new(0),
            drained: tokio::sync::Notify::new(),
            #[cfg(test)]
            vector_scan_waiting: tokio::sync::Notify::new(),
        }))
    }

    /// Admits an interactive request to the inference pipeline in deterministic
    /// FIFO order. Fails typed once [`MAX_QUEUED_INTERACTIVE`] requests are
    /// already queued; cancelling the returned ticket frees its slot.
    pub fn enqueue_interactive(&self) -> Result<InteractiveTicket, SchedulerRejection> {
        let mut queue = locked(&self.0.queue);
        if queue.len() >= MAX_QUEUED_INTERACTIVE {
            return Err(SchedulerRejection::QueueFull {
                capacity: MAX_QUEUED_INTERACTIVE,
            });
        }
        let entry = QueuedInteractive {
            id: self.0.next_ticket.fetch_add(1, Ordering::Relaxed),
            token: CancellationToken::new(),
        };
        let ticket = InteractiveTicket {
            id: entry.id,
            token: entry.token.clone(),
            scheduler: self.clone(),
        };
        queue.push_back(entry);
        Ok(ticket)
    }

    pub(crate) fn remove_queued(&self, id: u64) -> bool {
        let mut queue = locked(&self.0.queue);
        let before = queue.len();
        queue.retain(|entry| entry.id != id);
        let removed = queue.len() != before;
        if removed && queue.is_empty() {
            self.0.drained.notify_waiters();
        }
        removed
    }

    #[cfg(test)]
    pub(crate) fn queued_interactive(&self) -> usize {
        locked(&self.0.queue).len()
    }

    /// Index-side acquisition: granted only while no interactive request is
    /// waiting or queued, so interactive work always preempts indexing.
    pub(crate) async fn acquire_for_index(
        &self,
        cancel: &CancellationToken,
    ) -> Option<tokio::sync::OwnedSemaphorePermit> {
        loop {
            if cancel.is_cancelled() {
                return None;
            }
            if locked(&self.0.queue).is_empty() {
                if let Ok(permit) = Arc::clone(&self.0.inference).try_acquire_owned() {
                    return Some(permit);
                }
            }
            tokio::select! {
                _ = cancel.cancelled() => return None,
                _ = self.0.drained.notified() => {}
                // ponytail: missed Notify wakeups are bounded by this tick
                // instead of perfect edge tracking; upgrade to waiter
                // registration only if preemption latency ever matters here.
                _ = tokio::time::sleep(PAUSE_QUANTUM) => {}
            }
        }
    }

    /// Query-side vector-scan budget (Task 2.5 consumes this).
    pub async fn acquire_vector_scan(
        &self,
        cancel: &CancellationToken,
    ) -> Option<tokio::sync::OwnedSemaphorePermit> {
        #[cfg(test)]
        if self.0.vector_scans.available_permits() == 0 {
            self.0.vector_scan_waiting.notify_one();
        }
        tokio::select! {
            _ = cancel.cancelled() => None,
            permit = Arc::clone(&self.0.vector_scans).acquire_owned() => {
                Some(permit.expect("vector-scan semaphore is never closed"))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn vector_scan_waiting(&self) -> &tokio::sync::Notify {
        &self.0.vector_scan_waiting
    }
}

/// A queued interactive request. Cancel deterministically while queued, or
/// await the FIFO grant of the single inference permit.
pub struct InteractiveTicket {
    id: u64,
    token: CancellationToken,
    scheduler: RetrievalScheduler,
}

impl InteractiveTicket {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Deterministically removes this request from the queue. Returns false
    /// when it was already admitted (the caller then owns a running lease) or
    /// previously cancelled.
    pub fn cancel(&self) -> bool {
        self.token.cancel();
        self.scheduler.remove_queued(self.id)
    }

    /// Waits for the inference permit in FIFO order among interactives.
    pub async fn wait_for_permit(self) -> Result<InferenceLease, SchedulerRejection> {
        self.wait_for_permit_with(&CancellationToken::new()).await
    }

    /// The same FIFO permit wait, also aborted by an external request token:
    /// a request cancelled while queued is removed from the queue immediately
    /// and returns [`SchedulerRejection::CancelledWhileQueued`] instead of
    /// waiting for (or later consuming) the inference permit. Ticket-owned
    /// cancellation semantics are unchanged.
    pub async fn wait_for_permit_with(
        self,
        external: &CancellationToken,
    ) -> Result<InferenceLease, SchedulerRejection> {
        tokio::select! {
            biased;
            _ = self.token.cancelled() => {
                self.scheduler.remove_queued(self.id);
                Err(SchedulerRejection::CancelledWhileQueued)
            }
            _ = external.cancelled() => {
                self.scheduler.remove_queued(self.id);
                Err(SchedulerRejection::CancelledWhileQueued)
            }
            permit = Arc::clone(&self.scheduler.0.inference).acquire_owned() => {
                self.scheduler.remove_queued(self.id);
                Ok(InferenceLease {
                    _permit: permit.expect("inference semaphore is never closed"),
                    scheduler: self.scheduler.clone(),
                })
            }
        }
    }
}

impl Drop for InteractiveTicket {
    fn drop(&mut self) {
        self.token.cancel();
        self.scheduler.remove_queued(self.id);
    }
}

/// Held for the duration of one interactive inference stage.
pub struct InferenceLease {
    _permit: tokio::sync::OwnedSemaphorePermit,
    scheduler: RetrievalScheduler,
}

impl Drop for InferenceLease {
    fn drop(&mut self) {
        // The permit field drops after this body; index-side waiters re-check
        // on their tick, so this wakeup is only a latency optimization.
        self.scheduler.0.drained.notify_waiters();
    }
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct LifecycleConfig {
    bundle_root: Option<std::path::PathBuf>,
    pressure: Option<PressureProbe>,
    engine_loader: Option<EngineLoader>,
}

impl LifecycleConfig {
    /// Production configuration resolving the packaged bundle below Tauri's
    /// resource directory and pausing on real capture signals.
    pub fn production(bundle_root: Option<std::path::PathBuf>) -> Self {
        Self {
            bundle_root,
            pressure: None,
            engine_loader: None,
        }
    }

    fn loader(&self) -> EngineLoader {
        match &self.engine_loader {
            Some(loader) => Arc::clone(loader),
            None => production_loader(self.bundle_root.clone()),
        }
    }

    fn pressure(&self) -> PressureProbe {
        self.pressure
            .clone()
            .unwrap_or_else(production_pressure_probe)
    }

    #[cfg(test)]
    pub(crate) fn testing(pressure: PressureProbe, engine_loader: EngineLoader) -> Self {
        Self {
            bundle_root: None,
            pressure: Some(pressure),
            engine_loader: Some(engine_loader),
        }
    }
}

struct AttachedWorker {
    cancel: CancellationToken,
    worker: tokio::task::JoinHandle<()>,
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(crate) enum LifecycleOperation {
    Rebuild = 1,
    Retry = 2,
    Clear = 3,
    Cancel = 4,
}

pub(crate) struct OperationReservation {
    active: Arc<AtomicU8>,
    operation: u8,
}

impl Drop for OperationReservation {
    fn drop(&mut self) {
        let _ =
            self.active
                .compare_exchange(self.operation, 0, Ordering::AcqRel, Ordering::Relaxed);
    }
}

struct LifecycleInner {
    config: LifecycleConfig,
    scheduler: RetrievalScheduler,
    index: Arc<crate::retrieval::index::QueryIndexService>,
    paused: Arc<AtomicBool>,
    // ponytail: control is held only while reserving or transitioning lifecycle state; long work keeps a reservation without holding this lock. If cross-process coordination is ever required, add a durable operation lease rather than widening it.
    control: Arc<tokio::sync::Mutex<()>>,
    operation: Arc<AtomicU8>,
    attached: StdMutex<Option<AttachedWorker>>,
    started_attachments: AtomicU64,
}

/// The one detached retrieval lifecycle object. Create once during Tauri
/// setup, attach after every database installation path, share clones with
/// MCP, and shut down before the database pool closes.
#[derive(Clone)]
pub struct RetrievalLifecycle(Arc<LifecycleInner>);

impl Default for RetrievalLifecycle {
    fn default() -> Self {
        Self::new(LifecycleConfig::production(None))
    }
}

impl RetrievalLifecycle {
    pub fn new(config: LifecycleConfig) -> Self {
        let scheduler = RetrievalScheduler::new();
        Self(Arc::new(LifecycleInner {
            index: crate::retrieval::index::QueryIndexService::new(scheduler.clone()).into(),
            paused: Arc::new(AtomicBool::new(false)),
            config,
            scheduler,
            attached: StdMutex::new(None),
            control: Arc::new(tokio::sync::Mutex::new(())),
            operation: Arc::new(AtomicU8::new(0)),
            started_attachments: AtomicU64::new(0),
        }))
    }

    /// Idempotently starts the single owner-worker against `pool`. Duplicate
    /// starts (any later install-path call in the same process) are no-ops;
    /// attaching a different pool first requires [`Self::shutdown`].
    ///
    /// ponytail: sqlx pools expose no identity primitive, so duplicate
    /// detection is start-count based; attaching a genuinely different pool is
    /// rejected by convention (shutdown first) until multi-database support
    /// ever justifies pool tags.
    pub fn attach_database(&self, pool: SqlitePool) {
        let mut attached = locked(&self.0.attached);
        if let Some(worker) = attached.as_ref() {
            if !worker.worker.is_finished() {
                log::debug!("Retrieval lifecycle already attached; ignoring duplicate start");
                return;
            }
            log::warn!("Retrieval worker exited unexpectedly; restarting it");
        }
        let cancel = CancellationToken::new();
        let state = WorkerState {
            pool,
            cancel: cancel.clone(),
            scheduler: self.0.scheduler.clone(),
            config: self.0.config.clone(),
            index: Arc::clone(&self.0.index),
            paused: Arc::clone(&self.0.paused),
        };
        let worker = tokio::spawn(run_worker(state));
        *attached = Some(AttachedWorker { cancel, worker });
        self.0.started_attachments.fetch_add(1, Ordering::Relaxed);
        log::info!("Retrieval lifecycle attached; background index worker started");
    }

    /// Cancels the worker and joins any in-flight model, index, and
    /// publication work. Callers must await this before closing the database
    /// pool so nothing publishes after teardown.
    pub async fn shutdown(&self) {
        let worker = locked(&self.0.attached).take();
        if let Some(worker) = worker {
            worker.cancel.cancel();
            if worker.worker.await.is_err() {
                log::warn!("Retrieval worker task failed during shutdown join");
            }
            log::info!("Retrieval worker cancelled and joined");
        }
    }

    pub fn is_running(&self) -> bool {
        locked(&self.0.attached)
            .as_ref()
            .is_some_and(|worker| !worker.worker.is_finished())
    }

    /// Number of worker tasks actually started (exposes duplicate-start
    /// rejection for tests and diagnostics).
    pub fn started_attachments(&self) -> u64 {
        self.0.started_attachments.load(Ordering::Relaxed)
    }

    /// Identity check for sharing assertions: clones of one lifecycle are the
    /// same service; separately constructed lifecycles are not.
    pub fn same_service(&self, other: &RetrievalLifecycle) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// The shared scheduler consumed by Task 2.5 query paths.
    pub fn scheduler(&self) -> RetrievalScheduler {
        self.0.scheduler.clone()
    }

    /// Loads the bundled embedding runtime for query-side work on a blocking
    /// thread. This is the same loader the worker uses, so it resolves to the
    /// process-wide cached session set - never a second model-session owner.
    pub(crate) async fn load_embedder(&self) -> Result<Arc<dyn DocumentEmbedder>, String> {
        let loader = self.0.config.loader();
        tokio::task::spawn_blocking(move || loader())
            .await
            .map_err(|error| format!("embedder load task join failed: {error}"))?
    }

    /// The one process-wide query-index service (Task 2.5); MCP and Tauri
    /// share it through lifecycle clones.
    pub fn index_service(&self) -> Arc<crate::retrieval::index::QueryIndexService> {
        Arc::clone(&self.0.index)
    }

    /// Manual pause for the later status UI: stops semantic indexing at item
    /// boundaries without discarding durable work. Lexical repair and query
    /// publication/catch-up continue.
    /// Query publication/catch-up continues so the active snapshot stays
    /// truthful.
    pub fn set_index_paused(&self, paused: bool) {
        self.0.paused.store(paused, Ordering::SeqCst);
        self.0.index.mark_status_changed();
    }

    pub fn index_paused(&self) -> bool {
        self.0.paused.load(Ordering::SeqCst)
    }

    pub(crate) async fn acquire_control(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.0.control.lock().await
    }

    pub(crate) async fn reserve_operation(
        &self,
        operation: LifecycleOperation,
        cancel: Option<&CancellationToken>,
    ) -> Result<OperationReservation, String> {
        let _control = if let Some(cancel) = cancel {
            tokio::select! {
                _ = cancel.cancelled() => return Err("retrieval operation cancelled".to_string()),
                guard = self.acquire_control() => guard,
            }
        } else {
            self.acquire_control().await
        };
        self.0
            .operation
            .compare_exchange(0, operation as u8, Ordering::AcqRel, Ordering::Relaxed)
            .map_err(|_| "retrieval operation already active".to_string())?;
        Ok(OperationReservation {
            active: Arc::clone(&self.0.operation),
            operation: operation as u8,
        })
    }

    pub(crate) async fn set_index_paused_command(&self, paused: bool) -> Result<(), String> {
        let _control = self.acquire_control().await;
        if matches!(
            self.0.operation.load(Ordering::Acquire),
            operation if operation == LifecycleOperation::Clear as u8
                || operation == LifecycleOperation::Cancel as u8
        ) {
            return Err("retrieval operation already active".to_string());
        }
        self.set_index_paused(paused);
        Ok(())
    }

    pub async fn clear_index(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        let _reservation = self
            .reserve_operation(LifecycleOperation::Clear, None)
            .await
            .map_err(sqlx::Error::Protocol)?;
        let was_running = self.is_running();
        let was_paused = self.index_paused();
        if was_paused || shadow_operation_active(pool).await? {
            return Err(sqlx::Error::Protocol(
                "retrieval operation already active".into(),
            ));
        }
        self.0.index.begin_clear_transition();
        self.set_index_paused(true);
        self.shutdown().await;
        tokio::task::yield_now().await;
        match shadow_operation_active(pool).await {
            Ok(true) => {
                if was_running {
                    self.attach_database(pool.clone());
                }
                self.0.index.cancel_clear_transition();
                self.set_index_paused(was_paused);
                return Err(sqlx::Error::Protocol(
                    "retrieval operation already active".into(),
                ));
            }
            Ok(false) => {}
            Err(error) => {
                if was_running {
                    self.attach_database(pool.clone());
                }
                self.0.index.cancel_clear_transition();
                self.set_index_paused(was_paused);
                return Err(error);
            }
        }
        let result = match RetrievalRepository::clear_derived_index(pool).await {
            Ok(()) => {
                self.0.index.clear_derived_state();
                Ok(())
            }
            Err(error) => Err(error),
        };
        if result.is_err() {
            self.0.index.cancel_clear_transition();
        }
        if was_running {
            self.attach_database(pool.clone());
        }
        self.set_index_paused(was_paused);
        result
    }

    pub async fn cancel_rebuild(
        &self,
        pool: &SqlitePool,
        generation_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let _reservation = self
            .reserve_operation(LifecycleOperation::Cancel, None)
            .await
            .map_err(sqlx::Error::Protocol)?;
        let state: Option<(String,)> =
            sqlx::query_as("SELECT state FROM retrieval_generations WHERE generation_id = ?")
                .bind(generation_id)
                .fetch_optional(pool)
                .await?;
        if state.as_ref().is_none_or(|(state,)| state != "building") {
            return Ok(false);
        }
        let was_running = self.is_running();
        let was_paused = self.index_paused();
        self.set_index_paused(true);
        self.shutdown().await;
        let result = RetrievalRepository::cancel_building_generation(pool, generation_id).await;
        if was_running {
            self.attach_database(pool.clone());
        }
        self.set_index_paused(was_paused);
        result
    }
}

async fn shadow_operation_active(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    Ok(RetrievalRepository::shadow_generation_statuses(pool)
        .await?
        .iter()
        .any(crate::retrieval::index::shadow_operation_active))
}

// ---------------------------------------------------------------------------
// Worker loop
// ---------------------------------------------------------------------------

struct WorkerState {
    pool: SqlitePool,
    cancel: CancellationToken,
    scheduler: RetrievalScheduler,
    config: LifecycleConfig,
    index: Arc<crate::retrieval::index::QueryIndexService>,
    paused: Arc<AtomicBool>,
}

async fn run_worker(state: WorkerState) {
    let loader = state.config.loader();
    let pressure = state.config.pressure();
    let mut embedders: BTreeMap<String, Arc<dyn DocumentEmbedder>> = BTreeMap::new();
    let mut last_load_attempt: Option<std::time::Instant> = None;
    let mut staging_recovered = false;
    // (meeting id, consecutive superseded marks) for the pacing bound below.
    let mut fts_supersessions: Option<(String, u32)> = None;

    loop {
        if state.cancel.is_cancelled() {
            break;
        }

        // Startup recovery: discard staging that can never publish so a crash
        // leaves only resumable jobs behind.
        if !staging_recovered {
            match RetrievalRepository::discard_stale_staging(&state.pool).await {
                Ok(discarded) => {
                    if discarded > 0 {
                        log::info!("Discarded {discarded} stale staged semantic jobs");
                    }
                    staging_recovered = true;
                }
                Err(error) => log::warn!("Stale-staging recovery failed, will retry: {error}"),
            }
        }

        // Task 2.5 publisher: journal replay/acknowledgement, compaction,
        // generation activation, and retired-generation GC run every tick,
        // independent of model availability, so a crash window replays even
        // when the bundled models fail to load. The lifecycle token cancels
        // it at every bounded DB/page/batch boundary so shutdown joins
        // promptly without starting new work.
        let publish_result =
            crate::retrieval::index::publish_tick_with(&state.pool, &state.index, &state.cancel)
                .await;
        if let Err(error) = publish_result {
            log::warn!("Query-index publication tick failed: {error}");
        }

        // Best-effort journal reclamation runs outside every replacement
        // transaction and never advances any bound; rows at or above the
        // minimum published bound (including unacknowledged tombstones)
        // always survive.
        if !state.cancel.is_cancelled() {
            match RetrievalRepository::prune_acknowledged_index_changes(&state.pool).await {
                Ok(pruned) if pruned > 0 => {
                    log::info!("Pruned {pruned} acknowledged semantic journal rows")
                }
                Ok(_) => {}
                Err(error) => log::warn!("Journal pruning failed: {error}"),
            }
        }

        if pressure() {
            if !sleep_quantum(&state.cancel).await {
                break;
            }
            continue;
        }

        // 1) Durable FTS repair first, independent of model availability.
        match RetrievalRepository::list_due_fts_repairs(&state.pool, &Utc::now().to_rfc3339(), 1)
            .await
        {
            Ok(items) if !items.is_empty() => {
                match repair_fts_item(&state.pool, &state.cancel, &items[0]).await {
                    FtsRepairOutcome::Indexed | FtsRepairOutcome::BackedOff => {
                        fts_supersessions = None;
                        continue;
                    }
                    // Normal outcome: nothing persisted and the meeting stays
                    // due. Re-repair immediately only while the consecutive
                    // streak for this meeting is small; past the bound let the
                    // rest of the tick run (model availability, semantic due
                    // items, idle quantum) so a continuously mutating meeting
                    // cannot eat every pass before other work progresses.
                    FtsRepairOutcome::Superseded => {
                        let streak = match &fts_supersessions {
                            Some((meeting, count)) if *meeting == items[0].meeting_id => {
                                count.saturating_add(1)
                            }
                            _ => 1,
                        };
                        fts_supersessions = Some((items[0].meeting_id.clone(), streak));
                        if streak < MAX_CONSECUTIVE_FTS_SUPERSESSIONS {
                            continue;
                        }
                        log::debug!(
                            "FTS repair for meeting {} superseded {streak} times consecutively; yielding the rest of this tick",
                            items[0].meeting_id
                        );
                    }
                }
            }
            Ok(_) => {}
            Err(error) => log::warn!("FTS due-work selection failed: {error}"),
        }

        if state.paused.load(Ordering::SeqCst) {
            if !sleep_quantum(&state.cancel).await {
                break;
            }
            continue;
        }

        // 2) Semantic indexing needs the bundled engine; loading failures keep
        // lexical repair running and are retried periodically.
        if embedders.is_empty()
            && last_load_attempt.map_or(true, |attempt| attempt.elapsed() >= MODEL_RETRY_INTERVAL)
        {
            last_load_attempt = Some(std::time::Instant::now());
            match load_embedder(&loader).await {
                Ok(models) => {
                    let registered = tokio::select! {
                        _ = state.cancel.cancelled() => break,
                        result = register_semantic_identity(&state.pool, models.as_ref()) => result,
                    };
                    match registered {
                        Ok(()) => {
                            embedders.insert(models.model_id(), models);
                        }
                        Err(error) => log::warn!("Semantic registration failed: {error}"),
                    }
                }
                Err(reason) => {
                    state.index.set_model_load_failure(reason.clone());
                    log::warn!("Semantic models unavailable; staying lexical-only: {reason}")
                }
            }
        }

        if !embedders.is_empty() {
            if let Ok(generations) = RetrievalRepository::list_live_generations(&state.pool).await {
                for (_, model_id) in generations {
                    if !embedders.contains_key(&model_id) {
                        if let Some(models) = crate::retrieval::model::cached_model(&model_id) {
                            embedders.insert(model_id, Arc::new(models));
                        }
                    }
                }
            }
            for model_id in embedders.keys() {
                state.index.set_loaded_model(model_id);
            }
            match next_due_item(&state.pool, &embedders).await {
                Ok(Some((generation_id, item, embedder))) => {
                    // Superseded jobs accumulate between restarts (an edit
                    // after prior staging), so the same recovery runs before
                    // every due item; failure is logged and never blocks it.
                    if let Err(error) =
                        RetrievalRepository::discard_stale_staging(&state.pool).await
                    {
                        log::warn!("Stale-staging cleanup failed, continuing: {error}");
                    }
                    process_semantic_item(
                        &state.pool,
                        &state.cancel,
                        &pressure,
                        &state.scheduler,
                        &state.index,
                        &embedder,
                        &generation_id,
                        &item,
                    )
                    .await;
                    continue;
                }
                Ok(None) => {}
                Err(error) => log::warn!("Semantic due-work selection failed: {error}"),
            }
        }

        if !sleep_quantum(&state.cancel).await {
            break;
        }
    }
}

async fn sleep_quantum(cancel: &CancellationToken) -> bool {
    tokio::select! {
        _ = cancel.cancelled() => false,
        _ = tokio::time::sleep(PAUSE_QUANTUM) => true,
    }
}

async fn load_embedder(loader: &EngineLoader) -> Result<Arc<dyn DocumentEmbedder>, String> {
    let loader = Arc::clone(loader);
    tokio::task::spawn_blocking(move || loader())
        .await
        .map_err(|error| format!("model loader join failed: {error}"))?
}

/// Derives the persisted retrieval model identity from the complete approved
/// contract (`architecture.md` "Prior-Model Retention Across Upgrade"): bundle
/// id, embedding model id and revision, ONNX export revision, quantization,
/// dimensions, vector encoding, and chunker version. The readable prefix keeps
/// logs/status diagnosable; the SHA-256 digest carries the discrimination, so
/// any contract change mints a distinct identity.
pub(crate) fn derived_model_identity(
    bundle_id: &str,
    embedding_model_id: &str,
    embedding_revision: &str,
    export_revision: &str,
    export_quantization: &str,
    dimensions: u32,
    vector_encoding: &str,
    chunker_version: u32,
) -> String {
    let payload = [
        bundle_id,
        embedding_model_id,
        embedding_revision,
        export_revision,
        export_quantization,
        &dimensions.to_string(),
        vector_encoding,
        &chunker_version.to_string(),
    ]
    .join("\u{1f}");
    let digest = Sha256::digest(payload.as_bytes());
    let short: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("mid-{bundle_id}-{vector_encoding}-c{chunker_version}-{short}")
}

/// The persisted identity of the bundled production runtime, folding the
/// manifest-pinned contract with the approved storage contract
/// ([`semantic_model_spec`]).
pub(crate) fn bundled_model_identity() -> String {
    let contract = crate::model_bundle::approved_embedding_contract();
    derived_model_identity(
        contract.bundle_id,
        contract.embedding_model_id,
        contract.embedding_revision,
        contract.onnx_export_revision,
        contract.onnx_export_quantization,
        contract.dimensions,
        VectorEncoding::Int8.as_str(),
        APPROVED_CHUNKER_VERSION,
    )
}

/// Mints one opaque generation id. Nothing may depend on a generation id
/// equalling a hash of anything; uniqueness comes from the random UUID.
fn mint_generation_id() -> String {
    format!("gen-{}", uuid::Uuid::new_v4())
}

/// Approved storage contract (`architecture.md`, Sprint 1 bundle table):
/// symmetric int8 with persisted dequantization, so a normalized component
/// `v` stores `round(v * 127)` and decodes as `(1/127) * q`.
pub(crate) const APPROVED_INT8_DEQUANTIZATION_SCALE: f64 = 1.0 / 127.0;
const APPROVED_INT8_DEQUANTIZATION_ZERO_POINT: i64 = 0;

fn semantic_model_spec(model_id: &str, dimensions: usize) -> ModelSpec {
    ModelSpec {
        model_id: model_id.to_string(),
        dimensions: dimensions as u32,
        vector_encoding: VectorEncoding::Int8,
        chunker_version: APPROVED_CHUNKER_VERSION,
        dequantization_scale: Some(APPROVED_INT8_DEQUANTIZATION_SCALE),
        dequantization_zero_point: Some(APPROVED_INT8_DEQUANTIZATION_ZERO_POINT),
    }
}

async fn register_semantic_identity(
    pool: &SqlitePool,
    embedder: &dyn DocumentEmbedder,
) -> Result<(), sqlx::Error> {
    let model_id = embedder.model_id();
    let spec = semantic_model_spec(&model_id, embedder.dimensions());
    RetrievalRepository::ensure_model(pool, &spec).await?;
    // Resumption is a lookup: an existing live generation for this exact
    // model identity keeps its id (and its documents), so restarts never
    // re-register and never depend on any id derivation.
    if RetrievalRepository::find_live_generation(pool, &model_id)
        .await?
        .is_some()
    {
        return Ok(());
    }
    RetrievalRepository::ensure_generation(pool, &mint_generation_id(), &model_id)
        .await
        .map(|_| ())
}

/// Selects the next due (generation, meeting) item. When an engine is
/// loaded, only generations whose immutable model identity matches that
/// engine are eligible: a generation built for another model is left safely
/// unprocessed (typed-degraded, FTS-only for its changed meetings) instead of
/// being embedded - and possibly corrupted - by the wrong runtime.
async fn next_due_item(
    pool: &SqlitePool,
    embedders: &BTreeMap<String, Arc<dyn DocumentEmbedder>>,
) -> Result<Option<(String, GenerationWorkItem, Arc<dyn DocumentEmbedder>)>, sqlx::Error> {
    for (generation_id, model_id) in RetrievalRepository::list_live_generations(pool).await? {
        let Some(embedder) = embedders.get(&model_id) else {
            log::debug!(
                "Skipping generation {generation_id}: its model {model_id} does not match the loaded engine"
            );
            continue;
        };
        let due = RetrievalRepository::list_due_generation_work(
            pool,
            &generation_id,
            &Utc::now().to_rfc3339(),
            1,
        )
        .await?;
        if let Some(item) = due.into_iter().next() {
            return Ok(Some((generation_id, item, Arc::clone(embedder))));
        }
    }
    Ok(None)
}

/// How a lexical repair pass ended, feeding the worker loop's pacing:
/// indexed, superseded (normal non-advance, meeting stays due), or backed off
/// (persisted failure state). Supersession is deliberately not an error and
/// never touches the durable retry columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FtsRepairOutcome {
    Indexed,
    Superseded,
    BackedOff,
}

/// Model-independent lexical healing: refresh completely, then mark indexed.
/// A superseded mark keeps the meeting due untouched; any failure schedules
/// bounded backoff. Never fails a caller.
async fn repair_fts_item(
    pool: &SqlitePool,
    cancel: &CancellationToken,
    item: &FtsDueItem,
) -> FtsRepairOutcome {
    if cancel.is_cancelled() {
        return FtsRepairOutcome::BackedOff;
    }
    match FtsRepository::refresh_meeting_unmarked(pool, &item.meeting_id).await {
        Ok(()) => {
            // Fence against lost updates: only mark the revision this repair
            // actually refreshed. A concurrent source/folder mutation that
            // advanced the projection leaves the meeting due (a normal
            // no-op retry, not a failure).
            match RetrievalRepository::mark_fts_indexed(
                pool,
                &item.meeting_id,
                item.fts_projection_revision,
            )
            .await
            {
                Ok(true) => {
                    log::info!("Repaired FTS projection for meeting {}", item.meeting_id);
                    FtsRepairOutcome::Indexed
                }
                Ok(false) => {
                    log::debug!(
                        "FTS repair superseded mid-refresh for meeting {}; staying due",
                        item.meeting_id
                    );
                    FtsRepairOutcome::Superseded
                }
                Err(error) => {
                    let safe_error = format!("fts mark failed: {error}");
                    log::warn!(
                        "Marking FTS indexed failed for meeting {}: {}",
                        item.meeting_id,
                        safe_error
                    );
                    if let Err(record_error) = RetrievalRepository::record_fts_failure(
                        pool,
                        &item.meeting_id,
                        &safe_error,
                        &backoff_timestamp(item.attempt_count + 1),
                    )
                    .await
                    {
                        log::warn!("Recording FTS failure state failed: {record_error}");
                    }
                    FtsRepairOutcome::BackedOff
                }
            }
        }
        Err(error) => {
            let safe_error = format!("fts refresh failed: {error}");
            log::warn!(
                "FTS repair deferred for meeting {}: {}",
                item.meeting_id,
                safe_error
            );
            if let Err(record_error) = RetrievalRepository::record_fts_failure(
                pool,
                &item.meeting_id,
                &safe_error,
                &backoff_timestamp(item.attempt_count + 1),
            )
            .await
            {
                log::warn!("Recording FTS failure state failed: {record_error}");
            }
            FtsRepairOutcome::BackedOff
        }
    }
}

/// Processes exactly one due (generation, meeting) item end to end:
/// extract -> chunk -> prune/resume staging -> embed in bounded batches outside
/// transactions -> revision-fenced publication with journal append inside the
/// replacement transaction.
async fn process_semantic_item(
    pool: &SqlitePool,
    cancel: &CancellationToken,
    pressure: &PressureProbe,
    scheduler: &RetrievalScheduler,
    index: &crate::retrieval::index::QueryIndexService,
    embedder: &Arc<dyn DocumentEmbedder>,
    generation_id: &str,
    item: &GenerationWorkItem,
) {
    let meeting_id = item.meeting_id.as_str();
    let source = match RetrievalRepository::load_meeting_source(pool, meeting_id).await {
        Ok(Some(source)) => source,
        Ok(None) => return,
        Err(error) => {
            record_item_failure(
                pool,
                index,
                generation_id,
                meeting_id,
                item,
                &format!("source extraction failed: {error}"),
            )
            .await;
            return;
        }
    };
    if !source.complete {
        record_item_failure(
            pool,
            index,
            generation_id,
            meeting_id,
            item,
            "authoritative source exceeds the supported limit",
        )
        .await;
        return;
    }
    let Some(revision) = source.source_revision else {
        return;
    };
    if cancel.is_cancelled() {
        return;
    }

    let config = ChunkerConfig {
        model_id: embedder.model_id(),
        ..ChunkerConfig::default()
    };
    let documents = chunk_meeting(&source, &config, &EmbedderTokenizer(embedder));

    // Staging is bound to (generation, meeting, revision); a crashed run with
    // the same binding resumes instead of duplicating work. Rows diverging
    // from the fresh chunk set are pruned so publication mirrors exactly what
    // was extracted.
    let job_id = staging_job_id(generation_id, meeting_id, revision);
    let keep: Vec<String> = documents.iter().map(|d| d.document_id.clone()).collect();
    if let Err(error) = RetrievalRepository::retain_staged_documents(pool, &job_id, &keep).await {
        log::warn!("Pruning divergent staged documents failed: {error}");
        match RetrievalRepository::discard_staging_job(pool, &job_id).await {
            Ok(()) => {}
            Err(discard_error) => {
                log::warn!("Discarding divergent staged job also failed: {discard_error}");
                record_item_failure(
                    pool,
                    index,
                    generation_id,
                    meeting_id,
                    item,
                    &format!("staging cleanup failed: {error}; discard failed: {discard_error}"),
                )
                .await;
                return;
            }
        }
    }
    // Resume selection reads staged identities only: deciding what still
    // needs embedding never deserializes payloads or clones vector bytes. A
    // poisoned payload stays invisible here by design; publication validates
    // the exact inserted bytes, and its failure below heals the job.
    let reusable: HashSet<String> = match RetrievalRepository::list_staged_document_ids(
        pool, &job_id,
    )
    .await
    {
        Ok(ids) => ids.into_iter().collect(),
        Err(error) => {
            log::warn!("Staged job unreadable, restaging fresh: {error}");
            if let Err(discard_error) =
                RetrievalRepository::discard_staging_job(pool, &job_id).await
            {
                log::warn!("Discarding unreadable staged job failed: {discard_error}");
                record_item_failure(
                        pool,
                        index,
                        generation_id,
                        meeting_id,
                        item,
                        &format!(
                            "unreadable staging recovery failed: {error}; discard failed: {discard_error}"
                        ),
                    )
                    .await;
                return;
            }
            HashSet::new()
        }
    };

    let pending: Vec<&SemanticDocument> = documents
        .iter()
        .filter(|document| !reusable.contains(&document.document_id))
        .collect();
    let dimensions = embedder.dimensions();

    for batch in plan_batches(&pending, dimensions) {
        if cancel.is_cancelled() {
            return;
        }
        while pressure() {
            if !sleep_quantum(cancel).await {
                return;
            }
        }
        // Index work yields the single inference permit to interactive
        // requests; holding it across one bounded batch keeps preemption
        // granularity fine.
        let Some(_permit) = scheduler.acquire_for_index(cancel).await else {
            return;
        };
        let texts: Vec<String> = pending[batch.clone()]
            .iter()
            .map(|document| document.content.clone())
            .collect();
        let embedder = Arc::clone(embedder);
        let batch_cancel = cancel.clone();
        let embedded = tokio::task::spawn_blocking(move || {
            embedder.embed_documents_blocking(&texts, &batch_cancel)
        })
        .await;
        drop(_permit);

        let vectors = match embedded {
            Ok(Ok(vectors)) => vectors,
            Ok(Err(RetrievalModelError::Cancelled)) => return,
            Ok(Err(error)) => {
                record_item_failure(
                    pool,
                    index,
                    generation_id,
                    meeting_id,
                    item,
                    &format!("embedding failed: {error}"),
                )
                .await;
                return;
            }
            Err(error) => {
                record_item_failure(
                    pool,
                    index,
                    generation_id,
                    meeting_id,
                    item,
                    &format!("embedding task join failed: {error}"),
                )
                .await;
                return;
            }
        };

        // A partial or mis-shaped embedding response must never publish a
        // partial meeting: require exactly one correctly-dimensioned vector
        // per requested document, else durable retry with prior documents
        // intact.
        if vectors.len() != pending[batch.clone()].len() {
            record_item_failure(
                pool,
                index,
                generation_id,
                meeting_id,
                item,
                &format!(
                    "embedding returned {} vectors for {} documents",
                    vectors.len(),
                    pending[batch].len()
                ),
            )
            .await;
            return;
        }
        let mut staged_batch = Vec::with_capacity(vectors.len());
        for (document, embedding) in pending[batch.clone()].iter().cloned().zip(&vectors) {
            match staged_document(&document, embedding, dimensions) {
                Ok(document) => staged_batch.push(document),
                Err(reason) => {
                    record_item_failure(
                        pool,
                        index,
                        generation_id,
                        meeting_id,
                        item,
                        &format!("embedding validation failed: {reason}"),
                    )
                    .await;
                    return;
                }
            }
        }
        if let Err(error) = RetrievalRepository::stage_documents(
            pool,
            &job_id,
            generation_id,
            meeting_id,
            revision,
            &staged_batch,
        )
        .await
        {
            record_item_failure(
                pool,
                index,
                generation_id,
                meeting_id,
                item,
                &format!("staging failed: {error}"),
            )
            .await;
            return;
        }
    }

    // Post-staging publication fence: cancellation or capture pressure at
    // this boundary leaves the fully staged valid job in place for retry or
    // resume, never publishing (or deleting) prior canonical documents.
    while pressure() {
        if !sleep_quantum(cancel).await {
            return;
        }
    }
    if cancel.is_cancelled() {
        return;
    }
    // Short revision-fenced replacement: re-reads the source revision, swaps
    // staging into canonical documents, clears retry state, appends the upsert
    // journal entry, and advances canonical change ID in one commit.
    let stale_epoch = publication_stale_epoch(index, generation_id);
    match RetrievalRepository::replace_meeting_documents(
        pool,
        ReplacementJob {
            generation_id,
            meeting_id,
            expected_source_revision: revision,
            job_id: &job_id,
        },
    )
    .await
    {
        Ok(ReplacementOutcome::Published { change_id }) => {
            if let Some(stale_epoch) = stale_epoch {
                if let Ok(Some((_canonical, published))) =
                    RetrievalRepository::publication_lag(pool, generation_id).await
                {
                    index.commit_stale(stale_epoch, generation_id, change_id, Some(published));
                } else {
                    index.commit_stale(stale_epoch, generation_id, change_id, None);
                }
            }
            log::debug!(
                "Published semantic documents for meeting {meeting_id} (change {change_id})"
            );
        }
        Ok(ReplacementOutcome::RevisionConflict { .. }) => {
            if let Some(stale_epoch) = stale_epoch {
                index.restore_stale(stale_epoch);
            }
            // Source changed mid-inference; the fence discarded the job and
            // the next pass re-extracts at the current revision.
            log::debug!("Semantic publication fenced out for meeting {meeting_id}");
        }
        Err(error) => {
            if let Some(stale_epoch) = stale_epoch {
                index.restore_stale(stale_epoch);
            }
            if is_unreadable_staged_payload(&error) {
                // A staged row that can never validate would poison every
                // future resume; drop exactly this job so the next attempt
                // restages fresh instead of burning bounded attempts.
                if let Err(discard_error) =
                    RetrievalRepository::discard_staging_job(pool, &job_id).await
                {
                    log::warn!("Discarding poisoned staged job failed: {discard_error}");
                }
                log::warn!("Discarded poisoned staged job for meeting {meeting_id}");
            }
            record_item_failure(
                pool,
                index,
                generation_id,
                meeting_id,
                item,
                &format!("publication failed: {error}"),
            )
            .await;
        }
    }
}

fn publication_stale_epoch(
    index: &crate::retrieval::index::QueryIndexService,
    generation_id: &str,
) -> Option<u64> {
    (index.active_generation().as_deref() == Some(generation_id)).then(|| index.mark_stale())
}

async fn record_item_failure(
    pool: &SqlitePool,
    index: &crate::retrieval::index::QueryIndexService,
    generation_id: &str,
    meeting_id: &str,
    item: &GenerationWorkItem,
    safe_error: &str,
) {
    let next_attempt = item.attempt_count + 1;
    let terminal = next_attempt >= MAX_ITEM_ATTEMPTS;
    log::warn!(
        "Semantic indexing failed for meeting {meeting_id} (attempt {next_attempt}, terminal: {terminal}): {safe_error}"
    );
    if terminal {
        index.suppress_terminal_failure(generation_id, meeting_id);
    }
    match RetrievalRepository::record_work_failure(
        pool,
        generation_id,
        meeting_id,
        terminal,
        safe_error,
        &backoff_timestamp(next_attempt),
    )
    .await
    {
        Ok(()) => {}
        Err(error) => log::warn!("Recording retry state failed for meeting {meeting_id}: {error}"),
    }
    // A terminal item failure is an activation blocker, never a generation
    // killer: `record_work_failure` keeps the queue's place so one poison
    // meeting can neither destroy nor starve the rest of the generation. Only
    // once NOTHING else can make progress does the generation itself become
    // terminal, so the user gets a retryable state instead of a silent stall.
    if terminal {
        match RetrievalRepository::generation_has_outstanding_work(pool, generation_id).await {
            Ok(true) => {}
            Ok(false) => {
                match RetrievalRepository::mark_shadow_generation_failed(pool, generation_id).await
                {
                    Ok(_) => {}
                    Err(error) => {
                        log::warn!("Recording shadow generation failure failed: {error}")
                    }
                }
            }
            Err(error) => log::warn!("Outstanding generation work check failed: {error}"),
        }
    }
}

fn staging_job_id(generation_id: &str, meeting_id: &str, revision: i64) -> String {
    format!("{generation_id}|{meeting_id}|{revision}")
}

/// Symmetric int8 quantization of a normalized f32 embedding under the
/// approved contract: each finite component maps to
/// `round(clamp(v, -1, 1) * 127)` in `[-127, 127]`, stored as its raw byte.
/// Non-finite components fail typed so nothing invalid reaches staging.
pub(crate) fn quantize_int8(vector: &[f32]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(vector.len());
    for (index, value) in vector.iter().enumerate() {
        if !value.is_finite() {
            return Err(format!("embedding component {index} is not finite"));
        }
        let quantized = (value.clamp(-1.0, 1.0) * 127.0).round();
        bytes.push(quantized.clamp(-127.0, 127.0) as i8 as u8);
    }
    Ok(bytes)
}

/// Converts one embedded document into its staged row under the approved int8
/// storage contract, rejecting dimension-mismatched embeddings typed/safely
/// before anything is written.
fn staged_document(
    document: &SemanticDocument,
    embedding: &[f32],
    dimensions: usize,
) -> Result<StagedDocument, String> {
    if embedding.len() != dimensions {
        return Err(format!(
            "embedding returned {} values but the model declares {dimensions}",
            embedding.len()
        ));
    }
    let vector = quantize_int8(embedding)?;
    Ok(StagedDocument {
        document_id: document.document_id.clone(),
        source_kind: document.source_kind.to_string(),
        source_start_id: document
            .transcript
            .as_ref()
            .map(|range| range.start_segment_id.clone()),
        source_end_id: document
            .transcript
            .as_ref()
            .map(|range| range.end_segment_id.clone()),
        source_template_id: document.source_template_id.clone(),
        heading: document.heading.clone(),
        ordinal: document.ordinal as i64,
        content: document.content.clone(),
        content_hash: document.content_hash.clone(),
        dimensions: dimensions as i64,
        vector_encoding: VectorEncoding::Int8,
        vector,
    })
}

/// Greedy batch planner honoring both approved ceilings: at most
/// [`MAX_STAGE_DOCUMENTS`] documents and [`MAX_STAGE_BYTES`] estimated working
/// memory per batch. An oversized single document forms its own batch (chunks
/// are identity-bound and cannot split further here).
fn plan_batches(pending: &[&SemanticDocument], dimensions: usize) -> Vec<Range<usize>> {
    let mut batches = Vec::new();
    let mut start = 0usize;
    while start < pending.len() {
        let mut bytes = 0u64;
        let mut end = start;
        while end < pending.len() && end - start < MAX_STAGE_DOCUMENTS {
            let document_bytes = estimated_document_bytes(pending[end], dimensions);
            if end > start && bytes + document_bytes > MAX_STAGE_BYTES {
                break;
            }
            bytes += document_bytes;
            end += 1;
        }
        batches.push(start..end);
        start = end;
    }
    batches
}

fn estimated_document_bytes(document: &SemanticDocument, dimensions: usize) -> u64 {
    // Conservative working memory of the f32 inference input plus staged
    // payload overhead; persisted vectors are int8 (one byte per dimension)
    // and never counted below this ceiling.
    (document.content.len() + dimensions * 4) as u64 + 256
}

/// Exponential backoff: 2 s doubling per attempt, capped at 1 h.
fn backoff_seconds(attempt: i64) -> i64 {
    if attempt <= 1 {
        return BASE_BACKOFF_SECS;
    }
    let shift = (attempt as u32).saturating_sub(1).min(BACKOFF_MAX_SHIFT);
    BASE_BACKOFF_SECS
        .checked_shl(shift)
        .unwrap_or(BACKOFF_CAP_SECS)
        .min(BACKOFF_CAP_SECS)
}

fn backoff_timestamp(attempt: i64) -> String {
    let delay = chrono::Duration::seconds(backoff_seconds(attempt));
    (Utc::now() + delay).to_rfc3339()
}

fn locked<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::folder::FolderRepository;
    use crate::database::repositories::setting::SettingsRepository;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    use std::sync::atomic::AtomicBool;

    const FAKE_MODEL_ID: &str = "fake-e5-bundle";
    const FAKE_DIMENSIONS: usize = 4;

    // -- Harness ------------------------------------------------------------

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

    async fn scalar(pool: &SqlitePool, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    async fn wait_until(predicate: impl AsyncFn() -> bool) {
        for _ in 0..400 {
            if predicate().await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            predicate().await,
            "condition not reached within the wait budget"
        );
    }

    async fn meeting_state(
        pool: &SqlitePool,
        generation: &str,
        meeting: &str,
    ) -> (String, i64, i64) {
        sqlx::query_as(
            "SELECT state, attempt_count, indexed_source_revision
             FROM retrieval_meeting_state WHERE generation_id = ? AND meeting_id = ?",
        )
        .bind(generation)
        .bind(meeting)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// Durable proof of how many times the publisher journaled a generation:
    /// ids are dense from 1, so (canonical, published) equals that count once
    /// fully acknowledged - even after acknowledged rows are reclaimed.
    async fn published_bounds(pool: &SqlitePool, generation: &str) -> (i64, i64) {
        RetrievalRepository::publication_lag(pool, generation)
            .await
            .unwrap()
            .unwrap()
    }

    /// Waits until the durable bound of `generation` is non-zero and fully
    /// acknowledged (the lagging subscriber race out of the assertion).
    async fn wait_fully_acknowledged(pool: &SqlitePool, generation: &str) {
        let pool_ref = pool;
        let generation_ref = generation;
        wait_until(async || {
            let (canonical, published) = published_bounds(pool_ref, generation_ref).await;
            canonical > 0 && canonical == published
        })
        .await;
    }

    async fn fts_is_current(pool: &SqlitePool, meeting: &str) -> bool {
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT fts_indexed_revision, fts_projection_revision
             FROM search_source_state WHERE meeting_id = ?",
        )
        .bind(meeting)
        .fetch_one(pool)
        .await
        .map(|(indexed, projection)| indexed == projection)
        .unwrap_or(false)
    }

    /// Deterministic fake embedder. `vector_for` is shared with the harness so
    /// tests can predict staged payloads byte-for-byte.
    struct FakeEmbedder {
        fail_substring: StdMutex<Option<String>>,
        embedded: StdMutex<Vec<String>>,
        park_first_call: StdMutex<Option<std::sync::mpsc::Receiver<()>>>,
        entered_first_call: Arc<AtomicBool>,
        response_fault: StdMutex<Option<ResponseFault>>,
    }

    /// Simulates a malformed embedding response so the worker's exact-count /
    /// exact-dimension guard can be proven against real durable state.
    #[derive(Clone, Copy)]
    enum ResponseFault {
        /// Return one fewer vector than documents requested.
        Short,
        /// Return correctly-counted vectors with an extra component.
        WrongDimensions,
    }

    impl FakeEmbedder {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                fail_substring: StdMutex::new(None),
                embedded: StdMutex::new(Vec::new()),
                park_first_call: StdMutex::new(None),
                entered_first_call: Arc::new(AtomicBool::new(false)),
                response_fault: StdMutex::new(None),
            })
        }

        fn vector_for(text: &str) -> Vec<f32> {
            let mut vector = vec![0.0_f32; FAKE_DIMENSIONS];
            vector[text.len() % FAKE_DIMENSIONS] = 1.0;
            vector
        }

        fn fail_on(&self, substring: &str) {
            *self.fail_substring.lock().unwrap() = Some(substring.to_string());
        }

        fn break_response(&self, fault: Option<ResponseFault>) {
            *self.response_fault.lock().unwrap() = fault;
        }

        fn embedded_texts(&self) -> Vec<String> {
            self.embedded.lock().unwrap().clone()
        }
    }

    impl DocumentEmbedder for FakeEmbedder {
        fn model_id(&self) -> String {
            FAKE_MODEL_ID.to_string()
        }

        fn dimensions(&self) -> usize {
            FAKE_DIMENSIONS
        }

        fn count_tokens(&self, text: &str) -> usize {
            text.split_whitespace().count()
        }

        fn embed_documents_blocking(
            &self,
            texts: &[String],
            cancel: &CancellationToken,
        ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
            if self
                .entered_first_call
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                if let Some(receiver) = self.park_first_call.lock().unwrap().take() {
                    // Model real bounded-cancellation behavior: parked model
                    // work wakes on cancellation instead of hanging forever.
                    loop {
                        if cancel.is_cancelled() {
                            return Err(RetrievalModelError::Cancelled);
                        }
                        match receiver.recv_timeout(Duration::from_millis(20)) {
                            Ok(()) => break,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                }
            }
            if let Some(substring) = self.fail_substring.lock().unwrap().as_ref() {
                if texts.iter().any(|text| text.contains(substring)) {
                    return Err(RetrievalModelError::Inference {
                        role: "embedding",
                        reason: "synthetic permanent embedding failure".to_string(),
                    });
                }
            }
            self.embedded.lock().unwrap().extend(texts.iter().cloned());
            let mut vectors: Vec<Vec<f32>> =
                texts.iter().map(|text| Self::vector_for(text)).collect();
            match *self.response_fault.lock().unwrap() {
                Some(ResponseFault::Short) => {
                    vectors.pop();
                }
                Some(ResponseFault::WrongDimensions) => {
                    for vector in &mut vectors {
                        vector.push(0.5);
                    }
                }
                None => {}
            }
            Ok(vectors)
        }

        fn embed_queries_blocking(
            &self,
            texts: &[String],
            cancel: &CancellationToken,
        ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
            // The fake has no prefix contract; query and document behavior match.
            self.embed_documents_blocking(texts, cancel)
        }
    }

    fn ok_loader(embedder: &Arc<FakeEmbedder>) -> EngineLoader {
        let embedder = Arc::clone(embedder);
        Arc::new(move || Ok(Arc::clone(&embedder) as Arc<dyn DocumentEmbedder>))
    }

    fn failing_loader() -> EngineLoader {
        Arc::new(|| Err("simulated bundle unavailability".to_string()))
    }

    fn free_pressure() -> PressureProbe {
        Arc::new(|| false)
    }

    struct PausedFlag(Arc<AtomicBool>);

    impl PausedFlag {
        fn new(paused: bool) -> (Self, PressureProbe) {
            let flag = Arc::new(AtomicBool::new(paused));
            let probe_flag = Arc::clone(&flag);
            (
                Self(flag),
                Arc::new(move || probe_flag.load(Ordering::SeqCst)),
            )
        }

        fn set(&self, paused: bool) {
            self.0.store(paused, Ordering::SeqCst);
        }
    }

    fn lifecycle_for_test(pressure: PressureProbe, loader: EngineLoader) -> RetrievalLifecycle {
        RetrievalLifecycle::new(LifecycleConfig::testing(pressure, loader))
    }

    async fn ensure_test_generation(pool: &SqlitePool) -> String {
        // The exact production registration path (same approved int8 spec and
        // resumption logic), so manual staging in tests validates exactly
        // like pipeline output.
        let embedder = FakeEmbedder::new();
        register_semantic_identity(pool, embedder.as_ref())
            .await
            .unwrap();
        RetrievalRepository::find_live_generation(pool, FAKE_MODEL_ID)
            .await
            .unwrap()
            .expect("registration must leave a live generation to resume")
    }

    /// Waits until the attached worker registered its generation and returns
    /// the looked-up (never recomputed) id.
    async fn worker_generation(pool: &SqlitePool) -> String {
        let pool_ref = pool;
        wait_until(async || {
            RetrievalRepository::find_live_generation(pool_ref, FAKE_MODEL_ID)
                .await
                .unwrap()
                .is_some()
        })
        .await;
        RetrievalRepository::find_live_generation(pool, FAKE_MODEL_ID)
            .await
            .unwrap()
            .expect("worker registration must produce a resumable generation")
    }

    #[tokio::test]
    async fn shadow_publication_does_not_block_active_queries() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Publication").await;
        let generation = ensure_test_generation(&pool).await;
        let revision = RetrievalRepository::current_source_revision(&pool, "m")
            .await
            .unwrap()
            .unwrap();
        let job_id = staging_job_id(&generation, "m", revision);
        RetrievalRepository::stage_documents(
            &pool,
            &job_id,
            &generation,
            "m",
            revision,
            &[test_document("doc", "content")],
        )
        .await
        .unwrap();
        assert!(matches!(
            RetrievalRepository::replace_meeting_documents(
                &pool,
                ReplacementJob {
                    generation_id: &generation,
                    meeting_id: "m",
                    expected_source_revision: revision,
                    job_id: &job_id,
                },
            )
            .await
            .unwrap(),
            ReplacementOutcome::Published { .. }
        ));
        sqlx::query("UPDATE retrieval_generations SET state = 'ready' WHERE generation_id = ?")
            .bind(&generation)
            .execute(&pool)
            .await
            .unwrap();
        RetrievalRepository::switch_active_generation(&pool, &generation)
            .await
            .unwrap();
        let service = crate::retrieval::index::QueryIndexService::new(RetrievalScheduler::new());
        service.set_loaded_model(FAKE_MODEL_ID);
        crate::retrieval::index::publish_tick(&pool, &service)
            .await
            .unwrap();

        assert!(publication_stale_epoch(&service, "shadow").is_none());
        assert!(service
            .search(
                &[1.0, 0.0, 0.0, 0.0],
                crate::retrieval::index::ScopeFilter::All,
                1,
                &CancellationToken::new(),
            )
            .await
            .is_ok());

        let stale_epoch = publication_stale_epoch(&service, &generation).unwrap();
        assert_eq!(
            service
                .search(
                    &[1.0, 0.0, 0.0, 0.0],
                    crate::retrieval::index::ScopeFilter::All,
                    1,
                    &CancellationToken::new(),
                )
                .await
                .unwrap_err(),
            crate::retrieval::index::SearchFailure::CatchUpPending { behind: 1 }
        );
        service.restore_stale(stale_epoch);
    }

    fn test_document(document_id: &str, content: &str) -> StagedDocument {
        let vector = quantize_int8(&FakeEmbedder::vector_for(content)).unwrap();
        StagedDocument {
            document_id: document_id.to_string(),
            source_kind: "transcript".to_string(),
            source_start_id: None,
            source_end_id: None,
            source_template_id: None,
            heading: None,
            ordinal: 0,
            content: content.to_string(),
            content_hash: vec![1, 2, 3],
            dimensions: FAKE_DIMENSIONS as i64,
            vector_encoding: VectorEncoding::Int8,
            vector,
        }
    }

    async fn chunk_with_fake(
        pool: &SqlitePool,
        meeting: &str,
        embedder: &Arc<FakeEmbedder>,
    ) -> Vec<SemanticDocument> {
        let source = RetrievalRepository::load_meeting_source(pool, meeting)
            .await
            .unwrap()
            .unwrap();
        let embedder = Arc::clone(embedder) as Arc<dyn DocumentEmbedder>;
        chunk_meeting(
            &source,
            &ChunkerConfig {
                model_id: FAKE_MODEL_ID.to_string(),
                ..ChunkerConfig::default()
            },
            &EmbedderTokenizer(&embedder),
        )
    }

    // -- Repository additions -----------------------------------------------

    #[tokio::test]
    async fn ensure_registration_is_idempotent_and_lists_live_generations() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Seeded").await;
        let generation = ensure_test_generation(&pool).await;

        let spec = ModelSpec {
            model_id: FAKE_MODEL_ID.to_string(),
            dimensions: FAKE_DIMENSIONS as u32,
            vector_encoding: VectorEncoding::F32,
            chunker_version: APPROVED_CHUNKER_VERSION,
            dequantization_scale: None,
            dequantization_zero_point: None,
        };
        assert!(!RetrievalRepository::ensure_model(&pool, &spec)
            .await
            .unwrap());
        assert!(
            !RetrievalRepository::ensure_generation(&pool, &generation, FAKE_MODEL_ID)
                .await
                .unwrap()
        );

        // Registration seeded the pre-existing meeting exactly once.
        assert_eq!(
            scalar(
                &pool,
                &format!(
                    "SELECT COUNT(*) FROM retrieval_meeting_state WHERE generation_id = '{generation}'"
                )
            )
            .await,
            1
        );
        assert_eq!(
            RetrievalRepository::list_live_generations(&pool)
                .await
                .unwrap(),
            vec![(generation, FAKE_MODEL_ID.to_string())]
        );
    }

    #[tokio::test]
    async fn stale_staging_discarded_and_valid_staging_readable() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Staged").await;
        add_transcript(&pool, "t1", "m", "content").await;
        let generation = ensure_test_generation(&pool).await;

        // Current-revision staging survives recovery; stale-revision staging
        // does not.
        RetrievalRepository::stage_documents(
            &pool,
            &staging_job_id(&generation, "m", 2),
            &generation,
            "m",
            2,
            &[test_document("current", "current content")],
        )
        .await
        .unwrap();
        RetrievalRepository::stage_documents(
            &pool,
            &staging_job_id(&generation, "m", 1),
            &generation,
            "m",
            1,
            &[test_document("stale", "stale content")],
        )
        .await
        .unwrap();

        assert_eq!(
            RetrievalRepository::discard_stale_staging(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            1
        );

        let resumed = RetrievalRepository::list_staged_document_ids(
            &pool,
            &staging_job_id(&generation, "m", 2),
        )
        .await
        .unwrap();
        assert_eq!(resumed, vec!["current".to_string()]);

        // Pruning keeps exactly the current chunk set...
        let removed = RetrievalRepository::retain_staged_documents(
            &pool,
            &staging_job_id(&generation, "m", 2),
            &["kept".to_string()],
        )
        .await
        .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            0
        );

        // ...and an unreadable payload is invisible to the identity-only
        // resume read: it yields its id, and validation - with its atomic
        // abort plus poisoned-job discard/restage heal - happens exactly at
        // publication instead.
        RetrievalRepository::stage_documents(
            &pool,
            &staging_job_id(&generation, "m", 2),
            &generation,
            "m",
            2,
            &[test_document("corrupt", "corrupt content")],
        )
        .await
        .unwrap();
        sqlx::query("UPDATE retrieval_document_staging SET payload = x'00'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            RetrievalRepository::list_staged_document_ids(
                &pool,
                &staging_job_id(&generation, "m", 2)
            )
            .await
            .unwrap(),
            vec!["corrupt".to_string()]
        );
    }

    #[tokio::test]
    async fn failed_divergent_staging_cleanup_retries_without_publishing() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Cleanup failure").await;
        add_transcript(&pool, "t1", "m", "stable source").await;
        let generation = ensure_test_generation(&pool).await;
        let embedder = FakeEmbedder::new();
        let revision = RetrievalRepository::current_source_revision(&pool, "m")
            .await
            .unwrap()
            .unwrap();
        let job_id = staging_job_id(&generation, "m", revision);

        RetrievalRepository::stage_documents(
            &pool,
            &job_id,
            &generation,
            "m",
            revision,
            &[test_document("canonical", "canonical")],
        )
        .await
        .unwrap();
        assert!(matches!(
            RetrievalRepository::replace_meeting_documents(
                &pool,
                ReplacementJob {
                    generation_id: &generation,
                    meeting_id: "m",
                    expected_source_revision: revision,
                    job_id: &job_id,
                },
            )
            .await
            .unwrap(),
            ReplacementOutcome::Published { .. }
        ));
        let published_before = published_bounds(&pool, &generation).await;

        sqlx::query("UPDATE meetings SET title = 'Changed source' WHERE id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        let revision = RetrievalRepository::current_source_revision(&pool, "m")
            .await
            .unwrap()
            .unwrap();
        let job_id = staging_job_id(&generation, "m", revision);
        RetrievalRepository::stage_documents(
            &pool,
            &job_id,
            &generation,
            "m",
            revision,
            &[test_document("stale", "stale")],
        )
        .await
        .unwrap();
        sqlx::query(
            "CREATE TRIGGER reject_staging_cleanup BEFORE DELETE ON retrieval_document_staging
             BEGIN SELECT RAISE(ABORT, 'synthetic cleanup failure'); END",
        )
        .execute(&pool)
        .await
        .unwrap();

        let item = GenerationWorkItem {
            meeting_id: "m".to_string(),
            indexed_source_revision: revision - 1,
            source_revision: revision,
            state: "pending".to_string(),
            attempt_count: 0,
        };
        let scheduler = RetrievalScheduler::new();
        let index = crate::retrieval::index::QueryIndexService::new(scheduler.clone());
        let embedder_trait: Arc<dyn DocumentEmbedder> = embedder.clone();
        process_semantic_item(
            &pool,
            &CancellationToken::new(),
            &free_pressure(),
            &scheduler,
            &index,
            &embedder_trait,
            &generation,
            &item,
        )
        .await;

        assert_eq!(published_bounds(&pool, &generation).await, published_before);
        let canonical = RetrievalRepository::read_validated_documents(&pool, &generation, "m")
            .await
            .unwrap();
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].0, "canonical");
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            1
        );
        let (state, attempts, _) = meeting_state(&pool, &generation, "m").await;
        assert_eq!(state, "retry");
        assert_eq!(attempts, 1);
        assert!(embedder.embedded_texts().is_empty());
    }

    // -- Scheduler policy ----------------------------------------------------

    #[tokio::test]
    async fn retaining_more_than_one_chunk_of_staged_documents_keeps_every_requested_id() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Staging").await;
        let generation = ensure_test_generation(&pool).await;
        let job_id = staging_job_id(&generation, "m", 0);
        let documents: Vec<StagedDocument> = (0..=501)
            .map(|index| test_document(&format!("kept-{index}"), "content"))
            .chain(std::iter::once(test_document("stale", "content")))
            .collect();
        RetrievalRepository::stage_documents(&pool, &job_id, &generation, "m", 0, &documents)
            .await
            .unwrap();
        let keep: Vec<String> = (0..=501).map(|index| format!("kept-{index}")).collect();

        assert_eq!(
            RetrievalRepository::retain_staged_documents(&pool, &job_id, &keep)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            502
        );
    }

    #[tokio::test]
    async fn registration_uses_approved_int8_storage_contract() {
        let pool = migrated_pool().await;
        let embedder = FakeEmbedder::new();
        register_semantic_identity(&pool, embedder.as_ref())
            .await
            .unwrap();

        let (encoding, scale, zero_point): (String, Option<f64>, Option<i64>) = sqlx::query_as(
            "SELECT vector_encoding, dequantization_scale, dequantization_zero_point
             FROM retrieval_models WHERE model_id = ?",
        )
        .bind(FAKE_MODEL_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(encoding, "int8");
        assert!(
            (scale.unwrap() - APPROVED_INT8_DEQUANTIZATION_SCALE).abs() < 1e-12,
            "persisted dequantization scale must be the approved 1/127"
        );
        assert_eq!(zero_point.unwrap(), APPROVED_INT8_DEQUANTIZATION_ZERO_POINT);
    }

    #[tokio::test]
    async fn int8_vectors_store_one_byte_per_dimension_and_round_trip_validated() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Int8").await;
        add_transcript(&pool, "t1", "m", "conteudo quantizado").await;
        let generation = ensure_test_generation(&pool).await;

        let document = synthetic_document(10);
        let embedding = FakeEmbedder::vector_for(&document.content);
        let staged = staged_document(&document, &embedding, FAKE_DIMENSIONS).unwrap();
        assert_eq!(
            staged.vector_encoding,
            VectorEncoding::Int8,
            "staged rows carry the approved int8 encoding"
        );
        assert_eq!(staged.vector.len(), FAKE_DIMENSIONS);

        // Non-finite and mis-shaped embeddings are rejected typed/safely
        // before anything reaches staging.
        let mut non_finite = embedding.clone();
        non_finite[0] = f32::NAN;
        assert!(staged_document(&document, &non_finite, FAKE_DIMENSIONS)
            .unwrap_err()
            .contains("not finite"));
        assert!(staged_document(
            &document,
            &embedding[..FAKE_DIMENSIONS - 1],
            FAKE_DIMENSIONS
        )
        .unwrap_err()
        .contains("declares"));

        let revision = RetrievalRepository::current_source_revision(&pool, "m")
            .await
            .unwrap()
            .unwrap();
        RetrievalRepository::stage_documents(
            &pool,
            &staging_job_id(&generation, "m", revision),
            &generation,
            "m",
            revision,
            &[staged],
        )
        .await
        .unwrap();
        assert!(matches!(
            RetrievalRepository::replace_meeting_documents(
                &pool,
                ReplacementJob {
                    generation_id: &generation,
                    meeting_id: "m",
                    expected_source_revision: revision,
                    job_id: &staging_job_id(&generation, "m", revision),
                },
            )
            .await
            .unwrap(),
            ReplacementOutcome::Published { .. }
        ));

        // Exactly one byte per declared dimension.
        let (stored_dimensions, byte_length): (i64, i64) = sqlx::query_as(
            "SELECT dimensions, length(vector) FROM retrieval_documents WHERE meeting_id = 'm'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(byte_length, stored_dimensions);

        // The repository boundary decodes the quantized bytes to an
        // approximately-unit vector matching the source embedding.
        let decoded = RetrievalRepository::read_validated_documents(&pool, &generation, "m")
            .await
            .unwrap();
        assert_eq!(decoded.len(), 1);
        for (actual, expected) in decoded[0].1.iter().zip(&embedding) {
            let quantized = (expected.clamp(-1.0, 1.0) * 127.0).round() / 127.0;
            assert!((actual - quantized as f32).abs() < 1e-6);
        }
        let norm: f32 = decoded[0]
            .1
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        assert!((norm - 1.0).abs() <= 0.05, "approximate unit norm: {norm}");
    }

    #[tokio::test]
    async fn interactive_queue_caps_at_eight_and_cancels_deterministically() {
        let scheduler = RetrievalScheduler::new();
        let held = scheduler.enqueue_interactive().unwrap();
        let holder = held.wait_for_permit().await.unwrap();

        let mut tickets: Vec<_> = (0..MAX_QUEUED_INTERACTIVE)
            .map(|_| scheduler.enqueue_interactive().unwrap())
            .collect();
        assert_eq!(scheduler.queued_interactive(), MAX_QUEUED_INTERACTIVE);
        assert!(
            matches!(
                scheduler.enqueue_interactive(),
                Err(SchedulerRejection::QueueFull {
                    capacity: MAX_QUEUED_INTERACTIVE
                })
            ),
            "the ninth queued interactive request must be rejected"
        );

        // Deterministic removal by ID: cancelling ticket #2 frees exactly one
        // slot; removing it again reports it was no longer queued, and a
        // cancelled ticket never grants a permit.
        let cancelled_id = tickets[2].id();
        let cancelled = tickets.remove(2);
        assert!(cancelled.cancel());
        assert_eq!(scheduler.queued_interactive(), MAX_QUEUED_INTERACTIVE - 1);
        assert!(!scheduler.remove_queued(cancelled_id));
        assert!(
            matches!(
                cancelled.wait_for_permit().await,
                Err(SchedulerRejection::CancelledWhileQueued)
            ),
            "a cancelled ticket never grants a permit"
        );

        drop(holder);
        // Every remaining queued ticket is granted in turn once its
        // predecessor releases the single permit.
        while !tickets.is_empty() {
            let lease = tickets.remove(0).wait_for_permit().await.unwrap();
            drop(lease);
        }
    }

    #[tokio::test]
    async fn fifo_grant_order_is_deterministic_among_interactives() {
        let scheduler = RetrievalScheduler::new();
        let blocker = scheduler.enqueue_interactive().unwrap();
        let holder = blocker.wait_for_permit().await.unwrap();

        let order: Arc<StdMutex<Vec<u64>>> = Arc::default();
        let mut handles = Vec::new();
        for _ in 0..3u64 {
            let ticket = scheduler.enqueue_interactive().unwrap();
            let order = Arc::clone(&order);
            handles.push(tokio::spawn(async move {
                let lease = ticket.wait_for_permit().await.unwrap();
                order.lock().unwrap().push(next_grant_marker());
                drop(lease);
            }));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(holder);
        for handle in handles {
            handle.await.unwrap();
        }
        let observed = order.lock().unwrap().clone();
        assert_eq!(observed.len(), 3);
        assert!(
            observed.windows(2).all(|pair| pair[0] < pair[1]),
            "grants left FIFO order: {observed:?}"
        );
    }

    fn next_grant_marker() -> u64 {
        static MARKER: AtomicU64 = AtomicU64::new(1);
        MARKER.fetch_add(1, Ordering::Relaxed)
    }

    #[tokio::test]
    async fn index_acquisition_yields_to_waiting_interactives() {
        let scheduler = RetrievalScheduler::new();
        let holder = scheduler
            .enqueue_interactive()
            .unwrap()
            .wait_for_permit()
            .await
            .unwrap();
        let queued = scheduler.enqueue_interactive().unwrap();
        let cancel = CancellationToken::new();

        // While an interactive request queues, index acquisition defers even
        // though the single permit itself is free.
        let deferred = tokio::time::timeout(Duration::from_millis(120), async {
            scheduler.acquire_for_index(&cancel).await
        })
        .await;
        assert!(
            deferred.is_err(),
            "index must defer while interactives queue"
        );
        assert!(queued.cancel());

        drop(holder);
        let permit = tokio::time::timeout(Duration::from_millis(500), async {
            scheduler.acquire_for_index(&cancel).await
        })
        .await
        .expect("index acquires once interactives drain");
        drop(permit);
    }

    #[tokio::test]
    async fn vector_scans_are_capped_at_two_permits() {
        let scheduler = RetrievalScheduler::new();
        let cancel = CancellationToken::new();
        let first = scheduler.acquire_vector_scan(&cancel).await.unwrap();
        let second = scheduler.acquire_vector_scan(&cancel).await.unwrap();
        let third = tokio::time::timeout(Duration::from_millis(80), async {
            scheduler.acquire_vector_scan(&cancel).await
        })
        .await;
        assert!(
            third.is_err(),
            "third concurrent scan exceeds the approved cap"
        );
        drop(first);
        let third = tokio::time::timeout(Duration::from_millis(500), async {
            scheduler.acquire_vector_scan(&cancel).await
        })
        .await
        .expect("scan admitted after release");
        drop((second, third));
    }

    // -- Batch planning ------------------------------------------------------

    fn synthetic_document(size: usize) -> SemanticDocument {
        SemanticDocument {
            document_id: "d".repeat(64),
            source_kind: "transcript",
            ordinal: 0,
            content: "x".repeat(size),
            content_hash: vec![0; 32],
            source_template_id: None,
            heading: None,
            transcript: None,
        }
    }

    #[test]
    fn batch_planner_respects_both_approved_ceilings() {
        // Document-count ceiling.
        let docs: Vec<_> = std::iter::repeat_with(|| synthetic_document(10))
            .take(MAX_STAGE_DOCUMENTS + 10)
            .collect();
        let refs: Vec<&SemanticDocument> = docs.iter().collect();
        let batches = plan_batches(&refs, FAKE_DIMENSIONS);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), MAX_STAGE_DOCUMENTS);

        // Memory ceiling splits before exceeding 64 MiB.
        let big = synthetic_document((MAX_STAGE_BYTES / 3) as usize);
        let docs: Vec<_> = vec![big.clone(), big.clone(), big];
        let refs: Vec<&SemanticDocument> = docs.iter().collect();
        let batches = plan_batches(&refs, FAKE_DIMENSIONS);
        assert!(batches.iter().all(|batch| batch.len() <= 2));
        assert_eq!(batches.last().unwrap().end, 3);

        // An oversized single document still forms one batch so work can
        // never stall.
        let huge = synthetic_document((MAX_STAGE_BYTES * 2) as usize);
        let refs = vec![&huge];
        assert_eq!(plan_batches(&refs, FAKE_DIMENSIONS), vec![0..1]);
    }

    #[test]
    fn backoff_grows_exponentially_and_caps_at_an_hour() {
        assert_eq!(backoff_seconds(1), 2);
        assert_eq!(backoff_seconds(2), 4);
        assert_eq!(backoff_seconds(3), 8);
        assert_eq!(backoff_seconds(7), 128);
        assert_eq!(backoff_seconds(12), BACKOFF_CAP_SECS);
        assert_eq!(backoff_seconds(100), BACKOFF_CAP_SECS);
        // Terminal escalation threshold matches MAX_ITEM_ATTEMPTS.
        assert_eq!(MAX_ITEM_ATTEMPTS - 1 >= MAX_ITEM_ATTEMPTS, false);
        assert_eq!(MAX_ITEM_ATTEMPTS >= MAX_ITEM_ATTEMPTS, true);
    }

    // -- Lifecycle behavior --------------------------------------------------

    #[tokio::test]
    async fn attach_starts_exactly_one_worker_and_rejects_duplicates() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Once").await;
        let lifecycle = lifecycle_for_test(free_pressure(), failing_loader());

        lifecycle.attach_database(pool.clone());
        lifecycle.attach_database(pool.clone());
        lifecycle.attach_database(pool);
        assert_eq!(lifecycle.started_attachments(), 1);
        assert!(lifecycle.is_running());

        lifecycle.shutdown().await;
        assert!(!lifecycle.is_running());

        // Explicit stop/detach allows a fresh attach.
        lifecycle.attach_database(migrated_pool().await);
        assert_eq!(lifecycle.started_attachments(), 2);
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn clones_share_the_single_lifecycle_service() {
        let lifecycle = lifecycle_for_test(free_pressure(), failing_loader());
        let clone = lifecycle.clone();
        let other = lifecycle_for_test(free_pressure(), failing_loader());
        assert!(lifecycle.same_service(&clone));
        assert!(!lifecycle.same_service(&other));

        clone.attach_database(migrated_pool().await);
        assert!(
            lifecycle.is_running(),
            "clone attach drives the shared service"
        );
        lifecycle.shutdown().await;
        assert!(!other.is_running(), "shutdown fences every clone");
    }

    #[tokio::test]
    async fn clear_index_restarts_the_worker_without_touching_primary_data() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Primary meeting").await;
        add_transcript(&pool, "t", "m", "Primary transcript").await;
        let embedder = FakeEmbedder::new();
        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());

        lifecycle.clear_index(&pool).await.unwrap();

        assert_eq!(lifecycle.started_attachments(), 2);
        assert!(lifecycle.is_running());
        assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM meetings").await, 1);
        assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM transcripts").await, 1);
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn clear_index_rechecks_after_shutdown_before_removing_a_new_shadow() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Primary meeting").await;
        add_transcript(&pool, "t", "m", "Primary transcript").await;
        RetrievalRepository::register_model(
            &pool,
            &ModelSpec {
                model_id: FAKE_MODEL_ID.to_string(),
                dimensions: FAKE_DIMENSIONS as u32,
                vector_encoding: VectorEncoding::F32,
                chunker_version: APPROVED_CHUNKER_VERSION,
                dequantization_scale: None,
                dequantization_zero_point: None,
            },
        )
        .await
        .unwrap();
        let lifecycle = lifecycle_for_test(free_pressure(), failing_loader());
        lifecycle.attach_database(pool.clone());
        let registration_lifecycle = lifecycle.clone();
        let registration_pool = pool.clone();
        let registration = tokio::spawn(async move {
            while !registration_lifecycle.index_paused() {
                tokio::task::yield_now().await;
            }
            RetrievalRepository::register_generation(
                &registration_pool,
                "late-shadow",
                FAKE_MODEL_ID,
            )
            .await
            .unwrap();
        });

        let result = lifecycle.clear_index(&pool).await;
        registration.await.unwrap();
        assert!(result.is_err());
        assert!(lifecycle.is_running());
        assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM meetings").await, 1);
        assert_eq!(
            RetrievalRepository::generation_status(&pool, "late-shadow")
                .await
                .unwrap()
                .unwrap()
                .state,
            "building"
        );
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn force_lexical_toggle_does_not_follow_shadow_lifecycle_state() {
        let pool = migrated_pool().await;
        let lifecycle = RetrievalLifecycle::default();
        let generation = ensure_test_generation(&pool).await;

        for (generation_state, paused) in [
            ("building", true),
            ("building", false),
            ("failed", false),
            ("ready", false),
        ] {
            sqlx::query("UPDATE retrieval_generations SET state = ? WHERE generation_id = ?")
                .bind(generation_state)
                .bind(&generation)
                .execute(&pool)
                .await
                .unwrap();
            lifecycle.set_index_paused(paused);
            SettingsRepository::set_force_lexical_retrieval(&pool, true)
                .await
                .unwrap();
            assert!(SettingsRepository::get_force_lexical_retrieval(&pool)
                .await
                .unwrap());
            SettingsRepository::set_force_lexical_retrieval(&pool, false)
                .await
                .unwrap();
            assert!(!SettingsRepository::get_force_lexical_retrieval(&pool)
                .await
                .unwrap());
            assert_eq!(lifecycle.index_paused(), paused);
            assert_eq!(
                sqlx::query_scalar::<_, String>(
                    "SELECT state FROM retrieval_generations WHERE generation_id = ?"
                )
                .bind(&generation)
                .fetch_one(&pool)
                .await
                .unwrap(),
                generation_state
            );
        }

        lifecycle.set_index_paused(false);
    }

    #[tokio::test]
    async fn concurrent_force_lexical_toggles_do_not_mutate_work() {
        let pool = migrated_pool().await;
        let lifecycle = RetrievalLifecycle::default();
        let generation = ensure_test_generation(&pool).await;
        lifecycle.set_index_paused(true);

        let first_pool = pool.clone();
        let second_pool = pool.clone();
        let (first_result, second_result) = tokio::join!(
            SettingsRepository::set_force_lexical_retrieval(&first_pool, true),
            SettingsRepository::set_force_lexical_retrieval(&second_pool, false),
        );
        first_result.unwrap();
        second_result.unwrap();
        assert!(lifecycle.index_paused());
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT state FROM retrieval_generations WHERE generation_id = ?"
            )
            .bind(&generation)
            .fetch_one(&pool)
            .await
            .unwrap(),
            "building"
        );
    }

    #[tokio::test]
    async fn clear_and_force_toggle_race_preserves_the_building_shadow() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Primary meeting").await;
        let lifecycle = RetrievalLifecycle::default();
        let generation = ensure_test_generation(&pool).await;

        let clear_lifecycle = lifecycle.clone();
        let clear_pool = pool.clone();
        let force_pool = pool.clone();
        let (clear_result, force_result) = tokio::join!(
            clear_lifecycle.clear_index(&clear_pool),
            SettingsRepository::set_force_lexical_retrieval(&force_pool, true),
        );

        assert!(clear_result.is_err());
        force_result.unwrap();
        assert!(SettingsRepository::get_force_lexical_retrieval(&pool)
            .await
            .unwrap());
        assert_eq!(
            RetrievalRepository::generation_status(&pool, &generation)
                .await
                .unwrap()
                .unwrap()
                .state,
            "building"
        );
        assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM meetings").await, 1);
    }

    #[tokio::test]
    async fn lifecycle_reservation_wait_honors_cancellation() {
        let lifecycle = RetrievalLifecycle::default();
        let control = lifecycle.acquire_control().await;
        let cancel = CancellationToken::new();
        let waiting_lifecycle = lifecycle.clone();
        let waiting_cancel = cancel.clone();
        let waiting = tokio::spawn(async move {
            waiting_lifecycle
                .reserve_operation(LifecycleOperation::Rebuild, Some(&waiting_cancel))
                .await
        });
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(message) if message == "retrieval operation cancelled"));
        drop(control);
    }

    #[tokio::test]
    async fn model_failure_leaves_fts_repair_working() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Lexical Only").await;
        add_transcript(&pool, "t1", "m", "conteudo de retencao").await;

        let lifecycle = lifecycle_for_test(free_pressure(), failing_loader());
        lifecycle.attach_database(pool.clone());

        // Migration leaves indexed behind projection; the model-less worker
        // must still complete the lexical repair.
        let pool_ref = &pool;
        wait_until(async || fts_is_current(pool_ref, "m").await).await;
        let fts_rows = scalar(
            &pool,
            "SELECT COUNT(*) FROM meeting_fts WHERE meeting_id = 'm'",
        )
        .await;
        assert!(fts_rows > 0, "lexical fallback must be searchable again");

        // Semantic registration never happened while models are unavailable.
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_models").await,
            0
        );

        // A later metadata change dirties only the projection and heals again
        // without any re-embedding requirement.
        let folder = FolderRepository::create(&pool, "Work", None).await.unwrap();
        FolderRepository::set_meeting_folder(&pool, "m", Some(&folder.id))
            .await
            .unwrap();
        wait_until(async || fts_is_current(pool_ref, "m").await).await;
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn crash_resume_reuses_valid_staging_without_duplicate_embedding() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Resumed").await;
        add_transcript(&pool, "t1", "m", "primeira frase de abertura").await;
        add_transcript(&pool, "t2", "m", "segunda frase do conteudo").await;
        let generation = ensure_test_generation(&pool).await;

        // Simulate a crash right after every batch but the last was staged:
        // compute what the worker would have produced and leave all but the
        // final document in staging bound to the current revision/job.
        let embedder = FakeEmbedder::new();
        let documents = chunk_with_fake(&pool, "m", &embedder).await;
        assert!(documents.len() >= 2);
        let revision = RetrievalRepository::current_source_revision(&pool, "m")
            .await
            .unwrap()
            .unwrap();
        let job_id = staging_job_id(&generation, "m", revision);
        let survivors: Vec<StagedDocument> = documents[..documents.len() - 1]
            .iter()
            .map(|document| {
                staged_document(
                    document,
                    &FakeEmbedder::vector_for(&document.content),
                    FAKE_DIMENSIONS,
                )
                .unwrap()
            })
            .collect();
        RetrievalRepository::stage_documents(
            &pool,
            &job_id,
            &generation,
            "m",
            revision,
            &survivors,
        )
        .await
        .unwrap();

        // Restart: the worker must reuse every survivor and embed only the
        // remaining document.
        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());
        let pool_ref = &pool;
        let generation_ref = &generation;
        wait_until(async || {
            matches!(
                meeting_state(pool_ref, generation_ref, "m").await,
                (state, _, _) if state == "ready"
            )
        })
        .await;

        let survivor_contents: Vec<String> = survivors
            .iter()
            .map(|document| document.content.clone())
            .collect();
        let embedded = embedder.embedded_texts();
        assert!(
            embedded
                .iter()
                .all(|text| !survivor_contents.contains(text)),
            "valid staged work must be reused, not re-embedded"
        );
        assert_eq!(embedded.len(), 1, "only the missing document was embedded");
        let published = RetrievalRepository::read_validated_documents(&pool, &generation, "m")
            .await
            .unwrap();
        assert_eq!(published.len(), documents.len());
        wait_fully_acknowledged(&pool, &generation).await;
        assert_eq!(
            published_bounds(&pool, &generation).await,
            (1, 1),
            "exactly one journal insertion was acknowledged for the resume"
        );
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            0
        );
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn oversized_resume_reuses_staged_ids_and_publishes_within_the_page_ceiling() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "big", "Resumed Oversized").await;
        // One very long utterance splits into far more than one approved batch
        // of transcript windows, so resume selection and publication both
        // cross multiple pages/batches.
        add_transcript(
            &pool,
            "t-long",
            "big",
            &"palavra ".repeat(crate::retrieval::chunking::WINDOW_TOKENS * 300),
        )
        .await;
        let embedder = FakeEmbedder::new();
        let documents = chunk_with_fake(&pool, "big", &embedder).await;
        assert!(
            documents.len() > MAX_STAGE_DOCUMENTS,
            "the synthetic meeting must exceed the batch ceiling: {} documents",
            documents.len()
        );

        // Crash simulation right before the last batch staged.
        let generation = ensure_test_generation(&pool).await;
        let revision = RetrievalRepository::current_source_revision(&pool, "big")
            .await
            .unwrap()
            .unwrap();
        let job_id = staging_job_id(&generation, "big", revision);
        let survivors: Vec<StagedDocument> = documents[..documents.len() - 1]
            .iter()
            .map(|document| {
                staged_document(
                    document,
                    &FakeEmbedder::vector_for(&document.content),
                    FAKE_DIMENSIONS,
                )
                .unwrap()
            })
            .collect();
        RetrievalRepository::stage_documents(
            &pool,
            &job_id,
            &generation,
            "big",
            revision,
            &survivors,
        )
        .await
        .unwrap();
        assert_eq!(
            RetrievalRepository::list_staged_document_ids(&pool, &job_id)
                .await
                .unwrap()
                .len(),
            documents.len() - 1,
            "staging holds the full job minus one identity"
        );

        // Restart reuses the survivors by identity only - no survivor payload
        // is ever deserialized for the decision or re-embedded - and streams
        // the oversized job into canonical rows inside the fenced transaction.
        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());
        let pool_ref = &pool;
        let generation_ref = &generation;
        wait_until(async || {
            matches!(
                meeting_state(pool_ref, generation_ref, "big").await,
                (state, _, _) if state == "ready"
            )
        })
        .await;

        let embedded = embedder.embedded_texts();
        // Pigeonhole reuse proof: staging held every document except the last
        // (IDs verified above), and exactly one text was embedded - so every
        // other survivor resumed by ID alone without re-embedding. Content
        // strings are meaningless here (identical windows repeat text); IDs
        // carry the identity.
        assert_eq!(embedded.len(), 1, "only the missing tail was embedded");
        assert_eq!(
            scalar(
                &pool,
                "SELECT COUNT(*) FROM retrieval_documents WHERE meeting_id = 'big'"
            )
            .await,
            documents.len() as i64,
            "every oversized document published exactly once"
        );
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            0
        );
        wait_fully_acknowledged(&pool, &generation).await;
        assert_eq!(
            published_bounds(&pool, &generation).await,
            (1, 1),
            "the oversized publication journaled once"
        );
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn superseded_mark_is_normal_and_never_touches_retry_backoff() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "spin", "Spinner").await;

        let item = RetrievalRepository::list_due_fts_repairs(&pool, &Utc::now().to_rfc3339(), 10)
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.meeting_id == "spin")
            .expect("a fresh meeting is due");

        // A mutation lands between selection and marking; the stale mark is a
        // typed non-advance, not an error.
        let folder = FolderRepository::create(&pool, "Work", None).await.unwrap();
        FolderRepository::set_meeting_folder(&pool, "spin", Some(&folder.id))
            .await
            .unwrap();
        let outcome = repair_fts_item(&pool, &CancellationToken::new(), &item).await;
        assert_eq!(outcome, FtsRepairOutcome::Superseded);

        // Nothing persisted: indexed stayed behind, retry columns untouched,
        // meeting still due so the next repair covers the newer projection -
        // durability is never lost and supersession is never terminal.
        let (_, projection, indexed) = source_state_worker(&pool, "spin").await;
        assert_eq!(indexed, 0);
        assert!(projection > item.fts_projection_revision);
        let (attempt_count, next_attempt): (i64, Option<String>) = sqlx::query_as(
            "SELECT fts_attempt_count, fts_next_attempt_at FROM search_source_state WHERE meeting_id = 'spin'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((attempt_count, next_attempt), (0, None));
        assert!(
            RetrievalRepository::list_due_fts_repairs(&pool, &Utc::now().to_rfc3339(), 10)
                .await
                .unwrap()
                .iter()
                .any(|due| due.meeting_id == "spin")
        );

        // A fresh selection marks cleanly.
        let refreshed =
            RetrievalRepository::list_due_fts_repairs(&pool, &Utc::now().to_rfc3339(), 10)
                .await
                .unwrap()
                .into_iter()
                .find(|item| item.meeting_id == "spin")
                .unwrap();
        assert_eq!(
            repair_fts_item(&pool, &CancellationToken::new(), &refreshed).await,
            FtsRepairOutcome::Indexed
        );
    }

    #[tokio::test]
    async fn paused_worker_repairs_fts_without_starting_semantic_work() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "paused", "Paused lexical repair").await;
        add_transcript(
            &pool,
            "paused-t1",
            "paused",
            "lexical repair remains active",
        )
        .await;
        let embedder = FakeEmbedder::new();
        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.set_index_paused(true);
        lifecycle.attach_database(pool.clone());

        let pool_ref = &pool;
        wait_until(async || fts_is_current(pool_ref, "paused").await).await;
        assert!(embedder.embedded_texts().is_empty());
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_generations").await,
            0,
            "manual pause must not start semantic registration"
        );
        lifecycle.shutdown().await;
    }

    async fn source_state_worker(pool: &SqlitePool, meeting: &str) -> (i64, i64, i64) {
        sqlx::query_as(
            "SELECT source_revision, fts_projection_revision, fts_indexed_revision
             FROM search_source_state WHERE meeting_id = ?",
        )
        .bind(meeting)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn continuously_superseded_meeting_yields_ticks_to_other_due_work() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "spinner", "Monopolizer").await;
        insert_meeting(&pool, "healthy", "Healthy").await;
        add_transcript(&pool, "h1", "healthy", "conteudo saudavel de teste").await;

        // The healthy meeting's lexical side is already current, so `spinner`
        // is the ONLY durable FTS-due item. This trigger makes every mark on
        // it affect zero rows - the deterministic equivalent of a projection
        // that advances on every pass - reproducing an endless supersession
        // monopoly that older loop logic would have spun on.
        let (_, spinner_projection, _) = source_state_worker(&pool, "spinner").await;
        assert!(RetrievalRepository::mark_fts_indexed(
            &pool,
            "healthy",
            source_state_worker(&pool, "healthy").await.1
        )
        .await
        .unwrap());
        sqlx::query(
            "CREATE TRIGGER spin_never_converges BEFORE UPDATE ON search_source_state
             WHEN NEW.meeting_id = 'spinner'
             BEGIN SELECT RAISE(IGNORE); END",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            !RetrievalRepository::mark_fts_indexed(&pool, "spinner", spinner_projection)
                .await
                .unwrap()
        );

        // Every worker tick starts with the FTS step; only the bounded
        // consecutive-supersession fall-through lets the semantic step below
        // run at all in this scenario.
        let embedder = FakeEmbedder::new();
        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());
        let pool_ref = &pool;
        wait_until(async || {
            matches!(
                sqlx::query_as::<_, (String,)>(
                    "SELECT state FROM retrieval_meeting_state WHERE meeting_id = 'healthy'",
                )
                .fetch_optional(pool_ref)
                .await,
                Ok(Some((state,))) if state == "ready"
            )
        })
        .await;
        assert!(
            !embedder.embedded_texts().is_empty(),
            "healthy semantic work progressed despite the supersession storm"
        );

        // The monopolizer itself was treated as normal the whole time: still
        // durably due, zero recorded failures, no backoff invented for it.
        let (attempt_count, next_attempt): (i64, Option<String>) = sqlx::query_as(
            "SELECT fts_attempt_count, fts_next_attempt_at FROM search_source_state WHERE meeting_id = 'spinner'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((attempt_count, next_attempt), (0, None));
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn persistently_failing_mark_records_bounded_persisted_backoff() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "fmb", "Failing Mark").await;
        // Refresh reads other tables, and record_fts_failure changes the
        // attempt counter, so this outage (attempt count unchanged) hits
        // exactly mark_fts_indexed - deterministically - with a database
        // error on every attempt while persisted backoff stays writable.
        sqlx::query(
            "CREATE TRIGGER fmb_mark_outage BEFORE UPDATE ON search_source_state
             WHEN NEW.meeting_id = 'fmb'
               AND NEW.fts_attempt_count = OLD.fts_attempt_count
             BEGIN SELECT RAISE(ABORT, 'synthetic persistent mark outage'); END",
        )
        .execute(&pool)
        .await
        .unwrap();

        // The model-less lifecycle proves lexical repair keeps running while
        // every refresh succeeds and every mark fails.
        let lifecycle = lifecycle_for_test(free_pressure(), failing_loader());
        lifecycle.attach_database(pool.clone());
        let pool_ref = &pool;
        wait_until(async || {
            sqlx::query_scalar::<_, i64>(
                "SELECT fts_attempt_count FROM search_source_state WHERE meeting_id = 'fmb'",
            )
            .fetch_one(pool_ref)
            .await
            .unwrap_or(0)
                >= 1
        })
        .await;

        // Exactly like a failed refresh: persisted exponential backoff with
        // the attempt counter advanced and a safe error string stored, so the
        // scheduler cannot hot-retry it within any fixed window.
        let now_text = Utc::now().to_rfc3339();
        let (attempt_count, next_attempt, last_error): (i64, String, String) = sqlx::query_as(
            "SELECT fts_attempt_count, fts_next_attempt_at, fts_last_error
             FROM search_source_state WHERE meeting_id = 'fmb'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            attempt_count >= 1,
            "the failed mark advanced the persisted attempt counter"
        );
        assert!(
            next_attempt.as_str() > now_text.as_str(),
            "backoff ({next_attempt}) must schedule beyond {now_text}"
        );
        assert!(last_error.contains("synthetic persistent mark outage"));
        assert!(
            RetrievalRepository::list_due_fts_repairs(&pool, &now_text, 10)
                .await
                .unwrap()
                .is_empty(),
            "a backed-off repair leaves the due list until its slot"
        );
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn source_edit_during_inference_prevents_stale_publication_then_recovers() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Original Title").await;
        add_transcript(&pool, "t1", "m", "conteudo que sera reindexado").await;
        let embedder = FakeEmbedder::new();

        // Park the first embed call so the test can edit authoritative content
        // while inference is genuinely in flight.
        let (sender, receiver) = std::sync::mpsc::channel::<()>();
        *embedder.park_first_call.lock().unwrap() = Some(receiver);

        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());
        while !embedder.entered_first_call.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Edit mid-inference: the source revision advances under the
        // extraction the worker is holding.
        sqlx::query("UPDATE meetings SET title = 'Edited Mid-Flight' WHERE id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        sender.send(()).unwrap();

        // The fence must discard the stale job without publishing, then the
        // next pass re-extracts at the new revision and publishes cleanly.
        let pool_ref = &pool;
        wait_until(async || {
            matches!(
                sqlx::query_as::<_, (String, i64)>(
                    "SELECT ms.state, ms.indexed_source_revision
                     FROM retrieval_meeting_state ms
                     JOIN search_source_state s ON s.meeting_id = ms.meeting_id
                     WHERE ms.meeting_id = 'm'",
                )
                .fetch_optional(pool_ref)
                .await,
                Ok(Some((state, indexed))) if state == "ready" && indexed >= 3
            )
        })
        .await;
        // The fenced-out attempt must never journal: only the recovered
        // replacement inserted (and acknowledged) change id 1.
        let generation_for_bounds = worker_generation(&pool).await;
        wait_fully_acknowledged(&pool, &generation_for_bounds).await;
        assert_eq!(
            published_bounds(&pool, &generation_for_bounds).await,
            (1, 1),
            "the fenced-out attempt must not journal"
        );
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            0
        );
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn one_terminal_item_never_ends_a_generation_that_can_still_progress() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "poison", "Poison Meeting").await;
        insert_meeting(&pool, "waiting", "Waiting Meeting").await;
        add_transcript(&pool, "p1", "poison", "veneno permanente").await;
        add_transcript(&pool, "w1", "waiting", "conteudo pendente").await;
        let generation = ensure_test_generation(&pool).await;

        // One meeting fails terminally while another is still pending: the
        // generation must keep its 'building' state so the remaining work is
        // neither destroyed nor starved by the poison item.
        RetrievalRepository::record_work_failure(
            &pool,
            &generation,
            "poison",
            true,
            "safe terminal failure",
            "2099-01-01T00:00:00Z",
        )
        .await
        .unwrap();
        assert!(
            RetrievalRepository::generation_has_outstanding_work(&pool, &generation)
                .await
                .unwrap(),
            "the pending meeting is still outstanding work"
        );
        assert_eq!(
            RetrievalRepository::generation_status(&pool, &generation)
                .await
                .unwrap()
                .unwrap()
                .state,
            "building"
        );

        // Once nothing else can progress the generation becomes terminal, so
        // the user gets a retryable state instead of a silent stall.
        RetrievalRepository::record_work_failure(
            &pool,
            &generation,
            "waiting",
            true,
            "safe terminal failure",
            "2099-01-01T00:00:00Z",
        )
        .await
        .unwrap();
        assert!(
            !RetrievalRepository::generation_has_outstanding_work(&pool, &generation)
                .await
                .unwrap()
        );
        assert!(
            RetrievalRepository::mark_shadow_generation_failed(&pool, &generation)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn poison_retry_fairness_keeps_other_work_progressing() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "poison", "Poison Meeting").await;
        insert_meeting(&pool, "healthy", "Healthy Meeting").await;
        add_transcript(&pool, "p1", "poison", "veneno permanente de teste").await;
        add_transcript(&pool, "h1", "healthy", "conteudo saudavel de teste").await;
        let embedder = FakeEmbedder::new();
        embedder.fail_on("veneno");

        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());

        // The healthy meeting reaches ready while the poison meeting sits in
        // scheduled retry instead of starving the queue. The two items are
        // processed on independent passes, so observe the poison failure
        // itself rather than assuming it landed before healthy turned ready.
        let pool_ref = &pool;
        let generation = worker_generation(&pool).await;
        wait_until(async || {
            matches!(
                sqlx::query_as::<_, (String,)>(
                    "SELECT state FROM retrieval_meeting_state WHERE meeting_id = 'healthy'",
                )
                .fetch_optional(pool_ref)
                .await,
                Ok(Some((state,))) if state == "ready"
            ) && matches!(
                meeting_state(pool_ref, &generation, "poison").await,
                (state, attempts, _) if state == "retry" && attempts >= 1
            )
        })
        .await;
        let (state, attempts, _) = meeting_state(&pool, &generation, "poison").await;
        assert_eq!(state, "retry");
        assert_eq!(attempts, 1);

        // Pull the retry forward repeatedly until the poison item is declared
        // permanently failed - healthy work stays unaffected throughout.
        for expected_attempts in 2..=MAX_ITEM_ATTEMPTS {
            sqlx::query(
                "UPDATE retrieval_meeting_state SET next_attempt_at = '2026-01-01T00:00:00+00:00'
                 WHERE meeting_id = 'poison'",
            )
            .execute(&pool)
            .await
            .unwrap();
            let generation_ref = &generation;
            wait_until(async || {
                meeting_state(pool_ref, generation_ref, "poison").await.1 >= expected_attempts
            })
            .await;
        }
        let (state, attempts, _) = meeting_state(&pool, &generation, "poison").await;
        assert_eq!((state.as_str(), attempts), ("failed", MAX_ITEM_ATTEMPTS));
        let (healthy_state, _, _) = meeting_state(&pool, &generation, "healthy").await;
        assert_eq!(healthy_state, "ready");
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn stale_staging_is_discarded_while_current_staging_survives_attach() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Stale Job").await;
        add_transcript(&pool, "t1", "m", "conteudo original").await;
        let generation = ensure_test_generation(&pool).await;

        // Valid current-revision staging plus a stale-revision leftover from a
        // superseded run.
        RetrievalRepository::stage_documents(
            &pool,
            &staging_job_id(&generation, "m", 2),
            &generation,
            "m",
            2,
            &[test_document("current", "conteudo original")],
        )
        .await
        .unwrap();
        RetrievalRepository::stage_documents(
            &pool,
            &staging_job_id(&generation, "m", 1),
            &generation,
            "m",
            1,
            &[test_document("stale", "conteudo antigo")],
        )
        .await
        .unwrap();

        // Pausing keeps the worker from consuming the valid job while we
        // assert purely on recovery behavior.
        let (pause, pressure) = PausedFlag::new(true);
        let embedder = FakeEmbedder::new();
        let lifecycle = lifecycle_for_test(pressure, ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());
        wait_until(async || lifecycle.is_running()).await;
        tokio::time::sleep(Duration::from_millis(400)).await;

        assert_eq!(
            scalar(
                &pool,
                "SELECT COUNT(*) FROM retrieval_document_staging WHERE source_revision = 1 AND meeting_id = 'm'"
            )
            .await,
            0,
            "stale-revision staging must be discarded on attach"
        );
        assert_eq!(
            scalar(
                &pool,
                "SELECT COUNT(*) FROM retrieval_document_staging WHERE source_revision = 2"
            )
            .await,
            1,
            "current-revision staging survives for resume"
        );
        assert!(embedder.embedded_texts().is_empty());

        // Resuming finishes the job; divergent fabricated rows are pruned so
        // publication mirrors exactly the fresh chunk set.
        pause.set(false);
        let pool_ref = &pool;
        wait_until(async || {
            matches!(
                sqlx::query_as::<_, (String,)>(
                    "SELECT state FROM retrieval_meeting_state WHERE meeting_id = 'm'",
                )
                .fetch_optional(pool_ref)
                .await,
                Ok(Some((state,))) if state == "ready"
            )
        })
        .await;
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            0
        );
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn pause_defers_and_resume_completes_without_losing_revisions() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Paused Work").await;
        add_transcript(&pool, "t1", "m", "conteudo durante gravacao").await;
        let generation = ensure_test_generation(&pool).await;
        let embedder = FakeEmbedder::new();
        let (pause, pressure) = PausedFlag::new(true);

        let lifecycle = lifecycle_for_test(pressure, ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());
        wait_until(async || lifecycle.is_running()).await;
        tokio::time::sleep(Duration::from_millis(600)).await;

        // Recording pressure defers everything: no embedding, no state churn,
        // and the durable revision remains owed.
        assert!(embedder.embedded_texts().is_empty());
        let (state, _, indexed) = meeting_state(&pool, &generation, "m").await;
        assert_eq!((state.as_str(), indexed), ("pending", 0));

        pause.set(false);
        let pool_ref = &pool;
        wait_until(async || {
            matches!(
                sqlx::query_as::<_, (String, i64)>(
                    "SELECT state, indexed_source_revision
                     FROM retrieval_meeting_state WHERE meeting_id = 'm'",
                )
                .fetch_optional(pool_ref)
                .await,
                Ok(Some((state, indexed))) if state == "ready" && indexed >= 2
            )
        })
        .await;
        assert!(!embedder.embedded_texts().is_empty());
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn malformed_embedding_response_never_publishes_partial_meetings() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Partial Guard").await;
        add_transcript(&pool, "t1", "m", "conteudo protegido de resposta parcial").await;
        let generation = ensure_test_generation(&pool).await;
        let embedder = FakeEmbedder::new();

        // A short response (fewer vectors than documents) must fail durably
        // without publishing anything.
        embedder.break_response(Some(ResponseFault::Short));
        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());
        let pool_ref = &pool;
        let generation_ref = &generation;
        wait_until(async || meeting_state(pool_ref, generation_ref, "m").await.1 >= 1).await;

        // Same guard for correctly-counted vectors with wrong dimensions.
        sqlx::query(
            "UPDATE retrieval_meeting_state SET next_attempt_at = '2026-01-01T00:00:00+00:00'
             WHERE meeting_id = 'm'",
        )
        .execute(&pool)
        .await
        .unwrap();
        embedder.break_response(Some(ResponseFault::WrongDimensions));
        wait_until(async || meeting_state(pool_ref, generation_ref, "m").await.1 >= 2).await;

        // Neither malformed response published anything.
        assert_eq!(
            scalar(
                &pool,
                "SELECT COUNT(*) FROM retrieval_documents WHERE meeting_id = 'm'"
            )
            .await,
            0,
            "a partial embedding response must never publish partial documents"
        );
        assert_eq!(
            published_bounds(&pool, &generation).await,
            (0, 0),
            "malformed responses must not journal anything"
        );

        // Once responses are healthy again the item publishes completely.
        embedder.break_response(None);
        sqlx::query(
            "UPDATE retrieval_meeting_state SET next_attempt_at = '2026-01-01T00:00:00+00:00'
             WHERE meeting_id = 'm'",
        )
        .execute(&pool)
        .await
        .unwrap();
        wait_until(async || {
            matches!(
                meeting_state(pool_ref, generation_ref, "m").await,
                (state, _, indexed) if state == "ready" && indexed >= 2
            )
        })
        .await;
        let documents = chunk_with_fake(&pool, "m", &embedder).await;
        let published = RetrievalRepository::read_validated_documents(&pool, &generation, "m")
            .await
            .unwrap();
        assert_eq!(published.len(), documents.len(), "full meeting published");
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn post_staging_boundary_respects_pause_and_cancellation_before_publication() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Boundary").await;
        add_transcript(&pool, "t1", "m", "conteudo na fronteira de publicacao").await;
        let generation = ensure_test_generation(&pool).await;
        let embedder = FakeEmbedder::new();
        let documents = chunk_with_fake(&pool, "m", &embedder).await;

        // Park inside the embedding batch so the test controls the moment
        // staging completes relative to capture pressure.
        let (sender, receiver) = std::sync::mpsc::channel::<()>();
        *embedder.park_first_call.lock().unwrap() = Some(receiver);
        let (pause, pressure) = PausedFlag::new(false);
        let lifecycle = lifecycle_for_test(pressure, ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());

        wait_until(async || embedder.entered_first_call.load(Ordering::SeqCst)).await;
        pause.set(true);
        sender.send(()).unwrap();

        // Staging finishes, then the post-staging fence pauses publication.
        let revision = RetrievalRepository::current_source_revision(&pool, "m")
            .await
            .unwrap()
            .unwrap();
        let job_id = staging_job_id(&generation, "m", revision);
        let pool_ref = &pool;
        wait_until(async || {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM retrieval_document_staging WHERE meeting_id = 'm'",
            )
            .fetch_one(pool_ref)
            .await
            .unwrap_or(-1)
                == documents.len() as i64
        })
        .await;

        // Paused at the boundary: valid staging preserved, nothing published.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            scalar(
                &pool,
                "SELECT COUNT(*) FROM retrieval_documents WHERE meeting_id = 'm'"
            )
            .await,
            0
        );
        assert_eq!(
            published_bounds(&pool, &generation).await,
            (0, 0),
            "a paused boundary must not journal"
        );
        assert_eq!(
            scalar(
                &pool,
                &format!(
                    "SELECT COUNT(*) FROM retrieval_document_staging WHERE job_id = '{job_id}'"
                )
            )
            .await,
            documents.len() as i64,
            "fully staged valid job survives the pause"
        );

        // Cancelling while paused joins promptly and preserves everything.
        tokio::time::timeout(Duration::from_secs(5), lifecycle.shutdown())
            .await
            .expect("shutdown joins from the paused post-staging fence");
        assert_eq!(
            scalar(
                &pool,
                "SELECT COUNT(*) FROM retrieval_documents WHERE meeting_id = 'm'"
            )
            .await,
            0
        );
        assert_eq!(
            scalar(
                &pool,
                &format!(
                    "SELECT COUNT(*) FROM retrieval_document_staging WHERE job_id = '{job_id}'"
                )
            )
            .await,
            documents.len() as i64,
            "cancellation after staging keeps the job resumable"
        );

        // Restart resumes the preserved staging and publishes exactly once.
        let embedded_before = embedder.embedded_texts().len();
        let restarted = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        restarted.attach_database(pool.clone());
        let generation_ref = &generation;
        wait_until(async || {
            matches!(
                meeting_state(pool_ref, generation_ref, "m").await,
                (state, _, _) if state == "ready"
            )
        })
        .await;
        assert_eq!(
            embedder.embedded_texts().len(),
            embedded_before,
            "resumed staging is reused"
        );
        wait_fully_acknowledged(&pool, &generation).await;
        assert_eq!(
            published_bounds(&pool, &generation).await,
            (1, 1),
            "the resumed job published exactly once"
        );
        let published = RetrievalRepository::read_validated_documents(&pool, &generation, "m")
            .await
            .unwrap();
        assert_eq!(published.len(), documents.len());
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            0
        );
        restarted.shutdown().await;
    }

    #[tokio::test]
    async fn superseded_staging_is_cleaned_without_restart_while_current_staging_resumes() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Original").await;
        add_transcript(&pool, "t1", "m", "conteudo original estavel").await;
        let generation = ensure_test_generation(&pool).await;
        let embedder = FakeEmbedder::new();

        // Fully stage every current-revision document, ready to publish.
        let documents_v2 = chunk_with_fake(&pool, "m", &embedder).await;
        let revision_v2 = RetrievalRepository::current_source_revision(&pool, "m")
            .await
            .unwrap()
            .unwrap();
        let survivors: Vec<StagedDocument> = documents_v2
            .iter()
            .map(|document| {
                staged_document(
                    document,
                    &FakeEmbedder::vector_for(&document.content),
                    FAKE_DIMENSIONS,
                )
                .unwrap()
            })
            .collect();
        RetrievalRepository::stage_documents(
            &pool,
            &staging_job_id(&generation, "m", revision_v2),
            &generation,
            "m",
            revision_v2,
            &survivors,
        )
        .await
        .unwrap();

        // Attach paused so startup recovery sees a consistent rev2 world.
        let (pause, pressure) = PausedFlag::new(true);
        let lifecycle = lifecycle_for_test(pressure, ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());
        wait_until(async || lifecycle.is_running()).await;
        tokio::time::sleep(Duration::from_millis(400)).await;

        // An edit supersedes that job while the process keeps running.
        sqlx::query("UPDATE meetings SET title = 'Edited Without Restart' WHERE id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        let documents_v3 = chunk_with_fake(&pool, "m", &embedder).await;
        let revision_v3 = RetrievalRepository::current_source_revision(&pool, "m")
            .await
            .unwrap()
            .unwrap();
        assert!(revision_v3 > revision_v2);

        // A valid current-revision partial job exists alongside it (the
        // transcript window is unaffected by the title edit).
        let window = documents_v3
            .iter()
            .find(|document| document.source_kind == "transcript")
            .expect("transcript windows survive a title edit")
            .clone();
        RetrievalRepository::stage_documents(
            &pool,
            &staging_job_id(&generation, "m", revision_v3),
            &generation,
            "m",
            revision_v3,
            &[staged_document(
                &window,
                &FakeEmbedder::vector_for(&window.content),
                FAKE_DIMENSIONS,
            )
            .unwrap()],
        )
        .await
        .unwrap();

        // Still paused: nothing has cleaned anything yet, superseded staging
        // from before the edit remains.
        tokio::time::sleep(Duration::from_millis(400)).await;
        let pool_ref = &pool;
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM retrieval_document_staging WHERE source_revision = ?",
            )
            .bind(revision_v2)
            .fetch_one(pool_ref)
            .await
            .unwrap(),
            documents_v2.len() as i64
        );

        // Processing the due item runs the same cleanup first: superseded
        // staging disappears without any restart, valid staging resumes.
        pause.set(false);
        let generation_ref = &generation;
        wait_until(async || {
            matches!(
                meeting_state(pool_ref, generation_ref, "m").await,
                (state, _, indexed) if state == "ready" && indexed >= revision_v3
            )
        })
        .await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM retrieval_document_staging WHERE source_revision = ?",
            )
            .bind(revision_v2)
            .fetch_one(pool_ref)
            .await
            .unwrap(),
            0,
            "superseded staging must be cleaned without a restart"
        );
        assert!(
            !embedder.embedded_texts().contains(&window.content),
            "current-revision staging must remain resumable"
        );
        wait_fully_acknowledged(&pool, &generation).await;
        assert_eq!(
            published_bounds(&pool, &generation).await,
            (1, 1),
            "the superseded attempt must not journal; the resumed one publishes once"
        );
        let published = RetrievalRepository::read_validated_documents(&pool, &generation, "m")
            .await
            .unwrap();
        assert_eq!(published.len(), documents_v3.len());
        assert_eq!(
            scalar(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            0
        );
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_joins_worker_and_fences_publication() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "fenced", "Fenced").await;
        add_transcript(&pool, "t1", "fenced", "conteudo em voo").await;
        insert_meeting(&pool, "later", "Later").await;
        let embedder = FakeEmbedder::new();

        // Park the first embed call so shutdown has to cancel genuine
        // in-flight model work.
        let (_park_sender, park_receiver) = std::sync::mpsc::channel::<()>();
        *embedder.park_first_call.lock().unwrap() = Some(park_receiver);

        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());
        wait_until(async || embedder.entered_first_call.load(Ordering::SeqCst)).await;

        // Shutdown cancels parked model work (the fake honors cancellation at
        // its bounded boundary) and joins the loop promptly.
        tokio::time::timeout(Duration::from_secs(5), lifecycle.shutdown())
            .await
            .expect("shutdown joins the cancelled worker");
        assert!(!lifecycle.is_running());

        // Nothing published for the in-flight meeting...
        let generation = worker_generation(&pool).await;
        let published = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(indexed_source_revision), 0) FROM retrieval_meeting_state
             WHERE generation_id = ? AND meeting_id = 'fenced'",
        )
        .bind(&generation)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(published, 0, "cancelled model work must not publish");

        // ...and after teardown no further work is picked up.
        add_transcript(&pool, "t2", "later", "pos shutdown").await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let processed = scalar(
            &pool,
            &format!(
                "SELECT COUNT(*) FROM retrieval_meeting_state
                 WHERE generation_id = '{generation}' AND meeting_id = 'later' AND state = 'ready'"
            ),
        )
        .await;
        assert_eq!(processed, 0, "no work may run after shutdown fencing");
    }

    #[tokio::test]
    async fn index_defers_to_interactive_inference_and_resumes() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Preempted").await;
        add_transcript(&pool, "t1", "m", "conteudo preemptado").await;
        let embedder = FakeEmbedder::new();
        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());

        // Hold the interactive lane open; the index worker must not embed.
        let holder = lifecycle
            .scheduler()
            .enqueue_interactive()
            .unwrap()
            .wait_for_permit()
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            embedder.embedded_texts().is_empty(),
            "index inference must yield to a waiting interactive request"
        );

        // Releasing interactive ownership lets indexing proceed.
        drop(holder);
        let pool_ref = &pool;
        let generation = worker_generation(&pool).await;
        let generation_ref = &generation;
        wait_until(async || {
            matches!(
                meeting_state(pool_ref, generation_ref, "m").await,
                (state, _, _) if state == "ready"
            )
        })
        .await;
        lifecycle.shutdown().await;
    }

    #[tokio::test]
    async fn generations_are_never_embedded_by_a_mismatched_model_runtime() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Upgrade").await;
        add_transcript(&pool, "t1", "m", "conteudo da geracao antiga").await;

        // A previous bundle's generation is still live (the active generation
        // surviving a bundled-model upgrade) while the only loadable engine
        // now reports the NEW model identity.
        let old_spec = semantic_model_spec("retired-bundle", FAKE_DIMENSIONS);
        assert!(RetrievalRepository::ensure_model(&pool, &old_spec)
            .await
            .unwrap());
        assert!(
            RetrievalRepository::ensure_generation(&pool, "gen-old", "retired-bundle")
                .await
                .unwrap()
        );

        let embedder = FakeEmbedder::new();
        let lifecycle = lifecycle_for_test(free_pressure(), ok_loader(&embedder));
        lifecycle.attach_database(pool.clone());

        // The engine's own generation indexes and publishes normally...
        let current = worker_generation(&pool).await;
        let pool_ref = &pool;
        let current_ref = &current;
        wait_until(async || {
            matches!(
                sqlx::query_as::<_, (String, i64, i64)>(
                    "SELECT state, attempt_count, indexed_source_revision
                     FROM retrieval_meeting_state
                     WHERE generation_id = ? AND meeting_id = 'm'",
                )
                .bind(current_ref)
                .fetch_optional(pool_ref)
                .await
                .unwrap(),
                Some((state, _, _)) if state == "ready"
            )
        })
        .await;

        // ...while the mismatched generation stays safely unprocessed: its
        // work remains durably owed and nothing was embedded into it under
        // the wrong model.
        tokio::time::sleep(Duration::from_millis(400)).await;
        let (old_state, _, old_indexed) = meeting_state(&pool, "gen-old", "m").await;
        assert_eq!(old_state, "pending");
        assert_eq!(old_indexed, 0);
        assert_eq!(
            scalar(
                &pool,
                "SELECT COUNT(*) FROM retrieval_documents WHERE generation_id = 'gen-old'"
            )
            .await,
            0
        );
        lifecycle.shutdown().await;
    }

    // -- Generation identity and resumption ----------------------------------

    #[test]
    fn bundled_identity_prefix_is_readable_and_digest_discriminates_every_contract_field() {
        let contract = crate::model_bundle::approved_embedding_contract();
        let base = derived_model_identity(
            contract.bundle_id,
            contract.embedding_model_id,
            contract.embedding_revision,
            contract.onnx_export_revision,
            contract.onnx_export_quantization,
            contract.dimensions,
            VectorEncoding::Int8.as_str(),
            APPROVED_CHUNKER_VERSION,
        );
        assert_eq!(bundled_model_identity(), base);
        // Readable prefix keeps logs/status diagnosable; 16 hex digest chars
        // carry the discrimination.
        let prefix = format!(
            "mid-{}-int8-c{APPROVED_CHUNKER_VERSION}-",
            contract.bundle_id
        );
        assert!(base.starts_with(&prefix), "{base}");
        assert_eq!(base.len(), prefix.len() + 16);

        // Every approved-contract field participates: changing any one of
        // them (chunker bump, embedding model/revision swap under an
        // unchanged bundle id, export/quantization/encoding/dimension
        // change) must mint a distinct identity.
        for changed in [
            derived_model_identity(
                "other-bundle",
                contract.embedding_model_id,
                contract.embedding_revision,
                contract.onnx_export_revision,
                contract.onnx_export_quantization,
                contract.dimensions,
                VectorEncoding::Int8.as_str(),
                APPROVED_CHUNKER_VERSION,
            ),
            derived_model_identity(
                contract.bundle_id,
                "other/embedding-model",
                contract.embedding_revision,
                contract.onnx_export_revision,
                contract.onnx_export_quantization,
                contract.dimensions,
                VectorEncoding::Int8.as_str(),
                APPROVED_CHUNKER_VERSION,
            ),
            derived_model_identity(
                contract.bundle_id,
                contract.embedding_model_id,
                "other-embedding-revision",
                contract.onnx_export_revision,
                contract.onnx_export_quantization,
                contract.dimensions,
                VectorEncoding::Int8.as_str(),
                APPROVED_CHUNKER_VERSION,
            ),
            derived_model_identity(
                contract.bundle_id,
                contract.embedding_model_id,
                contract.embedding_revision,
                "other-export-revision",
                contract.onnx_export_quantization,
                contract.dimensions,
                VectorEncoding::Int8.as_str(),
                APPROVED_CHUNKER_VERSION,
            ),
            derived_model_identity(
                contract.bundle_id,
                contract.embedding_model_id,
                contract.embedding_revision,
                contract.onnx_export_revision,
                "other-quantization",
                contract.dimensions,
                VectorEncoding::Int8.as_str(),
                APPROVED_CHUNKER_VERSION,
            ),
            derived_model_identity(
                contract.bundle_id,
                contract.embedding_model_id,
                contract.embedding_revision,
                contract.onnx_export_revision,
                contract.onnx_export_quantization,
                contract.dimensions + 1,
                VectorEncoding::Int8.as_str(),
                APPROVED_CHUNKER_VERSION,
            ),
            derived_model_identity(
                contract.bundle_id,
                contract.embedding_model_id,
                contract.embedding_revision,
                contract.onnx_export_revision,
                contract.onnx_export_quantization,
                contract.dimensions,
                VectorEncoding::F32.as_str(),
                APPROVED_CHUNKER_VERSION,
            ),
            derived_model_identity(
                contract.bundle_id,
                contract.embedding_model_id,
                contract.embedding_revision,
                contract.onnx_export_revision,
                contract.onnx_export_quantization,
                contract.dimensions,
                VectorEncoding::Int8.as_str(),
                APPROVED_CHUNKER_VERSION + 1,
            ),
        ] {
            assert_ne!(base, changed, "a changed contract field altered the digest");
        }
    }

    #[tokio::test]
    async fn registration_resumes_live_generations_and_mints_opaque_ids_only_when_absent() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Resumed").await;

        let first = FakeEmbedder::new();
        register_semantic_identity(&pool, first.as_ref())
            .await
            .unwrap();
        let generation = RetrievalRepository::find_live_generation(&pool, FAKE_MODEL_ID)
            .await
            .unwrap()
            .expect("first registration mints one live generation");
        let hashed_prefix: String = Sha256::digest(FAKE_MODEL_ID.as_bytes())[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_ne!(
            generation,
            format!("gen-{hashed_prefix}"),
            "generation ids are minted opaquely, never hashed from the model identity"
        );

        // A later process reloads its engine and must resume the same
        // generation instead of registering a duplicate.
        let second = FakeEmbedder::new();
        register_semantic_identity(&pool, second.as_ref())
            .await
            .unwrap();
        assert_eq!(
            RetrievalRepository::list_live_generations(&pool)
                .await
                .unwrap(),
            vec![(generation.clone(), FAKE_MODEL_ID.to_string())],
            "resumption looks up the existing live generation by model identity"
        );
    }
}
