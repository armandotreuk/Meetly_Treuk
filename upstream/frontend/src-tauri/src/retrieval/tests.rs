//! Focused Task 3.1 regressions: scope isolation and current membership,
//! variant provenance, title-only lexical behavior, semantic
//! unavailability/fallback, cancellation, bounds, and the no-logging rule.

use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use super::model::RetrievalModelError;
use super::service::{
    CoreTermLanguage, LexicalMode, PersistedRetrievalScope, QueryVariantKind, RetrievalChannel,
    RetrievalError, RetrievalLimits, RetrievalPurpose, RetrievalRequest, RetrievalService,
    SemanticFallbackReason,
};
use super::worker::{quantize_int8, DocumentEmbedder, LifecycleConfig, RetrievalLifecycle};
use crate::database::repositories::retrieval::{
    ModelSpec, ReplacementJob, ReplacementOutcome, RetrievalRepository, StagedDocument,
    VectorEncoding,
};

const MODEL_ID: &str = "test-e5-int8";
const DIMS: usize = 4;
const SCALE: f64 = 1.0 / 127.0;

// -- Harness -----------------------------------------------------------------

async fn migrated_pool() -> SqlitePool {
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

async fn insert_meeting(pool: &SqlitePool, id: &str, title: &str) {
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(title)
        .bind("2026-08-29T00:00:00Z")
        .bind("2026-08-29T00:00:00Z")
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

async fn insert_folder(pool: &SqlitePool, id: &str, name: &str, parent: Option<&str>) {
    sqlx::query(
        "INSERT INTO meeting_folders (id, name, parent_id, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(parent)
    .bind("2026-08-29T00:00:00Z")
    .execute(pool)
    .await
    .unwrap();
}

async fn set_meeting_folder(pool: &SqlitePool, meeting_id: &str, folder_id: Option<&str>) {
    sqlx::query("UPDATE meetings SET folder_id = ? WHERE id = ?")
        .bind(folder_id)
        .bind(meeting_id)
        .execute(pool)
        .await
        .unwrap();
}

/// Bulk-inserts `count` title-only filler meetings directly inside `folder_id`
/// with one recursive-CTE statement so the over-cap fixture stays fast.
async fn bulk_insert_folder_meetings(pool: &SqlitePool, folder_id: &str, count: usize) {
    sqlx::query(
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < ?)
         INSERT INTO meetings (id, title, folder_id, created_at, updated_at)
         SELECT 'filler-' || n, 'Filler', ?, '2026-08-29T00:00:00Z', '2026-08-29T00:00:00Z' FROM seq",
    )
    .bind(count as i64)
    .bind(folder_id)
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

/// Deterministic one-hot embedding: the axis is picked from the first byte so
/// documents published through the Task 2.4 repository path and queries
/// embedded by the fake are exactly comparable.
fn vector_for(text: &str) -> Vec<f32> {
    let axis = text.as_bytes().first().copied().unwrap_or(0) as usize % DIMS;
    let mut vector = vec![0.0_f32; DIMS];
    vector[axis] = 1.0;
    vector
}

async fn publish_meeting(pool: &SqlitePool, generation: &str, meeting: &str, texts: &[&str]) {
    let revision = RetrievalRepository::current_source_revision(pool, meeting)
        .await
        .unwrap()
        .unwrap();
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
    let job_id = format!("job-{generation}-{meeting}-{revision}");
    RetrievalRepository::stage_documents(pool, &job_id, generation, meeting, revision, &documents)
        .await
        .unwrap();
    assert!(matches!(
        RetrievalRepository::replace_meeting_documents(
            pool,
            ReplacementJob {
                generation_id: generation,
                meeting_id: meeting,
                expected_source_revision: revision,
                job_id: &job_id,
            },
        )
        .await
        .unwrap(),
        ReplacementOutcome::Published { .. }
    ));
}

/// Deterministic test embedder: query and document behavior match, so the
/// shared loader resolves the same vectors the publish path used.
struct ServiceEmbedder {
    fail_queries: StdMutex<bool>,
    entered: Arc<std::sync::atomic::AtomicBool>,
    park_until: StdMutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl ServiceEmbedder {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            fail_queries: StdMutex::new(false),
            entered: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            park_until: StdMutex::new(None),
        })
    }

    fn fail_queries(&self) {
        *self.fail_queries.lock().unwrap() = true;
    }
}

impl DocumentEmbedder for ServiceEmbedder {
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
        _texts: &[String],
        _cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
        Err(RetrievalModelError::Inference {
            role: "embedding",
            reason: "tests publish canonical vectors directly".to_string(),
        })
    }

    fn embed_queries_blocking(
        &self,
        texts: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
        self.entered.store(true, Ordering::SeqCst);
        if let Some(receiver) = self.park_until.lock().unwrap().take() {
            loop {
                if cancel.is_cancelled() {
                    return Err(RetrievalModelError::Cancelled);
                }
                match receiver.recv_timeout(Duration::from_millis(20)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                }
            }
        }
        if *self.fail_queries.lock().unwrap() {
            return Err(RetrievalModelError::Inference {
                role: "embedding",
                reason: "synthetic query embedding failure".to_string(),
            });
        }
        Ok(texts.iter().map(|text| vector_for(text)).collect())
    }
}

fn query_lifecycle(embedder: &Arc<ServiceEmbedder>) -> RetrievalLifecycle {
    let embedder = Arc::clone(embedder);
    RetrievalLifecycle::new(LifecycleConfig::testing(
        Arc::new(|| false),
        Arc::new(move || Ok(Arc::clone(&embedder) as Arc<dyn DocumentEmbedder>)),
    ))
}

fn failing_lifecycle() -> RetrievalLifecycle {
    RetrievalLifecycle::new(LifecycleConfig::testing(
        Arc::new(|| false),
        Arc::new(|| Err("simulated bundle unavailability".to_string())),
    ))
}

/// Installs the active snapshot for the seeded generation through the exact
/// production publisher pass, with the matching model runtime registered the
/// way the worker registers it after a real load.
async fn install_snapshot(pool: &SqlitePool, lifecycle: &RetrievalLifecycle, model_id: &str) {
    lifecycle.index_service().set_loaded_model(model_id);
    crate::retrieval::index::publish_tick(pool, lifecycle.index_service().as_ref())
        .await
        .unwrap();
}

fn request(
    query: &str,
    scope: PersistedRetrievalScope,
    limits: RetrievalLimits,
    core_language: CoreTermLanguage,
    cancel: Option<CancellationToken>,
) -> RetrievalRequest {
    RetrievalRequest {
        original_query: query.to_string(),
        rewritten_query: None,
        scope,
        purpose: RetrievalPurpose::Chat,
        limits,
        core_language,
        cancellation: cancel,
    }
}

