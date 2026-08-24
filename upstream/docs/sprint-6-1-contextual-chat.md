# Sprint 6.1: Contextual Chat Everywhere

## Status

Complete

Tasks `6.1.R10` and `6.1.R11` are implemented and code-review approved. The
user confirmed all six interactive Windows/Tauri smoke checks passed and
approved sprint close on 2026-08-22.

## Goal

Let a user ask the existing Chat assistant from the appropriate context: all
meetings, one saved meeting, a folder including all descendants, a frozen set
of sidebar search results, or an active recording. A live-recording thread
must become the saved meeting's thread after a successful stop/save cycle.

## Scope

### In scope

- One application-wide Chat host with context-specific launchers.
- Persisted scopes: all meetings, one meeting, recursive folder, and a frozen
  sidebar search-result set.
- Live-recording scope grounded in the latest native in-memory transcript.
- Scope-aware conversation persistence and live-to-meeting promotion.
- Clear live-source rendering and cloud-provider disclosure before an
  in-progress transcript leaves the device.
- Automated scope, persistence, retrieval, and lifecycle coverage.
- Complete, bounded grounding for saved single-meeting Chat, including the
  current notes, latest non-empty summary, relevant transcript neighborhoods,
  and an explicit no-hit transcript fallback.

### Out of scope

- Recipes, Chat answer actions, AI note editing, transcript redaction, and
  note-to-transcript links.
- Embeddings, semantic/hybrid retrieval, or a retrieval-model change.
- Timestamp seeking, audio playback controls, and per-source live transcript
  navigation.
- Multiple simultaneous Chat streams or exposing live-recording context to
  MCP.
- Embeddings, semantic/hybrid retrieval, global/folder/snapshot retrieval
  changes, transcript seeking, or a new frontend/IPC contract for context
  coverage.

## Current State And Evidence

- `frontend/src/components/ChatPanel/index.tsx:22-25,42` accepts an optional
  `meetingId`, but it is mounted only from meeting details.
- `frontend/src/app/meeting-details/page-content.tsx:395-399` is the current
  Chat mount point; `page.tsx` has no Chat launcher during recording.
- `frontend/src/components/ChatPanel/index.tsx:71-100` resumes a conversation
  using only `meetingId`, which cannot distinguish global, folder, search, and
  live scopes.
- `frontend/src-tauri/src/api/chat.rs:155-307` centralizes model resolution,
  query rewriting, FTS retrieval, source construction, and prompt budgeting.
- `frontend/src-tauri/src/api/chat.rs:142-151,470-477` permits one active
  process-wide stream; multiple independent panels would cancel one another.
- `frontend/src-tauri/src/database/repositories/fts.rs:92-235` supports only
  optional single-meeting filtering; its folder query syntax resolves one
  direct folder by name.
- `frontend/src/hooks/useSidebarTree.ts:42-90` treats descendant folders as
  part of a folder's visible contents.
- `frontend/src/components/Sidebar/SidebarProvider.tsx:233-252` retains FTS
  search results, while `Sidebar/index.tsx:328-351` reduces them to meeting
  IDs for rendering.
- `frontend/src-tauri/src/audio/recording_commands.rs:1029-1041` exposes the
  backend-authoritative active transcript through `get_transcript_history`.
- `frontend/src/contexts/TranscriptContext.tsx:105-139` generates and stores
  a stable active-recording recovery ID; `hooks/useRecordingStop.ts:255-350`
  receives the persisted meeting ID after saving.

## Requirements And Acceptance Criteria

- Each Chat request reads only the currently selected scope.
- Folder scope includes the selected folder and every descendant folder.
- Search scope uses the result set selected at launch, never reruns the raw
  sidebar query to redefine its membership.
- Live Chat snapshots `get_transcript_history` at send time and never waits
  for SQLite/FTS persistence.
- A saved live Chat conversation is atomically promoted to the returned
  meeting ID after `saveMeeting` succeeds. Failed saving or promotion cannot
  discard the live conversation.
- Existing meeting and global conversations remain reachable after migration;
  conversations orphaned by a deleted meeting remain stored but never become a
  global thread.
- Scope changes open/resume the corresponding scope's thread rather than
  mixing history into the prior thread.
- The app keeps its current one-active-stream invariant.
- Live Chat visibly identifies live sources; cloud models require an explicit
  disclosure before sending in-progress transcript content.
- Saved single-meeting Chat reads the latest non-empty summary markdown and
  current notes markdown from their authoritative tables even when neither
  matches the search query. Empty sections are omitted.
- Single-meeting FTS search is transcript-only and preserves the existing
  rewritten-query/original-query `AND -> OR` retry order. Summary and note rows
  cannot consume the transcript hit limit.
- Each transcript hit includes at most one previous and one next segment from
  the same meeting. Overlapping windows are deduplicated and emitted in stable
  chronology using non-null `audio_start_time`, then timestamp and ID as
  deterministic tie-breakers.
- A successful search with zero transcript hits includes a bounded excerpt
  from the beginning of the meeting transcript in stable chronology. An FTS or
  database error remains an error and must not be disguised as a no-hit
  fallback.
- Summary, notes, and transcript content all remain inside the existing
  provider context budget. Any omitted content produces a coverage notice that
  states the transcript is partial and reports included versus total segments
  when known; the assistant is instructed to disclose that limitation.
- Transcript sources correspond only to transcript chunks actually present in the
  final prompt. Existing source serialization and frontend rendering remain
  unchanged.

## Technical Approach

Create a serializable `ChatScope` contract mirrored in TypeScript and Rust.
Persist the authoritative scope kind, key, and data on each conversation;
retain `meeting_id` and `origin` for existing meeting lineage and migration
compatibility. The Chat command receives a conversation ID and resolves its
stored scope server-side, so a renderer cannot inject arbitrary search text as
LLM context.

Add a scope resolver below `prepare_chat_inputs` in `api/chat.rs`:

- All-meetings and saved-meeting scopes reuse current FTS retrieval.
- Folder scope resolves stable descendant folder IDs at request time and uses
  them as an FTS filter, not the name-based `folder:"..."` convenience syntax.
- Search scope stores bounded FTS result identifiers. The backend rehydrates
  those identifiers from local data, preserving result-set membership without
  trusting client-supplied content.
- Live scope reads `get_transcript_history` at request time, applies the same
  context-budget rules, and returns a non-navigable live source. It does not
  modify MCP behavior.

Replace per-page panel mounts with one app-shell Chat host/context. Launchers
only select a scope. This keeps the existing global cancellation behavior
correct. Retire the current per-meeting "This meeting / All meetings" toggle;
those are separate conversations under the new scope contract.

After `storageService.saveMeeting` returns a meeting ID, promote the matching
live conversation transactionally and update any saved live-source metadata to
link to the new meeting. Preserve the live scope unchanged if that promotion
fails; surface the failure and leave it recoverable rather than losing history.

For `ChatRetrievalScope::Meeting` only, assemble context from authoritative
meeting data instead of treating all content types as interchangeable FTS
hits. Select the most recently updated summary row with non-empty
`result.markdown` and the current non-empty `meeting_notes.notes_markdown`.
Search only transcript FTS rows with the existing fallback sequence, rehydrate
each hit with one adjacent segment on either side, deduplicate by transcript
ID, and order the result chronologically. If the complete search sequence
successfully returns no transcript hits, load a bounded chronological excerpt
from the start of the transcript. Build the mandatory sections first, spend the
remaining budget on transcript segments, and reserve space for a stable
coverage notice so final prompt budgeting cannot silently remove it. Reuse the
existing `FtsSearchResult`/`ChatSource` shape; no migration, dependency, or IPC
change is required.

## Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 6.1.1 | Contextual Chat | Add the scope contract and scope-aware conversation migration/repository APIs. Backfill all-meeting, meeting, and deleted-meeting-orphan rows. | M | `worker-m` (`ses_fef6a3501ffexPapv1k785Bn5c`) | None | Passed: Rust repository tests prove exact-scope isolation and legacy lineage behavior. | New migration is additive; legacy columns and commands remain usable. |
| 6.1.2 | Contextual Chat | Resolve saved-meeting, recursive-folder, and frozen-search scopes server-side through the existing Chat preparation path. | M | `worker-m` (`ses_fef606b73ffezOC38aV94Lv3Uc`) | 6.1.1 | Passed: 14 focused Rust Chat tests cover scopes, descendants, snapshot validation, and legacy retrieval. | Remove additive scoped resolver/command; legacy Chat and MCP paths remain intact. |
| 6.1.3 | Contextual Chat | Generalize `ChatPanel`, add one app-wide host, and wire Home, meeting, folder, and search-result launchers. | M | `worker-m` (`ses_fef5409d9ffe74HHBIQPVrjsgo`) | 6.1.1, 6.1.2 | Passed: scoped host/panel tests plus 81 total Vitest tests and 14 focused Rust Chat tests. | Revert additive host/launcher/stream wiring; persisted scope data remains unaffected. |
| 6.1.4 | Contextual Chat | Add live transcript retrieval, live-source/disclosure UI, and atomic promotion after recording save. | L | `worker-l` (`ses_fef4a7506ffeto0zVac8xMFt5v`) | 6.1.1, 6.1.3 | Passed: 86 Vitest tests, 15 Chat API tests, six repository tests, Cargo check, and rustfmt. | Disable/remove live host integration and promotion command; stored live threads remain local. |
| 6.1.5 | Contextual Chat | Add cross-context regression coverage and perform the Windows native smoke path. | S | `worker-s` (`ses_fef3d0e7bffevKC0KgsftE7Hcq`); user smoke | 6.1.2, 6.1.3, 6.1.4 | Passed: automated scope coverage plus all six interactive Windows/Tauri smoke checks. | Test-only changes revert independently; no production rollback required. |
| 6.1.R1 | Review remediation | Own cancellation before preparation and fence delayed listener/setup work. | L | `worker-l` (`ses_fef23a694ffe2ciW3GajyRqxAJ`) | 6.1.4 | Passed: delayed preparation/listener tests, 88 Vitest tests, and 16 Chat API tests. | Retain the single-stream model if the ownership path must be disabled. |
| 6.1.R2 | Review remediation | Bind live scope identity and remote-provider consent at the Rust boundary. | L | `worker-l` (`ses_fef1808e4ffe3iyjUCInN24zlj`) | 6.1.R1 | Passed: 21 Rust Chat tests and 88 Vitest tests cover scope validation and consent. | Disable live Chat rather than fall back to renderer-only consent. |
| 6.1.R3 | Review remediation | Make live promotion stream-safe, idempotent, and crash-recoverable. | L | `worker-l` (`ses_feef915e0ffej6a25iCC4vGCOA`) | 6.1.R1, 6.1.R2 | Passed: 90 Vitest tests, 7 repository tests, and 362 Rust library tests. | Retain live threads until an explicit repair succeeds; never discard messages. |
| 6.1.R4 | Review remediation | Capture every displayed search result and create scoped threads atomically. | M | `worker-m` (`ses_feee1fb63ffe4v2TeurUBiT46j`) | 6.1.R3, R5 repair migration | Blocked: existing duplicate identities can prevent its new unique index from applying. | Preserve current result behavior until duplicates are repaired first. |
| 6.1.R5 | Review remediation | Add migration/backfill coverage and safe null-scope repair. | M | `worker-m` (`ses_feeb962b1ffeifGa1BK2I8hoPl`) | 6.1.R3 | Passed: repair plus R4 index migration test; 364 Rust library tests. | Restore pre-upgrade backup if post-success rollback is required. |
| 6.1.R6 | Review follow-up | Re-verify live scope key at transcript read; match key (not kind) in ChatHost. | S | `worker-s` (`ses_fee5aaf9effeDJteRbUaKVa7gc`) | 6.1.R5 | Passed: restart-key Rust test; stale-panel Vitest test. | Revert the two guarded checks. |
| 6.1.R7 | Review follow-up | Tolerate deleted snapshot members on resume; bound rehydration; remove dead helpers. | S | `worker-s` (`ses_fee493047ffe2ZSAvow5iZH76C`) | 6.1.R6 | Passed: deleted-member resume and bounded fan-out Rust tests. | Revert tolerance/bounding changes; helpers stay deleted. |
| 6.1.R8 | Review follow-up | Heal notes import and FTS refresh on idempotent save retry. | S | `worker-s` (`ses_fee517c91ffeTJHlaUBH2PiPmW`) | 6.1.R5 | Passed: retry-heal Rust test; 46 repository tests. | Retry remains save-only; data is rebuildable. |
| 6.1.R9 | Review follow-up | GC discarded live threads; keep most-recent transcript tail; fix cosmetics. | S | `worker-s` (`ses_fee3f8d84ffeg9ccKpFsJRbrYx`) | 6.1.R7 | Passed: discard-GC and tail-budget Rust tests. | Revert GC and budget changes; cosmetics are inert. |
| 6.1.R10 | Single-meeting retrieval completeness | Build bounded saved-meeting context from authoritative summary/notes, transcript-only FTS neighborhoods, and a chronological no-hit fallback with explicit coverage disclosure and source parity. | M | `worker-m` (`ses_fe0c72d4bffehVzQeVKklMevJ4`) | 6.1.R9 | Passed: authoritative-content, transcript quota/neighborhood, no-hit, stale-hit, error, Unicode budget, production-path coverage, and exact source-parity tests; 385 Rust library tests, Cargo check, rustfmt, typecheck, and 93 Vitest tests. | Revert the meeting-only resolver/context builder and transcript repository helpers; existing generic FTS retrieval remains available and no persisted data changes. |
| 6.1.R11 | Broader-scope fallback accumulation | Prevent strict or rewritten FTS hits in other meetings from suppressing OR/original-query evidence in folder and all-meetings Chat. | S | Main agent; investigation `ses_fe04565a0ffefag3KkLm7dpSte`; review `ses_fe037f9a5ffe1i7dPVIIO4vNes` | 6.1.R10 | Passed: 40 Chat tests, 388 Rust library tests with 2 ignored, Cargo check, rustfmt, diff check, focused re-review, CUDA build, and install. | Revert the `resolve_scope_results` attempt merge and its three regressions; no persisted data changes. |

## Dependency Order

`6.1.1 -> 6.1.2 -> 6.1.3 -> 6.1.4 -> 6.1.5`

`6.1.R9 -> 6.1.R10 -> 6.1.R11`. Task `6.1.R10` runs alone because its repository and
prompt-assembly changes are one retrieval invariant and share `chat.rs`,
`fts.rs`, and `context.rs`.

No implementation tasks are safely parallel: the scope contract feeds
retrieval and UI; the app-wide host must exist before live Chat; the final
journey coverage requires every context.

## Risks And Mitigations

- **Conversation leakage across scopes:** persist and resolve scope on the
  backend; never depend only on the frontend route or dropdown state.
- **Search-snapshot prompt injection or scope drift:** persist identifiers,
  rehydrate server-side, cap the set, and do not accept raw renderer content.
- **Folder hierarchy ambiguity:** descendant inclusion is approved; evaluate
  membership dynamically by stable folder IDs at each request.
- **Live-data privacy:** warn before sending active transcript content to a
  cloud provider; retain current local/cloud provider indicator.
- **Data loss on stop:** promote only after meeting persistence succeeds; leave
  the live thread unchanged and recoverable when promotion fails.
- **Global stream cancellation:** use a single host rather than multiple panel
  instances; do not alter `ChatStreamState` concurrency in this sprint.
- **Migration safety:** use additive fields/backfill and tests for global,
  meeting, and deleted-meeting conversations.
- **Mandatory content can exhaust the prompt budget:** include summary and
  notes first but truncate each safely when necessary; reserve the coverage
  notice before spending the remaining budget on transcript text.
