# Sprint 3: Broad Hybrid Chat

## Status

Planned, blocked by Sprint 2 approval and completion.

Revised 2026-08-21 after pre-implementation critique: reranking sub-budget and a
mandatory runtime kill switch added. Estimate: 8-12 working days.

## Goal

Replace folder/all-meetings global snippet ranking with meeting-aware hybrid
retrieval. Fast mode must combine FTS and vector recall, aggregate and rerank
meetings, hydrate authoritative summaries/notes/transcript neighborhoods, and
emit only sources retained in the final prompt. This sprint delivers the first
user-visible quality improvement and must solve the reference WhatsApp case
without Deep-mode iteration.

## Architecture Authority

All work follows [`architecture.md`](architecture.md) and the approved active
model, chunker, index backend, and status contracts from Sprints 1-2.

## Scope

### In Scope

- Concrete `RetrievalService` request/result contracts for persisted content.
- Scope-safe lexical and vector candidate generation.
- Original, rewritten, and lexical core-term query variants.
- Reciprocal-rank fusion and stable evidence deduplication.
- Meeting-level aggregation and diversity controls.
- Local cross-encoder reranking.
- Authoritative multi-meeting hydration and context allocation.
- Exact retained-evidence/source parity.
- Fast-mode integration for all and recursive-folder Chat.
- Reference and broader multilingual evaluation regressions.
- Lexical fallback through the same broad Chat call path.

### Out Of Scope

- Deep iterative retrieval.
- Saved single-meeting vector anchors.
- Search snapshot/today semantic retrieval.
- Sidebar, direct Tauri hybrid search/context, or MCP hybrid tools.
- Live-recording vector retrieval.
- Fast/Deep UI selector.

## Current State And Evidence

- `frontend/src-tauri/src/api/chat.rs:465-500` rewrites eligible follow-up
  queries and determines provider context/chunk limits.
- `frontend/src-tauri/src/api/chat.rs:522-588` selects live, saved-meeting, or
  generic scope context.
- `frontend/src-tauri/src/api/chat.rs:1180-1300` contains broad scope search and
  attempt merging that this sprint replaces/delegates.
- `frontend/src-tauri/src/database/repositories/fts.rs:92-322` remains the
  lexical source and authoritative recursive-folder query implementation.
- `frontend/src-tauri/src/export/context.rs:10-208` contains the two existing
  context builders.
- `frontend/src-tauri/src/api/chat.rs:1333-1368` performs final prompt
  budgeting after context assembly.
- Sprint 2 provides active semantic documents, query embeddings, local
  reranking, immutable vector search, and lexical-only availability status.

## Sprint Requirements

- Broad Chat MUST rank meetings before constructing context.
- FTS remains active in every healthy hybrid query.
- Raw BM25 and cosine scores are never added directly.
- Scope filtering occurs before candidates enter fusion/reranking/hydration.
- Dynamic folder/current-meeting membership is revalidated immediately before
  hydration/source publication.
- Long meetings cannot win solely by producing more chunks.
- Semantic candidates are verified against current authoritative content before
  use.
- Hybrid component failure degrades to the strongest available lower stage.
- Context and source retention are one contract.
- The reference case succeeds in Fast mode for folder and all scopes.
- Existing streaming ownership and cancellation remain intact.

## Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 3.1 | Hybrid candidates | Add concrete persisted-scope retrieval requests and scope-safe FTS/vector candidate generation. | L | Pending `worker-l` | Sprint 2 | Tests prove all/folder allow-lists, query variants, cancellation, semantic fallback, and no out-of-scope candidates. | Route broad Chat back to existing `resolve_scope_results`; index remains unused. |
| 3.2 | Ranking | Add RRF, stable dedupe, meeting aggregation/diversity, and local cross-encoder reranking. | L | Pending `worker-l` | 3.1 | Evaluation proves correct meeting ranking and reranker improvement without exact-term regression. | Disable reranking/fusion service and use ordered lexical candidates. |
| 3.3 | Context | Add authoritative multi-meeting hydration, bounded allocation, coverage, and retained-source output. | L | Pending `worker-l` | 3.2 | Reference context contains complete schedule/MPV facts and all sources match retained evidence. | Keep old generic context builder and lexical path. |
| 3.4 | Broad Chat rollout | Integrate Fast hybrid retrieval into all/folder streaming and non-streaming Chat through shared preparation, and ship the mandatory `force_lexical_retrieval` kill switch. | M | Pending `worker-m` | 3.1-3.3 | Product-path tests prove all/folder Fast behavior, lexical fallback, kill-switch behavior, cancellation, and source events. | Enable `force_lexical_retrieval` at runtime; no rebuild or reinstall required. |
| 3.5 | Quality regression | Run/fix multilingual evaluation, context budgets, performance, and Windows native broad-Chat smoke. | M | Pending `worker-m` | 3.4 | Required quality deltas and reference answer facts pass; no context/latency gate regresses. | Test/threshold changes revert independently; production rollback is Task 3.4 flag/path. |

