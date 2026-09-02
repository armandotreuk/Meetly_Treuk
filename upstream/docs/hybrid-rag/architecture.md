# Local Hybrid RAG Architecture

## Document Control

| Field | Value |
|---|---|
| Status | Approved by the user on 2026-08-22 |
| Date | 2026-08-21 (revised 2026-08-21 after pre-implementation critique) |
| Owner | Main orchestration agent |
| Product | Meetily desktop application |
| Platforms | **Release target: Windows x64 only.** macOS ARM64 and Linux x64 are deferred (see "Platform Scope"). |
| Related records | `docs/sprint-6-1-contextual-chat.md`, `../../../ROADMAP.md`, sprint files in this directory |

This document is normative. `MUST`, `MUST NOT`, `SHOULD`, and `MAY` describe
implementation requirements. A subagent may not weaken a `MUST` or `MUST NOT`
without a recorded user-approved architecture change.

## Product Outcome

Folder and all-meetings Chat must answer detailed questions with the same
completeness as saved-meeting Chat while remaining local, scope-safe,
grounded, cancellable, and usable when semantic indexing is unavailable.

The reference failure is:

```text
quais os dias de comunicacao por whatsapp para o fluxo de retencao?
```

The correct evidence states that WhatsApp communication was discussed for days
`1, 3, 7, 10 and 15`, with different day-one semantics for MPV and non-MPV
locations. Existing broad retrieval returned isolated `3 days` and `4 days`
fragments because it ranked chunks globally and exposed only short FTS
snippets. The correct summary was present in storage but not in the snippet
shown to the model.

The architecture therefore selects relevant meetings first and then hydrates
their authoritative content. Vector similarity alone is not the product.

## Approved Product Decisions

| Decision | Approved choice | Consequence |
|---|---|---|
| Embedding execution | Local bundled | Historical meeting text is never sent to an embedding API. |
| Model footprint | Quality first, no strict packaged-asset total | Model selection is quality- and license-gated; installer size must still be reported. |
| Retrieval depth | Hybrid retrieval, local reranker, and iterative LLM retrieval | The design includes Fast and Deep paths. |
| Expected scale | Up to 250,000 semantic documents per user | A measured exact-search or ANN path must satisfy the release gate. |
| Scope rollout | Folder/all first, then all saved scopes | Live recording remains separate. |
| Platform rollout | **Windows x64 for this release; macOS/Linux deferred** | Packaged inference smoke tests are mandatory on Windows x64. Other targets require CI that does not exist in this fork. |
| Initial indexing | Automatic background backfill after launch | Startup and primary writes cannot wait for embeddings. |
| Chat quality mode | User-selectable Fast/Deep, Deep default | Deep adds bounded model calls before final answer streaming. |
| Runtime envelope | Quality-first 1 GiB RAM target with explicit escalation band | Model, active/shadow snapshots, deltas, and sessions are measured together; no silent release above target. The envelope is an arithmetic constraint on model dimension and vector encoding, not a preference. See "Resource Budget Arithmetic". |
| Derived disk envelope | 2 GiB steady-state target, 3 GiB peak during shadow rebuild | Derived chunk text plus vectors plus two retained generations are measured together; no silent release above target. |
| Product surfaces | Chat, sidebar search, Tauri context/search APIs, and MCP | External BM25 contracts require additive hybrid APIs rather than silent score changes. |
| Deleted meeting Chat | Keep answer text, scrub deleted-meeting source data | Preserve conversation value without retaining navigable/snippet source copies. |

## Platform Scope

This release targets **Windows x64 only**.

The original draft required packaged tokenizer, embedding, and reranker
inference on Windows x64, macOS ARM64, and Linux x64 before Sprint 2, and again
on installed packages in Sprint 5. That requirement was unsatisfiable:

- This fork's only active CI workflow is `.github/workflows/build-windows.yml`
  at the repository root.
- `upstream/.github/workflows/build-macos.yml` and `build-linux.yml` exist on
  disk but are nested inside `upstream/`, which GitHub Actions never reads.
  They have never executed for this fork and cannot execute without new
  root-level workflows.
- No macOS or Linux development hardware is recorded anywhere in this project.

Rather than declare a `MUST` that no gate could satisfy, the program ships
Windows x64 and defers the other targets explicitly.

Consequences:

- Every packaging, inference, and smoke gate in Sprints 1 and 5 applies to
  Windows x64 only.
- Model selection MUST NOT depend on a platform-specific ONNX export, operator
  set, or execution provider. Portability is preserved in the design even
  though it is not gate-verified.
- Model artifacts, tokenizer contracts, and license attribution remain
  platform-neutral so a later macOS/Linux enablement is additive.
- Release notes and any capability documentation MUST state that hybrid
  retrieval is verified on Windows x64 only. Do not claim macOS or Linux
  support without executing installed-package inference on that target.

Re-enabling a deferred platform requires: a root-level build workflow for that
target, the Sprint 1 reference-inference gate executed on it, and the Sprint 5
installed-package smoke executed on it. That is a scope change requiring
explicit user approval, not a task-level decision.

## Resource Budget Arithmetic

The 1 GiB retrieval RAM envelope is not a preference that model selection may
trade against quality. It is an arithmetic constraint that eliminates most
candidates before benchmarking begins. Sprint 1 MUST apply it as a pre-filter
so Task 1.3 only benchmarks models that Task 1.4 can accept.

Steady-state retrieval RAM is bounded below by:

```text
steady_bytes >=
      dimensions
    * bytes_per_value          (4 for f32, 2 for fp16, 1 for int8)
    * document_count           (release gate: 250,000)
    + embedding_session_bytes
    + reranker_session_bytes
    + delta_and_tombstone_bytes
```

During a shadow rebuild, the vector term uses a `snapshot_overlap_factor` of
2: one active snapshot and one building shadow snapshot. A reader retains an
`Arc` to the active snapshot and does not allocate a third vector copy. An
implementation that materializes a genuinely separate third snapshot MUST
measure and count it; it is not covered by this contract.

Worked values at the 250,000-document gate, vectors only, including the
mandatory 2x shadow-activation overlap:

| Dimensions | Encoding | Vector bytes at 250k | With 2x overlap | Headroom under 1 GiB for both model sessions |
|---|---|---|---|---|
| 384 | int8 | 96 MB | 192 MB | ~830 MB — comfortable |
| 384 | f32 | 384 MB | 768 MB | ~250 MB — tight, likely fails with a cross-encoder |
| 768 | int8 | 192 MB | 384 MB | ~640 MB — workable |
| 768 | f32 | 768 MB | 1536 MB | none — **exceeds the 1.25 GiB hard fail before any model loads** |
| 1024 | f32 | 1024 MB | 2048 MB | none — **fails** |

Derived steady-state admissibility rule, which Sprint 1 Task 1.3 MUST apply
before benchmarking a candidate:

```text
dimensions * bytes_per_value * 250000 * 2
  + embedding_session_bytes
  + reranker_session_bytes
  + delta_and_tombstone_bytes
  <= 1.30 GiB (transient rebuild only; explicit user approval required)

dimensions * bytes_per_value * 250000
  + embedding_session_bytes
  + reranker_session_bytes
  + delta_and_tombstone_bytes
  <= 1 GiB   (automatic pass)
  <= 1.25 GiB (requires explicit user risk approval)
  >  1.25 GiB (inadmissible; do not benchmark)
```

The transient rebuild ceiling is **1.30 GiB** only for the approved
two-snapshot e5-base int8 bundle and only while a shadow snapshot builds or
activates. It is not a new steady-state band, does not authorize a third
snapshot, and does not weaken the 1.25 GiB model-selection cap.

Consequences that Sprint 1 MUST treat as given rather than rediscover:

- A 768-dimension f32 bi-encoder is inadmissible at the 250,000-document gate.
  It may only be considered together with an approved quantized encoding.
- Quantization is therefore a first-class part of model selection, not a
  contingency. `retrieval_models.vector_encoding` exists for this reason and
  the document table MUST NOT hardcode an f32 byte width.
- If no admissible pair meets the quality gates, Sprint 1 stops for an
  architecture decision. The permitted levers are lower dimensionality,
  quantization, memory-mapping the base snapshot, or an approved reduction of
  the 250,000-document scale gate. Adding an ANN index is not a lever; see
  "Vector Search Backend".

## Current State And Evidence

- `frontend/src-tauri/src/api/chat.rs:382-612` is the shared Chat preparation
  funnel used by streaming, non-streaming, scoped, legacy, and MCP Chat paths.
- `frontend/src-tauri/src/api/chat.rs:532-572` gives ordinary saved-meeting Chat
  an authoritative summary/notes/transcript context path.
- `frontend/src-tauri/src/api/chat.rs:1032-1126` loads current notes, the latest
  non-empty summary, transcript hits, and adjacent transcript segments.
- `frontend/src-tauri/src/api/chat.rs:1218-1300` performs broad FTS retrieval
  for all and folder scopes.
- `frontend/src-tauri/src/database/repositories/fts.rs:92-322` implements FTS5
  retrieval and authoritative folder filtering.
- `frontend/src-tauri/src/database/repositories/fts.rs:362-500` refreshes and
  rebuilds the derived FTS projection. Refresh is non-transactional and is
  usually called as a best-effort post-commit hook.
- `frontend/src-tauri/src/export/context.rs:10-93` builds saved-meeting context
  and reports retained transcript IDs.
- `frontend/src-tauri/src/export/context.rs:139-208` builds generic broad
  context but returns only a string, so broad sources can overstate evidence
  removed by later truncation.
- `frontend/src-tauri/migrations/20260727000000_add_fts5.sql` indexes transcript,
  summary, and note text but not meeting titles.
- `frontend/src-tauri/Cargo.toml:114` already includes ONNX Runtime for
  Parakeet. No text embedding tokenizer or vector search dependency exists.
- `frontend/src-tauri/src/parakeet_engine/model.rs:89-143` provides an existing
  CPU ONNX session construction pattern.
- `frontend/src-tauri/src/database/manager.rs:17-50` opens SQLite and runs
  migrations before application state is installed.
- `frontend/src-tauri/src/state.rs:1-5` currently manages only the database
  manager, so retrieval state will be additive.
- `frontend/src-tauri/src/mcp/server.rs:133-233` exposes lexical search,
  context, and shared Chat preparation to localhost MCP clients.
- The repository root `.github/workflows/` contains only `build-windows.yml`.
  The macOS and Linux workflows under `upstream/.github/workflows/` are inert
  for this fork. This is the basis for the Windows-only platform scope.
- `frontend/src-tauri/tests/` and `frontend/src-tauri/benches/` do not exist.
  Sprint 1 creates the first integration-test target and MUST pin its name.
- `upstream/.cargo/config.toml` sets only `WHISPER_DONT_GENERATE_BINDINGS`. It
  does **not** set `CARGO_TARGET_DIR`, contrary to `MIGRATION.md`. Every
  verification command in this program sets it explicitly.

## Sprint 3 Baseline And Release-Gate Inheritance (R40)

The reviewed Sprint 3 implementation baseline is commits `62d7730` and
`1047367`. These commits establish the implementation baseline for sequenced
later work; they do not mean Sprint 3 release acceptance is closed.

Sprint 3 release acceptance remains open on all of the following gates:

- a valid independently authored Portuguese corpus;
- production-path quality and final provider-answer evidence;
- native Windows/R13 hermetic session evidence;
- exact-head GitHub Actions evidence.

V1-V10 and the currently rejected corpus fixtures/harnesses are not acceptance
evidence. Internal production testing without a corpus is diagnostic only.
Task 4.5, Sprint 4 close, Task 5.5, and release close MUST NOT bypass these
gates, and no later Fast/Deep result may be substituted for them.

## Relationship To Other Program Records

This program is registered in the project `ROADMAP.md` under Phase 6. It is not
a parallel or competing plan.

| Record | Relationship |
|---|---|
| `ROADMAP.md` Sprint 6A task 6.1 | Contextual Chat entry points. This program depends on that surface work and does not duplicate it. |
| `ROADMAP.md` backlog "Semantic/hybrid search" | That item was deferred pending "a repeatable retrieval benchmark showing FTS5 misses important results at a material rate." **Sprint 1 Task 1.2 is that benchmark.** Sprint 1 closing with a recorded FTS baseline failure satisfies the deferral condition; Sprint 1 failing to demonstrate it cancels the program. |
| `docs/sprint-6-1-contextual-chat.md` | Sprint 6.1 closed on 2026-08-22 after the manual Windows/Tauri smoke passed. Task `6.1.R10` defines the saved-meeting invariants that this program's Sprint 4.3 must preserve. |
| `docs/fts5-search-mcp-plan.md` | Historical. Superseded for retrieval design by this directory. |

## Scope

### In Scope

- One bundled multilingual bi-encoder and one bundled multilingual
  cross-encoder reranker.
- Reproducible artifact acquisition, license capture, hashes, and signed app
  packaging.
- Deterministic semantic documents for meeting profiles, transcript windows,
  summaries, and notes.
- Durable source-revision tracking, FTS repair, and resumable background
  semantic indexing.
- SQLite vector persistence and a measured exact or ANN in-memory search path.
- FTS5 plus vector reciprocal-rank fusion.
- Meeting-level aggregation, local reranking, authoritative hydration, and
  exact source retention.
- Fast and Deep Chat modes with Deep as the new-conversation default.
- All, folder, saved meeting, search snapshot, and today Chat scopes.
- Hybrid sidebar search and additive Tauri/MCP hybrid APIs.
- Index diagnostics, pause, rebuild, progress, and failure recovery.
- Evaluation, performance, packaged inference, migration, and crash tests.

### Out Of Scope

- Remote embedding APIs.
- GPU-specific ONNX execution providers in the first release.
- A network vector service.
- Replacing SQLite or FTS5.
- Embedding unsaved live-recording transcript state.
- Using speaker diarization embeddings for text retrieval.
- Multiple simultaneous Chat streams.
- SQLCipher or a general database-backup redesign.
- MCP authentication, although its existing localhost trust boundary remains a
  documented risk.

## Quality Attributes

### Correctness

- The correct meeting MUST rank in the top three candidates when expected by
  the approved evaluation fixture.
- Context MUST contain the evidence needed for required facts; high meeting
  rank without retained evidence is a failure.
- Sources MUST represent only evidence retained in the final model prompt.
- A semantic failure MUST NOT suppress lexical evidence.

### Privacy

- Embedding and reranker inference MUST remain local.
- Retrieval models MUST be loaded from trusted application resources.
- Embeddings and derived chunks MUST be treated as sensitive meeting data.
- No model artifact download or transcript upload occurs at runtime.

### Scope Isolation

- Folder, meeting, snapshot, and date allow-lists MUST be resolved from
  authoritative SQLite state before evidence is returned.
- ANN overfetch MAY search a global index internally, but candidates outside
  the request's scope MUST NOT enter fusion, reranking, hydration, sources, or
  prompts.
- Deep-mode actions MUST NOT widen the original scope.
- A Deep round's `openMeetingIds` MUST be a subset of the bounded candidate or
  meeting-card IDs supplied to that round, not merely an ID that happens to
  exist in the authoritative scope. `expandEvidenceIds` MUST be limited to
  evidence IDs currently known and retained by that round. Scope revalidation
  remains required after every round and before publication.

### Availability

- Application startup and primary meeting writes MUST NOT depend on the
  semantic model or index.
- FTS5 MUST remain the fallback while backfill is incomplete or any semantic
  component is unhealthy.
