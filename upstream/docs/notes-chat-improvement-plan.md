# Notes & Chat — Product/UX Analysis + Improvement Plan

> Analysis of the two most "bolted-on" features in the app, and a phased plan to make them first-class. All paths relative to `upstream/frontend/`. Findings cite file:line evidence gathered via static analysis (read-only).

## Overview

Meetly's core loop is **record → transcribe → summarize → act**. Notes and chat are the two features that should close that loop (capture user intent during the meeting, retrieve intelligence after it), but both are half-finished:

- **Notes** = two ~80%-duplicate plain-textarea panels backed by one SQLite column — and the flagship flow ("take notes while recording") **silently loses data**.
- **Chat** = a single-shot, non-streaming keyword-RAG drawer that ignores the meeting you're looking at, can't follow up on itself, and forgets everything when closed.

Meanwhile, the building blocks for the fixes **already exist in the codebase**: BlockNote editor (installed, used only for summaries), `notes_json` column (never written), `delete_meeting_notes` command (registered, never called), `react-markdown` + `remark-gfm` deps (installed, imported nowhere), cancellation-token support in the LLM client (chat passes `None`), FTS5 index over transcripts+summaries+notes, and `ChatSource` payloads that already carry `meetingId`/`snippet` the UI throws away. Most wins are wiring, not new infrastructure.

### Severity map

| # | Severity | Finding |
|---|----------|---------|
| 1 | 🔴 data loss | Notes typed during a recording are never saved to the DB |
| 2 | 🔴 data loss | Load failure → empty editor overwrites saved notes on next autosave |
| 3 | 🔴 data quality | FTS index never refreshed on note save/delete → stale search/chat/MCP |
| 4 | 🟠 bug | Recording notes draft leaks across meetings (global sessionStorage key, never cleared) |
| 5 | 🟠 bug | Chat error bubbles are fed back to the model as history |
| 6 | 🟠 bug | Timeout error says "60 seconds"; actual timeout is 300s (`llm_client.rs:8` vs `:280`) |
| 7 | 🟠 UX | Chat: no streaming, no stop button, input disabled while waiting |
| 8 | 🟠 UX | Chat is context-blind about the open meeting (no `meetingId` prop at all) |
| 9 | 🟠 UX | Chat answers render raw markdown (`**`, `-`) as plain text |
| 10 | 🟠 UX | Chat sources are non-clickable; conversation destroyed on close |
| 11 | 🟡 UX | Notes: bare textarea while BlockNote is installed; `notes_json` always null |
| 12 | 🟡 UX | No notes-delete UI, no "has notes" indicator, no last-saved time, no Ctrl+S |
| 13 | 🟡 UX | Notes/chat only reachable via two unlabeled 16px gray icons; no deep links |
| 14 | 🟡 tech debt | Dead code: `/notes/[id]` demo route, `BasicBlockNoteTest.tsx`, `MainNav` (`h-0`), `Editor.tsx` render-time logging |

---

## Part 1 — Analysis

### 1.1 Notes feature: current state

**There are three unrelated things named "notes"** — a design smell in itself:

| Thing | Where | Status |
|-------|-------|--------|
| Per-meeting notes (`meeting_notes` table, 1:1 with meetings) | `NotesPanel.tsx`, `RecordingNotesPanel.tsx` | Real, half-finished |
| `MeetingNotes` section key inside summary JSON | `useMeetingData.ts:103-108`, `export/commands.rs:323-448` | Summary-editor domain |
| `/notes/[id]` demo route | `src/app/notes/[id]/page.tsx` | Dead — 4 hardcoded sample notes, no inbound links anywhere |

**Data model** (`migrations/20251223000000_add_meeting_notes.sql`): one row per meeting — `notes_markdown`, `notes_json` (never written), `created_at`, `updated_at`. No titles, versions, tags, standalone notes. File mirror: `notes.md` exported to the meeting folder on save (fire-and-forget; DB is source of truth).

