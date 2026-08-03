# FTS5 Search + Context Builder + Chat + MCP — Implementation Plan

## Overview

Replace the current `LIKE`-based transcript search with a proper FTS5 full-text search index, add an LLM-powered "Chat with Meetings" feature, and expose all search/context/chat functions via a minimal MCP server over HTTP.

### Why this matters

- Current search is a `LIKE '%query%'` full table scan on transcripts only — no index, no summaries, no notes, no folder filtering.
- No "Chat with meetings" feature exists (README says "Coming Soon").
- No way for external AI agents to access meeting data.
- meetily-memory (external Python tool) does the same job but doesn't know about our DB-only logical folders (`folder_id`/`meeting_folders` table).

### What we're building

| Phase | What | Why |
|-------|------|-----|
| 1 | FTS5 index + repository | Foundation for search, context, chat, MCP |
| 2 | Context builder | Markdown output for copy/paste into any LLM |
| 3 | Chat with meetings | GUI chat panel + LLM integration |
| 4 | MCP server | External AI agent access |
| 5 | Frontend integration | Wire everything into sidebar + meeting details ✅ |

### Decisions locked in

| Decision | Choice | Rationale |
|----------|--------|-----------|
| MCP implementation | Minimal: `axum` + `serde_json` + JSON-RPC 2.0 | No SDK bloat, we already have tokio runtime |
| Chat panel placement | Sidebar tab + meeting-details collapsible panel | Both global and per-meeting access |
| Folder filter UX | Checkbox tree + `folder:"name"` search operator | Discoverable + power-user friendly |
| FTS5 tokenizer | `unicode61` (no stemming) | Portuguese needs exact matches; porter stems aggressively |
| Dependencies | No new Rust crates except `axum` | Reuse existing `rusqlite` bundled FTS5, `llm_client.rs`, `serde_json` |

---

## Architecture

```
                    ┌─────────────────────────────────────────────┐
                    │            Personal Meetly (Tauri)          │
                    │                                             │
                    │  ┌──────────┐  ┌──────────┐  ┌──────────┐  │
                    │  │ FTS5     │  │ Context  │  │ MCP      │  │
                    │  │ Index    │  │ Builder  │  │ Server   │  │
                    │  │ (fts.rs) │  │(ctx.rs)  │  │(mcp/)    │  │
                    │  └────┬─────┘  └────┬─────┘  └────┬─────┘  │
                    │       │             │             │         │
                    │       │      ┌──────┴──────┐      │         │
                    │       │      │  Chat Cmd   │      │         │
                    │       │      │ (chat.rs)   │      │         │
                    │       │      └──────┬──────┘      │         │
                    │       │             │             │         │
                    │       │      ┌──────┴──────┐      │         │
                    │       │      │  LLM Client │      │         │
                    │       │      │(llm_client) │      │         │
                    │       │      └─────────────┘      │         │
                    │  ┌────┴──────────────────────────┴─────┐   │
                    │  │       SQLite: meeting_minutes.db     │   │
                    │  │  (meetings, transcripts, summaries,  │   │
                    │  │   notes, meeting_folders, meeting_fts)│   │
                    │  └──────────────────────────────────────┘   │
                    └─────────────────────────────────────────────┘
                              ↑                    ↑
                     Tauri IPC (GUI)        HTTP POST /mcp
                                              (MCP agents)
```

---

## Sprint Plan

### Sprint 1: FTS5 Foundation (Phase 1)

**Goal:** FTS5 virtual table populated from existing data, search function with folder filtering, hooks into write paths.

**Files:**
| File | Action | Purpose |
|------|--------|---------|
| `frontend/src-tauri/migrations/20260727000000_add_fts5.sql` | New | FTS5 virtual table + initial population |
| `frontend/src-tauri/src/database/repositories/fts.rs` | New | `search()`, `refresh_meeting()`, `remove_meeting()`, `rebuild_index()`, `sync_folder()` |
| `frontend/src-tauri/src/database/repositories/transcript.rs` | Modify | Call `fts::refresh_meeting()` after `save_transcript()` |
| `frontend/src-tauri/src/database/repositories/summary.rs` | Modify | Call `fts::refresh_meeting()` after `update_process_completed()` |
| `frontend/src-tauri/src/database/repositories/meeting.rs` | Modify | Call `fts::remove_meeting()` on delete |
| `frontend/src-tauri/src/database/repositories/folder.rs` | Modify | Call `fts::sync_folder()` after folder CRUD |
| `frontend/src-tauri/src/api/api.rs` | Modify | New commands: `api_search_fts`, `api_rebuild_fts_index` |
| `frontend/src-tauri/src/lib.rs` | Modify | Register new commands |