- Existing active semantic generations MUST remain available while a model
  upgrade builds a replacement generation.

### Performance

- Startup MUST NOT synchronously backfill embeddings.
- CPU-heavy tokenization, ONNX inference, exact scans, and index builds MUST run
  outside Tokio worker threads.
- Indexing MUST be single-owner, bounded, cancellable, and throttled or paused
  during recording/transcription pressure.
- The release implementation MUST satisfy the Sprint 1 benchmark gates at
  250,000 documents and stay within the approved retrieval RAM and derived-disk
  envelopes.
- Cross-encoder reranking is the most expensive interactive stage and MUST have
  its own measured sub-budget. It MUST NOT be permitted to consume the whole
  Fast preparation budget.

### Recoverability

- A user MUST be able to force lexical-only retrieval at runtime without
  reinstalling, rebuilding, or editing files. This is a persisted setting, not
  a compile-time flag. It is the operational rollback for the retrieval path,
  distinct from index pause/rebuild which only affect derived state.

## Architecture Overview

```text
Authoritative SQLite content
        |
        | triggers advance source revisions
        v
search_source_state + retrieval_meeting_state
        |
        v
Background Index Worker
        |
        | extract -> chunk -> tokenize -> embed
        v
retrieval_documents (SQLite, canonical derived vectors)
        |
        | immutable snapshot / optional ANN sidecar
        v
RetrievalService
        |
Question + validated scope
        |
        +--> FTS5 candidates
        +--> vector candidates
        |
        v
Reciprocal-rank fusion
        |
        v
Meeting aggregation
        |
        v
Local cross-encoder reranking
        |
        v
Authoritative meeting hydration
        |
        +--> Fast: final answer
        +--> Deep: bounded plan/search/open loop -> final answer
```

## Module Boundaries

### Retrieval Module

Add `frontend/src-tauri/src/retrieval/` only when Sprint 2 introduces the first
real retrieval implementation. Proposed files:

| File | Responsibility |
|---|---|
| `mod.rs` | Concrete `RetrievalService`, request orchestration, public types, and lifecycle. |
| `model.rs` | Tokenizer, embedding ONNX session, reranker ONNX session, pooling, and normalization. |
| `chunking.rs` | Deterministic source extraction and token-window construction. |
| `index.rs` | Immutable vector snapshots, exact scan, optional ANN, delta handling, and atomic swaps. |
| `ranking.rs` | RRF, deduplication, meeting aggregation, diversity, and reranker integration. |
| `agent.rs` | Deep-mode action schema, validation, bounded loop, and fallback; it may accept a privacy-safe progress sink but does not own Tauri events, `AppHandle`, or a second event bus. |

Do not add provider traits before a second concrete provider exists. Local
bundled inference is the only approved embedding provider.

### Database Repository

Add `frontend/src-tauri/src/database/repositories/retrieval.rs` for source/model
revisions, FTS repair state, document replacement, publication journal,
coverage, retry, and rebuild queries.
It MUST read source content from authoritative tables, not `meeting_fts`.

### Chat Adapter

`api/chat.rs` retains:

- Provider/model configuration.
- Live authorization and live context.
- Temporal and meeting-list intents.
- Scope resolution and conversation ownership.
- Prompt assembly and stream ownership.

`api/chat.rs` delegates persisted-content retrieval to `RetrievalService`.
Hybrid logic MUST NOT be copied into streaming, non-streaming, and MCP callers.

### Context Builder

Both saved-meeting and broad builders return a common result containing the
rendered context and retained evidence IDs. Sources are constructed only after
the final context and question/history budget is known.

## Runtime State

Manage one `RetrievalService` through Tauri state beside `AppState`.

Conceptual state:

```rust
struct RetrievalService {
    model_bundle: RetrievalModelBundle,
    index: ArcSwapLikeIndexSnapshot,
    worker_control: RetrievalWorkerControl,
    status: RetrievalStatus,
}
```

The concrete implementation MAY use `Arc<RwLock<Arc<IndexSnapshot>>>` if that
is sufficient; do not add `arc-swap` without benchmarked lock contention.
Queries clone the immutable snapshot and release locks before scanning.

Create one Tauri-managed retrieval lifecycle object during application setup,
before a database necessarily exists. It starts detached and exposes one
idempotent `attach_database/start` transition. Invoke that transition after
successful `AppState` installation in all three paths: existing database
startup, fresh database creation, and legacy database import. Duplicate starts
for the same pool are rejected/no-ops; attaching a different pool requires an
explicit stop/detach first. MCP receives a clone of the same service rather
than constructing another index/model runtime. Shutdown cancels queued work,
joins the worker and publisher, unloads model sessions, and only then allows
the database pool to close.

## Model Bundle Contract

The application bundles two models:

1. A multilingual bi-encoder for Portuguese and English query/document
   embeddings.
2. A multilingual cross-encoder for question/evidence reranking.

Sprint 1 selects exact models. Public benchmark reputation alone is not
acceptance evidence. The chosen pair MUST satisfy the repository evaluation
corpus, redistribution license, ONNX compatibility, tokenizer reproducibility,
package integrity, platform loading, memory, and latency gates.

Each bundle has a checked-in small manifest. Large artifacts are fetched by CI
from pinned immutable URLs or an approved artifact store and verified before
`tauri build`.

Required manifest fields:

```json
{
  "bundleId": "...",
  "embeddingModel": {
    "modelId": "...",
    "revision": "...",
    "dimensions": 384,
    "maxSequenceLength": 512,
    "tokenizer": {
      "type": "...",
      "revision": "...",
      "truncationSide": "right",
      "artifacts": []
    },
    "queryPrefix": "...",
    "documentPrefix": "...",
    "pooling": "...",
    "normalization": "l2",
    "inputs": [{"name": "input_ids", "dtype": "int64"}],
    "outputs": [{"name": "last_hidden_state", "dtype": "float32"}],
    "artifacts": []
  },
  "rerankerModel": {
    "modelId": "...",
    "revision": "...",
    "maxSequenceLength": 512,
    "tokenizer": {
      "type": "...",
      "revision": "...",
      "truncationSide": "right",
      "artifacts": []
    },
    "pairFormat": "question,evidence",
    "inputs": [{"name": "input_ids", "dtype": "int64"}],
    "outputs": [{"name": "logits", "dtype": "float32"}],
    "outputLabelIndex": 0,
    "scoreTransform": "identity",
    "artifacts": []
  },
  "chunkerVersion": 1,
  "licenses": [],
  "manifestVersion": 1
}
```

Every artifact entry records path, byte length, and SHA-256. Models MAY
reference the same tokenizer identity only when their complete tokenizer and
pair-format contracts are identical. The build fails on a missing artifact,
hash mismatch, unknown license, or incompatible manifest. Runtime lazily
rechecks length/hash before the first load in each process so post-install
resource corruption cannot reach ONNX Runtime.

### Package Authority And Provenance (1.R2, 2026-08-25)

The staged `resources/retrieval/bundle` is the only packaged retrieval
authority: it contains one manifest, its manifest-managed model/tokenizer/
license artifacts, and one hash-pinned README placeholder. Build-input copies
outside that directory are not packaged. Publication and recovery reject a
missing, corrupt, divergent, or unmanifested file before a bundle can be
activated or restored.

The manifest parser admits only the approved Sprint 1 contract, including
model/export identities, preprocessing, tensor I/O, artifact-source revisions,
and license authority. The e5-base ONNX conversion is attributed to its pinned
Xenova export and the MIT upstream model; the packaged notice preserves the
applicable Microsoft copyright and MIT permission text from the pinned E5
development repository. The mmarco package retains its pinned Apache-2.0
declaration and canonical license text. A model or provenance change requires
an architecture amendment and a new artifact/notice verification run.

Models load directly from Tauri's signed read-only resource directory. Copying
to app data is permitted only if the selected ONNX export requires writable or
co-located external-data files that cannot be loaded from resources. Such a
copy MUST be atomic, versioned, hash-verified, and recoverable.

ORT inference is CPU-only in the initial release. Whisper CUDA, Vulkan, Metal,
and OpenBLAS features do not imply ORT acceleration.

### Approved Sprint 1 Bundle And Runtime Contract (2026-08-24)

Sprint 1 Task `1.3` selected the following production bundle. Task `1.5` MUST
encode this contract in the reproducible artifact manifest; Sprint 2 MUST NOT
substitute a model, revision, encoding, or preprocessing detail without a
user-approved architecture amendment.

| Component | Approved contract |
|---|---|
| Bi-encoder | `intfloat/multilingual-e5-base` at `d128750597153bb5987e10b1c3493a34e5a4502a`; dynamic-int8 ONNX export at `1ec9243030a27d1a115d5c340572074c125b58b2`; 768 dimensions; MIT |
| Reranker | `cross-encoder/mmarco-mMiniLMv2-L12-H384-v1` at `1427fd652930e4ba29e8149678df786c240d8825`; `quint8_avx2` ONNX export; Apache-2.0 |
| Vector storage | int8, with the encoding recorded per generation and validated at the repository boundary |
| Chunk profile | 384 tokens with 64-token overlap |
| Fusion and aggregation | RRF `k=5`; `w_vector=1`; `w_lexical=0.5`; support `alpha=0.5`; title `beta=1`; reranker `gamma=0`; support cap 3 |
| Reranker runtime | Chat depth 50; Search depth 25; batch 1; ORT intra-op 4 |

The measured projected 250k peak is 1118.3 MiB, inside the user-approved
1-1.25 GiB e5-base band. Sprint 1's measured depth-50 rerank figure is a
historical derived estimate (`solo-pair p95 * 50`) of 720 ms. It justified the
initial depth but is not a current release-stage p95 measurement. Derived disk
is 558 B per document, projected at 0.13 GiB steady state and 0.26 GiB during
shadow rebuild.

This selection is **title-assisted**, not an embedding-only claim. Holding all
other earned constants fixed, reference-category Recall@1 is 12/15 at
`beta=1` and 7/15 at `beta=0`; semantic Recall@3 remains 30/30. Any future
quality report MUST preserve that qualification rather than attributing the
reference-family outcome solely to the bi-encoder.

The constants are the final held-out objective's output; its five
critical/pinned cases were never inspected by a tuning path. The diagnostic
probe's 78 quint8 configurations passed only Critical Recall@1, critical
retrieval-stage contamination, and exact-term no-regression. It did not test
every gate for those configurations and MUST NOT be cited as proof that an
alternative fully-passing configuration exists.

The fusion/aggregation row above is the Sprint 1 starting policy, not a
permanent ban on production-pipeline retuning. A later ranking task MAY replace
those ranking-only constants through its predeclared held-out protocol and
recorded evidence addendum. Model, tokenizer, encoding, chunking, batch size,
ORT thread caps, and rerank depth remain separate runtime contracts and do not
change by implication. The active production constants live in the reviewed
ranking implementation and its sprint execution entry; historical selection
reports remain evidence, not runtime authority.

### Prior-Model Retention Across Upgrade (2026-08-26)

A vector generation is queryable only with the embedding model that produced
its vectors. A bundled-model upgrade replaces the single packaged bundle in
place, so at the restart that installs the upgrade the prior model's
artifacts no longer exist. "Generation Activation" requires the prior active
generation to stay queryable while the new shadow generation builds; without
retention that requirement is unmet across exactly the restart it matters
for, and the prior generation becomes stored, still pointed to by the
singleton, and semantically unavailable.

**Status: no prior bundle exists yet.** Only `meetily-retrieval-bundle-1` has
ever shipped, so there is nothing to retain today and nothing in this
subsection is implementable now. It becomes implementable in the release that
introduces a second bundle; at that point the prior bundle is whichever one
shipped immediately before it, and its identity, artifacts, byte lengths, and
digests are read from its already-checked-in manifest. A prior-bundle
identity MUST NEVER be authored, guessed, or reconstructed: if it is not
already pinned in a shipped manifest, it is not a prior bundle.

**Only the prior embedding engine is retained.** The cross-encoder scores
`(question, evidence)` text pairs and has no dependency on which generation
produced a hit, so the current reranker serves hits from either generation.
Retention covers the prior embedding ONNX export and its tokenizer
artifacts; it does not cover the prior reranker.

**Retention lives inside package authority.** A transition release packages
the current bundle and at most one immediately-prior bundle under
`resources/retrieval/`, each with its own manifest and each validated
against the approved contract. The approved-contract check becomes an
ordered two-entry allowlist — one current bundle, at most one prior bundle —
and the release after the transition drops the prior entry. Copying a prior
bundle into app data remains forbidden: it would leave an unsigned, mutable
model store outside "Package Authority And Provenance" and outside the
pinned provenance chain, and the prior bundle would fail the current
binary's own contract validation.

**Query routing follows generation identity.** Query embedding is dispatched
by the active generation's model identity. Index work is never dispatched to
a retained prior engine: due-work selection continues to admit only
generations whose model matches the current bundle, so the prior generation
stops accepting updates the moment the upgrade lands. Meetings mutated
during the rebuild window are therefore dirty against the prior generation
and fall to FTS under the existing "Meeting dirty" rule, which is the
already-approved behavior rather than a new exception.

**Retention is bounded by the resource envelope.** Retention adds one
resident embedding session for the duration of the rebuild, concurrent with
the active and shadow snapshots the 1.30 GiB transient ceiling already
covers. That combination is unmeasured; the approved ceiling was measured
for two snapshots plus one session set. Retention MUST NOT be enabled until
a measured peak for active + shadow snapshots plus current + prior embedding
sessions is recorded and approved. The activation RAM gate MUST refuse to
load the prior engine — degrading to FTS-only — rather than exceed the
approved ceiling. A prior reranker session is never resident, and a third
embedding session remains unapproved regardless.

**Lexical fallback is required, not optional.** Retention is best-effort. An
install that skips the transition release has no prior bundle; so does one
whose prior bundle fails verification or is refused by the RAM gate. In each
case semantic retrieval degrades to FTS-only until the new generation
activates, reported as an explicit state. Prior-generation vectors MUST
NEVER be scored against query embeddings produced by a different model.

**Identity precondition.** Retention is meaningless unless the persisted
model identity changes on every model change. Model identity MUST be derived
from the full approved contract — bundle id, embedding model id, revision,
quantization, dimensions, and chunker version — not from `bundleId` alone. A
model swap that reuses an identity aliases the new model onto the prior
generation, and there is then no prior generation to preserve.

## Semantic Document Model

### Meeting Profiles

A meeting profile supports meeting selection. It contains bounded, labeled
content from:

- Current meeting title.
- Latest non-empty summary.
- Current notes.
- Optional high-signal summary headings selected deterministically.

Profiles do not replace evidence chunks and are not cited unless their source
text is separately retained in final context.

### Transcript Windows

Do not embed one short utterance per vector by default. Construct windows from
chronological transcript segments using the selected model tokenizer.

Initial benchmark candidates:

- 256 tokens with 48-token overlap.
- 384 tokens with 64-token overlap.
- 512 tokens with 96-token overlap when supported by the model.

Windows MUST preserve first/last transcript IDs, speaker/timestamp metadata,
and stable chronology. Prefer segment boundaries; split a single oversized
segment only when required by the model limit.

### Summary And Note Sections

Split Markdown by heading before applying token windows. Preserve summary
template ID and section heading. Empty sections are omitted.

