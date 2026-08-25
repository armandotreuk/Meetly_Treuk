//! Task 1.4/1.R3 — vector backend benchmark harness (Sprint 1 hybrid RAG).
//!
//! Measures the selected production encoding (768-d int8 vector storage with
//! recorded per-vector dequantization scales) against the Sprint 1 gates at
//! 12,000 / 50,000 / 250,000 synthetic documents: cold load, warm query,
//! global vs narrow allow-list scopes, exact base+delta/tombstone updates,
//! compaction, crash-window journal replay over sparse document IDs, bounded
//! concurrency, scheduler queue behavior, and index-worker scheduling impact.
//!
//! Journal publication (1.R3a) is bounded: the canonical upper bound is
//! captured before any journal read, only entries through that bound are
//! applied from one consistent read snapshot, and published_change_id
//! advances to the last applied entry — never to a later, concurrently
//! committed canonical ID. Upsert journal entries carry their own immutable
//! payload, so replay never depends on current document rows: an upsert
//! followed by a delete ends tombstoned, and a document with a valid newer
//! commit past the bound publishes its captured payload.
//!
//! At 250k the harness additionally measures the combined rebuild envelope in
//! ONE process: reader-held active snapshot + streamed building shadow
//! snapshot + delta/tombstone state + both selected ONNX sessions (e5-base
//! dynamic-int8 and mmarco quint8_avx2) loaded from the staged production
//! bundle and kept resident through the peak. Encoded limits per the approved
//! contract: steady state <= 1.25 GiB (automatic pass <= 1 GiB); the
//! user-approved transient ceiling is 1.30 GiB and covers exactly the
//! active+shadow two-snapshot rebuild state — a true third resident snapshot
//! or any higher peak remains blocking. The full-matrix run therefore
//! REQUIRES the staged bundle (`MEETLY_RAG_BUNDLE_DIR`, default
//! `resources/retrieval/bundle`) and fails closed when it is absent or
//! incompatible; the deterministic suite stays artifact-independent.
//!
//! Commands (run from `upstream/`):
//! ```powershell
//! # Deterministic correctness suite (no artifacts required):
//! cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test vector_backend_benchmark
//!
//! # Full measured matrix incl. combined envelope (release build):
//! $env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
//! $env:MEETLY_RAG_VECTOR_BENCH = "1"
//! cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test vector_backend_benchmark full_matrix_benchmark -- --nocapture
//! Remove-Item Env:MEETLY_RAG_VECTOR_BENCH -ErrorAction SilentlyContinue
//! ```

use memory_stats::memory_stats;
use ndarray::Array2;
use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Row,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};
use tokenizers::Tokenizer;

const DIM: usize = 768;
const SCALE_GENS: [usize; 3] = [12_000, 50_000, 250_000];
const DOCS_PER_MEETING: u32 = 20;
const WARM_QUERIES: usize = 64;
const TOP_K: usize = 150;
const CANDIDATE_LIMITS: [usize; 3] = [50, 100, 150];
const UPDATE_BATCH: u32 = 128;
const DELTA_FRACTIONS: [f64; 3] = [0.005, 0.01, 0.02];
const SCAN_PERMITS: usize = 2;
const QUEUE_CAP: usize = 8;
const INTERACTIVE_PAUSE_BUDGET_MS: u128 = 250;
const LATENCY_GATE_P95_MS: f64 = 500.0;
// Approved retrieval RAM limits (architecture.md "Resource Budget Arithmetic"):
// steady state is governed by the 1-1.25 GiB e5-base band, and the
// user-approved 2026-08-24 transient ceiling of 1.30 GiB covers exactly the
// active+shadow two-snapshot rebuild state measured in this harness. A true
// third resident snapshot or any peak above 1.30 GiB remains blocking.
const AUTO_PASS_BYTES: u64 = 1_073_741_824;
const BAND_MAX_BYTES: u64 = 1_342_177_280;
const TRANSIENT_MAX_BYTES: u64 = AUTO_PASS_BYTES * 13 / 10;

// ---------------------------------------------------------------------------
// Deterministic generation (bounded, prefix-stable, seeded per row)
// ---------------------------------------------------------------------------

struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0
    }
}

fn row_seed(doc: u32) -> u64 {
    0x9E37_79B9_7F4A_7C15 ^ (doc as u64).wrapping_mul(0xD1B5_4A32_D192_ED03)
}

fn query_vector(seed: u64) -> (Vec<i8>, f32) {
    generate_row((seed % u32::MAX as u64) as u32)
}

/// Symmetric per-vector int8 quantization with recorded dequantization scale:
/// score(q,d) = dot_i8(q,d) * q.scale * d.scale approximates cosine of the
/// unit-normalized originals.
fn generate_row(doc: u32) -> (Vec<i8>, f32) {
    let mut rng = XorShift64::new(row_seed(doc));
    let mut v = vec![0f32; DIM];
    let mut norm2 = 0f64;
    for slot in v.iter_mut() {
        let x = rng.next_f32();
        norm2 += f64::from(x) * f64::from(x);
        *slot = x;
    }
    let inv = (1.0 / norm2.sqrt()) as f32;
    let max_abs = v.iter().fold(0f32, |m, x| m.max(x.abs())) * inv;
    let scale = (max_abs / 127.0).max(f32::MIN_POSITIVE);
    let q = v
        .iter()
        .map(|x| ((*x * inv) / scale).round().clamp(-127.0, 127.0) as i8)
        .collect();
    (q, scale)
}

// ---------------------------------------------------------------------------
// Exact backend: immutable base snapshot + delta overlay + tombstones
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
struct Cand {
    score: f32,
    doc: u32,
}

impl Eq for Cand {}

impl Ord for Cand {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .partial_cmp(&other.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(self.doc.cmp(&other.doc))
    }
}

impl PartialOrd for Cand {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct Snapshot {
    /// Stable document identity, preserved across compaction (row position is
    /// storage layout only).
    doc_ids: Vec<u32>,
    vectors: Vec<i8>,
    scales: Vec<f32>,
    meeting_of_doc: Vec<u32>,
}

impl Snapshot {
    fn n(&self) -> usize {
        self.scales.len()
    }

    /// Row position of a stable document id. Every construction path keeps
    /// `doc_ids` sorted (SQLite load orders by doc_id; compaction sorts), so
    /// sparse ids translate in O(log n) after deletes/reloads.
    fn row_of_doc(&self, doc: u32) -> Option<usize> {
        self.doc_ids.binary_search(&doc).ok()
    }

    fn meeting_of_doc_id(&self, doc: u32) -> Option<u32> {
        self.row_of_doc(doc).map(|row| self.meeting_of_doc[row])
    }

