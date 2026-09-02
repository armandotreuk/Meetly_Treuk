# Sprint 3: Broad Hybrid Chat

## Status

In progress, 2026-08-30. Sprint 2 closed with user approval. This PRD received
mandatory pre-start amendments and user approval. Task 3.1 is complete. Task
3.2 implementation/remediation is complete and awaiting final review; its
release-hardware latency evidence is owned by Task 3.5 and no longer blocks
Task 3.3 code from proceeding after the ranking contract is approved.

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

This gate MUST be re-measured by Task 3.2 and MUST pass before release.
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
- The active **critical Recall@3 = 100%** release gate MUST be re-measured
  before sprint close: every expected meeting in each of the five critical
  cases must have final meeting rank <= 3, with the three named cases reported
  individually. The strict target-over-decoy ordering rule remains a separate
  stronger condition and is not weakened by this phase.
- Task 3.2 remeasures all five critical cases. If reviewed production ranking
  reaches 5/5, Task 3.6 is not required. If a terminological-gap case remains
  the sole attributable miss, Task 3.6 becomes dependency-ready only after the
  user selects an approach. The 5/5 Sprint-close threshold is unchanged.
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
| 3.1 | Hybrid candidates | Add concrete persisted-scope retrieval requests and scope-safe FTS/vector candidate generation. | L | `worker-l` (complete) | Sprint 2 | Tests prove all/folder allow-lists, query variants, cancellation, semantic fallback, and no out-of-scope candidates. | Route broad Chat back to existing `resolve_scope_results`; index remains unused. |
| 3.2 | Ranking | Add RRF, stable dedupe, meeting aggregation/diversity, and local cross-encoder reranking. Remeasure all five critical cases and own the inherited fusion demotions for `pt-ref-sla-suporte` and `pt-ref-nps-detrator`. | L | Implementation complete; review pending | 3.1 | Production-path evaluation proves correct meeting ranking and reranker improvement without exact-term regression; every critical expected meeting reaches rank <=3 under the active Recall@3 gate, and the strict target-over-decoy ordering rule remains separately enforced; constants use threshold semantics. Release-hardware p95 is Task 3.5 evidence. | Disable reranking/fusion service and use ordered lexical candidates. |
| 3.3 | Context | Add authoritative multi-meeting hydration, bounded allocation, coverage, and retained-source output. | L | Pending `worker-l` | 3.2 | Reference context contains complete schedule/MPV facts and all sources match retained evidence. | Keep old generic context builder and lexical path. |
| 3.4 | Broad Chat rollout | Integrate Fast hybrid retrieval into all/folder streaming and non-streaming Chat through shared preparation, ship the mandatory `force_lexical_retrieval` kill switch, and retire the pre-query GC exception. | M | Pending `worker-m` | 3.1-3.3 | Product-path tests prove all/folder Fast behavior, lexical fallback, kill-switch behavior, cancellation with no final answer/source/done event, source events, and GC requiring an acknowledged clean Fast query. | Enable `force_lexical_retrieval` at runtime; no rebuild or reinstall required. |
| 3.5 | Quality regression | Run/fix multilingual evaluation, final answer/source checks, production-bundle performance, Sprint 2 R13 full-application calibration/refusal evidence, and Windows native broad-Chat smoke/release evidence. | M | Pending `worker-m` | 3.4 | Required quality deltas, answer non-assertion, production-bundle latency, context/source parity, and R13 evidence pass; current-head Windows evidence passes. | Test/threshold changes revert independently; production rollback is Task 3.4 flag/path. |
| 3.6 | Conditional query expansion | Only if reviewed Task 3.2 evidence leaves an attributable terminological-gap miss, add single-turn expansion after the user resolves its architecture question. | M | Conditional; no dispatch while 3.2 is 5/5 | Reviewed 3.2 miss, 3.1-3.3, **user architecture decision** | Terminological-gap cases improve measurably without exact-term, privacy, or Fast-budget regression, and the unchanged 5/5 release gate passes. | Drop the expansion variant; original/rewritten/core variants continue unchanged. |

## Dependency Order

`3.1 -> 3.2 -> 3.3 -> 3.4 -> 3.5`

`reviewed Task 3.2 terminological-gap miss + 3.1-3.3 + user decision -> 3.6`

Every task shares retrieval contracts or `api/chat.rs` behavior with the next;
no implementation tasks are safely parallel by default. Tasks 3.1-3.3 are L
and run alone. Task `3.6` may run after `3.5` closes or alongside it once the
expansion approach is approved. It is omitted when reviewed Task 3.2 evidence
already reaches 5/5. The quality threshold, not implementation of Task 3.6,
blocks Sprint closure.

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
- `frontend/src-tauri/tests/retrieval_evaluation.rs` (production pipeline
  wiring plus the unenforced semantic gate)
