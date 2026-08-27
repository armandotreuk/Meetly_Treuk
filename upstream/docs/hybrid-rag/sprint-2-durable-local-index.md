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
| 2026-08-27 | Remedy the measured 1,482.7 MiB activation peak by cutting session residency and fixing the gate's measurement scope, rather than raising the 1.30 GiB transient ceiling. | The peak is 63% warm ONNX sessions and only 25% snapshot overlap, yet the ceiling's approving decision reasons about snapshot overlap; `2.R6` measured 573.3 MiB for the same activation with no sessions resident. Sprint 2 has no production rerank consumer, so a warm cross-encoder is residency nothing in this sprint uses. Raising the ceiling would also calibrate a retrieval-scoped budget against a whole-process RSS sample that moves with Whisper and webview state - `2.R9` recorded that its benchmark excludes exactly those - so the limit would need raising again for reasons unrelated to retrieval. | Raise the ceiling to 1.60 GiB; redesign activation to avoid two coexisting snapshots; leave Sprint 2 blocked at the gate. | User |
| 2026-08-26 | Record prior-embedding-model retention across an upgrading restart as an architecture amendment; implementation deferred to the sprint that ships a bundled-model upgrade. | Sprint 2B built the activation path this constrains, and the post-remediation reviews found the prior active generation is unqueryable after an upgrading restart. Sprint 2 ships one bundle and never upgrades one, so the defect is latent here and the fix belongs where the upgrade ships. See `architecture.md` "Prior-Model Retention Across Upgrade". | Copy the prior bundle into app data on upgrade; accept FTS-only for the entire rebuild window as the contract; implement retention inside Sprint 2. | User |
| 2026-08-26 | Keep Task `2.R3`'s one revision-fenced transaction; paging bounds memory, not writer-lock duration. Replace the lock-scaling criterion with a corpus-scale before/after lock measurement around the `document_count` recompute fix. | SQLite cannot release the writer lock between pages while preserving all-or-nothing replacement, canonical advance, and journal append under the current schema. Atomicity and bounded memory are the demonstrated requirements. | Add versioned document sets now; allow partial page publication. | User |

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
- Added measured status data (backend, semantic state, active model/generation, coverage counts, canonical/published IDs, activation blockers, resident index bytes, derived disk bytes against the approved 2 GiB target / 3 GiB activation limit) and additive Tauri commands `retrieval_index_status`, `retrieval_rebuild_index`, `retrieval_cancel_rebuild`, and `retrieval_set_index_paused` (manual pause stops semantic indexing at item boundaries without disabling FTS repair, queries, or publication).
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
- Leave `publish_tick` unhooked or remove the `retrieval::index` module: SQLite vectors, journal, and FTS behavior remain unchanged and lexical fallback covers every failure path. The four commands are additive and unregisterable independently.
**Decisions and follow-ups:**
- The publisher acknowledges the full-load bound BEFORE replaying so a complete canonical load is never double-counted into the overlay; deferred (quarantined) delta batches return unapplied and keep `published < canonical`, which disables semantic queries typed until the worker heals the meeting.
- Retired generations' journals are drained to canonical before GC because nothing serves them anymore; deletion additionally requires one clean restart plus one successful query, matching "Retain at most two complete generations".
- Manual pause does not stop FTS repair or publication/catch-up: pausing semantic indexing must not freeze lexical healing or deletion tombstones out of the query path.
- Disk-envelope accounting approximates page overhead from row counts (`ponytail:` marker); the activation gate blocks above 3 GiB and never deletes primary data.
- The publisher samples actual process RSS through `memory-stats` before activation and blocks when unavailable or at/above the approved 1.30 GiB ceiling; re-measure production RAM/disk during a real 250k-scale shadow activation because the governing transient margin from Sprint 1 was ~0.9%.
- Follow-ups for Sprint 3: consume `SearchFailure` as the typed lexical-fallback trigger and `VectorHit.score` as an internal diagnostic only.

### 2.R1 - Generation identity, retention, and journal reclamation

**Status:** Complete
**Owner:** `worker-l` (`ses_fc0343942ffe7YJNtxi3mC6OqN`)
**Completed:** 2026-08-26
**Implemented:**
- Derived the persisted model identity from the complete approved embedding and storage contract, with a readable prefix and SHA-256 discriminator.
- Replaced hash-derived generation resumption with a live-generation lookup and opaque UUID generation IDs.
- Added the forward-only legacy identity migration, the approved Option A GC transition, and acknowledged-journal pruning.
- Added focused regressions for legacy migration/no-reindex behavior, identity discrimination, GC guards/rebuild recovery, opaque resumption, and pruning bounds.
**Implementation:**
- Files: `frontend/src-tauri/src/model_bundle.rs`, `frontend/src-tauri/src/retrieval/model.rs`, `frontend/src-tauri/src/retrieval/worker.rs`, `frontend/src-tauri/src/retrieval/index.rs`, `frontend/src-tauri/src/database/repositories/retrieval.rs`, `frontend/src-tauri/src/database/migration_tests.rs`, `frontend/src-tauri/migrations/20260827000000_derive_legacy_bundle_model_identity.sql`.
- Approach: `mid-<bundle>-<encoding>-c<chunker>-<digest>` derives from the pinned bundle ID, embedding model/revision, ONNX export revision/quantization, dimensions, encoding, and chunker version. The legacy migration repoints generations while preserving documents, state, bounds, journals, and the active pointer. GC keeps every permanent guard and substitutes only the unavailable Fast-hybrid-query condition with the approved clean-restart, active-replacement, zero-lag condition until Sprint 3 close.
**Not implemented:**
- Tasks `2.R2` through `2.R4`, dual-bundle packaging, or any model/chunker/encoding/backend contract change.
**Why not implemented:**
- They are separate sequential tasks or explicitly deferred scope.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests` - pass, 26 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::migration_tests` - pass, 5 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::` - pass, 130 tests.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the pre-existing CRLF notice remains for `.github/workflows/build-windows.yml`.
**Rollback:**
- The migration is forward-only. A rollback after it has run requires a new compatibility migration or purging/rebuilding only derived retrieval state; FTS and primary meeting data remain unaffected.
**Decisions and follow-ups:**
- Option A is the user-approved transitional clause and carries a `ponytail:` expiry path to Sprint 3 close.
- Journal pruning deletes only rows at or below the minimum durable published bound and runs outside replacement transactions, so it does not advance any generation bound.

### 2.R2 - Envelope measurement and activation gate ordering

**Status:** Complete
**Owner:** `worker-l` (`ses_fbfc32efdffeMRd0ff1535QQ6O`)
**Completed:** 2026-08-26
**Implemented:**
- Replaced whole-database/shadow/RAM disk accounting with a seven-derived-table measurement that includes each table's indexes through SQLite `dbstat` when available.
- Added a conservative payload fallback, separate read-only WAL diagnostic, coverage-first activation gating, and safe derived-disk high-watermark reuse.
- Added regressions for primary-data isolation, checkpoint-free status, coverage-first probing, bounded probes, status labeling, stale permissive values, and post-validation admission remeasurement.
**Implementation:**
- Files: `frontend/src-tauri/src/database/repositories/retrieval.rs`, `frontend/src-tauri/src/retrieval/index.rs`.
- Approach: `dbstat` is available in this SQLite build and reports exact allocated pages for the approved derived btrees. The fallback applies a documented 3x payload/row allowance. WAL size is read with `PRAGMA database_list` plus filesystem metadata and remains an unattributed diagnostic because shared WAL pages cannot be safely allocated to derived versus primary data. Only a cached over-limit watermark may block; every permissive and final admission decision remeasures.
**Not implemented:**
- Tasks `2.R3` and `2.R4`, dual-bundle packaging, or any change to the model, chunker, vector encoding, or backend contracts.
**Why not implemented:**
- They are separate sequential tasks or explicitly deferred scope.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests` - pass, 55 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests` - pass, 27 tests.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the pre-existing CRLF notice remains for `.github/workflows/build-windows.yml`.
**Rollback:**
- Revert the measurement path; activation remains block-only and no primary or derived data is deleted. WAL reporting is diagnostics-only and independently removable.
**Decisions and follow-ups:**
- A cached permissive disk reading is not conservative after writes and is never reused. A cached over-limit reading can only temporarily over-block; it expires after 30 seconds and cannot admit an unsafe activation.
- `wal_file_size_bytes` is intentionally separate from the derived-disk gate to avoid allowing primary-data WAL activity to block semantic activation.

### 2.R3 - Bounded resume/publication memory and repair backoff

**Status:** Complete
**Owner:** `worker-l` (`ses_fbf9b5f32ffeC9OENE2QGQFM4W`)
**Completed:** 2026-08-26
**Implemented:**
- Replaced staged-payload resume selection with an ID-only read and streamed replacement pages through the unchanged atomic revision-fenced transaction.
- Preserved prior canonical documents and resumable staging on invalid pages, while restoring typed poisoned-staging cleanup at publication.
- Bounded repeated FTS supersessions and persisted failed-mark backoff.
- Replaced corpus-wide `document_count` recounts with exact replacement and meeting-deletion deltas.
**Implementation:**
- Files: `frontend/src-tauri/src/database/repositories/retrieval.rs`, `frontend/src-tauri/src/database/repositories/meeting.rs`, `frontend/src-tauri/src/retrieval/worker.rs`, `frontend/src-tauri/tests/document_count_lock_hold.rs`.
- Approach: replacement pages contain at most 256 staged documents and validate the exact payload bytes inserted. The one SQLite transaction retains the writer lock for atomic publication; paging is a memory bound. `document_count` now adds inserted rows and subtracts removed rows, with the meeting-deletion path applying its matching decrement.
**Not implemented:**
- Task `2.R4`, versioned document sets, dual-bundle packaging, or non-atomic page publication.
**Why not implemented:**
- The user retained atomic replacement. Versioned document sets are an evidence-gated, separately approved escalation only if the corrected 250k-scale measurement exceeds 250 ms.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::worker::tests` - pass, 32 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests` - pass, 32 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::meeting` - pass, 4 tests.
- `cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test document_count_lock_hold -- --ignored --nocapture` - pass: file-backed WAL fixture with 250,000 canonical rows and a 1,024-document replacement measured 23.05 ms minimum, 24.63 ms median, and 26.42 ms maximum. The worker's pre-correction replay measured 35.1-58.1 ms; that overwritten baseline was not independently replayed.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the pre-existing CRLF notice remains for `.github/workflows/build-windows.yml`.
**Rollback:**
- The counter can revert to a full recount without primary-data impact, but the delta path is required to retain the measured 250k-corpus lock headroom. FTS remains independent.
**Decisions and follow-ups:**
- The corrected measurement is below the 250 ms pause quantum, so no versioned-document-set scope change is triggered. Re-measure before approving that separate architecture if corpus scale or storage behavior materially changes.
- Benchmark limits are documented in `frontend/src-tauri/tests/document_count_lock_hold.rs`: synthetic text, a single process, and no concurrent scanner load; the test uses production migrations and repository code in a file-backed WAL database.

