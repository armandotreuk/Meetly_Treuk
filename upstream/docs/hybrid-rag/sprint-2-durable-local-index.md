# Sprint 2: Durable Local Semantic Index

## Status

Changes requested. Sprint 1 closed on 2026-08-25. The user approved this
Sprint 2 PRD and authorized all remaining Sprint 2 tasks on 2026-08-25.

Revised 2026-08-21 after pre-implementation critique: split into two
independently reviewed halves, and schema semantics corrected.

## Sprint Split

The original single sprint contained three `L` tasks plus a high-risk migration
covering durable triggers, generation lifecycle, staging, a publication journal,
two ONNX model runtimes, deterministic chunking, a scheduler, a background
worker, immutable snapshots, atomic activation, and garbage collection. That is
too much work behind one review gate: a flawed persistence foundation would not
be discovered until after the query index was built on top of it.

| Half | Tasks | Estimate | Gate |
|---|---|---|---|
| **Sprint 2A — Foundation** | 2.1 persistence, 2.2 model runtime, 2.3 chunking | 8-12 working days | Code + architecture review, then user sprint-half close |
| **Sprint 2B — Runtime** | 2.4 index worker, 2.5 query index and activation | 8-12 working days | Code + architecture review, then user sprint close |

Sprint 2B MUST NOT begin until 2A is reviewed, approved, and closed. Estimates
assume one worker at a time with the review overhead this program requires, and
exclude Sprint 1 model-selection time.

For calibration against the rest of the program: Sprint 1 is roughly 6-9 days,
Sprint 3 roughly 8-12, Sprint 4 roughly 6-9, and Sprint 5 roughly 8-12 —
placing the full program near 45-65 working days. These are planning figures,
not commitments, and should be revised once Sprint 1 produces real velocity
data.

## Goal

Create a crash-safe, resumable, local semantic indexing subsystem that turns
authoritative meeting content into versioned semantic documents without
blocking startup or primary data writes. At sprint close, a complete bundled
model generation can be built, atomically activated, searched locally, updated
after every relevant mutation, and discarded/rebuilt without primary data
loss.

## Architecture Authority

All work follows [`architecture.md`](architecture.md) plus the model, chunking,
backend, limit, and toolchain addenda approved at Sprint 1 close.

## Scope

### In Scope

- Additive semantic schema and durable source-revision triggers.
- Bundled model manifest/resource loading.
- Exact tokenizer, embedding ONNX engine, and reranker ONNX engine.
- Deterministic meeting-profile, transcript, summary, and notes chunking.
- Resumable single-owner background backfill and incremental reindexing.
- Model-generation build/activation and lexical-only behavior until complete.
- Immutable vector snapshot and the exact/ANN backend selected in Sprint 1.
- Backend status/progress contract needed by later UI.
- Startup, fresh-database, import, and shutdown lifecycle integration.
- Durable repair of stale/partial FTS before semantic work, so lexical fallback
  is actually recoverable after a crash or best-effort refresh failure.

### Out Of Scope

- Hybrid rank fusion or Chat integration.
- Sidebar/MCP semantic behavior.
- Fast/Deep UI.
- Runtime model download/delete.
- Remote embeddings.
- GPU ONNX providers.
- Settings status UI, which is delivered in Sprint 5.

## Current State And Evidence

- `frontend/src-tauri/src/database/manager.rs:17-50` opens SQLite and runs
  migrations.
- `frontend/src-tauri/src/database/setup.rs:9-35` has separate existing and
  first-launch database state paths.
- `frontend/src-tauri/src/database/commands.rs:142-235` installs state after
  legacy import or fresh creation.
- `frontend/src-tauri/src/database/repositories/transcript.rs:19-162` commits
  transcript data before notes import and best-effort FTS refresh.
- `frontend/src-tauri/src/audio/retranscription.rs:480-568` replaces transcript
  rows transactionally, then refreshes FTS best-effort.
- `frontend/src-tauri/src/database/repositories/summary.rs:452-507` generation-
  fences summary completion, then refreshes FTS.
- `frontend/src-tauri/src/database/commands.rs:306-389` saves/deletes notes and
  refreshes FTS best-effort.
- `frontend/src-tauri/src/database/repositories/meeting.rs:240-292` deletes a
  meeting and dependent content transactionally.
- `frontend/src-tauri/src/parakeet_engine/model.rs:89-231` demonstrates current
  CPU ORT session/tensor patterns.
- `frontend/src-tauri/src/lib.rs:574-582` initializes the database before MCP.

## Sprint Requirements

- Semantic state is disposable derived data.
- Migrations enqueue work but perform no tokenization or inference.
- Triggers capture all authoritative content mutations even when a future
  caller forgets an explicit wake hook.
- The worker does model work outside database transactions.
- Per-meeting replacement is atomic and generation-fenced.
- Initial partial coverage is not query-active.
- Model upgrade keeps the prior active generation until complete activation.
- Startup and primary content mutations succeed when model/index loading fails.
- Search snapshots use immutable reader state.
- Every lifecycle is cancellable and bounded.
- Active and shadow models track source revisions independently.
- SQLite commits and deletion tombstones are replayable into memory/sidecars
  after a crash.
- One poison meeting/generation item cannot starve the queue.
- Raw content never appears in logs.

## Task List

### Sprint 2A — Foundation

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 2.1 | Persistence | Add source revisions, per-generation state, semantic/staging documents, active-generation pointer, publication journal/tombstones, retry state, due-work indexes, triggers, and repository APIs. | M, high risk | Complete `worker-l` (`ses_fc46e8e0bffeV1FhRi0c0tWH28`) | Sprint 1 | Migration/repository tests prove coalescing, generation independence, staged deletion, deletion journal, retry state, revision fencing, encoding-aware vector validation, and no inference in migration. | Prior runtime ignores unused feature paths, but old-binary rollback requires the verified pre-upgrade DB backup. |
| 2.2 | Local models | Load and validate the bundled tokenizer, embedding model, and reranker model through bounded CPU ONNX sessions. | L | Complete `worker-l` (`ses_fc4213e51ffeKmJB9RJX5dWQc8`) | 1.3, 1.5 | Production engine matches reference outputs locally and preserves the Sprint 1 Windows x64 reference gate. | Disable/remove retrieval state registration; FTS unaffected. |
| 2.3 | Semantic documents | Implement authoritative source extraction and deterministic model-token chunking. | M | Complete `worker-l` (`ses_fc3d96e3fffecBHjWlJROBrSCQ`) | 2.1, 2.2 | Golden tests prove stable IDs, transcript ranges, Markdown sections, limits, and Unicode behavior. | Derived chunking module can be removed; no primary data change. |

### Sprint 2B — Runtime

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 2.4 | Index worker | Implement shared lifecycle/scheduler, model-independent FTS repair, and resumable per-generation indexing into canonical SQLite/publication journal. | L | Complete `worker-l` (`ses_fc3aba9f6ffesV2ctyKzhoi86W`) | 2A close | Crash/change/retry/poison/scheduler tests prove stale vectors cannot commit, FTS heals, and work is not lost/starved. | Stop worker and leave derived rows inactive; revision state remains recoverable. |
| 2.5 | Query index and activation | Implement exact base+delta/tombstone snapshots, journal replay, atomic swaps, complete model activation, disk-envelope reporting, and status API. | L | Complete `worker-l` (`ses_fc32adc3dffe1ghSodutD4W1UO`) | 1.4, 2.4 | Nearest-neighbor, scope, journal crash, deletion, activation, lifecycle, corruption, and cancellation tests pass. | Disable query index and use durably repaired FTS; SQLite vectors remain rebuildable. |

## Dependency Order

Within 2A: `2.1 + 2.2 -> 2.3`

Gate: **2A close approval**

Within 2B: `2.4 -> 2.5`

Task `2.1` is a migration and runs alone. Tasks `2.2`, `2.4`, and `2.5` are L
and run alone. Do not parallelize `2.1` and `2.2` without explicit approval if
both need shared state/module registration files.

## Task Specifications

### 2.1 - Source revisions, semantic schema, and repository [M, high risk]

**Outcome:** Every relevant authoritative mutation durably marks its meeting
for semantic reindexing, and derived documents can be replaced atomically.

**Likely touchpoints:**

- New migration under `frontend/src-tauri/migrations/`
- `frontend/src-tauri/src/database/repositories/mod.rs`
- New `frontend/src-tauri/src/database/repositories/retrieval.rs`
- Migration/repository test helpers

**Required implementation:**

- Implement the architecture schema semantics using repository naming
  conventions.
- Add triggers for transcript insert/update/delete, summary insert/result
  update/delete, meeting-notes insert/update/delete, meeting insert, and meeting
  title update.
- Advance a separate FTS projection revision, without semantic source revision,
  for meeting `folder_id` changes and meetings affected by folder rename,
  delete, or detach. Folder parent moves need no repair when immediate ID/name
  are unchanged.
- Coalesce repeated changes by monotonically advancing one source revision per
  meeting.
- Track FTS revision and per-`(generation_id, meeting_id)` indexed source revision
  independently.
- Add persisted attempt count, next-attempt time, and safe error state for FTS
  and each generation/meeting item.
- Seed source state per existing meeting; create per-generation work when a
  generation is registered.
- Use foreign-key cascade for semantic/staging rows/source/generation state on
  meeting delete while preserving non-FK publication tombstones until
  acknowledged.
- Before meeting deletion cascades, append a non-FK deletion journal entry for
  every built generation so an existing snapshot can remove vectors after commit or
  restart.
- Add a singleton active-generation pointer; do not infer active state from
  multiple generation rows. **`'active'` MUST NOT appear in the generation
  state enum** — the permitted values are `building`, `ready`, `failed`, and
  `retired`. Two representations of one fact cannot be kept consistent.
- **Store `vector_encoding` on each document row and validate vectors
  encoding-aware at the repository boundary. Do NOT add a fixed byte-width
  `CHECK` such as `length(vector) = dimensions * 4`** — it forbids the
  quantized encodings that `architecture.md` "Resource Budget Arithmetic"
  depends on. Validate encoding, dimension, byte length, finiteness, and norm
  in `repositories/retrieval.rs`.
- **Make `retrieval_documents` and `retrieval_document_staging` rowid tables**
  with `UNIQUE (generation_id, document_id)` and `UNIQUE (job_id, document_id)`
  respectively. They carry multi-KB vector BLOBs plus chunk text, far above the
  small-row profile `WITHOUT ROWID` is designed for; large rows there spill to
  overflow chains and degrade the full-table scan snapshot loading depends on.
  `retrieval_meeting_state` stays `WITHOUT ROWID` — its rows are small.