- `frontend/src-tauri/tests/fixtures/evaluation_policy.json`
- Production reranker runtime (Task 3.5 owns release-hardware p95 evidence)

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
  Sprint 1 derived and recorded these constants; start from them verbatim:
  chat rerank depth `50` (`floor(900 ms / measured p95)`, capped at the
  `RERANK_SET` ceiling), batch `1`, intra-op `4`, `gamma = 0`,
  `support_cap = 3` (`docs/hybrid-rag/task-1.3-final-selection.md:136-158`,
  `docs/hybrid-rag/task-1.3-final-selection.md:293-298`). Any deviation
  requires a documented evidence addendum, not silent tuning.
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
- Apply the active critical Recall@3 gate: every expected meeting in each of
  the five critical cases must reach rank <=3. The strict target-over-decoy
  ordering rule remains a separate, stronger condition; it is not replaced by
  Recall@3. Remeasure and report all five critical cases. A reviewed result
  meeting the active gates makes Task 3.6 unnecessary only when no attributable
  terminological-gap miss remains.
- **Wire the production retrieval + ranking path into
  `tests/retrieval_evaluation.rs` as a second evaluated pipeline alongside the
  pinned `run_current_fts` baseline, and assert `validate_quality_gates`
  against its metrics.** Today those gates are only ever executed against
  `oracle_results` and deliberately mutated copies of it
  (`frontend/src-tauri/tests/retrieval_evaluation.rs:904`,
  `frontend/src-tauri/tests/retrieval_evaluation.rs:2033-2112`), and the
  harness does not import `RetrievalService` at all. Without this wiring every
  ranking acceptance criterion below is unfalsifiable: the suite passes 6/6
  while measuring a hand-built oracle. Keep the existing FTS baseline and its
  `expectedBaseline` snapshot intact for comparison.
- Enforce the `semanticRecallAt3DeltaPoints` gate (currently `10.0`) inside
  `validate_quality_gates`. It is declared in
  `tests/fixtures/evaluation_policy.json` but never checked — only printed as
  a "semantic future gate" report line. This task produces the first fused
  semantic ranking, so the gate goes live here; enforce it, or record an
  explicit deferral decision with its reason and target task.

**Acceptance criteria:**

Every criterion below must be decided by an assertion over the production
retrieval + ranking pipeline. A criterion whose only evidence is the oracle
path, a printed report line, or reviewer judgement does not count as met.

- The reference meeting ranks first in folder and all evaluation cases,
  asserted against the production pipeline's metrics rather than
  `oracle_results`.
- All five critical cases are reported from the production pipeline and every
  expected critical meeting reaches rank <=3 under the active Recall@3 gate.
  The strict target-over-decoy ordering remains a separate stronger assertion;
  meeting the Recall@3 threshold alone does not close it or Task 3.6.
- On every `similar_topic_distractor` case, the expected meeting outranks that
  case's named distractor meeting — asserted per case, not inferred from
  aggregate Recall@1.
- Duplicating irrelevant chunks in a long meeting does not raise it above the
  correct meeting.
- Exact names/numbers remain retrievable through FTS even when semantic rank is
  weak.
- RRF results are deterministic for ties.
- Reranking passes the Sprint 1 designated-case and aggregate numeric gates,
  including the `semanticRecallAt3DeltaPoints` gate or its recorded deferral.
- The ranking stage exposes one production runtime path that Task 3.5 can
  benchmark with the signed bundle. Task 3.2 does not depend on the obsolete
  Sprint 1 multi-candidate staging tree. The unchanged reranking p95 <=900 ms
  release gate remains mandatory and is measured in Task 3.5 from at least 50
  complete warmed depth-50 production-runtime samples; synthetic evaluation
  latency and `solo-pair p95 * 50` are diagnostic only.
- If an adaptive depth policy is used, identical inputs produce identical depth
  and identical ordering across runs.
- Reranker component failure produces deterministic fused fallback; user/stream
  cancellation propagates and produces no answer/source event.
- No constants are tuned solely to one evaluation case, demonstrated by the
  Sprint 1 isolation protocol — retune with the critical/pinned cases held out,
  then report both held-out and full-corpus figures — not by assertion. This
  sits in deliberate tension with the two named Recall@1 cases above; the
  protocol, not judgement, resolves it.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::ranking::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

A `cargo test` filter that matches zero tests exits `0`, so report the observed
test count for every command above; `0 passed` is a failure to report, not a
pass.

**Worker report additions:** Record the final formula/constants with Sprint 1
evidence, reranker batch size, tie-breaking, and rejected ranking alternatives.
Additionally record: the production-pipeline harness wiring and the gate
results it produced (distinct from the retained FTS baseline snapshot); the
held-out and full-corpus figures from the constants-isolation protocol; and either the
`semanticRecallAt3DeltaPoints` result or its recorded deferral.

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
- Remove the Sprint 2 transitional retired-generation GC alternative. Once this
  product path can acknowledge a clean Fast hybrid query, cleanup again requires
  one clean restart plus one acknowledged successful Fast hybrid query; zero
  acknowledged queries can never trigger GC.

**Acceptance criteria:**

- Streaming and non-streaming all/folder paths use identical prepared evidence.
- MCP Chat inherits shared preparation without a separate implementation.
- Lexical-only state still answers through the existing broad path.
- Cancellation during embedding/reranking/hydration cannot emit into a newer
  stream.
