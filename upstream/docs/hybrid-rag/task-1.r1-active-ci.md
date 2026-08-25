# Task 1.R1 - Activate the Task 1.5 release gate in the active root Windows CI

**Status:** Complete
**Completed:** 2026-08-25
**Scope:** Sprint 1 review remediation `1.R1` only - move the Rust 1.88.0
toolchain contract, the model cache/staging steps, and the mandatory staged
reference-inference gate into the actual repository-root Windows workflow, and
add the clean-checkout ten-artifact package-preparation assertion. The inert
nested workflow (`upstream/.github/workflows/build-windows.yml`) was not
touched. No work on `1.R2`/`1.R3`.

## Active workflow path

Repository root `.github/workflows/build-windows.yml` (the only workflow GitHub
Actions loads in this fork; git root is the parent of `upstream/`). Both jobs
are covered:

- `build-windows` (release/debug packaging job)
- `check-rust` (quick cargo-check job)

## Toolchain policy

Both jobs previously installed floating `dtolnay/rust-toolchain@stable`. Each
now runs three steps:

1. **Read pinned channel** (bash): parse `channel = "..."` from
   `upstream/rust-toolchain.toml` with POSIX sed; fail if absent.
2. **Install pinned toolchain**: `dtolnay/rust-toolchain@master` with
   `toolchain: ${{ steps.rust-channel.outputs.channel }}` (currently resolves
   to `1.88.0`; the workflow follows the file rather than hardcoding it).
3. **Assert active rustc** (pwsh): compare `rustc --version` against the
   channel re-parsed from `upstream/rust-toolchain.toml`; mismatch fails the
   job. A future channel bump in the TOML propagates automatically; a workflow
   that drifts from the file fails loudly.

No `stable` remains anywhere in the active workflow.

## Cache/staging behavior (build job)

Inserted after frontend dependencies / Rust setup, before the llama-helper and
Tauri builds, in this order:

1. **Model cache** - `MODEL_CACHE_PATH` is exported from `$LOCALAPPDATA` into
   `GITHUB_ENV`, then cached via `actions/cache@v4` at
   `%LOCALAPPDATA%\meetily\model-cache` keyed on
   `hashFiles('upstream/frontend/src-tauri/resources/retrieval/model-bundle.manifest.json')`
   (+ prefix restore key). This is exactly the default `-CacheRoot` of
   `stage-retrieval-models.ps1`; a manifest change invalidates the cache key.
2. **Manifest contract validation (before staging)** -
   `cargo test --lib model_bundle` runs the existing 19-test focused suite,
   including `checked_in_production_manifest_matches_approved_bundle`, so a
   semantically incompatible checked-in manifest fails before any fetch.
3. **Staging** - `./upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1`
   fetches each artifact into the cache (immutable revision-resolve URLs;
   checked-in exact license sources), verifies length+SHA-256 per download,
   stages outside resources, re-verifies the whole package, publishes
   atomically to `resources/retrieval/bundle`.
4. **Clean-checkout/package-preparation assertion** - new gated test
   `model_bundle::tests::staged_production_bundle_artifacts_verify`
   (`MEETLY_RAG_VERIFY_STAGED_BUNDLE=1`) parses the bundle's own manifest copy
   with the production validator, asserts exactly **ten** managed artifacts
   (8 model/tokenizer + 2 licenses), and streams every one of them through
   `verify_artifacts` (existence + byte length + SHA-256). Missing/invalid
   artifacts fail the job.
5. **Staged reference-inference gate** - see below.

## Exact inference test command/environment

```bash
# env: MEETLY_RAG_BUNDLE_DIR=${{ github.workspace }}/upstream/frontend/src-tauri/resources/retrieval/bundle
cd upstream
cargo test --manifest-path frontend/src-tauri/Cargo.toml \
  --test model_benchmark reference_inference_is_stable_finite_and_dimensional -- --nocapture
```

Reuses the Task 1.3 harness (`tests/model_benchmark.rs`) - no second inference
implementation. Smallest adaptation made so the harness consumes the production
bundle layout:

- New `MEETLY_RAG_BUNDLE_DIR` gate mode on
  `reference_inference_is_stable_finite_and_dimensional`: when set, the
  embedding model is loaded from `models/embedding/<file>` with its tokenizer
  from `tokenizers/embedding`, and the approved reranker export from
  `models/reranker/model_quint8_avx2.onnx` with tokenizer from
  `tokenizers/reranker`. When set but missing/invalid, the test **fails**
  instead of skipping. Without the variable, legacy benchmark-staging behavior
  (`MEETLY_RAG_MODELS_DIR`, co-located tokenizers, skip-on-absent) is unchanged
  for local development.
- `TextModel::load_from(model_dir, tokenizer_dir, ...)` plus thin
  `Embedder::load_from` / `RerankModel::load_from` wrappers; `load` delegates
  with both dirs equal, so every pre-existing caller is untouched.
- In bundle mode the executed reranker list is exactly the packaged approved
  export (`model_quint8_avx2.onnx`). The recorded f32/bge expectation groups
  are not packaged by design (Task 1.5 manifest manages ten artifacts); bge
  stays retired-skipped as before, f32 prints an explicit "not applicable"
  line in bundle mode. All required stages - tokenizer (embedding+reranker),
  embedding int8, reranker quint8_avx2 - execute for real and are compared
  against the recorded platform-neutral reference expectations within their
  tolerances. Models, revisions, expected values, tolerances, and runtime
  behavior were not changed; no downloads were introduced.

## Clean-checkout assertion