**Flows that exist:**
1. Notes during recording — panel auto-opens (`page.tsx:89-94`), textarea, 2s autosave to `notes.md` in the recording folder only.
2. Notes after the meeting — meeting-details → sticky-note icon → `NotesPanel` loads from DB, autosave (2s debounce + blur) + manual Save.
3. **Notes feed the AI summary** — injected into the LLM prompt (`summary/service.rs:551-565`), and editing notes invalidates the summary cache. This is a genuinely differentiating behavior that is **completely invisible to users** (nothing mentions it).
4. Notes are FTS-indexed (`chunk_type='note'`) and reachable by chat/MCP — when the index is fresh (it usually isn't; finding #3).

**Critical finding #1 — the recording→DB bridge does not exist.** `RecordingNotesPanel.tsx:19-24` documents the intended design ("sessionStorage as the bridge to persist them to the meetings_notes DB table on stop (see useRecordingStop)"), but `useRecordingStop.ts` (471 lines) contains **zero** references to notes or `recording_notes_draft`, and `save_transcript` never reads `notes.md`. Result: stop recording → meeting-details notes panel opens **empty**; the user's notes survive only as a file in the folder. The core "take notes during the meeting" flow silently loses data.

**Critical finding #2 — draft leak.** The sessionStorage key is global (`recording_notes_draft`, `RecordingNotesPanel.tsx:12`) and never cleared anywhere. Meeting B's notes panel pre-fills with meeting A's notes.

**Critical finding #3 — stale FTS.** `save_meeting_notes` (`database/commands.rs:307-344`) does not call `FtsRepository::refresh_meeting`, unlike transcript saves (`repositories/transcript.rs:94`) and summary saves (`repositories/summary.rs:494`). Every note save makes search, chat-with-meetings and MCP results outdated until a manual `api_rebuild_fts_index` — which no UI surface exposes.

**Editor gap:** summaries get BlockNote 0.36.0 (toolbar, slash menu, tables, images); notes get a `<textarea>`. `notes_json` was designed for BlockNote JSON and is always written `null` (`NotesPanel.tsx:84`). The markdown round-trip helper (`lib/blocknote-markdown.ts`) is already built and tested.

**Smaller findings:** compact mode (<320px) hides save-status entirely (`NotesPanel.tsx:34-36`); recording-panel save errors are logged but not toasted (vs `NotesPanel.tsx:90` which does toast); no delete/clear UI despite working backend command; `created_at`/`updated_at` fetched but never rendered; no Ctrl+S; no unmount flush (last keystrokes within the 2s debounce can be dropped on meeting switch); the two panels diverge visually (one monospace-ish, one not); resize width not persisted (`usePanelResize.ts:20-24`); icon buttons lack `aria-label`; all strings hardcoded English while the sidebar is hardcoded Portuguese.

### 1.2 Chat feature: current state

**What it is:** a fixed 320px bottom drawer on the meeting-details page (the *only* mount point) that does keyword-RAG Q&A over **all** meetings. Landed as dogfooding-stage work in a snapshot commit together with FTS5.

**Request flow:**

```
ChatPanel.handleSend
  → api_chat_with_meetings { query, history: last 10 msgs }
      → FTS5 search: raw question as query, top-10 BM25 chunks across ALL meetings
        (sanitize_fts_query OR-joins every word incl. stopwords)
      → each chunk = ~48-token snippet() excerpt with <mark> tags
      → one flattened user message: history lines + question + context
      → generate_summary(...) — non-streaming, cancellation = None
  → append full answer at once
```

**Strengths worth keeping:** all 7 LLM providers work for chat (incl. fully local), independent chat model config with graceful fallback (`setting.rs:511-526`), FTS5+BM25 foundation, good API-key error guidance, MCP parity (`mcp/server.rs`), 19 backend unit tests across chat/fts/context.

**Finding #7 — waiting experience:** no streaming anywhere (`generate_summary` is a blocking POST; frontend `await`s the whole invoke). On the default local model (Ollama `llama3.2`) this is 10–60s of a static "Searching meetings..." spinner. No stop button (cancellation token exists in `llm_client.rs:126`; chat passes `None` at `chat.rs:169`). Input is *disabled* while waiting, so you can't even draft the next question. Tauri event streaming infra exists elsewhere in the app (model downloads), so this is a gap, not a platform limit.

**Finding #8 — context-blind:** `ChatPanel` receives no `meetingId` (`index.tsx:10-12`) even though it only mounts on a meeting page. "What were the action items?" searches all meetings by keyword instead of the meeting in front of you.

**Follow-ups break retrieval:** FTS query = raw latest question only (`chat.rs:57`). No query rewriting, so "And what did she say about the budget?" retrieves nothing. History is flattened text inside the user prompt (`chat.rs:143-147`), not a real message array.

**Finding #9/10 — rendering & affordances:** answers render as `whitespace-pre-wrap` plain text (`ChatMessage.tsx:30`) while LLMs emit markdown — users see raw `**`. `react-markdown`+`remark-gfm` are already in `package.json:87-88`, imported nowhere. Sources chips show only title/folder; `meetingId`, `chunkType`, `snippet` are in the payload (`types/index.ts:193-199`) but discarded — no jump-to-meeting, no snippet preview. No copy/regenerate/retry/edit-resend. Conversation is component state — closing the drawer destroys it; there is no chat table in any migration.

**Smaller findings:** single-line input (no multiline questions); no suggested prompts in empty state; no model/provider indicator in the panel; raw error strings surfaced verbatim with no Settings deep-link; no aria-labels, no `aria-live`, no Esc-to-close; full user queries logged at INFO level (`chat.rs:47-52`) with no disclosure of what leaves the machine — off-brand for a privacy-first product; the `folder:"Name"` operator exists (`fts.rs:46-79`) but is undocumented in the UI.

### 1.3 Integration & information architecture

- Meeting-details is a panel workspace, not tabs: transcript (left, resizable) | summary (center) | notes (right, optional) + chat (bottom drawer). Notes/chat visibility is component-local boolean state — **no deep links, resets closed on every visit**.
- The only entry points to notes/chat are two unlabeled, low-contrast 16px gray icons floating over the summary header (`page-content.tsx:386-405`). Poor discoverability.
- No chat from sidebar search ("Ask AI about these results" is an obvious win — search already returns `FtsSearchResult[]`), none from home, none during live recording.
- Transcript column vanishes below `md` (768px) with no fallback.
- The original FTS5/chat plan (`docs/fts5-search-mcp-plan.md`) decided "Chat panel placement: Sidebar tab + meeting-details collapsible panel" — the sidebar tab was never built.

---

## Part 2 — Improvement Plan

Phased by risk/value. Sprints 1–2 are correctness; 3–4 are the big UX wins; 5+ is feature growth. Each item lists effort (S ≤ half day, M ≤ 2 days, L > 2 days) and primary files.

### Sprint 1 — Stop losing data (P0 correctness)

| Item | Effort | What / how |
|------|--------|-----------|
| 1.1 Persist recording notes to DB | M | On recording stop: read `recording_notes_draft` (or `notes.md` from the recording folder) and pass into the save flow so `save_transcript`/`save_meeting_notes` upserts it before the user navigates away. Hook: `useRecordingStop.ts` (the place the docstring already promises). Clear the draft key after persistence. |
| 1.2 Per-meeting draft key | S | Scope sessionStorage key by meeting/recording id; delete on stop (`RecordingNotesPanel.tsx:12`). Fixes cross-meeting leak. |
| 1.3 Refresh FTS on note write | S | Call `FtsRepository::refresh_meeting` in `save_meeting_notes` / `delete_meeting_notes` (`database/commands.rs:307-373`), matching `transcript.rs:94`. |
| 1.4 Guard load-error overwrite | S | If `get_meeting_notes` fails: render read-only error state with Retry, don't mount an editable editor with `lastSavedRef=""` (`NotesPanel.tsx:55-62`). |
| 1.5 Unmount flush | S | Flush pending autosave on unmount + `beforeunload`; keep blur-save. |
| 1.6 Chat hygiene | S | Drop error bubbles from history sent to the model (`ChatPanel/index.tsx:42`); fix "60 seconds" → 300s message (`llm_client.rs:280,293`). |

**Exit criteria:** notes survive a record→stop→reopen cycle; a note save is immediately searchable; a failed load can never clobber saved notes.

### Sprint 2 — Notes panel trust & parity (P1)

| Item | Effort | What / how |
|------|--------|-----------|
| 2.1 One notes panel | M | Merge `RecordingNotesPanel` + `NotesPanel` into one component parameterized by context (recording folder vs meeting id). Same save-status, toasts, font everywhere. |
| 2.2 Notes menu | S | Wire existing `delete_meeting_notes` into a panel kebab menu (clear notes); show `updated_at` as "Saved HH:mm". |
| 2.3 "Has notes" indicator | S | Dot on the sticky-note toggle when notes exist; join `meeting_notes` in `api_get_meeting_metadata` or return `has_notes` flag. |
| 2.4 Keyboard + a11y pass | S | Ctrl/Cmd+S to save; aria-labels on icon buttons; `aria-live` on save status. |
| 2.5 Empty-state copy | S | Mention that notes are incorporated into the AI summary (the hidden differentiator). |
| 2.6 Delete dead code | S | Remove `/notes/[id]` demo route (+ its static-params hack), `BasicBlockNoteTest.tsx`, render-time logging in `BlockNoteEditor/Editor.tsx`. |

### Sprint 3 — Chat that feels alive (P1, highest perceived-quality ROI)

| Item | Effort | What / how |
|------|--------|-----------|
| 3.1 Streaming responses | M | Backend: stream chunks via Tauri events (pattern exists in `summary_engine/commands.rs:154-236`) with `stream: true` per provider in `llm_client.rs`. Frontend: render tokens as they arrive. |
| 3.2 Stop button | S | Pass a real `CancellationToken` instead of `None` (`chat.rs:169`); stop button cancels and keeps partial answer. |
| 3.3 Markdown rendering | S | `react-markdown` + `remark-gfm` are already installed — render answers (and strip `<mark>` from any shown snippet). |
| 3.4 Clickable sources | S | Use `meetingId`/`snippet` already in `ChatSource`: jump to `/meeting-details?id=…`, hover/expand snippet. Closes the RAG loop. |
| 3.5 "This meeting" scope | M | Pass `meetingId` prop; scope toggle "This meeting / All meetings". This-meeting mode: prefer/boost that meeting's chunks or inject its summary+notes directly. |
| 3.6 Multiline input + copy answer | S | Textarea with Enter=send/Shift+Enter=newline; copy button on assistant bubbles (reuse summary copy utilities). |
| 3.7 Model visibility | S | Show active provider/model in the panel header; deep-link to Settings → Chat tab on "no model configured" error. |

### Sprint 4 — Chat memory & retrieval quality (P2)

| Item | Effort | What / how |
|------|--------|-----------|
| 4.1 Conversation persistence | M | `chat_conversations` + `chat_messages` tables; per-meeting thread resume; clear-chat button; list/rename/delete later if needed. |
| 4.2 Query rewriting for follow-ups | M | Before FTS, one cheap LLM call (or heuristics) to resolve pronouns/ellipsis against history into a standalone search query. |
| 4.3 Retrieval depth | M | AND/phrase matching option, expand hits to full transcript segments around the snippet, raise chunk budget for large-context models; context-window guard for small local models. |
| 4.4 Suggested prompts | S | Starter questions in empty state (e.g. "What were the action items?", "Summarize decisions", scoped variants when a meeting is open). |
| 4.5 Privacy polish | S | Stop logging full queries at INFO (`chat.rs:47-52`); show which provider is cloud vs local in the panel (privacy-first positioning). |

### Sprint 5 — Notes as a real editor (P2)

| Item | Effort | What / how |
|------|--------|-----------|
| 5.1 BlockNote for notes | M | Swap textarea → existing `BlockNoteEditor` wrapper (same as summaries); persist to `notes_json` + markdown export (round-trip helper exists, tested). |
| 5.2 Ctrl+S, shortcuts | S | Editor-level save shortcut; panel-toggle shortcut. |
| 5.3 Persist panel width | S | Extend `usePanelResize` with localStorage (docstring already admits the gap). |
| 5.4 i18n groundwork | M | Extract notes/chat strings; align with sidebar language (currently hardcoded PT-BR vs EN mix). Flag for app-wide decision. |

### Sprint 6 — Growth features (P3, candidates for ROADMAP.md)

- **Actions on chat answers:** "Add to notes" (append via `save_meeting_notes`), "Insert into summary", copy. Bridges chat → notes → summary loop.
- **Note↔transcript links:** timestamp chips in notes that jump to the transcript/audio moment.
- **Note templates** (agenda, action items, Cornell) — pair with F1 template work.
- **Notes export** — include `meeting_notes` in PDF/DOCX export once F2/F3 land (`export/commands.rs` currently only merges summary-JSON sections).
- **More chat entry points:** sidebar search "Ask AI about results", during live recording ("what's been said so far?").
- **Standalone notes / quick capture** (notes library not tied to a meeting) — biggest scope; only after Sprints 1–5 prove out.
- **Semantic search** (embeddings/hybrid) — long-term retrieval upgrade.

### Sequencing notes

- Sprints 1–2 are pure correctness/trust; do them before marketing the notes feature at all (today it loses data).
- Sprint 3 is the biggest *perceived* quality jump per unit effort — most items reuse already-installed deps or existing infra.
- Nothing here needs new Rust crates except possibly Sprint 3.1 if per-provider SSE parsing gets ugly; prefer reusing existing event patterns.
- Verify each sprint per repo convention: `pnpm run typecheck`, `npx vitest run`, `cargo check` (with `CARGO_TARGET_DIR` outside OneDrive), plus a manual record→notes→chat smoke pass on Windows.

---

## Appendix — Key evidence files

| Area | Files |
|------|-------|
| Notes UI | `src/components/MeetingDetails/NotesPanel.tsx`, `src/components/RecordingNotesPanel.tsx`, `src/app/notes/[id]/page.tsx` (dead) |
| Notes backend | `src-tauri/src/database/repositories/meeting_notes.rs`, `database/commands.rs:296-373`, `audio/recording_commands.rs:970-987`, `migrations/20251223000000_add_meeting_notes.sql` |
| Notes↔summary | `src-tauri/src/summary/service.rs:551-582`, `summary/processor.rs:55-61` |
| Chat UI | `src/components/ChatPanel/{index,ChatMessage}.tsx`, `src/app/meeting-details/page-content.tsx:381-405`, `src/components/ChatModelSettings.tsx` |
| Chat backend | `src-tauri/src/api/chat.rs`, `export/context.rs`, `summary/llm_client.rs:113-341`, `database/repositories/fts.rs`, `migrations/20260727000001_add_chat_model_config.sql` |
| Layout/nav | `src/app/meeting-details/page-content.tsx`, `src/components/Sidebar/index.tsx`, `src/hooks/usePanelResize.ts` |
