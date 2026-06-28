# Personal Meetly — Roadmap

> Phased delivery tracker. Source of truth for progress; mirrors the phases in `SCOPE.md` §4 and the architecture in `ARCHITECTURE.md`.
>
> Status legend: `[ ]` pending · `[~]` in progress · `[x]` done · `[!]` blocked

---

## Phase 0 — Foundation & security

_Goal: a clean fork that builds on Windows, with the plaintext-key leak fixed and the notes table wired up. No new user-facing features beyond F11._

- [ ] **0.1 Fork & upstream integration**
  - [ ] `git clone https://github.com/Zackriya-Solutions/meetily` into `upstream/` (submodule or full clone)
  - [ ] Add `upstream` remote; record base commit in `MIGRATION.md`
  - [ ] Create our `src/` overlay directory structure (per `ARCHITECTURE.md` §1)
  - [ ] Remove `analytics/` module + all call sites (decision 3)
  - [ ] Disable Tauri auto-updater in `tauri.conf.json` (decision 4)
- [ ] **0.2 Windows build verification**
  - [ ] Verify prerequisites: `rustc` ≥ 1.77, Node.js + pnpm, VS Build Tools C++ workload, CMake
  - [ ] `cd frontend && pnpm install && pnpm tauri:dev` — app launches
  - [ ] Transcription smoke test (record 30s, confirm transcript + summary appear)
  - [ ] PT-BR smoke test (record/insert a Portuguese clip)
- [ ] **0.3 F10 — Encrypted API key storage**
  - [ ] Add `keyring` crate to `frontend/src-tauri/Cargo.toml`
  - [ ] Create `frontend/src-tauri/src/security/{mod.rs,aes.rs,keyring.rs}`
  - [ ] Migration: `key_metadata` table + one-time plaintext→ciphertext migration
  - [ ] Wrap all `*ApiKey` reads/writes in `database/settings.rs`
  - [ ] Verify: existing OpenRouter/OCI keys migrate automatically and still work
  - [ ] Verify: DB file alone no longer reveals keys
- [ ] **0.4 F11 — Meeting notes editor**
  - [ ] Create `frontend/src-tauri/src/database/meeting_notes.rs` (CRUD on existing empty table)
  - [ ] Expose Tauri commands: `get_notes`, `save_notes`
  - [ ] Create `frontend/src/components/MeetingNotesEditor.tsx` (markdown editor + autosave)
  - [ ] Add "Notes" tab to `frontend/src/components/MeetingDetail.tsx`
  - [ ] Verify: notes persist across app restarts

**Phase 0 exit criteria:** clean fork builds on Windows, runs dev mode, transcribes PT-BR audio, stores all API keys encrypted, and lets the user write per-meeting notes.

---

## Phase 1 — Core Pro-equivalents

_Goal: the productivity features most users actually want from Pro._

- [ ] **1.1 F1 — Custom summary templates** (unblocks F2, F3, F7, F8)
  - [ ] Migration: `templates` table (seed builtins with `is_builtin=1`)
  - [ ] `frontend/src-tauri/src/templates/mod.rs` — registry: DB-first, fallback to bundled JSON
  - [ ] Validate user JSON against upstream template schema
  - [ ] `frontend/src/components/settings/TemplateEditor.tsx` — editor + live preview
  - [ ] Extend `SummaryTemplatePicker.tsx` to list user templates
  - [ ] Tauri commands: `list_templates`, `create_template`, `update_template`, `delete_template`
- [ ] **1.2 F2 — PDF export**
  - [ ] Add `printpdf` (or `typst`) to `Cargo.toml`
  - [ ] `frontend/src-tauri/src/export/{mod.rs,pdf.rs}`
  - [ ] Template-driven layout: title, metadata, sections, action-item tables
  - [ ] Extend `ExportMenu.tsx` with "Export PDF" → `dialog.save`
  - [ ] Verify: PT-BR characters render correctly (font embedding)
