# Sprint 3: Broad Hybrid Chat

## Status

Approved for planning, 2026-08-29. Sprint 2 closed with user approval. This PRD
received mandatory pre-start amendments and user approval; no Sprint 3 TODO or
implementation task has started.

Revised 2026-08-21 after pre-implementation critique: reranking sub-budget and
a mandatory runtime kill switch added. Revised 2026-08-29 after architecture
review: Sprint 2 R13 calibration/refusal evidence, current Windows release
evidence, scheduler reuse, hydration consistency, and kill-switch migration
requirements are explicit. Estimate: 8-12 working days.

**Inherited from Sprint 1 (2026-08-24 gate split).** The **Reference/critical
meeting Recall@1 = 100%** gate moved here as a **release gate** — its threshold
is unchanged; only its owning sprint moved, because ordinal position is decided
by fusion, meeting aggregation, and reranking, which this sprint builds. Sprint
1's selected pair leaves it at `2/5`, with the three misses attributed by
measured cause:

| Case | Measured cause | Owner |
|---|---|---|
| `pt-ref-sla-suporte` | raw bi-encoder ranks the target **1**; fused aggregation demotes it to 3 | **Task 3.2** |
| `pt-ref-nps-detrator` | raw bi-encoder ranks the target **1**; fused aggregation demotes it to 2 | **Task 3.2** |
| `pt-ref-chaves-acesso` | terminological gap; raw bi-encoder rank 4 | **Task 3.6** |

This gate MUST be re-measured at Sprint 3 close and MUST pass before release.
It is retained, not waived: Sprint 1 established that all five critical
meetings already land inside the hydration window with 100% required-fact
coverage and zero retrieval-stage contamination, so what remains is ranking
quality produced by this sprint's stages.

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

Sprint 2 accepted the whole-process activation gate unchanged at
`ACTIVATION_RAM_CEILING_BYTES = 1_395_864_371`. Sprint 3 must measure it from a
real fully loaded application session before close, with Whisper, audio, and
WebView resident; record whether evidence confirms the ceiling or requires a
separate user-approved architecture amendment; and report the activation-refusal
rate. Until then no task may relax, widen, bypass, or reinterpret the
whole-process fail-closed gate, or change the approved model, chunking, vector
encoding, or exact-backend contract.

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
- Wiring the future `Search` retrieval purpose or Search-specific reranker
  depths; Sprint 5 owns those surfaces and activation.
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
- Reuse the existing shared `RetrievalLifecycle`, inference and vector-scan
  permits, bounded interactive queue, cancellation, and recording/import
  pressure controls. Broad Chat must not create a second runtime, scheduler, or
  model-session owner.
- The inherited **critical Recall@1 = 100%** release gate MUST be re-measured
  before sprint close, with the three named cases reported individually.
- The inherited `pt-ref-chaves-acesso` debt remains owned by Task 3.6. Task 3.6
  may not delay the 3.4 rollout, but its selected approach and a 5/5 critical
  Recall@1 result are required before Sprint 3 can close.
- Any tuning of fusion, aggregation, or reranking constants in this sprint MUST
  use **threshold semantics**: satisfy every gate threshold first, then
  optimize quality inside the feasible set. Sprint 1's lexicographic
  miss-minimizing objective was proven stricter than the gates themselves — it
  could not trade two semantic misses (28/30, still far above the gate) for
  three critical rank-1 hits, because the leading term dominated regardless of
  whether any gate was actually at risk. Reproducing that objective shape here
  would reproduce the same blind spot on real data.

## Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 3.1 | Hybrid candidates | Add concrete persisted-scope retrieval requests and scope-safe FTS/vector candidate generation. | L | Pending `worker-l` | Sprint 2 | Tests prove all/folder allow-lists, query variants, cancellation, semantic fallback, and no out-of-scope candidates. | Route broad Chat back to existing `resolve_scope_results`; index remains unused. |
| 3.2 | Ranking | Add RRF, stable dedupe, meeting aggregation/diversity, and local cross-encoder reranking. **Owns the inherited critical Recall@1 debt for `pt-ref-sla-suporte` and `pt-ref-nps-detrator`**, whose targets the bi-encoder already ranks first. | L | Pending `worker-l` | 3.1 | Evaluation proves correct meeting ranking and reranker improvement without exact-term regression; both inherited cases reach rank 1; constants tuned with threshold semantics. | Disable reranking/fusion service and use ordered lexical candidates. |
| 3.3 | Context | Add authoritative multi-meeting hydration, bounded allocation, coverage, and retained-source output. | L | Pending `worker-l` | 3.2 | Reference context contains complete schedule/MPV facts and all sources match retained evidence. | Keep old generic context builder and lexical path. |
| 3.4 | Broad Chat rollout | Integrate Fast hybrid retrieval into all/folder streaming and non-streaming Chat through shared preparation, and ship the mandatory `force_lexical_retrieval` kill switch. | M | Pending `worker-m` | 3.1-3.3 | Product-path tests prove all/folder Fast behavior, lexical fallback, kill-switch behavior, cancellation, and source events. | Enable `force_lexical_retrieval` at runtime; no rebuild or reinstall required. |
| 3.5 | Quality regression | Run/fix multilingual evaluation, context budgets, performance, Sprint 2 R13 full-application calibration/refusal evidence, and Windows native broad-Chat smoke/release evidence. | M | Pending `worker-m` | 3.4 | Required quality deltas and reference answer facts pass; no context/latency/R13 gate regresses; current-head Windows evidence passes. | Test/threshold changes revert independently; production rollback is Task 3.4 flag/path. |
| 3.6 | Query expansion | Add single-turn query expansion as an additional query variant, after the user resolves its open architecture question. | M | Pending `worker-l` (decision deferred) | 3.1-3.3, **user architecture decision** | Terminological-gap cases (`pt-ref-chaves-acesso` and its siblings) improve measurably against the Sprint 1 corpus with no exact-term regression or Fast-budget breach; the inherited critical Recall@1 gate reaches 5/5 before Sprint close. | Drop the expansion variant; original/rewritten/core variants continue unchanged. |

## Dependency Order

`3.1 -> 3.2 -> 3.3 -> 3.4 -> 3.5`