The meeting profile uses the same latest-summary policy as saved-meeting Chat.
Evidence indexing MAY include all non-empty summary templates when each chunk
retains its template identity and hydration can reproduce the matched text.
Sprint 1 evaluation decides whether this improves recall without unacceptable
conflict noise.

### Stable Identity

`document_id` is a deterministic hash over:

```text
model_id
chunker_version
meeting_id
source_kind
source row or transcript range
window ordinal
content hash
```

The content hash MUST cover the exact normalized text sent to the embedding
model. Prefixes, pooling, normalization, tokenizer revision, and chunker
version belong to `model_id` or bundle identity so incompatible vectors cannot
share a generation.

## Persistence Schema

Sprint 2 may adjust names to repository conventions, but it MUST preserve these
semantics:

```sql
CREATE TABLE retrieval_models (
    model_id TEXT PRIMARY KEY,
    dimensions INTEGER NOT NULL CHECK (dimensions > 0),
    vector_encoding TEXT NOT NULL,
    chunker_version INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

-- 'active' is deliberately NOT a generation state. The singleton pointer in
-- retrieval_active_model is the only authority on which generation is active.
-- Two representations of the same fact cannot be kept consistent.
CREATE TABLE retrieval_generations (
    generation_id TEXT PRIMARY KEY,
    model_id TEXT NOT NULL
        REFERENCES retrieval_models(model_id),
    state TEXT NOT NULL CHECK (state IN ('building', 'ready', 'failed', 'retired')),
    document_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    activated_at TEXT,
    retired_at TEXT
);

CREATE TABLE retrieval_active_model (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    generation_id TEXT NOT NULL
        REFERENCES retrieval_generations(generation_id),
    activated_at TEXT NOT NULL
);

CREATE TABLE search_source_state (
    meeting_id TEXT PRIMARY KEY
        REFERENCES meetings(id) ON DELETE CASCADE,
    source_revision INTEGER NOT NULL DEFAULT 1,
    fts_projection_revision INTEGER NOT NULL DEFAULT 1,
    fts_indexed_revision INTEGER NOT NULL DEFAULT 0,
    changed_at TEXT NOT NULL,
    fts_attempt_count INTEGER NOT NULL DEFAULT 0,
    fts_next_attempt_at TEXT,
    fts_last_error TEXT
);

-- Due-work selection is the worker's hottest query. Without this the worker
-- full-scans every meeting on every poll.
CREATE INDEX search_source_state_fts_due
    ON search_source_state(fts_next_attempt_at)
    WHERE fts_indexed_revision < fts_projection_revision;

-- Deliberately a rowid table. Rows carry a multi-KB vector BLOB plus chunk
-- text, which is far above the small-row profile WITHOUT ROWID is designed
-- for; large WITHOUT ROWID rows spill to overflow chains and degrade the
-- full-table scan that snapshot loading depends on.
CREATE TABLE retrieval_documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    generation_id TEXT NOT NULL
        REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE,
    document_id TEXT NOT NULL,
    meeting_id TEXT NOT NULL
        REFERENCES meetings(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL,
    source_start_id TEXT,
    source_end_id TEXT,
    source_template_id TEXT,
    ordinal INTEGER NOT NULL,
    content TEXT NOT NULL,
    content_hash BLOB NOT NULL,
    dimensions INTEGER NOT NULL CHECK (dimensions > 0),
    vector_encoding TEXT NOT NULL,
    vector BLOB NOT NULL,
    source_revision INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (generation_id, document_id)
);

-- No byte-width CHECK here. A hardcoded `length(vector) = dimensions * 4`
-- forbids every non-f32 encoding and would block the quantization path that
-- "Resource Budget Arithmetic" depends on. Encoding-aware validation of
-- (vector_encoding, dimensions, byte length, finiteness, norm) is performed at
-- the repository boundary, which this document already requires.

CREATE INDEX retrieval_documents_by_meeting
    ON retrieval_documents(generation_id, meeting_id);

CREATE TABLE retrieval_document_staging (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL,
    generation_id TEXT NOT NULL
        REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE,
    meeting_id TEXT NOT NULL
        REFERENCES meetings(id) ON DELETE CASCADE,
    source_revision INTEGER NOT NULL,
    document_id TEXT NOT NULL,
    payload BLOB NOT NULL,
    UNIQUE (job_id, document_id)
);

CREATE INDEX retrieval_document_staging_by_generation
    ON retrieval_document_staging(generation_id, meeting_id);

CREATE TABLE retrieval_meeting_state (
    generation_id TEXT NOT NULL
        REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE,
    meeting_id TEXT NOT NULL
        REFERENCES meetings(id) ON DELETE CASCADE,
    indexed_source_revision INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL CHECK (state IN ('pending', 'ready', 'retry', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT,
    last_error TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (generation_id, meeting_id)
) WITHOUT ROWID;

-- Due-work selection for semantic indexing. Same rationale as the FTS index
-- above: step 1 of the worker algorithm must not scan every meeting row.
CREATE INDEX retrieval_meeting_state_due
    ON retrieval_meeting_state(generation_id, state, next_attempt_at);

CREATE TABLE retrieval_index_state (
    generation_id TEXT PRIMARY KEY
        REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE,
    backend TEXT NOT NULL,
    state TEXT NOT NULL,
    document_count INTEGER NOT NULL,
    canonical_change_id INTEGER NOT NULL DEFAULT 0,
    published_change_id INTEGER NOT NULL DEFAULT 0,
    sidecar_hash BLOB,
    updated_at TEXT NOT NULL
);

CREATE TABLE retrieval_index_changes (
    change_id INTEGER PRIMARY KEY AUTOINCREMENT,
    generation_id TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
    source_revision INTEGER,
    created_at TEXT NOT NULL
);

-- Journal replay filters by generation inside a change_id range. change_id is
-- a rowid alias so the range is cheap, but the generation filter is not.
CREATE INDEX retrieval_index_changes_replay
    ON retrieval_index_changes(generation_id, change_id);
```

Vectors are normalized, finite, little-endian, and stored in the encoding named
by `vector_encoding` for their generation's model. `f32` is the reference
encoding; `int8` and `fp16` are permitted when Sprint 1 approves them under
"Resource Budget Arithmetic". Repository reads validate encoding, dimension,
byte length, finiteness, and norm before admitting a vector to an in-memory
index. A quantized encoding MUST record its dequantization parameters in
`retrieval_models` so a vector is never interpreted under the wrong scale.
Malformed derived rows are quarantined/rebuilt, not allowed to crash
application startup.

### Derived Storage Cost

Storing derived chunk text is intentional, but it is not free and the program
gates it. At the 250,000-document release scale, one generation holds a second
copy of the indexed transcript, summary, and note text plus its vectors, and a
shadow rebuild transiently doubles that. The derived disk envelope is a 2 GiB
steady-state target and a 3 GiB peak during rebuild, measured as the total of
`retrieval_documents`, `retrieval_document_staging`, any sidecar, and all
retained generations. Sprint 1 records the measured per-document cost; Sprint 5
qualifies against the envelope. Exceeding it blocks release in the same way an
over-budget RAM result does.

Storing derived chunk text is intentional. It gives reranking a stable,
content-hash-bound input and avoids recomputing every candidate window during a
query. It remains local sensitive data and is deleted with its meeting.

## Durable Change Tracking

Do not copy FTS's best-effort hook-only synchronization. Add SQLite triggers
that advance `search_source_state.source_revision` for:

- Meeting insert and title update.
- Transcript insert, update, and delete.
- Summary-process insert, result update, and delete.
- Meeting-notes insert, update, and delete.

Each retrieval generation has independent `(generation_id, meeting_id)` indexed
revision, retry schedule, and error state. A generation ID is distinct from the
model/bundle ID so a manual rebuild can shadow-build the same model safely.
This is required because an active generation and a shadow upgrade/rebuild must
both process edits. One generation may never clear another generation's work.

Meeting deletion cascades semantic documents/source/generation state and appends a
non-foreign-keyed deletion entry to `retrieval_index_changes` for every built
generation before the meeting disappears. In the same transaction, each
generation's canonical change ID advances to its tombstone change ID. This
tombstone survives the cascade so a running or restarted publisher can remove
deleted vectors from memory/sidecar; semantic queries pause while canonical is
ahead of published.

The same primary delete transaction decrements each affected generation's derived
`document_count` by the rows for that meeting using the indexed
`(generation_id, meeting_id)` lookup. The correlated count is bounded by one
meeting's rows across the at-most-two retained generations and uses
`MAX(document_count - N, 0)` to preserve the existing non-negative clamp; it is
not a full-generation recount. Debug-time reconciliation compares affected
counter values with the authoritative `retrieval_documents` row counts and
reports divergence without changing either value.

Folder metadata does not require re-embedding, but FTS stores folder ID/name.
Triggers or repository mutations therefore advance only
`fts_projection_revision` for `meetings.folder_id` changes and affected
meetings on folder rename/delete/detach. Content changes advance both
`source_revision` and `fts_projection_revision`. Folder parent moves need no
FTS projection change when immediate folder ID/name remain unchanged.

Migration backfill creates source state for every existing meeting and pending
per-generation state when a generation is registered. It MUST NOT run model
inference inside the migration.

FTS is also derived and currently best-effort. A model-independent derived-
index coordinator repairs FTS whenever `fts_indexed_revision <
fts_projection_revision`, then marks the indexed revision only after a
successful complete refresh. Existing
best-effort immediate refresh hooks MAY remain as latency optimizations, but
durable source revision is the correctness path. Semantic model failure cannot
stop lexical repair.

### Worker Algorithm

1. Select one due lexical repair or `(generation_id, meeting_id)` whose indexed
   revision is behind current source revision.
2. Skip a not-yet-due retry so one poison meeting cannot starve other work.
3. Load current authoritative title, summaries, notes, and transcripts with
   source revision.
4. Repair FTS first when needed, independently of model availability.
5. Build/tokenize/embed in measured batches no larger than 256 documents or 64
   MiB working memory, unless Sprint 1 approves lower limits.
6. Write each completed batch to a job/source-revision-bound staging table.
7. On cancellation/crash, resume valid matching staging or discard stale jobs;
   active documents remain unchanged.
8. After every batch is staged, begin a short SQLite transaction.
9. Re-read source revision and abort/discard/retry if it changed.
10. Replace all active documents from staging for that meeting/generation
    atomically and remove the staging job.
11. Update its indexed source revision/reset retry state.
12. Append an `upsert` publication-journal entry and advance canonical change
    ID in the same transaction.
13. Commit. A separate publisher replays journal entries into the exact delta
    or ANN delta and advances published change ID durably.

There is one indexing coordinator with bounded model work and a durable retry
schedule. Retry state includes attempt count, next-attempt time, and safe error
kind/message. Permanent failure for one item does not busy-loop or starve other
meetings. A building generation cannot activate while any current meeting is
behind or failed; active-generation failed meetings are excluded semantically and remain
available through durably repaired FTS.

This is the named **quarantined coverage** state: an active generation may
serve complete-minus-quarantined coverage when a terminal per-meeting semantic
failure occurs. Its canonical derived rows remain for retry/rebuild provenance,
but loaders and snapshots exclude the failed meeting; `failed_meetings` reports
the omission rather than silently presenting it as complete semantic coverage.

If the process crashes after SQLite replacement but before in-memory
publication, startup replays `retrieval_index_changes` from published through
canonical change ID. Queries never use a semantic snapshot whose published ID
is behind canonical state; they wait for bounded catch-up or use FTS-only.

### Generation Activation

Initial install uses FTS-only retrieval until the bundled model's complete
generation is indexed. Partial semantic coverage MUST NOT bias rankings toward
meetings indexed first.

On model/chunker upgrades, the singleton active-generation pointer continues to
reference the old generation while a new shadow generation builds. Activate the
new model in one transaction only after every current meeting's indexed source
revision equals its source revision, no permanent item error remains, its
snapshot/sidecar validates, and published change ID equals canonical change ID.
Manual rebuild creates a new shadow generation for the same model ID and uses
the same activation path. A healthy active generation remains queryable during
rebuild. If active state is known corrupt, clear/deactivate the singleton and
use FTS-only until the shadow validates. Rebuild cancellation deletes only the
shadow's staging/derived state and leaves a healthy active generation intact.
Crash/restart resumes the shadow from revisions/staging; concurrent readers see
the old active generation until one atomic pointer/snapshot switch.

An otherwise active generation with a terminal per-meeting failure instead
serves the named quarantined-coverage state: the failed meeting's canonical
rows survive but are excluded from semantic snapshots, its omission is reported
through `failed_meetings`, and durable FTS remains the fallback for that meeting.

Across the restart that installs a bundled-model upgrade, the prior active
generation stays queryable only while its own embedding engine is retained
and admissible; see "Prior-Model Retention Across Upgrade". When it is not,
the singleton keeps pointing at the prior generation, semantic retrieval
reports itself unavailable, and every surface uses FTS until the new
generation activates.

Retain at most two complete generations. The previous generation becomes
eligible for garbage collection only after the new active generation survives
one clean application restart and one successful Fast hybrid query. Cleanup is
idempotent and never removes the active generation, active snapshot, or a
generation with unacknowledged journal changes.

**Transitional clause (2026-08-26; expires when Task 3.4 activates Fast hybrid
Chat).** The Fast
hybrid query surface does not exist before Sprint 3, so the
successful-query condition above is unsatisfiable until then — and against
the two-generation retention ceiling that makes manual rebuild a single-use
operation and eventually dead-ends corrupt-active recovery. While no semantic
query surface exists, that condition is satisfied instead by one clean
application restart with the new generation active and its publication lag
zero. The restart condition itself is never relaxed, and every other cleanup
guard stands unchanged. Task 3.4 MUST remove this clause when the product Fast
hybrid path is activated; from that point the successful-query condition
applies as originally written. Sprint-close timing is not the authority: the
presence of a real product query surface is.

## Vector Search Backend

SQLite BLOBs are canonical. A sidecar index, if selected, is disposable and
must be rebuildable from SQLite.

### Exact Option

Use one immutable contiguous base snapshot plus an exact update delta and
tombstones. Query vectors are normalized, so cosine similarity is a dot
product. Search the base and delta, remove tombstones, and compact/rebuild the
base at the Sprint 1 measured threshold. Run scans in `spawn_blocking`; start
serial and use measured bounded parallelism only when needed. Meeting updates
MUST NOT copy all 250,000 base vectors synchronously.

Exact search is preferred because it has full recall and the smallest failure
surface. It ships if Sprint 1 proves that 250,000-document vector-stage p95 and
peak retrieval RAM satisfy the approved gates on reference hardware.

### Backend Decision Rule

The latency gate and the RAM gate have different remedies and MUST NOT be
treated as one "scale gate". An ANN index stores a proximity graph *in addition
to* the vectors it indexes; it reduces query latency and **increases** memory.
Selecting ANN in response to a RAM failure makes the failure worse.

| Measured result at 250k | Permitted remedy |
|---|---|
| Both gates pass | Ship exact search. Do not evaluate ANN. |
| Latency p95 misses, RAM passes | Evaluate a pure-Rust HNSW-style index. This is the ANN path. |
| RAM misses, latency passes | Reduce the footprint: quantized `vector_encoding`, a lower-dimension model, or memory-mapping the base snapshot. **ANN is not a remedy and MUST NOT be evaluated for this failure.** |
| Both miss | Stop. Architecture decision required. Do not silently reduce scale, quality, or the evaluation corpus. |

