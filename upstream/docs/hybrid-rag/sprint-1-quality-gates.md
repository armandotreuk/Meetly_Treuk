# Sprint 1: Retrieval Quality Gates

## Status

Awaiting approval

## Goal

Remove known correctness defects and make every irreversible hybrid-RAG choice
from reproducible evidence. At sprint close, the project must have an approved
evaluation corpus, embedding/reranker model pair, chunk profile, vector backend,
resource envelope, and verified model-bundle manifest. Sprint 2 must not guess
any of these contracts.

## Architecture Authority

All work follows [`architecture.md`](architecture.md). A task stops and reports
a blocker when it cannot satisfy that document without changing an approved
product decision.

## Scope

### In Scope

- Fix the today-meeting-list intersection bug.
- Make generic context builders report exactly retained evidence.
- Create a private-safe Portuguese/English retrieval evaluation corpus and
  deterministic scoring harness.
- Benchmark and select one bundled multilingual embedding model and one local
  cross-encoder reranker.
- Benchmark exact vector search and a pure-Rust ANN candidate at 250,000
  documents when exact search misses the gate.
- Select a semantic chunk profile and all-summary-template policy.
- Define the pinned model manifest, licenses, hashes, and reproducible artifact
  fetch/verification process.
- Reconcile the declared Rust toolchain requirement with ONNX Runtime.

### Out Of Scope

- Production semantic schema or background indexing.
- Production vector retrieval.
- Chat hybrid integration.
- Shipping model binaries in the installer.
- Sidebar/MCP semantic behavior.
- Remote embedding APIs.

## Current State And Evidence

- `frontend/src-tauri/src/api/chat.rs:501-532` can compute both today filtering
  and meeting-list intent, but the list branch does not apply the today-ID set.
- `frontend/src-tauri/src/export/context.rs:139-208` returns only Markdown for
  broad context, while `api/chat.rs:573-585` constructs sources before final
  prompt truncation.
- `frontend/src-tauri/src/database/repositories/fts.rs:578-599` turns every raw
  query token, including high-frequency Portuguese function words, into FTS
  terms.
- `frontend/src-tauri/Cargo.toml:114` includes `ort 2.0.0-rc.10`, but there is
  no text tokenizer or retrieval benchmark.
- `frontend/src-tauri/Cargo.toml:9` declares Rust 1.77 while the resolved ORT
  wrapper declares a newer minimum Rust version.
- Existing model downloads do not provide the hash-verified atomic package
  contract required for bundled retrieval assets.
- The live user database diagnosis measured approximately 12,254 searchable
  rows; committed tests must use synthetic/private-safe fixtures, not that
  database or raw meeting content.

## Sprint Requirements

- The evaluation harness MUST score meeting ranking and evidence retention,
  not only final model wording.
- Evaluation fixtures MUST contain no private raw transcripts, API keys, paths,
  or personal identifiers.
- The reference WhatsApp case MUST retain its factual structure while using
  synthetic meeting IDs/names/content suitable for source control.
- Model selection MUST record exact revisions, licenses, dimensions,
  tokenizer, pooling, normalization, prefixes, ONNX export source, artifact
  hashes, package size, RAM, latency, and quality metrics.
- Model selection MUST test Portuguese and English.
- Reranker selection MUST be evaluated separately from embedding recall.
- The vector-backend decision MUST use 12k, 50k, and 250k synthetic scales.
- Benchmark commands and corpus generation MUST be reproducible.
- The sprint MUST publish a user-approved numeric quality/resource gate table;
  qualitative phrases such as "improves" are not sufficient acceptance.
- The lexical core-term/high-frequency-word policy, RRF limits, meeting-
  aggregation constants, scheduler limits, and ORT thread cap MUST be recorded
  as measured decisions before Sprint 2.
- No task may add a remote service or native SQLite extension.

## Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 1.1 | Retrieval correctness | Fix today/list intersection, generic retained-source parity, and existing raw lexical query logging before semantic expansion. | M | Pending `worker-m` | None | Focused Rust tests prove date intersection/source parity and logs contain lengths/counts rather than query text. | Revert localized resolver/context/log changes; no data change. |
| 1.2 | Evaluation | Add a private-safe multilingual corpus, deterministic metrics, baseline runner, and the reference regression. | M | Pending `worker-m` | None | One command reports baseline Recall/MRR/evidence/fact metrics and fails a deliberately degraded ranking. | Test/fixture tooling only; remove without production effect. |
| 1.3 | Model selection | Benchmark and select the bundled multilingual embedding and reranker pair plus chunk policy. | L | Pending `worker-l` | 1.2 | Reproducible report identifies one approved pair satisfying quality, license, platform, RAM, and ONNX gates. | No production default changes before approval; remove benchmark artifacts. |
| 1.4 | Vector backend | Benchmark exact search and, only if needed, a pure-Rust HNSW candidate at 250k scale. | L | Pending `worker-l` | 1.3 | Report selects exact or exact+ANN and demonstrates the architecture performance/RAM gates. | Benchmark-only dependency/code can be removed; no persisted format ships. |
| 1.5 | Model supply chain | Implement the small bundle manifest and reproducible hash/license verification pipeline; reconcile Rust MSRV. | M | Pending `worker-m` | 1.3 | Valid artifacts pass; tampered/missing artifacts fail before packaging; toolchain contract is explicit and checked. | Remove additive manifest/fetch verification; no runtime behavior yet. |

## Dependency Order

`1.2 -> 1.3 -> 1.4`

`1.3 -> 1.5`

`1.1` and `1.2` are independent if the evaluation harness is kept outside
`api/chat.rs` inline tests. Tasks `1.3` and `1.4` are L and run alone. Task
`1.5` may start after `1.3`, but should not run concurrently with `1.4` if both
need to change `Cargo.toml`, benchmark targets, or model artifact scripts.

## Task Specifications

### 1.1 - Retrieval correctness prerequisites [M]

**Outcome:** Existing lexical Chat has correct date/list semantics and an
evidence-retention contract that hybrid retrieval can reuse.

**Likely touchpoints:**

- `frontend/src-tauri/src/api/chat.rs`
- `frontend/src-tauri/src/export/context.rs`
- Existing Rust tests in those modules

**Required implementation:**

- When a query requests both today's meetings and a meeting list, pass the
  computed today meeting IDs into title-list resolution or otherwise intersect
  titles before formatting.
- Preserve folder and search-snapshot scope restrictions during that
  intersection.
- Generalize the generic context builder to return Markdown plus stable
  retained chunk/evidence IDs.
- Build `ChatSource` values after generic context retention, not from the full
  pre-truncation retrieval vector.
- Account for final prompt budgeting. If `assemble_prompt` can remove evidence
  after the context builder, move or expose budgeting so retained IDs represent
  the actual final prompt.
- Preserve single-meeting source parity and live-source behavior.
- Replace full-query INFO logging in `api_search_fts`,
  `api_search_transcripts`, and any remaining current lexical search path with
  privacy-safe query length/mode/result counts.
- Do not introduce semantic types or dependencies in this task.

**Acceptance criteria:**

- A today+list query in all scope lists only meetings from the current local
  date.
- The same behavior holds inside a recursive folder scope.
- Search-snapshot membership remains an allow-list.
- Generic context truncation emits only sources whose content appears in the
  context delivered to the model.
- Final prompt overhead cannot silently remove a source-backed chunk while the
  source remains emitted.
- Existing saved-meeting source-parity tests still pass.
- Captured/static log assertions prove raw Portuguese/English query text does
  not reach INFO logs in existing lexical Tauri/Chat paths.
- No schema, IPC, or persisted-source compatibility change is introduced.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib export::context::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** State exactly where the final source-retention
decision now occurs and identify every Chat caller affected.

### 1.2 - Evaluation corpus and harness [M]

**Outcome:** Retrieval quality can be compared deterministically before and
after each architecture increment.

**Likely touchpoints:**

- New fixture directory under `frontend/src-tauri/tests/fixtures/`
- New integration test or benchmark/evaluation target under
  `frontend/src-tauri/tests/` or `frontend/src-tauri/benches/`
- Minimal dev-only dependencies only when standard library/Serde cannot cover
  the requirement

**Required fixture schema:**

Each case records:

