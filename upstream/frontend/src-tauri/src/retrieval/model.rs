//! Lazy bundled-model runtime for semantic retrieval (Sprint 2A Task 2.2).
//!
//! Resolves the approved signed-resource bundle from Tauri's resource
//! directory, lazily re-verifies artifact byte lengths and SHA-256 digests
//! through the Task 1.5 verifier before the first process load, and produces
//! reference-correct tokenizer, embedding, and reranker outputs through two
//! bounded CPU ORT sessions. Session sets are cached per exact bundle
//! identity with room for the architecture's active + shadow generations, so
//! loading a shadow-model bundle never evicts the still-active one. Inference
//! runs on blocking threads (never on Tokio workers or while holding an async
//! lock), and every failure is a typed privacy-safe result: errors carry only
//! artifact paths and structural reasons, never input text, token IDs,
//! embeddings, or credentials.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::ThreadId;

use ndarray::{Array2, ArrayD};
use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::tensor::TensorElementType;
use ort::value::{TensorRef, ValueType};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use tokenizers::{Tokenizer, TruncationDirection, TruncationParams, TruncationStrategy};

use crate::model_bundle::{
    parse_manifest, ArtifactEntry, ModelBundleError, ModelBundleManifest, TensorSpec,
};

const MANIFEST_FILE: &str = "model-bundle.manifest.json";
const TOKENIZER_FILE: &str = "tokenizer.json";

/// Sprint 1 approved ORT intra-op cap ("Reranker runtime ... ORT intra-op 4").
const APPROVED_ORT_INTRA_OP_CAP: usize = 4;
/// Approved runtime contract: exactly one inter-op thread.
const APPROVED_ORT_INTER_OP_THREADS: usize = 1;
/// Sprint 1 approved reranker runtime batch size.
const RERANKER_BATCH: usize = 1;
/// ponytail: the ONNX batch width for document embedding is an internal
/// throughput/memory knob, not an approved quality constant; it never spans
/// cancellation boundaries. Raise only with a memory re-measurement.
const EMBEDDING_BATCH: usize = 16;
/// Resident bundle identities mirror the architecture's active + shadow
/// generation envelope: a third distinct identity is refused rather than
/// displacing a still-active set.
const MAX_CACHED_BUNDLES: usize = 2;

#[derive(Debug, Error)]
pub enum RetrievalModelError {
    #[error("retrieval bundle unavailable at '{path}': {reason}")]
    ManifestUnavailable { path: String, reason: String },
    #[error("retrieval bundle manifest version unsupported: {0}")]
    ManifestUnsupported(String),
    #[error("retrieval bundle manifest invalid: {0}")]
    ManifestInvalid(String),
    #[error("retrieval artifact verification failed for '{path}': {reason}")]
    ArtifactVerification { path: String, reason: String },
    #[error("{role} tokenizer failed to load: {reason}")]
    TokenizerLoad { role: &'static str, reason: String },
    #[error("{role} ONNX session failed to load: {reason}")]
    SessionLoad { role: &'static str, reason: String },
    #[error("{role} session does not match the approved contract: {reason}")]
    ContractMismatch { role: &'static str, reason: String },
    #[error("{role} inference failed: {reason}")]
    Inference { role: &'static str, reason: String },
    #[error(
        "retrieval session cache is at its approved {capacity}-bundle capacity; refusing to displace an active or shadow bundle"
    )]
    CacheCapacity { capacity: usize },
    #[error("semantic model work cancelled at a batch boundary")]
    Cancelled,
}

impl From<ModelBundleError> for RetrievalModelError {
    fn from(error: ModelBundleError) -> Self {
        match error {
            ModelBundleError::UnsupportedManifestVersion { found } => {
                Self::ManifestUnsupported(format!("version {found}; this build supports v1 only"))
            }
            ModelBundleError::InvalidManifest(reason) => Self::ManifestInvalid(reason),
            ModelBundleError::ArtifactVerification { path, reason } => {
                Self::ArtifactVerification { path, reason }
            }
        }
    }
}

/// Exact identity a cached session set was loaded under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleIdentity {
    pub bundle_id: String,
    pub root: PathBuf,
}

/// Observed session tensor metadata reduced to plain data so the load-time
/// contract validation is testable without model artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IoSpec {
    name: String,
    dtype: String,
    shape: Vec<i64>,
}

struct EngineIo {
    inputs: Vec<IoSpec>,
    outputs: Vec<IoSpec>,
}

struct TokenRow {
    text: String,
    partner: Option<String>,
}

struct Engine {
    role: &'static str,
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    pad_id: i64,
    output_name: String,
}

struct RuntimeInner {
    identity: BundleIdentity,
    embedding: Engine,
    reranker: Engine,
    query_prefix: String,
    document_prefix: String,
    dimensions: u32,
    reranker_label_index: usize,
}

/// Shared handle to the lazily loaded embedding/reranker runtime. Cloning is
/// cheap; sessions are shared process-wide via [`get_or_load`].
#[derive(Clone)]
pub struct RetrievalModels(Arc<RuntimeInner>);

impl std::fmt::Debug for RetrievalModels {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RetrievalModels")
            .field("identity", &self.0.identity)
            .finish_non_exhaustive()
    }
}

static SESSION_CACHE: Mutex<Vec<(BundleIdentity, RetrievalModels)>> = Mutex::new(Vec::new());
static LAST_INFERENCE_THREAD: Mutex<Option<ThreadId>> = Mutex::new(None);

/// Resolves the packaged bundle directory below Tauri's resource directory
/// (`bundle.resources` maps `resources/retrieval/bundle` verbatim).
pub fn bundle_dir(resource_dir: &Path) -> PathBuf {
    resource_dir
        .join("resources")
        .join("retrieval")
        .join("bundle")
}

/// Returns the cached runtime for the bundle at `bundle_root`, or loads it
/// once: parse and validate the manifest, lazily recheck every artifact's
/// byte length and SHA-256, then build both bounded CPU sessions. Entries are
/// keyed by the full parsed [`BundleIdentity`] with room for exactly the
/// architecture's active + shadow generations; requesting a third distinct
/// identity fails typed with [`RetrievalModelError::CacheCapacity`] instead of
/// displacing either one. Failed loads are never cached and retry on the next
/// call, and concurrent callers of one identity share that single load.
pub fn get_or_load(bundle_root: &Path) -> Result<RetrievalModels, RetrievalModelError> {
    let canonical =
        bundle_root
            .canonicalize()
            .map_err(|e| RetrievalModelError::ManifestUnavailable {
                path: bundle_root.display().to_string(),
                reason: e.to_string(),
            })?;
    let manifest_path = canonical.join(MANIFEST_FILE);
    let json = fs::read_to_string(&manifest_path).map_err(|e| {
        RetrievalModelError::ManifestUnavailable {
            path: manifest_path.display().to_string(),
            reason: e.to_string(),
        }
    })?;
    // Parse first so the cache key carries the manifest-parsed bundle id, not
    // just the directory: identities collide only when every identity field
    // matches.
    let manifest = parse_manifest(&json)?;
    let identity = BundleIdentity {
        bundle_id: manifest.bundle_id.clone(),
        root: canonical,
    };

    let mut cache = locked(&SESSION_CACHE);
    if let Some(models) = cache_lookup(&cache, &identity) {
        return Ok(models);
    }
    // A third distinct identity must never displace an active or shadow
    // generation, so it is refused before any artifact verification work.
    reserve_cache_slot(&cache, &identity)?;
    // Lazy per-process gate: post-install corruption stops here, before any
    // bytes reach ONNX Runtime.
    manifest.verify_artifacts(&identity.root)?;
    let models = RetrievalModels(Arc::new(load_runtime(
        &identity.root,
        &manifest,
        identity.clone(),
    )?));
    cache.push((identity, models.clone()));
    Ok(models)
}