- Explicit user/stream cancellation aborts preparation and cannot fall through
  to lexical answer generation or emit a final answer, source, or
  `chat-stream-done` event. Internal cancellation cleanup is not a user-visible
  completion event.
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
- Retired-generation GC cannot run with zero acknowledged successful Fast
  hybrid queries after this integration is active.

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
- Evaluate every answer-stage forbidden fact whose current-authoritative carrier
  is retained. Folder/all generated answers assert zero such facts; report the
  eligible and total denominators, including the pinned WhatsApp case.
- Verify Portuguese/English and distractor breakdowns.
- Measure Fast stage p50/p95 and RAM at current and 250k synthetic scale,
  including a representative run while bounded background indexing is active.
- Measure the reranking stage from the hash-verified production bundle through
  `RetrievalModels::rerank` in a Rust `--release` build. After warm-up, collect
  at least 50 complete depth-50 request samples and report p50/p95/max, pair and
  sample counts, bundle/manifest digest, hardware/OS, ORT provider, and thread
  settings. The stage includes production tokenization, blocking-pool dispatch,
  and ONNX inference; scheduler queue wait remains part of the separate Fast
  preparation figure. Missing/mismatched artifacts or insufficient samples
  fail the evidence run. The Sprint 1 multi-candidate tree and
  `solo-pair p95 * 50` estimate are historical evidence, not prerequisites.
- Measure derived disk against its envelope at the same scales.
- Record whether current Sprint 3 query plans make
  `retrieval_documents_by_meeting_lookup` load-bearing. Retain the applied
  index unless measured evidence and a migration-risk review justify a later
  forward-only removal; do not create a drop migration merely to satisfy a
  calendar deadline.
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
  privacy-safe refusal reason. Refusal rate is diagnostic, not a tolerated
  percentage gate; the run must include at least one eligible admission below
  the ceiling, while threshold/unit tests prove refusal at or above the ceiling
  and when measurement is unavailable. Any ceiling change requires a separate
  user-approved architecture amendment, fresh review, and package evidence; no
  evidence permits a gate bypass.
- Dispatch the root Windows release workflow on the exact final
  workflow/package head. Intermediate runs are diagnostic; only the final-head
  run is closure evidence. Record its URL, both job conclusions, workspace
  Cargo Check exit `0`, helper staging, MSI/NSIS diagnostic outcomes,
  final-gate outcomes, and package artifact IDs. Re-dispatch only when later
  changes affect product code, workflow, gates, or package sources.
- Fix only defects required by already-approved Sprint 3 acceptance. Record
  larger findings as follow-ups.

**Acceptance criteria:**

- Approved Recall/MRR/evidence/fact thresholds pass.
- Every numeric category gate approved in Sprint 1 passes with recorded corpus
  count and measured value.
- Hybrid+reranker does not underperform FTS on exact-number/name cases.
- Reference folder/all answer includes complete required facts.
- Folder/all generated answers assert zero eligible answer-stage forbidden
  facts, with denominators recorded.
- Context and source parity is exact.
- Fast retrieval remains within approved latency, RAM, and disk gates, with the
  production-runtime reranking stage inside its own sub-budget and active
  indexing pressure included in Fast qualification.
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

### 3.6 - Conditional single-turn query expansion [M]

**Outcome:** A query whose vocabulary differs from the content that answers it
can still retrieve that content, through an additional query variant rather
than through corpus authoring or a hand-maintained lexicon.

**Conditional origin.** Sprint 1 Task `1.3G` left `pt-ref-chaves-acesso`
unresolved before production ranking existed: the
question says *trocar as chaves de acesso* while the decision says *rotação
periódica de credenciais*, and no production stage bridges that gap. The
implemented rewrite path at `frontend/src-tauri/src/api/chat.rs:465-494` is
**follow-up-only** — it triggers on conversational history, and the case is
single-turn, so it structurally cannot apply. Sprint 1 patched the fixture's
title so a production channel could discriminate, which closed the
admissibility defect but narrowed what the case tests. Task 3.2 now remeasures
the case through production ranking. This task is needed only if reviewed Task
3.2 evidence still shows an attributable terminological-gap miss; a reviewed
5/5 result closes the release gate without adding expansion.

**Blocking architecture question — user decision required before dispatch.**
The approach is not pre-decided, and the three candidates differ materially:

| Approach | Consequence |
|---|---|
| (a) Hand-authored PT/EN synonym lexicon | Promotes the evaluation harness's `CONCEPT_LEXICON` pattern into production. Sprint 1 explicitly classified that as a non-production proxy; it also cannot generalize to a user's own vocabulary and needs indefinite maintenance. |
| (b) Local LLM expansion via the existing provider path | Reuses infrastructure the app already has, but puts a provider round-trip **inside the retrieval path** against the 2 s Fast budget, and makes retrieval quality depend on provider choice and availability — a material change for a local-first product. `architecture.md` already counts the follow-up rewrite in its worst-case round-trip budget. |
| (c) Pseudo-relevance feedback (Rocchio-style) | Purely local, deterministic, no new dependency — but expands toward the first pass's top results, which for this failure mode are the neighbours that own the surface vocabulary. Query drift is the expected outcome and it may worsen the case it targets. |

