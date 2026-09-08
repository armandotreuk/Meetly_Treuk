//! Offline installed-resource diagnostic for Task 5.4b.
//!
//! The executable entry point accepts no resource argument and cannot use a
//! developer model override. It resolves exactly the Tauri resource tree and
//! invokes the existing verifier, CPU model runtime, worker, ranking service,
//! and authoritative hydration. The only database is a migrated in-memory
//! synthetic fixture. There is no application construction or network client.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::model_bundle::{parse_manifest, ModelBundleManifest};
use crate::retrieval::model::{self, RetrievalModelError, RetrievalModels};
use crate::retrieval::worker::{LifecycleConfig, RetrievalLifecycle};
use crate::retrieval::{
    hydrate_context, CoreTermLanguage, PersistedRetrievalScope, RetrievalChannel, RetrievalLimits,
    RetrievalPurpose, RetrievalRequest, RetrievalService,
};

const MANIFEST: &str = include_str!("../resources/retrieval/model-bundle.manifest.json");
// Only Sprint 1's recorded model reference expectations are consumed. This
// fixture is not an evaluation corpus and this diagnostic makes no quality claim.
const REFERENCE: &str = include_str!("../tests/fixtures/model_bundle_manifest.json");
const REFERENCE_QUERY: &str =
    "query: quais os dias de comunicacao por whatsapp para o fluxo de retencao?";
const REFERENCE_IDS: &[u32] = &[
    0, 41, 1294, 12, 53633, 362, 14850, 8, 34060, 123142, 196, 125072, 121, 36, 85679, 31, 8,
    73487, 123142, 32, 2,
];
const HYBRID_TIMEOUT: Duration = Duration::from_secs(90);
const FIXTURE_QUERY: &str = "When is the observatory telescope calibration scheduled?";
const FIXTURE_ROWS: &[(&str, &str, &str)] = &[
    (
        "smoke-observatory",
        "Observatory planning",
        "The observatory telescope calibration is scheduled for Tuesday at nine in the morning.",
    ),
    (
        "smoke-garden",
        "Garden planning",
        "The gardeners will plant yellow flowers beside the fountain on Saturday afternoon.",
    ),
];

/// Stable process outcomes. Never serialize the underlying error: model/SQL
/// errors may contain local paths or source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
enum Failure {
    Harness = 20,
    ResourceResolution = 21,
    ManifestMissing = 22,
    ManifestInvalid = 23,
    ArtifactMissing = 24,
    ArtifactIntegrity = 25,
    Tokenizer = 26,
    Embedding = 27,
    Reranker = 28,
    Hybrid = 29,
}

impl Failure {
    fn stage(self) -> &'static str {
        match self {
            Self::Harness => "harness",
            Self::ResourceResolution
            | Self::ManifestMissing
            | Self::ManifestInvalid
            | Self::ArtifactMissing
            | Self::ArtifactIntegrity => "resources",
            Self::Tokenizer => "tokenizer",
            Self::Embedding => "embedding",
            Self::Reranker => "reranker",
            Self::Hybrid => "hybrid",
        }
    }

    fn status(self) -> &'static str {
        match self {
            Self::Harness => "runtime_unavailable",
            Self::ResourceResolution => "resolution_rejected",
            Self::ManifestMissing => "manifest_missing",
            Self::ManifestInvalid => "manifest_invalid",
            Self::ArtifactMissing => "artifact_missing",
            Self::ArtifactIntegrity => "integrity_failed",
            Self::Tokenizer => "reference_failed",
            Self::Embedding => "reference_failed",
            Self::Reranker => "reference_failed",
            Self::Hybrid => "pipeline_failed",
        }
    }

    fn report(self) -> String {
        format!(
            "smoke-retrieval: stage={} status={} exit_code={}",
            self.stage(),
            self.status(),
            self as i32
        )
    }
}