```text
case ID
language
question
optional history/rewritten query
scope and allowed meeting IDs
synthetic meetings and evidence documents
expected meeting IDs and order constraints
required evidence IDs or required facts
forbidden facts when distractors conflict
answer mode category
```

**Required case categories:**

- Portuguese WhatsApp schedule reference case.
- Exact numbers/dates/names.
- Portuguese and English paraphrases with weak lexical overlap.
- Similar-topic distractor meetings.
- Summary-only, notes-only, and transcript-only answers.
- Multi-meeting synthesis.
- Follow-up query rewriting.
- Folder, all, meeting, snapshot, and today allow-lists.
- Deleted, dirty, and stale-derived candidates.

**Required metrics:**

- Meeting Recall@1, Recall@3, Recall@5.
- Mean reciprocal rank.
- Evidence Recall@K.
- Required-fact coverage from retained evidence.
- Forbidden-fact contamination.
- Citation/source precision when a context builder is available.
- Stage latency hooks, even if model stages are absent initially.

**Acceptance criteria:**

- Fixtures contain no copied private transcript text or real user identifiers.
- Baseline FTS results are recorded and reproducible.
- The reference case expects the complete day schedule and MPV distinction.
- The harness fails when the correct meeting is moved below the required rank.
- The harness fails when required evidence is removed despite correct meeting
  ranking.
- Corpus loading, metric computation, and output ordering are deterministic.
- The task publishes numeric pass/fail gates with corpus counts for every
  metric required by `architecture.md`; default floors may change only with
  explicit user approval based on baseline evidence.
- The task publishes the evaluated lexical normalization/core-term policy and
  high-frequency-word list/algorithm; it must cover Portuguese and English and
  preserve exact names/numbers.
- One documented command runs the evaluation locally.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

The exact test target name may differ; the worker must document the final
single command.

**Worker report additions:** List every case ID/category and explain how the
reference private case was syntheticized.

### 1.3 - Embedding, reranker, and chunk selection [L]

**Outcome:** One exact, redistributable model pair and chunking contract are
approved for production implementation.

**Likely touchpoints:**

- Reproducible benchmark target/scripts under `frontend/src-tauri/`
- `frontend/src-tauri/Cargo.toml` dev/benchmark dependencies if necessary
- This sprint decision log and task execution entry
- Small candidate metadata/manifests; no committed large model binaries

**Required candidates:**

- At least two multilingual bi-encoder families suitable for Portuguese and
  English.
- At least two multilingual cross-encoder/reranker candidates when licensing
  and ONNX availability permit.
- At least the transcript window profiles defined in `architecture.md`.
- Latest-summary-only versus labeled all-summary-template evidence.

**Required measurements:**

- Evaluation metrics from Task 1.2 for embedding retrieval.
- Reranker pairwise accuracy or NDCG on the reranker subset.
- Query and document inference latency.
- Batch throughput.
- Peak model-session RAM.
- Artifact and installed resource size.
- Embedding dimensions and expected 250k vector memory.
- Portuguese/English regression breakdown.
- Held-out hybrid simulations selecting RRF `k`/channel weights, concept/title/
  support meeting-aggregation constants, reranker candidate count, and reranker
  batch size without tuning only the reference case.
- CPU behavior on representatives of all supported architectures.
- License and redistribution evidence.
- Tokenizer, prefix, pooling, normalization, and quantization fidelity against
  a trusted reference implementation.

**Implementation constraints:**

- Use the existing ORT major/version contract unless evidence shows it cannot
  load an approved model.
- Do not select a model whose exact tokenizer/preprocessing cannot be
  reproduced in Rust.
- Do not infer redistribution rights from an upstream repository being public.
- Do not commit model weights.
- Record failed candidates and why they were rejected.

**Acceptance criteria:**

- One embedding and one reranker model/revision are selected.
- The pair meets the Task 1.2 approved numeric semantic-category delta/floor
  without violating exact-term/number no-regression gates after planned fusion.
- The reference case retrieves the correct meeting and complete evidence under
  at least one approved chunk profile.
- All required manifest fields are known and immutable.
- Licenses permit bundling in the distributed application.
- Model pair plus 250k vector estimate can meet the approved RAM envelope or a
  documented vector encoding/backend path exists for Task 1.4.