**Runnable check:** Test that creates a meeting with transcript + summary + note, searches for a keyword, asserts FTS result contains meeting title, chunk_type, snippet with `<mark>`, and folder_name.

### Sprint 2: Context Builder (Phase 2)

**Goal:** Markdown context output from FTS results, ready to paste into any LLM chat.

**Files:**
| File | Action | Purpose |
|------|--------|---------|
| `frontend/src-tauri/src/export/context.rs` | New | `build_context_markdown()` |
| `frontend/src-tauri/src/api/api.rs` | Modify | New command: `api_build_context` |
| `frontend/src-tauri/src/lib.rs` | Modify | Register command |

**Runnable check:** Test that builds context from mock FTS results, asserts Markdown contains meeting title, folder name, speaker, and source citation.

### Sprint 3: Chat with Meetings (Phase 3)

**Goal:** GUI chat panel + LLM integration using existing `llm_client.rs`.

**Files:**
| File | Action | Purpose |
|------|--------|---------|
| `frontend/src-tauri/src/api/chat.rs` | New | `api_chat_with_meetings` Tauri command |
| `frontend/src-tauri/src/api/api.rs` | Modify | Register chat command |
| `frontend/src-tauri/src/lib.rs` | Modify | Register command |
| `frontend/src/components/ChatPanel/index.tsx` | New | Chat UI with message list + input |
| `frontend/src/components/ChatPanel/ChatMessage.tsx` | New | Message bubble with source badges |
| `frontend/src/components/Sidebar/index.tsx` | Modify | Add Tabs: "Meetings" / "Chat" |
| `frontend/src/components/Sidebar/SidebarProvider.tsx` | Modify | Add chat state to context |
| `frontend/src/app/meeting-details/page-content.tsx` | Modify | Add ChatPanel as collapsible right panel |
| `frontend/src/types/index.ts` | Modify | Add `FtsSearchResult`, `ChatMessage` types |

**Runnable check:** Test that mocks FTS results, calls chat function, asserts LLM receives context prompt containing search results and returns answer + sources.

### Sprint 4: MCP Server (Phase 4)

**Goal:** HTTP JSON-RPC server exposing search/context/chat/list-folders to external agents.

**Files:**
| File | Action | Purpose |
|------|--------|---------|
| `frontend/src-tauri/src/mcp/mod.rs` | New | MCP module |
| `frontend/src-tauri/src/mcp/server.rs` | New | JSON-RPC HTTP server on `axum` |
| `frontend/src-tauri/Cargo.toml` | Modify | Add `axum = "0.7"` |
| `frontend/src-tauri/src/lib.rs` | Modify | Start MCP server in `.setup()` |

**Runnable check:** Test that starts server on random port, sends `tools/list`, asserts 4 tools returned. Sends `tools/call` with `search_meetings`, asserts FTS results.

### Sprint 5: Frontend Integration (Phase 5)

**Goal:** Wire FTS into sidebar search, add folder checkbox filter, polish chat panels.

**Files:**
| File | Action | Purpose |
|------|--------|---------|
| `frontend/src/components/Sidebar/index.tsx` | Modify | Replace `api_search_transcripts` with `api_search_fts`, add folder filter UI |
| `frontend/src/components/Sidebar/SidebarProvider.tsx` | Modify | Update search to use FTS, add folder filter state |
| `frontend/src/components/Sidebar/FolderFilterTree.tsx` | New | Checkbox tree for folder filtering |
| `frontend/src/types/index.ts` | Modify | Add `FolderFilter` type |

**Runnable check:** Manual test — search with folder filter, verify results show chunk_type badge + folder name, verify `folder:"name"` operator works.

---

## Todo List

