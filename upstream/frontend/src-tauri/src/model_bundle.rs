//! Approved retrieval model bundle contract (Tasks 1.5 and 1.R2).
//!
//! Parses and validates the checked-in production manifest against the exact
//! approved Sprint 1 bundle (model identities, revisions, exports,
//! preprocessing, tokenizer contracts, tensor I/O, licenses/provenance, and
//! managed artifact set) and lazily verifies artifact byte lengths and SHA-256
//! digests before first model load. No model download happens here and nothing
//! calls this during application startup.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const SUPPORTED_MANIFEST_VERSION: u32 = 1;

const APPROVED_PAIR_FORMAT: &str = "question,evidence";
const REQUIRED_INPUTS: &[(&str, &str)] = &[("input_ids", "int64"), ("attention_mask", "int64")];
const EMBEDDING_OUTPUTS: &[(&str, &str)] = &[("last_hidden_state", "float32")];
const RERANKER_OUTPUTS: &[(&str, &str)] = &[("logits", "float32")];
const READ_BUFFER_BYTES: usize = 1024 * 1024;

// The exact approved production contract ("Approved Sprint 1 Bundle And
// Runtime Contract" in docs/hybrid-rag/architecture.md). Substituting any of
// these values fails parse_manifest itself, not just a test assertion.
const APPROVED_BUNDLE_ID: &str = "meetily-retrieval-bundle-1";
const APPROVED_CHUNKER_VERSION: u32 = 1;
const APPROVED_EMBEDDING_MODEL_ID: &str = "intfloat/multilingual-e5-base";
const APPROVED_EMBEDDING_REVISION: &str = "d128750597153bb5987e10b1c3493a34e5a4502a";
const APPROVED_EMBEDDING_EXPORT_REPO: &str = "Xenova/multilingual-e5-base";
const APPROVED_EMBEDDING_EXPORT_REVISION: &str = "1ec9243030a27d1a115d5c340572074c125b58b2";
const APPROVED_EMBEDDING_QUANTIZATION: &str = "dynamic-int8";
const APPROVED_EMBEDDING_DIMENSIONS: u32 = 768;
const APPROVED_MAX_SEQUENCE_LENGTH: u32 = 512;
const APPROVED_QUERY_PREFIX: &str = "query: ";
const APPROVED_DOCUMENT_PREFIX: &str = "passage: ";
const APPROVED_POOLING: &str = "masked-mean over attention_mask positions of last_hidden_state";
const APPROVED_NORMALIZATION: &str = "l2";
const APPROVED_TOKENIZER_TYPE: &str = "XLM-RoBERTa unigram (tokenizer.json)";
const APPROVED_TRUNCATION_SIDE: &str = "right";
const APPROVED_RERANKER_MODEL_ID: &str = "cross-encoder/mmarco-mMiniLMv2-L12-H384-v1";
const APPROVED_RERANKER_REVISION: &str = "1427fd652930e4ba29e8149678df786c240d8825";
const APPROVED_RERANKER_QUANTIZATION: &str = "quint8_avx2";
const APPROVED_OUTPUT_LABEL_INDEX: usize = 0;
const APPROVED_SCORE_TRANSFORM: &str = "identity";
const APPROVED_EMBEDDING_MODEL_ARTIFACT: &str = "models/embedding/model_int8.onnx";
const APPROVED_RERANKER_MODEL_ARTIFACT: &str = "models/reranker/model_quint8_avx2.onnx";
const APPROVED_EMBEDDING_TOKENIZER_PATHS: &[&str] = &[
    "tokenizers/embedding/tokenizer.json",
    "tokenizers/embedding/tokenizer_config.json",
    "tokenizers/embedding/special_tokens_map.json",
];
const APPROVED_RERANKER_TOKENIZER_PATHS: &[&str] = &[
    "tokenizers/reranker/tokenizer.json",
    "tokenizers/reranker/tokenizer_config.json",
    "tokenizers/reranker/special_tokens_map.json",
];
const APPROVED_MIT_SPDX: &str = "MIT";
const APPROVED_MIT_NOTICE_PATH: &str = "licenses/e5-base-MIT-NOTICE.txt";
const APPROVED_MIT_ATTRIBUTION: &str = "intfloat/multilingual-e5-base (MIT per pinned model-card metadata at d128750597153bb5987e10b1c3493a34e5a4502a), packaged via the unlicensed mechanical ONNX conversion Xenova/multilingual-e5-base @ 1ec9243030a27d1a115d5c340572074c125b58b2, which declares no separate license and names this model as its sole base. Applicable copyright notice Copyright (c) Microsoft Corporation taken from microsoft/unilm LICENSE @ 0e31c7c09737df491e7ff74ded19614b884c52b4, the E5 development repository linked by the model card; neither upstream repo ships a LICENSE file. See packaged licenses/e5-base-MIT-NOTICE.txt for the full pinned evidence chain and permission text.";
const APPROVED_MIT_RESOURCE: &str =
    "https://huggingface.co/intfloat/multilingual-e5-base/tree/d128750597153bb5987e10b1c3493a34e5a4502a";
const APPROVED_MIT_SOURCE_URL: &str =
    "https://raw.githubusercontent.com/microsoft/unilm/0e31c7c09737df491e7ff74ded19614b884c52b4/LICENSE";
const APPROVED_APACHE_SPDX: &str = "Apache-2.0";
const APPROVED_APACHE_NOTICE_PATH: &str = "licenses/mmarco-mMiniLMv2-Apache-2.0.txt";
const APPROVED_APACHE_ATTRIBUTION: &str = "cross-encoder/mmarco-mMiniLMv2-L12-H384-v1 (Apache-2.0 declared both as the repository license tag and in the pinned model-card front matter at 1427fd652930e4ba29e8149678df786c240d8825; upstream ships neither a LICENSE nor a NOTICE file, so no additional notice is required beyond this license copy). The packaged onnx/model_quint8_avx2.onnx lives in that same pinned revision. Base model nreimers/mMiniLMv2-L12-H384-distilled-from-XLMR-Large carries no upstream license declaration and is not packaged.";
const APPROVED_APACHE_RESOURCE: &str =
    "https://huggingface.co/cross-encoder/mmarco-mMiniLMv2-L12-H384-v1/tree/1427fd652930e4ba29e8149678df786c240d8825";
const APPROVED_APACHE_SOURCE_URL: &str = "https://www.apache.org/licenses/LICENSE-2.0.txt";

/// The manifest-pinned contract fields that discriminate the persisted
/// retrieval model identity (the identity precondition in `architecture.md`
/// "Prior-Model Retention Across Upgrade"). Mirrors the approved constants the
/// parser enforces, so persisting this view keeps a single source of truth.
pub struct ApprovedEmbeddingContract {
    pub bundle_id: &'static str,
    pub embedding_model_id: &'static str,
    pub embedding_revision: &'static str,
    pub onnx_export_revision: &'static str,
    pub onnx_export_quantization: &'static str,
    pub dimensions: u32,
}

/// Returns the pinned approved-contract identity inputs.
pub const fn approved_embedding_contract() -> ApprovedEmbeddingContract {
    ApprovedEmbeddingContract {
        bundle_id: APPROVED_BUNDLE_ID,
        embedding_model_id: APPROVED_EMBEDDING_MODEL_ID,
        embedding_revision: APPROVED_EMBEDDING_REVISION,
        onnx_export_revision: APPROVED_EMBEDDING_EXPORT_REVISION,
        onnx_export_quantization: APPROVED_EMBEDDING_QUANTIZATION,
        dimensions: APPROVED_EMBEDDING_DIMENSIONS,
    }
}