## Dependency Order

`3.1 -> 3.2 -> 3.3 -> 3.4 -> 3.5`

Every task shares retrieval contracts or `api/chat.rs` behavior with the next;
no implementation tasks are safely parallel by default. Tasks 3.1-3.3 are L
and run alone.

## Task Specifications

### 3.1 - Scope-safe hybrid candidate generation [L]

**Outcome:** One shared service returns lexical and semantic evidence candidates
for persisted all/folder scopes under an authoritative scope boundary.

**Likely touchpoints:**

- `frontend/src-tauri/src/retrieval/mod.rs`
- `frontend/src-tauri/src/retrieval/index.rs`
- `frontend/src-tauri/src/database/repositories/fts.rs`
- `frontend/src-tauri/src/database/repositories/folder.rs`
- Focused retrieval tests/fixtures

**Required implementation:**

- Define concrete `RetrievalRequest`, scope, purpose, limits, and evidence types
  consistent with `architecture.md`.
- Resolve recursive folder IDs/current meeting membership once per request.
- Normalize exactly one tagged scope and reject conflicting explicit scope,
  allowed-ID, or `folder:"..."` combinations.
- Support original and rewritten queries as independent variants.
- Apply the approved lexical core-term policy without changing the answer
  question.
- Run bounded existing FTS retrieval for approved variants/modes.
- Add authoritative current-title lexical candidates; FTS does not index title
  text and title-only search behavior must not depend on semantic availability.
- Embed approved query variants and search the active semantic generation.
- Do not run partial/unavailable semantic state.
- Filter semantic candidates by current allowed meeting IDs before they enter
  downstream ranking.
- Map vector documents to stable source identity/provenance.
- Verify meeting still exists and dirty/stale semantic rows are ineligible.
- Verify the active query snapshot's published journal ID equals canonical
  state; otherwise use bounded catch-up or lexical fallback.
- Accept the Chat cancellation token and check it around every SQL/model/index
  boundary.
- Return channel-ranked candidates without deciding final fused rank.
- Keep live scope outside the service.

**Acceptance criteria:**

- All scope returns current persisted meetings only.
- Folder scope includes selected folder and descendants and excludes every
  meeting outside that subtree even when ANN returns it.
- Stale FTS folder metadata cannot bypass current membership.
- Candidate-time scope tests prove the returned allow-list/filter contract that
  Task 3.3 must revalidate before publication.
- Original/rewritten/core variants remain distinguishable for diagnostics and
  fusion.
- Semantic-unavailable, query-embedding failure, and cancelled paths return the
  documented lexical/error behavior.
- Candidate limits are enforced before unbounded allocations.
- No candidate text/query is logged.
- Existing direct FTS APIs retain their semantics.
- Title-only fixture returns the expected meeting in active and lexical-only
  semantic states.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::fts::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Provide the scope-filter sequence, candidate limits,
fallback matrix, and all callers of moved/replaced broad search helpers.

### 3.2 - Fusion, meeting aggregation, and local reranking [L]

**Outcome:** Candidate channels produce a stable meeting ranking that favors
complete relevant evidence rather than frequent isolated chunks.

**Likely touchpoints:**

- New `frontend/src-tauri/src/retrieval/ranking.rs`
- `frontend/src-tauri/src/retrieval/mod.rs`
- Model/reranker API from Sprint 2
- Evaluation harness and fixtures

**Required implementation:**