async fn wait_until(predicate: impl AsyncFn() -> bool) {
    for _ in 0..500 {
        if predicate().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached");
}

// -- Scope isolation and current membership -----------------------------------

#[tokio::test]
async fn all_scope_returns_current_persisted_meetings_only() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-keep", "Kept").await;
    insert_meeting(&pool, "m-gone", "Gone").await;
    add_transcript(&pool, "t-keep", "m-keep", "needle persisted content").await;
    add_transcript(&pool, "t-gone", "m-gone", "needle deleted content").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-keep")
        .await
        .unwrap();
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-gone")
        .await
        .unwrap();
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-all", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-all", "m-keep", &["needle persisted"]).await;
    publish_meeting(&pool, "gen-all", "m-gone", &["needle deleted"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    // A tombstoned deletion must leave neither the lexical projection nor the
    // semantic snapshot serving the deleted meeting.
    sqlx::query("DELETE FROM meetings WHERE id = 'm-gone'")
        .execute(&pool)
        .await
        .unwrap();
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(result.semantic_fallback.is_none());
    assert!(!result.candidates.is_empty());
    let meeting_ids: BTreeSet<String> = result
        .candidates
        .iter()
        .map(|candidate| candidate.meeting_id.clone())
        .collect();
    assert_eq!(meeting_ids, BTreeSet::from(["m-keep".to_string()]));
}

#[tokio::test]
async fn meeting_scope_excludes_every_other_meeting() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-target", "Target").await;
    insert_meeting(&pool, "m-other", "Other").await;
    add_transcript(
        &pool,
        "t-target",
        "m-target",
        "shared retention topic target",
    )
    .await;
    add_transcript(&pool, "t-other", "m-other", "shared retention topic other").await;
    for meeting in ["m-target", "m-other"] {
        crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, meeting)
            .await
            .unwrap();
    }
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-meeting", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(
        &pool,
        "gen-meeting",
        "m-target",
        &["shared retention target"],
    )
    .await;
    publish_meeting(&pool, "gen-meeting", "m-other", &["shared retention other"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "retention",
                PersistedRetrievalScope::Meeting("m-target".to_string()),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(result.semantic_fallback.is_none());
    assert!(!result.candidates.is_empty());
    let meetings: BTreeSet<String> = result
        .candidates
        .iter()
        .map(|candidate| candidate.meeting_id.clone())
        .collect();
    assert_eq!(meetings, BTreeSet::from(["m-target".to_string()]));
    // Both channels are scope-safe: neither ever produced a hit for m-other.
    assert!(result.candidates.iter().any(|candidate| candidate
        .provenance
        .iter()
        .any(|provenance| provenance.channel == RetrievalChannel::Semantic)));
    assert!(result.candidates.iter().any(|candidate| candidate
        .provenance
        .iter()
        .any(|provenance| provenance.channel == RetrievalChannel::Lexical)));
}

/// A regression that dropped the `meeting_id` bind on the single-meeting FTS
/// path would turn this into a corpus-wide search instead of failing; naming
/// a meeting that does not currently exist must fail closed rather than
/// silently widen to an unscoped search.
#[tokio::test]
async fn meeting_scope_naming_no_current_meeting_fails_closed() {
    let pool = migrated_pool().await;
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let error = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::Meeting("m-missing".to_string()),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RetrievalError::InvalidScope(_)));
}

#[tokio::test]
async fn folder_scope_includes_descendants_and_excludes_outside_subtree() {
    let pool = migrated_pool().await;
    insert_folder(&pool, "f-parent", "Parent", None).await;
    insert_folder(&pool, "f-child", "Child", Some("f-parent")).await;
    insert_meeting(&pool, "m-parent", "Parent meeting").await;
    insert_meeting(&pool, "m-child", "Child meeting").await;
    insert_meeting(&pool, "m-outside", "Outside meeting").await;
    set_meeting_folder(&pool, "m-parent", Some("f-parent")).await;
    set_meeting_folder(&pool, "m-child", Some("f-child")).await;
    add_transcript(
        &pool,
        "t-parent",
        "m-parent",
        "shared retention topic parent",
    )
    .await;
    add_transcript(&pool, "t-child", "m-child", "shared retention topic child").await;
    add_transcript(
        &pool,
        "t-outside",
        "m-outside",
        "shared retention topic outside",
    )
    .await;
    for meeting in ["m-parent", "m-child", "m-outside"] {
        crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, meeting)
            .await
            .unwrap();
    }
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-folder", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(
        &pool,
        "gen-folder",
        "m-parent",
        &["shared retention parent"],
    )
    .await;
    publish_meeting(&pool, "gen-folder", "m-child", &["shared retention child"]).await;
    publish_meeting(
        &pool,
        "gen-folder",
        "m-outside",
        &["shared retention outside"],
    )
    .await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "retention",
                PersistedRetrievalScope::Folder("f-parent".to_string()),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(result.semantic_fallback.is_none());
    let meetings: BTreeSet<String> = result
        .candidates
        .iter()
        .map(|candidate| candidate.meeting_id.clone())
        .collect();
    assert_eq!(
        meetings,
        BTreeSet::from(["m-parent".to_string(), "m-child".to_string()])
    );
    // Every channel is scope-safe: the semantic channel returned in-scope
    // hits and no candidate from outside the subtree entered the result.
    assert!(result.candidates.iter().any(|candidate| {
        candidate.meeting_id == "m-parent"
            && candidate
                .provenance
                .iter()
                .any(|provenance| provenance.channel == RetrievalChannel::Semantic)
    }));
    assert!(result.candidates.iter().any(|candidate| candidate
        .provenance
        .iter()
        .any(|provenance| provenance.channel == RetrievalChannel::Lexical)));
}