Because "Resource Budget Arithmetic" is applied as a pre-filter during model
selection, a RAM miss at Task 1.4 should be rare. If one occurs it means the
pre-filter arithmetic was wrong, which is itself a finding to record.

### Approved Exact Backend Contract (2026-08-24)

Task `1.4` selects exact search for the approved 768-d int8 bundle. ANN was
not evaluated: the 250k exact benchmark passes the vector-stage p95 gate at
61.1 ms (500 ms gate), has exact recall@150 `1.0000`, and records a measured
steady state of 1134.8 MiB inside the approved 1-1.25 GiB band. Task `1.R3`
replaced the preliminary arithmetic rebuild estimate with a same-process
measurement holding active snapshot, streamed shadow, delta/tombstones, and
  both warmed selected ONNX sessions. R3 measured 1316.3 MiB and the first
  independent rerun measured 1317.9 MiB. After R3a corrected bounded journal
  publication, valid release reruns measured 1319.9 MiB and 1316.9 MiB. The
  governing observed peak is therefore 1319.9 MiB, inside the explicitly
  approved 1.30 GiB transient ceiling by 11.3 MiB. These figures cover exactly
  two snapshots; a third remains unapproved.

### Activation RAM gate scope (2.R12)

The fixed `ACTIVATION_RAM_CEILING_BYTES` remains exactly `1,395,864,371`
bytes (1.30 GiB). Stage 3 deliberately re-derives its gate scope as a
**whole-process RSS budget**, rather than comparing whole-process RSS with the
retrieval-only arithmetic above. The production gate samples the process's
resident physical memory immediately before activation; its limit is therefore
the same quantity:

```text
whole_process_activation_residency =
    retrieval snapshots + retrieval sessions + Whisper/audio + Tauri/webview
    + allocator and other process overhead
<= 1,395,864,371 bytes
```

The retrieval arithmetic remains a model-selection/pre-filter bound and is not
used as an unscoped RSS comparator. This whole-process choice is intentionally
conservative: the release benchmark measures the same process scope but does
not load a full application's optional Whisper/webview state, so a real
application can consume part of the fixed budget before retrieval activation.
The status payload names the scope next to the measured resident value and
ceiling.

#### Close-out acceptance and its Sprint 3 obligation (2026-08-28)

The whole-process scope above is **accepted as-is for Sprint 2**, with the
ceiling unchanged at `1,395,864,371` bytes. It is knowingly mismatched: the
gate samples whole-process RSS while the ceiling derives from retrieval-only
arithmetic, so it refuses in states where retrieval alone would have fit. The
error direction is fail-safe — activation is withheld, the built generation
stays on disk, and every surface falls back to lexical retrieval.

The mismatch is accepted rather than corrected because both corrections were
proven unavailable, not merely deferred:

- A retrieval-scoped gate cannot be built. `2.R13` established that ORT
  1.22 as bound exposes no per-session memory query, that native session
  allocations never traverse the Rust global allocator, and that every
  whole-process delta substitute can undercount.
- Full-application calibration cannot be produced on the development host.
  `2C.3` established that the approved Whisper artifact is absent, that
  loading the real audio stack requires contending with live WASAPI streams,
  and that no production-path facility proves WebView residency.

**Sprint 3 obligation.** Sprint 3 is the first build in which a real, fully
loaded application runs a semantic query on a user machine — the measurement
`2C.3` could not manufacture. Sprint 3 MUST therefore, before its own close:

1. Record whole-process RSS at activation from a real application session
   with Whisper, audio, and the WebView resident.
2. Re-derive `ACTIVATION_RAM_CEILING_BYTES` from that measurement, or record
   that the existing value is confirmed correct.
3. Report the observed refusal rate. A gate that never admits is a defect
   even though it never crashes.

Until (1)-(3) land, no code may relax, widen, or bypass this gate.

#### Activation refusals must be observable

The gate stores its refusal reason in `IndexService::pending_blockers`, exposes
`pending_activation_blockers()` through the retrieval status report, and emits
one warn-level line for each activation pass that refuses one or more
generations. A refusal is therefore distinguishable from a normal run without
a debugger, even though no UI surface exists yet. The refusal reason names the
scope, measured bytes, and ceiling, and carries no meeting text, tokens, or
vectors, so it is safe to log verbatim.

Sprint 2 MUST implement one immutable contiguous base snapshot, an exact
upsert delta, and tombstones. It MUST preserve canonical SQLite plus the
publication journal, serve the active snapshot while a shadow builds, replay a
canonical-ahead-of-published crash window before semantic use, and compact in
the background rather than copying the base during a meeting update.

Initial measured operating limits are: 150 candidates; two concurrent vector
scans; interactive queue capacity 8; interactive index-worker pause within
250 ms; update batch 128 documents; and compaction at or before a 2% delta.
The production implementation MUST re-measure these limits under its actual
allocation and scheduling behavior. Any third resident snapshot or rebuild
peak above 1.30 GiB blocks activation until a user-approved remedy exists.

### ANN Option

Add a pure-Rust HNSW-style index **only under the latency-miss row above**. Do
not add a native extension or external service.

Requirements:

- The ANN graph's own memory MUST be counted in the retrieval RAM envelope
  alongside vectors and model sessions. An ANN candidate that pushes peak RAM
  above the approved band is rejected regardless of its latency benefit.
- SQLite vectors remain canonical.
- Sidecar files are versioned by model and index generation.
- Build to a temporary path, hash/validate, then atomically rename.
- Missing/corrupt sidecars trigger rebuild and lexical fallback.
- Scope filtering is authoritative after ANN candidate generation.
- Narrow allowed sets use exact scoped scans when cheaper and safer.
- Updates use an exact delta plus tombstones until a background base rebuild.
- Queries merge base ANN and exact delta before fusion.

Sprint 1 benchmarks update cost, reader-held old snapshots, compaction,
shadow-model activation, and peak old+new base/delta/model-session memory for
both exact and ANN candidates.

The implementation MUST NOT introduce a generic vector-backend trait solely
for this benchmark decision. Implement the selected backend and keep the
SQLite repository boundary stable.

## Retrieval Contracts

Conceptual request:

```rust
struct RetrievalRequest {
    original_query: String,
    rewritten_query: Option<String>,
    scope: PersistedRetrievalScope,
    purpose: RetrievalPurpose,
    limits: RetrievalLimits,
    cancellation: Option<CancellationToken>,
}

enum RetrievalPurpose {
    Chat,
    Search,
    Context,
}

enum PersistedRetrievalScope {
    All,
    Meeting(String),
    Folder(String),
    AllowedMeetingIds(Vec<String>),
}
```

External requests provide exactly one tagged scope. `All`, `Meeting`, `Folder`,
and `AllowedMeetingIds` cannot be combined. Allowed-ID sets use the existing
approved bounded size; duplicate IDs are removed while preserving order. A
`folder:"..."` query operator is normalized into folder scope only from `All`;
it must resolve to the same folder as an explicit folder scope or the request
is rejected. The normalized result is a `ResolvedScope` containing stable
request-start membership and the rules needed for publication-time validation.

Dynamic membership is revalidated immediately before hydration/source emission
and after each Deep round. A meeting moved out of a folder or deleted during a
slow query is removed before prompt/source publication. Snapshot IDs remain the
frozen membership set, but current meeting existence is rechecked.

Conceptual evidence:

```rust
struct RetrievedEvidence {
    evidence_id: String,
    meeting_id: String,
    meeting_title: String,
    source_kind: String,
    source_start_id: Option<String>,
    source_end_id: Option<String>,
    source_template_id: Option<String>,
    text: String,
    speaker: Option<String>,
    timestamp_start: Option<String>,
    timestamp_end: Option<String>,
    lexical_rank: Option<usize>,
    semantic_rank: Option<usize>,
    fused_rank: usize,
    reranker_score: Option<f32>,
}
```

Raw scores are internal diagnostics. Existing `FtsSearchResult.rank` remains a
BM25 value and MUST NOT silently become cosine or fused rank.

## Candidate Generation

For each request:

1. Resolve the current scope to authoritative meeting/folder allow-lists.
2. Preserve original and rewritten queries as distinct retrieval variants.
3. Build a lexical core-term variant that removes measured high-frequency
   function words without changing the user's answer question.
4. Run bounded FTS retrieval and an authoritative current-title lexical channel
   because meeting titles are not indexed in `meeting_fts`.
5. Embed query variants locally and run bounded vector retrieval.
6. Apply scope filters before candidates enter fusion.
7. Deduplicate by stable evidence identity.

Provisional candidate limits are evaluation parameters, not permanent magic
numbers:

| Stage | Initial benchmark range |
|---|---|
| FTS candidates per variant | 50-150 |
| Vector candidates per variant | 50-150 |
| Reranker evidence inputs | 30-50 |
| Candidate meetings | 8-12 |
| Hydrated meetings | 3-5 |

## Rank Fusion

Use reciprocal-rank fusion because BM25, cosine, and query-variant scores are
not numerically comparable.

```text
RRF(document) = sum(1 / (k + rank_in_list))
```

Sprint 1 fixes `k`, channel weights if any, candidate limits, lexical
normalization/core-term policy, concept extraction, and meeting-aggregation
constants from the evaluation corpus. A subagent MUST NOT tune constants only
for the reference WhatsApp query.

Later production ranking work may retune ranking-only constants when it uses a
predeclared search space, evaluates the exact full-gate predicate for every
configuration, and selects only by the tuning-partition objective among the
full-gate-feasible configurations. Critical/pinned cases therefore authorize
eligibility, while their post-selection recomputation remains defense-in-depth
verification; a miss must fail rather than change the selected configuration.
Historical model-selection constants are the required baseline candidate, not
an immutable production configuration.

## Meeting Aggregation

Broad retrieval ranks meetings before final context construction. Meeting
ranking considers:

- Best fused evidence rank.
- Distinct meaningful query concepts covered across evidence.
- Capped supporting-evidence count.
- Meeting-profile rank.
- Title overlap.
- Cross-encoder scores.
- Explicit user-requested date/recency semantics.

Long meetings MUST NOT win solely because they produce more chunks. Per-meeting
contribution is capped and diversity is measured separately.

"Considers" requires each signal to be represented and measured; it does not
require every signal to have a separate non-zero additive coefficient. A
meeting profile may participate through its fused rank, concept coverage may be
measured with a zero coefficient when the held-out protocol does not earn one,
and diversity may be implemented as distinct-region support rather than raw
chunk count. Any zero or implicit treatment must be explicit in the ranking
contract and protected by long-meeting, overlap, and ablation regressions.

## Local Reranking

The cross-encoder receives question/evidence pairs for a bounded top candidate
set. It reranks evidence and contributes to meeting ranking.

Reranking is the most expensive stage in an otherwise cheap pipeline. A
512-token cross-encoder pass on CPU costs roughly 30-80 ms per pair depending on
model size and the ORT intra-op cap, so the provisional 30-50 input range
implies **1.0-4.0 seconds for reranking alone** — potentially the entire Fast
preparation budget before embedding, FTS, vector scan, fusion, and hydration are
counted. The stage therefore has its own gate and an approved adaptive policy.

Requirements:

- Input truncation follows the reranker's tokenizer and manifest.
- Reranker inference is batched and cancellable between batches.
- Reranker failure preserves fused ordering.
- Search surfaces use local reranking without an LLM call.
- Reranker output is never presented as calibrated confidence unless Sprint 1
  proves calibration.
- The reranking stage MUST satisfy its own p95 sub-budget of **900 ms** on
  reference hardware, inside the overall Fast preparation budget.
- Sprint 1 MUST measure per-pair latency for the selected reranker and derive
  the maximum admissible candidate count from the sub-budget, rather than
  assuming the provisional 30-50 range is affordable.
- Adaptive reranking depth is pre-approved so Sprint 3 does not have to
  renegotiate a gate. The implementation MAY rerank a reduced head when the
  fused top-k margin is unambiguous, provided the policy is deterministic,
  recorded with Sprint 1 evidence, and evaluated by the corpus. It MUST NOT
  vary depth by wall-clock timing, which would make results irreproducible.
- The `Search` retrieval purpose MAY use a shallower approved depth than
  `Chat`. Sidebar search reranks on every debounced keystroke and does not
  need Chat-grade depth.

## Authoritative Hydration

After selecting meetings, load current content from authoritative tables. Do
not send only vector chunks to the answer model.

The top meeting receives:

- Current title and folder metadata.
- Latest non-empty summary.
- Current notes.
- Matched summary/note sections.
- Matched transcript windows rehydrated to current transcript segments.
- One adjacent segment on each side when it fits and improves continuity.
- Explicit transcript coverage.

Other selected meetings receive a guaranteed minimum evidence allocation, then
remaining context budget is relevance-weighted. The top meeting's mandatory
summary/notes cannot consume the entire multi-meeting budget.

Hydration verifies source content hashes. Stale semantic evidence is omitted
and its meeting remains eligible through lexical/current authoritative data.

## Context And Source Parity

Replace string-only broad context assembly with:

```rust
struct ContextBuild {
    markdown: String,
    retained_evidence_ids: Vec<String>,
    coverage: ContextCoverage,
}
```

`ChatSource` gains optional backward-compatible fields:

```text
chunkId
sourceKind
speaker
retrievalProvenance
```

Only evidence IDs retained after final prompt budgeting become sources. Summary
and notes that ground an answer are represented as sources. Existing persisted
source JSON remains readable because new fields are optional.

## Fast Mode

Fast performs one local retrieval pass:

```text
lexical + vector candidates
-> RRF
-> meeting aggregation
-> local reranking
-> authoritative hydration
-> final answer
```

Fast means one retrieval pass with no Deep planner call. The existing
follow-up query rewrite may use the configured Chat provider before retrieval,
so Fast does not mean that there can be no provider round-trip before final
answer generation.

## Deep Mode

Deep is the default for new interactive Chat conversations. It starts with the
Fast evidence set, then runs a bounded provider-independent planning loop.

Conceptual planner output:

```json
{
  "status": "search_more",
  "queries": ["regua whatsapp retencao dias"],
  "openMeetingIds": ["meeting-id"],
  "expandEvidenceIds": ["evidence-id"]
}
```

Allowed actions:

- Search the same authorized scope with additional queries.
- Open an authorized candidate meeting more deeply.
- Expand around retained transcript evidence.
- Finish with the current evidence.

Constraints:

- Maximum two additional retrieval rounds initially.
- Maximum three additional queries per round, each at most 256 Unicode
  characters.
- Maximum five opened meetings per round and eight across the request.
- Maximum ten expanded evidence IDs per round.
- Maximum planner input of 24,000 Unicode characters and strict output of one
  JSON object no larger than 8 KiB/512 output tokens.
- Planner generation MUST use planner-specific bounded-generation options inside
  the existing shared LLM client/request builder; it MUST NOT create a second
  provider client or move provider logic into `retrieval/agent.rs`. Every
  provider has a hard response-byte/parser cap, uses a provider output limit
  where supported, and receives a scoped child cancellation token. The child
  token is cancelled on both the per-call and total-preparation deadlines.
- Sprint 4 MUST record a capability/fallback matrix for every configured
  provider: OpenAI, Claude, Groq, Ollama, OpenRouter, BuiltInAI, and Custom
  OpenAI. The matrix records provider-specific output-limit support, the hard
  byte/parser cap, child-cancellation behavior, and the fallback. A provider
  that cannot enforce the requested generation or cancellation bound falls back to the current
  Fast evidence; the 512-token, 8 KiB, 15-second, and 30-second limits are not
  weakened.