### 2.R4 - Contract records and small corrections

**Status:** Complete
**Owner:** `worker-s` (`ses_fbefce03fffeSXAonXmkbnkUne`)
**Completed:** 2026-08-26
**Implemented:**
- Named quarantined coverage in the architecture's worker, generation-activation, and fallback contracts.
- Made manual pause defer semantic indexing only, while FTS repair and publication/catch-up continue.
- Corrected the heading migration timestamp expression and documented the previously omitted `retrieval_cancel_rebuild` command.
**Implementation:**
- Files: `docs/hybrid-rag/architecture.md`, `docs/hybrid-rag/sprint-2-durable-local-index.md`, `frontend/src-tauri/src/retrieval/worker.rs`, `frontend/src-tauri/src/database/migration_tests.rs`, `frontend/src-tauri/migrations/20260826000000_add_semantic_document_heading.sql`.
- Approach: the worker performs durable FTS repair before observing the semantic-pause branch. The still-uncommitted heading migration now uses `strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`, compatible with Rust's RFC 3339 UTC writers and parser.
**Not implemented:**
- The persisted `force_lexical_retrieval` Settings control, UI/Chat/MCP semantic consumers, or any model/encoding change.
**Why not implemented:**
- The kill switch remains approved Sprint 3.4 scope; consumer integration belongs to later sprints.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::worker::tests` - pass, 33 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::migration_tests` - pass, 5 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib` - pass, 569 tests, 0 failed, 2 ignored.
- `cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test vector_backend_benchmark full_matrix_benchmark -- --nocapture` with `MEETLY_RAG_VECTOR_BENCH=1` - pass. At 250,000 documents: global query p95 51.3 ms; two-snapshot active/shadow peak 1,329.3 MiB, within the approved 1.30 GiB ceiling; two retained generations used 0.40 GiB versus the 3 GiB rebuild envelope.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the pre-existing CRLF notice remains for `.github/workflows/build-windows.yml`.
- `npx tsc --noEmit` - pass; `npx vitest run` - pass, 95 tests. Existing React `act(...)` warnings and mocked disk-error logs remain non-failing test output.
**Rollback:**
- The pause branch and documentation changes revert independently. The heading migration remains uncommitted and was corrected in place; once shipped, timestamp changes require a forward-only follow-up migration.
**Decisions and follow-ups:**
- Quarantined coverage is explicit rather than an implicit semantic omission: canonical rows remain for retry/rebuild, `failed_meetings` exposes the state, and durable FTS remains available.
- The full 250k matrix is recorded here for Sprint 2 close. The resource headroom is narrow by design; later changes that add a retained prior model must remeasure the combined envelope before activation.

### 2.R5 - Retired-generation lag cleanup guard

**Status:** Complete
**Owner:** `worker-l`
**Completed:** 2026-08-27
**Implemented:**
- Retired-generation cleanup no longer acknowledges or drains a journal before attempting deletion. A generation whose canonical change ID is ahead of its published change ID remains retained.
- Added a restart regression proving a lagging retired generation is not deleted and its published bound is unchanged.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/index.rs`.
- Approach: Let the repository's existing unacknowledged-journal deletion guard decide eligibility without mutating the retired generation's publication bound.
**Not implemented:**
- None.
**Why not implemented:**
- Not applicable.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests restarted_retired_generation_with_lag_is_retained_without_acknowledgement` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass.
**Rollback:**
- Revert the cleanup guard change and focused regression; the repository deletion guard remains unchanged.
**Decisions and follow-ups:**
- Retired journals remain durable until their published bound catches up through the normal publisher path; cleanup must never make a lagging generation delete-eligible by acknowledging it.

### 2.R6 - Production-representation activation-envelope evidence

**Status:** Complete
**Owner:** `worker-l`
**Completed:** 2026-08-27
**Implemented:**
- Added a release-gated 250k benchmark that exercises the production representation and activation path end to end, closing the Final Code Review blocker that the retained compact-mirror benchmark cannot validate production activation (`frontend/src-tauri/src/retrieval/index.rs`, tests module: `bench_2r6_production_activation_envelope`, gated by `MEETLY_RAG_INDEX_BENCH=1`; skipped without cost otherwise).
- The fixture registers one approved model row (768 dimensions, Int8, fixed `1/127` dequantization) and two live generations of the same identity ("gen-bench-active", then the manual-rebuild-shaped "gen-bench-shadow"), each backfilled through the repository's staging plus revision-fenced atomic replacement transactions: canonical rows carry production bytes produced by the worker's own `quantize_int8`, and meeting state, journal entries, validation, and the incremental counter all move exactly as in production.
- Two `publish_tick` passes drive the real sequence: the first performs the full active-generation validation load, journal catch-up, coverage/disk/RAM gates, pointer promotion, and snapshot install; the second reloads and journal-catches-up the entire shadow candidate while the active snapshot stays installed - the exact state the production RAM gate measures - then promotes and retires the previous generation.
- Measurement methodology: Windows process working-set counters via `K32GetProcessMemoryInfo` (same metric family as the retained Sprint 1/2.R4 evidence) sampled at process start and around the measured activation window; peak working set is monotonic per process, so the reported peak bounds every prior phase including the two-snapshot coexistence moment. The asserted limit is the unchanged `ACTIVATION_RAM_CEILING_BYTES` (1.30 GiB transient ceiling); nothing was relaxed. A reader-path `QueryIndexService::search` over the freshly activated 250k-row snapshot verifies serveability. Output carries counts, byte figures, timings, and verdicts only - no raw text, tokens, or vector bytes are ever logged.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/index.rs`.
- Approach: exercised code is production code only (`publish_tick`, `try_activate_shadow_generation`, `base_snapshot`, journal replay, repository transactions); the benchmark owns fixtures, not logic. `IndexSnapshot` holds owned provenance metadata and contiguous validated int8 rows allocated during real activation instead of the benchmark-local compact numeric-metadata mirror the review flagged.
**Not implemented:**
- Bundled ONNX session residency is not loaded in this process; no second bundle exists to load, and fabricating one was out of scope.
**Why not implemented:**
- Model/session residency at scale is already evidenced by the retained 2.R4 full-matrix run recorded below; this task closes the remaining unproven component (the production two-snapshot envelope). Additively: 573.3 MiB snapshot-envelope peak + the session increments logged by `[envelope-sessions]` in that run remain what any future combined remeasure must compare against the same 1.30 GiB ceiling.
**Verification:**
- `$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'meetily-cargo-target'; $env:MEETLY_RAG_INDEX_BENCH = "1"; cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests::bench_2r6_production_activation_envelope -- --nocapture` - pass: `[active] production snapshot activated (250000 documents) in 10606 ms`; `[envelope-parts] working set before activation window 275.8 MiB, after 268.3 MiB; resident index vectors 183.1 MiB`; `[envelope-peak] measured active+shadow process peak working set 573.3 MiB (11085 ms window) vs the approved 1.30 GiB transient ceiling -> PASS`. Total run time ~53 s; result is stable across repeat runs (earlier pass: 589.3 MiB).
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests` - pass, 57 tests (gated test skips without the env flag).
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml" --lib` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the pre-existing CRLF notice remains for `.github/workflows/build-windows.yml`.
- `pnpm run typecheck` (`npx tsc --noEmit`) - pass; `npx vitest run` - pass, 95 tests. Unrelated to this change; rerun as standard verification hygiene.
**Rollback:**
- Delete the gated test block; production behavior is untouched (test-module-only change). The measurement stands independently of any other remediation file.
**Decisions and follow-ups:**
- Envelope arithmetic sanity: peak ≈ 2 × (183.1 MiB vectors + owned provenance metadata) + process overhead, matching the two-full-snapshot shape rather than a partial load; headroom to the ceiling is now wide in isolation, so future model-session residency growth is visible against its separately retained evidence.
- Fixture corpus shape mirrors the retained lock-span fixture precedent (251 meetings x ~995 documents): fewer meetings bound fixture time while every byte-level property of the snapshots is unchanged.
- The heading-provenance unit test in this file asserts a doubly-encoded `"DecisÃµes finais"` string (self-consistent write/assert, so passing). Pre-existing encoding drift unrelated to 2.R6; flagged for later cleanup.