#[derive(Debug, Error)]
pub enum ModelBundleError {
    #[error(
        "unsupported manifest version {found}: this build supports v{SUPPORTED_MANIFEST_VERSION} only"
    )]
    UnsupportedManifestVersion { found: u32 },
    #[error("invalid bundle manifest: {0}")]
    InvalidManifest(String),
    #[error("artifact '{path}' failed verification: {reason}")]
    ArtifactVerification { path: String, reason: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelBundleManifest {
    pub manifest_version: u32,
    pub bundle_id: String,
    pub embedding_model: EmbeddingModelContract,
    pub reranker_model: RerankerModelContract,
    pub chunker_version: u32,
    pub licenses: Vec<LicenseEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingModelContract {
    pub model_id: String,
    pub revision: String,
    pub dimensions: u32,
    pub max_sequence_length: u32,
    pub onnx_export: OnnxExport,
    pub tokenizer: TokenizerContract,
    pub query_prefix: String,
    pub document_prefix: String,
    pub pooling: String,
    pub normalization: String,
    pub inputs: Vec<TensorSpec>,
    pub outputs: Vec<TensorSpec>,
    pub artifacts: Vec<ArtifactEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RerankerModelContract {
    pub model_id: String,
    pub revision: String,
    pub max_sequence_length: u32,
    pub onnx_export: OnnxExport,
    pub tokenizer: TokenizerContract,
    pub pair_format: String,
    pub inputs: Vec<TensorSpec>,
    pub outputs: Vec<TensorSpec>,
    pub output_label_index: usize,
    pub score_transform: String,
    pub artifacts: Vec<ArtifactEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnnxExport {
    pub repo: String,
    pub revision: String,
    pub quantization: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenizerContract {
    #[serde(rename = "type")]
    pub kind: String,
    pub revision: String,
    pub truncation_side: String,
    pub artifacts: Vec<ArtifactEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TensorSpec {
    pub name: String,
    pub dtype: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LicenseEntry {
    pub spdx: String,
    pub applies_to: String,
    pub attribution: String,
    pub resource: String,
    pub path: String,
    pub byte_length: u64,
    pub sha256: String,
    pub source: ArtifactSource,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactEntry {
    pub path: String,
    pub byte_length: u64,
    pub sha256: String,
    pub source: ArtifactSource,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSource {
    pub url: String,
}

pub fn parse_manifest(json: &str) -> Result<ModelBundleManifest, ModelBundleError> {
    let manifest: ModelBundleManifest = serde_json::from_str(json)
        .map_err(|e| ModelBundleError::InvalidManifest(format!("JSON parse error: {e}")))?;
    manifest.validate()?;
    Ok(manifest)
}

impl ModelBundleManifest {
    fn validate(&self) -> Result<(), ModelBundleError> {
        if self.manifest_version != SUPPORTED_MANIFEST_VERSION {
            return Err(ModelBundleError::UnsupportedManifestVersion {
                found: self.manifest_version,
            });
        }
        ensure_non_empty("bundleId", &self.bundle_id)?;
        if self.chunker_version < 1 {
            return Err(ModelBundleError::InvalidManifest(
                "chunkerVersion must be >= 1".to_string(),
            ));
        }

        let e = &self.embedding_model;
        ensure_non_empty("embeddingModel.modelId", &e.model_id)?;
        ensure_non_empty("embeddingModel.revision", &e.revision)?;
        if e.dimensions == 0 {
            return Err(ModelBundleError::InvalidManifest(
                "embeddingModel.dimensions must be > 0".to_string(),
            ));
        }
        validate_common_model_fields("embeddingModel", e.max_sequence_length, &e.onnx_export)?;
        validate_tokenizer("embeddingModel.tokenizer", &e.tokenizer)?;
        ensure_non_empty("embeddingModel.queryPrefix", &e.query_prefix)?;
        ensure_non_empty("embeddingModel.documentPrefix", &e.document_prefix)?;
        ensure_non_empty("embeddingModel.pooling", &e.pooling)?;
        validate_tensors("embeddingModel.inputs", &e.inputs, REQUIRED_INPUTS)?;
        validate_tensors("embeddingModel.outputs", &e.outputs, EMBEDDING_OUTPUTS)?;
        validate_artifacts("embeddingModel.artifacts", &e.artifacts)?;

        let r = &self.reranker_model;
        ensure_non_empty("rerankerModel.modelId", &r.model_id)?;
        ensure_non_empty("rerankerModel.revision", &r.revision)?;
        validate_common_model_fields("rerankerModel", r.max_sequence_length, &r.onnx_export)?;
        validate_tokenizer("rerankerModel.tokenizer", &r.tokenizer)?;
        if r.pair_format != APPROVED_PAIR_FORMAT {
            return Err(ModelBundleError::InvalidManifest(format!(
                "rerankerModel.pairFormat {:?} is not the approved format {:?}",
                r.pair_format, APPROVED_PAIR_FORMAT
            )));
        }
        validate_tensors("rerankerModel.inputs", &r.inputs, REQUIRED_INPUTS)?;
        validate_tensors("rerankerModel.outputs", &r.outputs, RERANKER_OUTPUTS)?;
        if r.output_label_index >= r.outputs.len() {
            return Err(ModelBundleError::InvalidManifest(format!(
                "rerankerModel.outputLabelIndex {} is out of bounds for {} outputs",
                r.output_label_index,
                r.outputs.len()
            )));
        }
        if r.outputs[r.output_label_index].dtype != "float32" {
            return Err(ModelBundleError::InvalidManifest(
                "rerankerModel label output must be float32".to_string(),
            ));
        }
        ensure_non_empty("rerankerModel.scoreTransform", &r.score_transform)?;
        validate_artifacts("rerankerModel.artifacts", &r.artifacts)?;

        validate_license_entries(&self.licenses)?;

        // Distinct models must keep separate tokenizer contracts unless their
        // complete identities (type, revision, and artifact set) match.
        let et = &self.embedding_model.tokenizer;
        let rt = &self.reranker_model.tokenizer;
        if et.kind == rt.kind && et.revision == rt.revision && !tokenizer_artifacts_equal(et, rt) {
            return Err(ModelBundleError::InvalidManifest(
                "embedding and reranker declare the same tokenizer identity with different artifacts"
                    .to_string(),
            ));
        }

        // Tokenizer artifacts are pinned to the same commit as their ONNX
        // export/model; an unrelated revision is not the approved contract.
        if et.revision != e.onnx_export.revision {
            return Err(ModelBundleError::InvalidManifest(format!(
                "embeddingModel.tokenizer revision '{}' does not match its pinned ONNX export revision '{}'",
                et.revision, e.onnx_export.revision
            )));
        }
        if rt.revision != r.onnx_export.revision {
            return Err(ModelBundleError::InvalidManifest(format!(
                "rerankerModel.tokenizer revision '{}' does not match its pinned model revision '{}'",
                rt.revision, r.onnx_export.revision
            )));
        }

        let mut paths = HashSet::new();
        for entry in self.artifact_entries() {
            ensure_path_unique(&mut paths, entry)?;
        }
        for license in &self.licenses {
            let view = license.artifact_view();
            ensure_path_unique(&mut paths, &view)?;
        }

        // Last gate: every field must equal the exact approved production
        // contract, so a schema-valid substitution fails parsing itself.
        self.validate_approved_contract()?;
        Ok(())
    }

    fn validate_approved_contract(&self) -> Result<(), ModelBundleError> {
        expect_approved_str("bundleId", &self.bundle_id, APPROVED_BUNDLE_ID)?;
        expect_approved_u32(
            "chunkerVersion",
            self.chunker_version,
            APPROVED_CHUNKER_VERSION,
        )?;

        let e = &self.embedding_model;
        expect_approved_str(
            "embeddingModel.modelId",
            &e.model_id,
            APPROVED_EMBEDDING_MODEL_ID,
        )?;
        expect_approved_str(
            "embeddingModel.revision",
            &e.revision,
            APPROVED_EMBEDDING_REVISION,
        )?;
        expect_approved_u32(
            "embeddingModel.dimensions",
            e.dimensions,
            APPROVED_EMBEDDING_DIMENSIONS,
        )?;
        expect_approved_u32(
            "embeddingModel.maxSequenceLength",
            e.max_sequence_length,
            APPROVED_MAX_SEQUENCE_LENGTH,
        )?;
        expect_approved_str(
            "embeddingModel.onnxExport.repo",
            &e.onnx_export.repo,
            APPROVED_EMBEDDING_EXPORT_REPO,
        )?;
        expect_approved_str(
            "embeddingModel.onnxExport.revision",
            &e.onnx_export.revision,
            APPROVED_EMBEDDING_EXPORT_REVISION,
        )?;
        expect_approved_str(
            "embeddingModel.onnxExport.quantization",
            &e.onnx_export.quantization,
            APPROVED_EMBEDDING_QUANTIZATION,
        )?;
        expect_approved_str(
            "embeddingModel.queryPrefix",
            &e.query_prefix,
            APPROVED_QUERY_PREFIX,
        )?;
        expect_approved_str(
            "embeddingModel.documentPrefix",
            &e.document_prefix,
            APPROVED_DOCUMENT_PREFIX,
        )?;
        expect_approved_str("embeddingModel.pooling", &e.pooling, APPROVED_POOLING)?;
        expect_approved_str(
            "embeddingModel.normalization",
            &e.normalization,
            APPROVED_NORMALIZATION,
        )?;
        expect_approved_str(
            "embeddingModel.tokenizer.type",
            &e.tokenizer.kind,
            APPROVED_TOKENIZER_TYPE,
        )?;
        expect_approved_str(
            "embeddingModel.tokenizer.truncationSide",
            &e.tokenizer.truncation_side,
            APPROVED_TRUNCATION_SIDE,
        )?;
        ensure_exact_paths(
            "embeddingModel.artifacts",
            &e.artifacts,
            &[APPROVED_EMBEDDING_MODEL_ARTIFACT],
        )?;
        ensure_exact_paths(
            "embeddingModel.tokenizer.artifacts",
            &e.tokenizer.artifacts,
            APPROVED_EMBEDDING_TOKENIZER_PATHS,
        )?;
        expect_pinned_hf_urls(
            &e.artifacts,
            APPROVED_EMBEDDING_EXPORT_REPO,
            APPROVED_EMBEDDING_EXPORT_REVISION,
        )?;
        expect_pinned_hf_urls(
            &e.tokenizer.artifacts,
            APPROVED_EMBEDDING_EXPORT_REPO,
            APPROVED_EMBEDDING_EXPORT_REVISION,
        )?;

        let r = &self.reranker_model;
        expect_approved_str(
            "rerankerModel.modelId",
            &r.model_id,
            APPROVED_RERANKER_MODEL_ID,
        )?;
        expect_approved_str(
            "rerankerModel.revision",
            &r.revision,
            APPROVED_RERANKER_REVISION,
        )?;
        expect_approved_u32(
            "rerankerModel.maxSequenceLength",
            r.max_sequence_length,
            APPROVED_MAX_SEQUENCE_LENGTH,
        )?;
        expect_approved_str(
            "rerankerModel.onnxExport.repo",
            &r.onnx_export.repo,
            APPROVED_RERANKER_MODEL_ID,
        )?;
        expect_approved_str(
            "rerankerModel.onnxExport.revision",
            &r.onnx_export.revision,
            APPROVED_RERANKER_REVISION,
        )?;
        expect_approved_str(
            "rerankerModel.onnxExport.quantization",
            &r.onnx_export.quantization,
            APPROVED_RERANKER_QUANTIZATION,
        )?;
        expect_approved_str(
            "rerankerModel.tokenizer.type",
            &r.tokenizer.kind,
            APPROVED_TOKENIZER_TYPE,
        )?;
        expect_approved_str(
            "rerankerModel.tokenizer.truncationSide",
            &r.tokenizer.truncation_side,
            APPROVED_TRUNCATION_SIDE,
        )?;
        expect_approved_usize(
            "rerankerModel.outputLabelIndex",
            r.output_label_index,
            APPROVED_OUTPUT_LABEL_INDEX,
        )?;
        expect_approved_str(
            "rerankerModel.scoreTransform",
            &r.score_transform,
            APPROVED_SCORE_TRANSFORM,
        )?;
        ensure_exact_paths(
            "rerankerModel.artifacts",
            &r.artifacts,
            &[APPROVED_RERANKER_MODEL_ARTIFACT],
        )?;
        ensure_exact_paths(
            "rerankerModel.tokenizer.artifacts",
            &r.tokenizer.artifacts,
            APPROVED_RERANKER_TOKENIZER_PATHS,
        )?;
        expect_pinned_hf_urls(
            &r.artifacts,
            APPROVED_RERANKER_MODEL_ID,
            APPROVED_RERANKER_REVISION,
        )?;
        expect_pinned_hf_urls(
            &r.tokenizer.artifacts,
            APPROVED_RERANKER_MODEL_ID,
            APPROVED_RERANKER_REVISION,
        )?;

        let mit = self
            .licenses
            .iter()
            .find(|license| license.applies_to == "embeddingModel")
            .expect("coverage rule guarantees exactly one entry per component");
        expect_approved_str("license(embeddingModel).spdx", &mit.spdx, APPROVED_MIT_SPDX)?;
        expect_approved_str(
            "license(embeddingModel).path",
            &mit.path,
            APPROVED_MIT_NOTICE_PATH,
        )?;
        expect_approved_str(
            "license(embeddingModel).attribution",
            &mit.attribution,
            APPROVED_MIT_ATTRIBUTION,
        )?;
        expect_approved_str(
            "license(embeddingModel).resource",
            &mit.resource,
            APPROVED_MIT_RESOURCE,
        )?;
        expect_approved_str(
            "license(embeddingModel).source.url",
            &mit.source.url,
            APPROVED_MIT_SOURCE_URL,
        )?;

        let apache = self
            .licenses
            .iter()
            .find(|license| license.applies_to == "rerankerModel")
            .expect("coverage rule guarantees exactly one entry per component");
        expect_approved_str(
            "license(rerankerModel).spdx",
            &apache.spdx,
            APPROVED_APACHE_SPDX,
        )?;
        expect_approved_str(
            "license(rerankerModel).path",
            &apache.path,
            APPROVED_APACHE_NOTICE_PATH,
        )?;
        expect_approved_str(
            "license(rerankerModel).attribution",
            &apache.attribution,
            APPROVED_APACHE_ATTRIBUTION,
        )?;
        expect_approved_str(
            "license(rerankerModel).resource",
            &apache.resource,
            APPROVED_APACHE_RESOURCE,
        )?;
        expect_approved_str(
            "license(rerankerModel).source.url",
            &apache.source.url,
            APPROVED_APACHE_SOURCE_URL,
        )?;

        Ok(())
    }

    /// Every declared model/tokenizer artifact (licenses are verified
    /// separately from their own fields).
    pub fn artifact_entries(&self) -> impl Iterator<Item = &ArtifactEntry> {
        self.embedding_model
            .artifacts
            .iter()
            .chain(self.embedding_model.tokenizer.artifacts.iter())
            .chain(self.reranker_model.artifacts.iter())
            .chain(self.reranker_model.tokenizer.artifacts.iter())
    }

    /// Lazily re-checks every artifact under `bundle_root` (existence, byte
    /// length, SHA-256). Intended before the first model load in a process so
    /// post-install corruption cannot reach ONNX Runtime.
    pub fn verify_artifacts(&self, bundle_root: &Path) -> Result<(), ModelBundleError> {
        for entry in self.artifact_entries() {
            verify_file(
                &bundle_root.join(&entry.path),
                entry.byte_length,
                &entry.sha256,
                &entry.path,
            )?;
        }
        for license in &self.licenses {
            verify_file(
                &bundle_root.join(&license.path),
                license.byte_length,
                &license.sha256,
                &license.path,
            )?;
        }
        Ok(())
    }
}

impl LicenseEntry {
    fn artifact_view(&self) -> ArtifactEntry {
        ArtifactEntry {
            path: self.path.clone(),
            byte_length: self.byte_length,
            sha256: self.sha256.clone(),
            source: self.source.clone(),
        }
    }
}

fn validate_common_model_fields(
    section: &str,
    max_sequence_length: u32,
    export: &OnnxExport,
) -> Result<(), ModelBundleError> {
    if max_sequence_length == 0 {
        return Err(ModelBundleError::InvalidManifest(format!(
            "{section}.maxSequenceLength must be > 0"
        )));
    }
    ensure_non_empty(&format!("{section}.onnxExport.repo"), &export.repo)?;
    ensure_non_empty(&format!("{section}.onnxExport.revision"), &export.revision)?;
    ensure_non_empty(
        &format!("{section}.onnxExport.quantization"),
        &export.quantization,
    )
}

fn validate_tokenizer(
    section: &str,
    tokenizer: &TokenizerContract,
) -> Result<(), ModelBundleError> {
    ensure_non_empty(&format!("{section}.type"), &tokenizer.kind)?;
    ensure_non_empty(&format!("{section}.revision"), &tokenizer.revision)?;
    ensure_non_empty(
        &format!("{section}.truncationSide"),
        &tokenizer.truncation_side,
    )?;
    validate_artifacts(&format!("{section}.artifacts"), &tokenizer.artifacts)
}
fn validate_tensors(
    section: &str,
    tensors: &[TensorSpec],
    required: &[(&str, &str)],
) -> Result<(), ModelBundleError> {
    if tensors.is_empty() {
        return Err(ModelBundleError::InvalidManifest(format!(
            "{section} must not be empty"
        )));
    }
    let mut seen = HashSet::new();
    for tensor in tensors {
        ensure_non_empty(&format!("{section}.name"), &tensor.name)?;
        if !seen.insert(tensor.name.as_str()) {
            return Err(ModelBundleError::InvalidManifest(format!(
                "{section} declares duplicate tensor name '{}'",
                tensor.name
            )));
        }
    }
    for (name, expected_dtype) in required {
        match tensors.iter().find(|t| t.name == *name) {
            None => {
                return Err(ModelBundleError::InvalidManifest(format!(
                    "{section} is missing required tensor '{name}'"
                )));
            }
            Some(tensor) if tensor.dtype != *expected_dtype => {
                return Err(ModelBundleError::InvalidManifest(format!(
                    "{section} tensor '{name}' must have dtype '{expected_dtype}', got '{}'",
                    tensor.dtype
                )));
            }
            Some(_) => {}
        }
    }
    if tensors.len() != required.len() {
        let expected_names = required
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ");
        let actual_names = tensors
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ModelBundleError::InvalidManifest(format!(
            "{section} must declare exactly the tensors [{expected_names}], found [{}]",
            actual_names
        )));
    }
    Ok(())
}

fn validate_artifacts(section: &str, artifacts: &[ArtifactEntry]) -> Result<(), ModelBundleError> {
    if artifacts.is_empty() {
        return Err(ModelBundleError::InvalidManifest(format!(
            "{section} must not be empty"
        )));
    }
    for artifact in artifacts {
        validate_artifact_path(&artifact.path)?;
        validate_sha256_format(&artifact.sha256)?;
        if artifact.byte_length == 0 {
            return Err(ModelBundleError::InvalidManifest(format!(
                "{} declares byteLength 0",
                artifact.path
            )));
        }
        ensure_non_empty(
            &format!("{}.source.url", artifact.path),
            &artifact.source.url,
        )?;
    }
    Ok(())
}

fn ensure_path_unique(
    paths: &mut HashSet<String>,
    entry: &ArtifactEntry,
) -> Result<(), ModelBundleError> {
    if !paths.insert(entry.path.clone()) {
        return Err(ModelBundleError::InvalidManifest(format!(
            "duplicate artifact path '{}'",
            entry.path
        )));
    }
    Ok(())
}

fn validate_license_entries(licenses: &[LicenseEntry]) -> Result<(), ModelBundleError> {
    if licenses.is_empty() {
        return Err(ModelBundleError::InvalidManifest(
            "licenses must not be empty".to_string(),
        ));
    }
    for license in licenses {
        ensure_non_empty("license.spdx", &license.spdx)?;
        if license.applies_to != "embeddingModel" && license.applies_to != "rerankerModel" {
            return Err(ModelBundleError::InvalidManifest(format!(
                "license.appliesTo {:?} must be 'embeddingModel' or 'rerankerModel'",
                license.applies_to
            )));
        }
        ensure_non_empty("license.attribution", &license.attribution)?;
        ensure_non_empty("license.resource", &license.resource)?;
        validate_artifact_path(&license.path)?;
        validate_sha256_format(&license.sha256)?;
        if license.byte_length == 0 {
            return Err(ModelBundleError::InvalidManifest(format!(
                "{} declares byteLength 0",
                license.path
            )));
        }
        ensure_non_empty("license.source.url", &license.source.url)?;
    }
    for component in ["embeddingModel", "rerankerModel"] {
        match licenses
            .iter()
            .filter(|license| license.applies_to == component)
            .count()
        {
            0 => {
                return Err(ModelBundleError::InvalidManifest(format!(
                    "licenses provide no coverage for '{component}'"
                )));
            }
            count if count > 1 => {
                return Err(ModelBundleError::InvalidManifest(format!(
                    "licenses declare duplicate coverage for '{component}' ({count} entries)"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_artifact_path(path: &str) -> Result<(), ModelBundleError> {
    if path.is_empty()
        || path.contains('\\')
        || path.starts_with('/')
        || path.contains(':')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(ModelBundleError::InvalidManifest(format!(
            "unsafe artifact path '{path}'"
        )));
    }
    Ok(())
}

fn validate_sha256_format(sha256: &str) -> Result<(), ModelBundleError> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(ModelBundleError::InvalidManifest(format!(
            "malformed SHA-256 '{sha256}' (expected 64 lowercase hex characters)"
        )));
    }
    Ok(())
}

fn ensure_non_empty(field: &str, value: &str) -> Result<(), ModelBundleError> {
    if value.trim().is_empty() {
        return Err(ModelBundleError::InvalidManifest(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn expect_approved_str(field: &str, actual: &str, approved: &str) -> Result<(), ModelBundleError> {
    if actual != approved {
        return Err(ModelBundleError::InvalidManifest(format!(
            "{field} is {actual:?}; the approved production contract requires {approved:?}"
        )));
    }
    Ok(())
}

fn expect_approved_u32(field: &str, actual: u32, approved: u32) -> Result<(), ModelBundleError> {
    if actual != approved {
        return Err(ModelBundleError::InvalidManifest(format!(
            "{field} is {actual}; the approved production contract requires {approved}"
        )));
    }
    Ok(())
}

fn expect_approved_usize(
    field: &str,
    actual: usize,
    approved: usize,
) -> Result<(), ModelBundleError> {
    if actual != approved {
        return Err(ModelBundleError::InvalidManifest(format!(
            "{field} is {actual}; the approved production contract requires {approved}"
        )));
    }
    Ok(())
}

fn ensure_exact_paths(
    section: &str,
    artifacts: &[ArtifactEntry],
    approved: &[&str],
) -> Result<(), ModelBundleError> {
    let mut actual: Vec<&str> = artifacts
        .iter()
        .map(|artifact| artifact.path.as_str())
        .collect();
    actual.sort_unstable();
    let mut expected = approved.to_vec();
    expected.sort_unstable();
    if actual != expected {
        return Err(ModelBundleError::InvalidManifest(format!(
            "{section} must manage exactly {expected:?}, found {actual:?}"
        )));
    }
    Ok(())
}

/// Binds each packaged artifact to the pinned revision-resolve URL of its
/// component's export repository, so an ONNX repo/revision cannot be swapped
/// while keeping schema-valid artifacts.
fn expect_pinned_hf_urls(
    artifacts: &[ArtifactEntry],
    repo: &str,
    revision: &str,
) -> Result<(), ModelBundleError> {
    for artifact in artifacts {
        let remote = if let Some(rest) = artifact.path.strip_prefix("models/embedding/") {
            format!("onnx/{rest}")
        } else if let Some(rest) = artifact.path.strip_prefix("models/reranker/") {
            format!("onnx/{rest}")
        } else if let Some(rest) = artifact.path.strip_prefix("tokenizers/embedding/") {
            rest.to_string()
        } else if let Some(rest) = artifact.path.strip_prefix("tokenizers/reranker/") {
            rest.to_string()
        } else {
            artifact.path.clone()
        };
        let expected = format!("https://huggingface.co/{repo}/resolve/{revision}/{remote}");
        expect_approved_str(
            &format!("{} source.url", artifact.path),
            &artifact.source.url,
            &expected,
        )?;
    }
    Ok(())
}

fn tokenizer_artifacts_equal(a: &TokenizerContract, b: &TokenizerContract) -> bool {
    let same_len = a.artifacts.len() == b.artifacts.len();
    same_len
        && a.artifacts.iter().all(|entry| {
            b.artifacts
                .iter()
                .any(|other| other.path == entry.path && other.sha256 == entry.sha256)
        })
}

fn verify_file(
    path: &Path,
    expected_len: u64,
    expected_sha256: &str,
    display: &str,
) -> Result<(), ModelBundleError> {
    let fail = |reason: String| ModelBundleError::ArtifactVerification {
        path: display.to_string(),
        reason,
    };
    let mut file = File::open(path).map_err(|e| fail(format!("open failed: {e}")))?;
    let actual_len = file
        .metadata()
        .map_err(|e| fail(format!("metadata failed: {e}")))?
        .len();
    if actual_len != expected_len {
        return Err(fail(format!(
            "byte length mismatch: expected {expected_len}, got {actual_len}"
        )));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; READ_BUFFER_BYTES];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| fail(format!("read failed: {e}")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    if hex != expected_sha256 {
        return Err(fail(format!(
            "SHA-256 mismatch: expected {expected_sha256}, got {hex}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    const VALID_MANIFEST_TEMPLATE: &str = r#"{
  "manifestVersion": 1,
  "bundleId": "@@BUNDLE_ID@@",
  "embeddingModel": {
    "modelId": "intfloat/multilingual-e5-base",
    "revision": "d128750597153bb5987e10b1c3493a34e5a4502a",
    "dimensions": 768,
    "maxSequenceLength": 512,
    "onnxExport": {
      "repo": "Xenova/multilingual-e5-base",
      "revision": "1ec9243030a27d1a115d5c340572074c125b58b2",
      "quantization": "dynamic-int8"
    },
    "tokenizer": {
      "type": "XLM-RoBERTa unigram (tokenizer.json)",
      "revision": "1ec9243030a27d1a115d5c340572074c125b58b2",
      "truncationSide": "right",
      "artifacts": [
        { "path": "tokenizers/embedding/tokenizer.json", "byteLength": 10, "sha256": "@@TOK_E_JSON@@", "source": { "url": "https://huggingface.co/@@E5_REPO@@/resolve/@@E5_REV@@/tokenizer.json" } },
        { "path": "tokenizers/embedding/tokenizer_config.json", "byteLength": 10, "sha256": "@@TOK_E_CFG@@", "source": { "url": "https://huggingface.co/@@E5_REPO@@/resolve/@@E5_REV@@/tokenizer_config.json" } },
        { "path": "tokenizers/embedding/special_tokens_map.json", "byteLength": 10, "sha256": "@@TOK_E_MAP@@", "source": { "url": "https://huggingface.co/@@E5_REPO@@/resolve/@@E5_REV@@/special_tokens_map.json" } }
      ]
    },
    "queryPrefix": "query: ",
    "documentPrefix": "passage: ",
    "pooling": "masked-mean over attention_mask positions of last_hidden_state",
    "normalization": "l2",
    "inputs": [
      { "name": "input_ids", "dtype": "int64" },
      { "name": "attention_mask", "dtype": "int64" }
    ],
    "outputs": [{ "name": "last_hidden_state", "dtype": "float32" }],
    "artifacts": [
      { "path": "models/embedding/model_int8.onnx", "byteLength": 64, "sha256": "@@MODEL_E@@", "source": { "url": "https://huggingface.co/@@E5_REPO@@/resolve/@@E5_REV@@/onnx/model_int8.onnx" } }
    ]
  },
  "rerankerModel": {
    "modelId": "cross-encoder/mmarco-mMiniLMv2-L12-H384-v1",
    "revision": "1427fd652930e4ba29e8149678df786c240d8825",
    "maxSequenceLength": 512,
    "onnxExport": {
      "repo": "cross-encoder/mmarco-mMiniLMv2-L12-H384-v1",
      "revision": "1427fd652930e4ba29e8149678df786c240d8825",
      "quantization": "quint8_avx2"
    },
    "tokenizer": {
      "type": "XLM-RoBERTa unigram (tokenizer.json)",
      "revision": "1427fd652930e4ba29e8149678df786c240d8825",
      "truncationSide": "right",
      "artifacts": [
        { "path": "tokenizers/reranker/tokenizer.json", "byteLength": 10, "sha256": "@@TOK_R_JSON@@", "source": { "url": "https://huggingface.co/@@MM_REPO@@/resolve/@@MM_REV@@/tokenizer.json" } },
        { "path": "tokenizers/reranker/tokenizer_config.json", "byteLength": 10, "sha256": "@@TOK_R_CFG@@", "source": { "url": "https://huggingface.co/@@MM_REPO@@/resolve/@@MM_REV@@/tokenizer_config.json" } },
        { "path": "tokenizers/reranker/special_tokens_map.json", "byteLength": 10, "sha256": "@@TOK_R_MAP@@", "source": { "url": "https://huggingface.co/@@MM_REPO@@/resolve/@@MM_REV@@/special_tokens_map.json" } }
      ]
    },
    "pairFormat": "question,evidence",
    "inputs": [
      { "name": "input_ids", "dtype": "int64" },
      { "name": "attention_mask", "dtype": "int64" }
    ],
    "outputs": [{ "name": "logits", "dtype": "float32" }],
    "outputLabelIndex": 0,
    "scoreTransform": "identity",
    "artifacts": [
      { "path": "models/reranker/model_quint8_avx2.onnx", "byteLength": 64, "sha256": "@@MODEL_R@@", "source": { "url": "https://huggingface.co/@@MM_REPO@@/resolve/@@MM_REV@@/onnx/model_quint8_avx2.onnx" } }
    ]
  },
  "chunkerVersion": 1,
  "licenses": [
    {
      "spdx": "MIT",
      "appliesTo": "embeddingModel",
      "attribution": "@@LIC_MIT_ATTR@@",
      "resource": "@@LIC_MIT_RES@@",
      "path": "@@LIC_MIT_PATH@@",
      "byteLength": 10,
      "sha256": "@@LIC_MIT@@",
      "source": { "url": "@@LIC_MIT_SRC@@" }
    },
    {
      "spdx": "Apache-2.0",
      "appliesTo": "rerankerModel",
      "attribution": "@@LIC_APACHE_ATTR@@",
      "resource": "@@LIC_APACHE_RES@@",
      "path": "@@LIC_APACHE_PATH@@",
      "byteLength": 10,
      "sha256": "@@LIC_APACHE@@",
      "source": { "url": "@@LIC_APACHE_SRC@@" }
    }
  ]
}"#;

    struct TestBundle {
        #[allow(dead_code)]
        guard: tempfile::TempDir,
        dir: PathBuf,
        manifest_json: String,
    }

    fn write_bundle_with(mutate: impl FnOnce(String) -> String) -> TestBundle {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        let mut placeholders = Vec::new();
        let files: [(&str, [u8; 10]); 6] = [
            ("tokenizers/embedding/tokenizer.json", *b"tokEjson01"),
            ("tokenizers/embedding/tokenizer_config.json", *b"tokEcfg001"),
            (
                "tokenizers/embedding/special_tokens_map.json",
                *b"tokEmap001",
            ),
            ("tokenizers/reranker/tokenizer.json", *b"tokRjson01"),
            ("tokenizers/reranker/tokenizer_config.json", *b"tokRcfg001"),
            (
                "tokenizers/reranker/special_tokens_map.json",
                *b"tokRmap001",
            ),
        ];
        for (rel, content) in files {
            placeholders.push((format!("@@{}@@", placeholder_name(rel)), hash_hex(&content)));
            write_rel(root, rel, &content);
        }
        let model_e = (0..64u8).collect::<Vec<u8>>();
        let model_r = (64..128u8).collect::<Vec<u8>>();
        placeholders.push(("@@MODEL_E@@".to_string(), hash_hex(&model_e)));
        placeholders.push(("@@MODEL_R@@".to_string(), hash_hex(&model_r)));
        write_rel(root, "models/embedding/model_int8.onnx", &model_e);
        write_rel(root, "models/reranker/model_quint8_avx2.onnx", &model_r);
        placeholders.push(("@@LIC_MIT@@".to_string(), hash_hex(b"licMIT0001")));
        placeholders.push(("@@LIC_APACHE@@".to_string(), hash_hex(b"licAPACHE1")));
        write_rel(root, APPROVED_MIT_NOTICE_PATH, b"licMIT0001");
        write_rel(root, APPROVED_APACHE_NOTICE_PATH, b"licAPACHE1");

        let mut json = VALID_MANIFEST_TEMPLATE
            .replace("@@BUNDLE_ID@@", APPROVED_BUNDLE_ID)
            .replace("@@E5_REPO@@", APPROVED_EMBEDDING_EXPORT_REPO)
            .replace("@@E5_REV@@", APPROVED_EMBEDDING_EXPORT_REVISION)
            .replace("@@MM_REPO@@", APPROVED_RERANKER_MODEL_ID)
            .replace("@@MM_REV@@", APPROVED_RERANKER_REVISION)
            .replace("@@LIC_MIT_PATH@@", APPROVED_MIT_NOTICE_PATH)
            .replace("@@LIC_MIT_ATTR@@", APPROVED_MIT_ATTRIBUTION)
            .replace("@@LIC_MIT_RES@@", APPROVED_MIT_RESOURCE)
            .replace("@@LIC_MIT_SRC@@", APPROVED_MIT_SOURCE_URL)
            .replace("@@LIC_APACHE_PATH@@", APPROVED_APACHE_NOTICE_PATH)
            .replace("@@LIC_APACHE_ATTR@@", APPROVED_APACHE_ATTRIBUTION)
            .replace("@@LIC_APACHE_RES@@", APPROVED_APACHE_RESOURCE)
            .replace("@@LIC_APACHE_SRC@@", APPROVED_APACHE_SOURCE_URL);
        for (placeholder, digest) in placeholders {
            json = json.replace(&placeholder, &digest);
        }
        let json = mutate(json);
        let dir_path = dir.path().to_path_buf();
        TestBundle {
            guard: dir,
            dir: dir_path,
            manifest_json: json,
        }
    }

    fn placeholder_name(rel: &str) -> &'static str {
        match rel {
            "tokenizers/embedding/tokenizer.json" => "TOK_E_JSON",
            "tokenizers/embedding/tokenizer_config.json" => "TOK_E_CFG",
            "tokenizers/embedding/special_tokens_map.json" => "TOK_E_MAP",
            "tokenizers/reranker/tokenizer.json" => "TOK_R_JSON",
            "tokenizers/reranker/tokenizer_config.json" => "TOK_R_CFG",
            _ => "TOK_R_MAP",
        }
    }

    fn write_rel(root: &Path, rel: &str, content: &[u8]) {
        let target = root.join(rel);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, content).unwrap();
    }

    fn hash_hex(content: &[u8]) -> String {
        let digest = Sha256::digest(content);
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn parsed(bundle: &TestBundle) -> ModelBundleManifest {
        parse_manifest(&bundle.manifest_json).expect("valid fixture must parse")
    }

    #[test]
    fn valid_bundle_passes_parse_and_verification() {
        let bundle = write_bundle_with(|json| json);
        let manifest = parsed(&bundle);
        manifest
            .verify_artifacts(&bundle.dir)
            .expect("all hashes match");
    }

    #[test]
    fn one_byte_corruption_fails_verification() {
        let bundle = write_bundle_with(|json| json);
        let model_path = bundle.dir.join("models/embedding/model_int8.onnx");
        let mut bytes = fs::read(&model_path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        fs::write(&model_path, bytes).unwrap();
        let err = parsed(&bundle)
            .verify_artifacts(&bundle.dir)
            .expect_err("corruption must fail");
        assert!(err.to_string().contains("SHA-256 mismatch"), "{err}");
    }

    #[test]
    fn wrong_byte_length_fails_verification() {
        let bundle = write_bundle_with(|json| {
            json.replace(
                "\"path\": \"models/embedding/model_int8.onnx\", \"byteLength\": 64",
                "\"path\": \"models/embedding/model_int8.onnx\", \"byteLength\": 63",
            )
        });
        let err = parsed(&bundle)
            .verify_artifacts(&bundle.dir)
            .expect_err("length mismatch must fail");
        assert!(err.to_string().contains("byte length mismatch"), "{err}");
    }

    #[test]
    fn missing_artifacts_fail_verification() {
        for rel in [
            APPROVED_EMBEDDING_MODEL_ARTIFACT,
            APPROVED_RERANKER_MODEL_ARTIFACT,
            "tokenizers/embedding/tokenizer.json",
            "tokenizers/reranker/tokenizer_config.json",
            APPROVED_MIT_NOTICE_PATH,
            APPROVED_APACHE_NOTICE_PATH,
        ] {
            let bundle = write_bundle_with(|json| json);
            fs::remove_file(bundle.dir.join(rel)).unwrap();
            let err = parsed(&bundle)
                .verify_artifacts(&bundle.dir)
                .expect_err("missing artifact must fail");
            assert!(err.to_string().contains("open failed"), "{rel}: {err}");
        }
    }

    #[test]
    fn unknown_manifest_version_fails_closed() {
        let bundle = write_bundle_with(|json| {
            json.replace("\"manifestVersion\": 1", "\"manifestVersion\": 2")
        });
        let err = parse_manifest(&bundle.manifest_json).expect_err("unknown version must fail");
        assert!(matches!(
            err,
            ModelBundleError::UnsupportedManifestVersion { found: 2 }
        ));
    }

    #[test]
    fn malformed_sha256_fails_validation() {
        let bundle = write_bundle_with(|json| json);
        let mut broken = bundle.manifest_json.clone();
        let idx = broken.find("\"sha256\": \"").unwrap();
        let start = idx + "\"sha256\": \"".len();
        broken.replace_range(start..start + 2, "ZZ");
        let err = parse_manifest(&broken).expect_err("malformed hash must fail");
        assert!(
            matches!(err, ModelBundleError::InvalidManifest(m) if m.contains("malformed SHA-256"))
        );
    }

    #[test]
    fn wrong_input_dtype_and_missing_input_name_fail_validation() {
        let bad_dtype = write_bundle_with(|json| {
            json.replace(
                "{ \"name\": \"attention_mask\", \"dtype\": \"int64\" }",
                "{ \"name\": \"attention_mask\", \"dtype\": \"float64\" }",
            )
        });
        let err =
            parse_manifest(&bad_dtype.manifest_json).expect_err("non-approved dtype must fail");
        assert!(
            err.to_string()
                .contains("must have dtype 'int64', got 'float64'"),
            "{err}"
        );

        let bad_name = write_bundle_with(|json| {
            json.replace(
                "{ \"name\": \"input_ids\", \"dtype\": \"int64\" }",
                "{ \"name\": \"ids\", \"dtype\": \"int64\" }",
            )
        });
        let err = parse_manifest(&bad_name.manifest_json).expect_err("wrong input name must fail");
        assert!(
            err.to_string()
                .contains("missing required tensor 'input_ids'"),
            "{err}"
        );
    }

    #[test]
    fn known_but_wrong_dtype_fails_validation_and_valid_exact_values_pass() {
        // The untouched fixture declares the exact approved dtypes and parses.
        let valid = write_bundle_with(|json| json);
        parse_manifest(&valid.manifest_json).expect("exact approved dtypes must pass");

        let input_i32 = write_bundle_with(|json| {
            json.replace(
                "{ \"name\": \"input_ids\", \"dtype\": \"int64\" }",
                "{ \"name\": \"input_ids\", \"dtype\": \"int32\" }",
            )
        });
        let err = parse_manifest(&input_i32.manifest_json)
            .expect_err("known-but-wrong input dtype must fail");
        assert!(err.to_string().contains("must have dtype 'int64'"), "{err}");

        let output_i64 = write_bundle_with(|json| {
            json.replace(
                "\"outputs\": [{ \"name\": \"last_hidden_state\", \"dtype\": \"float32\" }]",
                "\"outputs\": [{ \"name\": \"last_hidden_state\", \"dtype\": \"int64\" }]",
            )
        });
        let err = parse_manifest(&output_i64.manifest_json)
            .expect_err("known-but-wrong embedding output dtype must fail");
        assert!(
            err.to_string().contains("must have dtype 'float32'"),
            "{err}"
        );

        let logits_i32 = write_bundle_with(|json| {
            json.replace(
                "\"outputs\": [{ \"name\": \"logits\", \"dtype\": \"float32\" }]",
                "\"outputs\": [{ \"name\": \"logits\", \"dtype\": \"int32\" }]",
            )
        });
        let err = parse_manifest(&logits_i32.manifest_json)
            .expect_err("known-but-wrong reranker label dtype must fail");
        assert!(
            err.to_string().contains("must have dtype 'float32'"),
            "{err}"
        );
    }

    #[test]
    fn extra_or_unexpected_tensors_fail_validation() {
        let extra_output = write_bundle_with(|json| {
            json.replace(
                "\"outputs\": [{ \"name\": \"last_hidden_state\", \"dtype\": \"float32\" }]",
                "\"outputs\": [\
                 { \"name\": \"last_hidden_state\", \"dtype\": \"float32\" }, \
                 { \"name\": \"sentence_embedding\", \"dtype\": \"float32\" }]",
            )
        });
        let err =
            parse_manifest(&extra_output.manifest_json).expect_err("extra output tensor must fail");
        assert!(
            err.to_string().contains("must declare exactly the tensors"),
            "{err}"
        );
    }

    #[test]
    fn tokenizer_revision_mismatch_fails_validation() {
        let unrelated = "f000000000000000000000000000000000000001";
        let embedding = write_bundle_with(|json| {
            json.replace(
                "\"revision\": \"1ec9243030a27d1a115d5c340572074c125b58b2\",\n      \"truncationSide\"",
                &format!("\"revision\": \"{unrelated}\",\n      \"truncationSide\""),
            )
        });
        let err = parse_manifest(&embedding.manifest_json)
            .expect_err("unrelated embedding tokenizer revision must fail");
        assert!(
            err.to_string()
                .contains("does not match its pinned ONNX export revision"),
            "{err}"
        );

        let reranker = write_bundle_with(|json| {
            json.replace(
                "\"revision\": \"1427fd652930e4ba29e8149678df786c240d8825\",\n      \"truncationSide\"",
                &format!("\"revision\": \"{unrelated}\",\n      \"truncationSide\""),
            )
        });
        let err = parse_manifest(&reranker.manifest_json)
            .expect_err("unrelated reranker tokenizer revision must fail");
        assert!(
            err.to_string()
                .contains("does not match its pinned model revision"),
            "{err}"
        );
    }

    #[test]
    fn missing_or_empty_artifact_source_fails_validation() {
        let embedding_url = format!(
            "https://huggingface.co/{APPROVED_EMBEDDING_EXPORT_REPO}/resolve/{APPROVED_EMBEDDING_EXPORT_REVISION}/onnx/model_int8.onnx"
        );
        let no_source = write_bundle_with(|json| {
            json.replace(
                &format!(", \"source\": {{ \"url\": \"{embedding_url}\" }}"),
                "",
            )
        });
        let err = parse_manifest(&no_source.manifest_json).expect_err("missing source must fail");
        assert!(err.to_string().contains("missing field `source`"), "{err}");

        let empty_url = write_bundle_with(|json| json.replace(&embedding_url, ""));
        let err = parse_manifest(&empty_url.manifest_json).expect_err("empty source url must fail");
        assert!(
            err.to_string().contains("source.url must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn license_coverage_must_be_exactly_one_per_component() {
        let duplicate = write_bundle_with(|json| {
            json.replace(
                "\"appliesTo\": \"rerankerModel\"",
                "\"appliesTo\": \"embeddingModel\"",
            )
        });
        let err =
            parse_manifest(&duplicate.manifest_json).expect_err("duplicate coverage must fail");
        assert!(
            err.to_string()
                .contains("duplicate coverage for 'embeddingModel'"),
            "{err}"
        );

        let missing = write_bundle_with(|json| {
            let mut j = json;
            let marker = "\"spdx\": \"Apache-2.0\"";
            let mpos = j.find(marker).unwrap();
            let obj_start = j[..mpos].rfind('{').unwrap();
            let comma = j[..obj_start].rfind(',').unwrap();
            let end_marker = "\n    }";
            let end = j[mpos..].find(end_marker).unwrap() + mpos + end_marker.len() - 1;
            j.replace_range(comma..=end, "");
            j
        });
        let err = parse_manifest(&missing.manifest_json)
            .expect_err("missing component license must fail");
        assert!(
            err.to_string().contains("no coverage for 'rerankerModel'"),
            "{err}"
        );
    }

    #[test]
    fn wrong_output_name_fails_validation() {
        let bundle = write_bundle_with(|json| {
            json.replace(
                "\"outputs\": [{ \"name\": \"last_hidden_state\", \"dtype\": \"float32\" }]",
                "\"outputs\": [{ \"name\": \"hidden\", \"dtype\": \"float32\" }]",
            )
        });
        let err = parse_manifest(&bundle.manifest_json).expect_err("wrong output name must fail");
        assert!(err.to_string().contains("last_hidden_state"), "{err}");
    }

    #[test]
    fn out_of_bounds_label_index_fails_validation() {
        let bundle = write_bundle_with(|json| {
            json.replace("\"outputLabelIndex\": 0", "\"outputLabelIndex\": 3")
        });
        let err = parse_manifest(&bundle.manifest_json).expect_err("label index must be bounded");
        assert!(err.to_string().contains("out of bounds"), "{err}");
    }

    #[test]
    fn wrong_pair_format_fails_validation() {
        let bundle = write_bundle_with(|json| {
            json.replace(
                "\"pairFormat\": \"question,evidence\"",
                "\"pairFormat\": \"query,passage\"",
            )
        });
        let err = parse_manifest(&bundle.manifest_json).expect_err("pair format must be approved");
        assert!(err.to_string().contains("pairFormat"), "{err}");
    }

    #[test]
    fn tokenizer_artifact_outside_ownership_fails_validation() {
        let bundle = write_bundle_with(|json| {
            json.replace(
                "{ \"path\": \"tokenizers/embedding/tokenizer.json\", \"byteLength\": 10, \"sha256\": \"",
                "{ \"path\": \"models/reranker/model_quint8_avx2.onnx\", \"byteLength\": 10, \"sha256\": \"",
            )
        });
        // The duplicate-path guard fires because the tokenizer now claims an
        // artifact owned by the reranker model section.
        let err = parse_manifest(&bundle.manifest_json).expect_err("stolen path must fail");
        assert!(err.to_string().contains("duplicate artifact path"), "{err}");
    }

    #[test]
    fn incoherent_shared_tokenizer_identity_fails_validation() {
        let bundle = write_bundle_with(|json| {
            json.replace(
                "\"type\": \"XLM-RoBERTa unigram (tokenizer.json)\",\n      \"revision\": \"1427fd652930e4ba29e8149678df786c240d8825\"",
                "\"type\": \"XLM-RoBERTa unigram (tokenizer.json)\",\n      \"revision\": \"1ec9243030a27d1a115d5c340572074c125b58b2\"",
            )
        });
        let err = parse_manifest(&bundle.manifest_json)
            .expect_err("same identity with different artifacts must fail");
        assert!(err.to_string().contains("same tokenizer identity"), "{err}");
    }

    #[test]
    fn non_approved_license_authority_fails_validation_and_missing_notice_fails_verification() {
        let wrong_spdx = write_bundle_with(|json| {
            json.replace("\"spdx\": \"MIT\"", "\"spdx\": \"CC-BY-NC-4.0\"")
        });
        let err = parse_manifest(&wrong_spdx.manifest_json)
            .expect_err("non-approved embedding SPDX must fail");
        assert!(
            err.to_string()
                .contains("approved production contract requires \"MIT\""),
            "{err}"
        );

        let missing = write_bundle_with(|json| json);
        fs::remove_file(missing.dir.join(APPROVED_MIT_NOTICE_PATH)).unwrap();
        let err = parsed(&missing)
            .verify_artifacts(&missing.dir)
            .expect_err("missing notice artifact must fail");
        assert!(err.to_string().contains("open failed"), "{err}");
    }

    /// Task 1.R2: every selected-contract substitution — model identity,
    /// revision, export repo/revision, quantization, dimensions,
    /// preprocessing, tokenizer contract, provenance URL, and license
    /// authority/attribution — must fail `parse_manifest` itself, not merely a
    /// test assertion.
    #[test]
    fn selected_contract_substitutions_fail_parsing() {
        let e5_rev = APPROVED_EMBEDDING_EXPORT_REVISION;
        let mm_rev = APPROVED_RERANKER_REVISION;
        let wrong_rev = |ch: char| std::iter::repeat(ch).take(40).collect::<String>();
        let e5_model_url = format!(
            "https://huggingface.co/{APPROVED_EMBEDDING_EXPORT_REPO}/resolve/{e5_rev}/onnx/model_int8.onnx"
        );
        let cases: Vec<(&str, String, String)> = vec![
            (
                "embedding model id",
                "\"modelId\": \"intfloat/multilingual-e5-base\"".into(),
                "\"modelId\": \"intfloat/multilingual-e5-large\"".into(),
            ),
            (
                "embedding model revision",
                concat!(
                    "\"revision\": \"d128750597153bb5987e10b1c3493a34e5a4502a\",\n",
                    "    \"dimensions\""
                )
                .into(),
                format!(
                    "\"revision\": \"{}\",\n    \"dimensions\"",
                    wrong_rev('a')
                ),
            ),
            (
                "embedding export repo",
                "\"repo\": \"Xenova/multilingual-e5-base\"".into(),
                "\"repo\": \"some-fork/multilingual-e5-base\"".into(),
            ),
            (
                "embedding export revision",
                e5_rev.to_string(),
                wrong_rev('b'),
            ),
            (
                "embedding quantization",
                "\"quantization\": \"dynamic-int8\"".into(),
                "\"quantization\": \"static-int8\"".into(),
            ),
            (
                "dimensions",
                "\"dimensions\": 768".into(),
                "\"dimensions\": 384".into(),
            ),
            (
                "query prefix",
                "\"queryPrefix\": \"query: \"".into(),
                "\"queryPrefix\": \"query:\"".into(),
            ),
            (
                "document prefix",
                "\"documentPrefix\": \"passage: \"".into(),
                "\"documentPrefix\": \"text: \"".into(),
            ),
            (
                "pooling",
                "\"pooling\": \"masked-mean over attention_mask positions of last_hidden_state\""
                    .into(),
                "\"pooling\": \"cls token of last_hidden_state\"".into(),
            ),
            (
                "normalization",
                "\"normalization\": \"l2\"".into(),
                "\"normalization\": \"none\"".into(),
            ),
            (
                "truncation side",
                "\"truncationSide\": \"right\"".into(),
                "\"truncationSide\": \"left\"".into(),
            ),
            (
                "tokenizer type",
                "\"type\": \"XLM-RoBERTa unigram (tokenizer.json)\"".into(),
                "\"type\": \"sentencepiece unigram (tokenizer.json)\"".into(),
            ),
            (
                "reranker model id",
                "\"modelId\": \"cross-encoder/mmarco-mMiniLMv2-L12-H384-v1\"".into(),
                "\"modelId\": \"cross-encoder/ms-marco-MiniLM-L-6-v2\"".into(),
            ),
            (
                "reranker model revision",
                concat!(
                    "\"revision\": \"1427fd652930e4ba29e8149678df786c240d8825\",\n",
                    "    \"maxSequenceLength\""
                )
                .into(),
                format!(
                    "\"revision\": \"{}\",\n    \"maxSequenceLength\"",
                    wrong_rev('c')
                ),
            ),
            (
                "reranker export repo",
                "\"repo\": \"cross-encoder/mmarco-mMiniLMv2-L12-H384-v1\"".into(),
                "\"repo\": \"cross-encoder/ms-marco-MiniLM-L-6-v2\"".into(),
            ),
            (
                "reranker export revision",
                mm_rev.to_string(),
                wrong_rev('d'),
            ),
            (
                "reranker quantization",
                "\"quantization\": \"quint8_avx2\"".into(),
                "\"quantization\": \"fp16\"".into(),
            ),
            (
                "provenance url drifts from pinned revision",
                e5_model_url.clone(),
                format!(
                    "https://huggingface.co/{APPROVED_EMBEDDING_EXPORT_REPO}/resolve/main/onnx/model_int8.onnx"
                ),
            ),
            (
                "mit notice authority swapped",
                format!(
                    "\"url\": \"{APPROVED_MIT_SOURCE_URL}\""
                ),
                "\"url\": \"https://www.apache.org/licenses/LICENSE-2.0.txt\"".into(),
            ),
            (
                "copyright attribution rewritten",
                "Copyright (c) Microsoft Corporation".into(),
                "Copyright (c) Anonymous".into(),
            ),
        ];
        assert_eq!(cases.len(), 20);
        for (label, old, new) in cases {
            let bundle = write_bundle_with(|json| json.replace(&old, &new));
            let err = parse_manifest(&bundle.manifest_json)
                .expect_err(format!("substituted {label} must fail parsing").as_str());
            assert!(
                err.to_string().contains("approved production contract"),
                "{label}: {err}"
            );
        }
    }

    /// CI release gate (Task 1.R1): after staging, verify every
    /// manifest-managed artifact in the published production bundle —
    /// presence, byte length, and SHA-256. Gated so plain `cargo test`
    /// stays green on clean checkouts where the bundle is intentionally
    /// just the committed placeholder.
    #[test]
    fn staged_production_bundle_artifacts_verify() {
        let bundle_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/retrieval/bundle");
        if std::env::var("MEETLY_RAG_VERIFY_STAGED_BUNDLE").as_deref() != Ok("1") {
            println!("SKIP staged-bundle verification (set MEETLY_RAG_VERIFY_STAGED_BUNDLE=1)");
            return;
        }
        assert!(
            bundle_root.is_dir(),
            "MEETLY_RAG_VERIFY_STAGED_BUNDLE=1 but no staged bundle at {} (run stage-retrieval-models.ps1)",
            bundle_root.display()
        );
        let json = fs::read_to_string(bundle_root.join("model-bundle.manifest.json"))
            .expect("staged bundle must contain its own manifest copy");
        let manifest = parse_manifest(&json).expect("staged bundle manifest must validate");
        assert_eq!(
            manifest.artifact_entries().count() + manifest.licenses.len(),
            10,
            "production bundle must manage exactly ten artifacts"
        );
        manifest
            .verify_artifacts(&bundle_root)
            .expect("all ten manifest-managed artifacts must be present and hash-verified");
    }

    #[test]
    fn checked_in_production_manifest_matches_approved_bundle() {
        let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/retrieval/model-bundle.manifest.json");
        let json = fs::read_to_string(manifest_path).expect("production manifest is checked in");
        let manifest = parse_manifest(&json).expect("production manifest must validate");

        assert_eq!(manifest.bundle_id, "meetily-retrieval-bundle-1");
        assert_eq!(manifest.chunker_version, 1);

        let e = &manifest.embedding_model;
        assert_eq!(e.model_id, "intfloat/multilingual-e5-base");
        assert_eq!(e.revision, "d128750597153bb5987e10b1c3493a34e5a4502a");
        assert_eq!(e.onnx_export.repo, "Xenova/multilingual-e5-base");
        assert_eq!(
            e.onnx_export.revision,
            "1ec9243030a27d1a115d5c340572074c125b58b2"
        );
        assert_eq!(e.onnx_export.quantization, "dynamic-int8");
        assert_eq!(e.dimensions, 768);
        assert_eq!(e.max_sequence_length, 512);
        assert_eq!(e.query_prefix, "query: ");
        assert_eq!(e.document_prefix, "passage: ");
        assert_eq!(e.normalization, "l2");
        assert!(e.pooling.contains("masked-mean"));
        assert_eq!(e.inputs.len(), 2);

        let r = &manifest.reranker_model;
        assert_eq!(r.model_id, "cross-encoder/mmarco-mMiniLMv2-L12-H384-v1");
        assert_eq!(r.revision, "1427fd652930e4ba29e8149678df786c240d8825");
        assert_eq!(r.onnx_export.quantization, "quint8_avx2");
        assert_eq!(r.pair_format, "question,evidence");
        assert_eq!(r.output_label_index, 0);
        assert_eq!(r.outputs[0].name, "logits");

        assert_eq!(manifest.licenses.len(), 2);
        assert_eq!(manifest.licenses[0].spdx, "MIT");
        assert_eq!(manifest.licenses[1].spdx, "Apache-2.0");
        assert_eq!(
            manifest.artifact_entries().count() + manifest.licenses.len(),
            10
        );

        // Task 1.R2: the packaged embedding notice must be the composed,
        // pinned-evidence artifact — never a generic placeholder template.
        let mit = manifest
            .licenses
            .iter()
            .find(|license| license.applies_to == "embeddingModel")
            .expect("MIT coverage entry");
        let notice = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("resources/retrieval")
                .join(&mit.path),
        )
        .expect("checked-in MIT notice artifact exists");
        assert!(
            !notice.contains("<year>"),
            "notice retains template placeholder"
        );
        assert!(
            !notice.contains("<copyright holders>"),
            "notice retains template placeholder"
        );
        assert!(notice.contains("Copyright (c) Microsoft Corporation"));
        assert!(notice.contains("Permission is hereby granted"));
        assert!(notice.contains(APPROVED_EMBEDDING_EXPORT_REPO));
        assert_eq!(
            mit.source.url,
            "https://raw.githubusercontent.com/microsoft/unilm/0e31c7c09737df491e7ff74ded19614b884c52b4/LICENSE"
        );

        // Tokenizer artifacts pin exactly the export/model revisions they
        // ship with (identical tokenizer.json bytes, different revisions and
        // config artifacts).
        assert_eq!(
            e.tokenizer.revision,
            "1ec9243030a27d1a115d5c340572074c125b58b2"
        );
        assert_eq!(
            r.tokenizer.revision,
            "1427fd652930e4ba29e8149678df786c240d8825"
        );
    }
}
