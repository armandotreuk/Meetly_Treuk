# Sprint 4: Deep Retrieval And Saved Scopes

## Status

Planned, blocked by Sprint 3 approval and completion

## Goal

Add user-selectable Fast/Deep Chat with Deep as the default for new interactive
conversations, then extend the shared hybrid retriever to saved-meeting,
search-snapshot, and today scopes. Deep mode must safely request additional
searches or meeting evidence without widening scope, leaking planner output, or
making Fast retrieval a weak fallback.

## Architecture Authority

All work follows [`architecture.md`](architecture.md) and the reviewed broad
Fast retrieval contracts delivered by Sprint 3.

## Scope

### In Scope

- Request-level `fast` and `deep` Chat retrieval modes.
- Deep default for new interactive Chat UI sessions.
- A visible Fast/Deep selector with accurate latency/provider-use copy.
- Provider-independent, schema-validated iterative retrieval planning.
- At most two additional retrieval rounds under explicit budgets.
- Vector/lexical transcript anchors for ordinary saved-meeting Chat.
- Query-aware hybrid retrieval within frozen search-snapshot membership.
- Query-aware hybrid retrieval within today meeting membership.
- Scope, cancellation, prompt-injection, failure, and source parity hardening.
- Streaming/non-streaming/MCP Chat shared behavior.

### Out Of Scope

- Persisting retrieval mode in conversation/database schema.
- Multiple concurrent streams.
- Deep mode for sidebar search or context-only tools.
- Vector indexing live unsaved transcript state.
- MCP hybrid search/context tools, delivered in Sprint 5.
- Provider-native tool-calling contracts; the planner remains provider-
  independent.

## Current State And Evidence

- `frontend/src/components/ChatPanel/index.tsx:199-368` owns product Chat send,
  stream listeners, current history, and cancellation UI.
- `frontend/src-tauri/src/api/chat.rs:382-612` prepares every Chat request.
- `frontend/src-tauri/src/api/chat.rs:465-494` already performs one bounded
  non-streaming model call for follow-up query rewriting.
- `frontend/src-tauri/src/api/chat.rs:532-572` specializes ordinary saved-
  meeting context.
- `frontend/src-tauri/src/api/chat.rs:1032-1178` implements authoritative saved-
  meeting content, transcript-only lexical anchors, neighborhoods, and no-hit
  fallback.
- `frontend/src-tauri/src/api/chat.rs:1226-1252` currently rehydrates snapshot
  and today IDs through deterministic chunks without query relevance.
- `frontend/src-tauri/src/api/chat.rs:1487-1702` owns one active stream before
  preparation and fences stale events.
- Sprint 3 provides scope-safe Fast retrieval, local reranking, authoritative
  broad hydration, and lexical fallback.

## Sprint Requirements

- Fast remains one retrieval pass with no planner model call.
- Deep is default only for new interactive Chat sessions.
- The mode is request-level; no schema change is needed.
- Planner output is strict internal data and never rendered/persisted as an
  assistant answer.
- Deep can search/open/expand only within the original authorized scope.
- Meeting content is untrusted evidence and cannot issue planner actions.
- Non-cancellation Deep component failure always continues from current Fast
  evidence; user/request cancellation aborts.
- Saved-meeting mandatory summary/notes and no-hit behavior remain intact.
- Snapshot membership stays frozen by meeting IDs.
- Today membership stays date-derived and current.
- Live recording continues through its existing direct context path.

## Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 4.1 | Quality mode | Add Fast/Deep request contract and accessible Chat selector with Deep default. | M | Pending `worker-m` | Sprint 3 | UI/backend tests prove default, selection, request propagation, and no persistence/schema change. | Remove optional mode and default backend to Fast broad behavior. |
| 4.2 | Deep retrieval | Implement bounded structured planner/search/open/expand loop with scope and cancellation enforcement. | L | Pending `worker-l` | 4.1 | Adversarial and functional tests prove iterative recall, max rounds, scope safety, fallback, and hidden planner output. | Disable Deep branch; Fast remains complete. |
| 4.3 | Saved meeting | Replace saved-meeting transcript anchor selection with hybrid retrieval while preserving authoritative context and fallback. | M | Pending `worker-m` | Sprint 3 | Saved-meeting regressions prove summary/notes, neighborhoods, no-hit fallback, coverage, and source parity. | Restore lexical `search_meeting_transcripts`; no data change. |
| 4.4 | Snapshot/today | Make snapshot and today membership query-aware through hybrid retrieval with deterministic Fast broad-summary coverage and bounded Deep actions. | M | Pending `worker-m` | 4.2 | Tests prove frozen/date membership, query relevance, deleted-member tolerance, broad summarization coverage, and bounds. | Restore deterministic `get_by_meeting_ids`. |
| 4.5 | Cross-scope hardening | Complete Fast/Deep integration, deleted-meeting source scrubbing/disclosure, provider/failure/cancellation/source tests, evaluation, and native smoke. | M | Pending `worker-m` | 4.2-4.4 | Full evaluation/native/deletion checks pass with no scope leak, stale deleted source, or source mismatch. | Disable Deep/per-scope hybrid paths; source scrub rollback requires explicit privacy approval. |

## Dependency Order

`4.1 -> 4.2 -> 4.5`

`Sprint 3 -> 4.3 -> 4.5`

`Sprint 3 -> 4.2 -> 4.4 -> 4.5`

Tasks `4.3` and `4.4` are conceptually independent but both are likely to edit
`api/chat.rs` and retrieval scope contracts, so they run sequentially unless a
fresh diff proves disjoint file ownership. Task 4.2 is L and runs alone.

## Task Specifications

### 4.1 - Fast/Deep request contract and UI [M]

**Outcome:** Users can choose retrieval depth, and new interactive Chat sessions
default to Deep without altering conversation identity or persistence.

**Likely touchpoints:**

- `frontend/src/types/index.ts`
- `frontend/src/components/ChatPanel/index.tsx`
- Focused Chat component tests
- `frontend/src-tauri/src/api/chat.rs`
- Tauri command signatures/call sites
- MCP Chat adapter/tests to prove it remains Fast-only

**Required implementation:**

- Add a serialized `fast`/`deep` retrieval-mode contract mirrored in Rust and
  TypeScript.
- Make new interactive Chat panel/session state default to Deep.
- Add an accessible selector in the established Chat visual language.
- Explain that Deep may take longer and make additional requests to the
  configured Chat provider.
- Pass the selected mode through both scoped streaming and supported
  non-streaming Chat commands.
- Add a request ID/backend cancellation token registry and explicit cancel
  command for non-streaming Chat. A newer request or explicit cancel owns the
  terminal result and suppresses stale completion.
- Keep conversation scope/key/history unchanged when mode changes.
- Do not add a database column or alter conversation uniqueness.
- Backend must validate unknown mode values and apply an explicit default.
- MCP Chat remains Fast-only in this release; do not expose Deep mode through
  unauthenticated localhost MCP.
- Live Chat accepts the UI mode but its retrieval remains direct; Deep planner
  integration for live is out of scope and should resolve to existing behavior.

**Acceptance criteria:**

- Opening a new interactive Chat presents Deep as selected.
- Selecting Fast affects the next request without changing/reseting thread.
- Closing/reopening need not remember a manual Fast choice; this is documented.
- Streaming and non-streaming requests carry the same validated enum.
- Non-streaming Deep can be cancelled while queued, planning, retrieving, or
  generating and cannot return a stale final result.
- Unknown backend values fail or use the explicitly documented safe default.
- Selector has accessible name, keyboard behavior, and explanatory text.
- Existing scope switch and stream-isolation tests remain green.
- No migration or persisted conversation format change occurs.

**Required verification:**

