# Sprint 4: Deep Retrieval And Saved Scopes

## Status

Planned; implementation may proceed in dependency order after the R40
documentation amendment and current user approval. Sprint 3 release acceptance
remains open and blocks Sprint 4 close and release claims.

Revised 2026-08-21 after pre-implementation critique: Deep preparation progress
contract added, budget reduced from 45 s to 30 s, and the mode selector
disabled in live scope. Estimate: 6-9 working days.

## Goal

Add user-selectable Fast/Deep Chat with Deep as the default for new interactive
conversations, then extend the shared hybrid retriever to saved-meeting,
search-snapshot, and today scopes. Deep mode must safely request additional
searches or meeting evidence without widening scope, leaking planner output, or
making Fast retrieval a weak fallback.

## Architecture Authority

All work follows [`architecture.md`](architecture.md) and the reviewed broad
Fast retrieval contracts delivered by Sprint 3. The reviewed implementation
baseline is commits `62d7730` and `1047367`; this is an implementation
dependency, not Sprint 3 release acceptance.

## Scope

### In Scope

- Request-level `fast` and `deep` Chat retrieval modes.
- Deep default for new interactive Chat UI sessions.
- A visible Fast/Deep selector with accurate latency/provider-use copy,
  disabled in live-recording scope where the mode has no effect.
- Stage-level progress events during Deep preparation.
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

## Sprint 3 Baseline And Release-Gate Inheritance (R40)

After this documentation amendment and the user's current approval, Sprint 4
implementation may start in the dependency order below, beginning with 4.1.
Sprint 3 release acceptance remains open on:

- a valid independently authored Portuguese corpus;
- production-path quality and final provider-answer evidence;
- native Windows/R13 hermetic session evidence;
- exact-head GitHub Actions evidence.

V1-V10 and the currently rejected corpus fixtures/harnesses are not acceptance
evidence. Internal production testing without a corpus is diagnostic only. Task
4.5, Sprint 4 close, Task 5.5, and release close MUST NOT bypass these gates;
no later Fast/Deep result may substitute for them.

## Sprint Requirements

- Fast remains one retrieval pass with no Deep planner call. The existing
  follow-up query rewrite may use the configured Chat provider.
- Deep is default only for new interactive Chat sessions.
- New interactive UI sessions explicitly send `deep`; omitted mode for every
  legacy, non-interactive, or MCP contract resolves to `fast`; explicit unknown
  mode is rejected; live sends `fast` and never enters Deep.
- The mode is request-level; no schema change is needed.
- Planner output is strict internal data and never rendered/persisted as an
  assistant answer.
- Deep can search/open/expand only within the original authorized scope.
- Meeting content is untrusted evidence and cannot issue planner actions.
- Non-cancellation Deep component failure always continues from current Fast
  evidence; user/request cancellation aborts.
- One persisted `force_lexical_retrieval` setting is read at the shared Rust
  preparation/service boundary and governs every initial/additional Deep
  retrieval and every Tauri, MCP, or sidebar hybrid request, preserving the
  typed `ForcedLexical` reason.
- Saved-meeting mandatory summary/notes and no-hit behavior remain intact.
- Snapshot membership stays frozen by meeting IDs.
- Today membership stays date-derived and current.
- Live recording continues through its existing direct context path.

## Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 4.1 | Quality mode | Add Fast/Deep request contract and accessible Chat selector with Deep default. | M | Pending `worker-m` | R40 docs + current user approval; Sprint 3 implementation baseline | UI/backend tests prove explicit mode compatibility, shared ownership/cancellation, default, selection, request propagation, and no persistence/schema change. | Remove optional mode and default backend to Fast broad behavior. |
| 4.2 | Deep retrieval | Implement bounded structured planner/search/open/expand loop with scope and cancellation enforcement. | L | Pending `worker-l` | 4.1 | Adversarial and functional tests prove iterative recall, max rounds, scope safety, fallback, and hidden planner output. | Disable Deep branch; Fast remains complete. |
| 4.3 | Saved meeting | Replace saved-meeting transcript anchor selection with hybrid retrieval while preserving authoritative context and fallback. | M | Pending `worker-m` | Sprint 3 implementation baseline (release acceptance remains open) | Saved-meeting regressions prove summary/notes, neighborhoods, no-hit fallback, coverage, and source parity. | Restore lexical `search_meeting_transcripts`; no data change. |
| 4.4 | Snapshot/today | Make snapshot and today membership query-aware through hybrid retrieval with deterministic Fast broad-summary coverage and bounded Deep actions. | M | Pending `worker-m` | 4.2; Sprint 3 implementation baseline | Tests prove frozen/date membership, query relevance, deleted-member tolerance, broad summarization coverage, and bounds. | Restore deterministic `get_by_meeting_ids`. |
| 4.5 | Cross-scope hardening | Complete Fast/Deep integration, deleted-meeting source scrubbing/disclosure, provider/failure/cancellation/source tests, evaluation, and native smoke. | M | Pending `worker-m` | 4.2-4.4; inherited Sprint 3 release gates | Full evaluation/native/deletion checks pass with no scope leak, stale deleted source, or source mismatch, and every inherited Sprint 3 release gate has valid evidence. | Disable Deep/per-scope hybrid paths; source scrub rollback requires explicit privacy approval. |

