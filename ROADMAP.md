# Personal Meetly — Roadmap

> Phased delivery tracker. Source of truth for progress; mirrors the phases in `SCOPE.md` §4 and the architecture in `ARCHITECTURE.md`.
>
> Status legend: `[ ]` pending · `[~]` in progress · `[x]` done · `[!]` blocked

---

## Phase 0 — Foundation & security

_Goal: a clean fork that builds on Windows, with the plaintext-key leak fixed and the notes table wired up. No new user-facing features beyond F11._

- [x] **0.1 Fork & upstream integration**
  - [x] `git clone https://github.com/Zackriya-Solutions/meetily` into `upstream/`
  - [x] Record base commit in `MIGRATION.md` (v0.4.0, commit `0281737`)
  - [x] Remove `analytics/` module + all call sites (decision 3) — 3,157 lines deleted
  - [x] Disable Tauri auto-updater in `tauri.conf.json` (decision 4)
  - [x] Pushed to GitHub at `https://github.com/armandotreuk/Meetly_Treuk`
- [x] **0.2 Windows build verification**
  - [x] Prerequisites: Rust 1.96.0, Node 24 + pnpm 11.9, CMake 11.13, VS 2026 C++
  - [x] Frontend (`pnpm build`) succeeds — 11 static pages generated
  - [x] CI workflow created (GitHub Actions Windows runner)
  - [x] Rust backend verification (`cargo check` + library tests with `CARGO_TARGET_DIR` outside OneDrive)
  - [ ] Packaged Tauri binary smoke test
  - [ ] Transcription + PT-BR smoke test (needs CI binary)
- [x] **0.3 F10 — Encrypted API key storage**
  - [x] Add `keyring`, `aes-gcm`, `base64` crates to `Cargo.toml`
  - [x] Create `frontend/src-tauri/src/security/{mod.rs,aes.rs,keyring.rs}`
  - [x] AES-256-GCM encryption with master key in OS keychain
  - [x] Wrap all API key reads/writes in `database/repositories/setting.rs`
  - [x] Lazy migration: plaintext keys encrypted on first read
  - [ ] Verify with CI binary
- [x] **0.4 F11 — Meeting notes editor**
  - [x] `database/repositories/meeting_notes.rs` — CRUD on existing table
  - [x] Tauri commands: `get_meeting_notes`, `save_meeting_notes`, `delete_meeting_notes`
  - [x] Shared BlockNote editor with 2s autosave; persist `notes_json` + derived markdown
  - [x] Notes toggle button in meeting details page
  - [ ] Verify with CI binary

**Phase 0 exit criteria:** clean fork builds on Windows, runs dev mode, transcribes PT-BR audio, stores all API keys encrypted, and lets the user write per-meeting notes.

---

## Phase 1 — Core Pro-equivalents

_Goal: the productivity features most users actually want from Pro._

- [x] **1.1 F1 — Custom summary templates** (unblocks F2, F3, F7, F8)
  - [x] Migration: `templates` table with stable IDs and DB/bundled source handling
  - [x] Template loader: DB-first with bundled JSON fallback
  - [x] Validate user JSON against the template schema
  - [x] `frontend/src/components/TemplateEditor.tsx` — editor + preview
  - [x] Summary template pickers list user templates
  - [x] Tauri commands: list/details/validate/create/update/delete templates
- [x] **1.2 F2 — PDF export**
  - [x] `printpdf` implementation with embedded/fallback Unicode fonts
  - [x] `frontend/src-tauri/src/export/{mod.rs,pdf.rs,commands.rs}`
  - [x] Template-driven layout: title, metadata, sections, lists, and action-item tables
  - [x] `ExportMenu.tsx` with save dialog
  - [x] PT-BR/Unicode font coverage and export tests
- [ ] **1.3 F3 — DOCX export**
  - [ ] Add `docx-rs` to `Cargo.toml`
  - [ ] `frontend/src-tauri/src/export/docx.rs`
  - [ ] Map template `format` types → Word elements (Paragraph / BulletList / Table)
  - [ ] Extend `ExportMenu.tsx` with "Export DOCX"
  - [ ] Verify: opens cleanly in Word + LibreOffice