pub(crate) fn cached_model(model_id: &str) -> Option<RetrievalModels> {
    locked(&SESSION_CACHE)
        .iter()
        .find(|(identity, _)| identity.bundle_id == model_id)
        .map(|(_, models)| models.clone())
}

fn cache_lookup(
    cache: &[(BundleIdentity, RetrievalModels)],
    identity: &BundleIdentity,
) -> Option<RetrievalModels> {
    cache
        .iter()
        .find(|(cached, _)| cached == identity)
        .map(|(_, models)| models.clone())
}

fn reserve_cache_slot(
    cache: &[(BundleIdentity, RetrievalModels)],
    incoming: &BundleIdentity,
) -> Result<(), RetrievalModelError> {
    if cache.len() >= MAX_CACHED_BUNDLES && !cache.iter().any(|(cached, _)| cached == incoming) {
        return Err(RetrievalModelError::CacheCapacity {
            capacity: MAX_CACHED_BUNDLES,
        });
    }
    Ok(())
}

fn load_runtime(
    root: &Path,
    manifest: &ModelBundleManifest,
    identity: BundleIdentity,
) -> Result<RuntimeInner, RetrievalModelError> {
    let intra_threads = approved_intra_threads();
    let embedding_contract = &manifest.embedding_model;
    let (embedding, embedding_io) = load_engine(
        root,
        "embedding",
        &embedding_contract.artifacts[0],
        &embedding_contract.tokenizer.artifacts,
        &embedding_contract.outputs[0].name,
        embedding_contract.max_sequence_length,
        intra_threads,
    )?;
    validate_session_io(
        "embedding",
        &embedding_contract.inputs,
        &embedding_io.inputs,
    )?;
    validate_session_io(
        "embedding",
        &embedding_contract.outputs,
        &embedding_io.outputs,
    )?;
    for spec in &embedding_io.inputs {
        expect_rank("embedding", spec, 2)?;
    }
    let hidden_state = &embedding_io.outputs[0];
    expect_rank("embedding", hidden_state, 3)?;
    validate_embedding_output(hidden_state, embedding_contract.dimensions)?;

    let reranker_contract = &manifest.reranker_model;
    let label_output = &reranker_contract.outputs[reranker_contract.output_label_index];
    let (reranker, reranker_io) = load_engine(
        root,
        "reranker",
        &reranker_contract.artifacts[0],
        &reranker_contract.tokenizer.artifacts,
        &label_output.name,
        reranker_contract.max_sequence_length,
        intra_threads,
    )?;
    validate_session_io("reranker", &reranker_contract.inputs, &reranker_io.inputs)?;
    validate_session_io("reranker", &reranker_contract.outputs, &reranker_io.outputs)?;
    for spec in &reranker_io.inputs {
        expect_rank("reranker", spec, 2)?;
    }
    let logits = &reranker_io.outputs[reranker_contract.output_label_index];
    expect_rank("reranker", logits, 2)?;
    validate_label_output(logits, reranker_contract.output_label_index)?;

    Ok(RuntimeInner {
        identity,
        embedding,
        reranker,
        query_prefix: embedding_contract.query_prefix.clone(),
        document_prefix: embedding_contract.document_prefix.clone(),
        dimensions: embedding_contract.dimensions,
        reranker_label_index: reranker_contract.output_label_index,
    })
}

fn load_engine(
    root: &Path,
    role: &'static str,
    model_artifact: &ArtifactEntry,
    tokenizer_artifacts: &[ArtifactEntry],
    output_name: &str,
    max_sequence_length: u32,
    intra_threads: usize,
) -> Result<(Engine, EngineIo), RetrievalModelError> {
    let tokenizer_entry = tokenizer_artifacts
        .iter()
        .find(|entry| entry.path.ends_with(TOKENIZER_FILE))
        .ok_or_else(|| RetrievalModelError::ContractMismatch {
            role,
            reason: format!("manifest manages no '{TOKENIZER_FILE}' artifact"),
        })?;
    let mut tokenizer = Tokenizer::from_file(root.join(&tokenizer_entry.path)).map_err(|e| {
        RetrievalModelError::TokenizerLoad {
            role,
            reason: e.to_string(),
        }
    })?;
    // The approved contract pins right-side LongestFirst truncation at the
    // manifest max sequence length with no stride, applied by the tokenizer
    // itself so special tokens/pair separators survive instead of cutting raw
    // IDs. Stated explicitly so tokenizer defaults can never drift under it.
    tokenizer
        .with_truncation(Some(TruncationParams {
            max_length: max_sequence_length as usize,
            stride: 0,
            strategy: TruncationStrategy::LongestFirst,
            direction: TruncationDirection::Right,
        }))
        .map_err(|e| RetrievalModelError::TokenizerLoad {
            role,
            reason: format!("truncation config: {e}"),
        })?;
    let pad_id = tokenizer.token_to_id("<pad>").unwrap_or(0) as i64;

    let session = bounded_cpu_session_builder(intra_threads)
        .and_then(|builder| builder.commit_from_file(root.join(&model_artifact.path)))
        .map_err(|e| RetrievalModelError::SessionLoad {
            role,
            reason: format!("{}: {e}", model_artifact.path),
        })?;

    let io = EngineIo {
        inputs: session
            .inputs
            .iter()
            .map(|i| io_spec(&i.name, &i.input_type))
            .collect(),
        outputs: session
            .outputs
            .iter()
            .map(|o| io_spec(&o.name, &o.output_type))
            .collect(),
    };
    Ok((
        Engine {
            role,
            session: Mutex::new(session),
            tokenizer,
            pad_id,
            output_name: output_name.to_string(),
        },
        io,
    ))
}

fn io_spec(name: &String, value_type: &ValueType) -> IoSpec {
    IoSpec {
        name: name.clone(),
        dtype: dtype_name(value_type),
        shape: match value_type {
            ValueType::Tensor { shape, .. } => shape.to_vec(),
            _ => Vec::new(),
        },
    }
}

