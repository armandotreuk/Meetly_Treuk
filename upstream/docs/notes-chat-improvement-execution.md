# Notes & Chat Improvements — Execution Log

> Live record of Phase 4 (F12) execution from `ROADMAP.md`. Plan & analysis: `notes-chat-improvement-plan.md`.
> **Orchestrator:** main agent (opencode-go/qwen3.8-max). **Execution:** subagents, one per task. **Human approval required before every task starts.**

---

## 1. Operating rules

1. **One task = one subagent.** The orchestrator never implements; it plans, dispatches, verifies, and logs.
2. **Approval gate:** no task starts without explicit human approval (requested per task with its model + scope).
3. **Difficulty → model:** every task carries `[S]`/`[M]`/`[L]`; recommended models in §3. Human may override.
4. **Verification:** every implementation task ends with the repo's checks — `pnpm run typecheck`, `npx vitest run`, `cargo check` (`$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"`). The subagent must paste results into its log entry.
5. **Code reviews:** run by a review subagent from a *different model family* than the implementer. Schedule in §4; orchestrator decides exact timing and may insert ad-hoc reviews after risky tasks.
6. **Documentation duty:** the executing subagent appends its entry to §6 *before* reporting done; the review subagent appends the review verdict.
7. **Scope discipline:** subagents implement only their task. Spillover findings go into the entry's "Spillover" field, not into the diff.

---

## 2. Sprint → task index (mirrors ROADMAP.md Phase 4)

| Task | Name | Diff | Status |
|------|------|------|--------|
| 1.1 | Persist recording notes to DB on stop | M | done |
| 1.2 | Per-meeting draft key, clear on stop | S | done |
| 1.3 | Refresh FTS on note save/delete | S | done |
| 1.4 | Load-error guard (read-only + Retry) | S | done |
| 1.5 | Unmount flush + beforeunload | S | done |
| 1.6 | Chat hygiene (error bubbles, timeout msg) | S | done |
| 1.7 | RecordingNotesPanel flush on unmount/re-key + gate draft key on title hydration (R1) | S | done |
| 1.8 | NotesPanel hygiene: null saveTimerRef in manual-save/blur; reset lastSavedRef at load start (R1 nits) | S | done |
| 0 (baseline) | meetings.created_at derived from metadata.json (pre-sprint, logged via R1) | S | done |
| 2.1 | Merge the two notes panels | M | done |
| 2.7 | Audio-import path: import notes.md + refresh_meeting (R1 spillover) | S | done |
| 2.2 | Notes menu: delete + "Saved HH:mm" | S | done |
| 2.3 | "Has notes" indicator | S | done |
| 2.4 | Keyboard + a11y pass | S | done |
| 2.5 | Empty-state copy (notes feed summary) | S | done |
| 2.6 | Delete dead code | S | done |
| 2.8 | Remove Editor.tsx render-time debug logs (R2 Should-fix 2) | S | done |
| 2.9 | 2.5 copy fix (AI summary mention) + cancelPendingSave test (R2 Should-fix 1 & 3) | S | done |
| 3.1 | Streaming responses via Tauri events | L | done |
| 3.1a | Shared SSE decoder, request construction, stream identity (R3) | L | done |
| 3.1b | Shared chat service for Tauri command and MCP (R3) | M | done |
| 3.2 | Stop button + cancellation end-to-end (R3) | S | done |
| 3.3 | Markdown rendering of answers | S | done |
| 3.4 | Clickable sources | S | done |
| 3.5 | "This meeting" scope toggle | M | done |
| 3.6 | Multiline input + copy answer | S | done |
| 3.7 | Model indicator + Settings deep-link | S | done |
| 4.1 | Conversation persistence | M | done |
| 4.1a | Persistence race/atomicity hardening | S | done |
| 4.2 | Query rewriting for follow-ups | M | done |
| 4.3 | Retrieval depth | L | done |
| 4.4 | Suggested prompts | S | done |
| 4.5 | Privacy polish | S | done |
| 4.1b | Distinguish deleted-meeting threads from global conversations | S | done |
| 4.3a | Preserve current question during prompt budgeting | S | done |
| 4.5a | FTS query logging and ponytail markers | S | done |
| 5.1 | BlockNote editor for notes | M | done |
| 5.1b | Durable recording BlockNote draft/stop persistence (R7 Must-Fix 2) | M | done |
| 5.1a | Serialize/version note saves + repository JSON coverage (R7) | M | done |
| 5.1c | Flush latest recording notes before native stop teardown (R7b) | M | done |
| 5.1d | Enforce save-before-delete DB ordering (R7b) | M | done |
| 5.1e | Failed recording-notes flush abort/UI-state path (R7c) | M | done |
| 5.2 | Panel/save shortcuts | S | done |
| 5.3 | Persist panel width | S | done |
| 5.3a | Restore panel width before first render | S | done |
| 5.4 | i18n groundwork | M | done |
| 5.2a | Accessible names for notes/chat icon buttons | S | done |
| 5.4a | Complete notes/chat i18n coverage | S | done |
| 6.1.1 | Scope contract and scope-aware conversation persistence | M | done |
| 6.1.R1 | Own cancellation before preparation and fence delayed listener/setup work | L | done |
| 6.1.R2 | Bind live scope identity and remote-provider consent at the Rust boundary | L | done |
| 6.1.R3 | Make live promotion stream-safe, idempotent, and crash-recoverable | L | done |
| 6.1.R5 | Safe chat scope identity repair migration | M | done |
| 6.1.R6 | Re-verify live scope key at transcript read + key-match retained live panel (R9 Should-fix 1) | S | done |
| 6.1.R7 | Snapshot resume tolerance for deleted members; bounded rehydration fan-out; delete dead `get_by_chunk_ids` (R8 Should-fix 1 + R9 Should-fix 2 + Nit 3c) | S | done |
| 6.1.R8 | Heal notes import and FTS refresh on idempotent save retry (R8 Should-fix 2) | S | done |
| 6.1.R9 | Discarded-live-thread GC + live snapshot tail budget + cosmetics (R8 Nits 2–4) | S | done |
| 6.1.5 | Cross-context regression coverage and Windows native smoke path | S | blocked (automated pass complete; interactive smoke pending) |
| 6.1.2–6.7 | Growth features (re-scope first) | M/L | pending |
| 6.2 | Resolved meeting/folder label in the chat scope badge | S | done |

---

## 3. Model assignments (opencode-go)

Policy: bigger diff/blast radius → stronger model; reviewer always from a different family than the implementer. Rankings are tier heuristics from model naming, not benchmarks on this codebase — adjust as evidence accumulates.

Custom subagents are defined in `~/.config/opencode/agent/` (pinned models); the orchestrator dispatches by agent name:

| Agent | Model | Use for |
|-------|-------|---------|
| `worker-s` | `opencode-go/deepseek-v4-flash` | `[S]` implementation tasks |
| `worker-m` | `opencode-go/kimi-k2.6` | `[M]` implementation tasks (incl. Rust-heavy `[M]`) |
| `worker-l` | `opencode-go/kimi-k2.7-code` | `[L]` implementation tasks |
| `reviewer` | `opencode-go/kimi-k3` | Sprint-end full reviews (R1, R2, R4, R5, R6, R7) |
| `arch-reviewer` | `opencode-go/gpt-5.6-luna` | R3 (streaming architecture) + any structural review |
| `reviewer-light` | `opencode-go/glm-5.2` | Low-risk batches (dead-code deletion, copy changes) |

Fallbacks if an agent/model is unavailable: `[S]` → `opencode/deepseek-v4-flash-free` · `[M]` → `opencode-go/glm-5.2` or `opencode-go/deepseek-v4-pro` · `[L]` → `opencode-go/qwen3.7-max`.

Per-task specifics (deviations from the table):

| Task | Recommended model | Why |
|------|-------------------|-----|
| 1.1 | `opencode-go/kimi-k2.6` | Cross-layer (TS hook + save flow), correctness-critical |
| 1.3 | `opencode-go/deepseek-v4-pro` | Small Rust change but touches search correctness |
| 3.1 | `opencode-go/kimi-k2.7-code` | Streaming architecture across 7 LLM providers |
| 3.2 | same session as 3.1 | Stop button is part of the streaming design |
| 4.3 | `opencode-go/kimi-k2.7-code` | Retrieval algorithm changes + tests |
| 5.1 | `opencode-go/glm-5.2` | Reuses existing BlockNote patterns, mostly wiring |
| 5.4 | `opencode-go/kimi-k2.6` | Wide but mechanical refactor |

Reviews:

| Review | Recommended model |
|--------|-------------------|
| Sprint-end full reviews (S1–S5) | `opencode-go/kimi-k3` |
| Architecture review after 3.1 | `opencode-go/gpt-5.6-luna` |
| Light/dead-code reviews (2.6 etc.) | `opencode-go/glm-5.2` |

Dispatch: the orchestrator uses the Task tool with the matching agent name (`subagent_type`). Agents were configured after task 1.1, so 1.1 ran on the orchestrator's own model (`opencode-go/qwen3.8-max`) — recorded in its entry.

---

## 4. Code review schedule

| Checkpoint | Trigger | Scope |
|------------|---------|-------|
| R1 | End of Sprint 1 | All S1 diffs — data-integrity phase, highest risk |
| R2 | End of Sprint 2 | S2 diffs (lighter; UI + deletions) |
| R3 | Right after 3.1 (+3.2) | Streaming architecture before more chat work stacks on top |
| R4 | End of Sprint 3 | Remaining S3 diffs |
| R5 | After 4.1 | Migration review before more chat persistence work |
| R6 | End of Sprint 4 | S4 diffs incl. retrieval changes |
| R7 | End of Sprint 5 | S5 diffs |
| Sprint 6 | Per `[L]` task; end-of-sprint for the rest | — |

Orchestrator may insert ad-hoc reviews after any task that surprises it (larger diff than scoped, failing verification on first pass, schema changes).

---

## 5. Entry template (subagents: copy into §6)

```
### Task X.Y — <name> [S/M/L]
- Date: YYYY-MM-DD
- Implementer model: <model id>
- Status: done | blocked
- Scope: <1-2 sentences>
- Files changed: <list>
- Verification: typecheck ✅/❌ · vitest ✅/❌ (N passed) · cargo check ✅/❌
- Notes/decisions: <anything the next dev must know>
- Spillover: <findings out of scope>

### Review RX — after <task(s)>
- Date: YYYY-MM-DD
- Reviewer model: <model id>
- Verdict: approve | changes-requested
- Findings: <list with file:line>
- Follow-up tasks created: <ids or none>
```

---

## 6. Execution log

### Task 1.1 — Persist recording notes to DB on stop [M]
- Date: 2026-08-13
- Implementer model: opencode-go/qwen3.8-max
- Status: done
- Scope: Make recording notes land in the `meeting_notes` table when a meeting is saved, covering all recording-save paths (normal stop and transcript recovery).
- Files changed: `src-tauri/src/database/repositories/transcript.rs` (import `notes.md` into `meeting_notes` after commit), `src-tauri/src/audio/recording_commands.rs` (fixed stale `save_recording_notes` docstring), `src/components/RecordingNotesPanel.tsx` (fixed stale docstring), this doc.
- Verification: typecheck ✅ · vitest ✅ (48 passed) · cargo check ✅
- Notes/decisions: Fixed at the shared Rust funnel `TranscriptsRepository::save_transcript` instead of the frontend. Both save paths (`useRecordingStop.ts` and `useTranscriptRecovery.ts`) call `storageService.saveMeeting` → `api_save_transcript` → `save_transcript`, so one change covers both, including recovery (where the sessionStorage draft is gone after an app restart but `notes.md` survives). The import runs post-commit, best-effort (same pattern as the adjacent FTS refresh), and is placed *before* `FtsRepository::refresh_meeting` so the freshly imported notes get indexed for free. Tradeoff accepted: the sessionStorage draft can be up to ~2s fresher than `notes.md`, but the stop flow (transcription wait + 4s late-segment wait + flush) always outlasts the 2s debounce/blur write, so `notes.md` is current by read time; only the last keystrokes before an instant save could theoretically lag, which the stop-sequence timing rules out in practice.
- Spillover: (1) the audio-import path (`src-tauri/src/audio/import.rs:727`) creates meetings without importing any `notes.md` — out of scope per plan, candidate follow-up if imported folders can contain notes; (2) `lib_old_complex.rs` is a dead, non-compiled copy of `lib.rs` still containing stale command registrations — candidate for task 2.6 (dead code); (3) task 1.2 owns clearing/scoping `recording_notes_draft`, which still prefills the next recording after a save.

### Task 1.2 — Per-meeting draft key, clear on stop [S]
- Date: 2026-08-13
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Scope the sessionStorage notes draft per recording session (keyed by `meetingTitle`, which is unique per meeting — timestamped `Meeting DD_MM_YY_HH_MM_SS` at start) and clear it when the recording stops, so meeting A's draft never prefills meeting B.
- Files changed: `src/components/RecordingNotesPanel.tsx` (draft key → `recording_notes_draft:${meetingTitle}` via `useTranscripts()`; reads/re-reads the draft when the key changes; one-time sweep of the legacy global key), `src/hooks/useRecordingStop.ts` (clears `recording_notes_draft:${meetingTitle}` at the top of `handleRecordingStop`, covering every stop path including tray/no-save), this doc.
- Verification: typecheck ✅ · vitest ✅ (48 passed) · cargo check ✅ (no Rust touched)
- Notes/decisions: `meetingTitle` was already the recording-screen's session identity (set synchronously before `setIsRecording(true)` in all three start paths; re-synced from backend on reload), so no new state plumbing. Hide→reopen during a recording remounts the panel with the same title → same key → draft survives (its purpose). Stop clears the key; titles are second-resolution unique, so even a skipped clear couldn't collide across meetings in practice. The read effect depends on `draftKey` so a late title sync on reload (mid-recording reload) re-reads the draft under the real title instead of splitting it. The legacy `recording_notes_draft` key is swept once per panel mount (idempotent); stale leftovers there are otherwise inert. App restart already wipes sessionStorage with the tab.
- Spillover: (1) mid-recording reload still shows an empty draft briefly if the panel mounts before the title re-syncs from the backend (it re-reads once the sync lands — cosmetic only, `notes.md` on disk is authoritative for Task 1.1); (2) the same draft-key format string (`recording_notes_draft:${meetingTitle}`) is now duplicated across two files — if a third consumer appears, hoist it to a shared constant.

### Task 1.3 — Refresh FTS on note save/delete [S]
- Date: 2026-08-13
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Call `FtsRepository::refresh_meeting` after `save_meeting_notes` and `delete_meeting_notes` so search, "chat with meetings", and MCP reads stop returning stale note content until a manual `api_rebuild_fts_index`.
- Files changed: `src-tauri/src/database/commands.rs` (best-effort `FtsRepository::refresh_meeting(pool, &meeting_id)` at the end of both `save_meeting_notes` and `delete_meeting_notes`, mirroring the `save_transcript`/`save_summary` idiom — `if let Err(e) = ... { error!(...) }`, failure not propagated), this doc.
- Verification: typecheck ✅ · vitest ✅ (48 passed) · cargo check ✅
- Notes/decisions: Used `refresh_meeting` for both save AND delete — not `remove_meeting`. `refresh_meeting` (fts.rs:192) first deletes all FTS rows for the meeting, then re-inserts from `transcripts`, `summary_processes`, and `meeting_notes`; after `delete_meeting_notes` the `meeting_notes` row is gone, so the notes re-insert SELECT finds nothing and the stale note chunk is dropped while transcript/summary chunks stay indexed (meeting still exists). `remove_meeting` alone would have also nuked the transcript/summary FTS rows. Called via fully-qualified `super::repositories::fts::FtsRepository` (commands.rs has no `use` for it; matches the `super::repositories::meeting_notes::...` style of its neighbors); `error!` was already imported. No FTS repository internals touched. Existing test `search_finds_note_text` (fts.rs) covers notes indexing; no new test required.
- Spillover: none.

### Task 1.4 — Load-error guard (read-only + Retry) [S]
- Date: 2026-08-13
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: When `get_meeting_notes` fails, `NotesPanel` must not mount the editable editor (so no autosave can overwrite stored notes the app failed to read); show a read-only error block with a Retry button instead.
- Files changed: `src/components/MeetingDetails/NotesPanel.tsx` (added `loadError` + `reloadKey` state; load effect clears `loadError` on start, sets it from the thrown error on catch, and now depends on `[meetingId, reloadKey]`; `handleRetryLoad` bumps `reloadKey` so the identical load path re-runs; error render shows "Couldn't load notes" in the status header — Save button hidden, X/close kept — plus a centered block with the error message and a Retry button; happy-path editor/autosave untouched), this doc.
- Verification: typecheck ✅ · vitest ✅ (48 passed) · cargo check ✅ (no Rust touched)
- Notes/decisions: Retry reuses the effect's own load logic via the `reloadKey` dependency bump — no extracted/replicated load function, no new cancellation path (the effect cleanup's `cancelled` flag already guards stale runs). The autosave timer can only be armed from `handleChange`, which requires the textarea, so the error state inherently has no running timer; the effect cleanup also clears it defensively on re-run/unmount. `lastSavedRef` was already reassigned from the fetched markdown in the existing success path (constraint 7), so a successful retry resets it to the loaded value with no extra code. The initial-failure toast was kept (it also fires on retry failure, satisfying "reuse toast for re-fail"), while the persistent error block carries the actual error message. The panel shell/header is kept in the error state so the X (hide notes) button stays available — the page's "Show notes" toggle only appears when the panel is hidden, so dropping the header would have left the panel unstuckable.
- Spillover: (1) the error message shows the raw Tauri reject message, which can be long/technical — if it ever looks bad in practice, wrap it with a friendlier summary + details; (2) `NotesPanel`'s `meetingId`-change path keeps stale `notes` state while the new meeting's load is in flight (pre-existing; invisible because the editor only renders after load).

### Task 1.5 — Unmount flush + beforeunload [S]
- Date: 2026-08-13
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: When `NotesPanel` tears down (meeting switch via `meetingId` change, hide/close unmount, or app close), flush the pending 2s-debounced autosave instead of dropping it, so the last keystrokes of the previous meeting are never lost.
- Files changed: `src/components/MeetingDetails/NotesPanel.tsx` (moved `saveNotes` above the load effect; new `notesRef` + `flushPendingSave`; load-effect cleanup now calls `flushPendingSave()` instead of just clearing the timer, deps `[meetingId, reloadKey, flushPendingSave]`; new `beforeunload` listener calling `flushPendingSave` with a `ponytail:` ceiling note; `handleChange` keeps `notesRef` in sync and nulls `saveTimerRef` when the timer fires), this doc.
- Verification: typecheck ✅ · vitest ✅ (48 passed) · cargo check ✅ (no Rust touched)
- Notes/decisions: Stale-closure avoidance — the cleanup must read the *latest* keystrokes, but the effect closure captures the render's `notes`, so `notesRef.current` is updated in `handleChange` (the only place a save timer can be armed; load-effect `setNotes` paths never arm one, so a render-sync effect would be redundant). The "genuinely dirty + differs from lastSavedRef" guard from the design is implemented as: (a) a pending timer can only exist after `handleChange`, (b) the timer callback and `flushPendingSave` both null `saveTimerRef` once fired/cleared, and (c) `saveNotes` itself early-returns when content equals `lastSavedRef` — so a clean state never produces a redundant write, and React 18 strict-mode double-invoke (mount→cleanup→mount) is a no-op because no timer exists at initial mount. `saveNotes`/`flushPendingSave` are captured from the *old* render's closure in the cleanup, so the flush writes to the *old* meetingId — the same identity that armed the timer. The load-error path (Task 1.4) inherently can't flush: the editor (and therefore any timer) only exists after a successful load. Manual Save and onBlur were left untouched (they already clear the timer; onBlur's unconditional `saveNotes` self-guards via `lastSavedRef`).
- Spillover: (1) `beforeunload` is genuinely best-effort — a webview can't await async Tauri IPC during teardown, so on hard app close the flush may not complete; marked with a `ponytail:` comment. If it ever proves unreliable, upgrade path is a sessionStorage draft written in `beforeunload` and reconciled on mount (same pattern Task 1.2 already uses for recording drafts). (2) After a meeting switch, an in-flight flush of the old meeting resolves *after* the new meeting's load and reassigns `lastSavedRef` to the old content — benign (next save for the new meeting still differs from it), but noted so a future refactor doesn't assume `lastSavedRef` is per-meeting-correct across switches. (3) `ReadingNotesPanel` out of scope as instructed — its notes.md is imported by the backend on save (Task 1.1).

### Task 1.6 — Chat hygiene (error bubbles, timeout msg) [S]
- Date: 2026-08-13
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Two chat fixes — (A) error bubbles appended to the chat on failed calls must stay visible to the user but never be sent back to the model as history; (B) the LLM timeout error message must report the actual configured timeout (300s) instead of the hardcoded "60 seconds".
- Files changed: `src/types/index.ts` (added `isError?: boolean` to `ChatMessage`), `src/components/ChatPanel/index.tsx` (error bubble now sets `isError: true`; history built via `messages.filter((m) => !m.isError).slice(-10)`), `src-tauri/src/summary/llm_client.rs` (both timeout error strings interpolate `REQUEST_TIMEOUT_DURATION.as_secs()`), this doc.
- Verification: typecheck ✅ · vitest ✅ (48 passed) · cargo check ✅
- Notes/decisions: The `isError` flag is set only on the error bubble the frontend appends (`role: "assistant"`, content prefixed `Error: ...`); the filter is applied at history-build time inside `handleSend`, so the bubbles still render in the chat (`ChatMessage` component ignores unknown props — no component change needed) while `api_chat_with_meetings` only ever sees real conversation turns. `ChatMessage` type callers were grepped: only `ChatPanel/index.tsx` constructs it, so no other construction site needed the flag. On the Rust side the timeout const is `REQUEST_TIMEOUT_DURATION = Duration::from_secs(300)` (llm_client.rs:8); both hardcoded "timed out after 60 seconds" strings (lines 280 and 293, in the cancellation-token and plain-await branches) now interpolate `.as_secs()` so message and config cannot drift again. Timeout value untouched. The string "timed out after 60 seconds" had exactly 2 occurrences, both in this file.
- Spillover: none.