`3.1-3.3 -> 3.6` (additionally gated on the user's architecture decision)

Every task shares retrieval contracts or `api/chat.rs` behavior with the next;
no implementation tasks are safely parallel by default. Tasks 3.1-3.3 are L
and run alone. Task `3.6` may run after `3.5` closes or alongside it once the
expansion approach is approved; it must not delay the `3.4` rollout, but it
blocks Sprint closure until the inherited 5/5 Recall@1 gate passes.

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
- Use the existing shared `RetrievalLifecycle` and its inference/scan permits,
  bounded queue, cancellation, and pressure controls; do not instantiate a
  second worker, scheduler, model session, or queue for broad Chat.
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
- Wire only the `Chat` retrieval purpose in this sprint. Preserve future
  `Search` contracts without invoking a Search depth or sidebar path.
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
- Make authoritative membership/content reads consistent with hydration, or
  perform a final membership and source-identity recheck after loading and
  before prompt/source publication. Omit a moved or deleted meeting rather than
  publishing a stale result.
- Allocate a guaranteed minimum to each selected meeting, then distribute the
  remaining context budget by approved relevance.
- Prevent the first meeting's summary/notes from starving all other meetings.
- Preserve enough top-meeting authoritative content to answer complete facts.
- Emit coverage per meeting/source and deterministic ordering.
- Return Markdown plus exact retained evidence/source IDs after the final
  context budget.
- Include summary/notes as sources when retained.
- Keep prior persisted `sources_json` readable while adding optional source
  provenance. New prompts, source events, and persisted sources must omit
  deleted or invalidated content.
- Preserve Unicode-safe truncation and mandatory coverage notices.

**Acceptance criteria:**

- Reference Fast context includes `1, 3, 7, 10 and 15` and the MPV distinction.
- Context does not claim unsupported `3 and 4 days` as the complete schedule.
- Multi-meeting fixture retains evidence from every required meeting.
- Stale semantic chunk text is never sent when authoritative source changed.
- A delayed test moves a meeting outside the folder after ranking and proves it
  is absent from hydrated context and sources.
- Delayed move and delete tests after content loading prove the final recheck
  removes the meeting and every source before publication.
- Summary, notes, and transcript evidence each have retained source identity.
- Context stays within provider-derived character budgets including temporal,
  question, and history overhead.
- Sources exactly equal evidence retained in the final prompt.
- Existing and newly persisted `sources_json` values deserialize, while deleted
  sources are absent from new prompt/event/persistence output.
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
  - Add a forward-only settings migration with default `false`, fresh and
    upgrade-migration regressions, and repository getter/setter APIs.
  - When enabled, every retrieval surface takes the existing lexical fallback
    path. No new code path is introduced.
  - Takes effect on the next request without restart, and does not delete,
    invalidate, or pause the semantic index.
  - Reported in diagnostics as a distinct user-selected reason, never as a
    model or index failure.
  - The Settings control is delivered in Sprint 5.3; this task delivers the
    setting, the backend behavior, and a temporary command to toggle it.
  - Apply the setting in shared preparation so MCP Chat inherits it without a
    separate retrieval implementation.

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
- Fresh and upgraded databases persist the default and toggled setting, and MCP
  Chat receives the same forced-lexical preparation as Tauri Chat.
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
budget, latency, Sprint 2 R13 activation evidence, and native product checks.

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
- On a user Windows x64 machine, use the release application with the approved
  staged retrieval bundle, an explicitly recorded local Whisper loadout, audio,
  and WebView resident. Run against hermetic temporary app-data, model, and
  database roots with a safe fixture, never a production instance or user data.
- Capture the production gate's whole-process current-RSS sample at active-plus-
  shadow activation. Record build/machine metadata, every measured value, the
  fixed ceiling, artifact identity, and the safe audio-residency procedure. If
  the Whisper loadout or equivalent audio residency cannot be proven, report
  the task blocked rather than substituting a partial/retrieval-only run.
- Before execution, predeclare the independent eligible activation-trial count.
  Report admissions, RAM-gate refusals, non-RAM exclusions, and refusal rate as
  RAM-gate refusals divided by eligible activation trials, using the existing
  privacy-safe refusal reason. Any ceiling change requires a separate
  user-approved architecture amendment, fresh review, and package evidence; no
  evidence permits a gate bypass.
- Dispatch the root Windows release workflow on the exact final workflow/package
  head. Record its URL, both job conclusions, workspace Cargo Check exit `0`,
  helper staging, MSI/NSIS diagnostic outcomes, final-gate outcomes, and package
  artifact IDs. Re-dispatch after every later product, workflow, gate, or
  package-source change.
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
- Full-application activation evidence uses a hermetic release session with
  proven Whisper/audio/WebView residency, records active-plus-shadow current RSS
  and every trial's machine/build/artifact metadata, confirms the fixed ceiling
  or records a separately approved amendment, and reports refusal rate with a
  predeclared eligible-trial denominator plus non-RAM exclusions.
- Current-head root Windows CI passes both jobs: Cargo Check reaches exit `0`
  after helper staging, MSI and NSIS installed diagnostics pass, final gates pass,
  and artifact IDs plus the immutable run URL are recorded.
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

Record the benchmark, R13 calibration/refusal procedure and results, exact-head
Windows evidence, and native-smoke procedure/results in the execution entry.

### 3.6 - Single-turn query expansion [M]

**Outcome:** A query whose vocabulary differs from the content that answers it
can still retrieve that content, through an additional query variant rather
than through corpus authoring or a hand-maintained lexicon.

**Origin.** Sprint 1 Task `1.3G` left `pt-ref-chaves-acesso` unresolved: the
question says *trocar as chaves de acesso* while the decision says *rotação
periódica de credenciais*, and no production stage bridges that gap. The
implemented rewrite path at `frontend/src-tauri/src/api/chat.rs:465-494` is
**follow-up-only** — it triggers on conversational history, and the case is
single-turn, so it structurally cannot apply. Sprint 1 patched the fixture's
title so a production channel could discriminate, which closed the
admissibility defect but narrowed what the case tests. This task is the real
remedy, and Sprint 1's record defers that case to it.

**Blocking architecture question — user decision required before dispatch.**
The approach is not pre-decided, and the three candidates differ materially:

| Approach | Consequence |
|---|---|
| (a) Hand-authored PT/EN synonym lexicon | Promotes the evaluation harness's `CONCEPT_LEXICON` pattern into production. Sprint 1 explicitly classified that as a non-production proxy; it also cannot generalize to a user's own vocabulary and needs indefinite maintenance. |
| (b) Local LLM expansion via the existing provider path | Reuses infrastructure the app already has, but puts a provider round-trip **inside the retrieval path** against the 2 s Fast budget, and makes retrieval quality depend on provider choice and availability — a material change for a local-first product. `architecture.md` already counts the follow-up rewrite in its worst-case round-trip budget. |
| (c) Pseudo-relevance feedback (Rocchio-style) | Purely local, deterministic, no new dependency — but expands toward the first pass's top results, which for this failure mode are the neighbours that own the surface vocabulary. Query drift is the expected outcome and it may worsen the case it targets. |

Do not begin implementation until the user selects an approach and it is
recorded in this sprint's decision log. The user deferred this decision on
2026-08-29; Task 3.6 is not dispatchable and Sprint 3 cannot close until a
selection and its acceptance evidence are recorded.

**Required implementation (approach-independent):**

- Expose expansion as an **additional query variant** alongside original,
  rewritten, and core-term variants. Do not replace or mutate the user's
  query, and preserve variant provenance through fusion and diagnostics as
  Task `3.1` already requires.
- Keep the expansion deterministic for evaluation, or record its
  nondeterminism explicitly and pin it in tests.
- Fail open: if expansion is unavailable, errors, or exceeds its budget,
  retrieval proceeds with the existing variants and logs a privacy-safe
  counter — never a raw query.
- Charge the expansion's cost to the query-preparation budget and report it as
  its own stage figure against the Fast budget.
- Respect the kill switch: `force_lexical_retrieval` disables expansion with
  the rest of the semantic path.

**Acceptance criteria:**

- `pt-ref-chaves-acesso` and its terminological-gap siblings measurably
  improve against the Sprint 1 corpus, reported with denominators.
- No exact-term/number/name regression, and no semantic-category regression.
- The Fast p95 budget holds with the expansion stage included and reported
  separately.
- Retrieval still functions with expansion disabled or failing.
- No raw query text reaches logs.
- If approach (b) is selected, the added round-trip is reflected in the
  documented worst-case provider count.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** the selected approach and why it satisfies the
recorded architecture decision; before/after metrics for the terminological-gap
cases with denominators; the measured expansion-stage latency against its
budget; and the failure/fallback behavior actually exercised.

## Sprint Acceptance Criteria

- Fast folder/all Chat retrieves, reranks, and hydrates meetings through one
  shared service.
- The reference case provides complete schedule evidence and answer facts.
- Scope isolation, cancellation, prompt budgets, and source parity pass.
- Hybrid quality beats approved FTS baseline without exact-term regression.
- Semantic failure preserves lexical Chat.
- Existing meeting/snapshot/today/live behavior remains unchanged.
- The inherited critical Recall@1 gate is 5/5, including
  `pt-ref-chaves-acesso`; Task 3.6 may not be waived from Sprint closure.
- The R13 full-application calibration/refusal-rate evidence confirms the fixed
  gate or records a separately approved amendment; the gate remains fail-closed.
- Exact-head root Windows CI records both green jobs, Cargo Check exit `0` after
  helper staging, MSI/NSIS installed diagnostic success, final gates, URL, and
  package artifact IDs; it is re-dispatched after every later product, workflow,
  gate, or package-source change.
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
- **Activation gate always refuses:** measure full application RSS with a
  denominator and retain lexical fallback; do not widen the gate without a
  separately approved architecture amendment.
- **Hydration TOCTOU:** perform a final authoritative membership/source recheck
  after loading and before source publication; test concurrent move/delete.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-21 | Broad Chat ranks meetings before hydration. | Solves the demonstrated global snippet failure. | Return top vector chunks directly. | Main agent, pending sprint approval |
| 2026-08-21 | Fast mode must solve the reference case before Deep exists. | Deep cannot mask a weak retrieval foundation. | Depend on iterative LLM search immediately. | Main agent, pending sprint approval |
| 2026-08-21 | Summary/notes sources join transcript sources. | Answers may be grounded by authoritative non-transcript content. | Continue showing transcript-only sources. | Main agent, pending sprint approval |
| 2026-08-21 | The `force_lexical_retrieval` kill switch is mandatory, not conditional. | This sprint replaces the primary Chat retrieval path. Index pause/rebuild affect derived state only, so without it the sole rollback from a bad result on a user's real corpus is a reinstall. | Keep the original "add a feature-disable only if needed" wording. | Main agent, pending sprint approval |
| 2026-08-21 | Reranking depth comes from Sprint 1's measured sub-budget, not the provisional 30-50 range. | The provisional range predates measurement and plausibly consumes the entire Fast budget on CPU. | Use the architecture's provisional range and adjust reactively. | Main agent, pending sprint approval |
| 2026-08-21 | Adaptive reranking depth must be deterministic, never wall-clock driven. | Timing-driven depth makes evaluation results irreproducible and quality gates meaningless. | Allow a time-boxed reranking budget. | Main agent, pending sprint approval |
| 2026-08-24 | Register Task `3.6` (single-turn query expansion) here rather than in Sprint 1, with its approach left as an open architecture question. | Sprint 1's `pt-ref-chaves-acesso` failure is a genuine vocabulary gap that no current stage bridges — the implemented rewrite is follow-up-only and the case is single-turn. Sprint 3 already carries the query-variant plumbing, so this is the natural home. It was kept out of Sprint 1 because Sprint 1 excludes production retrieval behavior, because building expansion to fix a gate case and then grading models on that case repeats the overfitting problem one stage upstream, and because model selection blocks Tasks `1.4`/`1.5` and all of Sprint 2 — putting a new feature in front of it inverts the dependency. | Build it inside Sprint 1 alongside the final model run; defer it informally without a registered task. | User |
| 2026-08-24 | Inherit the critical Recall@1 = 100% gate from Sprint 1 as a Sprint 3 release gate, with its three misses attributed by measured cause to Tasks `3.2` and `3.6`. | Ordinal position is produced by fusion, aggregation, and reranking — this sprint's stages — not by the embedding pair Sprint 1 selects. The bi-encoder already ranks four of five critical targets first; `sla-suporte` and `nps-detrator` are demoted by fusion (Task `3.2`), while `chaves-acesso` is a genuine vocabulary gap (Task `3.6`). The threshold is unchanged at 100% and the gate must pass before release. | Waive the gate; leave it in Sprint 1 and block model selection indefinitely; assign all three misses to Task `3.6`. | User |
| 2026-08-24 | Require threshold semantics for all constant tuning in this sprint. | Sprint 1's lexicographic miss-minimizing objective proved stricter than the gates it served: it could not trade two semantic misses (leaving 28/30, still far above the gate's floor) for three critical rank-1 hits, because the leading term dominated whether or not any gate was actually at risk. Tuning on real data with the same objective shape would reproduce the blind spot where it is harder to detect. | Reuse Sprint 1's objective shape unchanged. | User |
| 2026-08-24 | Leave the expansion approach undecided and block Task `3.6` dispatch on an explicit user choice. | The three candidates differ materially in architecture: a hand-authored lexicon promotes the non-production `CONCEPT_LEXICON` pattern into the product, LLM expansion places a provider round-trip inside the retrieval path of a local-first product against a 2 s budget, and pseudo-relevance feedback would likely drift toward the very distractors that own the surface vocabulary. Choosing among them is a product decision, not an implementation detail. | Pre-select an approach in the task specification. | User |
| 2026-08-29 | Apply mandatory pre-start architecture amendments. | Carry Sprint 2 R13 calibration/refusal evidence, exact-head Windows evidence, scheduler reuse, hydration consistency, settings migration, and Sprint 3 closure dependencies into executable acceptance criteria. | Add a new runtime, telemetry subsystem, Search implementation, or relax the activation gate. | User |
| 2026-08-29 | Defer the Task `3.6` expansion approach. | None of the three approaches can be selected by implementation inference; retain the 3.4 rollout path while making 3.6 and the inherited 5/5 gate explicit Sprint-close dependencies. | Pre-select a lexicon, provider expansion, or pseudo-relevance feedback. | User |
| 2026-08-29 | Approve this amended Sprint 3 PRD only. | The user approved planning authority but did not authorize Sprint 3 TODO creation or Task 3.1 dispatch. | Approve and dispatch Task 3.1 in the same decision. | User |

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