fn dtype_name(value_type: &ValueType) -> String {
    match value_type {
        ValueType::Tensor {
            ty: TensorElementType::Int64,
            ..
        } => "int64".to_string(),
        ValueType::Tensor {
            ty: TensorElementType::Float32,
            ..
        } => "float32".to_string(),
        ValueType::Tensor { ty, .. } => format!("{ty:?}").to_lowercase(),
        _ => "non-tensor".to_string(),
    }
}

fn validate_session_io(
    role: &'static str,
    expected: &[TensorSpec],
    observed: &[IoSpec],
) -> Result<(), RetrievalModelError> {
    let mismatch = |reason: String| RetrievalModelError::ContractMismatch { role, reason };
    if observed.len() != expected.len() {
        let observed_names: Vec<&str> = observed.iter().map(|s| s.name.as_str()).collect();
        let expected_names: Vec<&str> = expected.iter().map(|s| s.name.as_str()).collect();
        return Err(mismatch(format!(
            "session declares tensors {observed_names:?}, manifest requires {expected_names:?}"
        )));
    }
    for (observed, expected) in observed.iter().zip(expected) {
        if observed.name != expected.name {
            return Err(mismatch(format!(
                "session tensor '{}' found where manifest requires '{}'",
                observed.name, expected.name
            )));
        }
        if observed.dtype != expected.dtype {
            return Err(mismatch(format!(
                "tensor '{}' has dtype '{}', manifest requires '{}'",
                observed.name, observed.dtype, expected.dtype
            )));
        }
    }
    Ok(())
}

fn expect_rank(role: &'static str, spec: &IoSpec, rank: usize) -> Result<(), RetrievalModelError> {
    if spec.shape.len() != rank {
        return Err(RetrievalModelError::ContractMismatch {
            role,
            reason: format!(
                "tensor '{}' must have rank-{rank} shape, found {:?}",
                spec.name, spec.shape
            ),
        });
    }
    Ok(())
}

/// A fixed hidden dimension must equal the manifest dimensions; a dynamic
/// (-1) dimension is re-validated against the pooled vector at inference.
fn validate_embedding_output(spec: &IoSpec, dimensions: u32) -> Result<(), RetrievalModelError> {
    if spec.shape[2] > 0 && spec.shape[2] != dimensions as i64 {
        return Err(RetrievalModelError::ContractMismatch {
            role: "embedding",
            reason: format!(
                "output '{}' fixed dimension {} disagrees with manifest dimensions {dimensions}",
                spec.name, spec.shape[2]
            ),
        });
    }
    Ok(())
}

/// The manifest label index must address a column of the label output.
fn validate_label_output(spec: &IoSpec, label_index: usize) -> Result<(), RetrievalModelError> {
    if spec.shape[1] > 0 && label_index >= spec.shape[1] as usize {
        return Err(RetrievalModelError::ContractMismatch {
            role: "reranker",
            reason: format!(
                "label index {label_index} outside output width {}",
                spec.shape[1]
            ),
        });
    }
    Ok(())
}

impl Engine {
    fn inference_error(&self, reason: String) -> RetrievalModelError {
        RetrievalModelError::Inference {
            role: self.role,
            reason,
        }
    }

    fn tokenize(
        &self,
        rows: &[TokenRow],
    ) -> Result<(Array2<i64>, Array2<i64>), RetrievalModelError> {
        let mut encoded_rows = Vec::with_capacity(rows.len());
        for row in rows {
            let encoded = match &row.partner {
                Some(partner) => self
                    .tokenizer
                    .encode((row.text.as_str(), partner.as_str()), true),
                None => self.tokenizer.encode(row.text.as_str(), true),
            }
            .map_err(|e| self.inference_error(format!("tokenization failed: {e}")))?;
            encoded_rows.push(encoded.get_ids().iter().map(|&id| id as i64).collect());
        }
        let width = encoded_rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let batch = encoded_rows.len().max(1);
        let mut ids = vec![self.pad_id; batch * width];
        let mut mask = vec![0_i64; batch * width];
        for (row_index, row) in encoded_rows.iter().enumerate() {
            let length = row.len().min(width);
            ids[row_index * width..row_index * width + length].copy_from_slice(&row[..length]);
            mask[row_index * width..row_index * width + length].fill(1);
        }
        let ids = Array2::from_shape_vec((batch, width), ids)
            .map_err(|e| self.inference_error(format!("ids batch shape: {e}")))?;
        let mask = Array2::from_shape_vec((batch, width), mask)
            .map_err(|e| self.inference_error(format!("mask batch shape: {e}")))?;
        Ok((ids, mask))
    }

    fn run_batch(
        &self,
        ids: &Array2<i64>,
        mask: &Array2<i64>,
    ) -> Result<ArrayD<f32>, RetrievalModelError> {
        record_inference_thread();
        let mut session = locked(&self.session);
        let outputs = session
            .run(inputs![
                "input_ids" => TensorRef::from_array_view(ids.view())
                    .map_err(|e| self.inference_error(format!("input_ids tensor: {e}")))?,
                "attention_mask" => TensorRef::from_array_view(mask.view())
                    .map_err(|e| self.inference_error(format!("attention_mask tensor: {e}")))?,
            ])
            .map_err(|e| self.inference_error(format!("ORT run: {e}")))?;
        outputs
            .get(&self.output_name)
            .ok_or_else(|| self.inference_error(format!("missing output {}", self.output_name)))?
            .try_extract_array()
            .map(|view| view.to_owned())
            .map_err(|e| self.inference_error(format!("extract {}: {e}", self.output_name)))
    }
}

impl RetrievalModels {
    pub fn identity(&self) -> &BundleIdentity {
        &self.0.identity
    }

    /// Manifest-pinned embedding dimension count.
    pub fn dimensions(&self) -> u32 {
        self.0.dimensions
    }

    /// The pinned tokenizer artifact backing chunk-window arithmetic, so
    /// window budgets are counted with the exact packaged tokenizer contract.
    pub fn document_tokenizer(&self) -> &Tokenizer {
        &self.0.embedding.tokenizer
    }

    /// Embeds queries with the manifest query prefix (blocking; call from
    /// `spawn_blocking`). Cancellation is honored at document-batch boundaries.
    pub fn embed_queries_sync(
        &self,
        queries: &[&str],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
        self.embed_sync(&self.0.query_prefix, queries, cancel)
    }

    /// Embeds documents with the manifest passage prefix (blocking; call from
    /// `spawn_blocking`). Cancellation is honored at document-batch boundaries.
    pub fn embed_documents_sync(
        &self,
        documents: &[&str],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
        self.embed_sync(&self.0.document_prefix, documents, cancel)
    }