- [x] **Sprint 1: FTS5 Foundation**
  - [x] 1.1 Create migration `20260727000000_add_fts5.sql`
  - [x] 1.2 Create `database/repositories/fts.rs` — search, refresh, remove, rebuild, sync_folder
  - [x] 1.3 Hook `fts::refresh_meeting()` into `transcript.rs::save_transcript()`
  - [x] 1.4 Hook `fts::refresh_meeting()` into `summary.rs::update_process_completed()`
  - [x] 1.5 Hook `fts::remove_meeting()` into `meeting.rs` delete path
  - [x] 1.6 Hook `fts::sync_folder()` into `folder.rs` CRUD operations
  - [x] 1.7 Add Tauri commands `api_search_fts`, `api_rebuild_fts_index` to `api.rs`
  - [x] 1.8 Register commands in `lib.rs`
  - [x] 1.9 Write test: create meeting + search + assert result fields
  - [x] 1.10 Run `cargo test -p meetily --lib export` + `cargo check`
  - [x] 1.11 **Sprint 1 Code Review** — run subagent, update plan with findings
  - [x] 1.12 **Sprint 1 checkpoint** — ask user for doubts/additions

- [x] **Sprint 2: Context Builder**
  - [x] 2.1 Create `export/context.rs` — `build_context_markdown()`
  - [x] 2.2 Add Tauri command `api_build_context` to `api.rs`
  - [x] 2.3 Register command in `lib.rs`
  - [x] 2.4 Write test: build context from mock results, assert Markdown format
  - [x] 2.5 Run `cargo test -p meetily --lib export` + `cargo check`
  - [x] 2.6 **Sprint 2 Code Review** — run subagent, update plan with findings
  - [x] 2.7 **Sprint 2 checkpoint** — ask user for doubts/additions

- [x] **Sprint 3: Chat with Meetings**
  - [x] 3.1 Create `api/chat.rs` — `api_chat_with_meetings` command
  - [x] 3.2 Register command in `api.rs` and `lib.rs`
  - [x] 3.3 Create `ChatPanel/index.tsx` — chat UI
  - [x] 3.4 Create `ChatPanel/ChatMessage.tsx` — message bubble with sources
  - [x] 3.5 Add ChatPanel to `page-content.tsx`
  - [x] 3.6 Add types to `types/index.ts`
  - [x] 3.7 Write test: source building, prompt construction, serialization
  - [x] 3.8 Run `cargo test -p meetily --lib` + `cargo check`
  - [x] 3.9 **Sprint 3 Code Review** — run subagent, update plan with findings
  - [x] 3.10 **Sprint 3 checkpoint** — ask user for doubts/additions

- [x] **Sprint 4: MCP Server**
  - [x] 4.1 Create `mcp/mod.rs` and `mcp/server.rs`
  - [x] 4.2 Add `axum = "0.7"` to `Cargo.toml`
  - [x] 4.3 Start MCP server in `lib.rs` `.setup()`
  - [x] 4.4 Implement `tools/list` handler
  - [x] 4.5 Implement `tools/call` handler (search, context, chat, list_folders)
  - [x] 4.6 Write test: start server, `tools/list` returns 4 tools, `search_meetings` works
  - [x] 4.7 Run `cargo test -p meetily --lib` + `cargo check`
  - [x] 4.8 **Sprint 4 Code Review** — run subagent, update plan with findings
  - [x] 4.9 **Sprint 4 checkpoint** — ask user for doubts/additions

- [ ] **Sprint 5: Frontend Integration**
  - [x] 5.1 Replace `api_search_transcripts` with `api_search_fts` in sidebar
  - [x] 5.2 Create `FolderFilterTree.tsx` — checkbox tree component
  - [x] 5.3 Add folder filter state to `SidebarProvider.tsx`
  - [x] 5.4 Add folder filter UI below search bar
  - [x] 5.5 Implement `folder:"name"` operator parsing in search
  - [x] 5.6 Show chunk_type + folder_name badges on search results
  - [x] 5.7 Add `FolderFilter` type to `types/index.ts`
  - [x] 5.8 Manual test: folder filter, search operator, chat panels
  - [x] 5.9 Run `cargo test -p meetily --lib` + `cargo check`
  - [x] 5.10 **Sprint 5 Code Review** — run subagent, update plan with findings
  - [x] 5.11 **Sprint 5 checkpoint** — ask user for doubts/additions

---

## Code Review Protocol

After each sprint's implementation:

1. Run `cargo test -p meetily --lib` — all tests must pass
2. Run `cargo check -p meetily` — no warnings/errors
3. Spawn a `general` subagent with this prompt:

> Review the changes made in Sprint N of the FTS5 Search + MCP plan. Read all files listed in the sprint's file list. Check for:
> - Rust idioms and correctness (no unwrap in production code, proper error handling, no panics)
> - SQL injection safety (parameterized queries only)
> - FTS5 query correctness (proper escaping of special characters in MATCH)
> - Tauri command patterns (follow existing `api.rs` conventions)
> - Test coverage (does the runnable check actually verify the feature?)
> - No regressions (existing tests still pass)
> - Code style consistency with the rest of the codebase
>
> Return a list of findings with file:line references and severity (must-fix / should-fix / suggestion).