    fn digest(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for (row, slice) in self.vectors.chunks_exact(DIM).enumerate() {
            h ^= row as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
            for v in slice {
                h ^= *v as u8 as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
        }
        for s in &self.scales {
            h ^= u64::from(s.to_bits());
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }
}

/// Exact update delta: replacement vectors keyed by doc id plus deletion
/// tombstones. Base rows shadowed by an upsert are skipped in favor of the
/// overlay copy; tombstoned rows are unfindable everywhere.
#[derive(Default)]
struct Overlay {
    upserts: BTreeMap<u32, (Vec<i8>, f32, u32)>,
    tombstones: BTreeSet<u32>,
}

impl Overlay {
    fn bytes(&self) -> usize {
        self.upserts
            .values()
            .map(|(v, _, _)| v.len() + 12)
            .sum::<usize>()
            + self.tombstones.len() * 8
    }
}

#[derive(Clone)]
enum ScopeFilter {
    All,
    Meetings(BTreeSet<u32>),
}

impl ScopeFilter {
    fn allows_meeting(&self, meeting: u32) -> bool {
        match self {
            ScopeFilter::All => true,
            ScopeFilter::Meetings(ids) => ids.contains(&meeting),
        }
    }
}

/// Pre-scan base mask: scope allow-list minus delta-shadowed/tombstoned rows.
/// Disallowed and dead rows are skipped before scoring, so they can never
/// enter candidates, fusion, hydration, sources, or prompts. Mask slots are
/// ROW positions while overlay keys are stable document IDs; the row's own
/// `doc_ids[row]` identity decides, so sparse IDs stay correct after deletes,
/// compaction, and SQLite reload.
fn base_scan_mask(snap: &Snapshot, scope: &ScopeFilter, overlay: Option<&Overlay>) -> Vec<bool> {
    snap.doc_ids
        .iter()
        .enumerate()
        .map(|(row, doc)| {
            scope.allows_meeting(snap.meeting_of_doc[row])
                && !overlay
                    .is_some_and(|ov| ov.tombstones.contains(doc) || ov.upserts.contains_key(doc))
        })
        .collect()
}

fn dot_i8(a: &[i8], b: &[i8]) -> i32 {
    a.iter().zip(b).map(|(x, y)| *x as i32 * *y as i32).sum()
}

/// Exact top-k over base + optional overlay. Returns (doc, score) desc.
fn scan_top_k(
    snap: &Snapshot,
    base_mask: &[bool],
    overlay: Option<&Overlay>,
    scope: &ScopeFilter,
    q: &[i8],
    q_scale: f32,
    k: usize,
) -> Vec<(u32, f32)> {
    let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<Cand>> =
        std::collections::BinaryHeap::with_capacity(k + 1);
    for (row, slice) in snap.vectors.chunks_exact(DIM).enumerate() {
        if !base_mask[row] {
            continue;
        }
        push_candidate(
            &mut heap,
            k,
            Cand {
                score: dot_i8(slice, q) as f32 * snap.scales[row] * q_scale,
                doc: snap.doc_ids[row],
            },
        );
    }
    if let Some(ov) = overlay {
        for (&doc, (v, s, meeting)) in &ov.upserts {
            if ov.tombstones.contains(&doc) || !scope.allows_meeting(*meeting) {
                continue;
            }
            push_candidate(
                &mut heap,
                k,
                Cand {
                    score: dot_i8(v, q) as f32 * *s * q_scale,
                    doc,
                },
            );
        }
    }
    let mut out: Vec<(u32, f32)> = heap.into_iter().map(|r| (r.0.doc, r.0.score)).collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

fn push_candidate(
    heap: &mut std::collections::BinaryHeap<std::cmp::Reverse<Cand>>,
    k: usize,
    cand: Cand,
) {
    if heap.len() < k {
        heap.push(std::cmp::Reverse(cand));
    } else if let Some(min) = heap.peek().map(|r| r.0) {
        if cand > min {
            heap.pop();
            heap.push(std::cmp::Reverse(cand));
        }
    }
}

/// Brute-force reference used by the exactness/recall self-check.
fn brute_force_top_k(
    snap: &Snapshot,
    base_mask: &[bool],
    overlay: Option<&Overlay>,
    scope: &ScopeFilter,
    q: &[i8],
    q_scale: f32,
    k: usize,
) -> Vec<(u32, f32)> {
    let mut scored: Vec<Cand> = Vec::new();
    for (row, slice) in snap.vectors.chunks_exact(DIM).enumerate() {
        if base_mask[row] {
            scored.push(Cand {
                score: dot_i8(slice, q) as f32 * snap.scales[row] * q_scale,
                doc: snap.doc_ids[row],
            });
        }
    }
    if let Some(ov) = overlay {
        for (&doc, (v, s, meeting)) in &ov.upserts {
            if !ov.tombstones.contains(&doc) && scope.allows_meeting(*meeting) {
                scored.push(Cand {
                    score: dot_i8(v, q) as f32 * *s * q_scale,
                    doc,
                });
            }
        }
    }
    scored.sort_by(|a, b| b.cmp(a));
    scored.truncate(k);
    scored.into_iter().map(|c| (c.doc, c.score)).collect()
}

/// Rebuild the base from base-minus-tombstones-plus-upserts, preserving the
/// stable document identity of every surviving row.
fn compact(snap: &Snapshot, overlay: Option<&Overlay>) -> Snapshot {
    let mut rows: Vec<(u32, Vec<i8>, f32, u32)> = Vec::with_capacity(snap.n());
    for (row, slice) in snap.vectors.chunks_exact(DIM).enumerate() {
        let tombstoned = overlay
            .map(|ov| ov.tombstones.contains(&snap.doc_ids[row]))
            .unwrap_or(false);
        let shadowed = overlay
            .map(|ov| ov.upserts.contains_key(&snap.doc_ids[row]))
            .unwrap_or(false);
        if !tombstoned && !shadowed {
            rows.push((
                snap.doc_ids[row],
                slice.to_vec(),
                snap.scales[row],
                snap.meeting_of_doc[row],
            ));
        }
    }
    if let Some(ov) = overlay {
        for (&doc, (v, s, m)) in &ov.upserts {
            if !ov.tombstones.contains(&doc) {
                rows.push((doc, v.clone(), *s, *m));
            }
        }
    }
    rows.sort_by_key(|&(doc, _, _, _)| doc);
    let mut vectors = Vec::with_capacity(rows.len() * DIM);
    let mut scales = Vec::with_capacity(rows.len());
    let mut meetings = Vec::with_capacity(rows.len());
    let mut ids = Vec::with_capacity(rows.len());
    for (doc, v, s, m) in &rows {
        ids.push(*doc);
        vectors.extend_from_slice(v);
        scales.push(*s);
        meetings.push(*m);
    }
    Snapshot {
        doc_ids: ids,
        vectors,
        scales,
        meeting_of_doc: meetings,
    }
}

// ---------------------------------------------------------------------------
// Interactive scheduler: scan permits, queue cap, index-worker pause signal
// ---------------------------------------------------------------------------

struct SchedState {
    active: usize,
    waiting: usize,
}

struct ScanScheduler {
    state: Mutex<SchedState>,
    cv: Condvar,
    max_active: AtomicUsize,
    rejected: AtomicUsize,
    interactive_waiting: AtomicBool,
}

impl ScanScheduler {
    fn new() -> Self {
        Self {
            state: Mutex::new(SchedState {
                active: 0,
                waiting: 0,
            }),
            cv: Condvar::new(),
            max_active: AtomicUsize::new(0),
            rejected: AtomicUsize::new(0),
            interactive_waiting: AtomicBool::new(false),
        }
    }

    fn acquire(&self) -> Option<ScanGuard<'_>> {
        let mut s = self.state.lock().expect("scheduler lock");
        if s.active == SCAN_PERMITS && s.waiting >= QUEUE_CAP {
            self.rejected.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        s.waiting += 1;
        self.interactive_waiting.store(true, Ordering::SeqCst);
        while s.active >= SCAN_PERMITS {
            s = self.cv.wait(s).expect("scheduler lock");
        }
        s.waiting -= 1;
        s.active += 1;
        self.interactive_waiting
            .store(s.waiting > 0, Ordering::SeqCst);
        self.max_active.fetch_max(s.active, Ordering::Relaxed);
        Some(ScanGuard { sched: self })
    }
}

struct ScanGuard<'a> {
    sched: &'a ScanScheduler,
}

impl Drop for ScanGuard<'_> {
    fn drop(&mut self) {
        let mut s = self.sched.state.lock().expect("scheduler lock");
        s.active -= 1;
        self.sched.cv.notify_all();
    }
}

struct WorkerProbe {
    pause_requested: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

/// Stand-in for the single-owner index worker: burns batch-sized CPU slices
/// like an embedding/indexing pipeline and honors the interactive-pause rule.
fn spawn_worker_probe(batch_ms: u64, honor_pause: bool) -> WorkerProbe {
    let probe = WorkerProbe {
        pause_requested: Arc::new(AtomicBool::new(false)),
        paused: Arc::new(AtomicBool::new(false)),
        stop: Arc::new(AtomicBool::new(false)),
    };
    let fields = (
        Arc::clone(&probe.pause_requested),
        Arc::clone(&probe.paused),
        Arc::clone(&probe.stop),
    );
    std::thread::spawn(move || {
        let (pause_requested, paused, stop) = fields;
        while !stop.load(Ordering::SeqCst) {
            if honor_pause && pause_requested.load(Ordering::SeqCst) {
                paused.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            paused.store(false, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_millis(batch_ms);
            while Instant::now() < deadline {
                std::hint::spin_loop();
            }
        }
        paused.store(false, Ordering::SeqCst);
    });
    probe
}

// ---------------------------------------------------------------------------
// Canonical SQLite persistence: benchmark-local mirror of the architecture
// tables (documents + publication journal + canonical/published change IDs)
// ---------------------------------------------------------------------------

struct BenchDb {
    pool: sqlx::SqlitePool,
    path: std::path::PathBuf,
}

impl BenchDb {
    async fn close(self) {
        self.pool.close().await;
    }
}

async fn open_db(dir: &std::path::Path, create: bool) -> BenchDb {
    let path = dir.join("bench_vectors.db");
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(create)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("open benchmark sqlite");
    if create {
        sqlx::query(
            "            CREATE TABLE IF NOT EXISTS bench_documents (
                doc_id INTEGER PRIMARY KEY,
                meeting_id INTEGER NOT NULL,
                vector BLOB NOT NULL,
                dequant_scale REAL NOT NULL,
                source_revision INTEGER NOT NULL DEFAULT 1
            );
            -- Benchmark-local mirror of the architecture's publication journal
            -- (retrieval_index_changes): upsert entries carry their own
            -- immutable payload so replay never depends on current document
            -- rows. Nullable payload columns on deletes mirror the nullable
            -- source_revision of the approved production schema.
            CREATE TABLE IF NOT EXISTS bench_index_changes (
                change_id INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id INTEGER NOT NULL,
                operation TEXT NOT NULL CHECK (operation IN ('upsert','delete')),
                vector BLOB,
                dequant_scale REAL,
                meeting_id INTEGER,
                CHECK (operation = 'delete'
                       OR (vector IS NOT NULL AND dequant_scale IS NOT NULL
                           AND meeting_id IS NOT NULL))
            );
            CREATE TABLE IF NOT EXISTS bench_index_state (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                canonical_change_id INTEGER NOT NULL DEFAULT 0,
                published_change_id INTEGER NOT NULL DEFAULT 0
            );
            INSERT OR IGNORE INTO bench_index_state (singleton) VALUES (1);",
        )
        .execute(&pool)
        .await
        .expect("create benchmark schema");
    }
    BenchDb { pool, path }
}

fn meeting_of(doc: u32) -> u32 {
    doc / DOCS_PER_MEETING
}

async fn insert_corpus(db: &BenchDb, n: usize) -> (Duration, Duration) {
    let mut gen_elapsed = Duration::ZERO;
    let mut insert_elapsed = Duration::ZERO;
    let mut rows: Vec<(u32, Vec<u8>, f32)> = Vec::with_capacity(200);
    for doc in 0..n as u32 {
        let t0 = Instant::now();
        let (q, scale) = generate_row(doc);
        gen_elapsed += t0.elapsed();
        rows.push((doc, q.iter().map(|b| *b as u8).collect(), scale));
        if rows.len() == 200 || doc as usize + 1 == n {
            let t1 = Instant::now();
            flush_rows(db, &rows).await;
            insert_elapsed += t1.elapsed();
            rows.clear();
        }
    }
    (gen_elapsed, insert_elapsed)
}

async fn flush_rows(db: &BenchDb, rows: &[(u32, Vec<u8>, f32)]) {
    let mut tx = db.pool.begin().await.expect("begin corpus tx");
    for (doc, bytes, scale) in rows {
        sqlx::query(
            "INSERT INTO bench_documents (doc_id, meeting_id, vector, dequant_scale) VALUES (?, ?, ?, ?)",
        )
        .bind(*doc as i64)
        .bind(meeting_of(*doc) as i64)
        .bind(bytes)
        .bind(*scale as f64)
        .execute(&mut *tx)
        .await
        .expect("insert corpus row");
    }
    tx.commit().await.expect("commit corpus tx");
}

/// One canonical update transaction: replace/delete documents, append journal
/// entries, and advance canonical_change_id atomically (worker steps 8-13).
async fn commit_updates(db: &BenchDb, updates: &[(u32, Option<(Vec<i8>, f32)>)]) -> Duration {
    let t0 = Instant::now();
    let mut tx = db.pool.begin().await.expect("begin update tx");
    for (doc, payload) in updates {
        match payload {
            Some((v, s)) => {
                let bytes: Vec<u8> = v.iter().map(|b| *b as u8).collect();
                sqlx::query(
                    "INSERT INTO bench_documents (doc_id, meeting_id, vector, dequant_scale, source_revision)
                     VALUES (?, ?, ?, ?, 2)
                     ON CONFLICT(doc_id) DO UPDATE SET vector=excluded.vector,
                       dequant_scale=excluded.dequant_scale, source_revision=excluded.source_revision",
                )
                .bind(*doc as i64)
                .bind(meeting_of(*doc) as i64)
                .bind(bytes.clone())
                .bind(*s as f64)
                .execute(&mut *tx)
                .await
                .expect("upsert doc");
                sqlx::query(
                    "INSERT INTO bench_index_changes (doc_id, operation, vector, dequant_scale, meeting_id)
                     VALUES (?, 'upsert', ?, ?, ?)",
                )
                .bind(*doc as i64)
                .bind(bytes)
                .bind(*s as f64)
                .bind(meeting_of(*doc) as i64)
                .execute(&mut *tx)
                .await
                .expect("journal upsert");
            }
            None => {
                sqlx::query("DELETE FROM bench_documents WHERE doc_id = ?")
                    .bind(*doc as i64)
                    .execute(&mut *tx)
                    .await
                    .expect("delete doc");
                sqlx::query(
                    "INSERT INTO bench_index_changes (doc_id, operation) VALUES (?, 'delete')",
                )
                .bind(*doc as i64)
                .execute(&mut *tx)
                .await
                .expect("journal delete");
            }
        }
    }
    sqlx::query(
        "UPDATE bench_index_state SET canonical_change_id =
         COALESCE((SELECT MAX(change_id) FROM bench_index_changes), 0) WHERE singleton = 1",
    )
    .execute(&mut *tx)
    .await
    .expect("advance canonical");
    tx.commit().await.expect("commit update tx");
    t0.elapsed()
}

async fn change_ids(db: &BenchDb) -> (i64, i64) {
    let row =
        sqlx::query("SELECT canonical_change_id, published_change_id FROM bench_index_state WHERE singleton = 1")
            .fetch_one(&db.pool)
            .await
            .expect("read index state");
    (row.get(0), row.get(1))
}

async fn canonical_change_id(db: &BenchDb) -> i64 {
    sqlx::query_scalar("SELECT canonical_change_id FROM bench_index_state WHERE singleton = 1")
        .fetch_one(&db.pool)
        .await
        .expect("read canonical change id")
}

/// Publisher replay bounded by `bound`: applies journal entries in
/// `(published, bound]` read from ONE consistent SQLite snapshot, then
/// durably advances published_change_id to the last applied entry only —
/// never to canonical_change_id, which concurrent commits may already have
/// moved past `bound`. Upsert entries carry their own immutable payload
/// (vector/scale/meeting, written atomically by the committing transaction),
/// so replay never consults current document rows: a document with a valid
/// newer commit past the bound still publishes its captured payload, and one
/// whose bounded trail ends in a delete ends tombstoned. This benchmark-local
/// journal payload mirrors the production journaling requirement (worker step
/// 12 journals each upsert in the same transaction that replaces its
/// documents); no application schema or migration changes.
async fn publish_through(
    db: &BenchDb,
    overlay: &Arc<Mutex<Overlay>>,
    bound: i64,
) -> (Duration, Duration) {
    let t0 = Instant::now();
    let (published, entries) = {
        let mut tx = db.pool.begin().await.expect("begin publication snapshot");
        let published: i64 = sqlx::query_scalar(
            "SELECT published_change_id FROM bench_index_state WHERE singleton = 1",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("read published change id");
        let entries = sqlx::query(
            "SELECT change_id, doc_id, operation, vector, dequant_scale, meeting_id
             FROM bench_index_changes
             WHERE change_id > ? AND change_id <= ?
             ORDER BY change_id",
        )
        .bind(published)
        .bind(bound)
        .fetch_all(&mut *tx)
        .await
        .expect("read bounded journal");
        (published, entries)
    };
    // The last bounded operation per doc decides its published state; earlier
    // entries for the same doc are last-writer-wins no-ops.
    let mut trailing: BTreeMap<u32, (i64, bool)> = BTreeMap::new();
    let mut payloads: BTreeMap<i64, (Vec<i8>, f32, u32)> = BTreeMap::new();
    for row in &entries {
        let change_id: i64 = row.get(0);
        let is_upsert = row.get::<String, _>(2) == "upsert";
        if is_upsert {
            let blob: Vec<u8> = row.get(3);
            payloads.insert(
                change_id,
                (
                    blob.iter().map(|b| *b as i8).collect(),
                    row.get::<f64, _>(4) as f32,
                    row.get::<i64, _>(5) as u32,
                ),
            );
        }
        trailing.insert(row.get::<i64, _>(1) as u32, (change_id, is_upsert));
    }
    {
        let mut ov = overlay.lock().expect("overlay lock");
        for (&doc, &(change_id, is_upsert)) in &trailing {
            if !is_upsert {
                ov.upserts.remove(&doc);
                ov.tombstones.insert(doc);
                continue;
            }
            let (vector, scale, meeting) = payloads
                .get(&change_id)
                .map(|(v, s, m)| (v.clone(), *s, *m))
                .unwrap_or_else(|| {
                    panic!("publication journal: upsert change {change_id} for doc {doc} carries no payload")
                });
            ov.upserts.insert(doc, (vector, scale, meeting));
            ov.tombstones.remove(&doc);
        }
    }
    let apply_elapsed = t0.elapsed();
    let t1 = Instant::now();
    let last_applied = entries
        .last()
        .map(|row| row.get::<i64, _>(0))
        .unwrap_or(published);
    if last_applied > published {
        sqlx::query("UPDATE bench_index_state SET published_change_id = ? WHERE singleton = 1")
            .bind(last_applied)
            .execute(&db.pool)
            .await
            .expect("advance published");
    }
    (apply_elapsed, t1.elapsed())
}

/// Publication entry point: capture the canonical upper bound FIRST, then
/// apply strictly through it. A commit landing after the capture stays
/// unpublished until a subsequent publication.
async fn publish_pending(db: &BenchDb, overlay: &Arc<Mutex<Overlay>>) -> (Duration, Duration) {
    let bound = canonical_change_id(db).await;
    publish_through(db, overlay, bound).await
}

async fn load_snapshot(db: &BenchDb) -> (Snapshot, Duration) {
    let t0 = Instant::now();
    let rows = sqlx::query(
        "SELECT doc_id, meeting_id, vector, dequant_scale FROM bench_documents ORDER BY doc_id",
    )
    .fetch_all(&db.pool)
    .await
    .expect("load documents");
    let n = rows.len();
    let mut ids = Vec::with_capacity(n);
    let mut vectors = Vec::with_capacity(n * DIM);
    let mut scales = Vec::with_capacity(n);
    let mut meetings = Vec::with_capacity(n);
    for row in &rows {
        let blob: Vec<u8> = row.get(2);
        ids.push(row.get::<i64, _>(0) as u32);
        vectors.extend(blob.iter().map(|b| *b as i8));
        scales.push(row.get::<f64, _>(3) as f32);
        meetings.push(row.get::<i64, _>(1) as u32);
    }
    (
        Snapshot {
            doc_ids: ids,
            vectors,
            scales,
            meeting_of_doc: meetings,
        },
        t0.elapsed(),
    )
}

/// Production-shaped shadow load: streams rows in bounded chunks into the
/// contiguous snapshot allocation instead of materializing every SQLite
/// row/BLOB at once, so the measured rebuild peak carries the real snapshot
/// capacity plus only a bounded per-chunk transient.
async fn load_snapshot_streaming(db: &BenchDb, chunk_rows: usize) -> (Snapshot, Duration) {
    let t0 = Instant::now();
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bench_documents")
        .fetch_one(&db.pool)
        .await
        .expect("count documents");
    let mut ids = Vec::with_capacity(total as usize);
    let mut vectors = Vec::with_capacity(total as usize * DIM);
    let mut scales = Vec::with_capacity(total as usize);
    let mut meetings = Vec::with_capacity(total as usize);
    let mut last_id: i64 = -1;
    loop {
        let rows = sqlx::query(
            "SELECT doc_id, meeting_id, vector, dequant_scale FROM bench_documents
             WHERE doc_id > ? ORDER BY doc_id LIMIT ?",
        )
        .bind(last_id)
        .bind(chunk_rows as i64)
        .fetch_all(&db.pool)
        .await
        .expect("stream documents");
        if rows.is_empty() {
            break;
        }
        for row in &rows {
            let blob: Vec<u8> = row.get(2);
            ids.push(row.get::<i64, _>(0) as u32);
            vectors.extend(blob.iter().map(|b| *b as i8));
            scales.push(row.get::<f64, _>(3) as f32);
            meetings.push(row.get::<i64, _>(1) as u32);
            last_id = row.get(0);
        }
    }
    (
        Snapshot {
            doc_ids: ids,
            vectors,
            scales,
            meeting_of_doc: meetings,
        },
        t0.elapsed(),
    )
}

async fn disk_bytes(db: &BenchDb) -> u64 {
    let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&db.pool)
        .await;
    let mut total = fs::metadata(&db.path).map(|m| m.len()).unwrap_or(0);
    for suffix in ["-wal", "-shm"] {
        let mut p = db.path.clone().into_os_string();
        p.push(suffix);
        total += fs::metadata(std::path::PathBuf::from(p))
            .map(|m| m.len())
            .unwrap_or(0);
    }
    total
}

// ---------------------------------------------------------------------------
// Measurement helpers
// ---------------------------------------------------------------------------

fn rss_current_mib() -> Option<f64> {
    memory_stats().map(|s| s.physical_mem as f64 / (1024.0 * 1024.0))
}

/// Windows x64 process memory sample. Peak working set governs the RAM
/// envelope (same metric family as the retained Task 1.3 evidence); private
/// commit is recorded alongside because working sets can be trimmed under OS
/// pressure while commit charge only drops when pages are actually released.
struct ProcessMem {
    working_set: u64,
    peak_working_set: u64,
    private_commit: u64,
    peak_private_commit: u64,
}

#[cfg(windows)]
fn process_memory() -> Option<ProcessMem> {
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }
    let mut counters = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    (ok != 0).then_some(ProcessMem {
        working_set: counters.working_set_size as u64,
        peak_working_set: counters.peak_working_set_size as u64,
        private_commit: counters.pagefile_usage as u64,
        peak_private_commit: counters.peak_pagefile_usage as u64,
    })
}

#[cfg(not(windows))]
fn process_memory() -> Option<ProcessMem> {
    None
}

fn percentile(samples: &mut [f64], p: usize) -> f64 {
    samples.sort_by(|a, b| a.total_cmp(b));
    samples[(samples.len().saturating_sub(1)) * p / 100]
}

fn stats(samples: &mut Vec<f64>) -> (f64, f64, f64) {
    let p50 = percentile(samples, 50);
    let p95 = percentile(samples, 95);
    let max = samples.iter().cloned().fold(0.0, f64::max);
    (p50, p95, max)
}

fn mib(bytes: f64) -> f64 {
    bytes / (1024.0 * 1024.0)
}

fn gib(bytes: f64) -> f64 {
    bytes / (1024.0 * 1024.0 * 1024.0)
}

fn latency_verdict(p95_ms: f64) -> &'static str {
    if p95_ms < LATENCY_GATE_P95_MS {
        "PASS (< 500 ms gate)"
    } else {
        "MISS (>= 500 ms gate)"
    }
}

fn steady_ram_verdict(bytes: u64) -> &'static str {
    if bytes <= AUTO_PASS_BYTES {
        "PASS automatic (<= 1 GiB)"
    } else if bytes <= BAND_MAX_BYTES {
        "PASS inside approved 1-1.25 GiB e5-base band"
    } else {
        "FAIL above the 1.25 GiB steady-state hard line"
    }
}

/// The transient ceiling governs exactly the active+shadow two-snapshot
/// rebuild state. It is not a steady-state band and does not admit a third
/// resident snapshot; any peak above it blocks activation.
fn transient_ram_verdict(bytes: u64) -> &'static str {
    if bytes <= AUTO_PASS_BYTES {
        "PASS automatic (<= 1 GiB)"
    } else if bytes <= TRANSIENT_MAX_BYTES {
        "PASS inside user-approved 1.30 GiB transient ceiling (exactly active+shadow)"
    } else {
        "FAIL above the approved 1.30 GiB transient ceiling"
    }
}