- **Multiple summary templates are ambiguous:** use the latest updated row with
  non-empty markdown, matching the existing restore-on-open policy without
  adding active-template state to Chat.
- **Neighbor overlap can duplicate or reorder speech:** deduplicate by
  transcript ID after window expansion and sort with deterministic chronology.
- **Fallback can hide index failures:** activate it only for a successful empty
  search; propagate SQL/FTS failures unchanged.
- **Sources can claim unseen evidence:** construct sources from the transcript
  segments retained by final context assembly, not from pre-truncation hits.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-17 | Folder Chat includes descendant folders. | Matches the sidebar's visible tree semantics. | Direct-folder-only filtering. | User |
| 2026-08-17 | Promote live Chat to the saved meeting thread. | Retains useful conversation history and avoids data loss. | Session-only or separate persisted live thread. | User |
| 2026-08-17 | Use `upstream/docs/` for sprint records. | User selected the project documentation folder. | New project-level `sprint-docs/` directory. | User |
| 2026-08-17 | Scope-changing uses separate threads, not the current meeting drawer's global toggle. | Prevents query-rewrite and persistence context from mixing. | Keep the toggle and reuse a meeting thread. | Main agent, pending PRD approval |
| 2026-08-17 | Search snapshots store server-rehydrated identifiers, not renderer text. | Keeps membership stable and prevents untrusted context injection. | Re-run the query or pass raw snippets to the backend. | Main agent, pending PRD approval |
| 2026-08-17 | Sprint 6.1 approved; begin task 6.1.1 only. | Approval gate passed; downstream work remains dependency-gated. | Pause or revise the PRD. | User |
| 2026-08-17 | Keep legacy Chat conversation commands while adding a scoped get-or-create command. | The current meeting-details panel remains functional until task 6.1.3 changes its UI contract. | Break legacy callers now or combine the persistence and UI tasks. | Main agent |
| 2026-08-17 | Keep this per-sprint document as the canonical 6.1 record. | A worker also appended an informational entry to the historical Phase 4 execution log; its shared-record convention does not replace the approved sprint PRD. | Treat the historical execution log as the canonical 6.1 record. | Main agent |
| 2026-08-17 | Resolve folder membership dynamically from stable IDs and preserve search membership as a bounded stored snapshot. | Folder contents should follow the current hierarchy; search Chat must not silently change when its raw query produces different later results. | Name-based folder parsing or rerunning the sidebar query on each message. | Main agent |
| 2026-08-17 | Keep scoped retrieval additive and non-streaming until the app-wide UI host is ready. | Preserves the current ChatPanel and MCP contracts during staged delivery. | Change existing stream/MCP APIs in the retrieval task. | Main agent |
| 2026-08-17 | Use one app-shell Chat host and scope-specific threads. | `ChatStreamState` permits only one active stream; separate mounted panels would cancel each other and mix lifecycle ownership. | Page-local panels or a multi-stream backend redesign. | Main agent |
| 2026-08-17 | Derive a search snapshot key from a SHA-256 hash of bounded unique chunk IDs. | Equivalent captured result sequences resume the same local thread without storing raw search text. | Random session-only key or raw query-based scope. | Main agent |
| 2026-08-17 | Require per-query confirmation for cloud and custom providers in live Chat. | A custom endpoint may be remote; the safe privacy default is disclosure unless the provider is known local. | Warn only known cloud providers or persist a consent setting. | Main agent |
| 2026-08-17 | Close or reject persisted scopes while recording; retain only live scope. | Prevents an all/meeting/folder/search stream from being mistaken for current live context. | Leave persisted Chat available alongside live Chat. | Main agent |
| 2026-08-17 | Keep the Windows native smoke as an explicit sprint blocker. | Microphone/system-audio capture, model readiness, provider configuration, and live Tauri interaction cannot be truthfully verified non-interactively. | Claim a native smoke from automated tests or skip it silently. | Main agent |
| 2026-08-17 | Pause Sprint 6.1 for review remediation. | Code and architecture reviews returned changes requested for privacy, cancellation, recovery, and persistence correctness. | Continue to native smoke or close with unresolved findings. | Main agent |
| 2026-08-17 | Approve remediation tasks 6.1.R1 through 6.1.R5. | Address all code and architecture review findings before native smoke and sprint close. | Defer selected findings or leave the sprint blocked. | User |
| 2026-08-17 | Claim stream ownership before any asynchronous Chat preparation. | An old preparation must not later become active, emit stale events, or cancel newer work. | Register cancellation only around provider streaming. | Main agent |
| 2026-08-17 | Reorder R5 before R4. | Existing duplicate scoped rows must be repaired before R4 applies its unique scoped-identity index. | Apply the index first and repair later; merge repair into R4. | User |
| 2026-08-17 | Approve follow-up tasks 6.1.R6 through 6.1.R9 after approve-with-follow-ups verdicts. | Close remaining should-fix findings from re-reviews R8/R9 before native smoke. | Defer follow-ups post-sprint; partial subset. | User |
| 2026-08-19 | Specify `6.1.R10` as a meeting-only lexical retrieval hardening task. | Fix the highest-value saved-meeting grounding gap without embeddings, schema work, or changes to other scopes. | Semantic retrieval; global retrieval rewrite; frontend coverage contract. | Main agent, pending PRD approval |
| 2026-08-19 | Use the latest non-empty persisted summary plus current notes as mandatory context. | Chat has no active-template identity, and authoritative tables avoid stale FTS omissions. | Include every summary template; use only query-matching FTS rows. | Main agent, pending PRD approval |
| 2026-08-20 | Approve the `6.1.R10` specification as written. | Freeze the meeting-only scope, acceptance checks, and rollback before worker dispatch. | Revise or defer the task. | User |
| 2026-08-20 | Approve batch 1 containing only `6.1.R10 [M]` for `worker-m`. | The task is dependency-ready after `6.1.R9` and must run alone because it shares retrieval and prompt-assembly files. | Revise the batch or owner. | User |
| 2026-08-20 | Reopen `6.1.R10` after code review and repair it in the same worker session. | Stale-hit fallback, bounded meeting metadata, and production-path verification are required by the approved no-hit, mandatory-content, budget, and source-parity criteria. They are corrections, not a new feature or scope expansion. | Create reviewer-proposed `6.1.R11`; defer the findings. | Main agent |
| 2026-08-20 | Accept the final `6.1.R10` code re-review with no findings. | All review findings and the non-vacuous production source-parity regression are closed. | Defer the verification gap. | Reviewer and main agent |
| 2026-08-22 | Accept all six interactive Windows/Tauri smoke checks and close Sprint 6.1. | The final native prerequisite passed across persisted scopes, live local/cloud disclosure, recording promotion, sources, and stale-stream fencing. | Keep the sprint blocked or waive the smoke. | User |

## Task Execution Log

### 6.1.1 - Scope contract and scope-aware conversation persistence

**Status:** Complete
**Owner:** `worker-m` (`ses_fef6a3501ffexPapv1k785Bn5c`)
**Completed:** 2026-08-17
**Implemented:**
- Added persisted `all`, `meeting`, `folder`, `search_snapshot`, and
  `live_recording` scope variants, plus a non-creatable `orphaned_meeting`
  state for deleted meeting threads.
- Added an additive migration for `scope_kind`, `scope_key`, and `scope_data`;
  backfilled existing rows and added exact-scope lookup indexing.
- Added `api_chat_get_or_create_scoped_conversation` without breaking existing
  create/get callers.
- Added four repository tests for exact-scope isolation, global lineage,
  meeting lineage, and orphan exclusion from global lookup.
**Implementation:**
- Files: `frontend/src-tauri/migrations/20260817000000_add_chat_conversation_scopes.sql`, `frontend/src-tauri/src/database/repositories/chat.rs`, `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/lib.rs`, and `frontend/src/types/index.ts`.
- Approach: Rust validates the serializable scope contract at the Tauri
  boundary. The repository resumes only an exact kind/key/data match while
  legacy `meeting_id` and `origin` continue to preserve existing lineage.