    /// Scores (question, evidence) pairs with the cross-encoder (blocking;
    /// call from `spawn_blocking`). The manifest-approved score transform is
    /// identity: each result is the raw float32 logit at the manifest label
    /// index. Cancellation is honored between pair batches.
    pub fn rerank_sync(
        &self,
        pairs: &[(String, String)],
        cancel: &CancellationToken,
    ) -> Result<Vec<f32>, RetrievalModelError> {
        let mut scores = Vec::with_capacity(pairs.len());
        for chunk in pairs.chunks(RERANKER_BATCH) {
            if cancel.is_cancelled() {
                return Err(RetrievalModelError::Cancelled);
            }
            let rows: Vec<TokenRow> = chunk
                .iter()
                .map(|(query, evidence)| TokenRow {
                    text: query.clone(),
                    partner: Some(evidence.clone()),
                })
                .collect();
            let (ids, mask) = self.0.reranker.tokenize(&rows)?;
            let logits = self.0.reranker.run_batch(&ids, &mask)?;
            scores.extend(label_scores(
                &logits,
                self.0.reranker_label_index,
                self.0.reranker.role,
            )?);
        }
        Ok(scores)
    }

    /// Async wrapper running embedding on the blocking pool, off Tokio worker
    /// threads.
    pub async fn embed_queries(
        &self,
        queries: Vec<String>,
        cancel: CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
        let models = self.clone();
        spawn_model_work(move || {
            let references: Vec<&str> = queries.iter().map(String::as_str).collect();
            models.embed_queries_sync(&references, &cancel)
        })
        .await
    }

    /// Async wrapper running document embedding on the blocking pool, off
    /// Tokio worker threads.
    pub async fn embed_documents(
        &self,
        documents: Vec<String>,
        cancel: CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
        let models = self.clone();
        spawn_model_work(move || {
            let references: Vec<&str> = documents.iter().map(String::as_str).collect();
            models.embed_documents_sync(&references, &cancel)
        })
        .await
    }

    /// Async wrapper running cross-encoder scoring on the blocking pool, off
    /// Tokio worker threads.
    pub async fn rerank(
        &self,
        pairs: Vec<(String, String)>,
        cancel: CancellationToken,
    ) -> Result<Vec<f32>, RetrievalModelError> {
        let models = self.clone();
        spawn_model_work(move || models.rerank_sync(&pairs, &cancel)).await
    }

    fn embed_sync(
        &self,
        prefix: &str,
        texts: &[&str],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
        let mut vectors = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(EMBEDDING_BATCH) {
            if cancel.is_cancelled() {
                return Err(RetrievalModelError::Cancelled);
            }
            let rows: Vec<TokenRow> = chunk
                .iter()
                .map(|text| TokenRow {
                    text: format!("{prefix}{text}"),
                    partner: None,
                })
                .collect();
            let (ids, mask) = self.0.embedding.tokenize(&rows)?;
            let hidden = self.0.embedding.run_batch(&ids, &mask)?;
            vectors.extend(pooled_embeddings(
                &hidden,
                &mask,
                self.0.dimensions,
                self.0.embedding.role,
            )?);
        }
        Ok(vectors)
    }
}

async fn spawn_model_work<F, T>(work: F) -> Result<T, RetrievalModelError>
where
    F: FnOnce() -> Result<T, RetrievalModelError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| RetrievalModelError::Inference {
            role: "retrieval worker",
            reason: format!("blocking task join failed: {e}"),
        })?
}

fn pooled_embeddings(
    hidden: &ArrayD<f32>,
    masks: &Array2<i64>,
    dimensions: u32,
    role: &'static str,
) -> Result<Vec<Vec<f32>>, RetrievalModelError> {
    let mismatch = |reason: String| RetrievalModelError::ContractMismatch { role, reason };
    let shape = hidden.shape();
    if shape.len() != 3 {
        return Err(mismatch(format!(
            "last_hidden_state must have rank-3 [batch, sequence, hidden] shape, found {shape:?}"
        )));
    }
    let (batch, sequence, hidden_dims) = (shape[0], shape[1], shape[2]);
    if hidden_dims != dimensions as usize {
        return Err(mismatch(format!(
            "output dimension {hidden_dims} disagrees with manifest dimensions {dimensions}"
        )));
    }
    if batch != masks.nrows() {
        return Err(mismatch(format!(
            "batch {batch} disagrees with attention_mask rows {}",
            masks.nrows()
        )));
    }
    let data = hidden
        .as_slice()
        .ok_or_else(|| mismatch("last_hidden_state is not contiguous".to_string()))?;
    let mut vectors = Vec::with_capacity(batch);
    for row in 0..batch {
        let mut mean = vec![0_f32; hidden_dims];
        let mut positions = 0_f32;
        for step in 0..sequence {
            if masks[[row, step]] == 1 {
                positions += 1.0;
                let base = (row * sequence + step) * hidden_dims;
                for (summed, value) in mean.iter_mut().zip(&data[base..base + hidden_dims]) {
                    *summed += value;
                }
            }
        }
        if positions == 0.0 {
            return Err(mismatch("attention_mask selects no positions".to_string()));
        }
        for value in &mut mean {
            *value /= positions;
        }
        l2_normalize(&mut mean, role)?;
        if !mean.iter().all(|value| value.is_finite()) {
            return Err(RetrievalModelError::Inference {
                role,
                reason: "pooled embedding contains non-finite values".to_string(),
            });
        }
        vectors.push(mean);
    }
    Ok(vectors)
}

fn label_scores(
    logits: &ArrayD<f32>,
    label_index: usize,
    role: &'static str,
) -> Result<Vec<f32>, RetrievalModelError> {
    let shape = logits.shape();
    if shape.len() != 2 {
        return Err(RetrievalModelError::ContractMismatch {
            role,
            reason: format!("label output must have rank-2 [batch, labels] shape, found {shape:?}"),
        });
    }
    let (batch, width) = (shape[0], shape[1]);
    if width <= label_index {
        return Err(RetrievalModelError::ContractMismatch {
            role,
            reason: format!("label index {label_index} outside output width {width}"),
        });
    }
    let data = logits
        .as_slice()
        .ok_or_else(|| RetrievalModelError::ContractMismatch {
            role,
            reason: "label output is not contiguous".to_string(),
        })?;
    let scores = data
        .chunks_exact(width)
        .take(batch)
        .map(|row| row[label_index])
        .collect::<Vec<_>>();
    if !scores.iter().all(|score| score.is_finite()) {
        return Err(RetrievalModelError::Inference {
            role,
            reason: "label output contains non-finite values".to_string(),
        });
    }
    Ok(scores)
}

/// Unit-normalizes in place per the approved output contract. A vector whose
/// norm is non-finite or effectively zero can never satisfy that contract, so
/// it is rejected instead of being returned unchanged.
fn l2_normalize(vector: &mut [f32], role: &'static str) -> Result<(), RetrievalModelError> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return Err(RetrievalModelError::Inference {
            role,
            reason: "embedding norm is non-finite or effectively zero".to_string(),
        });
    }
    for value in vector {
        *value /= norm;
    }
    Ok(())
}