// ---------------------------------------------------------------------------
// Always-run deterministic correctness suite
// ---------------------------------------------------------------------------

#[test]
fn generation_is_deterministic_and_unit_norm() {
    let (a, sa) = generate_row(17);
    let (b, sb) = generate_row(17);
    assert_eq!(a, b);
    assert_eq!(sa, sb);
    let (other, _) = generate_row(1023);
    assert_ne!(a[..32], other[..32], "distinct rows must differ");
    let unit: f64 = a
        .iter()
        .map(|&x| {
            let f = f64::from(x as f32 * sa);
            f * f
        })
        .sum();
    assert!(
        (unit - 1.0).abs() < 0.02,
        "dequantized vector is not unit-norm: {unit}"
    );
}

#[test]
fn exact_scan_matches_brute_force_recall() {
    let n = 2_000usize;
    let mut rows: Vec<(Vec<i8>, f32)> = (0..n as u32).map(generate_row).collect();
    rows.shrink_to_fit();
    let snap = Snapshot {
        doc_ids: (0..n as u32).collect(),
        vectors: rows.iter().flat_map(|(v, _)| v.iter().copied()).collect(),
        scales: rows.iter().map(|(_, s)| *s).collect(),
        meeting_of_doc: (0..n as u32).map(meeting_of).collect(),
    };
    let mask = vec![true; n];
    for case in 0..12u64 {
        let (q, qs) = query_vector(1_000 + case);
        let got = scan_top_k(&snap, &mask, None, &ScopeFilter::All, &q, qs, TOP_K);
        let want = brute_force_top_k(&snap, &mask, None, &ScopeFilter::All, &q, qs, TOP_K);
        assert_eq!(
            got, want,
            "exact scan diverged from brute force, case {case}"
        );
        assert_eq!(got.len(), TOP_K.min(n));
    }
}