Step 4 above is the assertion: on a fresh checkout (only the committed bundle
README present), staging must produce a complete published bundle or the job
stops before `tauri build`; then the verifier proves all ten manifest-managed
artifacts are present and hash-verified in that staged bundle. Installed-package
inference remains Sprint 5 and is not claimed here.

## Verified commands and results

Run from `upstream/` unless noted (2026-08-25, Windows x64):

| Command | Result |
|---|---|
| `npx --yes yaml-lint .github/workflows/build-windows.yml` (repo root) | PASS |
| Channel parsing: sed pattern over `rust-toolchain.toml` (no BOM confirmed) and pwsh `Select-String` pattern | both yield `1.88.0` (sed executed under bash on runners; locally verified pattern-equivalently because Git Bash is unavailable on this machine) |
| `$env:CARGO_TARGET_DIR=...meetily-cargo-target; cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib model_bundle` | PASS - 20 tests (19 existing + new gated test skipping without env) |
| Same with `MEETLY_RAG_VERIFY_STAGED_BUNDLE=1`, filter `staged_production_bundle` | PASS - ten artifacts presence+length+SHA-256 verified against staged bundle |
| Same env, bundle dir temporarily nonexistent | FAILS with clear message (fail-closed proof) |
| `MEETLY_RAG_BUNDLE_DIR=<nonexistent>; cargo test ... --test model_benchmark reference_inference...` | FAILS fast ("set but missing") - no skip possible |
| `MEETLY_RAG_BUNDLE_DIR=<staged bundle>; cargo test ... --test model_benchmark reference_inference_is_stable_finite_and_dimensional -- --nocapture` | PASS - tokenizer/embedding/reranker inference vs recorded expectations (7.2 s) |
| Fresh-cache staging proof: `./frontend/src-tauri/scripts/stage-retrieval-models.ps1 -CacheRoot <empty temp>` | PASS - 8 artifacts fetched from pinned revision URLs, 2 licenses served checked-in, all 10 verified, atomically published (exit 0); temp cache deleted after |
| Both gates re-run against the freshly republished bundle | PASS (verifier 9.5 s, inference 8.0 s) |
| Legacy mode regression: same test with no env vars (default `%TEMP%` staging present) | PASS - full legacy replay incl. f32 group unchanged |
| `pnpm --dir "upstream/frontend" run typecheck` (repo root) | PASS (exit 0) |
| `npx vitest run` (frontend) | PASS - 20 files / 95 tests |
| `$env:CARGO_TARGET_DIR=<LOCALAPPDATA>\meetily-cargo-target; cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` | PASS |
| `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" -- --check` | PASS |
| `git diff --check` | PASS (pre-existing CRLF warnings only, untouched files) |

### What cannot be executed locally versus what CI will execute

- Locally executed end to end except the two items below: YAML lint, all four
  cargo gate commands, cold-cache staging with real network fetches, reference
  inference, typecheck, Vitest, cargo check/fmt, diff check.
- Not executable locally: the actual GitHub Actions execution (runner-level
  `shell: bash` sed step - Git Bash/sed is not installed on this machine; the
  pattern was validated byte-wise against the file and mirrors standard POSIX
  sed), and the `actions/cache` restore/save round trip. Both are first-run
  verifiable in CI; failure modes fail loudly (empty parse exits non-zero).
- CI additionally executes the full Tauri build/packaging after the gates;
  that part is unchanged from the previous workflow.

## Files changed

- `.github/workflows/build-windows.yml` (active root workflow - toolchain pin
  + assertion in both jobs; model cache; staging; ten-artifact assertion;
  reference-inference gate)
- `upstream/frontend/src-tauri/tests/model_benchmark.rs` (bundle-layout
  consumption of the existing reference-inference harness; fail-closed gate
  mode; loader tokenizer-dir decoupling)
- `upstream/frontend/src-tauri/src/model_bundle.rs` (one gated test reusing
  `parse_manifest` + `verify_artifacts` for the staged-bundle assertion)
- `upstream/docs/hybrid-rag/task-1.r1-active-ci.md` (this report)

Untouched as instructed: nested `upstream/.github/workflows/*`,
architecture/README/sprint documents, `docs/notes-chat-improvement-execution.md`,
Task 1.4 benchmark, manifest/licenses/staging script, all production code paths.

## Scope omissions

- `1.R2` (manifest/provenance hardening, unexpected-file rejection) and `1.R3`
  (vector rebuild RAM/recovery remeasurement) - explicitly out of scope.
- Installed-package inference (Sprint 5).
- macOS/Linux workflows remain out of platform scope.
- The build-job cargo gates compile in debug profile into `upstream/target`;
  the later Tauri step deletes that directory, so debug test artifacts do not
  persist across CI runs (compile cost paid each run). Not optimized further:
  correctness of the gate order takes priority, and `swatinem/rust-cache`
  still caches whatever exists at job end.

## Rollback

Revert the root workflow commit to restore floating `stable` and remove the
gate block (CI-only change; no runtime effect). Revert the two Rust files to
restore the exact prior harness/validator behavior - the new test skips
without its env var, and `MEETLY_RAG_BUNDLE_DIR` mode disappears. Delete
`%LOCALAPPDATA%\meetily\model-cache` to drop the CI-shaped local cache.

## Blockers

None blocking. Residual risks, stated openly: the sed/bash and pwsh snippets
run for the first time on a real runner (both are minimal, standard-tool code
with loud failure modes); HF revision-resolve URL availability remains a
network dependency of staging exactly as accepted in Task 1.5.