/// A folder above [`MAX_FOLDER_SCAN_MEMBERSHIP`] current meetings must not
/// materialize a membership allow-list: the semantic scan runs the bounded
/// global over-fetch and the recursive root-folder gate alone decides
/// admission. The higher-ranked out-of-scope document proves the gate, the
/// retained rank proves the per-variant cap, and `ResolvedScope` carries only
/// the `Folder` tag (no membership field exists to assert).
#[tokio::test]
async fn over_cap_folder_scan_is_root_scoped_capped_and_ranked_in_scope() {
    let pool = migrated_pool().await;
    insert_folder(&pool, "f-big", "Big", None).await;
    insert_meeting(&pool, "y-inside", "Y Inside").await;
    insert_meeting(&pool, "z-inside", "Z Inside").await;
    insert_meeting(&pool, "a-outside", "A Outside").await;
    set_meeting_folder(&pool, "y-inside", Some("f-big")).await;
    set_meeting_folder(&pool, "z-inside", Some("f-big")).await;
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-over-cap", MODEL_ID)
        .await
        .unwrap();
    // Equal-score documents on the query axis; the scan's document-id
    // tie-break ranks the out-of-scope document ahead of both in-scope ones.
    publish_meeting(&pool, "gen-over-cap", "a-outside", &["zeta outside"]).await;
    publish_meeting(&pool, "gen-over-cap", "y-inside", &["zeta inside"]).await;
    publish_meeting(&pool, "gen-over-cap", "z-inside", &["zeta second"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    // Install before the fillers: pending filler work must never block
    // activation coverage, and it carries no documents anyway.
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;
    bulk_insert_folder_meetings(
        &pool,
        "f-big",
        super::service::MAX_FOLDER_SCAN_MEMBERSHIP - 1,
    )
    .await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "zeta",
                PersistedRetrievalScope::Folder("f-big".to_string()),
                RetrievalLimits {
                    lexical_per_variant: 5,
                    vector_per_variant: 1,
                },
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(result.semantic_fallback.is_none());
    assert!(matches!(
        result.scope.scope,
        PersistedRetrievalScope::Folder(ref id) if id == "f-big"
    ));
    // Exactly one semantic candidate survives the root gate and the
    // per-variant bound; the higher-ranked out-of-scope document never enters
    // the result and the retained candidate is re-ranked to 1.
    let semantic: Vec<&_> = result
        .candidates
        .iter()
        .filter(|candidate| {
            candidate
                .provenance
                .iter()
                .any(|provenance| provenance.channel == RetrievalChannel::Semantic)
        })
        .collect();
    assert_eq!(
        semantic.len(),
        1,
        "over-cap semantic output must stay capped per variant"
    );
    assert_eq!(semantic[0].evidence_id, "doc-y-inside-0");
    assert_eq!(semantic[0].meeting_id, "y-inside");
    assert!(semantic[0].provenance.iter().any(|provenance| {
        provenance.channel == RetrievalChannel::Semantic && provenance.rank == 1
    }));
    assert!(
        result
            .candidates
            .iter()
            .all(|candidate| candidate.meeting_id != "a-outside"),
        "the recursive root gate must drop the out-of-scope document"
    );
}

#[tokio::test]
async fn stale_fts_folder_metadata_cannot_bypass_current_membership() {
    let pool = migrated_pool().await;
    insert_folder(&pool, "f-1", "Work", None).await;
    insert_meeting(&pool, "m-moved", "Moved meeting").await;
    set_meeting_folder(&pool, "m-moved", Some("f-1")).await;
    add_transcript(&pool, "t-moved", "m-moved", "durable needle text").await;
    // Project the FTS rows while the meeting still lives in the folder.
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-moved")
        .await
        .unwrap();

    // Authoritative move without the best-effort FTS refresh hook: the FTS
    // rows keep claiming folder f-1 while current membership says otherwise.
    set_meeting_folder(&pool, "m-moved", None).await;
    let stale_rows: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM meeting_fts WHERE meeting_id = 'm-moved' AND folder_id = 'f-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stale_rows.0, 1,
        "fixture requires stale FTS folder metadata"
    );

    let embedder = ServiceEmbedder::new();
    let service = RetrievalService::new(query_lifecycle(&embedder));
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::Folder("f-1".to_string()),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(
        result
            .candidates
            .iter()
            .all(|candidate| candidate.meeting_id != "m-moved"),
        "stale FTS folder metadata must not bypass current membership"
    );

    // The stale projection is fail-closed until its repair catches up.
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(result.candidates.is_empty());
}

#[tokio::test]
async fn moved_in_meeting_with_stale_projection_is_omitted() {
    let pool = migrated_pool().await;
    insert_folder(&pool, "f-2", "Later", None).await;
    insert_meeting(&pool, "m-new", "New meeting").await;
    add_transcript(&pool, "t-new", "m-new", "fresh needle text").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-new")
        .await
        .unwrap();
    set_meeting_folder(&pool, "m-new", Some("f-2")).await;

    let embedder = ServiceEmbedder::new();
    let service = RetrievalService::new(query_lifecycle(&embedder));
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::Folder("f-2".to_string()),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(result.candidates.is_empty());
}

#[tokio::test]
async fn allowed_ids_scope_deduplicates_and_intersects_current_meetings() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-a", "A").await;
    insert_meeting(&pool, "m-b", "B").await;
    add_transcript(&pool, "t-a", "m-a", "scoped needle in a").await;
    add_transcript(&pool, "t-b", "m-b", "scoped needle in b").await;
    for meeting in ["m-a", "m-b"] {
        crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, meeting)
            .await
            .unwrap();
    }

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::AllowedMeetingIds(vec![
                    "m-a".to_string(),
                    "m-a".to_string(),
                    "ghost".to_string(),
                ]),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    let meetings: BTreeSet<String> = result
        .candidates
        .iter()
        .map(|candidate| candidate.meeting_id.clone())
        .collect();
    assert_eq!(meetings, BTreeSet::from(["m-a".to_string()]));
}

// -- Scope validation ----------------------------------------------------------

#[tokio::test]
async fn conflicting_scope_combinations_are_rejected() {
    let pool = migrated_pool().await;
    insert_folder(&pool, "f-a", "Alpha", None).await;
    insert_folder(&pool, "f-b", "Beta", None).await;
    insert_meeting(&pool, "m-1", "Meeting").await;
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));

    // Meeting and allowed-ID scopes reject the folder operator.
    for scope in [
        PersistedRetrievalScope::Meeting("m-1".to_string()),
        PersistedRetrievalScope::AllowedMeetingIds(vec!["m-1".to_string()]),
    ] {
        let error = service
            .retrieve(
                &pool,
                request(
                    r#"folder:"Alpha" needle"#,
                    scope,
                    RetrievalLimits::default(),
                    CoreTermLanguage::English,
                    None,
                ),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, RetrievalError::InvalidScope(_)));
    }

    // A folder operator naming a different folder than the explicit scope.
    let error = service
        .retrieve(
            &pool,
            request(
                r#"folder:"Beta" needle"#,
                PersistedRetrievalScope::Folder("f-a".to_string()),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RetrievalError::InvalidScope(_)));

    // A folder operator that names no current folder fails closed from All.
    let error = service
        .retrieve(
            &pool,
            request(
                r#"folder:"Missing" needle"#,
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RetrievalError::InvalidScope(_)));

    // An empty original query is invalid.
    let error = service
        .retrieve(
            &pool,
            request(
                "   ",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RetrievalError::InvalidQuery(_)));

    // Allowed-ID scopes are bounded by the approved snapshot ceiling.
    let overflow = (0..101).map(|index| format!("m{index}")).collect();
    let error = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::AllowedMeetingIds(overflow),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RetrievalError::InvalidScope(_)));
}

#[tokio::test]
async fn non_chat_purposes_fail_closed() {
    let pool = migrated_pool().await;
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let mut retrieval = request(
        "needle",
        PersistedRetrievalScope::All,
        RetrievalLimits::default(),
        CoreTermLanguage::English,
        None,
    );
    retrieval.purpose = RetrievalPurpose::Search;
    let error = service.retrieve(&pool, retrieval).await.unwrap_err();
    assert!(matches!(error, RetrievalError::UnsupportedPurpose(_)));
}