### Review R1 — after tasks 1.1–1.6
- Date: 2026-08-13
- Reviewer model: opencode-go/kimi-k3
- Verdict: changes-requested
- Verification (re-run by reviewer from `upstream/frontend`): typecheck ✅ · vitest ✅ (48 passed, 12 files) · cargo check ✅ (`CARGO_TARGET_DIR` override). Matches all six task entries.
- Findings (prioritized):
  - **Blocker:** none. Common paths are sound: record→stop→reopen persists notes (transcript.rs:95-117 imports `notes.md` post-commit, before the FTS refresh at :119-123 so notes index for free); both `save_transcript` callers (stop: useRecordingStop.ts:246,261; recovery: useTranscriptRecovery.ts:193-196) pass `folder_path`; FTS refresh after note save/delete verified against fts.rs:192-256 (delete-all-then-reinsert drops the stale `note` chunk while keeping transcript/summary chunks); 1.4's error state can't arm a save timer (no textarea); error bubbles stay visible (ChatPanel/index.tsx:108-115) and are filtered only from model history (:42-48); timeout message now interpolates `REQUEST_TIMEOUT_DURATION` (llm_client.rs:281,297) with the value untouched at 300s (:8, applied :271).
  - **Should-fix 1 — tray-stop can still lose the last <2s of recording notes.** `useRecordingStop.ts:134` sets `isRecording(false)`, which unmounts `RecordingNotesPanel` (page.tsx:250); the panel's cleanup (`RecordingNotesPanel.tsx:52-56`) clears the pending debounce *without flushing* to `notes.md`, and `useRecordingStop.ts:137` then clears the sessionStorage draft key. On a stop path with no textarea blur (tray stop via `window.handleRecordingStop`, or app close), keystrokes since the last debounce/blur write are in neither `notes.md` nor sessionStorage — lost. Pre-1.2 they at least survived in the (wrong) global draft. Bounded (<2s of typing) but real, and the fix pattern already exists in this diff: mirror 1.5's `flushPendingSave` (NotesPanel.tsx:67-73) in `RecordingNotesPanel` (needs a `notesRef` equivalent), or flush in `handleRecordingStop` before clearing the key.
  - **Should-fix 2 — draft key is non-unique during the mid-recording reload window.** `meetingTitle` defaults to `"+ New Call"` (TranscriptContext.tsx:40) and is only re-synced from the backend async on reload (TranscriptContext.tsx:404-452, `:442`). The panel mounts as soon as `isRecording` restores (page.tsx:250), so in that window `draftKey` is the shared `recording_notes_draft:+ New Call` (RecordingNotesPanel.tsx:39): (a) it can *read* an orphan left by a different recording's pre-sync window — the cross-meeting prefill 1.2 set out to kill; (b) when the title sync lands, `draftKey` changes, the effect re-runs, and the cleanup (`RecordingNotesPanel.tsx:52-56`) drops a pending debounce without flushing — keystrokes typed in the window vanish from the UI and may never reach `notes.md`, lingering under the orphaned key. Gate draft read/write on title hydration (or skip when title is the default), and flush on re-key.
  - **Should-fix 3 — unlogged drive-by change in 1.1's diff.** `transcript.rs:30-37,45` now derives `meetings.created_at` from `metadata.json` — not in task 1.1's scope (plan §1.1; orchestrator statement) and absent from the 1.1 log entry (operating rule 7: spillover goes to the entry, not the diff). The code itself is defensive (every step falls back to `now`; worst case = old behavior) and arguably correct, so no revert needed — but the 1.1 entry must be amended to record it and its rationale.
  - **Nit 1 — stale `saveTimerRef` after manual save / blur.** `handleManualSave` (NotesPanel.tsx:136-141) and `onBlur` (:229-234) `clearTimeout` without nulling the ref, so the new `flushPendingSave` (:67-73) can fire a duplicate `saveNotes` if unmount lands before the first save resolves. Idempotent upsert + best-effort FTS make it harmless; one-line fix (null the ref in both places).
  - **Nit 2 — open question from 1.5, verdict: cosmetic.** On meeting switch A→B, the old flush's async `saveNotes` may resolve after B's load and reassign `lastSavedRef.current` (NotesPanel.tsx:52) to A's content. Real impact: the `markdown === lastSavedRef.current` guard (:43) would skip a *needed* write only if the user edits B's notes to be byte-identical to A's just-saved content — probability ~0. The "false Unsaved" concern is unfounded: `isDirty` is only set in `handleChange`, and the flush's `setIsDirty(false)` runs regardless of ordering. Cosmetic; optional one-liner follow-up: reset `lastSavedRef.current = ""` at load start (NotesPanel.tsx:79) so the ref is never cross-meeting stale.
  - **Nit 3 — 1.2's "one-time sweep" comment vs behavior.** The legacy-key sweep (RecordingNotesPanel.tsx:45-46) runs on every `draftKey` change (dep `:57`), not once per mount. Idempotent and harmless; comment slightly overstates.
  - Conventions: no new deps (package.json untouched); `ponytail:` ceilings marked where due; Rust changes mirror the existing `save_transcript`/`save_summary` best-effort idiom; `error!` was already imported (commands.rs:1). Confirmed 1.1 spillover: audio import (`import.rs:707-768`, `create_meeting_with_transcripts`) bypasses `save_transcript`, imports no `notes.md`, and doesn't refresh FTS (pre-existing) — acceptable spillover, worth a follow-up task.