### 2.R7 - Before/after lock and concurrent-writer benchmark evidence

**Status:** Complete
**Owner:** `worker-l`
**Completed:** 2026-08-27
**Implemented:**
- Strengthened the ignored release-only lock benchmark with a test-only reference replacement that independently replays the prior full-corpus `COUNT(*)` document-count update inside the prior replacement transaction shape; production remains on the exact-delta path.
- Added equivalent independent 250,000-row file-backed WAL fixtures for baseline/current measurement and a real primary `meetings` writer on a separate SQLite connection. The test asserts the writer overlaps the replacement, observes a non-trivial acquisition wait, and completes successfully without exposing source data.
- Retained the current-path 250 ms pause-quantum assertion and labeled replacement, writer-acquisition, and writer-completion timings as upper bounds rather than exact lock durations.
**Implementation:**
- Files: `frontend/src-tauri/tests/document_count_lock_hold.rs`, this file.
- Approach: each fixture applies the real migrations, seeds 250,000 canonical rows across 251 meetings, and replaces the 750-row worst-case set with 1,024 staged documents after two warmups and across seven measured iterations. Release results were baseline full-count min/median/max **52.2198/53.677/55.0496 ms**, current exact-delta **31.1563/32.9156/33.4253 ms**, and concurrent current replacement **32.5342 ms** with a separate-writer `BEGIN IMMEDIATE` wait upper bound of **46.2643 ms** and completion upper bound of **47.1465 ms** (writer started after a 1 ms scheduling delay; `blocked=true`).
**Limitations:**
- The 2-dimensional int8 vectors and synthetic bodies are shape-valid equivalent fixtures, not production 768-dimensional model payloads; contention covers one primary writer and no scanner or interactive search load.
**Not implemented:**
- Versioned document sets or any production replacement/contract change.
**Why not implemented:**
- The current replacement upper-bound maximum is 33.4253 ms, and the contended current replacement is 32.5342 ms, both below the approved 250 ms pause quantum; the evidence does not trigger the separate versioned-document-set escalation.
**Verification:**
- `$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'meetily-cargo-target'; cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test document_count_lock_hold -- --ignored --nocapture` - pass: baseline/current/concurrent-writer results above; current-path gate PASS.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test document_count_lock_hold --no-run` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass.
**Rollback:**
- Remove the test-only reference/concurrency harness and this execution entry; production implementation and atomic replacement remain unchanged.
**Decisions and follow-ups:**
- The benchmark proves a competing primary writer completed after overlapping the replacement, but its attempt-to-acquisition and completion clocks include scheduler, connection, transaction, and commit/return overhead. They are upper bounds and must not be read as exact writer-lock duration.
- No versioned document-set scope change is escalated. Re-run this benchmark before changing corpus scale, storage representation, or the 250 ms gate.

### 2.R8 - Fail-closed no-dbstat derived-disk activation gate

**Status:** Complete
**Owner:** `implementation subagent` (`openai/gpt-5.6-luna`)
**Completed:** 2026-08-27
**Implemented:**
- Changed the shared derived-disk measurement result to distinguish exact `dbstat` bytes from an unavailable measurement. The payload/row calculation remains available only as an explicitly labelled status estimate.
- Made the activation gate accept only exact derived-table bytes; an unavailable `dbstat` measurement now blocks activation with `derived disk measurement unavailable; refusing activation` and cannot be admitted through the cached path.
- Kept the seven-table/index allow-list, primary meeting/transcript/FTS exclusion, exact `dbstat` page accounting, and read-only WAL diagnostic behavior unchanged.
- Added a test-injected unavailable measurement regression that proves a fully covered candidate is not promoted without relying on the linked SQLite build lacking `dbstat`.
**Implementation:**
- Files: `frontend/src-tauri/src/database/repositories/retrieval.rs`, `frontend/src-tauri/src/retrieval/index.rs`, this file.
- Approach: exact measurements expose `bytes` and gate input; no-`dbstat` measurements expose `status = unavailable`, `bytes = null`, and an optional payload estimate for diagnostics only. The shared gate and blocking cache consume the typed measurement, so no caller can accidentally authorize activation with the estimate.
**Not implemented:**
- No schema, model, memory, chunking, vector-backend, primary-data, FTS, or WAL behavior changes.
**Why not implemented:**
- These areas are outside the remediation and the existing exact `dbstat`/diagnostics paths already satisfy their contracts.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests` - pass, 58 tests (includes unavailable-measurement gate).
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests` - pass, 32 tests.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the pre-existing CRLF notice remains for `.github/workflows/build-windows.yml`.
- From `frontend`: `pnpm run typecheck` - pass; `npx vitest run` - pass, 95 tests across 20 files.
**Compatibility/Rollback:**
- No migration or persisted-data change is required. Builds with `dbstat` retain the exact existing measurement and primary/WAL separation; builds without it remain FTS-compatible but keep semantic activation disabled until an exact measurement is available. Reverting the code and focused test is safe because retrieval rows and primary data remain rebuildable and untouched.
**Decisions and follow-ups:**
- A no-`dbstat` payload estimate is status-only and never enters the activation gate; `derived_disk_bytes` and `derived_disk_gate_input_bytes` are null with an explicit `unavailable` status, while `derived_disk_estimate_bytes` is diagnostic only.
- The existing cached high-watermark optimization also caches unavailable results only as blockers; permissive exact readings continue to be remeasured.

### 2.R9 - Warmed-session combined activation-envelope measurement

**Status:** Blocked
**Owner:** implementation subagent (`z-ai/glm-5.3-flash`)
**Completed:** 2026-08-27
**Implemented:**
- Closed the Final Code Review (R12) measurement gap by extending the release-gated R6 benchmark (`bench_2r6_production_activation_envelope`): the one existing approved staged bundle now loads its embedding and reranker ONNX sessions through the production `model::get_or_load` cache path and warms them (one document embed through the production batch path, one rerank pair) before any snapshot fixture is built. No second bundle or synthetic identity was fabricated.
- Both benchmark generations register under the real bundle-derived `bundled_model_identity()` instead of the former synthetic `"test-e5-int8"` label, keeping the unchanged fixed Int8 / `1/127` storage contract, and an assertion proves `model::cached_model` resolves that identity from the process-global session cache.
- The envelope verdict now prints before post-promotion assertions, so a refusal by the production RAM gate reports complete counts/MiB/timings plus the gate's own blocker instead of dying on a state assertion first.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/index.rs` (tests module), this file.
- Approach: methodology unchanged from 2.R6 apart from session residency: Windows process working-set counters via `K32GetProcessMemoryInfo` (monotonic per-process peak bounds every phase including fixtures) plus the production gate's own `memory-stats` RSS sample inside the unchanged `ram_gate_blocker`; 250,000 documents per generation in two live generations of the single approved identity; two `publish_tick` passes drive the real validation load, journal catch-up, coverage/disk/RAM gates, promotion, and retirement. The asserted limit is the unchanged `ACTIVATION_RAM_CEILING_BYTES` (1,395,864,371-byte 1.30 GiB transient ceiling); nothing was relaxed, redefined, or worked around.
**Blocker evidence (measured combined peak exceeds the fixed ceiling):**
- `$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'meetily-cargo-target'; $env:MEETLY_RAG_INDEX_BENCH = "1"; cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests::bench_2r6_production_activation_envelope -- --nocapture` - FAIL (activation blocked), stable across two runs: `[envelope-sessions] approved bundle embedding+reranker sessions loaded+warm in 2707 ms; working set 938.1 MiB, peak 1022.3 MiB`; `[active] production snapshot activated (250000 documents) in 17940 ms`; `[envelope-parts] working set before activation window 1192.7 MiB, after 1215.6 MiB; resident index vectors 183.1 MiB`; `[envelope-peak] measured active+shadow process peak working set 1482.7 MiB (16373 ms window) vs the approved 1.30 GiB transient ceiling -> FAIL`; panic: peak 1,554,751,488 bytes >= the 1,395,864,371-byte ceiling (~151.5 MiB over); `[envelope-gate] production RAM gate refused shadow activation: ["generation gen-bench-shadow: measured resident memory 1510690816 bytes meets or exceeds the 1395864371 byte activation ceiling"]`. First run agreed at gate level: gate-sampled resident memory 1,518,469,120 bytes vs the same ceiling (~117 MiB over). Total run time ~91-101 s.
- Verdict: with production-representative warmed model sessions resident, the combined active+shadow activation peak exceeds the unchanged 1.30 GiB ceiling on both metrics (gate RSS ~1.41-1.45 GiB at sample point; monotonic peak working set 1.4827 GiB), so the production RAM gate refuses a 250k-scale two-snapshot activation exactly as designed. No behavior or architectural-contract change was made in response; production remains fail-closed blocked-only.
**Not implemented:**
- Any user-approved remedy for the excess, and therefore any unblock.
**Why not implemented:**
- Choosing a remedy (approved ceiling revision, smaller/slimmer bundle residency, different retention/shadow strategy) requires a user decision against the normative envelope; fabricating headroom was out of scope.
**Limitations:**
- Evidence is single-machine (this Windows x64 host); absolute numbers vary with hardware, but the structural contributor is the ~900 MiB resident session weight (int8 embedding + quint8 reranker ONNX artifacts), which dominates the previously recorded 573.3 MiB session-free snapshot-only peak.
- Peak working set still counts every prior fixture phase (retained 2.R6 methodology); it excludes Whisper/webview/UI loads of a full application run beyond the retrieval session set.
- Warm-up runs one embed batch and one rerank pair rather than extended corpus-scale inference; arena drift after heavier use could move the figure further up, never materially down.
**Verification:**
- See Blocker evidence above for the gated release benchmark outcome.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests` - pass, 58 tests (gated benchmark skips without the env flag).
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- From `frontend`: `pnpm run typecheck` - pass; `npx vitest run` - pass, 95 tests across 20 files.
**Rollback:**
- Revert the tests-module changes; production behavior is untouched (test-module-only diff). The 2.R6-era un-warmed benchmark shape restores independently.
**Decisions and follow-ups:**
- The 1.30 GiB activation ceiling stands as normative; the measured failure does not weaken any gate or contract. `2.R9` blocks until the user approves a remedy; re-running this benchmark requires no other changes once a remedy lands.
- Session-cache identity coherence is now proven end to end: the cached bundle resolves `bundled_model_identity()`, and snapshot activations consume that same persisted identity.

### 2.R10 - Terminal-generation tombstone repair

**Status:** Complete
**Owner:** `implementation subagent` (`openai/gpt-5.6-luna`)
**Completed:** 2026-08-27
**Implemented:**
- Added a forward-only migration that replaces meeting-delete tombstone capture with live-generation-only publication and repairs obsolete terminal-generation journal tails.
- Added populated-upgrade and future-delete regressions covering active/building preservation, retired/failed cleanup eligibility, and primary/derived deletion cascades.
**Implementation:**
- Files: `frontend/src-tauri/migrations/20260827010000_repair_terminal_generation_tombstones.sql`, `frontend/src-tauri/src/database/migration_tests.rs`, `frontend/src-tauri/src/database/repositories/retrieval.rs`.
- Approach: terminal journal rows above each generation's existing `published_change_id` are discarded as obsolete terminal state; the published bound is never advanced. The replacement trigger journals deletes only for `building` and `ready` generations, leaving R5's GC guard unchanged.
**Not implemented:**
- No changes to GC acknowledgement/draining, primary-data deletion behavior, or model/encoding/backend contracts.
**Why not implemented:**
- Those behaviors are outside this remediation and remain the existing runtime contracts.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::retrieval::tests` - pass, 32 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::meeting::tests` - pass, 1 test.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::migration_tests` - pass, 6 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests` - pass, 58 tests.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the pre-existing CRLF notice remains for `.github/workflows/build-windows.yml`.
- From `frontend`: `pnpm run typecheck` - pass; `npx vitest run` - pass, 95 tests across 20 files.
**Rollback:**
- The migration is forward-only; rollback requires a compatibility migration or rebuilding derived retrieval state. Primary meetings and existing FTS behavior remain unaffected.
**Decisions and follow-ups:**
- Terminal tails are removed without changing `published_change_id`, so migration repair is not synthetic publication acknowledgement. Future terminal deletion tails cannot be created by the replacement trigger.

### 2.R11 - Upsert-aware compaction threshold

**Status:** Complete
**Owner:** `implementation subagent` (`openai/gpt-5.6-luna`)
**Completed:** 2026-08-27
**Implemented:**
- Counted base rows shadowed by upserts together with deleted base rows when evaluating the approved 2% compaction threshold, without double counting meetings affected by both states.
- Added a regression proving that replacing a 100-document base meeting with one upserted document triggers compaction and removes the stale vectors.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/index.rs`.
- Approach: The existing base meeting document counts are filtered through the overlay's shared shadow predicate, preserving immutable snapshot construction, cancellation, reader behavior, and the existing denominator.
**Not implemented:**
- None.
**Why not implemented:**
- Not applicable.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests` - pass, 59 tests.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the pre-existing CRLF notice remains for `.github/workflows/build-windows.yml`.
**Rollback:**
- Revert the shared shadow-count correction and focused regression; compaction and reader/search semantics otherwise remain unchanged.
**Decisions and follow-ups:**
- The approved 2% denominator remains the base row count; each shadowed base row is counted once, whether hidden by upsert or deletion.

### 2.R12 - Activation envelope remedy: session residency and gate scope

**Status:** Complete
**Owner:** `worker-l` (`openai/gpt-5.6-luna`)
**Completed:** 2026-08-27
**Implemented:**
- Changed `get_or_load` to verify the complete approved bundle, build and warm only the embedding engine, and retain the reranker configuration without constructing its ONNX session. The first non-empty rerank request constructs the reranker through the same cached runtime and runs the existing input/output name, dtype, rank, label-index, and score-contract validation before inference.
- Extended the release-gated `bench_2r6_production_activation_envelope` instead of adding a duplicate benchmark. It now measures embedding-only residency and the deferred reranker's own weight, then gates the real 250,000-document two-snapshot activation while only the embedding session is resident. Existing reference-parity reranker tests still build the reranker explicitly.
- Made the RAM gate's scope explicit as whole-process RSS and exposed that scope beside the measured status value and unchanged ceiling.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/model.rs`, `frontend/src-tauri/src/retrieval/index.rs`, `docs/hybrid-rag/architecture.md`, this file.
- Approach: `RuntimeInner` owns one lazily initialized reranker slot and the manifest-backed `BundleIdentity`; failed deferred loads remain retryable and cancellation avoids starting a reranker load. Stage 1 passed, so no session eviction was added. Stage 3 keeps the process RSS sample and explicitly treats `ACTIVATION_RAM_CEILING_BYTES` as the same whole-process budget, including retrieval snapshots/sessions, Whisper/audio, Tauri/webview, allocator, and other process overhead.