#[tokio::test]
async fn folder_operator_normalizes_into_folder_scope_from_all() {
    let pool = migrated_pool().await;
    insert_folder(&pool, "f-norm", "Normalize", None).await;
    insert_meeting(&pool, "m-in", "In").await;
    set_meeting_folder(&pool, "m-in", Some("f-norm")).await;
    add_transcript(&pool, "t-in", "m-in", "normalized needle").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-in")
        .await
        .unwrap();

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    // All + operator normalizes to the resolved folder.
    let result = service
        .retrieve(
            &pool,
            request(
                r#"folder:"Normalize" needle"#,
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(
        matches!(result.scope.scope, PersistedRetrievalScope::Folder(ref id) if id == "f-norm")
    );
    assert!(result
        .candidates
        .iter()
        .all(|candidate| candidate.meeting_id == "m-in"));

    // Folder + matching operator is accepted as the same scope.
    let result = service
        .retrieve(
            &pool,
            request(
                r#"folder:"Normalize" needle"#,
                PersistedRetrievalScope::Folder("f-norm".to_string()),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(!result.candidates.is_empty());
}

// -- Variants, provenance, and title behavior -----------------------------------

#[tokio::test]
async fn variant_provenance_remains_distinguishable() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-kafka", "Kafka").await;
    add_transcript(&pool, "t-kafka", "m-kafka", "kafka outbox decision pattern").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-kafka")
        .await
        .unwrap();

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let mut retrieval = request(
        "what did we decide about kafka",
        PersistedRetrievalScope::All,
        RetrievalLimits::default(),
        CoreTermLanguage::English,
        None,
    );
    retrieval.rewritten_query = Some("kafka outbox decision".to_string());
    let result = service.retrieve(&pool, retrieval).await.unwrap();
    assert!(result.semantic_fallback.is_some());

    let chunk = result
        .candidates
        .iter()
        .find(|candidate| candidate.meeting_id == "m-kafka")
        .expect("transcript chunk must be a candidate");
    let variants: BTreeSet<String> = chunk
        .provenance
        .iter()
        .map(|provenance| format!("{:?}", provenance.variant))
        .collect();
    assert!(variants.contains(&"Rewritten".to_string()));
    assert!(variants.contains(&"CoreTerms".to_string()));
    // Ranks are per channel list and 1-based.
    assert!(chunk
        .provenance
        .iter()
        .all(|provenance| provenance.rank >= 1));
}

#[tokio::test]
async fn title_only_query_returns_meeting_with_and_without_semantic() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-title", "Chaves de Acesso Rotation").await;
    add_transcript(&pool, "t-title", "m-title", "unrelated content words only").await;
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-title", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-title", "m-title", &["unrelated body"]).await;

    // Semantic unavailable: the title channel must stand alone.
    let lexical_only = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let result = lexical_only
        .retrieve(
            &pool,
            request(
                "chaves de acesso",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::Portuguese,
                None,
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::NoActiveGeneration)
    );
    let title_hit = result
        .candidates
        .iter()
        .find(|candidate| candidate.meeting_id == "m-title")
        .expect("title-only behavior must not depend on semantic availability");
    assert_eq!(title_hit.source_kind, "title");
    assert!(title_hit
        .provenance
        .iter()
        .any(|provenance| provenance.channel == RetrievalChannel::Title));

    // Semantic active: the title channel still returns the meeting.
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;
    let hybrid = RetrievalService::new(lifecycle);
    let result = hybrid
        .retrieve(
            &pool,
            request(
                "chaves de acesso",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::Portuguese,
                None,
            ),
        )
        .await
        .unwrap();
    let title_hit = result
        .candidates
        .iter()
        .find(|candidate| candidate.meeting_id == "m-title")
        .expect("title-only behavior with semantic active");
    assert!(title_hit
        .provenance
        .iter()
        .any(|provenance| provenance.channel == RetrievalChannel::Title));
}

// -- Semantic fallback matrix ----------------------------------------------------

#[tokio::test]
async fn semantic_unavailable_degrades_to_lexical_candidates() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-lex", "Lexical").await;
    add_transcript(&pool, "t-lex", "m-lex", "fallback needle text").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-lex")
        .await
        .unwrap();

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::NoActiveGeneration)
    );
    assert!(result
        .candidates
        .iter()
        .any(|candidate| candidate.meeting_id == "m-lex"
            && candidate
                .provenance
                .iter()
                .any(|provenance| provenance.channel == RetrievalChannel::Lexical)));
}

#[tokio::test]
async fn query_embedding_failure_degrades_to_lexical_candidates() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-embed", "Embed").await;
    add_transcript(&pool, "t-embed", "m-embed", "fallback needle text").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-embed")
        .await
        .unwrap();
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-embed", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-embed", "m-embed", &["fallback needle"]).await;
    let embedder = ServiceEmbedder::new();
    embedder.fail_queries();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::EmbeddingUnavailable)
    );
    assert!(result
        .candidates
        .iter()
        .any(|candidate| candidate.meeting_id == "m-embed"));
}

#[tokio::test]
async fn model_load_failure_degrades_to_lexical_candidates() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-load", "Load").await;
    add_transcript(&pool, "t-load", "m-load", "fallback needle text").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-load")
        .await
        .unwrap();
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-load", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-load", "m-load", &["fallback needle"]).await;
    let lifecycle = failing_lifecycle();
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::EmbeddingUnavailable)
    );
    assert!(!result.candidates.is_empty());
}

#[tokio::test]
async fn query_embedding_model_mismatch_never_scores_the_snapshot() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-mismatch", "Mismatch").await;
    add_transcript(&pool, "t-mismatch", "m-mismatch", "needle text").await;
    register_test_model(&pool).await;
    // Register a second approved-shaped model and activate its generation, so
    // the active snapshot's model differs from the query embedder's identity.
    assert!(RetrievalRepository::ensure_model(
        &pool,
        &ModelSpec {
            model_id: "other-bundle".to_string(),
            dimensions: DIMS as u32,
            vector_encoding: VectorEncoding::Int8,
            chunker_version: 1,
            dequantization_scale: Some(SCALE),
            dequantization_zero_point: Some(0),
        }
    )
    .await
    .unwrap());
    RetrievalRepository::ensure_generation(&pool, "gen-mismatch", "other-bundle")
        .await
        .unwrap();
    publish_meeting(&pool, "gen-mismatch", "m-mismatch", &["needle"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, "other-bundle").await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::ModelMismatch)
    );
    assert!(result.candidates.iter().all(|candidate| candidate
        .provenance
        .iter()
        .all(|provenance| provenance.channel != RetrievalChannel::Semantic)));
}

#[tokio::test]
async fn snapshot_journal_behind_canonical_falls_back_after_bounded_catchup() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-lag", "Lag").await;
    add_transcript(&pool, "t-lag", "m-lag", "fallback needle text").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-lag")
        .await
        .unwrap();
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-lag", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-lag", "m-lag", &["needle"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;
    // Journal behind canonical state: queries pause for bounded catch-up and
    // then degrade to lexical.
    lifecycle.index_service().mark_stale();

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(matches!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::CatchUpTimeout { behind: 1 })
    ));
    assert!(result
        .candidates
        .iter()
        .any(|candidate| candidate.meeting_id == "m-lag"));
}

#[tokio::test]
async fn dirty_source_rows_are_ineligible_for_all_channels() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-dirty", "Dirty").await;
    add_transcript(&pool, "t-dirty", "m-dirty", "needle indexed content").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-dirty")
        .await
        .unwrap();
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-dirty", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-dirty", "m-dirty", &["needle indexed"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    // A mid-query mutation: source revision advances without a journal entry,
    // so the snapshot still serves the meeting's rows at lag zero. The
    // candidate gate must reject them anyway.
    add_transcript(&pool, "t-dirty-2", "m-dirty", "needle current content").await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(result.semantic_fallback.is_none());
    assert!(result.candidates.is_empty());
    assert!(result.candidates.iter().all(|candidate| candidate
        .provenance
        .iter()
        .all(|provenance| provenance.channel != RetrievalChannel::Semantic)));
}

// -- Cancellation -----------------------------------------------------------------

#[tokio::test]
async fn cancelled_request_fails_closed_before_retrieval() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-cancel", "Cancel").await;
    add_transcript(&pool, "t-cancel", "m-cancel", "needle text").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-cancel")
        .await
        .unwrap();

    let cancel = CancellationToken::new();
    cancel.cancel();
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let error = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                Some(cancel),
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RetrievalError::Cancelled));
}

#[tokio::test]
async fn cancellation_during_query_embedding_propagates_without_lexical_fallback() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-park", "Park").await;
    add_transcript(&pool, "t-park", "m-park", "needle text").await;
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-park", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-park", "m-park", &["needle"]).await;
    let embedder = ServiceEmbedder::new();
    let (sender, receiver) = std::sync::mpsc::channel::<()>();
    *embedder.park_until.lock().unwrap() = Some(receiver);
    let cancel = CancellationToken::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let service = RetrievalService::new(lifecycle);
    let handle = tokio::spawn({
        let pool = pool.clone();
        let cancel = cancel.clone();
        async move {
            service
                .retrieve(
                    &pool,
                    request(
                        "needle",
                        PersistedRetrievalScope::All,
                        RetrievalLimits::default(),
                        CoreTermLanguage::English,
                        Some(cancel),
                    ),
                )
                .await
        }
    });
    wait_until(async || embedder.entered.load(Ordering::SeqCst)).await;
    cancel.cancel();
    drop(sender);
    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("cancelled preparation must not hang")
        .unwrap();
    assert!(matches!(result, Err(RetrievalError::Cancelled)));
}

// -- Bounds ------------------------------------------------------------------------