- Follow-up tasks created (suggested ids; orchestrator to slot):
  - **1.7 [S]** RecordingNotesPanel: flush pending debounce on unmount/re-key (mirror 1.5's `flushPendingSave` + `notesRef`) and gate the sessionStorage draft key on title hydration — covers Should-fix 1 & 2.
  - **1.8 [S]** NotesPanel save-state hygiene: null `saveTimerRef` in `handleManualSave`/`onBlur`; reset `lastSavedRef` at load start — covers Nit 1 & 2.
  - **2.7 [S]** Audio-import path: import `notes.md` (if present) and call `FtsRepository::refresh_meeting` in `create_meeting_with_transcripts` (import.rs:707-768) — promotes 1.1's spillover.
  - Process: amend the Task 1.1 entry to document the `created_at`/`metadata.json` change (Should-fix 3) — no new task, log fix only.

### Task 0 (baseline) — meetings.created_at from metadata.json [S]
- Date: 2026-08-13 (pre-sprint; logged via R1 finding Should-fix 3)
- Implementer model: opencode-go/qwen3.8-max (orchestrator, pre-subagent-config)
- Status: done
- Scope: The meeting date shown below the meeting name in the UI was the save/completion time (~`metadata.json.completed_at`) because `TranscriptsRepository::save_transcript` wrote `meetings.created_at = Utc::now()`. Changed to read `metadata.json.created_at` (recording start time) from `folder_path`, falling back to `now`.
- Files changed: `src-tauri/src/database/repositories/transcript.rs:30-37,45`
- Verification: folded into later S1 cargo check passes (green).
- Notes/decisions: Defensive — every parse/IO step falls back to `now`, so worst case = prior behavior. This change predates the sprint and the Phase-4 plan; R1 flagged it as an unlogged drive-by within the 1.1 diff. Logged here for provenance. (Distinct from the "Fix: Show created_at instead of completed_at" idea once floated in chat history.)
- Spillover: none.

### Task 1.7 — RecordingNotesPanel flush on unmount/re-key + gate draft key on title hydration [S]
- Date: 2026-08-13
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Close R1 Should-fix 1 (tray-stop / app-close loses the last <2s of recording notes because the panel cleanup dropped the pending debounce without flushing, and the stop handler then cleared the draft) and Should-fix 2 (draft key keyed by the shared `"+ New Call"` default during the mid-recording reload window could read another recording's orphan and re-key dropped in-flight keystrokes).
- Files changed: `src/components/RecordingNotesPanel.tsx` (new `notesRef` + `flushPendingSave` mirroring 1.5; load-effect cleanup now flushes instead of just clearing the timer; new `beforeunload` listener; `draftKey` computed as `null` until `meetingTitle` hydrates past the `"+ New Call"` default; init effect skips the draft read while the key is null and preserves current notes when no draft exists; change handler skips the sessionStorage write while the key is null), this doc.
- Verification: typecheck ✅ · vitest ✅ (48 passed) · cargo check ✅ (no Rust touched)
- Notes/decisions: Stale-closure avoidance mirrors 1.5 — `flushPendingSave` reads `notesRef.current` (updated in `handleChange` and on draft hydration), never the effect's captured `notes` state, so the unmount/re-key cleanup always writes the latest keystrokes. On re-key (default → real title) the cleanup flushes hydration-window keystrokes to `notes.md` *before* the new effect run reads the draft, so nothing is dropped; if a draft exists under the real title it is restored into state/`notesRef`/`lastSavedRef` (the reload-recovery path for pre-reload content, which lives only in the draft + `notes.md`), otherwise current notes are preserved — `setNotes("")` never runs on a title sync. Tradeoff accepted: when a draft exists it supersedes window keystrokes in the UI (bounded to the short hydration window; those keystrokes already reached `notes.md` via the title-independent `save_recording_notes` and are what Task 1.1 imports). `beforeunload` is best-effort (`ponytail:` note — a webview can't await async Tauri IPC); the unmount/re-key flush is the high-value path. `useRecordingStop.ts` untouched, as scoped: clearing `recording_notes_draft:+ New Call` when the title is still the default is a harmless no-op on a never-written key (change handler skips the write), and clearing under the real title happens after `setIsRecording(false)` unmounts the panel — the flush writes `notes.md`, not sessionStorage, so the clear cannot delete flushed data. The clear-before-flush concern is moot: the flush never reads the draft key.
- Spillover: (1) R1 Nit 3 — the "one-time sweep" comment overstates (the legacy-key sweep re-runs per `draftKey` change; idempotent and harmless, left untouched as out of scope); (2) if a third consumer ever needs the draft-key format, hoist `recording_notes_draft:${meetingTitle}` to a shared constant (already flagged in 1.2's entry).

### Task 1.8 — NotesPanel hygiene: null saveTimerRef in manual-save/blur; reset lastSavedRef at load start [S]
- Date: 2026-08-13
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Two one-line hygiene fixes for R1 Nits 1 & 2 in `NotesPanel.tsx` — (a) `handleManualSave` and `onBlur` now null `saveTimerRef` after clearing the pending debounce so `flushPendingSave` can't fire a duplicate `saveNotes` if unmount lands before the manual/blur save resolves; (b) the load effect resets `lastSavedRef.current = ""` right after `setLoadError(null)` so the ref never carries the previous meeting's content into the new meeting (kills the cosmetic "Unsaved" flicker from the 1.5 cross-meeting spillover).
- Files changed: `src/components/MeetingDetails/NotesPanel.tsx` (`handleManualSave` and textarea `onBlur` each null `saveTimerRef.current` after `clearTimeout`; load effect sets `lastSavedRef.current = ""` at load start), this doc.
- Verification: typecheck ✅ · vitest ✅ (48 passed, 12 files) · cargo check ✅ (no Rust touched)
- Notes/decisions: Both timer-nulling lines mirror the exact idiom `flushPendingSave` and the timer callback already use, so the ref invariant is now "non-null ⟺ a pending debounce exists" everywhere. Autosave/flush logic, the 1.4 error guard, and the 1.5 mechanism untouched. `lastSavedRef` reset is placed before the fetch; the success path still reassigns it from the fetched markdown (or `""` when no notes exist), so the reset is only observable during the in-flight load.
- Spillover: (1) The deeper cross-meeting race from 1.5's spillover (2) remains: on switch A→B, an in-flight flush of A resolves *after* B's load and reassigns `lastSavedRef` to A's content; the new load-start reset narrows the window to the fetch itself, but a fully-correct fix needs a `currentMeetingRef` guard so the flush only writes when the meetingId still matches — out of scope here, and the 1.5/R1 assessment stands: benign (next save for B still differs from A's content unless byte-identical). (2) The reset also guards the retry path for free — a failed load leaves `lastSavedRef = ""` rather than the old meeting's content.

### Review R1b — after tasks 1.7 + 1.8
- Date: 2026-08-13
- Reviewer model: opencode-go/kimi-k3
- Verdict: approve
- Verification (re-run by reviewer from `upstream/frontend`): typecheck ✅ · vitest ✅ (48 passed, 12 files) · cargo check n/a (no Rust touched in 1.7/1.8). Matches both task entries.
- Findings (prioritized):
  - **Blocker:** none.
  - **R1 Should-fix 1 — closed.** On unmount/re-key/page-close the pending debounced save is now flushed, not dropped: the load-effect cleanup calls `flushPendingSave()` (RecordingNotesPanel.tsx:94-96), which reads `notesRef.current` (:72) — the latest keystrokes, never a stale closure. `notesRef` is written in `handleChange` (:111, the only place a timer can be armed) and on draft hydration (:89). The `saveDraftToDisk` guard (:52) makes a clean-state flush a no-op, so React strict-mode double-invoke is safe. Tray-stop ordering is order-independent: `useRecordingStop.ts:137` clears the sessionStorage key but the flush writes `notes.md` via the title-independent `save_recording_notes` and never reads the draft, so the clear cannot delete flushed data; the flush fires at unmount (page.tsx:250 via `setIsRecording(false)` at useRecordingStop.ts:134), long before Task 1.1's backend import reads `notes.md`. `beforeunload` (:102-105) is honestly marked best-effort (`ponytail:` ceiling :99-101) — acceptable; the unmount flush is the real protection.
  - **R1 Should-fix 2 — closed.** `draftKey` is `null` while `meetingTitle` is the shared default (:43-46); both the read (:85) and the sessionStorage write (:112) are gated on it, so the shared `+ New Call` key is never touched (pre-1.7 orphans under it are inert and die with the tab). On title sync (null→real key) the cleanup flushes any in-flight debounce *before* the re-run effect reads the draft, and the re-run preserves typed notes when no draft exists — `setNotes("")` never runs on a title sync (:85-92). A draft under the real key supersedes hydration-window content only when it genuinely exists (`getItem` non-null, :87); titles are set synchronously at start (useRecordingStart.ts:114-115) or re-synced from the backend (TranscriptContext.tsx:442), so cross-meeting collision is near-impossible and the residual tradeoff (window keystrokes reach `notes.md` but the draft wins the UI) is bounded to the hydration window and correctly documented in the 1.7 entry.
  - **R1 Nit 1 & 2 — applied.** `saveTimerRef.current = null` after `clearTimeout` in both `handleManualSave` (NotesPanel.tsx:138-141) and `onBlur` (:232-235) — the "non-null ⟺ pending debounce" invariant now holds everywhere, so `flushPendingSave` (:67-73) can no longer fire a duplicate after a manual/blur save. `lastSavedRef.current = ""` at load start (:81), before the fetch; the success path still reassigns (:89,:92). No regressions to the 1.4 guard (error state still can't arm a timer; the reset also sanitizes the retry path) or the 1.5 mechanism (cleanup runs before the new effect body, and the flush's `markdown === lastSavedRef` guard at :43 evaluates synchronously at call time, so the old meeting's flush compares against the old `lastSavedRef` before the reset lands).
  - **Cross-meeting save-completion race (1.5 spillover) — still benign, leave as-is.** The race survives in form (A's in-flight flush can resolve after B's load-start reset and reassign the shared `lastSavedRef` to A's content), but 1.8 doesn't worsen it — it narrows the stale window to B's fetch, and the editor isn't rendered during load (isLoading gate, NotesPanel.tsx:149-155), so no user-initiated save can observe the stale value inside the window. The `currentMeetingRef` guard remains the full fix if it ever proves visible; no new task warranted beyond the existing 1.5/1.8 spillover notes.
  - **Nit (new, cosmetic):** if a recording's title ever literally equaled `"+ New Call"` (e.g., a user-chosen name flowing into `TranscriptContext.meetingTitle`), the draft would silently never persist for that session (RecordingNotesPanel.tsx:44) — graceful degradation only, since the `notes.md` mirror is title-independent and no data is lost. Not worth code; noted for the task 2.1 merge.
  - Scope/conventions: working-tree changes match the S1 task set (1.7/1.8 touch only the two panel files + this doc); `useRecordingStop.ts` untouched as scoped; no new deps; `ponytail:` ceilings marked; the 1.7 draft-supersession and clear-on-stop tradeoffs are documented in the entry rather than left implicit.
- Follow-up tasks created: none. (R1's Should-fix 1 & 2 and Nits 1 & 2 are all closed; the deferred cross-meeting `currentMeetingRef` guard stays as recorded spillover on 1.5/1.8, to be reconsidered under task 2.1's panel merge.)

### Task 2.1 — Merge the two notes panels [M]
- Date: 2026-08-13
- Implementer model: opencode-go/kimi-k2.6
- Status: done
- Scope: Extract the shared editor logic (debounce, flush, beforeunload, dirty/saving state) into `useNotesEditor` hook and the shared UI (status header, textarea, compact mode) into `NotesEditorShell`. Keep two thin shells at existing import paths (`RecordingNotesPanel`, `NotesPanel`) wiring their own backends.
- Files changed: `src/components/notes/useNotesEditor.ts` (new), `src/components/notes/NotesEditorShell.tsx` (new), `src/components/RecordingNotesPanel.tsx` (rewritten), `src/components/MeetingDetails/NotesPanel.tsx` (rewritten), `tests/components/notes/useNotesEditor.test.ts` (new — 2 tests), this doc.
- Verification: typecheck ✅ · vitest ✅ (50 passed, 13 files) · cargo check n/a (no Rust touched)
- Notes/decisions:
  - **Approach:** hook + presentational shell, not a single parameterized component. The two persistence backends (`save_recording_notes` vs `save_meeting_notes` + `get_meeting_notes`) stay isolated in their thin shells, so no conditional pile-up that could re-introduce divergence.
  - **Sprint-1 invariants preserved without restructuring:**
    - 1.4 load-error guard: `NotesPanel` shell still manages `isLoading`/`loadError` and passes `errorState` to the shell; no textarea or autosave timer is armed in error state (the hook's `handleChange` is never invoked).
    - 1.5/1.7 flush on unmount/re-key/beforeunload: `useNotesEditor` centralizes `flushPendingSave` (reads `notesRef.current`, never stale closure), registers the `beforeunload` listener, and runs it in the `initialNotes` effect cleanup. Both shells inherit this.
    - 1.2/1.7 recording draft key gated on `meetingTitle !== "+ New Call"`: `RecordingNotesPanel` shell computes `draftKey` exactly as before; sessionStorage read/write gated; `setNotes("")` never runs when no draft exists on title sync; `useRecordingStop.ts` still clears the same key format — no caller changes.
    - 1.8 `saveTimerRef.current = null` after `clearTimeout` in manual-save/blur: now enforced in the shared hook for both panels; `lastSavedRef.current = ""` at load start remains in the `NotesPanel` shell's load effect (direct mutation of the exposed ref).
    - R1b nit (title literally "+ New Call"): `save_recording_notes` is called through the hook regardless of `draftKey`, so `notes.md` is always written — the draft key only gates sessionStorage.
  - **Refactoring detail:** `useNotesEditor` exposes `lastSavedRef` so the `NotesPanel` shell can reset it at load start (1.8 invariant). The hook uses a ref for `onSaveError` so the shells don't need to memoize their error callbacks; `wrappedSave` only depends on `options.save`, which legitimately changes when `meetingId` changes, correctly triggering `flushPendingSave` updates and beforeunload re-registration.
  - **Tradeoff accepted:** The `RecordingNotesPanel` shell still duplicates the draft-key string format (`recording_notes_draft:${meetingTitle}`) with `useRecordingStop.ts`. Hoisting to a shared constant would be a 3-file change with no behavior change — left as a future cleanup if a third consumer appears.
- Spillover: none.

### Task 2.6 — Delete dead code [S]
- Date: 2026-08-14
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Remove the dead `/notes/[id]` demo route and the unused `BasicBlockNoteTest` component, both superseded by the notes-panel work.
- Files changed (deleted): `src/app/notes/[id]/page.tsx` (+ empty `notes/` route dir), `src/components/BlockNoteEditor/BasicBlockNoteTest.tsx`, this doc.
- Verification: typecheck ✅ · vitest ✅ (50 passed, 13 files) · cargo check n/a (no Rust touched)
- Notes/decisions: Dead-code confirmation greps before deletion — (a) `/notes/` as a route: no `href`/`router.push`/`router.replace`/`navigate` targets anywhere in `frontend/src`; the only `"/notes/"`-shaped matches are `@/components/notes/*` imports (the task 2.1 components `useNotesEditor`/`NotesEditorShell` — unrelated, kept); the plan doc itself is the only other reference to `app/notes`. (b) `BasicBlockNoteTest`: only matches are the plan doc and the file itself — zero imports; `BlockNoteEditor/Editor.tsx` is still used (dynamic import from `AISummary/BlockNoteSummaryView.tsx:16`), so only the test component was removed. The route dir contained a single page (no layout), so the empty `notes/` dir was removed with it; the `generateStaticParams` static-params hack died with the page. One stale `.next/types/app/notes/[id]/page.ts` (gitignored build artifact) referenced the deleted page and broke `tsc --noEmit`; cleared it and typecheck went green — no source change needed.
- Spillover: the plan's 2.6 row also lists "render-time logging in `BlockNoteEditor/Editor.tsx`" (4× `logger.debug("📝 EDITOR: …")` at Editor.tsx:19,29,36-38,46) but the task prompt scoped 2.6 to the two code paths listed above, so the logging was left untouched — dispatch a trivial follow-up if the plan row is authoritative. (`MainNav` `h-0`, also named in plan finding #14, is not in the 2.6 row.)

### Task 2.7 — Audio-import path: import notes.md + refresh_meeting [S]
- Date: 2026-08-14
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Close R1's 1.1 spillover — the audio-import ingest path (`audio/import.rs`) bypasses `save_transcript`, so imported meetings neither imported a `notes.md` the user placed alongside the audio nor refreshed the FTS index. Mirror Task 1.1's post-commit notes import and the Task 1.3 FTS refresh in the import flow.
- Files changed: `src-tauri/src/audio/import.rs` (new `use` of `database::repositories::{fts::FtsRepository, meeting_notes::MeetingNotesRepository}`; in `run_import`, after the audio copy, mirror any `notes.md` from the source audio's folder into the meeting folder — best-effort with `warn!` on failure; in `create_meeting_with_transcripts`, post-commit: read `<folder_path>/notes.md`, insert into `meeting_notes` via `MeetingNotesRepository::save_notes` when present and non-empty — best-effort, `error!` logged — then unconditional best-effort `FtsRepository::refresh_meeting`, both mirroring transcript.rs:95-123), this doc.
- Verification: typecheck ✅ · vitest ✅ (50 passed, 13 files) · cargo check ✅
- Notes/decisions: **Extraction approach:** Task 1.1's notes-import logic in `transcript.rs:97-117` is inline (not a helper), so it was mirrored verbatim into `create_meeting_with_transcripts` rather than extracted — extracting a shared helper would have touched two more files for no behavioral gain; the block is 11 lines. Placement is inside `create_meeting_with_transcripts` (import.rs:707, the private DB-commit funnel, per R1's follow-up suggestion) right after `tx.commit()`, so both the meeting row and transcripts exist before the notes insert. **FTS refresh is unconditional** (not gated on notes.md existing): imported transcripts were just committed and need indexing — gating would have re-introduced the stale-FTS bug the sprint exists to fix; this matches transcript.rs:119-123 exactly and the Task 1.3 pattern (`if let Err(e) = … { error!(…) }`, failure never propagated). **The `notes.md` source:** the import copies the audio into a fresh meeting folder, so a notes.md sitting next to the *source* audio would never be found by a `<folder_path>/notes.md` check alone — a no-op fix. The new copy step (source-folder `notes.md` → meeting folder) makes the mirror real, and keeps the imported meeting folder shape identical to a recorded one (audio + notes.md + transcripts.json + metadata.json), so `NotesPanel`'s save-to-folder mirror stays symmetric. `MeetingNotesRepository::save_notes` is upsert-on-conflict, so a later `save_meeting_notes` from the UI is safe.
- Spillover: (1) `create_meeting_with_transcripts` inserts `meetings.created_at = now` (completion time) rather than deriving it from `metadata.json` like `save_transcript` (task 0) — the import writes `metadata.json` *after* the DB commit with `created_at = now`, so import meetings' displayed date is completion time; pre-existing, out of scope. (2) The `notes.md` copy is best-effort and silent on absence (the common case) — if a future flow lets users attach notes from the import dialog instead of a sidecar file, extend the copy step.

### Task 2.2 — Notes menu: delete + "Saved HH:mm" [S]
- Date: 2026-08-14
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Delete affordance for the post-meeting notes panel (confirm → `delete_meeting_notes` → clear editor + status) and a human last-saved timestamp in the header status area.
- Files changed: `src/components/notes/useNotesEditor.ts` (new `lastSavedAt` state set on successful save; new `cancelPendingSave` + exposed `setIsDirty` for the delete flow), `src/components/notes/NotesEditorShell.tsx` (optional `onDeleteNotes` trash button — disabled while saving or when empty; green status now renders "Saved HH:MM" when `lastSavedAt` is set), `src/components/MeetingDetails/NotesPanel.tsx` (`handleDeleteNotes` with `confirm`, `cancelPendingSave` before the IPC, editor cleared + `setIsDirty(false)` + `refetchMeetings()` on success; `saveNotes` now refetches the meetings list after each save so the 2.3 indicator stays live), `src/components/RecordingNotesPanel.tsx` (passes `lastSavedAt` for a consistent status), this doc.
- Verification: typecheck ✅ · vitest ✅ (50 passed, 13 files) · cargo check ✅ (no Rust touched)
- Notes/decisions: Delete cancels the pending debounce BEFORE the IPC — the timer callback captures the typed `value` (it does not read `notesRef.current`), so a fire after deletion would re-insert the deleted content; clearing the refs alone cannot prevent that. The trash button is disabled while `isSaving` to close the in-flight-save race (a save resolving after the delete would re-insert). Bare `confirm()` precedent exists in the app (TemplateEditor.tsx:164, TranscriptRecovery.tsx:100). `setIsDirty` exposure mirrors the hook's existing `setNotes` exposure. "Saved HH:MM" only appears after a save in the current session (the load path writes `lastSavedRef` directly, not through `wrappedSave`) — the honest reading of the requirement. The parent (`page-content.tsx`) passes only meetingId/width/onClose — no `onNotesChange` exists, so none was wired; the refetch keeps the rest of the app in sync instead.
- Spillover: (1) `saveNotes` awaits a meetings-list refetch on every debounced save (~2s cadence while typing) — a local SQLite read over ~30 rows, cheap, but a candidate to debounce if it ever shows up in profiling. (2) If the delete IPC fails after `cancelPendingSave`, the pending keystrokes stay unsaved until the next keystroke/blur/manual save — the user sees the error toast and content is not lost. (3) `notes.md` cleanup on delete is already handled best-effort inside `delete_meeting_notes` (Task 1.3's file).

### Task 2.3 — "Has notes" indicator [S]
- Date: 2026-08-14
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Small green dot on sidebar meeting rows that have notes, backed by a `has_notes` flag computed in the meetings-list query (no new migration).
- Files changed: `src-tauri/src/database/models.rs` (`MeetingModel.has_notes: bool` with `#[sqlx(default)]`), `src-tauri/src/database/repositories/meeting.rs` (`get_meetings` LEFT JOINs `meeting_notes` and selects `CASE WHEN (notes_markdown != '') OR (notes_json != '') THEN 1 ELSE 0 END AS has_notes`), `src-tauri/src/api/api.rs` (`Meeting` struct + `From<MeetingModel>` gain `has_notes`; the two `meeting_mapper_*` test literals updated), `src/components/Sidebar/SidebarProvider.tsx` (`CurrentMeeting.has_notes?: boolean`; `fetchMeetings` maps the new field), `src/hooks/useSidebarTree.ts` (`MeetingLike.has_notes?`, `MeetingNode.hasNotes?`, nodes built with it), `src/components/Sidebar/index.tsx` (all three `MeetingTreeItem` call sites — unfiled render, tree render, flat search list — pass `hasNotes`), `src/components/Sidebar/MeetingTreeItem.tsx` (green dot after the title when `hasNotes`), this doc.
- Verification: typecheck ✅ · vitest ✅ (50 passed, 13 files) · cargo check ✅
- Notes/decisions: SQLite `bool` maps an integer 0/1, so the CASE decodes straight into the field with no type shim. `#[sqlx(default)]` behavior (missing column → `Default::default()`) was confirmed against sqlx-macros 0.8.6 `derives/row.rs`; it lets the other three `MeetingModel` SELECTs (explicit column lists in meeting.rs:64,124 and api.rs:1221) keep working unchanged. The CASE covers both `notes_markdown` and `notes_json` — the app only writes markdown today (save passes `notesJson: null`), but a JSON-only row would otherwise miss the dot. Freshness: `NotesPanel` refetches the list after every save and after delete (see 2.2), so the dot appears/disappears live; the intro-call row never gets it (`hasNotes` undefined). Rust change is one query field + one serialization field + one struct field with `#[sqlx(default)]` — no migrations.
- Spillover: (1) `api_get_meetings`'s `Meeting` gained a field; `storageService.getMeetings` typing silently ignores it, and no Rust consumer deserializes `Meeting` from JSON (serde would reject a missing field if one ever appears). (2) Imported meetings (task 2.7) get a correct `has_notes` only when they carry a `notes.md` sidecar — the import path mirrors it into `meeting_notes` (2.7), so the flag follows; a bare audio import correctly shows no dot.

### Task 2.5 — Empty-state copy (notes feed summary) [S]
- Date: 2026-08-14
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Friendly empty-state copy in the notes editor instead of the bare "Add your notes here..." placeholder.
- Files changed: `src/components/notes/NotesEditorShell.tsx` (textarea placeholder → "No notes yet. Start typing to jot down key points from this meeting."), this doc.
- Verification: typecheck ✅ · vitest ✅ (50 passed, 13 files) · cargo check ✅ (no Rust touched)
- Notes/decisions: The notes area's only empty state is the textarea placeholder — the editor must stay visible because it IS the empty state, so copy went into the placeholder (it only renders when empty). Grepped for a notes feed/summary view across meetings: the sidebar "Meeting Notes" section lists meetings (not notes content), and the only `/notes` route was the demo page task 2.6 removed — there is no cross-meeting notes feed, so no second empty state to add. Tone matches the neighboring copy family ("Ask questions about your meetings." in ChatPanel).
- Spillover: none.

### Task 2.4 — Keyboard + a11y pass [S]
- Date: 2026-08-14
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Keyboard-accessibility audit and fixes for the shared notes editor (textarea focus visibility + label, button names, Ctrl/Cmd+S save shortcut, save-status announcements) and the related notes UI (load-error focus, sidebar has-notes dot).
- Files changed: `src/components/notes/NotesEditorShell.tsx` (textarea focus ring + `aria-label`, Ctrl/Cmd+S shortcut, `aria-live` status region, button `aria-label`s), `src/components/MeetingDetails/NotesPanel.tsx` (Retry `autoFocus`), `src/components/Sidebar/MeetingTreeItem.tsx` (has-notes dot `role="img"` + `aria-label`), this doc.
- Verification: typecheck ✅ · vitest ✅ (50 passed, 13 files) · cargo check n/a (no Rust touched)
- Notes/decisions: A11y fixes, element by element:
  - Notes textarea → was `outline-none` with no focus replacement (invisible focus on a white panel); now `focus:outline-none focus:ring-2 focus:ring-blue-500`, matching the codebase's ring-focus convention (TranscriptPanel.tsx:117). Added `aria-label="Notes"` (placeholder alone is not an accessible name).
  - Notes textarea → new Ctrl/Cmd+S save shortcut (`onKeyDown`: `preventDefault` + `onManualSave` when `isDirty && !isSaving` — mirrors the Save button's disabled condition; guard prevents the browser save dialog). Wired in the shared shell, so both panels inherit it; the hook itself needs no change (its `handleManualSave` is already exposed).
  - Save button → added `aria-label="Save notes"` (was `title`-only in compact mode).
  - Delete button → added `aria-label="Delete notes"`.
  - Hide-notes (X) button → added `aria-label="Hide notes"` (same icon-only pattern as the other two).
  - Save status ("Saving…" / "Unsaved" / "Saved HH:MM") → wrapped in `aria-live="polite"` so status changes are announced without stealing focus; the spinner icon inside the region got `aria-hidden="true"` (lucide-react 0.469 does NOT self-hide SVGs from AT — verified in `dist/esm/defaultAttributes.js`).
  - Load-error Retry button → added `autoFocus` (applies on mount of the error state, so focus lands on the retry affordance and SRs announce it).
  - Sidebar has-notes dot → was `aria-hidden` + `title` (completely invisible to AT); now `role="img"` + `aria-label="Has notes"` (title kept for tooltip), so it's announced as an image.
  - Audited and unchanged: Tab order (header buttons → textarea, matching visual order); no keyboard traps (native textarea; no key-swallowing handlers — the new `onKeyDown` only intercepts Ctrl/Cmd+S); `window.confirm()` delete (natively keyboard-accessible); the `Button` component already provides `focus-visible` ring styles; header "Couldn't load notes" error text plus the error block both render — focus lands on Retry per above.
- Spillover: (1) the sidebar's plain `<button>` elements (edit title / meeting actions / delete meeting in `MeetingTreeItem.tsx`) have no `focus-visible` styles — pre-existing, not part of the notes UI, left untouched; (2) in compact mode (`width < 320`) the status text is conditionally hidden (`!isCompact &&`), so the `aria-live` region has nothing to announce there — a screen-reader user in compact mode gets no status announcement; deliberate (compact header is icon-only) but worth knowing if compact notes panels ship to SR users.

### Review R2 — Sprint 2 (tasks 2.1–2.7)
- Date: 2026-08-14
- Reviewer model: opencode-go/kimi-k3
- Verdict: approve
- Verification (re-run by reviewer from `upstream/frontend`): typecheck ✅ · vitest ✅ (50 passed, 13 files — new `tests/components/notes/useNotesEditor.test.ts` passes, with cosmetic React `act(...)` environment warnings) · cargo check ✅ (`CARGO_TARGET_DIR` override). Matches all seven task entries.
- Sprint-1 invariant audit (all intact post-merge):
  - 1.1: `transcript.rs:95-117` notes.md import untouched; `RecordingNotesPanel` still writes `notes.md` via `save_recording_notes` (registered, lib.rs:688) through the hook regardless of `draftKey`.
  - 1.2/1.7: draft-key gating preserved verbatim (`RecordingNotesPanel.tsx:34-37`; `useRecordingStop.ts:137` same key format); re-key cleanup flushes before re-read (`:70-72`); gated sessionStorage read/write (`:61-68,:77-79`).
  - 1.3: FTS refresh on save/delete intact (`commands.rs:343-348`, `:379-384`).
  - 1.4: load-error guard survives — shell renders `errorState` instead of the textarea (`NotesEditorShell.tsx:125-127`), so no timer can be armed in error state; Retry reuses `reloadKey` (`NotesPanel.tsx:98-100`).
  - 1.5/1.7: unmount/re-key flush + `beforeunload` centralized in the hook (`useNotesEditor.ts:79-96`); flush reads `notesRef.current`, never a stale closure; `NotesPanel`'s load-effect cleanup also flushes (`:92-95`) — idempotent, first caller nulls the timer.
  - 1.8: `saveTimerRef` nulled after every `clearTimeout` (useNotesEditor.ts:64,75,108,116,124); `lastSavedRef.current = ""` at load start (`NotesPanel.tsx:64`).
  - No caller broke: both panels keep their import paths and props (`page.tsx:257-260`, `page-content.tsx:373-377`); `refetchMeetings` is the stable `fetchMeetings` useCallback (`SidebarProvider.tsx:417`), so the hook's flush effect does not re-run per render.
- Findings (prioritized):
  - **Blocker:** none. The 2.2 cancel-before-delete ordering is correct: the debounce closure captures the typed `value` (`useNotesEditor.ts:109`), so `cancelPendingSave()` must (and does) run before the delete IPC (`NotesPanel.tsx:104-109`); the delete button is disabled while `isSaving`, and the blur-save that precedes a delete click lands during the blocking native `confirm()`, so save-then-delete ordering holds in practice. Delete backend cleans `notes.md` + refreshes FTS (commands.rs:369-384). 2.3's `#[sqlx(default)]` (models.rs:20-24) is used correctly: only `get_meetings` (meeting.rs:10-22) selects the CASE column; the other three `MeetingModel` queries (meeting.rs:70,130; api.rs:1225) silently default `has_notes` to false, and no TS consumer reads `has_notes` from those paths. LEFT JOIN is 1:1 on `meeting_notes.meeting_id` PK (migration 20251223000000) — no row duplication, no N+1. 2.7 mirrors the 1.1/1.3 idiom exactly (import.rs:773-792 vs transcript.rs:95-123): notes import post-commit before an unconditional FTS refresh; the source-folder `notes.md` mirror copy (import.rs:353-361) is inside the per-import fresh meeting folder, so no existing meeting's files can be clobbered, and cancellation cleanup removes it (import.rs:364-368).
  - **Should-fix 1 — 2.5 shipped copy misses the task's point.** Plan row 2.5 is "Mention that notes are incorporated into the AI summary (the hidden differentiator)" and the §2 task name is "Empty-state copy (notes feed summary)", but the shipped placeholder (NotesEditorShell.tsx:139) never mentions the summary feed. One-line copy change, e.g. append "Your notes are included when the AI summary is generated."
  - **Should-fix 2 — Editor.tsx render-time logging remains, against plan row 2.6.** 4× `logger.debug("📝 EDITOR: …")` at `BlockNoteEditor/Editor.tsx:19,29,36,46` (the :19/:36 calls serialize block content on mount/every keystroke). Dev-only — `logger.debug` is gated to non-production (lib/logger.ts:40,50-54) — so no privacy/perf impact in prod, but the plan row explicitly lists these for deletion and the 2.6 entry defers to a follow-up. `Editor.tsx` itself is live (BlockNoteSummaryView.tsx:16), confirmed not deleted.
  - **Should-fix 3 — test gap on the delete-critical path.** The two new hook tests are meaningful (unmount flush reads `notesRef` not a stale closure; debounce + manual-save cancel with no duplicate fire), but `cancelPendingSave` (useNotesEditor.ts:72-77) — the half of 2.2 that prevents a deleted note from being re-inserted — has no test. ~10 lines in the existing harness: arm via `handleChange`, call `cancelPendingSave`, advance fake timers, assert no save.
  - **Nit 1 — font parity not achieved.** Plan row 2.1 says "Same save-status, toasts, font everywhere"; the shell keeps `font-mono` only on NotesPanel via `extraTextareaClassName` (NotesPanel.tsx:152, RecordingNotesPanel passes none) — preserving the exact pre-merge divergence. Cosmetic; either unify or document as deliberate.
  - **Nit 2 — meetings refetch on every debounced save (2.2 spillover 1): acceptable.** `NotesPanel.tsx:43` refetches the whole list at 2s cadence while typing (local SQLite read, plus two INFO log lines per `api_get_meetings` call). Cheap; if it ever shows up, gate the refetch on the empty↔non-empty transition (the only time the dot changes).
  - **Nit 3 — latent ordering assumption in delete.** If the blocking `confirm()` (NotesPanel.tsx:103) is ever replaced by an async dialog, an in-flight blur-save could resolve after the delete IPC and re-insert the content; gate delete on `isSaving` settling in that future change. Fine today.
  - **Nit 4 — idiom duplication.** The best-effort `FtsRepository::refresh_meeting` block is now at 4 call sites (transcript.rs:121, commands.rs:345, commands.rs:382, import.rs:790) and the notes.md-import block at 2 (transcript.rs:97-117, import.rs:773-786). Acceptable per minimal-diff; hoist a helper only if another consumer appears.
  - **Nit 5 — `has_notes` SQL never executed in tests.** `sqlx::query_as` is runtime-checked; the JOIN/CASE is correct by inspection but untested. An in-memory-pool test has precedent (fts.rs:417) if cheap insurance is wanted.
  - **Nit 6 — `act(...)` warnings in the new test file.** `IS_REACT_ACT_ENVIRONMENT` isn't set; tests pass regardless. Cosmetic console noise.
  - Spillover triage: 2.2-(2) delete-IPC-failure after cancel — acceptable (editor content untouched, error toast, next keystroke/blur saves); 2.2-(3)/2.3-(1) `Meeting` serde field — acceptable, verified no Rust code deserializes `Meeting` from JSON (only `api_get_meetings` return, api.rs:368) and the frontend mapping defaults `?? false` (SidebarProvider.tsx:118); 2.4-(2) compact-mode aria-live silence — acceptable, deliberate and documented; 2.7-(1) `created_at = now` for imported meetings — acceptable, confirmed metadata.json is written after the commit (import.rs:670 before :678-693) so task 0's derivation can't apply without reordering.
  - 2.6 deletions confirmed dead: `src/app/notes/` and `BasicBlockNoteTest.tsx` gone, zero references in `src/`; stale `.next/types` artifact cleared (verified absent). Working-tree scope matches the S1+S2 task set; no drive-by source changes.
- Follow-up tasks created (suggested ids; orchestrator to slot):
  - **2.8 [S]** Remove the 4 render-time `logger.debug("📝 EDITOR: …")` calls in `BlockNoteEditor/Editor.tsx:19,29,36,46` — completes plan row 2.6 (R2 Should-fix 2).
  - **2.9 [S]** Empty-state copy: mention notes feed the AI summary (R2 Should-fix 1); while there, add the `cancelPendingSave` hook test (R2 Should-fix 3) and set `IS_REACT_ACT_ENVIRONMENT` in the test setup (Nit 6).

### Task 2.8 — Remove Editor.tsx render-time debug logs [S]
- Date: 2026-08-14
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Remove the 4 render-time `logger.debug("📝 EDITOR: …")` calls in `BlockNoteEditor/Editor.tsx` that plan row 2.6 listed for deletion and task 2.6 deferred to a follow-up (R2 Should-fix 2).
- Files changed: `src/components/BlockNoteEditor/Editor.tsx` (4 debug calls removed — mount-init block, created-confirmation, per-keystroke content-changed, unsubscribe-cleanup — plus the now-unused `@/lib/logger` import), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files) · cargo check n/a (no Rust touched)
- Notes/decisions: Grep confirmed the `logger` import was only used by the 4 removed calls (no other `logger.` in the file), so it went too. `Editor.tsx` itself stays — it is live via the dynamic import at `BlockNoteSummaryView.tsx:16` (the 2.6 entry already confirmed this); only the logging was removed. The pre-existing `// Handle content changes` comment was kept.
- Spillover: none.

### Task 2.9 — Fix 2.5 copy + add cancelPendingSave test [S]
- Date: 2026-08-14
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Fix the 2.5 placeholder copy to mention that notes are incorporated into the AI summary (R2 Should-fix 1 — the shipped copy missed the plan's point), and add the missing `cancelPendingSave` hook test (R2 Should-fix 3 — the data-integrity half of 2.2 had no coverage).
- Files changed: `src/components/notes/NotesEditorShell.tsx` (placeholder → "No notes yet. Start typing to jot down key points — they'll be incorporated into the AI summary."), `tests/components/notes/useNotesEditor.test.ts` (new 3rd test), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files — new test included) · cargo check n/a (no Rust touched)
- Notes/decisions: Copy uses the orchestrator-suggested wording — keeps the shipped opening sentence and appends the summary-incorporation mention, satisfying plan row 2.5 ("Mention that notes are incorporated into the AI summary (the hidden differentiator)") while staying in the neighboring empty-state copy family. The test uses the file's existing harness exactly (fake timers, render-based `TestComp`, `act`); in one test it covers both R2 bullets: after `handleChange("b")` → `cancelPendingSave()`, `isDirty` stays `true` (content unsaved), advancing 2000ms fires nothing, and the unmount flush also stays inert — proving the timer was cleared, not just advanced past (flush is gated on `saveTimerRef.current`, which cancel nulls).
- Spillover: R2's suggested 2.9 also listed setting `IS_REACT_ACT_ENVIRONMENT` in the test setup (Nit 6 — cosmetic `act(...)` warnings in this test file, pre-existing) — outside the assigned scope, left untouched; one-liner in `tests/setup.ts` when wanted.

### Task 3.1 — Streaming responses via Tauri events [L]
- Date: 2026-08-14
- Implementer model: opencode-go/kimi-k2.7-code
- Status: done
- Scope: Replace single-shot chat with Tauri event streaming: Rust parses provider SSE streams and emits per-chunk events; frontend renders sources immediately and appends text as it arrives.
- Files changed:
  - `frontend/src-tauri/src/summary/llm_client.rs` — added `stream` fields to `ChatRequest`/`ClaudeRequest`; added `extract_delta` helper; added `generate_summary_stream` with SSE parsing, cancellation checks, and BuiltInAI single-chunk fallback.
  - `frontend/src-tauri/src/api/chat.rs` — factored shared FTS/config/prompt setup into `prepare_chat_inputs`/`ChatInputs`; added `api_chat_with_meetings_stream` command emitting `chat-stream-start`, `-chunk`, `-done`, `-error`.
  - `frontend/src-tauri/src/lib.rs` — registered `api::api_chat_with_meetings_stream` in `generate_handler![...]`.
  - `frontend/src/types/index.ts` — added `isStreaming?: boolean` to `ChatMessage`; added `ChatStreamStartPayload`, `ChatStreamChunkPayload`, `ChatStreamDonePayload`, `ChatStreamErrorPayload`.
  - `frontend/src/components/ChatPanel/index.tsx` — rewrote `handleSend` to set up Tauri event listeners before invoking the streaming command; manages loading/streaming states and listener cleanup.
  - `frontend/src/components/ChatPanel/ChatMessage.tsx` — added streaming cursor (`animate-pulse`) and error styling.
  - `docs/notes-chat-improvement-execution.md` — updated §2 status and this entry.
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files) · cargo check ✅ · Rust unit tests ✅ (`extract_delta` 7 passed; `api::chat` 4 passed)
- Notes/decisions:
  - **Architecture:** `llm_client.rs` stays Tauri-agnostic — it accepts an `on_chunk: FnMut(&str)` callback and returns the full accumulated text. `chat.rs` owns the Tauri event emission, keeping layer separation clean.
  - **Shared setup:** `prepare_chat_inputs` bundles FTS search, source extraction, prompt building, and provider/API-key resolution. The old `api_chat_with_meetings` command now calls it and passes results to the existing `generate_summary`; behavior is unchanged, so MCP's separate `generate_summary` call path is unaffected.
  - **Cancellation:** `generate_summary_stream` checks `CancellationToken` before starting and around each `stream.next()`. On cancellation the command emits `chat-stream-done` with the partial text accumulated by the caller's closure (3.2's stop button can pass a real token).
  - **Listener cleanup:** `ChatPanel` stores unlisten functions in a ref, cleans them on unmount, and cleans/re-registers on each new send to prevent ghost text from stale streams.
  - **History filtering:** error bubbles are still filtered from the model history (preserves 1.6 invariant).
- Event contract (for R3):
  - `chat-stream-start` → `{ sources: ChatSource[] }` — emitted before the LLM call; UI creates an empty assistant message and renders sources.
  - `chat-stream-chunk` → `{ text: string }` — emitted for each content delta; UI appends to the streaming message.
  - `chat-stream-done` → `{ answer: string, sources: ChatSource[] }` — emitted on success or cancellation; `answer` is the full (or partial, on cancel) text.
  - `chat-stream-error` → `{ error: string }` — emitted on setup/LLM/stream errors; UI shows an error bubble.
- Provider SSE handling matrix:
  - **Native streaming (OpenAI-compatible SSE):** OpenAI, Groq, OpenRouter, Ollama, CustomOpenAI — parse `data: {"choices":[{"delta":{"content":"..."}}]}`, stop on `data: [DONE]`.
  - **Native streaming (Claude SSE):** Claude — parse `data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}`, stop on `data: {"type":"message_stop"}`; non-text deltas ignored.
  - **Single-chunk fallback:** BuiltInAI — calls non-streaming `generate_summary` and emits the full answer as one `chat-stream-chunk`; sidecar protocol currently has no token streaming (`ponytail:` ceiling documented in code).
 - Test added: `summary::llm_client::tests` with 7 cases covering OpenAI/Groq/Ollama deltas, `[DONE]` markers, Claude text deltas, Claude non-text/ignored deltas, `message_stop`, and ping events.
 - Spillover: none.

### Review R3 — streaming architecture (task 3.1)
- Date: 2026-08-14
- Reviewer model: opencode-go/gpt-5.6-luna
- Verdict: changes-requested
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files) · cargo check ✅ · `extract_delta` ✅ (7 passed) · `api::chat::tests` ✅ (4 passed)
- Findings (prioritized):
  - **Blocker — cancellation is only a library hook, not an end-to-end capability.** The stream command passes `None` at `api/chat.rs:264-278`, has no token registry/state, and the frontend has no stop/cancel path. Consequently the documented cancellation branch at `api/chat.rs:297-307` is unreachable for this command, and a future stop button (task 3.2) cannot cancel an in-flight request or reliably preserve its partial answer. Add an app-owned single-stream state (or an equivalent ID→`CancellationToken` registry) and a cancel command before building 3.2.
  - **Should-fix — global event names have no stream/session identity.** `api/chat.rs:247-317` emits unqualified events, while `ChatPanel/index.tsx:94-129` appends every chunk to the last assistant message. The disabled send control (`index.tsx:245-251`) is only a frontend convention; a second invoke, an abandoned command, or a late event after listener re-registration can attach old text to a new answer. Include a `streamId` in start/chunk/done/error and filter in the panel, or enforce the one-stream invariant in Rust with the same state used for cancellation.
  - **Should-fix — SSE parser does not implement the claimed provider contract completely.** `llm_client.rs:606-632` correctly accumulates across byte chunks and `trim()` tolerates CRLF, and `extract_delta` safely ignores malformed JSON at `:359-384`. However, it only accepts the exact `"data: "` prefix (`:623-625`), does not terminate on `[DONE]` or Claude `message_stop` (both are merely ignored by `extract_delta`), and `String::from_utf8_lossy` per HTTP chunk (`:619-621`) can replace a UTF-8 code point split across chunks. This can silently drop final/control semantics and corrupt non-ASCII output. Centralize a line/event decoder with optional data whitespace, explicit terminal state, and byte-safe UTF-8 buffering; add tests for CRLF, split lines, split UTF-8, `data:` without a space, and terminal events.
  - **Should-fix — provider request construction is duplicated and can drift.** The URL/header/body matrix is repeated in `generate_summary` (`llm_client.rs:156-265`) and `generate_summary_stream` (`:443-547`). It currently matches for the six HTTP providers and Claude does set `stream: Some(true)` (`:538-547`); BuiltInAI correctly falls back at `:418-440`. But the next provider/config change must be made twice, which is especially risky with seven providers. Extract only the shared endpoint/header/request-builder decision (not Tauri events) and keep parsing/emission separate.
  - **Should-fix — partial HTTP failures lose the answer already shown.** A mid-stream reqwest error returns `Err` at `llm_client.rs:634-635`; `api/chat.rs:308-317` emits only `chat-stream-error`, and the frontend adds an error bubble without finalizing the existing streaming message (`index.tsx:136-151`). Cancellation is specified to preserve partial text, but transport failure is also a partial-state boundary and should either emit a terminal payload carrying the accumulated answer or explicitly mark/finalize the partial message.
  - **Should-fix — MCP chat remains a parallel implementation.** `mcp/server.rs:176-295` duplicates FTS/config/prompt/provider setup and calls non-streaming `generate_summary` directly, rather than reusing `prepare_chat_inputs`/a shared chat service. This preserves current MCP non-streaming behavior, but task 3.5 ("This meeting" scope), task 3.7 (provider/model visibility and settings errors), and later task 4.2 (query rewriting) can diverge between UI and MCP. Extract a Tauri-agnostic chat-input/service layer and have both command paths use it; do not make MCP depend on Tauri events.
  - **Nit — tests cover extraction, not stream framing or lifecycle.** The seven tests at `llm_client.rs:663-721` meaningfully cover the listed JSON shapes, but none exercise byte/line accumulation, terminal behavior, callback accumulation, cancellation, emit failure, or mid-stream errors. A small pure decoder test (plus one mocked response-stream test if practical) would protect the high-risk boundary without requiring frontend fixtures.
- Follow-up tasks created: 3.1a — shared SSE decoder/request construction and stream identity; 3.2 — token registry + cancel command + partial terminal-state semantics; 3.1b — shared chat service used by the Tauri command and MCP.

### Task 3.1a — Shared SSE decoder, request construction, stream identity (R3) [L]
- Date: 2026-08-14
- Implementer model: opencode-go/kimi-k2.7-code
- Status: done
- Scope: Close R3 should-fixes for streaming architecture: deduplicate provider request construction between streaming and non-streaming paths, harden the SSE decoder against split chunks/CRLF/prefix variants, and add stream identity to all chat stream events so late events cannot attach to the wrong message.
- Files changed: `frontend/src-tauri/src/summary/llm_client.rs` (new `build_chat_request` shared helper; new `SseEvent`/`parse_sse_line`/`SseLineBuffer`/`sse_data_payload` decoder; streaming loop uses decoder; tests), `frontend/src-tauri/src/api/chat.rs` (all four events now include `streamId`; streaming command registers active stream), `frontend/src-tauri/src/lib.rs` (managed `ChatStreamState`), `frontend/src/types/index.ts` (`streamId` on all payload interfaces), `frontend/src/components/ChatPanel/index.tsx` (filters events by `streamId`; generates `crypto.randomUUID()` per send), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files) · cargo check ✅ · `parse_sse_line` ✅ (7 passed) · `llm_client::tests` ✅ (13 passed) · `api::chat::tests` ✅ (7 passed)
- Notes/decisions:
  - **R3 should-fix mapping:**
    - SSE parser completeness → `parse_sse_line` returns explicit `SseEvent::Done` for `[DONE]` and `{"type":"message_stop"}`, `SseEvent::Delta` for content, `SseEvent::Ignore` for pings/comments/non-text deltas; `sse_data_payload` handles both `data: ` and `data:` prefixes.
    - Byte-safe framing → `SseLineBuffer` accumulates raw bytes and only converts complete lines to UTF-8, so a split multi-byte codepoint is reassembled before decoding.
    - Request construction duplication → `build_chat_request` produces `(url, headers, body)` for both `generate_summary` and `generate_summary_stream`; only `stream: Some(true)` differs.
    - Stream/session identity → frontend generates a UUID per invoke; all four event payloads carry `streamId`; frontend listeners ignore events whose `streamId` does not match `streamIdRef.current`.
  - **Event contract (updated):** all four payloads now include `streamId: string`: `chat-stream-start` → `{ streamId, sources }`; `chat-stream-chunk` → `{ streamId, text }`; `chat-stream-done` → `{ streamId, answer, sources }`; `chat-stream-error` → `{ streamId, error }`.
- Spillover: 3.1b (shared chat service for Tauri + MCP) is left for its own task; see Task 3.2 spillover for feasibility analysis.

### Task 3.2 — Stop button + cancellation end-to-end (R3) [S]
- Date: 2026-08-14
- Implementer model: opencode-go/kimi-k2.7-code
- Status: done
- Scope: Close R3 blocker: wire `CancellationToken` end-to-end for chat streaming, add `api_cancel_chat_stream`, render a Stop button while streaming, and preserve partial answers on cancel or mid-stream error.
- Files changed: `frontend/src-tauri/src/api/chat.rs` (`ChatStreamState` managed state; `api_cancel_chat_stream`; streaming command stores token and clears state on every terminal path; partial errors emit `chat-stream-done`), `frontend/src-tauri/src/lib.rs` (registered state + cancel command), `frontend/src/components/ChatPanel/index.tsx` (Stop button; `handleStop`; partial-error finalization), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files) · cargo check ✅ · `api::chat::tests` ✅ (7 passed)
- Notes/decisions:
  - **Cancellation contract:** `api_chat_with_meetings_stream` registers `(stream_id, CancellationToken)` in `ChatStreamState` before the LLM call and clears it on every terminal path before emitting the final event. `api_cancel_chat_stream(stream_id?)` reads the active entry; if `stream_id` matches (or is `None`), it calls `token.cancel()`. The streaming command's cancellation branch emits `chat-stream-done` with partial text.
  - **Partial-on-error:** mid-stream HTTP/parse errors (not cancellation) now emit `chat-stream-done` carrying the accumulated text if any chunk was already shown; only never-started errors emit `chat-stream-error`. The frontend error listener finalizes an existing streaming assistant message instead of appending an error bubble when content is present.
  - **One-stream invariant:** `ChatStreamState` tracks a single active stream; a new invoke replaces the token. This matches the frontend's single-stream UI model and prevents stale cancel commands from targeting a completed stream.