- Deduplicate by stable evidence identity.
- Fuse channel ranks with the Sprint 1 approved RRF policy.
- Preserve provenance for lexical, semantic, original, rewritten, and core-term
  channels.
- Aggregate evidence into meetings with capped support contribution.
- Include approved meeting-profile, title-overlap, concept-coverage, and
  diversity terms.
- Do not use recency unless requested by the question/temporal intent.
- Select a bounded evidence set for local cross-encoder reranking, using the
  **candidate depth Sprint 1 derived from the 900 ms p95 sub-budget** — not the
  provisional 30-50 range from `architecture.md`, which predates measurement.
- Implement the Sprint 1 adaptive depth policy if one was approved: rerank a
  reduced head when the fused top-k margin is unambiguous. The policy MUST be
  deterministic and MUST NOT vary by wall-clock timing, which would make
  results irreproducible.
- Apply the shallower `Search` purpose depth where Sprint 1 approved one, so
  sidebar search does not pay Chat-grade reranking cost per keystroke.
- Batch reranker inference under approved limits and cancellation checks.
- Recompute final evidence/meeting order using the approved policy.
- On reranker failure, retain deterministic fused ordering.
- Emit local diagnostics without exposing raw content or treating scores as
  calibrated confidence.

**Acceptance criteria:**

- The reference meeting ranks first in folder and all evaluation cases.
- Similar-topic distractor meetings do not outrank stronger complete evidence.
- Duplicating irrelevant chunks in a long meeting does not raise it above the
  correct meeting.
- Exact names/numbers remain retrievable through FTS even when semantic rank is
  weak.
- RRF results are deterministic for ties.
- Reranking passes the Sprint 1 designated-case and aggregate numeric gates.
- **The reranking stage p95 is at or below 900 ms on reference hardware**,
  measured separately from the rest of Fast preparation and reported as its own
  figure.
- If an adaptive depth policy is used, identical inputs produce identical depth
  and identical ordering across runs.
- Reranker component failure produces deterministic fused fallback; user/stream
  cancellation propagates and produces no answer/source event.
- No constants are tuned solely to one evaluation case.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::ranking::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Record the final formula/constants with Sprint 1
evidence, reranker batch size, tie-breaking, and rejected ranking alternatives.

### 3.3 - Authoritative multi-meeting hydration [L]

**Outcome:** Top meetings contribute complete, current, budgeted evidence rather
than only stored vector/FTS snippets.

**Likely touchpoints:**

- `frontend/src-tauri/src/retrieval/mod.rs`
- `frontend/src-tauri/src/api/chat.rs`
- `frontend/src-tauri/src/export/context.rs`
- Existing meeting/summary/notes/transcript repositories or focused SQL helpers

**Required implementation:**

- Extract reusable authoritative saved-meeting loading from
  `resolve_meeting_context` without regressing its behavior.
- Hydrate selected meetings from current title, latest non-empty summary,
  current notes, matched summary/note sections, and current transcript ranges.
- Rehydrate semantic transcript windows to source transcript segments and
  include at most the approved adjacent neighborhood.
- Validate content hashes/source identities and omit stale semantic evidence.
- Revalidate current folder/existence membership after slow ranking and before
  loading or publishing each meeting.
- Allocate a guaranteed minimum to each selected meeting, then distribute the
  remaining context budget by approved relevance.
- Prevent the first meeting's summary/notes from starving all other meetings.
- Preserve enough top-meeting authoritative content to answer complete facts.
- Emit coverage per meeting/source and deterministic ordering.
- Return Markdown plus exact retained evidence/source IDs after the final
  context budget.
- Include summary/notes as sources when retained.
- Preserve Unicode-safe truncation and mandatory coverage notices.

**Acceptance criteria:**

- Reference Fast context includes `1, 3, 7, 10 and 15` and the MPV distinction.
- Context does not claim unsupported `3 and 4 days` as the complete schedule.
- Multi-meeting fixture retains evidence from every required meeting.
- Stale semantic chunk text is never sent when authoritative source changed.
- A delayed test moves a meeting outside the folder after ranking and proves it
  is absent from hydrated context and sources.
- Summary, notes, and transcript evidence each have retained source identity.
- Context stays within provider-derived character budgets including temporal,
  question, and history overhead.