- **Add the due-work and replay indexes** from `architecture.md`:
  `search_source_state_fts_due`, `retrieval_meeting_state_due`,
  `retrieval_document_staging_by_generation`, and
  `retrieval_index_changes_replay`. Step 1 of the worker algorithm selects one
  due item per poll and would otherwise full-scan every meeting row.
- When a quantized encoding is selected, persist its dequantization parameters
  in `retrieval_models` so a vector can never be interpreted under the wrong
  scale.
- Separate immutable model identity from rebuildable generation identity and
  add revision/job-bound staging storage so one large meeting can be embedded
  in bounded batches before atomic publication.
- Add repository methods to list/claim work without destructive dequeue,
  extract authoritative source rows, replace one meeting/generation atomically,
  compare source revision, append/read/ack publication changes, report
  per-generation coverage/errors, update FTS revision, atomically switch active
  generation, and delete/rebuild derived generations.
- Validate vector encoding, dimensions, byte length, finiteness, and norm at
  the repository/runtime boundary.
- Do not re-embed on folder metadata changes unless the approved chunk contract
  embeds folder text; FTS metadata repair remains mandatory.
- Do not alter current FTS tables or refresh behavior.

**Acceptance criteria:**

- Fresh migration creates all semantic tables/triggers and seeds existing
  meetings without inference.
- Multiple transcript inserts in one save coalesce to one meeting with an
  advanced source revision.
- A newly inserted title-only meeting is queued for its meeting profile.
- Summary result, notes, transcript, and title changes each enqueue the meeting.
- Folder parent move does not advance semantic or FTS projection revisions when
  immediate folder IDs/names are unchanged.
- Meeting folder assignment and folder rename/delete advance FTS projection
  revision and are eventually repaired while semantic models are unavailable.
- Meeting deletion removes semantic/staging/source/generation state
  transactionally and preserves publication tombstones.
- A partially staged job is removed on meeting deletion and cannot resume after
  restart; generation cleanup also cascades its staging rows.
- Meeting deletion leaves durable generation tombstones until each active snapshot
  publisher acknowledges them.
- The deletion transaction advances each affected generation's canonical change
  ID to its tombstone. Immediately after commit canonical is greater than
  published and semantic queries remain disabled until acknowledgment.
- Active and shadow generation state for the same immutable model and meeting advances,
  fails, retries, and cancels independently.
- Replacement fails/retries when source revision changes between extraction
  and commit.
- Failed replacement leaves the prior complete meeting documents intact.
- Retry scheduling can skip a poison item and process another due meeting.
- Exactly one active generation is representable, and `'active'` is not a
  valid generation state value.
- Malformed vectors are rejected without panicking startup.
- A vector in a non-f32 approved encoding round-trips correctly, and a vector
  whose byte length disagrees with its declared encoding and dimension is
  rejected at the repository boundary.
- Due-work selection uses an index rather than a full table scan; prove it with
  `EXPLAIN QUERY PLAN` in a test for both the FTS repair and semantic paths.
- Primary tables and existing migrations remain backward compatible.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::migration_tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

Use the actual migration test target if named differently. Record it.

**Worker report additions:** Provide the trigger matrix, transaction boundaries,
and post-release rollback implications.

### 2.2 - Bundled tokenizer and ONNX engines [L]

**Outcome:** Rust can load the approved signed-resource model bundle and produce
reference-correct embeddings and reranker scores without blocking async
workers.

**Likely touchpoints:**

- New `frontend/src-tauri/src/retrieval/mod.rs`
- New `frontend/src-tauri/src/retrieval/model.rs`
- `frontend/src-tauri/src/lib.rs` module/state registration as needed
- `frontend/src-tauri/Cargo.toml`
- `frontend/src-tauri/tauri.conf.json` resource entries
- Approved model manifest/resource verifier from Sprint 1

**Required implementation:**

- Parse and validate the approved manifest version.
- Lazily recheck artifact byte length/SHA-256 before first process load.
- Resolve retrieval resources from Tauri's resource directory.
- Initialize the exact tokenizer/preprocessing contract selected in Sprint 1.
- Create separate embedding and reranker ORT sessions.
- Apply exact query/document prefixes, pooling, normalization, and dimensions.
- Apply each model's independent tokenizer/truncation/pair formatting contract
  and validate input/output names, dtypes, shapes, label index, and score
  transform at load.
- Use CPU execution with explicit bounded thread settings.
- Run inference through blocking/bounded execution, never while holding an
  async lock.
- Cache sessions by exact bundle identity.
- Provide cancellation between document/reranker batches.
- Return typed availability/failure status suitable for lexical fallback.
- Avoid copying patterns from existing model downloads; no runtime download is
  needed.

**Acceptance criteria:**

- Reference input token IDs match Sprint 1 reference values.
- Embedding dimension, finite values, and unit norm match the manifest.
- Embedding similarity/reference tolerance matches Sprint 1.
- Reranker reference score/order matches Sprint 1 tolerance.
- Missing, corrupt, unknown-version, wrong-shape, and wrong-dimension resources
  fail cleanly without preventing database/app initialization.
- Concurrent requests do not initialize duplicate incompatible sessions.
- Inference does not run on Tokio worker threads.
- Logs reveal no input text or token sequence.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::model::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Record model paths, thread settings, session
lifecycle, cancellation boundaries, memory measurements, and target-specific
limitations.

### 2.3 - Deterministic semantic document chunking [M]

**Outcome:** Authoritative meeting content produces stable, evidence-preserving
documents under the approved tokenizer/chunk policy.

**Likely touchpoints:**

- New `frontend/src-tauri/src/retrieval/chunking.rs`
- Retrieval repository source row types
- Focused fixtures/tests

**Required implementation:**

- Create meeting-profile, transcript-window, summary-section, and notes-section
  documents exactly as selected in Sprint 1.
- Order transcripts by non-null `audio_start_time`, then timestamp and stable
  ID, matching saved-meeting chronology.
- Align windows to segment boundaries and split an oversized segment only when
  required.
- Preserve first/last transcript IDs, speaker/timestamp bounds, summary
  template ID, heading, ordinal, and source kind.
- Normalize text only according to the approved embedding contract.
- Compute deterministic content hashes and document IDs.
- Handle empty content, malformed summary JSON, Unicode, very long utterances,
  repeated headings, and missing timestamps.
- Keep folder metadata outside embedded content unless Sprint 1 explicitly
  approved it.

**Acceptance criteria:**

- Identical source/model/chunker input produces byte-identical IDs and text.
- One source edit changes affected content hashes and does not randomly reorder
  unaffected documents.
- Transcript windows respect token limits and approved overlap.
- No transcript segment disappears between adjacent windows unless explicitly
  excluded as empty.
- Summary/notes headings and template IDs remain recoverable.
- Meeting-profile content follows the approved latest-summary policy.
- Unicode tests include Portuguese accents and multibyte boundaries.
- Chunked evidence can rehydrate to authoritative source ranges.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::chunking::tests
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Document normalization, ID formula, token-window
rules, and intentional ceilings.

### 2.4 - Durable FTS/semantic index worker [L]

**Outcome:** Lexical repair and per-generation semantic indexing are resumable,
crash-safe, non-blocking, independently revisioned, and durably journaled for
later query-index publication.

**Likely touchpoints:**

- `frontend/src-tauri/src/retrieval/mod.rs`
- New worker/lifecycle code under `frontend/src-tauri/src/retrieval/`
- `frontend/src-tauri/src/state.rs`
- `frontend/src-tauri/src/database/setup.rs`
- `frontend/src-tauri/src/database/commands.rs`
- `frontend/src-tauri/src/lib.rs`
- Repository APIs from Task 2.1

**Required implementation:**

- Start one worker after database state exists for normal startup, fresh
  database creation, and legacy import.
- Wake from durable source revisions; explicit post-commit hooks MAY only optimize
  wake latency and are not correctness requirements.
- Repair FTS first whenever indexed projection revision trails FTS projection
  revision, even if retrieval models fail to load. Mark indexed revision only
  after a complete refresh.
- Process one bounded due item at a time under the approved scheduler policy.
- Stage at most 256 documents or 64 MiB working memory per batch unless Sprint
  1 approves lower values. Resume valid source-revision-bound staging after a
  crash and discard stale/cancelled staging without replacing active documents.
- Implement one shared scheduler with interactive priority, one ONNX inference
  permit, no more than two vector-scan permits, at most eight queued interactive
  requests, cancellation while queued, and Sprint 1 approved ORT thread cap.
- Pause index work within 250 ms of active recording, import, or
  retranscription signals and measure resumed behavior.
- Follow the source-revision compare-and-swap algorithm in `architecture.md`.
- Append publication changes in the same transaction as semantic replacement.
- Retain outdated state after failures and retry with persisted exponential or
  approved bounded backoff.
- Skip not-yet-due/poison items so other meetings continue.
- Pause/throttle during recording/transcription resource pressure without
  losing work.
- Record safe generation/meeting failures and expose enough repository state for the
  later status API.
- Do not activate semantic generations or publish memory snapshots in this
  task; Task 2.5 owns validated publication/activation.
- Use one detached retrieval lifecycle object created during Tauri setup and
  idempotently attach/start it after all three database installation paths.
- Give MCP a clone of the same lifecycle/service; reject duplicate worker
  starts.
- Shutdown cancels and joins worker/model work before DB pool closure.
- Primary meeting saves/deletes do not fail because model/index work fails.

**Acceptance criteria:**

- Startup returns before initial backfill completes.
- Crash/restart resumes revision work without duplicate current documents.
- Source edit during inference prevents stale publication.
- Failed embedding leaves prior active documents and outdated revision work
  intact.
- Failed post-commit FTS refresh is eventually repaired while semantic model
  loading is unavailable.
- Failed folder assignment/rename/delete FTS metadata synchronization is
  eventually repaired without unnecessary re-embedding.
- Active and shadow generation work for the same model both reach the latest
  source revision independently.
- One permanent/poison item does not spin or block other due work and is visible
  as a model activation blocker.
- A synthetic oversized meeting stays within the batch memory ceiling; crash
  and cancellation leave prior documents active and staging resumable/cleanable.