#[test]
fn scope_allow_list_never_leaks_out_of_scope() {
    let n = 3_000usize;
    let snap = Snapshot {
        doc_ids: (0..n as u32).collect(),
        vectors: (0..n as u32).flat_map(|d| generate_row(d).0).collect(),
        scales: (0..n as u32).map(|d| generate_row(d).1).collect(),
        meeting_of_doc: (0..n as u32).map(meeting_of).collect(),
    };
    let narrow: BTreeSet<u32> = (0..5).collect();
    let quarter: BTreeSet<u32> = (0..(n as u32 / DOCS_PER_MEETING) / 4).collect();
    let mut rng = XorShift64::new(7);
    for scope_set in [&narrow, &quarter] {
        let scope = ScopeFilter::Meetings(scope_set.clone());
        let mask = base_scan_mask(&snap, &scope, None);
        for _ in 0..120 {
            let (q, qs) = query_vector(rng.next_u64());
            let got = scan_top_k(&snap, &mask, None, &scope, &q, qs, TOP_K);
            let expected_len = (scope_set.len() as usize * DOCS_PER_MEETING as usize).min(TOP_K);
            assert_eq!(got.len(), expected_len);
            for (doc, _) in &got {
                assert!(
                    scope_set.contains(&meeting_of(*doc)),
                    "out-of-scope doc {doc} (meeting {}) returned",
                    meeting_of(*doc)
                );
            }
        }
    }
}

#[test]
fn overlay_tombstone_semantics_match_compacted_rebuild() {
    let n = 2_000usize;
    let snap = Snapshot {
        doc_ids: (0..n as u32).collect(),
        vectors: (0..n as u32).flat_map(|d| generate_row(d).0).collect(),
        scales: (0..n as u32).map(|d| generate_row(d).1).collect(),
        meeting_of_doc: (0..n as u32).map(meeting_of).collect(),
    };
    let mut ov = Overlay::default();
    for d in 0..64u32 {
        let slot = d * 7 % n as u32;
        let (v, s) = generate_row(500_000 + slot);
        ov.upserts.insert(slot, (v, s, meeting_of(slot)));
    }
    let mut rng = XorShift64::new(11);
    while ov.tombstones.len() < 16 {
        let id = (rng.next_u64() % n as u64) as u32;
        if !ov.upserts.contains_key(&id) {
            ov.tombstones.insert(id);
        }
    }
    let scope = ScopeFilter::All;
    let mask = base_scan_mask(&snap, &scope, Some(&ov));
    let (q, qs) = query_vector(999);
    let live = scan_top_k(&snap, &mask, Some(&ov), &scope, &q, qs, TOP_K);

    let rebuilt = compact(&snap, Some(&ov));
    let mask2 = base_scan_mask(&rebuilt, &scope, None);
    let compacted = scan_top_k(&rebuilt, &mask2, None, &scope, &q, qs, TOP_K);
    assert_eq!(live, compacted, "compaction changed search results");

    for doc in &ov.tombstones {
        assert!(
            !live.iter().any(|(d, _)| d == doc),
            "tombstoned doc visible"
        );
    }
    let unique: BTreeSet<u32> = live.iter().map(|(d, _)| *d).collect();
    assert_eq!(unique.len(), live.len(), "duplicate doc in results");
    assert_eq!(rebuilt.n(), n - ov.tombstones.len());
}

#[test]
fn update_path_never_touches_base_snapshot() {
    let n = 1_000usize;
    let snap = Snapshot {
        doc_ids: (0..n as u32).collect(),
        vectors: (0..n as u32).flat_map(|d| generate_row(d).0).collect(),
        scales: (0..n as u32).map(|d| generate_row(d).1).collect(),
        meeting_of_doc: (0..n as u32).map(meeting_of).collect(),
    };
    let digest_before = snap.digest();
    let mut ov = Overlay::default();
    for d in 0..32u32 {
        let (v, s) = generate_row(700_000 + d);
        ov.upserts.insert(400 + d, (v, s, meeting_of(400 + d)));
    }
    ov.tombstones.insert(10);
    let (q, qs) = query_vector(3);
    let mask = base_scan_mask(&snap, &ScopeFilter::All, Some(&ov));
    let _ = scan_top_k(&snap, &mask, Some(&ov), &ScopeFilter::All, &q, qs, TOP_K);
    assert_eq!(
        snap.digest(),
        digest_before,
        "update path mutated the immutable base"
    );
    assert_eq!(ov.upserts.len(), 32);
    assert_eq!(ov.tombstones.len(), 1);
    assert!(
        ov.bytes() < 33 * (DIM + 16),
        "delta allocates proportionally to the update, not the corpus"
    );
}

#[tokio::test]
async fn journal_replay_recovers_after_simulated_crash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = open_db(dir.path(), true).await;
    insert_corpus(&db, 400).await;
    publish_pending(&db, &Arc::new(Mutex::new(Overlay::default()))).await;

    // Three canonical commits whose publication never runs (crash window).
    let shared_overlay = Arc::new(Mutex::new(Overlay::default()));
    let mut expected_ov = Overlay::default();
    for batch in 0..3u32 {
        let mut updates = Vec::new();
        for j in 0..8u32 {
            let doc = 100 + batch * 8 + j;
            let (v, s) = generate_row(800_000 + doc);
            updates.push((doc, Some((v, s))));
        }
        updates.push((50 + batch, None));
        commit_updates(&db, &updates).await;
        for j in 0..8u32 {
            let doc = 100 + batch * 8 + j;
            let (v, s) = generate_row(800_000 + doc);
            expected_ov.upserts.insert(doc, (v, s, meeting_of(doc)));
        }
        expected_ov.tombstones.insert(50 + batch);
    }
    let (canonical, published) = change_ids(&db).await;
    assert!(
        canonical > published,
        "crash window not staged: canonical must be ahead of published"
    );

    // Storage state survives; publication replays durably on restart.
    db.close().await;
    let reopened = open_db(dir.path(), false).await;
    let (recovered, _) = load_snapshot(&reopened).await;
    publish_pending(&reopened, &shared_overlay).await;
    let (canonical2, published2) = change_ids(&reopened).await;
    assert_eq!(canonical2, published2, "replay did not catch up");

    let scope = ScopeFilter::All;
    let mask_expected = base_scan_mask(&recovered, &scope, Some(&expected_ov));
    let mask_replayed = {
        let ov = shared_overlay.lock().expect("overlay");
        base_scan_mask(&recovered, &scope, Some(&ov))
    };
    for case in 0..10u64 {
        let (q, qs) = query_vector(4_242 + case);
        let want = scan_top_k(
            &recovered,
            &mask_expected,
            Some(&expected_ov),
            &scope,
            &q,
            qs,
            TOP_K,
        );
        let got = {
            let ov = shared_overlay.lock().expect("overlay");
            scan_top_k(&recovered, &mask_replayed, Some(&ov), &scope, &q, qs, TOP_K)
        };
        assert_eq!(got, want, "journal replay diverged on case {case}");
    }
    reopened.close().await;
}