- Sources exactly equal evidence retained in the final prompt.
- Existing single-meeting context tests continue to pass.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib export::context::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Document budget allocation, source hash validation,
meeting order, source-parity boundary, and authoritative query reuse.

### 3.4 - Folder/all Fast Chat integration [M]

**Outcome:** Existing product Chat commands transparently use broad hybrid Fast
retrieval while preserving model configuration, streaming, persistence, and
cancellation behavior.

**Likely touchpoints:**

- `frontend/src-tauri/src/api/chat.rs`
- `frontend/src-tauri/src/lib.rs` state/command construction if needed
- Existing Chat Rust tests
- MCP Chat shared preparation only as an inherited caller; no new MCP tools

**Required implementation:**

- Invoke `RetrievalService` from the shared persisted broad branch of
  `prepare_chat_inputs`.
- Pass original/rewritten query, validated scope, provider limits, and current
  cancellation token.
- Keep temporal context, meeting-list intent, today detection, provider
  configuration, conversation identity, and stream ownership in `chat.rs`.
- Do not duplicate retrieval in streaming/non-streaming/MCP wrappers.
- Emit retained sources before answer generation through existing event and
  persistence contracts.
- Select lexical fallback when semantic generation is unavailable or any
  approved fallback condition occurs.
- Preserve current all/folder conversation scope and folder descendant
  semantics.
- **Implement the persisted `force_lexical_retrieval` kill switch required by
  `architecture.md` "Retrieval Kill Switch".** This is mandatory, not
  conditional. Index pause and rebuild only affect derived state; neither
  returns a user to the previously shipped retrieval behavior, so without this
  the only rollback for a bad result on a real corpus is a reinstall.
  - Persisted setting, readable by the backend on every request.
  - When enabled, every retrieval surface takes the existing lexical fallback
    path. No new code path is introduced.
  - Takes effect on the next request without restart, and does not delete,
    invalidate, or pause the semantic index.
  - Reported in diagnostics as a distinct user-selected reason, never as a
    model or index failure.
  - The Settings control is delivered in Sprint 5.3; this task delivers the
    setting, the backend behavior, and a temporary command to toggle it.

**Acceptance criteria:**

- Streaming and non-streaming all/folder paths use identical prepared evidence.
- MCP Chat inherits shared preparation without a separate implementation.
- Lexical-only state still answers through the existing broad path.
- Cancellation during embedding/reranking/hydration cannot emit into a newer
  stream.
- Explicit user/stream cancellation aborts preparation and cannot fall through
  to lexical answer generation.
- Source events and saved `sources_json` contain only retained evidence.
- Meeting-list and live branches bypass broad hybrid retrieval.
- **With `force_lexical_retrieval` enabled, every scope answers through the
  lexical path, the setting survives restart, disabling it restores hybrid
  behavior on the next request without restart, and the semantic index is
  neither paused nor invalidated by either transition.**
- Diagnostics distinguish user-forced lexical state from semantic failure.
- Existing scope/persistence/live tests remain green.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib mcp::server::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
pnpm --dir "frontend" run typecheck
pnpm --dir "frontend" exec vitest run
git diff --check
```

**Worker report additions:** List all Chat entry points proven to share the new
path and the exact runtime rollback/fallback mechanism.

### 3.5 - Broad quality and native regression [M]

**Outcome:** Broad hybrid Fast behavior is accepted against objective quality,
budget, latency, and native product checks.

**Likely touchpoints:**

- Evaluation fixtures/harness
- Focused retrieval/Chat tests only when a defect is found
- This sprint execution/decision log

**Required work:**

- Run the full approved evaluation suite comparing FTS baseline, vector-only
  diagnostic, hybrid, and hybrid+reranker.
- Verify reference required facts from retained context and final answer.
- Verify Portuguese/English and distractor breakdowns.
- Measure Fast stage p50/p95 and RAM at current and 250k synthetic scale,
  **reporting the reranking stage as its own figure against its 900 ms
  sub-budget**.
- Measure derived disk against its envelope at the same scales.
- Verify local lexical fallback with model/index deliberately unavailable.
- Verify the `force_lexical_retrieval` kill switch end to end on the native
  build, including persistence across restart.
- Perform a Windows installed/native folder and all Chat smoke using a safe
  fixture or approved local database.
- Fix only defects required by already-approved Sprint 3 acceptance. Record
  larger findings as follow-ups.

**Acceptance criteria:**

- Approved Recall/MRR/evidence/fact thresholds pass.
- Every numeric category gate approved in Sprint 1 passes with recorded corpus
  count and measured value.
- Hybrid+reranker does not underperform FTS on exact-number/name cases.
- Reference folder/all answer includes complete required facts.
- Context and source parity is exact.
- Fast retrieval remains within approved latency, RAM, and disk gates, with the
  reranking stage inside its own sub-budget.
- Lexical fallback works with semantic resources unavailable.
- The kill switch works at runtime and persists across restart.
- Windows native smoke proves visible answer/sources and scope isolation.
- Full automated suites pass.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
pnpm --dir "frontend" run typecheck
pnpm --dir "frontend" exec vitest run
git diff --check
```