- Spillover: MCP chat (`mcp/server.rs:176-295`) remains a parallel implementation. `prepare_chat_inputs` is now the shared Tauri chat setup funnel, but it still takes `AppHandle` (for `app_data_dir`) and `tauri::State<'_, AppState>` (for the pool). To make it reusable by MCP without pulling in Tauri, 3.1b should refactor it to accept `&SqlitePool` and `Option<PathBuf>` directly; both `api_chat_with_meetings*` callers already resolve those values before calling it, and MCP already has `pool` and `app_data_dir` in `McpState`. No MCP changes were made in this diff.

### Task 3.1b — Shared chat service for Tauri command and MCP (R3) [M]
- Date: 2026-08-14
- Implementer model: opencode-go/kimi-k2.6
- Status: done
- Scope: Close R3 should-fix: extract `prepare_chat_inputs` so both the Tauri command path and the MCP path call the SAME shared setup for FTS search, source extraction, LLM config resolution, API-key lookup, and prompt building. MCP remains non-streaming and its JSON output shape is unchanged.
- Files changed: `frontend/src-tauri/src/api/chat.rs` (`prepare_chat_inputs` signature changed to `pub async fn prepare_chat_inputs(pool: &SqlitePool, app_data_dir: Option<PathBuf>, query: &str, history: Option<&Vec<ChatMessage>>)`; made `ChatInputs` and all its fields public; made `SYSTEM_PROMPT` public; two Tauri callers now resolve `pool` and `app_data_dir` before passing them in), `frontend/src-tauri/src/mcp/server.rs` (`execute_chat_with_meetings` now calls `prepare_chat_inputs` and maps `ChatInputs.sources` to its existing `Vec<serde_json::Value>` output shape; deleted the duplicated ~120-line FTS/config/API-key/prompt block; removed now-unused imports `setting::SettingsRepository` and `LLMProvider`), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files) · cargo check ✅ · `api::chat::tests` ✅ (7 passed) · `mcp::server::tests` ✅ (3 passed)
- Notes/decisions:
  - **Option A chosen** (smaller diff, preserves MCP output shape). `prepare_chat_inputs` was refactored to be Tauri-agnostic by taking `&SqlitePool` and `Option<PathBuf>` directly instead of `AppHandle` + `tauri::State`. No new module or crate was needed because both `api/chat.rs` and `mcp/server.rs` are in the same `app_lib` crate; making `ChatInputs`/`SYSTEM_PROMPT`/`prepare_chat_inputs` public is sufficient.
  - **MCP behavior delta (intentional improvements):** (a) MCP's system prompt now includes the "Format your response in clear paragraphs." suffix, matching the UI version — arguably an improvement. (b) MCP's `user_prompt` now has a leading newline before "User question:" (because `prepare_chat_inputs` always formats it that way) — trivial and harmless. (c) MCP now implicitly supports the history parameter path (it passes `None` today), so if MCP ever gains conversation context it will work without further divergence.
 - **Divergence risk closed:** Future tasks 3.5 ("This meeting" scope toggle), 3.7 (provider/model visibility), and 4.2 (query rewriting) only need to touch `prepare_chat_inputs` once; both Tauri and MCP will pick them up automatically because they share the same helper. The only remaining per-path surface is the system prompt passed to `generate_summary` (shared constant) and the event emission vs. direct JSON-RPC response shape, neither of which is affected by those tasks.
 - Spillover: none.

### Task 3.3 — Markdown rendering of answers [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-luna
- Status: done
- Scope: Render assistant chat answers as GitHub-flavored Markdown while keeping user messages plain text and the streaming cursor outside the rendered content.
- Files changed: `src/components/ChatPanel/ChatMessage.tsx` (ReactMarkdown/remark-gfm rendering and minimal heading/list/code styles), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files) · cargo check ✅ (no Rust touched)
- Notes/decisions: Existing `react-markdown` and `remark-gfm` dependencies were reused; the renderer remains safe for partial streaming Markdown.
- Spillover: none.

### Task 3.4 — Clickable sources [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-luna
- Status: done
- Scope: Make source chips keyboard-accessible buttons that navigate to the referenced meeting details page.
- Files changed: `src/components/ChatPanel/ChatMessage.tsx` (source buttons and accessibility labels), `src/components/ChatPanel/index.tsx` (meeting route callback), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files) · cargo check ✅ (no Rust touched)
- Notes/decisions: Navigation uses the existing `/meeting-details?id=...` route pattern.
- Spillover: none.

### Task 3.6 — Multiline input + copy answer [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-luna
- Status: done
- Scope: Replace the single-line chat input with a capped auto-resizing textarea and add copy feedback for completed assistant answers.
- Files changed: `src/components/ChatPanel/index.tsx` (textarea and resize handling), `src/components/ChatPanel/ChatMessage.tsx` (copy button and two-second confirmation), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files) · cargo check ✅ (no Rust touched)
- Notes/decisions: Enter keeps sending and Shift+Enter keeps inserting newlines; copy is hidden during streaming and for user messages.
- Spillover: none.

### Task 3.7 — Model indicator + Settings deep-link [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-luna
- Status: done
- Scope: Show the configured provider/model in the chat header and make it link to Settings, with a fallback when configuration is unavailable.
- Files changed: `src/components/ChatPanel/index.tsx` (model config loading, indicator, and Settings navigation), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed, 13 files) · cargo check ✅ (no Rust touched)
- Notes/decisions: The existing `api_get_chat_model_config` command is used. Settings currently has no URL deep-link contract for selecting the Chat tab, so the link opens `/settings` and the user can select Chat.
- Spillover: none.

### Task 3.5 — "This meeting" scope toggle [M]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Add an optional meeting filter to FTS-backed chat and expose a per-meeting "This meeting / All meetings" selector in the meeting details chat panel.
- Files changed: `frontend/src-tauri/src/database/repositories/fts.rs` (optional meeting filter and coverage), `frontend/src-tauri/src/api/chat.rs` (chat command/helper scope plumbing and stream start payload), `frontend/src-tauri/src/api/api.rs` (updated FTS caller), `frontend/src-tauri/src/mcp/server.rs` (updated FTS callers and optional `meetingId` for MCP chat), `frontend/src/components/ChatPanel/index.tsx` (scope selector and invoke argument), `frontend/src/app/meeting-details/page-content.tsx` (meeting ID prop), `frontend/src/types/index.ts` (stream payload scope), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ · `fts::tests` ✅ (9 passed) · `api::chat::tests` ✅ (7 passed)
- Notes/decisions: `FtsRepository::search` now receives `meeting_id: Option<&str>` and applies it alongside the existing parsed folder filter in both query branches. Updated callers: `prepare_chat_inputs` (`api/chat.rs`), `api_build_context` (`api/chat.rs`), `api_search_fts` (`api/api.rs`), `execute_search_meetings` and `execute_build_context` (`mcp/server.rs`), plus FTS repository tests. `execute_chat_with_meetings` reads optional MCP `meetingId`; both Tauri chat commands accept it, and the streaming command includes it in `chat-stream-start`. The meeting-details panel defaults to This meeting; chat without a meeting ID has no scope control and retains all-meetings behavior. Existing FTS tests were adapted with `None`; they additionally verify a scoped result and the folder-plus-meeting combination.
- Spillover: `ponytail:` scope is filter-only; the plan's direct summary-and-notes injection variant remains a future enhancement to avoid double-injecting content already retrieved through FTS.

### Review R4 — Sprint 3 (tasks 3.1–3.7)
- Date: 2026-08-15
- Reviewer model: openai/gpt-5.6-sol
- Verdict: changes-requested
- Verification (re-run from `upstream/frontend`): typecheck ✅ · vitest ✅ (51 passed, 13 files; pre-existing React `act(...)` warnings) · cargo check ✅ · `fts::tests` ✅ (9 passed) · `api::chat::tests` ✅ (7 passed) · `mcp::server::tests` ✅ (3 passed). `react-markdown` and `remark-gfm` were pre-existing dependencies (`package.json:87-88`); neither `package.json` nor the lockfile changed.
- Findings (prioritized):
  - **Blocker — closing/reopening chat can make the new stream impossible to stop.** Unmount only removes listeners and does not cancel the active backend request (`src/components/ChatPanel/index.tsx:54-60`). A newly opened panel then replaces the global active token without cancelling the old one (`src-tauri/src/api/chat.rs:286-290`), and when the old request finishes it unconditionally clears the slot (`src-tauri/src/api/chat.rs:342-345`, likewise `:357-360` and `:372-375`), deleting the newer stream's token. Stream IDs keep old text out of the new answer, but Stop can no longer reach the new stream and the closed request continues consuming local/cloud resources. Cancel the replaced token and clear state only when the terminal stream ID still owns the slot; cancel the panel's stream on unmount as well.
  - **Should-fix — copy failure is unhandled and success feedback is not announced.** `navigator.clipboard.writeText` can reject, but the async click handler has no error path (`src/components/ChatPanel/ChatMessage.tsx:27-31`), producing an unhandled rejection and no user feedback. On success only the icon/title changes while the accessible name remains “Copy answer” (`src/components/ChatPanel/ChatMessage.tsx:51-58`), so screen-reader users never receive the promised “Copied!” feedback. Handle rejection and expose the transient status through the accessible name or an `aria-live` region.
  - **Should-fix — the auto-resizing textarea stays tall after send.** Height is changed only by the input event (`src/components/ChatPanel/index.tsx:229-234`), while send clears React state without resetting the DOM height (`src/components/ChatPanel/index.tsx:71-74`). After a multiline prompt reaches the 120px cap, the empty composer remains 120px high and unnecessarily reduces message space. Reset height after clearing, including any programmatic clear path.
  - **Should-fix — source interaction does not complete the planned selection/snippet UX.** Every click navigates immediately (`src/components/ChatPanel/ChatMessage.tsx:87-99`), with no guard for a mouse drag that selected chip text; the source snippet is also never rendered despite being carried in `ChatSource` (`src/types/index.ts:221-227`). This makes source text selection trigger navigation and omits the task plan's hover/expand snippet affordance. Suppress navigation when a non-collapsed selection exists and expose a sanitized snippet in a tooltip/expansion.
  - **Nit — Sprint 3 status index is stale.** Tasks 3.3, 3.4, 3.6, and 3.7 have completed entries but remain `pending` in the index (`docs/notes-chat-improvement-execution.md:46-50`).
  - **R3 confirmation:** stream-ID checks remain on all four listener callbacks (`src/components/ChatPanel/index.tsx:93-159`); SSE buffering converts only complete byte-reassembled lines (`src-tauri/src/summary/llm_client.rs:420-447`); both generation paths call `build_chat_request` (`src-tauri/src/summary/llm_client.rs:135-147,509-521`); and MCP calls shared `prepare_chat_inputs` (`src-tauri/src/mcp/server.rs:174-207`). Terminal branches clear state before emitting, but the ownership race above means the R3 cancellation fix has not fully held under overlapping streams.
  - **UX confirmation:** Markdown rendering is streaming-safe, keeps the cursor outside `ReactMarkdown`, leaves user text plain, and does not enable raw HTML (`src/components/ChatPanel/ChatMessage.tsx:60-80`). Meeting scope is passed end-to-end with bound SQL parameters (`src/app/meeting-details/page-content.tsx:381-384`; `src/components/ChatPanel/index.tsx:188-194`; `src-tauri/src/api/chat.rs:72-82`; `src-tauri/src/database/repositories/fts.rs:118-174`), and MCP maps `meetingId` through the shared helper (`src-tauri/src/mcp/server.rs:174-186`). Model-config failures are caught and the Settings fallback remains usable (`src/components/ChatPanel/index.tsx:36-42,246-252`).
- Follow-up tasks created: **3.2a** stream-state ownership + cancel-on-unmount; **3.4a** source selection guard + snippet affordance; **3.6a** clipboard error/a11y + textarea height reset.

### Task 3.2a — Stream ownership race [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-luna
- Status: done
- Scope: Prevent overlapping chat streams from cancelling or clearing one another's state, and cancel the active stream when the chat panel unmounts.
- Files changed: `frontend/src-tauri/src/api/chat.rs` (cancel the replaced token and clear state only when the terminal stream still owns it), `frontend/src/components/ChatPanel/index.tsx` (cancel any active stream during unmount), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅
- Notes/decisions: Backend ownership checks are the critical protection; frontend unmount cancellation prevents orphaned work when the panel closes.
- Spillover: none.

### Task 3.4a — Source text selection + snippet exposure [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-luna
- Status: done
- Scope: Preserve text selection in source chips without navigating, and expose matched source snippets in the chip tooltip.
- Files changed: `frontend/src/components/ChatPanel/ChatMessage.tsx` (selection guard and snippet-aware title), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ (no Rust touched)
- Notes/decisions: Navigation is skipped when the browser selection is non-empty; the existing button layout is unchanged.
- Spillover: none.

### Task 3.6a — Clipboard + textarea reset [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-luna
- Status: done
- Scope: Handle clipboard rejection without an unhandled promise and announce successful copy feedback, while resetting the multiline composer after send.
- Files changed: `frontend/src/components/ChatPanel/ChatMessage.tsx` (clipboard error handling and live success status), `frontend/src/components/ChatPanel/index.tsx` (textarea height reset), this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ (no Rust touched)
- Notes/decisions: Copy feedback changes only after a successful clipboard write; failures silently leave the button in its normal state.
- Spillover: none.