- Fresh/import/existing database paths each start exactly one worker.
- MCP and Tauri share the same runtime rather than duplicate sessions/workers.
- Shutdown cannot publish after database teardown.
- Pausing and resuming preserves progress and correctness.
- Interactive work preempts queued index inference; queue limits and queued
  cancellation are deterministic.
- Recording/import/retranscription signals pause indexing within the approved
  bound without losing source revisions.
- No raw source text appears in worker logs/errors.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Provide lifecycle diagrams for each DB setup path,
source/FTS/model revision transitions, publication transaction, retry/backoff/
poison rules, pause signals, and crash scenarios tested.

### 2.5 - Immutable query index, publication, and activation [L]

**Outcome:** Readers can search a complete active vector generation safely
while journaled updates/rebuilds proceed, and an active/shadow model switches
only after complete revision and snapshot validation.

**Likely touchpoints:**

- New `frontend/src-tauri/src/retrieval/index.rs`
- `frontend/src-tauri/src/retrieval/mod.rs`
- Retrieval repository
- Additive Tauri status/rebuild/pause commands and command registration
- Selected pure-Rust ANN dependency only when approved by Sprint 1

**Required implementation:**

- Load validated vectors/metadata for the active generation into an immutable
  base snapshot plus exact delta/tombstones.
- Implement normalized query search through the approved exact or exact+ANN
  backend.
- Clone/release reader state before scanning; do not hold locks across search.
- Support authoritative allow-list filtering by meeting ID.
- Apply the approved base/delta/tombstone strategy if ANN is selected.
- Apply a base/delta/tombstone compaction strategy for exact search as well;
  single-meeting updates cannot copy the complete 250k base.
- Replay canonical journal changes from published change ID after normal
  commits and process restart; advance published ID only after snapshot/delta
  publication succeeds.
- Disable semantic queries or complete bounded catch-up whenever published
  state is behind canonical state.
- Build sidecars to temporary files and atomically publish validated versions.
- Treat SQLite vectors as canonical and sidecars/caches as rebuildable.
- Swap snapshots atomically after complete validation.
- Activate a shadow model through the singleton pointer only when every current
  meeting equals current source revision, no permanent blocker remains, its
  snapshot validates, and canonical/published IDs match.
- Keep prior active generation queryable until that transaction succeeds.
- Implement manual rebuild as a new shadow generation for the same model ID.
  Healthy active retrieval continues; known-corrupt active retrieval is
  deactivated to FTS-only. Cancel removes only shadow staging/derived rows.
- Resume a crashed rebuild from valid staging/revisions. Retain at most two
  complete generations and garbage-collect the previous one only after the new
  active generation survives one clean restart plus one successful Fast query.
- Expose status, pause/resume, and rebuild backend commands with no raw data.
- Accept cancellation and bounded query limits.

**Acceptance criteria:**

- Known nearest-neighbor fixtures return expected order/tolerance.
- Narrow allow-list search never emits another meeting.
- Snapshot readers observe either old or new complete state, never partial
  mixtures.
- Crash after SQLite replacement but before memory publication replays the
  missing change on restart.
- Meeting deletion journal removes vectors from base/delta before semantic
  queries resume and cannot leak through scope/source publication.
- Active model pointer never references two or an incomplete generation.
- Manual rebuild, cancel, crash/restart, concurrent query, corrupt-active
  deactivation, and previous-generation cleanup state transitions are tested.
- Cleanup never deletes active state or a generation with unacknowledged
  journal changes.
- Exact single-meeting update uses delta and does not rebuild/copy the full base
  synchronously.
- Corrupt/missing cache or sidecar falls back/rebuilds without startup failure.
- Rebuild deletes only derived state.
- Query cancellation returns without replacing newer Chat ownership.
- Selected 250k benchmark gates remain satisfied in production code.
- Status reports active generation, backend, document/meeting coverage, safe
  failure reason, canonical/published IDs, activation blockers, and **measured
  derived disk usage against the approved envelope**.
- Derived disk usage crossing the approved envelope is reported and blocks
  generation activation; it never triggers automatic deletion of primary data.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

Run and record the production-backend 250k benchmark from Sprint 1.

**Worker report additions:** Describe snapshot ownership, scope filter path,
cache/sidecar format, corruption behavior, and memory/latency results.

## Sprint Acceptance Criteria

### Sprint 2A close

- Every relevant authoritative mutation durably dirties its meeting.
- The schema matches the corrected `architecture.md` semantics: no `'active'`
  generation state, no fixed byte-width vector `CHECK`, rowid document tables,
  and the due-work/replay indexes present and proven used.
- The bundled model pair produces reference-correct local outputs on Windows
  x64.
- Semantic documents are deterministic and source-rehydratable.
- Code and architecture reviews approve the migration, model runtime, and
  chunking boundaries.

### Sprint 2B close

- Initial and upgraded semantic generations activate only when complete.
- Worker failures cannot fail primary writes or app startup.
- The selected vector backend meets the 250k RAM, latency, and derived-disk
  release gates.
- Semantic index status/rebuild/pause backend contracts exist, including disk
  reporting.
- FTS behavior remains available and unchanged.
- Full Rust tests, typecheck, Vitest, Cargo check, rustfmt, and diff checks pass.
- Code and architecture reviews approve the worker and query-index boundaries.

## Risks And Mitigations

- **Migration startup failure:** keep SQL additive/simple and cover migrated
  legacy data.
- **Lost updates:** durable trigger generation plus compare-and-swap publish.
- **Long DB locks:** all model work occurs before a short replacement
  transaction.
- **CPU contention:** one bounded worker, blocking pool, pause/throttle signals.
- **Partial index bias:** FTS-only until complete initial activation.
- **Model upgrade mismatch:** model/chunker identity versions every document.
- **Memory spikes:** immutable snapshot build/activation must be measured with
  old/new overlap.
- **ANN corruption:** SQLite vectors are canonical; sidecar is disposable.
- **Deletion leakage:** FK cascade plus current-meeting filtering in readers.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-21 | Use durable SQL source-revision triggers rather than another best-effort hook chain. | Future write paths, model upgrades, and crashes must not permanently stale FTS or semantic state. | Add embedding calls beside every FTS refresh. | Main agent, pending sprint approval |
| 2026-08-21 | Keep semantic documents/vectors in SQLite and memory/ANN state disposable. | Preserve import/deletion integrity and one authoritative derived store. | Sidecar-only vectors or vector service. | Main agent, pending sprint approval |
| 2026-08-21 | Do not activate partial initial backfill. | Prevent rankings from favoring meetings indexed first. | Query partial coverage with a warning. | Main agent, pending sprint approval |
| 2026-08-21 | Split the sprint into 2A foundation and 2B runtime with separate review gates. | Three L tasks plus a high-risk migration behind one gate would surface a flawed persistence foundation only after the query index was built on it. | Keep one sprint; add a mid-sprint checkpoint. | Main agent, pending sprint approval |
| 2026-08-21 | Remove `'active'` from the generation state enum. | The singleton pointer is already the declared authority; a second representation invites divergence with nothing enforcing consistency. | Add a trigger keeping the two in sync. | Main agent, pending sprint approval |
| 2026-08-21 | No fixed byte-width `CHECK` on the vector column; validate encoding-aware at the repository. | `length(vector) = dimensions * 4` hardcodes f32 and forbids the quantization path the RAM envelope depends on. | Keep the CHECK and add a second column for quantized vectors. | Main agent, pending sprint approval |
| 2026-08-21 | Make the document and staging tables rowid tables. | Multi-KB rows defeat `WITHOUT ROWID`, which targets small rows; overflow chains degrade the snapshot-load scan. | Keep `WITHOUT ROWID` and raise SQLite page size. | Main agent, pending sprint approval |
| 2026-08-21 | Add due-work and journal-replay indexes in the initial migration. | The worker selects one due item per poll; without an index every poll scans all meetings. Adding them later costs another migration. | Add indexes reactively if profiling shows a problem. | Main agent, pending sprint approval |
| 2026-08-21 | Report derived disk usage and block activation when the envelope is exceeded. | Derived text plus vectors plus two retained generations plausibly reach ~2 GiB with no prior ceiling. | Track disk as a metric only. | Main agent, pending sprint approval |
| 2026-08-25 | Start Sprint 2A with Task 2.1 as the isolated first batch. | Sprint 1 is closed; the user approved the Sprint 2 PRD and the dependency-ready persistence batch. | Start model runtime first; parallelize persistence and model runtime. | User |
| 2026-08-25 | Dispatch Task 2.2 as Sprint 2A Batch 2. | Task 2.1 is verified; the user approved the dependency-ready bundled-model-runtime batch. | Defer Task 2.2 until Task 2.3; combine model runtime and chunking in one task. | User |
| 2026-08-26 | Authorize sequential implementation of all remaining Sprint 2 tasks without separate batch approval, retaining the required sprint-end code and architecture reviews. | Dependencies still govern dispatch order; removing approval pauses avoids unnecessary idle time. | Continue requesting each batch separately. | User |
| 2026-08-26 | Continue through Sprint 2B before an intermediate 2A close review. | The user requested all remaining implementations continue and requested both reviews at sprint end, superseding the interim close gate for this sprint. | Stop at 2A for the originally planned half-sprint reviews. | User |
| 2026-08-26 | Amend the garbage-collection gate with a transitional clause expiring at Sprint 3 close, unblocking Task `2.R1`. | The successful-Fast-hybrid-query condition cannot be met before Sprint 3 ships that surface, which made manual rebuild single-use and eventually dead-ended corrupt-active recovery. Substituting one clean restart with the new generation active and publication lag zero restores both, without relaxing the restart requirement or any other cleanup guard. | Leave the gate as written and accept single-use rebuild until Sprint 3. | User |
| 2026-08-26 | Record prior-embedding-model retention across an upgrading restart as an architecture amendment; implementation deferred to the sprint that ships a bundled-model upgrade. | Sprint 2B built the activation path this constrains, and the post-remediation reviews found the prior active generation is unqueryable after an upgrading restart. Sprint 2 ships one bundle and never upgrades one, so the defect is latent here and the fix belongs where the upgrade ships. See `architecture.md` "Prior-Model Retention Across Upgrade". | Copy the prior bundle into app data on upgrade; accept FTS-only for the entire rebuild window as the contract; implement retention inside Sprint 2. | User |

## Task Execution Log

<!-- Append one immutable entry per completed, blocked, or cancelled task. -->

### 2.1 - Source revisions, semantic schema, and repository