**Not implemented:**
- Folder/search/live retrieval, app-wide Chat host and launchers, cloud live
  disclosure, source rendering, and live-to-meeting promotion.
**Why not implemented:**
- These are the approved boundaries of tasks 6.1.2 through 6.1.4 and remain
  dependency-gated.
**Verification:**
- `pnpm run typecheck` - pass.
- `pnpm test` - pass, 77 tests in 18 files; existing React `act(...)` warnings
  remain non-failing.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib database::repositories::chat::tests` - pass, 4 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo check --manifest-path "src-tauri\Cargo.toml"` - pass.
- `cargo fmt --manifest-path "src-tauri\Cargo.toml" --check` - pass.
- `git diff --check` - pass.
**Rollback:**
- The migration is additive. Stop using the new scoped command and retain the
  existing legacy commands and columns if a later task must be reverted.
**Decisions and follow-ups:**
- The existing Phase 4 execution log received an informational task entry;
  this document remains the canonical Sprint 6.1 record.
- Task 6.1.2 may now begin; tasks 6.1.3 through 6.1.5 remain blocked.

### 6.1.2 - Server-side persisted scope retrieval

**Status:** Complete
**Owner:** `worker-m` (`ses_fef606b73ffezOC38aV94Lv3Uc`)
**Completed:** 2026-08-17
**Implemented:**
- Added a scoped Chat preparation path that loads the conversation by ID and
  resolves its persisted scope on the backend.
- Added recursive folder subtree lookup and FTS filtering by stable folder IDs.
- Added bounded, unique, syntactically validated snapshot identifiers;
  snapshot creation verifies every identifier exists locally and retrieval
  rehydrates those records without rerunning raw search text.
- Added focused coverage for all, meeting, recursive folder, frozen snapshot,
  invalid/oversized snapshots, and legacy retrieval equivalence.
**Implementation:**
- Files: `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/database/repositories/chat.rs`, `frontend/src-tauri/src/database/repositories/folder.rs`, `frontend/src-tauri/src/database/repositories/fts.rs`, and `frontend/src-tauri/src/lib.rs`.
- Approach: the existing model configuration, query-rewrite, context-budget,
  source construction, and FTS paths are shared; only result selection differs
  by persisted scope.
**Not implemented:**
- App-wide Chat host, launchers, live transcript retrieval/promotion, source UI
  changes, cloud disclosure, and MCP access to the new scopes.
**Why not implemented:**
- These are owned by later tasks. Keeping the current UI and MCP callers on
  their established all/meeting path prevents a staged contract break.
**Verification:**
- `pnpm run typecheck` - pass.
- `pnpm test` - pass, 77 tests in 18 files; existing React `act(...)` warnings
  remain non-failing.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib api::chat::tests` - pass, 14 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo check --manifest-path "src-tauri\Cargo.toml"` - pass.
- `cargo fmt --manifest-path "src-tauri\Cargo.toml" --check` - pass.
- `git diff --check` - pass.
**Rollback:**
- Remove the additive scoped retrieval helpers and command. Existing legacy Chat
  and MCP commands continue using the unchanged all/meeting preparation path.
**Decisions and follow-ups:**
- Folder membership is dynamic; search-result membership is frozen.
- Task 6.1.3 may now begin; tasks 6.1.4 and 6.1.5 remain blocked.

### 6.1.3 - App-wide Chat host and persisted-context launchers

**Status:** Complete
**Owner:** `worker-m` (`ses_fef5409d9ffe74HHBIQPVrjsgo`)
**Completed:** 2026-08-17
**Implemented:**
- Added one `ChatHost` in the app-shell provider tree and replaced page-local
  ChatPanel mounting with scope-specific launchers.
- Wired Home/all-meetings, meeting-detail, folder, and search-result entry
  points. Home hides Chat during an active recording until live Chat exists.
- Updated ChatPanel to resume exact scoped conversations, use an additive
  scoped streaming command, show an accessible scope label, and retire the
  unsafe per-meeting This/All toggle.
- Added deterministic bounded search snapshots using native SHA-256 over unique
  FTS chunk IDs.
- Added host/panel tests for exact scope delivery, scope-switch cancellation,
  stale terminal-event isolation, and stream ID filtering.
**Implementation:**
- Files: `frontend/src/components/ChatPanel/{index.tsx,ChatHost.tsx,scope.ts}`, `frontend/src/app/{layout.tsx,page.tsx,meeting-details/page-content.tsx}`, `frontend/src/components/Sidebar/{index.tsx,FolderTreeItem.tsx}`, `frontend/src/lib/strings/en.ts`, `frontend/src-tauri/src/{api/chat.rs,lib.rs}`, and `frontend/tests/components/chat-scope.test.tsx`.
- Approach: launcher components only set a typed scope in the app-wide host.
  The mounted panel fences event handling and persistence with both stream and
  conversation IDs when a scope changes.
**Not implemented:**
- Live transcript retrieval/promotion, live source rendering, cloud disclosure
  for in-progress transcripts, MCP scoped access, Recipes, and answer actions.
**Why not implemented:**
- These are not safe to expose before task 6.1.4 adds the live data and
  privacy lifecycle.
**Verification:**
- `pnpm run typecheck` - pass.
- `pnpm test` - pass, 81 tests in 19 files; existing React `act(...)` warnings
  remain non-failing.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib api::chat::tests` - pass, 14 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo check --manifest-path "src-tauri\Cargo.toml"` - pass.
- `cargo fmt --manifest-path "src-tauri\Cargo.toml" --check` - pass.
**Rollback:**
- Revert host/launcher and scoped-stream wiring. Existing persisted scope data
  remains intact; legacy Chat commands remain available.
**Decisions and follow-ups:**
- A recording start closes any open persisted-context panel rather than exposing
  all-meetings Chat with misleading live context.
- Task 6.1.4 may now begin; task 6.1.5 remains blocked.

### 6.1.4 - Live recording context and promotion

**Status:** Complete
**Owner:** `worker-l` (`ses_fef4a7506ffeto0zVac8xMFt5v`)
**Completed:** 2026-08-17
**Implemented:**
- Added a live-recording scope that reads the backend-native transcript history
  at send time and uses the existing prompt budget rather than React state or
  FTS persistence.
- Added clearly labelled, non-navigable live sources while retaining clickable
  saved-meeting sources.
- Added per-query confirmation before live transcript content is sent through
  cloud or custom providers; known-local providers bypass the confirmation.
- Added transactional live-to-meeting promotion after `saveMeeting`, including
  safe rewrite of saved live-source metadata and a recoverable warning on
  promotion failure.
- Restored the recording lifecycle guard: active recording closes/rejects
  persisted scopes but permits live scope; host promotion switches an open live
  panel to the resulting meeting thread.
**Implementation:**
- Files: `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/database/repositories/chat.rs`, `frontend/src-tauri/src/lib.rs`, `frontend/src/app/page.tsx`, `frontend/src/components/ChatPanel/{ChatHost.tsx,index.tsx,ChatMessage.tsx}`, `frontend/src/hooks/useRecordingStop.ts`, `frontend/src/lib/strings/en.ts`, `frontend/src/types/index.ts`, and `frontend/tests/components/chat-scope.test.tsx`.
- Approach: scope resolution branches to native transcript history only for
  `live_recording`; the existing stream/event path is reused. Promotion updates
  conversation scope and source metadata in one SQLite transaction after the
  meeting exists.
**Not implemented:**
- MCP live access, speaker diarization, transcript editing/deletion, Recipes,
  answer actions, calendar work, and external integrations.