Do not begin implementation unless reviewed Task 3.2 evidence first proves the
need, then the user selects an approach and it is recorded in this sprint's
decision log. The user deferred this decision on 2026-08-29. While reviewed
production ranking remains 5/5, Task 3.6 is not dispatchable and is not a
Sprint-close dependency.

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
- The active critical Recall@3 gate is 5/5: every expected critical meeting is
  at rank <=3, including `pt-ref-chaves-acesso`; the strict target-over-decoy
  ordering gate remains separate and stronger. Task 3.6 is required only if
  reviewed production ranking cannot meet the active gates without expansion.
- The R13 full-application calibration/refusal-rate evidence confirms the fixed
  gate or records a separately approved amendment; the gate remains fail-closed.
- Final-head root Windows CI records both green jobs, Cargo Check exit `0` after
  helper staging, MSI/NSIS installed diagnostic success, final gates, URL, and
  package artifact IDs. It is re-dispatched only for later product, workflow,
  gate, or package-source changes.
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
| 2026-08-29 | Approve Sprint 3 start and the first batch, Task 3.1 only. | Task 3.1 is the sole dependency-ready L task; ranking, hydration, rollout, calibration, and expansion remain sequenced or separately gated. | Start multiple tasks, or defer the foundation. | User |
| 2026-08-29 | Task 3.1 code review (R16) finding 14 (incompatible cross-channel `evidence_id` namespaces) is not remediated in `3.1.R1`; the doc's overclaiming contract text is corrected instead. | Semantic documents are 384-token sliding windows that generally span multiple transcript segments, while FTS chunks are per-segment - there is no clean bijection to key shared identity on without the overlap-range fusion Task 3.2 already owns. Building partial fusion inside Task 3.1 would duplicate that work under a narrower, riskier scope. | Attempt a heuristic cross-channel identity match inside Task 3.1. | Main agent |
| 2026-08-29 | Approve Task `3.1.R3`: folder semantic scans materialize at most 20,000 current meeting IDs behind `ScopeFilter::Meetings(Arc<BTreeSet<_>>)`, while `verified_semantic_meetings` applies the authoritative recursive root-folder SQL gate for every folder candidate. Folders above the cap scan a bounded global over-fetch and may return fewer semantic candidates than an exact folder-local top-k, but may never return an out-of-scope candidate. `ResolvedScope` retains only the persisted scope tag. | Review R21 found unbounded request membership allocation after R20's FTS/title fixes. The bounded scan accelerator avoids a migration and per-variant/result cloning; the root SQL gate, not the accelerator, establishes scope correctness. | A versioned folder-scope projection/migration for exact folder-local top-k; disable folder semantic retrieval and use lexical fallback; leave Task 3.1 blocked. | User |
| 2026-08-29 | Amend Task `3.2` acceptance and evaluation checks before dispatch: assert the quality gates against the production retrieval + ranking pipeline, enforce `semanticRecallAt3DeltaPoints`, source the 900 ms reranker p95 from `model_benchmark.rs` rather than the evaluation harness, and make the distractor and constants-tuning criteria falsifiable. | As written the criteria had no executable path: `validate_quality_gates` runs only against `oracle_results` and mutated copies, `retrieval_evaluation.rs` never imports `RetrievalService`, the declared semantic delta gate is printed but never checked, the latency hooks are explicitly observational over in-memory SQLite, and two criteria resolved to reviewer judgement. A worker could have reported 6/6 passing while measuring a hand-built oracle — the same overclaiming shape R20/R21 caught twice. | Dispatch 3.2 against the approved criteria unchanged and rely on R24 to catch unfalsifiable claims; or defer the harness wiring to Task 3.3. | User |
| 2026-08-30 | Replace Task 3.2's obsolete Sprint 1 multi-candidate benchmark prerequisite with Task 3.5 production-bundle release evidence; retain the unchanged 900 ms p95 threshold. | The current signed bundle correctly omits retired/benchmark-only candidate artifacts, so the historical `MEETLY_RAG_MODELS_DIR` harness cannot measure the release package. Task 3.2 gates correctness through `RetrievalService::retrieve_ranked`; Task 3.5 owns a fail-closed release-build benchmark through the hash-verified production runtime with 50 complete warmed depth-50 samples. | Restore retired/f32 artifacts to the package; lower or waive the threshold; use synthetic evaluation timing. | User |
| 2026-08-30 | Make Task 3.6 conditional on reviewed Task 3.2 evidence rather than a standing Sprint-close obligation. | Query expansion is high-risk scope with three materially different architectures. If production ranking already reaches 5/5, expansion is unnecessary. If an attributable terminological-gap miss remains, the user still selects the approach before dispatch. The 5/5 gate itself remains mandatory. | Build expansion regardless; lower the gate; let an implementer choose the approach. | User |
| 2026-08-30 | Move first answer-stage non-assertion and production-bundle reranker latency evidence to Task 3.5; keep source parity split across ranking, hydration, and caller stages. | Ranking cannot test generated answers or final provider-budgeted sources, and historical benchmark layouts do not prove current runtime latency. The owners now match the product boundary each gate actually measures without relaxing safety or quality thresholds. | Treat ranking fixtures as answer/source proof; gate current release on retired artifact layouts. | User |
| 2026-09-01 | Adopt the active critical Recall@3 phase and define feasibility-first selection precisely: full-gate eligibility includes critical/pinned cases, while objective and tie-order use only the tuning partition; retain strict target-over-decoy ordering and v3 release validation. | Reconciles the executable evaluation protocol with the user-approved policy without weakening any full gate or treating v1/v2 as release evidence. | Restore critical Recall@1 wording; select from infeasible candidates; accept v1/v2 as release validation. | User |

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