4. Apply must-fix items before proceeding
5. Update this .md file with the sprint's implementation notes

---

## Sprint Implementation Notes

*(Updated after each sprint completion)*

### Sprint 1: FTS5 Foundation — COMPLETED

**What:** FTS5 virtual table (`meeting_fts`) over transcripts, summaries, and notes with folder-aware search, write-path hooks, and Tauri commands.

**How:**
- Created migration `20260727000000_add_fts5.sql` — FTS5 virtual table with `unicode61` tokenizer (no stemming, Portuguese-safe), initial population from all three source tables via INSERT...SELECT with JOINs to `meetings` and `meeting_folders`.
- Created `database/repositories/fts.rs` with `FtsRepository` — `search()` with BM25 ranking and `folder:"name"` operator parsing (regex via `LazyLock`), `refresh_meeting()` for write-path hooks, `remove_meeting()`, `rebuild_index()`, `sync_folder()`.
- Hooked `refresh_meeting()` into `transcript.rs::save_transcript()` (after commit, best-effort) and `summary.rs::update_process_completed()` (after update, best-effort).
- Hooked `remove_meeting()` into `meeting.rs::delete_meeting_with_transaction()` (inside transaction, before meeting delete). Also added missing `DELETE FROM meeting_notes` (pre-existing bug found by code review).
- Hooked `sync_folder()` into `folder.rs` — `rename()` (update folder_name in FTS), `delete_with_cascade()` (clear folder_name for deleted folder IDs), `set_meeting_folder()` (refresh FTS for the moved meeting).
- Added Tauri commands `api_search_fts` and `api_rebuild_fts_index` with `auth_token` parameter.
- 9 tests: search transcripts/summaries/notes, folder filter, empty query, remove, rebuild, sanitize operators, collapse whitespace.

**Why:**
- FTS5 over `LIKE '%query%'`: indexed search, BM25 ranking, snippet highlighting, folder filtering.
- `unicode61` tokenizer: no stemming — Portuguese words like "decisões" stem incorrectly with porter.
- Write-path hooks: FTS stays consistent without a background daemon.
- `LazyLock` for regex: avoid recompilation on every search call.
- Best-effort FTS hooks: a failed FTS update doesn't invalidate committed data.

**Code review fixes applied:**
1. Added missing `DELETE FROM meeting_notes` in `meeting.rs` delete path (must-fix)
2. `LazyLock` for regex compilation (should-fix)
3. Fixed misleading comment about AND/OR/NOT behavior (must-fix)
4. Fixed Chinese comment残留 (cleanup)
5. Added `auth_token` parameter to both commands (convention)
6. Added `rebuild_index` test (coverage)


### Sprint 2: Context Builder — COMPLETED

**What:** Converts FTS search results into a structured Markdown document for LLM context injection.

**How:**
- Created `export/context.rs` with `build_context_markdown()` — groups FTS results by meeting, preserves BM25 rank order using `Vec` + `HashMap` (not `BTreeMap` which would sort by meeting_id).
- Groups chunks under meeting headers with metadata (chunk type, speaker, timestamp).
- Empty folder name omitted. Meeting ID always shown for LLM citation.
- Added `api_build_context` Tauri command (with `max_chunks` clamp to 100, `auth_token`).
- 7 tests: empty input, single meeting, multiple meetings grouped, speaker/timestamp metadata, empty folder omitted, meeting_id always present, rank order preserved.

**Why:**
- LLMs need structured context, not raw FTS snippets. Grouping by meeting with metadata lets the LLM cite specific sections.
- BM25 rank order preserved because LLMs exhibit positional attention bias — most relevant content first gets more weight.
- `max_chunks` capped at 100 to prevent unbounded context length in LLM prompts.

**Code review fix:**
1. Replaced `BTreeMap` with `Vec` + `HashMap` to preserve BM25 rank order (must-fix: BTreeMap sorted by meeting_id lexicographically, not by relevance).

### Sprint 3: Chat with Meetings — COMPLETED

**What:** "Chat with Meetings" feature — user asks a question, the system searches meetings via FTS5, builds context, calls the configured LLM, and returns an answer with source citations.