#[tokio::test]
async fn candidate_limits_are_enforced_before_return() {
    let pool = migrated_pool().await;
    for index in 0..4 {
        let meeting = format!("m-bound-{index}");
        insert_meeting(&pool, &meeting, &meeting).await;
        add_transcript(
            &pool,
            &format!("t-bound-{index}"),
            &meeting,
            "alpha needle1 body text",
        )
        .await;
        crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, &meeting)
            .await
            .unwrap();
    }
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-bound", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-bound", "m-bound-0", &["alpha", "alpha2"]).await;
    publish_meeting(&pool, "gen-bound", "m-bound-1", &["alpha", "alpha2"]).await;
    publish_meeting(&pool, "gen-bound", "m-bound-2", &["alpha", "alpha2"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "alpha needle1 needle2",
                PersistedRetrievalScope::All,
                RetrievalLimits {
                    lexical_per_variant: 2,
                    vector_per_variant: 1,
                },
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    // Two lexical variants (original + core terms), each capped at 2, then
    // deduplicated by stable identity.
    let lexical = result
        .candidates
        .iter()
        .filter(|candidate| {
            candidate
                .provenance
                .iter()
                .any(|provenance| provenance.channel == RetrievalChannel::Lexical)
        })
        .count();
    assert!(
        lexical <= 4,
        "lexical candidates must respect per-variant limits"
    );
    assert!(lexical >= 1);
    // Vector search returns at most one hit per variant, all tied on score,
    // so the limit decides deterministically.
    let semantic: Vec<&_> = result
        .candidates
        .iter()
        .filter(|candidate| {
            candidate
                .provenance
                .iter()
                .any(|provenance| provenance.channel == RetrievalChannel::Semantic)
        })
        .collect();
    assert!(semantic.len() <= 2);
    for candidate in &semantic {
        assert!(candidate
            .provenance
            .iter()
            .filter(|provenance| provenance.channel == RetrievalChannel::Semantic)
            .all(|provenance| provenance.rank <= 1));
    }
}

#[tokio::test]
async fn zero_limits_disable_their_channels() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-zero", "Zero").await;
    add_transcript(&pool, "t-zero", "m-zero", "needle text").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-zero")
        .await
        .unwrap();
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-zero", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-zero", "m-zero", &["needle"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits {
                    lexical_per_variant: 0,
                    vector_per_variant: 0,
                },
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(result.candidates.is_empty());
}

// -- Logging discipline -------------------------------------------------------------

/// The service must never grow a log call that could carry query or candidate
/// text: the only permitted call site is the privacy-safe `outcome_line`.
/// ponytail: line-based guard, not an AST check; it still fails on any new
/// single-line log call in the service module.
#[test]
fn service_source_never_introduces_content_logging() {
    for line in include_str!("service.rs").lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let calls_log = trimmed.contains("log::") || trimmed.contains("tracing::");
        if calls_log && trimmed.contains('(') {
            assert!(
                trimmed.contains("outcome_line"),
                "retrieval service log calls must go through outcome_line: {trimmed}"
            );
        }
    }
}

#[tokio::test]
async fn empty_membership_scope_returns_without_retrieval() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-empty", "Empty").await;

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::AllowedMeetingIds(vec!["ghost".to_string()]),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(result.candidates.is_empty());
    assert!(result.semantic_fallback.is_none());
}

// -- Repository candidate gates -------------------------------------------------------

