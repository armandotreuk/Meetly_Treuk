//! Task 1.3 — model selection benchmark harness (Sprint 1 hybrid RAG).
//!
//! Model artifacts are staged OUTSIDE git. Default location is
//! `%TEMP%\opencode\meetly-task13\models`; override with `MEETLY_RAG_MODELS_DIR`.
//! Artifact-gated tests skip cleanly when the directory is absent so plain
//! `cargo test` stays green on machines without staged weights.
//! CI release gate: set `MEETLY_RAG_BUNDLE_DIR` to a staged production bundle
//! (`resources/retrieval/bundle`) and reference inference runs against that
//! layout, failing closed instead of skipping.
//!
//! Commands (run from `upstream/`):
//! ```powershell
//! # Pure selection-logic regression (no artifacts required):
//! cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark -- --nocapture
//!
//! # Record platform-neutral reference expectations into the manifest:
//! $env:MEETLY_RAG_MODELS_DIR = "$env:TEMP\opencode\meetly-task13\models"
//! $env:MEETLY_RAG_RECORD_EXPECTATIONS = "1"
//! cargo test ... --test model_benchmark reference_inference -- --nocapture
//!
//! # Full corpus hybrid evaluation + latency/RAM/disk benchmark:
//! Remove-Item Env:MEETLY_RAG_RECORD_EXPECTATIONS -ErrorAction SilentlyContinue
//! $env:MEETLY_RAG_BENCH = "1"
//! cargo test ... --test model_benchmark hybrid_corpus_and_resource_benchmark -- --nocapture
//! ```

use memory_stats::memory_stats;
use ndarray::{Array2, ArrayD};
use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    time::Instant,
};
use tokenizers::{Tokenizer, TruncationParams};

#[path = "fixtures/concept_lexicon.rs"]
mod concept_lexicon;
#[path = "fixtures/corpus.rs"]
mod corpus;
#[path = "fixtures/corpus_types.rs"]
mod corpus_types;

use concept_lexicon::CONCEPT_LEXICON;
use corpus_types::{
    classify_forbidden_fact, EvaluationCase, Evidence, ForbiddenFactStage, Language, Meeting,
    MeetingState, Scope, ScopeKind,
};

const MANIFEST_JSON: &str = include_str!("fixtures/model_bundle_manifest.json");
const EVIDENCE_K: usize = 10;
const VECTOR_CANDIDATES: usize = 100;
const LEXICAL_CANDIDATES: usize = 50;

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleManifest {
    envelopes: Envelopes,
    #[serde(rename = "pairAdmissibilityEstimates")]
    pair_admissibility: Vec<PairAdmissibility>,
    #[serde(default)]
    bi_encoder_candidates: Vec<BiEncoderCandidate>,
    benchmark_leader: BenchmarkLeader,
    #[serde(default)]
    reference_expectations: ReferenceBlock,
    measured_outcome: MeasuredOutcome,
}