**Why not implemented:**
- These are outside Sprint 6.1 and have separate roadmap features/dependencies.
**Verification:**
- `pnpm run typecheck` - pass.
- `pnpm test` - pass, 86 tests in 19 files; existing React `act(...)` warnings
  remain non-failing.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib api::chat::tests` - pass, 15 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib database::repositories::chat::tests` - pass, 6 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo check --manifest-path "src-tauri\Cargo.toml"` - pass.
- `cargo fmt --manifest-path "src-tauri\Cargo.toml" --check` - pass.
- `git diff --check` - pass.
**Rollback:**
- Remove the live launcher, host lifecycle/promotion call, and additive
  promotion command. Persisted `live_recording` threads remain local and can be
  explicitly removed; no migration was added by this task.
**Decisions and follow-ups:**
- Custom providers are disclosed like cloud providers because endpoint locality
  is not yet exposed to the UI.
- Task 6.1.5 may now begin.

### 6.1.5 - Cross-context regression coverage and Windows native smoke

**Status:** Blocked
**Owner:** `worker-s` (`ses_fef3d0e7bffevKC0KgsftE7Hcq`)
**Completed:** 2026-08-17 (automated portion)
**Implemented:**
- Added a regression test covering all-meetings, meeting, folder, frozen search
  snapshot, live scope, and live-to-meeting promotion through the single host.
- Re-ran the full automated frontend and focused backend verification suite.
**Implementation:**
- Files: `frontend/tests/components/chat-scope.test.tsx`.
- Approach: extend the existing scoped-host mock test rather than duplicating
  product flows or introducing native test dependencies.
**Not implemented:**
- Interactive Windows/Tauri recording smoke.
**Why not implemented:**
- It requires a loaded model, microphone and system-audio devices, a configured
  provider, a user-visible desktop window, and real audio capture. Those are
  unavailable to non-interactive automation.
**Verification:**
- `pnpm run typecheck` - pass.
- `pnpm test` - pass, 87 tests in 19 files; existing React `act(...)` warnings
  remain non-failing.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib api::chat::tests` - pass, 15 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib database::repositories::chat::tests` - pass, 6 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo check --manifest-path "src-tauri\Cargo.toml"` - pass.
- `cargo fmt --manifest-path "src-tauri\Cargo.toml" --check` - pass.
- `git diff --check` - pass.
**Rollback:**
- Revert the focused regression test. No production rollback is required.
**Decisions and follow-ups:**
- The historical Phase 4 execution log received an informational task entry;
  this document remains the canonical Sprint 6.1 record.
- Sprint close remains blocked until the manual smoke below succeeds and the
  required reviews are complete.

**2026-08-22 manual-smoke completion addendum:** The user confirmed all six
checks below passed in the installed Windows application. Task `6.1.5` and
Sprint 6.1 are complete; sprint close was explicitly approved.

### Manual Windows Smoke Checklist

1. Launch the Tauri app with a configured local provider and ready transcript
   model; grant/select microphone and system-audio devices.
2. From Home, open all-meetings Chat and verify a response streams with sources.
3. From a saved meeting, open meeting Chat; from a folder menu, ask the folder;
   from sidebar search, ask the displayed results. Confirm each scope shows its
   own resumed thread and only expected sources.
4. Start recording and confirm persisted-context Chat closes. Use "Ask about
   this recording" after transcript text appears; verify a local-provider
   answer cites a non-navigable live source.
5. With a cloud or custom provider, verify each live request asks for explicit
   disclosure and cancelling it sends no transcript content.
6. Stop and save. Reopen the saved meeting Chat and confirm live history is
   present, sources link to the saved meeting, and no stale stream writes appear.

### 6.1.R1 - Cancellation ownership before preparation

**Status:** Complete
**Owner:** `worker-l` (`ses_fef23a694ffe2ciW3GajyRqxAJ`)
**Completed:** 2026-08-17
**Implemented:**
- Claimed the global stream token before model configuration, query rewrite,
  retrieval, and live transcript preparation.
- Propagated cancellation through preparation boundaries and fenced event
  emission by active stream ownership.
- Fenced asynchronous frontend listener setup and invoke work by scope
  generation, unregistering late stale listeners.
**Implementation:**
- Files: `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/mcp/server.rs`, `frontend/src/components/ChatPanel/index.tsx`, and `frontend/tests/components/chat-scope.test.tsx`.
- Approach: a newer request cancels the prior token before it prepares; stale
  work returns without invoking a provider or terminal-event persistence.
**Not implemented:**
- Backend-bound live consent, recovery/promotion, snapshot membership, and
  migration repairs.
**Why not implemented:**
- These are separately approved remediation tasks R2 through R5.
**Verification:**
- `pnpm run typecheck` - pass.
- `npx vitest run` - pass, 88 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib api::chat::tests` - pass, 16 tests.
- Cargo check, rustfmt, and `git diff --check` - pass.
**Rollback:**
- Revert the additive ownership/fencing path to the prior single-stream model.
**Decisions and follow-ups:**
- Task R2 may now begin; R3 through R5 remain dependency-gated.

### 6.1.R2 - Backend-bound live scope and consent

**Status:** Complete
**Owner:** `worker-l` (`ses_fef1808e4ffe3iyjUCInN24zlj`)
**Completed:** 2026-08-17
**Implemented:**
- Issued an ephemeral native recording scope key and rejected inactive or mismatched live scopes in Rust.
- Required explicit per-request consent for actual non-local providers in scoped streaming and non-streaming Chat.
- Passed the existing UI disclosure decision as an ephemeral request field; it is not persisted or analyzed.
**Implementation:**
- Files: `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/audio/recording_commands.rs`, `frontend/src/services/recordingService.ts`, `frontend/src/contexts/RecordingStateContext.tsx`, `frontend/src/app/page.tsx`, `frontend/src/hooks/useRecordingStop.ts`, `frontend/src/components/ChatPanel/index.tsx`, and `frontend/tests/components/chat-scope.test.tsx`.
- Approach: native recording state is authoritative; Rust resolves the provider before allowing live transcript transmission.
**Not implemented:**
- Promotion/recovery, scoped creation atomicity, snapshot membership, and migration repair.
**Why not implemented:**
- These are separately approved remediation tasks R3 through R5.
**Verification:**
- `pnpm run typecheck` - pass.
- `npx vitest run` - pass, 88 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib api::chat::tests` - pass, 21 tests.
- Cargo check, rustfmt, and `git diff --check` - pass.
**Rollback:**
- Disable live Chat rather than restore renderer-only privacy enforcement.
**Decisions and follow-ups:**
- R3 is dependency-ready. R4 and R5 remain ordered after R3 because they share scoped persistence contracts.

### 6.1.R3 - Recoverable live promotion

**Status:** Complete
**Owner:** `worker-l` (`ses_feef915e0ffej6a25iCC4vGCOA`)
**Completed:** 2026-08-17
**Implemented:**
- Serialized promotion with Chat streaming and persisted additive live-promotion lineage for idempotent retries.
- Promoted within the meeting/transcript transaction, merged any target thread, and rewrote late/live source metadata to the meeting identity.
- Retained the native live key through IndexedDB-backed recovery and surfaced a recoverable warning when repair fails.
**Implementation:**
- Files: `frontend/src-tauri/migrations/20260817110000_add_chat_promotion_lineage.sql`, `frontend/src-tauri/src/api/{api,chat}.rs`, `frontend/src-tauri/src/database/repositories/{chat,transcript}.rs`, `frontend/src/components/ChatPanel/ChatHost.tsx`, `frontend/src/contexts/TranscriptContext.tsx`, `frontend/src/hooks/{useRecordingStop,useTranscriptRecovery}.ts`, `frontend/src/services/{indexedDBService,storageService}.ts`, and related tests.
- Approach: a nullable promotion lineage key and partial unique index make retries converge while the existing transaction preserves message/source continuity.
**Not implemented:**
- R4 snapshot membership and generic scoped get-or-create atomicity; R5 migration repair coverage.
**Why not implemented:**
- Explicitly deferred to approved tasks R4 and R5.
**Verification:**
- `pnpm run typecheck` - pass.
- `npx vitest run` - pass, 90 tests.
- Focused Chat repository tests - pass, 7 tests; full Rust library tests - pass, 362 passed and 2 ignored.
- Cargo check, rustfmt, and `git diff --check` - pass.
**Rollback:**
- Revert the additive lineage migration and promotion/recovery path together; retained live conversations are never deleted by rollback.
**Decisions and follow-ups:**
- R4 is dependency-ready and must run alone because it changes the shared scoped-conversation persistence contract.