#[tokio::test]
async fn verified_semantic_meetings_drops_missing_dirty_and_unindexed() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-good", "Good").await;
    insert_meeting(&pool, "m-dirty", "Dirty").await;
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-verify", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-verify", "m-good", &["content"]).await;
    publish_meeting(&pool, "gen-verify", "m-dirty", &["content"]).await;
    // Dirty: content changed after publication.
    add_transcript(&pool, "t-verify", "m-dirty", "newer content").await;

    let verified: Vec<(String, String)> = RetrievalRepository::verified_semantic_meetings(
        &pool,
        "gen-verify",
        &[
            "m-good".to_string(),
            "m-dirty".to_string(),
            "missing".to_string(),
        ],
        None,
    )
    .await
    .unwrap();
    assert_eq!(verified, vec![("m-good".to_string(), "Good".to_string())]);

    // A meeting never indexed for the generation is ineligible: registering
    // the generation seeds pending per-meeting state (indexed 0), which is
    // behind the current source revision even though canonical rows could
    // exist under another generation.
    RetrievalRepository::ensure_generation(&pool, "gen-other", MODEL_ID)
        .await
        .unwrap();
    let verified = RetrievalRepository::verified_semantic_meetings(
        &pool,
        "gen-other",
        &["m-good".to_string(), "m-dirty".to_string()],
        None,
    )
    .await
    .unwrap();
    assert!(verified.is_empty());

    // Eligibility requires an exact revision match: an indexed revision ahead
    // of the source revision is an anomaly and is dropped, not tolerated.
    sqlx::query(
        "UPDATE retrieval_meeting_state
         SET indexed_source_revision = indexed_source_revision + 1
         WHERE generation_id = 'gen-verify' AND meeting_id = 'm-good'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let verified = RetrievalRepository::verified_semantic_meetings(
        &pool,
        "gen-verify",
        &["m-good".to_string()],
        None,
    )
    .await
    .unwrap();
    assert!(verified.is_empty());

    // A non-ready per-meeting state is ineligible even when revisions match.
    sqlx::query(
        "UPDATE retrieval_meeting_state
         SET indexed_source_revision = (SELECT source_revision FROM search_source_state WHERE meeting_id = 'm-good'),
             state = 'failed'
         WHERE generation_id = 'gen-verify' AND meeting_id = 'm-good'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let verified = RetrievalRepository::verified_semantic_meetings(
        &pool,
        "gen-verify",
        &["m-good".to_string()],
        None,
    )
    .await
    .unwrap();
    assert!(verified.is_empty());

    // The recursive root-folder gate: the same candidate list admits only
    // current subtree members when a folder root is supplied.
    insert_folder(&pool, "f-root", "Root", None).await;
    insert_folder(&pool, "f-child", "Child", Some("f-root")).await;
    insert_meeting(&pool, "m-in", "In").await;
    insert_meeting(&pool, "m-descendant", "Descendant").await;
    insert_meeting(&pool, "m-out", "Out").await;
    set_meeting_folder(&pool, "m-in", Some("f-root")).await;
    set_meeting_folder(&pool, "m-descendant", Some("f-child")).await;
    publish_meeting(&pool, "gen-other", "m-in", &["content"]).await;
    publish_meeting(&pool, "gen-other", "m-descendant", &["content"]).await;
    publish_meeting(&pool, "gen-other", "m-out", &["content"]).await;
    let candidates = [
        "m-in".to_string(),
        "m-descendant".to_string(),
        "m-out".to_string(),
    ];
    let verified =
        RetrievalRepository::verified_semantic_meetings(&pool, "gen-other", &candidates, None)
            .await
            .unwrap();
    assert_eq!(
        verified.len(),
        3,
        "without a root the gate is membership-free"
    );
    let verified = RetrievalRepository::verified_semantic_meetings(
        &pool,
        "gen-other",
        &candidates,
        Some("f-root"),
    )
    .await
    .unwrap();
    let verified_ids: BTreeSet<String> = verified.into_iter().map(|(id, _)| id).collect();
    assert_eq!(
        verified_ids,
        BTreeSet::from(["m-in".to_string(), "m-descendant".to_string()])
    );

    // Vanished canonical rows never become evidence.
    let contents = RetrievalRepository::document_contents(
        &pool,
        "gen-other",
        &["doc-m-good-0".to_string(), "ghost-doc".to_string()],
    )
    .await
    .unwrap();
    assert!(contents.is_empty());
}

// -- Evaluated core-term policy (R16 finding 1) ---------------------------------

use super::service::{core_terms, normalize_core_token};

/// Portuguese: only the fixed evaluated PT list is removed; content words,
/// names, and numbers survive in original order.
#[test]
fn core_terms_apply_the_evaluated_portuguese_list() {
    let terms = core_terms(
        "quais os dias de comunicacao por whatsapp para o fluxo de retencao?",
        CoreTermLanguage::Portuguese,
    );
    assert_eq!(
        terms,
        ["dias", "comunicacao", "whatsapp", "fluxo", "retencao"]
    );
}

/// English: only the fixed evaluated EN list is removed.
#[test]
fn core_terms_apply_the_evaluated_english_list() {
    let terms = core_terms(
        "what was the decision about the kafka outbox pattern",
        CoreTermLanguage::English,
    );
    assert_eq!(terms, ["decision", "about", "kafka", "outbox", "pattern"]);
}

/// Diacritic folding matches the evaluated normalizer character for
/// character; folded tokens that are not on the fixed list are preserved.
#[test]
fn core_terms_fold_listed_portuguese_diacritics() {
    assert_eq!(normalize_core_token("comunicação"), "comunicacao");
    assert_eq!(normalize_core_token("não"), "nao");
    assert_eq!(normalize_core_token("fluxo"), "fluxo");
    let terms = core_terms("a comunicação não foi", CoreTermLanguage::Portuguese);
    assert_eq!(terms, ["comunicacao", "nao"]);
}

/// Title Case and all-caps titles carry uppercase accented letters
/// (`to_ascii_lowercase` leaves non-ASCII characters untouched), so folding
/// must match the uppercase forms directly rather than relying on ASCII
/// lowercasing to expose them first.
#[test]
fn core_terms_fold_uppercase_portuguese_diacritics() {
    assert_eq!(normalize_core_token("Água"), "agua");
    assert_eq!(normalize_core_token("REUNIÃO"), "reuniao");
    assert_eq!(normalize_core_token("Órgão"), "orgao");
}

/// All-stopword fallback: when every token would be removed, the untouched
/// normalized tokens are kept instead of an empty variant.
#[test]
fn core_terms_fall_back_to_untouched_normalized_tokens() {
    assert_eq!(
        core_terms("o que foi", CoreTermLanguage::Portuguese),
        ["o", "que", "foi"]
    );
    assert_eq!(
        core_terms("was the is are of to", CoreTermLanguage::English),
        ["was", "the", "is", "are", "of", "to"]
    );
}

/// Numeric and name tokens are never removed; an unknown language applies no
/// removal list at all, so its core variant may equal the original text while
/// keeping distinct provenance.
#[test]
fn core_terms_preserve_names_numbers_and_unknown_languages() {
    assert_eq!(
        core_terms("revisao do item 42 e do SLA", CoreTermLanguage::Portuguese),
        ["revisao", "item", "42", "sla"]
    );
    assert_eq!(
        core_terms("kafka 250 mil", CoreTermLanguage::Unknown),
        ["kafka", "250", "mil"]
    );
}

/// The production stopword lists are hand-copied from
/// `tests/fixtures/evaluation_policy.json`, the document the evaluation
/// harness treats as authoritative for the lexical policy. Nothing else ties
/// the two together, so a policy edit that is not mirrored here would let
/// the evaluation gates measure a lexical policy production does not run.
#[test]
fn core_terms_stopword_lists_match_the_evaluation_policy_fixture() {
    let policy: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/evaluation_policy.json"))
            .expect("evaluation_policy.json must parse as JSON");
    let fixture_list = |field: &str| -> Vec<&str> {
        policy["lexicalPolicy"][field]
            .as_array()
            .unwrap_or_else(|| panic!("lexicalPolicy.{field} must be an array"))
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .unwrap_or_else(|| panic!("lexicalPolicy.{field} entries must be strings"))
            })
            .collect()
    };
    assert_eq!(
        fixture_list("portugueseHighFrequency"),
        super::service::PORTUGUESE_HIGH_FREQUENCY.to_vec()
    );
    assert_eq!(
        fixture_list("englishHighFrequency"),
        super::service::ENGLISH_HIGH_FREQUENCY.to_vec()
    );
}

// -- Queued cancellation (R16 finding 2) ------------------------------------------

#[tokio::test]
async fn request_cancelled_while_queued_is_removed_immediately() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-queued", "Queued").await;
    add_transcript(&pool, "t-queued", "m-queued", "needle text").await;
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-queued", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-queued", "m-queued", &["needle"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    // Hold the only interactive inference permit so the request must queue.
    let held = lifecycle
        .scheduler()
        .enqueue_interactive()
        .unwrap()
        .wait_for_permit()
        .await
        .unwrap();
    let scheduler = lifecycle.scheduler();

    let cancel = CancellationToken::new();
    let service = RetrievalService::new(lifecycle.clone());
    let handle = tokio::spawn({
        let pool = pool.clone();
        let cancel = cancel.clone();
        async move {
            service
                .retrieve(
                    &pool,
                    request(
                        "needle",
                        PersistedRetrievalScope::All,
                        RetrievalLimits::default(),
                        CoreTermLanguage::English,
                        Some(cancel),
                    ),
                )
                .await
        }
    });
    wait_until(async || scheduler.queued_interactive() == 1).await;

    // The request token must remove the queued entry immediately and abort
    // with a typed cancellation, without waiting for the held permit.
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("cancelled queued request must not wait for the inference permit")
        .unwrap();
    assert!(matches!(result, Err(RetrievalError::Cancelled)));
    assert_eq!(scheduler.queued_interactive(), 0);
    // The permit was never released by the service; the holder still owns it.
    drop(held);
}

// -- Pinned generation (R16 finding 3) ---------------------------------------------

#[tokio::test]
async fn activation_swap_during_embedding_cannot_score_the_new_generation() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-pin", "Pin").await;
    add_transcript(&pool, "t-pin", "m-pin", "needle text").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-pin")
        .await
        .unwrap();
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-a", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-a", "m-pin", &["needle"]).await;
    let embedder = ServiceEmbedder::new();
    let (sender, receiver) = std::sync::mpsc::channel::<()>();
    *embedder.park_until.lock().unwrap() = Some(receiver);
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let cancel = CancellationToken::new();
    let service = RetrievalService::new(lifecycle.clone());
    let handle = tokio::spawn({
        let pool = pool.clone();
        let cancel = cancel.clone();
        async move {
            service
                .retrieve(
                    &pool,
                    request(
                        "needle",
                        PersistedRetrievalScope::All,
                        RetrievalLimits::default(),
                        CoreTermLanguage::English,
                        Some(cancel),
                    ),
                )
                .await
        }
    });
    wait_until(async || embedder.entered.load(Ordering::SeqCst)).await;

    // While the query is parked, a shadow generation of the same model
    // activates and the publisher installs its snapshot.
    RetrievalRepository::ensure_generation(&pool, "gen-b", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-b", "m-pin", &["needle"]).await;
    RetrievalRepository::set_generation_state(&pool, "gen-b", "ready")
        .await
        .unwrap();
    RetrievalRepository::switch_active_generation(&pool, "gen-b")
        .await
        .unwrap();
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    drop(sender);
    let outcome = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("pinned request must resolve after the swap")
        .unwrap();
    let result = outcome.unwrap();
    assert_eq!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::GenerationChanged)
    );
    assert!(result.candidates.iter().all(|candidate| candidate
        .provenance
        .iter()
        .all(|provenance| provenance.channel != RetrievalChannel::Semantic)));
    assert!(result
        .candidates
        .iter()
        .any(|candidate| candidate.meeting_id == "m-pin"));
    // A fenced request is never acknowledged.
    assert_eq!(lifecycle.index_service().fast_hybrid_query_count(), 0);
}