/// The complete argv iterator includes argv[0]. A forwarded/deep-link argument
/// containing this flag later in argv never selects a process diagnostic.
pub fn requested(arguments: impl IntoIterator<Item = OsString>) -> bool {
    arguments.into_iter().nth(1).as_deref() == Some(OsStr::new("--smoke-retrieval"))
}

fn resource_bundle(
    resource_dir: &Path,
    development_override_present: bool,
) -> Result<PathBuf, Failure> {
    if development_override_present {
        return Err(Failure::ResourceResolution);
    }
    Ok(model::bundle_dir(resource_dir))
}

/// Called before normal logging, single-instance hooks, GUI, audio, app data,
/// database installation, and MCP/server startup. Windows x64 is the approved
/// installed-smoke target; another platform cannot produce a passing claim.
pub fn run() -> i32 {
    // This process is already committed to a diagnostic exit. Suppress panic
    // payloads from any native/worker path before catching the top-level panic;
    // normal application launches never install this hook.
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(installed_run).unwrap_or(Err(Failure::Harness));
    // Keep the sanitized hook until main exits: a timed-out blocking worker
    // can finish (or panic) after the bounded runtime teardown returns.
    match result {
        Ok(report) => {
            println!("{}", report.line());
            0
        }
        Err(failure) => {
            eprintln!("{}", failure.report());
            failure as i32
        }
    }
}

fn installed_run() -> Result<Report, Failure> {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Err(Failure::ResourceResolution);
    }
    let overridden = ["MEETLY_RAG_BUNDLE_DIR", "MEETLY_RAG_MODELS_DIR"]
        .iter()
        .any(|name| std::env::var_os(name).is_some());
    if overridden {
        return Err(Failure::ResourceResolution);
    }
    let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    // Identical resolver used by PathResolver::resource_dir in normal startup.
    // On Windows it returns the current executable's parent, independent of
    // working directory, source tree, cache, and app-data directories.
    let resources =
        tauri::utils::platform::resource_dir(context.package_info(), &tauri::utils::Env::default())
            .map_err(|_| Failure::ResourceResolution)?;
    let root = resource_bundle(&resources, overridden)?;
    diagnose(&resources, &root)
}

struct Report {
    manifest_digest: String,
    embeddings: usize,
    pairs: usize,
    sources: usize,
}

impl Report {
    fn line(&self) -> String {
        format!(
            "smoke-retrieval: stage=complete status=passed bundle=meetily-retrieval-bundle-1 manifest_sha256={} dimensions=768 embeddings={} pairs={} sources={} finite=true exit_code=0",
            self.manifest_digest, self.embeddings, self.pairs, self.sources
        )
    }
}

fn verify_resources(resources: &Path, root: &Path) -> Result<String, Failure> {
    let manifest_path = root.join("model-bundle.manifest.json");
    let metadata = std::fs::metadata(&manifest_path).map_err(|_| Failure::ManifestMissing)?;
    // The installed authority is byte-identical to the checked-in manifest;
    // reject size drift before allocating or parsing untrusted installed bytes.
    if !metadata.is_file() || metadata.len() != MANIFEST.len() as u64 {
        return Err(Failure::ManifestInvalid);
    }
    let json = std::fs::read_to_string(manifest_path).map_err(|_| Failure::ManifestInvalid)?;
    let manifest = parse_manifest(&json).map_err(|_| Failure::ManifestInvalid)?;
    if json != MANIFEST {
        return Err(Failure::ManifestInvalid);
    }
    let canonical_resources = resources
        .canonicalize()
        .map_err(|_| Failure::ResourceResolution)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|_| Failure::ResourceResolution)?;
    if !canonical_root.starts_with(&canonical_resources)
        || !root
            .join("model-bundle.manifest.json")
            .canonicalize()
            .map_err(|_| Failure::ResourceResolution)?
            .starts_with(&canonical_root)
    {
        return Err(Failure::ResourceResolution);
    }
    for relative in managed_paths(&manifest) {
        let path = root.join(relative);
        if !path.is_file() {
            return Err(Failure::ArtifactMissing);
        }
        // Junction/symlink redirects outside the package are not packaged
        // resources even when they happen to point to a valid developer cache.
        if !path
            .canonicalize()
            .map_err(|_| Failure::ArtifactMissing)?
            .starts_with(&canonical_root)
        {
            return Err(Failure::ResourceResolution);
        }
    }
    manifest
        .verify_artifacts(root)
        .map_err(|_| Failure::ArtifactIntegrity)?;
    Ok(format!("{:x}", Sha256::digest(json.as_bytes())))
}

