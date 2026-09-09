//! Focused Task 3.1 regressions: scope isolation and current membership,
//! variant provenance, title-only lexical behavior, semantic
//! unavailability/fallback, cancellation, bounds, and the no-logging rule.

use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use axum::{extract::State as AxumState, response::IntoResponse, Json};
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use super::contracts::{HybridRetrievalStatus, HybridScope};
use super::model::RetrievalModelError;
use super::service::{
    CoreTermLanguage, LexicalMode, PersistedRetrievalScope, QueryVariantKind, RetrievalChannel,
    RetrievalError, RetrievalLimits, RetrievalPurpose, RetrievalRequest, RetrievalService,
    SemanticFallbackReason,
};
use super::worker::{quantize_int8, DocumentEmbedder, LifecycleConfig, RetrievalLifecycle};
use crate::api::api::{
    execute_hybrid_context, execute_hybrid_search, set_hybrid_publication_gate,
    with_hybrid_request, HybridPublicationGate,
};
use crate::api::chat::{ChatRequestState, ChatRequestSurface};
use crate::database::repositories::retrieval::{
    ModelSpec, ReplacementJob, ReplacementOutcome, RetrievalRepository, StagedDocument,
    VectorEncoding,
};
use crate::mcp::server::{handle_jsonrpc, JsonRpcRequest, McpState};

const MODEL_ID: &str = "test-e5-int8";
const DIMS: usize = 4;
const SCALE: f64 = 1.0 / 127.0;

// -- Harness -----------------------------------------------------------------