// -- Semantic-stage SQL failures (R16 finding 4) -------------------------------------

#[tokio::test]
async fn semantic_gate_failure_keeps_lexical_candidates() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-gate", "Gate").await;
    add_transcript(&pool, "t-gate", "m-gate", "fallback needle text").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-gate")
        .await
        .unwrap();
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-gate", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-gate", "m-gate", &["fallback needle"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    // The candidate-gate read fails; the request must degrade to lexical
    // with a typed reason instead of failing whole.
    sqlx::query("DROP TABLE search_source_state")
        .execute(&pool)
        .await
        .unwrap();

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::SemanticScanFailed)
    );
    assert!(result
        .candidates
        .iter()
        .any(|candidate| candidate.meeting_id == "m-gate"
            && candidate
                .provenance
                .iter()
                .any(|provenance| provenance.channel == RetrievalChannel::Lexical)));
    assert!(result.candidates.iter().all(|candidate| candidate
        .provenance
        .iter()
        .all(|provenance| provenance.channel != RetrievalChannel::Semantic)));
}

// -- Bounded streaming title scan (R16 finding 6) --------------------------------------

#[tokio::test]
async fn title_scan_finds_matches_across_streamed_pages() {
    let pool = migrated_pool().await;
    for index in 0..(super::service::TITLE_SCAN_PAGE + 40) {
        insert_meeting(
            &pool,
            &format!("m-fill-{index}"),
            &format!("filler titulo {index}"),
        )
        .await;
    }
    // The strongest title match is inserted last, so it lives in the final
    // streamed page.
    insert_meeting(&pool, "m-last", "Chaves de Acesso Rotation").await;

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let result = service
        .retrieve(
            &pool,
            request(
                "chaves de acesso",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::Portuguese,
                None,
            ),
        )
        .await
        .unwrap();
    let title_hit = result
        .candidates
        .iter()
        .find(|candidate| candidate.meeting_id == "m-last")
        .expect("the streamed scan must cover every page");
    assert_eq!(title_hit.source_kind, "title");
    assert!(title_hit
        .provenance
        .iter()
        .any(|provenance| provenance.channel == RetrievalChannel::Title && provenance.rank == 1));
    assert!(result
        .candidates
        .iter()
        .all(|candidate| candidate.meeting_id == "m-last"));
}