fn managed_paths(manifest: &ModelBundleManifest) -> impl Iterator<Item = &str> {
    manifest
        .artifact_entries()
        .map(|artifact| artifact.path.as_str())
        .chain(
            manifest
                .licenses
                .iter()
                .map(|license| license.path.as_str()),
        )
}

fn model_failure(error: RetrievalModelError) -> Failure {
    match error {
        RetrievalModelError::ManifestUnavailable { .. } => Failure::ManifestMissing,
        RetrievalModelError::ManifestInvalid(_) | RetrievalModelError::ManifestUnsupported(_) => {
            Failure::ManifestInvalid
        }
        RetrievalModelError::ArtifactVerification { .. } => Failure::ArtifactIntegrity,
        RetrievalModelError::TokenizerLoad { .. } => Failure::Tokenizer,
        RetrievalModelError::SessionLoad { role, .. }
        | RetrievalModelError::ContractMismatch { role, .. }
        | RetrievalModelError::Inference { role, .. } => {
            if role == "reranker" {
                Failure::Reranker
            } else {
                Failure::Embedding
            }
        }
        RetrievalModelError::CacheCapacity { .. } | RetrievalModelError::Cancelled => {
            Failure::Harness
        }
    }
}

fn diagnose(resources: &Path, root: &Path) -> Result<Report, Failure> {
    let manifest_digest = verify_resources(resources, root)?;
    let models = model::get_or_load(root).map_err(model_failure)?;
    check_tokenizer(&models)?;
    let fixture: ReferenceFixture =
        serde_json::from_str(REFERENCE).map_err(|_| Failure::Harness)?;
    let embeddings = check_embeddings(&models, &fixture.reference_expectations.embedding)?;
    let pairs = check_reranker(&models, &fixture.reference_expectations.reranker_pairs)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| Failure::Harness)?;
    let sources = runtime.block_on(hybrid_fixture(root));
    // A diagnostic never waits indefinitely for a cancelled blocking worker.
    // The executable exits immediately after this function returns.
    runtime.shutdown_timeout(Duration::from_secs(5));
    Ok(Report {
        manifest_digest,
        embeddings,
        pairs,
        sources: sources?,
    })
}

