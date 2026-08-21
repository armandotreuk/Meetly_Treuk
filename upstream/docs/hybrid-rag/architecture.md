# Local Hybrid RAG Architecture

## Document Control

| Field | Value |
|---|---|
| Status | Proposed, awaiting approval |
| Date | 2026-08-21 |
| Owner | Main orchestration agent |
| Product | Meetily desktop application |
| Platforms | Windows x64, macOS ARM64, Linux x64 |
| Related records | `docs/sprint-6-1-contextual-chat.md`, sprint files in this directory |

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
| Platform rollout | All current desktop targets | Packaged inference smoke tests are mandatory on all three targets. |
| Initial indexing | Automatic background backfill after launch | Startup and primary writes cannot wait for embeddings. |
| Chat quality mode | User-selectable Fast/Deep, Deep default | Deep adds bounded model calls before final answer streaming. |
| Runtime envelope | Quality-first 1 GiB target with explicit escalation band | Model, active/shadow snapshots, deltas, and sessions are measured together; no silent release above target. |
| Product surfaces | Chat, sidebar search, Tauri context/search APIs, and MCP | External BM25 contracts require additive hybrid APIs rather than silent score changes. |
| Deleted meeting Chat | Keep answer text, scrub deleted-meeting source data | Preserve conversation value without retaining navigable/snippet source copies. |

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
  250,000 documents and stay within the approved retrieval RAM envelope.

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
| `agent.rs` | Deep-mode action schema, validation, bounded loop, and fallback. |

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

Models load directly from Tauri's signed read-only resource directory. Copying
to app data is permitted only if the selected ONNX export requires writable or
co-located external-data files that cannot be loaded from resources. Such a
copy MUST be atomic, versioned, hash-verified, and recoverable.

ORT inference is CPU-only in the initial release. Whisper CUDA, Vulkan, Metal,
and OpenBLAS features do not imply ORT acceleration.

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

