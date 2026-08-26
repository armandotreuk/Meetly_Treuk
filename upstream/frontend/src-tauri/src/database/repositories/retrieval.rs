use std::collections::HashSet;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Error as SqlxError, Sqlite, SqlitePool, Transaction};
use tokio_util::sync::CancellationToken;

pub const GENERATION_STATES: [&str; 4] = ["building", "ready", "failed", "retired"];

/// Index backend recorded at generation registration. Sprint 1 selected exact
/// search; Task 2.5 owns any future backend vocabulary.
pub const EXACT_INDEX_BACKEND: &str = "exact";

/// Encodings admitted at the repository boundary. The approved production
/// bundle stores int8; f32 is the reference encoding. There is intentionally
/// no fixed byte-width rule in SQL — byte length is validated here against
/// the declared encoding and dimension count instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorEncoding {
    F32,
    Fp16,
    Int8,
}

impl VectorEncoding {
    pub fn as_str(self) -> &'static str {
        match self {
            VectorEncoding::F32 => "f32",
            VectorEncoding::Fp16 => "fp16",
            VectorEncoding::Int8 => "int8",
        }
    }

    fn parse(value: &str) -> Result<Self, SqlxError> {
        match value {
            "f32" => Ok(VectorEncoding::F32),
            "fp16" => Ok(VectorEncoding::Fp16),
            "int8" => Ok(VectorEncoding::Int8),
            other => Err(SqlxError::Protocol(format!(
                "unsupported vector encoding '{}'",
                other
            ))),
        }
    }

    fn bytes_per_value(self) -> usize {
        match self {
            VectorEncoding::F32 => 4,
            VectorEncoding::Fp16 => 2,
            VectorEncoding::Int8 => 1,
        }
    }

    pub(crate) fn norm_tolerance(self) -> f64 {
        match self {
            // Quantized vectors keep their approximate unit norm only within
            // quantization error.
            VectorEncoding::Int8 => 0.05,
            _ => 1e-3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub model_id: String,
    pub dimensions: u32,
    pub vector_encoding: VectorEncoding,
    pub chunker_version: u32,
    /// Required for quantized encodings, rejected on float encodings, so a
    /// vector can never be interpreted under the wrong scale.
    pub dequantization_scale: Option<f64>,
    pub dequantization_zero_point: Option<i64>,
}

#[derive(Debug, Clone)]
struct ModelDescriptor {
    dimensions: i64,
    vector_encoding: VectorEncoding,
    dequantization_scale: Option<f64>,
    dequantization_zero_point: Option<i64>,
}

/// One derived semantic document as handed to staging. The vector bytes are
/// encoded exactly as the owning model's `vector_encoding` prescribes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedDocument {
    pub document_id: String,
    pub source_kind: String,
    pub source_start_id: Option<String>,
    pub source_end_id: Option<String>,
    pub source_template_id: Option<String>,
    /// Section-heading provenance carried through staging into the canonical
    /// row; `None` for transcript windows and for rows published before the
    /// column existed (never reconstructed heuristically).
    #[serde(default)]
    pub heading: Option<String>,
    pub ordinal: i64,
    pub content: String,
    pub content_hash: Vec<u8>,
    pub dimensions: i64,
    pub vector_encoding: VectorEncoding,
    /// ponytail: serialized into the staging payload as a JSON byte array for
    /// this task; the bounded-batch worker may switch the payload to a compact
    /// binary form if profiled size matters.
    pub vector: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct SourceTranscript {
    pub id: String,
    pub text: String,
    pub speaker: Option<String>,
    pub timestamp: String,
    pub audio_start_time: Option<f64>,
    pub audio_end_time: Option<f64>,
}

/// Authoritative meeting content read from primary tables (never from
/// `meeting_fts`), using the same latest-summary policy and transcript
/// chronology as saved-meeting Chat.
#[derive(Debug, Clone)]
pub struct MeetingSource {
    pub meeting_id: String,
    pub title: String,
    pub source_revision: Option<i64>,
    pub latest_summary_template_id: Option<String>,
    pub latest_summary_markdown: Option<String>,
    pub notes_markdown: Option<String>,
    pub transcripts: Vec<SourceTranscript>,
}

#[derive(Debug, Clone)]
pub struct FtsDueItem {
    pub meeting_id: String,
    pub source_revision: i64,
    pub fts_projection_revision: i64,
    pub fts_indexed_revision: i64,
    pub attempt_count: i64,
}

#[derive(Debug, Clone)]
pub struct GenerationWorkItem {
    pub meeting_id: String,
    pub indexed_source_revision: i64,
    pub source_revision: i64,
    pub state: String,
    pub attempt_count: i64,
}

#[derive(Debug, Clone)]
pub struct IndexChange {
    pub change_id: i64,
    pub meeting_id: String,
    pub operation: String,
    pub source_revision: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerationStatus {
    pub generation_id: String,
    pub model_id: String,
    pub state: String,
    pub document_count: i64,
    pub tracked_meetings: i64,
    pub current_meetings: i64,
    pub retry_meetings: i64,
    pub failed_meetings: i64,
    pub canonical_change_id: Option<i64>,
    pub published_change_id: Option<i64>,
}

pub struct ReplacementJob<'a> {
    pub generation_id: &'a str,
    pub meeting_id: &'a str,
    pub expected_source_revision: i64,
    pub job_id: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReplacementOutcome {
    Published { change_id: i64 },
    RevisionConflict { current_revision: Option<i64> },
}

pub struct RetrievalRepository;

impl RetrievalRepository {
    // -- Registration and lifecycle --------------------------------------

    pub async fn register_model(pool: &SqlitePool, spec: &ModelSpec) -> Result<(), SqlxError> {
        if spec.dimensions == 0 {
            return Err(SqlxError::Protocol(
                "model dimensions must be positive".into(),
            ));
        }
        if let Some(scale) = spec.dequantization_scale {
            if !(scale.is_finite() && scale > 0.0) {
                return Err(SqlxError::Protocol(
                    "dequantization scale must be finite and positive".into(),
                ));
            }
        }
        match spec.vector_encoding {
            VectorEncoding::Int8 => {
                if spec.dequantization_scale.is_none() {
                    return Err(SqlxError::Protocol(
                        "int8 models require persisted dequantization parameters".into(),
                    ));
                }
            }
            _ => {
                if spec.dequantization_scale.is_some() || spec.dequantization_zero_point.is_some() {
                    return Err(SqlxError::Protocol(
                        "float encodings must not carry dequantization parameters".into(),
                    ));
                }
            }
        }
        sqlx::query(
            "INSERT INTO retrieval_models (model_id, dimensions, vector_encoding, chunker_version, dequantization_scale, dequantization_zero_point, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&spec.model_id)
        .bind(spec.dimensions)
        .bind(spec.vector_encoding.as_str())
        .bind(spec.chunker_version)
        .bind(spec.dequantization_scale)
        .bind(spec.dequantization_zero_point)
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Registers a build generation for `model_id`, creates its durable
    /// exact-backend index state atomically, and seeds pending work for every
    /// existing meeting; meetings created afterwards are seeded for live
    /// generations by the meeting-insert trigger. Pure bookkeeping: nothing
    /// here tokenizes or runs inference. Returns the number of seeded work
    /// items.
    pub async fn register_generation(
        pool: &SqlitePool,
        generation_id: &str,
        model_id: &str,
    ) -> Result<u64, SqlxError> {
        // ponytail: the write lock serializes registration against the
        // meeting-insert trigger, so a concurrently committed meeting cannot
        // miss this generation's seed; single-owner registration makes that
        // race rare, and the lock closes it outright.
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let retained: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM retrieval_generations g
             JOIN retrieval_index_state s ON s.generation_id = g.generation_id
             WHERE g.state IN ('building', 'ready', 'failed', 'retired')",
        )
        .fetch_one(&mut *tx)
        .await?;
        if retained.0 >= 2 {
            return Err(SqlxError::Protocol(
                "generation retention limit reached".into(),
            ));
        }
        let exists: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM retrieval_models WHERE model_id = ?")
                .bind(model_id)
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Err(SqlxError::Protocol(format!("unknown model '{}'", model_id)));
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO retrieval_generations (generation_id, model_id, state, created_at) VALUES (?, ?, 'building', ?)",
        )
        .bind(generation_id)
        .bind(model_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO retrieval_index_state (generation_id, backend, state, document_count, canonical_change_id, published_change_id, updated_at)
             VALUES (?, ?, 'building', 0, 0, 0, ?)",
        )
        .bind(generation_id)
        .bind(EXACT_INDEX_BACKEND)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        let seeded = sqlx::query(
            "INSERT INTO retrieval_meeting_state (generation_id, meeting_id, state, updated_at)
             SELECT ?, id, 'pending', ? FROM meetings WHERE true
             ON CONFLICT(generation_id, meeting_id) DO NOTHING",
        )
        .bind(generation_id)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(seeded)
    }

    pub async fn set_generation_state(
        pool: &SqlitePool,
        generation_id: &str,
        state: &str,
    ) -> Result<(), SqlxError> {
        if !GENERATION_STATES.contains(&state) {
            return Err(SqlxError::Protocol(format!(
                "'{}' is not a permitted generation state",
                state
            )));
        }
        let result =
            sqlx::query("UPDATE retrieval_generations SET state = ? WHERE generation_id = ?")
                .bind(state)
                .bind(generation_id)
                .execute(pool)
                .await?;
        if result.rows_affected() == 0 {
            return Err(SqlxError::Protocol(format!(
                "unknown generation '{}'",
                generation_id
            )));
        }
        Ok(())
    }

    /// Atomically moves the singleton active-generation pointer to `generation_id`,
    /// retiring the previous active generation. The pointer is the only
    /// authority on which generation is active; `'active'` is not a state.
    pub async fn switch_active_generation(
        pool: &SqlitePool,
        generation_id: &str,
    ) -> Result<(), SqlxError> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let state: Option<(String,)> =
            sqlx::query_as("SELECT state FROM retrieval_generations WHERE generation_id = ?")
                .bind(generation_id)
                .fetch_optional(&mut *tx)
                .await?;
        match state {
            None => {
                return Err(SqlxError::Protocol(format!(
                    "unknown generation '{}'",
                    generation_id
                )))
            }
            Some((state,)) if state == "ready" => {}
            Some((state,)) => {
                return Err(SqlxError::Protocol(format!(
                    "generation '{}' cannot be activated from state '{}'",
                    generation_id, state
                )))
            }
        }
        let previous: Option<String> =
            sqlx::query_as("SELECT generation_id FROM retrieval_active_model WHERE singleton = 1")
                .fetch_optional(&mut *tx)
                .await?
                .map(|(id,)| id);
        if previous.as_deref() == Some(generation_id) {
            tx.commit().await?;
            return Ok(());
        }
        if let Some(previous_id) = previous {
            sqlx::query(
                "UPDATE retrieval_generations SET state = 'retired', retired_at = ? WHERE generation_id = ?",
            )
            .bind(Utc::now().to_rfc3339())
            .bind(&previous_id)
            .execute(&mut *tx)
            .await?;
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO retrieval_active_model (singleton, generation_id, activated_at) VALUES (1, ?, ?)
             ON CONFLICT(singleton) DO UPDATE SET generation_id = excluded.generation_id, activated_at = excluded.activated_at",
        )
        .bind(generation_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE retrieval_generations SET activated_at = ? WHERE generation_id = ?")
            .bind(now)
            .bind(generation_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Revalidates completeness and the caught-up publication bound while
    /// atomically making a generation ready and active.
    pub async fn activate_generation_if_ready(
        pool: &SqlitePool,
        generation_id: &str,
        caught_up_to: i64,
    ) -> Result<bool, SqlxError> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let state: Option<(String,)> =
            sqlx::query_as("SELECT state FROM retrieval_generations WHERE generation_id = ?")
                .bind(generation_id)
                .fetch_optional(&mut *tx)
                .await?;
        if state.as_ref().is_none_or(|(state,)| state != "building") {
            tx.rollback().await?;
            return Ok(false);
        }
        let coverage: (i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*),
                    COALESCE(SUM(ms.indexed_source_revision >= s.source_revision), 0),
                    COALESCE(SUM(ms.state = 'failed'), 0)
             FROM retrieval_meeting_state ms
             JOIN search_source_state s ON s.meeting_id = ms.meeting_id
             WHERE ms.generation_id = ?",
        )
        .bind(generation_id)
        .fetch_one(&mut *tx)
        .await?;
        let canonical: Option<(i64,)> = sqlx::query_as(
            "SELECT canonical_change_id FROM retrieval_index_state WHERE generation_id = ?",
        )
        .bind(generation_id)
        .fetch_optional(&mut *tx)
        .await?;
        if coverage.0 == 0
            || coverage.0 != coverage.1
            || coverage.2 > 0
            || canonical.is_none_or(|(canonical,)| canonical > caught_up_to)
        {
            tx.rollback().await?;
            return Ok(false);
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE retrieval_generations SET state = 'ready', activated_at = ? WHERE generation_id = ?")
            .bind(&now)
            .bind(generation_id)
            .execute(&mut *tx)
            .await?;
        if let Some((previous,)) = sqlx::query_as::<_, (String,)>(
            "SELECT generation_id FROM retrieval_active_model WHERE singleton = 1",
        )
        .fetch_optional(&mut *tx)
        .await?
        {
            if previous != generation_id {
                sqlx::query("UPDATE retrieval_generations SET state = 'retired', retired_at = ? WHERE generation_id = ?")
                    .bind(&now)
                    .bind(previous)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        sqlx::query(
            "INSERT INTO retrieval_active_model (singleton, generation_id, activated_at) VALUES (1, ?, ?)
             ON CONFLICT(singleton) DO UPDATE SET generation_id = excluded.generation_id, activated_at = excluded.activated_at",
        )
        .bind(generation_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn active_generation_id(pool: &SqlitePool) -> Result<Option<String>, SqlxError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT generation_id FROM retrieval_active_model WHERE singleton = 1")
                .fetch_optional(pool)
                .await?;
        Ok(row.map(|(id,)| id))
    }

    /// Deletes a derived generation (shadow cancel, rebuild cleanup, GC).
    /// Refuses to touch the active generation and any generation whose journal
    /// changes were not fully acknowledged; acknowledged journal rows are
    /// pruned with it. Documents/staging/meeting state cascade via FK.
    pub async fn delete_generation(
        pool: &SqlitePool,
        generation_id: &str,
    ) -> Result<bool, SqlxError> {
        if Self::active_generation_id(pool).await?.as_deref() == Some(generation_id) {
            return Err(SqlxError::Protocol(
                "refusing to delete the active generation".into(),
            ));
        }
        if let Some((canonical, published)) = Self::publication_lag(pool, generation_id).await? {
            if canonical > published {
                return Err(SqlxError::Protocol(format!(
                    "generation '{}' has unacknowledged journal changes",
                    generation_id
                )));
            }
        }
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM retrieval_index_changes WHERE generation_id = ?")
            .bind(generation_id)
            .execute(&mut *tx)
            .await?;
        let deleted = sqlx::query("DELETE FROM retrieval_generations WHERE generation_id = ?")
            .bind(generation_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        Ok(deleted > 0)
    }

    // -- Durable due-work selection (non-destructive reads) ---------------

    pub async fn list_due_fts_repairs(
        pool: &SqlitePool,
        now: &str,
        limit: i64,
    ) -> Result<Vec<FtsDueItem>, SqlxError> {
        let rows: Vec<(String, i64, i64, i64, i64)> = sqlx::query_as(
            "SELECT meeting_id, source_revision, fts_projection_revision, fts_indexed_revision, fts_attempt_count
             FROM search_source_state
             WHERE fts_indexed_revision < fts_projection_revision
               AND (fts_next_attempt_at IS NULL OR fts_next_attempt_at <= ?)
             ORDER BY fts_next_attempt_at
             LIMIT ?",
        )
        .bind(now)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    meeting_id,
                    source_revision,
                    fts_projection_revision,
                    fts_indexed_revision,
                    attempt_count,
                )| {
                    FtsDueItem {
                        meeting_id,
                        source_revision,
                        fts_projection_revision,
                        fts_indexed_revision,
                        attempt_count,
                    }
                },
            )
            .collect())
    }

    /// Marks FTS indexed only when the projection revision still equals the
    /// revision selected in [`FtsDueItem`]. A concurrent source/folder
    /// mutation between refresh and mark advances the projection past what the
    /// refresh actually contained: returns `false`, nothing is copied, and the
    /// meeting stays due so the next repair covers the newer projection.
    pub async fn mark_fts_indexed(
        pool: &SqlitePool,
        meeting_id: &str,
        expected_projection_revision: i64,
    ) -> Result<bool, SqlxError> {
        let result = sqlx::query(
            "UPDATE search_source_state
             SET fts_indexed_revision = ?,
                 fts_attempt_count = 0,
                 fts_next_attempt_at = NULL,
                 fts_last_error = NULL
             WHERE meeting_id = ? AND fts_projection_revision = ?",
        )
        .bind(expected_projection_revision)
        .bind(meeting_id)
        .bind(expected_projection_revision)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn record_fts_failure(
        pool: &SqlitePool,
        meeting_id: &str,
        safe_error: &str,
        next_attempt_at: &str,
    ) -> Result<(), SqlxError> {
        sqlx::query(
            "UPDATE search_source_state
             SET fts_attempt_count = fts_attempt_count + 1,
                 fts_next_attempt_at = ?,
                 fts_last_error = ?
             WHERE meeting_id = ?",
        )
        .bind(next_attempt_at)
        .bind(truncate_safe_error(safe_error))
        .bind(meeting_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn list_due_generation_work(
        pool: &SqlitePool,
        generation_id: &str,
        now: &str,
        limit: i64,
    ) -> Result<Vec<GenerationWorkItem>, SqlxError> {
        let rows: Vec<(String, i64, i64, String, i64)> = sqlx::query_as(
            "SELECT ms.meeting_id, ms.indexed_source_revision, s.source_revision, ms.state, ms.attempt_count
             FROM retrieval_meeting_state ms
             JOIN search_source_state s ON s.meeting_id = ms.meeting_id
             WHERE ms.generation_id = ?
               AND ms.state IN ('pending', 'retry')
               AND ms.indexed_source_revision < s.source_revision
               AND (ms.next_attempt_at IS NULL OR ms.next_attempt_at <= ?)
             ORDER BY ms.state, ms.next_attempt_at
             LIMIT ?",
        )
        .bind(generation_id)
        .bind(now)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(meeting_id, indexed_source_revision, source_revision, state, attempt_count)| {
                    GenerationWorkItem {
                        meeting_id,
                        indexed_source_revision,
                        source_revision,
                        state,
                        attempt_count,
                    }
                },
            )
            .collect())
    }

    /// Persists retry state after a failed indexing attempt. `terminal` marks
    /// the item permanently failed (an activation blocker) instead of
    /// scheduled for retry; either way the queue keeps its place so one poison
    /// meeting never destroys or starves other work.
    pub async fn record_work_failure(
        pool: &SqlitePool,
        generation_id: &str,
        meeting_id: &str,
        terminal: bool,
        safe_error: &str,
        next_attempt_at: &str,
    ) -> Result<(), SqlxError> {
        sqlx::query(
            "UPDATE retrieval_meeting_state
             SET state = CASE WHEN ? THEN 'failed' ELSE 'retry' END,
                 attempt_count = attempt_count + 1,
                 next_attempt_at = ?,
                 last_error = ?,
                 updated_at = ?
             WHERE generation_id = ? AND meeting_id = ?",
        )
        .bind(terminal)
        .bind(next_attempt_at)
        .bind(truncate_safe_error(safe_error))
        .bind(Utc::now().to_rfc3339())
        .bind(generation_id)
        .bind(meeting_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Live generations with their immutable model identities, still
    /// collecting durable work: 'building' shadows and 'ready' active
    /// generations. Terminal states never run a worker again. The model id
    /// lets callers refuse embedding a generation under a different model.
    pub async fn list_live_generations(
        pool: &SqlitePool,
    ) -> Result<Vec<(String, String)>, SqlxError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT generation_id, model_id FROM retrieval_generations
             WHERE state IN ('building', 'ready')
             ORDER BY created_at, generation_id",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Idempotent model registration; reports whether this call created the row.
    pub async fn ensure_model(pool: &SqlitePool, spec: &ModelSpec) -> Result<bool, SqlxError> {
        let exists: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM retrieval_models WHERE model_id = ?")
                .bind(&spec.model_id)
                .fetch_optional(pool)
                .await?;
        if exists.is_some() {
            return Ok(false);
        }
        Self::register_model(pool, spec).await?;
        Ok(true)
    }

    /// Idempotent generation registration with exactly the bookkeeping of
    /// [`Self::register_generation`]; reports whether this call created it.
    pub async fn ensure_generation(
        pool: &SqlitePool,
        generation_id: &str,
        model_id: &str,
    ) -> Result<bool, SqlxError> {
        let exists: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM retrieval_generations WHERE generation_id = ?")
                .bind(generation_id)
                .fetch_optional(pool)
                .await?;
        if exists.is_some() {
            return Ok(false);
        }
        match Self::register_generation(pool, generation_id, model_id).await {
            Ok(_) => Ok(true),
            Err(SqlxError::Database(error)) if error.is_unique_violation() => Ok(false),
            Err(error) => Err(error),
        }
    }

    // -- Authoritative source retrieval -----------------------------------

    pub async fn current_source_revision(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<i64>, SqlxError> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT source_revision FROM search_source_state WHERE meeting_id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;
        Ok(row.map(|(revision,)| revision))
    }

    pub async fn load_meeting_source(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingSource>, SqlxError> {
        let meta: Option<(String, Option<i64>)> = sqlx::query_as(
            "SELECT m.title, s.source_revision
             FROM meetings m LEFT JOIN search_source_state s ON s.meeting_id = m.id
             WHERE m.id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?;
        let Some((title, source_revision)) = meta else {
            return Ok(None);
        };

        let notes: Option<(Option<String>,)> =
            sqlx::query_as("SELECT notes_markdown FROM meeting_notes WHERE meeting_id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;

        // Latest-valid-summary policy, matching saved-meeting Chat: candidates
        // are read newest-first and parsed in Rust so one malformed JSON
        // result cannot fail an otherwise valid extraction; older readable
        // summaries still apply. Unreadable results are counted for a
        // privacy-safe log (never content) and skipped.
        let summary_rows: Vec<(Option<String>, String)> = sqlx::query_as(
            "SELECT result, template_id
             FROM summary_processes
             WHERE meeting_id = ? AND result IS NOT NULL
             ORDER BY updated_at DESC, template_id DESC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
        let mut latest_summary_template_id = None;
        let mut latest_summary_markdown = None;
        let mut unreadable_summaries = 0_usize;
        for (result, template_id) in &summary_rows {
            let markdown = result
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|value| {
                    value
                        .get("markdown")
                        .and_then(|markdown| markdown.as_str())
                        .map(str::to_string)
                })
                .filter(|markdown| !markdown.trim().is_empty());
            if let Some(markdown) = markdown {
                latest_summary_template_id = Some(template_id.clone());
                latest_summary_markdown = Some(markdown);
                break;
            }
            unreadable_summaries += 1;
        }
        if unreadable_summaries > 0 {
            log::warn!(
                "Skipped {unreadable_summaries} unreadable summary result(s) while extracting meeting source"
            );
        }

        let transcripts: Vec<(
            String,
            String,
            Option<String>,
            String,
            Option<f64>,
            Option<f64>,
        )> = sqlx::query_as(
            "SELECT id, transcript, speaker, timestamp, audio_start_time, audio_end_time
                 FROM transcripts
                 WHERE meeting_id = ? AND transcript IS NOT NULL AND transcript != ''
                 ORDER BY CASE WHEN audio_start_time IS NULL THEN 1 ELSE 0 END,
                          audio_start_time ASC, timestamp ASC, id ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        Ok(Some(MeetingSource {
            meeting_id: meeting_id.to_string(),
            title,
            source_revision,
            latest_summary_template_id,
            latest_summary_markdown,
            notes_markdown: notes.and_then(|(markdown,)| markdown),
            transcripts: transcripts
                .into_iter()
                .map(
                    |(id, text, speaker, timestamp, audio_start_time, audio_end_time)| {
                        SourceTranscript {
                            id,
                            text,
                            speaker,
                            timestamp,
                            audio_start_time,
                            audio_end_time,
                        }
                    },
                )
                .collect(),
        }))
    }

    // -- Staged atomic replacement ----------------------------------------

    pub async fn stage_documents(
        pool: &SqlitePool,
        job_id: &str,
        generation_id: &str,
        meeting_id: &str,
        source_revision: i64,
        documents: &[StagedDocument],
    ) -> Result<(), SqlxError> {
        let model = generation_model(pool, generation_id)
            .await?
            .ok_or_else(|| {
                SqlxError::Protocol(format!("unknown generation '{}'", generation_id))
            })?;
        for document in documents {
            validate_document(&model, document)?;
        }
        let mut tx = pool.begin().await?;
        for document in documents {
            let payload = serde_json::to_vec(document).map_err(|error| {
                SqlxError::Protocol(format!("failed to serialize staged document: {}", error))
            })?;
            sqlx::query(
                "INSERT INTO retrieval_document_staging (job_id, generation_id, meeting_id, source_revision, document_id, payload)
                 VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT(job_id, document_id) DO UPDATE SET
                     source_revision = excluded.source_revision,
                     payload = excluded.payload",
            )
            .bind(job_id)
            .bind(generation_id)
            .bind(meeting_id)
            .bind(source_revision)
            .bind(&document.document_id)
            .bind(payload)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn discard_staging_job(pool: &SqlitePool, job_id: &str) -> Result<(), SqlxError> {
        sqlx::query("DELETE FROM retrieval_document_staging WHERE job_id = ?")
            .bind(job_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Reads a staged job back (validated against the owning generation's
    /// model) so a crashed worker can resume instead of re-embedding batches
    /// that were already staged before the crash.
    pub async fn list_staged_documents(
        pool: &SqlitePool,
        job_id: &str,
    ) -> Result<Vec<StagedDocument>, SqlxError> {
        let rows: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
            "SELECT generation_id, document_id, payload
             FROM retrieval_document_staging WHERE job_id = ? ORDER BY id",
        )
        .bind(job_id)
        .fetch_all(pool)
        .await?;
        let Some((generation_id, _, _)) = rows.first() else {
            return Ok(Vec::new());
        };
        let model = generation_model(pool, generation_id)
            .await?
            .ok_or_else(|| SqlxError::Protocol(format!("unknown generation '{generation_id}'")))?;
        rows.into_iter()
            .map(|(_, document_id, payload)| {
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
                validate_document(&model, &document)?;
                Ok(document)
            })
            .collect()
    }

    /// Removes staged rows that no longer belong to the current chunk set for
    /// their job (divergent leftovers from a changed chunker/model contract),
    /// keeping publication exactly mirrored to freshly extracted documents.
    pub async fn retain_staged_documents(
        pool: &SqlitePool,
        job_id: &str,
        keep_document_ids: &[String],
    ) -> Result<u64, SqlxError> {
        let keep: HashSet<&str> = keep_document_ids.iter().map(String::as_str).collect();
        let existing: Vec<String> = sqlx::query_scalar(
            "SELECT document_id FROM retrieval_document_staging WHERE job_id = ?",
        )
        .bind(job_id)
        .fetch_all(pool)
        .await?;
        let stale: Vec<String> = existing
            .into_iter()
            .filter(|document_id| !keep.contains(document_id.as_str()))
            .collect();
        let mut removed = 0_u64;
        for chunk in stale.chunks(500) {
            let placeholders = std::iter::repeat("?")
                .take(chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "DELETE FROM retrieval_document_staging
                 WHERE job_id = ? AND document_id IN ({placeholders})"
            );
            let mut query = sqlx::query(&sql).bind(job_id);
            for document_id in chunk {
                query = query.bind(document_id);
            }
            removed += query.execute(pool).await?.rows_affected();
        }
        Ok(removed)
    }

    /// Discards staging that can never publish: jobs bound to terminal
    /// generations, or bound to a source revision that no longer matches the
    /// meeting's current authoritative revision. Valid staging for current
    /// work survives so a crashed run can resume.
    pub async fn discard_stale_staging(pool: &SqlitePool) -> Result<u64, SqlxError> {
        let result = sqlx::query(
            "DELETE FROM retrieval_document_staging
             WHERE generation_id NOT IN (
                     SELECT generation_id FROM retrieval_generations
                     WHERE state IN ('building', 'ready')
                 )
                 OR source_revision != COALESCE((
                     SELECT s.source_revision FROM search_source_state s
                     WHERE s.meeting_id = retrieval_document_staging.meeting_id
                 ), -1)",
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Atomically replaces every active document of one meeting/generation
    /// from a staged job. The transaction re-reads the current source revision
    /// and aborts (keeping prior documents intact) unless it still equals the
    /// caller's extraction-time revision. On success it clears retry state,
    /// appends an `upsert` journal entry, and advances that generation's
    /// canonical change ID in the same commit.
    pub async fn replace_meeting_documents(
        pool: &SqlitePool,
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
            // Stale extraction: discard the job's staging and keep whatever is
            // currently published for this meeting untouched.
            sqlx::query("DELETE FROM retrieval_document_staging WHERE job_id = ?")
                .bind(job.job_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(ReplacementOutcome::RevisionConflict { current_revision });
        }

        let model = generation_model(&mut *tx, job.generation_id)
            .await?
            .ok_or_else(|| {
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
            validate_document(&model, &document)?;
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

    /// Reads back and validates every stored document vector for one
    /// meeting/generation, decoded to f32. A malformed row surfaces as an
    /// error naming its document so callers quarantine/rebuild it instead of
    /// admitting a bad vector to an index.
    pub async fn read_validated_documents(
        pool: &SqlitePool,
        generation_id: &str,
        meeting_id: &str,
    ) -> Result<Vec<(String, Vec<f32>)>, SqlxError> {
        let model = generation_model(pool, generation_id)
            .await?
            .ok_or_else(|| {
                SqlxError::Protocol(format!("unknown generation '{}'", generation_id))
            })?;
        let dequant = match model.vector_encoding {
            VectorEncoding::Int8 => {
                let scale = model.dequantization_scale.ok_or_else(|| {
                    SqlxError::Protocol("int8 model is missing its dequantization scale".into())
                })?;
                Some((scale, model.dequantization_zero_point.unwrap_or(0)))
            }
            _ => None,
        };
        let rows: Vec<(String, i64, Vec<u8>)> = sqlx::query_as(
            "SELECT document_id, dimensions, vector
             FROM retrieval_documents
             WHERE generation_id = ? AND meeting_id = ?
             ORDER BY ordinal",
        )
        .bind(generation_id)
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
        rows.into_iter()
            .map(|(document_id, dimensions, vector)| {
                let values = decoded_vector(
                    model.vector_encoding,
                    model.dimensions,
                    dequant,
                    dimensions,
                    &vector,
                )
                .map_err(|reason| {
                    SqlxError::Protocol(format!("document '{}': {}", document_id, reason))
                })?;
                Ok((document_id, values))
            })
            .collect()
    }

    /// Reads every validated canonical document of one generation (optionally
    /// scoped to one meeting) for in-memory snapshot construction or delta
    /// reload. Validation is encoding-aware; malformed rows are returned as
    /// per-document rejections naming their meeting so callers quarantine the
    /// affected meeting instead of admitting a bad vector or aborting a load.
    ///
    /// ponytail: paged reads keep only one page of multi-KB rows resident, but
    /// per-page validation runs on the calling task; move to a dedicated
    /// blocking thread if 250k-scale load profiling ever shows it matters.
    pub async fn read_snapshot_documents(
        pool: &SqlitePool,
        generation_id: &str,
        meeting_id: Option<&str>,
    ) -> Result<SnapshotPage, SqlxError> {
        let model = generation_model(pool, generation_id)
            .await?
            .ok_or_else(|| SqlxError::Protocol(format!("unknown generation '{generation_id}'")))?;
        if model.vector_encoding != VectorEncoding::Int8 {
            // ponytail: the exact query index stores the approved int8 base
            // contract only; a future float-encoding bundle adds a second base
            // representation here behind the same repository boundary.
            return Err(SqlxError::Protocol(format!(
                "the exact query index supports int8 generations but '{}' encodes '{}'",
                generation_id,
                model.vector_encoding.as_str()
            )));
        }
        let scale = model.dequantization_scale.ok_or_else(|| {
            SqlxError::Protocol("int8 model is missing its dequantization scale".into())
        })?;
        let zero_point = model.dequantization_zero_point.unwrap_or(0);

        const PAGE_ROWS: i64 = 2000;
        let mut page = SnapshotPage::default();
        let mut last_id = 0_i64;
        loop {
            let rows: Vec<SnapshotRow> = match meeting_id {
                Some(meeting) => {
                    sqlx::query_as(
                        "SELECT id, document_id, meeting_id, source_kind, source_start_id, source_end_id, source_template_id, heading, ordinal, dimensions, vector
                         FROM retrieval_documents
                          WHERE generation_id = ? AND meeting_id = ? AND id > ?
                            AND NOT EXISTS (SELECT 1 FROM retrieval_meeting_state ms WHERE ms.generation_id = retrieval_documents.generation_id AND ms.meeting_id = retrieval_documents.meeting_id AND ms.state = 'failed')
                         ORDER BY id LIMIT ?",
                    )
                    .bind(generation_id)
                    .bind(meeting)
                    .bind(last_id)
                    .bind(PAGE_ROWS)
                    .fetch_all(pool)
                    .await?
                }
                None => {
                    sqlx::query_as(
                        "SELECT id, document_id, meeting_id, source_kind, source_start_id, source_end_id, source_template_id, heading, ordinal, dimensions, vector
                         FROM retrieval_documents
                          WHERE generation_id = ? AND id > ?
                            AND NOT EXISTS (SELECT 1 FROM retrieval_meeting_state ms WHERE ms.generation_id = retrieval_documents.generation_id AND ms.meeting_id = retrieval_documents.meeting_id AND ms.state = 'failed')
                         ORDER BY id LIMIT ?",
                    )
                    .bind(generation_id)
                    .bind(last_id)
                    .bind(PAGE_ROWS)
                    .fetch_all(pool)
                    .await?
                }
            };
            if rows.is_empty() {
                break;
            }
            last_id = rows[rows.len() - 1].0;
            append_validated_snapshot_rows(model.dimensions, scale, zero_point, rows, &mut page)?;
        }
        Ok(page)
    }

    /// Full canonical load for publication: vector rows, the replay bound
    /// (`canonical_change_id`), and the generation's model identity are read
    /// from ONE SQLite read transaction, so a concurrently committed
    /// upsert/delete is either fully inside the loaded rows with its change
    /// inside the bound, or fully outside both - never acknowledged while
    /// absent from or present in the snapshot inconsistently. Returns `None`
    /// when the caller's cancellation fired between pages (nothing was read
    /// to completion and nothing may be acknowledged from it).
    pub async fn read_canonical_snapshot(
        pool: &SqlitePool,
        generation_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<CanonicalSnapshotRead>, SqlxError> {
        let mut tx = pool.begin().await?;
        let read = Self::read_canonical_snapshot_tx(&mut tx, generation_id, cancel).await?;
        tx.commit().await?;
        Ok(read)
    }

    async fn read_canonical_snapshot_tx(
        tx: &mut Transaction<'_, Sqlite>,
        generation_id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<CanonicalSnapshotRead>, SqlxError> {
        let identity: Option<(i64, String, String, Option<f64>, Option<i64>)> = sqlx::query_as(
            "SELECT m.dimensions, m.model_id, m.vector_encoding, m.dequantization_scale, m.dequantization_zero_point
             FROM retrieval_generations g JOIN retrieval_models m ON m.model_id = g.model_id
             WHERE g.generation_id = ?",
        )
        .bind(generation_id)
        .fetch_optional(&mut **tx)
        .await?;
        let Some((dimensions, model_id, encoding, scale, zero_point)) = identity else {
            return Err(SqlxError::Protocol(format!(
                "generation '{generation_id}' has no model"
            )));
        };
        // ponytail: the exact query index stores the approved int8 base
        // contract only; a future float-encoding bundle adds a second base
        // representation here behind the same repository boundary.
        if VectorEncoding::parse(&encoding)? != VectorEncoding::Int8 {
            return Err(SqlxError::Protocol(format!(
                "the exact query index supports int8 generations but '{generation_id}' encodes '{encoding}'"
            )));
        }
        let scale = scale.ok_or_else(|| {
            SqlxError::Protocol("int8 model is missing its dequantization scale".into())
        })?;
        let zero_point = zero_point.unwrap_or(0);
        let bound: Option<(i64,)> = sqlx::query_as(
            "SELECT canonical_change_id FROM retrieval_index_state WHERE generation_id = ?",
        )
        .bind(generation_id)
        .fetch_optional(&mut **tx)
        .await?;
        let Some((canonical_change_id,)) = bound else {
            return Err(SqlxError::Protocol(format!(
                "generation '{generation_id}' has no index state"
            )));
        };

        const PAGE_ROWS: i64 = 2000;
        let mut page = SnapshotPage::default();
        let mut last_id = 0_i64;
        loop {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            let rows: Vec<SnapshotRow> = sqlx::query_as(
                "SELECT id, document_id, meeting_id, source_kind, source_start_id, source_end_id, source_template_id, heading, ordinal, dimensions, vector
                 FROM retrieval_documents
                  WHERE generation_id = ? AND id > ?
                    AND NOT EXISTS (SELECT 1 FROM retrieval_meeting_state ms WHERE ms.generation_id = retrieval_documents.generation_id AND ms.meeting_id = retrieval_documents.meeting_id AND ms.state = 'failed')
                 ORDER BY id LIMIT ?",
            )
            .bind(generation_id)
            .bind(last_id)
            .bind(PAGE_ROWS)
            .fetch_all(&mut **tx)
            .await?;
            if rows.is_empty() {
                break;
            }
            last_id = rows[rows.len() - 1].0;
            append_validated_snapshot_rows(dimensions, scale, zero_point, rows, &mut page)?;
        }
        Ok(Some(CanonicalSnapshotRead {
            page,
            canonical_change_id,
            model_id,
            dimensions: dimensions.max(0) as usize,
        }))
    }

    /// Marks one generation/meeting's work as owed again so the worker
    /// re-indexes it after quarantine (malformed derived rows are rebuilt,
    /// never served and never fatal).
    pub async fn requeue_meeting_work(
        pool: &SqlitePool,
        generation_id: &str,
        meeting_id: &str,
        safe_error: &str,
    ) -> Result<(), SqlxError> {
        sqlx::query(
            "UPDATE retrieval_meeting_state
             SET indexed_source_revision = 0,
                 state = 'pending',
                 next_attempt_at = NULL,
                 last_error = ?,
                 updated_at = ?
             WHERE generation_id = ? AND meeting_id = ?",
        )
        .bind(truncate_safe_error(safe_error))
        .bind(Utc::now().to_rfc3339())
        .bind(generation_id)
        .bind(meeting_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Known-corrupt active retrieval is deactivated to FTS-only: clears the
    /// singleton pointer inside one transaction and marks the generation
    /// failed so no worker revives it. Returns the deactivated generation.
    pub async fn deactivate_active_generation(
        pool: &SqlitePool,
    ) -> Result<Option<String>, SqlxError> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let current: Option<(String,)> =
            sqlx::query_as("SELECT generation_id FROM retrieval_active_model WHERE singleton = 1")
                .fetch_optional(&mut *tx)
                .await?;
        let Some((generation_id,)) = current else {
            tx.commit().await?;
            return Ok(None);
        };
        sqlx::query("DELETE FROM retrieval_active_model WHERE singleton = 1")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE retrieval_generations SET state = 'failed', retired_at = ? WHERE generation_id = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(&generation_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(generation_id))
    }

    /// Generations still counted against the two-generation retention ceiling:
    /// live builds plus terminal generations awaiting garbage collection.
    pub async fn count_retained_generations(pool: &SqlitePool) -> Result<i64, SqlxError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM retrieval_generations g
             JOIN retrieval_index_state s ON s.generation_id = g.generation_id
              WHERE g.state IN ('building', 'ready', 'failed', 'retired')",
        )
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    pub async fn cancel_building_generation(
        pool: &SqlitePool,
        generation_id: &str,
    ) -> Result<bool, SqlxError> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let state: Option<(String,)> =
            sqlx::query_as("SELECT state FROM retrieval_generations WHERE generation_id = ?")
                .bind(generation_id)
                .fetch_optional(&mut *tx)
                .await?;
        if state.as_ref().is_none_or(|(state,)| state != "building") {
            tx.rollback().await?;
            return Ok(false);
        }
        let active: Option<(String,)> =
            sqlx::query_as("SELECT generation_id FROM retrieval_active_model WHERE singleton = 1")
                .fetch_optional(&mut *tx)
                .await?;
        if active
            .as_ref()
            .is_some_and(|(active,)| active == generation_id)
        {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query("DELETE FROM retrieval_index_changes WHERE generation_id = ?")
            .bind(generation_id)
            .execute(&mut *tx)
            .await?;
        let deleted = sqlx::query(
            "DELETE FROM retrieval_generations WHERE generation_id = ? AND state = 'building'",
        )
        .bind(generation_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(deleted > 0)
    }

    /// Non-active terminal generations with their retirement timestamps, for
    /// restart-plus-successful-query gated garbage collection.
    pub async fn retired_generations(
        pool: &SqlitePool,
    ) -> Result<Vec<(String, String)>, SqlxError> {
        sqlx::query_as(
            "SELECT generation_id, retired_at FROM retrieval_generations
              WHERE state IN ('failed', 'retired')
                AND generation_id NOT IN (SELECT generation_id FROM retrieval_active_model)
              ORDER BY retired_at",
        )
        .fetch_all(pool)
        .await
    }

    /// Payload-row ESTIMATE of derived bytes: `retrieval_documents`
    /// content/hash/vector bytes plus staging payloads, with a fixed per-row
    /// overhead allowance for identity columns. This is a lower-bound-ish
    /// estimate only - it excludes indexes, journal/state rows, page
    /// fragmentation, free pages, and WAL - so status labels it as an
    /// estimate and the activation gate must use
    /// [`Self::derived_backing_store_upper_bound_bytes`] instead.
    pub async fn derived_disk_bytes(pool: &SqlitePool) -> Result<u64, SqlxError> {
        let documents: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(length(content) + length(vector) + length(content_hash)), 0)
             FROM retrieval_documents",
        )
        .fetch_one(pool)
        .await?;
        let staging: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(length(payload)), 0) FROM retrieval_document_staging",
        )
        .fetch_one(pool)
        .await?;
        const PER_ROW_OVERHEAD: u64 = 192;
        Ok(documents.0 as u64 * PER_ROW_OVERHEAD
            + staging.0 as u64 * PER_ROW_OVERHEAD
            + documents.1.max(0) as u64
            + staging.1.max(0) as u64)
    }

    /// Conservative upper bound on the SQLite backing store this database can
    /// occupy: every allocated main-file page (measured via `PRAGMA
    /// page_count * page_size`) plus any committed-but-uncheckpointed WAL
    /// frames (`PRAGMA wal_checkpoint(PASSIVE)` frame count). It necessarily
    /// counts primary storage alongside derived data, so it may overcount -
    /// which is safe for a block-only activation gate - but it cannot
    /// undercount material data the way the payload estimate can.
    pub async fn derived_backing_store_upper_bound_bytes(
        pool: &SqlitePool,
    ) -> Result<u64, SqlxError> {
        let page_size: (i64,) = sqlx::query_as("PRAGMA page_size").fetch_one(pool).await?;
        let page_count: (i64,) = sqlx::query_as("PRAGMA page_count").fetch_one(pool).await?;
        // Non-WAL databases report no usable frame counts; tolerating a failed
        // probe there measures 0 WAL bytes, which is exact rather than lazy.
        let wal_frames = sqlx::query_as::<_, (i64, i64, i64)>("PRAGMA wal_checkpoint(PASSIVE)")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .map_or(0, |(_, log_frames, _)| log_frames.max(0) as u64);
        Ok(page_count.0.max(0) as u64 * page_size.0.max(0) as u64
            + wal_frames * page_size.0.max(0) as u64)
    }

    /// Resident-size estimate for building shadow generations
    /// (`document_count * dimensions` int8 bytes each), used by the
    /// activation envelope gate while a shadow snapshot does not exist yet.
    pub async fn estimated_shadow_snapshot_bytes(pool: &SqlitePool) -> Result<u64, SqlxError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(g.document_count * m.dimensions), 0)
             FROM retrieval_generations g JOIN retrieval_models m ON m.model_id = g.model_id
             WHERE g.state = 'building'",
        )
        .fetch_one(pool)
        .await?;
        Ok(row.0.max(0) as u64)
    }

    // -- Publication journal ----------------------------------------------

    pub async fn read_journal_since(
        pool: &SqlitePool,
        generation_id: &str,
        after_change_id: i64,
        limit: i64,
    ) -> Result<Vec<IndexChange>, SqlxError> {
        let rows: Vec<(i64, String, String, Option<i64>, String)> = sqlx::query_as(
            "SELECT change_id, meeting_id, operation, source_revision, created_at
             FROM retrieval_index_changes
             WHERE generation_id = ? AND change_id > ?
             ORDER BY change_id
             LIMIT ?",
        )
        .bind(generation_id)
        .bind(after_change_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(change_id, meeting_id, operation, source_revision, created_at)| IndexChange {
                    change_id,
                    meeting_id,
                    operation,
                    source_revision,
                    created_at,
                },
            )
            .collect())
    }

    /// Durably records that a publisher applied everything through
    /// `published_change_id`. Monotonic and idempotent.
    pub async fn acknowledge_journal(
        pool: &SqlitePool,
        generation_id: &str,
        published_change_id: i64,
    ) -> Result<(), SqlxError> {
        let current: Option<(i64, i64)> = sqlx::query_as(
            "SELECT canonical_change_id, published_change_id FROM retrieval_index_state WHERE generation_id = ?",
        )
        .bind(generation_id)
        .fetch_optional(pool)
        .await?;
        let Some((canonical, already_published)) = current else {
            return Err(SqlxError::Protocol(format!(
                "generation '{}' has no index state to acknowledge",
                generation_id
            )));
        };
        if published_change_id > canonical {
            return Err(SqlxError::Protocol(
                "cannot acknowledge beyond the canonical change ID".into(),
            ));
        }
        if published_change_id > already_published {
            sqlx::query(
                "UPDATE retrieval_index_state SET published_change_id = ?, updated_at = ? WHERE generation_id = ? AND published_change_id < ?",
            )
            .bind(published_change_id)
            .bind(Utc::now().to_rfc3339())
            .bind(generation_id)
            .bind(published_change_id)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn publication_lag(
        pool: &SqlitePool,
        generation_id: &str,
    ) -> Result<Option<(i64, i64)>, SqlxError> {
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT canonical_change_id, published_change_id FROM retrieval_index_state WHERE generation_id = ?",
        )
        .bind(generation_id)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    // -- Status and coverage ------------------------------------------------

    pub async fn generation_status(
        pool: &SqlitePool,
        generation_id: &str,
    ) -> Result<Option<GenerationStatus>, SqlxError> {
        let generation: Option<(String, String, i64)> = sqlx::query_as(
            "SELECT model_id, state, document_count FROM retrieval_generations WHERE generation_id = ?",
        )
        .bind(generation_id)
        .fetch_optional(pool)
        .await?;
        let Some((model_id, state, document_count)) = generation else {
            return Ok(None);
        };
        let coverage: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*),
                    COALESCE(SUM(ms.indexed_source_revision >= s.source_revision), 0),
                    COALESCE(SUM(ms.state = 'retry'), 0),
                    COALESCE(SUM(ms.state = 'failed'), 0)
             FROM retrieval_meeting_state ms
             JOIN search_source_state s ON s.meeting_id = ms.meeting_id
             WHERE ms.generation_id = ?",
        )
        .bind(generation_id)
        .fetch_one(pool)
        .await?;
        let ids = Self::publication_lag(pool, generation_id).await?;
        Ok(Some(GenerationStatus {
            generation_id: generation_id.to_string(),
            model_id,
            state,
            document_count,
            tracked_meetings: coverage.0,
            current_meetings: coverage.1,
            retry_meetings: coverage.2,
            failed_meetings: coverage.3,
            canonical_change_id: ids.map(|(canonical, _)| canonical),
            published_change_id: ids.map(|(_, published)| published),
        }))
    }

    pub async fn building_generation_statuses(
        pool: &SqlitePool,
    ) -> Result<Vec<GenerationStatus>, SqlxError> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT generation_id FROM retrieval_generations WHERE state = 'building' ORDER BY created_at, generation_id",
        )
        .fetch_all(pool)
        .await?;
        let mut statuses = Vec::with_capacity(ids.len());
        for generation_id in ids {
            if let Some(status) = Self::generation_status(pool, &generation_id).await? {
                statuses.push(status);
            }
        }
        Ok(statuses)
    }
}

/// One canonical derived document as admitted to the in-memory query index:
/// identity/provenance metadata plus the raw validated vector bytes of its
/// generation's encoding.
#[derive(Debug, Clone)]
pub struct SnapshotDocument {
    pub document_id: String,
    pub meeting_id: String,
    pub source_kind: String,
    pub source_start_id: Option<String>,
    pub source_end_id: Option<String>,
    pub source_template_id: Option<String>,
    /// Section-heading provenance persisted with the canonical row (`None`
    /// for transcript windows and pre-migration rows).
    pub heading: Option<String>,
    pub ordinal: i64,
}

/// A canonical row refused at the validation boundary. The affected meeting
/// is quarantined/rebuilt by the publisher, never admitted to an index.
#[derive(Debug, Clone)]
pub struct RejectedDocument {
    pub document_id: String,
    pub meeting_id: String,
    pub reason: String,
}

/// Validated snapshot reads: admissible documents plus per-document
/// rejections (malformed rows never abort the whole load).
#[derive(Debug, Default, Clone)]
pub struct SnapshotPage {
    pub documents: Vec<SnapshotDocument>,
    pub vectors: Vec<u8>,
    pub rejected: Vec<RejectedDocument>,
}

/// One raw canonical row as selected by the snapshot loaders.
type SnapshotRow = (
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i64,
    Vec<u8>,
);

/// One consistent full-canonical read for publication: validated rows plus
/// the replay bound and model identity observed in the same SQLite read
/// transaction, so the bound never describes changes the rows do not contain.
#[derive(Debug, Clone)]
pub struct CanonicalSnapshotRead {
    pub page: SnapshotPage,
    pub canonical_change_id: i64,
    pub model_id: String,
    pub dimensions: usize,
}

fn truncate_safe_error(error: &str) -> String {
    error.chars().take(300).collect()
}

async fn generation_model<'e, E>(
    executor: E,
    generation_id: &str,
) -> Result<Option<ModelDescriptor>, SqlxError>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let row: Option<(i64, String, Option<f64>, Option<i64>)> = sqlx::query_as(
        "SELECT m.dimensions, m.vector_encoding, m.dequantization_scale, m.dequantization_zero_point
         FROM retrieval_generations g JOIN retrieval_models m ON m.model_id = g.model_id
         WHERE g.generation_id = ?",
    )
    .bind(generation_id)
    .fetch_optional(executor)
    .await?;
    row.map(|(dimensions, encoding, scale, zero_point)| {
        Ok(ModelDescriptor {
            dimensions,
            vector_encoding: VectorEncoding::parse(&encoding)?,
            dequantization_scale: scale,
            dequantization_zero_point: zero_point,
        })
    })
    .transpose()
}

fn validate_document(model: &ModelDescriptor, document: &StagedDocument) -> Result<(), SqlxError> {
    if document.vector_encoding != model.vector_encoding {
        return Err(SqlxError::Protocol(format!(
            "document '{}' declares encoding '{}' but its generation encodes '{}'",
            document.document_id,
            document.vector_encoding.as_str(),
            model.vector_encoding.as_str()
        )));
    }
    let dequant = match model.vector_encoding {
        VectorEncoding::Int8 => {
            let scale = model.dequantization_scale.ok_or_else(|| {
                SqlxError::Protocol("int8 model is missing its dequantization scale".into())
            })?;
            Some((scale, model.dequantization_zero_point.unwrap_or(0)))
        }
        _ => None,
    };
    decoded_vector(
        model.vector_encoding,
        model.dimensions,
        dequant,
        document.dimensions,
        &document.vector,
    )
    .map_err(|reason| {
        SqlxError::Protocol(format!("document '{}': {}", document.document_id, reason))
    })?;
    Ok(())
}

/// Encoding-aware validation: declared dimension agreement, exact byte length
/// for the encoding, finiteness, and unit norm. Returns the decoded f32 values.
fn decoded_vector(
    encoding: VectorEncoding,
    model_dimensions: i64,
    dequant: Option<(f64, i64)>,
    declared_dimensions: i64,
    bytes: &[u8],
) -> Result<Vec<f32>, String> {
    if declared_dimensions != model_dimensions {
        return Err(format!(
            "declares {} dimensions but its model has {}",
            declared_dimensions, model_dimensions
        ));
    }
    let dimensions = model_dimensions as usize;
    let expected_len = dimensions
        .checked_mul(encoding.bytes_per_value())
        .ok_or_else(|| "dimension count overflows".to_string())?;
    if bytes.len() != expected_len {
        return Err(format!(
            "{} encoding with {} dimensions needs {} vector bytes but has {}",
            encoding.as_str(),
            dimensions,
            expected_len,
            bytes.len()
        ));
    }
    let mut values = Vec::with_capacity(dimensions);
    match encoding {
        VectorEncoding::F32 => {
            for chunk in bytes.chunks_exact(4) {
                values.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
            }
        }
        VectorEncoding::Fp16 => {
            for chunk in bytes.chunks_exact(2) {
                let bits = u16::from_le_bytes([chunk[0], chunk[1]]);
                values.push(fp16_bits_to_f32(bits));
            }
        }
        VectorEncoding::Int8 => {
            let (scale, zero_point) = dequant
                .ok_or_else(|| "int8 vector without dequantization parameters".to_string())?;
            for &byte in bytes {
                let quantized = byte as i8;
                values.push((scale * (quantized as f64 - zero_point as f64)) as f32);
            }
        }
    }
    if let Some(invalid) = values.iter().position(|value| !value.is_finite()) {
        return Err(format!("vector value {} is not finite", invalid));
    }
    let norm = values
        .iter()
        .map(|v| (*v as f64) * (*v as f64))
        .sum::<f64>()
        .sqrt();
    if (norm - 1.0).abs() > encoding.norm_tolerance() {
        return Err(format!(
            "vector norm {} is not unit within the '{}' tolerance",
            norm,
            encoding.as_str()
        ));
    }
    Ok(values)
}

/// Validates one page of canonical rows and appends them to the snapshot
/// page: admissible rows become documents, malformed rows become rejections
/// naming their document and meeting.
fn append_validated_snapshot_rows(
    model_dimensions: i64,
    scale: f64,
    zero_point: i64,
    rows: Vec<SnapshotRow>,
    page: &mut SnapshotPage,
) -> Result<(), SqlxError> {
    for (
        _,
        document_id,
        meeting_id,
        source_kind,
        start,
        end,
        template,
        heading,
        ordinal,
        dimensions,
        vector,
    ) in rows
    {
        if let Err(reason) =
            validate_int8_bytes(model_dimensions, dimensions, &vector, scale, zero_point)
        {
            page.rejected.push(RejectedDocument {
                document_id: document_id.clone(),
                meeting_id: meeting_id.clone(),
                reason,
            });
            continue;
        }
        page.documents.push(SnapshotDocument {
            document_id,
            meeting_id,
            source_kind,
            source_start_id: start,
            source_end_id: end,
            source_template_id: template,
            heading,
            ordinal,
        });
        page.vectors.extend_from_slice(&vector);
    }
    Ok(())
}

/// Memory-lean int8 validation for snapshot admission: dimension agreement,
/// exact byte length, and unit norm computed from integer sums (finiteness is
/// guaranteed by the persisted positive finite scale), so loading a 250k
/// base never materializes per-row float vectors.
fn validate_int8_bytes(
    model_dimensions: i64,
    declared_dimensions: i64,
    bytes: &[u8],
    scale: f64,
    zero_point: i64,
) -> Result<(), String> {
    if declared_dimensions != model_dimensions {
        return Err(format!(
            "declares {declared_dimensions} dimensions but its model has {model_dimensions}"
        ));
    }
    if bytes.len() != model_dimensions as usize {
        return Err(format!(
            "int8 vector needs {} bytes but has {}",
            model_dimensions,
            bytes.len()
        ));
    }
    let sum_sq: i64 = bytes
        .iter()
        .map(|byte| {
            let quantized = *byte as i8 as i64 - zero_point;
            quantized * quantized
        })
        .sum();
    let norm = (scale * scale * sum_sq as f64).sqrt();
    let tolerance = VectorEncoding::Int8.norm_tolerance() as f64;
    if (norm - 1.0).abs() > tolerance {
        return Err(format!(
            "vector norm {norm} is not unit within the 'int8' tolerance"
        ));
    }
    Ok(())
}

/// IEEE 754 half precision to f32, including subnormals, infinities, and NaN.
fn fp16_bits_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 0x1) as u32;
    let exponent = ((bits >> 10) & 0x1f) as u32;
    let fraction = (bits & 0x3ff) as u32;
    let word = match exponent {
        0 if fraction == 0 => sign << 31,
        0 => {
            // Subnormal: renormalize into an f32 exponent.
            let mut shifted = fraction;
            let mut adjust = 0u32;
            while shifted & 0x400 == 0 {
                shifted <<= 1;
                adjust += 1;
            }
            shifted &= 0x3ff;
            (sign << 31) | ((127 - 15 - adjust + 1) << 23) | (shifted << 13)
        }
        0x1f => (sign << 31) | (0xff << 23) | (fraction << 13),
        normal => (sign << 31) | ((normal as i32 - 15 + 127) as u32) << 23 | (fraction << 13),
    };
    f32::from_bits(word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::folder::FolderRepository;
    use crate::database::repositories::meeting::MeetingsRepository;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

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
            .bind("2026-08-25T00:00:00Z")
            .bind("2026-08-25T00:00:00Z")
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

    /// (source_revision, fts_projection_revision, fts_indexed_revision)
    async fn source_state(pool: &SqlitePool, meeting_id: &str) -> Option<(i64, i64, i64)> {
        sqlx::query_as(
            "SELECT source_revision, fts_projection_revision, fts_indexed_revision
             FROM search_source_state WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .unwrap()
    }

    async fn scalar_count(pool: &SqlitePool, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    fn f32_spec(model_id: &str, dimensions: u32) -> ModelSpec {
        ModelSpec {
            model_id: model_id.to_string(),
            dimensions,
            vector_encoding: VectorEncoding::F32,
            chunker_version: 1,
            dequantization_scale: None,
            dequantization_zero_point: None,
        }
    }

    fn doc(
        document_id: &str,
        dimensions: i64,
        encoding: VectorEncoding,
        vector: Vec<u8>,
    ) -> StagedDocument {
        StagedDocument {
            document_id: document_id.to_string(),
            source_kind: "transcript".to_string(),
            source_start_id: Some("t1".to_string()),
            source_end_id: Some("t1".to_string()),
            source_template_id: None,
            heading: None,
            ordinal: 0,
            content: "derived text".to_string(),
            content_hash: vec![1, 2, 3],
            dimensions,
            vector_encoding: encoding,
            vector,
        }
    }

    fn normalized_f32(values: &[f64]) -> Vec<u8> {
        let norm = values.iter().map(|v| v * v).sum::<f64>().sqrt();
        values
            .iter()
            .flat_map(|v| ((*v / norm) as f32).to_le_bytes())
            .collect()
    }

    #[tokio::test]
    async fn transcript_inserts_in_one_save_coalesce_to_one_queued_meeting() {
        let pool = migrated_pool().await;
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m', 'Coalesced', '2026-08-25T00:00:00Z', '2026-08-25T00:00:00Z')")
            .execute(&mut *tx)
            .await
            .unwrap();
        for ordinal in 0..3 {
            sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, 'm', ?, '10:00')")
                .bind(format!("t{ordinal}"))
                .bind(format!("segment {ordinal}"))
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();

        assert_eq!(
            scalar_count(&pool, "SELECT COUNT(*) FROM search_source_state").await,
            1
        );
        assert_eq!(source_state(&pool, "m").await, Some((4, 4, 0)));
    }

    #[tokio::test]
    async fn title_only_meeting_is_queued_for_its_profile() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Title Only").await;

        assert_eq!(source_state(&pool, "m").await, Some((1, 1, 0)));
        let source = RetrievalRepository::load_meeting_source(&pool, "m")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(source.title, "Title Only");
        assert_eq!(source.source_revision, Some(1));
        assert!(source.transcripts.is_empty());
        assert_eq!(source.latest_summary_markdown, None);
        assert_eq!(source.notes_markdown, None);
    }

    #[tokio::test]
    async fn content_triggers_advance_both_revisions() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Original").await;
        assert_eq!(source_state(&pool, "m").await, Some((1, 1, 0)));

        // Transcript insert / update / delete.
        add_transcript(&pool, "t1", "m", "first").await;
        assert_eq!(source_state(&pool, "m").await.map(|(s, _f, _i)| s), Some(2));
        sqlx::query("UPDATE transcripts SET transcript = 'second' WHERE id = 't1'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await.map(|(s, _f, _i)| s), Some(3));
        sqlx::query("DELETE FROM transcripts WHERE id = 't1'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await.map(|(s, _f, _i)| s), Some(4));

        // Summary-process insert, result update, and delete. A same-value
        // result write must not dirty anything.
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result)
             VALUES ('m', 'std', 'PENDING', '2026-08-25T00:00:00Z', '2026-08-25T00:00:00Z', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(source_state(&pool, "m").await.map(|(s, _f, _i)| s), Some(5));
        sqlx::query("UPDATE summary_processes SET result = '{\"markdown\":\"done\"}' WHERE meeting_id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await.map(|(s, _f, _i)| s), Some(6));
        sqlx::query("UPDATE summary_processes SET result = '{\"markdown\":\"done\"}', status = 'completed' WHERE meeting_id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await.map(|(s, _f, _i)| s), Some(6));
        sqlx::query("DELETE FROM summary_processes WHERE meeting_id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await.map(|(s, _f, _i)| s), Some(7));

        // Notes insert / update / delete.
        sqlx::query(
            "INSERT INTO meeting_notes (meeting_id, notes_markdown, created_at, updated_at)
             VALUES ('m', 'note one', '2026-08-25T00:00:00Z', '2026-08-25T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(source_state(&pool, "m").await.map(|(s, _f, _i)| s), Some(8));
        sqlx::query("UPDATE meeting_notes SET notes_markdown = 'note two' WHERE meeting_id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await.map(|(s, _f, _i)| s), Some(9));
        sqlx::query("UPDATE meeting_notes SET notes_markdown = 'note two' WHERE meeting_id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await.map(|(s, _f, _i)| s), Some(9));
        sqlx::query("DELETE FROM meeting_notes WHERE meeting_id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            source_state(&pool, "m").await.map(|(s, _f, _i)| s),
            Some(10)
        );

        // Meeting title update; an unchanged title write is not a change.
        sqlx::query("UPDATE meetings SET title = 'Renamed' WHERE id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            source_state(&pool, "m").await.map(|(s, _f, _i)| s),
            Some(11)
        );
        sqlx::query("UPDATE meetings SET title = 'Renamed', updated_at = '2026-08-25T01:00:00Z' WHERE id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await, Some((11, 11, 0)));
    }

    #[tokio::test]
    async fn folder_metadata_advances_only_the_fts_projection() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Filed").await;
        assert_eq!(source_state(&pool, "m").await, Some((1, 1, 0)));

        let folder = FolderRepository::create(&pool, "Work", None).await.unwrap();
        FolderRepository::set_meeting_folder(&pool, "m", Some(&folder.id))
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await, Some((1, 2, 0)));

        FolderRepository::rename(&pool, &folder.id, "Work Renamed")
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await, Some((1, 3, 0)));

        FolderRepository::rename(&pool, &folder.id, "Work Renamed")
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await, Some((1, 3, 0)));

        let parent = FolderRepository::create(&pool, "Root", None).await.unwrap();
        FolderRepository::move_folder(&pool, &folder.id, Some(&parent.id))
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await, Some((1, 3, 0)));

        FolderRepository::delete_with_cascade(&pool, &parent.id)
            .await
            .unwrap();
        assert_eq!(source_state(&pool, "m").await, Some((1, 4, 0)));
    }

    #[tokio::test]
    async fn active_and_shadow_generations_progress_independently() {
        let pool = migrated_pool().await;
        RetrievalRepository::register_model(&pool, &f32_spec("model-a", 2))
            .await
            .unwrap();
        insert_meeting(&pool, "m", "Shared").await;
        RetrievalRepository::register_generation(&pool, "gen-active", "model-a")
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-shadow", "model-a")
            .await
            .unwrap();

        let documents = [doc(
            "d0",
            2,
            VectorEncoding::F32,
            normalized_f32(&[0.6, -0.8]),
        )];
        RetrievalRepository::stage_documents(&pool, "job-a", "gen-active", "m", 1, &documents)
            .await
            .unwrap();
        let outcome = RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen-active",
                meeting_id: "m",
                expected_source_revision: 1,
                job_id: "job-a",
            },
        )
        .await
        .unwrap();
        assert!(matches!(outcome, ReplacementOutcome::Published { .. }));

        let active_status = RetrievalRepository::generation_status(&pool, "gen-active")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (active_status.current_meetings, active_status.document_count),
            (1, 1)
        );
        let shadow_status = RetrievalRepository::generation_status(&pool, "gen-shadow")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                shadow_status.current_meetings,
                shadow_status.retry_meetings,
                shadow_status.document_count
            ),
            (0, 0, 0)
        );

        RetrievalRepository::record_work_failure(
            &pool,
            "gen-shadow",
            "m",
            false,
            "embedding session unavailable",
            "2099-01-01T00:00:00+00:00",
        )
        .await
        .unwrap();
        let shadow_status = RetrievalRepository::generation_status(&pool, "gen-shadow")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (shadow_status.retry_meetings, shadow_status.current_meetings),
            (1, 0)
        );
        let active_status = RetrievalRepository::generation_status(&pool, "gen-active")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(active_status.state, "building");
        assert_eq!(active_status.current_meetings, 1);

        // Exactly one active generation is representable and only via the pointer.
        assert_eq!(
            RetrievalRepository::active_generation_id(&pool)
                .await
                .unwrap(),
            None
        );
        RetrievalRepository::set_generation_state(&pool, "gen-active", "ready")
            .await
            .unwrap();
        RetrievalRepository::switch_active_generation(&pool, "gen-active")
            .await
            .unwrap();
        assert_eq!(
            RetrievalRepository::active_generation_id(&pool)
                .await
                .unwrap(),
            Some("gen-active".to_string())
        );
        assert!(
            RetrievalRepository::switch_active_generation(&pool, "gen-shadow")
                .await
                .is_err()
        );
        assert!(
            RetrievalRepository::set_generation_state(&pool, "gen-active", "active")
                .await
                .is_err(),
            "'active' must not be a generation state"
        );
        let duplicate_pointer = sqlx::query(
            "INSERT INTO retrieval_active_model (singleton, generation_id, activated_at) VALUES (2, 'gen-active', 'now')",
        )
        .execute(&pool)
        .await;
        assert!(
            duplicate_pointer.is_err(),
            "singleton CHECK must forbid a second row"
        );
        let status = RetrievalRepository::generation_status(&pool, "gen-active")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.state, "ready");
    }

    #[tokio::test]
    async fn replacement_is_revision_fenced_and_preserves_prior_documents() {
        let pool = migrated_pool().await;
        RetrievalRepository::register_model(&pool, &f32_spec("model", 2))
            .await
            .unwrap();
        insert_meeting(&pool, "m", "Fenced").await;
        add_transcript(&pool, "t1", "m", "content").await; // revision 2
        RetrievalRepository::register_generation(&pool, "gen", "model")
            .await
            .unwrap();
        // Registration itself establishes the durable publication state.
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen")
                .await
                .unwrap(),
            Some((0, 0))
        );

        let old_docs = [doc(
            "old",
            2,
            VectorEncoding::F32,
            normalized_f32(&[1.0, 0.0]),
        )];
        RetrievalRepository::stage_documents(&pool, "job-old", "gen", "m", 2, &old_docs)
            .await
            .unwrap();
        let first = RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen",
                meeting_id: "m",
                expected_source_revision: 2,
                job_id: "job-old",
            },
        )
        .await
        .unwrap();
        let ReplacementOutcome::Published {
            change_id: first_change,
        } = first
        else {
            panic!("first replacement must publish");
        };

        // Extraction at revision 2 goes stale when the title changes to 3.
        let stale_docs = [doc(
            "new",
            2,
            VectorEncoding::F32,
            normalized_f32(&[0.0, 1.0]),
        )];
        RetrievalRepository::stage_documents(&pool, "job-stale", "gen", "m", 2, &stale_docs)
            .await
            .unwrap();
        sqlx::query("UPDATE meetings SET title = 'Edited' WHERE id = 'm'")
            .execute(&pool)
            .await
            .unwrap();
        let outcome = RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen",
                meeting_id: "m",
                expected_source_revision: 2,
                job_id: "job-stale",
            },
        )
        .await
        .unwrap();
        assert_eq!(
            outcome,
            ReplacementOutcome::RevisionConflict {
                current_revision: Some(3)
            }
        );
        assert_eq!(
            scalar_count(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            0,
            "conflicted staging is discarded"
        );
        let surviving = RetrievalRepository::read_validated_documents(&pool, "gen", "m")
            .await
            .unwrap();
        assert_eq!(
            surviving
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            vec!["old"],
            "prior documents survive a fenced-out replacement"
        );
        assert_eq!(
            scalar_count(
                &pool,
                "SELECT COUNT(*) FROM retrieval_index_changes WHERE operation = 'upsert'"
            )
            .await,
            1
        );

        let fresh_docs = [doc(
            "new",
            2,
            VectorEncoding::F32,
            normalized_f32(&[0.0, 1.0]),
        )];
        RetrievalRepository::stage_documents(&pool, "job-fresh", "gen", "m", 3, &fresh_docs)
            .await
            .unwrap();
        let second = RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen",
                meeting_id: "m",
                expected_source_revision: 3,
                job_id: "job-fresh",
            },
        )
        .await
        .unwrap();
        let ReplacementOutcome::Published {
            change_id: second_change,
        } = second
        else {
            panic!("fresh replacement must publish");
        };
        assert!(second_change > first_change);
        let surviving = RetrievalRepository::read_validated_documents(&pool, "gen", "m")
            .await
            .unwrap();
        assert_eq!(
            surviving
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            vec!["new"]
        );
        let work: (String, i64, i64) = sqlx::query_as(
            "SELECT state, attempt_count, indexed_source_revision FROM retrieval_meeting_state WHERE generation_id = 'gen' AND meeting_id = 'm'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(work, ("ready".to_string(), 0, 3));
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen")
                .await
                .unwrap(),
            Some((second_change, 0))
        );
        RetrievalRepository::acknowledge_journal(&pool, "gen", first_change)
            .await
            .unwrap();
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen")
                .await
                .unwrap(),
            Some((second_change, first_change))
        );
        RetrievalRepository::acknowledge_journal(&pool, "gen", second_change)
            .await
            .unwrap();
        assert!(
            RetrievalRepository::acknowledge_journal(&pool, "gen", second_change + 1)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn malformed_vectors_are_rejected_safely() {
        let pool = migrated_pool().await;
        RetrievalRepository::register_model(&pool, &f32_spec("model", 4))
            .await
            .unwrap();
        insert_meeting(&pool, "m", "Vectors").await;
        RetrievalRepository::register_generation(&pool, "gen", "model")
            .await
            .unwrap();

        let nan_vector = f32::NAN.to_le_bytes().repeat(2);
        let inf_vector = f32::INFINITY.to_le_bytes().repeat(2);
        let cases: Vec<StagedDocument> = vec![
            doc("short", 4, VectorEncoding::F32, vec![0u8; 12]),
            doc("nan", 4, VectorEncoding::F32, nan_vector),
            doc("inf", 4, VectorEncoding::F32, inf_vector),
            doc(
                "zero-norm",
                4,
                VectorEncoding::F32,
                vec![0f32.to_le_bytes(), 0f32.to_le_bytes()].concat(),
            ),
            doc(
                "dimension-mismatch",
                8,
                VectorEncoding::F32,
                normalized_f32(&[1.0, 0.0, 0.0, 0.0]),
            ),
            doc("encoding-mismatch", 2, VectorEncoding::Int8, vec![100, 50]),
        ];
        for case in &cases {
            assert!(
                RetrievalRepository::stage_documents(
                    &pool,
                    "job-bad",
                    "gen",
                    "m",
                    1,
                    std::slice::from_ref(case)
                )
                .await
                .is_err(),
                "vector {:?} must be rejected",
                case.document_id
            );
        }
        assert_eq!(
            scalar_count(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            0
        );

        // Read-side boundary: a row corrupted after publication is reported,
        // not admitted or panicked on.
        let good = [doc(
            "good",
            4,
            VectorEncoding::F32,
            normalized_f32(&[0.7071, 0.7071, 0.0, 0.0]),
        )];
        RetrievalRepository::stage_documents(&pool, "job-good", "gen", "m", 1, &good)
            .await
            .unwrap();
        RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen",
                meeting_id: "m",
                expected_source_revision: 1,
                job_id: "job-good",
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE retrieval_documents SET vector = x'00000000000000000000000000000000'")
            .execute(&pool)
            .await
            .unwrap();
        let error = RetrievalRepository::read_validated_documents(&pool, "gen", "m")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("good"),
            "read-side rejection names the document"
        );
    }

    #[tokio::test]
    async fn non_f32_encodings_round_trip() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Quantized").await;

        // int8 with persisted dequantization parameters (approved production encoding).
        let int8_spec = ModelSpec {
            model_id: "int8-model".to_string(),
            dimensions: 2,
            vector_encoding: VectorEncoding::Int8,
            chunker_version: 1,
            dequantization_scale: Some(0.01),
            dequantization_zero_point: Some(0),
        };
        RetrievalRepository::register_model(&pool, &int8_spec)
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-int8", "int8-model")
            .await
            .unwrap();
        // 0.6 and -0.8 are exact multiples of 0.01 and [0.6, -0.8] is unit.
        let quantized = [(60i8) as u8, (-80i8) as u8].to_vec();
        let docs = [doc("q0", 2, VectorEncoding::Int8, quantized)];
        RetrievalRepository::stage_documents(&pool, "job-q", "gen-int8", "m", 1, &docs)
            .await
            .unwrap();
        RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen-int8",
                meeting_id: "m",
                expected_source_revision: 1,
                job_id: "job-q",
            },
        )
        .await
        .unwrap();
        let decoded = RetrievalRepository::read_validated_documents(&pool, "gen-int8", "m")
            .await
            .unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].0, "q0");
        assert!((decoded[0].1[0] - 0.6).abs() < 1e-6);
        assert!((decoded[0].1[1] + 0.8).abs() < 1e-6);

        // An int8 model without persisted dequantization parameters is
        // unusable, and a float model must not carry them.
        let broken_int8 = ModelSpec {
            model_id: "broken-int8".to_string(),
            dimensions: 2,
            vector_encoding: VectorEncoding::Int8,
            chunker_version: 1,
            dequantization_scale: None,
            dequantization_zero_point: None,
        };
        assert!(RetrievalRepository::register_model(&pool, &broken_int8)
            .await
            .is_err());
        assert!(
            RetrievalRepository::register_model(&pool, &float_spec_with_scale())
                .await
                .is_err()
        );
        // fp16 round trip through the same boundary.
        let fp16_spec = ModelSpec {
            model_id: "fp16-model".to_string(),
            dimensions: 2,
            vector_encoding: VectorEncoding::Fp16,
            chunker_version: 1,
            dequantization_scale: None,
            dequantization_zero_point: None,
        };
        RetrievalRepository::register_model(&pool, &fp16_spec)
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-fp16", "fp16-model")
            .await
            .unwrap();
        let half = [doc(
            "h0",
            2,
            VectorEncoding::Fp16,
            0x0000u16
                .to_le_bytes()
                .iter()
                .copied()
                .chain(0xBC00u16.to_le_bytes().iter().copied())
                .collect(),
        )];
        RetrievalRepository::stage_documents(&pool, "job-h", "gen-fp16", "m", 1, &half)
            .await
            .unwrap();
        RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen-fp16",
                meeting_id: "m",
                expected_source_revision: 1,
                job_id: "job-h",
            },
        )
        .await
        .unwrap();
        let decoded = RetrievalRepository::read_validated_documents(&pool, "gen-fp16", "m")
            .await
            .unwrap();
        assert_eq!(decoded[0].1, vec![0.0, -1.0]);
    }

    fn float_spec_with_scale() -> ModelSpec {
        // A float encoding carrying dequantization parameters is rejected.
        ModelSpec {
            model_id: "float-with-scale".to_string(),
            dimensions: 2,
            vector_encoding: VectorEncoding::F32,
            chunker_version: 1,
            dequantization_scale: Some(0.5),
            dequantization_zero_point: None,
        }
    }

    #[tokio::test]
    async fn poison_item_is_bypassed_until_due_again() {
        let pool = migrated_pool().await;
        RetrievalRepository::register_model(&pool, &f32_spec("model", 2))
            .await
            .unwrap();
        insert_meeting(&pool, "poison", "Poison").await;
        insert_meeting(&pool, "healthy", "Healthy").await;
        RetrievalRepository::register_generation(&pool, "gen", "model")
            .await
            .unwrap();

        RetrievalRepository::record_work_failure(
            &pool,
            "gen",
            "poison",
            false,
            "tokenizer exploded",
            "2099-01-01T00:00:00+00:00",
        )
        .await
        .unwrap();

        let due = RetrievalRepository::list_due_generation_work(
            &pool,
            "gen",
            "2026-08-25T00:00:00+00:00",
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            due.iter()
                .map(|item| item.meeting_id.as_str())
                .collect::<Vec<_>>(),
            vec!["healthy"]
        );

        sqlx::query("UPDATE retrieval_meeting_state SET next_attempt_at = '2026-01-01T00:00:00+00:00' WHERE meeting_id = 'poison'")
            .execute(&pool)
            .await
            .unwrap();
        let due = RetrievalRepository::list_due_generation_work(
            &pool,
            "gen",
            "2026-08-25T00:00:00+00:00",
            10,
        )
        .await
        .unwrap();
        assert!(due.iter().any(|item| item.meeting_id == "poison"));
        let item = due.iter().find(|item| item.meeting_id == "poison").unwrap();
        assert_eq!((item.state.as_str(), item.attempt_count), ("retry", 1));

        // A permanently failed poison item never re-enters the queue.
        RetrievalRepository::record_work_failure(
            &pool,
            "gen",
            "poison",
            true,
            "tokenizer exploded",
            "2099-01-01T00:00:00+00:00",
        )
        .await
        .unwrap();
        let due = RetrievalRepository::list_due_generation_work(
            &pool,
            "gen",
            "2099-12-31T00:00:00+00:00",
            10,
        )
        .await
        .unwrap();
        assert!(!due.iter().any(|item| item.meeting_id == "poison"));

        // FTS retry scheduling skips not-yet-due meetings the same way.
        RetrievalRepository::record_fts_failure(
            &pool,
            "healthy",
            "fts locked",
            "2099-01-01T00:00:00+00:00",
        )
        .await
        .unwrap();
        assert!(
            RetrievalRepository::list_due_fts_repairs(&pool, "2026-08-25T00:00:00+00:00", 10)
                .await
                .unwrap()
                .iter()
                .all(|item| item.meeting_id != "healthy")
        );
        assert!(
            RetrievalRepository::mark_fts_indexed(&pool, "poison", 1)
                .await
                .unwrap(),
            "marking with the selected projection revision must succeed"
        );
        // Once the deferred attempt comes due, the previously poisoned item
        // re-enters the queue instead of being lost.
        let due_later =
            RetrievalRepository::list_due_fts_repairs(&pool, "2099-06-01T00:00:00+00:00", 10)
                .await
                .unwrap();
        assert!(due_later.iter().any(|item| item.meeting_id == "healthy"));
    }

    #[tokio::test]
    async fn meeting_delete_cascades_derived_rows_but_preserves_tombstones() {
        let pool = migrated_pool().await;
        RetrievalRepository::register_model(&pool, &f32_spec("model", 2))
            .await
            .unwrap();
        insert_meeting(&pool, "doomed", "Doomed").await;
        insert_meeting(&pool, "survivor", "Survivor").await;
        RetrievalRepository::register_generation(&pool, "gen", "model")
            .await
            .unwrap();
        // Registration itself establishes the durable publication state.
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen")
                .await
                .unwrap(),
            Some((0, 0))
        );

        let doomed_docs = [doc(
            "dd",
            2,
            VectorEncoding::F32,
            normalized_f32(&[1.0, 0.0]),
        )];
        RetrievalRepository::stage_documents(&pool, "job-doomed", "gen", "doomed", 1, &doomed_docs)
            .await
            .unwrap();
        let outcome = RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen",
                meeting_id: "doomed",
                expected_source_revision: 1,
                job_id: "job-doomed",
            },
        )
        .await
        .unwrap();
        let ReplacementOutcome::Published {
            change_id: published,
        } = outcome
        else {
            panic!("baseline publication must succeed");
        };
        RetrievalRepository::acknowledge_journal(&pool, "gen", published)
            .await
            .unwrap();

        // Partially staged jobs exist for both meetings before deletion.
        let partial_doomed = [doc(
            "pd",
            2,
            VectorEncoding::F32,
            normalized_f32(&[0.0, 1.0]),
        )];
        RetrievalRepository::stage_documents(
            &pool,
            "partial-doomed",
            "gen",
            "doomed",
            1,
            &partial_doomed,
        )
        .await
        .unwrap();
        let partial_survivor = [doc(
            "ps",
            2,
            VectorEncoding::F32,
            normalized_f32(&[0.7071, 0.7071]),
        )];
        RetrievalRepository::stage_documents(
            &pool,
            "partial-survivor",
            "gen",
            "survivor",
            1,
            &partial_survivor,
        )
        .await
        .unwrap();

        assert!(MeetingsRepository::delete_meeting(&pool, "doomed")
            .await
            .unwrap());

        // Derived rows cascade away with the meeting...
        assert_eq!(source_state(&pool, "doomed").await, None);
        assert_eq!(
            scalar_count(
                &pool,
                "SELECT COUNT(*) FROM retrieval_documents WHERE meeting_id = 'doomed'"
            )
            .await,
            0
        );
        assert_eq!(
            scalar_count(
                &pool,
                "SELECT COUNT(*) FROM retrieval_meeting_state WHERE meeting_id = 'doomed'"
            )
            .await,
            0
        );
        // ...including its partially staged job, so it cannot resume later.
        assert_eq!(
            scalar_count(
                &pool,
                "SELECT COUNT(*) FROM retrieval_document_staging WHERE meeting_id = 'doomed'"
            )
            .await,
            0
        );
        assert_eq!(
            scalar_count(
                &pool,
                "SELECT COUNT(*) FROM retrieval_document_staging WHERE meeting_id = 'survivor'"
            )
            .await,
            1
        );

        // The tombstone survives outside foreign keys, and canonical is now
        // observably ahead of published.
        let tombstone: (i64, String) = sqlx::query_as(
            "SELECT change_id, operation FROM retrieval_index_changes WHERE meeting_id = 'doomed' AND operation = 'delete'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(tombstone.1, "delete");
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen")
                .await
                .unwrap(),
            Some((tombstone.0, published)),
            "canonical must be ahead of published immediately after commit"
        );
        RetrievalRepository::acknowledge_journal(&pool, "gen", tombstone.0)
            .await
            .unwrap();
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen")
                .await
                .unwrap(),
            Some((tombstone.0, tombstone.0))
        );
    }

    #[tokio::test]
    async fn meetings_created_after_registration_are_queued_for_every_live_generation() {
        let pool = migrated_pool().await;
        RetrievalRepository::register_model(&pool, &f32_spec("model", 2))
            .await
            .unwrap();
        insert_meeting(&pool, "early", "Early").await;
        RetrievalRepository::register_generation(&pool, "gen-active", "model")
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-shadow", "model")
            .await
            .unwrap();

        // Title-only meeting inserted after registration.
        insert_meeting(&pool, "late", "Late").await;
        assert_eq!(source_state(&pool, "late").await, Some((1, 1, 0)));

        let now = "2026-08-25T00:00:00+00:00";
        for generation in ["gen-active", "gen-shadow"] {
            let due = RetrievalRepository::list_due_generation_work(&pool, generation, now, 10)
                .await
                .unwrap();
            let item = due
                .iter()
                .find(|item| item.meeting_id == "late")
                .unwrap_or_else(|| panic!("generation {generation} must owe work for 'late'"));
            assert_eq!(
                (
                    item.state.as_str(),
                    item.indexed_source_revision,
                    item.source_revision
                ),
                ("pending", 0, 1)
            );
        }
        // Publishing in one generation clears only that generation's work.
        let docs = [doc(
            "ld",
            2,
            VectorEncoding::F32,
            normalized_f32(&[0.6, -0.8]),
        )];
        RetrievalRepository::stage_documents(&pool, "job-late", "gen-active", "late", 1, &docs)
            .await
            .unwrap();
        RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen-active",
                meeting_id: "late",
                expected_source_revision: 1,
                job_id: "job-late",
            },
        )
        .await
        .unwrap();
        let active_due =
            RetrievalRepository::list_due_generation_work(&pool, "gen-active", now, 10)
                .await
                .unwrap();
        assert!(!active_due.iter().any(|item| item.meeting_id == "late"));
        let shadow_due =
            RetrievalRepository::list_due_generation_work(&pool, "gen-shadow", now, 10)
                .await
                .unwrap();
        assert!(shadow_due.iter().any(|item| item.meeting_id == "late"));

        // An activated generation keeps receiving newly created meetings too.
        RetrievalRepository::set_generation_state(&pool, "gen-active", "ready")
            .await
            .unwrap();
        RetrievalRepository::switch_active_generation(&pool, "gen-active")
            .await
            .unwrap();
        insert_meeting(&pool, "post-switch", "After").await;
        for generation in ["gen-active", "gen-shadow"] {
            let due = RetrievalRepository::list_due_generation_work(&pool, generation, now, 10)
                .await
                .unwrap();
            assert!(
                due.iter().any(|item| item.meeting_id == "post-switch"),
                "generation {generation} must owe work for a post-switch meeting"
            );
        }
    }

    #[tokio::test]
    async fn retired_generation_tombstones_block_cleanup_until_acknowledged() {
        let pool = migrated_pool().await;
        RetrievalRepository::register_model(&pool, &f32_spec("model", 2))
            .await
            .unwrap();
        insert_meeting(&pool, "m", "Retired Gen").await;
        RetrievalRepository::register_generation(&pool, "gen", "model")
            .await
            .unwrap();

        let docs = [doc(
            "d",
            2,
            VectorEncoding::F32,
            normalized_f32(&[1.0, 0.0]),
        )];
        RetrievalRepository::stage_documents(&pool, "job", "gen", "m", 1, &docs)
            .await
            .unwrap();
        let outcome = RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen",
                meeting_id: "m",
                expected_source_revision: 1,
                job_id: "job",
            },
        )
        .await
        .unwrap();
        let ReplacementOutcome::Published {
            change_id: published,
        } = outcome
        else {
            panic!("baseline publication must succeed");
        };
        RetrievalRepository::acknowledge_journal(&pool, "gen", published)
            .await
            .unwrap();

        // Retire the built generation; it still retains derived state and its
        // publisher may still be draining, so it keeps earning tombstones.
        RetrievalRepository::set_generation_state(&pool, "gen", "retired")
            .await
            .unwrap();
        assert!(MeetingsRepository::delete_meeting(&pool, "m")
            .await
            .unwrap());
        let tombstone: (i64,) = sqlx::query_as(
            "SELECT change_id FROM retrieval_index_changes WHERE meeting_id = 'm' AND operation = 'delete'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            RetrievalRepository::publication_lag(&pool, "gen")
                .await
                .unwrap(),
            Some((tombstone.0, published)),
            "a retired generation must observe canonical ahead of published"
        );

        // Cleanup stays blocked until the publisher acknowledges the delete...
        assert!(RetrievalRepository::delete_generation(&pool, "gen")
            .await
            .is_err());
        RetrievalRepository::acknowledge_journal(&pool, "gen", tombstone.0)
            .await
            .unwrap();
        // ...then the acknowledged generation can be garbage collected.
        assert!(RetrievalRepository::delete_generation(&pool, "gen")
            .await
            .unwrap());
        assert_eq!(
            scalar_count(
                &pool,
                "SELECT COUNT(*) FROM retrieval_index_changes WHERE generation_id = 'gen'"
            )
            .await,
            0,
            "acknowledged journal rows are pruned with their generation"
        );
    }

    #[tokio::test]
    async fn due_work_selection_uses_indexes_not_full_scans() {
        let pool = migrated_pool().await;
        let explain = |sql: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query_as::<_, (i64, i64, i64, String)>(sql)
                    .fetch_all(&pool)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|(_, _, _, detail)| detail)
                    .collect::<Vec<_>>()
            }
        };

        let plan = explain(
            "EXPLAIN QUERY PLAN
             SELECT meeting_id, source_revision, fts_projection_revision, fts_indexed_revision
             FROM search_source_state
             WHERE fts_indexed_revision < fts_projection_revision
               AND (fts_next_attempt_at IS NULL OR fts_next_attempt_at <= '2026-01-01')
             ORDER BY fts_next_attempt_at LIMIT 50",
        )
        .await;
        assert!(
            plan.iter()
                .any(|detail| detail.contains("search_source_state_fts_due")),
            "FTS repair selection must use its due-work index: {plan:?}"
        );

        let plan = explain(
            "EXPLAIN QUERY PLAN
             SELECT ms.meeting_id
             FROM retrieval_meeting_state ms
             JOIN search_source_state s ON s.meeting_id = ms.meeting_id
             WHERE ms.generation_id = 'g'
               AND ms.state IN ('pending', 'retry')
               AND ms.indexed_source_revision < s.source_revision
               AND (ms.next_attempt_at IS NULL OR ms.next_attempt_at <= '2026-01-01')
             ORDER BY ms.state, ms.next_attempt_at LIMIT 10",
        )
        .await;
        assert!(
            plan.iter()
                .any(|detail| detail.contains("retrieval_meeting_state_due")),
            "semantic work selection must use its due-work index: {plan:?}"
        );

        let plan = explain(
            "EXPLAIN QUERY PLAN
             SELECT document_id, payload FROM retrieval_document_staging
             WHERE generation_id = 'g' AND meeting_id = 'm'",
        )
        .await;
        assert!(
            plan.iter()
                .any(|detail| detail.contains("retrieval_document_staging_by_generation")),
            "staging resume must use the per-generation index: {plan:?}"
        );

        let plan = explain(
            "EXPLAIN QUERY PLAN
             SELECT change_id, meeting_id, operation, source_revision, created_at
             FROM retrieval_index_changes
             WHERE generation_id = 'g' AND change_id > 0
             ORDER BY change_id LIMIT 100",
        )
        .await;
        assert!(
            plan.iter()
                .any(|detail| detail.contains("retrieval_index_changes_replay")),
            "journal replay must use the replay index: {plan:?}"
        );
    }

    #[tokio::test]
    async fn generation_cleanup_cascades_and_respects_guards() {
        let pool = migrated_pool().await;
        RetrievalRepository::register_model(&pool, &f32_spec("model", 2))
            .await
            .unwrap();
        insert_meeting(&pool, "m", "Cleanup").await;
        RetrievalRepository::register_generation(&pool, "gen-active", "model")
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-shadow", "model")
            .await
            .unwrap();

        let docs = [doc(
            "d",
            2,
            VectorEncoding::F32,
            normalized_f32(&[1.0, 0.0]),
        )];
        RetrievalRepository::stage_documents(&pool, "job-a", "gen-active", "m", 1, &docs)
            .await
            .unwrap();
        RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen-active",
                meeting_id: "m",
                expected_source_revision: 1,
                job_id: "job-a",
            },
        )
        .await
        .unwrap();
        RetrievalRepository::set_generation_state(&pool, "gen-active", "ready")
            .await
            .unwrap();
        RetrievalRepository::switch_active_generation(&pool, "gen-active")
            .await
            .unwrap();

        // Shadow staging cascades with the generation.
        RetrievalRepository::stage_documents(&pool, "job-s", "gen-shadow", "m", 1, &docs)
            .await
            .unwrap();
        assert!(RetrievalRepository::delete_generation(&pool, "gen-shadow")
            .await
            .unwrap());
        assert_eq!(
            scalar_count(&pool, "SELECT COUNT(*) FROM retrieval_document_staging").await,
            0
        );
        assert!(RetrievalRepository::generation_status(&pool, "gen-shadow")
            .await
            .unwrap()
            .is_none());

        // The active generation is protected.
        assert!(RetrievalRepository::delete_generation(&pool, "gen-active")
            .await
            .is_err());

        // Unacknowledged journal changes block cleanup.
        let docs2 = [doc(
            "d2",
            2,
            VectorEncoding::F32,
            normalized_f32(&[0.0, 1.0]),
        )];
        RetrievalRepository::stage_documents(&pool, "job-a2", "gen-active", "m", 1, &docs2)
            .await
            .unwrap();
        RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen-active",
                meeting_id: "m",
                expected_source_revision: 1,
                job_id: "job-a2",
            },
        )
        .await
        .unwrap();
        assert!(RetrievalRepository::delete_generation(&pool, "gen-active")
            .await
            .is_err());

        // Rebuild foundation: registering a new generation for the same model
        // seeds fresh work independent of the retired one.
        let retired = RetrievalRepository::active_generation_id(&pool)
            .await
            .unwrap()
            .unwrap();
        RetrievalRepository::set_generation_state(&pool, &retired, "retired")
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-rebuild", "model")
            .await
            .unwrap();
        let rebuild = RetrievalRepository::generation_status(&pool, "gen-rebuild")
            .await
            .unwrap()
            .unwrap();
        assert_eq!((rebuild.tracked_meetings, rebuild.document_count), (1, 0));
    }

    #[test]
    fn fp16_decoding_handles_special_values() {
        assert_eq!(fp16_bits_to_f32(0x3800), 0.5);
        assert_eq!(fp16_bits_to_f32(0xB800), -0.5);
        assert_eq!(fp16_bits_to_f32(0x0000), 0.0);
        assert_eq!(fp16_bits_to_f32(0x8000), -0.0);
        assert!(fp16_bits_to_f32(0x7C00).is_infinite());
        assert!(fp16_bits_to_f32(0xFC00).is_infinite() && fp16_bits_to_f32(0xFC00) < 0.0);
        assert!(fp16_bits_to_f32(0x7E00).is_nan());
        assert_eq!(fp16_bits_to_f32(0x0001), 2.0f32.powi(-24));
        assert_eq!(fp16_bits_to_f32(0x03FF), 1023.0 * 2.0f32.powi(-24));
        assert_eq!(fp16_bits_to_f32(0x4000), 2.0);
    }

    #[test]
    fn decoded_vector_rejects_bad_inputs() {
        let unit = normalized_f32(&[3.0, 4.0]);
        assert!(decoded_vector(VectorEncoding::F32, 2, None, 2, &unit).is_ok());
        assert!(decoded_vector(VectorEncoding::F32, 2, None, 3, &unit).is_err());
        assert!(decoded_vector(VectorEncoding::F32, 2, None, 2, &unit[..4]).is_err());
        assert!(decoded_vector(VectorEncoding::Int8, 2, None, 2, &[60, 200]).is_err());
        // Non-unit vectors are refused at the boundary.
        assert!(decoded_vector(
            VectorEncoding::F32,
            2,
            None,
            2,
            &[3.0f32.to_le_bytes(), 4.0f32.to_le_bytes()].concat()
        )
        .is_err());
    }

    fn int8_unit_row(values: [i8; 2], _scale: f64) -> Vec<u8> {
        // A quantized pair whose decoded norm sits within the 5% int8
        // tolerance: sqrt(96^2 + 84^2)/127 ~= 1.0055.
        let _ = values;
        vec![96u8, 84u8]
    }

    #[test]
    fn int8_snapshot_validation_rejects_bad_rows_without_float_materialization() {
        let scale = crate::retrieval::worker::APPROVED_INT8_DEQUANTIZATION_SCALE;
        let row = int8_unit_row([3, 4], scale);
        assert!(validate_int8_bytes(2, 2, &row, scale, 0).is_ok());
        assert!(validate_int8_bytes(2, 3, &row, scale, 0).is_err());
        assert!(validate_int8_bytes(2, 2, &row[..1], scale, 0).is_err());
        // A non-unit payload fails the norm gate.
        assert!(validate_int8_bytes(2, 2, &[10, 10], scale, 0).is_err());
    }

    #[tokio::test]
    async fn snapshot_reads_return_validated_raw_bytes_and_requeue_resets_work() {
        let pool = migrated_pool().await;
        // No transcript rows: the meeting's source revision stays at 1 so the
        // fenced replacement below publishes.
        insert_meeting(&pool, "m", "Snapshot").await;
        let spec = ModelSpec {
            model_id: "int8-model".to_string(),
            dimensions: 2,
            vector_encoding: VectorEncoding::Int8,
            chunker_version: 1,
            dequantization_scale: Some(1.0 / 127.0),
            dequantization_zero_point: Some(0),
        };
        RetrievalRepository::register_model(&pool, &spec)
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-snap", "int8-model")
            .await
            .unwrap();

        let staged = StagedDocument {
            document_id: "doc-1".to_string(),
            source_kind: "transcript".to_string(),
            source_start_id: Some("t1".to_string()),
            source_end_id: Some("t1".to_string()),
            source_template_id: None,
            heading: None,
            ordinal: 0,
            content: "conteudo".to_string(),
            content_hash: vec![9; 32],
            dimensions: 2,
            vector_encoding: VectorEncoding::Int8,
            vector: int8_unit_row([3, 4], 1.0 / 127.0),
        };
        RetrievalRepository::stage_documents(&pool, "job-1", "gen-snap", "m", 1, &[staged])
            .await
            .unwrap();
        RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen-snap",
                meeting_id: "m",
                expected_source_revision: 1,
                job_id: "job-1",
            },
        )
        .await
        .unwrap();

        let snapshot = RetrievalRepository::read_snapshot_documents(&pool, "gen-snap", None)
            .await
            .unwrap();
        assert_eq!(snapshot.documents.len(), 1);
        assert_eq!(snapshot.documents[0].document_id, "doc-1");
        assert_eq!(snapshot.documents[0].meeting_id, "m");
        assert_eq!(
            RetrievalRepository::read_snapshot_documents(&pool, "gen-snap", Some("other"))
                .await
                .unwrap()
                .documents
                .len(),
            0
        );

        // Quarantine path: requeue resets owed work so the worker rebuilds it.
        RetrievalRepository::requeue_meeting_work(&pool, "gen-snap", "m", "quarantined")
            .await
            .unwrap();
        let (state, indexed): (String, i64) = sqlx::query_as(
            "SELECT state, indexed_source_revision FROM retrieval_meeting_state
             WHERE generation_id = 'gen-snap' AND meeting_id = 'm'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((state.as_str(), indexed), ("pending", 0));

        // Corrupt derived bytes are rejected by name, never admitted.
        sqlx::query("UPDATE retrieval_documents SET vector = x'0007' WHERE document_id = 'doc-1'")
            .execute(&pool)
            .await
            .unwrap();
        let page = RetrievalRepository::read_snapshot_documents(&pool, "gen-snap", None)
            .await
            .unwrap();
        assert_eq!(page.documents.len(), 0);
        assert_eq!(page.rejected.len(), 1);
        assert_eq!(page.rejected[0].document_id, "doc-1");
        assert_eq!(page.rejected[0].meeting_id, "m");
    }

    #[tokio::test]
    async fn deactivate_clears_pointer_and_marks_generation_failed() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Deactivate").await;
        RetrievalRepository::register_model(&pool, &f32_spec("model", 2))
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-a", "model")
            .await
            .unwrap();
        RetrievalRepository::set_generation_state(&pool, "gen-a", "ready")
            .await
            .unwrap();
        RetrievalRepository::switch_active_generation(&pool, "gen-a")
            .await
            .unwrap();

        assert_eq!(
            RetrievalRepository::deactivate_active_generation(&pool)
                .await
                .unwrap(),
            Some("gen-a".to_string())
        );
        assert!(RetrievalRepository::active_generation_id(&pool)
            .await
            .unwrap()
            .is_none());
        let state: (String,) =
            sqlx::query_as("SELECT state FROM retrieval_generations WHERE generation_id = 'gen-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state.0, "failed");
        assert!(RetrievalRepository::list_live_generations(&pool)
            .await
            .unwrap()
            .is_empty());

        // Idempotent when nothing is active.
        assert!(RetrievalRepository::deactivate_active_generation(&pool)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn retention_count_and_disk_accounting_cover_derived_tables() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Disk").await;
        RetrievalRepository::register_model(&pool, &f32_spec("model", 2))
            .await
            .unwrap();
        assert_eq!(
            RetrievalRepository::count_retained_generations(&pool)
                .await
                .unwrap(),
            0
        );
        RetrievalRepository::register_generation(&pool, "gen-disk", "model")
            .await
            .unwrap();
        assert_eq!(
            RetrievalRepository::derived_disk_bytes(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            RetrievalRepository::estimated_shadow_snapshot_bytes(&pool)
                .await
                .unwrap(),
            0
        );

        let docs = [doc(
            "d",
            2,
            VectorEncoding::F32,
            normalized_f32(&[1.0, 0.0]),
        )];
        RetrievalRepository::stage_documents(&pool, "job", "gen-disk", "m", 1, &docs)
            .await
            .unwrap();
        let with_staging = RetrievalRepository::derived_disk_bytes(&pool)
            .await
            .unwrap();
        assert!(with_staging > 0);
        RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen-disk",
                meeting_id: "m",
                expected_source_revision: 1,
                job_id: "job",
            },
        )
        .await
        .unwrap();
        // Publishing consumes the staging job but the canonical document
        // (content + vector + hash) still counts against the envelope.
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM retrieval_document_staging")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert!(
            RetrievalRepository::derived_disk_bytes(&pool)
                .await
                .unwrap()
                > 0,
            "published documents count against the envelope"
        );
        assert_eq!(
            RetrievalRepository::count_retained_generations(&pool)
                .await
                .unwrap(),
            1,
            "building generations are retained"
        );

        // Retired generations remain counted until garbage collection.
        RetrievalRepository::set_generation_state(&pool, "gen-disk", "retired")
            .await
            .unwrap();
        assert_eq!(
            RetrievalRepository::count_retained_generations(&pool)
                .await
                .unwrap(),
            1
        );
        let retired = RetrievalRepository::retired_generations(&pool)
            .await
            .unwrap();
        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0].0, "gen-disk");
    }

    #[tokio::test]
    async fn fts_mark_is_fenced_against_the_selected_projection_revision() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Fenced FTS").await;

        let now = "2026-08-26T00:00:00+00:00";
        let selected = RetrievalRepository::list_due_fts_repairs(&pool, now, 10)
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.meeting_id == "m")
            .expect("fresh meeting is due for repair");

        // A source/folder mutation lands between selection and marking: the
        // projection advances past what the refresh actually contained.
        let folder = FolderRepository::create(&pool, "Work", None).await.unwrap();
        FolderRepository::set_meeting_folder(&pool, "m", Some(&folder.id))
            .await
            .unwrap();
        let (_, projection_after_mutation, _) = source_state(&pool, "m").await.unwrap();
        assert!(projection_after_mutation > selected.fts_projection_revision);

        // The stale mark is a typed no-op: nothing copied, meeting stays due.
        assert!(!RetrievalRepository::mark_fts_indexed(
            &pool,
            "m",
            selected.fts_projection_revision
        )
        .await
        .unwrap());
        let (_, _, indexed) = source_state(&pool, "m").await.unwrap();
        assert_eq!(indexed, 0);

        // A fresh selection carries the newer revision and marks cleanly.
        let refreshed = RetrievalRepository::list_due_fts_repairs(&pool, now, 10)
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.meeting_id == "m")
            .expect("superseded repair must remain due");
        assert_eq!(refreshed.fts_projection_revision, projection_after_mutation);
        assert!(RetrievalRepository::mark_fts_indexed(
            &pool,
            "m",
            refreshed.fts_projection_revision
        )
        .await
        .unwrap());
        assert!(RetrievalRepository::list_due_fts_repairs(&pool, now, 10)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn malformed_summary_json_does_not_fail_source_extraction() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Summary JSON").await;
        add_transcript(&pool, "t1", "m", "transcript survives broken summaries").await;
        // Newest-first: an unreadable result sits above a valid one.
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result)
             VALUES ('m', 'new', 'completed', '2026-08-26T00:00:00Z', '2026-08-26T02:00:00Z', '{definitely not json'),
                    ('m', 'old', 'completed', '2026-08-26T00:00:00Z', '2026-08-26T01:00:00Z', '{\"markdown\":\"resumo valido\"}')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let source = RetrievalRepository::load_meeting_source(&pool, "m")
            .await
            .unwrap()
            .expect("extraction must succeed despite the malformed newest summary");
        assert_eq!(
            source.latest_summary_template_id.as_deref(),
            Some("old"),
            "the newest readable summary wins"
        );
        assert_eq!(
            source.latest_summary_markdown.as_deref(),
            Some("resumo valido")
        );
        assert_eq!(source.transcripts.len(), 1, "non-summary sources are kept");

        // A meeting whose every summary is unreadable still extracts, without
        // any summary content.
        insert_meeting(&pool, "m2", "All Broken").await;
        sqlx::query(
            "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result)
             VALUES ('m2', 'std', 'completed', '2026-08-26T00:00:00Z', '2026-08-26T01:00:00Z', 'not json at all')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let source = RetrievalRepository::load_meeting_source(&pool, "m2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(source.latest_summary_markdown, None);
        assert_eq!(source.latest_summary_template_id, None);
        assert_eq!(source.title, "All Broken");
    }

    #[tokio::test]
    async fn heading_provenance_persists_through_staging_and_snapshot_reads() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Headings").await;
        let spec = ModelSpec {
            model_id: "int8-model".to_string(),
            dimensions: 2,
            vector_encoding: VectorEncoding::Int8,
            chunker_version: 1,
            dequantization_scale: Some(1.0 / 127.0),
            dequantization_zero_point: Some(0),
        };
        RetrievalRepository::register_model(&pool, &spec)
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-h", "int8-model")
            .await
            .unwrap();

        let mut staged = doc("h0", 2, VectorEncoding::Int8, vec![96u8, 84u8]);
        staged.heading = Some("Decisões".to_string());
        RetrievalRepository::stage_documents(&pool, "job-h", "gen-h", "m", 1, &[staged.clone()])
            .await
            .unwrap();

        // Staging round-trips the heading (serde payload keeps it).
        let resumed = RetrievalRepository::list_staged_documents(&pool, "job-h")
            .await
            .unwrap();
        assert_eq!(resumed[0].heading.as_deref(), Some("Decisões"));

        RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen-h",
                meeting_id: "m",
                expected_source_revision: 1,
                job_id: "job-h",
            },
        )
        .await
        .unwrap();
        let page = RetrievalRepository::read_snapshot_documents(&pool, "gen-h", None)
            .await
            .unwrap();
        assert_eq!(page.documents[0].heading.as_deref(), Some("Decisões"));

        // Rows published before the column existed stay nullable/None - never
        // reconstructed heuristically.
        sqlx::query("UPDATE retrieval_documents SET heading = NULL")
            .execute(&pool)
            .await
            .unwrap();
        let page = RetrievalRepository::read_snapshot_documents(&pool, "gen-h", None)
            .await
            .unwrap();
        assert_eq!(page.documents[0].heading, None);
    }

    #[tokio::test]
    async fn backing_store_measurement_reports_allocated_pages_conservatively() {
        let pool = migrated_pool().await;
        insert_meeting(&pool, "m", "Backing").await;
        let upper_bound = RetrievalRepository::derived_backing_store_upper_bound_bytes(&pool)
            .await
            .unwrap();
        assert!(upper_bound > 0, "any allocated database reports pages");
        let (page_size,): (i64,) = sqlx::query_as("PRAGMA page_size")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            upper_bound % page_size.max(1) as u64 == 0 || upper_bound >= page_size as u64,
            "the measurement must be page-granular backing storage, not payload sums"
        );

        // Material data moves the bound: staging a document cannot shrink it.
        let spec = ModelSpec {
            model_id: "int8-model".to_string(),
            dimensions: 2,
            vector_encoding: VectorEncoding::Int8,
            chunker_version: 1,
            dequantization_scale: Some(1.0 / 127.0),
            dequantization_zero_point: Some(0),
        };
        RetrievalRepository::register_model(&pool, &spec)
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-b", "int8-model")
            .await
            .unwrap();
        let staged = doc("d", 2, VectorEncoding::Int8, vec![96u8, 84u8]);
        RetrievalRepository::stage_documents(&pool, "job", "gen-b", "m", 1, &[staged])
            .await
            .unwrap();
        assert!(
            RetrievalRepository::derived_backing_store_upper_bound_bytes(&pool)
                .await
                .unwrap()
                >= upper_bound
        );
    }

    #[tokio::test]
    async fn canonical_snapshot_read_captures_rows_and_replay_bound_in_one_database_snapshot() {
        // The injected read boundary needs real concurrent writers, so this
        // test uses a file-backed WAL database with two pooled connections.
        let db_path = std::env::temp_dir().join(format!(
            "meetly-retrieval-consistent-{}-{}.sqlite",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let options = sqlx::sqlite::SqliteConnectOptions::from_str(db_path.to_str().unwrap())
            .unwrap()
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let spec = ModelSpec {
            model_id: "int8-model".to_string(),
            dimensions: 2,
            vector_encoding: VectorEncoding::Int8,
            chunker_version: 1,
            dequantization_scale: Some(1.0 / 127.0),
            dequantization_zero_point: Some(0),
        };
        RetrievalRepository::register_model(&pool, &spec)
            .await
            .unwrap();
        RetrievalRepository::register_generation(&pool, "gen-c", "int8-model")
            .await
            .unwrap();
        insert_meeting(&pool, "doomed", "Doomed").await;
        let staged = doc("c0", 2, VectorEncoding::Int8, vec![96u8, 84u8]);
        RetrievalRepository::stage_documents(&pool, "job-c", "gen-c", "doomed", 1, &[staged])
            .await
            .unwrap();
        RetrievalRepository::replace_meeting_documents(
            &pool,
            ReplacementJob {
                generation_id: "gen-c",
                meeting_id: "doomed",
                expected_source_revision: 1,
                job_id: "job-c",
            },
        )
        .await
        .unwrap();
        let loaded_bound = RetrievalRepository::publication_lag(&pool, "gen-c")
            .await
            .unwrap()
            .unwrap()
            .0;

        // Pin a reader snapshot BEFORE the deletion commits, exactly like a
        // paged full load crossing a concurrent mutation boundary.
        let mut reader = pool.begin().await.unwrap();
        sqlx::query("SELECT COUNT(*) FROM retrieval_documents")
            .fetch_one(&mut *reader)
            .await
            .unwrap();
        sqlx::query("DELETE FROM meetings WHERE id = 'doomed'")
            .execute(&pool)
            .await
            .unwrap();
        let tombstone: (i64,) = sqlx::query_as(
            "SELECT MAX(change_id) FROM retrieval_index_changes WHERE operation = 'delete'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // Rows AND bound come from the pinned pre-delete snapshot: the loaded
        // vectors still exist and the acknowledged bound excludes the
        // tombstone, so acknowledgement can never outrun the reader state.
        let read = RetrievalRepository::read_canonical_snapshot_tx(
            &mut reader,
            "gen-c",
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .expect("an uncancelled consistent read returns state");
        drop(reader);
        assert_eq!(read.page.documents.len(), 1);
        assert_eq!(read.page.documents[0].meeting_id, "doomed");
        assert_eq!(read.canonical_change_id, loaded_bound);
        assert!(
            read.canonical_change_id < tombstone.0,
            "the pinned bound must exclude the concurrently committed tombstone"
        );

        // Acknowledging the pinned bound stays consistent with its rows; the
        // tombstone remains queued and replays afterwards.
        RetrievalRepository::acknowledge_journal(&pool, "gen-c", read.canonical_change_id)
            .await
            .unwrap();
        let pending =
            RetrievalRepository::read_journal_since(&pool, "gen-c", read.canonical_change_id, 10)
                .await
                .unwrap();
        assert!(pending
            .iter()
            .any(|change| change.operation == "delete" && change.meeting_id == "doomed"));

        pool.close().await;
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("sqlite-shm"));
    }
}