### 6.1.R4 - Complete search snapshots and atomic scoped threads

**Status:** Blocked
**Owner:** `worker-m` (`ses_feee1fb63ffe4v2TeurUBiT46j`)
**Completed:** 2026-08-17
**Implemented:**
- Captured IDs from the sidebar's rendered search list, including title-only results.
- Added an exact backend rehydration path and atomic scoped get-or-create implementation with a scoped identity index.
**Implementation:**
- Files: `frontend/src/components/{ChatPanel/scope,Sidebar/index}.tsx`, `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/database/repositories/{chat,fts}.rs`, `frontend/src-tauri/migrations/20260817120000_add_chat_scope_identity.sql`, and scope tests.
- Approach: the unique index uses normalized scope data and `INSERT ... ON CONFLICT ... RETURNING`.
**Not implemented:**
- Duplicate/null identity repair before applying the new unique index.
**Why not implemented:**
- Existing duplicate scoped rows can cause the R4 migration to fail before a later R5 migration runs.
**Verification:**
- Typecheck, 91 Vitest tests, focused Rust snapshot/concurrency tests, Cargo check, rustfmt, and diff check - pass.
**Rollback:**
- Do not apply the R4 migration until repair succeeds; its source changes remain unaccepted pending R5.
**Decisions and follow-ups:**
- Requires user approval to reorder R5 before R4, or to merge the prerequisite repair into R4.

**2026-08-17 addendum:** R5 repaired data before the index migration; R4's focused concurrent scoped get-or-create test and the combined repair-plus-index migration test passed. R4 is complete.

### 6.1.R6 - Live-key TOCTOU closure

**Status:** Complete
**Owner:** `worker-s` (`ses_fee5aaf9effeDJteRbUaKVa7gc`)
**Completed:** 2026-08-17
**Implemented:**
- Re-verified the active live scope key immediately before the transcript snapshot read, failing closed on mismatch.
- ChatHost now matches the live panel by key, not kind, so a stale panel cannot cross recording restarts.
**Implementation:**
- Files: `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/audio/recording_commands.rs`, `frontend/src/components/ChatPanel/ChatHost.tsx`, `frontend/tests/components/chat-scope.test.tsx`.
**Not implemented:**
- Consent re-check at the read point (provider and per-request flag cannot change mid-function).
**Why not implemented:**
- R9 scoped the fix to the key; minimal diff.
**Verification:**
- Typecheck, 92 Vitest tests, 22 Chat API Rust tests, Cargo check/fmt, diff check - pass.
**Rollback:**
- Revert the two guarded checks.
**Decisions and follow-ups:**
- Retained (not cleared) `liveTranscriptScopeKey` on stop is harmless: key changes on next start, exactly when the effect compares.

### 6.1.R7 - Snapshot resume tolerance and bounded rehydration

**Status:** Complete
**Owner:** `worker-s` (`ses_fee493047ffe2ZSAvow5iZH76C`)
**Completed:** 2026-08-17
**Implemented:**
- Snapshot membership validation strict on creation only; resume tolerates deleted members and retrieves survivors without mutating stored scope data.
- Bounded rehydration per meeting (existing chunk budget) and total cap of 100 chunks; removed dead `get_by_chunk_ids`.
**Implementation:**
- Files: `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/database/repositories/fts.rs`.
- Approach: `get_latest_conversation_for_scope` gained a caller as the exact-scope resume lookup (previously dead code).
**Not implemented:**
- Subset-scope regeneration (different identity -> fresh thread) intentionally unchanged.
**Why not implemented:**
- Review prescribed exact-scope tolerance only.
**Verification:**
- Full Rust library tests - pass, 368 passed and 2 ignored; typecheck, 92 Vitest tests, Cargo check/fmt, diff check - pass.
**Rollback:**
- Revert tolerance/bounding changes; deleted helper stays deleted.
**Decisions and follow-ups:**
- Total cap mirrors `MAX_SEARCH_SNAPSHOT_RESULTS`; relevance-ranked upgrade path marked with a `ponytail:` comment.

### 6.1.R8 - Idempotent save retry heals notes/FTS

**Status:** Complete
**Owner:** `worker-s` (`ses_fee517c91ffeTJHlaUBH2PiPmW`)
**Completed:** 2026-08-17
**Implemented:**
- Extracted post-commit notes import + FTS refresh into a helper invoked from both the first-save tail and the idempotent-retry early return.
**Implementation:**
- Files: `frontend/src-tauri/src/database/repositories/transcript.rs`.
- Approach: retry hitting the already-promoted meeting re-runs the heal; promotion idempotence untouched.
**Not implemented:**
- Sharing the helper with `audio/import.rs`'s duplicated block.
**Why not implemented:**
- Out of task diff boundary; recorded as spillover.
**Verification:**
- Repository Rust tests - pass, 46 tests incl. new retry-heal test; Cargo check/fmt, diff check - pass.
**Rollback:**
- Retry remains save-only; data is rebuildable.
**Decisions and follow-ups:**
- Test schema note: promotion queries `chat_messages` unconditionally (test-only observation).

### 6.1.R9 - Discarded-live GC, tail budget, cosmetics

**Status:** Complete
**Owner:** `worker-s` (`ses_fee3f8d84ffeg9ccKpFsJRbrYx`)
**Completed:** 2026-08-17
**Implemented:**
- Conservative `api_chat_discard_live_recording` GC command invoked from both discard paths; deletes only exact-key unpromoted live threads.
- Live snapshot budget now keeps the most-recent transcript tail; fixed indentation and the meeting-scope Chat launcher title via i18n.
**Implementation:**
- Files: `frontend/src-tauri/src/{api/chat.rs,lib.rs}`, `frontend/src-tauri/src/database/repositories/chat.rs`, `frontend/src/hooks/{useRecordingStop,useTranscriptRecovery}.ts`, `frontend/src/components/{ChatPanel/index,Sidebar/index}.tsx`, `frontend/src/lib/strings/en.ts`, `frontend/src/app/meeting-details/page-content.tsx`.
**Not implemented:**
- Vitest coverage for the two fire-and-forget discard invokes.
**Why not implemented:**
- Rust covers the GC contract; wiring is fire-and-forget.
**Verification:**
- Full Rust library tests - pass, 370 passed and 2 ignored incl. discard-GC and tail-budget tests; typecheck, 92 Vitest tests, Cargo check/fmt, diff check - pass.
**Rollback:**
- Revert GC and budget changes; cosmetics are inert.
**Decisions and follow-ups:**
- Discard seam is frontend-driven because `stop_recording` clears the live key before the save-vs-discard decision exists.

### 6.1.R5 - Repair scoped migration data

**Status:** Complete
**Owner:** `worker-m` (`ses_feeb962b1ffeifGa1BK2I8hoPl`)
**Completed:** 2026-08-17
**Implemented:**
- Added an ordered repair migration before R4's index migration.
- Normalized legacy global, meeting, null, and orphaned rows; deterministically merged duplicate identities while retaining messages and promotion lineage.
**Implementation:**
- Files: `frontend/src-tauri/migrations/20260817115000_repair_chat_scope_identities.sql` and `frontend/src-tauri/src/database/repositories/chat.rs`.
- Approach: the transactional migration repairs data before R4's unique index; post-success rollback uses a pre-upgrade database backup rather than splitting merged conversations.
**Not implemented:**
- R4 UI snapshot/get-or-create changes.
**Why not implemented:**
- Already implemented in R4 and now unblocked by this prerequisite.
**Verification:**
- Focused repair plus index migration test - pass.
- Rust repository tests - pass, 9 tests; full library tests - pass, 364 passed and 2 ignored.
- Typecheck, Vitest, Cargo check, rustfmt, and diff check - pass.
**Rollback:**
- SQLx transaction rolls back on failure; restore a pre-upgrade database backup after a successful merge if necessary.
**Decisions and follow-ups:**
- R4 may be revalidated and completed using its existing approved implementation.