- [ ] **1.3 F3 — DOCX export**
  - [ ] Add `docx-rs` to `Cargo.toml`
  - [ ] `frontend/src-tauri/src/export/docx.rs`
  - [ ] Map template `format` types → Word elements (Paragraph / BulletList / Table)
  - [ ] Extend `ExportMenu.tsx` with "Export DOCX"
  - [ ] Verify: opens cleanly in Word + LibreOffice
- [ ] **1.4 F4 — Auto-detect & auto-join meetings**
  - [ ] `frontend/src-tauri/src/audio_v2/detector.rs` — window-title polling (`EnumWindows` on Windows) + audio-session state cross-check
  - [ ] Platform signatures in `config.rs` (Zoom, Teams, Meet, Webex, Discord)
  - [ ] Hook into `state.rs` recording lifecycle (auto-start / auto-stop after 30s silence)
  - [ ] `frontend/src/components/settings/AutoDetectSettings.tsx` — per-platform toggles
  - [ ] **Privacy safeguard:** explicit opt-in + persistent notification while auto-recording

**Phase 1 exit criteria:** user can author templates, export to PDF + DOCX, and have meetings auto-detected/recorded without manual clicks.

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

- [ ] **3.1 F5 — Speaker diarization** (complexity L — sherpa-onnx)
  - [ ] Verify `sherpa-rs` maintenance status; if archived, vendor thin FFI wrapper
  - [ ] Verify `sherpa-onnx-pyannote-segmentation-3.0` license in tarball; record attribution in `LICENSE`
  - [ ] PT-BR smoke test: 3D-Speaker ERes2Net vs NeMo TitaNet small → pick embedding model
  - [ ] Add `sherpa-rs` to `Cargo.toml`; create `frontend/src-tauri/src/audio_v2/diarization.rs`
  - [ ] Migration: `transcripts.speaker` + `speaker_profiles` table
  - [ ] Integrate as post-transcription step in `transcript_pipeline.rs`
  - [ ] Onboarding: add segmentation (~6MB) + embedding (~30MB) model downloads
  - [ ] UI: "Identifying speakers..." spinner; one-click rename `Speaker 1` → `Alice`
  - [ ] `frontend/src/components/settings/DiarizationSettings.tsx`
- [ ] **3.2 F6 — Calendar integration**
  - [ ] Phase A: `frontend/src-tauri/src/calendar/ics_parser.rs` — local `.ics` import via `ics` crate
  - [ ] `calendar_cache` table; `CalendarPanel.tsx` shows upcoming events
  - [ ] Pre-fill meeting title/attendees from event; pre-arm auto-detect 5 min before start
  - [ ] Phase B: Google Calendar OAuth (Tauri deep-link or loopback HTTP), `calendar.events.readonly`
  - [ ] Phase C: Outlook via Microsoft Graph (same OAuth flow)
  - [ ] `frontend/src/components/settings/CalendarSettings.tsx`
- [ ] **3.3 F9 — Chat with meetings**
  - [ ] Migration: `transcript_fts` (FTS5) virtual table; populate at meeting-save time
  - [ ] `frontend/src-tauri/src/chat/{mod.rs,retriever.rs}` — top-k retrieval
  - [ ] On question: retrieve chunks → context prompt → configured LLM (no new model)
  - [ ] `frontend/src/components/MeetingChat.tsx` — answers with inline citations to transcript timestamps
  - [ ] Cross-meeting queries ("across this week's meetings, what did we decide about X?")

**Phase 3 exit criteria:** transcripts show speaker labels, meetings auto-link to calendar events, and users can Q&A their meeting history.

---

## Cross-cutting (every phase)

- [ ] Keep `MIGRATION.md` updated with upstream-incompatible changes
- [ ] Rebase on `upstream/main` before each internal release
- [ ] CI build pipeline producing per-OS binaries (Windows first)
- [ ] No analytics events introduced anywhere in our additions
- [ ] LICENSE notice includes sherpa-onnx / pyannote-segmentation attribution once F5 lands

---