Component and gate measurement from the release benchmark:

| Component | Bytes | MiB | Source/meaning |
|---|---:|---:|---|
| Embedding-only session weight | 583,065,600 | 556.1 | `get_or_load` after embedding warm-up, relative to process baseline |
| Reranker own weight | 392,376,320 | 374.2 | first post-activation rerank request; built and validated then |
| Snapshot-only activation peak | not emitted in bytes | 573.3 | retained `2.R6` no-session 250k two-snapshot measurement |
| Stage-1 combined active+shadow peak | 1,172,094,976 | 1,117.8 | release benchmark with embedding only resident |

The fixed ceiling is `1,395,864,371` bytes (1.30 GiB). Stage 1 therefore
cleared it by **223,769,395 bytes (213.4 MiB)**. Stage 2 was not run because
stage 1 passed; no eviction precondition measurement was required. The
benchmark's deferred reranker load reported the own-weight row after the
activation gate had passed.

**Not implemented:**
- Stage 2 session eviction, activation-window FTS-only transition, and
  post-activation reload handling; the stage-1 margin makes them unnecessary.
- No model, chunk, encoding, backend, bundle, or production rerank-consumer
  change.
**Why not implemented:**
- Stage 1 is the first clearing remedy required by the task and its measured
  peak is below the unchanged ceiling with substantial margin. There is no
  production Sprint 2 rerank consumer whose residency would justify paying
  the eviction complexity.