- The required provider capability/fallback matrix MUST record supported or
  unsupported status for each capability and is:

  | Provider | Required capability record | Fallback when a required bound/capability is unsupported |
  |---|---|---|
  | OpenAI | Provider output-limit support, shared byte/parser cap, and child-cancellation behavior | Current Fast evidence |
  | Claude | Provider output-limit support, shared byte/parser cap, and child-cancellation behavior | Current Fast evidence |
  | Groq | Provider output-limit support, shared byte/parser cap, and child-cancellation behavior | Current Fast evidence |
  | Ollama | Provider output-limit support, shared byte/parser cap, and child-cancellation behavior | Current Fast evidence |
  | OpenRouter | Provider output-limit support, shared byte/parser cap, and child-cancellation behavior | Current Fast evidence |
  | BuiltInAI | Provider output-limit support, shared byte/parser cap, and child-cancellation behavior | Current Fast evidence |
  | Custom OpenAI | Provider output-limit support, shared byte/parser cap, and child-cancellation behavior | Current Fast evidence |

- Maximum 15 seconds per planner call and **30 seconds total Deep preparation**,
  reduced from 45 seconds because Deep is the default path and the budget is
  time a user spends watching nothing happen.
- Maximum two planner provider calls before final answer generation. Note that
  a Deep turn may additionally incur the existing follow-up query-rewrite call
  (`api/chat.rs:465-494`), so the worst case is four provider round-trips
  including final generation. Sprint 4 MUST report this total, not just the
  planner count.
- Every action is schema-validated and allow-listed.
- Meeting text is untrusted data and cannot alter system instructions.
- Scope cannot widen.
- `openMeetingIds` is additionally restricted to the bounded candidate or
  meeting-card IDs supplied to the current planner round; an existing ID that
  was not offered in that round is not authorized. `expandEvidenceIds` is
  restricted to evidence IDs currently known and retained by that round.
- Authoritative scope membership is revalidated after each round and
  immediately before final evidence/source publication.
- Planner output is internal and is not persisted as an assistant answer.
- Provider timeout, malformed output, refusal, or component failure falls back
  to the current Fast evidence.
- User/stream cancellation is a distinct typed outcome that aborts preparation;
  it never falls back into answer generation.
- Final answer generation continues through the existing streaming boundary.

Interactive Chat accepts a request-level Fast/Deep selection. New interactive
conversations explicitly send `deep`; selecting Fast explicitly sends `fast`.
An omitted mode in every legacy, non-interactive, or MCP contract resolves to
`fast`, while an explicit unknown mode is rejected. Live sends `fast` and uses
its direct path with no Deep planner. Do not add conversation persistence for
the mode unless a later approved task requires it. MCP Chat is not an
interactive conversation and remains Fast-only in this release. Deep through
unauthenticated localhost MCP is deferred until it has an approved
cancellation/cost contract.

### Deep Preparation Progress

Deep inserts up to 30 seconds of preparation in front of a UI that currently
begins streaming almost immediately. Static explanatory copy is not sufficient
for a default experience; silence reads as a hang.

- Deep preparation MUST emit stage-level progress events through the existing
  Chat event channel before final answer streaming begins, through Chat's
  current ownership/cancellation publication fence.
- Events carry **stage identity and counts only** — for example searching,
  ranking, reviewing N meetings, expanding evidence, or writing the answer.
- Events MUST NOT carry planner output, reasoning text, queries, meeting
  content, or any evidence text. The prohibition on displaying hidden planner
  reasoning is unchanged; this is a progress contract, not a reasoning display.
- Cancellation remains available and typed throughout the progress phase.
- Fast mode emits no preparation progress events; it has no user-perceptible
  preparation phase to report.

### Open Decision: Deep As Default

Deep-as-default is a recorded user decision and remains in force. It is flagged
here because the critique that produced this revision recommended re-examining
it, and the authority order requires the user, not an implementation agent, to
settle it.

The tension: Sprint 3 requires Fast alone to solve the reference case before
Deep exists, so Deep is a quality margin rather than the mechanism that fixes
the reported failure — and it costs up to 30 seconds and up to three extra
provider calls on every new conversation. Fast-by-default with Deep one click
away would deliver the same fix at a fraction of the latency and provider cost.

No implementation task may change the default. Sprint 4 MUST report measured
Deep preparation latency and provider-call counts, and the user MAY revisit the
default at Sprint 4 close with that evidence in hand.

### Mode Applicability By Scope

The selector MUST NOT present a choice that the backend ignores.

| Scope | Fast/Deep behavior | Required UI |
|---|---|---|
| All, folder, meeting, snapshot, today | Both modes are honored | Selector enabled |
| Live recording | The UI sends `fast`; retrieval is the direct in-memory transcript path and never enters Deep | Selector disabled, with a short explanation that live Chat reads the current transcript directly |
| MCP Chat | Omitted mode resolves to Fast; Fast-only by decision | Not applicable; no MCP selector exists |

## Scope Semantics

| Scope | Semantic behavior |
|---|---|
| All | Search all current persisted meetings. |
| Folder | Resolve selected folder plus descendants at request time, then search only meetings in that allow-list. |
| Meeting | Use hybrid transcript anchors while retaining the existing authoritative meeting builder and fallback. |
| Search snapshot | Treat frozen meeting IDs as an allow-list; rank current content by the current question. |
| Today | Treat date-derived meeting IDs as an allow-list; rank current content by the current question. |
| Meeting list | Use authoritative metadata listing; bypass semantic content search. |
| Live recording | Keep the existing in-memory transcript-tail path; never persist/embed unsaved live text. |

The existing today plus meeting-list intersection bug is a prerequisite fix:
when both intents apply, the title list must use the date intersection rather
than every title in the outer scope.

## Product Surfaces

### Chat

All streaming and non-streaming persisted Chat commands use the shared
`RetrievalService` through `prepare_chat_inputs`. Live Chat remains separate.

### Sidebar Search

Add a meeting-level hybrid search command and switch sidebar search after a
complete semantic generation activates. While unavailable, use current FTS.
Search snapshots continue storing displayed meeting IDs, so their conversation
identity remains retrieval-model-independent.

### Tauri APIs

Keep `api_search_fts` and existing BM25 behavior for compatibility. Add
explicit hybrid search/context commands. Do not reuse a `rank` field with a new
meaning.

Current surface classification:

| Surface | First-release contract |
|---|---|
| `api_search_transcripts` | Retain explicit legacy transcript lexical search. |
| `api_search_fts` | Retain explicit FTS/BM25 search. |
| `api_build_context` | Retain lexical context behavior. |
| New hybrid search command | Meeting-level hybrid + local reranking. |
| New hybrid context command | Hybrid retrieval with retained-source context. |
| Sidebar | Consume the new hybrid search command with lexical fallback. |
| Persisted Chat | Use shared hybrid retrieval according to rollout sprint. |
| MCP lexical tools | Retain current contracts. |
| MCP hybrid tools | Add separately named/versioned Fast hybrid contracts. |

### MCP

Keep existing lexical tools. Add versioned hybrid search and hybrid context
tools with explicit descriptions and provenance. Shared MCP Chat inherits
hybrid Chat preparation, but remains Fast-only. MCP hybrid tools remain
local-only and do not expose raw embeddings.

## Cancellation And Concurrency

- Query embedding, vector search, reranking, hydration, and Deep planning accept
  a request cancellation token.
- User/stream cancellation always propagates as cancellation and emits no final
  answer/source event. It is never a quality fallback.
- Check cancellation before/after every blocking/model/database stage and while
  waiting in a queue.
- One small Rust-owned request-ownership/cancellation mechanism, generalized
  from the accepted Sprint 3 Chat stream guard, serves Chat and sidebar
  requests. It is keyed so those surfaces may coexist; Sprint 4 Task 4.1
  establishes it and Sprint 5 Task 5.2 reuses it. There are no parallel request
  registries. Interactive Tauri search/context requests carry a request ID, and
  non-streaming Chat requests use the same mechanism and its explicit cancel
  path. Replacement uses ownership checks so stale or older work cannot publish
  final content.
- Deep progress is emitted by the Chat adapter through that same stream-ID
  ownership/cancellation publication fence. `retrieval/agent.rs` MAY accept a
  privacy-safe sink, but MUST NOT own `AppHandle`, Tauri events, or a second
  event bus.
- Terminal, error, and timeout paths MUST clean up ownership entries. Tests MUST
  cover stale, replaced, and cancelled progress, cleanup on each terminal path,
  and bounded request-registry lifetime.
- MCP hybrid search/context is Fast and strictly bounded and does not claim
  public client cancellation in the first release. MCP Chat remains Fast-only.
  The server owns an internal deadline cancellation token and passes it through
  shared retrieval; a timeout cancels queued/running scheduler and ONNX work
  even without a public MCP cancel API.
- Blocking operations use `spawn_blocking` or a dedicated bounded worker.
- Do not hold async locks across ONNX inference or vector scans.
- One global retrieval scheduler prioritizes interactive Chat/search over
  indexing. The initial policy allows one concurrent ONNX inference pipeline,
  at most two concurrent vector scans, and at most eight queued interactive
  requests; Sprint 1 may lower these values from measurements.
- Index work runs only when no interactive model permit is waiting. It pauses
  within 250 ms of active recording, audio import, or retranscription signals
  identified in Sprint 2, and resumes after pressure clears.
- ORT uses one inter-op thread and a Sprint 1 measured intra-op cap no greater
  than `min(4, max(1, available_parallelism / 2))` unless platform evidence
  approves another cap.
- The index worker is independent of the one-active-Chat-stream invariant.
- Index snapshot swaps are atomic from the reader's perspective.
- Model sessions are lazy, cached by exact bundle identity, and thread-bounded.
- Recording qualification requires no new audio overflow/drop warning and no
  more than 10% p95 transcription-throughput degradation versus indexing
  paused on reference hardware.

## Failure And Fallback Matrix

| Failure | Required behavior |
|---|---|
| Model resource missing/corrupt | Mark semantic unavailable, report diagnostics, use FTS. |
| Tokenizer/model mismatch | Reject bundle, never publish vectors, use FTS. |
| Prior-model bundle absent or unverifiable after upgrade | Report the prior generation's model as unavailable; use FTS-only until the new generation activates. Never score prior-generation vectors with another model's query embeddings. |
| Prior-model engine refused by the RAM gate | Same as absent: FTS-only, reported as a measured envelope state, never partially resident. |
| Initial backfill incomplete | Use FTS-only retrieval until complete activation. |
| Meeting dirty | Exclude stale semantic rows for that meeting; allow current FTS/hydration. |
| Vector BLOB malformed | Quarantine/requeue affected meeting; continue without that vector. |
| Terminal per-meeting semantic failure | Serve named quarantined coverage: retain canonical rows for retry/rebuild, exclude that meeting from semantic snapshots, report `failed_meetings`, and use durable FTS for it. |
| Exact cache unavailable | Rebuild asynchronously; use FTS. |
| ANN sidecar missing/corrupt | Rebuild from SQLite; use exact if available, otherwise FTS. |
| Query embedding error | Continue with FTS and hydration. |
| Reranker error | Continue with fused ordering. |
| Deep planner error | Continue with Fast evidence. |
| User/stream cancellation | Propagate typed cancellation; stop preparation and emit no stale/final answer or source event. |
| Scope validation failure | Fail closed before retrieval. |
| Database/index write contention | Retry derived work; never fail the primary meeting mutation. |
| User forced lexical-only | Read the one persisted setting at the shared Rust preparation/service boundary; use FTS for every initial/additional Deep retrieval and every Tauri, MCP, or sidebar hybrid request, preserving the typed `ForcedLexical` reason. Report it as an explicit user-selected state, not a failure. |
| Derived disk envelope exceeded | Report in diagnostics, block generation activation, and offer rebuild/cleanup; never delete primary data automatically. |

## Security And Privacy

- Model artifacts are pinned, hash-verified, license-reviewed, and included in
  signed packages.
- Runtime model paths are fixed application resources, never renderer- or
  user-controlled extension paths.
- No SQLite extension loading is required.
- Semantic documents and vectors are local derived sensitive data.
- Meeting deletion cascades derived data and publishes durable index tombstones.
  Search also rechecks current meeting existence before returning results.
- Existing assistant/user answer text is retained when a meeting is deleted,
  matching the approved conversation-history decision. Persisted source JSON
  for that meeting is scrubbed of source snippets/navigation metadata in
  meeting-scoped and broad messages. Orphaned threads disclose that answer text
  may still quote deleted content.
- Every server-side Chat message persistence path sanitizes source arrays
  against current meetings in the same message-insert transaction. A meeting
  deleted after source emission but before delayed save therefore cannot be
  re-persisted. Meeting deletion also invalidates/cancels the active Chat stream
  when its prepared evidence contains that meeting, and source existence is
  rechecked before final source/done emission.
- Scope restrictions apply before reranking and hydration.
- Deep planner actions are data, not executable instructions.
- The single persisted `force_lexical_retrieval` setting is read at the shared
  Rust preparation/service boundary and governs initial and additional Deep
  retrieval plus every Tauri, MCP, and sidebar hybrid request. It preserves the
  typed `ForcedLexical` reason. Enable-next-request, restart, and
  disable-restore tests cover Fast, Deep, and all hybrid surfaces; no second
  setting or diagnostics service may be introduced.
- Prompts label retrieved content as untrusted meeting evidence.
- Logs contain lengths, counts, stage timings, model IDs, and status, never raw
  queries, transcript text, notes, summaries, embeddings, or API keys.
- Existing localhost MCP access is not strengthened by this program; do not
  claim semantic search introduces authentication.

## Observability

Record privacy-safe diagnostics:

- Active model and chunker IDs.
- Semantic generation state and coverage.
- Dirty/backfill meeting counts.
- Index backend and document count.
- Stage timings for query embedding, FTS, vector search, fusion, reranking,
  hydration, planner rounds, and context build.
- Candidate and selected meeting counts.
- Fallback reason counters.
- Index worker errors and last successful progress.
- Current/peak estimated index memory where practical.

Do not add remote telemetry. Diagnostics are local logs/status APIs.

## Settings And UX

Add retrieval status under Settings after the backend status contract exists:

- Bundled model identity and license link.
- Indexed/total meeting count.
- Active, building, paused, failed, or lexical-only state.
- Background progress.
- Pause/resume indexing.
- Rebuild semantic index.
- Last error with retry.
- Estimated local index size, shown against the approved disk envelope.
- **Force lexical-only retrieval** (see below).

The model is bundled, so there is no download/delete workflow. Rebuild deletes
only derived semantic state, never transcripts, summaries, notes, or FTS.

### Retrieval Kill Switch

Hybrid retrieval replaces the primary Chat retrieval path. Index pause and
rebuild only affect derived state; neither returns a user to the previously
shipped retrieval behavior. Without a runtime control, recovery from a bad
result on a user's real corpus requires a reinstall.

- A persisted `force_lexical_retrieval` setting MUST exist and MUST be
  surfaced in Settings.
- The setting is read at the shared Rust preparation/service boundary. When
  enabled, every initial/additional Deep retrieval and every retrieval surface
  — Chat in all scopes, sidebar search, Tauri hybrid commands, and MCP hybrid
  tools — takes the same lexical path used when semantic state is unavailable.
  No new code path is introduced; the switch reuses the existing fallback and
  preserves the typed `ForcedLexical` reason.