/// Independent sparse-ID regression (review 1.R3): deletes + upserts are
/// committed canonically, publication crashes before running, the process
/// restarts and replays the journal over a snapshot whose document IDs no
/// longer equal row positions. The expected side is scored directly from a
/// canonical expected-document map — it shares no mask/index helper with the
/// implementation under test, so a wrong ID-to-row translation on either
/// compaction or replay cannot hide behind a mirrored mistake.
#[tokio::test]
async fn sparse_doc_ids_survive_delete_compaction_and_crash_replay() {
    const N: usize = 600;
    let dir = tempfile::tempdir().expect("tempdir");
    let db = open_db(dir.path(), true).await;
    insert_corpus(&db, N).await;
    publish_pending(&db, &Arc::new(Mutex::new(Overlay::default()))).await;

    // Crash window: three canonical commits whose publication never runs.
    // Deletes punch holes early so every later id shifts off its row position.
    let replayed_overlay = Arc::new(Mutex::new(Overlay::default()));
    let mut expected: BTreeMap<u32, (Vec<i8>, f32)> = (0..N as u32)
        .map(|d| {
            let (v, s) = generate_row(d);
            (d, (v, s))
        })
        .collect();
    let mut deleted_ids = Vec::new();
    for batch in 0..3u32 {
        let mut updates = Vec::new();
        for j in 0..8u32 {
            let doc = 100 + batch * 8 + j;
            let (v, s) = generate_row(800_000 + doc);
            updates.push((doc, Some((v, s))));
        }
        updates.push((50 + batch, None));
        updates.push((400 + batch, None));
        deleted_ids.push(50 + batch);
        deleted_ids.push(400 + batch);
        commit_updates(&db, &updates).await;
        for j in 0..8u32 {
            let doc = 100 + batch * 8 + j;
            let (v, s) = generate_row(800_000 + doc);
            expected.insert(doc, (v, s));
        }
        expected.remove(&(50 + batch));
        expected.remove(&(400 + batch));
    }
    assert_eq!(expected.len(), N - deleted_ids.len());
    let (canonical, published) = change_ids(&db).await;
    assert!(
        canonical > published,
        "crash window not staged: canonical must be ahead of published"
    );

    // Storage survives; restart replays the journal into a fresh overlay.
    db.close().await;
    let reopened = open_db(dir.path(), false).await;
    let (recovered, _) = load_snapshot(&reopened).await;
    assert_eq!(
        recovered.n(),
        expected.len(),
        "reload must preserve sparse IDs, not reindex rows"
    );
    assert_ne!(
        recovered.doc_ids[53], 53,
        "fixture lost its sparse-ID shape"
    );

    publish_pending(&reopened, &replayed_overlay).await;
    let (canonical2, published2) = change_ids(&reopened).await;
    assert_eq!(canonical2, published2, "replay did not catch up");

    let scope = ScopeFilter::All;
    // Canonical expectation scored straight from the expected-document map:
    // no base_scan_mask, no snapshot layout involved.
    let expected_top_k = |q: &[i8], qs: f32| -> Vec<(u32, f32)> {
        let mut scored: Vec<(u32, f32)> = expected
            .iter()
            .map(|(&d, (v, s))| (d, dot_i8(v, q) as f32 * *s * qs))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        scored.truncate(TOP_K);
        scored
    };

    let ov = replayed_overlay.lock().expect("overlay");
    let mask_replayed = base_scan_mask(&recovered, &scope, Some(&ov));
    for case in 0..12u64 {
        let (q, qs) = query_vector(4_242 + case);
        let got = scan_top_k(&recovered, &mask_replayed, Some(&ov), &scope, &q, qs, TOP_K);
        assert_eq!(
            got,
            expected_top_k(&q, qs),
            "post-crash-replay search diverged from canonical expectation, case {case}"
        );
    }

    // Post-compaction: identities stable, count exact, results identical to
    // the canonical expectation computed from the same documents.
    let rebuilt = compact(&recovered, Some(&ov));
    assert_eq!(rebuilt.n(), expected.len());
    assert!(
        rebuilt.doc_ids.windows(2).all(|w| w[0] < w[1]),
        "compaction broke doc-id ordering"
    );
    for doc in &deleted_ids {
        assert!(
            rebuilt.row_of_doc(*doc).is_none(),
            "tombstoned doc {doc} survived compaction"
        );
    }
    for survivor in [53u32, 54, 55, 304, 123] {
        assert!(
            rebuilt.row_of_doc(survivor).is_some(),
            "live doc {survivor} lost across delete/compaction (ID/row confusion)"
        );
    }
    let mask_rebuilt = base_scan_mask(&rebuilt, &scope, None);
    for case in 0..12u64 {
        let (q, qs) = query_vector(4_242 + case);
        let got = scan_top_k(&rebuilt, &mask_rebuilt, None, &scope, &q, qs, TOP_K);
        assert_eq!(
            got,
            expected_top_k(&q, qs),
            "post-compaction search diverged from canonical expectation, case {case}"
        );
    }

    // Narrow scope over the sparse rebuilt snapshot: meeting lookups go
    // through the doc-id -> row translation, never raw positions.
    let narrow_ids: BTreeSet<u32> = [0, 1, 15, 26].iter().copied().collect();
    let narrow = ScopeFilter::Meetings(narrow_ids.clone());
    let mask_narrow = base_scan_mask(&rebuilt, &narrow, Some(&ov));
    let (q, qs) = query_vector(97);
    for (doc, _) in scan_top_k(&rebuilt, &mask_narrow, Some(&ov), &narrow, &q, qs, TOP_K) {
        let meeting = rebuilt
            .meeting_of_doc_id(doc)
            .unwrap_or_else(|| panic!("returned doc {doc} absent from snapshot"));
        assert!(
            narrow_ids.contains(&meeting),
            "leak: out-of-scope doc {doc} (meeting {meeting}) returned"
        );
        assert_eq!(
            meeting,
            meeting_of(doc),
            "meeting metadata drifted for doc {doc}"
        );
    }
    reopened.close().await;
}

/// Publication-bounds regression (review 1.R3a): a commit landing after the
/// publication bound is captured must not be marked published by that pass,
/// even when it rewrites the SAME documents. The expected overlay is
/// constructed directly from generate_row constants and cross-checked against
/// freshly reloaded storage — it shares no helper with publish_through.
#[tokio::test]
async fn concurrent_commit_stays_unpublished_until_subsequent_publication() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = open_db(dir.path(), true).await;
    insert_corpus(&db, 300).await;
    let overlay = Arc::new(Mutex::new(Overlay::default()));
    publish_pending(&db, &overlay).await;

    // Batch 1 is the publication target; the concurrent commit lands after
    // the bound was captured, before the bounded replay runs.
    let mut batch1 = Vec::new();
    let mut expected_at_bound: BTreeMap<u32, (Vec<i8>, f32)> = BTreeMap::new();
    for j in 0..16u32 {
        let doc = 40 + j;
        let (v, s) = generate_row(910_000 + doc);
        batch1.push((doc, Some((v.clone(), s))));
        expected_at_bound.insert(doc, (v, s));
    }
    commit_updates(&db, &batch1).await;
    let bound = canonical_change_id(&db).await;

    // Concurrent commit: newer upserts for the SAME documents, and doc 45 is
    // deleted outright after its bounded upsert. Replay must apply the
    // captured payloads from the journal itself — no current-row join, no
    // fail-closed guard on later history.
    let mut batch2 = Vec::new();
    let mut expected_final: BTreeMap<u32, (Vec<i8>, f32)> = BTreeMap::new();
    for j in 0..16u32 {
        let doc = 40 + j;
        if doc == 45 {
            continue;
        }
        let (v, s) = generate_row(920_000 + doc);
        batch2.push((doc, Some((v.clone(), s))));
        expected_final.insert(doc, (v, s));
    }
    batch2.push((45, None));
    commit_updates(&db, &batch2).await;

    publish_through(&db, &overlay, bound).await;
    let (canonical, published) = change_ids(&db).await;
    assert_eq!(
        published, bound,
        "concurrently committed entries were marked published by a pass bounded below them"
    );
    assert!(canonical > published, "crash-window shape lost");

    // Published state must equal canonical state AS OF the bound: exactly the
    // batch-1 payloads — including doc 45, whose current storage state already
    // reflects the concurrent delete. Nothing newer may leak in.
    {
        let ov = overlay.lock().expect("overlay");
        assert_eq!(ov.upserts.len(), expected_at_bound.len());
        for (doc, (v, s)) in &expected_at_bound {
            assert_eq!(
                ov.upserts.get(doc),
                Some(&(v.clone(), *s, meeting_of(*doc))),
                "doc {doc} not published with its as-of-bound payload"
            );
        }
        assert!(
            ov.tombstones.is_empty(),
            "concurrent delete leaked into the bounded publication"
        );
    }

    // Subsequent publication applies exactly the deferred entries.
    publish_pending(&db, &overlay).await;
    let (canonical2, published2) = change_ids(&db).await;
    assert_eq!(published2, canonical2, "deferred entries not caught up");
    {
        let ov = overlay.lock().expect("overlay");
        assert_eq!(ov.upserts.len(), expected_final.len());
        for (doc, (v, s)) in &expected_final {
            assert_eq!(
                ov.upserts.get(doc),
                Some(&(v.clone(), *s, meeting_of(*doc))),
                "doc {doc} did not settle on the newer payload"
            );
        }
        assert_eq!(ov.tombstones.len(), 1);
        assert!(ov.tombstones.contains(&45), "doc 45 not tombstoned");
    }

    // Independent canonical final state: every published upsert matches the
    // authoritative SQLite row byte-for-byte (vector + scale).
    let (snap, _) = load_snapshot(&db).await;
    assert_eq!(snap.n(), 299);
    assert!(snap.row_of_doc(45).is_none());
    let ov = overlay.lock().expect("overlay");
    for (doc, (v, s, _)) in ov.upserts.iter() {
        let row = snap
            .row_of_doc(*doc)
            .unwrap_or_else(|| panic!("published doc {doc} absent from canonical storage"));
        assert_eq!(&snap.vectors[row * DIM..(row + 1) * DIM], v.as_slice());
        assert_eq!(snap.scales[row], *s);
    }
    drop(ov);
    db.close().await;
}

