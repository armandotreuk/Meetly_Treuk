//! Focused, reproducible before/after measurement of the writer-lock span of
//! [`RetrievalRepository::replace_meeting_documents`] at Sprint-2 benchmark
//! corpus scale (250k canonical documents), added with the 2.R3 amendment.
//!
//! The test-only reference replacement independently replays the prior
//! replacement transaction, including its full `COUNT(*)` document-count
//! update. The production replacement is measured on an equivalent fixture.
//! Both timings are wall-clock around the repository call on a file-backed WAL
//! database. They are upper bounds on the corresponding writer-lock span, not
//! exact lock durations: connection/scheduler work and the commit-to-return
//! gap are included.
//!
//! Fixture realism: the production migrations build the real derived schema;
//! vectors follow the approved int8 contract shape; the corpus is 250,000
//! canonical rows across 251 meetings (matching the scale of the Sprint 1/2
//! production-backend benchmark), and the published worst-case meeting holds
//! 1024 documents - four times the approved 256-document batch ceiling, i.e.
//! a plausible full-day transcribed meeting.
//!
//! The concurrent-writer pass starts a real primary-data transaction through a
//! separate SQLite connection while the production replacement is running.
//! Its `BEGIN IMMEDIATE` attempt-to-acquire duration and completion
//! duration are also upper bounds, not exact lock/wait durations.
//!
//! Known limitations versus the production benchmark corpus (`tests/
//! vector_backend_benchmark.rs`): synthetic text bodies, one competing
//! primary writer, and no concurrent scanner or interactive load. The lock
//! span here is dominated by SQLite row work, which those factors barely
//! change; they matter for search latency, not for this transaction's duration
//! shape.
//!
//! Run explicitly (release profile, target outside OneDrive):
//!
//! ```powershell
//! $env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
//! cargo test --release --manifest-path frontend/src-tauri/Cargo.toml `
//!     --test document_count_lock_hold -- --ignored --nocapture
//! ```

use app_lib::database::repositories::retrieval::{
    ModelSpec, ReplacementJob, ReplacementOutcome, RetrievalRepository, StagedDocument,
    VectorEncoding,
};
use chrono::Utc;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Connection, Error as SqlxError, SqliteConnection,
};
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant};

/// Approved int8 storage contract mirror (dimensions/scale identical to what
/// the worker registers); keeps validation real without loading models.
const DIMENSIONS: u32 = 2;
const DEQUANTIZATION_SCALE: f64 = 1.0 / 127.0;
/// Approved unit vector [96, 84]/127: decoded norm ~1.0044, inside the 5% int8 tolerance.
const UNIT_VECTOR: [u8; 2] = [96u8, 84u8];
/// Production-backend benchmark corpus scale.
const CORPUS_ROWS: usize = 250_000;
/// Realistic worst-case meeting document count (full-day transcript windows).
const WORST_MEETING_DOCS: usize = 1024;
/// Canonical rows the worst-case meeting already holds before being replaced.
const WORST_MEETING_PRIOR: usize = 750;
/// Remaining rows spread over filler meetings: 250 x 997 + 750 + ... = 250k.
const FILLER_MEETINGS: usize = 250;
const GENERATION_ID: &str = "gen-bench";
const MEETING_ID: &str = "worst";
const PAUSE_QUANTUM: Duration = Duration::from_millis(250);
const WARMUP_ITERATIONS: usize = 2;
const MEASURED_ITERATIONS: usize = 7;

fn sqlite_options(db_path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::from_str(db_path.to_str().unwrap())
        .unwrap()
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(10))
        .foreign_keys(true)
}

async fn bench_pool(db_path: &Path) -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(sqlite_options(db_path))
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