- It takes effect on the next request without restart, and does not delete,
  invalidate, or pause the semantic index. Turning it off restores hybrid
  behavior immediately.
- Diagnostics MUST report it as an explicit distinct reason, so a
  user-forced lexical state is never mistaken for a model failure.
- Fast/Deep and all-surface tests MUST cover enabling for the next request,
  persistence across restart, and disabling to restore hybrid behavior. No
  second setting or diagnostics service may be introduced.

Chat exposes Fast and Deep. New interactive conversations explicitly send
`deep`; omitted mode on legacy/non-interactive/MCP contracts resolves to
`fast`, explicit unknown mode is rejected, and live sends `fast` without a Deep
planner. The UI explains
that Deep may take longer and use additional requests to the configured Chat
provider, and shows stage-level progress during Deep preparation. The selector
is disabled in live-recording scope, where mode has no effect. No hidden
reasoning/planner output is displayed.

## Evaluation Contract

Sprint 1 creates a private, repository-safe evaluation corpus. It MUST contain
Portuguese and English questions and include:

- Exact-term fact lookup.
- Paraphrased semantic lookup.
- Number/date/list questions.
- Similar-topic distractor meetings.
- Multi-meeting synthesis.
- Follow-up questions requiring rewrite.
- Folder, all, meeting, snapshot, and today scope cases.
- Summary-only, notes-only, and transcript-only evidence.
- Deleted/stale/dirty meeting cases.

Metrics:

- Meeting Recall@1, Recall@3, and Recall@5.
- Mean reciprocal meeting rank.
- Evidence Recall@K.
- Reranker NDCG or pairwise accuracy.
- Required-fact coverage.
- Citation/source precision.
- Fallback correctness.
- Fast and Deep end-to-end answer completeness.
- Query-stage p50/p95 latency.
- Backfill throughput.
- Peak retrieval RAM and on-disk size.

Deterministic expected meetings/evidence/facts are primary acceptance evidence.
An LLM judge MAY supplement but MUST NOT replace deterministic checks.

### Baseline Failure Reproduction

A synthetic fixture that today's retrieval already answers correctly proves
nothing, and the same agent that must beat the reference case also authors the
fixture standing in for it. The corpus therefore carries an explicit
falsifiability requirement:

- The synthetic reference case MUST **fail** under the current FTS-only
  baseline in the same mode as the observed production failure: isolated
  numeric fragments, incomplete schedule, missing MPV distinction.
- The harness MUST assert that baseline failure explicitly, as a passing test.
  If a change ever makes the baseline succeed, that assertion fails and the
  fixture is revealed as unrepresentative.
- The same requirement applies to every case in the semantic/paraphrase
  category: each MUST be shown to be under-served by the FTS baseline, or it
  cannot be used to claim a semantic improvement.
- Categories that exist to protect against regression — exact terms, numbers,
  names — are exempt. Those SHOULD pass on the baseline, and their gate is
  no-regression rather than improvement.

### Corpus Solvability

"Baseline Failure Reproduction" constrains the corpus in one direction only: the
fixture must be hard enough that the FTS baseline fails it. Satisfying that
requirement alone is trivial and useless — a corpus can be made impossible for
*every* retriever, which passes the letter of the falsifiability rule while
destroying its purpose. Task `1.2` did exactly this, and Task `1.3` spent an
L-sized benchmark discovering it. Both constraints are therefore normative, and
neither may be satisfied at the other's expense.

- Every case MUST be solvable from its own text. The expected evidence MUST be
  closer to the query than each of that case's distractors on at least one
  retrieval channel — lexical overlap, or semantic relatedness a multilingual
  bi-encoder can be expected to represent.
- A distractor MUST NOT contain the query verbatim, or contain a superset of the
  query's content terms, unless the expected evidence also contains them. A
  target that shares less surface with the query than its distractors do is not
  a hard case; it is an unanswerable one.
- The harness MUST assert structural solvability as a passing test, computed
  from fixture text without consulting the answer key: query-copy/superset
  distractors, nonce discriminators, duplicated templates, and non-varying
  ranking attributes are rejected directly.
- A separate supervised margin assertion MAY use expected IDs only to label
  which raw-text evidence is the target. Every channel score and target-versus-
  strongest-distractor margin MUST be computed from fixture text; IDs MUST NOT
  contribute to the score or bypass retrieval. An oracle that returns
  `required_evidence_ids` directly does not satisfy this: it proves the scoring
  code can score a perfect result, not that a retriever could produce one.
- Cases MUST be materially distinct from one another. A corpus generated by
  interpolating an ordinal into a shared template has an effective sample size
  equal to its template count, not its case count, and cannot satisfy "Corpus
  Size Floors" no matter how many rows it emits.
- Nonce tokens (`Sintetico42`, `Cedar42`) MUST NOT carry the discriminating
  signal of a semantic case. They tokenize to subword noise in a multilingual
  encoder and defeat the paraphrase relation the case exists to test.
- Fixture attributes that a ranking input depends on MUST vary across meetings
  within a case. Identical titles, identical dates, or a folder allow-list that
  excludes nothing render title overlap, recency semantics, and scope isolation
  untestable while appearing to be covered.
- Every zero-tolerance gate a case participates in MUST carry a supervised
  admissibility proof: an existence check, computed from fixture text with
  expected IDs used as labels only, that some retrieval ordering satisfies the
  gate. Solvability proven only through a channel the production pipeline does
  not implement (for example a hand-authored concept lexicon) does not count —
  the discriminating margin must exist on a production-implementable channel
  (lexical, title, or the measured vector channel). A forbidden fact whose
  only carrier is current authoritative content inside the expected meeting is
  not retrieval-admissible and MUST be classified as an answer-stage fact.

**Diagnostic signal.** If two unrelated model families — different
architectures, dimensions, or training corpora — produce identical aggregate
metrics on this corpus, the corpus is the deciding variable rather than the
model. Treat that as evidence of a corpus defect and stop; do not tune
constants, change models, or adjust gates against it.

### Corpus Size Floors

Percentage gates are uninterpretable without sample size. At twenty cases a
single case moves Recall@3 by five points and a "+10 percentage point"
improvement is one or two cases of noise. Minimum counts:

| Scope | Minimum cases |
|---|---|
| Total corpus | 120 |
| Each required category in the list above | 15 |
| Portuguese subset | 40 |
| English subset | 40 |
| Reference/critical designated cases | 5 |

Below these counts a gate is reported as **indicative only** and cannot be used
to close a sprint. Sprint 1 MAY propose different floors with a documented
power/precision argument, but MUST NOT close with unstated sample sizes.

Sprint 1 does not close until it publishes a numeric gate table approved by the
user. Minimum defaults, which may only be tightened or changed by explicit
approval after baseline evidence, are:

| Gate | Minimum |
|---|---|
| Reference/critical meeting hydration (rank within the hydrated set, currently top 5) | 100%. **Sprint 1 model-selection gate.** This is the property that determines whether the answer can be correct: a critical meeting inside the hydration window contributes its evidence to the prompt. Corroborated by critical required-fact coverage and critical retrieval-stage contamination, which are gated separately |
| Reference/critical meeting Recall@1 | 100%. **Sprint 3 release gate, not a Sprint 1 model-selection gate** (2026-08-24). Ordinal position is produced by fusion, meeting aggregation, and reranking — built and tuned in Sprint 3 Task 3.2 — not by the embedding pair Sprint 1 selects. The threshold is unchanged at 100%; only the sprint that owns it moved. It MUST be re-measured at Sprint 3 close and MUST pass before release. The pinned Reference Acceptance Case retains its own rank-1 requirement in Sprint 1 |
| Scope safety | 100%; no out-of-scope, deleted, or stale-derived evidence enters ranking, hydration, prompts, or sources |
| Ranked-output retained-ID precision | 100% for the ranked/context-builder boundary. This is not final prompt/source parity |
| Final prompt/source parity | Exactly the user-visible and persisted sources whose evidence remains after authoritative hydration and final provider budgeting; owned by hydration/caller integration, not Task 3.2 ranking |
| Overall meeting Recall@3 | At least 95% |
| Overall meeting Recall@5 | At least 98% |
| Required evidence Recall@10 | At least 90% |
| Exact-term category | No case moves expected meeting below top 3; aggregate Recall@3 is not below FTS baseline |
| Semantic/paraphrase category | At least +10 percentage points Recall@3 over FTS, or at least 95% when baseline is already above 85% |
| Reranker designated cases | Improves pairwise/NDCG metric and causes no critical-case regression |
| Forbidden-fact contamination in critical cases (retrieval stage) | 0 for forbidden facts carried by superseded, stale-derived, or deleted sources. Forbidden facts carried by current authoritative content inside a correctly retrieved meeting are answer-stage facts: retrieval delivers that content by design (hydration includes current notes wholesale), and the safety property is that the **answer** does not assert them — evaluated by the answer-stage non-assertion gate below, not by this row. Each critical forbidden fact is classified by its carrier's source state, computed from fixture text |
| Answer-stage non-assertion | The generated answer asserts no forbidden fact present in its retrieved context (the Reference Acceptance Case's "does not reduce the schedule to only 3 and 4 days" is this gate's pinned instance). Sprint 3.5 owns the first folder/all product-path measurement; Sprint 4.5 repeats it for every persisted Fast/Deep scope; Sprint 5.5 repeats installed-release qualification. Retrieval-only tests MUST NOT claim this gate |
| Gate admissibility | Every zero-tolerance gate carries a supervised existence proof — computed from fixture text with expected IDs as labels only — that at least one retrieval ordering satisfies it, before any model is benchmarked against it. A gate without an admissibility proof is not a gate; it is an unfalsifiable trap (materialized twice: `1.2` corpus solvability, `1.3` rerun critical contamination) |
| Retrieval RAM at 250k | `<=1 GiB` automatic pass; `>1 GiB` through `1.25 GiB` requires explicit user risk/quality approval; `>1.25 GiB` fails without a product scope change. Includes active sessions, ANN graph if selected, and old/new snapshot overlap. |
| Derived disk at 250k | `<=2 GiB` steady state and `<=3 GiB` during shadow rebuild automatic pass; above either figure requires explicit user approval. Includes documents, staging, sidecars, and all retained generations. |
| Baseline falsifiability | Reference and semantic-category cases demonstrably fail the FTS-only baseline; asserted by the harness |
| Corpus solvability | Every case satisfies "Corpus Solvability"; asserted by the harness from fixture text without consulting the answer key. A corpus may not pass falsifiability by being unanswerable |
| Corpus size | Meets the floors in "Corpus Size Floors"; every reported percentage carries its denominator |

The approved Sprint 1 gate table records corpus counts so percentages cannot be
interpreted without sample size.

## Reference Acceptance Case

For the WhatsApp retention question:

- Folder and all-meetings candidate ranking returns synthetic fixture meeting
  `fixture-whatsapp-retention` at rank 1. The mapping to any private live
  meeting remains outside source control and tracked documentation.
- Hydrated context contains `1, 3, 7, 10 and 15`.
- Hydrated context contains the MPV/non-MPV day-one distinction.
- Context retains evidence for any additional statement about email or
  suppression checks before the answer may include it.
- The answer does not reduce the schedule to only 3 and 4 days.
- Every source shown to the user was present in the final answer prompt.

## Performance Gates

Sprint 1 defines reference hardware and records results. Provisional release
ceilings:

- Up to 250,000 semantic documents.
- Target at most 1 GiB peak retrieval RAM, including vector snapshot, ANN graph
  when selected, active retrieval model sessions, delta/tombstones, reader-held
  old snapshots, and shadow activation overlap. A measured 1-1.25 GiB result
  blocks release until explicitly approved; above 1.25 GiB fails the
  architecture gate.
- Target at most 2 GiB derived disk in steady state and 3 GiB during shadow
  rebuild. Above either figure blocks release until explicitly approved.
- Vector-search stage p95 below 500 ms on reference hardware.
- Cross-encoder reranking stage p95 below 900 ms on declared Windows x64
  reference hardware. This is a sub-budget of Fast preparation, not additional
  to it. The authoritative run uses the hash-verified production bundle and
  `RetrievalModels::rerank` in a Rust `--release` build. After explicit warm-up,
  collect at least 50 complete depth-50 request samples; report p50, p95,
  maximum, sample count, actual pair count, bundle/manifest identity, ORT
  provider/thread settings, OS, and hardware. The measurement includes
  production tokenization, blocking-pool dispatch, and ONNX inference, but
  excludes scheduler queue wait and the rest of Fast preparation. Bundle
  absence, artifact mismatch, fewer than 50 pairs, or insufficient samples
  fails the evidence run rather than skipping. `solo-pair p95 * depth` is a
  historical derived estimate only and cannot close the release gate.
- Fast local retrieval preparation p95 below 2 seconds excluding final LLM
  answer generation. If Sprint 1 measurement shows the selected reranker cannot
  fit its sub-budget at any useful candidate depth, the correct response is an
  approved change to this gate with recorded evidence, not silent depth
  reduction below evaluated quality.
- Deep preparation p95 below 30 seconds including all planner calls, with
  stage-level progress events emitted throughout.
- App startup is not blocked by backfill.
- Interactive audio capture shows no new drop/overflow warning and no more than
  10% p95 transcription-throughput degradation under the scheduler test.

If no candidate model/backend satisfies these gates, Sprint 1 stops for an
architecture decision. It does not silently lower quality, scale, or platform
support.

### Gate Ownership And Evidence Lifecycle

Historical selection harnesses prove why a model/backend/depth was chosen; they
are not required release artifact layouts. Current acceptance uses the smallest
production path that measures the stated behavior:

- Task correctness gates block dependent code only when the contract itself is
  unsafe or failing. Hardware/package evidence may move to the sprint's
  dedicated validation task when later integration is required to measure it.
- Sprint/release gates still block sprint or release closure. Moving evidence
  ownership never lowers a threshold or converts a failed gate into a pass.
- Quality gates use the exact returned production pipeline. Oracle output,
  reconstructed ranking, printed diagnostics, or skipped artifact-gated tests
  are not acceptance evidence.
- Release performance uses the current signed production bundle and runtime.
  A legacy tree containing retired candidate models or benchmark-only f32
  exports MUST NOT be reconstructed or packaged merely to run historical code.
- Refusal rate is diagnostic, not a percentage threshold. Record a predeclared
  eligible-trial denominator, admissions, RAM refusals, and non-RAM exclusions;
  the fail-closed activation ceiling itself remains unchanged.

## Packaging And Platform Gates

These gates apply to **Windows x64 only**, per "Platform Scope". macOS ARM64 and
Linux x64 gates are deferred with the platforms themselves.

Before Sprint 2, the Windows x64 CI runner must execute reference tokenizer,
embedding, and reranker inference for the selected pair. Sprint 5 additionally
proves installed-package inference.

For Sprint 1 on Windows x64:

- CI fetches/verifies model artifacts and runs reference tokenizer, embedding,
  and reranker inference from the staged resource layout.
- A known sentence tokenizes/embeds to the expected dimension and tolerance.
- A known question/evidence pair produces the expected finite reranker order.
- The reference outputs are recorded in a platform-neutral form so a future
  macOS/Linux enablement can be checked against the same expectations.

For Sprint 5 on the installed Windows package:

- The Tauri package includes all model/tokenizer artifacts and licenses.
- Installed application locates the signed resource path and loads both ONNX
  sessions.