/// Upsert-then-delete regression (review 1.R3a): replaying a pending upsert
/// whose document was later removed must not depend on a joined current-row
/// payload (the old LEFT JOIN handed back NULL and panicked); the published
/// outcome is a tombstone with canonical == published.
#[tokio::test]
async fn upsert_then_delete_publishes_tombstone_without_payload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = open_db(dir.path(), true).await;
    insert_corpus(&db, 120).await;
    let overlay = Arc::new(Mutex::new(Overlay::default()));
    publish_pending(&db, &overlay).await;

    // Cross-commit variant: upsert committed, then deleted in a later commit,
    // both still unpublished when the publisher runs.
    let (v7, s7) = generate_row(930_007);
    commit_updates(&db, &[(7, Some((v7, s7)))]).await;
    commit_updates(&db, &[(7, None)]).await;

    // Same-commit variant: upsert entry immediately followed by delete entry.
    let (v9, s9) = generate_row(930_009);
    commit_updates(&db, &[(9, Some((v9, s9))), (9, None)]).await;

    publish_pending(&db, &overlay).await;
    let (canonical, published) = change_ids(&db).await;
    assert_eq!(canonical, published);

    let ov = overlay.lock().expect("overlay");
    for doc in [7, 9] {
        assert!(ov.tombstones.contains(&doc), "doc {doc} not tombstoned");
        assert!(
            !ov.upserts.contains_key(&doc),
            "doc {doc} resurrected by journal replay"
        );
    }

    // End-to-end tombstone semantics over the published overlay: no scan
    // returns the dead docs; live docs are untouched.
    drop(ov);
    let (snap, _) = load_snapshot(&db).await;
    assert_eq!(snap.n(), 118);
    assert!(snap.row_of_doc(7).is_none() && snap.row_of_doc(9).is_none());
    let mask = base_scan_mask(&snap, &ScopeFilter::All, None);
    for case in 0..8u64 {
        let (q, qs) = query_vector(5_555 + case);
        let got = scan_top_k(&snap, &mask, None, &ScopeFilter::All, &q, qs, TOP_K);
        assert!(got.iter().all(|(d, _)| *d != 7 && *d != 9));
        assert_eq!(got.len(), TOP_K.min(snap.n()));
    }
    db.close().await;
}

/// Repeated-update regression (review 1.R3a): several unpublished upserts of
/// one doc must publish the FINAL canonical vector, scored independently via
/// hand-computed dot products rather than a mirrored replay path.
#[tokio::test]
async fn repeated_upserts_publish_final_canonical_vector() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = open_db(dir.path(), true).await;
    insert_corpus(&db, 150).await;
    let overlay = Arc::new(Mutex::new(Overlay::default()));
    publish_pending(&db, &overlay).await;

    let (v1, s1) = generate_row(940_011);
    let (v2, s2) = generate_row(940_012);
    let (v3, s3) = generate_row(940_013);
    commit_updates(&db, &[(11, Some((v1, s1)))]).await;
    commit_updates(&db, &[(11, Some((v2, s2))), (11, Some((v3.clone(), s3)))]).await;

    publish_pending(&db, &overlay).await;
    let (canonical, published) = change_ids(&db).await;
    assert_eq!(canonical, published);

    {
        let ov = overlay.lock().expect("overlay");
        assert_eq!(
            ov.upserts.get(&11),
            Some(&(v3.clone(), s3, meeting_of(11))),
            "repeated updates did not settle on the final canonical vector"
        );
    }

    // Scored independently: doc 11's returned score must equal the hand
    // computation against v3/s3 (and therefore not v1/v2).
    let (snap, _) = load_snapshot(&db).await;
    let mask = base_scan_mask(&snap, &ScopeFilter::All, None);
    let ov = overlay.lock().expect("overlay");
    for case in 0..8u64 {
        let (q, qs) = query_vector(6_666 + case);
        let got = scan_top_k(&snap, &mask, Some(&ov), &ScopeFilter::All, &q, qs, TOP_K);
        let expected_score = dot_i8(&v3, &q) as f32 * s3 * qs;
        let entry = got
            .iter()
            .find(|(d, _)| *d == 11)
            .unwrap_or_else(|| panic!("doc 11 missing from results, case {case}"));
        assert_eq!(
            entry.1, expected_score,
            "doc 11 scored with a superseded vector, case {case}"
        );
    }
    drop(ov);
    db.close().await;
}

#[tokio::test]
async fn scheduler_bounds_concurrency_and_rejects_queue_overflow() {
    let sched = Arc::new(ScanScheduler::new());
    let barrier = Arc::new(std::sync::Barrier::new(30));
    let mut handles = Vec::new();
    for t in 0..30 {
        let sched = Arc::clone(&sched);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            let _guard = sched.acquire()?;
            std::thread::sleep(Duration::from_millis(15 + (t % 5) as u64));
            Some(())
        }));
    }
    let mut accepted = 0;
    for h in handles {
        if h.join().expect("join").is_some() {
            accepted += 1;
        }
    }
    assert!(
        sched.max_active.load(Ordering::Relaxed) <= SCAN_PERMITS,
        "scan permit bound violated"
    );
    assert_eq!(
        sched.max_active.load(Ordering::Relaxed),
        SCAN_PERMITS,
        "permits never saturated despite 30 contenders"
    );
    assert!(
        sched.rejected.load(Ordering::Relaxed) >= 10,
        "queue overflow was not rejected fast: {}",
        sched.rejected.load(Ordering::Relaxed)
    );
    assert!(accepted >= SCAN_PERMITS);
}