**Status:** Complete
**Owner:** `worker-l` (`ses_fc46e8e0bffeV1FhRi0c0tWH28`)
**Completed:** 2026-08-25
**Implemented:**
- Added additive semantic schema, source/FTS revision triggers, per-generation work state, staging/documents, durable publication journal, tombstones, retry state, and the required due-work/replay indexes.
- Added encoding-aware vector validation, authoritative-source reads, revision-fenced replacement, journal/status APIs, generation registration, activation pointer, and cleanup foundations.
- Added focused migration/repository coverage, including future-meeting seeding for active/shadow generations and retired-generation tombstone replay.
**Implementation:**
- Files: `frontend/src-tauri/migrations/20260825000000_add_semantic_retrieval.sql`, `frontend/src-tauri/src/database/repositories/retrieval.rs`, `frontend/src-tauri/src/database/migration_tests.rs`, `frontend/src-tauri/src/database/mod.rs`, `frontend/src-tauri/src/database/repositories/mod.rs`.
- Approach: SQLite triggers are the durable mutation boundary, including delete tombstones. Generation registration atomically creates exact-backend publication state and seeds current meetings; the meeting-insert trigger seeds later meetings for each live generation.
**Not implemented:**
- Bundled ONNX runtime, chunking, lifecycle worker, snapshot search/activation gates, and UI or Chat integration.
**Why not implemented:**
- Owned by Tasks 2.2 through 2.5.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests` - pass, 16 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::migration_tests` - pass, 2 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories` - pass, 66 tests.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass.
**Rollback:**
- Semantic state is additive and derived; retrieval remains unregistered and FTS behavior is unchanged. A binary rollback across the migration still requires the verified pre-upgrade database backup.
**Decisions and follow-ups:**
- Generation states remain `building`, `ready`, `failed`, and `retired`; the singleton pointer is the only active-generation authority.
- The repository validates vector encoding, dimensions, bytes, finiteness, and norm without a fixed f32-width SQL check. The JSON staging-payload ceiling is documented with a `ponytail:` upgrade path.
- Task 2.4 must recheck due-work query plans against realistic database statistics; Task 2.5 owns acknowledged-journal pruning beyond generation cleanup.

### 2.2 - Bundled tokenizer and ONNX engines

**Status:** Complete
**Owner:** `worker-l` (`ses_fc4213e51ffeKmJB9RJX5dWQc8`)
**Completed:** 2026-08-26
**Implemented:**
- Added a lazy bundled-model runtime that parses the Task 1.5 manifest, re-verifies every managed artifact before its first process load, and builds separate CPU embedding and reranker ONNX sessions.
- Added manifest-derived tokenizer configuration, input/output contract validation, mean pooling plus strict L2 normalization, query/document prefixes, raw-logit reranking, cancellation between bounded batches, and typed privacy-safe failures.
- Added exact `(bundle_id, canonical root)` session caching for the active and shadow generations. A third identity is refused typed rather than evicting either resident generation.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/mod.rs`, `frontend/src-tauri/src/retrieval/model.rs`, `frontend/src-tauri/src/lib.rs`, `frontend/src-tauri/Cargo.toml`.
- Approach: the resource resolver maps Tauri's resource directory to `resources/retrieval/bundle`; model work uses the CPU provider with intra-op `min(4, max(1, logical cores / 2))` and exactly one inter-op thread. Async APIs delegate inference to `spawn_blocking`; document embeddings check cancellation every 16 items and reranking checks every pair.
**Not implemented:**
- Startup loading, retrieval lifecycle/worker registration, document chunking, index publication/activation, or a frontend status surface.
**Why not implemented:**
- Tasks 2.3 through 2.5 own those integration and runtime lifecycle concerns. The module is registered only; it does not load models during application or database initialization.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::model::tests` - pass, 14 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib model_bundle` - pass, 21 tests.
- `MEETLY_RAG_BUNDLE_DIR="C:\\Users\\arman\\OneDrive\\Repositório Projetos\\Personal Meetly\\upstream\\frontend\\src-tauri\\resources\\retrieval\\bundle" cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark reference_inference_is_stable_finite_and_dimensional` - pass.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the only notice is the pre-existing CRLF warning for `.github/workflows/build-windows.yml`.
**Rollback:**
- Remove the additive `retrieval` module registration or leave it unused; no model is loaded at startup and FTS behavior is unaffected.
**Decisions and follow-ups:**
- The tokenizer explicitly fixes right-side `LongestFirst` truncation, zero stride, and the manifest-pinned 512-token maximum rather than relying on dependency defaults.
- Zero or non-finite embedding norms fail typed rather than returning a finite but non-unit vector.
- Windows x64 debug reference loading measured a 933.5 MiB RSS increase for one session set. Task 2.5 must measure active-plus-shadow production overlap and enforce the architecture disk/RAM activation envelope before retaining two loaded generations.

### 2.3 - Deterministic semantic document chunking

**Status:** Complete
**Owner:** `worker-l` (`ses_fc3d96e3fffecBHjWlJROBrSCQ`)
**Completed:** 2026-08-26
**Implemented:**
- Added deterministic meeting-profile, transcript-window, summary-section, and notes-section construction directly from Task 2.1's authoritative `MeetingSource` and `SourceTranscript` types.
- Added package-tokenizer-backed token accounting, 384-token/64-token-overlap windows, stable transcript chronology, whitespace-boundary oversized-segment splitting, ATX heading sections, evidence provenance, SHA-256 content hashes, and deterministic document IDs.
- Added focused coverage for stable/golden identity, affected-only edits, limits/overlap, source-range rehydration, headings/template IDs, latest-summary profiles, long/oversized content, Unicode, degenerate source data, and the packaged tokenizer.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/chunking.rs`, `frontend/src-tauri/src/retrieval/mod.rs`.
- Approach: content is preserved without casing, Unicode, or whitespace normalization; its hash covers the exact `passage: `-prefixed string passed to the embedding runtime. Transcript ordering is non-null `audio_start_time`, timestamp, then stable ID; an oversized segment splits only between whitespace-delimited words and fragments tile the original text byte-for-byte. Summary/note documents retain heading/template metadata and transcript windows retain segment/speaker/timestamp bounds for authoritative rehydration.
**Not implemented:**
- Database writes, tokenization/inference execution, worker lifecycle, index publication, activation, and frontend status.
**Why not implemented:**
- Task 2.4 maps these pure derived documents into durable staged documents and owns the worker. Tasks 2.2 and 2.5 own model execution and index publication respectively.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::chunking::tests` - pass, 13 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests` - pass, 16 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::model::tests` - pass, 14 tests.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the only notice is the pre-existing CRLF warning for `.github/workflows/build-windows.yml`.
**Rollback:**
- Remove the additive `chunking` module and its re-export. It has no primary-data writes, startup registration, or FTS behavior.
**Decisions and follow-ups:**
- The profile intentionally retains only its first token window (`ponytail:` selection-aid ceiling); add bounded profile windows only if recall evidence requires them.
- Oversized-fragment candidate recounting is intentionally quadratic (`ponytail:` sentence-scale ceiling); upgrade to tokenizer offsets if profiling identifies it as material.
- Task 2.4 must use the same packaged tokenizer artifact when it creates `PackagedTokenizer` and map transcript range IDs to `StagedDocument.source_start_id`/`source_end_id`.

### 2.4 - Durable FTS/semantic index worker

**Status:** Complete
**Owner:** `worker-l` (`ses_fc3aba9f6ffesV2ctyKzhoi86W`)
**Completed:** 2026-08-26
**Implemented:**
- Added one detached, shared, cancellable retrieval lifecycle with a bounded scheduler, model-independent FTS repair, per-generation due-work processing, revision-bound staging/resume, source-revision-fenced replacement, durable backoff/poison state, and recording/import/retranscription pause boundaries.
- Attached the existing lifecycle after normal, fresh, and legacy-import database installation; MCP receives a clone of the same lifecycle, and Tauri exit joins it before database cleanup.
- Persisted the approved symmetric int8 vector storage contract (`scale = 1/127`, `zero_point = 0`) and rejects non-finite, short, or wrongly dimensioned embedding responses before staging.
- Fixed `FolderRepository::delete_with_cascade` to release its transaction connection before best-effort FTS work, removing one-connection-pool starvation.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/worker.rs`, `frontend/src-tauri/src/retrieval/mod.rs`, `frontend/src-tauri/src/retrieval/model.rs`, `frontend/src-tauri/src/database/repositories/retrieval.rs`, `frontend/src-tauri/src/database/repositories/folder.rs`, `frontend/src-tauri/src/audio/recording_commands.rs`, `frontend/src-tauri/src/database/setup.rs`, `frontend/src-tauri/src/database/commands.rs`, `frontend/src-tauri/src/mcp/server.rs`, `frontend/src-tauri/src/lib.rs`.
- Approach: FTS repair is selected before model work and marks its revision only after `refresh_meeting` completes. Semantic work extracts authoritative source at revision N, chunks/tokenizes/embeds outside a transaction, stages at most 256 documents or 64 MiB per batch, then uses Task 2.1's atomic revision-fenced replacement/journal transaction. Valid staged work survives cancellation/crash; stale jobs are pruned at startup and before each due item.
**Not implemented:**
- Query snapshots, journal acknowledgement/replay into memory, semantic query APIs, activation/rebuild commands, model-generation UI/status, and atomic active-generation swaps.
**Why not implemented:**
- Task 2.5 owns query-index construction, publication replay, model generation activation, disk/RAM envelope enforcement, and status exposure.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::worker::tests` - pass, 23 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests` - pass, 16 tests in 0.39 s.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::folder` - pass, 5 tests, including a max-1-pool regression guard.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::model::tests` - pass, 14 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::migration_tests` - pass, 2 tests.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the only notice is the pre-existing CRLF warning for `.github/workflows/build-windows.yml`.
**Rollback:**
- Leave the additive retrieval worker unattached or remove its module/lifecycle wiring; semantic rows remain derived and FTS remains available. The folder transaction scoping change is independently safe and preserves best-effort FTS behavior.
**Decisions and follow-ups:**
- The worker never activates generations, publishes memory snapshots, or acknowledges the journal. Canonical/published lag deliberately remains until Task 2.5's validated publisher owns it.
- Initial bundled-model generation IDs are deterministic from model identity, so restarts resume the same generation. Task 2.5 must provide distinct shadow-generation identity and correct bundle/model selection for model upgrades while preserving active/shadow independence.
- Scheduler policy is one ONNX permit, two vector-scan permits, and at most eight FIFO interactive tickets with deterministic queued cancellation. Its public scheduler is the only Task 2.5 query concurrency boundary.