- [ ] **1.4 F4 — Call detection, start prompt, and auto-stop** — delivery sequenced in Phase 5.5
  - [ ] `frontend/src-tauri/src/audio/detector.rs` — window-title polling (`EnumWindows` on Windows) + audio-session state cross-check
  - [ ] Platform signatures in `config.rs` (Zoom, Teams, Meet, Webex, Discord)
  - [ ] Hook into the current recording lifecycle (detection prompt / auto-stop after sustained silence)
  - [ ] `frontend/src/components/settings/AutoDetectSettings.tsx` — per-platform toggles
  - [ ] **Privacy safeguard:** one-click start by default; auto-start requires explicit opt-in + persistent notification

**Phase 1 exit criteria:** user can author templates, export to PDF + DOCX, and have calls detected with the configured privacy-first start/stop workflow.

---

## Phase 2 — Integrations

_Goal: get summaries out of the app into the tools the team already uses._

- [ ] **2.1 F8 — Obsidian vault sync** (simplest, highest value for the Obsidian template)
  - [ ] `frontend/src-tauri/src/integrations/obsidian.rs`
  - [ ] Directory picker via `dialog.open` → save `<vault>/Meetings/<YYYY-MM-DD> <title>.md`
  - [ ] Preserve `[[wiki-links]]` verbatim
  - [ ] Optional append to `<vault>/Daily/<YYYY-MM-DD>.md` under `## Meetings`
  - [ ] Conflict policy: `(2)` suffix on name collision
  - [ ] `frontend/src/components/integrations/ObsidianConnect.tsx`
  - [ ] "Save to Vault" action in `SummaryView.tsx`
- [ ] **2.2 F7 — Notion integration**
  - [ ] `frontend/src-tauri/src/integrations/{mod.rs,notion.rs}`
  - [ ] Internal-integration token stored encrypted via `security/` (F10)
  - [ ] First-connect flow: list databases → user picks target + property mapping
  - [ ] "Send to Notion" creates a page with summary markdown, properties from meeting metadata
  - [ ] Respect Notion 2MB block limit (split long summaries)
  - [ ] `frontend/src/components/integrations/NotionConnect.tsx`
  - [ ] Privacy: token local only, no cloud relay

**Phase 2 exit criteria:** summaries can be pushed to a local Obsidian vault and a Notion database on demand.

---

## Phase 3 — Advanced

_Goal: the harder, higher-value features that round out the Pro-equivalent set._

- [ ] **3.1 F5 — Speaker diarization** (complexity L — sherpa-onnx; delivery sequenced in Phase 5.6)
  - [ ] Verify `sherpa-rs` maintenance status; if archived, vendor thin FFI wrapper
  - [ ] Verify `sherpa-onnx-pyannote-segmentation-3.0` license in tarball; record attribution in `LICENSE`
  - [ ] PT-BR smoke test: 3D-Speaker ERes2Net vs NeMo TitaNet small → pick embedding model
  - [ ] Add `sherpa-rs` to `Cargo.toml`; create a compiled top-level `diarization/` module
  - [ ] Migration: `transcripts.speaker_label` / `speaker_id` + optional `speaker_profiles` table
  - [ ] Integrate as a post-transcription step in the current `audio/` + persistence pipeline
  - [ ] Onboarding: add segmentation (~6MB) + embedding (~30MB) model downloads
  - [ ] UI: "Identifying speakers..." spinner; one-click rename `Speaker 1` → `Alice`
  - [ ] `frontend/src/components/settings/DiarizationSettings.tsx`