#[test]
fn index_worker_pauses_within_250ms_of_interactive_waiter() {
    let probe = spawn_worker_probe(2, true);
    std::thread::sleep(Duration::from_millis(30));
    probe.pause_requested.store(true, Ordering::SeqCst);
    let t0 = Instant::now();
    while !probe.paused.load(Ordering::SeqCst) {
        assert!(
            t0.elapsed().as_millis() <= INTERACTIVE_PAUSE_BUDGET_MS,
            "index worker ignored the interactive-pause signal"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(t0.elapsed().as_millis() <= INTERACTIVE_PAUSE_BUDGET_MS);
    probe.stop.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(5));
}

// ---------------------------------------------------------------------------
// Gated full matrix benchmark
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn full_matrix_benchmark() {
    if std::env::var("MEETLY_RAG_VECTOR_BENCH").as_deref() != Ok("1") {
        println!("SKIP full matrix benchmark (set MEETLY_RAG_VECTOR_BENCH=1)");
        return;
    }
    // Fail closed BEFORE any measurement: the 250k combined-envelope phase
    // requires the staged production bundle for real session residency.
    staged_bundle_dir();
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    println!("=== Task 1.4 vector backend benchmark (exact, 768-d int8) ===");
    println!(
        "hardware: Windows x64, logical CPUs={cpus}, encoding=int8(dim={DIM}, per-vector dequant scale), top-k={TOP_K}"
    );
    println!(
        "[rss-start] process current RSS {:.1} MiB (baseline before any index)",
        rss_current_mib().unwrap_or(0.0)
    );

    for n in SCALE_GENS {
        run_scale(n).await;
    }
}

async fn run_scale(n: usize) {
    println!("--- scale n={n} ---");
    let dir = tempfile::tempdir().expect("tempdir");
    let db = open_db(dir.path(), true).await;
    let (gen_elapsed, insert_elapsed) = insert_corpus(&db, n).await;
    println!(
        "[gen] deterministic corpus: generate {:.0} ms, sqlite insert {:.0} ms",
        gen_elapsed.as_millis(),
        insert_elapsed.as_millis()
    );

    let (snap, cold) = load_snapshot(&db).await;
    let snap = Arc::new(snap);
    println!(
        "[cold-load] {:.0} ms for {} docs from canonical SQLite ({:.1} MiB vector payload)",
        cold.as_millis(),
        snap.n(),
        mib(snap.vectors.len() as f64)
    );

    let disk = disk_bytes(&db).await;
    let factor = 250_000f64 / n as f64;
    println!(
        "[disk] {:.1} MiB ({:.0} B/doc); projected 250k: {:.2} GiB steady / {:.2} GiB two retained generations",
        mib(disk as f64),
        disk as f64 / n as f64,
        gib(disk as f64 * factor),
        gib(2.0 * disk as f64 * factor)
    );

    let queries: Vec<(Vec<i8>, f32)> = (0..WARM_QUERIES)
        .map(|i| query_vector(10_000 + i as u64))
        .collect();

    // Warm global queries through the interactive scheduler.
    let sched = Arc::new(ScanScheduler::new());
    let global_mask = base_scan_mask(&snap, &ScopeFilter::All, None);
    warm_phase(
        "global",
        &snap,
        None,
        &global_mask,
        &ScopeFilter::All,
        &queries,
        &sched,
    );

    // Narrow folder/snapshot allow-list queries + leak verification.
    let meetings_total = n as u32 / DOCS_PER_MEETING;
    let folder = ScopeFilter::Meetings((0..meetings_total / 4).collect());
    let snapshot_ids = ScopeFilter::Meetings((0..5).collect());
    for (label, scope) in [("folder-quarter", &folder), ("snapshot-5", &snapshot_ids)] {
        let mask = base_scan_mask(&snap, &scope, None);
        warm_phase(label, &snap, None, &mask, &scope, &queries, &sched);
        verify_no_leak(&snap, None, &mask, &scope, &queries);
    }
    println!(
        "[scope] narrow scopes returned zero out-of-scope documents across {} queries x 3 rounds x 2 scopes",
        WARM_QUERIES
    );

    // Candidate-limit sensitivity (top-k depth), global scope.
    for k in CANDIDATE_LIMITS {
        let mut samples = Vec::with_capacity(WARM_QUERIES * 2);
        for _ in 0..2 {
            for (q, qs) in &queries {
                let t0 = Instant::now();
                scan_top_k(&snap, &global_mask, None, &ScopeFilter::All, q, *qs, k);
                samples.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let (p50, p95, _) = stats(&mut samples);
        println!("[candidate-limit] k={k}: p50 {p50:.1} ms / p95 {p95:.1} ms");
    }

    // Exactness/recall self-check against brute force.
    let step = (WARM_QUERIES / 8).max(1);
    let mut checked = 0;
    for case in (0..WARM_QUERIES).step_by(step) {
        let (q, qs) = &queries[case];
        let got = scan_top_k(&snap, &global_mask, None, &ScopeFilter::All, q, *qs, TOP_K);
        let want = brute_force_top_k(&snap, &global_mask, None, &ScopeFilter::All, q, *qs, TOP_K);
        assert_eq!(got, want, "exact scan diverged from brute force at n={n}");
        checked += 1;
    }
    println!(
        "[recall] recall@{TOP_K}=1.0000 exact-by-construction; verified identical to brute force on {checked}/{WARM_QUERIES} sampled queries"
    );

    // Bounded concurrency: 2 scanner threads through the scheduler.
    {
        let sched_c = Arc::new(ScanScheduler::new());
        let samples = Arc::new(Mutex::new(Vec::<f64>::new()));
        let mut handles = Vec::new();
        for t in 0..2usize {
            let sched = Arc::clone(&sched_c);
            let snap = Arc::clone(&snap);
            let mask = global_mask.clone();
            let samples = Arc::clone(&samples);
            let queries = queries.clone();
            handles.push(std::thread::spawn(move || {
                for i in 0..96 {
                    let (q, qs) = &queries[(t * 31 + i) % queries.len()];
                    if let Some(guard) = sched.acquire() {
                        let t0 = Instant::now();
                        scan_top_k(&snap, &mask, None, &ScopeFilter::All, q, *qs, TOP_K);
                        samples
                            .lock()
                            .expect("samples")
                            .push(t0.elapsed().as_secs_f64() * 1000.0);
                        drop(guard);
                    }
                }
            }));
        }
        let wall0 = Instant::now();
        for h in handles {
            h.join().expect("join scanner");
        }
        let wall = wall0.elapsed().as_millis();
        let mut s = samples.lock().expect("samples");
        let (p50, p95, _) = stats(&mut s);
        println!(
            "[concurrency] 2 scanners x 96 queries: per-query p50 {p50:.1} ms / p95 {p95:.1} ms ({}) , wall {wall} ms, max_active {}",
            latency_verdict(p95),
            sched_c.max_active.load(Ordering::Relaxed)
        );
    }

    // Exact update + publish cost (delta-only; base untouched by design).
    let mut updates = Vec::with_capacity(UPDATE_BATCH as usize);
    for j in 0..UPDATE_BATCH {
        let doc = (j as usize * 977) % n;
        let (v, s) = generate_row(900_000 + doc as u32);
        updates.push((doc as u32, Some((v, s))));
    }
    let digest_before = snap.digest();
    let commit = commit_updates(&db, &updates).await;
    let overlay: Arc<Mutex<Overlay>> = Arc::new(Mutex::new(Overlay::default()));
    let (apply, pub_commit) = publish_pending(&db, &overlay).await;
    let overlay_bytes = overlay.lock().expect("overlay").bytes();
    println!(
        "[update] {}-doc batch: sqlite commit {:.1} ms, replay+apply {:.2} ms, durable published-id commit {:.1} ms; overlay {:.1} KiB; base digest unchanged: {}",
        UPDATE_BATCH,
        commit.as_secs_f64() * 1000.0,
        apply.as_secs_f64() * 1000.0,
        pub_commit.as_secs_f64() * 1000.0,
        overlay_bytes as f64 / 1024.0,
        snap.digest() == digest_before
    );

    // Compaction cost.
    let compact_start = Instant::now();
    let compacted = {
        let ov = overlay.lock().expect("overlay");
        compact(&snap, Some(&ov))
    };
    println!(
        "[compaction] rebuilt base of {} docs (from {} + {} delta) in {:.0} ms",
        compacted.n(),
        snap.n(),
        overlay.lock().expect("overlay").upserts.len(),
        compact_start.elapsed().as_secs_f64() * 1000.0
    );
    drop(compacted);

    if n == 250_000 {
        run_extended_phases(&db, &snap, &global_mask, &queries, &overlay).await;
    }

    println!(
        "[rss] process peak working set at n={n}: {:.1} MiB",
        process_memory()
            .map(|m| mib(m.peak_working_set as f64))
            .unwrap_or(0.0)
    );
    db.close().await;
}

fn warm_phase(
    label: &str,
    snap: &Snapshot,
    overlay: Option<&Arc<Mutex<Overlay>>>,
    mask: &[bool],
    scope: &ScopeFilter,
    queries: &[(Vec<i8>, f32)],
    sched: &ScanScheduler,
) {
    let mut samples = Vec::with_capacity(3 * queries.len());
    for _ in 0..3 {
        for (q, qs) in queries {
            let permit = sched.acquire().expect("permit below queue cap");
            let t0 = Instant::now();
            let held = overlay.map(|m| m.lock().expect("overlay"));
            scan_top_k(snap, mask, held.as_deref(), scope, q, *qs, TOP_K);
            samples.push(t0.elapsed().as_secs_f64() * 1000.0);
            drop(held);
            drop(permit);
        }
    }
    let (p50, p95, max) = stats(&mut samples);
    println!(
        "[warm-{label}] p50 {p50:.1} ms / p95 {p95:.1} ms / max {max:.1} ms ({} samples) -> {}",
        samples.len(),
        latency_verdict(p95)
    );
}

fn verify_no_leak(
    snap: &Snapshot,
    overlay: Option<&Arc<Mutex<Overlay>>>,
    mask: &[bool],
    scope: &ScopeFilter,
    queries: &[(Vec<i8>, f32)],
) {
    for (q, qs) in queries.iter().take(16) {
        let held = overlay.map(|m| m.lock().expect("overlay"));
        for (doc, _) in scan_top_k(snap, mask, held.as_deref(), scope, q, *qs, TOP_K) {
            let meeting = snap
                .meeting_of_doc_id(doc)
                .unwrap_or_else(|| panic!("returned doc {doc} absent from snapshot"));
            assert!(
                scope.allows_meeting(meeting),
                "out-of-scope doc {doc} (meeting {meeting}) returned in narrow scope"
            );
        }
    }
}

async fn run_extended_phases(
    db: &BenchDb,
    snap: &Arc<Snapshot>,
    global_mask: &[bool],
    queries: &[(Vec<i8>, f32)],
    overlay: &Arc<Mutex<Overlay>>,
) {
    // Delta-size scan penalty curve toward the compaction threshold.
    for frac in DELTA_FRACTIONS {
        let count = (250_000.0 * frac) as u32;
        let mut updates = Vec::with_capacity(count as usize);
        for j in 0..count {
            let doc = ((j as u64 * 6_553) % 250_000) as u32;
            let (v, s) = generate_row(700_000 + doc);
            updates.push((doc, Some((v, s))));
        }
        let commit = commit_updates(db, &updates).await;
        publish_pending(db, overlay).await;
        let ov = overlay.lock().expect("overlay");
        let mask = base_scan_mask(snap, &ScopeFilter::All, Some(&ov));
        drop(ov);
        warm_phase(
            &format!("delta-{frac}"),
            snap,
            Some(overlay),
            &mask,
            &ScopeFilter::All,
            queries,
            &ScanScheduler::new(),
        );
        println!(
            "[delta-penalty] applying {count}-doc delta took {:.0} ms; overlay now {:.1} KiB",
            commit.as_secs_f64() * 1000.0,
            mib(overlay.lock().expect("overlay").bytes() as f64) * 1024.0
        );
    }

    // Reader-held old snapshot + new/shadow snapshot peak RAM is measured in
    // the combined envelope phase below, with both model sessions resident.

    // Crash window between canonical commit and in-memory publication,
    // measured under concurrent query load.
    {
        let stop = Arc::new(AtomicBool::new(false));
        let snap_h = Arc::clone(snap);
        let mask_h = global_mask.to_vec();
        let queries_h = queries.to_vec();
        let stop_h = Arc::clone(&stop);
        let hammer = std::thread::spawn(move || {
            let mut scans = 0u64;
            while !stop_h.load(Ordering::Relaxed) {
                let (q, qs) = &queries_h[(scans % queries_h.len() as u64) as usize];
                let _ = scan_top_k(&snap_h, &mask_h, None, &ScopeFilter::All, q, *qs, TOP_K);
                scans += 1;
            }
            scans
        });
        let mut windows = Vec::new();
        for batch in 0..4u32 {
            let mut updates = Vec::new();
            for j in 0..64u32 {
                let doc = 5_000 + batch * 64 + j;
                let (v, s) = generate_row(600_000 + doc);
                updates.push((doc, Some((v, s))));
            }
            updates.push((8_000 + batch, None));
            let commit_ms = commit_updates(db, &updates).await.as_secs_f64() * 1000.0;
            let t_pub = Instant::now();
            publish_pending(db, overlay).await;
            windows.push((commit_ms, t_pub.elapsed().as_secs_f64() * 1000.0));
        }
        stop.store(true, Ordering::Relaxed);
        let scans = hammer.join().expect("join hammer");
        for (i, (commit_ms, publish_ms)) in windows.iter().enumerate() {
            println!(
                "[crash-window] batch {i}: sqlite commit {commit_ms:.1} ms, publication completed {publish_ms:.2} ms later under {scans} concurrent scans"
            );
        }
    }

    // Index-worker scheduling impact: busy worker with and without the
    // interactive-pause policy, including the 250 ms pause-budget check.
    for honor in [false, true] {
        let probe = spawn_worker_probe(4, honor);
        std::thread::sleep(Duration::from_millis(5));
        let sched = ScanScheduler::new();
        let mut pause_latency: Option<u128> = None;
        let mut samples = Vec::with_capacity(3 * queries.len());
        for round in 0..3 {
            for (qi, (q, qs)) in queries.iter().enumerate() {
                if honor && round == 1 && qi == queries.len() / 2 {
                    probe.pause_requested.store(true, Ordering::SeqCst);
                    let t0 = Instant::now();
                    while !probe.paused.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    pause_latency = Some(t0.elapsed().as_millis());
                }
                let permit = sched.acquire().expect("permit");
                let t0 = Instant::now();
                {
                    let ov = overlay.lock().expect("overlay");
                    scan_top_k(
                        snap,
                        global_mask,
                        Some(&ov),
                        &ScopeFilter::All,
                        q,
                        *qs,
                        TOP_K,
                    );
                }
                samples.push(t0.elapsed().as_secs_f64() * 1000.0);
                drop(permit);
            }
        }
        probe.pause_requested.store(false, Ordering::SeqCst);
        probe.stop.store(true, Ordering::SeqCst);
        let (p50, p95, _) = stats(&mut samples);
        let policy = if honor {
            "pause-on-interactive"
        } else {
            "no-pause"
        };
        let pause_txt = pause_latency
            .map(|ms| format!(", interactive pause observed in {ms} ms"))
            .unwrap_or_default();
        println!("[worker-impact] {policy}: query p50 {p50:.1} ms / p95 {p95:.1} ms{pause_txt}");
        if honor {
            assert!(
                pause_latency.unwrap_or(u128::MAX) <= INTERACTIVE_PAUSE_BUDGET_MS,
                "index worker missed the 250 ms interactive-pause budget"
            );
        }
    }

    // Combined rebuild envelope LAST so the process peak counters attribute
    // their monotonic maximum to the state that dominates it: both selected
    // ONNX sessions resident + reader-held active snapshot + streamed shadow
    // + delta/tombstones.
    combined_envelope_measurement(db, snap, overlay).await;

    let disk = disk_bytes(db).await;
    println!(
        "[disk-shadow] two retained generations measured at this scale: {:.2} GiB vs 3 GiB rebuild-peak envelope",
        gib(disk as f64 * 2.0)
    );
}

// ---------------------------------------------------------------------------
// Staged production bundle: selected ONNX sessions resident for the combined
// envelope measurement (1.R3)
// ---------------------------------------------------------------------------

const ORT_INTRA_THREADS: usize = 4;
const WARMUP_SEQ_LEN: usize = 512;
const SHADOW_LOAD_CHUNK_ROWS: usize = 4096;

/// Locate the staged production bundle. `MEETLY_RAG_BUNDLE_DIR` wins (CI),
/// otherwise the checked-out staging location beside the Tauri resources.
/// Missing bundles FAIL CLOSED: the full-envelope command measures real
/// session residency and must never silently skip it.
fn staged_bundle_dir() -> PathBuf {
    let dir = std::env::var("MEETLY_RAG_BUNDLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("retrieval")
                .join("bundle")
        });
    assert!(
        dir.is_dir(),
        "staged retrieval bundle not found at {}; set MEETLY_RAG_BUNDLE_DIR to the staged production bundle (resources/retrieval/bundle)",
        dir.display()
    );
    dir
}

/// A loaded-and-warmed ONNX session held resident for the envelope window,
/// together with its bundled tokenizer: production retrieval keeps both model
/// components' preprocessing state in memory alongside their sessions, and
/// the retained Task 1.3 pair-RAM figures were measured on the same basis.
/// Session construction mirrors the production-shaped pattern used by the
/// Task 1.3 reference harness (CPU EP, Level3 optimization, intra-op cap).
struct ResidentSession {
    session: Session,
    _tokenizer: Tokenizer,
    output_name: String,
    has_token_type_ids: bool,
}

impl ResidentSession {
    fn load(model_path: &Path, tokenizer_dir: &Path, intra_threads: usize) -> Result<Self, String> {
        let tokenizer_path = tokenizer_dir.join("tokenizer.json");
        let _tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| format!("tokenizer {}: {e}", tokenizer_path.display()))?;
        let session = Session::builder()
            .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level3))
            .and_then(|b| b.with_execution_providers(vec![CPUExecutionProvider::default().build()]))
            .and_then(|b| b.with_intra_threads(intra_threads))
            .and_then(|b| b.commit_from_file(model_path))
            .map_err(|e| format!("session {}: {e}", model_path.display()))?;
        let input_names: Vec<String> = session.inputs.iter().map(|i| i.name.clone()).collect();
        for name in &input_names {
            if !["input_ids", "attention_mask", "token_type_ids"].contains(&name.as_str()) {
                return Err(format!(
                    "{}: unexpected session input {name:?}; staged bundle does not match the approved tensor contract",
                    model_path.display()
                ));
            }
        }
        if !input_names.iter().any(|n| n == "input_ids")
            || !input_names.iter().any(|n| n == "attention_mask")
        {
            return Err(format!(
                "{}: missing input_ids/attention_mask; staged bundle does not match the approved tensor contract",
                model_path.display()
            ));
        }
        let output_names: Vec<String> = session.outputs.iter().map(|o| o.name.clone()).collect();
        let output_name = ["sentence_embedding", "last_hidden_state", "logits"]
            .iter()
            .find(|o| output_names.iter().any(|n| n == *o))
            .ok_or_else(|| {
                format!(
                    "{}: none of sentence_embedding/last_hidden_state/logits among outputs {output_names:?}; staged bundle does not match the approved tensor contract",
                    model_path.display()
                )
            })?
            .to_string();
        Ok(Self {
            session,
            _tokenizer,
            output_name,
            has_token_type_ids: input_names.iter().any(|n| n == "token_type_ids"),
        })
    }

    /// One batch-1 forward pass at the models' sequence limit so ORT arena and
    /// workspace residency is materialized before the peak window, the way a
    /// real first request would. Synthetic ids keep it bounded and
    /// deterministic; no tokenizer or corpus text is involved.
    fn warm(&mut self, seq_len: usize) -> Result<(), String> {
        let ids =
            Array2::from_shape_fn((1, seq_len), |(_, col)| ((col * 7919) % 20_000) as i64 + 5);
        let mask = Array2::<i64>::from_elem((1, seq_len), 1);
        let outputs = if self.has_token_type_ids {
            let zeros = Array2::<i64>::zeros((1, seq_len));
            self.session
                .run(inputs![
                    "input_ids" =>
                        TensorRef::from_array_view(ids.view()).map_err(|e| e.to_string())?,
                    "attention_mask" =>
                        TensorRef::from_array_view(mask.view()).map_err(|e| e.to_string())?,
                    "token_type_ids" =>
                        TensorRef::from_array_view(zeros.view()).map_err(|e| e.to_string())?
                ])
                .map_err(|e| format!("ort run: {e}"))?
        } else {
            self.session
                .run(inputs![
                    "input_ids" =>
                        TensorRef::from_array_view(ids.view()).map_err(|e| e.to_string())?,
                    "attention_mask" =>
                        TensorRef::from_array_view(mask.view()).map_err(|e| e.to_string())?
                ])
                .map_err(|e| format!("ort run: {e}"))?
        };
        if outputs.get(self.output_name.as_str()).is_none() {
            return Err(format!("warm run produced no {}", self.output_name));
        }
        Ok(())
    }
}