### 2.5 - Immutable query index, publication, and activation

**Status:** Complete
**Owner:** `worker-l` (`ses_fc32adc3dffe1ghSodutD4W1UO`)
**Completed:** 2026-08-26
**Implemented:**
- Added one process-wide exact query-index service (`QueryIndexService`) serving immutable `IndexSnapshot`s: a contiguous validated int8 base plus a bounded per-meeting overlay (upsert replacements and deletion tombstones). Readers clone the snapshot `Arc` and release every lock before scanning; publication swaps finished snapshots atomically, so readers observe only old-complete or new-complete state.
- Added journal publication inside the shared Task 2.4 worker loop: startup/attach performs a full canonical load, acknowledges the loaded bound first (so pre-bound changes are never re-applied onto a complete load), then replays only changes beyond it; steady-state replay folds last-writer-wins per meeting through sparse IDs and upsert/delete ordering without copying the base, acknowledges durably per batch, and compacts at the approved 2% delta threshold on a blocking thread. Deferred batches (quarantined delta reloads) keep publication lag visible and pause semantic queries typed until healed.
- Added deterministic normalized-query exact search over base+delta with post-search authoritative scope re-filtering, symmetric int8 cosine scoring under the approved 1/127 contract, two-permit vector-scan scheduler integration with queued cancellation, typed availability outcomes (`NoActiveGeneration`, `CatchUpPending`, `InvalidQuery`, `Cancelled`), and hydration-ready metadata on every hit.
- Added generation activation: completed shadow generations load+validate, catch up their own journals, pass coverage/permanent-failure/publication/disk gates, mark `ready`, switch the singleton pointer in one transaction, and swap memory atomically while the previous active generation stays resident and queryable; initial partial coverage never activates; known-corrupt active generations are deactivated to FTS-only with quarantined meetings requeued; at most two generations are retained and a third rebuild is refused typed.
- Added measured status data (backend, semantic state, active model/generation, coverage counts, canonical/published IDs, activation blockers, resident index bytes, derived disk bytes against the approved 2 GiB target / 3 GiB activation limit) and additive Tauri commands `retrieval_index_status`, `retrieval_rebuild_index`, and `retrieval_set_index_paused` (manual pause stops lexical repair and indexing at item boundaries without disabling queries or publication).
- Added retired-generation GC: retired journals drain to canonical (a retired generation is never served again), and deletion waits for one clean restart plus one successful query; cleanup deletes only derived rows via the repository guards.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/index.rs`, `frontend/src-tauri/src/retrieval/commands.rs`, `frontend/src-tauri/src/retrieval/mod.rs`, `frontend/src-tauri/src/retrieval/worker.rs`, `frontend/src-tauri/src/database/repositories/retrieval.rs`, `frontend/src-tauri/src/lib.rs`, `frontend/src-tauri/Cargo.toml`.
- Approach: SQLite vectors stay canonical and sidecars are not used (exact search needs none); snapshot admission is encoding-aware at the repository boundary with a memory-lean int8 norm check so a 250k load never materializes float rows; the publisher runs as one `publish_tick` step each worker tick independent of model availability, and shutdown joins it before database teardown through the existing lifecycle exit hook that MCP shares by clone.
**Not implemented:**
- Chat/Sidebar/MCP semantic search wiring, hybrid fusion/reranking consumers, Settings UI, model-upgrade trigger UI, automatic recovery of a corrupt-deactivated active generation (user-initiated rebuild is the path), and fp16/f32 base encodings behind the int8 contract.
**Why not implemented:**
- Consumers belong to Sprints 3-5; the architecture requires user approval for any new model or encoding; auto-recovery of corrupt active state would mask a data-integrity signal.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests` - pass, 23 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests` - pass, 20 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::` - pass, 89 tests (index + worker + model + chunking + repository).
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib` - pass, 511 tests, 0 failed, 2 ignored (independently re-run after final corrections).
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib model_bundle` - pass, 21 tests; `--lib database::migration_tests` - pass, 2 tests.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass (pre-existing CRLF notice on `.github/workflows/build-windows.yml`).
- Frontend regression safety: `npx tsc --noEmit` pass; `npx vitest run` 95/95 pass (no frontend changes).
**Rollback:**
- Leave `publish_tick` unhooked or remove the `retrieval::index` module: SQLite vectors, journal, and FTS behavior remain unchanged and lexical fallback covers every failure path. The three commands are additive and unregisterable independently.
**Decisions and follow-ups:**
- The publisher acknowledges the full-load bound BEFORE replaying so a complete canonical load is never double-counted into the overlay; deferred (quarantined) delta batches return unapplied and keep `published < canonical`, which disables semantic queries typed until the worker heals the meeting.
- Retired generations' journals are drained to canonical before GC because nothing serves them anymore; deletion additionally requires one clean restart plus one successful query, matching "Retain at most two complete generations".
- Manual pause does not stop publication/catch-up: pausing indexing must not freeze deletion tombstones out of the query path.
- Disk-envelope accounting approximates page overhead from row counts (`ponytail:` marker); the activation gate blocks above 3 GiB and never deletes primary data.
- The publisher samples actual process RSS through `memory-stats` before activation and blocks when unavailable or at/above the approved 1.30 GiB ceiling; re-measure production RAM/disk during a real 250k-scale shadow activation because the governing transient margin from Sprint 1 was ~0.9%.
- Follow-ups for Sprint 3: consume `SearchFailure` as the typed lexical-fallback trigger and `VectorHit.score` as an internal diagnostic only.

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

**Required because:** Additive migration, persistent derived data, SQL triggers,
background concurrency, model runtime, memory snapshots, and optional ANN
sidecar lifecycle.

**Reviewer:** Pending
**Verdict:** Pending
**Findings:** Pending

## Approval Gates

- Sprint 1 close and exact model, vector encoding, and backend decisions must be
  approved first.
- User approval of this PRD is required before Sprint 2 TODO creation.
- Task 2.1 migration and every L task require separate dependency-ready batch
  approval and normally run alone.
- **Sprint 2A close approval is required before any 2B task is dispatched.**
- Any new model, ANN dependency, schema semantic change, or runtime download
  requires scope-change approval.
- Sprint-close approval is required before Sprint 3 begins.

### Sprint 2 Architecture Review

**Reviewer:** `gpt-5.6-sol` (final architecture review, 2026-08-26)  
**Verdict:** `changes-requested`

**Findings (severity order):**
1. **Critical — publication can durably acknowledge state that no reader snapshot contains, and full loads are not point-in-time snapshots.** Canonical rows are paged through separate reads (`frontend/src-tauri/src/database/repositories/retrieval.rs:1081-1118`), after which the current canonical bound is acknowledged before install (`frontend/src-tauri/src/retrieval/index.rs:687-706`, `1073-1115`). A concurrent meeting deletion after its rows were read can therefore be included in memory while its tombstone is acknowledged, permanently leaking deleted vectors until another rebuild. Steady replay likewise acknowledges before returning/swapping the new snapshot (`frontend/src-tauri/src/retrieval/index.rs:711-721`, `814-818`), contrary to the required acknowledge-after-publication ordering. Load rows plus a journal bound from one SQLite read transaction, build the immutable state, gate queries, swap it, then acknowledge only the installed bound (or persist a recoverable snapshot identity).
2. **High — active/shadow generations do not own matching model runtimes.** The lifecycle caches one embedder (`frontend/src-tauri/src/retrieval/worker.rs:530-534`) and applies it to due work selected from every live generation (`frontend/src-tauri/src/retrieval/worker.rs:602-622`, `690-705`) without checking that generation's `model_id`. After a bundled-model upgrade, edits to an old active generation can be embedded by the new model and admitted when dimensions/encoding happen to match, corrupting that generation and constraining Sprint 3 hybrid ranking and Sprint 5 upgrade/rebuild recovery. Select/cache the runtime by each generation's immutable model identity, or stop updating the old generation and make that degraded behavior explicit and FTS-only for changed meetings.
3. **High — durable FTS repair has a lost-update window.** Refresh runs and then marks whatever projection revision is current (`frontend/src-tauri/src/retrieval/worker.rs:710-725`; `frontend/src-tauri/src/database/repositories/retrieval.rs:459-470`). A source/folder mutation between those operations advances the projection, but `mark_fts_indexed` then copies that newer revision although the refresh did not include it, defeating the promised FTS-only fallback. Fence the mark with the selected `fts_projection_revision` and retry on mismatch.
4. **High — shutdown cancellation does not cover publisher/activation work.** The worker invokes `publish_tick` without a cancellation token (`frontend/src-tauri/src/retrieval/worker.rs:537-562`), while full snapshot paging and compaction have no cancellation boundary (`frontend/src-tauri/src/database/repositories/retrieval.rs:1081-1118`; `frontend/src-tauri/src/retrieval/index.rs:825-841`). `shutdown` must await that work (`frontend/src-tauri/src/retrieval/worker.rs:461-472`), so a 250k load can make exit unbounded. Thread lifecycle cancellation through page loads, replay, activation, and compaction and leave durable bounds unchanged when cancelled.
5. **Medium — section-heading provenance is dropped at persistence.** Chunking creates per-document heading metadata (`frontend/src-tauri/src/retrieval/chunking.rs:110-126`, `394-410`), but staging omits it (`frontend/src-tauri/src/retrieval/worker.rs:1034-1064`) and neither canonical schema nor snapshot metadata has a heading field (`frontend/src-tauri/migrations/20260825000000_add_semantic_retrieval.sql:52-70`; `frontend/src-tauri/src/retrieval/index.rs:88-99`). This breaks the normative recoverability contract and constrains Sprint 3 reranking/hydration. Persist the heading through the existing derived-document row rather than reconstructing it heuristically.
6. **Medium — reported “measured derived disk” is an estimate that omits material SQLite storage.** It sums selected payload lengths plus a fixed row allowance (`frontend/src-tauri/src/database/repositories/retrieval.rs:1204-1227`) but excludes indexes, journal/state rows, page fragmentation/free pages, and WAL, while status presents it as the activation metric (`frontend/src-tauri/src/retrieval/index.rs:1270-1272`, `1333-1337`). Use SQLite page/file measurements partitioned enough to avoid counting unrelated primary data, or label this estimate and retain a conservative measured gate before activation.