### 3.1 - Scope-safe hybrid candidate generation

**Status:** Complete
**Owner:** `worker-l` (`ses_fb20bd702ffeBpJKipm14HLKIL`)
**Completed:** 2026-08-29
**Implemented:**
- Added a persisted-scope `RetrievalService` with concrete request, scope,
  limits, candidate, provenance, and typed fallback contracts.
- Added bounded original/rewritten/core-term FTS candidates, authoritative title
  candidates, and pinned-generation semantic candidates, all post-filtered by
  request-start current membership.
- Reused the existing `RetrievalLifecycle` scheduler, interactive inference
  permit, vector-scan permits, cancellation, queue, and model session loader;
  added only the query-side embedder hook and scope/repository access needed to
  use that shared lifecycle.
- R16 corrections: replaced the provisional function-word list with the exact
  evaluated core-term policy (`evaluation_policy.json`, fixed PT/EN lists,
  diacritic folding, all-stopword fallback); connected the request cancellation
  token to the queued interactive permit wait; pinned semantic search to the
  pre-embedding generation/model via `QueryIndexService::search_pinned`
  (typed `GenerationChanged` refusal, never scored against another
  generation/model); degraded semantic-stage candidate-gate/content SQL
  failures to a typed lexical fallback instead of a request error; required
  `'ready'` state and exact revision equality in `verified_semantic_meetings`;
  made the title channel a keyset-paged streaming scan with a bounded top-k
  heap (identical scope safety, overlap score, and deterministic ordering).
- R17 corrections: removed whatlang inference from the retrieval core-term
  path entirely; `RetrievalRequest` now carries an explicit public
  `CoreTermLanguage` (Portuguese/English/Unknown) discriminator mirroring the
  evaluation corpus's explicit language field, and only it selects the closed
  lists (no dependency or auto-detector added). Request cancellation now flows
  into scope normalization and title scanning with checks before and after
  every awaited scope/title SQL read/page and before each lexical FTS
  boundary, while scope/FTS database failures stay request-fatal. Generation
  and model fencing is request-atomic: the first fence failure discards all
  accumulated semantic hits, skips the Fast hybrid query acknowledgement, and
  returns the typed lexical fallback; the active generation is re-fenced
  before accumulated hits are used and after the awaited candidate SQL reads.
  All scope resolves to `ScopeFilter::All` directly (no per-meeting ID
  materialization; current-meeting verification retained). The Fast hybrid
  query counter now increments only at the end of successful semantic
  candidate validation - zero-hit clean completions count under an explicit
  tested rule, and catch-up, fence, and SQL failures never count.