**How:**
- Created `api/chat.rs` with `api_chat_with_meetings` command — orchestrates FTS search → context build → LLM call.
- Reuses `generate_summary` from `summary/llm_client.rs` for LLM calls (supports all providers: Ollama, OpenAI, Claude, Groq, CustomOpenAI, BuiltInAI).
- Gets model config from `SettingsRepository::get_model_config()`, API keys from `Setting` struct's per-provider fields.
- Builds conversation history from last 10 messages for multi-turn context.
- Returns `ChatResponse { answer, sources }` where sources are the FTS search results used.
- Created `ChatPanel/index.tsx` — chat UI with message list, input, loading state, auto-scroll.
- Created `ChatPanel/ChatMessage.tsx` — message bubbles with source badges (meeting title + folder).
- Added ChatPanel as collapsible panel at bottom of `page-content.tsx` (320px height, toggle via MessageSquare button).
- Added types: `ChatMessage`, `ChatSource`, `ChatResponse` to `types/index.ts`.
- 4 tests: source building, prompt construction with history, ChatSource serialization, ChatResponse serialization.

**Why:**
- Reuses `generate_summary` instead of writing a new LLM client — all provider support comes for free.
- Sources included in response so the UI can show which meetings the answer came from (transparency for LLM trust).
- Conversation history sent to LLM for multi-turn context — user can ask follow-up questions.
- ChatPanel as bottom panel (not sidebar tab) because it's contextually tied to the current meeting view.

**Code review fix:**
1. Frontend `ChatMessage` type was missing `sources` field, and `ChatPanel` wasn't passing sources to `ChatMessage` component — fixed both.

### Sprint 4: MCP Server — COMPLETED

**What:** HTTP JSON-RPC server (on `axum`) exposing search, context, chat, and folder listing to external MCP-compatible agents (Claude Desktop, Cursor, etc.).

**How:**
- Created `mcp/mod.rs` + `mcp/server.rs` — axum HTTP server on `127.0.0.1:5167`.
- JSON-RPC 2.0 protocol with `initialize`, `tools/list`, `tools/call`, and `ping` handlers.
- 4 tools exposed: `search_meetings` (FTS search with BM25 ranking), `build_context` (Markdown context builder), `chat_with_meetings` (FTS → context → LLM → answer + sources), `list_folders` (all meeting folders).
- `McpState` holds `SqlitePool`, `app_data_dir` (for BuiltInAI provider), and `reqwest::Client` (reused across requests).
- Server auto-starts in `lib.rs` `.setup()` after database initialization, on default port 5167.
- 3 tests: tool_definitions_returns_4_tools, jsonrpc_response_serializes, jsonrpc_error_response_serializes.

**Why:**
- MCP (Model Context Protocol) is the standard way for external AI agents to access local tools. Exposing Meetly's search/chat via MCP lets Claude Desktop, Cursor, and other agents query meeting data without custom integration.
- `initialize` handler required by MCP spec — without it, clients abort on connect.
- `app_data_dir` in state required for BuiltInAI provider (local sidecar needs models directory).
- `reqwest::Client` reused via state to benefit from connection pooling.

**Code review fixes (round 1):**
1. Added `initialize` handler (required by MCP spec — server was non-functional without it)
2. Added `app_data_dir` to `McpState` (BuiltInAI provider always failed with `None`)
3. Reuse `reqwest::Client` via `McpState` (connection pooling)