- A minimal hybrid query returns the expected fixture.
- Missing/corrupt-resource simulation produces lexical fallback, not startup
  failure.

### Toolchain Contract (Task 1.5, 2026-08-25)

The locked dependency graph requires Rust 1.88: `ort` 2.0.0-rc.10 itself
requires Rust 1.81, while locked transitive crates require 1.88. Task `1.5`
updates the workspace and Tauri crate declarations to `rust-version = "1.88"`
and pins `upstream/rust-toolchain.toml` to `1.88.0`. Review follow-up `1.R1`
configures the active repository-root Windows workflow to read, install, and
assert that same exact version in both jobs. CI MUST NOT substitute unpinned
`stable` for this contract. Future dependency updates that raise the floor must
update all declarations and exercise the new exact toolchain in the active
workflow.

## Migration And Rollback

Semantic schema changes are additive. Migration creates tables/triggers and
queues backfill; it performs no inference.

Rollback principles, ordered from cheapest to most disruptive:

1. **Force lexical-only setting.** Runtime, reversible, no data change, no
   restart. This is the first response to a retrieval-quality problem in the
   field and the only one a user can perform themselves.
2. **Pause indexing.** Stops semantic indexing only; FTS repair and
   publication/catch-up continue, and the active generation stays queryable.
3. **Rebuild semantic index.** Deletes derived state only; primary data and FTS
   are untouched.
4. **Ship a build with semantic paths disabled.** Requires a release.
5. **Binary rollback across the semantic migration.** Requires a verified
   pre-upgrade database backup; see below.

- Disabling semantic retrieval leaves FTS and primary meeting data usable.
- Derived tables and sidecars can be deleted/rebuilt without data loss.
- A shipped SQL migration is not reversed destructively in-place. The current
  SQLx migrator may reject a database containing migrations unknown to an older
  binary, so binary rollback requires a verified pre-upgrade database backup.
  Do not claim an older application can simply ignore additive tables unless a
  separately approved migrator-policy change and old-binary startup test proves
  it.
- Model upgrade activation retains the old semantic generation until the new
  one is complete and validated. Retention keeps that generation *queryable*
  across the upgrading restart only when its own embedding engine is
  retained and admissible; otherwise the old generation remains stored and
  pointed to but semantically unavailable, and retrieval is FTS-only until
  the new generation activates.
- External lexical API contracts remain available.

## Test Strategy

### Unit Tests

- Manifest validation and artifact hashes.
- Tokenizer reference vectors.
- Pooling and normalization.
- Deterministic chunking and document IDs.
- Vector encode/decode validation.
- RRF and meeting aggregation.
- Scope filtering.
- Planner action parsing/validation.
- Context budget and retained-source parity.

### Repository And Migration Tests

- Initial migration/source-revision state and per-generation work.
- Trigger coalescing for transcript, summary, notes, and title changes.
- Empty meeting insert/title profile indexing.
- Durable FTS repair after failed post-commit refresh.
- Meeting-delete cascade, Chat source scrub, and index tombstone replay.
- Source-revision compare-and-swap replacement.
- Canonical/published journal crash replay.
- Crash/retry behavior.
- Poison-item backoff/non-starvation.
- Model generation activation.

### Integration Tests

- FTS-only, vector-only failure, and hybrid paths.
- Folder descendants and stale FTS folder metadata.
- Snapshot/today allow-lists.
- Saved-meeting authoritative hydration.
- Deep loop bounded actions and fallback.
- Explicit interactive `deep`/`fast` propagation, omitted-mode Fast
  compatibility, rejected unknown mode, live Fast/no-Deep behavior, and an MCP
  behavioral regression through shared Chat preparation.
- One shared Rust ownership/cancellation mechanism, Chat-fenced progress,
  stale/replaced/cancelled progress, terminal/error/timeout cleanup, and bounded
  registry lifetime.
- Planner provider capability/fallback matrix and hard output/deadline bounds.
- Sidebar, Tauri, and MCP contracts.

### Performance Tests

- Synthetic 12k, 50k, and 250k corpora.
- Cold/warm index load.
- Exact and candidate ANN backends.
- Reranker batch sizes.
- Backfill throughput and cancellation.
- Query during recording/index pause.

### Packaged Smoke Tests

- Installed application model loading and inference on Windows x64, the only
  supported target this release.
- Every claimed Windows installer (currently MSI and NSIS) is installed silently
  and its installed `meetily.exe` is run with `--smoke-dbstat` before artifact
  upload. NSIS installs into an isolated runner-temporary directory. MSI is
  directed there too, but `INSTALLDIR` binds only if the generated WiX exposes
  that property as its root directory; when it does not, msiexec still succeeds
  into the default location, so the MSI smoke also searches
  `%ProgramFiles%\meetily` and `%ProgramFiles(x86)%\meetily` and checks the same
  roots for residue after uninstalling. The diagnostic uses an in-memory
  migrated SQLite database, registers one fixed model and generation, and calls
  `RetrievalRepository::derived_disk_usage`. The asserted invariant is that the
  shipped SQLite exposes `dbstat` and returns an exact allocated-page
  measurement; the byte total is reported, never asserted, because it tracks the
  derived schema's page layout and legitimately moves with every migration.
- The verdict crosses the process boundary as an exit code and nothing else,
  because release builds are `windows_subsystem = "windows"` and their printed
  output reaches no console. The codes are a contract between
  `run_dbstat_smoke` and the root workflow's gate:

  | Code | Meaning | Emitted by |
  |---|---|---|
  | `0` | Exact `dbstat` measurement — the smoke passes | diagnostic |
  | `1` | Packaged smoke harness failed: installer, executable discovery, diagnostic timeout, or teardown | workflow step |
  | `2` | `dbstat` unavailable — the shipped SQLite lacks `ENABLE_DBSTAT_VTAB` | diagnostic |
  | `3` | Async runtime construction failed before a verdict | diagnostic |
  | `4` | SQLite connection failed before a verdict | diagnostic |
  | `5` | Migration failed before a verdict | diagnostic |
  | `6` | Deterministic model/generation setup failed before a verdict | diagnostic |
  | `7` | Measurement failed before a verdict | diagnostic |

  Codes `3`-`7` name the stage only; the underlying error is printed where a
  console exists and never crosses the boundary, so no SQL, path, or user
  content reaches a public log. A teardown problem downgrades a passing run to
  `1` but never overwrites a verdict the diagnostic already returned, because
  `2` is the condition this smoke exists to detect. Any other code is reported
  as unrecognized rather than mapped. The installer is uninstalled and the
  temporary directory removed after each run.
- Windows CUDA/Vulkan Whisper variants do not change ORT correctness.
- Missing resource and corrupt sidecar fallback.
- Forced lexical-only setting produces lexical behavior on every surface.

This installed-package `dbstat` contract remains unverified until the root
Windows workflow completes an actual MSI and NSIS run; local unit coverage and
package construction do not count as a passing installed-package smoke.

## Subagent Guardrails

Every implementation subagent MUST:

- Read this document and its exact sprint/task section before editing.
- Trace every caller of a shared function it changes.
- Preserve FTS fallback and scope isolation.
- Keep semantic state derived and disposable.
- Add the smallest runnable regression required by the task.
- Use the approved model/backend decision from Sprint 1; do not substitute a
  dependency or model silently.
- Keep raw meeting text and queries out of logs.
- Stop on an unapproved migration, remote service, model license issue,
  external contract break, or data-loss risk.
- Report omissions and spillover instead of absorbing adjacent tasks.
- Avoid editing another task's in-progress files.

Subagents MUST NOT:

- Replace FTS with vector-only search.
- Generate embeddings inside primary-data transactions or SQL migrations.
- Activate partial initial semantic coverage.
- Send content to remote embedding providers.
- Trust renderer-provided scope content.
- Expose raw cosine/BM25/RRF values as one public `rank` field.
- Build sources before final context budgeting.
- Add a native SQLite extension without a user-approved architecture change.
- Claim packaged support without executing installed-package inference tests.
- Claim macOS or Linux support. This release is Windows x64 only.
- Reintroduce a fixed byte-width `CHECK` on the vector column.
- Add `'active'` back to the generation state enum.
- Select an ANN backend in response to a memory-budget failure.
- Present a Fast/Deep control in a scope whose retrieval ignores it.
- Emit planner text, queries, or evidence content through progress events.

## Review Gates

Every sprint requires code and architecture review.

Architecture reviewers focus on:

- Sprint 1: model license/package contract and benchmark validity.
- Sprint 2: migrations, trigger durability, worker concurrency, generation
  activation, and deletion safety.
- Sprint 3: scope filtering, ranking, hydration, prompt budgets, and source
  parity.
- Sprint 4: planner safety, cancellation, provider behavior, and bounded cost.
- Sprint 5: external contracts, Windows x64 packages, scale, recovery, and
  privacy claims.

No sprint closes with unresolved blocker or should-fix findings.

The Sprint 3 release gates named in the R40 carry-forward section remain open
until independently valid evidence closes them. They are mandatory for Task
4.5, Sprint 4 close, Task 5.5, and release close; V1-V10/rejected corpus
fixtures are not acceptance evidence, and corpus-free internal production tests
are diagnostic only.

## Decisions Deferred To Measured Gates

These are not open product questions. Sprint 1 must resolve them with recorded
evidence:

- Exact embedding and reranker models/revisions, **from the admissible set
  defined by "Resource Budget Arithmetic"**.
- Vector encoding (`f32`, `fp16`, or `int8`) and, when quantized, its
  dequantization parameters and measured recall cost.
- Token-window profile.
- Whether all summary templates improve retrieval enough to index.
- Exact vector scan versus exact plus HNSW, **decided by the Backend Decision
  Rule table, not by an undifferentiated scale gate**.
- Reranker candidate depth derived from the 900 ms sub-budget, and the adaptive
  depth policy if one is adopted.
- Measured per-document derived disk cost and the projected 250k footprint.
- Lexical normalization/core-term and high-frequency-word policy.
- Candidate limits, RRF constant/weights, and meeting-aggregation constants.
- Reranker batch size and candidate count.
- Retrieval scheduler queue/concurrency and ORT intra-op thread caps.
- Numeric quality table values when the approved defaults require adjustment
  from measured corpus size/baseline evidence.
- Reference hardware and final latency thresholds within approved product
  constraints.

Any failure to resolve one of these gates blocks Sprint 2 approval.

## Rejected Alternatives

| Alternative | Reason rejected |
|---|---|
| Vector-only RAG | Loses exact names, dates, numbers, and graceful fallback. |
| Keep global chunk ranking | Reproduces the current incomplete-context failure. |
| Remote embeddings | Conflicts with the approved local-bundled decision and privacy positioning. |
| Bundle only an embedding model | User approved local reranking and Deep retrieval as part of the target quality. |
| `sqlite-vec` immediately | Adds native packaging while normal search remains exact; benchmark pure Rust first. |
| Separate vector service | Unnecessary process, deployment, security, and lifecycle complexity. |
| Synchronous save-time embedding | Increases data-write latency and couples primary data to model health. |
| Partial-generation activation | Biases results toward meetings indexed first. |
| Silently change MCP/BM25 rank semantics | Breaks concrete external consumers and observability. |
| Embed live transcript continuously | Adds resource/privacy/lifecycle complexity; live Chat already has an authoritative in-memory path. |
| Require macOS/Linux inference gates without macOS/Linux CI | The gate could never be satisfied in this fork, so it would have silently blocked Sprint 1 forever or been quietly ignored. Deferring the platform is honest; an unsatisfiable `MUST` is not. |
| ANN as the remedy for a memory-budget miss | An HNSW graph is stored in addition to the vectors; it cannot reduce a footprint it adds to. |
| `WITHOUT ROWID` for `retrieval_documents` | Rows carry multi-KB vectors and chunk text, far above the small-row profile the storage form is designed for. |
| Fixed `length(vector) = dimensions * 4` constraint | Hardcodes f32 and forbids the quantization path the RAM envelope depends on. |
| `'active'` as a generation state alongside the singleton pointer | Two representations of one fact with nothing keeping them consistent. |
| Code revert as the only retrieval rollback | Requires a rebuild and reinstall to recover from a bad result on a user's real corpus. |
| Static copy alone as the Deep latency mitigation | Up to 30 seconds of silence in a streaming UI reads as a hang regardless of what a tooltip said earlier. |

## Decision Log