**Residual risks:** the status contract does not retain a typed model-load failure (only logs it at `frontend/src-tauri/src/retrieval/worker.rs:583-599`), pause is process-local rather than the architecture's separate persisted lexical-only rollback, and MCP has the shared lifecycle but no semantic tool parity yet (`frontend/src-tauri/src/mcp/server.rs:365-391`); these must be closed in the owning later sprints without duplicating retrieval logic.

### Sprint 2 Code Review

**Reviewer:** `gpt-5.6-sol` (final code review, 2026-08-26)  
**Verdict:** `changes-requested`

**Findings (severity order):**
1. **Critical — FTS repair can mark a concurrently changed projection current.** `frontend/src-tauri/src/retrieval/worker.rs:710-717` refreshes a meeting, then `frontend/src-tauri/src/database/repositories/retrieval.rs:459-470` copies the *current* projection revision into `fts_indexed_revision`. A source or folder update between those steps can make stale FTS appear current. Fence the mark against `FtsDueItem.fts_projection_revision` and leave mismatches due for repair.
2. **Critical — journal acknowledgement precedes snapshot installation, and full loads lack a captured bound.** `frontend/src-tauri/src/retrieval/index.rs:668-818` acknowledges full-load and replay bounds before swapping the reader snapshot; `frontend/src-tauri/src/database/repositories/retrieval.rs:1045-1119` reads full loads without one consistent snapshot/bound. A deletion can be acknowledged while its old vector remains queryable. Load rows and bound consistently, publish first, then acknowledge; preserve correct replay if acknowledgement fails.
3. **Medium — deletion-only overlays never compact.** `frontend/src-tauri/src/retrieval/index.rs:143-157,825-842` counts only overlay upserts, so tombstone-only churn can indefinitely retain deleted base vectors in memory. Include tombstones in the 2% compaction threshold.
4. **Medium — malformed summary JSON can terminally fail semantic indexing.** `frontend/src-tauri/src/database/repositories/retrieval.rs:625-700` and `frontend/src-tauri/src/retrieval/worker.rs:762-775` should retain usable title/transcript/notes extraction when a summary is unreadable.

**Required regression coverage:** deterministic FTS revision-CAS, bounded/full-load replay consistency, publish-before-ack and acknowledgement-retry, deletion-only compaction, model-identity isolation, publisher cancellation, heading persistence, and disk-gate labeling/measurement.

### Resume Handoff

Remediation has not started. The architecture review above additionally requires model-identity isolation across live generations, cancellation through publisher/activation/compaction, persisted heading provenance via a forward-only migration, and conservative disk-gate reporting. Resume by implementing those shared-path fixes first, then run focused tests plus `cargo test --lib`, `cargo check`, `cargo fmt --check`, and `git diff --check`. Re-run fresh code and architecture reviews only after all findings are resolved. The main agent independently ran `cargo test --lib` after Task 2.5 corrections: 511 passed, 0 failed, 2 ignored.

> **Superseded 2026-08-26.** This handoff is stale. Remediation of every
> `gpt-5.6-sol` finding did land (see the post-remediation reviews below);
> the handoff text was never updated.

### Post-Remediation Code Review

**Reviewer:** `claude-sonnet-5` (session `e762b0ec-7ac3-454d-a05c-49ad443b817d`), 2026-08-26
**Verdict:** Changes requested

**Independently verified:** `cargo check --manifest-path "frontend/src-tauri/Cargo.toml" --lib` - pass, exit 0, no warnings from the crate. The full test suite was NOT re-run in this review; the Task 2.5 log's 511-test result stands as its own evidence.

**Prior findings confirmed resolved.** All four `gpt-5.6-sol` code-review findings and all six architecture findings are closed in the working tree: FTS marking is now revision-fenced (`repositories/retrieval.rs:558-577`, `retrieval/worker.rs:749-776`); full loads take rows, bound, and model identity from one SQLite read transaction and publish before acknowledging (`repositories/retrieval.rs:1269-1353`, `retrieval/index.rs:961-1066`); tombstones count toward the 2% compaction threshold (`retrieval/index.rs:1194-1201`); malformed summary JSON no longer erases extraction or the lexical projection (`repositories/retrieval.rs:764-798`, `repositories/fts.rs:363-431`); heading provenance is persisted through a forward-only migration (`migrations/20260826000000_add_semantic_document_heading.sql`); cancellation is threaded through page loads, replay, activation, and compaction; model identity gates due-work selection (`retrieval/worker.rs:716-739`) and activation (`retrieval/index.rs:1403-1408`); the disk figure is labeled an estimate and the gate consumes a separate bound.

**Findings (severity order):**

1. **High - retired-generation GC is unreachable in shipped code, so manual rebuild permanently dead-ends after one use.** `retrieval/index.rs:1629-1631` gates deletion on `acknowledged_fast_hybrid_queries() > 0`; the only incrementer, `acknowledge_fast_hybrid_query` (`retrieval/index.rs:737-741`), is `#[allow(dead_code)]` and called from tests only - it is a Sprint 3 consumer hook. Meanwhile `register_generation` counts `building|ready|failed|retired` against a hard ceiling of two (`repositories/retrieval.rs:253-264`). Failure scenario: `gen-<hash>` builds and activates (1 retained); the user invokes `retrieval_rebuild_index`, the shadow activates and the first generation retires (2 retained); GC never fires, so every subsequent rebuild returns `RetentionLimit` forever, and the corrupt-active recovery path (`deactivate_active_generation` to `failed`, then rebuild) is blocked with it. The Sprint 2B criterion "previous-generation cleanup state transitions are tested" is satisfiable only in tests. Fix: gate GC on a signal that exists in Sprint 2 (a successful `QueryIndexService::search`, or a clean publish tick on the new active generation after restart), or make `request_rebuild` reclaim an eligible retired generation before refusing.

2. **High - the activation disk gate measures the entire database plus a RAM figure, and double-counts shadows.** `envelope_usage_bytes` (`retrieval/index.rs:1361-1372`) sums `derived_backing_store_upper_bound_bytes` (whole-file `page_count * page_size` plus WAL, i.e. every primary meeting/transcript/summary page), `estimated_shadow_snapshot_bytes` (rows already inside that page count), and `resident_vector_bytes()` (process RAM, not disk) - then compares the total to `DERIVED_DISK_ACTIVATION_LIMIT_BYTES`. Failure scenario: a user whose primary SQLite file alone reaches 3 GiB can never activate semantic retrieval; the reported blocker names derived disk, and no derived-data cleanup can ever clear it, because the gate is not a function of derived data. Fix: measure the derived tables and their indexes specifically (`dbstat`, or the payload estimate plus a measured index/page allowance), leave RAM to `ram_gate_blocker`, and drop the shadow term already counted in the page total.

3. **Medium - `PRAGMA wal_checkpoint(PASSIVE)` is used as a measurement and runs on every worker tick and every status poll.** `repositories/retrieval.rs:1519` performs a write-side checkpoint attempt to read a frame count. `envelope_usage_bytes` calls it for every non-active live generation inside `try_activate_shadow_generation`, which runs from `publish_tick_inner` on every worker iteration (`retrieval/worker.rs:562`) - and the loop `continue`s without sleeping while any item is due. During initial backfill that is one checkpoint attempt per indexed meeting; `retrieval_index_status` adds one per UI poll. The measurement is also taken *before* the cheap coverage gate (`retrieval/index.rs:1420-1428`), contradicting the "cheap gates first" comment: it runs at 0% coverage too. Fix: evaluate `coverage_blockers` first, and read WAL size without mutating the database.

4. **Medium - the bounded-batch memory contract covers embedding only, not resume or publication.** `list_staged_documents` deserializes and validates every staged payload for a job into one `Vec` (`repositories/retrieval.rs:897-931`), and `process_semantic_item` then clones every vector into `reusable` although only the key set is used (`retrieval/worker.rs:855-875`). `replace_meeting_documents` likewise materializes and validates all staged documents *inside* the `BEGIN IMMEDIATE` write transaction before inserting them one by one (`repositories/retrieval.rs:1025-1077`). Failure scenario: the synthetic oversized meeting the sprint requires stays under 64 MiB while embedding, then on resume/publish holds the whole document set resident and keeps the write lock across N JSON parses, N norm checks, and N inserts, contending with primary meeting writes. Fix: select only `document_id` for the reuse check, and stream staged rows during replacement.

5. **Medium - FTS repair can spin at full CPU with no backoff.** `run_worker` repairs a due FTS item and `continue`s without a sleep quantum (`retrieval/worker.rs:577-586`). `repair_fts_item` records backoff only when `refresh_meeting` itself fails (`retrieval/worker.rs:778-795`); the `Ok(false)` superseded branch and the `Err` branch of `mark_fts_indexed` (`retrieval/worker.rs:763-776`) write no attempt count and no `fts_next_attempt_at`. Failure scenario: a persistent write error against `search_source_state` leaves the meeting due forever, and the worker re-runs a full `refresh_meeting` for it in a tight loop. Fix: record bounded backoff on both non-advancing branches, or fall through to the sleep quantum after a repair that did not advance the indexed revision.

6. **Medium - acknowledged journal rows are never pruned for a live generation.** `retrieval_index_changes` rows are deleted only in `delete_generation` (`repositories/retrieval.rs:499`) and `cancel_building_generation` (`repositories/retrieval.rs:1449`). Every meeting publication appends one row per live generation, so the table and its `retrieval_index_changes_replay` index grow monotonically for the whole life of the active generation. Task 2.1's own follow-up assigned this pruning to Task 2.5, which did not implement it. Fix: prune rows at or below the minimum `published_change_id` across generations holding index state.

7. **Low - `retrieval_cancel_rebuild` ships but is undocumented.** The command is implemented (`retrieval/commands.rs:48-57`) and registered (`lib.rs`), but the Task 2.5 execution-log entry lists only `retrieval_index_status`, `retrieval_rebuild_index`, and `retrieval_set_index_paused`. Documentation drift on a user-reachable command surface.

8. **Low - timestamp format inconsistency in the heading migration.** `migrations/20260826000000_add_semantic_document_heading.sql:12` writes `CURRENT_TIMESTAMP` into `retrieval_meeting_state.updated_at`, which every Rust writer populates as RFC 3339. Nothing parses that column today, so this is a consistency defect rather than a live bug - but `retired_at`, which *is* parsed for GC eligibility, sits one table away.