/// Test-only replay of the pre-2.R3 replacement transaction. It deliberately
/// retains the old full-corpus `COUNT(*)` update and does not participate in
/// production code.
async fn reference_replace_with_full_count(
    pool: &sqlx::SqlitePool,
    job: ReplacementJob<'_>,
) -> Result<ReplacementOutcome, SqlxError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current: Option<(i64,)> =
        sqlx::query_as("SELECT source_revision FROM search_source_state WHERE meeting_id = ?")
            .bind(job.meeting_id)
            .fetch_optional(&mut *tx)
            .await?;
    let current_revision = current.map(|(revision,)| revision);
    if current_revision != Some(job.expected_source_revision) {
        sqlx::query("DELETE FROM retrieval_document_staging WHERE job_id = ?")
            .bind(job.job_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(ReplacementOutcome::RevisionConflict { current_revision });
    }

    let model: Option<(i64, String, Option<f64>, Option<i64>)> = sqlx::query_as(
        "SELECT m.dimensions, m.vector_encoding, m.dequantization_scale, m.dequantization_zero_point
         FROM retrieval_generations g JOIN retrieval_models m ON m.model_id = g.model_id
         WHERE g.generation_id = ?",
    )
    .bind(job.generation_id)
    .fetch_optional(&mut *tx)
    .await?;
    let model = model.ok_or_else(|| {
        SqlxError::Protocol(format!("unknown generation '{}'", job.generation_id))
    })?;

    let staged: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT document_id, payload FROM retrieval_document_staging WHERE job_id = ? ORDER BY id",
    )
    .bind(job.job_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut documents = Vec::with_capacity(staged.len());
    for (document_id, payload) in staged {
        let document: StagedDocument = serde_json::from_slice(&payload).map_err(|_| {
            SqlxError::Protocol(format!(
                "staged payload for '{}' is unreadable",
                document_id
            ))
        })?;
        if document.document_id != document_id {
            return Err(SqlxError::Protocol(
                "staged payload identity does not match its key".into(),
            ));
        }
        validate_reference_document(&model, &document)?;
        documents.push(document);
    }

    sqlx::query("DELETE FROM retrieval_documents WHERE generation_id = ? AND meeting_id = ?")
        .bind(job.generation_id)
        .bind(job.meeting_id)
        .execute(&mut *tx)
        .await?;
    let now = Utc::now().to_rfc3339();
    for document in &documents {
        sqlx::query(
            "INSERT INTO retrieval_documents (generation_id, document_id, meeting_id, source_kind, source_start_id, source_end_id, source_template_id, heading, ordinal, content, content_hash, dimensions, vector_encoding, vector, source_revision, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(job.generation_id)
        .bind(&document.document_id)
        .bind(job.meeting_id)
        .bind(&document.source_kind)
        .bind(&document.source_start_id)
        .bind(&document.source_end_id)
        .bind(&document.source_template_id)
        .bind(&document.heading)
        .bind(document.ordinal)
        .bind(&document.content)
        .bind(&document.content_hash)
        .bind(document.dimensions)
        .bind(document.vector_encoding.as_str())
        .bind(&document.vector)
        .bind(job.expected_source_revision)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM retrieval_document_staging WHERE job_id = ?")
        .bind(job.job_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO retrieval_meeting_state (generation_id, meeting_id, indexed_source_revision, state, attempt_count, next_attempt_at, last_error, updated_at)
         VALUES (?, ?, ?, 'ready', 0, NULL, NULL, ?)
         ON CONFLICT(generation_id, meeting_id) DO UPDATE SET
             indexed_source_revision = excluded.indexed_source_revision,
             state = 'ready',
             attempt_count = 0,
             next_attempt_at = NULL,
             last_error = NULL,
             updated_at = excluded.updated_at",
    )
    .bind(job.generation_id)
    .bind(job.meeting_id)
    .bind(job.expected_source_revision)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let change_id = sqlx::query(
        "INSERT INTO retrieval_index_changes (generation_id, meeting_id, operation, source_revision, created_at)
         VALUES (?, ?, 'upsert', ?, ?)",
    )
    .bind(job.generation_id)
    .bind(job.meeting_id)
    .bind(job.expected_source_revision)
    .bind(&now)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    sqlx::query(
        "UPDATE retrieval_index_state
         SET canonical_change_id = (
                 SELECT MAX(c.change_id) FROM retrieval_index_changes c
                 WHERE c.generation_id = retrieval_index_state.generation_id
             ),
             updated_at = ?
         WHERE generation_id = ?",
    )
    .bind(&now)
    .bind(job.generation_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE retrieval_generations
         SET document_count = (SELECT COUNT(*) FROM retrieval_documents WHERE generation_id = retrieval_generations.generation_id)
         WHERE generation_id = ?",
    )
    .bind(job.generation_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ReplacementOutcome::Published { change_id })
}

fn validate_reference_document(
    model: &(i64, String, Option<f64>, Option<i64>),
    document: &StagedDocument,
) -> Result<(), SqlxError> {
    if document.vector_encoding != VectorEncoding::Int8
        || model.1 != VectorEncoding::Int8.as_str()
        || document.dimensions != model.0
        || document.vector.len() != model.0 as usize
    {
        return Err(SqlxError::Protocol(format!(
            "document '{}' does not match the benchmark model",
            document.document_id
        )));
    }
    let scale = model
        .2
        .ok_or_else(|| SqlxError::Protocol("int8 benchmark model is missing its scale".into()))?;
    let zero_point = model.3.unwrap_or(0);
    let norm = document
        .vector
        .iter()
        .map(|byte| {
            let value = scale * (i8::from_ne_bytes([*byte]) as f64 - zero_point as f64);
            value * value
        })
        .sum::<f64>()
        .sqrt();
    if !(norm.is_finite() && (norm - 1.0).abs() <= 0.05) {
        return Err(SqlxError::Protocol(format!(
            "document '{}' has a non-unit benchmark vector",
            document.document_id
        )));
    }
    Ok(())
}

/// Bulk-inserts `count` canonical rows for one meeting using chunked
/// multi-row INSERTs so fixture setup stays out of the measured window.
async fn insert_canonical_rows(
    pool: &sqlx::SqlitePool,
    generation_id: &str,
    meeting_id: &str,
    prefix: &str,
    count: usize,
) {
    const CHUNK: usize = 500;
    let mut done = 0usize;
    while done < count {
        let take = (count - done).min(CHUNK);
        let mut sql =
            String::from("INSERT INTO retrieval_documents (generation_id, document_id, meeting_id, source_kind, ordinal, content, content_hash, dimensions, vector_encoding, vector, source_revision, updated_at) VALUES ");
        for index in 0..take {
            if index > 0 {
                sql.push(',');
            }
            sql.push_str("(?, ?, ?, 'transcript', ?, 'fixture content', X'01', ");
            sql.push_str(&DIMENSIONS.to_string());
            sql.push_str(", 'int8', ?, 7, '2026-01-01T00:00:00Z')");
        }
        let mut query = sqlx::query(&sql);
        for index in 0..take {
            query = query
                .bind(generation_id)
                .bind(format!("{prefix}-{}", done + index))
                .bind(meeting_id)
                .bind((done + index) as i64)
                .bind(UNIT_VECTOR.as_slice());
        }
        query.execute(pool).await.unwrap();
        done += take;
    }
}

/// Loads a fresh staging job of `count` documents for the measured meeting.
async fn load_staging_job(pool: &sqlx::SqlitePool, generation_id: &str, count: usize) -> String {
    let job_id = format!("bench-job-{generation_id}");
    sqlx::query("DELETE FROM retrieval_document_staging WHERE job_id = ?")
        .bind(&job_id)
        .execute(pool)
        .await
        .unwrap();
    const CHUNK: usize = 256;
    let mut done = 0usize;
    while done < count {
        let take = (count - done).min(CHUNK);
        let mut sql = String::from(
            "INSERT INTO retrieval_document_staging (job_id, generation_id, meeting_id, source_revision, document_id, payload) VALUES ",
        );
        for index in 0..take {
            if index > 0 {
                sql.push(',');
            }
            sql.push_str("(?, ?, 'worst', 1, ?, ?)");
        }
        let mut query = sqlx::query(&sql);
        for index in 0..take {
            let payload = serde_json::json!({
                "document_id": format!("stage-{}", done + index),
                "source_kind": "transcript",
                "source_start_id": null,
                "source_end_id": null,
                "source_template_id": null,
                "heading": null,
                "ordinal": (done + index) as i64,
                "content": "fixture window",
                "content_hash": [1u8, 2, 3],
                "dimensions": DIMENSIONS as i64,
                "vector_encoding": "Int8",
                "vector": UNIT_VECTOR.to_vec(),
            });
            query = query
                .bind(&job_id)
                .bind(generation_id)
                .bind(format!("stage-{}", done + index))
                .bind(serde_json::to_vec(&payload).unwrap());
        }
        query.execute(pool).await.unwrap();
        done += take;
    }
    job_id
}

async fn prepare_fixture(db_path: &Path) -> sqlx::SqlitePool {
    let pool = bench_pool(db_path).await;
    let spec = ModelSpec {
        model_id: "int8-model".to_string(),
        dimensions: DIMENSIONS,
        vector_encoding: VectorEncoding::Int8,
        chunker_version: 1,
        dequantization_scale: Some(DEQUANTIZATION_SCALE),
        dequantization_zero_point: Some(0),
    };
    RetrievalRepository::register_model(&pool, &spec)
        .await
        .unwrap();
    RetrievalRepository::register_generation(&pool, GENERATION_ID, "int8-model")
        .await
        .unwrap();

    const FILLER_PER: usize = (CORPUS_ROWS - WORST_MEETING_PRIOR) / FILLER_MEETINGS;
    for filler in 0..FILLER_MEETINGS {
        let meeting_id = format!("filler-{filler}");
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'Filler', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .bind(&meeting_id)
            .execute(&pool)
            .await
            .unwrap();
        insert_canonical_rows(&pool, GENERATION_ID, &meeting_id, &meeting_id, FILLER_PER).await;
    }
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('worst', 'Worst Case', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
        .execute(&pool)
        .await
        .unwrap();
    insert_canonical_rows(
        &pool,
        GENERATION_ID,
        MEETING_ID,
        "worst-prior",
        WORST_MEETING_PRIOR,
    )
    .await;
    let seeded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM retrieval_documents")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(seeded, CORPUS_ROWS as i64);
    sqlx::query("UPDATE retrieval_generations SET document_count = ?")
        .bind(seeded)
        .execute(&pool)
        .await
        .unwrap();
    pool
}

#[derive(Clone, Copy)]
enum ReplacementPath {
    FullCountReference,
    ExactDeltaProduction,
}

async fn replace_for_benchmark(
    pool: &sqlx::SqlitePool,
    path: ReplacementPath,
    job_id: &str,
) -> Result<ReplacementOutcome, SqlxError> {
    let job = ReplacementJob {
        generation_id: GENERATION_ID,
        meeting_id: MEETING_ID,
        expected_source_revision: 1,
        job_id,
    };
    match path {
        ReplacementPath::FullCountReference => reference_replace_with_full_count(pool, job).await,
        ReplacementPath::ExactDeltaProduction => {
            RetrievalRepository::replace_meeting_documents(pool, job).await
        }
    }
}

async fn measure_path(pool: &sqlx::SqlitePool, path: ReplacementPath) -> Vec<Duration> {
    let label = match path {
        ReplacementPath::FullCountReference => "baseline-full-count",
        ReplacementPath::ExactDeltaProduction => "current-exact-delta",
    };
    let mut samples = Vec::with_capacity(MEASURED_ITERATIONS);
    for iteration in 0..(WARMUP_ITERATIONS + MEASURED_ITERATIONS) {
        let job_id = load_staging_job(pool, GENERATION_ID, WORST_MEETING_DOCS).await;
        let started = Instant::now();
        let outcome = replace_for_benchmark(pool, path, &job_id).await.unwrap();
        let elapsed = started.elapsed();
        assert!(matches!(outcome, ReplacementOutcome::Published { .. }));
        if iteration >= WARMUP_ITERATIONS {
            samples.push(elapsed);
        } else {
            println!("{label} warmup {iteration}: {elapsed:?}");
        }
    }
    samples.sort_unstable();
    println!(
        "{label} lock-span upper bound over {MEASURED_ITERATIONS} measured replacements (1024-doc meeting): min {:?} / median {:?} / max {:?}",
        samples[0],
        samples[samples.len() / 2],
        samples[samples.len() - 1]
    );
    samples
}

async fn assert_document_count(pool: &sqlx::SqlitePool, expected: i64) {
    let stored: i64 = sqlx::query_scalar(
        "SELECT document_count FROM retrieval_generations WHERE generation_id = ?",
    )
    .bind(GENERATION_ID)
    .fetch_one(pool)
    .await
    .unwrap();
    let live: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM retrieval_documents WHERE generation_id = ?")
            .bind(GENERATION_ID)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(stored, live, "document counter must equal canonical rows");
    assert_eq!(
        stored, expected,
        "replacement deltas must preserve the total"
    );
}

struct WriterContention {
    replacement_elapsed: Duration,
    writer_wait_upper_bound: Duration,
    writer_completion_upper_bound: Duration,
    start_delay_ms: u64,
}

async fn measure_current_with_writer(pool: &sqlx::SqlitePool, db_path: &Path) -> WriterContention {
    for (attempt, start_delay_ms) in [1_u64, 2, 5, 10, 20].into_iter().enumerate() {
        let job_id = load_staging_job(pool, GENERATION_ID, WORST_MEETING_DOCS).await;
        let writer_options = sqlite_options(db_path);
        let mut writer = SqliteConnection::connect_with(&writer_options)
            .await
            .unwrap();
        let replacement_pool = pool.clone();
        let replacement = tokio::spawn(async move {
            let started = Instant::now();
            let outcome = replace_for_benchmark(
                &replacement_pool,
                ReplacementPath::ExactDeltaProduction,
                &job_id,
            )
            .await;
            let finished = Instant::now();
            (started, finished, outcome)
        });
        tokio::time::sleep(Duration::from_millis(start_delay_ms)).await;
        let writer_task = tokio::spawn(async move {
            let attempted = Instant::now();
            let mut tx = writer.begin_with("BEGIN IMMEDIATE").await?;
            let acquired = Instant::now();
            let result = sqlx::query(
                "UPDATE meetings SET title = 'Benchmark writer', updated_at = ? WHERE id = 'filler-0'",
            )
            .bind(format!("2026-01-01T00:01:{attempt:02}Z"))
            .execute(&mut *tx)
            .await?;
            assert_eq!(result.rows_affected(), 1);
            tx.commit().await?;
            let completed = Instant::now();
            Ok::<_, SqlxError>((attempted, acquired, completed))
        });

        let (replacement_started, replacement_finished, replacement_outcome) =
            tokio::time::timeout(Duration::from_secs(10), replacement)
                .await
                .expect("production replacement must complete")
                .unwrap();
        assert!(matches!(
            replacement_outcome.unwrap(),
            ReplacementOutcome::Published { .. }
        ));
        let (writer_attempted, writer_acquired, writer_completed) =
            tokio::time::timeout(Duration::from_secs(10), writer_task)
                .await
                .expect("competing primary writer must complete")
                .unwrap()
                .unwrap();
        let writer_wait = writer_acquired.duration_since(writer_attempted);
        let writer_completion = writer_completed.duration_since(writer_attempted);
        let replacement_elapsed = replacement_finished.duration_since(replacement_started);

        // The writer must have attempted while the replacement call was live
        // and observed a non-trivial BEGIN wait. This is contention evidence,
        // not an exact lock measurement: both wait values include scheduling,
        // connection, and commit/return overhead. ponytail: the fixed 5 ms
        // slack classifies contention only; exact lock timestamps need a
        // SQLite-level hook, which is outside this test-only benchmark.
        let expected_wait_floor =
            replacement_elapsed.saturating_sub(Duration::from_millis(start_delay_ms + 5));
        if writer_attempted < replacement_finished
            && writer_wait >= Duration::from_millis(1)
            && writer_wait >= expected_wait_floor
        {
            return WriterContention {
                replacement_elapsed,
                writer_wait_upper_bound: writer_wait,
                writer_completion_upper_bound: writer_completion,
                start_delay_ms,
            };
        }
    }
    panic!("could not obtain a competing writer interval over the production replacement");
}

#[tokio::test]
#[ignore = "write-lock hold measurement at 250k corpus scale; run explicitly in release"]
async fn measure_replace_write_lock_hold_at_corpus_scale() {
    let run_id = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let baseline_path =
        std::env::temp_dir().join(format!("meetly-dc-lock-hold-baseline-{run_id}.sqlite"));
    let current_path =
        std::env::temp_dir().join(format!("meetly-dc-lock-hold-current-{run_id}.sqlite"));
    let baseline_pool = prepare_fixture(&baseline_path).await;
    let current_pool = prepare_fixture(&current_path).await;

    println!(
        "environment: os={} arch={} profile={} threads={} database=file-backed WAL, migrations applied",
        std::env::consts::OS,
        std::env::consts::ARCH,
        if cfg!(debug_assertions) { "debug" } else { "release" },
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
    );
    println!(
        "fixtures: {} canonical rows each across {} meetings; worst-case meeting publishes {} documents against {} pre-existing ones",
        CORPUS_ROWS, FILLER_MEETINGS + 1, WORST_MEETING_DOCS, WORST_MEETING_PRIOR
    );

    measure_path(&baseline_pool, ReplacementPath::FullCountReference).await;
    let current_samples = measure_path(&current_pool, ReplacementPath::ExactDeltaProduction).await;
    let current_max = *current_samples.last().unwrap();
    assert!(
        current_max <= PAUSE_QUANTUM,
        "current exact-delta replacement exceeded the 250 ms pause quantum: {current_max:?}"
    );
    println!("current-path gate: max {current_max:?} <= {PAUSE_QUANTUM:?} pause quantum -> PASS");

    let contention = measure_current_with_writer(&current_pool, &current_path).await;
    assert!(
        contention.replacement_elapsed <= PAUSE_QUANTUM,
        "contended current replacement exceeded the 250 ms pause quantum: {:?}",
        contention.replacement_elapsed
    );
    println!(
        "concurrent-primary-writer: current replacement {:?}; BEGIN IMMEDIATE wait upper bound (not exact lock duration) {:?}; writer completion upper bound {:?}; blocked=true; start delay {} ms",
        contention.replacement_elapsed,
        contention.writer_wait_upper_bound,
        contention.writer_completion_upper_bound,
        contention.start_delay_ms
    );

    let expected = (CORPUS_ROWS + WORST_MEETING_DOCS - WORST_MEETING_PRIOR) as i64;
    assert_document_count(&baseline_pool, expected).await;
    assert_document_count(&current_pool, expected).await;
    baseline_pool.close().await;
    current_pool.close().await;
    for db_path in [&baseline_path, &current_path] {
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(db_path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("sqlite-shm"));
    }
}