**Verification:**
- `$env:CARGO_TARGET_DIR = 'C:\Users\arman\cargo-target'; $env:MEETLY_RAG_INDEX_BENCH = '1'; cargo test --release --manifest-path 'frontend/src-tauri/Cargo.toml' --lib retrieval::index::tests::bench_2r6_production_activation_envelope -- --nocapture` - pass; latest 250,000-document stage-1 peak `1,172,094,976` bytes, margin `223,769,395` bytes; embedding weight `583,065,600` bytes; reranker weight `392,376,320` bytes. Earlier repeat also passed at peak `1,169,698,816` bytes.
- `cargo test --manifest-path 'frontend/src-tauri/Cargo.toml' --lib retrieval::` - pass, 152 tests.
- `cargo check --manifest-path 'frontend/src-tauri/Cargo.toml'` - pass.
- `cargo fmt --manifest-path 'frontend/src-tauri/Cargo.toml' --check` - pass.
- `git diff --check` - pass for the task files; the pre-existing CRLF notice remains on `.github/workflows/build-windows.yml`.
- From `frontend`, `pnpm run typecheck` - pass; `npx vitest run` - pass, 95 tests across 20 files.
**Rollback:**
- Revert the lazy slot/warm-up and benchmark/status-scope changes; eager reranker construction and the prior whole-process gate behavior return. No persisted data or model contract changes are involved.
**Decisions and follow-ups:**
- Stage 1 selected; stage 2 intentionally stopped after the measured pass. The gate-scope decision is whole-process RSS, documented next to the ceiling in `architecture.md`; this avoids comparing an unscoped process measurement with retrieval-only arithmetic.
- The `2.R6` snapshot-only baseline was recorded only to one decimal MiB by its original run, so its exact byte value is not claimed. Future benchmark output now emits exact component and peak bytes.
- The benchmark is single-machine Windows x64 evidence and does not load a full application's optional Whisper/webview state; the whole-process gate remains conservative for production.

### 2.R13 - Retrieval-scoped activation RAM gate (final-review blocker)

**Status:** Blocked
**Owner:** implementation subagent (`z-ai/glm-5.3-flash`)
**Recorded:** 2026-08-27
**Closes:** the Final Code Review (R13) blocker "R12 relabels a whole-process gate without whole-application calibration" - by technical proof that the requested retrieval-scoped remedy is not implementable, not by shipping a defective gate.

**Task:** implement the retrieval-scoped alternative in the existing `2.R12` Stage 3 authority. The gate must measure/reason only about actual retrieval residency required at activation - approved retrieval model sessions plus active+shadow snapshots and their metadata/overlays - must never block solely on unrelated process RSS, must not subtract an arbitrary process baseline or use a fixed heuristic factor that can undercount, must prefer the smallest conservative mechanism available from the existing runtime and model/snapshot ownership, and must fail closed on any unavailable or unprovable retrieval term. The `ACTIVATION_RAM_CEILING_BYTES` value `1,395,864,371` stays unchanged; no second bundle or model may be invented.

**Blocker proof - the session term cannot be measured without undercounting.** Every candidate mechanism was verified against the actual dependency sources (`ort-sys` 2.0.0-rc.10 binding ONNX Runtime 1.22.0, `ort` 2.0.0-rc.10, `memory-stats` 1.x) and fails at least one hard requirement:

1. **No per-session query exists in the bound ONNX Runtime API.** The generated `OrtApi` surface in `ort-sys` 2.0.0-rc.10 declares no session memory/stat query of any kind. `OrtApi::CreateAllocator(session, mem_info)` exists, but the returned `OrtAllocator` struct exposes only `Alloc`/`Free`/`Info`/`Reserve` function pointers - no byte totals, no arena statistics. The `ort` 2.0.0-rc.10 Rust crate likewise exposes no session memory API (its `memory::Allocator`/`MemoryInfo` surface serves IO binding only). Model-session residency is therefore unreadable at runtime through existing public facilities.
2. **Rust allocator accounting cannot see the sessions.** ONNX Runtime is native code allocating through the native Windows heap; FFI allocations never traverse the Rust `#[global_allocator]`. Ownership-based accounting can count Rust-owned bytes but structurally cannot observe the dominant session term.
3. **Windows offers no per-component attribution.** `K32GetProcessMemoryInfo`, `QueryWorkingSetEx`, `GetProcessHeaps`/`HeapWalk`, and `VirtualQuery` are all process-wide enumerations with no owner tag for anonymous/native allocations; no Windows facility attributes pages to a component. The existing `memory-stats` 1.x returns only process totals (`GetProcessMemoryInfo` -> `WorkingSetSize`/`PagefileUsage`).
4. **Whole-process deltas around a session load/drop can undercount.** Working-set size is not monotonic: concurrent frees by unrelated components during the window subtract from the delta, and freed-but-resident pages being reused keep RSS or peak-working-set deltas flat while the session's true residency grows. Such a sample is also exactly the whole-process quantity the review rejected, and it can block on unrelated allocations - both prohibited properties.
5. **A fixed session-weight constant is a prohibited undercounting factor.** Session residency varies with the machine-dependent intra-op thread count (`approved_intra_threads()` = clamp(cores/2, 1, 4)), ORT arena growth over batch/sequence history, thread-pool stacks, and allocator behavior. Any constant - including 2.R12's measured 581-583 MB embedding weight - undercounts on some host or after heavier arena growth.
6. **Weights-file bytes lower-bound, never upper-bound, session residency.** `models/embedding/model_int8.onnx` is 278,184,162 bytes and `models/reranker/model_quint8_avx2.onnx` is 118,620,016 bytes, but activation arena, runtime thread stacks, and tokenizer/runtime overhead are not bounded by any public data, so a file-size-derived figure undercounts.
7. **The snapshot term alone is provably accountable - and proves the undercount.** `IndexSnapshot` ownership (base vector `Vec<u8>` capacity, `DocumentMeta`/`OverlayDoc` string capacities, `BTreeMap`/`BTreeSet` node accounting, a bounded per-allocation overhead margin) yields a conservative non-undercounting figure. But per the 2.R12 measured table, snapshots were ~25% of the combined peak (183.1 MiB resident vectors; 573.3 MiB snapshot-only peak) while sessions were 63% (~556-938 MiB). A "retrieval-scoped" gate over snapshots only would omit the dominant term - exactly the cosmetic scope label this task prohibits.
8. **Evicting sessions across the activation window cannot make the term provably zero.** No ownership-time facility can prove ORT released its arena (2.R12 Stage 2's own precondition warns arena blocks may be retained and demands an RSS-delta proof - itself process-wide and undercount-capable), and gating a sessions-free quantity moves the real envelope peak (the post-activation session reload) outside the gate entirely. Stage 2 was also explicitly conditional on Stage 1 failing; Stage 1 passed with margin.

**Conclusion.** The three hard requirements - retrieval-scoped, provably non-undercounting, and never blocking on unrelated process RSS - cannot be jointly satisfied for the session term with existing public/runtime facilities. The two implementable substitutes are both prohibited: a snapshots-only gate undercounts the dominant term (cosmetic scope), and a fail-closed-on-sessions gate would permanently block every activation (a feature regression masquerading as a measurement). `2.R13` is therefore recorded Blocked with no production change: the `2.R12` whole-process gate remains in force, unchanged, and still fails closed when the measurement is unavailable.

**Not implemented:** everything - no production code, benchmark, status-payload, or `architecture.md` change. The requested `architecture.md` scope note next to the ceiling cannot be written truthfully: no retrieval scope was chosen, and the existing section documents the `2.R12` whole-process decision that remains in force.
**Why not implemented:** the blocker proof above; both substitutes are prohibited by the task's own constraints.
**What would unblock (user decisions, none taken here):**
- Approve calibrating the whole-process gate against a real full-application run (Whisper/audio/Tauri/webview resident) and re-derive the whole-process ceiling from that measurement - requires a ceiling change.
- Fund runtime attribution infrastructure so session bytes become measurable in production (e.g., an ORT-side counted allocator; not achievable through the ORT 1.22 C API as bound, since `CreateAllocator`/`RegisterAllocator` do not intercept the internal CPU arena).
- Wait for an upstream ONNX Runtime per-session memory-stat API, then implement the scoped gate like-for-like.
- Approve `2.R12` Stage 2 eviction plus snapshots-only accounting, explicitly accepting that the gate quantity excludes sessions by construction and the post-activation reload peak is ungated.
- Relax the no-fixed-factor constraint and approve a constructed conservative session upper bound with its accepted undercount risk.
**Verification** (all against the unchanged tree; the R12 gate remains functional as shipped):
- `$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'meetily-cargo-target'; $env:MEETLY_RAG_INDEX_BENCH = "1"; cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests::bench_2r6_production_activation_envelope -- --nocapture` - pass: `[envelope-sessions]` embedding loaded+warm in 1645 ms, embedding-only weight 581,218,304 bytes; `[active]` 250,000 documents activated in 10125 ms; `[envelope-peak]` peak working set 1,170,399,232 bytes (1116.2 MiB), margin 225,465,139 bytes (215.0 MiB) vs the unchanged 1.30 GiB ceiling -> PASS; `[envelope-reranker]` deferred reranker built+validated, own weight 391,102,464 bytes.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::` - pass, 152 tests.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; only the pre-existing CRLF notice on `.github/workflows/build-windows.yml` remains.
- From `frontend`: `pnpm run typecheck` - pass; `npx vitest run` - pass, 95 tests across 20 files.
**Rollback:**
- None needed - no production or benchmark diff exists for this task; the working tree carries only the prior tasks' changes.
**Decisions and follow-ups:**
- The gate-scope decision returns to the user with the unblock options above; `2.R12`'s entry, the review records, and the `architecture.md` gate-scope section are intentionally untouched because no scope change shipped.
- Verified source-level facts for this proof: `ort-sys` 2.0.0-rc.10 `OrtApi`/`OrtAllocator` declarations (no stats surface), `ort-sys` `build.rs` `ONNXRUNTIME_VERSION = "1.22.0"`, `ort` 2.0.0-rc.10 session API (no memory query), `memory-stats` 1.x Windows implementation (`GetProcessMemoryInfo`), and the staged bundle artifact byte lengths.

### 2.R14 - Lazy reranker initialization ownership

**Status:** Complete
**Owner:** `implementation subagent` (`openai/gpt-5.6-luna`)
**Completed:** 2026-08-27
**Implemented:**
- Changed the deferred reranker slot to own an `Arc<Engine>`, so initialization publishes one fully loaded and contract-validated engine while callers receive owned handles rather than a mutex guard.
- Added cancellation-aware waiting and retry-safe lazy construction, with focused regressions for concurrent first load, cancellation while waiting, and failed-load retry.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/model.rs`, this file.
- Approach: `lazy_engine` holds the initialization mutex only through the build/validation and Arc publication; `rerank_sync` then tokenizes, batches, checks cancellation, and invokes the session without that mutex. `Engine.session` remains the sole serialization boundary for the existing ONNX session.
**Not implemented:**
- No model/bundle I/O, manifest, contract, or ONNX session configuration changes.
**Why not implemented:**
- Not applicable.
**Verification:**
- Focused tests `concurrent_first_reranker_load_shares_one_initialized_engine`, `waiting_rerank_initialization_observes_cancellation_before_building`, and `failed_reranker_load_is_retryable` - pass, 1 each.
- `cargo test --manifest-path 'frontend/src-tauri/Cargo.toml' --lib retrieval::model::tests -- --nocapture` - pass, 18 tests.
- `$env:CARGO_TARGET_DIR = 'C:\Users\arman\cargo-target'; cargo check --manifest-path 'frontend/src-tauri/Cargo.toml'` - pass.
- `cargo fmt --manifest-path 'frontend/src-tauri/Cargo.toml' --check` - pass.
- `git diff --check` - pass; the pre-existing CRLF notice remains on `.github/workflows/build-windows.yml`.
**Rollback:**
- Revert the lazy-slot ownership change and its focused regressions; bundled model contracts and persisted retrieval data are unaffected.
**Decisions and follow-ups:**
- Failed construction leaves the slot empty, so the next caller retries; a cancelled caller never starts deferred construction.
- Waiting uses a 1 ms polling quantum, and cancellation cannot interrupt an already-started ONNX construction or a session run; the latter remains serialized by `Engine.session`.

### 2.R15 - Fail-closed staging cleanup recovery

**Status:** Complete
**Owner:** `implementation subagent` (`openai/gpt-5.6-luna`)
**Completed:** 2026-08-27
**Implemented:**
- Cleanup is now a worker boundary: if divergent-row pruning and its fallback discard both fail, or unreadable-staging recovery and discard both fail, the item records bounded retry/backoff and returns before embedding or publication.
- Added a focused regression proving failed cleanup preserves prior canonical documents and publication state and schedules retry without embedding.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/worker.rs`, this file.
- Approach: retain successful cleanup and resumable staging unchanged; fail closed only when neither cleanup route can establish safe staging.
**Not implemented:**
- No data model, model/bundle contract, dependency, or logging-surface changes.
**Why not implemented:**
- Not applicable.
**Verification:**
- `cargo test --manifest-path 'frontend/src-tauri/Cargo.toml' --lib retrieval::worker::tests::failed_divergent_staging_cleanup_retries_without_publishing -- --nocapture` - pass.
- `cargo test --manifest-path 'frontend/src-tauri/Cargo.toml' --lib retrieval::worker::tests` - pass, 34 tests.
- `cargo test --manifest-path 'frontend/src-tauri/Cargo.toml' --lib database::repositories::retrieval::tests` - pass, 32 tests.
- From `frontend`: `pnpm run typecheck` - pass; `npx vitest run` - pass, 95 tests across 20 files.
- `$env:CARGO_TARGET_DIR = 'C:\Users\arman\cargo-target'; cargo check --manifest-path 'frontend/src-tauri/Cargo.toml'` - pass.
- `cargo fmt --manifest-path 'frontend/src-tauri/Cargo.toml' --check` - pass; `git diff --check` - pass for the task files.
**Rollback:**
- Revert the worker guard and focused regression; staging remains durable and recoverable.
**Decisions and follow-ups:**
- Cleanup errors remain bounded and opaque: logs contain error context and identifiers only, never meeting text, tokens, or vectors.

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

### Final Code Review (R11)

**Reviewer:** `gpt-5.6-sol` (`ses_fbef08e9affeCx7235RSW2Ljmr`), 2026-08-27
**Verdict:** Changes requested

**Findings:**
1. **Blocker - retired-generation GC acknowledges lag before deciding whether deletion is eligible.** It can acknowledge a lagging retired journal through canonical and then delete in the same pass, bypassing the 2.R1 requirement that non-zero lag prevents reclamation. `frontend/src-tauri/src/retrieval/index.rs:1719-1737`.
2. **Blocker - the 250k release benchmark does not exercise the production index representation.** Its compact numeric metadata and per-vector scale differ from `QueryIndexService`'s owned provenance and fixed int8 storage, so the narrow 1.30 GiB envelope result cannot validate production activation. `frontend/src-tauri/tests/vector_backend_benchmark.rs:121-139,170-177`; `frontend/src-tauri/src/retrieval/index.rs:106-128,470-478`.
3. **Should-fix - the lock benchmark records only the post-fix path and no concurrent writer.** It does not independently reproduce the required before/after lock evidence. `frontend/src-tauri/tests/document_count_lock_hold.rs:236-270`.
4. **Should-fix - the no-`dbstat` fallback is not demonstrated conservative.** Fixed row allowances omit metadata and index-key bytes despite feeding a block-only gate. `frontend/src-tauri/src/database/repositories/retrieval.rs:1615-1649`.

**Verification:** `cargo test --lib` passed (569 passed, 2 ignored); `cargo check`, `cargo fmt --check`, and scoped `git diff --check` passed. The reviewer did not rerun the gated 250k matrix because its harness is the finding.

**Required follow-ups:** fix GC lag ordering; add production-representation activation-envelope evidence; make before/after lock and concurrent-writer evidence reproducible; establish a demonstrably conservative no-`dbstat` bound.

### Final Code Review (R12)

**Reviewer:** `gpt-5.6-sol` (`ses_fbe5c3d22ffesxdBg925jA9aw9`), 2026-08-27
**Verdict:** Changes requested

**Findings:**
1. **Blocker - the R6 activation-envelope test does not warm the bundled model sessions.** It sets a loaded identity rather than loading the embedding and reranker sessions, so its 574.4 MiB process peak cannot prove the normative combined 1.30 GiB envelope. `frontend/src-tauri/src/retrieval/index.rs:5402-5404,5436-5447`.
2. **Blocker - a meeting deletion can permanently strand a retired generation.** The deletion trigger appends a tombstone to retired generations, but no publisher advances their bound; GC correctly refuses the lagging generation and the two-generation limit then prevents another rebuild. `frontend/src-tauri/migrations/20260825000000_add_semantic_retrieval.sql:333-357`; `frontend/src-tauri/src/retrieval/index.rs:986-996`; `frontend/src-tauri/src/database/repositories/retrieval.rs:744-758`.
3. **Should-fix - upsert-shadowed base rows do not count toward compaction.** A large meeting replacement can leave stale base vectors below the 2% compaction threshold indefinitely. `frontend/src-tauri/src/retrieval/index.rs:239-249,1293-1295`.

**Verification:** The reviewer relied on the completed closure suite and reran scoped `git diff --check`, which passed.

**Required follow-ups:** measure production snapshots plus warmed sessions; make retired-generation deletion journals terminal without synthetic acknowledgement; count upsert-shadowed base rows in compaction.

### Final Code Review (R13)

**Reviewer:** `gpt-5.6-sol` (`ses_fbc7096b3ffe7EdD3BUig9xMFw`), 2026-08-27
**Verdict:** Changes requested

**Findings:**
1. **Blocker - R12 relabels a whole-process gate without whole-application calibration.** The unchanged ceiling names Whisper/audio and Tauri/webview but the qualifying test excludes them, so the result is not like-for-like production evidence. `docs/hybrid-rag/architecture.md:1049-1069`; `frontend/src-tauri/src/retrieval/index.rs:5424-5437,5457-5487`.
2. **Should-fix - lazy reranker initialization holds its initialization mutex through inference.** Concurrent requests cannot observe cancellation until the preceding rerank completes. `frontend/src-tauri/src/retrieval/model.rs:308-339,618-656`.
3. **Should-fix - dual failure while cleaning divergent staging can publish stale rows.** Processing continues if pruning and fallback discard both fail. `frontend/src-tauri/src/retrieval/worker.rs:996-1024`; `frontend/src-tauri/src/database/repositories/retrieval.rs:1062-1076`.

**Verification:** Rust library tests passed (575 passed, 2 ignored); Cargo check, rustfmt, and diff check passed. The reviewer accepted orchestrator TypeScript, Vitest, R7, and release-envelope evidence.

**Required follow-ups:** use a like-for-like retrieval-scoped gate or a full-app calibrated budget; release the reranker initialization lock before inference; fail/repair the work item when all divergent-staging cleanup attempts fail.

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
| 2.R12 | Activation envelope | Cut session residency during the activation window and align the RAM gate's measurement scope with its ceiling. Closes the 2.R9 blocker without changing the 1.30 GiB ceiling. | M | `worker-l` | Complete | The 2.R9 benchmark passes against the unchanged ceiling with recorded margin; the reranker still passes load-time validation and Sprint 1 reference parity; gate and ceiling measure the same quantity. | Restore eager reranker loading and the prior gate; activation returns to blocked-only and never deletes data. |
| 2.R13 | Activation envelope | Implement the retrieval-scoped activation RAM gate the Final Code Review (R13) blocker requires: the gate measures only approved retrieval sessions plus active+shadow snapshots and their metadata/overlays, cannot undercount, never blocks on unrelated process RSS, and keeps the unchanged 1.30 GiB ceiling. | M | `worker-l` | 2.R12 | Blocked with proof: no existing public/runtime facility can measure the session term without undercounting (see the 2.R13 execution entry). The R12 whole-process gate remains in force and still passes the release benchmark. | None needed - no production or benchmark diff exists; the R12 gate ships unchanged. |
| 2.R15 | Staging cleanup fail-closed boundary | Abort semantic work and record bounded retry/backoff when divergent-staging pruning plus discard, or unreadable-staging recovery plus discard, both fail. | S | `implementation subagent` | Complete | Cleanup failure leaves prior canonical documents and publication bounds unchanged, performs no embedding or replacement, and schedules retry. | Revert the worker guard and regression; staging remains durable and recoverable. |

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
- Keep the one transaction: paging is a memory bound, not a writer-lock
  release mechanism. Correct the `document_count` recompute in that
  transaction, then measure and record realistic 250k-corpus-scale worst-case
  replacement lock hold time before and after the correction. If the corrected
  measurement exceeds the approved 250 ms pause quantum, record the evidence
  and escalate versioned document sets as a separate scope-change task; do not
  weaken atomic replacement here.
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
- A realistic worst-case replacement at corpus scale records before/after
  write-lock hold time around the `document_count` recompute fix. A concurrent
  primary write remains able to complete after the bounded transaction; if the
  corrected hold time exceeds 250 ms, versioned document sets are proposed as
  a separate, evidence-backed scope change.
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
  switch remains later Sprint 3.4 scope (`architecture.md` "Retrieval Kill
  Switch" requires a Settings surface) and is deliberately not implemented
  here, so a later review does not re-raise it as a Sprint 2 gap.

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

### 2.R12 - Activation envelope remedy: session residency and gate scope [M]

**Closes:** the `2.R9` blocker. **The 1.30 GiB transient ceiling is NOT
changed by this task**, nor is the approved model, chunk, encoding, or backend
contract. The remedy is to stop holding memory Sprint 2 does not need during
the window that peaks, and to make the gate compare like with like.

**Outcome:** the 250k two-snapshot activation peak fits inside the unchanged
1.30 GiB ceiling with recorded margin, and the number the gate samples and the
ceiling it is compared against measure the same thing.

**Measured starting point (from `2.R6` and `2.R9`):**

| Component | MiB | Note |
|---|---|---|
| Snapshot-only activation peak, no sessions (`2.R6`) | 573.3 | passes with 758 MiB spare |
| Warm embedding + reranker sessions (`2.R9`) | 938.1 | working set; 1022.3 peak |
| Two resident snapshots (2 x 183.1) | 366.2 | inside the 573.3 above |
| Combined peak (`2.R9`) | 1482.7 | 151.5 MiB over the ceiling |

Sessions are 63% of the peak. Snapshot overlap - the term the ceiling's
approving decision actually reasons about - is 25%. Work the dominant term.

**Likely touchpoints:**

- `frontend/src-tauri/src/retrieval/model.rs`
- `frontend/src-tauri/src/retrieval/index.rs`
- `frontend/src-tauri/src/retrieval/worker.rs`
- `upstream/docs/hybrid-rag/architecture.md` (stage 3 only)

**Required implementation.** Three stages. Stages 1 and 2 are each gated on
their own measurement; stop at the first stage that clears the ceiling with
margin. Stage 3 is required regardless of where you stop.

*Stage 1 - lazy reranker session.*

- `get_or_load` builds and warms only the embedding engine. The reranker
  engine is built on first rerank request, behind the same cache and the same
  `BundleIdentity`.
- The reranker's load-time contract validation - input/output names, dtypes,
  ranks, label index, score transform - MUST still run when it is built. Defer
  instantiation, never validation.
- Sprint 1's reference-parity gate is unaffected: the CI reference-inference
  test loads the reranker explicitly. No approved contract changes; only the
  moment of instantiation.
- Re-run the `2.R9` benchmark. Record the embedding-only session weight, the
  reranker's own weight when built, and the new combined peak.
- Sprint 2 has no production rerank consumer - the only `rerank_sync` caller
  outside `model.rs` is in the `index.rs` test module - so this removes
  residency that nothing in this sprint uses, and it lowers steady state as
  well as the transient peak.

*Stage 2 - session eviction across the activation window (only if stage 1
leaves the peak at or above the ceiling).*

- **Precondition, measured first:** prove that dropping the session handles
  actually returns resident memory. ORT arena allocators may retain freed
  blocks. Add a bench phase that loads, warms, drops every handle
  (`SESSION_CACHE` entry and the worker's `embedders` map both), and samples
  RSS. If RSS does not fall materially, **stop and report**: this remedy does
  not work on this runtime and the decision returns to the user. Do not
  implement eviction on the assumption that it frees.
- If it does free: release retrieval sessions before the two-snapshot
  activation window and reload after it. Activation runs only once a shadow
  generation is complete, so no embedding work is owed at that moment.
- Never evict while an embedding batch is in flight.
- Semantic queries during the window fall to FTS-only, reported as an explicit
  typed state, never as a failure.
- A reload failure after activation is non-fatal: lexical fallback stands and
  the newly activated generation stays installed.

*Stage 3 - gate scope (required regardless of stage 1/2 outcome).*

- `measure_resident_ram()` samples whole-process RSS;
  `ACTIVATION_RAM_CEILING_BYTES` derives from retrieval-only arithmetic. Those
  are different quantities and the gate currently compares them directly. This
  is the same defect class as the derived-disk gate in `2.R2`.
- `2.R9` recorded that its benchmark peak "excludes Whisper/webview/UI loads
  of a full application run". Production includes them, so the shipped gate is
  strictly more likely to block than the benchmark that calibrated it, for
  reasons unrelated to retrieval.
- Choose one and implement it: either measure retrieval's own residency
  (sessions plus snapshots) against a retrieval-scoped ceiling, or keep
  whole-process RSS and re-derive the ceiling as an explicit whole-process
  budget that names what else may be resident.
- Record the chosen scope in `architecture.md` next to the ceiling, so the
  number and its limit are documented as measuring the same thing.

**Acceptance criteria:**

- The `2.R9` benchmark passes against the unchanged 1,395,864,371-byte
  ceiling, with the margin recorded.
- One table records embedding-only session weight, reranker session weight,
  snapshot-only peak, and combined peak, so a future remeasure has a baseline.
- The reranker still passes its load-time contract validation and Sprint 1
  reference parity when built.
- If stage 2 ran: a recorded before/after RSS measurement proving eviction
  returns memory; semantic queries fall to FTS-only during the window; a
  reload failure leaves lexical fallback and the activated generation intact.
- If stage 2 did not run: the report says stage 1 alone cleared the ceiling
  and by how much.
- The gate's measurement scope and the ceiling's scope agree, and
  `architecture.md` states which was chosen.
- `ACTIVATION_RAM_CEILING_BYTES` is unchanged. Any proposal to change it is a
  separate user decision and is out of scope here.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
$env:MEETLY_RAG_INDEX_BENCH = "1"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests::bench_2r6_production_activation_envelope -- --nocapture
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** the component table, which stage cleared the
ceiling and the resulting margin, the stage 2 eviction measurement if it ran,
and the gate-scope decision with its rationale.

### Deliberately Not In This Remediation

- **Architecture finding 6** (MCP holds the lifecycle but exposes no semantic
  surface). Correct for Sprint 2 scope; Sprints 3-5 consume it.
- **The persisted force-lexical kill switch.** Approved architecture,
  unimplemented, and Sprint 3.4 scope by its own Settings requirement.
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