- R18 corrections: semantic candidate generation is all-or-nothing. Any typed
  vector-stage failure discards accumulated hits and returns lexical fallback;
  cancellation is checked before both candidate-validation reads, and the
  active generation is re-fenced after canonical content loads before semantic
  evidence can be recorded or the Fast hybrid counter can advance.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/service.rs`,
  `frontend/src-tauri/src/retrieval/tests.rs`,
  `frontend/src-tauri/src/retrieval/mod.rs`,
  `frontend/src-tauri/src/retrieval/index.rs`,
  `frontend/src-tauri/src/retrieval/worker.rs`,
  `frontend/src-tauri/src/database/repositories/fts.rs`, and
  `frontend/src-tauri/src/database/repositories/retrieval.rs`.
- Approach: resolve one authoritative persisted scope per request, preserve
  channel/variant provenance without fusing ranks, and degrade semantic
  preparation to bounded lexical candidates on typed non-cancellation failures.
**Not implemented:**
- No RRF, aggregation, reranking, hydration/source publication, Chat caller or
  UI integration, kill switch/migration, R13 calibration, query expansion,
  Search purpose wiring, live scope, MCP hybrid tool, dependency, schema, or
  model/ceiling/gate change.
**Why not implemented:**
- Those behaviors belong to Tasks 3.2-3.6. This task supplies candidates only
  and preserves the Sprint 2 whole-process fail-closed R13 authority.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::tests`
  - pass: 71 passed.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::fts::tests`
  - pass: 16 passed.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` (and `--tests`)
  - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` and
  `git diff --check`
  - pass.
- Full `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib`
  - pass: 625 passed, 0 failed.
**Rollback:**
- Revert these Task 3.1 files. Existing broad Chat remains on
  `resolve_scope_results` because this task adds no caller integration.
**Decisions and follow-ups:**
- `RetrievalPurpose::Search` and `Context` are rejected placeholders, not wired
  surfaces; Sprint 3 invokes `Chat` only.
- Core-term list selection uses only the explicit request language (the
  evaluated harness's explicit language field); Task 3.4's caller integration
  must state the language deliberately per request.
- Task 3.2 is now dependency-ready but requires its own L-task batch approval.

### 3.1.R1 - Task 3.1 code review (R16) remediation

**Status:** Complete
**Owner:** Main agent (session `e762b0ec-7ac3-454d-a05c-49ad443b817d`)
**Completed:** 2026-08-29
**Implemented:**
- Closed thirteen of the fourteen Task 3.1 code review (R16) findings below;
  the fourteenth is recorded as a scope decision in the decision log, not an
  omission.
- Fixed `split_folder_operator` (`database/repositories/fts.rs`) to preserve
  query text on both sides of the `folder:"..."` operator instead of
  discarding everything before it, and refactored `parse_query` to call the
  shared helper instead of duplicating the same extraction logic with a
  stale "finds the last pair" comment.
- Added a `strip_residual_folder_operators` pass so a second `folder:"..."`
  occurrence in the remaining text cannot leak into an FTS `MATCH` clause as
  literal search terms on scope branches that never re-parse the operator.
- Fixed `normalize_core_token` to fold uppercase accented Portuguese letters
  (`character.to_ascii_lowercase()` leaves non-ASCII characters untouched, so
  `Água`/`REUNIÃO`-style titles never matched their lowercase query terms in
  the title channel); mirrored the identical fix into the evaluation
  harness's copy of the same function to keep the doc's parity claim true.
- Stripped FTS5 `<mark>`/`</mark>` snippet markup out of lexical evidence
  text so it reads as plain prose like the semantic channel's canonical
  content, instead of leaking HTML tags into whatever later reads
  `RetrievedEvidence.text` (Task 3.2's reranker, Task 3.3's hydration).
- Split `SemanticFallbackReason::GenerationChanged` out of `ModelMismatch`:
  a routine mid-request activation swap and a genuine embedder/index model
  divergence were reported as the same typed reason at all four sites that
  can observe a pinned-generation mismatch, which would have made Sprint
  3.4's kill-switch/observability work unable to tell a benign snapshot
  rotation from an operator-actionable fault.
- Skipped the redundant OR-mode FTS pass when the AND pass already filled the
  per-variant candidate bound, avoiding both the extra query and its
  per-transcript-row snippet-expansion cost.
- Replaced the title channel's unscoped full-`meetings`-table streamed scan
  with a direct `WHERE id IN (...)` read for every bounded scope (meeting,
  folder, allowed-IDs); the paginated streaming scan is now used only for the
  genuinely unbounded `ScopeFilter::All` case.
- Fixed a `CatchUpPending { behind: 0 }` inconsistency on the pre-scan
  staleness check in `index.rs` (every other emission site in the same
  function floors `behind` at 1).
- Fixed `TitleCandidate`'s `PartialEq` to compare the same fields as its
  manual `Ord` (`overlap`, `meeting_id`), closing a latent `BinaryHeap`
  invariant gap that was unreachable today only because `meetings.id` is a
  primary key.
- Gated the title channel off for `CoreTermLanguage::Unknown`: without a
  stopword list, function words scored equally with content words and could
  occupy candidate slots ahead of every semantic hit in the deterministic
  channel ordering.
- Added a test asserting the production stopword consts
  (`PORTUGUESE_HIGH_FREQUENCY`, `ENGLISH_HIGH_FREQUENCY`) match
  `tests/fixtures/evaluation_policy.json` byte-for-byte, so a future policy
  edit that is not mirrored into production fails loudly instead of silently
  invalidating the evaluation gates.
- Added `meeting_scope_excludes_every_other_meeting` and
  `meeting_scope_naming_no_current_meeting_fails_closed`:
  `PersistedRetrievalScope::Meeting` had zero positive coverage before this -
  only one negative (conflicting-scope) assertion touched it anywhere in the
  test file.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/service.rs`,
  `frontend/src-tauri/src/retrieval/tests.rs`,
  `frontend/src-tauri/src/retrieval/index.rs`,
  `frontend/src-tauri/src/database/repositories/fts.rs`,
  `frontend/src-tauri/tests/retrieval_evaluation.rs`.
- Approach: fix each finding at its narrowest correct scope; where a full fix
  belonged to a later task (finding 14), correct the misleading contract text
  instead of building partial cross-task logic.
**Not implemented:**
- Finding 14, cross-channel `evidence_id` unification (lexical/title/semantic
  candidates covering the same source text stay separate identities at this
  stage). Recorded as a decision below and in the decision log above.
**Why not implemented:**
- Semantic documents are 384-token sliding windows that generally span
  multiple transcript segments while FTS chunks are per-segment, so there is
  no clean bijection to key shared identity on without the overlap-range
  fusion Task 3.2 already owns. Corrected `RetrievedEvidence` and
  `record_candidate`'s doc comments, which previously implied cross-channel
  dedup already happens.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::tests`
  - pass: 75 passed (was 71; +4 new tests).
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::fts::tests`
  - pass: 17 passed (was 16; +1 new test).
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` (and `--tests`)
  - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` and
  `git diff --check`
  - pass.
- Full `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib`
  - pass: 630 passed, 0 failed, 2 ignored (was 625 passed, 0 failed).