- RRF/channel, meeting-aggregation, reranker candidate/batch, and ORT intra-op
  thread values are selected with numeric evidence and recorded in an approved
  architecture addendum.
- A reference sentence/query produces stable expected outputs for future
  packaged smoke tests.
- Windows x64, macOS ARM64, and Linux x64 development/CI runners execute actual
  tokenizer, embedding, and reranker reference inference before Sprint 2.
- Embedding and reranker preprocessing are recorded independently, including
  tokenizer revision, pair formatting, input/output names/dtypes, truncation,
  output-label interpretation, and score transform.
- Architecture review approves the selection evidence.

**Required verification:**

The worker defines reproducible commands for each selected target and then
runs at minimum:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Include a candidate comparison table, exact model
IDs/revisions, license URLs/files, artifact hashes if available, rejected
alternatives, and the recommended chunk policy.

### 1.4 - Vector backend benchmark [L]

**Outcome:** The project has a measured search backend for 250,000 documents,
not an assumed dependency choice.

**Likely touchpoints:**

- Reproducible benchmark target and synthetic corpus generator
- Temporary or dev-only pure-Rust ANN dependency only if exact misses its gate
- Sprint decision log

**Required benchmark matrix:**

- 12,000, 50,000, and 250,000 vectors.
- Selected production dimension and encoding.
- Cold load and warm query.
- Global all-meetings query.
- Narrow folder/snapshot allow-list query.
- Update/delta and full rebuild cost if ANN is evaluated.
- Exact base+delta/tombstone update and compaction cost.
- Reader-held old snapshot plus new/shadow snapshot/model-session peak RAM.
- Crash window between canonical SQLite commit and memory/sidecar publication.
- Single and bounded concurrent queries.
- Candidate limits, vector-scan concurrency, interactive queue size/permits,
  exact/ANN compaction threshold, and index-worker scheduling impact.
- Peak RSS and on-disk size.
- Recall against exact nearest neighbors for ANN.

**Decision rule:**

- Select exact-only when it satisfies the approved p95/RAM gates at 250k.
- Evaluate and select exact+HNSW only when exact misses a gate.
- Reject any ANN candidate that cannot be persisted/rebuilt safely on all
  platforms or whose measured recall harms the evaluation corpus.

**Acceptance criteria:**

- Benchmark generation is deterministic and does not allocate unbounded test
  data.
- Results include p50/p95, load time, RAM, disk, and recall.
- Narrow scope filtering cannot return out-of-scope documents.
- The selected backend has a documented update and crash-recovery strategy.
- Meeting updates do not synchronously copy/rebuild all 250k base vectors.
- Peak old/new snapshot, delta/tombstone, and active model-session RAM is at or
  below 1 GiB, or a measured 1-1.25 GiB result is explicitly approved before
  selection; above 1.25 GiB fails without a product scope change.
- Any added ANN library has acceptable license, maintenance, and all-target
  build evidence.
- Vector candidate limits, scan permits, interactive queue limit, index
  compaction threshold, and scheduler policy are recorded in the architecture
  addendum; unresolved values block Sprint 2.
- The final backend decision is recorded in `architecture.md` by a dated,
  approved addendum before Sprint 2 implementation.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

The benchmark command and output artifact are mandatory worker-report items.

### 1.5 - Bundle manifest, artifact verification, and MSRV [M]

**Outcome:** Retrieval model artifacts can be fetched reproducibly and rejected
before packaging when altered, missing, unlicensed, or incompatible.

**Likely touchpoints:**

- Small checked-in manifest under `frontend/src-tauri/` resources/config
- Build or CI script for pinned artifact acquisition and hash verification
- `.github/workflows/build-{windows,macos,linux}.yml`
- Root/frontend Rust toolchain or manifest metadata
- License-resource configuration

**Required implementation:**

- Encode every field required by `architecture.md`.
- Encode separate complete tokenizer/preprocessing/input/output contracts for
  embedding and reranker models, unless both explicitly reference an identical
  shared tokenizer identity.
- Fetch into a build cache/staging location, never directly into a final signed
  package directory.