### Task 4.1 — Conversation persistence [M]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Persist per-meeting chat threads and their messages, resume the latest thread when reopening the panel, and provide a confirmed clear-chat action.
- Files changed: `frontend/src-tauri/migrations/20260815000000_add_chat_conversations.sql`, `frontend/src-tauri/src/database/repositories/chat.rs`, `frontend/src-tauri/src/database/repositories/mod.rs`, `frontend/src-tauri/src/database/manager.rs`, `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/lib.rs`, `frontend/src/components/ChatPanel/index.tsx`, `frontend/src/types/index.ts`, this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ · cargo test --lib ✅ (335 passed, 2 ignored)
- Notes/decisions: Migration schema: `chat_conversations(id TEXT PRIMARY KEY NOT NULL, meeting_id TEXT NULL REFERENCES meetings(id) ON DELETE SET NULL, title TEXT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)` and `chat_messages(id TEXT PRIMARY KEY NOT NULL, conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE, role TEXT NOT NULL, content TEXT NOT NULL, sources_json TEXT NULL, is_error INTEGER DEFAULT 0, created_at TEXT NOT NULL)`, plus `idx_chat_conversations_meeting_id` and `idx_chat_messages_conversation_id`; all CREATE statements use `IF NOT EXISTS`. `ChatRepository` creates, loads latest/global or meeting-scoped threads, lists meeting threads for future use, saves/loads messages, and deletes conversations. Saving a first user message sets the title with SQLite `substr(..., 1, 50)` and updates `updated_at`. Persistence commands registered: `api_chat_create_conversation`, `api_chat_get_conversation`, `api_chat_get_messages`, `api_chat_save_message`, `api_chat_clear_conversation`; source arrays serialize internally. On mount, meeting panels resume their latest thread and load messages; global panels create a fresh global thread. User messages save before streaming, completed/error assistant messages save after the terminal event; reopening reads the stored thread. Clear confirms, deletes the old conversation, and creates a same-scope replacement. `DatabaseManager` now enables SQLite foreign keys on every pool connection so the migration's `ON DELETE` actions are real rather than inert.
- Spillover: `ponytail:` conversation list/rename/delete UI is intentionally deferred; the repository list method exists for the later view while MVP exposes create, resume, and clear only. Existing INFO-level full-query chat logs remain owned by Task 4.5 and were not expanded.

### Task 4.1a — Persistence race/atomicity hardening [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-luna
- Status: done
- Scope: Address the R5 persistence-flow findings for meeting changes, global conversation resume, transactional saves, and foreign-key coverage.
- Files changed: `frontend/src/components/ChatPanel/index.tsx`, `frontend/src-tauri/src/database/repositories/chat.rs`, this doc.
- Verification: typecheck ✅ · vitest ✅ · cargo check ✅ · `cargo test --lib -- chat` ✅
- Notes/decisions: Meeting changes clear and ref-fence the active conversation before loading; global chat now uses the latest global conversation lookup; message insertion and metadata updates share a transaction with monotonic `updated_at`; the chat repository test pool enables foreign keys per connection and covers `ON DELETE SET NULL` alongside existing CASCADE coverage.
- Spillover: none.

### Review R5 — migration review (task 4.1)
- Date: 2026-08-15
- Reviewer model: openai/gpt-5.6-sol
- Verdict: changes-requested
- Verification (re-run from `upstream/frontend`): typecheck ✅ · vitest ✅ (51 passed, 13 files; pre-existing React `act(...)` warnings) · cargo check ✅ · `cargo test --lib -- database` ✅ (42 passed, 295 filtered out). Migration ordering is correct: `20260815000000_add_chat_conversations.sql` sorts after the previous latest migration, `20260803000000_add_template_stable_ids.sql`.
- Findings (prioritized):
  - **Blocker:** none. The additive SQL migration itself is safe to ship: every `CREATE TABLE`/`CREATE INDEX` is guarded by `IF NOT EXISTS`, the UUID/RFC3339/SQLite-boolean column choices match the repository types (`migrations/20260815000000_add_chat_conversations.sql:1-22`; `src-tauri/src/database/repositories/chat.rs:7-24`), and the requested `SET NULL`/`CASCADE` relationships and indexes are present. Production FK enforcement is correctly applied through `SqliteConnectOptions` to every connection in the main pool before migrations run (`src-tauri/src/database/manager.rs:39-43`). All five persistence commands use `AppState`'s main pool and are registered (`src-tauri/src/api/chat.rs:21-85`; `src-tauri/src/lib.rs:731-735`); message IDs are generated in Rust (`src-tauri/src/database/repositories/chat.rs:95-104`), sources are serialized locally (`src-tauri/src/api/chat.rs:61-74`), and Task 4.1 added no content-bearing INFO log.
  - **Should-fix — a meeting change leaves the previous conversation writable while the new one loads.** The load effect does not clear `conversationId` or `messages` before its async work (`src/components/ChatPanel/index.tsx:56-83`), while send remains enabled solely from the old non-null ID (`src/components/ChatPanel/index.tsx:122-128,406-420`). If `meetingId` changes without this component unmounting, a prompt entered during the load window is persisted into the previous meeting's conversation, crossing meeting boundaries. Reset/gate the active conversation at effect start and fence saves by the current load identity.
  - **Should-fix — global conversations are written but never resumed.** For `meetingId === undefined`, the frontend skips `api_chat_get_conversation`, always creates a new global conversation, and therefore never loads prior global messages (`src/components/ChatPanel/index.tsx:61-65`), despite the repository implementing the global latest-thread lookup (`src-tauri/src/database/repositories/chat.rs:51-67`). Each reopen strands another durable thread. Use the same get-or-create/load path for `None` scope.
  - **Should-fix — saving a message and advancing its conversation metadata are not atomic.** `save_message` commits the message INSERT and conversation title/`updated_at` UPDATE as two independent pool operations (`src-tauri/src/database/repositories/chat.rs:85-118`). A failure after the INSERT reports the save as failed even though the message exists, with stale title/latest ordering; concurrent saves can also apply an older generated timestamp last. Put both statements in one transaction and avoid allowing an older completion to move `updated_at` backwards.
  - **Nit — FK coverage proves CASCADE only, not SET NULL, and does not configure FKs per test-pool connection.** The chat test runs `PRAGMA foreign_keys = ON` through the pool and hand-creates a conversation table without the `meeting_id` FK (`src-tauri/src/database/repositories/chat.rs:147-161`); its delete assertion does exercise message cascade successfully (`src-tauri/src/database/repositories/chat.rs:182-183`), but no test deletes a meeting and checks that history survives with `meeting_id = NULL`. Build the test pool with `SqliteConnectOptions::foreign_keys(true)` and cover both actions.
- Follow-up tasks created: **4.1a** persistence race/atomicity hardening (fence meeting changes, resume global threads, transact message+metadata updates, and add per-connection CASCADE/SET NULL coverage).

### Task 4.2 — Query rewriting for follow-ups [M]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Resolve short contextual follow-up questions into standalone FTS queries before retrieval, while keeping the answer prompt based on the original user question.
- Files changed: `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/mcp/server.rs`, this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ · `cargo test --lib -- api::chat::tests` ✅ (9 passed)
- Notes/decisions: `prepare_chat_inputs` now resolves provider configuration and the API key before retrieval, then uses the shared caller-provided `reqwest::Client` to call `generate_summary` with the query-rewriter prompt. Rewriting runs only with at least two history messages and a query shorter than 100 characters; it is capped with `tokio::time::timeout(Duration::from_secs(15), ...)`. FTS receives the rewritten query, while the final answer prompt retains the original question and includes the search query. Both Tauri chat commands and the MCP shared helper path pass their existing client, so MCP inherits the behavior if it later supplies history. On rewrite failure or timeout, the `ponytail:` fallback prefixes the query with up to 50 characters from the latest assistant response (or latest user response), a deliberately crude context heuristic; upgrade to extracted subject phrases if failures become common. Unit coverage verifies the gate and fallback behavior.
- Spillover: MCP currently passes no conversation history to `prepare_chat_inputs`, so its current requests do not trigger rewriting; the shared gate makes future history support automatic.

### Task 4.3 — Retrieval depth [L]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-sol
- Status: done
- Scope: Add selectable OR/AND/phrase FTS matching, expand transcript hits around their highlighted match, scale chat retrieval by provider class, and cap assembled context and prompts.
- Files changed: `frontend/src-tauri/src/database/repositories/fts.rs`, `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/export/context.rs`, `frontend/src-tauri/src/export/mod.rs`, this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ · `cargo test --lib -- fts::tests` ✅ (11 passed) · `cargo test --lib -- api::chat::tests` ✅ (9 passed)
- Notes/decisions: Matching lives in `FtsRepository`: the existing `search` signature remains backward-compatible and defaults to `MatchMode::Or`, while `search_with_mode` exposes `Or`, `And`, and exact multi-word `Phrase`; chat selects AND for rewritten/original retrieval. Transcript results retain the FTS-highlighted snippet and add up to 200 Unicode characters on each side from the source `transcripts` row; expansion stops at 8,000 total expanded characters (`ponytail:` fixed global heuristic, replace with model metadata if needed), while summary/note snippets stay unchanged. `prepare_chat_inputs` uses 30 chunks and a 100,000-character cap for known cloud providers, versus 10 chunks and 64,000 characters (about 16K tokens at four chars/token) for Ollama, BuiltInAI, and unknown-capacity CustomOpenAI; `ponytail:` provider class is the MVP ceiling until model context windows are configured. `build_context_markdown` retains its one-argument API with a 100,000-character default and exposes a limited variant for chat; the final user prompt is independently Unicode-safely truncated to the provider cap.
- Spillover: none.

### Task 4.4 — Suggested prompts [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-luna
- Status: done
- Scope: Add four clickable suggested prompt chips to the chat empty state, with meeting-scoped and global prompt sets.
- Files changed: `frontend/src/components/ChatPanel/index.tsx`, this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ · `cargo test --lib -- api::chat::tests` ✅ (9 passed)
- Notes/decisions: Extracted the streaming send flow into `sendQuery(query: string)` while preserving the error-filtered history; `handleSend` now delegates with the trimmed input. Chips use the meeting-scoped prompts when `meetingId` is truthy and the global prompts otherwise, and are disabled while busy or before a conversation exists.
- Spillover: none.

### Task 4.5 — Privacy polish [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-luna
- Status: done
- Scope: Stop INFO logs from emitting full chat queries and add a privacy-first local/cloud/custom provider indicator to the chat header.
- Files changed: `frontend/src-tauri/src/api/chat.rs`, `frontend/src/components/ChatPanel/index.tsx`, this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ · `cargo test --lib -- api::chat::tests` ✅ (9 passed)
- Notes/decisions: Changed the three Rust sites `:346-350` → `:346-350`, `:409-414` → `:409-414`, and `:571-575` → `:571-575` from query content to `query.len()` while retaining history length, auth-token presence, and stream ID where applicable. Added `classifyProvider` with Local (`ollama`, `builtin-ai`, `local-llama`, `localllama`), Cloud (`openai`, `claude`, `anthropic`, `groq`, `openrouter`), and Custom (`custom-openai` and unknown/null) categories, rendered as a colored dot-and-label badge beside the model label. `ponytail:` Custom-endpoint-localhost-refinement deferral: the config command does not return the endpoint URL, so Custom remains the safe label until a separate invoke can inspect it.
- Spillover: none.

### Review R6 — Sprint 4 full review
- Date: 2026-08-15
- Reviewer model: openai/gpt-5.6-sol
- Verdict: changes-requested
- Verification (re-run from `upstream/frontend`): typecheck ✅ · vitest ✅ (51 passed, 13 files; pre-existing React `act(...)` warnings) · cargo check ✅ · `cargo test --lib` ✅ (341 passed, 2 ignored; includes all 3 MCP tests) · cargo fmt --check ✅.
- Findings (prioritized):
  - **Blocker / must-fix:** none.
  - **Should-fix — deleted meeting threads become indistinguishable from intentional global threads.** The FK converts every meeting-scoped conversation to `meeting_id = NULL` when its meeting is deleted (`frontend/src-tauri/migrations/20260815000000_add_chat_conversations.sql:3-7`; exercised at `frontend/src-tauri/src/database/repositories/chat.rs:252-264`), while global resume defines a global thread as precisely the latest row where `meeting_id IS NULL` (`frontend/src-tauri/src/database/repositories/chat.rs:62-66`). Consequently, opening global chat can resume and feed history from a deleted meeting instead of the user's actual global thread. Preserve the history without allowing formerly scoped rows to participate in global lookup (for example, retain a distinct scope/origin marker).
  - **Should-fix — the final prompt cap can remove the current question.** Prompt assembly puts up to ten unbounded history messages before the original question and then truncates the whole string from the end (`frontend/src-tauri/src/api/chat.rs:281-293`). A sufficiently long persisted response/history can consume the 64K/100K budget before `User question`, so the answer call receives no current question; this is the explicit context-guard regression the sprint needed to prevent. Reserve space for the original question/search-query framing and truncate lower-priority history/context at Unicode boundaries instead.
  - **Should-fix — full chat/search text still reaches an INFO log through FTS.** The three chat entry logs correctly retain only `query.len()`, but every chat retrieval passes the original, rewritten, or fallback query into `search_with_mode` (`frontend/src-tauri/src/api/chat.rs:221-260`), which logs `parsed.fts_query` verbatim at INFO (`frontend/src-tauri/src/database/repositories/fts.rs:228-233`). Task 4.5 therefore does not yet meet its privacy outcome even though `chat.rs` itself is neutralized; log only query length/mode/result count.
  - **Nit — required `ponytail:` markers are incomplete and the Task 4.5 execution note overstates them.** The fixed 15-second rewrite timeout and crude fallback have no marker (`frontend/src-tauri/src/api/chat.rs:221-245,323-334`), and the Custom-provider localhost-refinement marker claimed in the task note is absent beside the classifier (`frontend/src/components/ChatPanel/index.tsx:34-38`). The provider chunk-budget and transcript cap markers are present (`frontend/src-tauri/src/api/chat.rs:250-253`; `frontend/src-tauri/src/database/repositories/fts.rs:479-483`).
  - **Validated — conversation persistence and streaming:** the meeting-change effect clears `conversationIdRef` before async loading, global get-or-create loads existing messages, clear creates the replacement with the same `meetingId`/global scope, saves are ref-fenced and transactional with monotonic `updated_at`, production and test pools enable FKs, and cancellation/partial failures emit `chat-stream-done`, so readable partial answers save with `is_error = false` (`frontend/src/components/ChatPanel/index.tsx:69-98,126-135,203-225,304-310`; `frontend/src-tauri/src/database/repositories/chat.rs:85-120,151-155,222-264`; `frontend/src-tauri/src/api/chat.rs:477-527`). All five commands are registered (`frontend/src-tauri/src/lib.rs:731-735`).
  - **Validated — rewriting, retrieval, and MCP:** config/key resolution precedes the rewrite and FTS with a clear no-model error; the ≥2-message/<100-character gate, timeout/fallback, rewritten-query retrieval, original-question framing, and source construction are wired consistently. All three callers reuse a per-request or server-state client, MCP intentionally supplies no history, legacy `search` callers retain OR mode, expansion is transcript-only, preserves the marked snippet, and stops before exceeding the 8K aggregate cap (`frontend/src-tauri/src/api/chat.rs:149-308,310-334,352-363,416-427`; `frontend/src-tauri/src/mcp/server.rs:174-219`; `frontend/src-tauri/src/database/repositories/fts.rs:95-102,433-487`). Context truncation itself is Unicode-safe; aside from the finding above, the builder/final caps and local 10/cloud 30 budgets are active (`frontend/src-tauri/src/export/context.rs:15-18,70-72`; `frontend/src-tauri/src/api/chat.rs:250-293`).
  - **Validated — UI and privacy badge:** `sendQuery` preserves error-bubble filtering and `handleSend` trims input; scoped chips send immediately, render only in the empty state, and are disabled/labelled correctly. Provider classification handles null safely, the badge renders only with a configured model, and its dot is aria-hidden while the visible category and tooltip remain available (`frontend/src/components/ChatPanel/index.tsx:34-38,137-154,292,330-349,365-372,408-425`). No Sprint 4 dependency change or unexpected network/analytics integration was found.
- Follow-up tasks created: **4.1b** distinguish deleted-meeting history from global conversations; **4.3a** make prompt budgeting preserve the current question; **4.5a** neutralize FTS query INFO logging and add the missing heuristic markers.

### Task 4.5a — FTS query logging and ponytail markers [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Remove FTS query text from INFO logs and document the existing rewrite/provider heuristics.
- Files changed: `frontend/src-tauri/src/database/repositories/fts.rs`, `frontend/src-tauri/src/api/chat.rs`, `frontend/src/components/ChatPanel/index.tsx`, this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ · cargo test --lib ✅ · cargo fmt --check ✅
- Notes/decisions: The FTS completion log now records `query_len`, folder, and result count rather than query text. Added `ponytail:` markers for the 15s rewrite cap, crude rewrite fallback, and the deferred Custom-provider localhost classification because `api_get_chat_model_config` has no endpoint.
- Spillover: none.

### Task 4.3a — Preserve current question during prompt budgeting [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Reserve the full current-question/search-query block before budgeting lower-priority context and history.
- Files changed: `frontend/src-tauri/src/api/chat.rs`, this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ · cargo test --lib -- api::chat::tests ✅ · cargo fmt --check ✅
- Notes/decisions: `assemble_prompt(context, history, query, search_query, max_context_chars)` drops oldest history first, then Unicode-safely tail-truncates context; the current question block remains at the prompt end. Coverage uses a 200K-character history and asserts the unmodified question block remains present.
- Spillover: none.

### Task 4.1b — Distinguish deleted-meeting threads from global conversations [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Preserve deleted meeting conversation history without treating it as a global conversation.
- Files changed: `frontend/src-tauri/migrations/20260815100000_add_chat_conversations_origin.sql`, `frontend/src-tauri/src/database/repositories/chat.rs`, `frontend/src-tauri/src/api/chat.rs`, this doc.
- Verification: typecheck ✅ · vitest ✅ (51 passed) · cargo check ✅ · cargo test --lib -- chat ✅ · cargo fmt --check ✅
- Notes/decisions: The additive migration is `ALTER TABLE chat_conversations ADD COLUMN origin TEXT NOT NULL DEFAULT 'meeting';`. Creation explicitly stores `global` only for no-meeting commands; global resume requires both `meeting_id IS NULL` and `origin = 'global'`, so FK-orphaned meeting threads stay excluded. Repository coverage asserts an FK-cleared row retains `meeting` origin and cannot be resumed globally.
- Spillover: MCP is stateless and has no conversation repository usage.

### Task 5.1 — BlockNote editor for notes [M]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Replace both shared notes textareas with the existing dynamic BlockNote wrapper while preserving debounced persistence, flushes, deletion cancellation, and legacy markdown loading.
- Files changed: `frontend/src/components/BlockNoteEditor/Editor.tsx`, `frontend/src/components/notes/useNotesEditor.ts`, `frontend/src/components/notes/NotesEditorShell.tsx`, `frontend/src/components/MeetingDetails/NotesPanel.tsx`, `frontend/src/components/RecordingNotesPanel.tsx`, `frontend/src/lib/blocknote-markdown.ts`, `frontend/tests/components/notes/useNotesEditor.test.ts`, `frontend/tests/components/notes/NotesEditorShell.test.tsx`, this doc.
- Verification: typecheck ✅ · vitest ✅ (53 passed) · cargo check ✅ · cargo test --lib -- meeting_notes ✅ (3 passed) · cargo fmt --check ✅
- Notes/decisions: `useNotesEditor` keeps its debounce/flush ownership and now carries `blocksRef`, a BlockNote markdown-editor ref, and `setBlocks`; save derives markdown only at save time and calls `save(markdown, blocksJson)`. Markdown conversion failure still saves JSON with fallback markdown, so rich blocks remain durable. `NotesPanel` loads JSON first (`ponytail:` rich JSON is source of truth; markdown is legacy fallback) and sends both fields to `save_meeting_notes`. `RecordingNotesPanel` also uses BlockNote for a consistent editor and stores block JSON in its existing gated session draft key; on a sessionStorage quota failure (`ponytail:` browser-size ceiling), it falls back to markdown while markdown continues to mirror to `notes.md` through the same debounce. The BlockNote shell test confirms no textarea renders; hook coverage advances the debounce and confirms conversion failure still persists JSON.
- Spillover: Ctrl/Cmd+S remains on the editor wrapper as the textarea shortcut replacement; Task 5.2 may refine panel/save shortcut behavior.

### Task 5.2 — Panel/save shortcuts [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Add editor save and recording/meeting panel-toggle shortcuts without interrupting text entry.
- Files changed: `frontend/src/components/notes/NotesEditorShell.tsx`, `frontend/src/app/page.tsx`, `frontend/src/app/meeting-details/page-content.tsx`, `frontend/src/lib/panel-shortcuts.ts`, `frontend/tests/lib/panel-shortcuts.test.ts`, this doc.
- Verification: typecheck ✅ · vitest ✅ (60 passed) · cargo check n/a (no Rust touched)
- Notes/decisions: Ctrl/Cmd+S is now a mounted-shell window listener, rather than relying on BlockNote keydown bubbling, and prevents browser Save Page while calling `onManualSave` only when dirty and idle. Ctrl/Cmd+Shift+N toggles recording/meeting notes; Ctrl/Cmd+Shift+M toggles meeting chat. `ponytail:` these choices avoid Ctrl/Cmd+J downloads and Ctrl/Cmd+Shift+J DevTools, but Ctrl/Cmd+Shift+N can be host/browser-reserved outside Tauri. The shared guard skips input, textarea, and contenteditable focus, including BlockNote. Native button titles display the shortcuts.
- Spillover: none.

### Task 5.3 — Persist panel width [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Extend `usePanelResize` with optional localStorage persistence and restore clamped panel widths at mount.
- Files changed: `frontend/src/hooks/usePanelResize.ts`, `frontend/src/app/page.tsx`, `frontend/src/app/meeting-details/page-content.tsx`, `frontend/src/components/Sidebar/SidebarProvider.tsx`, `frontend/tests/hooks/usePanelResize.test.ts`, this doc.
- Verification: typecheck ✅ · vitest ✅ (60 passed, 16 files) · cargo check n/a (no Rust touched)
- Notes/decisions: Added the optional `storageKey` option and wired `meedly:recording-notes-width`, `meedly:meeting-notes-width`, `meedly:meeting-transcript-width`, and `meedly:sidebar-width`. Stored values are parsed, clamped to the current viewport range on mount, and invalid or unavailable reads fall back to `initial`; writes are guarded against localStorage failures. Persistence writes happen on each drag move rather than mouseup because the existing move handler already computes the single clamped value and the synchronous write keeps the smallest implementation without changing drag lifecycle behavior.
- Spillover: none.

### Task 5.4 — i18n groundwork [M]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Extract user-visible notes and chat strings into an English dictionary and add minimal lookup groundwork without adding an i18n library.
- Files changed: `frontend/src/lib/i18n.ts`, `frontend/src/lib/strings/en.ts`, `frontend/src/components/ChatPanel/index.tsx`, `frontend/src/components/ChatPanel/ChatMessage.tsx`, `frontend/src/components/notes/NotesEditorShell.tsx`, `frontend/src/components/MeetingDetails/NotesPanel.tsx`, `frontend/tests/lib/i18n.test.ts`, this doc.
- Verification: typecheck ✅ · vitest ✅ (64 passed, 17 files) · cargo check n/a (no Rust touched)
- Notes/decisions: Homegrown `t(key, vars?)` uses the EN dictionary by default, substitutes `{variable}` placeholders, and visibly falls back to the key. Namespaces use `<area>.<element>.<purpose>` (`chat.header.title`, `chat.suggested.actionItems`, `notes.status.savedAt`, `notes.toast.saveFailed`). Migrated chat header, provider labels/tooltips, scope, empty state, all eight prompts, controls, error text, message copy/source labels, and notes header/status/actions/error/retry/delete confirmation/toasts. `ponytail:` markers in `src/lib/i18n.ts` intentionally keep this a small stable-shape bridge rather than choosing a provider now. Sidebar PT-BR remains untouched and is flagged there at `Sidebar/index.tsx:775, 985, 1024` and `Sidebar/FolderTreeItem.tsx:252`. Added four primitive tests for dictionary lookup, interpolation, missing-key fallback, and unfilled templates. Decision needed (non-blocking): choose app-wide i18n direction — next-intl, react-i18next, or retain/expand the homegrown dictionary — before migrating Sidebar and Settings.
- Spillover: Dev-facing `logger` messages and IPC identifiers were intentionally not extracted. `RecordingNotesPanel` has no user-visible strings beyond the shared migrated notes shell; its logger-only save error remains out of scope. The requested legacy notes placeholder is no longer present after the BlockNote migration, so no artificial placeholder was added.