### 6.1.R10 - Single-meeting retrieval completeness

**Status:** Complete
**Owner:** `worker-m` (`ses_fe0c72d4bffehVzQeVKklMevJ4`)
**Completed:** 2026-08-20
**Implemented:**
- Added an ordinary saved-meeting retrieval path that always reads the latest
  non-empty summary and current notes from authoritative tables.
- Restricted meeting-query FTS to transcript rows, retained the rewritten and
  original `AND -> OR` fallback order, and expanded hits by one adjacent segment
  on each side with overlap deduplication and deterministic chronology.
- Added a bounded chronological transcript-head fallback after successful
  zero-hit searches while preserving FTS/database failures as errors.
- Added Unicode-safe context and final-prompt budgeting with mandatory section
  truncation markers, partial-transcript coverage disclosure, and transcript
  sources limited to evidence retained in the final prompt.
- Preserved the existing all, folder, search snapshot, live, meeting-list, and
  today's-meetings paths without a schema, dependency, frontend, or IPC change.
**Implementation:**
- Files: `frontend/src-tauri/src/api/chat.rs`,
  `frontend/src-tauri/src/database/repositories/fts.rs`, and
  `frontend/src-tauri/src/export/context.rs`.
- Approach: specialize only ordinary `ChatRetrievalScope::Meeting` requests;
  reuse `FtsSearchResult` and `ChatSource`, then allocate mandatory content and
  transcript evidence inside one character-counted prompt budget.
**Not implemented:**
- Embeddings, semantic/hybrid retrieval, changes to non-meeting retrieval,
  frontend coverage metadata, and schema or IPC changes.
**Why not implemented:**
- Explicitly outside the approved meeting-only lexical hardening scope.
**Verification:**
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib database::repositories::fts::tests` - pass, 13 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib export::context::tests` - pass, 9 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib api::chat::tests` - pass, 35 tests.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --manifest-path "src-tauri\Cargo.toml" --lib` - pass, 383 passed and 2 ignored.
- `$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo check --manifest-path "src-tauri\Cargo.toml"` - pass.
- `cargo fmt --manifest-path "src-tauri\Cargo.toml" --check` - pass.
- `git diff --check` - pass; existing line-ending warnings only.
**Rollback:**
- Revert the meeting-only resolver/context builder and transcript-only FTS helper.
  Generic FTS retrieval remains available and no persisted data needs repair.
**Decisions and follow-ups:**
- The latest non-empty summary is selected deterministically by updated time and
  template ID because Chat has no active-template identity.
- An initial acceptance audit found prompt-budget and source-parity gaps; the
  same worker corrected them and added the missing regressions before completion.
- The original Sprint 6.1 interactive Windows smoke remains a separate blocker.

**2026-08-20 review addendum:** Code review requested changes for stale FTS hits
that do not rehydrate, unbounded meeting-title budget consumption, and helper-only
source-parity coverage. Task `6.1.R10` is reopened in the same worker session;
the proposed `6.1.R11` is not created because these are mechanical corrections
needed to satisfy the already approved acceptance criteria.

**2026-08-20 remediation addendum:** The same worker closed all three findings:
unmapped stale FTS IDs now use the bounded chronological fallback; ordinary
meeting temporal/context titles are Unicode-safely capped; and a production-path
preparation test verifies authoritative summary/notes, coverage retention,
included-source parity, and omitted-source exclusion. Independent verification
passed 13 FTS tests, 9 context tests, 37 Chat tests, 385 full Rust library tests
with 2 ignored, Cargo check, rustfmt, and `git diff --check`.

**2026-08-20 re-review addendum:** Production behavior was accepted, but the
production-path source-parity assertion could pass with an empty source list and
its omitted segment was outside the selected neighborhood rather than removed
by prompt budgeting. `6.1.R10` is reopened for a test-only correction requiring
a non-empty exact source set and a budget-driven selected-segment omission.

**2026-08-20 test-remediation addendum:** The production-path test now creates
an authoritative `before -> anchor -> after` selected neighborhood with three
30K-character segments. It requires the exact non-empty source sequence
`before, anchor`, proves both snippets reach the final prompt, and proves the
selected `after` segment is absent from both prompt and sources because it
exceeds the remaining budget. Independent Chat tests (37), full Rust library
tests (385 passed, 2 ignored), Cargo check, and rustfmt passed.

**2026-08-20 completion addendum:** Final code re-review approved `6.1.R10`
with no findings. `pnpm run typecheck` passed and `npx vitest run` passed all 93
tests in 20 files; existing non-failing React `act(...)` warnings remain. Task
`6.1.R10` is complete. Sprint 6.1 remains blocked only on the original manual
Windows/Tauri smoke and is not being closed by this increment.

**2026-08-20 deployment addendum:** A CUDA-enabled `tauri build` completed and
the generated `meetily_0.4.0_x64-setup.exe` was installed successfully. The
installed `meetily` 0.4.0 package is at `C:\Users\arman\AppData\Local\meetily`.
The manual Windows/Tauri smoke remains pending.

**2026-08-22 deployment verification addendum:** The user completed and passed
the six-step manual Windows/Tauri checklist against the installed package. The
earlier pending statement is retained as execution history and is superseded by
this addendum.

### 6.1.R11 - Broader-scope fallback accumulation

**Status:** Complete
**Owner:** Main agent; investigation `ses_fe04565a0ffefag3KkLm7dpSte`; focused
review `ses_fe037f9a5ffe1i7dPVIIO4vNes`
**Completed:** 2026-08-20
**Implemented:**
- Confirmed from the live database that folder and all-meetings Chat returned
  after strict FTS found unrelated summaries, preventing the OR attempt that
  ranked the correct meeting transcript first in-folder and second globally.
- Changed generic scoped retrieval to run the bounded rewritten/original and
  `AND -> OR` attempts, progressively overfetch later attempts past duplicate
  strict hits, assign each chunk to its earliest/strongest attempt, and merge
  disjoint candidates round-robin under the existing final 10/30 chunk cap.
- Preserved folder subtree isolation and the meeting, snapshot, today, list,
  and live retrieval paths.
**Implementation:**
- File: `frontend/src-tauri/src/api/chat.rs`.
- No schema, dependency, frontend, IPC, or persisted-data change.
**Not implemented:**
- Semantic/embedding retrieval or authoritative hydration of every meeting in
  a folder/all-meetings scope.
**Why not implemented:**
- Broadly hydrating every meeting is unbounded; semantic candidate selection is
  a separate retrieval increment rather than part of this deterministic bug fix.
**Verification:**
- Chat API tests - pass, 40 tests.
- Full Rust library tests - pass, 388 passed and 2 ignored.
- Cargo check, rustfmt, and diff check - pass.
- Focused final re-review - approved with no findings.
- CUDA Tauri build and silent NSIS installation - pass.
**Rollback:**
- Revert the generic attempt accumulation and its three regression tests; no
  data repair is required.
**Decisions and follow-ups:**
- Retry the reported Portuguese retention-flow question in both folder and
  all-meetings Chat as part of the remaining manual Windows/Tauri smoke.

## Sprint Reviews

### Required Reviews

- Code review is required before sprint close.
- Architecture review is required because task 6.1.4 is L and this sprint
  changes persistence, IPC contracts, streaming lifecycle, and live-data
  privacy behavior.

### Review Results

### Code Review

**Reviewer:** `reviewer` (`ses_fef393cc9ffeyaUEOHUoS94nv6`)
**Verdict:** Changes requested
**Findings:**
- **Blocker:** cancellation starts only after asynchronous request preparation;
  a stale scope can subsequently register, cancel a newer stream, and send an
  unobserved request.
- **Blocker:** live remote-provider disclosure is renderer-only and not bound
  to the provider the backend resolves; non-streaming scoped Chat bypasses it.