- [ ] **3.2 F6 — Calendar integration** — delivery sequenced in Phase 5.1
  - [ ] Phase A: `frontend/src-tauri/src/calendar/ics_parser.rs` — local `.ics` import via `ics` crate
  - [ ] `calendar_cache` table; `CalendarPanel.tsx` shows upcoming events
  - [ ] Pre-fill meeting title/attendees from event; pre-arm auto-detect 5 min before start
  - [ ] Phase B: Google Calendar OAuth (Tauri deep-link or loopback HTTP), `calendar.events.readonly`
  - [ ] Phase C: Outlook via Microsoft Graph (same OAuth flow)
  - [ ] `frontend/src/components/settings/CalendarSettings.tsx`
- [x] **3.3 F9 — Chat with meetings** — shipped upstream and hardened through Phase 4
  - [x] FTS5 index covers transcripts, summaries, and manual notes; refreshes on data changes
  - [x] `api/chat.rs` retrieval, prompt construction, query rewriting, and context budgeting
  - [x] Uses the configured local or cloud LLM; model/provider is visible in the UI
  - [x] `ChatPanel` streams Markdown answers with clickable meeting/snippet citations
  - [x] Per-meeting and cross-meeting queries, persisted conversations, cancellation, and MCP reuse

**Phase 3 exit criteria:** transcripts show speaker labels, meetings auto-link to calendar events, and users can Q&A their meeting history.

---

## Phase 4 — Notes & Chat improvements (F12)

_Goal: stop losing user data, then raise notes and chat from bolted-on panels to first-class features. Full analysis + rationale in `upstream/docs/notes-chat-improvement-plan.md`. Live execution record in `upstream/docs/notes-chat-improvement-execution.md`._

> Difficulty tags guide model selection for subagent execution (orchestrator = main agent):
> `[S]` simple/small — fast model · `[M]` medium — mid-tier model · `[L]` large/complex — top-tier model.

- [x] **Sprint 1 — Stop losing data (P0 correctness)**
  - [x] 1.1 `[M]` Persist recording notes to DB on stop (bridge sessionStorage/`notes.md` → `meeting_notes` via `useRecordingStop.ts`)
  - [x] 1.2 `[S]` Per-meeting draft key; clear on stop (`RecordingNotesPanel.tsx:12`)
  - [x] 1.3 `[S]` Refresh FTS on note save/delete (`database/commands.rs:307-373`)
  - [x] 1.4 `[S]` Load-error guard: read-only + Retry, never mount editable empty editor (`NotesPanel.tsx:55-62`)
  - [x] 1.5 `[S]` Unmount flush + `beforeunload` guard for pending autosave
  - [x] 1.6 `[S]` Chat hygiene: drop error bubbles from history; fix 60s→300s timeout message (`llm_client.rs:280,293`)
  - _Reviewed and approved after R1/R1b; follow-ups 1.7/1.8 are recorded in the execution log._
- [x] **Sprint 2 — Notes panel trust & parity (P1)**
  - [x] 2.1 `[M]` Merge `RecordingNotesPanel` + `NotesPanel` into one parameterized component
  - [x] 2.2 `[S]` Notes menu: wire existing `delete_meeting_notes`; show "Saved HH:mm"
  - [x] 2.3 `[S]` "Has notes" indicator on sticky-note toggle
  - [x] 2.4 `[S]` Keyboard (Ctrl/Cmd+S) + a11y pass (aria-labels, `aria-live`)
  - [x] 2.5 `[S]` Empty-state copy: notes are incorporated into the AI summary
  - [x] 2.6 `[S]` Delete dead code: `/notes/[id]` demo route, `BasicBlockNoteTest.tsx`, `Editor.tsx` render logging
  - _Reviewed and approved after R2; audio-import and review follow-ups are recorded in the execution log._
- [x] **Sprint 3 — Chat that feels alive (P1)**
  - [x] 3.1 `[L]` Streaming responses via Tauri events
  - [x] 3.2 `[S]` Stop button and cancellation that keeps the partial answer
  - [x] 3.3 `[S]` Markdown rendering of answers
  - [x] 3.4 `[S]` Clickable sources: jump to meeting + snippet preview
  - [x] 3.5 `[M]` "This meeting" scope toggle
  - [x] 3.6 `[S]` Multiline input + copy-answer button
  - [x] 3.7 `[S]` Model/provider indicator in header + Settings deep-link on error
  - _Reviewed and approved after R3/R4; streaming ownership and architecture follow-ups are complete._