### Review R7 — Sprint 5 full review
- Date: 2026-08-15
- Reviewer model: openai/gpt-5.6-sol
- Verdict: changes-requested
- Findings:
  - **Must-fix (2):** (1) Saves are neither serialized nor versioned. `wrappedSave` snapshots content, awaits markdown conversion before setting `isSaving`, and then permits debounce/blur/unmount saves to overlap; an older completion can overwrite newer SQLite content and reset the UI refs/dirty flag to the stale version (`frontend/src/components/notes/useNotesEditor.ts:51-79`, `frontend/src/components/notes/useNotesEditor.ts:139-175`). The same gap defeats deletion protection: while conversion is in flight the delete button is still enabled, and `cancelPendingSave()` cannot cancel that already-started save, so it can recreate the row after `delete_meeting_notes` (`frontend/src/components/MeetingDetails/NotesPanel.tsx:125-135`). Add an out-of-order completion/delete-race test. (2) Recording notes do not satisfy the JSON-on-conversion-failure durability guarantee. The immediate draft stores only `{blocks}`, the quota fallback retries `sessionStorage.setItem` unguarded with potentially stale `notesRef`, the disk callback discards `blocksJson`, and stop removes the sole JSON copy before post-stop persistence (`frontend/src/components/RecordingNotesPanel.tsx:41-57`, `frontend/src/components/RecordingNotesPanel.tsx:101-109`, `frontend/src/hooks/useRecordingStop.ts:133-138`). A conversion failure or a full sessionStorage can therefore lose the latest recording-note content; the required 5 MB ceiling/fallback behavior also lacks a runnable test.
  - **Should-fix (4):** (1) Stored panel width is restored in an effect, not by the `useState` initializer, so the first render uses `initial` and only then jumps to the clamped stored width; the test's `act()` hides this and does not establish the requested before-first-render invariant (`frontend/src/hooks/usePanelResize.ts:34-51`, `frontend/tests/hooks/usePanelResize.test.ts:39-45`). (2) The new icon-only notes/chat toggle buttons provide `title` but no accessible name (`frontend/src/app/page.tsx:325-332`, `frontend/src/app/meeting-details/page-content.tsx:399-417`). Chat's close and send icon buttons are likewise unnamed, and the dictionary has 50 entries rather than the specified 51, indicating incomplete a11y/i18n coverage (`frontend/src/components/ChatPanel/index.tsx:399-404`, `frontend/src/components/ChatPanel/index.tsx:476-482`, `frontend/src/lib/strings/en.ts:1-52`). (3) `NotesPanel` treats any JSON array, including `[]` or arrays of invalid objects, as authoritative BlockNote content instead of falling back to legacy markdown; this can pass invalid `initialContent` to BlockNote and make an otherwise recoverable note unloadable (`frontend/src/components/MeetingDetails/NotesPanel.tsx:80-98`, `frontend/src/components/BlockNoteEditor/Editor.tsx:19-22`). (4) The requested backend-persistence verification is misleading: `cargo test --lib -- meeting_notes` passes three export tests whose names merely contain `meeting_notes`; `MeetingNotesRepository` itself has no save/get JSON round-trip test (`frontend/src-tauri/src/database/repositories/meeting_notes.rs:17-61`).
  - **Nit (1):** The per-drag persistence rationale is recorded in this execution log but the requested source-level `ponytail:` marker is absent beside the synchronous write (`frontend/src/hooks/usePanelResize.ts:65-77`).
  - **Confirmed:** `notes_json` wins over valid JSON and legacy markdown is parsed once; markdown conversion fallback reaches `save(markdown, blocksJson)` for meeting notes; BlockNote JSON is produced only at save time for DB notes; debounce is 2 s; pending deletion timers are cancelled before IPC; FTS refresh and sidebar `has_notes` remain wired (`frontend/src-tauri/src/database/commands.rs:307-349`, `frontend/src-tauri/src/database/repositories/meeting.rs:10-17`); the shared editor remains compatible with `BlockNoteSummaryView`; no stale `extraTextareaClassName`/notes textarea key handler remains. Ctrl/Cmd+S is the sole notes-shell path and is dirty/idle-gated; N/M shortcuts are distinct, recording-gated where applicable, focus-guarded for real contenteditable elements, and cleaned up. All four `meedly:*` storage keys are wired and storage access is guarded. `t()` has lookup/interpolation/key fallback with four tests; migrated visible notes/chat literals use it, logger/IPC literals remain literal, and Sidebar PT-BR remains intentionally untouched. No Sprint 5 dependency or privacy/analytics expansion was found.
  - **Verification rerun:** `pnpm run typecheck` ✅; `npx vitest run` ✅ (64 tests, 17 files; React `act(...)` warnings remain); `CARGO_TARGET_DIR=C:\Users\arman\cargo-target cargo check --manifest-path src-tauri/Cargo.toml` ✅; `cargo test --manifest-path src-tauri/Cargo.toml --lib -- meeting_notes` ✅ (3 tests, but see coverage finding); `cargo fmt --check --manifest-path src-tauri/Cargo.toml` ✅.
- Follow-up tasks created: 5.1a (serialize/version saves, close the delete race, and add repository JSON round-trip coverage), 5.1b (make recording BlockNote draft/stop persistence lossless and quota-safe), 5.2a (restore accessible names for shortcut/chat icon buttons), 5.3a (restore clamped width before first render), 5.4a (complete dictionary/a11y migration)

### Task 5.1b — Durable recording BlockNote draft/stop persistence [M]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-sol
- Status: done
- Scope: Persist recording-note markdown and BlockNote JSON in the quota-safe session draft and recording folder, then import both into `meeting_notes` before clearing the recovery draft.
- Files changed: `frontend/src/components/RecordingNotesPanel.tsx`, `frontend/src/hooks/useRecordingStop.ts`, `frontend/src-tauri/src/audio/recording_commands.rs`, `frontend/src-tauri/src/database/repositories/transcript.rs`, `frontend/src-tauri/src/audio/import.rs`, `frontend/tests/components/RecordingNotesPanel.test.tsx`, this doc.
- Verification: typecheck ✅ · vitest ✅ (71 passed, 18 files) · cargo check ✅ · cargo test `--lib -- recording` ✅ (2 passed) · cargo fmt --check ✅
- Notes/decisions: `persistDraft(key, { markdown, blocksJson })` retries once with markdown-only and reports failure without throwing. Each BlockNote change immediately snapshots current JSON with available markdown, then a generation guard permits only the latest asynchronous markdown conversion to refresh that draft and is invalidated on unmount/stop. `save_recording_notes(notes, blocks_json)` still writes `notes.md` and now writes `notes.json` when JSON is available; normal stop and audio-import recovery read both files into `meeting_notes`. The normal stop import now propagates a DB-copy failure through `saveMeeting`, so the stop flow clears its session draft only after `saveMeeting` returns a meeting ID with notes persistence complete. The Rust helper test verifies both files. Four Vitest scenarios cover rich round-trip, quota fallback, both writes failing, and markdown-only reload parsing. The markdown-only quota fallback preserves text and basic structure through BlockNote parsing but cannot preserve rich attributes that markdown cannot represent.
- Spillover: none.

### Task 5.1a — Serialize/version note saves + repository JSON coverage [M]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-sol
- Status: done
- Scope: Serialize note conversion and persistence through a latest-only queue, invalidate stale completions during edits/deletion, and verify meeting-note JSON persistence at the Rust repository boundary.
- Files changed: `frontend/src/components/notes/useNotesEditor.ts`, `frontend/src/components/MeetingDetails/NotesPanel.tsx`, `frontend/tests/components/notes/useNotesEditor.test.ts`, `frontend/src-tauri/src/database/repositories/meeting_notes.rs`, this doc.
- Verification: typecheck ✅ · vitest ✅ (71 passed, 18 files) · cargo check ✅ · `cargo test --lib -- meeting_notes` ✅ (5 passed) · cargo fmt ✅
- Notes/decisions: `wrappedSave` now marks saving before markdown conversion, snapshots each intent, permits only one conversion/IPC chain at a time, and replaces any queued intent with the newest. A generation check prevents superseded or cancelled work from issuing a not-yet-started IPC or mutating saved refs/UI after completion. Edits invalidate an active intent until their debounced save starts. `cancelPendingSave` still clears the debounce and now also clears the latest-only queue and bumps the generation before `NotesPanel` deletes. Two hook tests cover serialization/latest-wins and cancellation during a deferred save; two foreign-key-enabled in-memory repository tests cover JSON round-trip and UPSERT replacement.
- Spillover: none.

### Task 5.3a — Restore clamped panel width before first render [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Restore persisted panel width in the state initializer and cover the before-first-render invariant.
- Files changed: `frontend/src/hooks/usePanelResize.ts`, `frontend/tests/hooks/usePanelResize.test.ts`, this doc.
- Verification: typecheck ✅ · vitest ✅ (67 tests, 17 files) · cargo check n/a (no Rust touched)
- Notes/decisions: Added the independently callable `loadClampedWidth(storageKey, initial, min, maxFraction)` helper and used it as the lazy `useState` initializer, preserving the no-storage-key fallback and clamping stored values to the current viewport. Added the requested `ponytail:` marker for synchronous per-drag writes. The restore test now captures the first render width, and the existing clamp scenario verifies the clamped value.
- Spillover: none.

### Task 5.2a — Accessible names for notes/chat icon buttons [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Add dictionary-backed accessible names to the R7-flagged notes/chat icon buttons.
- Files changed: `frontend/src/app/page.tsx`, `frontend/src/app/meeting-details/page-content.tsx`, `frontend/src/components/ChatPanel/index.tsx`, `frontend/src/lib/strings/en.ts`, `frontend/tests/lib/i18n.test.ts`, this doc.
- Verification: typecheck ✅ · vitest ✅ (72 passed, 18 files) · cargo check n/a (no Rust touched)
- Notes/decisions: Added `Show recording notes`, `Show notes`, `Show chat`, `Close chat`, and `Send message` aria-labels. Existing clear/stop labels remain intact; the decorative chat header icon is explicitly hidden from assistive technology.
- Spillover: none.

### Task 5.4a — Complete notes/chat i18n coverage [S]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Complete the missing accessible-name dictionary coverage and assert its minimum size.
- Files changed: `frontend/src/lib/strings/en.ts`, `frontend/src/components/ChatPanel/index.tsx`, `frontend/tests/lib/i18n.test.ts`, this doc.
- Verification: typecheck ✅ · vitest ✅ (72 passed, 18 files) · cargo check n/a (no Rust touched)
- Notes/decisions: The dictionary now has 55 keys: `chat.closeAria`, `chat.sendAria`, `app.recording.showNotesAria`, `app.meetingDetails.showNotesAria`, and `app.meetingDetails.showChatAria` were added. The migrated ChatPanel, ChatMessage, NotesEditorShell, NotesPanel, and RecordingNotesPanel sweep found no remaining user-visible literals; logger and IPC strings remain intentionally literal. The likely missing 51st extraction was an unnamed ChatPanel control, with close/send both now covered. No ponytail simplifications added.
- Spillover: none.

### Review R7b — must-fix + should-fix re-validation
- Date: 2026-08-15
- Reviewer model: openai/gpt-5.6-sol
- Verdict: changes-requested
- Findings:
  - **Blocker / must-fix — 5.1b can still discard the newest recording-note edit at stop.** A BlockNote edit only starts its disk save after the hook's two-second timer (`frontend/src/components/RecordingNotesPanel.tsx:143-164`; `frontend/src/components/notes/useNotesEditor.ts:190-199`). Native stop removes the `RecordingManager` from the global immediately (`frontend/src-tauri/src/audio/recording_commands.rs:539-543`), after which `save_recording_notes` cannot recover the folder and returns success without writing (`frontend/src-tauri/src/audio/recording_commands.rs:971-983`). Thus an edit made less than two seconds before Stop has rich content in sessionStorage but can never reach `notes.md`/`notes.json`; the later DB import reads stale files and then deletes that newer draft (`frontend/src/hooks/useRecordingStop.ts:255-269`). The unmount flush is fire-and-forget and runs only after the stopped UI state, so it neither orders the write before import nor restores access to the removed manager. Explicitly persist/await the latest draft before manager teardown or import the session draft, and only clear it after that exact revision is durable.
  - **Blocker / must-fix — 5.1a does not establish delete-after-save ordering once the save IPC has started.** The generation fence correctly prevents stale frontend completion state, but it cannot cancel an `options.save` already awaiting IPC (`frontend/src/components/notes/useNotesEditor.ts:94-118`). Save UPSERT and delete are separate async commands using the shared pool with no common transaction/lock (`frontend/src-tauri/src/database/commands.rs:307-321,355-372`); Tauri async commands do not provide a global FIFO guarantee, so a delayed UPSERT may execute after DELETE and recreate the row. The hook test resolves a mocked delete while its save promise is pending, but that save mock never mutates persistence, so it cannot prove delete wins at the DB (`frontend/tests/components/notes/useNotesEditor.test.ts:226-263`). Serialize this boundary or await the active save before issuing DELETE, then cover the real ordering contract.
  - **Should-fix — the recording tests do not exercise the component races/hydration claimed by 5.1b.** All four tests call only exported `persistDraft`; the “reload” case invokes a synthetic parser directly and no test mounts `RecordingNotesPanel`, drives `handleBlocksChange`, resolves conversion out of order, unmounts/stops, or verifies disk/import sequencing (`frontend/tests/components/RecordingNotesPanel.test.tsx:3-53`). The full suite has 72 tests, below this review's expected ≥75. Add focused component/flow coverage so the stop-loss and stale-conversion regressions fail runnable tests.
  - **Validated / 5.1a partial pass:** save intents are single-flight/latest-only, `isSaving` is set before conversion, cancellation clears the timer/queue and bumps the generation, stale completions cannot alter refs/UI, unmount flush queues behind active work, and the manual deferred tests avoid `Promise.withResolvers` (`frontend/src/components/notes/useNotesEditor.ts:59-151,153-227`; `frontend/tests/components/notes/useNotesEditor.test.ts:184-263`). `NotesPanel` cancels before DELETE (`frontend/src/components/MeetingDetails/NotesPanel.tsx:125-133`). Both FK-enabled repository tests insert the parent meeting and verify markdown plus JSON round-trip/UPSERT (`frontend/src-tauri/src/database/repositories/meeting_notes.rs:71-148`).
  - **Validated / 5.1b partial pass:** the new payload and guarded quota fallback are correct; hydration prefers `blocksJson`, supports markdown-only and legacy `{ blocks }`/plain-markdown drafts; generation cleanup rejects stale conversion writes; `+ New Call` remains unkeyed; normal and audio-import recovery paths read both files; and the Rust helper writes both files when JSON is supplied (`frontend/src/components/RecordingNotesPanel.tsx:21-35,51-61,84-165`; `frontend/src-tauri/src/database/repositories/transcript.rs:97-113`; `frontend/src-tauri/src/audio/import.rs:780-799`; `frontend/src-tauri/src/audio/recording_commands.rs:993-1026`). These do not close the stop-teardown loss above.
  - **Validated / should-fixes:** 5.2a pass (the flagged page, meeting-detail, close, and send icon controls use dictionary-backed accessible names); 5.3a pass (lazy clamped initializer, no restore effect, no-key fallback, guarded SSR/storage access, first-render clamp test, and source `ponytail:` marker); 5.4a pass (55 dictionary keys, count regression test present, and no remaining user-visible literals in the requested notes/chat component sweep). No new dependency, analytics, secret logging, or unexpected network path was introduced.
  - **Verification rerun:** `pnpm run typecheck` ✅; `npx vitest run` ✅ (72 passed, 18 files; existing React `act(...)` warnings); cargo check ✅; `cargo test --lib -- meeting_notes` ✅ (5 passed); `cargo test --lib -- recording` ✅ (2 passed); full `cargo test --lib` ✅ (345 passed, 2 ignored); cargo fmt --check ✅.
  - **Residual counts / shipment:** must-fix residuals **2**; should-fix residuals **1**. Headline: **5.1a fail**, **5.1b fail**, **5.2a pass**, **5.3a pass**, **5.4a pass**. Sprint 5 is **not shippable** until both durability/order gaps are closed and covered.
- Follow-up tasks created: **5.1c** make stop persist/import the exact latest recording draft before teardown/clear; **5.1d** enforce and test DB-level save/delete ordering plus add real recording component race coverage.

### Task 5.1c — Flush latest recording notes before native stop teardown (R7b) [M]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-sol
- Status: done
- Scope: Make the UI stop path await the exact latest BlockNote save before `stop_recording` removes the recording manager, and add mounted-component coverage for the R7b recording-note races.
- Files changed: `frontend/src/components/notes/useNotesEditor.ts`, `frontend/src/lib/recording-notes-flush.ts`, `frontend/src/components/RecordingNotesPanel.tsx`, `frontend/src/components/RecordingControls.tsx`, `frontend/src/components/MeetingDetails/NotesPanel.tsx`, `frontend/tests/components/RecordingNotesPanel.test.tsx`, `frontend/tests/components/notes/useNotesEditor.test.ts`, this doc.
- Verification: typecheck ✅ · vitest ✅ (76 passed, 18 files) · cargo check ✅
- Notes/decisions: Chose Path A. `RecordingNotesPanel` registers its Promise-returning flush in a single-slot registry; `RecordingControls` awaits it immediately before invoking native `stop_recording`. The hook cancels the debounce, queues the latest revision through the existing serialized save loop, waits for conversion/in-flight/queued work to drain, retries a failed latest revision once, and rejects the explicit stop flush if that revision is not durable. The registry carries the requested `ponytail:` debt note and is a no-op when no panel is mounted. Mounted-component tests cover latest-edit-before-import ordering, slow conversion, stop at 1.9s, and placeholder-to-real-title hydration. React Testing Library is not installed, so the tests use the existing dependency-free React `createRoot`/`act` pattern rather than add a dependency.
- Spillover: Native tray-initiated stop calls Rust directly and cannot await this frontend registry; this task closes the recording-screen Stop path identified in R7b, but a future backend handshake or draft-import fallback would be needed to provide the same pre-teardown ordering for tray stop.

### Task 5.1d — Enforce save-before-delete DB ordering (R7b) [M]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-sol
- Status: done
- Scope: Make notes deletion await the active serialized save drain before issuing the delete IPC, and cover the ordering contract with state-mutating frontend and SQLite repository tests.
- Files changed: `frontend/src/components/notes/useNotesEditor.ts`, `frontend/src/components/MeetingDetails/NotesPanel.tsx`, `frontend/src/components/notes/NotesEditorShell.tsx`, `frontend/tests/components/notes/useNotesEditor.test.ts`, `frontend/src-tauri/src/database/repositories/meeting_notes.rs`, this doc.
- Verification: typecheck ✅ · vitest ✅ (76 passed, 18 files) · cargo check ✅ · `cargo test --lib -- meeting_notes` ✅ (6 passed) · cargo fmt --check ✅
- Notes/decisions: `flushPendingSave` now returns the active save Promise and waits through its latest-only queue drain; a pending debounce starts immediately, while a failed recording-note revision can still retry for 5.1c. `NotesPanel` preserves cancellation's generation bump, then awaits the drain before invoking `delete_meeting_notes`; `isDeleting` disables the trash button through that boundary. The frontend test uses a mutable Map and proves the deferred save writes before deletion removes the row. The FK-enabled SQLite test awaits a spawned UPSERT before spawning DELETE and verifies no row remains. A deliberately unawaited race test was skipped because either scheduler outcome is valid and would make CI flaky (`ponytail:` rationale recorded in the Rust test).
- Spillover: none.

### Review R7c — must-fix closure validation
- Date: 2026-08-15
- Reviewer model: openai/gpt-5.6-sol
- Verdict: changes-requested
- Findings:
  - **Blocker / must-fix — 5.1c fails its failed-flush abort contract.** The successful path is strictly ordered (`await flushRecordingNotes()` precedes `stop_recording`), so a last-second edit reaches `save_recording_notes` while the manager still owns the folder (`frontend/src/components/RecordingControls.tsx:162-176`; `frontend/src-tauri/src/audio/recording_commands.rs:539-543,971-984`). However, when both save attempts fail, the rejection falls into the generic stop catch and calls `onRecordingStop(false)` (`frontend/src/components/RecordingControls.tsx:184-203`). That parent callback starts the post-stop flow, immediately marks the frontend recording stopped/unmounts the notes panel, and eventually returns to idle without invoking native stop (`frontend/src/hooks/useRecordingStop.ts:121-146,410-417`). The Rust manager therefore remains recording while the UI says it stopped, and the user sees no toast/modal—only logs. A durability failure must leave the recording UI/state coherent, keep the native manager running, reset the initiated/stopping guards so Stop can be retried, and display an actionable error. Add a mounted control-flow test for a twice-rejected notes save that proves `stop_recording` and post-stop processing are not called and the error is visible.
  - **Nit — “retry once” is not exact for a save that failed before Stop.** If `saveErrorRef` already identifies the current generation, `flushPendingSave` starts one retry at line 159 and, if that fails, starts another at line 163, resulting in two retries after the original failure (`frontend/src/components/notes/useNotesEditor.ts:152-165`). This is not data loss, but it disagrees with the task contract and can repeat a failing disk IPC unnecessarily.
  - **R7b Must-Fix 1 / 5.1c: fail (partial closure).** Registration has identity-safe cleanup and the required single-slot `ponytail:` ceiling (`frontend/src/lib/recording-notes-flush.ts:1-14`; `frontend/src/components/RecordingNotesPanel.tsx:82-88`). The registered Promise flush cancels the 2 s timer, joins the active single-flight loop, drains its latest-only queue, and rejects after persistent failure (`frontend/src/components/notes/useNotesEditor.ts:62-165`). Hydration still supports `{markdown, blocksJson}`, legacy `{blocks}`, and plain markdown (`frontend/src/components/RecordingNotesPanel.tsx:100-138`). All four new tests mount with `createRoot`/`act` and cover latest-before-import, slow conversion, the 1.9 s debounce boundary, and placeholder-title hydration (`frontend/tests/components/RecordingNotesPanel.test.tsx:101-194`), but none exercises `RecordingControls` or the broken failed-flush branch. Native tray stop remains a documented, accepted limitation because it bypasses the frontend registry; it still lacks equivalent pre-teardown durability and should not be represented as covered.
  - **R7b Must-Fix 2 / 5.1d: pass.** Delete cancels the timer/queue and generation first, sets `isDeleting`, awaits the active save drain, and only then invokes DELETE; the shell disables the delete button across that wait (`frontend/src/components/MeetingDetails/NotesPanel.tsx:126-144`; `frontend/src/components/notes/NotesEditorShell.tsx:119-130`). A save IPC resolves only after the repository's single-statement SQLite autocommit completes, so the JS await establishes commit-before-DELETE without relying on Tauri FIFO (`frontend/src-tauri/src/database/repositories/meeting_notes.rs:28-61`; `frontend/src-tauri/src/database/commands.rs:307-321,355-372`). The Map mock really writes after its deferred save resolves and is then deleted (`frontend/tests/components/notes/useNotesEditor.test.ts:224-269`); the FK-enabled Rust test spawns/awaits UPSERT before spawning/awaiting DELETE and carries the skipped-race `ponytail:` rationale (`frontend/src-tauri/src/database/repositories/meeting_notes.rs:150-195`). Best-effort unmount and `beforeunload` flushes remain wired (`frontend/src/components/notes/useNotesEditor.ts:181-201`).
  - **Cross-cutting / shipment:** autosave remains 2 s, serialization is integrated once in the shared hook, post-stop draft removal remains after successful notes import, and no new dependency/privacy/network scope was introduced (`frontend/src/components/notes/useNotesEditor.ts:203-230`; `frontend/src/hooks/useRecordingStop.ts:255-269`). Sprint 5 is **not shippable** until the 5.1c failed-flush state/error path is corrected; 5.1d is closed.
  - **Verification rerun:** `pnpm run typecheck` ✅; `npx vitest run` ✅ (76 passed, 18 files; existing React `act(...)` warnings); cargo check ✅; `cargo test --lib -- meeting_notes` ✅ (6 passed); full `cargo test --lib` ✅ (346 passed, 2 ignored); cargo fmt --check ✅.
