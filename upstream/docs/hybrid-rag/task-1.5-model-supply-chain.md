# Task 1.5 - Bundle Manifest, Artifact Verification, and MSRV Reconciliation

**Status:** Complete
**Owner:** implementation subagent (`ox-alpha`)
**Completed:** 2026-08-25
**Scope:** Reproducible supply-chain preparation for the approved Sprint 1
bundle: checked-in production manifest, pinned fetch/stage/publish pipeline,
Rust manifest validation plus lazy artifact verifier (no startup integration),
offline tests, and reconciliation of the declared Rust 1.77 MSRV against the
locked dependency tree. No model substitution, no runtime download, no
macOS/Linux coverage claim, no Sprint 2 retrieval integration.

## Approved bundle encoded

Exactly the approved contract from `architecture.md` ("Approved Sprint 1
Bundle And Runtime Contract"), no substitutions:

| Component | Pinned source |
|---|---|
| Bi-encoder weights | `intfloat/multilingual-e5-base` @ `d128750597153bb5987e10b1c3493a34e5a4502a` |
| Bi-encoder dynamic-int8 ONNX export | `Xenova/multilingual-e5-base` @ `1ec9243030a27d1a115d5c340572074c125b58b2` |
| Reranker + quint8_avx2 ONNX export | `cross-encoder/mmarco-mMiniLMv2-L12-H384-v1` @ `1427fd652930e4ba29e8149678df786c240d8825` |

## Artifact provenance (URL / revision / length / SHA-256)

All files were downloaded from the pinned revision-resolve URLs and hashed
locally with SHA-256. Local hashes match both the Hugging Face LFS OIDs served
for those revisions and the Task 1.3 evidence manifest where one exists.

| Manifest path | Bytes | SHA-256 | Source |
|---|---|---|---|
| `models/embedding/model_int8.onnx` | 278184162 | `9ddfd8b45086dabc59a7e1bb00463225dace8954962418b240840f2153bc87da` | `https://huggingface.co/Xenova/multilingual-e5-base/resolve/1ec9243030a27d1a115d5c340572074c125b58b2/onnx/model_int8.onnx` |
| `tokenizers/embedding/tokenizer.json` | 17082660 | `62c24cdc13d4c9952d63718d6c9fa4c287974249e16b7ade6d5a85e7bbb75626` | same repo/revision `/tokenizer.json` |
| `tokenizers/embedding/tokenizer_config.json` | 418 | `efb5c0d09722e5fe59a462cd2a9976ee216d55b037597d997cd3fe833216da15` | same repo/revision |
| `tokenizers/embedding/special_tokens_map.json` | 280 | `06e405a36dfe4b9604f484f6a1e619af1a7f7d09e34a8555eb0b77b66318067f` | same repo/revision |
| `models/reranker/model_quint8_avx2.onnx` | 118620016 | `6c2513767fb63d008a4377bef7a7a3555433d9436342bb53e35a3a72ffc52d4b` | `https://huggingface.co/cross-encoder/mmarco-mMiniLMv2-L12-H384-v1/resolve/1427fd652930e4ba29e8149678df786c240d8825/onnx/model_quint8_avx2.onnx` |
| `tokenizers/reranker/tokenizer.json` | 17082660 | `62c24cdc13d4c9952d63718d6c9fa4c287974249e16b7ade6d5a85e7bbb75626` | reranker repo/revision |
| `tokenizers/reranker/tokenizer_config.json` | 435 | `e7fbfbfa6347b4e414c1cee50d142e2c2f9a895dad68b068ae83a8b564c3837e` | reranker repo/revision |
| `tokenizers/reranker/special_tokens_map.json` | 239 | `378eb3bf733eb16e65792d7e3fda5b8a4631387ca04d2015199c4d4f22ae554d` | reranker repo/revision |
| `licenses/e5-base-MIT.txt` | 1078 | `b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5` | SPDX canonical MIT text @ `spdx/license-list-data` commit `f3c81d77d1947e091e973637e32074f005b0967a` (also checked in) |
| `licenses/mmarco-mMiniLMv2-Apache-2.0.txt` | 11358 | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` | canonical Apache-2.0 text (apache.org) |

Cross-evidence:

- The embedding export is **`onnx/model_int8.onnx`**, not `model_quantized.onnx`.
  Its hash equals the hash recorded in the Task 1.3 evidence manifest
  (`tests/fixtures/model_bundle_manifest.json`,
  `sha256-9ddfd8b4...`), so the packaged bytes are the exact session that
  produced the recorded reference expectations and benchmark figures.
- The reranker byte length equals the fixture's measured
  `sessionArtifactBytes.quint8Avx2` (118620016).
- Both models carry byte-identical `tokenizer.json` (same XLM-R unigram vocab),
  but their complete identities differ (different tokenizer revisions,
  different config/special-token maps). Per the architecture rule ("MAY share a
  tokenizer only when complete identities match"), the two tokenizer contracts
  are encoded **separately**.

License behavior: neither upstream repository ships a standalone LICENSE file;
both declare their licenses in immutable pinned model-card metadata
(`license: mit` verified via HF API at `d12875...`; `license: apache-2.0` at
`1427fd65...`). The package therefore ships the canonical license texts (SPDX
MIT template including its `<year> <copyright holders>` placeholders, and the
canonical Apache-2.0 text) plus per-model attribution and pinned-resource URLs
in the manifest `licenses[]`. Every artifact carries a reproducible origin:
the eight model/tokenizer entries pin revision-resolve Hugging Face URLs (no
floating refs), and both license texts are checked into
`resources/retrieval/licenses/` as exact sources - the staging script prefers
those checked-in copies (hash-verified) over any network fetch. This satisfies
MIT notice retention and the Apache-2.0 "provide a copy of the license"
requirement; noted openly rather than silently.

## Cache, staging, and publication behavior

`frontend/src-tauri/scripts/stage-retrieval-models.ps1` (Windows PowerShell,
the only active release platform):

1. Loads the manifest and fails fast before any network use on: unknown
   `manifestVersion`, missing `bundleId`, unsafe paths, malformed SHA-256,
   non-positive lengths, duplicate paths.
2. Fetches each artifact into `%LOCALAPPDATA%\meetily\model-cache\<bundleId>\`
   (never into final resources), skipping entries whose cached copy already has
   matching length + SHA-256. On a cache miss it uses the checked-in exact
   source for license texts, otherwise the pinned revision-resolve URL; every
   download is length/hash-verified before being moved into place.
3. Copies the full artifact set into `.staging-<guid>` outside the final
   package directory, copies the manifest itself in, preserves committed
   non-artifact files (the bundle README), then re-verifies every entry
   (existence, byte length, SHA-256) as one package inside staging.
4. Publishes atomically by renaming the previous `bundle\` aside and renaming
   staging into `resources/retrieval/bundle\` (two same-volume renames; the
   window between them is documented in-script as an accepted single-publisher
   simplification).
5. Cleans stale `.staging-*` / `.bundle-backup-*` leftovers from crashed runs.

CI (`build-windows.yml`) gains: a model-cache step keyed on the manifest hash
and a "Stage and verify retrieval model bundle" step ahead of the Tauri build.
`tauri.conf.json` packages `resources/retrieval/bundle` (directory walk),
`model-bundle.manifest.json`, and `licenses` as application resources. A
committed `bundle/README.md` keeps the directory present in Git so builds work
before the first staging run; everything else under `bundle/` is Git-excluded
(root `.gitignore` additions), so no model/tokenizer binary enters history.

## Rust validation and lazy verification

New module `frontend/src-tauri/src/model_bundle.rs` (registered in `lib.rs`,
not referenced by any startup path):

- `parse_manifest` deserializes strictly (`deny_unknown_fields`) and validates:
  version gate (only v1 accepted, anything else fails closed), required
  embedding fields (dimensions, max sequence, prefixes, masked-mean pooling
  description, l2 normalization), the exact approved tensor sets - same count
  as required, every name present exactly once, and exact dtypes (`input_ids`
  and `attention_mask` must be `int64`; embedding `last_hidden_state` and
  reranker `logits` must be `float32`; extra or unexpected tensors fail) -
  approved pair format, bounded label index over a float32 label output,
  tokenizer revisions pinned to their model's ONNX export/model revision,
  mandatory nonempty provenance URLs on every model/tokenizer artifact and
  license entry, known quantization/score-transform/truncation-side/
  normalization values, safe relative paths, 64-hex lowercase digests, global
  path uniqueness, and the tokenizer-sharing coherence rule (equal
  type+revision forces equal artifact sets). Licenses fail closed unless SPDX
  is MIT or Apache-2.0 with attribution, resource, a hashed license artifact,
  and exactly one coverage entry per component (missing or duplicate
  `embeddingModel`/`rerankerModel` coverage fails).
- `verify_artifacts(bundle_root)` lazily streams every artifact (models,
  tokenizers, licenses) checking existence, byte length, and SHA-256 before
  first model load (Sprint 2 call site), so post-install corruption cannot
  reach ONNX Runtime.
- `sha2 = "0.10"` added as a direct dependency; `sha2 0.10.9` was already in
  `Cargo.lock` transitively, so no new package entered the tree.

## Offline tests (no downloads)

`cargo test --lib model_bundle`: 19 focused tests, all passing - valid bundle
parses and verifies end-to-end with the exact approved dtypes; one-byte
corruption, wrong byte length, every missing-artifact class
(model/tokenizer/license) fail verification; unknown version, malformed
SHA-256, unknown dtype, known-but-wrong input/output dtypes (`input_ids: int32`,
`last_hidden_state: int64`, `logits: int32`), extra/unexpected tensor names,
wrong input name, out-of-bounds label index, wrong pair format, unrelated
tokenizer revisions (either model), missing or empty artifact provenance URLs,
duplicate and missing per-component license coverage, tokenizer path ownership
violation, and incoherent shared-tokenizer identity fail validation; plus a
production-manifest test asserting the checked-in file still encodes the exact
approved constants (IDs, revisions, pinned tokenizer revisions equal to their
export/model revisions, 768 dims, prefixes, int8/quint8_avx2 exports, MIT +
Apache-2.0, 10 managed artifacts, separate tokenizer identities).

## MSRV decision and evidence

The declared `rust-version = "1.77"` was false. Evidence:

- `ort` / `ort-sys` 2.0.0-rc.10 (locked) declare `rust-version = "1.81"`
  (crates.io registry manifests, checksums matching `Cargo.lock`) - the
  mismatch named by the architecture doc.
- A mechanical scan of every registry-cached crate in `Cargo.lock` found
  higher floors elsewhere: `time 0.3.47`, `plist 1.9.0`, `darling 0.23.0`,
  `serde_with 3.20.0`, `home 0.5.12` require **1.88**; `icu_* 2.2` require
  1.86; `uuid 1.23.1` and `clap 4.6.1` require 1.85. The locked tree cannot
  build below 1.88 regardless of ORT.
- Building with the chosen toolchain upgrades `Cargo.lock` to lockfile format
  v4 (cargo >= 1.83), independently reinforcing a floor well above 1.77/1.81.

Decision: declare the true floor and exercise exactly that toolchain.

- `rust-version = "1.88"` in the workspace root and member manifest (honest
  minimum; not floating, not stale).
- New root `rust-toolchain.toml` pins `channel = "1.88.0"` (minimal profile +
  clippy/rustfmt) - an exact version, no `stable`.
- Windows CI installs `dtolnay/rust-toolchain@master` with `toolchain: 1.88.0`
  (replacing unpinned `@stable`) and asserts `rustc --version` equals the
  channel parsed from `rust-toolchain.toml`, so the workflow exercises the
  decision rather than assuming it.
- All local verification below ran under 1.88.0 (rustup override confirmed via
  `rust-toolchain.toml`), including the full dependency-tree `cargo check` -
  empirical proof that the pinned floor compiles the locked tree end to end.

## Verification commands and results

Run from `upstream/` on Windows x64 (Intel reference machine):

| Command | Result |
|---|---|
| Warm-cache staging run (offline verification command): `./frontend/src-tauri/scripts/stage-retrieval-models.ps1` | PASS - 10/10 artifacts length+SHA-256 verified from cache (411 MiB), staged outside resources, package re-verified, published atomically to `resources/retrieval/bundle` |
| Fresh-cache reproducibility proof: same script with an empty temporary `-CacheRoot` | PASS - all 8 model/tokenizer artifacts re-fetched from their pinned revision-resolve URLs, both licenses served from the checked-in exact source, full length+SHA-256 verification of each download, staged package re-verified, published atomically (exit 0); the temporary cache root was then deleted |
| `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib model_bundle` | PASS - 19 passed, 0 failed |
| `pnpm --dir "frontend" run typecheck` | PASS (`tsc --noEmit`, clean) |
| `$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"; cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` | PASS under rustc 1.88.0 (full locked tree) |
| `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` | PASS |
| `npx vitest run` (in `frontend/`) | PASS - 20 files, 95 tests |
| `git diff --check` | PASS (exit 0; CRLF normalization warnings are pre-existing repo-wide behavior) |
| `npx --yes yaml-lint .github/workflows/build-windows.yml` | PASS |

Observation (spillover, not introduced here): `cargo clippy --lib` exits 101 on
this workspace due to a pre-existing deny-level correctness finding in existing
audio code (inherent `to_string` on `AudioCaptureBackend`); HEAD without my
changes behaves identically, and `model_bundle.rs` contributes zero findings.

## Changed files

- `frontend/src-tauri/resources/retrieval/model-bundle.manifest.json` (new, committed)
- `frontend/src-tauri/resources/retrieval/licenses/e5-base-MIT.txt` (new, committed)
- `frontend/src-tauri/resources/retrieval/licenses/mmarco-mMiniLMv2-Apache-2.0.txt` (new, committed)
- `frontend/src-tauri/resources/retrieval/bundle/README.md` (new, committed placeholder)
- `frontend/src-tauri/scripts/stage-retrieval-models.ps1` (new)
- `frontend/src-tauri/src/model_bundle.rs` (new)
- `frontend/src-tauri/src/lib.rs` (module registration only)
- `frontend/src-tauri/Cargo.toml` (rust-version 1.88, sha2 dep)
- `Cargo.toml` (workspace rust-version 1.88)
- `Cargo.lock` (sha2 direct edge; lockfile v3->v4 written by cargo 1.88)
- `rust-toolchain.toml` (new, root)
- `.gitignore` (bundle artifact exclusions)
- `frontend/src-tauri/tauri.conf.json` (three retrieval resource entries)
- `.github/workflows/build-windows.yml` (pinned toolchain + assertion, model cache, staging step)

Untouched as instructed: `docs/hybrid-rag/architecture.md`,
`sprint-1-quality-gates.md`, `docs/hybrid-rag/README.md`,
`docs/notes-chat-improvement-execution.md`, Task 1.4 files, application code.

## Omissions (deliberate)

- No runtime model download anywhere; no macOS/Linux workflow or support
  claims (Windows x64 only, per Platform Scope).
- `sentencepiece.bpe.model` is not packaged: the runtime tokenizer contract is
  `tokenizer.json` (+config/special-token maps); adding the slow-tokenizer
  fallback would add another pinned binary with no consumer. Recorded here so
  the reviewer can request it if "complete" is interpreted more broadly.
- ONNX-graph-level input/output name/dtype conformance is enforced at manifest
  level now; verifying the loaded session against those declarations happens
  where sessions are constructed (Sprint 2), since it requires ORT.
- `scoreTransform` is encoded as `"identity"` per the architecture's required
  manifest template (monotonic with the benchmark's sigmoid; ordering-equivalent).

## Rollback

Revert the commit(s) carrying these files and delete
`frontend/src-tauri/resources/retrieval/` plus
`%LOCALAPPDATA%\meetily\model-cache\` locally; remove the three resource
entries from `tauri.conf.json`. Nothing else consumes the module, script, or
resources yet, so removal is clean. Reverting only the toolchain pin while
keeping the lockfile requires cargo >= 1.83 (lockfile v4) and would resurrect
the false-MSRV problem.

## Blockers / risks

- None blocking acceptance. Risks recorded: first release build on any machine
  must run the staging script (CI does; local `tauri build` without staging
  fails loudly at packaging rather than shipping an empty bundle - intended
  fail-closed behavior); the publish swap is single-publisher by design; HF
  revision-resolve URLs are the immutability anchor, and the CI cache key
  changes automatically if the manifest ever changes.
- Process note: a mid-session `git stash -u` failed partway (OneDrive file
  lock on the licenses directory) leaving a redundant stash entry; worktree
  integrity was verified blob-by-blob against the stash and the duplicate was
  dropped. The pre-existing unrelated stash (`enhance/meeting-date-display`)
  was left untouched. No content was lost.