CREATE TABLE retrieval_generations (
    generation_id TEXT PRIMARY KEY,
    model_id TEXT NOT NULL
        REFERENCES retrieval_models(model_id),
    state TEXT NOT NULL CHECK (state IN ('building', 'ready', 'active', 'failed', 'retired')),
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

CREATE TABLE retrieval_documents (
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
    dimensions INTEGER NOT NULL,
    vector BLOB NOT NULL,
    source_revision INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (generation_id, document_id),
    CHECK (length(vector) = dimensions * 4)
) WITHOUT ROWID;

CREATE INDEX retrieval_documents_by_meeting
    ON retrieval_documents(generation_id, meeting_id);

CREATE TABLE retrieval_document_staging (
    job_id TEXT NOT NULL,
    generation_id TEXT NOT NULL
        REFERENCES retrieval_generations(generation_id) ON DELETE CASCADE,
    meeting_id TEXT NOT NULL
        REFERENCES meetings(id) ON DELETE CASCADE,
    source_revision INTEGER NOT NULL,
    document_id TEXT NOT NULL,
    payload BLOB NOT NULL,
    PRIMARY KEY (job_id, document_id)
) WITHOUT ROWID;

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
```

Vectors are normalized finite little-endian `f32`. Repository reads validate
dimension, byte length, finiteness, and norm before admitting a vector to an
in-memory index. Malformed derived rows are quarantined/rebuilt, not allowed to
crash application startup.

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

Retain at most two complete generations. The previous generation becomes
eligible for garbage collection only after the new active generation survives
one clean application restart and one successful Fast hybrid query. Cleanup is
idempotent and never removes the active generation, active snapshot, or a
generation with unacknowledged journal changes.

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

### ANN Option

If exact search fails the scale gate, add a pure-Rust HNSW-style index. Do not
add a native extension or external service.

Requirements:

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

## Local Reranking

The cross-encoder receives question/evidence pairs for a bounded top candidate
set. It reranks evidence and contributes to meeting ranking.

Requirements:

- Input truncation follows the reranker's tokenizer and manifest.
- Reranker inference is batched and cancellable between batches.
- Reranker failure preserves fused ordering.
- Search surfaces use local reranking without an LLM call.
- Reranker output is never presented as calibrated confidence unless Sprint 1
  proves calibration.

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

Fast does not call the Chat model before final answer generation.

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
- Maximum 20 seconds per planner call and 45 seconds total Deep preparation.
- Maximum two planner provider calls before final answer generation.
- Every action is schema-validated and allow-listed.
- Meeting text is untrusted data and cannot alter system instructions.
- Scope cannot widen.
- Planner output is internal and is not persisted as an assistant answer.
- Provider timeout, malformed output, refusal, or component failure falls back
  to the current Fast evidence.
- User/stream cancellation is a distinct typed outcome that aborts preparation;
  it never falls back into answer generation.
- Final answer generation continues through the existing streaming boundary.

Interactive Chat accepts a request-level Fast/Deep selection. New
conversations default to Deep. Do not add conversation persistence for the
mode unless a later approved task requires it. MCP Chat is not an interactive
conversation and remains Fast-only in this release. Deep through unauthenticated
localhost MCP is deferred until it has an approved cancellation/cost contract.

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
hybrid Chat preparation. MCP hybrid tools remain local-only and do not expose
raw embeddings.

## Cancellation And Concurrency

- Query embedding, vector search, reranking, hydration, and Deep planning accept
  a request cancellation token.
- User/stream cancellation always propagates as cancellation and emits no final
  answer/source event. It is never a quality fallback.
- Check cancellation before/after every blocking/model/database stage and while
  waiting in a queue.
- Interactive Tauri search/context requests carry a request ID. New sidebar
  requests cancel the older request owned by that sidebar search session. An
  explicit cancel command covers other non-streaming clients that need it.
- Non-streaming Chat requests also carry a request ID registered to a backend
  cancellation token and have an explicit cancel command. Replacement uses
  request ownership checks so a cancelled/older Deep call cannot return stale
  final content.
- MCP hybrid search/context is Fast, strictly bounded, and does not claim
  cancellation in the first release. MCP Chat remains Fast-only. A request is
  terminated by its server-side timeout even if the client disconnects.
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
| Initial backfill incomplete | Use FTS-only retrieval until complete activation. |
| Meeting dirty | Exclude stale semantic rows for that meeting; allow current FTS/hydration. |
| Vector BLOB malformed | Quarantine/requeue affected meeting; continue without that vector. |
| Exact cache unavailable | Rebuild asynchronously; use FTS. |
| ANN sidecar missing/corrupt | Rebuild from SQLite; use exact if available, otherwise FTS. |
| Query embedding error | Continue with FTS and hydration. |
| Reranker error | Continue with fused ordering. |
| Deep planner error | Continue with Fast evidence. |
| User/stream cancellation | Propagate typed cancellation; stop preparation and emit no stale/final answer or source event. |
| Scope validation failure | Fail closed before retrieval. |
| Database/index write contention | Retry derived work; never fail the primary meeting mutation. |

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
- Estimated local index size.

The model is bundled, so there is no download/delete workflow. Rebuild deletes
only derived semantic state, never transcripts, summaries, notes, or FTS.

Chat exposes Fast and Deep. New conversations default to Deep. The UI explains
that Deep may take longer and use additional requests to the configured Chat
provider. No hidden reasoning/planner output is displayed.

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

Sprint 1 does not close until it publishes a numeric gate table approved by the
user. Minimum defaults, which may only be tightened or changed by explicit
approval after baseline evidence, are:

| Gate | Minimum |
|---|---|
| Reference/critical meeting Recall@1 | 100% |
| Scope safety and retained-source precision | 100% |
| Overall meeting Recall@3 | At least 95% |
| Overall meeting Recall@5 | At least 98% |
| Required evidence Recall@10 | At least 90% |
| Exact-term category | No case moves expected meeting below top 3; aggregate Recall@3 is not below FTS baseline |
| Semantic/paraphrase category | At least +10 percentage points Recall@3 over FTS, or at least 95% when baseline is already above 85% |
| Reranker designated cases | Improves pairwise/NDCG metric and causes no critical-case regression |
| Forbidden-fact contamination in critical cases | 0 |
| Retrieval RAM at 250k | `<=1 GiB` automatic pass; `>1 GiB` through `1.25 GiB` requires explicit user risk/quality approval; `>1.25 GiB` fails without a product scope change. Includes active sessions and old/new snapshot overlap. |

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
- Target at most 1 GiB peak retrieval RAM, including vector snapshot, active
  retrieval model sessions, delta/tombstones, reader-held old snapshots, and
  shadow activation overlap. A measured 1-1.25 GiB result blocks release until
  explicitly approved; above 1.25 GiB fails the architecture gate.
- Vector-search stage p95 below 500 ms on reference hardware.
- Fast local retrieval preparation p95 below 2 seconds excluding final LLM
  answer generation.
- App startup is not blocked by backfill.
- Interactive audio capture shows no new drop/overflow warning and no more than
  10% p95 transcription-throughput degradation under the scheduler test.

If no candidate model/backend satisfies these gates, Sprint 1 stops for an
architecture decision. It does not silently lower quality, scale, or platform
support.

## Packaging And Platform Gates

Before Sprint 2, development/CI runners for Windows x64, macOS ARM64, and Linux
x64 must execute reference tokenizer, embedding, and reranker inference for the
selected pair. Sprint 5 additionally proves installed-package inference.

For Sprint 1 on each target:

- CI fetches/verifies model artifacts and runs reference tokenizer, embedding,
  and reranker inference from the staged resource layout.
- A known sentence tokenizes/embeds to the expected dimension and tolerance.
- A known question/evidence pair produces the expected finite reranker order.

For Sprint 5 on each installed package:

- Tauri packages include all model/tokenizer artifacts and licenses.
- Installed application locates the signed resource path and loads both ONNX
  sessions.
- A minimal hybrid query returns the expected fixture.
- Missing/corrupt-resource simulation produces lexical fallback, not startup
  failure.

The current ORT crate requires a newer Rust version than the manifest's stated
MSRV. Sprint 1 must reconcile and test the declared toolchain rather than rely
only on CI's unpinned `stable` behavior.

## Migration And Rollback

Semantic schema changes are additive. Migration creates tables/triggers and
queues backfill; it performs no inference.

Rollback principles:

- Disabling semantic retrieval leaves FTS and primary meeting data usable.
- Derived tables and sidecars can be deleted/rebuilt without data loss.
- A shipped SQL migration is not reversed destructively in-place. The current
  SQLx migrator may reject a database containing migrations unknown to an older
  binary, so binary rollback requires a verified pre-upgrade database backup.
  Do not claim an older application can simply ignore additive tables unless a
  separately approved migrator-policy change and old-binary startup test proves
  it.
- Model upgrade activation retains the old semantic generation until the new
  one is complete and validated.
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
- Sidebar, Tauri, and MCP contracts.

### Performance Tests

- Synthetic 12k, 50k, and 250k corpora.
- Cold/warm index load.
- Exact and candidate ANN backends.
- Reranker batch sizes.
- Backfill throughput and cancellation.
- Query during recording/index pause.

### Packaged Smoke Tests

- Installed application model loading and inference on each supported target.
- Windows CUDA/Vulkan Whisper variants do not change ORT correctness.
- Missing resource and corrupt sidecar fallback.

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

## Review Gates

Every sprint requires code and architecture review.

Architecture reviewers focus on:

- Sprint 1: model license/package contract and benchmark validity.
- Sprint 2: migrations, trigger durability, worker concurrency, generation
  activation, and deletion safety.
- Sprint 3: scope filtering, ranking, hydration, prompt budgets, and source
  parity.
- Sprint 4: planner safety, cancellation, provider behavior, and bounded cost.
- Sprint 5: external contracts, cross-platform packages, scale, recovery, and
  privacy claims.

No sprint closes with unresolved blocker or should-fix findings.

## Decisions Deferred To Measured Gates

These are not open product questions. Sprint 1 must resolve them with recorded
evidence:

- Exact embedding and reranker models/revisions.
- Token-window profile.
- Whether all summary templates improve retrieval enough to index.
- Exact vector scan versus exact plus HNSW.
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

## Decision Log

| Date | Decision | Rationale | Approved by |
|---|---|---|---|
| 2026-08-21 | Store this program under `docs/hybrid-rag/`. | Keep architecture and per-sprint records together. | User |
| 2026-08-21 | Use local bundled embeddings. | Preserve local processing and offline retrieval. | User |
| 2026-08-21 | Optimize for quality without a strict total model-asset limit. | Retrieval quality is the primary product target. | User |
| 2026-08-21 | Include local reranking and iterative LLM retrieval. | Reach beyond one-shot vector retrieval. | User |
| 2026-08-21 | Support up to 250,000 documents with a quality-first 1 GiB target and approval band through 1.25 GiB. | Make the approximate user-selected envelope testable without silently rejecting a material quality win or releasing above target. | Main agent interpretation, pending architecture approval |
| 2026-08-21 | Roll out broad Chat first, then all saved scopes. | Fix the demonstrated gap while preserving staged validation. | User |
| 2026-08-21 | Target all current desktop platforms. | Preserve the supported product footprint. | User |
| 2026-08-21 | Backfill automatically after launch. | Avoid manual setup while keeping startup non-blocking. | User |
| 2026-08-21 | Expose Fast/Deep and default new Chat conversations to Deep. | Make highest quality the normal experience while retaining a faster option. | User |
| 2026-08-21 | Extend all retrieval surfaces. | Keep Chat, sidebar, context APIs, and MCP capabilities aligned. | User |
| 2026-08-21 | Keep answer text but scrub deleted-meeting source snippets/navigation metadata. | Preserve conversation history while reducing retained source copies after deletion. | User |
| 2026-08-21 | Keep MCP Chat Fast-only in the first release. | Avoid iterative provider calls through unauthenticated localhost MCP without cancellation/cost controls. | Main agent, pending architecture approval |
| 2026-08-21 | Use per-generation indexed source revisions plus a canonical/publication journal. | Prevent model upgrades/rebuilds, crashes, and deletions from leaving stale active vectors. | Main agent, pending architecture approval |
| 2026-08-21 | Durably repair FTS before semantic indexing. | FTS cannot be the availability fallback if best-effort refresh failures remain permanent. | Main agent, pending architecture approval |