```powershell
pnpm --dir "frontend" run typecheck
pnpm --dir "frontend" exec vitest run tests/components/chat-scope.test.tsx
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Record default semantics for interactive,
non-interactive, MCP, and live callers and all changed IPC signatures. Explain
that MCP is intentionally Fast-only.

### 4.2 - Bounded Deep retrieval loop [L]

**Outcome:** Deep mode can identify missing/conflicting evidence and perform
additional scope-safe retrieval before the final answer.

**Likely touchpoints:**

- New `frontend/src-tauri/src/retrieval/agent.rs`
- `frontend/src-tauri/src/retrieval/mod.rs`
- `frontend/src-tauri/src/api/chat.rs`
- Existing shared LLM request/client code
- Retrieval evaluation and adversarial fixtures

**Required implementation:**

- Define a strict versioned planner action schema with `finish`, `search_more`,
  `openMeetingIds`, and `expandEvidenceIds` or the approved equivalent.
- Build a compact planner prompt from question, scope description, meeting
  cards, evidence metadata/text, coverage, and prior actions.
- Mark meeting content as untrusted evidence and separate it from instructions.
- Use provider-independent non-streaming generation rather than provider-native
  tool calling.
- Parse a strict whole-payload JSON object. Any prefix/suffix prose, reasoning
  tags, trailing object, unknown field, or oversized output is malformed and
  triggers Fast-evidence fallback.
- Enforce the architecture limits: at most two rounds/two planner calls, three
  queries per round, 256 Unicode characters per query, five opened meetings per
  round/eight total, ten expanded evidence IDs per round, 24,000 planner-input
  characters, 8 KiB/512 output tokens, 20 seconds per planner call, and 45
  seconds total Deep preparation.
- Deduplicate queries/actions and prevent loops.
- Merge additional evidence through the same fusion/rerank/hydration contracts,
  not by appending arbitrary planner text.
- Pass the streaming ownership token or non-streaming request token through
  planner, retrieval, and final generation.
- On timeout, malformed output, unsupported provider behavior, refusal, empty
  action, or component failure, continue with current Fast evidence.
- User/stream cancellation is typed separately and aborts preparation without
  final answer/source events; it never falls back.
- Never persist or emit planner output as assistant content.
- Begin final existing streaming generation only after Deep preparation ends.

**Acceptance criteria:**

- A fixture where first-pass evidence is incomplete succeeds after one
  additional search.
- A fixture requiring a second meeting open succeeds without scope widening.
- Planner cannot open an existing meeting outside folder/snapshot/today scope.
- Folder/current membership is revalidated after each round and immediately
  before final evidence publication.
- Retrieved evidence is never parsed directly as an action and grants no
  authority. Deterministic fake-planner tests show malicious evidence plus
  valid-but-out-of-scope, unknown-action, unknown-field, and malformed outputs
  cannot bypass schema, numeric, evidence-ID, meeting-ID, or scope allow-lists.
- Duplicate/self-repeating actions stop within max rounds.
- Every numeric planner limit above is tested at boundary and over-boundary.
- Malformed JSON, timeout, and provider error fall back to Fast evidence;
  user/stream cancellation aborts without stale stream events.
- Fast mode makes no planner call.
- Planner requests are observable through privacy-safe stage metrics only.
- Final sources derive from final retained evidence, including new rounds.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::agent::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Include planner schema, strict parse behavior,
provider/model capability matrix, system prompt boundaries, all limits/timeouts,
fallback/cancellation matrix, request counts, and prompt-injection defenses.

### 4.3 - Hybrid saved-meeting transcript anchors [M]

**Outcome:** Saved-meeting Chat gains semantic transcript recall while retaining
the authoritative behavior that currently produces the best answer.

**Likely touchpoints:**

- `frontend/src-tauri/src/api/chat.rs`
- `frontend/src-tauri/src/retrieval/mod.rs`
- `frontend/src-tauri/src/export/context.rs`
- Saved-meeting Chat tests

**Required implementation:**

- Invoke the shared retriever inside a strict one-meeting allow-list for
  transcript anchor candidates.
- Preserve latest non-empty summary and current notes regardless of query
  match.
- Preserve one-segment adjacency, overlap dedupe, stable chronology, Unicode
  budgeting, coverage notice, and retained-source parity.
- Use hybrid anchors to find transcript evidence missed by lexical wording.
- Preserve the successful-zero-hit chronological transcript-head fallback.
- Do not treat embedding/query failure as a successful zero-hit search; use
  lexical fallback and propagate true database errors.
- Keep summary/note candidates from consuming transcript anchor limits.
- Verify semantic source hashes before transcript rehydration.

**Acceptance criteria:**

- Existing authoritative summary/notes tests pass unchanged in intent.
- Paraphrased transcript query retrieves expected evidence.
- Exact transcript query does not regress versus lexical baseline.
- No-hit transcript-head fallback remains bounded and chronological.
- Stale/missing vector evidence falls back without losing authoritative
  summary/notes. This preserves the saved-meeting R10 invariants documented in
  `../sprint-6-1-contextual-chat.md`.
- Transcript sources equal retained transcript segments.
- Meeting scope cannot return another meeting.
- Reference single-meeting answer remains complete.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib export::context::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Compare old/new anchor ordering and document every
preserved saved-meeting invariant from task `6.1.R10` in
`../sprint-6-1-contextual-chat.md`.

### 4.4 - Query-aware snapshot and today retrieval [M]

**Outcome:** Snapshot/today Chat uses current question relevance without
changing its authoritative membership or losing broad-summary capability.

**Likely touchpoints:**

- `frontend/src-tauri/src/api/chat.rs`
- `frontend/src-tauri/src/retrieval/mod.rs`
- Snapshot/today tests
- Existing `FtsRepository::get_by_meeting_ids` remains available for fallback

**Required implementation:**

- Treat stored snapshot meeting IDs and derived today IDs as allow-lists.
- Run hybrid candidate/fusion/reranking only inside that allow-list.
- Tolerate deleted snapshot members as today.
- Preserve the snapshot's stable stored membership; never rerun the original
  sidebar query.
- Make ordinary factual questions query-relevant.
- Detect broad summarize/compare/list intents deterministically in Fast mode
  and reserve bounded per-meeting coverage rather than letting one meeting
  monopolize context. Deep planner signals may refine only after Task 4.2.
- Preserve the existing 100 total-result/meeting snapshot ceiling or replace it
  only with an approved measured context/evidence bound.
- Use lexical/deterministic fallback while semantic generation is unavailable.

**Acceptance criteria:**

- Factual snapshot/today queries rank expected evidence within allowed IDs.
- Deleted snapshot members are skipped without invalidating thread resume.
- Meetings outside the allow-list never appear in candidates, context, or
  sources.
- Broad summarize fixture includes bounded evidence from every required
  meeting rather than only the top lexical/vector meeting.
- Oversized 100-meeting snapshot remains bounded in retrieval and prompt.
- Today membership uses the current local-date rules and corrected list
  intersection from Sprint 1.
- Existing snapshot identity/persistence tests pass.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Document broad-intent policy, allow-list filtering,
deleted-member behavior, and final fan-out bounds.

### 4.5 - Cross-scope Deep/Fast hardening [M]

**Outcome:** Every persisted Chat scope has verified Fast/Deep behavior,
cancellation, sources, failure fallback, and native product integration.

**Likely touchpoints:**

- Retrieval evaluation fixtures/harness
- `frontend/src-tauri/src/api/chat.rs` tests
- `frontend/src-tauri/src/database/repositories/chat.rs`
- `frontend/src-tauri/src/database/repositories/meeting.rs`
- `frontend/tests/components/chat-scope.test.tsx`
- Minimal production corrections required by existing acceptance criteria

**Required work:**

- Test all, folder, saved meeting, snapshot, and today in Fast and Deep.
- Verify live remains direct and scope-safe when Deep is selected in UI.
- Test provider error, planner error, model/index unavailable, cancellation at
  each preparation stage, and replaced stream ownership.
- Verify source serialization/resume after Deep evidence additions.
- On meeting deletion, preserve conversation answer text but scrub every source
  entry for that meeting from meeting-scoped and broad message `sources_json`;
  show orphan-thread disclosure that answer text may retain quoted content.
- Perform source scrubbing in the meeting-deletion transaction before the
  meeting row disappears. Parse source arrays and remove matching `meetingId`
  entries. If malformed legacy source JSON contains the deleted ID, clear that
  message's source payload rather than allowing a copied snippet to survive.
- Keep existing orphan-thread lineage and user/assistant message text.
- Sanitize every source array against current meetings in the same transaction
  as server-side message persistence so delayed save cannot reintroduce a
  deleted snippet.
- Invalidate/cancel an active stream whose prepared evidence contains the
  deleted meeting and recheck source existence before final source/done events.
- Verify history rewriting plus Deep does not lose the original user question.
- Run full evaluation with Fast/Deep answer fact/source metrics.
- Measure Deep request count/latency and enforce approved ceilings.
- Perform a Windows native smoke for mode switching and representative scopes.

**Acceptance criteria:**

- All persisted scopes pass scope isolation in both modes.
- Deep improves designated incomplete-evidence cases and does not reduce Fast
  evidence when its planner fails.
- Live behavior is unchanged and does not enter persisted semantic retrieval.
- Switching mode does not change conversation identity/history.
- Cancellation before final generation emits no stale answer/source event.
- Deleted-meeting source snippets/navigation metadata cannot remain in persisted
  messages or UI, while existing answer text/thread lineage remains available.
- A race test deletes the meeting after source emission but before delayed
  message save and proves no source is re-persisted or emitted as final.
- Required facts and exact retained sources pass evaluation.
- Full Rust/frontend suites and native smoke pass.

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

Record Deep request/latency metrics and native-smoke steps/results.

## Sprint Acceptance Criteria

- New interactive Chat defaults to Deep and exposes Fast.
- Deep is bounded, schema-validated, scope-safe, cancellable, and hidden from
  answer history/UI.
- Saved-meeting hybrid anchors preserve every authoritative-context invariant.
- Snapshot and today scopes become query-aware without membership drift.
- All persisted scopes pass Fast/Deep quality and source evaluation.
- Live scope remains direct and unchanged.
- Full automated/native checks and code/architecture reviews pass.

## Risks And Mitigations

- **Additional provider cost/latency:** explicit UI copy, max rounds, budgets,
  Fast mode, and metrics.
- **Prompt injection:** strict action schema, content delimiters, allow-lists,
  and evidence-is-data system rules.
- **Scope widening:** resolve original allow-list once and validate every action.
- **Planner instability:** provider-independent JSON parser plus Fast fallback.
- **First-token delay:** communicate Deep behavior and preserve Fast.
- **Saved-meeting regression:** reuse authoritative builder and exhaustive task
  `6.1.R10` tests from `../sprint-6-1-contextual-chat.md`.
- **Broad-summary starvation:** explicit bounded per-meeting coverage policy.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-21 | Deep is request-level and not persisted in conversation schema. | Meet the requested mode choice without an unnecessary migration. | Persist mode per thread. | Main agent, pending sprint approval |
| 2026-08-21 | Deep uses provider-independent structured JSON rather than native tool calling. | All configured Chat providers must behave consistently. | Provider-specific tools/function calling. | Main agent, pending sprint approval |
| 2026-08-21 | MCP Chat remains Fast-only even though new interactive conversations default to Deep. | MCP is an unauthenticated local API invocation without the approved Deep cancellation/cost contract. | Optional or default Deep for MCP. | Main agent, pending sprint approval |

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

**Required because:** Additional LLM orchestration, prompt-injection boundary,
provider cost/latency, cancellation, persisted scope authorization, and changes
to all saved Chat retrieval paths.

**Reviewer:** Pending
**Verdict:** Pending
**Findings:** Pending

## Approval Gates

- Sprint 3 close must be approved first.
- User approval of this PRD is required before Sprint 4 TODO creation.
- Task 4.2 is L and requires a single-task batch approval.
- Planner actions/rounds, remote data behavior, or conversation persistence
  changes require explicit scope-change approval.
- Sprint-close approval is required before Sprint 5 begins.