- [x] **Sprint 4 — Chat memory & retrieval quality (P2)**
  - [x] 4.1 `[M]` Conversation persistence: `chat_conversations` + `chat_messages` tables, resume, clear
  - [x] 4.2 `[M]` Query rewriting for follow-ups (resolve pronouns/ellipsis vs history)
  - [x] 4.3 `[L]` Retrieval depth: AND/phrase matching, segment expansion, context-window guard
  - [x] 4.4 `[S]` Suggested prompts in empty state (scoped variants when a meeting is open)
  - [x] 4.5 `[S]` Privacy: stop INFO-logging queries; surface cloud-vs-local provider
  - _Reviewed after R5/R6; persistence, prompt-budget, and privacy follow-ups are complete._
- [x] **Sprint 5 — Notes as a real editor (P2)**
  - [x] 5.1 `[M]` Swap textarea → BlockNote for notes; persist `notes_json` + markdown round-trip
  - [x] 5.2 `[S]` Panel-toggle + save keyboard shortcuts
  - [x] 5.3 `[S]` Persist panel width (`usePanelResize` + localStorage)
  - [x] 5.4 `[M]` i18n groundwork: extract notes/chat strings; flag the sidebar PT-BR/EN decision
  - _R7d approved Sprint 5 as shippable after the save-ordering and recording-stop durability follow-ups. Final verification: 77 Vitest tests, cargo check, and rustfmt._

### Granola-aligned gap register (reviewed 2026-08-17)