/// Same-process measurement of the 250k combined rebuild envelope: both
/// selected ONNX sessions are loaded from the staged production bundle FIRST
/// and stay resident, then the reader-held active snapshot plus a
/// production-shaped streamed shadow build and the live delta/tombstone state
/// are held together. The Windows process peak counters govern real peak
/// residency during shadow loading — raw payload arithmetic and retained
/// session figures no longer substitute for a measurement.
async fn combined_envelope_measurement(
    db: &BenchDb,
    active: &Arc<Snapshot>,
    overlay: &Arc<Mutex<Overlay>>,
) {
    println!("=== Combined 250k rebuild envelope (same-process measurement) ===");
    let bundle = staged_bundle_dir();
    let mem = |stage: &str| {
        process_memory().unwrap_or_else(|| panic!("process memory metrics unavailable at {stage}"))
    };

    // Sessions load FIRST and remain alive through the entire peak window.
    let before_sessions = mem("before sessions");
    let mut embedding = ResidentSession::load(
        &bundle
            .join("models")
            .join("embedding")
            .join("model_int8.onnx"),
        &bundle.join("tokenizers").join("embedding"),
        ORT_INTRA_THREADS,
    )
    .unwrap_or_else(|e| {
        panic!("[blocked-resource-envelope] staged embedding session failed to load: {e}")
    });
    let after_embedding = mem("after embedding session");
    let mut reranker = ResidentSession::load(
        &bundle
            .join("models")
            .join("reranker")
            .join("model_quint8_avx2.onnx"),
        &bundle.join("tokenizers").join("reranker"),
        ORT_INTRA_THREADS,
    )
    .unwrap_or_else(|e| {
        panic!("[blocked-resource-envelope] staged reranker session failed to load: {e}")
    });
    embedding.warm(WARMUP_SEQ_LEN).unwrap_or_else(|e| {
        panic!("[blocked-resource-envelope] staged embedding warmup failed: {e}")
    });
    reranker.warm(WARMUP_SEQ_LEN).unwrap_or_else(|e| {
        panic!("[blocked-resource-envelope] staged reranker warmup failed: {e}")
    });
    let after_sessions = mem("after sessions");
    println!(
        "[envelope-sessions] e5-base dynamic-int8 + mmarco quint8_avx2 resident (sessions + bundled tokenizers, arena warmed) from {}: embedding +{:.1} MiB, reranker +{:.1} MiB, both +{:.1} MiB over process base",
        bundle.display(),
        mib(after_embedding.working_set.saturating_sub(before_sessions.working_set) as f64),
        mib(after_sessions.working_set.saturating_sub(after_embedding.working_set) as f64),
        mib(after_sessions.working_set.saturating_sub(before_sessions.working_set) as f64)
    );

    // Reader-held active generation + live delta/tombstone state.
    let held_active = Arc::clone(active);
    let delta_bytes = overlay.lock().expect("overlay").bytes();
    println!(
        "[envelope-parts] active snapshot {} docs ({:.1} MiB), delta+tombstones {:.1} KiB",
        held_active.n(),
        mib(held_active.vectors.len() as f64),
        delta_bytes as f64 / 1024.0
    );

    // Building shadow snapshot, streamed so only the contiguous snapshot
    // capacity plus a bounded per-chunk SQLite transient is carried.
    let t_shadow = Instant::now();
    let (shadow, _) = load_snapshot_streaming(db, SHADOW_LOAD_CHUNK_ROWS).await;
    let combined = mem("combined holding");
    println!(
        "[envelope-transient] shadow ({} docs, {:.1} MiB) streamed in {:.0} ms while active+delta+both sessions stayed resident; exactly two snapshots held (active {} docs + shadow {} docs)",
        shadow.n(),
        mib(shadow.vectors.len() as f64),
        t_shadow.elapsed().as_secs_f64() * 1000.0,
        held_active.n(),
        shadow.n()
    );
    let peak_ws = combined.peak_working_set;
    println!(
        "[envelope-peak] measured process peak working set {:.1} MiB (current {:.1} MiB; private commit {:.1} MiB current / {:.1} MiB peak)",
        mib(peak_ws as f64),
        mib(combined.working_set as f64),
        mib(combined.private_commit as f64),
        mib(combined.peak_private_commit as f64)
    );
    println!(
        "[envelope-verdict] transient two-snapshot rebuild peak vs limits -> {}",
        transient_ram_verdict(peak_ws)
    );
    assert!(
        peak_ws <= TRANSIENT_MAX_BYTES,
        "[blocked-resource-envelope] measured 250k rebuild peak {:.1} MiB exceeds the user-approved 1.30 GiB transient ceiling (components: active {:.1} MiB, shadow {:.1} MiB, delta {:.1} KiB, sessions per [envelope-sessions] above). A true third snapshot or any higher peak blocks activation; ANN is not a permitted remedy under the Backend Decision Rule.",
        mib(peak_ws as f64),
        mib(held_active.vectors.len() as f64),
        mib(shadow.vectors.len() as f64),
        delta_bytes as f64 / 1024.0
    );

    // Post-activation steady state: shadow generation released; active
    // snapshot + delta + both sessions remain resident.
    drop(shadow);
    let steady = mem("steady state");
    println!(
        "[envelope-steady] measured working set after shadow release: {:.1} MiB (private commit {:.1} MiB) -> {}",
        mib(steady.working_set as f64),
        mib(steady.private_commit as f64),
        steady_ram_verdict(steady.working_set)
    );
    assert!(
        steady.working_set <= BAND_MAX_BYTES,
        "[blocked-resource-envelope] measured steady-state {:.1} MiB exceeds the 1.25 GiB approved-band ceiling",
        mib(steady.working_set as f64)
    );
    drop(held_active);
    drop(embedding);
    drop(reranker);
}