pub(crate) async fn migrated_pool() -> SqlitePool {
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

pub(crate) async fn insert_meeting(pool: &SqlitePool, id: &str, title: &str) {
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(title)
        .bind("2026-08-29T00:00:00Z")
        .bind("2026-08-29T00:00:00Z")
        .execute(pool)
        .await
        .unwrap();
}

pub(crate) async fn add_transcript(pool: &SqlitePool, id: &str, meeting_id: &str, text: &str) {
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
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    park_until: StdMutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl ServiceEmbedder {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            fail_queries: StdMutex::new(false),
            entered: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
                    self.cancelled.store(true, Ordering::SeqCst);
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

pub(crate) fn failing_lifecycle() -> RetrievalLifecycle {
    RetrievalLifecycle::new(LifecycleConfig::testing(
        Arc::new(|| false),
        Arc::new(|| Err("simulated bundle unavailability".to_string())),
    ))
}

async fn active_test_retrieval(
    meeting_id: &str,
) -> (SqlitePool, RetrievalLifecycle, Arc<ServiceEmbedder>) {
    let pool = migrated_pool().await;
    insert_meeting(&pool, meeting_id, "Hybrid meeting").await;
    add_transcript(
        &pool,
        &format!("transcript-{meeting_id}"),
        meeting_id,
        "needle text",
    )
    .await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, meeting_id)
        .await
        .unwrap();
    register_test_model(&pool).await;
    let generation = format!("generation-{meeting_id}");
    RetrievalRepository::ensure_generation(&pool, &generation, MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, &generation, meeting_id, &["needle text"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;
    (pool, lifecycle, embedder)
}

fn mcp_state(
    pool: &SqlitePool,
    retrieval: &RetrievalLifecycle,
    chat_requests: &ChatRequestState,
) -> McpState {
    McpState {
        pool: pool.clone(),
        app_data_dir: None,
        client: reqwest::Client::new(),
        retrieval: retrieval.clone(),
        chat_requests: chat_requests.clone(),
    }
}

async fn call_mcp_tool(state: McpState, name: &str, arguments: Value) -> Value {
    let response = handle_jsonrpc(
        AxumState(state),
        Json(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "tools/call".to_string(),
            params: json!({"name": name, "arguments": arguments}),
        }),
    )
    .await
    .into_response();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn assert_mcp_error(response: &Value, message: &str) {
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(response["result"]["content"][0]["text"], message);
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
async fn terminal_scope_revalidation_checks_current_existence_and_membership() {
    let pool = migrated_pool().await;
    insert_folder(&pool, "folder-a", "A", None).await;
    insert_folder(&pool, "folder-b", "B", None).await;
    insert_meeting(&pool, "m-a", "A").await;
    insert_meeting(&pool, "m-b", "B").await;
    set_meeting_folder(&pool, "m-a", Some("folder-a")).await;
    set_meeting_folder(&pool, "m-b", Some("folder-b")).await;
    let service = RetrievalService::new(failing_lifecycle());
    let cancel = CancellationToken::new();
    let ids = vec!["m-a".to_string(), "m-b".to_string(), "missing".to_string()];

    assert_eq!(
        service
            .revalidate_ids_in_scope(&pool, &PersistedRetrievalScope::All, &ids, &cancel,)
            .await
            .unwrap(),
        ["m-a".to_string(), "m-b".to_string()]
    );
    assert_eq!(
        service
            .revalidate_ids_in_scope(
                &pool,
                &PersistedRetrievalScope::Folder("folder-a".to_string()),
                &ids,
                &cancel,
            )
            .await
            .unwrap(),
        ["m-a".to_string()]
    );

    sqlx::query("DELETE FROM meetings WHERE id = 'm-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        service
            .revalidate_ids_in_scope(&pool, &PersistedRetrievalScope::All, &ids, &cancel,)
            .await
            .unwrap(),
        ["m-b".to_string()]
    );
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

#[tokio::test]
async fn broad_allowed_ids_retrieval_adds_one_lexical_candidate_per_meeting() {
    let pool = migrated_pool().await;
    for (meeting_id, text) in [
        ("m-a", "alpha target evidence"),
        ("m-b", "beta background evidence"),
        ("m-c", "gamma background evidence"),
    ] {
        insert_meeting(&pool, meeting_id, meeting_id).await;
        add_transcript(&pool, &format!("t-{meeting_id}"), meeting_id, text).await;
        crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, meeting_id)
            .await
            .unwrap();
    }

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let ranked = service
        .retrieve_ranked_with_broad_coverage(
            &pool,
            request(
                "alpha",
                PersistedRetrievalScope::AllowedMeetingIds(vec![
                    "m-a".to_string(),
                    "m-b".to_string(),
                    "m-c".to_string(),
                    "ghost".to_string(),
                ]),
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            ),
        )
        .await
        .unwrap();

    assert_eq!(
        ranked.semantic_fallback,
        Some(SemanticFallbackReason::NoActiveGeneration)
    );
    let meetings: BTreeSet<String> = ranked
        .ranking
        .evidence
        .iter()
        .map(|entry| entry.evidence.meeting_id.clone())
        .collect();
    assert_eq!(
        meetings,
        BTreeSet::from(["m-a".to_string(), "m-b".to_string(), "m-c".to_string()])
    );
    for meeting_id in ["m-b", "m-c"] {
        assert!(ranked.ranking.evidence.iter().any(|entry| {
            entry.evidence.meeting_id == meeting_id
                && entry.evidence.provenance.iter().any(|provenance| {
                    provenance.channel == RetrievalChannel::Lexical
                        && provenance.rank == super::index::MAX_QUERY_LIMIT
                })
        }));
    }
    assert!(ranked
        .ranking
        .evidence
        .iter()
        .all(|entry| entry.evidence.meeting_id != "ghost"));
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
async fn search_and_context_purposes_use_the_shared_service() {
    let pool = migrated_pool().await;
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    for purpose in [RetrievalPurpose::Search, RetrievalPurpose::Context] {
        let mut retrieval = request(
            "needle",
            PersistedRetrievalScope::All,
            RetrievalLimits::default(),
            CoreTermLanguage::English,
            None,
        );
        retrieval.purpose = purpose;
        let result = service.retrieve(&pool, retrieval).await.unwrap();
        assert!(result.candidates.is_empty());
    }
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
    insert_folder(&pool, "title-folder", "Title Folder", None).await;
    insert_meeting(&pool, "m-title", "Chaves de Acesso Rotation").await;
    set_meeting_folder(&pool, "m-title", Some("title-folder")).await;
    add_transcript(&pool, "t-title", "m-title", "unrelated content words only").await;
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-title", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-title", "m-title", &["unrelated body"]).await;

    // Semantic unavailable: the title channel must stand alone.
    let lexical_only = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let mut title_request = request(
        "chaves de acesso",
        PersistedRetrievalScope::All,
        RetrievalLimits::default(),
        CoreTermLanguage::Unknown,
        None,
    );
    title_request.purpose = RetrievalPurpose::Search;
    let result = lexical_only.retrieve(&pool, title_request).await.unwrap();
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

    let request_state = ChatRequestState::new();
    let response = execute_hybrid_search(
        &pool,
        failing_lifecycle(),
        &request_state,
        ChatRequestSurface::Sidebar,
        "title-search".to_string(),
        "chaves de acesso".to_string(),
        HybridScope::Folder {
            folder_id: "title-folder".to_string(),
        },
        Some(50),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    assert_eq!(
        response
            .results
            .iter()
            .map(|result| result.meeting_id.as_str())
            .collect::<Vec<_>>(),
        ["m-title"]
    );
    assert_eq!(response.results[0].sources[0].source_kind, "title");
    assert_eq!(response.results[0].provenance[0].channel, "title");

    // Semantic active: the title channel still returns the meeting.
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;
    let hybrid = RetrievalService::new(lifecycle);
    let mut title_request = request(
        "chaves de acesso",
        PersistedRetrievalScope::All,
        RetrievalLimits::default(),
        CoreTermLanguage::Unknown,
        None,
    );
    title_request.purpose = RetrievalPurpose::Search;
    let result = hybrid.retrieve(&pool, title_request).await.unwrap();
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

#[tokio::test]
async fn mixed_title_and_content_search_preserves_independent_provenance() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-mixed", "Chaves de Acesso Rotation").await;
    add_transcript(&pool, "t-mixed", "m-mixed", "chaves de acesso decision").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "m-mixed")
        .await
        .unwrap();

    let response = execute_hybrid_search(
        &pool,
        failing_lifecycle(),
        &ChatRequestState::new(),
        ChatRequestSurface::Sidebar,
        "mixed-title-content".to_string(),
        "chaves de acesso".to_string(),
        HybridScope::All {},
        Some(50),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    let result = response
        .results
        .iter()
        .find(|result| result.meeting_id == "m-mixed")
        .expect("mixed title and content result");
    let title_source = result
        .sources
        .iter()
        .find(|source| source.source_kind == "title")
        .expect("title metadata source");
    assert_eq!(title_source.evidence_ids, vec!["title:m-mixed"]);
    let content_source = result
        .sources
        .iter()
        .find(|source| source.source_kind == "transcript")
        .expect("transcript source");
    let content_id = content_source
        .evidence_ids
        .first()
        .expect("transcript evidence identity");
    assert!(result
        .retained_evidence_ids
        .iter()
        .any(|evidence_id| evidence_id == "title:m-mixed"));
    assert!(result
        .retained_evidence_ids
        .iter()
        .any(|evidence_id| evidence_id == content_id));

    let title_provenance = result
        .provenance
        .iter()
        .find(|provenance| provenance.evidence_id == "title:m-mixed")
        .expect("title provenance");
    assert_eq!(title_provenance.channel, "title");
    assert_eq!(title_provenance.channel_rank, 1);
    let content_provenance = result
        .provenance
        .iter()
        .filter(|provenance| provenance.evidence_id == *content_id)
        .collect::<Vec<_>>();
    assert!(content_provenance
        .iter()
        .all(|provenance| provenance.channel == "lexical"));
    let variants = content_provenance
        .iter()
        .map(|provenance| provenance.variant.as_str())
        .collect::<BTreeSet<_>>();
    assert!(variants.contains("original"));
    assert!(variants.contains("core_terms"));
}

#[tokio::test]
async fn title_channel_runs_for_every_purpose() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-common", "What we decided").await;
    let service = RetrievalService::new(failing_lifecycle());

    for purpose in [
        RetrievalPurpose::Chat,
        RetrievalPurpose::Search,
        RetrievalPurpose::Context,
    ] {
        let mut purpose_request = request(
            "what",
            PersistedRetrievalScope::All,
            RetrievalLimits::default(),
            CoreTermLanguage::Unknown,
            None,
        );
        purpose_request.purpose = purpose;
        let result = service.retrieve(&pool, purpose_request).await.unwrap();
        assert!(
            result
                .candidates
                .iter()
                .any(|candidate| candidate.meeting_id == "m-common"
                    && candidate.source_kind == "title"),
            "the indexed title lookup must run for {purpose:?}, not only Search"
        );
    }
}

#[tokio::test]
async fn unknown_language_search_titles_need_every_distinct_core_term() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-exact", "Retention policy review").await;
    insert_meeting(&pool, "m-partial", "What we shipped").await;
    let service = RetrievalService::new(failing_lifecycle());

    let title_hits = |result: &crate::retrieval::RetrievalResult| {
        result
            .candidates
            .iter()
            .filter(|candidate| candidate.source_kind == "title")
            .map(|candidate| candidate.meeting_id.clone())
            .collect::<Vec<_>>()
    };
    let search = |query: &'static str| {
        let service = &service;
        let pool = &pool;
        async move {
            let mut request = request(
                query,
                PersistedRetrievalScope::All,
                RetrievalLimits::default(),
                CoreTermLanguage::Unknown,
                None,
            );
            request.purpose = RetrievalPurpose::Search;
            service.retrieve(pool, request).await.unwrap()
        }
    };

    // Every distinct term present: an exact authoritative title match.
    assert_eq!(
        title_hits(&search("retention policy").await),
        vec!["m-exact".to_string()]
    );
    // A shared function word alone must not title-match without a stopword
    // list; that noise would otherwise outrank semantic evidence.
    assert!(title_hits(&search("what happened to retention").await).is_empty());
    // A repeated query token still matches: the all-core-term gate uses the
    // DISTINCT count, which a repeated token cannot inflate.
    assert_eq!(
        title_hits(&search("retention retention").await),
        vec!["m-exact".to_string()]
    );
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
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;

    let state = ChatRequestState::new();
    let service = RetrievalService::new(lifecycle);
    let handle = tokio::spawn({
        let pool = pool.clone();
        let task_state = state.clone();
        async move {
            with_hybrid_request(
                &task_state,
                ChatRequestSurface::Sidebar,
                "hybrid-running-id".to_string(),
                Duration::from_secs(30),
                move |token| async move {
                    service
                        .retrieve(
                            &pool,
                            request(
                                "needle",
                                PersistedRetrievalScope::All,
                                RetrievalLimits::default(),
                                CoreTermLanguage::English,
                                Some(token.as_ref().clone()),
                            ),
                        )
                        .await
                        .map_err(|error| error.to_string())
                },
            )
            .await
        }
    });
    wait_until(async || embedder.entered.load(Ordering::SeqCst)).await;
    assert!(state.cancel_request(ChatRequestSurface::Sidebar, Some("hybrid-running-id")));
    drop(sender);
    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("cancelled preparation must not hang")
        .unwrap();
    assert!(matches!(
        result,
        Err(error) if error == "Hybrid request was cancelled or superseded"
    ));
    assert_eq!(state.request_count(), 0);
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

#[tokio::test]
async fn hybrid_request_id_cancellation_reaches_queued_retrieval() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-hybrid-queued", "Queued").await;
    add_transcript(&pool, "t-hybrid-queued", "m-hybrid-queued", "needle text").await;
    register_test_model(&pool).await;
    RetrievalRepository::ensure_generation(&pool, "gen-hybrid-queued", MODEL_ID)
        .await
        .unwrap();
    publish_meeting(&pool, "gen-hybrid-queued", "m-hybrid-queued", &["needle"]).await;
    let embedder = ServiceEmbedder::new();
    let lifecycle = query_lifecycle(&embedder);
    install_snapshot(&pool, &lifecycle, MODEL_ID).await;
    let held = lifecycle
        .scheduler()
        .enqueue_interactive()
        .unwrap()
        .wait_for_permit()
        .await
        .unwrap();
    let scheduler = lifecycle.scheduler();
    let state = ChatRequestState::new();
    let service = RetrievalService::new(lifecycle);
    let task = tokio::spawn({
        let task_pool = pool.clone();
        let task_state = state.clone();
        async move {
            with_hybrid_request(
                &task_state,
                ChatRequestSurface::Sidebar,
                "hybrid-queued-id".to_string(),
                Duration::from_secs(30),
                move |token| async move {
                    service
                        .retrieve(
                            &task_pool,
                            request(
                                "needle",
                                PersistedRetrievalScope::All,
                                RetrievalLimits::default(),
                                CoreTermLanguage::English,
                                Some(token.as_ref().clone()),
                            ),
                        )
                        .await
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                },
            )
            .await
        }
    });
    wait_until(async || scheduler.queued_interactive() == 1).await;
    assert!(state.cancel_request(ChatRequestSurface::Sidebar, Some("hybrid-queued-id")));
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("cancelled hybrid request must not wait for the permit")
        .unwrap();
    assert_eq!(
        result,
        Err("Hybrid request was cancelled or superseded".to_string())
    );
    assert_eq!(scheduler.queued_interactive(), 0);
    assert_eq!(state.request_count(), 0);
    drop(held);
}

#[tokio::test]
async fn hybrid_tauri_deletion_after_terminal_recheck_suppresses_search_and_context() {
    for (meeting_id, context) in [("m-tauri-search", false), ("m-tauri-context", true)] {
        let pool = migrated_pool().await;
        insert_meeting(&pool, meeting_id, "Hybrid meeting").await;
        add_transcript(
            &pool,
            &format!("transcript-{meeting_id}"),
            meeting_id,
            "needle text",
        )
        .await;
        crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, meeting_id)
            .await
            .unwrap();
        let request_state = ChatRequestState::new();
        let request_id = format!("r2-tauri-{}", if context { "context" } else { "search" });
        let gate = HybridPublicationGate::new();
        set_hybrid_publication_gate(&request_id, Some(gate.clone()));
        let task_pool = pool.clone();
        let task_state = request_state.clone();
        let task_request_id = request_id.clone();
        let task = tokio::spawn(async move {
            let result = if context {
                execute_hybrid_context(
                    &task_pool,
                    failing_lifecycle(),
                    &task_state,
                    ChatRequestSurface::Sidebar,
                    task_request_id,
                    "needle".to_string(),
                    HybridScope::All {},
                    Some(1_000),
                    Duration::from_secs(30),
                )
                .await
                .map(|_| ())
            } else {
                execute_hybrid_search(
                    &task_pool,
                    failing_lifecycle(),
                    &task_state,
                    ChatRequestSurface::Sidebar,
                    task_request_id,
                    "needle".to_string(),
                    HybridScope::All {},
                    Some(1),
                    Duration::from_secs(30),
                )
                .await
                .map(|_| ())
            };
            result
        });
        gate.wait().await;
        let invalidated = request_state.clone();
        let deleted = crate::database::repositories::meeting::MeetingsRepository::delete_meeting(
            &pool,
            meeting_id,
            |deleted_meeting_id| {
                invalidated.invalidate_meeting(deleted_meeting_id);
            },
        )
        .await
        .unwrap();
        assert!(deleted);
        gate.release();
        let result = task.await.unwrap();
        set_hybrid_publication_gate(&request_id, None);
        assert_eq!(
            result,
            Err("Hybrid request was cancelled or superseded".to_string())
        );
        assert_eq!(request_state.request_count(), 0);
        let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    }
}

#[tokio::test]
async fn hybrid_mcp_tools_execute_through_jsonrpc_with_fallback_and_errors() {
    let pool = migrated_pool().await;
    for index in 1..=6 {
        let meeting_id = format!("mcp-jsonrpc-{index}");
        insert_meeting(&pool, &meeting_id, "Hybrid meeting").await;
        add_transcript(
            &pool,
            &format!("{meeting_id}-transcript"),
            &meeting_id,
            "needle text",
        )
        .await;
        crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, &meeting_id)
            .await
            .unwrap();
    }
    let chat_requests = ChatRequestState::new();
    let state = mcp_state(&pool, &failing_lifecycle(), &chat_requests);

    let search = call_mcp_tool(
        state.clone(),
        "search_meetings_hybrid_v1",
        json!({"query": "needle", "scope": {"kind": "all"}, "limit": 6}),
    )
    .await;
    assert_eq!(search["jsonrpc"], "2.0");
    assert_eq!(search["id"], 1);
    assert_eq!(search["result"]["isError"], false);
    let search_body: Value =
        serde_json::from_str(search["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(search_body["version"], "v1");
    assert_eq!(search_body["retrievalStatus"], "lexical_fallback");
    assert_eq!(search_body["scope"]["kind"], "all");
    assert_eq!(search_body["results"].as_array().unwrap().len(), 6);
    assert_eq!(
        search_body["results"][0]["provenance"][0]["evidenceId"],
        search_body["results"][0]["sources"][0]["evidenceIds"][0]
    );

    let context = call_mcp_tool(
        state.clone(),
        "build_context_hybrid_v1",
        json!({"query": "needle", "scope": {"kind": "all"}, "max_chars": 10_000}),
    )
    .await;
    assert_eq!(context["result"]["isError"], false);
    let context_body: Value =
        serde_json::from_str(context["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(context_body["version"], "v1");
    assert_eq!(context_body["retrievalStatus"], "lexical_fallback");
    assert!(context_body["context"].as_str().unwrap().contains("needle"));
    assert_eq!(context_body["sources"].as_array().unwrap().len(), 5);

    let scope_error = call_mcp_tool(
        state.clone(),
        "search_meetings_hybrid_v1",
        json!({"query": "needle", "scope": {"kind": "all", "meetingId": "mcp-jsonrpc-1"}}),
    )
    .await;
    assert_mcp_error(&scope_error, "Invalid hybrid scope");
    let limit_error = call_mcp_tool(
        state,
        "search_meetings_hybrid_v1",
        json!({"query": "needle", "scope": {"kind": "all"}, "limit": 51}),
    )
    .await;
    assert_mcp_error(&limit_error, "Invalid hybrid result limit");
}

#[tokio::test]
async fn hybrid_mcp_jsonrpc_deadline_cancels_queued_retrieval_and_reclaims_capacity() {
    let (pool, lifecycle, _) = active_test_retrieval("mcp-queued-timeout").await;
    let held = lifecycle
        .scheduler()
        .enqueue_interactive()
        .unwrap()
        .wait_for_permit()
        .await
        .unwrap();
    let scheduler = lifecycle.scheduler();
    let chat_requests = ChatRequestState::new();
    let state = mcp_state(&pool, &lifecycle, &chat_requests);
    let task = tokio::spawn(call_mcp_tool(
        state,
        "search_meetings_hybrid_v1",
        json!({"query": "needle", "scope": {"kind": "all"}, "limit": 1}),
    ));
    for _ in 0..1_000 {
        if scheduler.queued_interactive() == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(scheduler.queued_interactive(), 1);
    tokio::time::sleep(Duration::from_secs(11)).await;
    let response = task.await.unwrap();
    assert_mcp_error(&response, "Hybrid request timed out");
    for _ in 0..1_000 {
        if scheduler.queued_interactive() == 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(scheduler.queued_interactive(), 0);
    assert_eq!(chat_requests.request_count(), 0);
    drop(held);
    let request_id = "mcp-queued-reclaimed";
    let token = chat_requests
        .try_claim_request(ChatRequestSurface::Mcp, request_id)
        .expect("timeout must reclaim MCP ownership");
    assert!(chat_requests.clear_if_owner(ChatRequestSurface::Mcp, request_id, &token));
}

#[tokio::test]
async fn hybrid_mcp_jsonrpc_deadline_cancels_running_retrieval_and_reclaims_capacity() {
    let (pool, lifecycle, embedder) = active_test_retrieval("mcp-running-timeout").await;
    let (release_tx, receiver) = std::sync::mpsc::channel();
    *embedder.park_until.lock().unwrap() = Some(receiver);
    let chat_requests = ChatRequestState::new();
    let state = mcp_state(&pool, &lifecycle, &chat_requests);
    let task = tokio::spawn(call_mcp_tool(
        state,
        "build_context_hybrid_v1",
        json!({"query": "needle", "scope": {"kind": "all"}, "max_chars": 1_000}),
    ));
    for _ in 0..1_000 {
        if embedder.entered.load(Ordering::SeqCst) {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(embedder.entered.load(Ordering::SeqCst));
    tokio::time::sleep(Duration::from_secs(11)).await;
    let response = task.await.unwrap();
    assert_mcp_error(&response, "Hybrid request timed out");
    std::thread::sleep(Duration::from_millis(100));
    assert!(embedder.cancelled.load(Ordering::SeqCst));
    assert_eq!(chat_requests.request_count(), 0);
    let lease = lifecycle
        .scheduler()
        .enqueue_interactive()
        .unwrap()
        .wait_for_permit()
        .await
        .unwrap();
    drop(lease);
    drop(release_tx);
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

// -- Authoritative indexed title lookup (HR-5.R6) --------------------------------------

/// Seeds the beyond-the-scan-cap corpus: 10,000 fillers plus two exact-title
/// matches whose ids sort past the retired 10,000-row budget; one lives in a
/// folder, one does not.
async fn seed_beyond_cap_corpus(pool: &SqlitePool) {
    sqlx::query(
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < ?)
         INSERT INTO meetings (id, title, created_at, updated_at)
         SELECT 'filler-' || printf('%05d', n), 'Filler titulo', '2026-08-29T00:00:00Z', '2026-08-29T00:00:00Z' FROM seq",
    )
    .bind(10_000i64)
    .execute(pool)
    .await
    .unwrap();
    insert_folder(pool, "fold", "Folder", None).await;
    insert_meeting(pool, "zz-target", "Chaves de Acesso Rotation").await;
    set_meeting_folder(pool, "zz-target", Some("fold")).await;
    insert_meeting(pool, "aa-outside", "Chaves de Acesso Rotation").await;
}

#[tokio::test]
async fn title_lookup_production_routes_find_matches_beyond_the_scan_cap_and_fence_scope() {
    let pool = migrated_pool().await;
    seed_beyond_cap_corpus(&pool).await;

    // Service level: the indexed lookup has no row cap.
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
    let title_ids = result
        .candidates
        .iter()
        .filter(|candidate| candidate.source_kind == "title")
        .map(|candidate| candidate.meeting_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        title_ids,
        vec!["aa-outside".to_string(), "zz-target".to_string()]
    );
    let title_hit = result
        .candidates
        .iter()
        .find(|candidate| candidate.meeting_id == "zz-target")
        .expect("the indexed title lookup has no row cap");
    assert!(title_hit
        .provenance
        .iter()
        .any(|provenance| provenance.channel == RetrievalChannel::Title));
    assert!(result
        .candidates
        .iter()
        .all(|candidate| candidate.source_kind == "title"));

    // Tauri Search route: the exact title matches surface with title
    // provenance, and the deterministic response order is (rank, meeting id)
    // - the identical titles tie, so meeting id ascending decides.
    let response = execute_hybrid_search(
        &pool,
        failing_lifecycle(),
        &ChatRequestState::new(),
        ChatRequestSurface::Sidebar,
        "title-search-beyond-cap".to_string(),
        "chaves de acesso".to_string(),
        HybridScope::All {},
        Some(50),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    assert_eq!(
        response
            .results
            .iter()
            .map(|result| result.meeting_id.as_str())
            .collect::<Vec<_>>(),
        ["aa-outside", "zz-target"]
    );
    assert!(response.results[0]
        .provenance
        .iter()
        .any(|provenance| provenance.channel == "title"));

    // Scope fencing on the Tauri route: a folder scope returns only the
    // in-folder match; a meeting or allowed-ID scope can never admit another
    // meeting's title.
    let folder_response = execute_hybrid_search(
        &pool,
        failing_lifecycle(),
        &ChatRequestState::new(),
        ChatRequestSurface::Sidebar,
        "title-search-folder".to_string(),
        "chaves de acesso".to_string(),
        HybridScope::Folder {
            folder_id: "fold".to_string(),
        },
        Some(50),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    assert_eq!(
        folder_response
            .results
            .iter()
            .map(|result| result.meeting_id.as_str())
            .collect::<Vec<_>>(),
        ["zz-target"]
    );
    for scope in [
        HybridScope::Meeting {
            meeting_id: "filler-00001".to_string(),
        },
        HybridScope::AllowedMeetingIds {
            meeting_ids: vec!["filler-00001".to_string()],
        },
    ] {
        let fenced = execute_hybrid_search(
            &pool,
            failing_lifecycle(),
            &ChatRequestState::new(),
            ChatRequestSurface::Sidebar,
            "title-search-fenced".to_string(),
            "chaves de acesso".to_string(),
            scope,
            Some(50),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        assert!(
            fenced
                .results
                .iter()
                .all(|result| result.meeting_id != "zz-target"),
            "scope fencing must exclude the out-of-scope title match"
        );
    }

    // MCP route through the full JSON-RPC tool dispatch.
    let response = call_mcp_tool(
        mcp_state(&pool, &failing_lifecycle(), &ChatRequestState::new()),
        "search_meetings_hybrid_v1",
        json!({
            "query": "chaves de acesso",
            "scope": {"kind": "all"},
            "limit": 50
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], false);
    let payload: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["results"][0]["meetingId"], "aa-outside");
    assert!(payload["results"][0]["provenance"]
        .as_array()
        .unwrap()
        .iter()
        .any(|provenance| provenance["channel"] == "title"));
}

/// Scope and output-cap regression, not a database-work bound. An FTS5
/// EXPLAIN plan without a temp b-tree does not prove bounded visited work;
/// the diagnostic below separately counts actual SQLite VM progress.
#[tokio::test]
async fn title_lookup_preserves_scope_and_candidate_cap_for_common_terms() {
    let pool = migrated_pool().await;
    // 10,000 out-of-scope titles share the common term `standup`; a handful
    // of in-scope meetings share it too, so a scope-constrained lookup must
    // never enumerate the global match set.
    sqlx::query(
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < ?)
         INSERT INTO meetings (id, title, created_at, updated_at)
         SELECT 'filler-' || printf('%05d', n), 'Standup meeting notes ' || n, '2026-08-29T00:00:00Z', '2026-08-29T00:00:00Z' FROM seq",
    )
    .bind(10_000i64)
    .execute(&pool)
    .await
    .unwrap();
    insert_folder(&pool, "other", "Other", None).await;
    sqlx::query("UPDATE meetings SET folder_id = 'other' WHERE id LIKE 'filler-%'")
        .execute(&pool)
        .await
        .unwrap();
    insert_folder(&pool, "fold", "Folder", None).await;
    insert_folder(&pool, "child", "Child", Some("fold")).await;
    for index in 0..30 {
        let id = format!("in-fold-{index:02}");
        insert_meeting(&pool, &id, "Standup meeting plan").await;
        set_meeting_folder(
            &pool,
            &id,
            Some(if index % 2 == 0 { "fold" } else { "child" }),
        )
        .await;
    }
    insert_meeting(&pool, "zz-agenda", "Standup meeting agenda").await;

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let title_hits = |result: &crate::retrieval::RetrievalResult| {
        result
            .candidates
            .iter()
            .filter(|candidate| candidate.source_kind == "title")
            .map(|candidate| candidate.meeting_id.clone())
            .collect::<Vec<_>>()
    };
    let search = |query: &'static str, scope: PersistedRetrievalScope| {
        let service = &service;
        let pool = &pool;
        async move {
            let mut request = request(
                query,
                scope,
                RetrievalLimits::default(),
                CoreTermLanguage::English,
                None,
            );
            request.purpose = RetrievalPurpose::Search;
            service.retrieve(pool, request).await.unwrap()
        }
    };

    // All scope: the common term matches every title; the response is the
    // bounded per-variant cap.
    let result = search("standup", PersistedRetrievalScope::All).await;
    assert_eq!(
        title_hits(&result).len(),
        crate::retrieval::service::HYBRID_CANDIDATES_PER_VARIANT,
        "the common term matches every title; the response is the bounded per-variant cap"
    );

    // Folder scope (direct + descendant folders): the scope term lives INSIDE
    // the FTS match, so the intersection is the 30 in-scope meetings, never
    // the 10,000 out-of-scope matches.
    let result = search(
        "standup",
        PersistedRetrievalScope::Folder("fold".to_string()),
    )
    .await;
    let hits = title_hits(&result);
    assert_eq!(
        hits.len(),
        30,
        "the folder-scoped intersection is the in-scope set"
    );
    assert!(hits.iter().all(|id| id.starts_with("in-fold-")));

    // The same exact ranked plan shape for every scope: scope lives inside
    // MATCH and SQLite sorts the matching set by explicit score and stable
    // string ID before applying the output cap. This intentionally has linear
    // matching-set work; the result cap is not a database-work bound.
    let folder_keys = vec![
        crate::retrieval::service::title_scope_token('f', "fold"),
        crate::retrieval::service::title_scope_token('f', "child"),
    ];
    let all_query =
        crate::retrieval::service::title_match_query(&["standup".to_string()], &[]).unwrap();
    let folder_query =
        crate::retrieval::service::title_match_query(&["standup".to_string()], &folder_keys)
            .unwrap();
    let meeting_query = crate::retrieval::service::title_match_query(
        &["standup".to_string()],
        &[crate::retrieval::service::title_scope_token(
            'm',
            "in-fold-00",
        )],
    )
    .unwrap();
    let allowed_query = crate::retrieval::service::title_match_query(
        &["standup".to_string()],
        &[
            crate::retrieval::service::title_scope_token('m', "in-fold-00"),
            crate::retrieval::service::title_scope_token('m', "zz-agenda"),
        ],
    )
    .unwrap();
    for (label, match_query) in [
        ("all", &all_query),
        ("folder", &folder_query),
        ("meeting", &meeting_query),
        ("allowed", &allowed_query),
    ] {
        let plan = sqlx::query_as::<_, (i64, i64, i64, String)>(&format!(
            "EXPLAIN QUERY PLAN SELECT m.id, m.title, bm25(retrieval_title_fts, 1.0, 1.0, 0.0) \
             FROM retrieval_title_fts \
             JOIN meetings m ON m.id = retrieval_title_fts.meeting_id \
             WHERE retrieval_title_fts MATCH ? \
             ORDER BY bm25(retrieval_title_fts, 1.0, 1.0, 0.0) ASC, m.id COLLATE BINARY ASC \
             LIMIT ?"
        ))
        .bind(match_query)
        .bind(100i64)
        .fetch_all(&pool)
        .await
        .unwrap();
        let details: Vec<&str> = plan.iter().map(|(.., detail)| detail.as_str()).collect();
        assert!(
            details
                .iter()
                .any(|detail| detail.contains("VIRTUAL TABLE INDEX") && detail.contains("M")),
            "the {label} lookup must evaluate the scoped match inside FTS5: {details:?}"
        );
        assert!(
            details
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE")),
            "the {label} lookup must sort exact BM25/ID top-k in SQLite: {details:?}"
        );
        assert!(
            !details.iter().any(|detail| detail.contains("folder_scope")),
            "the {label} lookup must keep the scope inside the FTS match, not a CTE: {details:?}"
        );
    }

    // Meeting and allowed-ID scopes are exact: only the requested meetings.
    let result = search(
        "standup",
        PersistedRetrievalScope::Meeting("in-fold-00".to_string()),
    )
    .await;
    assert_eq!(title_hits(&result), vec!["in-fold-00".to_string()]);
    let result = search(
        "standup",
        PersistedRetrievalScope::AllowedMeetingIds(vec!["zz-agenda".to_string()]),
    )
    .await;
    assert_eq!(title_hits(&result), vec!["zz-agenda".to_string()]);

    // The all-distinct-core-term gate still holds under the ranked form.
    let result = search("standup agenda", PersistedRetrievalScope::All).await;
    assert_eq!(title_hits(&result), vec!["zz-agenda".to_string()]);
}

/// Explicit, synthetic SQL diagnostic; it is not corpus/release acceptance
/// evidence. Count SQLite VM progress, selected rows, and elapsed time for
/// the current exhaustive pages and two exact single-statement alternatives.
/// Keep the 250k fixture out of ordinary unit runs.
#[tokio::test]
#[ignore = "explicit 1k/10k/250k title SQL work diagnostic"]
async fn title_lookup_measures_common_in_scope_sql_work() {
    use futures_util::TryStreamExt;
    use sqlx::Connection;
    use std::collections::BinaryHeap;
    use std::sync::atomic::AtomicU64;
    use std::time::Instant;

    #[derive(Debug, PartialEq)]
    struct Hit(f64, String);
    impl Eq for Hit {}
    impl Ord for Hit {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.0
                .total_cmp(&other.0)
                .then_with(|| self.1.cmp(&other.1))
        }
    }
    impl PartialOrd for Hit {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    fn retain(heap: &mut BinaryHeap<Hit>, hit: Hit) {
        if heap.len() < 50 {
            heap.push(hit);
        } else if heap.peek().is_some_and(|worst| hit < *worst) {
            heap.pop();
            heap.push(hit);
        }
    }

    let mut connection = sqlx::SqliteConnection::connect("sqlite::memory:")
        .await
        .unwrap();
    // Only the tables needed by the production title migration and query;
    // no model, transcript, semantic/evaluation fixture, or user data.
    sqlx::raw_sql(
        "CREATE TABLE meetings (id TEXT PRIMARY KEY, title TEXT NOT NULL, folder_id TEXT);
         CREATE TABLE meeting_folders (id TEXT PRIMARY KEY);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/20260908000000_add_retrieval_title_fts.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    let mut previous = 0i64;
    let mut previous_steps = [0; 3];
    for count in [1_000i64, 10_000, 250_000] {
        sqlx::query(
            "WITH RECURSIVE seq(n) AS (SELECT ? UNION ALL SELECT n + 1 FROM seq WHERE n < ?)
             INSERT INTO meetings (id, title)
             SELECT printf('common-%06d', n), 'Standup meeting notes ' || n FROM seq",
        )
        .bind(previous + 1)
        .bind(count)
        .execute(&mut connection)
        .await
        .unwrap();
        previous = count;
        let query = super::service::title_match_query(&["standup".to_string()], &[]).unwrap();
        let mut expected = None;
        for (algorithm, name) in ["pages", "stream", "sql_top_k"].into_iter().enumerate() {
            let steps = Arc::new(AtomicU64::new(0));
            let observed = steps.clone();
            connection
                .lock_handle()
                .await
                .unwrap()
                .set_progress_handler(100, move || {
                    observed.fetch_add(100, Ordering::Relaxed);
                    true
                });
            let started = Instant::now();
            let mut heap = BinaryHeap::new();
            let mut returned_rows = 0usize;
            let mut statements = 0usize;
            if algorithm == 0 {
                let mut cursor = 0i64;
                loop {
                    let rows: Vec<(String, String, f64, i64)> = sqlx::query_as(
                        "SELECT m.id, m.title, bm25(retrieval_title_fts, 1.0, 1.0, 0.0),
                         retrieval_title_fts.rowid FROM retrieval_title_fts
                         JOIN meetings m ON m.id = retrieval_title_fts.meeting_id
                         WHERE retrieval_title_fts MATCH ? AND retrieval_title_fts.rowid > ? LIMIT ?",
                    )
                    .bind(&query)
                    .bind(cursor)
                        .bind(400i64)
                    .fetch_all(&mut connection)
                    .await
                    .unwrap();
                    statements += 1;
                    if rows.is_empty() {
                        break;
                    }
                    returned_rows += rows.len();
                    for (id, _title, score, rowid) in rows {
                        cursor = cursor.max(rowid);
                        retain(&mut heap, Hit(score, id));
                    }
                }
            } else if algorithm == 1 {
                let mut rows = sqlx::query_as::<_, (String, String, f64)>(
                    "SELECT m.id, m.title, bm25(retrieval_title_fts, 1.0, 1.0, 0.0)
                     FROM retrieval_title_fts JOIN meetings m ON m.id = retrieval_title_fts.meeting_id
                     WHERE retrieval_title_fts MATCH ?",
                )
                .bind(&query)
                .fetch(&mut connection);
                statements += 1;
                while let Some((id, _title, score)) = rows.try_next().await.unwrap() {
                    returned_rows += 1;
                    retain(&mut heap, Hit(score, id));
                }
            } else {
                let rows: Vec<(String, String, f64)> = sqlx::query_as(
                    "SELECT m.id, m.title, bm25(retrieval_title_fts, 1.0, 1.0, 0.0) AS score
                     FROM retrieval_title_fts JOIN meetings m ON m.id = retrieval_title_fts.meeting_id
                     WHERE retrieval_title_fts MATCH ?
                     ORDER BY score ASC, m.id COLLATE BINARY ASC LIMIT 50",
                )
                .bind(&query)
                .fetch_all(&mut connection)
                .await
                .unwrap();
                statements += 1;
                returned_rows += rows.len();
                for (id, _title, score) in rows {
                    retain(&mut heap, Hit(score, id));
                }
            }
            let elapsed = started.elapsed();
            connection
                .lock_handle()
                .await
                .unwrap()
                .remove_progress_handler();
            let measured_steps = steps.load(Ordering::Relaxed);
            let result = heap.into_sorted_vec();
            assert_eq!(result.len(), 50);
            if let Some(expected) = &expected {
                assert_eq!(&result, expected, "exact score/ID result parity for {name}");
            } else {
                expected = Some(result);
            }
            assert!(
                measured_steps > previous_steps[algorithm],
                "actual VM work grows for {name}; a capped result is not a work bound"
            );
            previous_steps[algorithm] = measured_steps;
            eprintln!(
                "TITLE_WORK in_scope={count} algorithm={name} vm_steps~={measured_steps} returned_rows={returned_rows} statements={statements} elapsed_ms={}",
                elapsed.as_millis()
            );
        }
    }
    connection.close().await.unwrap();
}

/// A selected folder disappearing between normalization and the title
/// snapshot is an empty constrained scope, never an unscoped MATCH query.
#[tokio::test]
async fn title_lookup_deleted_folder_before_title_snapshot_never_widens_scope() {
    let pool = migrated_pool().await;
    insert_folder(&pool, "selected", "Selected", None).await;
    insert_folder(&pool, "other", "Other", None).await;
    insert_meeting(&pool, "formerly-inside", "Needle meeting").await;
    set_meeting_folder(&pool, "formerly-inside", Some("selected")).await;
    insert_meeting(&pool, "always-outside", "Needle meeting").await;
    set_meeting_folder(&pool, "always-outside", Some("other")).await;
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let (arrived, mut arrival) = tokio::sync::mpsc::unbounded_channel();
    service.arm_title_scope_gate(arrived);
    let mut search_request = request(
        "needle",
        PersistedRetrievalScope::Folder("selected".to_string()),
        RetrievalLimits {
            lexical_per_variant: 50,
            vector_per_variant: 0,
        },
        CoreTermLanguage::Unknown,
        None,
    );
    search_request.purpose = RetrievalPurpose::Search;
    let running_service = service.clone();
    let running_pool = pool.clone();
    let running = tokio::spawn(async move {
        running_service
            .retrieve_ranked(&running_pool, search_request)
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(10), arrival.recv())
        .await
        .unwrap()
        .unwrap();
    sqlx::query("DELETE FROM meeting_folders WHERE id = 'selected'")
        .execute(&pool)
        .await
        .unwrap();
    let folder: Option<String> =
        sqlx::query_scalar("SELECT folder_id FROM meetings WHERE id = 'formerly-inside'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        folder, None,
        "the old member is now outside the deleted scope"
    );
    service.release_title_scope_gate();
    let ranked = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .unwrap()
        .unwrap();
    assert!(ranked.ranking.title_matches.is_empty());
    assert!(ranked.ranking.evidence.is_empty());
    assert!(
        ranked.ranking.meetings.is_empty(),
        "neither old members nor other global matches may enter ranking"
    );
}

/// Public scope IDs support 512 bytes each, including all 100 allowed IDs
/// and a large subtree of long direct-folder IDs. Partition MATCH expressions
/// without changing the global title score/ID cut or widening membership.
#[tokio::test]
async fn title_lookup_long_public_scope_ids_partition_without_losing_global_top_k() {
    use super::service::{
        title_match_queries, title_match_query, title_scope_token, MAX_TITLE_MATCH_QUERY_BYTES,
    };

    fn long_id(prefix: &str, index: usize) -> String {
        let prefix = format!("{prefix}-{index:03}-");
        format!("{prefix}{}", "x".repeat(512 - prefix.len()))
    }
    let pool = migrated_pool().await;
    let root = long_id("root", 0);
    insert_folder(&pool, &root, "Selected root", None).await;
    insert_folder(&pool, "outside", "Outside", None).await;
    let mut ids = Vec::new();
    let mut folder_keys = vec![title_scope_token('f', &root)];
    for index in 0..100 {
        let folder = long_id("folder", index);
        insert_folder(&pool, &folder, "Child", Some(&root)).await;
        folder_keys.push(title_scope_token('f', &folder));
        // Reverse folder/meeting ordering: later folder groups contain
        // smaller stable IDs. Stronger title winners also occur in later
        // allowed-ID groups, so either per-group cutoff is falsifiable.
        let id = long_id("meeting", 99 - index);
        let title = if !(5..95).contains(&index) {
            "Needle access"
        } else {
            "Needle access meeting notes budget plans"
        };
        insert_meeting(&pool, &id, title).await;
        set_meeting_folder(&pool, &id, Some(&folder)).await;
        ids.push(id);
    }
    insert_meeting(&pool, "000-outside", "Needle access needle access").await;
    set_meeting_folder(&pool, "000-outside", Some("outside")).await;
    ids.sort();
    assert!(ids.iter().all(|id| id.len() == 512));
    let core = vec!["needle".to_string(), "access".to_string()];
    let reference_query = title_match_query(&core, &[]).unwrap();
    let reference: Vec<(String, f64)> = sqlx::query_as(
        "SELECT m.id, bm25(retrieval_title_fts, 1.0, 1.0, 0.0) AS score
         FROM retrieval_title_fts JOIN meetings m ON m.id = retrieval_title_fts.meeting_id
         WHERE retrieval_title_fts MATCH ? AND m.id != '000-outside'
         ORDER BY score, m.id",
    )
    .bind(&reference_query)
    .fetch_all(&pool)
    .await
    .unwrap();
    let expected: Vec<String> = reference.iter().take(7).map(|(id, _)| id.clone()).collect();
    assert_eq!(expected, [&ids[..5], &ids[95..97]].concat());

    let meeting_keys: Vec<String> = ids.iter().map(|id| title_scope_token('m', id)).collect();
    for keys in [&folder_keys, &meeting_keys] {
        assert!(keys.iter().all(|key| key.len() == 1025));
        let queries = title_match_queries(&core, keys).unwrap();
        assert!(queries.len() > 1, "maximal identities require partitioning");
        assert!(queries
            .iter()
            .all(|query| query.len() <= MAX_TITLE_MATCH_QUERY_BYTES));
        let duplicates: Vec<String> = keys.iter().chain(keys.iter()).cloned().collect();
        assert_eq!(title_match_queries(&core, &duplicates).unwrap(), queries);
        let mut observed = Vec::new();
        for query in queries {
            let rows: Vec<(String, f64)> = sqlx::query_as(
                "SELECT m.id, bm25(retrieval_title_fts, 1.0, 1.0, 0.0)
                 FROM retrieval_title_fts JOIN meetings m ON m.id = retrieval_title_fts.meeting_id
                 WHERE retrieval_title_fts MATCH ?",
            )
            .bind(query)
            .fetch_all(&pool)
            .await
            .unwrap();
            observed.extend(rows);
        }
        observed.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        assert_eq!(
            observed, reference,
            "disjoint groups preserve every exact title score and ID"
        );
    }

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    for scope in [
        PersistedRetrievalScope::Folder(root.clone()),
        PersistedRetrievalScope::AllowedMeetingIds(ids.clone()),
    ] {
        let result = service
            .retrieve(
                &pool,
                request(
                    "needle access",
                    scope,
                    RetrievalLimits {
                        lexical_per_variant: 7,
                        vector_per_variant: 0,
                    },
                    CoreTermLanguage::Unknown,
                    None,
                ),
            )
            .await
            .unwrap();
        let actual: Vec<String> = result
            .candidates
            .iter()
            .filter(|candidate| candidate.source_kind == "title")
            .map(|candidate| candidate.meeting_id.clone())
            .collect();
        assert_eq!(
            actual, expected,
            "the production channel retains one global top-k"
        );
    }

    // Exercise validation and both public adapters with the supported
    // maximum-length Meeting, Folder and full 100-ID allowed scope.
    for (index, scope) in [
        HybridScope::Meeting {
            meeting_id: ids[99].clone(),
        },
        HybridScope::Folder {
            folder_id: root.clone(),
        },
        HybridScope::AllowedMeetingIds {
            meeting_ids: ids.clone(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let expected_count = if index == 0 { 1 } else { 50 };
        let response = execute_hybrid_search(
            &pool,
            failing_lifecycle(),
            &ChatRequestState::new(),
            ChatRequestSurface::Sidebar,
            format!("title-max-id-{index}"),
            "needle access".to_string(),
            scope.clone(),
            Some(50),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        assert_eq!(response.results.len(), expected_count);
        assert!(response
            .results
            .iter()
            .all(|result| ids.contains(&result.meeting_id)));
        if index == 0 {
            assert_eq!(response.results[0].meeting_id, ids[99]);
        }
        let response = call_mcp_tool(
            mcp_state(&pool, &failing_lifecycle(), &ChatRequestState::new()),
            "search_meetings_hybrid_v1",
            json!({ "query": "needle access", "scope": scope, "limit": 50 }),
        )
        .await;
        assert_eq!(response["result"]["isError"], false);
        let payload: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        let results = payload["results"].as_array().unwrap();
        assert_eq!(results.len(), expected_count);
        assert!(results.iter().all(|result| ids
            .iter()
            .any(|id| id == result["meetingId"].as_str().unwrap())));
        if index == 0 {
            assert_eq!(results[0]["meetingId"], ids[99]);
        }
    }
}

/// Retain the original title-match proof through ranking and fence it
/// against authoritative metadata before Search or Chat/Context hydration.
/// A title matching a Deep planner query must not be reinterpreted using
/// the different core terms of the original question.
#[tokio::test]
async fn title_hydration_fences_renames_deletions_and_moves_after_ranking() {
    use super::hydration::{hydrate_context, hydrate_search_context};

    let pool = migrated_pool().await;
    insert_folder(&pool, "inside", "Inside", None).await;
    insert_folder(&pool, "outside", "Outside", None).await;
    let selected_title = "Ação de Acesso";
    for id in ["keep", "rename-away", "rename-match", "moved", "deleted"] {
        insert_meeting(&pool, id, selected_title).await;
        set_meeting_folder(&pool, id, Some("inside")).await;
        add_transcript(&pool, &format!("t-{id}"), id, "Unrelated budget decision").await;
    }
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let mut ranked = service
        .retrieve_ranked(
            &pool,
            request(
                "ação acesso",
                PersistedRetrievalScope::Folder("inside".to_string()),
                RetrievalLimits {
                    lexical_per_variant: 50,
                    vector_per_variant: 0,
                },
                CoreTermLanguage::Portuguese,
                None,
            ),
        )
        .await
        .unwrap();
    assert!(ranked.ranking.evidence.is_empty());
    assert_eq!(ranked.ranking.title_matches.len(), 5);
    assert!(ranked
        .ranking
        .title_matches
        .iter()
        .all(|title| title.selected_title == selected_title));
    // Model a title signal supplied by a different Deep planner query slot.
    // The exact selected title already proved eligibility for that query.
    ranked.ranking.core_terms = vec!["different".to_string()];
    for title in &mut ranked.ranking.title_matches {
        for provenance in &mut title.provenance {
            provenance.query_slot = 1;
        }
    }
    sqlx::raw_sql(
        "UPDATE meetings SET title = 'Unrelated meeting' WHERE id = 'rename-away';
         UPDATE meetings SET title = 'Ação de Acesso revisada' WHERE id = 'rename-match';
         UPDATE meetings SET folder_id = 'outside' WHERE id = 'moved';
         DELETE FROM meetings WHERE id = 'deleted';",
    )
    .execute(&pool)
    .await
    .unwrap();

    let context = hydrate_context(&pool, &ranked, 10_000, None).await.unwrap();
    assert_eq!(
        context
            .meetings
            .iter()
            .map(|m| m.meeting_id.as_str())
            .collect::<Vec<_>>(),
        ["keep"],
        "Chat/Context retain only the unchanged title-selected meeting"
    );
    assert!(context
        .sources
        .iter()
        .all(|source| source.source_kind != "title"));
    assert!(context.markdown.contains("Unrelated budget decision"));

    let search = hydrate_search_context(&pool, &ranked, 10_000, 50, None)
        .await
        .unwrap();
    assert_eq!(
        search
            .meetings
            .iter()
            .map(|m| m.meeting_id.as_str())
            .collect::<Vec<_>>(),
        ["keep"],
        "Search also drops renamed/moved/deleted title identities"
    );
    assert!(search
        .retained_evidence_ids
        .contains(&"title:keep".to_string()));
    assert_eq!(search.sources[0].meeting_title, selected_title);
}

#[tokio::test]
async fn title_hydration_drops_renamed_signal_but_keeps_matching_content() {
    use super::hydration::hydrate_search_context;

    let pool = migrated_pool().await;
    insert_meeting(&pool, "mixed", "Ação de Acesso").await;
    add_transcript(&pool, "t-mixed", "mixed", "Ação e acesso foram discutidos").await;
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "mixed")
        .await
        .unwrap();
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let mut search_request = request(
        "ação acesso",
        PersistedRetrievalScope::Meeting("mixed".to_string()),
        RetrievalLimits {
            lexical_per_variant: 50,
            vector_per_variant: 0,
        },
        CoreTermLanguage::Portuguese,
        None,
    );
    search_request.purpose = RetrievalPurpose::Search;
    let ranked = service
        .retrieve_ranked(&pool, search_request)
        .await
        .unwrap();
    assert_eq!(ranked.ranking.title_matches.len(), 1);
    assert!(!ranked.ranking.evidence.is_empty());
    sqlx::query("UPDATE meetings SET title = 'Different title' WHERE id = 'mixed'")
        .execute(&pool)
        .await
        .unwrap();
    let search = hydrate_search_context(&pool, &ranked, 10_000, 50, None)
        .await
        .unwrap();
    assert_eq!(search.meetings.len(), 1, "valid content remains eligible");
    assert!(!search
        .retained_evidence_ids
        .contains(&"title:mixed".to_string()));
    assert!(search
        .sources
        .iter()
        .all(|source| source.source_kind != "title"));
    assert!(search
        .sources
        .iter()
        .all(|source| source.meeting_title == "Different title"));
    assert!(!search.retained_evidence_ids.is_empty());
}

/// Folder expansion and exact title top-k belong to one read snapshot. A
/// writer can commit after the title query without changing its selected rows.
/// The following request must then see the committed state, rather than
/// retaining that snapshot.
#[tokio::test]
async fn title_lookup_uses_one_snapshot_across_rename_delete_and_folder_move() {
    let directory = tempfile::tempdir().unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(directory.path().join("title-snapshot.sqlite"))
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    insert_folder(&pool, "inside", "Inside", None).await;
    insert_folder(&pool, "outside", "Outside", None).await;
    sqlx::query(
        "WITH RECURSIVE seq(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM seq WHERE n < 429)
         INSERT INTO meetings (id, title, folder_id, created_at, updated_at)
         SELECT printf('snapshot-%05d', n), 'Needle meeting notes', 'inside',
                '2026-09-08T00:00:00Z', '2026-09-08T00:00:00Z' FROM seq",
    )
    .execute(&pool)
    .await
    .unwrap();
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let (arrived, mut arrival) = tokio::sync::mpsc::unbounded_channel();
    service.arm_title_query_gate(arrived);
    let search_request = request(
        "needle",
        PersistedRetrievalScope::Folder("inside".to_string()),
        RetrievalLimits {
            lexical_per_variant: 5,
            vector_per_variant: 0,
        },
        CoreTermLanguage::Unknown,
        None,
    );
    let running_pool = pool.clone();
    let running_service = service.clone();
    let running_request = search_request.clone();
    let running = tokio::spawn(async move {
        running_service
            .retrieve(&running_pool, running_request)
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(10), arrival.recv())
        .await
        .unwrap()
        .unwrap();

    // The matching rename reinserts the mirror row. The query snapshot must
    // retain its old exact top-k, while the next request sees this commit.
    sqlx::raw_sql(
        "UPDATE meetings SET title = 'Needle meeting plans' WHERE id = 'snapshot-00000';
         DELETE FROM meetings WHERE id = 'snapshot-00001';
         UPDATE meetings SET title = 'Unrelated meeting notes' WHERE id = 'snapshot-00002';
         UPDATE meetings SET folder_id = 'outside' WHERE id = 'snapshot-00003';",
    )
    .execute(&pool)
    .await
    .unwrap();
    service.release_title_query_gate();
    let snapshot = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .unwrap()
        .unwrap();
    let titles = |result: &crate::retrieval::RetrievalResult| {
        result
            .candidates
            .iter()
            .filter(|candidate| candidate.source_kind == "title")
            .map(|candidate| {
                (
                    candidate.meeting_id.clone(),
                    candidate.meeting_title.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        titles(&snapshot),
        (0..5)
            .map(|index| (
                format!("snapshot-{index:05}"),
                "Needle meeting notes".to_string()
            ))
            .collect::<Vec<_>>(),
        "all five distinct hits must come from the original read snapshot"
    );
    let current = service.retrieve(&pool, search_request).await.unwrap();
    assert_eq!(
        titles(&current),
        vec![
            (
                "snapshot-00000".to_string(),
                "Needle meeting plans".to_string()
            ),
            (
                "snapshot-00004".to_string(),
                "Needle meeting notes".to_string()
            ),
            (
                "snapshot-00005".to_string(),
                "Needle meeting notes".to_string()
            ),
            (
                "snapshot-00006".to_string(),
                "Needle meeting notes".to_string()
            ),
            (
                "snapshot-00007".to_string(),
                "Needle meeting notes".to_string()
            ),
        ],
        "a new request observes the rename and excludes the deleted, nonmatching, and moved rows"
    );
    pool.close().await;
}

/// Equal-rank selection is deterministic from stable meeting identity: ten
/// identically-shaped titles (equal bm25 scores) inserted in reverse produce
/// the five lexicographically smallest meeting IDs in the bounded top-k, and
/// a second database with the same meetings inserted in forward order
/// produces the identical selection - the meeting ID itself is the
/// tie-break, so insertion history is irrelevant and no ID can fail.
#[tokio::test]
async fn title_equal_rank_selection_follows_the_stable_identity_key() {
    let pool = migrated_pool().await;
    let mut tied: Vec<String> = (0..10).map(|index| format!("tie-{index:02}")).collect();
    for id in tied.iter().rev() {
        insert_meeting(&pool, id, "Identical title words").await;
    }
    tied.sort();

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let mut request1 = request(
        "identical title",
        PersistedRetrievalScope::All,
        RetrievalLimits {
            lexical_per_variant: 5,
            vector_per_variant: 0,
        },
        CoreTermLanguage::Unknown,
        None,
    );
    request1.purpose = RetrievalPurpose::Search;
    let result = service.retrieve(&pool, request1).await.unwrap();
    let hits: Vec<String> = result
        .candidates
        .iter()
        .filter(|candidate| candidate.source_kind == "title")
        .map(|candidate| candidate.meeting_id.clone())
        .collect();
    assert_eq!(hits.len(), 5);
    assert_eq!(
        hits,
        tied[..5].to_vec(),
        "the equal-rank cut follows the stable meeting identity, not insertion history"
    );
    let mut sorted_hits = hits.clone();
    sorted_hits.sort();
    assert_eq!(
        hits, sorted_hits,
        "the response order is meeting id ascending among equal ranks"
    );

    // Identical input, identical output: a fresh database with the same
    // meetings inserted in forward order yields the same selected set.
    let pool2 = migrated_pool().await;
    for id in tied.iter() {
        insert_meeting(&pool2, id, "Identical title words").await;
    }
    let mut request2 = request(
        "identical title",
        PersistedRetrievalScope::All,
        RetrievalLimits {
            lexical_per_variant: 5,
            vector_per_variant: 0,
        },
        CoreTermLanguage::Unknown,
        None,
    );
    request2.purpose = RetrievalPurpose::Search;
    let service2 = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let result = service2.retrieve(&pool2, request2).await.unwrap();
    let hits2: Vec<String> = result
        .candidates
        .iter()
        .filter(|candidate| candidate.source_kind == "title")
        .map(|candidate| candidate.meeting_id.clone())
        .collect();
    assert_eq!(
        hits, hits2,
        "equal-rank selection is independent of insertion history"
    );
}

/// Impossible inputs fail closed: a scope key that would exceed the approved
/// encoded size is rejected before any FTS query, and a folder subtree with
/// more than the approved number of direct folders fails closed.
#[tokio::test]
async fn title_scope_and_match_inputs_fail_closed_on_impossible_inputs() {
    let pool = migrated_pool().await;
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let search = |scope: PersistedRetrievalScope| {
        let service = &service;
        let pool = &pool;
        async move {
            let mut request = request(
                "needle",
                scope,
                RetrievalLimits::default(),
                CoreTermLanguage::Unknown,
                None,
            );
            request.purpose = RetrievalPurpose::Search;
            service.retrieve(pool, request).await
        }
    };

    // An oversized meeting ID produces an oversized encoded scope key, which
    // is rejected before any FTS query.
    let oversized = "x".repeat(2_000);
    insert_meeting(&pool, &oversized, "Oversized").await;
    let error = search(PersistedRetrievalScope::Meeting(oversized))
        .await
        .unwrap_err();
    assert!(matches!(error, RetrievalError::InvalidScope(_)));

    // A folder subtree with more than the approved number of direct folders
    // fails closed once a meeting inside it is looked up (an empty subtree has
    // no work to bound and legitimately returns nothing).
    let mut last = "root-fold".to_string();
    insert_folder(&pool, &last, "Root", None).await;
    for index in 0..crate::retrieval::service::MAX_TITLE_SCOPE_KEYS {
        let id = format!("sub-fold-{index}");
        insert_folder(&pool, &id, &id, Some(&last)).await;
        last = id;
    }
    insert_meeting(&pool, "deep-meeting", "Needle title").await;
    set_meeting_folder(&pool, "deep-meeting", Some(&last)).await;
    let error = search(PersistedRetrievalScope::Folder("root-fold".to_string()))
        .await
        .unwrap_err();
    assert!(matches!(error, RetrievalError::InvalidScope(_)));
}

/// Arbitrary schema-valid meeting IDs are collision-free by construction
/// (the ID itself is the tie-break; no derived key exists): adversarial
/// ordinary IDs that would collide under a case-folding or polynomial hash
/// ('Aa' and 'BB' both fold to 2112 under a 31-polynomial) coexist, are all
/// findable, and rank deterministically by (score, meeting ID).
#[tokio::test]
async fn title_arbitrary_ids_are_collision_free_and_rank_deterministically() {
    let pool = migrated_pool().await;
    for (id, title) in [
        ("Aa", "Alpha Needle"),
        ("BB", "Alpha Needle"),
        ("\u{3}\u{4}", "Alpha Needle"),
        ("a", "Alpha Needle"),
        ("zz-ordinary", "Other meeting"),
    ] {
        insert_meeting(&pool, id, title).await;
    }
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let mut request = request(
        "alpha needle",
        PersistedRetrievalScope::All,
        RetrievalLimits::default(),
        CoreTermLanguage::Unknown,
        None,
    );
    request.purpose = RetrievalPurpose::Search;
    let result = service.retrieve(&pool, request).await.unwrap();
    let hits: Vec<String> = result
        .candidates
        .iter()
        .filter(|candidate| candidate.source_kind == "title")
        .map(|candidate| candidate.meeting_id.clone())
        .collect();
    // Identical titles -> identical scores -> meeting ID ascending; every
    // adversarial ID is present and nothing failed insertion.
    assert_eq!(
        hits,
        vec![
            "\u{3}\u{4}".to_string(),
            "Aa".to_string(),
            "BB".to_string(),
            "a".to_string(),
        ]
    );
}
/// Context production route: the title channel runs on Context and the test
/// is falsifiable - the in-folder target's summary and transcript share NO
/// token with the query, so only the title channel can select it, its
/// authoritative content is cited, the title itself is never a citation (the
/// frozen Task 3.3 boundary), and the allowed-ID scope fences other matches.
#[tokio::test]
async fn hybrid_context_tauri_route_runs_the_title_lookup_beyond_the_scan_cap() {
    let pool = migrated_pool().await;
    seed_beyond_cap_corpus(&pool).await;
    // FALSIFIABLE: the target's content shares no token with the query, so
    // the lexical channel cannot find it - only the title channel can.
    add_transcript(
        &pool,
        "t-target",
        "zz-target",
        "fully unrelated budget decision",
    )
    .await;
    sqlx::query(
        "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result)
         VALUES ('zz-target', 'summary', 'completed', '2026-09-02T00:00:00Z', '2026-09-02T00:00:00Z', '{\"markdown\":\"Unrelated rollout summary\"}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    crate::database::repositories::fts::FtsRepository::refresh_meeting(&pool, "zz-target")
        .await
        .unwrap();
    let response = execute_hybrid_context(
        &pool,
        failing_lifecycle(),
        &ChatRequestState::new(),
        ChatRequestSurface::Sidebar,
        "title-context-beyond-cap".to_string(),
        "chaves de acesso".to_string(),
        HybridScope::Folder {
            folder_id: "fold".to_string(),
        },
        Some(32_000),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    assert_eq!(
        response.retrieval_status,
        HybridRetrievalStatus::LexicalFallback
    );
    assert!(
        response
            .sources
            .iter()
            .any(|source| source.meeting_id == "zz-target"),
        "the title-selected meeting must reach the context"
    );
    assert!(
        response.context.contains("Unrelated rollout summary"),
        "the title-selected meeting's authoritative content is cited"
    );
    // Titles are selection signals on Context, never citations.
    assert!(response
        .sources
        .iter()
        .all(|source| source.source_kind != "title"));
    // Folder fencing: the out-of-folder title match never reaches Context.
    assert!(response
        .sources
        .iter()
        .all(|source| source.meeting_id != "aa-outside"));

    // The allowed-ID scope route stays within its allow-list.
    let allowed = execute_hybrid_context(
        &pool,
        failing_lifecycle(),
        &ChatRequestState::new(),
        ChatRequestSurface::Sidebar,
        "title-context-allowed".to_string(),
        "chaves de acesso".to_string(),
        HybridScope::AllowedMeetingIds {
            meeting_ids: vec!["zz-target".to_string()],
        },
        Some(32_000),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    assert!(allowed
        .sources
        .iter()
        .all(|source| source.meeting_id == "zz-target"));
    assert!(allowed
        .sources
        .iter()
        .all(|source| source.source_kind != "title"));
}

/// Scope tokens carry ZERO bm25 weight, so their document frequencies cannot
/// influence title relevance or order: a meeting in a rare child folder (its
/// folder token's document frequency is 1) must NOT outrank an
/// identically-titled meeting in a populous sibling folder merely because of
/// that rarity - the bounded top-k follows (title score, meeting ID), which
/// would fail under default bm25 weights.
#[tokio::test]
async fn title_scope_token_document_frequencies_do_not_change_ranking() {
    let pool = migrated_pool().await;
    insert_folder(&pool, "parent", "Parent", None).await;
    insert_folder(&pool, "common", "Common", Some("parent")).await;
    insert_folder(&pool, "rare", "Rare", Some("parent")).await;
    // The populous sibling: 40 identically-titled meetings.
    for index in 0..40 {
        let id = format!("c-{index:02}");
        insert_meeting(&pool, &id, "Alpha Beta").await;
        set_meeting_folder(&pool, &id, Some("common")).await;
    }
    // The rare folder's only meeting, identical title.
    insert_meeting(&pool, "bbb", "Alpha Beta").await;
    set_meeting_folder(&pool, "bbb", Some("rare")).await;
    // An id that sorts FIRST among the tied set, in the populous folder.
    insert_meeting(&pool, "aaa", "Alpha Beta").await;
    set_meeting_folder(&pool, "aaa", Some("common")).await;

    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let mut request = request(
        "alpha beta",
        PersistedRetrievalScope::Folder("parent".to_string()),
        RetrievalLimits::default(),
        CoreTermLanguage::Unknown,
        None,
    );
    request.purpose = RetrievalPurpose::Search;
    let result = service.retrieve(&pool, request).await.unwrap();
    let hits: Vec<String> = result
        .candidates
        .iter()
        .filter(|candidate| candidate.source_kind == "title")
        .map(|candidate| candidate.meeting_id.clone())
        .collect();
    assert_eq!(hits.len(), 42);
    assert_eq!(
        hits.first(),
        Some(&"aaa".to_string()),
        "title relevance (and the identity tie-break) decides the order, not folder-token rarity"
    );
    // The rare-folder meeting ranks by its title alone: identical title ->
    // identical score -> meeting-ID ascending places it second, ahead of the
    // populous folder's meetings. Under default bm25 weights its rare folder
    // token would inflate the score and rank it FIRST.
    assert_eq!(hits.get(1), Some(&"bbb".to_string()));
}

/// Exercises the mirror contract end to end: diacritic-folded matching,
/// repeated query terms, transactional maintenance on title update and
/// delete, folder-move scope intersection, and per-scope authorization.
#[tokio::test]
async fn title_mirror_lookup_tracks_updates_deletes_and_scope() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "m-fold", "Comunicação Reunião").await;
    insert_meeting(&pool, "m-out", "Comunicação Reunião").await;
    insert_meeting(&pool, "m-solo", "Comunicação Solo").await;
    insert_meeting(&pool, "m-del", "Comunicação Apagada").await;
    insert_folder(&pool, "fold", "Folder", None).await;
    set_meeting_folder(&pool, "m-fold", Some("fold")).await;
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));

    let search = |query: &'static str, scope: PersistedRetrievalScope| {
        let service = &service;
        let pool = &pool;
        async move {
            let mut request = request(
                query,
                scope,
                RetrievalLimits::default(),
                CoreTermLanguage::Unknown,
                None,
            );
            request.purpose = RetrievalPurpose::Search;
            let result = service.retrieve(pool, request).await.unwrap();
            result
                .candidates
                .iter()
                .filter(|candidate| candidate.source_kind == "title")
                .map(|candidate| candidate.meeting_id.clone())
                .collect::<Vec<_>>()
        }
    };

    // Diacritics fold both ways and ordering is meeting id ascending.
    assert_eq!(
        search("comunicação reunião", PersistedRetrievalScope::All).await,
        vec!["m-fold".to_string(), "m-out".to_string()]
    );
    // A repeated query token is deduplicated, not a second required term.
    assert_eq!(
        search("reunião reunião", PersistedRetrievalScope::All).await,
        vec!["m-fold".to_string(), "m-out".to_string()]
    );
    // Folder scope intersects current membership; a meeting scope can only
    // ever return its own meeting, even when others share the exact title.
    assert_eq!(
        search(
            "comunicação",
            PersistedRetrievalScope::Folder("fold".to_string())
        )
        .await,
        vec!["m-fold".to_string()]
    );
    assert_eq!(
        search(
            "comunicação",
            PersistedRetrievalScope::Meeting("m-out".to_string())
        )
        .await,
        vec!["m-out".to_string()]
    );

    // A title update moves the mirror: the old words stop matching and the
    // new ones start, in the same transaction as the meetings row, and the
    // update leaves exactly one live mirror row for the meeting.
    sqlx::query("UPDATE meetings SET title = 'Comunicação Editada' WHERE id = 'm-fold'")
        .execute(&pool)
        .await
        .unwrap();
    let mirror_rows: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM retrieval_title_fts WHERE meeting_id = 'm-fold'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(mirror_rows.0, 1, "exactly one live mirror row per meeting");
    assert_eq!(
        search("comunicação reunião", PersistedRetrievalScope::All).await,
        vec!["m-out".to_string()]
    );
    assert_eq!(
        search("comunicação editada", PersistedRetrievalScope::All).await,
        vec!["m-fold".to_string()]
    );

    // A folder move changes current membership, not the mirror.
    set_meeting_folder(&pool, "m-out", Some("fold")).await;
    assert_eq!(
        search(
            "comunicação",
            PersistedRetrievalScope::Folder("fold".to_string())
        )
        .await,
        vec!["m-fold".to_string(), "m-out".to_string()]
    );

    // A deletion removes the meeting from every scope.
    sqlx::query("DELETE FROM meetings WHERE id = 'm-del'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        search("comunicação", PersistedRetrievalScope::All).await,
        vec![
            "m-fold".to_string(),
            "m-out".to_string(),
            "m-solo".to_string()
        ]
    );
    let mirror_rows: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM retrieval_title_fts WHERE meeting_id = 'm-del'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(mirror_rows.0, 0, "a deletion removes every mirror row");

    // Recreating the same meeting id leaves exactly one live mirror row.
    insert_meeting(&pool, "m-del", "Comunicação Retornada").await;
    let mirror_rows: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM retrieval_title_fts WHERE meeting_id = 'm-del'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(mirror_rows.0, 1, "exactly one live mirror row per meeting");
    assert_eq!(
        search("comunicação retornada", PersistedRetrievalScope::All).await,
        vec!["m-del".to_string()]
    );
}

async fn bounded_folder_title_ids(reverse_insert: bool) -> Vec<String> {
    let pool = migrated_pool().await;
    insert_folder(&pool, "title-root", "Title Root", None).await;
    // Distinct token counts give the five matches strictly distinct bm25
    // ranks (shorter titles rank better), so the expected order is
    // insert-order independent by construction rather than by tie luck.
    let mut meetings: Vec<(String, String)> = vec![
        ("match-1".to_string(), "Alpha Beta".to_string()),
        ("match-2".to_string(), "Alpha Beta c".to_string()),
        ("match-3".to_string(), "Alpha Beta c d".to_string()),
        ("match-4".to_string(), "Alpha Beta c d e".to_string()),
        ("match-5".to_string(), "Alpha Beta c d e f".to_string()),
        ("match-partial".to_string(), "Alpha".to_string()),
        ("match-zero".to_string(), "Gamma".to_string()),
    ];
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

/// Exact SQL score-and-ID top-k returns the same per-variant cap regardless
/// of insertion/page order; partial and zero-overlap titles are excluded by
/// the same all-core-term gate.
#[tokio::test]
async fn folder_title_sql_top_k_is_exact_and_insertion_order_independent() {
    let expected = vec![
        "match-1".to_string(),
        "match-2".to_string(),
        "match-3".to_string(),
    ];
    assert_eq!(bounded_folder_title_ids(false).await, expected);
    assert_eq!(bounded_folder_title_ids(true).await, expected);
}

/// Cancellation is observed at the safe boundary immediately after exact
/// title SQL returns and before its snapshot can publish candidates.
#[tokio::test]
async fn title_lookup_cancels_after_exact_sql_before_snapshot_commit() {
    let pool = migrated_pool().await;
    insert_meeting(&pool, "cancel-title", "Needle meeting").await;
    let service = RetrievalService::new(query_lifecycle(&ServiceEmbedder::new()));
    let (arrived, mut arrival) = tokio::sync::mpsc::unbounded_channel();
    service.arm_title_query_gate(arrived);
    let cancel = CancellationToken::new();
    let running_service = service.clone();
    let running_pool = pool.clone();
    let running_cancel = cancel.clone();
    let running = tokio::spawn(async move {
        running_service
            .retrieve(
                &running_pool,
                request(
                    "needle",
                    PersistedRetrievalScope::All,
                    RetrievalLimits {
                        lexical_per_variant: 10,
                        vector_per_variant: 0,
                    },
                    CoreTermLanguage::English,
                    Some(running_cancel),
                ),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), arrival.recv())
        .await
        .unwrap()
        .unwrap();
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("cancellation at the title SQL boundary must resolve")
        .unwrap();
    assert!(matches!(result, Err(RetrievalError::Cancelled)));
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