Record the benchmark and native-smoke procedure/results in the execution entry.

## Sprint Acceptance Criteria

- Fast folder/all Chat retrieves, reranks, and hydrates meetings through one
  shared service.
- The reference case provides complete schedule evidence and answer facts.
- Scope isolation, cancellation, prompt budgets, and source parity pass.
- Hybrid quality beats approved FTS baseline without exact-term regression.
- Semantic failure preserves lexical Chat.
- Existing meeting/snapshot/today/live behavior remains unchanged.
- Full Rust, frontend, format, diff, evaluation, performance, and Windows
  native checks pass.
- Code and architecture reviews approve the first production retrieval path.

## Risks And Mitigations

- **Many similar retention meetings:** meeting aggregation plus local reranking
  and authoritative hydration.
- **Long-meeting dominance:** cap support-count contribution.
- **Score incompatibility:** rank fusion, never raw score arithmetic.
- **Context starvation:** guaranteed per-meeting minimum and top-meeting budget
  tests.
- **Stale semantic content:** content hash/source rehydration check.
- **Stream regression:** shared preparation only; retain existing ownership
  fences.
- **Evaluation overfit:** all categories must pass, not only reference query.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-21 | Broad Chat ranks meetings before hydration. | Solves the demonstrated global snippet failure. | Return top vector chunks directly. | Main agent, pending sprint approval |
| 2026-08-21 | Fast mode must solve the reference case before Deep exists. | Deep cannot mask a weak retrieval foundation. | Depend on iterative LLM search immediately. | Main agent, pending sprint approval |
| 2026-08-21 | Summary/notes sources join transcript sources. | Answers may be grounded by authoritative non-transcript content. | Continue showing transcript-only sources. | Main agent, pending sprint approval |
| 2026-08-21 | The `force_lexical_retrieval` kill switch is mandatory, not conditional. | This sprint replaces the primary Chat retrieval path. Index pause/rebuild affect derived state only, so without it the sole rollback from a bad result on a user's real corpus is a reinstall. | Keep the original "add a feature-disable only if needed" wording. | Main agent, pending sprint approval |
| 2026-08-21 | Reranking depth comes from Sprint 1's measured sub-budget, not the provisional 30-50 range. | The provisional range predates measurement and plausibly consumes the entire Fast budget on CPU. | Use the architecture's provisional range and adjust reactively. | Main agent, pending sprint approval |
| 2026-08-21 | Adaptive reranking depth must be deterministic, never wall-clock driven. | Timing-driven depth makes evaluation results irreproducible and quality gates meaningless. | Allow a time-boxed reranking budget. | Main agent, pending sprint approval |

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

**Required because:** New central retrieval service, ranking algorithm, local
reranker, authoritative multi-meeting hydration, prompt/source contract, and
streaming integration.

**Reviewer:** Pending
**Verdict:** Pending
**Findings:** Pending

## Approval Gates

- Sprint 2 close must be approved first.
- User approval of this PRD is required before Sprint 3 TODO creation.
- Tasks 3.1-3.3 are L and require individual batch approval.
- Ranking constants or model limits that differ from Sprint 1 require a
  documented evidence addendum, not silent tuning.
- Sprint-close approval is required before Sprint 4 begins.