**Required follow-ups:** resolve findings 1-6 before sprint close; 7-8 may ride along. Re-run `cargo test --lib`, `cargo check`, `cargo fmt --check`, `git diff --check`, and the production-backend 250k benchmark afterwards. Do not alter the approved model, chunk, encoding, or backend contracts while fixing these.

### Post-Remediation Architecture Review

**Required because:** additive migration, persistent derived data, SQL triggers, background concurrency, model runtime, immutable memory snapshots, generation activation, and disk/RAM envelope enforcement.

**Reviewer:** `claude-sonnet-5` (session `e762b0ec-7ac3-454d-a05c-49ad443b817d`), 2026-08-26
**Verdict:** Changes requested

**Findings (severity order):**

1. **High - immutable model identity is bundle-id-derived, so a model swap that keeps `bundleId` aliases onto the previous generation.** `RetrievalModels::model_id()` returns `identity().bundle_id` (`retrieval/worker.rs:105-107`); `register_semantic_identity` persists that string as `retrieval_models.model_id` (`retrieval/worker.rs:700-709`); the deterministic generation id is `sha256(model_id)[..8]` (`retrieval/worker.rs:674-681`). `APPROVED_BUNDLE_ID` (`model_bundle.rs:30`) is an independent constant from `APPROVED_EMBEDDING_MODEL_ID` and `APPROVED_EMBEDDING_REVISION` (`model_bundle.rs:32-33`), and nothing binds one to the other. Failure scenario: a future bundle swaps the embedding model or revision but keeps `meetily-retrieval-bundle-1`; `ensure_model` short-circuits, the same `gen-<hash>` is reused, and the new model's vectors are written into the old generation beside the old model's - exactly the corruption the `next_due_item` identity check exists to prevent, and undetectable because dimensions and encoding match. Fix: derive the persisted model identity from the full approved contract (bundle id, embedding model id, revision, quantization, dimensions, chunker version), not from `bundleId` alone.

2. **High - the derived-disk envelope decision is implemented as a whole-database gate.** The Decisions log (2026-08-21) approves "Report derived disk usage and block activation when the envelope is exceeded"; the implementation blocks on total SQLite backing store plus a RAM term (`retrieval/index.rs:1361-1372`, `repositories/retrieval.rs:1512-1527`). The normative property - derived state has a ceiling and primary data is never deleted to satisfy it - is inverted: primary data growth alone can permanently disable semantic retrieval, and the blocker text misattributes it to derived data. Same defect as code-review finding 2; recorded here because the decision it implements is architectural.

3. **Medium - a chunker-contract bump does not version a generation.** `document_id` includes `config.chunker_version` (`retrieval/chunking.rs:549-561`), but the generation id does not, and `ensure_model` is a no-op once `model_id` exists (`repositories/retrieval.rs:688-699`). Failure scenario: `APPROVED_CHUNKER_VERSION` moves to 2; only meetings that happen to be re-indexed produce v2 documents, so the active generation permanently mixes v1 and v2 chunk geometry while `retrieval_models.chunker_version` still reports 1. The sprint risk register's mitigation ("model/chunker identity versions every document") holds for documents but not for the generation that serves them. Fix: include `chunker_version` in the deterministic generation id, and make `ensure_model` refuse or migrate a stored chunker version that differs from the compiled constant.

4. **Medium - terminal per-meeting failure silently narrows the served index, outside the stated snapshot contract.** Every canonical loader excludes documents whose `retrieval_meeting_state.state = 'failed'` through a `NOT EXISTS` clause (`repositories/retrieval.rs:1227`, `:1242`, `:1333`), and `suppress_terminal_failure` tombstones the meeting in memory (`retrieval/index.rs:680-693`). Canonical rows survive but stop being served; nothing is journaled and `document_count` is unaffected. The reader contract as documented is "either the old complete state or the new complete state"; in practice there is a third, complete-minus-quarantined state. Coverage status does surface `failed_meetings`, so the user-visible signal exists - but this behavior should be a named state in `architecture.md`, not an emergent property of a subquery.

5. **Medium - manual pause is process-local and also stops lexical healing.** `set_index_paused` is an `AtomicBool` on the lifecycle (`retrieval/worker.rs:508-514`) with no durable representation, so it does not survive restart and is not the architecture's persisted lexical-only rollback. It additionally short-circuits before FTS repair (`retrieval/worker.rs:569`), so pausing semantic indexing also pauses the lexical fallback the pause is supposed to preserve. Carried forward from the prior review's residual risks and still open.

6. **Low - MCP holds the shared lifecycle but exposes no semantic surface.** `McpState.retrieval` (`mcp/server.rs:22`) is currently write-only. Correct for Sprint 2 scope; Sprints 3-5 must consume it rather than construct a second runtime.

**Assumptions and risks:**
- The Task 2.5 log's 511-test result and the production-backend 250k benchmark were taken as recorded; this review re-verified only `cargo check --lib`.
- The RAM activation gate samples whole-process RSS immediately before the swap (`retrieval/index.rs:1483`), which is the right measurement point, but the Sprint 1 transient margin was ~0.9%; a real 250k-scale shadow activation must still be measured.
- Exactly one bundle root exists in production, so the two-slot session cache and the active-plus-shadow runtime story are untested against a genuine second model.
- `read_canonical_snapshot` relies on WAL-mode read-snapshot semantics from one pooled connection; a non-WAL deployment would weaken the consistency guarantee that closes the prior critical finding.
- Sprint 3 must consume `SearchFailure` as the typed lexical-fallback trigger and treat `VectorHit.score` as an internal diagnostic only.

**Required follow-ups:** resolve findings 1-5 before sprint close; record the quarantine state (finding 4) in `architecture.md` as a scope-recorded decision rather than code-only behavior.

## Sprint 2 Remediation

Drafted 2026-08-26 from the two post-remediation reviews above. Task IDs
follow the Sprint 1 remediation convention. Nothing here is dispatched; task
`2.R1`'s garbage-collection decision was settled on 2026-08-26 (option A, see
below), so all four tasks are ready to dispatch.

### Resolved decision for 2.R1: the garbage-collection gate

**Resolved 2026-08-26: option A.** The user approved the transitional clause,
now recorded in `architecture.md` "Generation Activation" and its Decision
Log. `2.R1` is unblocked and implements the option A branch below; the option
B column is retained only as the rejected alternative.

`architecture.md` "Generation Activation" makes a retired generation eligible
for cleanup "only after the new active generation survives one clean
application restart **and one successful Fast hybrid query**". The Fast hybrid
query surface does not exist until Sprint 3, so the second condition is
unsatisfiable in shipped Sprint 2 code. Combined with the two-generation
retention ceiling, that makes manual rebuild a single-use operation and
eventually blocks the corrupt-active recovery path as well.

| Option | Effect |
|---|---|
| **A (recommended)** — amend the gate with a dated transitional clause: "one successful semantic query, or, while no semantic query surface exists, one clean restart with the new generation active and publication lag zero". The clause expires when Sprint 3 lands the Fast hybrid path. | Rebuild and corrupt-active recovery work in Sprint 2. Costs one architecture amendment. |
| **B** — leave the gate as written; accept that rebuild works once per install until Sprint 3. | No amendment. Ships a user-reachable rebuild control that silently stops working, and a recovery path that dead-ends after its first use. |

Option A was chosen. `2.R1`'s acceptance criteria below are the option A
branch; no further approval is outstanding for this task.

### Remediation Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 2.R1 | Generation identity and lifecycle | Derive model identity from the full approved contract, make generation resumption a lookup, implement the approved transitional GC condition, and prune acknowledged journal rows. | M, high risk | Unassigned `worker-l` | 2.5 | Existing installs keep their generation without re-indexing; a chunker-version bump mints a new generation; rebuild succeeds twice in a row; journal rows below the published bound are reclaimed. | Revert the identity derivation and its migration; derived state remains rebuildable and FTS is unaffected. |
| 2.R2 | Envelope gates | Measure derived disk as derived data, stop mutating the database to read it, and order the activation gates cheapest-first. | M | Unassigned `worker-l` | 2.R1 | The disk figure excludes primary storage and process RAM; no `wal_checkpoint` runs from a measurement path; coverage blocks before any size probe. | Revert to the prior gate; activation remains blocked-only and never deletes data. |
| 2.R3 | Worker memory and repair backoff | Bound resume and publication memory to the approved batch ceiling, shorten the replacement write lock, and stop the FTS repair spin. | M | Unassigned `worker-l` | 2.R2 | A synthetic oversized meeting stays within the ceiling on resume and publish; a persistently failing FTS mark does not spin. | Revert the worker/repository changes; durable state and staging semantics are unchanged. |
| 2.R4 | Contract records | Name the quarantine state, stop pause from freezing lexical repair, and clear the two documentation/format defects. | S | Unassigned `worker-l` | 2.R3 | `architecture.md` names the quarantined-coverage state; pause leaves FTS repair running; the command surface and timestamp format are consistent. | Documentation-only apart from the pause branch, which reverts independently. |

### Dependency Order

`2.R1 -> 2.R2 -> 2.R3 -> 2.R4`

All four touch `database/repositories/retrieval.rs`; they run alone and in
order. `2.R1` carries a migration and runs alone regardless.

### 2.R1 - Generation identity, retention, and journal reclamation [M, high risk]

**Outcome:** A generation's identity reflects the contract that produced its
vectors, an upgrade or chunker bump mints a distinct generation, rebuild is
repeatable, and acknowledged journal rows do not accumulate forever.

Closes code finding 1 and 6, architecture findings 1 and 3, and satisfies the
identity precondition in `architecture.md` "Prior-Model Retention Across
Upgrade".

**Likely touchpoints:**

- `frontend/src-tauri/src/retrieval/worker.rs`
- `frontend/src-tauri/src/retrieval/index.rs`
- `frontend/src-tauri/src/database/repositories/retrieval.rs`
- New forward-only migration under `frontend/src-tauri/migrations/`

**Required implementation:**

- Derive the persisted model identity from the full approved contract:
  bundle id, embedding model id and revision, ONNX export revision,
  quantization, dimensions, vector encoding, and chunker version. Keep a
  human-readable prefix so logs and status stay diagnosable; the digest
  carries the discrimination.
- Replace `generation_id_for(model_id)` hashing as the resumption mechanism.
  Resume by **looking up** an existing live generation for the derived model
  identity, and mint a new generation id only when none exists. Generation
  ids stay opaque; nothing may depend on them equalling a hash of anything.