This is the canonical gap list for features that fit Meetily's local-first direction. Competitive evidence came from Granola's official [101](https://docs.granola.ai/help-center/getting-started/granola-101), [Chat](https://docs.granola.ai/help-center/getting-more-from-your-notes/chatting-with-your-meetings), [Recipes](https://docs.granola.ai/help-center/getting-more-from-your-notes/recipes), [Calendar](https://docs.granola.ai/help-center/getting-started/syncing-your-calendars), [Briefs](https://docs.granola.ai/help-center/taking-notes/pre-meeting-briefs), [People and Companies](https://docs.granola.ai/help-center/people-and-companies), [Spaces and folders](https://docs.granola.ai/help-center/sharing/folders/spaces-and-folders), and [Integrations](https://www.granola.ai/integrations) documentation.

| Missing or partial function | Current Meetily state | Delivery target |
|---|---|---|
| Chat during a live recording | Missing | Sprint 6.1 |
| Chat from home, folders, and selected meetings | Partial: retrieval/MCP can search broadly, but `ChatPanel` is mounted only in meeting details | Sprint 6.1 |
| Saved prompt Recipes | Missing | Sprint 6.3 |
| AI-driven note editing and safe application of chat output | Missing | Sprint 6.2 |
| Note/summary claim → transcript provenance | Partial: chat citations exist; notes and summaries have no source links | Sprint 6.4 |
| Copy/delete individual transcript chunks and refresh dependent search | Missing | Sprint 6.4 |
| Meeting playbooks combining agenda, output template, and prompts | Partial: custom summary templates exist; pre-meeting note scaffolds/recipes do not | Sprint 6.5 |
| Complete meeting export | Partial: PDF summary ships; manual notes are excluded and DOCX is a placeholder | Sprint 6.6 + F3 |
| Trash, restore, and safe permanent purge | Missing; current deletion is permanent | Sprint 6.7 |
| Calendar sync, upcoming meetings, attendees, and recurring-event links | Missing; F6 is already scoped | Phase 5.1 |
| Call-detected prompt, meeting reminders, and reliable auto-stop | Missing; F4 is already scoped | Phase 5.5 |
| People and Companies relationship views | Missing | Phase 5.2 |
| Pre-meeting Briefs grounded in local history | Missing | Phase 5.3 |
| Dedicated follow-up draft workflow | Partial: Chat can generate text, but there is no review/send workflow | Phase 5.4 |
| Speaker identification/diarization | Missing in the product; transcript attribution is still mic/system | Phase 5.6 / F5 |
| Participant transparency assistance | Missing beyond Meetily's own recording indicator | Phase 5.7 |
| Transcript retention policy | Missing | Backlog after 6.7 |
| Attachments as extra Chat context | Missing | Backlog; require a concrete use case first |
| Many-to-many folders and recurring auto-add rules | Partial: each meeting has one `folder_id` | Backlog after Phase 5.1 |

### Original Sprint 6 candidate disposition

| Original candidate | Decision | Canonical replacement |
|---|---|---|
| Actions on chat answers | Keep and expand | 6.2 adds safe note editing and artifact actions |
| Note↔transcript timestamp links | Keep and expand | 6.4 adds provenance, quote insertion, seeking, and transcript redaction |
| Note templates | Merge | 6.5 delivers meeting playbooks rather than generic Cornell-only templates |
| Notes export | Keep | 6.6 ships complete PDF/Markdown first; DOCX follows F3 |
| More chat entry points | Promote to first | 6.1 covers home, folder, live, and selected-meeting contexts |
| Standalone notes / quick-capture library | Defer | Ad-hoc meeting capture already exists; do not turn Meetily into a general notes app without demand |
| Semantic/hybrid search | Defer and benchmark-gate | Current FTS5 retrieval is mature; add embeddings only after measured recall failures on real corpora |

### Revised Sprint 6 — Context to action (P2/P3)

Deliver as two independently reviewed increments. Do not begin 6B until 6A is reviewed and stable.

- [ ] **Sprint 6A — Chat as a work surface**
  - [ ] 6.1 `[L]` **Contextual Chat everywhere** — home/all-meetings, folder-scoped, selected-meeting, and live-recording entry points. Live Chat must include the current in-memory transcript rather than waiting for DB persistence.
  - [ ] 6.2 `[L]` **Chat actions and AI note editing** — copy; append to notes; create a note section; rewrite selected note text with preview/confirmation. Direct mutation of an active generated summary is out of scope until summaries have an explicit revision/undo contract.
  - [ ] 6.3 `[M]` **Local Recipes** — SQLite-backed saved prompts, slash menu, single-meeting vs multi-meeting scope, optional model preference, and useful built-ins (actions, decisions, follow-up draft, PRD/brief). Personal/local only: no sharing or marketplace.
  - _Review: per-task review for 6.1/6.2, then a 6A integration review._
- [ ] **Sprint 6B — Trust and complete artifacts**
  - [ ] 6.4 `[L]` **Transcript provenance and privacy controls** — timestamp links in notes, add transcript quote to notes, jump/seek from a note, delete a transcript chunk, refresh FTS, and prompt to regenerate affected summaries. Automatic per-bullet AI provenance is a later extension.
  - [ ] 6.5 `[M]` **Meeting playbooks** — bundle a raw-note/agenda scaffold, an existing summary template, and suggested Recipes for stand-ups, 1:1s, interviews, sales calls, retros, and project syncs.
  - [ ] 6.6 `[M]` **Complete meeting export** — PDF and Markdown containing metadata, active summary, manual notes, and optional transcript. Add DOCX when F3 is implemented; do not block PDF/Markdown parity on F3.
  - [ ] 6.7 `[M]` **Trash and restore** — soft-delete meetings/notes, restore, permanent purge, and a default 30-day trash window. Transcript retention policy is separate follow-up work.
  - _Review: per-task review for 6.4, then a Sprint 6 end-to-end review._

**Sprint 6 exit criteria:** a user can ask from the right context before/during/after a meeting, reuse local Recipes, safely turn an answer into notes, trace or remove the transcript evidence behind an artifact, export the complete meeting record, and recover an accidentally deleted item.

**Phase 4 core exit criteria (achieved through Sprint 5):** notes survive a record→stop→reopen cycle and are searchable immediately; a failed note load cannot clobber saved notes; chat streams with cancellation, renders markdown, cites clickable sources, remembers conversations, and scopes to the open meeting.

---

## Phase 5 — Meeting context & relationship memory (F13)

_Goal: close the largest remaining Granola-class workflow gap: prepare before a meeting, stay oriented during it, and carry relationship context forward afterward. This program sequences already-scoped F4/F5/F6 work with the entity and workflow layers that depend on it; technical details for those features remain in Phases 1 and 3._

- [ ] **5.1 `[L]` Calendar foundation (F6)**
  - [ ] Local `.ics` import first, then Google Calendar read-only OAuth, then Outlook/Microsoft Graph
  - [ ] Upcoming-meeting view, event-to-meeting linking, title/agenda/attendee metadata, recurring event ID
  - [ ] Related-meeting navigation using recurring event ID with exact-title fallback
- [ ] **5.2 `[L]` Attendees, People, and Companies**
  - [ ] Normalize attendees from linked events into local people/company records
  - [ ] People/company views listing related meetings, notes, decisions, and action items
  - [ ] Person/company-scoped Chat; no external profile enrichment by default
- [ ] **5.3 `[L]` Local pre-meeting Briefs**
  - [ ] Generate a short cited brief from calendar agenda, prior meetings, open decisions, and action items
  - [ ] Start local-only; web research and Gmail context require separate explicit opt-in decisions
- [ ] **5.4 `[M]` Follow-up drafts**
  - [ ] Dedicated reviewable draft generated from transcript, notes, attendees, and next steps
  - [ ] Start with copy and default-mail-app handoff; Gmail read/send OAuth is a later integration
- [ ] **5.5 `[L]` Call detection, reminders, and auto-stop (F4)**
  - [ ] Calendar reminder and ad-hoc microphone/app detection prompt
  - [ ] One-click start by default; auto-start remains explicit opt-in
  - [ ] Reliable auto-stop from platform/audio/calendar signals with persistent recording notice
- [ ] **5.6 `[L]` Speaker identification (F5)**
  - [ ] Post-transcription diarization, renameable speaker labels, and timestamp-aligned persistence
  - [ ] Voice-profile persistence remains opt-in and local
- [ ] **5.7 `[M]` Participant transparency controls**
  - [ ] Consent reminder before capture and persistent local recording indicator
  - [ ] Platform chat notices/watermarks require separate platform extensions and are not initial scope

**Phase 5 sequencing:** 5.1 → 5.2 → 5.3/5.4. Tasks 5.5 and 5.6 can proceed independently, but 5.7 must ship before any auto-start default is considered.

**Phase 5 exit criteria:** calendar-linked meetings carry attendee and recurring context; users can review cited local Briefs before external meetings, query relationship history, create a follow-up draft, receive call-detected prompts, and see trustworthy speaker/transparency behavior.

### Deferred or product-direction-dependent Granola gaps

These are real missing functions but are not approved implementation scope. They require a cloud identity/sync/platform decision that conflicts with the current full-local-ownership principle if added casually.

- **Team collaboration:** shared notes, viewer/collaborator permissions, workspaces, team/custom spaces, user groups, `@mentions`, Shared with me, shared Chat, shared templates/Recipes.
- **Mobile and cross-device:** iOS/Android capture, phone-call capture, Apple Watch, and cross-device synchronization.
- **Hosted platform:** public cloud REST API, signed webhooks, Zapier, CRM automation, and externally hosted sharing links. Meetily's local MCP remains the preferred near-term integration surface.
- **Enterprise administration:** SSO/SAML, SCIM, domain management, org-wide retention/sharing controls, usage analytics, and managed deployment.
- **Standalone notes library:** reconsider only if demand shows users want non-meeting knowledge capture; ad-hoc meeting notes remain supported.
- **Semantic/hybrid search:** reconsider only after a repeatable retrieval benchmark shows FTS5 misses important results at a material rate.

---

## Cross-cutting (every phase)

- [ ] Keep `MIGRATION.md` updated with upstream-incompatible changes
- [ ] Rebase on `upstream/main` before each internal release
- [ ] CI build pipeline producing per-OS binaries (Windows first)
- [ ] No analytics events introduced anywhere in our additions
- [ ] LICENSE notice includes sherpa-onnx / pyannote-segmentation attribution once F5 lands

---

## Status snapshot (2026-08-17)

| Phase | Features | Status |
|---|---|---|
| Scoping | README, SCOPE, ARCHITECTURE, ROADMAP, diarization research | ✅ complete |
| Phase 0 | Fork, build verify, F10, F11 | ⏳ implementation complete; packaged-app/PT-BR smoke verification remains |
| Phase 1 | F1, F2, F3, F4 | partial — custom templates and PDF export ship; DOCX and auto-detect remain |
| Phase 2 | F8, F7 | pending |
| Phase 3 | F5, F6, F9 | partial — Chat shipped and was hardened in Phase 4; diarization/calendar remain |
| Phase 4 | F12 Notes & Chat | Sprints 1–5 ✅ approved/shippable; revised Sprint 6 pending |
| Phase 5 | F13 Meeting context & relationship memory | scoped; pending Sprint 6 completion |

### Build verification results (Phase 0.1)

- ✅ **Upstream repo cloned** — `upstream/` at v0.4.0 (commit `0281737`)
- ✅ **Prerequisites installed** — Rust 1.96.0, Node 24 + pnpm 11.9 (via corepack), CMake 11.13, VS 2026 Community with C++ workload
- ✅ **Frontend (Next.js) builds** — `pnpm build` succeeds, 11 static pages generated
- ✅ **Rust backend checks/tests** — run successfully with `CARGO_TARGET_DIR=C:\Users\arman\cargo-target` outside OneDrive (R7d: cargo check + rustfmt; R7c: full library tests 346 passed, 2 ignored)
- [ ] **Packaged Tauri binary + PT-BR hardware smoke test** — still required before an internal release

### Resolved local Rust-build blocker: OneDrive target directory

**Current resolution:** keep Cargo artifacts outside the OneDrive-synced repository:

```powershell
$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"
cargo check --manifest-path upstream/frontend/src-tauri/Cargo.toml
```

**Remaining release verification:** build a packaged Tauri binary, record/transcribe PT-BR audio on Windows hardware, stop/reopen the meeting, and smoke-test notes, summary, Chat, PDF export, and key storage end to end.

### Findings from upstream code inspection and current resolution

- **`audio_v2/` is dead code** — not declared as a module in `lib.rs`, not referenced in any `.rs` file. Our F4/F5 modules should go in `audio/` or be declared as new top-level modules.
- **F11 is wired and hardened** — `BlockNoteEditor/Editor.tsx` now persists rich `notes_json` plus derived markdown through the shared notes editor flow; save ordering and recording-stop durability were approved in R7d.
- **The obsolete `/notes/[id]` demo and `BasicBlockNoteTest.tsx` were deleted** in Sprint 2; meeting-linked notes live in the recording and meeting-details panels.
- **Templates are at `frontend/src-tauri/templates/`** (not `src/templates/`) — bundled as Tauri resources via `tauri.conf.json` `"resources": ["templates/*.json"]`.
- **`silero_rs` and `ort` are already dependencies** — Silero VAD and ONNX Runtime already in `Cargo.toml`. F5 (sherpa-onnx) shares these foundations.
- **`speaker` column already exists** on `transcripts` but stores audio source (`'mic'`/`'system'`), not speaker identity. F5 must use `speaker_label` / `speaker_id` instead (ARCHITECTURE.md updated).
- **Telemetry and auto-update decisions are applied** — analytics call sites/module were removed for the fork and the Tauri auto-updater is disabled.
- **Existing migrations** include licensing schema (`add_pro_license_custom_openai`, `add_grace_period_to_licensing`) — harmless with 0 rows, can be left in place.