## Dependency Order

`R40 docs + current user approval -> 4.1 -> 4.2 -> 4.5`

`Sprint 3 implementation baseline -> 4.3 -> 4.5`

`Sprint 3 implementation baseline -> 4.2 -> 4.4 -> 4.5`

Tasks `4.3` and `4.4` are conceptually independent but both are likely to edit
`api/chat.rs` and retrieval scope contracts, so they run sequentially unless a
fresh diff proves disjoint file ownership. Task 4.2 is L and runs alone.
The Sprint 3 implementation-baseline edges above do not waive Sprint 3 release
acceptance; those gates remain closure/release dependencies.
Every Sprint 4 implementation task remains subject to the R40 amendment and its
existing per-task human approval; 4.1 is the first implementation task.

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
- Make new interactive Chat panel/session state default to Deep and explicitly
  send `deep` for its new-session request; an explicit Fast selection sends
  `fast`.
- Add an accessible selector in the established Chat visual language.
- Explain that Deep may take longer and make additional requests to the
  configured Chat provider.
- Pass the selected mode through both scoped streaming and supported
  non-streaming Chat commands.
- Generalize/reuse the accepted Sprint 3 Rust ownership/cancellation semantics
  as one request mechanism for Chat and sidebar work, keyed so both surfaces
  may coexist. A newer request or explicit cancel owns the terminal result and
  suppresses stale completion; retain the explicit non-streaming Chat cancel
  command through this mechanism and do not add a parallel registry.
- Ensure terminal, error, and timeout paths clean up the mechanism and add
  bounded-registry-lifetime tests.
- Keep conversation scope/key/history unchanged when mode changes.
- Do not add a database column or alter conversation uniqueness.
- Omitted mode for every legacy, non-interactive, or MCP contract resolves to
  `fast`; an explicit unknown mode is rejected.
- MCP Chat remains Fast-only in this release; do not expose Deep mode through
  unauthenticated localhost MCP.
- Add a behavioral MCP regression through the same shared Chat preparation,
  proving that it remains Fast-only rather than only checking serialization.
- **Disable the selector in live-recording scope** with a short explanation
  that live Chat reads the current transcript directly. Live retrieval ignores
  the mode entirely; the live UI sends `fast` and no Deep planner runs, and
  presenting an active "Deep" control that does nothing
  misleads the user in the scope most likely to want extra depth. Follow the
  Mode Applicability By Scope table in `architecture.md`.
- **Emit stage-level Deep preparation progress** through the existing Chat
  event channel, per `architecture.md` "Deep Preparation Progress". Events
  carry stage identity and counts only — never planner output, queries, or
  evidence text — and pass through Chat's current ownership/cancellation
  publication fence. A privacy-safe sink in `retrieval/agent.rs` must not own
  Tauri events, `AppHandle`, or a second event bus.
- Render progress accessibly, replacing the current silent gap between send and
  first token.

**Acceptance criteria:**

- Opening a new interactive Chat presents Deep as selected.
- Selecting Fast affects the next request without changing/reseting thread.
- Closing/reopening need not remember a manual Fast choice; this is documented.
- Streaming and non-streaming requests carry the same validated enum.
- Non-streaming Deep can be cancelled while queued, planning, retrieving, or
  generating and cannot return a stale final result through the shared
  ownership mechanism.
- Omitted legacy/non-interactive/MCP mode is always Fast, explicit unknown mode
  is rejected, and a new interactive request explicitly carries Deep.
- The behavioral MCP regression traverses shared Chat preparation and proves
  omitted MCP mode is Fast-only with no Deep planner call.
- Selector has accessible name, keyboard behavior, and explanatory text.
- **In live-recording scope the selector is disabled and explains why; no
  request carries a Deep mode that live retrieval would ignore.**