- Verify byte length and SHA-256 before publication.
- Verify all required artifacts as one package.
- Include exact license text/attribution in packaged resources.
- Add runtime lazy length/SHA-256 verification before first model load.
- Define whether CI or a release-preparation command fetches artifacts.
- Resolve the mismatch between the project's declared Rust 1.77 and the
  resolved ORT wrapper's newer MSRV using an explicit tested toolchain policy.
- Add no runtime model download behavior.

**Acceptance criteria:**

- Valid pinned artifacts pass on each target workflow path.
- One-byte corruption fails verification.
- Missing tokenizer/model/license fails verification.
- Unknown manifest version fails closed.
- Wrong input/output name, dtype, label index, pair format, or tokenizer
  reference fails validation.
- Model files remain excluded from normal Git history.
- The selected Rust toolchain is explicit and exercised by CI.
- No application startup code depends on model artifacts yet.

**Required verification:**

```powershell
# Worker supplies the final artifact verification command.
pnpm --dir "frontend" run typecheck
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Record artifact provenance, cache behavior,
license packaging, CI impact, and the exact Rust toolchain decision.

## Sprint Acceptance Criteria

- All five task acceptance sets pass.
- `architecture.md` contains approved addenda for selected models, chunking,
  vector backend, limits, and toolchain.
- Evaluation baseline and selected-model results are reproducible.
- No private corpus content is committed.
- No production semantic schema or runtime retrieval is introduced early.
- Full Rust library tests, typecheck, Vitest, Cargo check, rustfmt, and diff
  checks pass.
- Code review and architecture review return Approved with no unresolved
  blocker or should-fix finding.

## Risks And Mitigations

- **Benchmark overfitting:** require multilingual categories and held-out cases,
  not only the reported WhatsApp question.
- **Model license ambiguity:** treat unclear redistribution as a blocker.
- **Reference mismatch:** compare Rust tokenization/outputs with trusted model
  reference vectors.
- **Installer growth:** report exact package size even though no strict total
  asset limit was selected.
- **ANN complexity without need:** exact must fail a measured gate before ANN is
  selected.
- **Private fixture leakage:** use synthetic text and review fixture diffs.
- **CI artifact drift:** immutable revisions plus SHA-256, never floating URLs.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-21 | Sprint 1 is a quality gate and does not ship semantic retrieval. | Prevent foundational model/index guesses from leaking into production architecture. | Implement a chosen model immediately. | Main agent, pending sprint approval |
| 2026-08-21 | Use synthetic source-controlled evaluation fixtures. | Preserve privacy while retaining factual retrieval structure. | Commit a copy of the live user database. | Main agent, pending sprint approval |
| 2026-08-21 | Exact search must be benchmarked before ANN. | Current scale is small and deletion is cheaper than a speculative subsystem. | Adopt sqlite-vec/HNSW immediately. | Main agent, pending sprint approval |

## Task Execution Log

<!-- Append one immutable entry per completed, blocked, or cancelled task. -->

### Task Entry Template

```markdown
### <Task ID> - <task name>

**Status:** Complete | Blocked | Cancelled
**Owner:** `<subagent type>` (`<task/session ID>`)
**Completed:** YYYY-MM-DD
**Implemented:**
- ...
**Implementation:**
- Files: `...`
- Approach: ...
**Not implemented:**
- ... or `None.`
**Why not implemented:**
- ... or `Not applicable.`
**Verification:**
- `<command>` - pass/fail and result.
**Rollback:**
- ...
**Decisions and follow-ups:**
- ...
```

## Sprint Reviews

### Code Review

**Reviewer:** Pending
**Verdict:** Pending
**Findings:** Pending
**Required follow-ups:** Pending

### Architecture Review

**Required because:** Model supply chain, new dependencies, retrieval algorithm,
cross-platform native runtime, and a decision that constrains every later
sprint.

**Reviewer:** Pending
**Verdict:** Pending
**Findings:** Pending

## Approval Gates

- User approval of this PRD is required before creating the Sprint 1 TODO list.
- User approval of each dependency-ready batch is required before dispatch.
- User approval is required for the exact production model pair and any ANN
  dependency before Sprint 2.
- Sprint-close approval is required before Sprint 2 begins.