async fn bounded_folder_title_ids(reverse_insert: bool) -> Vec<String> {
    let pool = migrated_pool().await;
    insert_folder(&pool, "title-root", "Title Root", None).await;
    let mut meetings: Vec<(String, String)> = (0..super::service::TITLE_SCAN_PAGE)
        .map(|index| (format!("filler-{index:03}"), format!("Filler {index}")))
        .collect();
    meetings.extend([
        ("match-best".to_string(), "Alpha Beta".to_string()),
        ("match-a".to_string(), "Alpha".to_string()),
        ("match-b".to_string(), "Alpha".to_string()),
        ("match-c".to_string(), "Alpha".to_string()),
        ("match-zero".to_string(), "Gamma".to_string()),
    ]);
    if reverse_insert {
        meetings.reverse();
    }
    for (meeting_id, title) in meetings {
        insert_meeting(&pool, &meeting_id, &title).await;
        set_meeting_folder(&pool, &meeting_id, Some("title-root")).await;
    }

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let result = service
        .retrieve(
            &pool,
            request(
                "alpha beta",
                PersistedRetrievalScope::Folder("title-root".to_string()),
                RetrievalLimits {
                    lexical_per_variant: 3,
                    vector_per_variant: 0,
                },
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    result
        .candidates
        .iter()
        .filter(|candidate| candidate.source_kind == "title")
        .map(|candidate| candidate.meeting_id.clone())
        .collect()
}

#[tokio::test]
async fn folder_title_top_k_is_bounded_deterministic_and_page_order_independent() {
    let mut oracle = vec![
        ("match-best", 2),
        ("match-a", 1),
        ("match-b", 1),
        ("match-c", 1),
    ];
    oracle.sort_by(|(left_id, left_overlap), (right_id, right_overlap)| {
        right_overlap
            .cmp(left_overlap)
            .then_with(|| left_id.cmp(right_id))
    });
    let expected: Vec<String> = oracle
        .into_iter()
        .take(3)
        .map(|(meeting_id, _)| meeting_id.to_string())
        .collect();
    assert_eq!(bounded_folder_title_ids(false).await, expected);
    assert_eq!(bounded_folder_title_ids(true).await, expected);
}

#[tokio::test]
async fn lexical_evidence_preserves_literal_mark_tags() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-mark", "Marked").await;
    add_transcript(
        &pool,
        "t-mark",
        "m-mark",
        "literal <mark> needle </mark> text",
    )
    .await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-mark")
        .await
        .unwrap();
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits {
                    lexical_per_variant: 10,
                    vector_per_variant: 0,
                },
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    let evidence = result
        .candidates
        .iter()
        .find(|candidate| candidate.source_kind == "transcript")
        .expect("literal-mark transcript should be retrieved");
    assert!(evidence.text.contains("<mark>"));
    assert!(evidence.text.contains("</mark>"));
}

// -- R17 finding 1: explicit request language drives the core variant ----------

fn has_core_and_provenance(result: &super::service::RetrievalResult) -> bool {
    result.candidates.iter().any(|candidate| {
        candidate.provenance.iter().any(|provenance| {
            provenance.channel == RetrievalChannel::Lexical
                && provenance.variant == QueryVariantKind::CoreTerms
                && provenance.mode == Some(LexicalMode::And)
        })
    })
}

/// Service entry: a Portuguese request applies the evaluated PT list, so the
/// core variant AND-matches content words alone; an unknown language keeps
/// every normalized token and cannot AND-match. English mirrors the same
/// discriminator.
#[tokio::test]
async fn request_language_selects_the_evaluated_core_list() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-pt", "PT").await;
    add_transcript(&pool, "t-pt", "m-pt", "dias retencao comunicacao").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-pt")
        .await
        .unwrap();
    insert_meeting(&pool, "m-en", "EN").await;
    add_transcript(&pool, "t-en", "m-en", "decision outbox").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-en")
        .await
        .unwrap();
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));

    // Portuguese: the PT list removes quais/os/de, so the core variant
    // AND-matches the content words alone.
    let pt = service
        .retrieve(
            &pool,
            request(
                "quais os dias de retencao",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::Portuguese,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(has_core_and_provenance(&pt));

    // Unknown language: nothing is removed, the AND over all tokens cannot
    // match a transcript lacking the stopwords.
    let unknown = service
        .retrieve(
            &pool,
            request(
                "quais os dias de retencao",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::Unknown,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(!has_core_and_provenance(&unknown));

    // English: the EN list removes what/was/the, so the core variant
    // AND-matches "decision" alone.
    let en = service
        .retrieve(
            &pool,
            request(
                "what was the decision",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(has_core_and_provenance(&en));
    assert!(en
        .candidates
        .iter()
        .any(|candidate| candidate.meeting_id == "m-en"));

    let en_unknown = service
        .retrieve(
            &pool,
            request(
                "what was the decision",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::Unknown,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(!has_core_and_provenance(&en_unknown));
}

// -- R17 finding 2: cancellation across scope and title SQL boundaries ----------

/// A cancelled request must return Cancelled before any scope SQL read runs:
/// the meetings table is dropped, so any attempted read would fail with a
/// database error instead.
#[tokio::test]
async fn cancelled_request_fails_before_scope_resolution_sql() {
    let pool = migrated_pool().await;
    sqlx::query("DROP TABLE meetings")
        .execute(&pool)
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let error = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                Some(cancel),
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RetrievalError::Cancelled));
}

/// Control for the test above: without cancellation the same scope SQL
/// failure stays request-fatal.
#[tokio::test]
async fn scope_database_failures_stay_request_fatal() {
    let pool = migrated_pool().await;
    sqlx::query("DROP TABLE meetings")
        .execute(&pool)
        .await
        .unwrap();
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let error = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RetrievalError::Database(_)));
}

/// Cancellation landing while a scope SQL read is awaiting its connection
/// must abort the request through the post-read boundary check instead of
/// proceeding through normalization.
#[tokio::test]
async fn cancellation_during_scope_sql_read_aborts_the_request() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-hold", "Hold").await;

    // Hold the only pool connection so the first scope SQL read blocks.
    let held = pool.acquire().await.unwrap();
    let cancel = CancellationToken::new();
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let handle = tokio::spawn({
        let pool = pool.clone();
        let cancel = cancel.clone();
        async move {
            service
                .retrieve(
                    &pool,
                    request(
                        "needle",
                        PersistedRetrievalScope::All,
                        RetrievalLimits::default(),
                        CoreTermLanguage::English,
                        Some(cancel),
                    ),
                )
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    drop(held);

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("cancelled request must resolve after the read completes")
        .unwrap();
    assert!(matches!(result, Err(RetrievalError::Cancelled)));
}

// -- R17 finding 3: request-atomic generation fencing ----------------------------

/// A generation change after one successful variant scan discards every
/// accumulated semantic hit: the fenced request returns the typed fallback,
/// records no semantic candidate, and never acknowledges the Fast hybrid
/// query.
#[tokio::test]
async fn generation_change_after_a_scan_cannot_retain_semantic_hits() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-pin", "Pin").await;
    add_transcript(&pool, "t-pin", "m-pin", "kafka outbox decision pattern").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-pin")
        .await
        .unwrap();
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-a", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-a", "m-pin", &["kafka outbox decision"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let service = RetrievalService::new(lifecycle.clone());
    let (signal, mut signal_rx) = tokio::sync::mpsc::unbounded_channel();
    service.arm_scan_gate(signal);
    let release_handle = service.clone();

    let mut retrieval = request(
        "what did we decide about kafka",
        PersistedRetrievalScope::All,
        RetrievalLimits::default(),
        CoreTermLanguage::English,
        None,
    );
    retrieval.rewritten_query = Some("kafka outbox decision".to_string());
    let handle = tokio::spawn({
        let pool = pool.clone();
        async move { service.retrieve(&pool, retrieval).await }
    });

    // First variant scan completed with hits accumulated; swap generations
    // while the loop waits at the test gate.
    signal_rx
        .recv()
        .await
        .expect("the first variant scan must complete");
    RetrievalRepository::ensure_generation(&pool, "gen-b", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-b", "m-pin", &["needle"]).await;
    RetrievalRepository::set_generation_state(&pool, "gen-b", "ready")
        .await
        .unwrap();
    RetrievalRepository::switch_active_generation(&pool, "gen-b")
        .await
        .unwrap();
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;
    release_handle.release_scan_gate();

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("fenced request must resolve")
        .unwrap();
    let result = result.unwrap();
    assert_eq!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::GenerationChanged)
    );
    assert!(result.candidates.iter().all(|candidate| candidate
        .provenance
        .iter()
        .all(|provenance| provenance.channel != RetrievalChannel::Semantic)));
    assert!(result
        .candidates
        .iter()
        .any(|candidate| candidate.meeting_id == "m-pin"));
    // A fenced request is never acknowledged.
    assert_eq!(lifecycle.index_service().fast_hybrid_query_count(), 0);
}

// -- R17 findings 4+5: All scope representation and the Fast hybrid counter ------

/// All scope resolves without a materialized per-meeting allow-list (the
/// request-start membership stays internal to the service) while the returned
/// candidates still exclude noncurrent data.
#[tokio::test]
async fn all_scope_membership_stays_all_and_excludes_noncurrent_data() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-keep", "Kept").await;
    insert_meeting(&pool, "m-gone", "Gone").await;
    add_transcript(&pool, "t-keep", "m-keep", "needle persisted content").await;
    add_transcript(&pool, "t-gone", "m-gone", "needle deleted content").await;
    for meeting in ["m-keep", "m-gone"] {
        crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, meeting)
            .await
            .unwrap();
    }
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-all", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-all", "m-keep", &["needle persisted"]).await;
    publish_meeting(&pool, "gen-all", "m-gone", &["needle deleted"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;
    sqlx::query("DELETE FROM meetings WHERE id = 'm-gone'")
        .execute(&pool)
        .await
        .unwrap();
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let service = RetrievalService::new(lifecycle);
    let result = service
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(matches!(result.scope.scope, PersistedRetrievalScope::All));
    let meetings: BTreeSet<String> = result
        .candidates
        .iter()
        .map(|candidate| candidate.meeting_id.clone())
        .collect();
    assert_eq!(meetings, BTreeSet::from(["m-keep".to_string()]));
}

/// The Fast hybrid counter increments only for a semantic stage that
/// completed cleanly (zero-hit or with hits) and never for catch-up, fence,
/// or candidate-gate SQL failures.
#[tokio::test]
async fn fast_hybrid_query_counter_counts_only_clean_completions() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-docs", "Docs").await;
    insert_meeting(&pool, "m-target", "Target").await;
    add_transcript(&pool, "t-docs", "m-docs", "needle text").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-docs")
        .await
        .unwrap();
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-count", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-count", "m-docs", &["needle"]).await;
    // The allowed meeting is current (complete coverage) but carries no
    // canonical documents, so its scoped semantic scan is zero-hit.
    publish_meeting(&pool, "gen-count", "m-target", &[]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;
    let index = lifecycle.index_service();

    // Zero-hit success: the scoped semantic scan has no rows for the allowed
    // meeting, completes without any typed failure, and counts.
    let before = index.fast_hybrid_query_count();
    let result = RetrievalService::new(lifecycle.clone())
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::AllowedMeetingIds(vec!["m-target".to_string()]),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(result.semantic_fallback.is_none());
    assert_eq!(index.fast_hybrid_query_count(), before + 1);

    // Catch-up degradation does not count.
    let stale_token = index.mark_stale();
    let result = RetrievalService::new(lifecycle.clone())
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(matches!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::CatchUpTimeout { .. })
    ));
    assert_eq!(index.fast_hybrid_query_count(), before + 1);
    index.restore_stale(stale_token);

    // A generation/model fence failure never counts; that path is proven by
    // generation_change_after_a_scan_cannot_retain_semantic_hits, which
    // asserts the counter stays at its pre-request value.

    // A candidate-gate SQL failure never counts.
    sqlx::query("DROP TABLE search_source_state")
        .execute(&pool)
        .await
        .unwrap();
    let result = RetrievalService::new(lifecycle.clone())
        .retrieve(
            &pool,
            request(
                "needle",
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        result.semantic_fallback,
        Some(SemanticFallbackReason::SemanticScanFailed)
    );
    assert_eq!(index.fast_hybrid_query_count(), before + 1);
    assert!(result.candidates.iter().any(|candidate| candidate
        .provenance
        .iter()
        .any(|provenance| provenance.channel == RetrievalChannel::Lexical)));
}