- **Deep preparation emits stage-level progress events that reach the UI before
  the first answer token**, and a test asserts no planner text, query text, or
  evidence content appears in any progress payload.
- Progress is announced accessibly without excessive screen-reader chatter.
- Cancellation remains available throughout the progress phase.
- Stale, replaced, and cancelled progress is suppressed; terminal/error/timeout
  cleanup runs, and registry lifetime stays bounded under repeated requests.
- Existing scope switch and stream-isolation tests remain green.
- No migration or persisted conversation format change occurs.

**Required verification:**

```powershell
pnpm --dir "frontend" run typecheck
pnpm --dir "frontend" exec vitest run tests/components/chat-scope.test.tsx
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib mcp::server::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Record default semantics for interactive,
non-interactive, MCP, and live callers and all changed IPC signatures. Explain
that MCP is intentionally Fast-only, identify the shared ownership mechanism,
and report stale-progress and cleanup/lifetime regressions.

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
- Route planner generation through planner-specific bounded-generation options in
  the existing shared LLM client/request builder; do not create a second client
  or fork provider logic into `retrieval/agent.rs`.
- Every provider MUST have a hard response-byte/parser cap, use its output limit
  where supported, and receive a scoped child cancellation token cancelled on
  both the per-call and total-preparation deadline. A provider that cannot
  enforce a required generation or cancellation bound falls back to current
  Fast evidence. Populate and test the
  capability/fallback matrix for OpenAI, Claude, Groq, Ollama, OpenRouter,
  BuiltInAI, and Custom OpenAI.
- Parse a strict whole-payload JSON object. Any prefix/suffix prose, reasoning
  tags, trailing object, unknown field, or oversized output is malformed and
  triggers Fast-evidence fallback.
- Enforce the architecture limits: at most two rounds/two planner calls, three
  queries per round, 256 Unicode characters per query, five opened meetings per
  round/eight total, ten expanded evidence IDs per round, 24,000 planner-input
  characters, 8 KiB/512 output tokens, **15 seconds per planner call, and 30
  seconds total Deep preparation** (reduced from 20/45 because Deep is the
  default path and the budget is time the user spends watching nothing happen).
- Emit a stage-level progress event at each phase boundary: initial retrieval,
  each planner round, each additional search, and handoff to answer generation.
  Publish through Chat's current ownership/cancellation publication fence; the
  agent may receive only a privacy-safe progress sink.
- Count and report **total provider round-trips per Deep turn**, including the
  pre-existing follow-up query-rewrite call at `api/chat.rs:465-494`. The worst
  case is four: rewrite, two planner calls, and final generation. Reporting
  only the planner count understates the cost the user pays.
- Deduplicate queries/actions and prevent loops.
- Merge additional evidence through the same fusion/rerank/hydration contracts,
  not by appending arbitrary planner text.
- Restrict `openMeetingIds` to the bounded candidate/meeting-card IDs supplied to
  the current planner round, and `expandEvidenceIds` to evidence IDs currently
  known and retained by that round. Revalidate authoritative scope after every
  round and before final publication.
- Pass the streaming ownership token or non-streaming request token through
  planner, retrieval, and final generation.
- Read the single persisted `force_lexical_retrieval` setting at the shared Rust
  preparation/service boundary before initial retrieval and honor it through
  every additional round, preserving the typed `ForcedLexical` reason. Do not
  add another setting or diagnostics service.
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
  user/stream cancellation aborts without stale stream events. Unsupported
  provider bounds also fall back to Fast evidence.
- Open-meeting actions outside the current round's supplied candidate/cards and
  evidence expansions outside its known/retained IDs are rejected, including in
  `All`; scope revalidation remains enforced.
- Stale, replaced, and cancelled progress is not published; terminal, error,
  and timeout cleanup completes and the shared registry lifetime remains bounded.
- Fast/Deep and cross-surface tests cover force-lexical enable-next-request,
  restart persistence, and disable-restore behavior with the typed reason.
- Fast mode makes no planner call and emits no preparation progress events.
- Planner requests are observable through privacy-safe stage metrics only.
- **Deep preparation p95 is at or below 30 seconds**, measured and reported.
- **Total provider round-trips per Deep turn are counted and reported**,
  including the query-rewrite call, with the worst case shown to be four.
- Final sources derive from final retained evidence, including new rounds.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::agent::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib summary::llm_client::tests
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
  each preparation stage, and replaced stream ownership through the one shared
  Rust ownership/cancellation mechanism, including stale/replaced/cancelled
  progress and terminal/error/timeout cleanup with bounded registry lifetime.
- Exercise the single persisted `force_lexical_retrieval` setting at the shared
  Rust boundary through initial/additional Deep retrieval and every scope, with
  typed `ForcedLexical` diagnostics.
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
- Evaluate every answer-stage forbidden fact whose current-authoritative carrier
  remains in final context. Generated answers across each persisted scope assert
  zero such facts; report eligible and total denominators rather than treating
  retrieval-only contamination checks as answer evidence.
- Measure Deep request count/latency and enforce approved ceilings, reporting
  total provider round-trips rather than planner calls alone.
- **Produce the evidence the user needs to revisit Deep-as-default at sprint
  close**: measured Deep preparation p50/p95, provider round-trips and their
  cost implication, and the measured quality delta of Deep over Fast on the
  evaluation corpus. `architecture.md` records this as an open user decision;
  this task supplies the numbers, and MUST NOT change the default itself.
- Perform a Windows native smoke for mode switching and representative scopes,
  including the disabled selector in live scope and visible Deep progress.

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
- Generated answers across every persisted scope assert zero eligible
  answer-stage forbidden facts, with denominators recorded.
- Task 4.5 is not complete unless the valid independently authored Portuguese
  corpus, production-path quality and final provider-answer evidence, native
  Windows/R13 hermetic session evidence, and exact-head GitHub Actions evidence
  all exist. V1-V10 and currently rejected corpus fixtures/harnesses do not
  satisfy these gates; corpus-free internal production testing is diagnostic
  only.
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
The verification record MUST identify each inherited Sprint 3 gate separately;
no Fast/Deep result or task-local check may substitute for a missing gate.

## Sprint Acceptance Criteria

- New interactive Chat defaults to Deep and exposes Fast, with the selector
  disabled in live-recording scope.
- Deep preparation shows stage-level progress and stays inside 30 seconds p95.
- Deep is bounded, schema-validated, scope-safe, cancellable, and hidden from
  answer history/UI.
- Measured Deep latency, provider round-trips, and quality delta are recorded
  so the user can settle the Deep-as-default question at sprint close.
- Saved-meeting hybrid anchors preserve every authoritative-context invariant.
- Snapshot and today scopes become query-aware without membership drift.
- All persisted scopes pass Fast/Deep quality and source evaluation.
- Live scope remains direct and unchanged.
- Full automated/native checks and code/architecture reviews pass.
- Sprint 3 release acceptance is still explicitly satisfied by the valid
  independently authored Portuguese corpus, production-path quality and final
  provider-answer evidence, native Windows/R13 hermetic session evidence, and
  exact-head GitHub Actions evidence; without all four, Sprint 4 cannot close.
- V1-V10 and currently rejected corpus fixtures/harnesses are not acceptance
  evidence, and corpus-free internal production testing is diagnostic only.

## Risks And Mitigations

- **Additional provider cost/latency:** explicit UI copy, max rounds, a reduced
  30 s budget, Fast mode, reported round-trip counts, and a recorded open
  decision on whether Deep should remain the default.
- **Silent preparation reads as a hang:** stage-level progress events, since a
  tooltip read before sending does not help during a 30-second wait.
- **Selector that does nothing:** disabled in live scope rather than accepted
  and ignored.
- **Prompt injection:** strict action schema, content delimiters, allow-lists,
  and evidence-is-data system rules.
- **Scope widening:** resolve original allow-list once and validate every action.
- **Planner instability/provider variance:** provider-independent JSON parser,
  shared-client bounded generation, capability/fallback matrix, child deadline
  cancellation, and Fast fallback.
- **Stale or leaked progress:** one Rust ownership/cancellation mechanism,
  Chat publication fence, terminal cleanup, and bounded registry tests; no
  parallel registries or agent-owned Tauri bus.
- **Rollback drift:** the one persisted `force_lexical_retrieval` setting is read
  at the shared Rust boundary for every initial/additional round and surface;
  typed `ForcedLexical` plus enable-next-request/restart/disable-restore tests
  protect the behavior.
- **Authority widening:** planner open IDs are limited to supplied
  candidate/card IDs and evidence expansion to known/retained IDs, with scope
  revalidation after each round and before publication.
- **First-token delay:** communicate Deep behavior and preserve Fast.
- **Saved-meeting regression:** reuse authoritative builder and exhaustive task
  `6.1.R10` tests from `../sprint-6-1-contextual-chat.md`.
- **Broad-summary starvation:** explicit bounded per-meeting coverage policy.
- **Release-gate laundering:** carry Sprint 3's four named gates into Task 4.5
  and sprint close; rejected fixtures and corpus-free diagnostics cannot become
  release evidence.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-21 | Deep is request-level and not persisted in conversation schema. | Meet the requested mode choice without an unnecessary migration. | Persist mode per thread. | Main agent, pending sprint approval |
| 2026-08-21 | Deep uses provider-independent structured JSON rather than native tool calling. | All configured Chat providers must behave consistently. | Provider-specific tools/function calling. | Main agent, pending sprint approval |
| 2026-08-21 | MCP Chat remains Fast-only even though new interactive conversations default to Deep. | MCP is an unauthenticated local API invocation without the approved Deep cancellation/cost contract. | Optional or default Deep for MCP. | Main agent, pending sprint approval |
| 2026-08-21 | Require stage-level Deep progress events and cut the budget from 45 s to 30 s. | Deep is the default and inserts silence into a UI that currently streams almost immediately; static copy does not stop a hang from reading as a hang. | Keep 45 s with explanatory copy only. | Main agent, pending sprint approval |
| 2026-08-21 | Disable the Fast/Deep selector in live-recording scope. | Live retrieval ignores the mode; an active control that does nothing misleads the user in the scope most likely to want depth. | Accept and silently ignore the mode. | Main agent, pending sprint approval |
| 2026-08-21 | Report total provider round-trips per Deep turn, not planner calls alone. | The pre-existing query-rewrite call makes the true worst case four round-trips; reporting two understates user-visible cost. | Report planner calls only. | Main agent, pending sprint approval |
| 2026-08-21 | Task 4.5 supplies evidence for the Deep-as-default question but must not change the default. | Deep-as-default is a recorded user decision; only the user may revise it, and they need measured latency, cost, and quality-delta data to do so. | Change the default based on implementation judgement. | Main agent, **open question for user at sprint close** |
| 2026-09-02 | Permit sequenced Sprint 4 implementation after the R40 documentation amendment and current user approval while retaining Sprint 3's open release gates. | Commits `62d7730` and `1047367` are the reviewed implementation baseline; implementation dependency is not release acceptance. | Require Sprint 3 release closure before any Sprint 4 implementation. | User-authorized R40 |
| 2026-09-02 | Make mode compatibility explicit and use one Rust ownership/cancellation mechanism for Chat/sidebar, with Chat-fenced Deep progress. | New interactive requests send `deep`, omissions are Fast, live is Fast/no-Deep, stale progress is fenced, and no parallel registries or agent-owned event bus are permitted. | Separate non-streaming/sidebar registries or agent-owned progress events. | User-authorized R40 |
| 2026-09-02 | Bound planner generation inside the shared LLM client and enforce candidate/evidence action authority. | Provider capability/fallback handling, hard 512-token/8 KiB caps, deadline cancellation, and per-round supplied-ID bounds prevent cost, parser, and scope failures. | New planner client or broad-scope ID validation only. | User-authorized R40 |
| 2026-09-02 | Carry the single persisted `force_lexical_retrieval` setting through every Deep round and hybrid surface. | Shared-boundary reads, typed `ForcedLexical`, and enable-next-request/restart/disable-restore tests preserve one reversible fallback without a second service. | Per-surface switches or diagnostics services. | User-authorized R40 |

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

- Sprint 3 release acceptance need not precede sequenced Sprint 4 implementation
  after this R40 amendment and current user approval, but remains mandatory for
  Task 4.5 and Sprint 4 close.
- User approval of this PRD is required before Sprint 4 TODO creation.
- Task 4.2 is L and requires a single-task batch approval.
- Planner actions/rounds, remote data behavior, or conversation persistence
  changes require explicit scope-change approval.
- **At sprint close, present the measured Deep latency, provider round-trip,
  and quality-delta evidence and ask the user to confirm or change
  Deep-as-default.** Record the outcome in the `architecture.md` decision log
  either way.
- Task 4.5, Sprint 4 close, Task 5.5, and release close MUST each have valid
  evidence for the independently authored Portuguese corpus, production-path
  quality and final provider-answer evidence, native Windows/R13 hermetic
  session evidence, and exact-head GitHub Actions evidence. V1-V10 and
  currently rejected corpus fixtures/harnesses are not acceptance evidence;
  corpus-free internal production testing is diagnostic only.
- Sprint-close approval is required before Sprint 5 begins.