fn check_tokenizer(models: &RetrievalModels) -> Result<(), Failure> {
    let encoded = models
        .document_tokenizer()
        .encode(REFERENCE_QUERY, true)
        .map_err(|_| Failure::Tokenizer)?;
    if encoded.get_ids() != REFERENCE_IDS
        || encoded.get_attention_mask().iter().any(|mask| *mask != 1)
    {
        return Err(Failure::Tokenizer);
    }
    let long_text = "palavra ".repeat(1200);
    let pair = models
        .document_tokenizer()
        .encode(("pergunta?", long_text.as_str()), true)
        .map_err(|_| Failure::Tokenizer)?;
    if pair.len() != 512 || pair.get_ids().first() != Some(&0) || pair.get_ids().last() != Some(&2)
    {
        return Err(Failure::Tokenizer);
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceFixture {
    reference_expectations: Expectations,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expectations {
    embedding: Vec<EmbeddingReference>,
    reranker_pairs: Vec<RerankerReference>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddingReference {
    label: String,
    text: String,
    norm: f64,
    sample_dims: BTreeMap<String, f64>,
    cosine_with: Vec<CosineReference>,
}
#[derive(Deserialize)]
struct CosineReference {
    label: String,
    cosine: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RerankerReference {
    model_dir: String,
    pairs: Vec<PairReference>,
}
#[derive(Deserialize)]
struct PairReference {
    query: String,
    evidence: String,
    score: f64,
}

fn check_embeddings(
    models: &RetrievalModels,
    references: &[EmbeddingReference],
) -> Result<usize, Failure> {
    if models.dimensions() != 768 || references.len() != 6 {
        return Err(Failure::Embedding);
    }
    let cancel = CancellationToken::new();
    let mut vectors = BTreeMap::new();
    for reference in references {
        let embedded = if let Some(query) = reference.text.strip_prefix("query: ") {
            models.embed_queries_sync(&[query], &cancel)
        } else if let Some(document) = reference.text.strip_prefix("passage: ") {
            models.embed_documents_sync(&[document], &cancel)
        } else {
            return Err(Failure::Embedding);
        }
        .map_err(|_| Failure::Embedding)?;
        if embedded.len() != 1 {
            return Err(Failure::Embedding);
        }
        let vector = embedded.into_iter().next().ok_or(Failure::Embedding)?;
        let norm = embedding_norm(&vector)?;
        if (norm - 1.0).abs() > 0.001 || (norm - reference.norm).abs() > 0.01 {
            return Err(Failure::Embedding);
        }
        for (index, expected) in &reference.sample_dims {
            let actual = vector
                .get(index.parse::<usize>().map_err(|_| Failure::Embedding)?)
                .ok_or(Failure::Embedding)?;
            if (f64::from(*actual) - expected).abs() > 0.06 {
                return Err(Failure::Embedding);
            }
        }
        vectors.insert(reference.label.as_str(), vector);
    }
    let cosine = |left: &str, right: &str| -> Result<f64, Failure> {
        Ok(vectors
            .get(left)
            .ok_or(Failure::Embedding)?
            .iter()
            .zip(vectors.get(right).ok_or(Failure::Embedding)?)
            .map(|(left, right)| f64::from(*left) * f64::from(*right))
            .sum())
    };
    for reference in references {
        for expected in &reference.cosine_with {
            if (cosine(&reference.label, &expected.label)? - expected.cosine).abs() > 0.03 {
                return Err(Failure::Embedding);
            }
        }
    }
    let unrelated = cosine("pt_query", "en_doc")?;
    if cosine("pt_query", "pt_reference_doc")? <= unrelated
        || cosine("pt_query", "en_query")? <= unrelated
    {
        return Err(Failure::Embedding);
    }
    Ok(vectors.len())
}

fn embedding_norm(vector: &[f32]) -> Result<f64, Failure> {
    if vector.len() != 768 || vector.iter().any(|value| !value.is_finite()) {
        return Err(Failure::Embedding);
    }
    Ok(vector
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt())
}

fn check_reranker(
    models: &RetrievalModels,
    references: &[RerankerReference],
) -> Result<usize, Failure> {
    let reference = references
        .iter()
        .find(|reference| reference.model_dir == "mmarco-reranker/model_quint8_avx2.onnx")
        .ok_or(Failure::Reranker)?;
    if reference.pairs.len() != 5 {
        return Err(Failure::Reranker);
    }
    let pairs = reference
        .pairs
        .iter()
        .map(|pair| (pair.query.clone(), pair.evidence.clone()))
        .collect::<Vec<_>>();
    let logits = models
        .rerank_sync(&pairs, &CancellationToken::new())
        .map_err(model_failure)?;
    check_reranker_values(&logits, &reference.pairs)?;
    Ok(logits.len())
}

fn check_reranker_values(logits: &[f32], references: &[PairReference]) -> Result<(), Failure> {
    if logits.len() != 5 || references.len() != 5 || logits.iter().any(|value| !value.is_finite()) {
        return Err(Failure::Reranker);
    }
    for (actual, expected) in logits.iter().zip(references) {
        // The selected runtime returns raw logits; Sprint 1 recorded sigmoid
        // scores. This comparison does not alter the production score contract.
        let sigmoid = 1.0 / (1.0 + (-f64::from(*actual)).exp());
        if (sigmoid - expected.score).abs() > 0.20 {
            return Err(Failure::Reranker);
        }
    }
    if logits[0] <= logits[2] || logits[3] <= logits[4] {
        return Err(Failure::Reranker);
    }
    Ok(())
}

async fn fixture_pool() -> Result<SqlitePool, Failure> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .in_memory(true)
                .foreign_keys(true),
        )
        .await
        .map_err(|_| Failure::Hybrid)?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|_| Failure::Hybrid)?;
    for (id, title, text) in FIXTURE_ROWS {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(title)
            .bind("2026-09-08T00:00:00Z")
            .bind("2026-09-08T00:00:00Z")
            .execute(&pool)
            .await
            .map_err(|_| Failure::Hybrid)?;
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, ?, ?)",
        )
        .bind(format!("segment-{id}"))
        .bind(id)
        .bind(text)
        .bind("09:00")
        .execute(&pool)
        .await
        .map_err(|_| Failure::Hybrid)?;
    }
    Ok(pool)
}

