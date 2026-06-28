# Personal Meetly — Feature Implementation Scope

> **Base:** Fork of [Zackriya-Solutions/meetily](https://github.com/Zackriya-Solutions/meetily) (MIT)
> **Goal:** Self-hosted desktop meeting assistant with Pro-equivalent features built as original code on top of the open-source Community Edition.
> **Status:** Scoping phase — no code yet.

---

## 1. Project Goals & Principles

### Goals
- **Full local ownership** — no license server, no telemetry, no vendor lock-in
- **Pro-equivalent feature set** — custom templates, PDF/DOCX export, auto-detect meetings, speaker diarization, calendar integration, Notion/Obsidian sync, meeting chat/Q&A
- **Forward-compatible** — keep merge-ability with upstream Community Edition where possible
- **Original implementation** — all Pro-equivalent features written from scratch (not extracted from the closed-source Pro binary)

### Principles
1. **MIT license preserved** — derivative work honors upstream MIT
2. **No circumvention** — we never touch the vendor's RSA license system; we simply don't include it (the open-source repo doesn't have it)
3. **Original code only** — features are designed and implemented independently; no reverse-engineering of the Pro `.exe`
4. **Modular additions** — new features live in clearly separated modules (`features/`) so upstream merges stay clean
5. **Privacy-first maintained** — all processing stays local; new integrations (Notion/Obsidian) are opt-in and explicit

---

## 2. Baseline (what we get for free from upstream)

Already in the MIT repo, no work needed:
- Real-time transcription (Whisper + Parakeet engines)
- 7 summary templates (Daily Standup, Standard Meeting, Project Sync, Retrospective, Sales/Client, Psychiatric SOAP, Obsidian Vault)
- LLM clients: Claude, Groq, OpenAI, Ollama, OpenRouter, Custom OpenAI-compatible endpoint
- SQLite storage (meetings, transcripts, summaries, settings)
- Audio import (MP4/M4A/WAV/MP3/FLAC/OGG/AAC/MKV/WebM/WMA)
- Markdown export
- System-audio capture (bot-free, works with any meeting platform)
- Desktop notifications, meeting history, multi-language transcription

---

## 3. Features to Implement (scope)

Each feature below is scoped with: **description, why, files affected, dependencies, complexity (S/M/L/XL), and implementation approach.**

### F1 — Custom Summary Templates (Pro-equivalent)
- **Description:** Allow users to add, edit, delete, and select their own summary templates from the UI (not just the 7 hardcoded ones).
- **Why:** Pro-gated in closed build; core productivity feature.
- **Files:**
  - `frontend/src-tauri/src/templates/mod.rs` (new module — template registry)
  - `frontend/src-tauri/templates/` (existing folder, expose to UI)
  - `frontend/src/components/settings/TemplateEditor.tsx` (new UI)
  - `frontend/src/components/SummaryTemplatePicker.tsx` (extend existing picker)
  - DB migration: `templates` table (id, name, description, schema_json, created_at, updated_at, is_builtin)
- **Dependencies:** None
- **Complexity:** M
- **Approach:**
  1. Add `templates` SQLite table storing user-authored template JSON
  2. Template loader reads from DB first, falls back to bundled JSON files
  3. UI: simple JSON editor with live preview of section structure
  4. Validation against the existing template schema (name, description, sections[].title/instruction/format)
  5. Builtin templates flagged `is_builtin=1`, read-only

### F2 — PDF Export (Pro-equivalent)
- **Description:** Export meeting summaries to formatted PDF.
- **Why:** Pro-gated; required for sharing/archiving in regulated industries.
- **Files:**
  - `frontend/src-tauri/Cargo.toml` (add `printpdf` or `typst` crate)
  - `frontend/src-tauri/src/export/pdf.rs` (new)
  - `frontend/src-tauri/src/export/mod.rs` (new module)
  - `frontend/src/components/ExportMenu.tsx` (extend)
- **Dependencies:** F1 (templates define export structure)
- **Complexity:** M
- **Approach:**
  1. Render summary markdown → HTML → PDF via `printpdf` (or shell out to `wkhtmltopdf`/`typst` if richer styling needed)
  2. Template-driven layout: title, metadata, sections, action-item tables
  3. Embed meeting title, date, attendees in header/footer
  4. Save dialog via Tauri `dialog.save`

### F3 — DOCX Export (Pro-equivalent)
- **Description:** Export meeting summaries to Microsoft Word (.docx).
- **Why:** Pro-gated; enterprise standard format.
- **Files:**
  - `frontend/src-tauri/Cargo.toml` (add `docx-rs` crate)
  - `frontend/src-tauri/src/export/docx.rs` (new)
  - `frontend/src/components/ExportMenu.tsx` (extend)
- **Dependencies:** F1
- **Complexity:** M
- **Approach:**
  1. Use `docx-rs` to build Heading/Paragraph/Table elements from the structured summary JSON
  2. Map each template `format` type (`paragraph` → Paragraph, `list` → BulletList, `string` → Paragraph)
  3. Preserve markdown tables from `item_format` as Word tables
  4. Save dialog

### F4 — Auto-Detect & Auto-Join Meetings (Pro-equivalent)
- **Description:** Automatically detect when a meeting platform (Zoom, Teams, Meet, Webex, Discord) starts producing audio and begin recording without manual click.
- **Why:** Pro-gated; eliminates "forgot to record" failures.
- **Files:**
  - `frontend/src-tauri/src/audio_v2/detector.rs` (new)
  - `frontend/src-tauri/src/state.rs` (wire detector into state machine)
  - `frontend/src/components/settings/AutoDetectSettings.tsx` (new)
  - `frontend/src-tauri/src/config.rs` (add platform signatures)
- **Dependencies:** None
- **Complexity:** L
- **Approach:**
  1. Detect active meeting via window-title polling (Windows: `EnumWindows` + title match against known platform strings — "Zoom Meeting", "Microsoft Teams", "Google Meet", etc.)
  2. Cross-check with system audio session state (audio is flowing → meeting is live)
  3. Auto-start recording if toggle enabled AND a recognized meeting window is focused
  4. Auto-stop when window closes or audio stream ends for >30s
  5. Per-platform enable/disable toggles in settings
  6. **Privacy safeguard:** require explicit opt-in; show persistent notification when auto-recording is active

### F5 — Speaker Diarization (Pro-equivalent + upstream-planned)
- **Description:** Automatically separate and label speakers in the transcript (Speaker 1, Speaker 2, ...).
- **Why:** Pro "coming soon"; essential for multi-person meetings.
- **Files:**
  - `frontend/src-tauri/Cargo.toml` (add `sherpa-rs` crate, or vendor `sherpa-onnx` FFI)
  - `frontend/src-tauri/src/audio_v2/diarization.rs` (new — wraps `sherpa_rs::diarize::Diarize`)
  - `frontend/src-tauri/src/transcript_pipeline.rs` (integrate as post-transcription step)
  - `frontend/src/components/TranscriptView.tsx` (render speaker labels)
  - `frontend/src/components/settings/DiarizationSettings.tsx` (new)
  - `models/diarization/` (downloaded models, alongside existing parakeet/whisper model dir)
- **Dependencies:** None (parallel to F4)
- **Complexity:** L (reduced from XL — turnkey sherpa-onnx pipeline + Rust bindings + Tauri examples)
- **Approach:**
  1. Use **sherpa-onnx (k2-fsa)** via `sherpa-rs` — segmentation + embedding + fast clustering pipeline, ONNX-native, no Python runtime
  2. Segmentation model: `sherpa-onnx-pyannote-segmentation-3.0` (~6MB, or 1.5MB int8) — permissive weights
  3. Embedding model: `3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx` (~30MB) — multilingual, preferred for PT-BR; fallback `nemo_en_titanet_small.onnx` (~23MB, fastest)
  4. Clustering: built-in fast clustering (threshold-based, unknown k) or fixed-k if user specifies speaker count
  5. Post-processing: run after Whisper/Parakeet transcription completes; merge speaker labels onto transcript segments by timestamp overlap (show "Identifying speakers..." spinner in UI)
  6. CPU RTF ≈ 0.12–0.24 — trivial overhead on hardware that already runs Whisper/Parakeet
  7. UI: rename "Speaker 1" → "Alice" with a one-click prompt; optional voice-profile persistence across meetings (opt-in)
  8. Add models to onboarding download flow (alongside Parakeet/Whisper)
  9. **License:** Apache 2.0 code; segmentation weights Apache 2.0 / CC-BY-4.0 (attribution documented in our LICENSE notice); avoid `reverb-diarization-v1` (non-commercial)
- **See:** `research/diarization.md` for full model comparison and open items

### F6 — Calendar Integration (Pro "coming soon")
- **Description:** Read local calendar (.ics / Google Calendar / Outlook) to show upcoming meetings, pre-arm auto-detect, and attach meeting metadata (title, attendees) to recordings.
- **Why:** Pro roadmap; reduces manual title entry and improves metadata accuracy.
- **Files:**
  - `frontend/src-tauri/src/calendar/mod.rs` (new)
  - `frontend/src-tauri/src/calendar/ics_parser.rs` (new)
  - `frontend/src-tauri/src/calendar/google.rs` (new, OAuth)
  - `frontend/src/components/CalendarPanel.tsx` (new)
  - `frontend/src/components/settings/CalendarSettings.tsx` (new)
- **Dependencies:** F4 (auto-detect benefits most from calendar pre-arm)
- **Complexity:** L
- **Approach:**
  1. **Phase 1:** Local `.ics` file import — parse with `ics` crate, show upcoming events, pre-fill meeting title from event name
  2. **Phase 2:** Google Calendar via OAuth (Tauri deep-link redirect) — read-only scope `calendar.events.readonly`
  3. **Phase 3:** Outlook via Microsoft Graph (same OAuth flow)
  4. Pre-arm auto-detect 5 min before calendar event start
  5. Attach `calendar_event_id` to meeting record for later linking

### F7 — Notion Integration (Pro "planned")
- **Description:** Push meeting summaries to a Notion database as new pages.
- **Why:** Pro planned; most-requested integration.
- **Files:**
  - `frontend/src-tauri/src/integrations/mod.rs` (new)
  - `frontend/src-tauri/src/integrations/notion.rs` (new)
  - `frontend/src/components/integrations/NotionConnect.tsx` (new)
  - `frontend/src/components/SummaryView.tsx` (add "Send to Notion" action)
- **Dependencies:** F1 (template defines what gets sent)
- **Complexity:** M
- **Approach:**
  1. Notion internal integration token (user provides, stored encrypted in `settings`)
  2. On first connect: list databases → user picks target database + property mapping
  3. "Send to Notion" creates a page with summary markdown as page content, properties from meeting metadata
  4. Respect Notion's 2MB block limit (split long summaries)
  5. **Privacy:** token stored locally only; no cloud relay

### F8 — Obsidian Vault Sync (Pro "planned")
- **Description:** Write meeting summaries as markdown notes into a local Obsidian vault with wiki-links preserved.
- **Why:** Pro planned; template `meeting_transcript_summary.json` already uses Obsidian wiki-link syntax.
- **Files:**
  - `frontend/src-tauri/src/integrations/obsidian.rs` (new)
  - `frontend/src/components/integrations/ObsidianConnect.tsx` (new)
  - `frontend/src/components/SummaryView.tsx` (add "Save to Vault" action)
- **Dependencies:** F1
- **Complexity:** S
- **Approach:**
  1. User picks vault folder via Tauri `dialog.open` (directory picker)
  2. Save summary as `<vault>/Meetings/<YYYY-MM-DD> <title>.md`
  3. Preserve `[[wiki-links]]` from template output verbatim
  4. Optional: append to daily note (`<vault>/Daily/<YYYY-MM-DD>.md`) under a `## Meetings` heading
  5. Conflict policy: append `(2)` suffix if file exists

### F9 — Chat with Meetings (Pro "coming soon")
- **Description:** Ask questions about a past meeting ("what did we decide about X?", "list Alice's action items") using the same local LLM already configured.
- **Why:** Pro roadmap; high-value Q&A over transcripts.
- **Files:**
  - `frontend/src-tauri/src/chat/mod.rs` (new)
  - `frontend/src-tauri/src/chat/retriever.rs` (new — simple BM25 or embeddings index)
  - `frontend/src/components/MeetingChat.tsx` (new)
- **Dependencies:** None (uses existing LLM client)
- **Complexity:** L
- **Approach:**
  1. Index transcript chunks + summary sections at meeting-save time (SQLite FTS5 or in-memory BM25)
  2. On user question: retrieve top-k relevant chunks → build context prompt → call configured LLM
  3. Show answers with inline citations linking back to transcript timestamps
  4. Cross-meeting queries ("across all meetings this week, what did we decide about pricing?")
  5. **No external API** — uses whatever local/remote LLM the user already configured

### F10 — Encrypted API Key Storage (security hardening, not in upstream)
- **Description:** Encrypt all API keys at rest in the SQLite DB using OS keychain (Windows Credential Manager / macOS Keychain / Secret Service).
- **Why:** Current open-source build stores keys in plaintext (verified in your local DB). Critical security gap.
- **Files:**
  - `frontend/src-tauri/Cargo.toml` (add `keyring` crate)
  - `frontend/src-tauri/src/database/migrations/` (new migration to encrypt existing plaintext keys)
  - `frontend/src-tauri/src/settings.rs` (wrap key reads/writes)
- **Dependencies:** None
- **Complexity:** S
- **Approach:**
  1. Use `keyring` crate for cross-platform OS credential store
  2. Store a single master encryption key in OS keychain
  3. AES-256-GCM encrypt all `*ApiKey` columns in `settings` and `transcript_settings`
  4. One-time migration on app start: detect plaintext → encrypt → overwrite
  5. **Do this first** — before adding any new integrations that store more credentials

### F11 — Meeting Notes Editor (uses existing empty table + existing editor component)
- **Description:** The `meeting_notes` table exists but is unused, and a `BlockNoteEditor/Editor.tsx` component already exists in the upstream frontend. Wire the editor to the DB with autosave.
- **Why:** Schema and editor are already there; trivial to connect; useful for human annotations alongside AI summary.
- **Files:**
  - `frontend/src-tauri/src/database/repositories/meeting_notes.rs` (new — CRUD, follows existing repository pattern)
  - `frontend/src-tauri/src/database/commands.rs` (add `get_notes`, `save_notes` Tauri commands)
  - `frontend/src/components/MeetingDetails/NotesPanel.tsx` (new — wraps existing `BlockNoteEditor/Editor.tsx`)
  - `frontend/src/components/MeetingDetails/` (add "Notes" tab)
  - `frontend/src/app/notes/[id]/page.tsx` (replace static demo with DB-backed implementation)
- **Dependencies:** None
- **Complexity:** S (reduced — editor component already exists, just needs DB wiring)

---

## 4. Implementation Phases (suggested order)

### Phase 0 — Foundation (1–2 days)
- [ ] Fork repo, set up build on Windows, verify dev mode runs
- [ ] **F10: Encrypted key storage** (security first — fix the plaintext key leak before adding more integrations)
- [ ] **F11: Meeting Notes Editor** (quick win, uses existing schema)

### Phase 1 — Core Pro-equivalents (3–5 days)
- [ ] **F1: Custom Templates** (unblocks F2, F3, F7, F8)
- [ ] **F2: PDF Export**
- [ ] **F3: DOCX Export**
- [ ] **F4: Auto-Detect Meetings**

### Phase 2 — Integrations (2–4 days)
- [ ] **F8: Obsidian Vault Sync** (simplest, high value for your Obsidian template)
- [ ] **F7: Notion Integration**

### Phase 3 — Advanced (3–6 days)
- [ ] **F5: Speaker Diarization** (L — sherpa-onnx via sherpa-rs, post-processing pipeline)
- [ ] **F6: Calendar Integration** (ICS first, then Google/Outlook OAuth)
- [ ] **F9: Chat with Meetings** (retrieval index + LLM Q&A)

---

## 5. Repository Structure (planned)

```
Personal Meetly/                      ← C:\Users\arman\OneDrive\Repositório Projetos\Personal Meetly
├── README.md                         ← project overview + setup
├── SCOPE.md                          ← this file
├── ARCHITECTURE.md                   ← (next) module diagram + data flow
├── ROADMAP.md                        ← (next) phased delivery tracker
├── upstream/                         ← git submodule pointing to Zackriya-Solutions/meetily
├── src/                              ← our derivative work
│   ├── frontend/
│   │   ├── src-tauri/
│   │   │   ├── src/
│   │   │   │   ├── export/           ← F2, F3
│   │   │   │   ├── templates/        ← F1
│   │   │   │   ├── audio_v2/         ← F4, F5 (extend upstream)
│   │   │   │   ├── calendar/         ← F6
│   │   │   │   ├── integrations/     ← F7, F8
│   │   │   │   ├── chat/             ← F9
│   │   │   │   └── security/         ← F10
│   │   │   └── migrations/
│   │   └── src/components/
│   │       ├── settings/
│   │       ├── integrations/
│   │       └── meeting/
│   └── docs/
└── build/                            ← local build artifacts (gitignored)
```

---

## 6. Upstream Merge Strategy

- Track `upstream/main` as a git remote
- Keep our additions in clearly-named modules under `src/` (not inlined into upstream files where avoidable)
- When upstream ships a feature we've also built, prefer upstream if equivalent, keep ours if more capable
- Document upstream-incompatible changes in `MIGRATION.md`
- Rebase our branch on upstream before each release

---

## 7. Out of Scope (explicit non-goals)

- ❌ Reverse-engineering or extracting code from the closed-source Pro `meetily.exe`
- ❌ Bypassing any license verification (we simply don't include it)
- ❌ Cloud relay of meeting audio or transcripts
- ❌ Mobile apps (desktop only, matching upstream)
- ❌ Rebranding for resale (this is a personal/internal fork)

---

## 8. Decisions (locked)

1. **Distribution:** Shared with team — needs CI build pipeline, binary releases, and an internal update channel.
2. **Model download hosting:** Both — try upstream HuggingFace URLs first, fall back to our own mirrored storage for reliability.
3. **Telemetry:** Strip entirely — no analytics, no opt-in/opt-out, no collection. Clean removal of upstream's analytics module.
4. **Update mechanism:** Disable Tauri auto-updater — our build diverges from upstream; updates handled manually or via our own internal channel later.
5. **F5 diarization model:** **sherpa-onnx (k2-fsa)** via `sherpa-rs` — Apache 2.0 code, ONNX-native, CPU RTF 0.12–0.24, Rust bindings + in-repo Tauri examples, post-processing (batch) pipeline. Segmentation: `sherpa-onnx-pyannote-segmentation-3.0` (~6MB). Embedding: 3D-Speaker ERes2Net (~30MB, multilingual, preferred for PT-BR) with NeMo TitaNet small (~23MB) fallback. `reverb-diarization-v1` and DiariZen excluded (non-commercial). See `research/diarization.md`.

---

_Next step: write `ARCHITECTURE.md` and `ROADMAP.md`, then start Phase 0 (fork, build verify, F10, F11)._
