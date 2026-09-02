use app_lib::retrieval::get_or_load;
use sha2::{Digest, Sha256};
use std::{path::Path, time::Instant};
use tokio_util::sync::CancellationToken;

const SAMPLE_COUNT: usize = 50;
const RERANK_DEPTH: usize = 50;
const ORT_INTRA_OP_CAP: usize = 4;
const APPROVED_MANIFEST_SHA256: &str =
    "8a3751069f4c77ddec4db7c92f75d99900525bbe48e00e28ec1cf3ffff264ff4";

fn release_profile_guard(is_release: bool) -> Result<(), &'static str> {
    is_release
        .then_some(())
        .ok_or("release profile is required")
}

fn manifest_digest_guard(digest: &str) -> Result<(), &'static str> {
    (digest == APPROVED_MANIFEST_SHA256)
        .then_some(())
        .ok_or("manifest digest is not approved")
}

fn benchmark_pairs() -> Vec<(String, String)> {
    (0..RERANK_DEPTH)
        .map(|index| {
            (
                format!(
                    "local synthetic query {index}: which day has the retention contact?"
                ),
                format!(
                    "local synthetic evidence {index}: the retention schedule has a contact on day {}.",
                    index % 15 + 1
                ),
            )
        })
        .collect()
}

fn percentile(samples: &[u128], percentile: usize) -> u128 {
    samples[(samples.len() - 1) * percentile / 100]
}

fn sha256(path: &Path) -> String {
    Sha256::digest(std::fs::read(path).expect("artifact must be readable"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn assert_scores(scores: &[f32]) {
    assert_eq!(scores.len(), RERANK_DEPTH);
    assert!(scores.iter().all(|score| score.is_finite()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the staged production bundle and an explicit release run"]
async fn production_bundle_rerank_depth_50_benchmark() {
    release_profile_guard(cfg!(not(debug_assertions)))
        .expect("production evidence requires Cargo release profile");
    let root = std::env::var("MEETLY_RAG_BUNDLE_DIR")
        .map(std::path::PathBuf::from)
        .expect("MEETLY_RAG_BUNDLE_DIR is required for evidence");
    assert!(
        root.is_dir(),
        "bundle directory is missing: {}",
        root.display()
    );
    let manifest = root.join("model-bundle.manifest.json");
    let manifest_sha256 = sha256(&manifest);
    manifest_digest_guard(&manifest_sha256).expect("staged manifest digest must be approved");

    let models = get_or_load(&root).expect("hash-verified production bundle must load");
    assert_eq!(models.identity().bundle_id, "meetily-retrieval-bundle-1");
    let pairs = benchmark_pairs();
    assert_eq!(pairs.len(), RERANK_DEPTH);

    assert_scores(
        &models
            .rerank(pairs.clone(), CancellationToken::new())
            .await
            .expect("warm-up rerank must complete"),
    );

    let mut samples = Vec::with_capacity(SAMPLE_COUNT);
    for _ in 0..SAMPLE_COUNT {
        let request = pairs.clone();
        let started = Instant::now();
        let scores = models
            .rerank(request, CancellationToken::new())
            .await
            .expect("complete depth-50 rerank request must succeed");
        let elapsed = started.elapsed().as_micros();
        assert_scores(&scores);
        samples.push(elapsed);
    }
    samples.sort_unstable();

    let logical_cpus = std::thread::available_parallelism().map_or(1, |count| count.get());
    let intra_op_threads = (logical_cpus / 2).clamp(1, ORT_INTRA_OP_CAP);
    let cpu = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unreported".to_string());
    let identity = models.identity();
    let reranker = identity.root.join("models/reranker/model_quint8_avx2.onnx");

    println!(
        "[production-rerank] bundle_id={} root={} manifest_sha256={} reranker_sha256={}",
        identity.bundle_id,
        identity.root.display(),
        manifest_sha256,
        sha256(&reranker)
    );
    println!(
        "[production-rerank] hardware cpu={cpu} logical_cpus={logical_cpus} os={} arch={}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!(
        "[production-rerank] runtime provider=CPUExecutionProvider intra_op_threads={} inter_op_threads=1 batch=1 depth={RERANK_DEPTH}",
        intra_op_threads
    );
    println!(
        "[production-rerank] samples={} pairs_per_sample={} total_pairs={} p50={:.3}ms p95={:.3}ms max={:.3}ms",
        samples.len(),
        pairs.len(),
        samples.len() * pairs.len(),
        percentile(&samples, 50) as f64 / 1000.0,
        percentile(&samples, 95) as f64 / 1000.0,
        samples.last().copied().expect("samples are non-empty") as f64 / 1000.0
    );
    assert_eq!(samples.len(), SAMPLE_COUNT);
    assert_eq!(samples.len() * pairs.len(), 2_500);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_profile_guard_covers_debug_and_release() {
        assert!(release_profile_guard(true).is_ok());
        assert!(release_profile_guard(false).is_err());
    }

    #[test]
    fn manifest_digest_guard_covers_expected_and_mismatched_values() {
        assert!(manifest_digest_guard(APPROVED_MANIFEST_SHA256).is_ok());
        assert!(manifest_digest_guard("mismatched-digest").is_err());
    }
}