- Also cleaned a stale `tauri` build-cache directory (`cargo clean -p tauri`)
  left over from an earlier OneDrive-to-`D:\` project relocation, which was
  blocking `cargo check` with a permissions-file read error against the old
  path; unrelated to this remediation but required to compile at all.
**Rollback:**
- Revert the `3.1.R1` commit. Task 3.1's original behavior returns, including
  the fourteen findings; no persisted data, schema, or model contract is
  involved.
**Decisions and follow-ups:**
- Finding 14 is not a Task 3.1 gap; it is recorded as Task 3.2's cross-channel
  fusion responsibility in the decision log above.
- The `GenerationChanged` vs `ModelMismatch` split is a new public
  `SemanticFallbackReason` variant; any Task 3.4 diagnostics/observability
  surface consuming this enum must handle it explicitly, not fall through a
  wildcard arm.

### 3.1.R2 - Bounded recursive-folder retrieval and FTS safeguards

**Status:** Complete
**Owner:** `worker-m` (`ses_fb0e46a8dffeQQ4OIXFMhzP2td`)
**Completed:** 2026-08-29
**Implemented:**
- Replaced recursive folder bind lists in FTS, title, and Chat paths with
  root-scoped recursive SQL; legacy explicit lists use deterministic 400-ID
  chunks and global caps.
- Made direct FTS folder parsing fail closed, preserved repeated-operator text,
  added plain retrieval snippets that preserve literal `<mark>` content, and
  covered title top-k eviction/ties/caps.
- Restored folder allow-list result metadata to the current `meetings` and
  `meeting_folders` values rather than stale FTS metadata.
**Implementation:**
- Files: `frontend/src-tauri/src/database/repositories/folder.rs`,
  `frontend/src-tauri/src/database/repositories/fts.rs`,
  `frontend/src-tauri/src/api/chat.rs`,
  `frontend/src-tauri/src/retrieval/service.rs`, and
  `frontend/src-tauri/src/retrieval/tests.rs`.
- Approach: retain legacy highlighted FTS behavior while sending source-faithful
  plain snippets only through the new retrieval candidate path.
**Not implemented:**
- Bounded semantic folder membership.
**Why not implemented:**
- Review R21 found the remaining unbounded semantic scan membership and it was
  corrected by the approved R3 entry below.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib` - pass:
  655 passed, 2 ignored after R3.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation`
  - pass: 6 passed.
**Rollback:**
- Revert the focused Task 3.1 remediation commit.
**Decisions and follow-ups:**
- Cross-channel evidence identity remains Task 3.2 work; R3 was required before
  Task 3.1 could be accepted.

### 3.1.R3 - Capped folder semantic scan and root SQL gate

**Status:** Complete
**Owner:** `worker-l` (`ses_fb0294810ffeidltRM8z5bY5YT`)
**Completed:** 2026-08-29
**Implemented:**
- Capped the internal folder scan accelerator at 20,000 current meeting IDs and
  shared it as `ScopeFilter::Meetings(Arc<BTreeSet<_>>)`, removing public scope
  membership from `ResolvedScope`.
- Added the authoritative recursive root-folder SQL gate to semantic candidate
  validation for both under-cap and over-cap folder requests.
- Added a >20,000-meeting regression with a higher-ranked out-of-scope document
  that proves root-gated, per-variant-capped semantic output.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/index.rs`,
  `frontend/src-tauri/src/retrieval/service.rs`,
  `frontend/src-tauri/src/retrieval/tests.rs`, and
  `frontend/src-tauri/src/database/repositories/retrieval.rs`.
- Approach: over-cap folders scan the bounded global ceiling and retain only
  root-gated candidates; under-cap folders retain the scan accelerator.
**Not implemented:**
- Exact folder-local semantic top-k above the 20,000-member cap.
**Why not implemented:**
- The user approved bounded global over-fetch for over-cap folders; it may
  return fewer semantic candidates but cannot return an out-of-scope candidate.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::tests`
  - pass: 78 passed.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib` - pass:
  655 passed, 2 ignored.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation`
  - pass: 6 passed.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml" --tests`,
  `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check`, and
  `git diff --check` - pass.
**Rollback:**
- Revert the focused Task 3.1 remediation commit.
**Decisions and follow-ups:**
- Code Review R22 and Architecture Review R23 approved the capped accelerator
  and root-gate trade-off. Task 3.2 still requires separate user approval.

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

### Task 3.1 Code Review (R16)

**Reviewer:** `claude-sonnet-5` (session `e762b0ec-7ac3-454d-a05c-49ad443b817d`), 2026-08-29
**Scope:** Task 3.1's implementation range only:
`frontend/src-tauri/src/retrieval/service.rs`,
`frontend/src-tauri/src/retrieval/tests.rs`,
`frontend/src-tauri/src/retrieval/index.rs`,
`frontend/src-tauri/src/database/repositories/fts.rs`. Not a full-sprint
review; the pending "Architecture Review" below remains required once later
tasks land.
**Verdict:** Changes requested - thirteen findings resolved in `3.1.R1` above,
one resolved by scope decision (deferred to Task 3.2).