**Code review fixes (round 2):**
4. MCP `isError` semantics — tool execution errors now returned as successful JSON-RPC result with `"isError": true` content (per MCP 2025-03-26 spec), not as JSON-RPC error objects
5. First-launch MCP server startup — added `spawn_from_app()` helper, called from `import_and_initialize_database` and `initialize_fresh_database` commands so MCP server starts after onboarding completes (previously never started on first launch because AppState wasn't managed yet in `.setup()`)

### Sprint 5: Frontend Integration — COMPLETED

**What:** Wired FTS5 search into the sidebar, added folder chip filter, and added chunk_type + folder_name badges on search results.

**How:**
- Replaced `api_search_transcripts` (LIKE-based) with `api_search_fts` (FTS5/RTREE) in `SidebarProvider.tsx`. `searchTranscripts` is now `useCallback`-memoized (empty deps — no closure state).
- Added `FtsSearchResult` interface to `types/index.ts` mirroring the Rust struct (serde renames applied, camelCase over IPC).
- Created `FolderFilterTree.tsx` — compact folder chip picker below search bar. Clicking a chip selects it (radio-style: only one active); clicking again or "limpar" clears. `aria-pressed` and `aria-label` for accessibility.
- Added `folderFilter: string | null` state to `SidebarProvider` context; `setFolderFilter` exposed to Sidebar.
- Search is triggered by a single `useEffect` with deps `[searchQuery, folderFilter, searchTranscripts]` — single source of truth, no double-fetch. Constructs `folder:"<name>" <query>` when a chip is active; strips `"` from folder names before embedding (safe against FTS regex `[^"]*`).
- `flatSearchResults` deduplicates FTS per-chunk results by `meeting_id`, keeping the lowest `rank` (BM25: lower = more relevant). Falls back to title-substring matches for meetings not in FTS results.
- Extended `MeetingTreeItem.tsx` with optional `chunkType` and `folderName` props — rendered as small indigo/gray badges below the snippet when present.
- Deleted legacy `TranscriptSearchResult` interface and dead `findMatchingSnippet` helper.
- Cleaned up unused Rust imports: `BTreeMap` (context.rs), `FtsSearchResult` (mcp/server.rs), `sqlx::Sqlite` (folder.rs test).

**Why:**
- Single `useEffect` search trigger (vs. `handleSearchChange` + separate filter-change effect) avoids stale closures and double-fetches — the code review caught the original `eslint-disable` masking a real dependency bug.
- `"` stripping in folder names prevents search breakage when folder names contain double quotes (the backend regex `folder:"([^"]*)"` can't escape embedded quotes).
- BM25 rank dedup (lowest rank wins) ensures the most relevant chunk per meeting surfaces as the snippet, not an arbitrary chunk.
- Chip-based filter (vs. checkbox tree) is more compact in the narrow sidebar and discoverable — the plan said "checkbox tree" but chips are the same UX in less space; the `folder:"name"` power-user operator still works when typed manually.

**Code review fixes applied:**
1. Folder name `"` escaping — strip double quotes before embedding in `folder:"..."` query (should-fix)
2. Re-search `useEffect` restructured to single source of truth with correct deps; removed `eslint-disable` (should-fix)
3. Dead `findMatchingSnippet` deleted (should-fix)
4. Clearing search box now calls `searchTranscripts("")` to clear stale provider state (should-fix)
5. `aria-pressed` + `aria-label` on folder chips (suggestion)
6. Added type-contract comment on `FtsSearchResult` interface (suggestion)
7. Removed unused Rust imports: `BTreeMap`, `FtsSearchResult`, `sqlx::Sqlite` (cleanup)

---

## File Summary

| Action | Count | Files |
|--------|-------|-------|
| New Rust files | 5 | `fts.rs`, `context.rs`, `chat.rs`, `mcp/mod.rs`, `mcp/server.rs` |
| New migration | 1 | `20260727000000_add_fts5.sql` |
| New TSX files | 3 | `ChatPanel/index.tsx`, `ChatMessage.tsx`, `FolderFilterTree.tsx` |
| Modified Rust files | 7 | `api.rs`, `lib.rs`, `transcript.rs`, `summary.rs`, `meeting.rs`, `folder.rs`, `Cargo.toml` |
| Modified TSX files | 4 | `Sidebar/index.tsx`, `SidebarProvider.tsx`, `page-content.tsx`, `types/index.ts` |
| **Total new** | **9** | |
| **Total modified** | **11** | |

---

## What we're NOT doing (and why)

| Feature | Reason |
|---------|--------|
| meetily-memory as sidecar | No Python dependency, port to Rust natively |
| Topic explorer with aliases | FTS5 + folder filter covers the use case |
| Structured entity regex extraction | LLM summaries already extract these; regex is brittle |
| Obsidian sync | Can be added later as "export context as .md" |
| Semantic/vector search | `sqlite-vec` has no mature Rust binding |
| Auto-refresh daemon | FTS updates on write, not on timer |
| Contract versioning (v1/v2 API) | Internal tool, no public API |
| `rmcp` MCP SDK | Minimal: `axum` + `serde_json` only |

---

## Open Questions (resolved)

| # | Question | Resolution |
|---|----------|------------|
| 1 | MCP crate choice | Minimal: `axum` + `serde_json`, no SDK |
| 2 | Chat panel placement | Sidebar tab + meeting-details collapsible panel |
| 3 | Folder filter UX | Checkbox tree + `folder:"name"` search operator |
| 4 | FTS5 tokenizer | `unicode61` (no stemming — Portuguese needs exact matches) |