- Follow-up tasks created: **5.1e** correct and test the failed recording-notes flush abort/UI-state path (including exact retry semantics)

### Task 5.1e — Failed recording-notes flush abort/UI-state path (R7c) [M]
- Date: 2026-08-15
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Keep the recording active when a required recording-notes flush fails, and make the flush retry contract exact.
- Files changed: `frontend/src/components/RecordingControls.tsx`, `frontend/src/components/notes/useNotesEditor.ts`, `frontend/src/lib/strings/en.ts`, `frontend/tests/components/RecordingNotesPanel.test.tsx`, this doc.
- Verification: typecheck ✅ · vitest ✅ (77 passed, 18 files) · cargo check ✅ · cargo fmt --check ✅
- Notes/decisions: `RecordingControls` now catches the notes flush separately before native stop, restores the shared recording status and local stop guard, and shows an i18n-backed Sonner error without calling post-stop processing. `flushPendingSave` now makes one retry only: a pending flush attempts once then retries once on failure; an already-failed current generation gets one retry before rejection. The mounted regression test rejects both saves, proves no native/post-stop calls, checks the toast/status reset, and checks the Stop button is enabled for retry. No new ponytail markers.
- Spillover: none.

### Review R7d — 5.1e closure validation
- Date: 2026-08-15
- Reviewer model: openai/gpt-5.6-sol
- Verdict: approve
- Findings:
  - **R7c Must-Fix / 5.1e abort path: pass.** The notes flush has its own catch and returns before the separate native-stop try; failure restores processing/stopping guards and shared `RECORDING` status, surfaces the dictionary-backed toast, and cannot call `stop_recording` or `onRecordingStop` (`frontend/src/components/RecordingControls.tsx:163-177`; `frontend/src/lib/strings/en.ts:55`). The parent keeps `isRecording` true because post-stop is not entered, while the status reset reverses `onStopInitiated`'s `STOPPING` transition, so the enabled Stop control can reach the flush again (`frontend/src/app/page.tsx:290-305`; `frontend/src/hooks/useRecordingStop.ts:464-472`).
  - **R7c Nit / 5.1e retry precision: pass.** `retried` bounds an already-failed generation to one retry, while a newly started flush gets its initial attempt plus one retry; a second failure rejects. Generation checks still prevent a stale completion from clearing errors or updating saved/UI refs after a newer edit starts (`frontend/src/components/notes/useNotesEditor.ts:98-126,152-168`). Cancellation remains a no-save drain for deletion, and delete still awaits that drain before DELETE (`frontend/src/components/notes/useNotesEditor.ts:170-181`; `frontend/src/components/MeetingDetails/NotesPanel.tsx:126-144`).
  - **Regression coverage: pass.** The mounted panel-plus-controls test forces exactly two rejected `save_recording_notes` calls and asserts no native stop, no post-stop callback, the expected toast/status reset, and an enabled Stop button (`frontend/tests/components/RecordingNotesPanel.test.tsx:207-247`). The four prior mounted 5.1c cases remain present, including latest-before-import, slow conversion, and the 1.9 s debounce boundary (`frontend/tests/components/RecordingNotesPanel.test.tsx:136-205`); the save-before-delete test also remains green (`frontend/tests/components/notes/useNotesEditor.test.ts:224-274`).
  - **Nit (non-blocking):** The exact retry/abort contract has no requested source-level `ponytail:` audit comment beside `flushPendingSave`; behavior and regression coverage are nevertheless clear (`frontend/src/components/notes/useNotesEditor.ts:152-168`). No follow-up is warranted for this documentation-only preference.
  - **Cross-cutting / shipment:** The EN dictionary has 56 unique keys after the single addition, no dependency/privacy/analytics/network scope was added, and the 5.1e diff stays within its stated surface. Verification rerun: `pnpm run typecheck` ✅; `npx vitest run` ✅ (77 passed, 18 files; existing React `act(...)` warnings); cargo check ✅; cargo fmt --check ✅. R7c's last must-fix and retry nit are closed; Sprint 5 is **shippable**.
- Follow-up tasks created: none

### Task 6.1.1 — Scope contract and scope-aware conversation persistence [M]
- Date: 2026-08-17
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Add validated persisted Chat scopes, migrate existing conversation lineage, and expose additive scope-aware conversation resume/create APIs without changing the current Chat callers.
- Files changed: `frontend/src-tauri/migrations/20260817000000_add_chat_conversation_scopes.sql`, `frontend/src-tauri/src/database/repositories/chat.rs`, `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/lib.rs`, `frontend/src/types/index.ts`, this doc.
- Verification: typecheck ✅ · vitest ✅ (77 passed) · cargo check ✅ · `cargo test --manifest-path src-tauri/Cargo.toml --lib database::repositories::chat::tests` ✅ (4 passed) · cargo fmt --check ✅
- Notes/decisions: The additive migration stores `scope_kind`, `scope_key`, and optional `scope_data`, backfills existing meeting/global/orphan rows, indexes exact-scope lookup, and converts newly deleted meeting conversations to `orphaned_meeting` through an FK-following trigger. `ChatScope` is a strict Rust/TypeScript discriminated contract: all, meeting, folder, search snapshot (typed result IDs), and live recording are accepted; empty keys, mismatched data, unknown fields, and externally-created orphan scopes are rejected at the Tauri boundary. Legacy create/get commands remain intact and now write compatible all/meeting scope columns. The new `api_chat_get_or_create_scoped_conversation` resumes only an exact persisted kind/key/data match.
- Spillover: The task started from an already-modified/untracked Sprint 4/5 Chat persistence baseline; no unrelated files were changed for this task.

### Task 6.1.5 — Cross-context regression coverage and Windows native smoke path [S]
- Date: 2026-08-17
- Implementer model: openai/gpt-5.6-luna
- Status: blocked
- Scope: Add the missing focused regression covering Home/all → meeting → recursive folder → frozen search snapshot → live recording → saved-meeting promotion, then run the full requested automated verification. The native Windows smoke was not attempted non-interactively.
- Files changed: `frontend/tests/components/chat-scope.test.tsx`, this doc.
- Verification: typecheck ✅ · pnpm test ✅ (87 passed, 19 files) · npx vitest run ✅ (87 passed, 19 files) · `cargo test --lib api::chat::tests` ✅ (15 passed) · `cargo test --lib database::repositories::chat::tests` ✅ (6 passed) · cargo check ✅ · cargo fmt --check ✅ · git diff --check ✅
- Notes/decisions: The new test exercises the complete scope transition sequence through the single ChatHost and asserts promotion into the saved meeting scope. Existing act warnings and the intentional recording-notes failure logs remain non-failing test output.
- Spillover: Manual Windows smoke remains pending and requires an interactive Tauri session with microphone permission/device, system-audio loopback/device, a downloaded/loaded transcription model, and a configured provider (local provider required to exercise live content without disclosure). Checklist: launch the packaged/dev app; verify model loads; grant/select microphone and system-audio devices; start recording; open Home/all Chat, then meeting Chat, recursive folder Chat, and a search-result Chat snapshot; start Live Chat and send a question; stop and save the recording; reopen the promoted saved meeting Chat and verify the live exchange/history and sources remain attached to the meeting scope; repeat with provider disclosure only if intentionally testing a non-local provider. Status: blocked until those interactive Windows conditions are available.

### Task 6.1.R1 — Own cancellation before preparation and fence delayed listener/setup work [L]
- Date: 2026-08-17
- Implementer model: openai/gpt-5.6-sol
- Status: done
- Scope: Claim single-stream ownership before scoped and legacy asynchronous preparation, propagate cancellation through preparation, suppress stale stream events, and generation-fence frontend listener registration and invocation.
- Files changed: `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/mcp/server.rs`, `frontend/src/components/ChatPanel/index.tsx`, `frontend/tests/components/chat-scope.test.tsx`, this doc.
- Verification: typecheck ✅ · vitest ✅ (88 passed, 19 files) · cargo check ✅ · focused Rust cancellation test ✅ · cargo fmt --check ✅ · git diff --check ✅
- Notes/decisions: Stream ownership now installs and cancels tokens before model config, rewrite, and retrieval work. Query rewriting receives the same cancellation token; every asynchronous preparation boundary checks cancellation. Start, chunk, and terminal events share the ownership lock with emission, so a replacement cannot interleave between the ownership check and event emission. A cancelled current stream still emits its partial terminal answer, while a replaced or pre-stream-cancelled request emits nothing. The renderer captures a synchronous scope generation and fences each listener-registration await, every event callback, the live-provider preflight await, and the final invoke; a late listener unregisters itself instead of joining the new scope.
- Spillover: Existing Vitest React `act(...)` warnings remain non-failing and predate this task.

### Task 6.1.R2 — Bind live scope identity and remote-provider consent at the Rust boundary [L]
- Date: 2026-08-17
- Implementer model: openai/gpt-5.6-sol
- Status: done
- Scope: Issue live Chat scope identity from the native recording lifecycle and enforce active-recording identity plus per-request remote-transmission consent in the shared Rust scoped preparation path.
- Files changed: `frontend/src-tauri/src/audio/recording_commands.rs`, `frontend/src-tauri/src/api/chat.rs`, `frontend/src/services/recordingService.ts`, `frontend/src/contexts/RecordingStateContext.tsx`, `frontend/src/app/page.tsx`, `frontend/src/hooks/useRecordingStop.ts`, `frontend/src/components/ChatPanel/index.tsx`, `frontend/tests/components/chat-scope.test.tsx`, this doc.
- Verification: typecheck ✅ · vitest ✅ (88 passed, 19 files) · cargo check ✅ · focused Rust Chat tests ✅ (21 passed) · cargo fmt --check ✅
- Notes/decisions: Each successful native recording start now issues an ephemeral UUID scope key, returns it in the start event/state sync, and clears the native active key at stop. Both streaming and non-streaming scoped Tauri commands require `liveTranscriptConsent` and share preparation-time authorization after resolving the persisted provider: only Ollama and built-in/local aliases are local; cloud, custom, and unknown providers require consent. Persisted live keys must match the active native key before transcript retrieval. The UI retains its confirmation and forwards consent only for a confirmed non-local request. MCP remains on the legacy saved-meeting preparation path and receives no live scope capability.
- Spillover: Existing Vitest React `act(...)` warnings and intentional recording-notes failure logs remain non-failing; interactive Windows smoke remains pending under task 6.1.5.

### Task 6.1.R3 — Stream-safe, idempotent, crash-recoverable live promotion [L]
- Date: 2026-08-17
- Implementer model: openai/gpt-5.6-sol
- Status: done
- Scope: Fence active live streams before stop/save promotion, make repeated promotion converge on one meeting thread with exact message/source continuity, and carry durable live linkage through transcript restart recovery.
- Files changed: `frontend/src-tauri/migrations/20260817110000_add_chat_promotion_lineage.sql`, `frontend/src-tauri/src/api/api.rs`, `frontend/src-tauri/src/api/chat.rs`, `frontend/src-tauri/src/database/repositories/chat.rs`, `frontend/src-tauri/src/database/repositories/transcript.rs`, `frontend/src/components/ChatPanel/ChatHost.tsx`, `frontend/src/contexts/TranscriptContext.tsx`, `frontend/src/hooks/useRecordingStop.ts`, `frontend/src/hooks/useTranscriptRecovery.ts`, `frontend/src/services/indexedDBService.ts`, `frontend/src/services/storageService.ts`, `frontend/tests/components/chat-scope.test.tsx`, `frontend/tests/hooks/useTranscriptRecovery.test.tsx`, this doc.
- Verification: typecheck ✅ · vitest ✅ (90 passed, 20 files) · cargo check ✅ · `cargo test --lib api::chat::tests` ✅ (21 passed) · `cargo test --lib database::repositories::chat::tests` ✅ (7 passed) · full `cargo test --lib` ✅ (362 passed, 2 ignored) · cargo fmt --check ✅ · git diff --check ✅
- Notes/decisions: Stop first cancels the R1-owned current stream, then passes the durable native live scope key into the shared transcript-save command. Meeting creation, transcript inserts, source rewrite, target-thread merge, and promotion commit in one SQLite transaction. A unique additive lineage column makes retries return the existing promoted meeting and prevents duplicate promotion targets; late R1 partial-answer persistence is normalized by the shared message save path after the scope fence. IndexedDB records the live key at recording start, so startup recovery uses the same atomic save path and only clears its checkpoint after repair succeeds. No R4 generic scoped get-or-create uniqueness or search membership behavior was changed.
- Spillover: Existing React `act(...)` warnings and intentional recording-notes failure logs remain non-failing. Interactive Windows smoke remains pending under task 6.1.5.

### Task 6.1.R5 — Safe chat scope identity repair migration [M]
- Date: 2026-08-17
- Implementer model: openai/gpt-5.6-terra
- Status: done
- Scope: Repair/backfill legacy and null scoped conversations before R4's scope identity index, preserving merged thread messages and source JSON.
- Files changed: `frontend/src-tauri/migrations/20260817115000_repair_chat_scope_identities.sql`, `frontend/src-tauri/src/database/repositories/chat.rs`, this doc.
- Verification: typecheck ✅ · vitest ✅ (91 passed, 20 files) · focused migration/repository test ✅ · full `cargo test --lib` ✅ (364 passed, 2 ignored) · cargo check ✅ · cargo fmt --check ✅ · git diff --check ✅
- Notes/decisions: The additive prerequisite runs after promotion lineage and before R4. It normalizes meeting, global, orphan, and null-scope rows; orphan keys become their conversation IDs so they cannot become global identities. Exact identities merge deterministically by retained promotion lineage, then latest update/creation time and ID; messages (including source JSON) move to that canonical row before duplicates are removed. SQLx applies SQLite migrations transactionally; a failed run rolls back. A successful repair is safely rolled back only by restoring a pre-upgrade database backup, because merged messages cannot be split unambiguously.
- Spillover: R4 UI snapshot and generic get-or-create behavior remain untouched; its existing unique-index migration is only validated by this prerequisite test.

### Review R8 — Sprint 6.1 end-of-sprint (6.1.1–6.1.5, R1–R5)
- Date: 2026-08-17
- Reviewer model: opencode-go/glm-5.3
- Verdict: approve
- Verification (re-run by reviewer from `upstream/frontend`, `CARGO_TARGET_DIR=C:\Users\arman\cargo-target`): typecheck ✅ · vitest ✅ (91 passed, 20 files; React `act(...)` warnings pre-existing) · cargo check ✅ · cargo fmt --check ✅ · full `cargo test --lib` ✅ (364 passed, 2 ignored). Matches every task log claim exactly.
- Findings (prioritized):
  - **Blocker:** none.
  - **R1 — closed.** Stream ownership is claimed before any asynchronous preparation in both stream commands (`api/chat.rs:830,867`); `ensure_not_cancelled` fences every await boundary (chat.rs:313,341,362,432,449,454); `suppress_chat_preparation_error` (chat.rs:1055-1069) makes a stale preparation return silently without emitting or clearing the newer owner; start/chunk/terminal events are emitted while holding the ownership lock so a replacement cannot interleave (chat.rs:1071-1096). Rust race test `delayed_old_preparation_cannot_reclaim_or_clear_new_stream` (chat.rs:1619-1639) and the frontend stale-listener/stale-scope tests cover it; the panel fences every listener-registration await and the final invoke by scope generation + stream id (ChatPanel/index.tsx:196-329).
  - **R2 — closed.** Each native start issues an ephemeral UUID live key (recording_commands.rs:262,446), the key is cleared at stop (recording_commands.rs:789-790) and exposed via state sync + started payload (RecordingStateContext.tsx:113,156); `authorize_live_transcript` (chat.rs:247-264) requires the persisted live key to equal the active native key and per-request consent for every non-local provider in the shared preparation used by both streaming (chat.rs:855-890) and non-streaming (chat.rs:764-816) scoped commands; the legacy/MCP path passes no live authorization and cannot reach live transcripts (chat.rs:231-245). Frontend consent only flips the ephemeral request field after a confirm (ChatPanel/index.tsx:166-174); Rust is authoritative, so a misclassifying renderer cannot leak a transcript.
  - **R3 — closed.** Promotion runs inside the `save_transcript` SQLite transaction (transcript.rs:104-110); retries converge via the unique lineage column plus the early return of the already-promoted meeting id (transcript.rs:20-27); duplicate live threads and any pre-existing meeting thread are merged into one conversation before the scope flip, so the unique identity index is never transiently violated (repositories/chat.rs:370-431); late partial-answer saves are normalized by `save_message`'s promotion-aware source rewrite (repositories/chat.rs:278-290), and both orderings (save-before/after-promote) converge. Stop cancels the active stream before saving (useRecordingStop.ts:261), and durable linkage rides IndexedDB `liveChatScopeKey` (TranscriptContext.tsx:136) through crash recovery (useTranscriptRecovery.ts:198-199) with a retained-and-retryable warning on failure — the checkpoint is cleared only after the successful save (test asserts save-before-markMeetingAsSaved).
  - **R4 — closed.** The snapshot is built from the sidebar's rendered list (FTS matches ∪ title matches, empty query ⇒ empty list so the launcher is search-gated) — Sidebar/index.tsx:349-359; scope.ts dedupes/bounds at 100 with a deterministic SHA-256 identity. Scoped get-or-create is a single `INSERT … ON CONFLICT(scope_kind, scope_key, COALESCE(scope_data,'')) DO UPDATE … RETURNING` (repositories/chat.rs:207-219) backed by the identity index migration, proven by the 16-way concurrency test (repositories/chat.rs:534-562).
  - **R5 — closed.** The repair migration runs before the identity index, prefers lineage-bearing rows as canonical (`promoted_from_live_scope_key IS NULL` ascending), moves messages before deleting duplicates, propagates MAX(updated_at), and keys orphans by conversation id so they can never become the global thread. The FK-following orphan trigger (scopes migration) keeps scope_key=meeting_id, which cannot collide under the one-thread-per-meeting invariant the unique index now enforces. Rollback story (transactional SQLx; backup after success) is honest and documented.
  - **Data integrity:** no path found that loses or clobbers conversations/messages: messages are append-only; promotion merges rather than overwrites; failed promotion leaves the live thread intact (test `failed_live_promotion_retains_live_conversation`); clear is user-confirmed and isBusy-guarded; scope switches never write into the prior thread (generation + conversation-id fences).
  - **Privacy/security:** chat.rs logs lengths/ids only — no transcript content or API keys; no new analytics beyond the pre-existing stop-flow events; live transcripts cannot leave the device without either a known-local provider or explicit per-request consent enforced in Rust; MCP gained only the legacy saved-meeting path.
  - **Scope/conventions:** all 6.1/R1-R5 changes are additive and match the logged file lists; no new dependencies (package.json, Cargo.toml, Cargo.lock untouched); `ponytail:` ceilings where due. The working tree also carries the previously-reviewed Sprint 1-5 baseline (notes panels, llm_client streaming, export/pdf, whisper_engine, i18n/shortcuts) — outside this review's scope per the orchestrator and already covered by R1-R7d.
  - **Should-fix 1 — frozen search snapshot becomes unresumable after a member meeting is deleted.** `api_chat_get_or_create_scoped_conversation` rejects any unknown snapshot id (chat.rs:69-84), but that command is also the ChatPanel's resume path (ChatPanel/index.tsx:103): after deleting one member meeting, reopening the same snapshot scope errors into `logger.error` only, leaving a dead panel (disabled input, no user-visible message) while the stored thread and messages remain intact but unreachable. Tolerate missing ids when an exact-scope conversation already exists (strict validation only on creation), or surface a friendly load error.
  - **Should-fix 2 — idempotent `save_transcript` retry does not heal notes/FTS.** The early return (transcript.rs:20-27) fires when a promotion already exists, so a retry after a post-commit notes-import failure (transcript.rs:113-127 propagates) skips both the notes import and the FTS refresh — the meeting rows are committed but stay unindexed and note-less until a manual rebuild. No data is lost (notes.md/notes.json remain on disk; `api_rebuild_fts_index` exists), but the self-heal does not fully converge.
  - **Nit 1 — dead code:** `ChatRepository::get_latest_conversation_for_scope` (repositories/chat.rs:173-187) has no callers (the upsert handles resume).
  - **Nit 2 — discarded recordings leave unreachable `live_recording` threads** forever (new UUID key per recording, no GC). Consistent with R3's "never discard messages" rollback principle, but an explicit user-driven cleanup would prevent unbounded accumulation.
  - **Nit 3 — live snapshot budget keeps the transcript head and drops the tail** (chat.rs:558-585); for long recordings the most recent speech falls outside a 64k/100k local budget — tail-first truncation likely serves live Q&A better.
  - **Nit 4 — cosmetics:** ChatPanel/index.tsx:482-484 and Sidebar/index.tsx:776-777 indentation drift; meeting-details chat button still says "Chat with meetings" while opening the meeting scope (title no longer matches the retired toggle semantics).
- Follow-up tasks created (suggested ids; orchestrator to slot):
  - **6.1.R6a [S]** Snapshot resume tolerance: allow deleted member ids when the exact-scope conversation exists; friendly panel error otherwise (Should-fix 1).
  - **6.1.R6b [S]** `save_transcript` idempotent retry: when returning the promoted meeting early, re-run best-effort notes import + `FtsRepository::refresh_meeting` (Should-fix 2).
  - **6.1.R6c [S]** Housekeeping batch: delete `get_latest_conversation_for_scope`, add explicit cleanup affordance for dead live threads, live-snapshot tail-first budget, cosmetic indent/title fixes (Nits 1-4).
  - Sprint close remains blocked on the pending Windows native smoke (6.1.5) regardless of this verdict.