- **Should-fix:** failed live promotion has only a transient recovery action;
  the retained thread becomes unreachable after restart.
- **Should-fix:** a search snapshot excludes title-only meetings displayed in
  sidebar results.
- **Should-fix:** scoped get-or-create has a select/insert race and no unique
  scope identity constraint.
**Required follow-ups:**
- Awaiting user approval to add remediation tasks for cancellation ownership,
  backend-bound consent, durable recovery, complete sidebar membership, and
  atomic scope creation.

### Architecture Review

**Required because:** task 6.1.4 is L and changes persistence, streaming,
live-data privacy, and recording lifecycle behavior.
**Reviewer:** `arch-reviewer` (`ses_fef393b7effex1ze0KCbDKt2hc`)
**Verdict:** Changes requested
**Findings:**
- **High:** the Rust boundary neither validates that a live scope key belongs
  to the active recording nor enforces remote-transmission consent.
- **High:** crash recovery saves meetings without promoting the matching live
  conversation, leaving it inaccessible after restart.
- **High:** promotion can race an active live stream, leaving source metadata
  stale and creating a duplicate meeting thread.
- **Medium:** scoped thread creation is not atomic; frozen search membership is
  incomplete; additive migration rollback can leave later binaries unable to
  read null scope fields.
**Required follow-ups:**
- Address the above findings, rerun both reviews, and complete the Windows
  native smoke before requesting sprint-close approval.

### Approved Remediation

| ID | Task | Size | Rationale |
|---|---|---|---|
| 6.1.R1 | Own cancellation before preparation and fence delayed listener/setup work. | L | Prevent stale scopes from starting or cancelling newer streams. |
| 6.1.R2 | Bind live scope identity and remote-provider consent at the Rust boundary. | L | Make disclosure enforceable and local-first safe for every caller. |
| 6.1.R3 | Make live promotion stream-safe, idempotent, and crash-recoverable. | L | Preserve conversation/source lineage across stop, failure, and recovery. |
| 6.1.R4 | Capture every displayed search result and create scoped threads atomically. | M | Match user-visible scope and avoid fragmented history. |
| 6.1.R5 | Add migration/backfill coverage and safe null-scope repair. | M | Validate upgrade/rollback behavior of persisted data. |

### Remediation Re-Review (2026-08-17)

### Code Review (re-run)

**Reviewer:** `reviewer` (`ses_fee86b3ebffe2ZUGx7zxKZw8v8`, model `opencode-go/glm-5.3`; logged as Review R8 in the execution doc)
**Verdict:** Approved with follow-ups
**Findings:**
- All R1-R5 acceptance evidence verified; verification re-run independently and passed.
- **Should-fix:** a frozen search snapshot whose member meeting was deleted is rejected on resume, dead-ending the panel silently (`api/chat.rs:69-84`).
- **Should-fix:** idempotent `save_transcript` retry skips notes import and FTS refresh (`transcript.rs:20-27`).
- Nit: dead `get_latest_conversation_for_scope`; discarded recordings leave unreachable live threads; live snapshot budget keeps transcript head over most-recent tail; cosmetic indentation/title drift.
**Required follow-ups:**
- Proposed `6.1.R6a` snapshot resume tolerance, `6.1.R6b` retry heals notes/FTS, `6.1.R6c` housekeeping batch (awaiting user approval).

### Architecture Review (re-run)

**Required because:** L tasks and persistence/streaming/privacy changes in this sprint.
**Reviewer:** `arch-reviewer` (`ses_fee86a9a4ffeiRsf4t2AprT2Qu`, model `opencode-go/glm-5.3`; logged as Review R9 in the execution doc)
**Verdict:** Approved with follow-ups
**Findings:**
- All five prior architecture findings verified closed with end-to-end evidence.
- **Should-fix (medium):** live-key TOCTOU — the key is captured before and checked around a 15s query-rewrite window; stop+restart in that window can let a K1-consented thread read K2's transcript (`api/chat.rs:291,346-350,448`). `ChatHost.tsx:28-30` also matches only the scope kind, keeping a stale live panel across recordings.
- **Should-fix (low-med):** snapshot rehydration is unbounded (`fts.rs:311-339`, `api/chat.rs:456-466`).
- Nits: non-Option scope columns vs downgrade NULLs; meeting save coupled to promotion success; duplicated provider taxonomy; unique index forecloses multi-thread-per-scope; dead helpers.
**Required follow-ups:**
- Proposed `6.1.R6` TOCTOU re-verify at read point + ChatHost key-match, `6.1.R7` bounded rehydration + dead-helper removal (awaiting user approval).

### Code Review - 6.1.R10
**Reviewer:** `reviewer` (`6.1.R10-review`)
**Verdict:** Changes requested
**Findings:**
- **Should-fix:** A stale FTS transcript hit whose source row has been deleted suppresses the required chronological no-hit fallback. The branch selects fallback solely when the FTS hit-ID set is empty; if that set is non-empty but none of those IDs exist in the authoritative transcript rows, `included` remains empty and Chat sends `0/N` transcript coverage instead of the bounded beginning excerpt (`frontend/src-tauri/src/api/chat.rs:1034-1069`).
- **Should-fix:** Mandatory summary/notes and the partial-coverage notice are not guaranteed under the existing budget when the user-controlled meeting title (also repeated in temporal context) exhausts it. The context builder emits the full title before considering `max_context_chars`, can therefore return over budget, and final prompt assembly truncates the prefix before the mandatory sections/notice (`frontend/src-tauri/src/export/context.rs:19-28`, `frontend/src-tauri/src/api/chat.rs:541-560`, `frontend/src-tauri/src/api/chat.rs:1286-1312`).
- **Should-fix (verification):** The R10 acceptance tests exercise private helpers, not the production meeting-scope dispatch and final source construction. In particular, the source-parity test manually reconstructs the filtering logic, so it can pass if the real branch at `prepare_chat_inputs_for_scope` diverges (`frontend/src-tauri/src/api/chat.rs:522-571`, `frontend/src-tauri/src/api/chat.rs:1987-2015`).
**Required follow-ups:**
- Create `6.1.R11` to fall back when no FTS hit rehydrates to an authoritative segment, budget/truncate meeting metadata before mandatory sections, and add a production preparation-path regression covering authoritative sections, final coverage notice, and exact source parity.

### Code Re-Review - 6.1.R10
**Reviewer:** `reviewer` (`ses_fe0acb399ffe7XpCRVJJVMecqq`)
**Verdict:** Changes requested
**Findings:**
- **Should-fix (verification):** The new regression now invokes the real ordinary-meeting preparation path and checks authoritative sections and coverage, but its source assertions are still vacuous when `inputs.sources` is empty and do not establish the exact expected retained source set. The `OMITTED` segment is outside the selected hit neighborhood, so checking that returned sources do not contain it does not prove that a transcript candidate omitted by final prompt budgeting is excluded from sources (`frontend/src-tauri/src/api/chat.rs:2037-2089`). Production source filtering is therefore still not genuinely acceptance-covered (`frontend/src-tauri/src/api/chat.rs:561-570`).
**Required follow-ups:**
- Strengthen the R10 production-path regression to assert the exact non-empty returned transcript source IDs/snippets and arrange an otherwise-selected transcript segment that is excluded specifically by final context budgeting, then assert it is absent from both the prompt and sources.

### Final Code Re-Review - 6.1.R10
**Reviewer:** `reviewer` (`ses_fe0acb399ffe7XpCRVJJVMecqq`)
**Verdict:** Approved
**Findings:**
- None.
**Required follow-ups:**
- None.

### Sprint Close

**Approved:** 2026-08-22 by the user.
**Delivered:** Scoped persisted and live Chat, safe promotion/recovery,
authoritative saved-meeting context, broader lexical fallback accumulation,
source parity, and native Windows coverage.
**Residual work:** Semantic/hybrid retrieval remains in the separately approved
`docs/hybrid-rag/` program.
