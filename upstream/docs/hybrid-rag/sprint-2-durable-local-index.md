# Sprint 2: Durable Local Semantic Index

## Status

Planned, blocked by Sprint 1 approval and completion.

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
| 2.1 | Persistence | Add source revisions, per-generation state, semantic/staging documents, active-generation pointer, publication journal/tombstones, retry state, due-work indexes, triggers, and repository APIs. | M, high risk | Pending `worker-m` | Sprint 1 | Migration/repository tests prove coalescing, generation independence, staged deletion, deletion journal, retry state, revision fencing, encoding-aware vector validation, and no inference in migration. | Prior runtime ignores unused feature paths, but old-binary rollback requires the verified pre-upgrade DB backup. |
| 2.2 | Local models | Load and validate the bundled tokenizer, embedding model, and reranker model through bounded CPU ONNX sessions. | L | Pending `worker-l` | 1.3, 1.5 | Production engine matches reference outputs locally and preserves the Sprint 1 Windows x64 reference gate. | Disable/remove retrieval state registration; FTS unaffected. |
| 2.3 | Semantic documents | Implement authoritative source extraction and deterministic model-token chunking. | M | Pending `worker-m` | 2.1, 2.2 | Golden tests prove stable IDs, transcript ranges, Markdown sections, limits, and Unicode behavior. | Derived chunking module can be removed; no primary data change. |

### Sprint 2B — Runtime

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 2.4 | Index worker | Implement shared lifecycle/scheduler, model-independent FTS repair, and resumable per-generation indexing into canonical SQLite/publication journal. | L | Pending `worker-l` | 2A close | Crash/change/retry/poison/scheduler tests prove stale vectors cannot commit, FTS heals, and work is not lost/starved. | Stop worker and leave derived rows inactive; revision state remains recoverable. |
| 2.5 | Query index and activation | Implement exact/ANN base+delta/tombstone snapshots, journal replay, atomic swaps, complete model activation, disk-envelope reporting, and status API. | L | Pending `worker-l` | 1.4, 2.4 | Nearest-neighbor, scope, journal crash, deletion, activation, lifecycle, corruption, and cancellation tests pass. | Disable query index and use durably repaired FTS; SQLite vectors remain rebuildable. |

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