fn fixture_request() -> RetrievalRequest {
    RetrievalRequest {
        original_query: FIXTURE_QUERY.to_string(),
        rewritten_query: None,
        scope: PersistedRetrievalScope::All,
        purpose: RetrievalPurpose::Chat,
        limits: RetrievalLimits {
            lexical_per_variant: 4,
            vector_per_variant: 4,
        },
        core_language: CoreTermLanguage::English,
        cancellation: None,
    }
}

async fn hybrid_fixture(root: &Path) -> Result<usize, Failure> {
    let pool = fixture_pool().await?;
    let lifecycle = RetrievalLifecycle::new(LifecycleConfig::production(Some(root.to_path_buf())));
    lifecycle.attach_database(pool.clone());
    let result = tokio::time::timeout(HYBRID_TIMEOUT, async {
        let index = lifecycle.index_service();
        loop {
            let status = crate::retrieval::index::index_status(&pool, &index, false)
                .await
                .map_err(|_| Failure::Hybrid)?;
            if status.model_load_failure.is_some() || status.failed_meetings > 0 {
                return Err(Failure::Hybrid);
            }
            if status.serving_state == "ready"
                // Each meeting has one selection-only profile and one
                // authoritative transcript window under the approved chunker.
                && status.document_count == 4
                && status.current_meetings == 2
                && status.active_generation_id.is_some()
                && status.canonical_change_id == status.published_change_id
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let ranked = RetrievalService::new(lifecycle.clone())
            .retrieve_ranked(&pool, fixture_request())
            .await
            .map_err(|_| Failure::Hybrid)?;
        if ranked.semantic_fallback.is_some()
            || !ranked.ranking.reranker_used
            || ranked.ranking.rerank_fallback.is_some()
            || ranked.ranking.dedupe_degraded
            || ranked
                .ranking
                .meetings
                .first()
                .map(|meeting| meeting.meeting_id.as_str())
                != Some(FIXTURE_ROWS[0].0)
        {
            #[cfg(test)]
            println!("smoke-retrieval: stage=hybrid status=ranking_contract_failed");
            return Err(Failure::Hybrid);
        }
        let mut semantic = false;
        let mut lexical = false;
        for candidate in &ranked.ranking.evidence {
            if !candidate.fused_score.is_finite()
                || candidate
                    .reranker_score
                    .is_some_and(|score| !score.is_finite())
            {
                return Err(Failure::Hybrid);
            }
            if candidate.evidence.meeting_id == FIXTURE_ROWS[0].0
                && candidate.evidence.source_kind == "transcript"
            {
                // Production deduplication retains lexical source identities
                // as aliases on the matching semantic window. Fusion and
                // hydration intentionally preserve that alias provenance.
                for provenance in candidate.evidence.provenance.iter().chain(
                    candidate
                        .evidence
                        .source_aliases
                        .iter()
                        .flat_map(|alias| alias.provenance.iter()),
                ) {
                    semantic |= provenance.channel == RetrievalChannel::Semantic;
                    lexical |= provenance.channel == RetrievalChannel::Lexical;
                }
            }
        }
        if !semantic || !lexical {
            #[cfg(test)]
            println!("smoke-retrieval: stage=hybrid status=channel_provenance_failed");
            return Err(Failure::Hybrid);
        }
        let hydrated = hydrate_context(&pool, &ranked, 4096, None)
            .await
            .map_err(|_| Failure::Hybrid)?;
        let retained: BTreeSet<_> = hydrated.retained_evidence_ids.iter().collect();
        let published: BTreeSet<_> = hydrated
            .sources
            .iter()
            .flat_map(|source| source.evidence_ids.iter())
            .collect();
        if hydrated.sources.is_empty()
            || retained.is_empty()
            || retained != published
            || !hydrated
                .sources
                .iter()
                .any(|source| source.meeting_id == FIXTURE_ROWS[0].0)
        {
            #[cfg(test)]
            println!("smoke-retrieval: stage=hybrid status=retained_sources_failed");
            return Err(Failure::Hybrid);
        }
        for source in &hydrated.sources {
            let Some((id, _, text)) = FIXTURE_ROWS
                .iter()
                .find(|(id, _, _)| *id == source.meeting_id)
            else {
                return Err(Failure::Hybrid);
            };
            if source.source_kind != "transcript"
                || source.source_start_id.as_deref() != Some(format!("segment-{id}").as_str())
                || source.source_end_id.as_deref() != Some(format!("segment-{id}").as_str())
                || source.snippet != *text
                || !hydrated.markdown.contains(&source.snippet)
                || source.evidence_ids.is_empty()
                || source.evidence_ids.iter().any(|id| !retained.contains(id))
            {
                #[cfg(test)]
                println!("smoke-retrieval: stage=hybrid status=source_contract_failed");
                return Err(Failure::Hybrid);
            }
        }
        Ok(hydrated.sources.len())
    })
    .await
    .unwrap_or(Err(Failure::Hybrid));
    bounded_teardown(lifecycle.shutdown(), pool.close(), Duration::from_secs(5)).await?;
    result
}

async fn bounded_teardown(
    shutdown: impl std::future::Future<Output = ()>,
    close: impl std::future::Future<Output = ()>,
    limit: Duration,
) -> Result<(), Failure> {
    let stopped = tokio::time::timeout(limit, shutdown).await.is_ok();
    let closed = tokio::time::timeout(limit, close).await.is_ok();
    if stopped && closed {
        Ok(())
    } else {
        Err(Failure::Hybrid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(resources: &Path) -> PathBuf {
        let root = model::bundle_dir(resources);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("model-bundle.manifest.json"), MANIFEST).unwrap();
        root
    }

    #[test]
    fn only_first_argument_selects_retrieval_smoke() {
        let selected = |args: &[&str]| requested(args.iter().map(OsString::from));
        assert!(selected(&["app", "--smoke-retrieval"]));
        assert!(selected(&["app", "--smoke-retrieval", "payload"]));
        assert!(!selected(&["app"]));
        assert!(!selected(&["app", "deep-link", "--smoke-retrieval"]));
        assert!(!selected(&["app", "--smoke-dbstat", "--smoke-retrieval"]));
    }

    #[test]
    fn resource_selection_rejects_developer_override() {
        let resources = Path::new("installed");
        assert_eq!(
            resource_bundle(resources, true),
            Err(Failure::ResourceResolution)
        );
        assert_eq!(
            resource_bundle(resources, false),
            Ok(resources.join("resources/retrieval/bundle"))
        );
    }

    #[test]
    fn installed_resources_fail_closed_before_model_loading() {
        let temp = tempfile::tempdir().unwrap();
        let root = model::bundle_dir(temp.path());
        assert_eq!(
            verify_resources(temp.path(), &root),
            Err(Failure::ManifestMissing)
        );
        write_manifest(temp.path());
        assert_eq!(
            verify_resources(temp.path(), &root),
            Err(Failure::ArtifactMissing)
        );
        let manifest = parse_manifest(MANIFEST).unwrap();
        // Empty synthetic artifacts establish the presence check only; no
        // model parser ever consumes their invalid bytes.
        for relative in managed_paths(&manifest) {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, []).unwrap();
        }
        assert_eq!(
            verify_resources(temp.path(), &root),
            Err(Failure::ArtifactIntegrity)
        );
        let license = root.join(&manifest.licenses[0].path);
        std::fs::remove_file(&license).unwrap();
        assert_eq!(
            verify_resources(temp.path(), &root),
            Err(Failure::ArtifactMissing)
        );
        let mut corrupt = MANIFEST.as_bytes().to_vec();
        corrupt[0] = b'!';
        std::fs::write(root.join("model-bundle.manifest.json"), corrupt).unwrap();
        assert_eq!(
            verify_resources(temp.path(), &root),
            Err(Failure::ManifestInvalid)
        );
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn manifest_link_cannot_redirect_to_an_external_resource_tree() {
        let temp = tempfile::tempdir().unwrap();
        let resources = temp.path().join("installed");
        let root = model::bundle_dir(&resources);
        std::fs::create_dir_all(&root).unwrap();
        let outside = temp.path().join("outside-manifest.json");
        std::fs::write(&outside, MANIFEST).unwrap();
        let linked = root.join("model-bundle.manifest.json");
        #[cfg(windows)]
        let link = std::os::windows::fs::symlink_file(&outside, &linked);
        #[cfg(unix)]
        let link = std::os::unix::fs::symlink(&outside, &linked);
        if let Err(error) = link {
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314)
            {
                println!("SKIP manifest-link check: symbolic-link privilege unavailable");
                return;
            }
            panic!("manifest-link fixture setup failed");
        }
        assert_eq!(
            verify_resources(&resources, &root),
            Err(Failure::ResourceResolution)
        );
    }

    #[tokio::test]
    async fn stuck_fixture_teardown_finishes_with_a_typed_failure() {
        let closed = std::sync::atomic::AtomicBool::new(false);
        let result = bounded_teardown(
            std::future::pending(),
            async {
                closed.store(true, std::sync::atomic::Ordering::SeqCst);
            },
            Duration::from_millis(1),
        )
        .await;
        assert_eq!(result, Err(Failure::Hybrid));
        assert!(closed.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(
            bounded_teardown(async {}, std::future::pending(), Duration::from_millis(1)).await,
            Err(Failure::Hybrid)
        );
        assert_eq!(
            bounded_teardown(async {}, async {}, Duration::from_secs(1)).await,
            Ok(())
        );
    }

    #[test]
    fn failure_stages_have_distinct_private_output_and_codes() {
        let failures = [
            Failure::Harness,
            Failure::ResourceResolution,
            Failure::ManifestMissing,
            Failure::ManifestInvalid,
            Failure::ArtifactMissing,
            Failure::ArtifactIntegrity,
            Failure::Tokenizer,
            Failure::Embedding,
            Failure::Reranker,
            Failure::Hybrid,
        ];
        assert_eq!(
            failures
                .map(|failure| failure as i32)
                .into_iter()
                .collect::<BTreeSet<_>>()
                .len(),
            10
        );
        for failure in failures {
            let line = failure.report();
            assert!(line.starts_with("smoke-retrieval: stage="));
            assert!(!line.contains(FIXTURE_QUERY));
            assert!(!line.contains("\\") && !line.contains("/"));
        }
        assert_eq!(
            model_failure(RetrievalModelError::TokenizerLoad {
                role: "embedding",
                reason: "private-path".into()
            }),
            Failure::Tokenizer
        );
        for role in ["embedding", "reranker"] {
            let error = RetrievalModelError::SessionLoad {
                role,
                reason: "private-path".into(),
            };
            assert_eq!(
                model_failure(error),
                if role == "embedding" {
                    Failure::Embedding
                } else {
                    Failure::Reranker
                }
            );
        }
    }

    #[test]
    fn invalid_reranker_outputs_cannot_pass() {
        let reference = (0..5)
            .map(|_| PairReference {
                query: String::new(),
                evidence: String::new(),
                score: 0.5,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            check_reranker_values(&[f32::NAN; 5], &reference),
            Err(Failure::Reranker)
        );
        assert_eq!(
            check_reranker_values(&[0.0; 4], &reference),
            Err(Failure::Reranker)
        );
        assert_eq!(
            check_reranker_values(&[0.0; 5], &reference),
            Err(Failure::Reranker)
        );
    }

    #[test]
    fn non_finite_or_wrong_dimensional_embeddings_cannot_pass() {
        assert_eq!(embedding_norm(&[0.0; 767]), Err(Failure::Embedding));
        assert_eq!(embedding_norm(&[f32::NAN; 768]), Err(Failure::Embedding));
        assert_eq!(
            embedding_norm(&[f32::INFINITY; 768]),
            Err(Failure::Embedding)
        );
        let mut normalized = [0.0; 768];
        normalized[0] = 1.0;
        assert_eq!(embedding_norm(&normalized), Ok(1.0));
    }

    async fn assert_production_fallback(root: PathBuf) {
        let pool = fixture_pool().await.unwrap();
        let lifecycle = RetrievalLifecycle::new(LifecycleConfig::production(Some(root.clone())));
        lifecycle.attach_database(pool.clone());
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let status =
                    crate::retrieval::index::index_status(&pool, &lifecycle.index_service(), false)
                        .await
                        .unwrap();
                if status.model_load_failure.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            let result = RetrievalService::new(lifecycle.clone())
                .retrieve_ranked(&pool, fixture_request())
                .await
                .unwrap();
            assert_eq!(
                result.semantic_fallback,
                Some(crate::retrieval::SemanticFallbackReason::NoActiveGeneration)
            );
            assert!(!result.ranking.reranker_used);
            assert!(result
                .ranking
                .evidence
                .iter()
                .any(|entry| entry.evidence.meeting_id == FIXTURE_ROWS[0].0));
            assert!(result.ranking.evidence.iter().all(|entry| entry
                .evidence
                .provenance
                .iter()
                .all(|p| p.channel != RetrievalChannel::Semantic)));
            assert!(lifecycle.is_running());
        })
        .await;
        lifecycle.shutdown().await;
        pool.close().await;
        assert!(result.is_ok(), "production lexical fallback timed out");
    }

    #[tokio::test]
    async fn missing_resources_keep_production_lifecycle_and_lexical_search_usable() {
        let temp = tempfile::tempdir().unwrap();
        let root = model::bundle_dir(temp.path());
        assert_production_fallback(root.clone()).await;
        assert!(
            !root.exists(),
            "fallback must not create a replacement bundle"
        );
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn corrupt_resources_keep_production_lifecycle_and_lexical_search_usable() {
        let temp = tempfile::tempdir().unwrap();
        let root = write_manifest(temp.path());
        let manifest = parse_manifest(MANIFEST).unwrap();
        let first = root.join(&manifest.embedding_model.artifacts[0].path);
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        std::fs::write(&first, b"corrupt").unwrap();
        assert_production_fallback(root).await;
        assert_eq!(std::fs::read(first).unwrap(), b"corrupt");
    }

    /// Explicit source-side package-layout evidence only. This cannot select
    /// an override for the installed executable and cannot prove MSI/NSIS.
    #[test]
    #[ignore = "requires the complete staged production bundle and real CPU inference"]
    fn real_package_layout_reference_and_hybrid_smoke() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR"));
        let result = diagnose(resources, &model::bundle_dir(resources));
        match result {
            Ok(report) => println!("{}", report.line()),
            Err(failure) => panic!("{}", failure.report()),
        }
    }
}