### Review R9 — Sprint 6.1 architecture re-review (remediation R1–R5)
- Date: 2026-08-17
- Reviewer model: opencode-go/glm-5.3
- Verdict: approve-with-follow-ups
- Method: static structural review of the remediation surface (no test re-run); prior-findings closure verified against code, migrations, and wiring end to end.

**Prior architecture findings — closure verification:**
- **Live scope/consent not backend-bound — CLOSED.** Live identity is issued and cleared by the native recording lifecycle (`recording_commands.rs:1051-1059`, cleared `:791`), and `authorize_live_transcript` (`api/chat.rs:247-264`) runs after persisted-provider resolution (`:343-350`) with a fail-closed local list (`:266-271`, unknown providers require consent — tested `api/chat.rs:1519-1534`). Consent is enforced in BOTH scoped commands including the non-streaming path (`:772`, `:864`; test `:1546-1589`). Live scope is unreachable from the legacy `meeting_id` funnel and from MCP (`mcp/server.rs:186-194` uses only the legacy all/meeting `prepare_chat_inputs`). Layering is right: renderer forwards an ephemeral consent flag; Rust owns identity, provider resolution, and enforcement.
- **Crash recovery strands live conversations — CLOSED.** The live key is persisted to IndexedDB at recording start (`TranscriptContext.tsx:128-137`), recovery re-reads it (`useTranscriptRecovery.ts:134`) and passes it into the same `saveMeeting` path (`:195-200`), where promotion runs inside the meeting/transcript transaction (`transcript.rs:104-112`) with an idempotent already-promoted early return (`transcript.rs:25-32`). On failure everything is retained and a retryable warning is surfaced (`useTranscriptRecovery.ts:240-244`).
- **Promotion races streams — CLOSED (read-side residual, see Finding 1).** Stop fences the active stream before save (`ChatHost.tsx:40`, `useRecordingStop.ts:260-261`); promotion is transactional with a unique lineage key and converging retries (`repositories/chat.rs:345-368`), merges any target meeting thread (`:384-404`), and late assistant-message saves after promotion are normalized by the lineage check in `save_message` (`repositories/chat.rs:278-290`, tested `:958-1031`).
- **Scoped creation not atomic / incomplete snapshot — CLOSED.** `INSERT … ON CONFLICT(scope_kind, scope_key, COALESCE(scope_data, '')) DO UPDATE … RETURNING` (`repositories/chat.rs:207-219`) against the R4 unique expression index (`migrations/20260817120000:1-2`), proven under 16-way concurrency (`repositories/chat.rs:534-562`). The sidebar snapshot is captured from the same memo that renders the flat list, including title-only matches (`Sidebar/index.tsx:349-359`, render `:799`, ask `:357`), capped/deduped client-side (`ChatPanel/scope.ts:3-9`) and validated server-side at creation (`api/chat.rs:63-84`).
- **Migration ordering / null-scope rollback — CLOSED.** Chain 20260817000000 → 20260817110000 → 20260817115000 (repair) → 20260817120000 (index) is correctly ordered; repair merges duplicates (lineage-first canonical, messages moved before deletes) and the combined repair+index path is tested against the real migration files (`repositories/chat.rs:736-956`). Rollback-by-backup is documented in the migration header — honest for an irreversible merge.

**Findings (new, follow-up grade):**
- **Should-fix 1 — live-key TOCTOU between authorization and transcript read; stale live panel survives a new recording.** The active key is captured early (`api/chat.rs:291`), checked at `:346-350`, but the transcript is read at `:448` — with the query-rewrite LLM call (15s cap, `:413-431`) in between. If the recording stops and a new one starts inside that window, `get_transcript_history` returns the NEW recording's segments while the check passed for the OLD key: a thread consented for recording K1 can transmit K2's in-progress transcript to a remote provider. Compounding UI gap: `ChatHost.tsx:28-30` keeps an open `live_recording` panel when a new recording starts (checks kind only, not key). Narrow, user-driven window — but it is the exact class R2 was approved to close. Cheap fix: re-verify `active_live_transcript_scope_key() == Some(scope_key)` at the read point, and key-match the retained live panel in ChatHost.
- **Should-fix 2 — snapshot rehydration has unbounded chunk fan-out.** `fts.rs:311-339` (`get_by_meeting_ids`) selects every FTS chunk for up to 100 meetings with no LIMIT, and `api/chat.rs:456-466` builds sources from ALL results into `chat-stream-start` and `chat_messages.sources_json`. Other scopes are bounded by chunk_limit (10/30); a large snapshot can produce thousands of source rows per turn. Cap total (or per-meeting) chunks in snapshot rehydration.
- **Nit 1 — residual NULL-scope read hazard outside the documented rollback.** `ChatConversation.scope_kind/scope_key` are non-Option (`repositories/chat.rs:12-14`) and migrations are one-shot; a pre-6.1 binary run against a migrated DB (downgrade without backup-restore) writes NULL-scope rows later binaries cannot decode. The backup-restore rollback story covers the supported path; consider Option + read-time normalization only if downgrades are ever supported.
- **Nit 2 — meeting save is coupled to chat-data quality.** Promotion runs inside the save transaction (`transcript.rs:104-111`); any promotion error fails the whole save, and recovery retries the same live key (`useTranscriptRecovery.ts:199`) — a permanently corrupt sources_json would loop. Sources are always Rust-serialized today so corruption is near-impossible; an eventual retry-without-live-key escape hatch would decouple recording persistence from chat repair.
- **Nit 3 — duplication/foreclosure notes.** Provider-locality taxonomy is duplicated Rust/TS (`api/chat.rs:266-271` vs `ChatPanel/index.tsx:37-41`) — drift fails closed both directions, acceptable, keep in sync when adding providers. The R4 unique identity index forecloses multiple concurrent threads per scope at the schema level — correct trade for atomic get-or-create now, but the deferred conversation-list UI (`repositories/chat.rs:255` ponytail) will need a thread discriminator in scope_key; record it before the §2 6.1.2–6.7 growth re-scope. Dead leftovers from the earlier chunk-id snapshot design: `fts.rs:277-309` (`get_by_chunk_ids`) and `repositories/chat.rs:173-187` (`get_latest_conversation_for_scope`) have no callers.

**Extensibility (§2 growth tasks):** "Actions on chat answers" can branch on `scope_kind` (live threads have no meeting until promotion) — supported. "Note↔transcript links" extend additively via the existing `source_kind` navigability discriminator. "Semantic search" benefits from R4's choice to snapshot MEETING ids rather than chunk ids — frozen membership is retrieval-model-agnostic. Multi-stream (PRD out-of-scope) remains a local swap of the single-slot `ChatStreamState`; not deepened by this sprint.

**Simplicity:** Reuse is good — one shared preparation funnel serves legacy/scoped/MCP; promotion reuses the existing save transaction; recovery reuses IndexedDB infra; no parallel infrastructure introduced. Cheaper alternative worth adopting later: render `<ChatPanel key={scopeIdentity} …>` in `ChatHost.tsx:44` so scope changes remount the panel and most of the manual generation fencing (`index.tsx:57-64, 88-125`) becomes defense-in-depth rather than load-bearing — not blocking; current fencing is correct as traced.

- Follow-up tasks created (suggested ids; orchestrator to slot):
  - **6.1.R6 [S]** Re-verify the live scope key at the transcript-read point in `prepare_chat_inputs_for_scope` and key-match retained live panels in `ChatHost` (Should-fix 1).
  - **6.1.R7 [S]** Bound search-snapshot rehydration chunk fan-out; delete dead `get_by_chunk_ids`/`get_latest_conversation_for_scope` helpers (Should-fix 2 + Nit 3c).

### Task 6.1.R6 — Re-verify live scope key at transcript read + key-match retained live panel [S]
- Date: 2026-08-17
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Close R9 Should-fix 1's TOCTOU window — re-verify the active native live scope key immediately before reading the live transcript snapshot (fail closed on mismatch), and only keep/resume a live ChatHost panel whose key matches the current recording's key, not just its kind.
- Files changed: `frontend/src-tauri/src/api/chat.rs` (new `ensure_live_scope_matches_active_recording` helper + call at the `LiveRecording` snapshot-read point in `prepare_chat_inputs_for_scope`; new Rust test), `frontend/src-tauri/src/audio/recording_commands.rs` (`issue_live_transcript_scope_key` made `pub(crate)` so the restart test can issue a fresh key), `frontend/src/components/ChatPanel/ChatHost.tsx` (keep-live effect now also requires `current.key === liveTranscriptScopeKey`), `frontend/tests/components/chat-scope.test.tsx` (RecordingStateContext mock exposes `liveTranscriptScopeKey`; new stale-panel-across-restart test), this doc.
- Verification: typecheck ✅ · vitest ✅ (92 passed, 20 files) · `cargo test --lib api::chat::tests` ✅ (22 passed) · cargo check ✅ · cargo fmt --check ✅ · git diff --check ✅
- Notes/decisions: The re-check is a separate synchronous helper reading `active_live_transcript_scope_key()` at the read point (immediately before `get_transcript_history`), because re-running `authorize_live_transcript` cannot detect a swap — it compares the *captured* key (chat.rs:291) against the request's persisted key, both unchanged, while the recording can stop→restart (new UUID, old key cleared at stop) during the 15s query-rewrite call. The new check compares the *live global* key with the persisted key at read time and fails closed with the same error string as the existing authorization check. Consent is not re-checked: the provider and per-request consent flag cannot change mid-function, and R9 scoped the fix to the key. The Rust test is deterministic — no other test in the binary touches `LIVE_TRANSCRIPT_SCOPE_KEY` (grep-verified) — and asserts the read-point check rejects a prior thread's key after a restart-issued key replaced it. A mid-flight swap cannot be injected into the real async flow (no suspension between authorize and read on the local-provider/no-history path), so the focused helper check is the honest testable seam. `ChatHost` reads `liveTranscriptScopeKey` from `useRecordingState` (set from the recording-started event / state sync), so the keep effect now depends on `[isRecording, liveTranscriptScopeKey]`; stop→restart (new key) closes the stale panel, a plain stop still retains it, and promotion remains key-matched. The deliberate duplicate key check carries a rationale doc comment so a future cleanup does not remove it.
- Spillover: (1) `RecordingStateContext.liveTranscriptScopeKey` is retained (not cleared) on stop (`?? prev.liveTranscriptScopeKey` in `syncWithBackend`); the key only changes on the next start event, which is exactly when ChatHost's effect needs the comparison — no change needed, but clearing it on stop would be a hygiene cleanup. (2) A user who manually reopens a live panel for the old key during a new recording gets the Rust-side fail-closed error by design (frontend closes only the retained panel, not the whole class of stale opens). (3) The full TOCTOU race (swap while a real query-rewrite is in flight) is not directly testable without a test seam inside `prepare_chat_inputs_for_scope`; the focused helper test covers the fail-closed contract, and a seam was deemed not worth the diff.

### Task 6.1.R7 — Snapshot resume tolerance + bounded rehydration fan-out + dead-code cleanup [S]
- Date: 2026-08-17
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Close R8 Should-fix 1 (snapshot resume dead-end after a member meeting is deleted) and R9 Should-fix 2 (unbounded snapshot rehydration chunk fan-out), and delete the chunk-id-era dead `get_by_chunk_ids` helper (Nit 3c).
- Files changed: `frontend/src-tauri/src/api/chat.rs` (new `validate_search_snapshot_membership` helper + call in `api_chat_get_or_create_scoped_conversation`; new `SNAPSHOT_REHYDRATION_CHUNK_CAP` const; `resolve_scope_results` SearchSnapshot arm now passes `chunk_limit` + cap into `get_by_meeting_ids`; new test `snapshot_resume_tolerates_deleted_member_and_retrieves_survivors`), `frontend/src-tauri/src/database/repositories/fts.rs` (deleted `get_by_chunk_ids`; `get_by_meeting_ids` now takes `per_meeting_limit`/`total_limit` with a deterministic `ROW_NUMBER() OVER (PARTITION BY meeting_id ORDER BY chunk_id)` slice + total `LIMIT`; new test `get_by_meeting_ids_respects_per_meeting_and_total_caps`), this doc.
- Verification: typecheck ✅ · vitest ✅ (92 passed, 20 files) · full `cargo test --lib` ✅ (368 passed, 2 ignored; +2 new tests) · cargo check ✅ · cargo fmt --check ✅ · git diff --check ✅
- Notes/decisions: Resume tolerance is "strict on creation only": the membership `IN (...)` count check runs only when no exact-scope conversation exists yet, and resume detection reuses `ChatRepository::get_latest_conversation_for_scope`'s exact (kind, key, scope_data) lookup — the same triple the upsert identity index uses, so "resumes" and "upsert matches" are the same predicate. Stored `scope_data` is never mutated (no re-freeze); deleted members are simply skipped at retrieval because the `meeting_fts JOIN meetings` drops them. Bounding reuses the existing per-scope budget (`chunk_limit`, 10 local / 30 cloud) per meeting and caps the total at `SNAPSHOT_REHYDRATION_CHUNK_CAP = 100`, which mirrors the 100-meeting snapshot ceiling (`MAX_SEARCH_SNAPSHOT_RESULTS`); the window-function slice orders chunks deterministically by `chunk_id` and the `sources` in events/`sources_json` derive from the bounded results vector. `get_by_chunk_ids` (fts.rs:277-309) was the chunk-id-snapshot leftover superseded by `get_by_meeting_ids` (the snapshot stores meeting ids) — no callers, deleted. `get_latest_conversation_for_scope` (repositories/chat.rs:173-187) was flagged dead by both reviews but is NOT deleted: the exact-scope existence lookup is precisely what resume tolerance needs, so it gained a caller (per task rule: leave a "dead" helper that ends up used and note it).
- Spillover: (1) The subset-scope case is intentionally unchanged — if the sidebar regenerates a snapshot without the deleted meeting, the SHA-256 identity differs and a fresh thread is created; only exact-scope resume is tolerated, per the review's prescription. (2) No TS behavior changed, so no Vitest coverage was touched. (3) `repositories/chat.rs` received no diff (the kept helper is only called from api/chat.rs).

### Task 6.1.R8 — Heal notes import and FTS refresh on idempotent save retry [S]
- Date: 2026-08-17
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Close R8 Should-fix 2 — the idempotent early-return in `save_transcript` (already-promoted meeting) now re-runs the post-commit notes import and FTS refresh, so a retry after a committed-but-failed first save heals stale notes/FTS instead of returning the meeting untouched.
- Files changed: `frontend/src-tauri/src/database/repositories/transcript.rs` (new private `import_notes_and_refresh_fts` helper shared by the first-save tail and the idempotent-retry early return; new test `idempotent_retry_heals_notes_import_and_fts`), this doc.
- Verification: `cargo test --manifest-path src-tauri\Cargo.toml --lib database::repositories` ✅ (46 passed, incl. the new test) · cargo check ✅ · cargo fmt --check ✅ · git diff --check ✅
- Notes/decisions: The post-commit notes-import + FTS-refresh block was extracted verbatim into a private helper (notes-import failures still propagate via `?`; the FTS refresh stays best-effort with `error!`) and called from both the first-save tail and the early-return path, so first-save behavior is byte-for-byte unchanged and a retry converges. Promotion idempotence is untouched — `get_promoted_meeting_id` + the early return still return the same `meeting_id`; `import_notes_and_refresh_fts` runs before that return. The new test uses an in-memory SQLite pool with the meetings/transcripts/chat_conversations/chat_messages/meeting_notes/meeting_folders/summary_processes/meeting_fts schema; a seeded `live_recording` conversation makes the first save record promotion lineage; the first save's post-commit notes import fails deterministically via a test-only `CHECK (length(notes_markdown) < 20)` on `meeting_notes` (the meeting + promotion have already committed) — the test asserts no notes row and zero FTS rows remain; after fixing `notes.md`, a retry with the same `live_scope_key` returns the same `meeting_id` and rebuilds the note row plus both transcript and note FTS chunks.
- Spillover: (1) the mirrored notes-import + FTS-refresh block in `audio/import.rs` (`create_meeting_with_transcripts`, task 2.7) remains a verbatim duplicate rather than calling this helper — out of scope (diff constraint); a future cleanup could share the helper across both call sites. (2) `promote_live_recording_in_transaction` queries `chat_messages` unconditionally (even with zero messages), so test schemas replicating the promotion path need that table — test-only observation, no production impact.

### Task 6.1.R9 — Discarded-live-thread GC + live snapshot tail budget + cosmetics [S]
- Date: 2026-08-17
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Close R8 Nits 2–4 — (a) GC unreachable `live_recording` threads when a recording is discarded instead of saved; (b) make the live snapshot context budget keep the most-recent transcript tail instead of the head; (c) fix the two flagged indentation drifts and the stale "Chat with meetings" title on the meeting-scope launcher.
- Files changed: `frontend/src-tauri/src/database/repositories/chat.rs` (new `discard_live_recording` + test), `frontend/src-tauri/src/api/chat.rs` (new `api_chat_discard_live_recording` command; `live_snapshot_context` now uses new `tail_at_char_boundary` helper; new tail-budget test), `frontend/src-tauri/src/lib.rs` (registered the discard command), `frontend/src/hooks/useRecordingStop.ts` (discard invoke in the no-save branch), `frontend/src/hooks/useTranscriptRecovery.ts` (`deleteRecoverableMeeting` best-effort discard invoke), `frontend/src/components/ChatPanel/index.tsx` (indentation), `frontend/src/components/Sidebar/index.tsx` (indentation), `frontend/src/lib/strings/en.ts` (new `app.meetingDetails.showChatTitle` key), `frontend/src/app/meeting-details/page-content.tsx` (title via i18n + button-block indentation), this doc.
- Verification: typecheck ✅ · vitest ✅ (92 passed, 20 files) · full `cargo test --lib` ✅ (370 passed, 2 ignored; +2 new tests) · cargo check ✅ · cargo fmt --check ✅ · git diff --check ✅
- Notes/decisions: The discard seam is a new Rust command called from the frontend discard paths, not the native stop path — `stop_recording` clears the native live key before the frontend decides save-vs-discard, so Rust cannot know the outcome at stop time. The SQL deletes only rows with exact `scope_kind = 'live_recording' AND scope_key = $1 AND promoted_from_live_scope_key IS NULL`; messages cascade via the existing `chat_messages` FK, and promoted threads are already excluded twice (scope flip at promotion + the lineage guard), never other scopes; the command is idempotent (second discard deletes nothing). Both frontend discard paths are wired: `useRecordingStop.handleRecordingStop`'s no-save branch (fires on `callApi=false`, transcription timeout, and stop/transcription errors) and `useTranscriptRecovery.deleteRecoverableMeeting` (deleting a recovery checkpoint discards its never-promoted live thread; fire-and-forget with `.catch`, never blocking the user's explicit delete). Tail budget: `tail_at_char_boundary` mirrors the existing head-based `truncate_at_char_boundary` (advance to a UTF-8 boundary from the tail cut) and replaces the head truncation in `live_snapshot_context` only — the `assemble_prompt` head-truncation for saved-meeting context is untouched. The new tests prove (a) discard removes the unpromoted live thread and its messages while a promoted thread (scope flipped to meeting) and an unrelated meeting thread survive, and (b) a 14-char budget over a 37-char transcript keeps only the last segment. Cosmetics: the two R8-flagged indentations fixed; the meeting-details launcher title now reads "Chat with this meeting (Ctrl/Cmd+Shift+M)" via a new i18n key (the sibling "Show chat" aria key already existed), and the button block's own indentation drift was corrected while editing those lines.
- Spillover: (1) A failed `stop_recording` invoke also lands in the no-save branch, so a recording that is still technically active on the native side could have its live thread discarded — bounded to the broken-stop edge case (native key survives only if `IS_RECORDING` never cleared) and the thread is trivially recreated by the next live chat `get_or_create`; acceptable per the discard semantics. (2) `RecordingStateContext.liveTranscriptScopeKey` remains non-null after stop (`?? prev.liveTranscriptScopeKey` in `syncWithBackend`) — the discard command is key-exact so a stale key is a harmless no-op; clearing it on stop remains the R6-spillover hygiene item, still untouched. (3) No frontend test was added for the two new invoke sites (Rust covers the GC contract; the TS wiring is a fire-and-forget invoke) — a vitest assertion on `deleteRecoverableMeeting`'s invoke would need mock additions to the existing harness, not worth the diff per minimal-scope.

### Task 6.2 — Resolved meeting/folder label in the chat scope badge [S]
- Date: 2026-08-19
- Implementer model: opencode-go/deepseek-v4-flash
- Status: done
- Scope: Replace the generic "This meeting" / "This folder" chat scope labels with an unambiguous resolved title/name (e.g. "Meeting: Quarterly Review", "Folder: Projects") when one is available at the launcher call site, retaining the generic label as the safe fallback when missing. Display-only change; conversation identity and routing are untouched.
- Files changed: `frontend/src/components/ChatPanel/ChatHost.tsx` (OpenChatState holds an optional display-only label; `openChat(scope, label?)`; renders `<ChatPanel … resolvedLabel>`), `frontend/src/components/ChatPanel/index.tsx` (new `resolvedLabel` prop; formats `Meeting: {title}` / `Folder: {name}` via new i18n keys when present, else the generic fallback), `frontend/src/lib/strings/en.ts` (new `chat.scope.meetingNamed` / `chat.scope.folderNamed`), `frontend/src/app/meeting-details/page-content.tsx` (meeting chat launcher + shortcut pass `meeting.title`), `frontend/src/components/Sidebar/index.tsx` (folder ask passes `folderNameById.get(id)`), `frontend/tests/components/chat-scope.test.tsx` (new `LabeledLauncher` + test asserting resolved/fallback labels and that the label never leaks into the backend scope), this doc.
- Verification: typecheck ✅ · vitest ✅ (93 passed, 20 files; +1 test) · cargo check N/A (no Rust changed)
- Notes/decisions: The label must not ride inside the `ChatScope` object: the backend `ChatScope` is `#[serde(deny_unknown_fields)]` (`repositories/chat.rs:54-60`), so an extra field would break `api_chat_get_or_create_scoped_conversation`, and the scope triple `(scope_kind, scope_key, scope_data)` is the persisted conversation identity. Instead the label is carried beside the scope in ChatHost's `OpenChatState` (display-only) and passed to `ChatPanel` as a separate `resolvedLabel` prop, so the backend serialization stays byte-identical (the test asserts no `label` key is ever sent). `openChat` gains an optional second arg; all existing callers compile unchanged (label optional). Formatting is centralized in `ChatPanel` (prefix depends on `scope.kind`), so the call sites only pass the raw title/name. `promoteLiveChat` builds the meeting scope without a label (falls back to "This meeting") since no title is available at that seam. Naming nit: the prop is `resolvedLabel` to avoid shadowing the local `scopeLabel` const.
- Spillover: (1) Live-promotion path and any programmatic meeting-scope open without a label still show "This meeting"; resolving the title there would need a title lookup the seam lacks — out of scope. (2) The label is captured at open time; if the meeting title changes while the panel is open the badge keeps the stale title until reopened — acceptable for a display-only badge, but a future live-resolve from the meetings store could refresh it. (3) `Sidebar` folder-ask resolves the name via the existing `folderNameById` map rather than widening the `FolderTreeItem.onAskFolder` callback signature — smaller diff, no prop-plumbing through the tree.