| Date | Decision | Rationale | Approved by |
|---|---|---|---|
| 2026-08-21 | Store this program under `docs/hybrid-rag/`. | Keep architecture and per-sprint records together. | User |
| 2026-08-21 | Use local bundled embeddings. | Preserve local processing and offline retrieval. | User |
| 2026-08-21 | Optimize for quality without a strict total model-asset limit. | Retrieval quality is the primary product target. | User |
| 2026-08-21 | Include local reranking and iterative LLM retrieval. | Reach beyond one-shot vector retrieval. | User |
| 2026-08-21 | Support up to 250,000 documents with a quality-first 1 GiB target and approval band through 1.25 GiB. | Make the approximate user-selected envelope testable without silently rejecting a material quality win or releasing above target. | User |
| 2026-08-21 | Roll out broad Chat first, then all saved scopes. | Fix the demonstrated gap while preserving staged validation. | User |
| 2026-08-21 | Target all current desktop platforms. | Preserve the supported product footprint. | User |
| 2026-08-21 | Backfill automatically after launch. | Avoid manual setup while keeping startup non-blocking. | User |
| 2026-08-21 | Expose Fast/Deep and default new Chat conversations to Deep. | Make highest quality the normal experience while retaining a faster option. | User |
| 2026-08-21 | Extend all retrieval surfaces. | Keep Chat, sidebar, context APIs, and MCP capabilities aligned. | User |
| 2026-08-21 | Keep answer text but scrub deleted-meeting source snippets/navigation metadata. | Preserve conversation history while reducing retained source copies after deletion. | User |
| 2026-08-21 | Keep MCP Chat Fast-only in the first release. | Avoid iterative provider calls through unauthenticated localhost MCP without cancellation/cost controls. | User |
| 2026-08-21 | Use per-generation indexed source revisions plus a canonical/publication journal. | Prevent model upgrades/rebuilds, crashes, and deletions from leaving stale active vectors. | User |
| 2026-08-21 | Durably repair FTS before semantic indexing. | FTS cannot be the availability fallback if best-effort refresh failures remain permanent. | User |
| 2026-08-21 | **Ship Windows x64 only; defer macOS ARM64 and Linux x64.** | This fork's only active workflow is the root `build-windows.yml`; the macOS/Linux workflows are nested under `upstream/` where GitHub Actions never reads them, and no macOS/Linux hardware is recorded. The original all-three-target `MUST` was unsatisfiable and would have blocked Sprint 1 permanently. | User |
| 2026-08-21 | Express the RAM envelope as budget arithmetic and apply it as a model-selection pre-filter. | A 768-dim f32 model is inadmissible at 250k documents before any model loads. Deriving that up front avoids two L-sized benchmark tasks discovering arithmetic empirically. | User |
| 2026-08-21 | Split the vector-backend decision rule by gate; ANN answers latency only. | An HNSW graph adds memory to the vectors it indexes and cannot remedy a RAM miss. The original single "scale gate" rule sent workers down a path that could not succeed. | User |
| 2026-08-21 | Remove the fixed byte-width vector `CHECK`; validate encoding at the repository boundary. | The constraint hardcoded f32 and forbade the quantization escape hatch that the same document offers as the RAM remedy. | User |
| 2026-08-21 | Add a derived-disk envelope with the same target/approval/fail structure as RAM. | Derived chunk text plus vectors plus two retained generations plausibly reach ~2 GiB at the release scale, with no ceiling previously stated anywhere. | User |
| 2026-08-21 | Give cross-encoder reranking a 900 ms p95 sub-budget and pre-approve deterministic adaptive depth. | Reranking 30-50 pairs on CPU plausibly consumes the entire 2 s Fast budget alone; it was the only expensive stage without its own gate. | User |
| 2026-08-21 | Require stage-level progress events during Deep preparation and cut the budget from 45 s to 30 s. | Deep is the default and inserts silence into a streaming UI. Static copy does not prevent a hang from reading as a hang. | User |
| 2026-08-21 | Flag Deep-as-default for user re-examination at Sprint 4 close with measured evidence. | Deep-as-default is a recorded user decision; Fast must independently solve the reference case, which makes Deep a margin rather than the fix. Only the user may change it. | Main agent, **open question for user** |
| 2026-08-21 | Disable the Fast/Deep selector in live-recording scope. | Live retrieval ignores the mode; showing an active control that does nothing misleads the user in the scope most likely to want depth. | User |
| 2026-08-21 | Require a persisted force-lexical-retrieval kill switch. | Index pause/rebuild affect derived state only and cannot return a user to prior retrieval behavior; the sole documented rollback was a code revert. | User |
| 2026-08-21 | Remove `'active'` from the generation state enum. | The singleton pointer is already declared the sole authority; keeping a second representation invites divergence. | User |
| 2026-08-21 | Make `retrieval_documents` a rowid table and add due-work/replay indexes. | Multi-KB rows defeat `WITHOUT ROWID`, and the worker's due-item selection had no supporting index on either state table. | User |
| 2026-08-21 | Require the reference fixture to reproduce the baseline FTS failure, and set corpus size floors. | A synthetic fixture the current retrieval already passes makes the program's headline gate unfalsifiable; percentage gates at small N are noise. | User |
| 2026-08-21 | Register the program in `ROADMAP.md` and gate dispatch on closing Sprint 6.1. | Two live plans with no stated relationship; Sprint 6.1 is still blocked and defines the saved-meeting invariants Sprint 4.3 must preserve. | User |
| 2026-08-22 | Record Sprint 6.1 as closed after all six manual Windows/Tauri checks passed. | Clears the final prerequisite without waiving native scope, provider-disclosure, recording-promotion, or source checks. | User |
| 2026-08-22 | Add "Corpus Solvability" as a normative counterweight to "Baseline Failure Reproduction". | The falsifiability rule constrained the corpus in one direction only. Task `1.2` satisfied it by making the corpus impossible for every retriever — the reference target shares almost no surface with its query while 30 distractors contain the query verbatim — and Task `1.3` consumed an L-sized benchmark before the defect surfaced. | User |
| 2026-08-22 | Implement corpus solvability as an answer-key-free structural check plus a supervised raw-text margin check. | An unsupervised check cannot know which evidence is expected, while an oracle proves nothing. Expected IDs may label the target only; all scores and margins come from text. | User |
| 2026-08-22 | Treat identical aggregate metrics across unrelated model families as a corpus-defect signal, not a model finding. | e5-small (384-d), e5-base (768-d), and paraphrase-MiniLM produced byte-identical corpus metrics in Task `1.3`. Architectures that differ in dimension and training data cannot agree to the digit unless the model is not the deciding variable. | User |
| 2026-08-22 | Void the Task `1.3` fusion, aggregation, and reranker-weight constants; they do not carry into Sprint 2. | The 144-configuration grid ran against an objective whose first two terms were constant across every configuration (exact violations 0 everywhere, semantic misses 30 everywhere), so the search degenerated to its "prefer smaller constants" tie-break. The locked values are search artifacts, not measurements. | User |
| 2026-08-24 | Re-scope the critical forbidden-contamination gate by carrier source state: retrieval stage owns zero contamination from superseded/stale/deleted sources; current-content contradictions move to a defined-but-deferred answer-stage non-assertion gate. | Task `1.3F` proved the flat gate unachievable as staged (four facts carried by current notes inside required meetings; hydrated pools of 6-8 docs below `EVIDENCE_K=10` retain the carrier under every ordering; `0/2160` configurations pass jointly). Hydration includes current notes wholesale by design, so requiring retrieval to erase them asks the wrong layer to censor authoritative content — the recorded production failure was an answer asserting the wrong value, which is answer-stage by nature. The fidelity-fixed harness shows the retrieval-stage half is real and achievable (contamination 30/121 → 15/121 once stale paths closed). | User |
| 2026-08-24 | Split the critical-case gate: hydration-window membership (rank within top 5) is the Sprint 1 model-selection gate; Recall@1 at 100% is retained in full and reassigned to Sprint 3 as a release gate. | The final Task `1.3` run measured the raw bi-encoder ranking the expected meeting **first for four of the five critical cases** — only `pt-ref-chaves-acesso` (rank 4) misses. The demotions are produced by fusion and meeting aggregation, which Sprint 3 Task 3.2 builds and tunes; Sprint 1 selects an embedding/reranker pair and cannot fix a stage that does not exist. All five critical meetings land inside the hydration window (ranks 1,1,2,3,2) with critical required-fact coverage 100% (9/9) and zero retrieval-stage contamination, so the product outcome is correct for all five and the residual failure is ordinal position. The threshold is not lowered; ownership moves to the sprint whose mechanism decides it. | Keep Recall@1 as a Sprint 1 gate and leave model selection blocked indefinitely; grant a dated exception contingent on query expansion, which the evidence shows would address only one of the three misses; lower the threshold below 100%. | User |
| 2026-08-24 | Record that a lexicographic-minimizing tuning objective is stricter than a threshold gate, and require threshold semantics for Sprint 3 fusion tuning. | The final run's feasibility probe showed every gate-passing configuration paying `+2` semantic and `+2` overall Recall@3 misses against the tuned configuration — yet 28/30 semantic still clears its gate overwhelmingly against a 0/30 baseline, and ~133/135 still clears the 95% floor. An objective that minimizes misses lexicographically can therefore never trade two semantic misses for three critical rank-1 hits, even when every gate would accept that trade. Sprint 3 Task 3.2 must gate on thresholds and optimize only within the feasible set, or it inherits the same blind spot on real data. | Leave the objective shape unexamined and let Sprint 3 rediscover it. | User |
| 2026-08-24 | Require a supervised admissibility proof for every zero-tolerance gate, on production-implementable channels only, before any model is benchmarked against it. | The same defect shape consumed two L-sized benchmark tasks in one sprint: `1.2` shipped an unwinnable corpus past a falsifiability-only rule, and the `1.2R` corpus certified three critical cases "solvable" via a hand-authored concept lexicon no production channel implements. An existence proof from fixture text (expected IDs as labels only) makes the trap structurally impossible to re-author. | User |
| 2026-08-24 | Clarify two-snapshot rebuild accounting and approve a 1.30 GiB transient rebuild ceiling for the selected e5-base int8 bundle; select exact vector search and do not evaluate ANN. | A reader's `Arc` to the active snapshot does not allocate a third vector copy, so rebuild accounting is active plus shadow snapshots. The measured two-snapshot peak is 1296.5 MiB, inside the explicit 1.30 GiB transient ceiling; steady state remains 1113.4 MiB inside the existing approved 1-1.25 GiB band. Exact search passes p95 48.2 ms at 250k with recall@150 1.0000, while ANN would add memory and its only trigger (latency miss with passing RAM) did not occur. Any true third allocation or peak above 1.30 GiB remains blocking. | User |
| 2026-08-26 | Preserve a prior bundled embedding model across an upgrading restart by packaging at most one prior bundle in the transition release, retaining only the embedding engine, and making FTS-only the required fallback when it is absent or inadmissible. | A vector generation is queryable only with the model that produced it, and the single packaged bundle is replaced in place on upgrade — so the prior active generation becomes unqueryable at the next restart (`retrieval/index.rs:564`, `:795` set and consume `model_mismatch`), while "Generation Activation" requires it to stay queryable while the shadow builds. Copying the prior bundle to app data would create an unsigned mutable model store outside package authority and outside the pinned provenance chain. The reranker needs no retention: it scores text pairs and is generation-independent, so retention is one embedding session, not two. Version-skipping installs make a lexical fallback unavoidable regardless, so it is specified rather than left implicit. No bundled-model upgrade has shipped yet, so this is forward-looking with no migration burden today. Enabling retention additionally requires a measured active + shadow snapshot plus current + prior embedding session peak, and the identity derivation fix; this decision approves neither by implication. | User |
| 2026-08-26 | Amend the garbage-collection gate with a transitional clause: while no semantic query surface exists, the successful-Fast-hybrid-query condition is satisfied by one clean application restart with the new generation active and publication lag zero. The clause expires at Sprint 3 close. | The Fast hybrid surface is Sprint 3 work, so the condition as written is unsatisfiable in Sprint 2 — `retrieval/index.rs:1630` gates cleanup on a counter that only a Sprint 3 consumer increments. Against the two-generation retention ceiling that makes `retrieval_rebuild_index` succeed exactly once per install and eventually dead-ends the corrupt-active recovery path, which depends on registering a replacement generation. The restart condition is not relaxed and no other cleanup guard changes; only the query condition is substituted, and only while no query surface exists to satisfy it. | User |
| 2026-08-28 | Accept the whole-process activation RAM gate as-is for Sprint 2 with the ceiling unchanged, and move full-application calibration into Sprint 3 as a close obligation. | Both corrections were proven unavailable rather than deferred: `2.R13` showed a retrieval-scoped gate is not implementable against ORT 1.22 as bound, and `2C.3` showed full-application calibration is not producible on the development host (Whisper artifact absent, audio stack requires live WASAPI contention, no WebView residency proof). The mismatch errs fail-safe — activation is withheld and lexical retrieval serves every surface — and semantic retrieval has no query surface in Sprint 2, so the defect is unreachable by users today. Sprint 3 is the first build where a real fully loaded application runs a semantic query, which is exactly the measurement `2C.3` could not manufacture; calibrating there uses better evidence than any synthetic harness could produce, at no cost to a sprint with no deadline. | User |
| 2026-08-28 | Require activation-refusal observability before the release that first exposes semantic retrieval. | The gate's refusal reason is written to `pending_blockers` and read by nothing — not logged, not surfaced. Accepting a knowingly conservative gate without this makes "working" and "never once activated" indistinguishable in the field, which would leave the Sprint 3 calibration obligation unmeasurable. Harmless while no query surface exists; unacceptable from the build that adds one. | User |
| 2026-08-28 | Retain the `retrieval_documents(meeting_id, generation_id)` index despite it serving only a `debug_assertions`-gated query, and revisit at Sprint 3 close. | Measured against SQLite 3.49.1, the release-path deletion decrement is a covering-index search on the pre-existing `retrieval_documents_by_meeting` with or without the new index, so release builds carry it unused. Removing it costs a third forward-only migration in a codebase where two of three migration incidents broke application startup, and restores the write-lock scan `2C.1` was commissioned to remove. Trading a bounded cost (order 10-20 MB and a small write-throughput margin on a 250k-document corpus) for a high-risk operation is a poor exchange; Sprint 3 adds query surfaces that may make the index load-bearing, at which point the question is decidable on evidence. | User |
| 2026-08-29 | Record external prior art from `0x12th/meetily-memory` (an independent, Python-based Meetily indexing tool) as corroborating evidence for two existing design choices; no design change. (1) Its contract explicitly forbids accepting "a generation-local integer" as evidence identity, requiring `source UUID + external ID + fingerprint` instead — independent confirmation that keeping `RetrievedEvidence.evidence_id` (line ~1210) a stable identity separate from `generation_id` was the right call. (2) Its index-rebuild swap (`BEGIN EXCLUSIVE`, WAL checkpoint/truncate, verify no `-wal`/`-shm`/`-journal` sidecars, then swap the file) solves a narrower problem than this program's in-process atomic generation/snapshot switch with hash-verified atomic rename and durable journal replay ("Generation Activation", line ~964) — not a gap, but confirms file-level atomic swap-then-verify is a recognized pattern for the same class of problem. | User |
| 2026-08-30 | Replace historical-artifact release gates with current production-path evidence while preserving every numeric threshold and fail-closed invariant. | The Sprint 1 multi-candidate staging tree contains retired and benchmark-only models that the signed production bundle correctly excludes. Requiring that obsolete layout made current release evidence impossible without weakening package authority. Task correctness now uses the exact production retrieval/ranking path; release reranker p95 uses complete depth-50 calls through the hash-verified production bundle/runtime; the old `solo-pair p95 * 50` result remains dated selection evidence only. | User |
| 2026-08-30 | Make Task 3.6 query expansion conditional on reviewed post-ranking evidence rather than an unconditional Sprint-close dependency. | Task 3.2 must remeasure all five critical cases. If reviewed production ranking reaches 5/5, building expansion only to satisfy an already-closed case is YAGNI and adds privacy/latency/provider risk. If `pt-ref-chaves-acesso` or another terminological-gap case remains the sole attributable miss, Task 3.6 becomes dependency-ready after the user selects an approach. The 5/5 Sprint-close threshold is unchanged. | User |
| 2026-08-30 | Keep `retrieval_documents_by_meeting_lookup` installed without a Sprint 3 removal deadline. | An automatic reconsideration deadline created pressure to delete an applied migration despite no measured regression or replacement query. Removal now requires measured evidence that the index is obsolete, a forward-only migration, and a query-plan regression proving deletion cannot return to a table scan. | User |
| 2026-08-30 | Clarify meeting-aggregation signal requirements as measured behavior rather than mandatory non-zero coefficients. | Requiring a separate non-zero weight for every listed signal invites arbitrary constants when held-out evidence does not support them. Profile participation through fused rank, zero-weight but reported concept coverage, and distinct-region support are valid only when explicit and regression-tested; user-visible quality floors remain the authority. | User |
| 2026-09-02 | Separate Sprint 4 implementation dependency from Sprint 3 release/close gates and record commits `62d7730` and `1047367` as the reviewed implementation baseline. | R40 permits sequenced Sprint 4 implementation after this documentation amendment and current user approval, while Sprint 3 release acceptance remains open and is inherited by later close/release gates. | User-authorized R40 |
| 2026-09-02 | Make Fast/Deep compatibility deterministic and extend one Rust ownership/cancellation publication fence from Sprint 3 to Chat and sidebar. | Explicit interactive modes, omitted-mode Fast compatibility, live Fast/no-Deep behavior, fenced progress, cleanup, and bounded ownership prevent accidental Deep calls, stale publication, and parallel registries. | User-authorized R40 |
| 2026-09-02 | Require bounded Deep generation options in the shared LLM client, a provider capability/fallback matrix, and candidate/evidence authority bounds. | Provider-specific output limits, hard byte/parser caps, deadline cancellation, and per-round offered-ID validation preserve the 512-token/8 KiB/15-second/30-second contract without a second client or scope widening. | User-authorized R40 |
| 2026-09-02 | Carry the single persisted `force_lexical_retrieval` decision through every hybrid round/surface and give MCP timeouts an internal shared-retrieval cancellation token. | The typed `ForcedLexical` fallback remains reversible across requests/restarts, while server timeouts terminate queued/running work without claiming a public MCP cancel API. | User-authorized R40 |