**Findings (severity order):**
1. **Should-fix - a `folder:"..."` operator with query text before it
   silently drops that text.** `split_folder_operator` returned only the text
   after the match, so `migration risks folder:"Sales"` ran as an empty
   query with a valid folder scope, and `retrieve` returned `Ok` with zero
   candidates - indistinguishable from "no matches exist."
   `database/repositories/fts.rs:97`.
2. **Should-fix - the title channel silently disagrees with the lexical
   channel on the corpus's primary language.** `normalize_core_token`
   matched only lowercase accented characters after
   `character.to_ascii_lowercase()`, which leaves non-ASCII characters
   untouched, so ordinary Title Case (`Água`, `Ação`) and all-caps
   (`REUNIÃO`) titles never folded and never scored a title-overlap match.
   `retrieval/service.rs:1177`.
3. **Should-fix - lexical evidence text carries FTS5 `<mark>` markup while
   semantic evidence text is plain prose.** The same `RetrievedEvidence.text`
   field meant different things per channel with nothing signaling it; Task
   3.2's reranker would have scored HTML tags as tokens.
   `retrieval/service.rs:1058`.
4. **Should-fix - a second `folder:"..."` operator leaks into the FTS
   `MATCH` clause as literal search terms.** Only the first operator is
   consumed for scope; `search_with_folder_ids`/`search_with_meeting_ids`
   never re-parse the operator, so a residual second occurrence became
   required/optional search terms including the literal word `folder`.
   `retrieval/service.rs:649`.
5. **Should-fix - a benign mid-request activation swap is reported
   identically to a genuine model mismatch.** `SearchFailure::GenerationChanged`
   collapsed into `SemanticFallbackReason::ModelMismatch` at all four
   observation sites, which would have made kill-switch/observability work
   unable to tell a self-healing snapshot rotation from an
   operator-actionable fault. `retrieval/service.rs:887`.
6. Correctness (minor) - `TitleCandidate` derives `PartialEq`/`Eq` over all
   three fields but its manual `Ord` compares only two, breaking the
   `BinaryHeap` equal-ordering invariant (latent; unreachable while
   `meetings.id` stays a primary key). `retrieval/service.rs:1243`.
7. Correctness (minor) - `CoreTermLanguage::Unknown` has no stopword list, so
   the title channel scored function words as real overlap and could rank
   noise ahead of every semantic hit. `retrieval/service.rs:1220`.
8. Correctness (minor) - one `CatchUpPending` emission site returned
   `behind: 0` where the other three in the same function floor it at 1,
   producing a self-contradictory "waited for zero changes" diagnostic.
   `retrieval/index.rs:935`.
9. Efficiency - the OR-mode FTS pass ran unconditionally even when the AND
   pass already filled the per-variant bound, each call paying an extra
   per-transcript-row snippet-expansion cost. `retrieval/service.rs:578`.
10. Efficiency - the title channel streamed the entire `meetings` table for
    every request regardless of scope, discarding out-of-scope rows in Rust
    instead of binding the already-known allow-list into SQL.
    `retrieval/service.rs:693`.
11. Test coverage - `PersistedRetrievalScope::Meeting` had zero positive
    coverage; only one negative (conflicting-scope) assertion touched it
    anywhere in the 2,208-line test file. `retrieval/service.rs:454`.
12. Reuse - the production stopword lists are hand-copied from
    `tests/fixtures/evaluation_policy.json` with nothing asserting they stay
    equal, so a future policy edit could silently decouple production from
    the gates that are supposed to measure it. `retrieval/service.rs:1161`.
13. Reuse - `split_folder_operator` duplicated `parse_query`'s extraction
    logic verbatim, and the two copies' doc comments already disagreed about
    whether the regex finds the first or last match.
    `database/repositories/fts.rs:94`.
14. Correctness (minor, not remediated) - lexical, title, and semantic
    channels mint `evidence_id` in incompatible namespaces, so
    `record_candidate`'s dedupe cannot unify the same source text hit by two
    channels. `retrieval/service.rs:1085`.

**Verification:** `cargo test --lib` was green before this review (625
passed, 2 ignored) and stayed green after remediation (630 passed, 2 ignored,
+5 new tests), plus `cargo fmt --check` and `git diff --check`, so every
finding was a latent defect rather than a broken build.

**Required follow-ups:** all addressed in `3.1.R1`, except finding 14, which
is correctly Task 3.2's cross-channel fusion responsibility and is recorded
in the decision log rather than fixed here.

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
- Tasks 3.1 and 3.2 were individually approved; Task 3.2 source is awaiting
  the user's manual review. The user authorized Task 3.3 as the next Sprint 3
  batch after the 2026-08-30 plan update. Task 3.4 must not integrate a ranking
  contract with unresolved final review findings.
- Task 3.6 remains blocked unless reviewed Task 3.2 evidence leaves an
  attributable terminological-gap miss and the user selects its approach.
- Ranking-constant deviations require a documented, reproducible held-out
  evidence addendum. Model/runtime limits remain separately approved contracts.
- The final workflow/package source requires final-head root Windows evidence
  before Sprint close.
- Sprint-close approval is required before Sprint 4 begins.