### Pre-start Architecture Review (R15)

**Reviewer:** `openai/gpt-5.6-sol`, 2026-08-29
**Scope:** Sprint 3 PRD/specifications against closed Sprint 2C authority and
current Windows CI/package evidence.
**Verdict:** Changes requested - required planning amendments adopted above;
final implementation architecture review remains pending.

**Findings:** R13 full-application calibration/refusal evidence, exact-head
Windows evidence, scheduler reuse, kill-switch migration compatibility,
hydration TOCTOU/source compatibility, explicit Chat-only purpose scope, and
Task 3.6's Sprint-close dependency were absent or underspecified. The review
found no authority to relax the R13 gate or select an expansion approach.

**Required follow-ups:** PRD approval is recorded in the decision log. Obtain
separate batch approval before Sprint 3 TODO creation or Task 3.1 dispatch. Do
not dispatch Task 3.6 until the user selects an approach; do not close Sprint 3
before its 5/5 critical Recall@1 evidence and all R13 evidence are recorded.

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

- Sprint 2 close is approved.
- User approval of this PRD is recorded. Separate batch approval is required
  before Sprint 3 TODO creation or Task 3.1 dispatch.
- Tasks 3.1-3.3 are L and require individual batch approval.
- Task 3.6 remains blocked by the deferred user architecture decision and its
  inherited 5/5 Recall@1 gate blocks Sprint close.
- Ranking constants or model limits that differ from Sprint 1 require a
  documented evidence addendum, not silent tuning.
- The final workflow/package source requires exact-head root Windows evidence
  before Sprint close.
- Sprint-close approval is required before Sprint 4 begins.