/// Sprint 1 selection rule: min(4, max(1, logical cores / 2)).
fn approved_intra_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / 2).clamp(1, APPROVED_ORT_INTRA_OP_CAP)
}

/// CPU-only session builder carrying the approved thread bounds: the Sprint 1
/// intra-op cap and exactly one inter-op thread.
fn bounded_cpu_session_builder(
    intra_threads: usize,
) -> ort::Result<ort::session::builder::SessionBuilder> {
    Ok(Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_execution_providers(vec![CPUExecutionProvider::default().build()])?
        .with_intra_threads(intra_threads)?
        .with_inter_threads(APPROVED_ORT_INTER_OP_THREADS)?)
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn record_inference_thread() {
    *locked(&LAST_INFERENCE_THREAD) = Some(std::thread::current().id());
}

#[cfg(test)]
fn last_inference_thread() -> Option<ThreadId> {
    *locked(&LAST_INFERENCE_THREAD)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::BTreeMap;
    use std::sync::Barrier;

    const PACKAGED_MANIFEST_JSON: &str =
        include_str!("../../resources/retrieval/bundle/model-bundle.manifest.json");
    const REFERENCE_FIXTURE_JSON: &str =
        include_str!("../../tests/fixtures/model_bundle_manifest.json");

    // Sprint 1 reference query text (with its approved prefix applied).
    const REFERENCE_QUERY_TEXT: &str =
        "query: quais os dias de comunicacao por whatsapp para o fluxo de retencao?";

    // Pinned against the staged bundle's pinned-revision tokenizer.json
    // (REFERENCE_QUERY_TEXT with add_special_tokens).
    const REFERENCE_QUERY_IDS: &[i64] = &[
        0, 41, 1294, 12, 53633, 362, 14850, 8, 34060, 123142, 196, 125072, 121, 36, 85679, 31, 8,
        73487, 123142, 32, 2,
    ];

    // Sprint 1 recorded tolerances (tests/model_benchmark.rs).
    const NORM_TOL: f64 = 0.01;
    const DIM_TOL: f64 = 0.06;
    const COSINE_TOL: f64 = 0.03;
    const SCORE_TOL: f64 = 0.20;

    /// Serializes success-path tests that share the process-global cache.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ReferenceFixture {
        reference_expectations: FixtureExpectations,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureExpectations {
        embedding: Vec<FixtureEmbedding>,
        reranker_pairs: Vec<FixtureGroup>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureEmbedding {
        label: String,
        text: String,
        norm: f64,
        sample_dims: BTreeMap<String, f64>,
        cosine_with: Vec<FixtureCosine>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureCosine {
        label: String,
        cosine: f64,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureGroup {
        model_dir: String,
        pairs: Vec<FixturePair>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixturePair {
        query: String,
        evidence: String,
        score: f64,
    }

    fn fixture() -> ReferenceFixture {
        serde_json::from_str(REFERENCE_FIXTURE_JSON).expect("reference fixture must parse")
    }

    fn staged_bundle_dir() -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("MEETLY_RAG_BUNDLE_DIR") {
            let path = PathBuf::from(dir);
            return path.is_dir().then_some(path);
        }
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("retrieval")
            .join("bundle");
        dir.is_dir().then_some(dir)
    }

    fn staged_models_ready(dir: &Path) -> bool {
        dir.join("models/embedding/model_int8.onnx").is_file()
            && dir.join("models/reranker/model_quint8_avx2.onnx").is_file()
    }

    fn sigmoid(value: f32) -> f64 {
        1.0 / (1.0 + (-value).exp()) as f64
    }

    fn temp_bundle(mutate: impl FnOnce(&Path)) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        mutate(dir.path());
        let path = dir.path().to_path_buf();
        (dir, path)
    }

    fn write_rel(root: &Path, rel: &str, content: &[u8]) {
        let target = root.join(rel);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, content).unwrap();
    }

    // ------------------------------------------------------------------
    // Non-artifact behavior: typed failures stay lazy and privacy-safe.
    // ------------------------------------------------------------------

    #[test]
    fn missing_bundle_fails_typed_without_caching() {
        let (_guard, empty) = temp_bundle(|_| {});

        let err = get_or_load(&empty).expect_err("empty directory must fail");
        assert!(
            matches!(err, RetrievalModelError::ManifestUnavailable { .. }),
            "{err}"
        );
        assert!(err.to_string().contains("unavailable"), "{err}");

        // A failed load must not poison anything: retrying fails identically.
        let err_again = get_or_load(&empty).expect_err("retry must also fail");
        assert!(matches!(
            err_again,
            RetrievalModelError::ManifestUnavailable { .. }
        ));
    }

    #[test]
    fn unknown_manifest_version_fails_closed_before_any_model_load() {
        let broken =
            PACKAGED_MANIFEST_JSON.replace("\"manifestVersion\": 1", "\"manifestVersion\": 2");
        let (_guard, root) = temp_bundle(|root| {
            write_rel(root, MANIFEST_FILE, broken.as_bytes());
        });
        let err = get_or_load(&root).expect_err("unknown version must fail");
        assert!(
            matches!(err, RetrievalModelError::ManifestUnsupported(_)),
            "{err}"
        );
    }

    #[test]
    fn corrupt_and_missing_artifacts_fail_verification_before_onnx_load() {
        // A short fake model file trips the exact byte-length gate before any
        // session construction, proving the lazy hash recheck runs first.
        let (_guard, root) = temp_bundle(|root| {
            write_rel(root, MANIFEST_FILE, PACKAGED_MANIFEST_JSON.as_bytes());
            write_rel(root, "models/embedding/model_int8.onnx", b"not-a-model");
        });
        let err = get_or_load(&root).expect_err("short artifact must fail");
        assert!(
            matches!(err, RetrievalModelError::ArtifactVerification { ref reason, .. }
                if reason.contains("byte length mismatch")),
            "{err}"
        );

        // Manifest present but every managed file missing.
        let (_guard, root) = temp_bundle(|root| {
            write_rel(root, MANIFEST_FILE, PACKAGED_MANIFEST_JSON.as_bytes());
        });
        let err = get_or_load(&root).expect_err("missing artifacts must fail");
        assert!(
            matches!(err, RetrievalModelError::ArtifactVerification { ref reason, .. }
                if reason.contains("open failed")),
            "{err}"
        );
    }

    #[test]
    fn wrong_shape_wrong_dimension_and_wrong_dtype_fail_the_io_contract() {
        let input_spec = |name: &str, shape: &[i64]| IoSpec {
            name: name.to_string(),
            dtype: "int64".to_string(),
            shape: shape.to_vec(),
        };

        // Wrong input name.
        let err = validate_session_io(
            "embedding",
            &[TensorSpec {
                name: "input_ids".into(),
                dtype: "int64".into(),
            }],
            &[input_spec("token_ids", &[2, -1])],
        )
        .expect_err("wrong input name must fail");
        assert!(err.to_string().contains("'token_ids' found where"), "{err}");

        // Wrong input dtype.
        let err = validate_session_io(
            "embedding",
            &[TensorSpec {
                name: "input_ids".into(),
                dtype: "int64".into(),
            }],
            &[IoSpec {
                name: "input_ids".into(),
                dtype: "float32".into(),
                shape: vec![2, -1],
            }],
        )
        .expect_err("wrong dtype must fail");
        assert!(
            err.to_string()
                .contains("dtype 'float32', manifest requires 'int64'"),
            "{err}"
        );

        // Wrong tensor count.
        let err = validate_session_io(
            "embedding",
            &[TensorSpec {
                name: "input_ids".into(),
                dtype: "int64".into(),
            }],
            &[],
        )
        .expect_err("missing tensor must fail");
        assert!(
            err.to_string().contains("session declares tensors []"),
            "{err}"
        );

        // Rank violations.
        let err = expect_rank("embedding", &input_spec("input_ids", &[1, 2, 3]), 2)
            .expect_err("rank-3 input must fail");
        assert!(err.to_string().contains("must have rank-2"), "{err}");
        let err = expect_rank(
            "embedding",
            &IoSpec {
                name: "last_hidden_state".into(),
                dtype: "float32".into(),
                shape: vec![-1, -1],
            },
            3,
        )
        .expect_err("rank-2 hidden state must fail");
        assert!(err.to_string().contains("must have rank-3"), "{err}");

        // A fixed wrong embedding dimension (384 vs the approved 768) fails
        // closed; a dynamic dimension passes here and is re-checked per batch.
        let hidden_384 = IoSpec {
            name: "last_hidden_state".into(),
            dtype: "float32".into(),
            shape: vec![-1, -1, 384],
        };
        let err =
            validate_embedding_output(&hidden_384, 768).expect_err("wrong dimension must fail");
        assert!(
            err.to_string()
                .contains("fixed dimension 384 disagrees with manifest dimensions 768"),
            "{err}"
        );
        validate_embedding_output(
            &IoSpec {
                name: "last_hidden_state".into(),
                dtype: "float32".into(),
                shape: vec![-1, -1, -1],
            },
            768,
        )
        .expect("dynamic hidden dimension defers to inference checks");
        validate_embedding_output(&hidden_384, 384).expect("matching dimension passes");

        // A reranker label index outside a fixed logits width fails closed.
        let narrow_logits = IoSpec {
            name: "logits".into(),
            dtype: "float32".into(),
            shape: vec![-1, 1],
        };
        let err = validate_label_output(&narrow_logits, 1).expect_err("label index must fit");
        assert!(
            err.to_string()
                .contains("label index 1 outside output width 1"),
            "{err}"
        );
        validate_label_output(&narrow_logits, 0).expect("approved label index fits");
    }

    #[test]
    fn zero_norm_hidden_states_rejected_and_normal_rows_unit_normalize() {
        let mask = Array2::from_shape_vec((1, 3), vec![1_i64, 1, 1]).expect("mask shape");

        // Degenerate export output: every selected position carries zeros, so
        // the pooled vector can never satisfy the unit-norm contract.
        let zero_hidden = ndarray::ArrayD::<f32>::zeros(ndarray::IxDyn(&[1, 3, 4]));
        let rejected = pooled_embeddings(&zero_hidden, &mask, 4, "embedding")
            .expect_err("zero-norm embedding must be rejected");
        assert!(
            matches!(rejected, RetrievalModelError::Inference { role: "embedding", ref reason }
                if reason.contains("norm")),
            "{rejected}"
        );

        // Normal rows still pool and normalize to exact unit vectors:
        // masked mean of [3,0,0,0] and [0,4,0,0] is [1.5,2,0,0] -> [0.6,0.8,0,0].
        let hidden = ndarray::ArrayD::<f32>::from_shape_vec(
            ndarray::IxDyn(&[1, 2, 4]),
            vec![3.0_f32, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0],
        )
        .expect("hidden shape");
        let vectors = pooled_embeddings(
            &hidden,
            &Array2::from_shape_vec((1, 2), vec![1_i64, 1]).unwrap(),
            4,
            "embedding",
        )
        .expect("normal pooling succeeds");
        assert_eq!(vectors.len(), 1);
        let expected = [0.6_f32, 0.8, 0.0, 0.0];
        for (actual, expected) in vectors[0].iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= 1e-6,
                "{:?} vs {expected:?}",
                vectors[0]
            );
        }
        let norm: f32 = vectors[0]
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        assert!((norm - 1.0).abs() <= 1e-6, "unit norm: {norm}");
    }

    #[test]
    fn failure_messages_never_carry_user_content() {
        let marker = "SEGREDO-whatsapp-fluxo-de-retencao";
        let (_guard, empty) = temp_bundle(|_| {});
        let rendered = get_or_load(&empty)
            .expect_err("missing bundle must fail")
            .to_string();
        assert!(
            !rendered.contains(marker),
            "error leaked user content: {rendered}"
        );

        let structural = [
            RetrievalModelError::ManifestUnsupported("v99".into()),
            RetrievalModelError::ManifestInvalid("bad field".into()),
            RetrievalModelError::ArtifactVerification {
                path: "models/embedding/model_int8.onnx".into(),
                reason: "SHA-256 mismatch".into(),
            },
            RetrievalModelError::TokenizerLoad {
                role: "embedding",
                reason: "invalid tokenizer.json".into(),
            },
            RetrievalModelError::SessionLoad {
                role: "reranker",
                reason: "protobuf error".into(),
            },
            RetrievalModelError::ContractMismatch {
                role: "embedding",
                reason: "tensor shapes disagree".into(),
            },
            RetrievalModelError::Inference {
                role: "reranker",
                reason: "non-finite values".into(),
            },
            RetrievalModelError::Cancelled,
        ];
        for variant in structural {
            let rendered = variant.to_string();
            assert!(!rendered.contains(marker), "{rendered}");
            assert!(
                !rendered.contains("<pad>"),
                "no token surface in errors: {rendered}"
            );
        }
    }

    // ------------------------------------------------------------------
    // Artifact-gated behavior: full loads and reference correctness.
    // ------------------------------------------------------------------

    #[test]
    fn session_builder_applies_approved_thread_bounds() {
        // Real ORT construction assertion: the approved caps flow through the
        // runtime's SetIntraOpNumThreads/SetInterOpNumThreads without error.
        bounded_cpu_session_builder(approved_intra_threads())
            .expect("approved intra/inter-op bounds accepted");
        assert_eq!(APPROVED_ORT_INTRA_OP_CAP, 4);
        assert_eq!(APPROVED_ORT_INTER_OP_THREADS, 1);
        let intra = approved_intra_threads();
        assert!(
            (1..=APPROVED_ORT_INTRA_OP_CAP).contains(&intra),
            "intra-op selection left the approved cap: {intra}"
        );
    }

    #[test]
    fn cache_refuses_third_identity_and_retains_residents() {
        let Some(dir) = staged_bundle_dir() else {
            println!("SKIP cache retention: no staged retrieval bundle");
            return;
        };
        if !staged_models_ready(&dir) {
            println!("SKIP cache retention: staged bundle lacks model weights");
            return;
        }
        let _serial = locked(&TEST_LOCK);
        // One real loaded handle is reused under synthetic identities: the
        // policy under test is identity keying and capacity refusal, not
        // engine construction, so no model artifacts are duplicated.
        let real = get_or_load(&dir).expect("load staged bundle");
        let identity_at = |n: u8, root: &Path| BundleIdentity {
            bundle_id: format!("test-bundle-{n}"),
            root: root.to_path_buf(),
        };
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let dir_c = tempfile::tempdir().unwrap();
        let id_a = identity_at(1, dir_a.path());
        let id_b = identity_at(2, dir_b.path());
        let id_c = identity_at(3, dir_c.path());

        let mut cache: Vec<(BundleIdentity, RetrievalModels)> = Vec::new();
        cache.push((id_a.clone(), real.clone()));
        cache.push((id_b.clone(), real.clone()));

        // A third distinct identity is refused typed instead of evicting an
        // active/shadow generation into oblivion.
        let refused =
            reserve_cache_slot(&cache, &id_c).expect_err("third distinct identity must be refused");
        assert!(
            matches!(refused, RetrievalModelError::CacheCapacity { capacity: 2 }),
            "{refused}"
        );
        assert!(
            !refused.to_string().contains("test-bundle"),
            "no raw detail in the refusal: {refused}"
        );
        assert_eq!(cache.len(), MAX_CACHED_BUNDLES);

        // Both resident identities survive with their session set intact.
        assert!(Arc::ptr_eq(
            &cache_lookup(&cache, &id_a).unwrap().0,
            &real.0
        ));
        assert!(Arc::ptr_eq(
            &cache_lookup(&cache, &id_b).unwrap().0,
            &real.0
        ));

        // Full-identity keying: a different bundle id at a known root is a
        // distinct identity, so it is equally refused past capacity.
        let other_bundle_same_root = BundleIdentity {
            bundle_id: "other-bundle".to_string(),
            root: id_a.root.clone(),
        };
        assert!(cache_lookup(&cache, &other_bundle_same_root).is_none());
        assert!(reserve_cache_slot(&cache, &other_bundle_same_root).is_err());

        // An already-resident identity never trips the guard (in get_or_load
        // the lookup path returns before this point).
        reserve_cache_slot(&cache, &id_a).expect("resident identity re-check passes");
    }

    #[test]
    fn reference_token_ids_match_staged_bundle() {
        let Some(dir) = staged_bundle_dir() else {
            println!("SKIP reference token IDs: no staged retrieval bundle");
            return;
        };
        let models = get_or_load(&dir).expect("load staged bundle");
        let engine = &models.0.embedding;

        let encoded = engine
            .tokenizer
            .encode(REFERENCE_QUERY_TEXT, true)
            .expect("encode reference query");
        let ids: Vec<i64> = encoded.get_ids().iter().map(|&id| id as i64).collect();
        assert_eq!(
            ids, REFERENCE_QUERY_IDS,
            "reference query token IDs drifted"
        );

        // Pair formatting/truncation contract mirrors the Sprint 1 probe.
        let long_evidence = "palavra ".repeat(1200);
        let paired = engine
            .tokenizer
            .encode(("pergunta?", long_evidence.as_str()), true)
            .expect("encode truncated pair");
        let pair_ids = paired.get_ids();
        assert_eq!(
            pair_ids.len(),
            512,
            "pair must truncate exactly at the limit"
        );
        assert_eq!(pair_ids[0], 0, "pair starts with <s>");
        assert_eq!(*pair_ids.last().unwrap(), 2, "pair ends with </s>");
    }

    #[test]
    fn concurrent_requests_share_one_cached_session_set() {
        let Some(dir) = staged_bundle_dir() else {
            println!("SKIP concurrency: no staged retrieval bundle");
            return;
        };
        if !staged_models_ready(&dir) {
            println!("SKIP concurrency: staged bundle lacks model weights");
            return;
        }
        let _serial = locked(&TEST_LOCK);
        let barrier = Arc::new(Barrier::new(4));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let barrier = barrier.clone();
                let dir = dir.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    get_or_load(&dir)
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread panicked"))
            .collect();
        let first = results
            .first()
            .expect("at least one result")
            .as_ref()
            .unwrap();
        assert_eq!(results.len(), 4);
        for result in &results {
            let models = result.as_ref().unwrap_or_else(|e| panic!("{e}"));
            assert!(
                Arc::ptr_eq(&models.0, &first.0),
                "duplicate session set initialized"
            );
            assert_eq!(models.identity(), first.identity());
        }
    }

    #[test]
    fn embeddings_match_reference_expectations() {
        let Some(dir) = staged_bundle_dir() else {
            println!("SKIP embeddings: no staged retrieval bundle");
            return;
        };
        if !staged_models_ready(&dir) {
            println!("SKIP embeddings: staged bundle lacks model weights");
            return;
        }
        let _serial = locked(&TEST_LOCK);
        let rss_before = memory_stats::memory_stats().map(|stats| stats.physical_mem);

        let models = get_or_load(&dir).expect("load staged bundle");
        let fixture = fixture();
        // Fixture texts were recorded WITH their approved prefixes applied;
        // the runtime owns prefix application, so strip them here and route
        // each entry through its role-specific API instead.
        const QUERY_PREFIX: &str = "query: ";
        let mut vectors_by_label: BTreeMap<&str, Vec<f32>> = BTreeMap::new();
        let queries: Vec<(&str, &str)> = fixture
            .reference_expectations
            .embedding
            .iter()
            .filter(|entry| entry.text.starts_with(QUERY_PREFIX))
            .map(|entry| (entry.label.as_str(), &entry.text[QUERY_PREFIX.len()..]))
            .collect();
        let documents: Vec<(&str, &str)> = fixture
            .reference_expectations
            .embedding
            .iter()
            .filter(|entry| !entry.text.starts_with(QUERY_PREFIX))
            .map(|entry| {
                (
                    entry.label.as_str(),
                    entry.text.strip_prefix("passage: ").unwrap_or(&entry.text),
                )
            })
            .collect();
        for (embedded, batch) in [
            (
                models.embed_queries_sync(
                    &queries.iter().map(|(_, text)| *text).collect::<Vec<_>>(),
                    &CancellationToken::new(),
                ),
                &queries,
            ),
            (
                models.embed_documents_sync(
                    &documents.iter().map(|(_, text)| *text).collect::<Vec<_>>(),
                    &CancellationToken::new(),
                ),
                &documents,
            ),
        ] {
            let vectors = embedded.expect("embed reference texts");
            assert_eq!(vectors.len(), batch.len());
            for ((label, _), vector) in batch.iter().zip(vectors) {
                vectors_by_label.insert(label, vector);
            }
        }

        let labels: Vec<&str> = fixture
            .reference_expectations
            .embedding
            .iter()
            .map(|entry| entry.label.as_str())
            .collect();
        let vectors: Vec<&Vec<f32>> = labels
            .iter()
            .map(|label| &vectors_by_label[label])
            .collect();
        for vector in &vectors {
            assert_eq!(vector.len(), 768, "manifest dimension");
            assert!(vector.iter().all(|v| v.is_finite()), "finite values");
            let norm: f64 = vector
                .iter()
                .map(|v| (*v as f64) * (*v as f64))
                .sum::<f64>()
                .sqrt();
            assert!(
                (norm - 1.0).abs() <= 1e-3,
                "unit norm after L2 normalization: {norm}"
            );
        }

        if let (Some(before), Some(after)) = (
            rss_before,
            memory_stats::memory_stats().map(|stats| stats.physical_mem),
        ) {
            let delta_mib = after.saturating_sub(before) as f64 / (1024.0 * 1024.0);
            println!("session-load RSS delta: {delta_mib:.1} MiB");
        }

        let cosine = |a: &str, b: &str| -> f64 {
            let x = &vectors_by_label[a];
            let y = &vectors_by_label[b];
            x.iter()
                .zip(y)
                .map(|(p, q)| ((*p as f64) * (*q as f64)))
                .sum()
        };
        for expected in &fixture.reference_expectations.embedding {
            let vector = &vectors_by_label[expected.label.as_str()];
            let norm: f64 = vector
                .iter()
                .map(|v| (*v as f64) * (*v as f64))
                .sum::<f64>()
                .sqrt();
            assert!(
                (norm - expected.norm).abs() <= NORM_TOL,
                "{} norm drifted: {norm} vs {}",
                expected.label,
                expected.norm
            );
            for (index, recorded) in &expected.sample_dims {
                let actual = vector[index.parse::<usize>().unwrap()] as f64;
                assert!(
                    (actual - recorded).abs() <= DIM_TOL,
                    "{} dim {index} drifted: {actual} vs {recorded}",
                    expected.label
                );
            }
            for recorded in &expected.cosine_with {
                let actual = cosine(&expected.label, &recorded.label);
                assert!(
                    (actual - recorded.cosine).abs() <= COSINE_TOL,
                    "{} cosine vs {} drifted: {actual} vs {}",
                    expected.label,
                    recorded.label,
                    recorded.cosine
                );
            }
        }

        // Semantic sanity independent of recorded numbers (Sprint 1 contract).
        let on_topic = cosine("pt_query", "pt_reference_doc");
        let paraphrase = cosine("pt_query", "en_query");
        let unrelated = cosine("pt_query", "en_doc");
        assert!(
            paraphrase > unrelated && on_topic > unrelated,
            "topical/paraphrase content must outrank unrelated text ({on_topic} / {paraphrase} / {unrelated})"
        );
    }

    #[test]
    fn reranker_matches_reference_scores_and_order() {
        let Some(dir) = staged_bundle_dir() else {
            println!("SKIP reranker: no staged retrieval bundle");
            return;
        };
        if !staged_models_ready(&dir) {
            println!("SKIP reranker: staged bundle lacks model weights");
            return;
        }
        let _serial = locked(&TEST_LOCK);
        let models = get_or_load(&dir).expect("load staged bundle");
        let fixture = fixture();

        let group = fixture
            .reference_expectations
            .reranker_pairs
            .iter()
            .find(|group| {
                group
                    .model_dir
                    .ends_with("mmarco-reranker/model_quint8_avx2.onnx")
            })
            .expect("quint8 reranker expectations recorded");
        let pairs: Vec<(String, String)> = group
            .pairs
            .iter()
            .map(|pair| (pair.query.clone(), pair.evidence.clone()))
            .collect();
        let logits = models
            .rerank_sync(&pairs, &CancellationToken::new())
            .expect("score reference pairs");

        assert_eq!(logits.len(), pairs.len());
        assert!(logits.iter().all(|score| score.is_finite()));
        for (index, (recorded, logit)) in group.pairs.iter().zip(&logits).enumerate() {
            let transformed = sigmoid(*logit);
            assert!(
                (transformed - recorded.score).abs() <= SCORE_TOL,
                "pair {index} drifted: sigmoid({logit})={transformed} vs {}",
                recorded.score
            );
        }
        assert!(
            logits[0] > logits[2],
            "complete schedule above unrelated text: {logits:?}"
        );
        assert!(
            logits[3] > logits[4],
            "relevant evidence above distractor: {logits:?}"
        );
    }

    #[test]
    fn async_inference_runs_off_tokio_worker_threads() {
        let Some(dir) = staged_bundle_dir() else {
            println!("SKIP tokio boundary: no staged retrieval bundle");
            return;
        };
        if !staged_models_ready(&dir) {
            println!("SKIP tokio boundary: staged bundle lacks model weights");
            return;
        }
        let _serial = locked(&TEST_LOCK);
        let models = get_or_load(&dir).expect("load staged bundle");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        runtime.block_on(async {
            let polling_thread = std::thread::current().id();
            let cancel = CancellationToken::new();
            // The runtime owns prefix application; callers pass raw queries.
            let vectors = models
                .embed_queries(vec!["teste de fronteira".to_string()], cancel)
                .await
                .expect("async embed");
            assert_eq!(vectors.len(), 1);
            assert_eq!(vectors[0].len(), 768);
            let ran_on = last_inference_thread().expect("inference recorded its thread");
            assert_ne!(
                ran_on, polling_thread,
                "inference must not run on the thread awaiting the future"
            );
        });
    }

    #[test]
    fn cancellation_stops_at_batch_boundaries() {
        let Some(dir) = staged_bundle_dir() else {
            println!("SKIP cancellation: no staged retrieval bundle");
            return;
        };
        if !staged_models_ready(&dir) {
            println!("SKIP cancellation: staged bundle lacks model weights");
            return;
        }
        let _serial = locked(&TEST_LOCK);
        let models = get_or_load(&dir).expect("load staged bundle");
        let cancel = CancellationToken::new();
        cancel.cancel();
        let inference_before = last_inference_thread();

        let embedding = models.embed_queries_sync(&["qualquer coisa"], &cancel);
        assert!(
            matches!(embedding, Err(RetrievalModelError::Cancelled)),
            "{embedding:?}"
        );
        let reranking = models.rerank_sync(&[("q".to_string(), "d".to_string())], &cancel);
        assert!(
            matches!(reranking, Err(RetrievalModelError::Cancelled)),
            "{reranking:?}"
        );

        // Cancelled work never reached inference.
        assert_eq!(last_inference_thread(), inference_before);
    }
}