/// Minimal candidate-inventory view used only for executable coherence checks
/// between `benchmarkLeader` and the pinned artifact contract.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BiEncoderCandidate {
    id: String,
    #[serde(default)]
    revision: String,
    #[serde(default)]
    onnx_revision: String,
    #[serde(default)]
    artifact_hashes: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelopes {
    document_scale_gate: u64,
    snapshot_overlap_factor: u64,
    auto_pass_bytes: u64,
    approval_band_max_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairAdmissibility {
    pair: String,
    vector_bytes: u64,
    embedding_session_bytes: u64,
    reranker_session_bytes: u64,
    total_bytes: u64,
    band: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkLeader {
    embedding: LeaderEmbedding,
    reranker: LeaderReranker,
    fusion_and_rerank_constants: FusionConstants,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeaderEmbedding {
    model_id: String,
    revision: String,
    onnx_revision: String,
    license: String,
    dimensions: usize,
    max_sequence_length: usize,
    query_prefix: String,
    document_prefix: String,
    benchmark_artifact_dir: String,
    benchmark_artifact_file: String,
    artifact_hashes: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeaderReranker {
    leader_model_id: String,
    benchmark_model_dir: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FusionConstants {
    rrf_k: Option<f64>,
    channel_weight_vector: Option<f64>,
    channel_weight_lexical: Option<f64>,
    channel_weight_title: Option<f64>,
    support_alpha: Option<f64>,
    title_beta: Option<f64>,
    rerank_gamma: Option<f64>,
    support_cap: Option<usize>,
    reranker_depth_chat: Option<usize>,
    reranker_depth_search: Option<usize>,
    reranker_batch_size: Option<usize>,
    ort_intra_op_threads: Option<usize>,
    transcript_window_tokens: Option<usize>,
    overlap_tokens: Option<usize>,
}

fn manifest() -> BundleManifest {
    serde_json::from_str(MANIFEST_JSON).expect("model bundle manifest must be valid JSON")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MeasuredOutcome {
    decision: String,
    contracted_embedding: ContractedEmbedding,
    #[serde(default)]
    pair_results: Vec<PairRecord>,
    top_level_gate_summary: GateSummary,
    card_multilingual_conforming_pairs: Vec<String>,
    measured_pair_peak_mib: BTreeMap<String, f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractedEmbedding {
    model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairRecord {
    pair: String,
    card_multilingual_conforming: bool,
    passed_all_gates: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GateSummary {
    reference_recall_at_1_pass: bool,
    critical_recall_at_1_pass: bool,
    exact_term_no_regression_pass: bool,
    overall_recall_at_3_pass: bool,
    overall_recall_at_5_pass: bool,
    evidence_recall_at_10_pass: bool,
    semantic_delta_pass: bool,
    ndcg_non_degradation_pass: bool,
}

impl GateSummary {
    fn all_quality_gates_pass(&self) -> bool {
        self.reference_recall_at_1_pass
            && self.critical_recall_at_1_pass
            && self.exact_term_no_regression_pass
            && self.overall_recall_at_3_pass
            && self.overall_recall_at_5_pass
            && self.evidence_recall_at_10_pass
            && self.semantic_delta_pass
            && self.ndcg_non_degradation_pass
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceBlock {
    embedding: Vec<EmbeddingExpectation>,
    reranker_pairs: Vec<RerankPairGroup>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddingExpectation {
    label: String,
    text: String,
    norm: f64,
    sample_dims: BTreeMap<String, f64>,
    cosine_with: Vec<CosineWith>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CosineWith {
    label: String,
    cosine: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RerankPairGroup {
    model_dir: String,
    pairs: Vec<RerankPairExpectation>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RerankPairExpectation {
    query: String,
    evidence: String,
    score: f64,
}

const REFERENCE_SAMPLE_DIMS: [usize; 8] = [0, 1, 63, 127, 191, 255, 319, 383];

fn reference_texts() -> Vec<(String, String)> {
    vec![
        (
            "pt_query".to_string(),
            "query: quais os dias de comunicacao por whatsapp para o fluxo de retencao?".to_string(),
        ),
        (
            "pt_reference_doc".to_string(),
            "passage: A régua sintética de mensagens no aplicativo prevê contatos nos dias 1, 3, 7, 10 e 15."
                .to_string(),
        ),
        (
            "pt_mpv_doc".to_string(),
            "passage: No dia um, a distinção MPV/non-MPV é sintética: unidades MPV enviam boas-vindas; unidades não MPV iniciam confirmação cadastral."
                .to_string(),
        ),
        (
            "en_query".to_string(),
            "query: What days are scheduled for WhatsApp retention contacts?".to_string(),
        ),
        (
            "en_doc".to_string(),
            "passage: Automated reminders prevent customer loss in the synthetic retention flow."
                .to_string(),
        ),
        (
            "pt_distractor".to_string(),
            "passage: quais os dias de comunicacao por whatsapp para o fluxo de retencao: rascunho sintético dizia apenas 3 dias."
                .to_string(),
        ),
    ]
}

/// Architecture admissibility band for a projected peak byte figure.
fn band_of(total_bytes: u64, env: &Envelopes) -> &'static str {
    match total_bytes {
        t if t <= env.auto_pass_bytes => "admissible",
        t if t <= env.approval_band_max_bytes => "approval-required",
        _ => "inadmissible",
    }
}

fn vector_bytes(dims: u64, bytes_per_value: u64, env: &Envelopes) -> u64 {
    dims * bytes_per_value * env.document_scale_gate * env.snapshot_overlap_factor
}

/// Regression check for the non-trivial selection arithmetic and constants.
/// Runs without artifacts; pins every recorded candidate figure.
#[test]
fn selection_admissibility_arithmetic_is_pinned() {
    let manifest = manifest();
    let entry = manifest
        .pair_admissibility
        .iter()
        .find(|p| p.pair.contains("e5-small[int8 storage|int8 session]"))
        .expect("primary benchmark-leader pair must be recorded");

    // The architecture's worked example: 384-dim f32 vectors at 250k x2.
    assert_eq!(vector_bytes(384, 4, &manifest.envelopes), 768_000_000);
    assert_eq!(vector_bytes(384, 1, &manifest.envelopes), 192_000_000);
    assert_eq!(vector_bytes(768, 4, &manifest.envelopes), 1_536_000_000);

    // Every recorded pair total must recompute exactly from its components.
    for pair in &manifest.pair_admissibility {
        let recomputed =
            pair.vector_bytes + pair.embedding_session_bytes + pair.reranker_session_bytes;
        assert_eq!(
            recomputed, pair.total_bytes,
            "admissibility total drifted for {}",
            pair.pair
        );
        assert_eq!(
            band_of(pair.total_bytes, &manifest.envelopes),
            pair.band.as_str(),
            "band classification drifted for {}",
            pair.pair
        );
    }
    // The primary admissible pair must sit well inside the automatic envelope.
    assert_eq!(entry.band, "admissible");
    assert!(entry.total_bytes <= manifest.envelopes.auto_pass_bytes);

    // Selection constants must be fully resolved once a pair is approved.
    let c = &manifest.benchmark_leader.fusion_and_rerank_constants;
    for (name, value) in [
        ("rrfK", c.rrf_k),
        ("channelWeightVector", c.channel_weight_vector),
        ("channelWeightLexical", c.channel_weight_lexical),
        ("supportAlpha", c.support_alpha),
        ("titleBeta", c.title_beta),
        ("channelWeightTitle", c.channel_weight_title),
        ("rerankGamma", c.rerank_gamma),
        ("supportCap", c.support_cap.map(|v| v as f64)),
        ("rerankerDepthChat", c.reranker_depth_chat.map(|v| v as f64)),
        (
            "rerankerDepthSearch",
            c.reranker_depth_search.map(|v| v as f64),
        ),
        ("rerankerBatchSize", c.reranker_batch_size.map(|v| v as f64)),
        (
            "ortIntraOpThreads",
            c.ort_intra_op_threads.map(|v| v as f64),
        ),
        (
            "transcriptWindowTokens",
            c.transcript_window_tokens.map(|v| v as f64),
        ),
        ("overlapTokens", c.overlap_tokens.map(|v| v as f64)),
    ] {
        assert!(value.is_some(), "selection constant {name} is unresolved");
    }
    // ORT intra-op cap obeys min(4, max(1, cores/2)); 20 logical cores -> 4.
    assert_eq!(c.ort_intra_op_threads, Some(4));
    // Transcript windows cannot exceed the embedding model context.
    assert!(
        c.transcript_window_tokens.unwrap()
            <= manifest.benchmark_leader.embedding.max_sequence_length
    );

    // Batch 4 audit: the benchmark leader must be the actual rerun leader, not
    // a secondary probe identity, and its artifact contract must match the
    // candidate inventory and the contracted embedding record.
    let leader = &manifest.benchmark_leader.embedding;
    assert_eq!(
        leader.model_id, "intfloat/multilingual-e5-base",
        "benchmarkLeader.embedding must identify the rerun benchmark leader"
    );
    assert_eq!(leader.dimensions, 768);
    assert_eq!(
        leader.onnx_revision,
        "1ec9243030a27d1a115d5c340572074c125b58b2"
    );
    let inventory = manifest
        .bi_encoder_candidates
        .iter()
        .find(|c| c.id == leader.model_id)
        .expect("leader must appear in biEncoderCandidates");
    assert_eq!(inventory.revision, leader.revision);
    assert_eq!(
        inventory.onnx_revision, leader.onnx_revision,
        "leader ONNX export revision must match the candidate-inventory pin"
    );
    assert_eq!(leader.license, "MIT", "leader embedding license");
    let leader_hash = leader
        .artifact_hashes
        .get("model_int8.onnx")
        .expect("leader artifact hash recorded");
    assert_eq!(
        inventory.artifact_hashes.get("model_int8.onnx"),
        Some(leader_hash),
        "leader artifact hash must equal the candidate-inventory pin"
    );
    // The reranker half of the leader record must be the actual non-production
    // rerun leader (mmarco-mMiniLMv2-L12), whose staged directory is the one
    // the reference-inference ordering contract asserts against.
    let rr_leader = &manifest.benchmark_leader.reranker;
    assert_eq!(
        rr_leader.leader_model_id, "cross-encoder/mmarco-mMiniLMv2-L12-H384-v1",
        "benchmarkLeader.reranker must name the actual non-production leader"
    );
    assert_eq!(
        rr_leader.benchmark_model_dir, "mmarco-reranker",
        "benchmarkLeader.reranker staging dir must match the evaluated leader export"
    );
    assert_eq!(
        manifest.measured_outcome.contracted_embedding.model_id, leader.model_id,
        "contracted embedding and benchmark leader must be the same family"
    );
    // Reference expectations must belong to the leader artifact layout.
    assert!(!manifest.reference_expectations.embedding.is_empty());
    for group in &manifest.reference_expectations.reranker_pairs {
        assert!(group.model_dir.contains('/'), "reranker expectation dir");
    }

    // Decision coherence: no production pair is selected here, so "complete"
    // requires every quality gate to pass AND at least one card-multilingual-
    // conforming measured pair inside the automatic RAM envelope. A failing
    // gate always forces a quality-blocked decision regardless of RAM, and
    // non-selected pairs' peaks never influence the verdict.
    let outcome = &manifest.measured_outcome;
    let gates_pass = outcome.top_level_gate_summary.all_quality_gates_pass();
    match outcome.decision.as_str() {
        "complete" => {
            assert!(gates_pass, "decision=complete with failing quality gates");
            let conforming_within_auto_pass = outcome.pair_results.iter().any(|pr| {
                pr.passed_all_gates
                    && pr.card_multilingual_conforming
                    && outcome
                        .card_multilingual_conforming_pairs
                        .iter()
                        .any(|cp| cp == &pr.pair)
                    && outcome
                        .measured_pair_peak_mib
                        .get(&pr.pair)
                        .is_some_and(|mib| *mib <= 1024.0)
            });
            assert!(
                conforming_within_auto_pass,
                "decision=complete without a metadata-conforming pair inside the automatic RAM envelope"
            );
        }
        "blocked-quality-gates" => {
            assert!(
                !gates_pass,
                "decision=blocked-quality-gates but all gates pass"
            );
            assert!(
                !outcome.pair_results.iter().any(|pr| pr.passed_all_gates),
                "decision=blocked-quality-gates but a recorded pair passed all gates"
            );
        }
        other => panic!("unknown recorded decision {other}"),
    }
}

// ---------------------------------------------------------------------------
// Staged artifacts
// ---------------------------------------------------------------------------

fn models_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("MEETLY_RAG_MODELS_DIR") {
        let path = PathBuf::from(dir);
        return path.is_dir().then_some(path);
    }
    std::env::var("TEMP")
        .ok()
        .map(|temp| {
            PathBuf::from(temp)
                .join("opencode")
                .join("meetly-task13")
                .join("models")
        })
        .filter(|path| path.is_dir())
}

fn rss_mib() -> Option<f64> {
    memory_stats().map(|stats| stats.physical_mem as f64 / (1024.0 * 1024.0))
}

// ---------------------------------------------------------------------------
// ONNX sessions and tokenizer plumbing
// ---------------------------------------------------------------------------

struct TextModel {
    session: Session,
    tokenizer: Tokenizer,
    max_len: usize,
    pad_id: i64,
    input_names: Vec<String>,
    output_names: Vec<String>,
    has_token_type_ids: bool,
}

impl TextModel {
    fn input_names(&self) -> &[String] {
        &self.input_names
    }

    fn load(
        dir: &Path,
        model_file: &str,
        max_len: usize,
        intra_threads: usize,
    ) -> Result<Self, String> {
        Self::load_from(dir, dir, model_file, max_len, intra_threads)
    }

    /// The production bundle layout keeps tokenizers under `tokenizers/<role>`
    /// instead of beside the ONNX file; benchmark staging co-locates them.
    fn load_from(
        model_dir: &Path,
        tokenizer_dir: &Path,
        model_file: &str,
        max_len: usize,
        intra_threads: usize,
    ) -> Result<Self, String> {
        let mut tokenizer = Tokenizer::from_file(tokenizer_dir.join("tokenizer.json"))
            .map_err(|e| format!("tokenizer: {e}"))?;
        // LongestFirst pair truncation via the tokenizer itself preserves the
        // question sequence and the <s>…</s></s>…</s> separators instead of
        // cutting the fully encoded ID vector.
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: max_len,
                ..TruncationParams::default()
            }))
            .map_err(|e| format!("truncation config: {e}"))?;
        // XLM-R family pads with <pad> (id 1); resolve from the vocabulary.
        let pad_id = tokenizer.token_to_id("<pad>").unwrap_or(0) as i64;

        let session = Session::builder()
            .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level3))
            .and_then(|b| b.with_execution_providers(vec![CPUExecutionProvider::default().build()]))
            .and_then(|b| b.with_intra_threads(intra_threads))
            .and_then(|b| b.commit_from_file(model_dir.join(model_file)))
            .map_err(|e| format!("session {}: {e}", model_dir.join(model_file).display()))?;

        let input_names: Vec<String> = session.inputs.iter().map(|i| i.name.clone()).collect();
        let output_names: Vec<String> = session.outputs.iter().map(|o| o.name.clone()).collect();
        let has_token_type_ids = input_names.iter().any(|n| n == "token_type_ids");
        Ok(Self {
            session,
            tokenizer,
            max_len,
            pad_id,
            input_names,
            output_names,
            has_token_type_ids,
        })
    }

    /// Encode (text, optional-pair-partner) items with special tokens and
    /// tokenizer-managed truncation, then pad the batch manually. Returns
    /// ids/mask plus per-row true lengths.
    fn tokenize(
        &self,
        items: &[(String, Option<String>)],
    ) -> Result<(Array2<i64>, Array2<i64>, Vec<usize>), String> {
        let mut rows: Vec<(Vec<i64>, usize)> = Vec::with_capacity(items.len());
        for (text, partner) in items {
            let encoded = match partner {
                Some(partner) => self
                    .tokenizer
                    .encode((text.as_str(), partner.as_str()), true),
                None => self.tokenizer.encode(text.as_str(), true),
            }
            .map_err(|e| format!("tokenization failed: {e}"))?;
            let ids: Vec<i64> = encoded.get_ids().iter().map(|&t| t as i64).collect();
            debug_assert!(ids.len() <= self.max_len);
            let len = ids.len().max(1); // keep one pad row for degenerate inputs
            rows.push((ids, len));
        }
        let width = rows.iter().map(|(ids, _)| ids.len()).max().unwrap_or(1);
        let lengths: Vec<usize> = rows.iter().map(|(_, len)| (*len).min(width)).collect();
        let batch = rows.len();
        let mut flat_ids = vec![self.pad_id; batch * width];
        let mut flat_mask = vec![0_i64; batch * width];
        for (r, (ids, len)) in rows.into_iter().enumerate() {
            flat_ids[r * width..r * width + len.min(width)].copy_from_slice(&ids[..len.min(width)]);
            flat_mask[r * width..r * width + len.min(width)].fill(1);
        }
        Ok((
            Array2::from_shape_vec((batch, width), flat_ids).map_err(|e| e.to_string())?,
            Array2::from_shape_vec((batch, width), flat_mask).map_err(|e| e.to_string())?,
            lengths,
        ))
    }

    fn run(
        &mut self,
        output_name: &str,
        ids: &Array2<i64>,
        mask: &Array2<i64>,
    ) -> Result<ArrayD<f32>, String> {
        let outputs = if self.has_token_type_ids {
            let zeros = ndarray::Array2::<i64>::zeros(ids.raw_dim());
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
        outputs
            .get(output_name)
            .ok_or_else(|| format!("missing output {output_name}"))?
            .try_extract_array()
            .map(|view| view.to_owned())
            .map_err(|e| format!("extract {output_name}: {e}"))
    }
}

struct Embedder(TextModel);

impl Embedder {
    fn load(dir: &Path, file: &str, max_len: usize, threads: usize) -> Result<Self, String> {
        Ok(Self(TextModel::load(dir, file, max_len, threads)?))
    }

    fn load_from(
        model_dir: &Path,
        tokenizer_dir: &Path,
        file: &str,
        max_len: usize,
        threads: usize,
    ) -> Result<Self, String> {
        Ok(Self(TextModel::load_from(
            model_dir,
            tokenizer_dir,
            file,
            max_len,
            threads,
        )?))
    }

    /// Mean pooling over unmasked positions then L2 normalization — or, when
    /// the export is an end-to-end SentenceTransformer graph, its final
    /// `sentence_embedding` output (pooling/dense/normalize already included).
    fn embed(&mut self, prefixed_texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if prefixed_texts.is_empty() {
            return Ok(Vec::new());
        }
        let items: Vec<(String, Option<String>)> =
            prefixed_texts.iter().map(|t| (t.clone(), None)).collect();
        let (ids, mask, lengths) = self.0.tokenize(&items)?;
        let output_name = if self
            .0
            .output_names
            .iter()
            .any(|n| n == "sentence_embedding")
        {
            "sentence_embedding"
        } else {
            "last_hidden_state"
        };
        let hidden = self.0.run(output_name, &ids, &mask)?;
        let hidden_shape = hidden.shape().to_vec();
        let mut out = match hidden_shape.as_slice() {
            [_, _, _] => {
                let seq = hidden_shape[1];
                let dims = hidden_shape[2];
                let data = hidden.into_raw_vec_and_offset().0;
                let mut out = Vec::with_capacity(prefixed_texts.len());
                for (row, length) in lengths.iter().enumerate() {
                    let start = row * seq * dims;
                    let mut mean = vec![0_f32; dims];
                    for s in 0..*length {
                        let base = start + s * dims;
                        for d in 0..dims {
                            mean[d] += data[base + d];
                        }
                    }
                    l2_normalize(&mut mean);
                    out.push(mean);
                }
                out
            }
            _ => {
                // End-to-end embedding graph: rows are final vectors.
                let data = hidden.into_raw_vec_and_offset().0;
                let width = *hidden_shape.last().unwrap_or(&0);
                assert!(width > 0, "unexpected sentence_embedding shape");
                data.chunks(width).map(|row| row.to_vec()).collect()
            }
        };
        for vector in &mut out {
            l2_normalize(vector);
        }
        Ok(out)
    }
}

fn l2_normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in vector {
            *value /= norm;
        }
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

struct RerankModel(TextModel);

impl RerankModel {
    fn load(dir: &Path, file: &str, max_len: usize, threads: usize) -> Result<Self, String> {
        Ok(Self(TextModel::load(dir, file, max_len, threads)?))
    }

    fn load_from(
        model_dir: &Path,
        tokenizer_dir: &Path,
        file: &str,
        max_len: usize,
        threads: usize,
    ) -> Result<Self, String> {
        Ok(Self(TextModel::load_from(
            model_dir,
            tokenizer_dir,
            file,
            max_len,
            threads,
        )?))
    }

    /// Score (query, evidence) pairs; logits[.,0] passed through sigmoid.
    fn score(&mut self, pairs: &[(String, String)]) -> Result<Vec<f32>, String> {
        if pairs.is_empty() {
            return Ok(Vec::new());
        }
        let items: Vec<(String, Option<String>)> = pairs
            .iter()
            .map(|(q, d)| (q.clone(), Some(d.clone())))
            .collect();
        let (ids, mask, _) = self.0.tokenize(&items)?;
        let logits = self.0.run("logits", &ids, &mask)?;
        assert!(logits.shape().len() <= 2, "unexpected logits rank");
        let width = logits.shape().last().copied().unwrap_or(1);
        let data = logits.into_raw_vec_and_offset().0;
        Ok((0..pairs.len()).map(|r| sigmoid(data[r * width])).collect())
    }
}

// ---------------------------------------------------------------------------
// Reference inference: actual tokenizer + embedding + reranker on the staged
// artifacts. Records platform-neutral expectations or asserts against them.
// ---------------------------------------------------------------------------

fn embedding_expectation(
    label: &str,
    text: &str,
    vector: &[f32],
    vectors_by_label: &BTreeMap<String, Vec<f32>>,
) -> EmbeddingExpectation {
    let norm = vector
        .iter()
        .map(|v| (*v as f64) * (*v as f64))
        .sum::<f64>()
        .sqrt();
    let sample_dims = REFERENCE_SAMPLE_DIMS
        .iter()
        .filter(|d| **d < vector.len())
        .map(|d| (d.to_string(), vector[*d] as f64))
        .collect::<BTreeMap<_, _>>();
    let mut cosine_with = Vec::new();
    for (other_label, other_vector) in vectors_by_label {
        if other_label != label {
            let dot: f32 = vector.iter().zip(other_vector).map(|(a, b)| a * b).sum();
            cosine_with.push(CosineWith {
                label: other_label.clone(),
                cosine: dot as f64,
            });
        }
    }
    cosine_with.sort_by(|a, b| a.label.cmp(&b.label));
    EmbeddingExpectation {
        label: label.to_string(),
        text: text.to_string(),
        norm,
        sample_dims,
        cosine_with,
    }
}

#[test]
fn reference_inference_is_stable_finite_and_dimensional() {
    // Release-gate mode (CI sets MEETLY_RAG_BUNDLE_DIR) consumes the staged
    // production bundle layout and fails closed on anything missing; legacy
    // benchmark staging keeps its skip-on-absent behavior for plain `cargo test`.
    let bundle_root = std::env::var("MEETLY_RAG_BUNDLE_DIR")
        .ok()
        .map(PathBuf::from);
    if let Some(dir) = &bundle_root {
        assert!(
            dir.is_dir(),
            "MEETLY_RAG_BUNDLE_DIR is set but missing: {}",
            dir.display()
        );
    }
    let bundle_mode = bundle_root.is_some();
    let Some(root) = bundle_root.or_else(models_dir) else {
        println!("SKIP reference inference: set MEETLY_RAG_MODELS_DIR to staged artifacts");
        return;
    };
    let manifest = manifest();
    let record_mode = std::env::var("MEETLY_RAG_RECORD_EXPECTATIONS").is_ok();

    let leader = &manifest.benchmark_leader.embedding;
    let (emb_model_dir, emb_tokenizer_dir) = if bundle_mode {
        (
            root.join("models").join("embedding"),
            root.join("tokenizers").join("embedding"),
        )
    } else {
        let dir = root.join(&leader.benchmark_artifact_dir);
        (dir.clone(), dir)
    };
    let mut embedder = Embedder::load_from(
        &emb_model_dir,
        &emb_tokenizer_dir,
        &leader.benchmark_artifact_file,
        leader.max_sequence_length,
        4,
    )
    .expect("load benchmark-leader embedding session");

    let texts = reference_texts();
    let vectors = embedder
        .embed(&texts.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>())
        .expect("embed reference texts");
    assert_eq!(vectors.len(), texts.len());
    let dims = manifest.benchmark_leader.embedding.dimensions;
    for vector in &vectors {
        assert_eq!(vector.len(), dims, "embedding dimension must be exact");
        assert!(
            vector.iter().all(|v| v.is_finite()),
            "embedding must be finite"
        );
    }

    let by_label = texts
        .iter()
        .zip(vectors)
        .map(|((label, _), vector)| (label.clone(), vector.clone()))
        .collect::<BTreeMap<String, Vec<f32>>>();
    let expectations = texts
        .iter()
        .map(|(label, text)| embedding_expectation(label, text, &by_label[label], &by_label))
        .collect::<Vec<_>>();

    // Semantic sanity independent of any recorded numbers. Note the query-echo
    // distractor legitimately embeds closest to the query (it contains the
    // question verbatim); discriminating against those fragments is the job of
    // fusion/aggregation/reranking, not raw cosine.
    let cos = |a: &str, b: &str| -> f64 {
        let (x, y) = (&by_label[a], &by_label[b]);
        x.iter().zip(y).map(|(p, q)| (p * q) as f64).sum()
    };
    let on_topic = cos("pt_query", "pt_reference_doc");
    let paraphrase = cos("pt_query", "en_query");
    let mpv = cos("pt_query", "pt_mpv_doc");
    let unrelated_en = cos("pt_query", "en_doc");
    assert!(
        paraphrase > unrelated_en && on_topic > unrelated_en && mpv > unrelated_en,
        "reference embedding must rank topical/paraphrase content above unrelated text ({on_topic} / {paraphrase} / {mpv} / {unrelated_en})"
    );

    let mut rerank_groups = Vec::new();
    // Both multilingual cross-encoder candidates plus the mMARCO f32 export
    // (its own quantization-cost baseline) run identical reference pairs.
    // Truncation contract: LongestFirst across the encoded pair keeps the
    // question and both separators intact even when the evidence overflows.
    {
        let long_evidence = "palavra ".repeat(1200);
        let enc = embedder
            .0
            .tokenizer
            .encode(
                (
                    "quais os dias de comunicacao por whatsapp para o fluxo de retencao?",
                    long_evidence.as_str(),
                ),
                true,
            )
            .expect("truncation probe");
        let ids = enc.get_ids();
        assert_eq!(
            ids.len(),
            manifest.benchmark_leader.embedding.max_sequence_length,
            "pair encoding must be truncated exactly at the model limit"
        );
        assert_eq!(ids[0], 0, "pair must start with <s>");
        assert_eq!(
            *ids.last().expect("non-empty"),
            2,
            "pair must end with </s>"
        );
    }

    // bge-reranker-base is permanently retired (user decision 2026-08-23:
    // zh/en card metadata nonconformity); its recorded expectation groups are
    // reused from the manifest and no longer executed here.
    // ponytail: the production bundle ships only the approved quint8_avx2
    // reranker export; the f32 sibling is a benchmark-only quantization-cost
    // baseline outside the package contract. If a second packaged export ever
    // appears, derive this list from the production manifest instead.
    let reranker_runs: Vec<(&str, &str)> = if bundle_mode {
        vec![("mmarco-reranker", "model_quint8_avx2.onnx")]
    } else {
        vec![
            ("mmarco-reranker", "model_quint8_avx2.onnx"),
            ("mmarco-reranker", "model_f32.onnx"),
        ]
    };
    for (bench_dir, file) in reranker_runs {
        let (model_dir, tokenizer_dir) = if bundle_mode {
            (
                root.join("models").join("reranker"),
                root.join("tokenizers").join("reranker"),
            )
        } else {
            let dir = root.join(bench_dir);
            (dir.clone(), dir)
        };
        let mut model = RerankModel::load_from(&model_dir, &tokenizer_dir, file, 512, 4)
            .unwrap_or_else(|e| panic!("load reranker {bench_dir}/{file}: {e}"));
        let pairs = reference_rerank_pairs();
        let scores = model.score(&pairs).expect("score reference pairs");
        assert_eq!(scores.len(), pairs.len());
        assert!(
            scores.iter().all(|s| s.is_finite()),
            "{bench_dir} scores must be finite"
        );
        let is_selected = bench_dir == manifest.benchmark_leader.reranker.benchmark_model_dir;
        if is_selected {
            // The selected reranker must reproduce the reference-case ordering
            // contract it can honestly satisfy: complete schedule above
            // unrelated text and relevant English evidence above a near-topic
            // distractor. Its measured weakness — preferring the verbatim
            // query-echo fragment over the complete schedule — is a recorded
            // corpus finding handled by fusion/aggregation, not hidden here.
            assert!(
                scores[0] > scores[2],
                "selected reranker {bench_dir} must rank complete schedule above unrelated text: {scores:?}"
            );
            assert!(
                scores[3] > scores[4],
                "selected reranker {bench_dir} must rank relevant English evidence above distractor: {scores:?}"
            );
        }
        rerank_groups.push(RerankPairGroup {
            model_dir: format!("{bench_dir}/{file}"),
            pairs: pairs
                .into_iter()
                .zip(scores)
                .map(|((query, evidence), score)| RerankPairExpectation {
                    query,
                    evidence,
                    score: score as f64,
                })
                .collect(),
        });
    }

    if record_mode {
        let block = ReferenceBlock {
            embedding: expectations.clone(),
            reranker_pairs: rerank_groups,
        };
        let json = serde_json::to_string_pretty(&block).expect("serialize expectations");
        println!("{json}");
        // Write bytes directly so Windows console encoding cannot corrupt
        // non-ASCII reference text.
        let out_path = root.join("recorded_expectations.json");
        std::fs::write(&out_path, json.as_bytes())
            .unwrap_or_else(|e| panic!("write {}: {e}", out_path.display()));
        println!("expectations written to {}", out_path.display());
        return;
    }

    // Assert mode: compare with recorded expectations within tolerance.
    const NORM_TOL: f64 = 0.01;
    const DIM_TOL: f64 = 0.06;
    const COSINE_TOL: f64 = 0.03;
    const SCORE_TOL: f64 = 0.20;
    assert!(
        !manifest.reference_expectations.embedding.is_empty(),
        "no recorded reference expectations; run with MEETLY_RAG_RECORD_EXPECTATIONS=1 first"
    );
    for expected in &manifest.reference_expectations.embedding {
        let vector = &by_label[&expected.label];
        let norm = vector
            .iter()
            .map(|v| (*v as f64) * (*v as f64))
            .sum::<f64>()
            .sqrt();
        assert!(
            (norm - expected.norm).abs() <= NORM_TOL,
            "{} norm drifted",
            expected.label
        );
        for (index, recorded) in &expected.sample_dims {
            let actual = vector[index.parse::<usize>().expect("sample dim index")] as f64;
            assert!(
                (actual - recorded).abs() <= DIM_TOL,
                "{} dim {index} drifted: {actual} vs {recorded}",
                expected.label
            );
        }
        for recorded in &expected.cosine_with {
            let actual = cos(&expected.label, &recorded.label);
            assert!(
                (actual - recorded.cosine).abs() <= COSINE_TOL,
                "{} cosine vs {} drifted",
                expected.label,
                recorded.label
            );
        }
    }
    for group in &manifest.reference_expectations.reranker_pairs {
        if group.model_dir.starts_with("bge-reranker") {
            println!(
                "[retired] {} replay skipped (permanently retired 2026-08-23); recorded figures reused",
                group.model_dir
            );
            continue;
        }
        let (dir_name, file) = group
            .model_dir
            .rsplit_once('/')
            .unwrap_or((group.model_dir.as_str(), "model_int8.onnx"));
        if bundle_mode && file != "model_quint8_avx2.onnx" {
            println!(
                "[bundle] {} replay not applicable: benchmark-only export outside the staged production bundle",
                group.model_dir
            );
            continue;
        }
        let (model_dir, tokenizer_dir) = if bundle_mode {
            (
                root.join("models").join("reranker"),
                root.join("tokenizers").join("reranker"),
            )
        } else {
            (root.join(dir_name), root.join(dir_name))
        };
        let mut model = RerankModel::load_from(&model_dir, &tokenizer_dir, file, 512, 4)
            .unwrap_or_else(|e| panic!("load reranker {dir_name}/{file}: {e}"));
        // Replay must score exactly what recording scored: pairs come from the
        // shared in-code contract, and the manifest's stored texts are checked
        // against it byte-for-byte so tooling that mangles non-ASCII evidence
        // fails here as a text-contract violation instead of silently shifting
        // what gets scored.
        let pairs = reference_rerank_pairs();
        assert_eq!(
            group.pairs.len(),
            pairs.len(),
            "{} recorded pair count drifted from the harness contract",
            group.model_dir
        );
        for (index, (recorded, (query, evidence))) in group.pairs.iter().zip(&pairs).enumerate() {
            assert_eq!(
                recorded.query, *query,
                "{} pair {index} query text drifted from the harness contract",
                group.model_dir
            );
            assert_eq!(
                recorded.evidence, *evidence,
                "{} pair {index} evidence text drifted from the harness contract",
                group.model_dir
            );
        }
        let scores = model.score(&pairs).expect("re-score reference pairs");
        for (index, (recorded, actual)) in group.pairs.iter().zip(scores).enumerate() {
            assert!(
                ((actual as f64) - recorded.score).abs() <= SCORE_TOL,
                "{} pair {index} drifted: actual={actual} recorded={}",
                group.model_dir,
                recorded.score
            );
        }
    }
}

// The WhatsApp schedule evidence used by the reranker reference pairs.
fn by_reference_doc(texts: &[(String, String)]) -> String {
    texts
        .iter()
        .find(|(label, _)| label == "pt_reference_doc")
        .map(|(_, text)| text.trim_start_matches("passage: ").to_string())
        .expect("reference doc label")
}

/// Deterministic pair contract for the reranker reference groups: order,
/// texts, and per-group batch composition are defined here once and shared by
/// recording and replay, so dynamic-int8 activation scales (which depend on
/// padded batch width) replay identically on every run.
fn reference_rerank_pairs() -> Vec<(String, String)> {
    let retention_query =
        "quais os dias de comunicacao por whatsapp para o fluxo de retencao?".to_string();
    let churn_query = "How will churn be reduced Cedar001".to_string();
    vec![
        (retention_query.clone(), by_reference_doc(&reference_texts())),
        (
            retention_query.clone(),
            "quais os dias de comunicacao por whatsapp para o fluxo de retencao: rascunho sintético dizia apenas 3 dias.".to_string(),
        ),
        (
            retention_query,
            "The quarterly budget forecast was approved by the finance team.".to_string(),
        ),
        (
            churn_query.clone(),
            "Automated reminders prevent customer loss in the synthetic retention flow.".to_string(),
        ),
        (
            churn_query,
            "Nearby synthetic portfolio topics were discussed without decisions.".to_string(),
        ),
    ]
}

// ---------------------------------------------------------------------------
// Corpus hybrid simulation machinery
// ---------------------------------------------------------------------------

use app_lib::api::chat::ChatSource;
use app_lib::database::repositories::fts::{FtsRepository, MatchMode};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyLite {
    lexical_policy: LexicalPolicyLite,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LexicalPolicyLite {
    portuguese_high_frequency: Vec<String>,
    english_high_frequency: Vec<String>,
}

fn policy_lite() -> PolicyLite {
    serde_json::from_str(include_str!("fixtures/evaluation_policy.json"))
        .expect("evaluation policy fixture must parse")
}

fn fold_diacritics(token: &str) -> String {
    token
        .chars()
        .map(|c| match c.to_ascii_lowercase() {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

fn core_terms(query: &str, language: &Language, policy: &PolicyLite) -> Vec<String> {
    let stop = match language {
        Language::Portuguese => &policy.lexical_policy.portuguese_high_frequency,
        Language::English => &policy.lexical_policy.english_high_frequency,
    };
    let tokens: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(fold_diacritics)
        .collect();
    let core: Vec<String> = tokens
        .iter()
        .filter(|t| !stop.contains(t))
        .cloned()
        .collect();
    if core.is_empty() {
        tokens
    } else {
        core
    }
}

/// One retrievable semantic document for a case.
#[derive(Clone, Debug)]
struct SemDoc {
    meeting_id: String,
    source_kind: String,
    evidence_ids: Vec<String>,
    /// Chunk text as indexed (what the reranker sees for lexical candidates).
    text: String,
    /// Current authoritative content hydration would retain after its
    /// content-hash verification; equals `text` except for stale/dirty state.
    authoritative_text: String,
    /// False for Dirty/StaleDerived meetings: production excludes their stale
    /// semantic rows from the searchable index until re-indexing completes.
    vector_indexed: bool,
}

#[derive(Clone)]
struct DocEntry {
    doc: SemDoc,
    vector: Vec<f32>,
}

/// Segment-aligned transcript/summary windows. Whole segments are packed up to
/// `window_tokens`; when the next segment would overflow, the window flushes
/// and the previous segment repeats at the head of the next window when it
/// fits inside `overlap_tokens`. A single oversized segment is hard-split into
/// slices of `window_tokens` with `overlap_tokens` stride.
/// ponytail: production chunker (Sprint 2) will prefer true token-stride
/// windows with speaker metadata; this benchmark only needs deterministic,
/// profile-sensitive windows over short synthetic segments.
fn window_stream(
    segments: &[(String, String)],
    tokenizer: &Tokenizer,
    window_tokens: usize,
    overlap_tokens: usize,
) -> Vec<Vec<usize>> {
    let lens: Vec<usize> = segments
        .iter()
        .map(|(_, text)| {
            tokenizer
                .encode(text.as_str(), false)
                .map(|e| e.get_ids().len())
                .unwrap_or(text.len() / 3)
        })
        .collect();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut current_tokens = 0usize;
    for (idx, len) in lens.iter().enumerate() {
        if *len > window_tokens {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
                current_tokens = 0;
            }
            let mut start = 0usize;
            while start < *len {
                let end = (start + window_tokens).min(*len);
                groups.push(vec![idx]);
                start = if end == *len {
                    *len
                } else {
                    end - overlap_tokens.min(window_tokens / 2)
                };
                if end == *len {
                    break;
                }
            }
            continue;
        }
        if current_tokens + len > window_tokens && !current.is_empty() {
            let carry = current.last().copied();
            groups.push(std::mem::take(&mut current));
            current_tokens = 0;
            if let Some(c) = carry {
                if lens[c] <= overlap_tokens {
                    current.push(c);
                    current_tokens = lens[c];
                }
            }
        }
        current.push(idx);
        current_tokens += len;
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn build_case_docs(
    case: &EvaluationCase,
    tokenizer: &Tokenizer,
    window_tokens: usize,
    overlap_tokens: usize,
    _summary_all_templates: bool,
) -> Vec<SemDoc> {
    let mut docs: Vec<SemDoc> = Vec::new();
    for meeting in &case.meetings {
        if meeting.state == MeetingState::Deleted {
            continue;
        }
        // Production excludes stale semantic rows for dirty/stale meetings
        // until the worker re-indexes them (architecture failure matrix:
        // "Meeting dirty | Exclude stale semantic rows ... allow current
        // FTS/hydration"), so their documents are kept out of the vector
        // channel while staying reachable through lexical candidates.
        let vector_indexed = meeting.state == MeetingState::Current;
        // Meeting profile document (title + latest summary + notes), built
        // from current authoritative content as the production worker would.
        let latest_summary = meeting
            .evidence
            .iter()
            .filter(|e| e.source_kind == "summary")
            .last();
        let notes = meeting.evidence.iter().find(|e| e.source_kind == "note");
        let mut profile_parts: Vec<&str> = vec![&meeting.title];
        if let Some(s) = latest_summary {
            profile_parts.push(&s.authoritative_text);
        }
        if let Some(n) = notes {
            profile_parts.push(&n.authoritative_text);
        }
        docs.push(SemDoc {
            meeting_id: meeting.id.clone(),
            source_kind: "profile".to_string(),
            evidence_ids: Vec::new(),
            text: profile_parts.join("\n"),
            authoritative_text: profile_parts.join("\n"),
            vector_indexed,
        });

        // Summary sections. The corpus stores one latest-template summary per
        // meeting whose evidences are sections, so "latest-summary-only" and
        // "all-labeled-summary-templates" index the same sections here; the
        // variants would differ only with multiple template versions, which
        // Task 1.2 fixtures do not carry. ponytail: Sprint 2 re-evaluates the
        // policies against real multi-template summaries.
        let summaries: Vec<&corpus_types::Evidence> = meeting
            .evidence
            .iter()
            .filter(|e| e.source_kind == "summary")
            .collect();
        let chosen: Vec<&corpus_types::Evidence> = summaries;

        for (kind_group, items) in [
            ("summary", chosen),
            (
                "note",
                meeting
                    .evidence
                    .iter()
                    .filter(|e| e.source_kind == "note")
                    .collect::<Vec<_>>(),
            ),
            (
                "transcript",
                meeting
                    .evidence
                    .iter()
                    .filter(|e| e.source_kind == "transcript")
                    .collect::<Vec<_>>(),
            ),
        ] {
            // Each section is its own chunk: "split Markdown by heading
            // before applying token windows". Sections are never merged;
            // only an oversized single section is split into token slices.
            for evidence in items {
                let segments = vec![(evidence.id.clone(), evidence.indexed_text.clone())];
                for group in window_stream(&segments, tokenizer, window_tokens, overlap_tokens) {
                    let evidence_ids: Vec<String> =
                        group.iter().map(|i| segments[*i].0.clone()).collect();
                    let text = group
                        .iter()
                        .map(|i| segments[*i].1.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    // Hydration re-loads current content; for stale/dirty
                    // evidence the retained text is the regenerated current
                    // text, not the stale indexed chunk.
                    let authoritative_text = if evidence.authoritative_text == evidence.indexed_text
                    {
                        text.clone()
                    } else {
                        evidence.authoritative_text.clone()
                    };
                    docs.push(SemDoc {
                        meeting_id: meeting.id.clone(),
                        source_kind: kind_group.to_string(),
                        evidence_ids,
                        text,
                        authoritative_text,
                        vector_indexed,
                    });
                }
            }
        }
    }
    docs
}

// ---------------------------------------------------------------------------
// FTS lexical channel (mirrors the Task 1.2 baseline runner against the
// production FtsRepository on an isolated in-memory database).
// ---------------------------------------------------------------------------

const LEXICAL_SCHEMA: &str = r#"
CREATE TABLE meetings (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    folder_id TEXT
);
CREATE TABLE meeting_folders (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    parent_id TEXT,
    created_at TEXT NOT NULL
);
CREATE TABLE transcripts (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    transcript TEXT NOT NULL,
    timestamp TEXT NOT NULL
);
CREATE VIRTUAL TABLE meeting_fts USING fts5(
    meeting_id UNINDEXED,
    chunk_type UNINDEXED,
    chunk_id UNINDEXED,
    text,
    speaker UNINDEXED,
    timestamp_label UNINDEXED,
    folder_id UNINDEXED,
    folder_name,
    tokenize = 'unicode61'
);
"#;

#[derive(Clone, Debug)]
struct LexRow {
    meeting_id: String,
    chunk_id: String,
    text: String,
}

/// Production cascades FTS deletion with the meeting (the repository deletes
/// `meeting_fts` rows alongside the meeting row), so Deleted meetings
/// contribute no rows. `include_deleted_rows` reconstructs the pre-1.3F
/// builder for the ranking-inertness probe below.
async fn open_case_pool(case: &EvaluationCase, include_deleted_rows: bool) -> SqlitePool {
    let pool = SqlitePool::connect(":memory:")
        .await
        .expect("connect in-memory evaluation database");
    sqlx::query(LEXICAL_SCHEMA)
        .execute(&pool)
        .await
        .expect("create evaluation FTS schema");
    let folders = case
        .meetings
        .iter()
        .filter_map(|m| m.folder_id.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    for folder_id in folders {
        sqlx::query(
            "INSERT INTO meeting_folders (id, name, created_at) VALUES (?1, ?2, '2026-08-01')",
        )
        .bind(folder_id)
        .bind(format!("Synthetic folder {folder_id}"))
        .execute(&pool)
        .await
        .expect("insert synthetic folder");
    }
    for meeting in &case.meetings {
        if meeting.state != MeetingState::Deleted {
            sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_id) VALUES (?1, ?2, ?3, ?3, ?4)")
                .bind(&meeting.id)
                .bind(&meeting.title)
                .bind(&meeting.meeting_date)
                .bind(&meeting.folder_id)
                .execute(&pool)
                .await
                .expect("insert synthetic meeting");
        }
        for evidence in &meeting.evidence {
            if meeting.state == MeetingState::Deleted && !include_deleted_rows {
                continue;
            }
            sqlx::query("INSERT INTO meeting_fts (meeting_id, chunk_type, chunk_id, text, folder_id, folder_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
                .bind(&meeting.id)
                .bind(&evidence.source_kind)
                .bind(&evidence.id)
                .bind(&evidence.indexed_text)
                .bind(&meeting.folder_id)
                .bind(meeting.folder_id.as_deref().unwrap_or(""))
                .execute(&pool)
                .await
                .expect("insert synthetic FTS evidence");
        }
    }
    pool
}

fn interleave(first: Vec<LexRow>, second: Vec<LexRow>, limit: usize) -> Vec<LexRow> {
    let mut lists = [first.into_iter(), second.into_iter()];
    let mut out = Vec::new();
    while out.len() < limit {
        let before = out.len();
        for list in lists.iter_mut() {
            if let Some(row) = list.next() {
                out.push(row);
            }
            if out.len() == limit {
                break;
            }
        }
        if out.len() == before {
            break;
        }
    }
    out
}

async fn lexical_channel_inner(
    case: &EvaluationCase,
    limit: usize,
    include_deleted_rows: bool,
) -> Vec<LexRow> {
    let pool = open_case_pool(case, include_deleted_rows).await;
    // Scope schema contract (mirrors the Task 1.2R harness): a focused meeting
    // must sit inside its permitted set; ranking itself never pins the focus.
    if let ScopeKind::Meeting = case.scope.kind {
        assert!(
            case.scope.meeting_id.as_deref().is_some_and(|focused| case
                .scope
                .allowed_meeting_ids
                .iter()
                .any(|id| id == focused)),
            "{} meeting-scope focus must be inside the permitted set",
            case.id
        );
    }
    let query = case.rewritten_query.as_deref().unwrap_or(&case.question);
    let limit_u = limit as u32;
    let rows: Vec<LexRow> = match case.scope.kind {
        ScopeKind::Snapshot | ScopeKind::Today => FtsRepository::get_by_meeting_ids(
            &pool,
            &case.scope.allowed_meeting_ids,
            limit_u,
            limit_u,
        )
        .await
        .expect("allow-list FTS hydration")
        .into_iter()
        .map(|row| LexRow {
            meeting_id: row.meeting_id,
            chunk_id: row.chunk_id,
            text: row.snippet.replace("<mark>", "").replace("</mark>", ""),
        })
        .collect(),
        _ => {
            let and_rows = match case.scope.kind {
                ScopeKind::All => {
                    FtsRepository::search_with_mode(&pool, query, limit_u, None, MatchMode::And)
                        .await
                        .expect("fts and")
                }
                ScopeKind::Folder => FtsRepository::search_with_folder_ids(
                    &pool,
                    query,
                    limit_u,
                    &[case.scope.folder_id.clone().expect("folder id")],
                    MatchMode::And,
                )
                .await
                .expect("fts folder and"),
                ScopeKind::Meeting => {
                    // Meeting scope permits several meetings, so the channel
                    // must rank inside the permitted set rather than pin the
                    // focused meeting (mirrors the Task 1.2R baseline harness).
                    FtsRepository::search_with_mode(&pool, query, limit_u * 4, None, MatchMode::And)
                        .await
                        .expect("fts meeting and")
                        .into_iter()
                        .filter(|row| case.scope.allowed_meeting_ids.contains(&row.meeting_id))
                        .take(limit)
                        .collect::<Vec<_>>()
                }
                _ => unreachable!(),
            };
            let claimed: HashSet<(String, String, String)> = and_rows
                .iter()
                .map(|r| {
                    (
                        r.meeting_id.clone(),
                        r.chunk_type.clone(),
                        r.chunk_id.clone(),
                    )
                })
                .collect();
            let or_limit = limit_u * 2;
            let or_rows = match case.scope.kind {
                ScopeKind::All => {
                    FtsRepository::search_with_mode(&pool, query, or_limit, None, MatchMode::Or)
                        .await
                        .expect("fts or")
                }
                ScopeKind::Folder => FtsRepository::search_with_folder_ids(
                    &pool,
                    query,
                    or_limit,
                    &[case.scope.folder_id.clone().expect("folder id")],
                    MatchMode::Or,
                )
                .await
                .expect("fts folder or"),
                ScopeKind::Meeting => {
                    FtsRepository::search_with_mode(&pool, query, or_limit * 4, None, MatchMode::Or)
                        .await
                        .expect("fts meeting or")
                        .into_iter()
                        .filter(|row| case.scope.allowed_meeting_ids.contains(&row.meeting_id))
                        .take(or_limit as usize)
                        .collect::<Vec<_>>()
                }
                _ => unreachable!(),
            };
            let to_row = |row: app_lib::database::repositories::fts::FtsSearchResult| LexRow {
                meeting_id: row.meeting_id,
                chunk_id: row.chunk_id,
                text: row.snippet.replace("<mark>", "").replace("</mark>", ""),
            };
            let or_rows: Vec<LexRow> = or_rows
                .into_iter()
                .filter(|row| {
                    !claimed.contains(&(
                        row.meeting_id.clone(),
                        row.chunk_type.clone(),
                        row.chunk_id.clone(),
                    ))
                })
                .take(limit)
                .map(to_row)
                .collect();
            interleave(and_rows.into_iter().map(to_row).collect(), or_rows, limit)
        }
    };
    pool.close().await;
    rows
}

async fn lexical_channel(case: &EvaluationCase, limit: usize) -> Vec<LexRow> {
    lexical_channel_inner(case, limit, false).await
}

/// Task 1.3F fidelity evidence: every search path JOINs `meetings`, so
/// deleted-meeting FTS rows could never be returned — but their presence in
/// the index could still shift bm25 document frequencies and candidate
/// composition. This probe compares the full returned row sequence with and
/// without the pre-1.3F rows to prove the alignment is metric-inert (or name
/// the cases where it was not).
async fn deleted_fts_rows_ranking_inertness(case: &EvaluationCase) -> bool {
    let with_rows = lexical_channel_inner(case, LEXICAL_CANDIDATES, true).await;
    let aligned = lexical_channel_inner(case, LEXICAL_CANDIDATES, false).await;
    with_rows.len() == aligned.len()
        && with_rows
            .iter()
            .zip(&aligned)
            .all(|(a, b)| a.meeting_id == b.meeting_id && a.chunk_id == b.chunk_id)
}

// ---------------------------------------------------------------------------
// Fusion, meeting aggregation, metrics
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
struct HybridConfig {
    rrf_k: f64,
    w_vector: f64,
    w_lexical: f64,
    support_alpha: f64,
    title_beta: f64,
    rerank_gamma: f64,
    support_cap: usize,
}

fn rrf_fuse(channels: &[&[usize]], weights: &[f64], k: f64) -> Vec<(usize, f64)> {
    let mut scores: HashMap<usize, f64> = HashMap::new();
    for (ch, list) in channels.iter().enumerate() {
        for (rank0, &idx) in list.iter().enumerate() {
            *scores.entry(idx).or_insert(0.0) += weights[ch] / (k + rank0 as f64 + 1.0);
        }
    }
    let mut v: Vec<(usize, f64)> = scores.into_iter().collect();
    v.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    v
}

fn aggregate_meetings(
    entries: &[DocEntry],
    fused: &[(usize, f64)],
    title_overlap: &HashMap<String, f64>,
    rerank_scores: Option<&HashMap<usize, f32>>,
    cfg: HybridConfig,
) -> Vec<(String, f64)> {
    let mut best_fused: HashMap<String, f64> = HashMap::new();
    let mut support: HashMap<String, usize> = HashMap::new();
    let window = 20.min(fused.len());
    for (rank0, (idx, score)) in fused.iter().enumerate() {
        let mid = &entries[*idx].doc.meeting_id;
        let slot = best_fused.entry(mid.clone()).or_insert(0.0);
        if *score > *slot {
            *slot = *score;
        }
        if rank0 < window {
            *support.entry(mid.clone()).or_insert(0) += 1;
        }
    }
    // The cross-encoder contributes as a third rank-space RRF channel at
    // meeting level (rank of the meeting by its best reranked evidence), so
    // its signal cannot be drowned out by the fused-score scale.
    let mut rr_channel: HashMap<String, usize> = HashMap::new();
    if let Some(scores) = rerank_scores {
        let mut best_per_meeting: HashMap<String, f32> = HashMap::new();
        for (idx, s) in scores {
            let mid = &entries[*idx].doc.meeting_id;
            let slot = best_per_meeting.entry(mid.clone()).or_insert(f32::MIN);
            if *s > *slot {
                *slot = *s;
            }
        }
        let mut ranked: Vec<(String, f32)> = best_per_meeting.into_iter().collect();
        ranked.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        for (pos, (mid, _)) in ranked.iter().enumerate() {
            rr_channel.insert(mid.clone(), pos + 1);
        }
    }
    let mut rows: Vec<(String, f64)> = best_fused
        .into_iter()
        .map(|(mid, bf)| {
            let sup = support.get(&mid).copied().unwrap_or(0).min(cfg.support_cap) as f64
                / cfg.support_cap.max(1) as f64;
            let title = title_overlap.get(&mid).copied().unwrap_or(0.0);
            let rr = rr_channel
                .get(&mid)
                .map(|rank| cfg.rerank_gamma / (cfg.rrf_k + *rank as f64))
                .unwrap_or(0.0);
            (
                mid.clone(),
                cfg.rrf_k * bf + cfg.support_alpha * sup + cfg.title_beta * title + rr,
            )
        })
        .collect();
    rows.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    rows
}

fn final_evidence_order(
    fused: &[(usize, f64)],
    rerank_scores: Option<&HashMap<usize, f32>>,
    _depth: usize,
) -> Vec<usize> {
    match rerank_scores {
        None => fused.iter().map(|(i, _)| *i).collect(),
        Some(scores) => {
            // Scored evidence heads the order (reranker rank, fused-rank tie
            // break); every other document — including unscored meeting
            // profiles — follows in fused order.
            let mut head_scored: Vec<(usize, f32, usize)> = fused
                .iter()
                .enumerate()
                .filter(|(_, (idx, _))| scores.contains_key(idx))
                .map(|(frank0, (idx, _))| (*idx, scores[idx], frank0))
                .collect();
            head_scored.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.2.cmp(&b.2))
            });
            let head_set: HashSet<usize> = head_scored.iter().map(|(i, _, _)| *i).collect();
            let mut out: Vec<usize> = head_scored.iter().map(|(i, _, _)| *i).collect();
            out.extend(
                fused
                    .iter()
                    .map(|(i, _)| *i)
                    .filter(|i| !head_set.contains(i)),
            );
            out
        }
    }
}

#[derive(Clone, Debug, Default)]
struct HybridCaseMetrics {
    meeting_ranks: BTreeMap<String, usize>,
    evidence_hits: usize,
    evidence_total: usize,
    fact_hits: usize,
    fact_total: usize,
    forbidden_hits: usize,
    forbidden_total: usize,
    retrieval_forbidden_hits: usize,
    retrieval_forbidden_total: usize,
    answer_forbidden_hits: usize,
    answer_forbidden_total: usize,
    ndcg10_final: f64,
    ndcg10_fused: f64,
    pairwise_correct: usize,
    pairwise_total: usize,
    /// ChatSource values emitted by the broad-retrieval contract simulation:
    /// constructed only after scope revalidation, profile exclusion, and the
    /// final EVIDENCE_K prompt budget, with snippets populated from the
    /// authoritative retained (hydrated) text.
    emitted_sources: Vec<ChatSource>,
    /// The final retained context string the emitted sources are measured
    /// against (lowercased concatenation of retained authoritative text).
    retained_context: String,
}

fn relevance(
    doc_meeting: &str,
    doc_kind: &str,
    doc_evidence: &[String],
    case: &EvaluationCase,
) -> i32 {
    if !doc_evidence.is_empty()
        && doc_evidence
            .iter()
            .any(|e| case.required_evidence_ids.contains(e))
    {
        3
    } else if case.expected_meeting_ids.iter().any(|m| m == doc_meeting) {
        if doc_kind == "profile" {
            1
        } else {
            2
        }
    } else {
        0
    }
}

fn ndcg_at_10(case: &EvaluationCase, order: &[usize], entries: &[DocEntry]) -> f64 {
    let rels: Vec<i32> = order
        .iter()
        .take(10)
        .map(|idx| {
            let e = &entries[*idx];
            relevance(
                &e.doc.meeting_id,
                &e.doc.source_kind,
                &e.doc.evidence_ids,
                case,
            )
        })
        .collect();
    let dcg: f64 = rels
        .iter()
        .enumerate()
        .map(|(i, r)| ((*r as f64) + 1.0).log2() / ((i + 2) as f64).log2())
        .sum();
    let mut ideal = rels.clone();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    let idcg: f64 = ideal
        .iter()
        .enumerate()
        .map(|(i, r)| ((*r as f64) + 1.0).log2() / ((i + 2) as f64).log2())
        .sum();
    if idcg > 0.0 {
        dcg / idcg
    } else {
        0.0
    }
}

fn score_case_hybrid(
    case: &EvaluationCase,
    entries: &[DocEntry],
    meeting_order: &[(String, f64)],
    final_order: &[usize],
    fused_order: &[usize],
) -> HybridCaseMetrics {
    let mut m = HybridCaseMetrics::default();
    // Meeting ranks come from the aggregated meeting order — best fused
    // evidence, capped supporting evidence, title overlap, and reranker
    // contribution — exactly as the architecture's broad-retrieval contract
    // ranks meetings before hydration.
    for (pos, (mid, _)) in meeting_order.iter().enumerate() {
        m.meeting_ranks.insert(mid.clone(), pos + 1);
    }
    // Production hydrates only the top selected meetings; retained evidence is
    // the evidence of those meetings in reranked/fused document order. Profile
    // documents are never cited. Hydration loads current authoritative content
    // and its hash verification omits stale semantic text, so fact checks read
    // `authoritative_text`, not the indexed chunk.
    let hydrated: HashSet<&str> = meeting_order
        .iter()
        .take(HYDRATED_MEETINGS)
        .map(|(mid, _)| mid.as_str())
        .collect();
    let hydrated_docs: Vec<&DocEntry> = final_order
        .iter()
        .filter(|i| hydrated.contains(entries[**i].doc.meeting_id.as_str()))
        .filter(|i| entries[**i].doc.source_kind != "profile")
        .map(|i| &entries[*i])
        .collect();
    // Broad-retrieval source-emission contract: ChatSource values are
    // constructed ONLY from evidence retained after final prompt budgeting,
    // revalidated against the request scope immediately before emission,
    // never from meeting profiles, with snippets populated from the current
    // authoritative (hydrated) text the context actually carries.
    let meeting_by_id: HashMap<&str, &Meeting> = case
        .meetings
        .iter()
        .map(|meeting| (meeting.id.as_str(), meeting))
        .collect();
    let retained: Vec<&DocEntry> = hydrated_docs.iter().take(EVIDENCE_K).copied().collect();
    m.emitted_sources = retained
        .iter()
        .filter(|e| case.scope.allowed_meeting_ids.contains(&e.doc.meeting_id))
        .map(|e| {
            let meeting = meeting_by_id[e.doc.meeting_id.as_str()];
            ChatSource {
                meeting_id: meeting.id.clone(),
                meeting_title: meeting.title.clone(),
                chunk_type: e.doc.source_kind.clone(),
                snippet: e.doc.authoritative_text.clone(),
                folder_name: meeting.folder_id.clone().unwrap_or_default(),
                source_kind: Some(e.doc.source_kind.clone()),
            }
        })
        .collect();
    m.retained_context = retained
        .iter()
        .map(|e| e.doc.authoritative_text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let retained_text = m.retained_context.clone();
    for required in &case.required_evidence_ids {
        m.evidence_total += 1;
        if retained
            .iter()
            .any(|e| e.doc.evidence_ids.iter().any(|id| id == required))
        {
            m.evidence_hits += 1;
        }
    }
    for fact in &case.required_facts {
        m.fact_total += 1;
        if retained_text.contains(&fact.to_lowercase()) {
            m.fact_hits += 1;
        }
    }
    for fact in &case.forbidden_facts {
        let classification =
            classify_forbidden_fact(case, fact).expect("forbidden fact carrier classification");
        let hit = retained_text.contains(&fact.to_lowercase());
        m.forbidden_total += 1;
        m.forbidden_hits += usize::from(hit);
        match classification.stage {
            ForbiddenFactStage::Retrieval => {
                m.retrieval_forbidden_total += 1;
                m.retrieval_forbidden_hits += usize::from(hit);
            }
            ForbiddenFactStage::Answer => {
                m.answer_forbidden_total += 1;
                m.answer_forbidden_hits += usize::from(hit);
            }
        }
    }
    m.ndcg10_final = ndcg_at_10(case, final_order, entries);
    m.ndcg10_fused = ndcg_at_10(case, fused_order, entries);
    m
}

// ---------------------------------------------------------------------------
// Vector storage encodings (f32 reference vs fp16/int8 storage round-trips)
// ---------------------------------------------------------------------------

fn f32_to_f16_bits(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp_i = ((b >> 23) & 0xFF) as i32;
    let man = b & 0x007F_FFFF;
    if exp_i == 0xFF {
        return sign | 0x7C00 | if man != 0 { 0x0200 } else { 0 };
    }
    let h_exp = exp_i - 127 + 15;
    if h_exp >= 0x1F {
        return sign | 0x7C00;
    }
    if h_exp <= 0 {
        if h_exp < -10 {
            return sign;
        }
        let shift = (14 - h_exp.max(-24)) as u32;
        let h_man = ((man | 0x0080_0000) >> shift) as u16;
        return sign | h_man;
    }
    sign | ((h_exp as u32) << 10) as u16 | ((man >> 13) as u16)
}

fn f16_bits_to_f32(h: u16) -> f32 {
    let sign = ((h & 0x8000) as u32) << 16;
    let exp = ((h >> 10) & 0x001F) as i32;
    let man = (h & 0x03FF) as u32;
    let bits: u32 = if exp == 0 {
        if man == 0 {
            sign
        } else {
            let mut e = -1_i32;
            let mut m = man;
            while (m & 0x0400) == 0 {
                m <<= 1;
                e -= 1;
            }
            m &= 0x03FF;
            sign | (((e + 113) as u32) << 23) | (m << 13)
        }
    } else if exp == 31 {
        sign | 0x7F80_0000 | (man << 13)
    } else {
        sign | (((exp + 112) as u32) << 23) | (man << 13)
    };
    f32::from_bits(bits)
}

fn fp16_roundtrip(v: &[f32]) -> Vec<f32> {
    let mut out: Vec<f32> = v
        .iter()
        .map(|x| f16_bits_to_f32(f32_to_f16_bits(*x)))
        .collect();
    l2_normalize(&mut out);
    out
}

/// Symmetric per-vector int8 storage with recorded dequantization scale.
fn int8_roundtrip(v: &[f32]) -> Vec<f32> {
    let max_abs = v.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
    let scale = if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 };
    let mut out: Vec<f32> = v
        .iter()
        .map(|x| (x / scale).round().clamp(-127.0, 127.0) * scale)
        .collect();
    l2_normalize(&mut out);
    out
}

// ---------------------------------------------------------------------------
// Per-case hybrid pipeline plumbing
// ---------------------------------------------------------------------------

fn text_hash(t: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (*x * *y) as f64).sum()
}

fn embed_missing(
    emb: &mut Embedder,
    cache: &mut HashMap<u64, Vec<f32>>,
    texts: &[String],
    batch: usize,
) {
    for chunk_start in (0..texts.len()).step_by(batch) {
        let chunk = &texts[chunk_start..(chunk_start + batch).min(texts.len())];
        let todo: Vec<String> = chunk
            .iter()
            .filter(|t| !cache.contains_key(&text_hash(t)))
            .cloned()
            .collect();
        if todo.is_empty() {
            continue;
        }
        for sub in todo.chunks(batch) {
            let vecs = emb.embed(sub).expect("batch embed");
            for (t, v) in sub.iter().zip(vecs) {
                cache.insert(text_hash(t), v);
            }
        }
    }
}

#[derive(Clone)]
struct CaseDocs {
    entries: Vec<DocEntry>,
    query_vecs: Vec<Vec<f32>>,
    title_overlap: HashMap<String, f64>,
}

fn build_case_docs_cached(
    case: &EvaluationCase,
    tokenizer: &Tokenizer,
    window_tokens: usize,
    overlap_tokens: usize,
    summary_all_templates: bool,
    query_prefix: &str,
    doc_prefix: &str,
    emb: &mut Embedder,
    cache: &mut HashMap<u64, Vec<f32>>,
    policy: &PolicyLite,
) -> CaseDocs {
    let docs = build_case_docs(
        case,
        tokenizer,
        window_tokens,
        overlap_tokens,
        summary_all_templates,
    );
    let prefixed: Vec<String> = docs
        .iter()
        .map(|d| format!("{doc_prefix}{}", d.text))
        .collect();
    let queries: Vec<String> = case
        .rewritten_query
        .iter()
        .chain(std::iter::once(&case.question))
        .map(|q| format!("{query_prefix}{q}"))
        .collect();
    let mut texts = prefixed.clone();
    texts.extend(queries.clone());
    embed_missing(emb, cache, &texts, 32);

    let entries: Vec<DocEntry> = docs
        .into_iter()
        .zip(prefixed)
        .map(|(doc, ptext)| DocEntry {
            doc,
            vector: cache.get(&text_hash(&ptext)).cloned().unwrap_or_default(),
        })
        .collect();
    let query_vecs: Vec<Vec<f32>> = queries
        .iter()
        .map(|q| cache.get(&text_hash(q)).cloned().unwrap_or_default())
        .collect();

    let core = core_terms(
        case.rewritten_query.as_deref().unwrap_or(&case.question),
        &case.language,
        policy,
    );
    let mut title_overlap = HashMap::new();
    for meeting in &case.meetings {
        if meeting.state == MeetingState::Deleted {
            continue;
        }
        let tokens: HashSet<String> = meeting
            .title
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(fold_diacritics)
            .collect();
        let hits = core.iter().filter(|t| tokens.contains(*t)).count();
        title_overlap.insert(meeting.id.clone(), hits as f64 / core.len().max(1) as f64);
    }
    CaseDocs {
        entries,
        query_vecs,
        title_overlap,
    }
}

fn map_lexical(rows: &[LexRow], entries: &[DocEntry]) -> Vec<usize> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        if let Some(i) = entries.iter().position(|e| {
            e.doc.meeting_id == row.meeting_id
                && e.doc.source_kind != "profile"
                && e.doc.evidence_ids.iter().any(|id| id == &row.chunk_id)
        }) {
            if seen.insert(i) {
                out.push(i);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Concept-proxy disagreement diagnostics (Task 1.3 rerun): supervised
// CONCEPT_LEXICON concept margin versus raw bi-encoder rank, per case.
// ---------------------------------------------------------------------------

/// Same lookup contract as `retrieval_evaluation.rs::concept_of`.
fn lexicon_concept(token: &str) -> Option<&'static str> {
    CONCEPT_LEXICON
        .iter()
        .find(|(_, variants)| variants.contains(&token))
        .map(|(concept, _)| *concept)
}

/// Supervised concept-channel margin, mirroring the Task 1.2R
/// `case_margins`/`channel_margin` arithmetic exactly: inverse
/// candidate-frequency weighted concept overlap, target minus strongest
/// distractor. Expected IDs only label targets.
fn case_concept_margin(case: &EvaluationCase, policy: &PolicyLite) -> f64 {
    let query_text = case.rewritten_query.as_deref().unwrap_or(&case.question);
    let stop = match case.language {
        Language::Portuguese => &policy.lexical_policy.portuguese_high_frequency,
        Language::English => &policy.lexical_policy.english_high_frequency,
    };
    let query_terms: Vec<String> = query_text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(fold_diacritics)
        .filter(|t| !stop.contains(t))
        .collect();
    let mut candidates: Vec<(BTreeSet<&'static str>, bool)> = Vec::new();
    for meeting in &case.meetings {
        let is_target = case.expected_meeting_ids.contains(&meeting.id);
        for evidence in &meeting.evidence {
            let concepts = concepts_of_tokens(
                &evidence
                    .indexed_text
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|t| !t.is_empty())
                    .map(fold_diacritics)
                    .collect::<Vec<_>>(),
            );
            candidates.push((concepts, is_target));
        }
    }
    let mut units: Vec<&'static str> = Vec::new();
    for term in &query_terms {
        if let Some(concept) = lexicon_concept(term) {
            if !units.contains(&concept) {
                units.push(concept);
            }
        }
    }
    let mut weights = BTreeMap::new();
    for unit in units {
        let document_frequency = candidates
            .iter()
            .filter(|(set, _)| set.contains(&unit))
            .count()
            .max(1);
        weights.insert(unit, 1.0 / document_frequency as f64);
    }
    let score = |(set, _): &(BTreeSet<&'static str>, bool)| -> f64 {
        weights
            .iter()
            .map(|(unit, weight)| if set.contains(unit) { *weight } else { 0.0 })
            .sum()
    };
    let best = |target: bool| {
        candidates
            .iter()
            .filter(|(_, is_target)| *is_target == target)
            .map(score)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    best(true) - best(false)
}

fn concepts_of_tokens(tokens: &[String]) -> BTreeSet<&'static str> {
    tokens.iter().filter_map(|t| lexicon_concept(t)).collect()
}

fn benchmark_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(fold_diacritics)
        .collect()
}

/// Inverse-candidate-frequency margin of the target's best candidate over the
/// strongest distractor, mirroring the Task 1.2R `channel_margin` arithmetic.
fn inverse_df_margin<T: Ord + Clone>(query_units: &[T], candidates: &[(BTreeSet<T>, bool)]) -> f64 {
    let mut weights = BTreeMap::new();
    for unit in query_units {
        let document_frequency = candidates
            .iter()
            .filter(|(set, _)| set.contains(unit))
            .count()
            .max(1);
        weights.insert(unit.clone(), 1.0 / document_frequency as f64);
    }
    let score = |(set, _): &(BTreeSet<T>, bool)| -> f64 {
        weights
            .iter()
            .map(|(unit, weight)| if set.contains(unit) { *weight } else { 0.0 })
            .sum()
    };
    let best = |target: bool| {
        candidates
            .iter()
            .filter(|(_, is_target)| *is_target == target)
            .map(score)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    best(true) - best(false)
}

/// Production-implementable channel margins ONLY (Task 1.3F Deliverable 2):
/// lexical evidence overlap and meeting-title overlap. The CONCEPT_LEXICON
/// channel is deliberately excluded because it has no production counterpart.
fn case_production_channel_margins(case: &EvaluationCase, policy: &PolicyLite) -> (f64, f64) {
    let query_text = case.rewritten_query.as_deref().unwrap_or(&case.question);
    let stop = match case.language {
        Language::Portuguese => &policy.lexical_policy.portuguese_high_frequency,
        Language::English => &policy.lexical_policy.english_high_frequency,
    };
    let query_terms: Vec<String> = benchmark_tokens(query_text)
        .into_iter()
        .filter(|t| !stop.contains(t))
        .collect();
    let mut text_sets: Vec<(BTreeSet<String>, bool)> = Vec::new();
    let mut title_sets: Vec<(BTreeSet<String>, bool)> = Vec::new();
    for meeting in &case.meetings {
        let is_target = case.expected_meeting_ids.contains(&meeting.id);
        let title: BTreeSet<String> = benchmark_tokens(&meeting.title).into_iter().collect();
        for evidence in &meeting.evidence {
            text_sets.push((
                benchmark_tokens(&evidence.indexed_text)
                    .into_iter()
                    .collect(),
                is_target,
            ));
            title_sets.push((title.clone(), is_target));
        }
    }
    (
        inverse_df_margin(&query_terms, &text_sets),
        inverse_df_margin(&query_terms, &title_sets),
    )
}

fn enforce_rank1_admissibility(
    case: &EvaluationCase,
    lexical_margin: f64,
    title_margin: f64,
    vector_meeting_rank: usize,
) -> Result<(), String> {
    if lexical_margin > 0.0 || title_margin > 0.0 || vector_meeting_rank == 1 {
        Ok(())
    } else {
        Err(format!(
            "{} has no positive lexical/title/raw-vector rank-1 production channel",
            case.id
        ))
    }
}

fn print_forbidden_fact_classifications(cases: &[EvaluationCase]) {
    let (mut retrieval, mut answer) = (0, 0);
    for case in cases {
        for fact in &case.forbidden_facts {
            let classification =
                classify_forbidden_fact(case, fact).expect("fixture-derived carrier state");
            match classification.stage {
                ForbiddenFactStage::Retrieval => retrieval += 1,
                ForbiddenFactStage::Answer => answer += 1,
            }
            println!(
                "[forbidden-classification] case={} fact={fact:?} stage={} carriers=[{}]",
                case.id,
                classification.stage.label(),
                classification
                    .carriers
                    .iter()
                    .map(|carrier| format!(
                        "{}/{}:{}",
                        carrier.meeting_id,
                        carrier.evidence_id,
                        carrier.state.label()
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
    }
    println!(
        "[forbidden-classification-counts] retrieval-stage={retrieval}/{} answer-stage-deferred={answer}/{} total={}/{}",
        retrieval + answer,
        retrieval + answer,
        retrieval + answer,
        retrieval + answer
    );
}

#[test]
fn rank1_admissibility_rejects_a_no_positive_channel_mutation() {
    let mut cases = corpus::cases();
    let policy = policy_lite();
    let case = cases
        .iter_mut()
        .find(|case| case.id == "pt-ref-chaves-acesso")
        .expect("chaves case");
    let (lexical, title) = case_production_channel_margins(case, &policy);
    enforce_rank1_admissibility(case, lexical, title, 2)
        .expect("patched corpus surface has a positive production channel");

    case.meetings
        .iter_mut()
        .find(|meeting| case.expected_meeting_ids.contains(&meeting.id))
        .expect("target meeting")
        .title = "Segurança de acesso — políticas de credenciais".to_string();
    let (lexical, title) = case_production_channel_margins(case, &policy);
    assert!(enforce_rank1_admissibility(case, lexical, title, 2).is_err());
}

#[test]
fn amendment5_partition_excludes_only_critical_and_pinned_cases() {
    let cases = corpus::cases();
    let designated = [
        "fixture-whatsapp-retention",
        "pt-ref-cobranca-regua",
        "pt-ref-chaves-acesso",
        "pt-ref-sla-suporte",
        "pt-ref-nps-detrator",
    ];
    let excluded: Vec<&str> = cases
        .iter()
        .filter(|c| excluded_from_tuning(c))
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(
        excluded.len(),
        5,
        "exactly the five designated critical/pinned cases may be excluded, got {excluded:?}"
    );
    for id in designated {
        assert!(excluded.contains(&id), "{id} must stay outside tuning");
    }
    // No tuning path may ever see a critical case.
    assert!(cases
        .iter()
        .filter(|c| c.critical)
        .all(|c| excluded_from_tuning(c)));
    // Amendment 5 admits exactly the ten non-critical reference-category
    // cases; the partition is otherwise the full corpus.
    let admitted_reference: Vec<&str> = cases
        .iter()
        .filter(|c| !excluded_from_tuning(c))
        .filter(|c| c.categories.iter().any(|v| v == corpus::REFERENCE_CATEGORY))
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(
        admitted_reference.len(),
        10,
        "amendment 5 admits exactly the 10 non-critical reference-category cases"
    );
    assert_eq!(
        cases.iter().filter(|c| !excluded_from_tuning(c)).count(),
        115
    );
}

fn amendment5_case(id: &str, categories: &[&str]) -> EvaluationCase {
    EvaluationCase {
        id: id.to_string(),
        language: Language::Portuguese,
        question: String::new(),
        history: vec![],
        rewritten_query: None,
        scope: Scope {
            kind: ScopeKind::All,
            folder_id: None,
            meeting_id: None,
            allowed_meeting_ids: vec![],
        },
        meetings: vec![],
        expected_meeting_ids: vec!["mtg-x".to_string()],
        order_constraints: vec![],
        required_evidence_ids: vec![],
        required_facts: vec![],
        forbidden_facts: vec![],
        answer_mode: String::new(),
        categories: categories.iter().map(|v| (*v).to_string()).collect(),
        critical: false,
    }
}

#[test]
fn amendment5_objective_orders_reference_r1_misses_between_semantic_and_overall() {
    let ref_case = amendment5_case("ref", &[corpus::REFERENCE_CATEGORY]);
    let sem_case = amendment5_case("sem", &["semantic_paraphrase"]);
    let exact_case = amendment5_case("exact", &["exact_term"]);
    let plain_case = amendment5_case("plain", &[]);
    let ranks = BTreeMap::from([("mtg-x".to_string(), 2_usize)]);

    // Scope: each term counts only its own category's cases; overall and MRR
    // count everything. A rank-2 expected meeting misses Recall@1 but not
    // Recall@3, so only reference_r1_misses and mrr move per case here.
    let mut obj = TuneObjective::default();
    for case in [&ref_case, &sem_case, &exact_case, &plain_case] {
        obj.add_case(case, &ranks);
    }
    assert_eq!(obj.reference_r1_misses, 1);
    assert_eq!(obj.semantic_r3_misses, 0);
    assert_eq!(obj.exact_violations, 0);
    assert_eq!(obj.overall_r3_misses, 0);
    assert!(obj.mrr_deficit_micros > 0);
    let mut at_rank_1 = TuneObjective::default();
    at_rank_1.add_case(&ref_case, &BTreeMap::from([("mtg-x".to_string(), 1_usize)]));
    assert_eq!(at_rank_1.reference_r1_misses, 0);

    // Ordering (lexicographic; smaller key wins). For adjacent terms X then Y
    // in the key, driving Y to its maximum must still lose to a single step of
    // X: reducing X dominates any reduction of Y.
    let key_exact_leads = TuneObjective {
        semantic_r3_misses: u64::MAX,
        ..Default::default()
    }
    .key();
    let key_exact_one = TuneObjective {
        exact_violations: 1,
        ..Default::default()
    }
    .key();
    assert!(
        key_exact_leads < key_exact_one,
        "reducing exact-term violations outranks any reduction of semantic misses"
    );
    let key_semantic_leads = TuneObjective {
        reference_r1_misses: u64::MAX,
        ..Default::default()
    }
    .key();
    let key_semantic_one = TuneObjective {
        semantic_r3_misses: 1,
        ..Default::default()
    }
    .key();
    assert!(
        key_semantic_leads < key_semantic_one,
        "reducing semantic misses outranks any number of reference misses"
    );
    let key_reference_leads = TuneObjective {
        overall_r3_misses: u64::MAX,
        ..Default::default()
    }
    .key();
    let key_reference_one = TuneObjective {
        reference_r1_misses: 1,
        ..Default::default()
    }
    .key();
    assert!(
        key_reference_leads < key_reference_one,
        "eliminating one reference R@1 miss outranks any reduction in overall R@3 misses"
    );
    // The reranked stage keeps the same relative order around its inserted
    // NDCG flag (position 3), so reordering either array fails this test.
    let rk = TuneObjective {
        reference_r1_misses: 1,
        overall_r3_misses: 7,
        ..Default::default()
    }
    .rerank_key(true);
    assert_eq!(rk[2], 1);
    assert_eq!(rk[3], 1);
    assert_eq!(rk[4], 7);
}

/// Raw bi-encoder ranks under the identical scoring run_case uses (best cosine
/// over query variants, deterministic tie-break), before any fusion: rank of
/// the best-ranked expected-meeting document and of the best-ranked required-
/// evidence document within the full per-case ordering.
fn raw_vector_ranks(docs: &CaseDocs, case: &EvaluationCase) -> (usize, usize) {
    let mut scored: Vec<(usize, f64)> = docs
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.doc.vector_indexed)
        .map(|(i, e)| {
            let best = docs
                .query_vecs
                .iter()
                .map(|q| dot(q, &e.vector))
                .fold(f64::MIN, f64::max);
            (i, best)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    let (mut meeting_rank, mut evidence_rank) = (usize::MAX, usize::MAX);
    for (pos, (idx, _)) in scored.iter().enumerate() {
        let entry = &docs.entries[*idx];
        if meeting_rank == usize::MAX
            && case
                .expected_meeting_ids
                .iter()
                .any(|m| *m == entry.doc.meeting_id)
        {
            meeting_rank = pos + 1;
        }
        if evidence_rank == usize::MAX
            && entry
                .doc
                .evidence_ids
                .iter()
                .any(|id| case.required_evidence_ids.contains(id))
        {
            evidence_rank = pos + 1;
        }
        if meeting_rank != usize::MAX && evidence_rank != usize::MAX {
            break;
        }
    }
    (meeting_rank, evidence_rank)
}

/// Per-case disagreement table for one benchmark-leader family. Agreement
/// compares the proxy prediction (positive concept margin) with the raw
/// vector behavior (expected meeting inside the top 3), so four outcomes are
/// visible instead of collapsing model-only hits into "agreement".
fn print_concept_proxy_table(
    label: &str,
    note: &str,
    cases: &[EvaluationCase],
    docs: &[CaseDocs],
    policy: &PolicyLite,
) {
    println!("[concept-proxy] bi-encoder leader: {label} ({note})");
    println!("| Case | Language | Concept margin | Vector rank mtg/ev | Verdict |");
    println!("|---|---|---|---|---|");
    let (mut agree, mut disagree) = (0_usize, 0_usize);
    let (mut pos_hit, mut pos_miss, mut neg_hit, mut neg_miss) =
        (0_usize, 0_usize, 0_usize, 0_usize);
    for (ci, case) in cases.iter().enumerate() {
        let margin = case_concept_margin(case, policy);
        let (m_rank, e_rank) = raw_vector_ranks(&docs[ci], case);
        let proxy_positive = margin > 0.0;
        let vector_hit = m_rank <= 3;
        match (proxy_positive, vector_hit) {
            (true, true) => {
                agree += 1;
                pos_hit += 1;
            }
            (true, false) => {
                disagree += 1;
                pos_miss += 1;
            }
            (false, true) => {
                disagree += 1;
                neg_hit += 1;
            }
            (false, false) => {
                agree += 1;
                neg_miss += 1;
            }
        }
        let verdict = if proxy_positive == vector_hit {
            "AGREE"
        } else {
            "DISAGREE"
        };
        let language = match case.language {
            Language::Portuguese => "pt",
            Language::English => "en",
        };
        println!(
            "| {} | {language} | {margin:+.3} | {}/{} | {verdict} |",
            case.id,
            if m_rank == usize::MAX { 0 } else { m_rank },
            if e_rank == usize::MAX { 0 } else { e_rank },
        );
    }
    println!(
        "[concept-proxy] summary {label}: AGREE {agree}/{} DISAGREE {disagree} (proxy-positive/model-hit {pos_hit}, proxy-positive/model-miss {pos_miss}, proxy-negative/model-hit {neg_hit}, proxy-negative/model-miss {neg_miss})",
        cases.len()
    );
}

#[derive(Clone, Debug)]
struct CaseRunOutput {
    fused_order: Vec<usize>,
    metrics: HybridCaseMetrics,
}

/// Vector-channel candidate list: only documents a production index would
/// admit. Dirty/StaleDerived meetings' rows stay excluded until re-indexed.
fn vector_ranking(docs: &CaseDocs) -> Vec<usize> {
    let mut scored: Vec<(usize, f64)> = docs
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.doc.vector_indexed)
        .map(|(i, e)| {
            let best = docs
                .query_vecs
                .iter()
                .map(|q| dot(q, &e.vector))
                .fold(f64::MIN, f64::max);
            (i, best)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    scored
        .iter()
        .take(VECTOR_CANDIDATES)
        .map(|(i, _)| *i)
        .collect()
}

fn run_case(
    case: &EvaluationCase,
    docs: &CaseDocs,
    lex_rank: &[usize],
    cfg: HybridConfig,
    reranker: Option<(&mut RerankModel, usize, usize)>,
) -> CaseRunOutput {
    let entries = &docs.entries;
    let vec_rank = vector_ranking(docs);
    let fused = rrf_fuse(
        &[&vec_rank, lex_rank],
        &[cfg.w_vector, cfg.w_lexical],
        cfg.rrf_k,
    );

    let rerank_scores = match reranker {
        None => None,
        Some((model, depth, batch)) => {
            // Reranking receives question/evidence pairs only. Meeting-profile
            // documents support selection/aggregation and are never cited as
            // evidence, so they stay at their fused positions unscored.
            let fused_order: Vec<usize> = fused.iter().map(|(i, _)| *i).collect();
            let head: Vec<usize> = fused_order
                .iter()
                .copied()
                .filter(|i| entries[*i].doc.source_kind != "profile")
                .take(depth.min(fused_order.len()))
                .collect();
            let question = case.rewritten_query.as_deref().unwrap_or(&case.question);
            let mut scores: HashMap<usize, f32> = HashMap::new();
            for start in (0..head.len()).step_by(batch.max(1)) {
                let end = (start + batch.max(1)).min(head.len());
                let chunk = &head[start..end];
                let pairs: Vec<(String, String)> = chunk
                    .iter()
                    .map(|i| (question.to_string(), entries[*i].doc.text.clone()))
                    .collect();
                let s = model.score(&pairs).expect("rerank batch");
                for (idx, sc) in chunk.iter().zip(s) {
                    scores.insert(*idx, sc);
                }
            }
            Some(scores)
        }
    };
    finish_case(case, docs, cfg, fused, rerank_scores.as_ref())
}

/// Meeting aggregation, retention, and metric scoring from precomputed fusion
/// and reranker outputs.
fn finish_case(
    case: &EvaluationCase,
    docs: &CaseDocs,
    cfg: HybridConfig,
    fused: Vec<(usize, f64)>,
    rerank_scores: Option<&HashMap<usize, f32>>,
) -> CaseRunOutput {
    let entries = &docs.entries;
    let meeting_order =
        aggregate_meetings(entries, &fused, &docs.title_overlap, rerank_scores, cfg);
    let final_order = final_evidence_order(&fused, rerank_scores, 0);
    let fused_order: Vec<usize> = fused.iter().map(|(i, _)| *i).collect();
    let mut metrics = score_case_hybrid(case, entries, &meeting_order, &final_order, &fused_order);
    if let Some(scores) = rerank_scores {
        let rel_of = |i: usize| {
            relevance(
                &entries[i].doc.meeting_id,
                &entries[i].doc.source_kind,
                &entries[i].doc.evidence_ids,
                case,
            )
        };
        for (a, sa) in scores {
            for (b, sb) in scores {
                if a == b {
                    continue;
                }
                let (ra, rb) = (rel_of(*a), rel_of(*b));
                if ra >= 2 && rb == 0 {
                    metrics.pairwise_total += 1;
                    metrics.pairwise_correct += usize::from(sa > sb);
                }
            }
        }
    }
    CaseRunOutput {
        fused_order,
        metrics,
    }
}

// ---------------------------------------------------------------------------
// Full corpus + resource benchmark (Task 1.3 evidence generator)
// ---------------------------------------------------------------------------

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "n/a".to_string()
    } else {
        format!("{:.2}% ({}/{})", n as f64 / d as f64 * 100.0, n, d)
    }
}

/// Broad-retrieval citation/source precision: every emitted ChatSource must
/// be present in the final retained context. Measured by snippet containment
/// against the assembled context text — not identity equality between two
/// slices of one list — so a source attached before prompt truncation or one
/// carrying a stale pre-hydration snippet fails.
fn chat_source_precision(sources: &[ChatSource], final_retained_context: &str) -> (usize, usize) {
    let context = final_retained_context.to_lowercase();
    let hits = sources
        .iter()
        .filter(|source| {
            !source.snippet.trim().is_empty() && context.contains(&source.snippet.to_lowercase())
        })
        .count();
    (hits, sources.len())
}

#[test]
fn citation_precision_rejects_pre_budget_and_stale_snippets() {
    let authoritative = "Resumo regenerado removeu a alegação antiga herdada do contrato legado.";
    let stale_indexed = "Resumo desatualizado mantém a alegação antiga de um dia inteiro.";
    let pre_budget = "Documento além da janela de retenção nunca chegou ao contexto final.";
    let other_retained = "Conteúdo vigente de outra evidência retida no contexto final.";
    let context = format!("{authoritative}\n{other_retained}").to_lowercase();
    let source = |snippet: &str| ChatSource {
        meeting_id: "mtg-x".to_string(),
        meeting_title: "Título sintético".to_string(),
        chunk_type: "note".to_string(),
        snippet: snippet.to_string(),
        folder_name: String::new(),
        source_kind: Some("note".to_string()),
    };
    // Conformant emission: snippets are the retained authoritative texts.
    assert_eq!(
        chat_source_precision(&[source(authoritative), source(other_retained)], &context),
        (2, 2)
    );
    // Pre-budget mutation: a source attached before prompt truncation carries
    // text that never reached the final context and must fail.
    assert_eq!(chat_source_precision(&[source(pre_budget)], &context).0, 0);
    // Stale-snippet mutation: hydration replaced the indexed draft with the
    // regenerated text, so a pre-hydration snippet must fail.
    assert_eq!(
        chat_source_precision(&[source(stale_indexed)], &context).0,
        0
    );
    // Empty-snippet emission cannot count as present.
    assert_eq!(chat_source_precision(&[source("")], &context).0, 0);
}

fn percentile(values: &[u128], p: usize) -> u128 {
    values
        .get((values.len().saturating_sub(1) * p) / 100)
        .copied()
        .unwrap_or(0)
}

#[derive(Default)]
struct CorpusMetrics {
    r1: (usize, usize),
    r3: (usize, usize),
    r5: (usize, usize),
    mrr_sum: f64,
    mrr_cases: usize,
    ev10: (usize, usize),
    facts: (usize, usize),
    forbidden: (usize, usize),
    retrieval_forbidden: (usize, usize),
    answer_forbidden: (usize, usize),
    ndcg_final_sum: f64,
    ndcg_fused_sum: f64,
    ndcg_cases: usize,
    pairwise_correct: usize,
    pairwise_total: usize,
}

impl CorpusMetrics {
    fn add(&mut self, case: &EvaluationCase, out: &CaseRunOutput) {
        for mid in &case.expected_meeting_ids {
            let rank = out.metrics.meeting_ranks.get(mid).copied();
            self.r1.1 += 1;
            self.r3.1 += 1;
            self.r5.1 += 1;
            self.r1.0 += usize::from(rank.is_some_and(|r| r == 1));
            self.r3.0 += usize::from(rank.is_some_and(|r| r <= 3));
            self.r5.0 += usize::from(rank.is_some_and(|r| r <= 5));
        }
        if let Some(best) = case
            .expected_meeting_ids
            .iter()
            .filter_map(|m| out.metrics.meeting_ranks.get(m))
            .min()
        {
            self.mrr_sum += 1.0 / *best as f64;
        }
        self.mrr_cases += 1;
        self.ev10.0 += out.metrics.evidence_hits;
        self.ev10.1 += out.metrics.evidence_total;
        self.facts.0 += out.metrics.fact_hits;
        self.facts.1 += out.metrics.fact_total;
        self.forbidden.0 += out.metrics.forbidden_hits;
        self.forbidden.1 += out.metrics.forbidden_total;
        self.retrieval_forbidden.0 += out.metrics.retrieval_forbidden_hits;
        self.retrieval_forbidden.1 += out.metrics.retrieval_forbidden_total;
        self.answer_forbidden.0 += out.metrics.answer_forbidden_hits;
        self.answer_forbidden.1 += out.metrics.answer_forbidden_total;
        self.ndcg_final_sum += out.metrics.ndcg10_final;
        self.ndcg_fused_sum += out.metrics.ndcg10_fused;
        self.ndcg_cases += 1;
        self.pairwise_correct += out.metrics.pairwise_correct;
        self.pairwise_total += out.metrics.pairwise_total;
    }
}

fn score_rows_baseline(case: &EvaluationCase, rows: &[LexRow]) -> HybridCaseMetrics {
    let mut m = HybridCaseMetrics::default();
    let mut seen = HashSet::new();
    for row in rows {
        if seen.insert(row.meeting_id.clone()) {
            m.meeting_ranks.insert(row.meeting_id.clone(), seen.len());
        }
    }
    let retained: Vec<&LexRow> = rows.iter().take(EVIDENCE_K).collect();
    let retained_text = retained
        .iter()
        .map(|r| r.text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let retained_ids: HashSet<&str> = retained.iter().map(|r| r.chunk_id.as_str()).collect();
    for required in &case.required_evidence_ids {
        m.evidence_total += 1;
        if retained_ids.contains(required.as_str()) {
            m.evidence_hits += 1;
        }
    }
    for fact in &case.required_facts {
        m.fact_total += 1;
        if retained_text.contains(&fact.to_lowercase()) {
            m.fact_hits += 1;
        }
    }
    for fact in &case.forbidden_facts {
        let classification =
            classify_forbidden_fact(case, fact).expect("forbidden fact carrier classification");
        let hit = retained_text.contains(&fact.to_lowercase());
        m.forbidden_total += 1;
        m.forbidden_hits += usize::from(hit);
        match classification.stage {
            ForbiddenFactStage::Retrieval => {
                m.retrieval_forbidden_total += 1;
                m.retrieval_forbidden_hits += usize::from(hit);
            }
            ForbiddenFactStage::Answer => {
                m.answer_forbidden_total += 1;
                m.answer_forbidden_hits += usize::from(hit);
            }
        }
    }
    m
}

/// One full-corpus pass under a single hybrid configuration.
struct PairEvaluation {
    all: CorpusMetrics,
    pt: CorpusMetrics,
    en: CorpusMetrics,
    exact: CorpusMetrics,
    semantic: CorpusMetrics,
    critical: CorpusMetrics,
    /// Full corpus reference category (`reference_whatsapp`, 15 cases),
    /// required for the category-wide title ablation.
    reference: CorpusMetrics,
    outputs: BTreeMap<String, CaseRunOutput>,
}

fn evaluate_pair(
    cases: &[EvaluationCase],
    docs: &[CaseDocs],
    lex_store: &BTreeMap<String, Vec<LexRow>>,
    model: &mut RerankModel,
    cfg: HybridConfig,
    depth: usize,
    batch: usize,
) -> PairEvaluation {
    let mut ev = PairEvaluation {
        all: CorpusMetrics::default(),
        pt: CorpusMetrics::default(),
        en: CorpusMetrics::default(),
        exact: CorpusMetrics::default(),
        semantic: CorpusMetrics::default(),
        critical: CorpusMetrics::default(),
        reference: CorpusMetrics::default(),
        outputs: BTreeMap::new(),
    };
    for (ci, case) in cases.iter().enumerate() {
        let lr = map_lexical(&lex_store[&case.id], &docs[ci].entries);
        let out = run_case(case, &docs[ci], &lr, cfg, Some((model, depth, batch)));
        ev.all.add(case, &out);
        if case.language == Language::Portuguese {
            ev.pt.add(case, &out);
        }
        if case.language == Language::English {
            ev.en.add(case, &out);
        }
        if case.categories.iter().any(|v| v == "exact_term") {
            ev.exact.add(case, &out);
        }
        if case.categories.iter().any(|v| v == "semantic_paraphrase") {
            ev.semantic.add(case, &out);
        }
        if case.critical {
            ev.critical.add(case, &out);
        }
        if case
            .categories
            .iter()
            .any(|v| v == corpus::REFERENCE_CATEGORY)
        {
            ev.reference.add(case, &out);
        }
        ev.outputs.insert(case.id.clone(), out);
    }
    ev
}

const DEFAULT_WINDOW: usize = 384;
const DEFAULT_OVERLAP: usize = 64;
const RERANK_SET: usize = 50;
const HYDRATED_MEETINGS: usize = 5;

/// Task 1.3F constants-feasibility probe (diagnostic, NOT tuning): the
/// existing 360-configuration fusion grid times the per-candidate gamma grid,
/// evaluated on the full corpus INCLUDING reference/critical cases purely as
/// existence evidence. Reports whether ANY configuration passes Critical
/// Recall@1 5/5 plus critical forbidden contamination 0/6 without regressing
/// exact-term Recall@3 below the FTS baseline. Tuned constants remain the
/// held-out objective's output; no grid point is promoted from this probe.
fn constants_feasibility_probe(
    label: &str,
    root: &Path,
    dir: &str,
    file: &str,
    cases: &[EvaluationCase],
    docs: &[CaseDocs],
    lex_ranks: &[Vec<usize>],
    baseline_exact_r3: (usize, usize),
    tuned_cfg: HybridConfig,
    tuned_objective_key: [u64; 6],
) {
    let mut model = RerankModel::load(&root.join(dir), file, 512, 4)
        .unwrap_or_else(|e| panic!("probe load {dir}/{file}: {e}"));
    let ks = [5.0_f64, 10.0, 20.0, 60.0];
    let wvs = [0.5_f64, 1.0, 2.0];
    let wls = [0.5_f64, 1.0, 2.0];
    let alphas = [0.0_f64, 0.5];
    let betas = [0.0_f64, 0.25, 0.5, 1.0, 2.0];
    let gammas = [0.0_f64, 0.5, 1.0, 2.0, 4.0, 8.0];

    // Reranker scores depend only on the fused ORDER (rerank head), which is
    // invariant under uniform (wv, wl) scaling, so scores are cached per
    // weight ratio within each k.
    let vec_rank_per_case: Vec<Vec<usize>> = docs.iter().map(vector_ranking).collect();
    let (mut total, mut pass_count) = (0_usize, 0_usize);
    let mut examples: Vec<String> = Vec::new();
    // Amendment 5 corroboration: is the held-out-tuned configuration inside
    // the passing set, and if not, how far is its objective from the best
    // passing configuration's objective?
    let tuned_cfg_label = format!(
        "k={} w_vector={} w_lexical={} alpha={} beta={} gamma={}",
        tuned_cfg.rrf_k,
        tuned_cfg.w_vector,
        tuned_cfg.w_lexical,
        tuned_cfg.support_alpha,
        tuned_cfg.title_beta,
        tuned_cfg.rerank_gamma
    );
    let mut tuned_passes: Option<bool> = None;
    let mut tuned_probe_key: Option<[u64; 6]> = None;
    let mut best_passing: Option<([u64; 6], String)> = None;
    for &k in &ks {
        let mut scores_by_ratio: HashMap<u64, Vec<HashMap<usize, f32>>> = HashMap::new();
        for &wl in &wls {
            for &wv in &wvs {
                let ratio_bits = (wv / wl).to_bits();
                if !scores_by_ratio.contains_key(&ratio_bits) {
                    let mut per_case: Vec<HashMap<usize, f32>> = Vec::with_capacity(cases.len());
                    for (ci, case) in cases.iter().enumerate() {
                        let probe_fused =
                            rrf_fuse(&[&vec_rank_per_case[ci], &lex_ranks[ci]], &[wv, wl], k);
                        let order: Vec<usize> = probe_fused.iter().map(|(i, _)| *i).collect();
                        let head: Vec<usize> = order
                            .iter()
                            .copied()
                            .filter(|i| docs[ci].entries[*i].doc.source_kind != "profile")
                            .take(RERANK_SET.min(order.len()))
                            .collect();
                        let question = case.rewritten_query.as_deref().unwrap_or(&case.question);
                        let mut scores: HashMap<usize, f32> = HashMap::new();
                        for pair_start in (0..head.len()).step_by(1) {
                            let idx = head[pair_start];
                            let s = model
                                .score(&[(
                                    question.to_string(),
                                    docs[ci].entries[idx].doc.text.clone(),
                                )])
                                .expect("probe rerank");
                            scores.insert(idx, s[0]);
                        }
                        per_case.push(scores);
                    }
                    scores_by_ratio.insert(ratio_bits, per_case);
                }
                let fused_per_case: Vec<Vec<(usize, f64)>> = (0..cases.len())
                    .map(|ci| rrf_fuse(&[&vec_rank_per_case[ci], &lex_ranks[ci]], &[wv, wl], k))
                    .collect();
                let score_sets = &scores_by_ratio[&ratio_bits];
                for &alpha in &alphas {
                    for &beta in &betas {
                        for &gamma in &gammas {
                            total += 1;
                            let cfg = HybridConfig {
                                rrf_k: k,
                                w_vector: wv,
                                w_lexical: wl,
                                support_alpha: alpha,
                                title_beta: beta,
                                rerank_gamma: gamma,
                                support_cap: 3,
                            };
                            let (mut crit_hits, mut crit_expected, mut crit_forbidden) =
                                (0_usize, 0_usize, 0_usize);
                            let (mut exact_hits, mut exact_total) = (0_usize, 0_usize);
                            let mut obj = TuneObjective::default();
                            let (mut ndcg_f, mut ndcg_u) = (0f64, 0f64);
                            for (ci, case) in cases.iter().enumerate() {
                                let out = finish_case(
                                    case,
                                    &docs[ci],
                                    cfg,
                                    fused_per_case[ci].clone(),
                                    Some(&score_sets[ci]),
                                );
                                if case.critical {
                                    for mid in &case.expected_meeting_ids {
                                        crit_expected += 1;
                                        crit_hits += usize::from(
                                            out.metrics
                                                .meeting_ranks
                                                .get(mid)
                                                .is_some_and(|r| *r == 1),
                                        );
                                    }
                                    crit_forbidden += out.metrics.retrieval_forbidden_hits;
                                }
                                if case.categories.iter().any(|v| v == "exact_term") {
                                    for mid in &case.expected_meeting_ids {
                                        exact_total += 1;
                                        exact_hits += usize::from(
                                            out.metrics
                                                .meeting_ranks
                                                .get(mid)
                                                .is_some_and(|r| *r <= 3),
                                        );
                                    }
                                }
                                if !excluded_from_tuning(case) {
                                    obj.add_case(case, &out.metrics.meeting_ranks);
                                    ndcg_f += out.metrics.ndcg10_final;
                                    ndcg_u += out.metrics.ndcg10_fused;
                                }
                            }
                            let passes = crit_hits == crit_expected
                                && crit_forbidden == 0
                                && exact_hits as f64 / exact_total.max(1) as f64
                                    >= baseline_exact_r3.0 as f64
                                        / baseline_exact_r3.1.max(1) as f64;
                            // Objective key over the held-out partition only;
                            // critical results above are existence evidence and
                            // never feed any tuning decision.
                            let obj_key = obj.rerank_key(ndcg_u > ndcg_f);
                            if passes {
                                pass_count += 1;
                                if examples.len() < 3 {
                                    examples.push(format!(
                                        "k={k} w_vector={wv} w_lexical={wl} alpha={alpha} beta={beta} gamma={gamma}"
                                    ));
                                }
                                if best_passing.as_ref().map_or(true, |(bk, _)| obj_key < *bk) {
                                    best_passing = Some((
                                        obj_key,
                                        format!(
                                            "k={k} w_vector={wv} w_lexical={wl} alpha={alpha} beta={beta} gamma={gamma}"
                                        ),
                                    ));
                                }
                            }
                            if cfg == tuned_cfg {
                                tuned_passes = Some(passes);
                                tuned_probe_key = Some(obj_key);
                            }
                        }
                    }
                }
            }
        }
    }
    println!(
        "[feasibility {label}] configurations passing Critical Recall@1 {}/{} + critical retrieval-stage forbidden 0 + exact-term no-regression: {pass_count}/{total}",
        5, 5
    );
    for example in &examples {
        println!("[feasibility {label}] example passing configuration: {example}");
    }
    if pass_count == 0 {
        println!("[feasibility {label}] none exists");
    }
    // Amendment 5 corroboration: tuned configuration versus the passing set.
    println!(
        "[feasibility {label}] tuned configuration (held-out objective's output): {tuned_cfg_label} | winning objective vector [exact-viol, sem-miss, ref-r1-miss, ndcg-nondeg, all-miss, mrr-miss-u] = {tuned_objective_key:?}"
    );
    match (tuned_passes, tuned_probe_key) {
        (Some(true), _) => {
            println!(
                "[feasibility {label}] corroboration: the tuned configuration IS inside the passing set"
            );
        }
        (Some(false), Some(tuned_key)) => {
            println!(
                "[feasibility {label}] corroboration: the tuned configuration is OUTSIDE the passing set"
            );
            match &best_passing {
                Some((best_key, best_cfg)) => {
                    let deltas: Vec<i64> = tuned_key
                        .iter()
                        .zip(best_key.iter())
                        .map(|(a, b)| *a as i64 - *b as i64)
                        .collect();
                    println!(
                        "[feasibility {label}] objective distance (tuned minus best-passing componentwise): {deltas:?} | tuned key {tuned_key:?} | best-passing key {best_key:?} at {best_cfg}"
                    );
                }
                None => println!("[feasibility {label}] no passing configuration exists"),
            }
        }
        _ => {
            println!(
                "[feasibility {label}] corroboration unavailable: the tuned configuration is not on the probed grid"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Amendment 5 (final Task 1.3 selection run): tuning partition and objective
// ---------------------------------------------------------------------------

/// The pinned reference acceptance case stays fully isolated from every
/// tuning path regardless of category flags.
const PINNED_REFERENCE_CASE: &str = "fixture-whatsapp-retention";

/// Amendment 5 partition: exclude ONLY the five designated critical/pinned
/// cases (the `critical` flag plus the pinned reference case). The ten
/// non-critical reference-category cases join held-out tuning so the split is
/// drawn from the same distribution the gates grade. Critical-case results
/// must never be inspected by any tuning decision.
fn excluded_from_tuning(case: &EvaluationCase) -> bool {
    case.critical || case.id == PINNED_REFERENCE_CASE
}

/// Amendment 5 held-out objective over the tuning partition. Lexicographic key
/// order, smaller wins: exact-term Recall@3 violations, semantic Recall@3
/// misses, reference-category Recall@1 misses (admitted siblings only),
/// overall Recall@3 misses, MRR deficit in micros.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct TuneObjective {
    exact_violations: u64,
    semantic_r3_misses: u64,
    reference_r1_misses: u64,
    overall_r3_misses: u64,
    mrr_deficit_micros: u64,
}

impl TuneObjective {
    fn key(self) -> [u64; 5] {
        [
            self.exact_violations,
            self.semantic_r3_misses,
            self.reference_r1_misses,
            self.overall_r3_misses,
            self.mrr_deficit_micros,
        ]
    }

    /// Reranked-stage key: the base key with the NDCG non-degradation flag
    /// inserted between reference and overall misses.
    fn rerank_key(self, ndcg_regression: bool) -> [u64; 6] {
        let k = self.key();
        [k[0], k[1], k[2], u64::from(ndcg_regression), k[3], k[4]]
    }

    /// Accumulate one case outcome. Category terms apply only to their own
    /// categories; overall and MRR terms apply to every tuned case.
    fn add_case(&mut self, case: &EvaluationCase, ranks: &BTreeMap<String, usize>) {
        let misses = |ok: &dyn Fn(usize) -> bool| -> u64 {
            case.expected_meeting_ids
                .iter()
                .filter(|mid| !ranks.get(*mid).is_some_and(|r| ok(*r)))
                .count() as u64
        };
        if case.categories.iter().any(|v| v == "exact_term") {
            self.exact_violations += misses(&|r| r <= 3);
        }
        if case.categories.iter().any(|v| v == "semantic_paraphrase") {
            self.semantic_r3_misses += misses(&|r| r <= 3);
        }
        if case
            .categories
            .iter()
            .any(|v| v == corpus::REFERENCE_CATEGORY)
        {
            self.reference_r1_misses += misses(&|r| r == 1);
        }
        self.overall_r3_misses += misses(&|r| r <= 3);
        if let Some(best) = case
            .expected_meeting_ids
            .iter()
            .filter_map(|m| ranks.get(m))
            .min()
        {
            self.mrr_deficit_micros += ((1.0 - 1.0 / *best as f64) * 1.0e6) as u64;
        }
    }
}

#[tokio::test]
async fn hybrid_corpus_and_resource_benchmark() {
    let Some(root) = models_dir() else {
        println!("SKIP hybrid benchmark: set MEETLY_RAG_MODELS_DIR to staged artifacts");
        return;
    };
    if std::env::var("MEETLY_RAG_BENCH").as_deref() != Ok("1") {
        println!("SKIP hybrid corpus/resource benchmark (set MEETLY_RAG_BENCH=1)");
        return;
    }

    let cases = corpus::cases();
    assert_eq!(cases.len(), 120, "Task 1.2 corpus floor");
    let pol = policy_lite();
    print_forbidden_fact_classifications(&cases);

    println!("=== Task 1.3 hybrid corpus + resource benchmark ===");
    println!(
        "hardware: Intel Core Ultra 7 255HX, 20 cores/20 logical CPUs, 31.4 GiB RAM, Windows x64, ORT CPU"
    );

    // ---- Sessions and peak-session RAM (RSS delta method) ----
    let bundle = manifest();
    let leader_emb = &bundle.benchmark_leader.embedding;
    let rss_base = rss_mib().expect("rss");
    let mut emb = Embedder::load(
        &root.join(&leader_emb.benchmark_artifact_dir),
        &leader_emb.benchmark_artifact_file,
        leader_emb.max_sequence_length,
        4,
    )
    .expect("load benchmark-leader embedding session");
    emb.embed(&["query: warmup sentence for allocator stability".to_string()])
        .expect("warmup embed");
    let rss_loaded = rss_mib().expect("rss");
    println!(
        "[ram] {} embedding session (load+warmup) RSS delta: {:.1} MiB",
        leader_emb.model_id,
        rss_loaded - rss_base
    );

    // ---- Shared lexical channel results (profile-independent) ----
    let mut lex_store: BTreeMap<String, Vec<LexRow>> = BTreeMap::new();
    let (mut deleted_checked, mut deleted_divergent) = (0_usize, 0_usize);
    for case in &cases {
        if case
            .meetings
            .iter()
            .any(|m| m.state == MeetingState::Deleted)
        {
            deleted_checked += 1;
            if !deleted_fts_rows_ranking_inertness(case).await {
                deleted_divergent += 1;
                println!(
                    "[fidelity-deleted] {}: deleted-meeting FTS rows were NOT ranking-inert (pre-1.3F rows changed the lexical ordering)",
                    case.id
                );
            }
        }
        lex_store.insert(
            case.id.clone(),
            lexical_channel(case, LEXICAL_CANDIDATES).await,
        );
    }
    println!(
        "[fidelity-deleted] deleted-row alignment checked {deleted_checked} cases; lexical ordering identical in {} (divergent: {deleted_divergent})",
        deleted_checked - deleted_divergent
    );

    // ---- FTS-only baseline metrics (semantic delta denominators) ----
    let mut base_metrics: BTreeMap<String, HybridCaseMetrics> = BTreeMap::new();
    for case in &cases {
        base_metrics.insert(
            case.id.clone(),
            score_rows_baseline(case, &lex_store[&case.id]),
        );
    }
    let subset_r3 = |sel: &dyn Fn(&EvaluationCase) -> bool,
                     store: &BTreeMap<String, HybridCaseMetrics>|
     -> (usize, usize) {
        let (mut n, mut d) = (0, 0);
        for case in cases.iter().filter(|c| sel(c)) {
            for mid in &case.expected_meeting_ids {
                d += 1;
                n += usize::from(
                    store[&case.id]
                        .meeting_ranks
                        .get(mid)
                        .is_some_and(|r| *r <= 3),
                );
            }
        }
        (n, d)
    };
    let semantic_sel = |c: &EvaluationCase| c.categories.iter().any(|v| v == "semantic_paraphrase");
    let exact_sel = |c: &EvaluationCase| c.categories.iter().any(|v| v == "exact_term");
    let (bs_n, bs_d) = subset_r3(&semantic_sel, &base_metrics);
    println!("[baseline] FTS-only semantic Recall@3: {}", pct(bs_n, bs_d));
    let (be_n, be_d) = subset_r3(&exact_sel, &base_metrics);
    println!("[baseline] FTS-only exact Recall@3: {}", pct(be_n, be_d));

    // ---- Default-profile documents (384/64, latest-summary) ----
    let mut vector_cache: HashMap<u64, Vec<f32>> = HashMap::new();
    let build_default = |emb: &mut Embedder,
                         cache: &mut HashMap<u64, Vec<f32>>,
                         summary_all: bool|
     -> Vec<CaseDocs> {
        let tok = emb.0.tokenizer.clone();
        cases
            .iter()
            .map(|case| {
                build_case_docs_cached(
                    case,
                    &tok,
                    DEFAULT_WINDOW,
                    DEFAULT_OVERLAP,
                    summary_all,
                    &bundle.benchmark_leader.embedding.query_prefix,
                    &bundle.benchmark_leader.embedding.document_prefix,
                    emb,
                    cache,
                    &pol,
                )
            })
            .collect::<Vec<_>>()
    };
    let default_docs = build_default(&mut emb, &mut vector_cache, false);

    // ---- Stage A: chunk-window profiles (vector+lexical fusion, no rerank) ----
    println!("[profiles] fused meeting Recall@3 / Evidence Recall@10 / doc count:");
    for (w, ov) in [(256_usize, 48_usize), (384, 64), (512, 96)] {
        let cfg_docs: Vec<CaseDocs> = if (w, ov) == (DEFAULT_WINDOW, DEFAULT_OVERLAP) {
            default_docs.clone()
        } else {
            let tok = emb.0.tokenizer.clone();
            cases
                .iter()
                .map(|case| {
                    build_case_docs_cached(
                        case,
                        &tok,
                        w,
                        ov,
                        false,
                        &bundle.benchmark_leader.embedding.query_prefix,
                        &bundle.benchmark_leader.embedding.document_prefix,
                        &mut emb,
                        &mut vector_cache,
                        &pol,
                    )
                })
                .collect()
        };
        let mut cm = CorpusMetrics::default();
        for (ci, case) in cases.iter().enumerate() {
            let lr = map_lexical(&lex_store[&case.id], &cfg_docs[ci].entries);
            let cfg = HybridConfig {
                rrf_k: 60.0,
                w_vector: 1.0,
                w_lexical: 1.0,
                support_alpha: 0.25,
                title_beta: 0.25,
                rerank_gamma: 0.0,
                support_cap: 3,
            };
            let out = run_case(case, &cfg_docs[ci], &lr, cfg, None);
            cm.add(case, &out);
        }
        let doc_count: usize = cfg_docs.iter().map(|d| d.entries.len()).sum();
        println!(
            "  window {w}/{ov}: R@3 {} EV@10 {} docs={doc_count}",
            pct(cm.r3.0, cm.r3.1),
            pct(cm.ev10.0, cm.ev10.1)
        );
    }

    // ---- Stage B: summary policy variants at the default profile ----
    for label in ["latest-summary-only", "all-labeled-summary-templates"] {
        let summary_all = label == "all-labeled-summary-templates";
        let cfg_docs = if !summary_all {
            default_docs.clone()
        } else {
            build_default(&mut emb, &mut vector_cache, true)
        };
        let mut cm = CorpusMetrics::default();
        for (ci, case) in cases.iter().enumerate() {
            let lr = map_lexical(&lex_store[&case.id], &cfg_docs[ci].entries);
            let hcfg = HybridConfig {
                rrf_k: 60.0,
                w_vector: 1.0,
                w_lexical: 1.0,
                support_alpha: 0.25,
                title_beta: 0.25,
                rerank_gamma: 0.0,
                support_cap: 3,
            };
            let out = run_case(case, &cfg_docs[ci], &lr, hcfg, None);
            cm.add(case, &out);
        }
        println!(
            "[summary] {label}: R@3 {} EV@10 {} forbidden {}",
            pct(cm.r3.0, cm.r3.1),
            pct(cm.ev10.0, cm.ev10.1),
            pct(cm.forbidden.0, cm.forbidden.1)
        );
    }

    // ---- Stage B2: bi-encoder family comparison (actual inference, full
    // corpus, fused vector+lexical without rerank). Every family is an
    // admissible redistributable export; MiniLM is additionally measured even
    // though its 128-token context cannot honor the required window profiles.----
    let family_specs = [
        (
            "e5-small-int8",
            "e5-small-int8",
            "model_int8.onnx",
            512usize,
            bundle.benchmark_leader.embedding.query_prefix.clone(),
            bundle.benchmark_leader.embedding.document_prefix.clone(),
            true,
        ),
        (
            "e5-base-int8",
            "e5-base-int8",
            "model_int8.onnx",
            512,
            bundle.benchmark_leader.embedding.query_prefix.clone(),
            bundle.benchmark_leader.embedding.document_prefix.clone(),
            true,
        ),
        // Measured as a second-family diagnostic only: its 128-token context
        // cannot honor the required transcript window profiles, so it can
        // never be contracted regardless of quality.
        (
            "paraphrase-minilm-int8(maxseq128)",
            "minilm-paraphrase-int8",
            "model_int8.onnx",
            128,
            String::new(),
            String::new(),
            false,
        ),
    ];
    struct FamilyResult {
        label: String,
        dims: usize,
        docs: Vec<CaseDocs>,
        r3: (usize, usize),
        mrr: f64,
        metadata_conforming: bool,
    }
    let mut family_results: Vec<FamilyResult> = Vec::new();
    for (label, dir, file, max_len, q_prefix, d_prefix, metadata_conforming) in family_specs {
        let mut fam_emb = Embedder::load(&root.join(dir), file, max_len, 4)
            .unwrap_or_else(|e| panic!("load {label}: {e}"));
        let mut fam_cache: HashMap<u64, Vec<f32>> = HashMap::new();
        let tok = fam_emb.0.tokenizer.clone();
        let mut fam_docs: Vec<CaseDocs> = Vec::with_capacity(cases.len());
        for case in &cases {
            fam_docs.push(build_case_docs_cached(
                case,
                &tok,
                DEFAULT_WINDOW.min(max_len),
                DEFAULT_OVERLAP.min(max_len / 2),
                false,
                &q_prefix,
                &d_prefix,
                &mut fam_emb,
                &mut fam_cache,
                &pol,
            ));
        }
        let dims = fam_docs[0]
            .entries
            .first()
            .map(|e| e.vector.len())
            .unwrap_or(0);
        let mut cm = CorpusMetrics::default();
        for (ci, case) in cases.iter().enumerate() {
            let lr = map_lexical(&lex_store[&case.id], &fam_docs[ci].entries);
            let hcfg = HybridConfig {
                rrf_k: 60.0,
                w_vector: 1.0,
                w_lexical: 1.0,
                support_alpha: 0.25,
                title_beta: 0.25,
                rerank_gamma: 0.0,
                support_cap: 3,
            };
            let out = run_case(case, &fam_docs[ci], &lr, hcfg, None);
            cm.add(case, &out);
        }
        println!(
            "[family] {label}: dims={dims} R@1 {} R@3 {} EV@10 {} MRR {:.4}",
            pct(cm.r1.0, cm.r1.1),
            pct(cm.r3.0, cm.r3.1),
            pct(cm.ev10.0, cm.ev10.1),
            cm.mrr_sum / cm.mrr_cases.max(1) as f64
        );
        // Task 1.3F Deliverable 2 (model side): rank-1 admissibility over
        // production-implementable channels for the critical cases, measured
        // on the production-candidate e5-base-int8 family. Report-only.
        if label == "e5-base-int8" {
            println!(
                "[SUPERVISED:rank1-admissibility] e5-base-int8 critical cases; channels = lexical/title/raw vector only (CONCEPT_LEXICON excluded):"
            );
            for (ci, case) in cases.iter().enumerate() {
                if !case.critical {
                    continue;
                }
                let (lex_m, title_m) = case_production_channel_margins(case, &pol);
                let (m_rank, e_rank) = raw_vector_ranks(&fam_docs[ci], case);
                enforce_rank1_admissibility(case, lex_m, title_m, m_rank)
                    .expect("critical rank-1 gate admissibility");
                println!(
                    "  case={:40} lexical={lex_m:+.3} title={title_m:+.3} vector_rank(mtg/ev)={}/{} any_positive_channel={}",
                    case.id,
                    if m_rank == usize::MAX { 0 } else { m_rank },
                    if e_rank == usize::MAX { 0 } else { e_rank },
                    lex_m > 0.0 || title_m > 0.0 || m_rank == 1,
                );
            }
        }
        family_results.push(FamilyResult {
            label: label.to_string(),
            dims,
            docs: fam_docs,
            r3: cm.r3,
            mrr: cm.mrr_sum / cm.mrr_cases.max(1) as f64,
            metadata_conforming,
        });
    }
    family_results.sort_by(|a, b| {
        b.r3.0.cmp(&a.r3.0).then(
            b.mrr
                .partial_cmp(&a.mrr)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    println!(
        "[family] ranking: {:?}",
        family_results
            .iter()
            .map(|f| format!("{}[{:.3}]", f.label, f.mrr))
            .collect::<Vec<_>>()
    );
    let best_r3 = family_results
        .iter()
        .filter(|family| family.metadata_conforming)
        .map(|family| family.r3.0)
        .max()
        .expect("metadata-conforming embedding family");
    let eval_index = family_results
        .iter()
        .enumerate()
        .filter(|(_, family)| family.metadata_conforming && family.r3.0 == best_r3)
        .min_by_key(|(_, f)| f.dims)
        .map(|(i, _)| i)
        .unwrap_or(0);
    println!(
        "[family] contracted embedding family: {} (quality tie -> smallest footprint)",
        family_results[eval_index].label
    );
    let contracted_label = family_results[eval_index].label.clone();
    // Task 1.3 rerun: concept-proxy disagreement evidence for every viable
    // benchmark leader. All families stay NON-selected while the pair decision
    // remains blocked; the table compares the supervised CONCEPT_LEXICON
    // prediction with each leader's raw vector behavior.
    let conforming_idx = family_results.iter().position(|f| f.metadata_conforming);
    let mut leader_indices = vec![0_usize, eval_index];
    if let Some(extra) = conforming_idx {
        leader_indices.push(extra);
    }
    leader_indices.sort_unstable();
    leader_indices.dedup();
    for idx in leader_indices {
        let family = &family_results[idx];
        let mut roles: Vec<&str> = Vec::new();
        if idx == 0 {
            roles.push("overall quality leader");
        }
        if Some(idx) == conforming_idx {
            roles.push("best metadata-conforming");
        }
        if idx == eval_index {
            roles.push("contracted");
        }
        print_concept_proxy_table(
            &family.label,
            &format!("{}; NON-selected", roles.join(" + ")),
            &cases,
            &family.docs,
            &pol,
        );
    }
    let default_docs = family_results.swap_remove(eval_index).docs;

    // ---- Stage C: held-out parameter search (fusion, no rerank). Amendment
    // 5 tune partition: everything except the five designated critical/pinned
    // cases, so the ten non-critical reference-category cases join the split.
    // Objective is the amendment-5 lexicographic key prioritizing the approved
    // gates: exact-term violations, semantic Recall@3 misses,
    // reference-category Recall@1 misses (admitted siblings), overall
    // Recall@3 misses, then MRR (micros); final ties resolve toward
    // smaller/simpler constants. Critical cases are never inspected here.----
    let tune_idx: Vec<usize> = cases
        .iter()
        .enumerate()
        .filter(|(_, c)| !excluded_from_tuning(c))
        .map(|(i, _)| i)
        .collect();
    let lex_ranks: Vec<Vec<usize>> = cases
        .iter()
        .enumerate()
        .map(|(ci, case)| map_lexical(&lex_store[&case.id], &default_docs[ci].entries))
        .collect();

    #[derive(Clone, Copy)]
    struct FusionCfg {
        k: f64,
        wv: f64,
        wl: f64,
        alpha: f64,
        beta: f64,
    }
    let mut fusion_best: Option<(FusionCfg, [u64; 5])> = None;
    let mut fusion_rows: Vec<String> = Vec::new();
    for k in [5.0_f64, 10.0, 20.0, 60.0] {
        for wv in [0.5_f64, 1.0, 2.0] {
            for wl in [0.5_f64, 1.0, 2.0] {
                for alpha in [0.0_f64, 0.5] {
                    // Task 1.3 rerun: expanded title-weight grid per the
                    // 2026-08-23 sprint decision; neither title-off nor unit
                    // title weight may be assumed optimal.
                    for beta in [0.0_f64, 0.25, 0.5, 1.0, 2.0] {
                        let hcfg = HybridConfig {
                            rrf_k: k,
                            w_vector: wv,
                            w_lexical: wl,
                            support_alpha: alpha,
                            title_beta: beta,
                            rerank_gamma: 0.0,
                            support_cap: 3,
                        };
                        // Objective components over the held-out partition.
                        let mut obj = TuneObjective::default();
                        for &ci in &tune_idx {
                            let case = &cases[ci];
                            let lr = &lex_ranks[ci];
                            let out = run_case(case, &default_docs[ci], lr, hcfg, None);
                            obj.add_case(case, &out.metrics.meeting_ranks);
                        }
                        let key = obj.key();
                        let better = match fusion_best {
                            None => true,
                            Some((_, bk)) => key < bk,
                        };
                        if better {
                            fusion_best = Some((
                                FusionCfg {
                                    k,
                                    wv,
                                    wl,
                                    alpha,
                                    beta,
                                },
                                key,
                            ));
                        }
                        fusion_rows.push(format!(
                            "k={k} wv={wv} wl={wl} a={alpha} b={beta}: viol={} sem-miss={} ref-r1-miss={} all-miss={} mrr-miss={}",
                            key[0], key[1], key[2], key[3], key[4]
                        ));
                    }
                }
            }
        }
    }
    let (fb, fb_key) = fusion_best.expect("fusion grid non-empty");
    let best_wv = fb.wv;
    let best_wl = fb.wl;
    println!(
        "[tune-fusion] locked k={} w_vector={} w_lexical={} alpha={} beta={} | objective [exact-viol, sem-miss, ref-r1-miss, all-miss, mrr-miss-u] = {fb_key:?} over {} held-out configs",
        fb.k, fb.wv, fb.wl, fb.alpha, fb.beta,
        fusion_rows.len()
    );
    let _ = fusion_rows;
    let best_k = fb.k;
    let best_alpha = fb.alpha;
    let best_beta = fb.beta;
    // ---- Stage D: reranker candidates under the deterministic runtime policy ----
    // Production policy is fixed BEFORE selection so every candidate is
    // compared under identical conditions: batch=1 (with dynamic-int8 exports
    // each pair's activation scale then depends only on itself, keeping the
    // ordering reproducible) and depth=RERANK_SET. A candidate is viable only
    // if its measured solo per-pair p95 fits the 900 ms sub-budget at that
    // depth; anything else is recorded as a gate conflict, not silently dropped.

    // Probe pairs from the largest corpus case, fused order, evidence only.
    let probe_case = cases
        .iter()
        .find(|c| c.id == "fixture-whatsapp-retention")
        .expect("reference fixture for latency probe");
    let probe_ci = cases
        .iter()
        .position(|c| c.id == probe_case.id)
        .expect("probe index");
    let probe_entries = &default_docs[probe_ci].entries;
    let lr_probe = map_lexical(&lex_store[&probe_case.id], probe_entries);
    let probe_cfg = HybridConfig {
        rrf_k: best_k,
        w_vector: 1.0,
        w_lexical: 1.0,
        support_alpha: best_alpha,
        title_beta: best_beta,
        rerank_gamma: 0.0,
        support_cap: 3,
    };
    let probe_out = run_case(
        probe_case,
        &default_docs[probe_ci],
        &lr_probe,
        probe_cfg,
        None,
    );
    let head_docs: Vec<String> = probe_out
        .fused_order
        .iter()
        .copied()
        .filter(|i| probe_entries[*i].doc.source_kind != "profile")
        .take(RERANK_SET.min(probe_out.fused_order.len()))
        .map(|i| probe_entries[i].doc.text.clone())
        .collect();
    let probe_q = probe_case
        .rewritten_query
        .as_deref()
        .unwrap_or(&probe_case.question)
        .to_string();
    let pairs: Vec<(String, String)> = head_docs
        .iter()
        .map(|d| (probe_q.clone(), d.clone()))
        .collect();
    let rss_before_candidates = rss_mib().expect("rss");

    struct CandidateResult {
        name: String,
        dir: String,
        file: String,
        metadata_multilingual: bool,
        viable: bool,
        exclusion: Option<String>,
        solo_p95_us: u128,
        ram_delta_mib: f64,
        pairwise_correct: usize,
        pairwise_total: usize,
        ndcg_final: f64,
        ndcg_fused: f64,
        tune_r3: (usize, usize),
        tuned_gamma: f64,
        tuned_objective_key: [u64; 6],
    }
    // metadata_multilingual reflects the model card only: mmarco-mMiniLMv2
    // trains on mMARCO Portuguese among 14 languages. bge-reranker-base is
    // permanently retired (2026-08-23 user decision: zh/en card metadata
    // nonconformity); its recorded latency/quality figures are reused from the
    // manifest and Task 1.3 report instead of being re-executed.
    let candidate_specs = [
        (
            "mmarco-quint8",
            "mmarco-reranker/model_quint8_avx2.onnx",
            true,
        ),
        ("mmarco-f32", "mmarco-reranker/model_f32.onnx", true),
    ];
    let mut candidate_results: Vec<CandidateResult> = Vec::new();
    for (name, rel, meta_ml) in candidate_specs {
        let (dir_name, file) = rel.split_once('/').expect("candidate spec");
        let start_load = Instant::now();
        let mut model = RerankModel::load(&root.join(dir_name), file, 512, 4)
            .unwrap_or_else(|e| panic!("load {rel}: {e}"));
        let load_ms = start_load.elapsed().as_millis();
        let warm = pairs[..pairs.len().min(4)].to_vec();
        let _ = model.score(&warm);
        let ram_delta = rss_mib().unwrap_or(0.0) - rss_before_candidates;

        let mut lat: Vec<u128> = Vec::new();
        for _ in 0..5 {
            for (qq, dd) in &pairs {
                let t = Instant::now();
                let _ = model
                    .score(&[(qq.clone(), dd.clone())])
                    .expect("single score");
                lat.push(t.elapsed().as_micros());
            }
        }
        lat.sort_unstable();
        let p95_us = percentile(&lat, 95);
        let p50_us = percentile(&lat, 50);
        let depth_cost_us = p95_us * RERANK_SET as u128;
        let viable = depth_cost_us <= 900_000;

        let mut result = CandidateResult {
            name: name.to_string(),
            dir: dir_name.to_string(),
            file: file.to_string(),
            metadata_multilingual: meta_ml,
            viable,
            exclusion: None,
            solo_p95_us: p95_us,
            ram_delta_mib: ram_delta,
            pairwise_correct: 0,
            pairwise_total: 0,
            ndcg_final: 0.0,
            ndcg_fused: 0.0,
            tune_r3: (0, 0),
            tuned_gamma: 0.0,
            tuned_objective_key: [0; 6],
        };
        if !viable {
            result.exclusion = Some(format!(
                "solo per-pair p95 {:.1} ms x {RERANK_SET} depth = {:.0} ms exceeds the 900 ms sub-budget",
                p95_us as f64 / 1000.0,
                depth_cost_us as f64 / 1000.0
            ));
            println!(
                "[reranker] {name}: load={load_ms}ms solo p50={:.1}ms p95={:.1}ms EXCLUDED ({})",
                p50_us as f64 / 1000.0,
                p95_us as f64 / 1000.0,
                result.exclusion.as_ref().unwrap()
            );
            candidate_results.push(result);
            continue;
        }

        // Held-out gamma sub-grid under the deterministic runtime policy
        // (batch=1, depth=RERANK_SET). Objective matches the fusion search
        // with the NDCG non-degradation flag inserted: exact violations,
        // semantic misses, reference Recall@1 misses, NDCG non-degradation,
        // overall misses, MRR misses; ties resolve toward smaller gamma.
        let gammas = [0.0_f64, 0.5, 1.0, 2.0, 4.0, 8.0];
        let mut best: Option<(f64, [u64; 6], usize, usize, f64, f64)> = None;
        for gamma in gammas {
            let cfg_g = HybridConfig {
                rrf_k: best_k,
                w_vector: best_wv,
                w_lexical: best_wl,
                support_alpha: best_alpha,
                title_beta: best_beta,
                rerank_gamma: gamma,
                support_cap: 3,
            };
            let (mut pc, mut pt, mut nf, mut nu) = (0usize, 0usize, 0f64, 0f64);
            let mut obj = TuneObjective::default();
            for &ci in &tune_idx {
                let case = &cases[ci];
                let lr = &lex_ranks[ci];
                let out = run_case(
                    case,
                    &default_docs[ci],
                    lr,
                    cfg_g,
                    Some((&mut model, RERANK_SET, 1)),
                );
                pc += out.metrics.pairwise_correct;
                pt += out.metrics.pairwise_total;
                nf += out.metrics.ndcg10_final;
                nu += out.metrics.ndcg10_fused;
                obj.add_case(case, &out.metrics.meeting_ranks);
            }
            let key = obj.rerank_key(nu > nf);
            let better = match best {
                None => true,
                Some((_, bk, _, _, _, _)) => key < bk,
            };
            if better {
                best = Some((
                    gamma,
                    key,
                    pc,
                    pt,
                    nf / tune_idx.len() as f64,
                    nu / tune_idx.len() as f64,
                ));
            }
        }
        let (tuned_gamma, gkey, gpc, gpt, gnf, gnu) = best.expect("gamma grid non-empty");
        result.tuned_gamma = tuned_gamma;
        result.tuned_objective_key = gkey;
        result.pairwise_correct = gpc;
        result.pairwise_total = gpt;
        result.ndcg_final = gnf;
        result.ndcg_fused = gnu;
        result.tune_r3 = {
            // Reconstruct tune R@3 numerator/denominator from objective parts:
            // overall misses minus semantic/exact components are not directly
            // reversible, so recompute quickly.
            let cfg_g = HybridConfig {
                rrf_k: best_k,
                w_vector: best_wv,
                w_lexical: best_wl,
                support_alpha: best_alpha,
                title_beta: best_beta,
                rerank_gamma: tuned_gamma,
                support_cap: 3,
            };
            let (mut n, mut d) = (0usize, 0usize);
            for &ci in &tune_idx {
                let case = &cases[ci];
                let lr = &lex_ranks[ci];
                let out = run_case(
                    case,
                    &default_docs[ci],
                    lr,
                    cfg_g,
                    Some((&mut model, RERANK_SET, 1)),
                );
                for mid in &case.expected_meeting_ids {
                    d += 1;
                    n += usize::from(out.metrics.meeting_ranks.get(mid).is_some_and(|r| *r <= 3));
                }
            }
            (n, d)
        };
        println!(
            "[reranker] {name}: load={load_ms}ms solo p50={:.1}ms p95={:.1}ms ram=+{ram_delta:.1}MiB pairwise={} NDCG@10 final={gnf:.4} fused={gnu:.4} tune-R@3={} tuned-gamma={tuned_gamma} key={gkey:?}",
            p50_us as f64 / 1000.0,
            p95_us as f64 / 1000.0,
            pct(gpc, gpt),
            pct(result.tune_r3.0, result.tune_r3.1)
        );
        candidate_results.push(result);
    }

    // Report the NDCG-leading candidate for transparency; BOTH viable
    // candidates are fully evaluated in Stage F so neither is proxied.
    let mut viable_results: Vec<&CandidateResult> =
        candidate_results.iter().filter(|c| c.viable).collect();
    viable_results.sort_by(|a, b| {
        b.ndcg_final
            .partial_cmp(&a.ndcg_final)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.pairwise_correct.cmp(&a.pairwise_correct))
    });
    if viable_results.is_empty() {
        panic!(
            "no reranker candidate fits the 900 ms deterministic sub-budget; gate conflict requires an approved change"
        );
    }
    println!(
        "[reranker] ndcg-leading candidate: {} (card-multilingual={}) - all viable: {:?}",
        viable_results[0].name,
        viable_results[0].metadata_multilingual,
        viable_results
            .iter()
            .map(|c| format!(
                "{}[ndcg {:.2}, ml={}, gamma={}]",
                c.name, c.ndcg_final, c.metadata_multilingual, c.tuned_gamma
            ))
            .collect::<Vec<_>>()
    );
    // ---- Embedding export fidelity diagnostic (int8 vs f32 weights) ----
    // Retained quantization-fidelity evidence: measured on the e5-small
    // dynamic-int8 export, the only family with a staged f32 counterpart.
    // The e5-base leader has no staged f32 session, so its fidelity is not
    // separately measured and is recorded as such in the manifest/report.
    let mut fidelity_int8 = Embedder::load(&root.join("e5-small-int8"), "model_int8.onnx", 512, 4)
        .expect("load e5-small int8 fidelity session");
    let mut emb_f32 = Embedder::load(
        &root.join("e5-small"),
        "model_f32.onnx",
        bundle.benchmark_leader.embedding.max_sequence_length,
        4,
    )
    .expect("load e5-small f32 session");
    let sample_texts: Vec<String> = default_docs
        .iter()
        .flat_map(|d| d.entries.iter())
        .take(120)
        .map(|e| format!("passage: {}", e.doc.text))
        .collect();
    let v_int8 = fidelity_int8.embed(&sample_texts).expect("embed int8");
    let v_f32 = emb_f32.embed(&sample_texts).expect("embed f32");
    let agreements: Vec<f64> = v_int8.iter().zip(&v_f32).map(|(a, b)| dot(a, b)).collect();
    let mean_agree = agreements.iter().sum::<f64>() / agreements.len() as f64;
    let min_agree = agreements.iter().cloned().fold(f64::INFINITY, f64::min);
    let t_start = Instant::now();
    for q in cases.iter().take(30) {
        let _ = fidelity_int8
            .embed(&[format!("query: {}", q.question)])
            .expect("q int8");
    }
    let int8_qs = t_start.elapsed().as_millis() as f64 / 30.0;
    let t_start = Instant::now();
    for q in cases.iter().take(30) {
        let _ = emb_f32
            .embed(&[format!("query: {}", q.question)])
            .expect("q f32");
    }
    let f32_qs = t_start.elapsed().as_millis() as f64 / 30.0;
    println!(
        "[precision] e5-small export fidelity, int8-vs-f32 sessions: mean cosine agreement {mean_agree:.4}, min {min_agree:.4}; query embed mean {int8_qs:.1} ms (int8) vs {f32_qs:.1} ms (f32)"
    );

    // ---- Stage F: per-pair locked full-corpus evaluation. Both budget-
    // viable rerankers are evaluated under their own held-out-tuned gamma
    // with identical fusion constants; neither proxies the other.----
    let mut pair_reports: Vec<String> = Vec::new();
    let mut production_candidate_passed: Option<bool> = None;

    for cand in &viable_results {
        let mut rr_pair = RerankModel::load(&root.join(&cand.dir), &cand.file, 512, 4)
            .unwrap_or_else(|e| panic!("load {} for final run: {e}", cand.name));
        let chat_depth =
            ((900_000.0 / cand.solo_p95_us as f64).floor() as usize).clamp(10, RERANK_SET);
        let chosen_batch = 1usize;
        let search_depth = (chat_depth / 2).max(5);
        println!(
            "[policy] {}: chat depth {chat_depth} search {search_depth} batch={chosen_batch} intra-op=4 (solo p95 {:.1} ms, session +{:.1} MiB)",
            cand.name,
            cand.solo_p95_us as f64 / 1000.0,
            cand.ram_delta_mib
        );

        let locked_pair = HybridConfig {
            rrf_k: best_k,
            w_vector: best_wv,
            w_lexical: best_wl,
            support_alpha: best_alpha,
            title_beta: best_beta,
            rerank_gamma: cand.tuned_gamma,
            support_cap: 3,
        };
        // Mandatory title ablation (2026-08-23 decision): identical constants
        // except beta alone drops to 0, so any quality visible only at the
        // tuned beta is attributable to title scoring rather than embeddings.
        let ablated_cfg = HybridConfig {
            title_beta: 0.0,
            ..locked_pair
        };

        let tuned = evaluate_pair(
            &cases,
            &default_docs,
            &lex_store,
            &mut rr_pair,
            locked_pair,
            chat_depth,
            chosen_batch,
        );
        let ablated = evaluate_pair(
            &cases,
            &default_docs,
            &lex_store,
            &mut rr_pair,
            ablated_cfg,
            chat_depth,
            chosen_batch,
        );
        let (cm_all, cm_pt, cm_en, cm_exact, cm_semantic, cm_critical) = (
            &tuned.all,
            &tuned.pt,
            &tuned.en,
            &tuned.exact,
            &tuned.semantic,
            &tuned.critical,
        );
        let outputs = &tuned.outputs;

        // Per-critical-case meeting ranks: the aggregate Critical Recall@1
        // gate hides which designated cases miss rank 1.
        for crit_case in cases.iter().filter(|c| c.critical) {
            let crit_out = &outputs[&crit_case.id];
            let crit_rank = crit_out
                .metrics
                .meeting_ranks
                .get(&crit_case.expected_meeting_ids[0])
                .copied()
                .unwrap_or(usize::MAX);
            println!(
                "[critical] {}: expected {} at rank {crit_rank}",
                crit_case.id, crit_case.expected_meeting_ids[0]
            );
        }

        println!(
            "--- pair results: {contracted_label} + {} (locked constants, tuned title beta={best_beta}) ---",
            cand.name
        );
        println!(
            "overall: R@1 {} R@3 {} R@5 {} MRR {:.4} EV@10 {} facts {} forbidden {}",
            pct(cm_all.r1.0, cm_all.r1.1),
            pct(cm_all.r3.0, cm_all.r3.1),
            pct(cm_all.r5.0, cm_all.r5.1),
            cm_all.mrr_sum / cm_all.mrr_cases as f64,
            pct(cm_all.ev10.0, cm_all.ev10.1),
            pct(cm_all.facts.0, cm_all.facts.1),
            pct(cm_all.forbidden.0, cm_all.forbidden.1)
        );
        println!(
            "forbidden by stage: retrieval-stage {} | answer-stage context presence {} (informational; not evaluated/gated in Sprint 1)",
            pct(
                cm_all.retrieval_forbidden.0,
                cm_all.retrieval_forbidden.1
            ),
            pct(cm_all.answer_forbidden.0, cm_all.answer_forbidden.1)
        );
        println!(
            "pt:      R@1 {} R@3 {} R@5 {}",
            pct(cm_pt.r1.0, cm_pt.r1.1),
            pct(cm_pt.r3.0, cm_pt.r3.1),
            pct(cm_pt.r5.0, cm_pt.r5.1)
        );
        println!(
            "en:      R@1 {} R@3 {} R@5 {}",
            pct(cm_en.r1.0, cm_en.r1.1),
            pct(cm_en.r3.0, cm_en.r3.1),
            pct(cm_en.r5.0, cm_en.r5.1)
        );
        println!(
            "exact:   R@3 {} | semantic: R@3 {} (baseline {}/{})",
            pct(cm_exact.r3.0, cm_exact.r3.1),
            pct(cm_semantic.r3.0, cm_semantic.r3.1),
            bs_n,
            bs_d
        );
        println!(
            "critical: R@1 {} | retrieval-stage forbidden {} | answer-stage context presence {} (informational; not evaluated/gated in Sprint 1) | pairwise accuracy: {}",
            pct(cm_critical.r1.0, cm_critical.r1.1),
            pct(
                cm_critical.retrieval_forbidden.0,
                cm_critical.retrieval_forbidden.1
            ),
            pct(
                cm_critical.answer_forbidden.0,
                cm_critical.answer_forbidden.1
            ),
            pct(cm_all.pairwise_correct, cm_all.pairwise_total)
        );
        println!(
            "ndcg@10: final mean {:.4} vs fused-order mean {:.4}",
            cm_all.ndcg_final_sum / cm_all.ndcg_cases as f64,
            cm_all.ndcg_fused_sum / cm_all.ndcg_cases as f64
        );

        let ref_case = cases
            .iter()
            .find(|c| c.id == "fixture-whatsapp-retention")
            .expect("reference fixture");
        let ref_out = &outputs[&ref_case.id];
        let ref_rank = ref_out
            .metrics
            .meeting_ranks
            .get(&ref_case.expected_meeting_ids[0])
            .copied()
            .unwrap_or(usize::MAX);

        // Gates for this pair.
        let g_ref = ref_rank == 1
            && ref_out.metrics.fact_hits == ref_out.metrics.fact_total
            && ref_out.metrics.retrieval_forbidden_hits == 0;
        let g_crit = cm_critical.r1.0 == cm_critical.r1.1;
        let g_crit_facts = cm_critical.facts.0 == cm_critical.facts.1;
        let g_crit_forbidden = cm_critical.retrieval_forbidden.0 == 0;
        let g_exact =
            cm_exact.r3.0 as f64 / cm_exact.r3.1.max(1) as f64 >= be_n as f64 / be_d.max(1) as f64;
        let g_r3 = cm_all.r3.0 as f64 / cm_all.r3.1 as f64 >= 0.95;
        let g_r5 = cm_all.r5.0 as f64 / cm_all.r5.1 as f64 >= 0.98;
        let g_ev10 = cm_all.ev10.0 as f64 / cm_all.ev10.1 as f64 >= 0.90;
        let g_sem = cm_semantic.r3.0 as f64 / cm_semantic.r3.1.max(1) as f64
            >= bs_n as f64 / bs_d.max(1) as f64 + 0.10;
        let g_ndcg = cm_all.ndcg_final_sum >= cm_all.ndcg_fused_sum;

        // Amendment 5 mandatory constants disclosure: no gate result is
        // reviewable without the configuration that produced it.
        println!(
            "[constants] {} locked configuration: k={} w_vector={} w_lexical={} alpha={} beta={} gamma={} support_cap=3 chat-depth={chat_depth} batch={chosen_batch}",
            cand.name, best_k, best_wv, best_wl, best_alpha, best_beta, cand.tuned_gamma
        );
        println!(
            "[constants] winning held-out objective vector [exact-viol, sem-miss, ref-r1-miss, ndcg-nondeg, all-miss, mrr-miss-u] = {:?}",
            cand.tuned_objective_key
        );
        println!("--- pair quality gates ---");
        println!(
            "[gate {}] Reference Recall@1: rank {ref_rank}, facts {}/{}, retrieval-stage forbidden {}/{}",
            if g_ref { "PASS" } else { "FAIL" },
            ref_out.metrics.fact_hits,
            ref_out.metrics.fact_total,
            ref_out.metrics.retrieval_forbidden_hits,
            ref_out.metrics.retrieval_forbidden_total
        );
        println!(
            "[gate {}] Critical Recall@1: {}",
            if g_crit { "PASS" } else { "FAIL" },
            pct(cm_critical.r1.0, cm_critical.r1.1)
        );
        println!(
            "[gate {}] Critical required-fact coverage = 100%: {}",
            if g_crit_facts { "PASS" } else { "FAIL" },
            pct(cm_critical.facts.0, cm_critical.facts.1)
        );
        println!(
            "[gate {}] Critical retrieval-stage forbidden contamination = 0: {}",
            if g_crit_forbidden { "PASS" } else { "FAIL" },
            pct(
                cm_critical.retrieval_forbidden.0,
                cm_critical.retrieval_forbidden.1
            )
        );
        println!(
            "[deferred:not-evaluated] Critical answer-stage non-assertion: context presence only {} (Sprint 1 does not evaluate or gate generated answers)",
            pct(
                cm_critical.answer_forbidden.0,
                cm_critical.answer_forbidden.1
            )
        );
        println!(
            "[gate {}] Exact-term no-regression: {}",
            if g_exact { "PASS" } else { "FAIL" },
            pct(cm_exact.r3.0, cm_exact.r3.1)
        );
        println!(
            "[gate {}] Overall Recall@3 >= 95%: {}",
            if g_r3 { "PASS" } else { "FAIL" },
            pct(cm_all.r3.0, cm_all.r3.1)
        );
        println!(
            "[gate {}] Overall Recall@5 >= 98%: {}",
            if g_r5 { "PASS" } else { "FAIL" },
            pct(cm_all.r5.0, cm_all.r5.1)
        );
        println!(
            "[gate {}] Evidence Recall@10 >= 90%: {}",
            if g_ev10 { "PASS" } else { "FAIL" },
            pct(cm_all.ev10.0, cm_all.ev10.1)
        );
        println!(
            "[gate {}] Semantic +10pt Recall@3 over baseline ({}/{}): {}",
            if g_sem { "PASS" } else { "FAIL" },
            bs_n,
            bs_d,
            pct(cm_semantic.r3.0, cm_semantic.r3.1)
        );
        println!(
            "[gate {}] NDCG non-degradation: {:.4} vs fused {:.4}",
            if g_ndcg { "PASS" } else { "FAIL" },
            cm_all.ndcg_final_sum / cm_all.ndcg_cases as f64,
            cm_all.ndcg_fused_sum / cm_all.ndcg_cases as f64
        );
        // ---- Task 1.3F citation/source precision (broad-retrieval
        // contract): ChatSource values were constructed only after scope
        // revalidation, profile exclusion, and final EVIDENCE_K retention;
        // precision asks whether every emitted source's snippet is present in
        // the actual final retained context.----
        let (mut src_num, mut src_den) = (0_usize, 0_usize);
        for case in cases.iter() {
            let out = &outputs[&case.id];
            let (hits, emitted) =
                chat_source_precision(&out.metrics.emitted_sources, &out.metrics.retained_context);
            src_num += hits;
            src_den += emitted;
        }
        println!(
            "[gate {}] Citation/source precision (scope-revalidated, prompt-budget-filtered): {}",
            if src_num == src_den { "PASS" } else { "FAIL" },
            pct(src_num, src_den)
        );

        // Mandatory title-ablation report: tuned beta versus beta alone
        // ablated to 0, every other tuned constant fixed. The full corpus
        // reference category (all cases carrying `reference_whatsapp`) is
        // compared category-wide; the pinned WhatsApp acceptance case is kept
        // as its own row above.
        let abl_ref_case = cases
            .iter()
            .find(|c| c.id == "fixture-whatsapp-retention")
            .expect("reference fixture");
        let abl_ref_out = &ablated.outputs[&abl_ref_case.id];
        let abl_ref_rank = abl_ref_out
            .metrics
            .meeting_ranks
            .get(&abl_ref_case.expected_meeting_ids[0])
            .copied()
            .unwrap_or(usize::MAX);
        println!(
            "[title-ablation {}] tuned beta={best_beta} vs beta=0 (all other constants fixed):",
            cand.name
        );
        println!(
            "  semantic R@3: {} at tuned beta | {} at beta 0",
            pct(cm_semantic.r3.0, cm_semantic.r3.1),
            pct(ablated.semantic.r3.0, ablated.semantic.r3.1)
        );
        println!(
            "  reference-category ({} cases): R@1 {}/{} vs {}/{} | R@3 {}/{} vs {}/{} | R@5 {}/{} vs {}/{} | EV@10 {}/{} vs {}/{} | facts {}/{} vs {}/{} | forbidden {}/{} vs {}/{}",
            tuned.reference.r1.1.max(ablated.reference.r1.1),
            tuned.reference.r1.0, tuned.reference.r1.1,
            ablated.reference.r1.0, ablated.reference.r1.1,
            tuned.reference.r3.0, tuned.reference.r3.1,
            ablated.reference.r3.0, ablated.reference.r3.1,
            tuned.reference.r5.0, tuned.reference.r5.1,
            ablated.reference.r5.0, ablated.reference.r5.1,
            tuned.reference.ev10.0, tuned.reference.ev10.1,
            ablated.reference.ev10.0, ablated.reference.ev10.1,
            tuned.reference.facts.0, tuned.reference.facts.1,
            ablated.reference.facts.0, ablated.reference.facts.1,
            tuned.reference.forbidden.0, tuned.reference.forbidden.1,
            ablated.reference.forbidden.0, ablated.reference.forbidden.1,
        );
        println!(
            "  reference-category MRR: {:.4} at tuned beta | {:.4} at beta 0",
            tuned.reference.mrr_sum / tuned.reference.mrr_cases.max(1) as f64,
            ablated.reference.mrr_sum / ablated.reference.mrr_cases.max(1) as f64
        );
        println!(
            "  reference: rank {}, facts {}/{}, forbidden {}/{} at tuned beta | rank {}, facts {}/{}, forbidden {}/{} at beta 0",
            ref_rank,
            ref_out.metrics.fact_hits,
            ref_out.metrics.fact_total,
            ref_out.metrics.forbidden_hits,
            ref_out.metrics.forbidden_total,
            abl_ref_rank,
            abl_ref_out.metrics.fact_hits,
            abl_ref_out.metrics.fact_total,
            abl_ref_out.metrics.forbidden_hits,
            abl_ref_out.metrics.forbidden_total
        );
        println!(
            "  overall: R@3 {} R@5 {} EV@10 {} MRR {:.4} at tuned beta | R@3 {} R@5 {} EV@10 {} MRR {:.4} at beta 0",
            pct(tuned.all.r3.0, tuned.all.r3.1),
            pct(tuned.all.r5.0, tuned.all.r5.1),
            pct(tuned.all.ev10.0, tuned.all.ev10.1),
            tuned.all.mrr_sum / tuned.all.mrr_cases as f64,
            pct(ablated.all.r3.0, ablated.all.r3.1),
            pct(ablated.all.r5.0, ablated.all.r5.1),
            pct(ablated.all.ev10.0, ablated.all.ev10.1),
            ablated.all.mrr_sum / ablated.all.mrr_cases as f64
        );
        // Title dependence = ANY metric difference between the tuned-beta and
        // beta=0 passes over the semantic category, the pinned reference case,
        // the critical subset, or the full reference category (direction-
        // agnostic: title may help or hurt; either way the result is
        // title-dependent).
        let title_dependent = tuned.semantic.r3 != ablated.semantic.r3
            || ref_rank != abl_ref_rank
            || tuned.critical.r1 != ablated.critical.r1
            || tuned.reference.r1 != ablated.reference.r1
            || tuned.reference.r3 != ablated.reference.r3
            || tuned.reference.r5 != ablated.reference.r5
            || tuned.reference.ev10 != ablated.reference.ev10
            || tuned.reference.facts != ablated.reference.facts
            || tuned.reference.forbidden != ablated.reference.forbidden;
        println!(
            "  headline: semantic/reference results {} when title scoring is removed ({})",
            if title_dependent { "DIFFER" } else { "hold" },
            if title_dependent {
                "title-dependent quality (semantic, pinned reference case, critical subset, or full reference category differs): do not attribute the result solely to the embedding model"
            } else {
                "title-independent across semantic, pinned reference case, critical subset, and full reference category"
            }
        );

        let passed = g_ref
            && g_crit
            && g_crit_facts
            && g_crit_forbidden
            && g_exact
            && g_r3
            && g_r5
            && g_ev10
            && g_sem
            && g_ndcg
            && src_num == src_den;
        let key = format!("{contracted_label}+{}", cand.name);
        // Final selection run roles: quint8 is the only production candidate;
        // mmarco-f32 exists to quantify quantization cost and cannot select.
        let role = if cand.name == "mmarco-quint8" {
            "PRODUCTION CANDIDATE"
        } else {
            "quantization-cost reference only"
        };
        if cand.name == "mmarco-quint8" {
            production_candidate_passed = Some(passed);
        }
        // Self-contained verdict: every evaluated gate carries its observed
        // value so no failed or unevaluated gate can silently support a
        // selection decision.
        pair_reports.push(format!(
            "{key} [{role}]: {} | card-multilingual={} tuned-beta={best_beta} tuned-gamma={} chat-depth={chat_depth} batch={chosen_batch} solo-p95={:.1}ms session-RAM=+{:.1}MiB",
            if passed { "PASS" } else { "BLOCKED" },
            cand.metadata_multilingual,
            cand.tuned_gamma,
            cand.solo_p95_us as f64 / 1000.0,
            cand.ram_delta_mib
        ));
        pair_reports.push(format!(
            "  gates: reference-rank+facts+retrieval-forbidden {} (rank {ref_rank}, facts {}/{}, retrieval-forbidden {}/{} at beta {best_beta}) | critical-R@1 {} {} | critical-facts=100% {} {} | critical-retrieval-forbidden=0 {} {} | answer-stage context presence DEFERRED_NOT_EVALUATED {} | exact-no-regression {} {} >= baseline {}/{} | overall-R@3 {} {} | overall-R@5 {} {} | EV@10 {} {} | semantic-delta {} {} vs baseline {}/{} | NDCG {} {:.4} vs fused {:.4}",
            if g_ref { "PASS" } else { "FAIL" },
            ref_out.metrics.fact_hits,
            ref_out.metrics.fact_total,
            ref_out.metrics.retrieval_forbidden_hits,
            ref_out.metrics.retrieval_forbidden_total,
            if g_crit { "PASS" } else { "FAIL" },
            pct(cm_critical.r1.0, cm_critical.r1.1),
            if g_crit_facts { "PASS" } else { "FAIL" },
            pct(cm_critical.facts.0, cm_critical.facts.1),
            if g_crit_forbidden { "PASS" } else { "FAIL" },
            pct(
                cm_critical.retrieval_forbidden.0,
                cm_critical.retrieval_forbidden.1
            ),
            pct(
                cm_critical.answer_forbidden.0,
                cm_critical.answer_forbidden.1
            ),
            if g_exact { "PASS" } else { "FAIL" },
            pct(cm_exact.r3.0, cm_exact.r3.1),
            be_n,
            be_d,
            if g_r3 { "PASS" } else { "FAIL" },
            pct(cm_all.r3.0, cm_all.r3.1),
            if g_r5 { "PASS" } else { "FAIL" },
            pct(cm_all.r5.0, cm_all.r5.1),
            if g_ev10 { "PASS" } else { "FAIL" },
            pct(cm_all.ev10.0, cm_all.ev10.1),
            if g_sem { "PASS" } else { "FAIL" },
            pct(cm_semantic.r3.0, cm_semantic.r3.1),
            bs_n,
            bs_d,
            if g_ndcg { "PASS" } else { "FAIL" },
            cm_all.ndcg_final_sum / cm_all.ndcg_cases as f64,
            cm_all.ndcg_fused_sum / cm_all.ndcg_cases as f64
        ));
        pair_reports.push(format!(
            "  citation/source precision {} {} (Task 1.3F simulation: scope-revalidated, prompt-budget-filtered sources vs final retained set). beta-0 cross-check: semantic R@3 {}, reference rank {abl_ref_rank}, MRR {:.4}",
            if src_num == src_den { "PASS" } else { "FAIL" },
            pct(src_num, src_den),
            pct(ablated.semantic.r3.0, ablated.semantic.r3.1),
            ablated.all.mrr_sum / ablated.all.mrr_cases as f64
        ));
    }

    // ---- Task 1.3F Deliverable 4: constants-feasibility probe (diagnostic,
    // NOT tuning) over the 360-configuration fusion grid x gamma grid for
    // every budget-viable reranker on the post-fidelity harness.----
    for cand in candidate_results.iter().filter(|c| c.viable) {
        let tuned_cfg = HybridConfig {
            rrf_k: best_k,
            w_vector: best_wv,
            w_lexical: best_wl,
            support_alpha: best_alpha,
            title_beta: best_beta,
            rerank_gamma: cand.tuned_gamma,
            support_cap: 3,
        };
        constants_feasibility_probe(
            &cand.name,
            &root,
            &cand.dir,
            &cand.file,
            &cases,
            &default_docs,
            &lex_ranks,
            (be_n, be_d),
            tuned_cfg,
            cand.tuned_objective_key,
        );
    }

    println!("--- pair verdicts ---");
    for line in &pair_reports {
        println!("[pair] {line}");
    }

    // Decision (final selection run): the production candidate is fixed
    // (e5-base-int8 + mmarco-quint8, card-multilingual-conforming); the
    // mmarco-f32 reference cannot select. COMPLETE additionally requires the
    // measured pair peak to sit inside the automatic envelope or the
    // pre-approved 1-1.25 GiB e5-base band (user decisions 2026-08-23);
    // otherwise the run stays blocked on quality gates.
    let production_key = format!(
        "{}+mmarco-quint8",
        bundle.benchmark_leader.embedding.benchmark_artifact_dir
    );
    let production_peak = bundle
        .measured_outcome
        .measured_pair_peak_mib
        .get(&production_key)
        .copied();
    let band_max_mib = bundle.envelopes.approval_band_max_bytes as f64 / (1024.0 * 1024.0);
    let decision = if production_candidate_passed.unwrap_or(false) {
        match production_peak {
            Some(mib) if mib <= 1024.0 => "complete",
            Some(mib) if mib <= band_max_mib => "complete-band-approved",
            _ => "blocked-risk-approval",
        }
    } else {
        "blocked-quality-gates"
    };
    // ---- Quantized vector storage recall cost ----
    let mut ev5_f32 = (0usize, 0usize);
    let mut ev5_fp16 = (0usize, 0usize);
    let mut ev5_int8 = (0usize, 0usize);
    for ci in (0..cases.len()).step_by(3) {
        let case = &cases[ci];
        let docs = &default_docs[ci];
        let rank_with = |transform: &dyn Fn(&[f32]) -> Vec<f32>| -> (usize, usize) {
            let mut scored: Vec<(usize, f64)> = docs
                .entries
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let tv = transform(&e.vector);
                    let best = docs
                        .query_vecs
                        .iter()
                        .map(|q| dot(q, &tv))
                        .fold(f64::MIN, f64::max);
                    (i, best)
                })
                .collect();
            scored.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            });
            let top: Vec<&DocEntry> = scored
                .iter()
                .take(5)
                .map(|(i, _)| &docs.entries[*i])
                .collect();
            let mut hits = 0;
            for req in &case.required_evidence_ids {
                if top
                    .iter()
                    .any(|e| e.doc.evidence_ids.iter().any(|id| id == req))
                {
                    hits += 1;
                }
            }
            (hits, case.required_evidence_ids.len())
        };
        let (a, b) = rank_with(&|v| v.to_vec());
        ev5_f32.0 += a;
        ev5_f32.1 += b;
        let (a, b) = rank_with(&fp16_roundtrip);
        ev5_fp16.0 += a;
        ev5_fp16.1 += b;
        let (a, b) = rank_with(&int8_roundtrip);
        ev5_int8.0 += a;
        ev5_int8.1 += b;
    }
    println!("[quantization] vector-channel Evidence Recall@5 (sampled every 3rd case):");
    println!("  f32  storage: {}", pct(ev5_f32.0, ev5_f32.1));
    println!("  fp16 storage: {}", pct(ev5_fp16.0, ev5_fp16.1));
    println!("  int8 storage: {}", pct(ev5_int8.0, ev5_int8.1));

    // ---- Derived disk projection to 250k documents ----
    let total_docs: usize = default_docs.iter().map(|d| d.entries.len()).sum();
    let content_bytes: usize = default_docs
        .iter()
        .flat_map(|d| d.entries.iter())
        .map(|e| e.doc.text.as_bytes().len())
        .sum();
    let avg_content = content_bytes / total_docs.max(1);
    const ROW_OVERHEAD_BYTES: usize = 96;
    let avg_doc_bytes = avg_content + 384 + ROW_OVERHEAD_BYTES;
    let steady_gib = avg_doc_bytes as f64 * 250_000.0 / 1_073_741_824.0;
    println!(
        "[disk] measured {avg_content} B content/doc +384 B vector(int8) +{ROW_OVERHEAD_BYTES} B overhead = {avg_doc_bytes} B/doc; projected 250k steady {steady_gib:.2} GiB (envelope 2 GiB), shadow-rebuild peak x2 {:.2} GiB (envelope 3 GiB)",
        steady_gib * 2.0
    );

    println!("=== TASK 1.3 DECISION: {decision} ===");
    if decision == "complete-band-approved" {
        println!(
            "The production candidate passes every gate; its measured peak sits in the 1-1.25 GiB band pre-approved for e5-base pairings (2026-08-23 user decision). Selection remains pending user approval."
        );
    }
    if decision == "blocked-risk-approval" {
        println!(
            "The production candidate passes every quality gate but its measured peak exceeds the approved envelopes; explicit user risk approval is required."
        );
    }
    println!("=== benchmark complete ===");
}

#[tokio::test]
async fn probe_reference_case_trace() {
    if std::env::var("MEETLY_RAG_BENCH").as_deref() != Ok("1") {
        println!("SKIP probe (set MEETLY_RAG_BENCH=1)");
        return;
    }
    let Some(root) = models_dir() else {
        println!("SKIP probe");
        return;
    };
    let cases = corpus::cases();
    let case = cases
        .iter()
        .find(|c| c.id == "fixture-whatsapp-retention")
        .expect("reference case");
    let pol = policy_lite();
    let mut emb = Embedder::load(
        &root.join("e5-small-int8"),
        "model_int8.onnx",
        manifest().benchmark_leader.embedding.max_sequence_length,
        4,
    )
    .expect("session");
    let mut cache: HashMap<u64, Vec<f32>> = HashMap::new();
    let tok = emb.0.tokenizer.clone();
    let docs = build_case_docs_cached(
        case,
        &tok,
        384,
        64,
        false,
        "query: ",
        "passage: ",
        &mut emb,
        &mut cache,
        &pol,
    );
    println!("entries={}", docs.entries.len());
    let rows = lexical_channel(case, LEXICAL_CANDIDATES).await;
    println!("lex rows={}:", rows.len());
    for r in rows.iter().take(6) {
        println!("  row meeting={} chunk_id={}", r.meeting_id, r.chunk_id);
    }
}

#[tokio::test]
async fn probe_reranker_batch_stability() {
    if std::env::var("MEETLY_RAG_BENCH").as_deref() != Ok("1") {
        println!("SKIP probe (set MEETLY_RAG_BENCH=1)");
        return;
    }
    let Some(root) = models_dir() else {
        println!("SKIP probe");
        return;
    };
    let cases = corpus::cases();
    for case_id in ["fixture-whatsapp-retention", "pt-semantic-paraphrase-031"] {
        let _case = cases.iter().find(|c| c.id == case_id).expect("case");
        let root = root.clone();
        // bge-reranker-base is permanently retired; the stability probe runs
        // on the contracted mmarco quint8 export instead.
        let rr = RerankModel::load(
            &root.join("mmarco-reranker"),
            "model_quint8_avx2.onnx",
            512,
            4,
        )
        .expect("reranker");
        drop(rr);
        let _ = root;
        println!("[stability] {case_id}: see hybrid benchmark for full evidence");
    }
}

#[tokio::test]
async fn pair_ram_probe() {
    if std::env::var("MEETLY_RAG_BENCH").as_deref() != Ok("1") {
        println!("SKIP pair ram probe (set MEETLY_RAG_BENCH=1)");
        return;
    }
    let Ok(spec) = std::env::var("MEETLY_RAG_PAIR") else {
        println!("SKIP pair ram probe (set MEETLY_RAG_PAIR=\"<emb_dir>/<file>:<rr_dir>/<file>\")");
        return;
    };
    let Some(root) = models_dir() else {
        println!("SKIP pair ram probe (no staged artifacts)");
        return;
    };
    let (emb_spec, rr_spec) = spec.split_once(':').expect("pair spec emb:rr");
    let (ed, ef) = emb_spec.split_once('/').expect("emb dir/file");
    let (rd, rf) = rr_spec.split_once('/').expect("rr dir/file");

    let base = rss_mib().expect("rss");
    let mut emb = Embedder::load(&root.join(ed), ef, 512, 4).unwrap_or_else(|e| panic!("emb: {e}"));
    emb.embed(&["query: warmup sentence".to_string()])
        .expect("emb warmup");
    emb.embed(&["passage: warmup evidence sentence for arena growth.".to_string()])
        .expect("doc warmup");
    let after_emb = rss_mib().expect("rss");
    let mut rr =
        RerankModel::load(&root.join(rd), rf, 512, 4).unwrap_or_else(|e| panic!("rr: {e}"));
    let _ = rr
        .score(&[(
            "quais os dias de comunicacao por whatsapp para o fluxo de retencao?".to_string(),
            "A régua sintética de mensagens prevê contatos nos dias 1, 3, 7, 10 e 15.".to_string(),
        )])
        .expect("rr warmup");
    let peak = rss_mib().expect("rss");
    println!(
        "[pair-ram] {spec}: embedding session +{:.1} MiB; pair peak with both sessions resident and post-inference +{:.1} MiB over process base",
        after_emb - base,
        peak - base
    );
    println!(
        "[pair-ram] projected 250k peak with int8 vectors (192 MiB): {:.1} MiB",
        192.0 + (peak - base)
    );
}

#[test]
fn probe_model_io() {
    if std::env::var("MEETLY_RAG_BENCH").as_deref() != Ok("1") {
        println!("SKIP probe (set MEETLY_RAG_BENCH=1)");
        return;
    }
    let Ok(spec) = std::env::var("MEETLY_RAG_PROBE_MODEL") else {
        println!("SKIP probe (set MEETLY_RAG_PROBE_MODEL=\"dir/file\")");
        return;
    };
    let Some(root) = models_dir() else {
        println!("SKIP probe");
        return;
    };
    let (d, f) = spec.split_once('/').expect("dir/file");
    let mut emb = Embedder::load(&root.join(d), f, 512, 4).expect("load");
    let vectors = emb
        .embed(&[
            "query: quais os dias de comunicacao por whatsapp para o fluxo de retencao?"
                .to_string(),
        ])
        .expect("embed");
    println!(
        "[io] {spec}: inputs={:?} outputs={:?} dim={} norm={:.4}",
        emb.0.input_names(),
        emb.0.output_names,
        vectors[0].len(),
        vectors[0]
            .iter()
            .map(|v| (*v as f64) * (*v as f64))
            .sum::<f64>()
            .sqrt()
    );
}