## Status snapshot (2026-06-28)

| Phase | Features | Status |
|---|---|---|
| Scoping | README, SCOPE, ARCHITECTURE, ROADMAP, diarization research | ✅ complete |
| Phase 0 | Fork, build verify, F10, F11 | ⏳ in progress — see blocker below |
| Phase 1 | F1, F2, F3, F4 | pending |
| Phase 2 | F8, F7 | pending |
| Phase 3 | F5, F6, F9 | pending |

### Build verification results (Phase 0.1)

- ✅ **Upstream repo cloned** — `upstream/` at v0.4.0 (commit `0281737`)
- ✅ **Prerequisites installed** — Rust 1.96.0, Node 24 + pnpm 11.9 (via corepack), CMake 11.13, VS 2026 Community with C++ workload
- ✅ **Frontend (Next.js) builds** — `pnpm build` succeeds, 11 static pages generated
- ❌ **Rust/Tauri backend blocked** — Smart App Control (SAC) blocks all unsigned executables, including Rust build scripts (os error 4551). See blocker below.

### ⚠️ Blocker: Smart App Control (SAC)

**Root cause:** Windows Smart App Control is in **enforced mode** (`VerifiedAndReputablePolicyState: 1`). It blocks execution of all unsigned executables. Rust build scripts (compiled `build-script-build.exe` in `target/`) are unsigned and get blocked at the kernel level. This is not path-specific — tested with target dirs in OneDrive, `C:/Users/arman/`, and `C:/Users/arman/AppData/Local/Temp/` — all blocked.

**Impact:** No Rust crate with a `build.rs` can compile (serde, proc-macro2, ort, whisper-rs, etc.). This makes all local Rust/Tauri development impossible until resolved.

**Options (user decision required):**
1. **Turn off SAC** — Windows Settings → Privacy & Security → Windows Security → App & browser control → Smart App Control → Turn off. ⚠️ One-way switch: once off, cannot be re-enabled. Restores full local dev capability.
2. **CI-based builds** — set up GitHub Actions with a Windows runner (no SAC) to compile and produce binaries. Slower iteration (push → wait for CI) but no security compromise. Frontend dev still works locally.
3. **Build on another machine** — any Windows PC without SAC can build. Transfer binaries back.
4. **Windows VM** — if Hyper-V is available, create a VM without SAC for local builds.

**What still works regardless of SAC:**
- Frontend development (Next.js/React/TypeScript — `pnpm build` succeeds)
- Writing Rust code (compilation deferred to CI or a non-SAC machine)
- All planning, architecture, and code review

### Findings from upstream code inspection

- **`audio_v2/` is dead code** — not declared as a module in `lib.rs`, not referenced in any `.rs` file. Our F4/F5 modules should go in `audio/` or be declared as new top-level modules.
- **`BlockNoteEditor/Editor.tsx` already exists** — a working BlockNote rich text editor component. F11 just needs to wire it to the `meeting_notes` DB table via Tauri commands.
- **`/notes/[id]` page is a static demo** — hardcoded sample data, not connected to the database. Replace with a real DB-backed implementation for F11.
- **Templates are at `frontend/src-tauri/templates/`** (not `src/templates/`) — bundled as Tauri resources via `tauri.conf.json` `"resources": ["templates/*.json"]`.
- **`silero_rs` and `ort` are already dependencies** — Silero VAD and ONNX Runtime already in `Cargo.toml`. F5 (sherpa-onnx) shares these foundations.
- **`speaker` column already exists** on `transcripts` but stores audio source (`'mic'`/`'system'`), not speaker identity. F5 must use `speaker_label` / `speaker_id` instead (ARCHITECTURE.md updated).
- **`posthog-rs`** is the telemetry crate to strip (decision 3). **`tauri-plugin-updater`** is the auto-updater to remove (decision 4).
- **Existing migrations** include licensing schema (`add_pro_license_custom_openai`, `add_grace_period_to_licensing`) — harmless with 0 rows, can be left in place.