- Add a forward-only migration that rewrites the legacy `bundleId`-derived
  model identity to the newly derived one for the single bundle that has
  shipped: insert the new `retrieval_models` row, repoint
  `retrieval_generations.model_id` at it, then delete the legacy row.
  Existing generations keep their ids and their documents. **Existing installs
  MUST NOT re-index as a result of this task.**
- Implement the approved transitional GC condition (option A, already recorded
  in `architecture.md` "Generation Activation"): while no semantic query
  surface exists, treat the successful-query requirement as satisfied by one
  clean application restart with the new generation active and its publication
  lag zero. Keep the restart requirement and every other cleanup guard intact.
  Mark the transitional branch in code so Sprint 3 can find and remove it, and
  do not delete the original condition — it applies again once the Fast hybrid
  path ships.
- Prune `retrieval_index_changes` rows at or below the minimum
  `published_change_id` across every generation still holding index state.
  Pruning MUST NOT run inside the replacement transaction and MUST NOT
  advance any bound.
- Do not change the approved model, chunk, encoding, or backend contracts.

**Acceptance criteria:**

- An existing database carrying the legacy identity migrates in place, keeps
  its active generation, and indexes zero meetings as a result.
- Two consecutive `retrieval_rebuild_index` calls both succeed, with the
  intervening retired generation reclaimed under the transitional condition:
  one clean restart with the new generation active and publication lag zero.
- A retired generation is NOT reclaimed while publication lag is non-zero, nor
  within the process that retired it, nor while any other cleanup guard would
  refuse it. The transitional clause substitutes one condition; it does not
  weaken the rest.
- A corrupt-active deactivation followed by a rebuild succeeds twice.
- Bumping `APPROVED_CHUNKER_VERSION` mints a distinct model identity and a
  distinct generation; the prior generation keeps serving until the new one
  activates.
- A bundle whose embedding model id or revision changes while `bundleId` is
  unchanged produces a distinct model identity, and its work is never
  dispatched to the prior generation.
- Journal rows below the minimum published bound are reclaimed; rows at or
  above it, and every tombstone not yet acknowledged by some publisher,
  survive.
- Generation ids are never recomputed from a hash at runtime.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::migration_tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** the derived identity formula, the legacy
migration's row-level steps, the resolved GC gate, and the journal-pruning
bound with its safety argument.

### 2.R2 - Envelope measurement and activation gate ordering [M]

**Outcome:** The derived-disk gate is a function of derived data, reading it
does not write to the database, and expensive probes run only for a candidate
that already looks activatable.

Closes code findings 2 and 3, and architecture finding 2.

**Likely touchpoints:**

- `frontend/src-tauri/src/database/repositories/retrieval.rs`
- `frontend/src-tauri/src/retrieval/index.rs`

**Required implementation:**

- Measure derived storage over the derived tables and their indexes only
  (`retrieval_documents`, `retrieval_document_staging`,
  `retrieval_meeting_state`, `retrieval_index_state`,
  `retrieval_index_changes`, `retrieval_generations`, `retrieval_models`).
  Prefer `dbstat` where the linked SQLite exposes it; otherwise use the
  payload sum plus a measured, documented overhead factor. The figure MUST
  remain conservative for a block-only gate while excluding primary storage.
- Remove the process-RAM term from the disk figure. RAM stays entirely in
  `ram_gate_blocker`, which already measures it at the correct moment.
- Remove the building-shadow term, whose rows are already counted by any
  derived-table measurement.
- Read WAL size without mutating the database. `PRAGMA wal_checkpoint` MUST
  NOT be invoked from any measurement or status path.
- Evaluate `coverage_blockers` before any size probe in
  `try_activate_shadow_generation`, and do not probe at all for a candidate
  already blocked on coverage.
- Throttle or cache the measurement so it cannot run once per indexed meeting
  during backfill.
- Keep `derived_disk_is_estimate` honest: if the measurement becomes exact,
  say so; if it stays an estimate, keep the label and keep the gate on the
  conservative bound.

**Acceptance criteria:**

- A database whose primary tables are large and whose derived tables are
  small reports a small derived figure and does not block activation.
- No measurement or status path issues a checkpoint; a test asserts the WAL
  frame count is unchanged across a status call.
- A candidate blocked on coverage performs no disk probe.
- During a multi-meeting backfill, the number of size probes is bounded and
  does not scale with indexed meetings.
- The reported figure, the gate input, and the RAM ceiling are three distinct
  values in the status report, each labeled for what it measures.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** the measurement method actually used, whether
`dbstat` was available, the overhead factor and how it was measured, and the
probe-frequency bound.

### 2.R3 - Bounded resume/publication memory and repair backoff [M]

**Outcome:** The approved 256-document / 64 MiB batch ceiling holds across
resume and publication, the replacement write lock is short, and a failing
lexical repair cannot spin.

Closes code findings 4 and 5.

**Likely touchpoints:**

- `frontend/src-tauri/src/database/repositories/retrieval.rs`
- `frontend/src-tauri/src/retrieval/worker.rs`

**Required implementation:**

- Add a staged-identity read that returns document ids only, and use it for
  the resume/reuse check. Do not deserialize payloads or clone vectors to
  decide what still needs embedding.
- Stream staged rows through `replace_meeting_documents` in bounded pages:
  read, validate, and insert one page at a time inside the single
  revision-fenced transaction, never holding more than the approved batch
  ceiling resident. The revision fence, journal append, and canonical
  advance stay in that same transaction.
- Keep validation on exactly the bytes that are inserted; a page that fails
  validation aborts the whole replacement with prior documents intact.
- Distinguish the two non-advancing FTS repair outcomes. A superseded mark
  (`Ok(false)`) is normal: leave the meeting due, but count consecutive
  supersessions per item and fall through to the sleep quantum once a small
  bound is reached, so a continuously mutating meeting cannot monopolize the
  loop. A failed mark (`Err`) records persisted bounded backoff exactly as a
  failed refresh does.
- Do not change staging job identity, the revision fence, or journal
  semantics.

**Acceptance criteria:**

- A synthetic meeting far above the batch ceiling resumes from staging and
  publishes without materializing its full document set; peak resident
  document count stays within the approved ceiling on both paths.
- The replacement transaction's held-lock duration scales with page size, not
  with the meeting's document count; a test asserts a concurrent primary
  write is not starved.
- A meeting whose projection revision advances on every pass does not prevent
  other due items from being processed.
- A persistently failing `mark_fts_indexed` records backoff and does not spin;
  a test asserts a bounded number of refresh attempts within a fixed window.
- Crash and cancellation still leave prior documents active and staging
  resumable.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::worker::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** the page size chosen and why, measured peak
resident documents on resume and publish, and the supersession bound.

### 2.R4 - Contract records and small corrections [S]

**Outcome:** The quarantined-coverage state is named where it is normative,
pause stops semantic work without freezing the lexical fallback, and two
small consistency defects are cleared.

Closes architecture findings 4 and 5, and code findings 7 and 8.

**Likely touchpoints:**

- `upstream/docs/hybrid-rag/architecture.md`
- `upstream/docs/hybrid-rag/sprint-2-durable-local-index.md`
- `frontend/src-tauri/src/retrieval/worker.rs`
- `frontend/src-tauri/migrations/20260826000000_add_semantic_document_heading.sql`

**Required implementation:**

- Name the quarantined-coverage state in `architecture.md`: an active
  generation may serve complete-minus-quarantined coverage, canonical rows for
  a terminally failed meeting survive but are not served, and the state is
  reported through `failed_meetings` rather than silently narrowing results.
  Add it to the Failure And Fallback Matrix and to "Generation Activation".
- Stop manual pause from short-circuiting lexical repair. Pause stops semantic
  indexing; FTS repair and publication/catch-up continue, so pausing semantic
  work never leaves the user with both a stale lexical index and no semantic
  index. Record the corrected pause semantics in `architecture.md`'s rollback
  list, where "Pause indexing" currently implies derived work generally.
- Record `retrieval_cancel_rebuild` in the Task 2.5 execution-log entry
  alongside the other three commands.
- Correct `CURRENT_TIMESTAMP` to the RFC 3339 form the rest of the schema
  uses. Both semantic migrations are still uncommitted, so this is an in-place
  edit; if either has been committed by the time this task runs, add a
  forward-only migration instead and say so in the report.
- Confirm in the report that the persisted `force_lexical_retrieval` kill
  switch remains Sprint 5 scope (`architecture.md` "Retrieval Kill Switch"
  requires a Settings surface) and is deliberately not implemented here, so a
  later review does not re-raise it as a Sprint 2 gap.

**Acceptance criteria:**

- `architecture.md` names the quarantined-coverage state in both places.
- Pausing the index leaves a due FTS repair to complete; a test asserts a
  stale projection heals while paused.
- The Task 2.5 log lists all four Tauri commands.
- No timestamp column mixes RFC 3339 and `CURRENT_TIMESTAMP` forms.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::worker::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::migration_tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** the corrected pause semantics, and confirmation
of the kill-switch deferral.

### Deliberately Not In This Remediation

- **Architecture finding 6** (MCP holds the lifecycle but exposes no semantic
  surface). Correct for Sprint 2 scope; Sprints 3-5 consume it.
- **The persisted force-lexical kill switch.** Approved architecture,
  unimplemented, and Sprint 5 scope by its own Settings requirement.
- **Dual-bundle retention packaging itself.** `architecture.md` "Prior-Model
  Retention Across Upgrade" is a forward-looking contract. No prior bundle
  exists — `meetily-retrieval-bundle-1` is the only bundle that has ever
  shipped — so there is nothing to package and no prior identity, artifact
  set, or digest to author. It becomes implementable in the release that
  introduces a second bundle. Only the identity-derivation precondition in
  `2.R1` is in scope now.
- **The prior-model retention RAM measurement.** Required before retention is
  enabled, not before these findings are closed, and only meaningful when a
  bundled-model upgrade actually ships.
- **Re-running the 250k production benchmark.** Required at sprint close after
  `2.R3` lands, not per task.

### Close Conditions

Sprint 2 closes after `2.R1` through `2.R4` complete, `cargo test --lib`,
`cargo check`, `cargo fmt --check`, and `git diff --check` pass, the
production-backend 250k benchmark is re-run and recorded, and fresh code and
architecture reviews approve the result.
