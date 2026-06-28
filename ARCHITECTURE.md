# Personal Meetly — Architecture

> Companion to `SCOPE.md`. Describes the module layout, data flow, integration points, and database schema additions for our derivative work on top of the MIT-licensed [Zackriya-Solutions/meetily](https://github.com/Zackriya-Solutions/meetily) Community Edition.

---

## 1. High-level stack

```
┌────────────────────────────────────────────────────────────────────┐
│  Tauri desktop app (frontend/)                                     │
│                                                                    │
│  ┌──────────────────────────────┐   ┌────────────────────────────┐ │
│  │  Next.js UI (src/)           │   │  Rust core (src-tauri/src) │ │
│  │  - React components           │◄─►│  - Tauri commands         │ │
│  │  - Settings / meeting views   │   │  - sqlx (SQLite)          │ │
│  │  - Tauri IPC (invoke)         │   │  - audio engines          │ │
│  └──────────────────────────────┘   └─────────────┬──────────────┘ │
│                                                    │                │
│  llama-helper (gRPC sidecar) ◄────────────────────┘                │
│  - llama-cpp-2 local LLM (Qwen / Gemma GGUF)                       │
└────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
        Local SQLite DB (meeting_minutes.sqlite) + OS keychain
        Local ONNX/GGUF models in AppData/.../models/
```

**Existing upstream components** (untouched unless noted):
- `audio/` — system-audio capture, Whisper + Parakeet engines (note: `audio_v2/` exists on disk but is dead code — not declared in `lib.rs`, not compiled)
- `whisper_engine/`, `parakeet_engine/` — ONNX transcription
- `anthropic/`, `groq/`, `ollama/`, `openai/`, `openrouter/` — LLM clients
- `summary/` — summary pipeline
- `database/` — sqlx migrations + queries
- `notifications/`, `tray.rs`, `onboarding.rs`, `state.rs`, `config.rs`

**Our additions** (clearly separated modules):
- `templates/` — F1 custom templates (note: upstream templates live at `frontend/src-tauri/templates/` as Tauri resources, not under `src/`)
- `export/` — F2 PDF, F3 DOCX
- `audio/detector.rs` — F4 auto-detect (note: upstream `audio_v2/` is dead code — not declared in `lib.rs`; our additions go in `audio/` or as new top-level modules)
- `diarization/` — F5 sherpa-onnx speaker diarization (new top-level module)
- `calendar/` — F6 ICS + Google/Outlook
- `integrations/` — F7 Notion, F8 Obsidian
- `chat/` — F9 meeting Q&A
- `security/` — F10 encrypted key storage
- `database/meeting_notes.rs` — F11 notes editor backend

**Removed from upstream:**
- `analytics/` — stripped entirely (decision 3)
- Tauri auto-updater config — disabled (decision 4)

---

## 2. Module dependency graph

```
                     ┌─────────────┐
                     │  state.rs   │  (app state machine, recording lifecycle)
                     └──────┬──────┘
            ┌───────────────┼────────────────┐
            ▼               ▼                ▼
      ┌──────────┐   ┌──────────┐    ┌──────────────┐
      │ audio_v2 │   │ summary/ │    │ database/    │
      │  (cap +  │   │ (LLM)    │    │ (sqlx, migs) │
      │ engines) │   └────┬─────┘    └──────┬───────┘
      └────┬─────┘        │                 │
           │         ┌────┴────┐            │
           ▼         ▼         ▼            │
    ┌────────────┐  anthropic groq ollama   │
    │ detector   │  openai   openrouter     │
    │ (F4)       │                          │
    └────┬───────┘                          │
         │                                  │
         ▼                                  │
    ┌────────────┐    ┌─────────────┐       │
    │ diarization│    │ templates/  │◄──────┤  (F1 reads/writes templates table)
    │ (F5)       │    │ (F1)        │       │
    │ sherpa-rs  │    └──────┬──────┘       │
    └────┬───────┘           │              │
         │                   ▼              │
         │             ┌─────────────┐      │
         │             │ export/     │◄─────┤  (F2/F3 read meeting + template)
         │             │ pdf  docx   │      │
         │             └─────────────┘      │
         │                                  │
         ▼                                  ▼
    ┌────────────┐    ┌─────────────┐  ┌─────────────┐
    │ calendar/  │    │integrations/│  │  security/  │
    │ ics google │    │ notion      │  │ keyring +   │
    │ (F6)       │    │ obsidian    │  │ AES-256-GCM │
    └────────────┘    │ (F7/F8)     │  │ (F10)       │
                      └─────────────┘  └──────┬──────┘
                                              │
                                              ▼
                                       ┌─────────────┐
                                       │  chat/      │
                                       │ retriever   │
                                       │ (F9, FTS5)  │
                                       └─────────────┘
```

**Key edges:**
- `templates/` (F1) is the foundation — `export/`, `integrations/` consume the template schema to know what to render/send.
- `security/` (F10) wraps **all** credential reads/writes in `database/` and `integrations/`. Implemented in Phase 0 before any new integration stores tokens.
- `diarization/` (F5) and `detector/` (F4) plug into `audio_v2/` and the recording state machine, not into the LLM path.
- `chat/` (F9) reads from `database/` (transcripts + summaries) and calls the existing LLM clients; no new model.

---

## 3. Data flow — recording a meeting

```
1. Meeting start
   ├─ F4 detector: window-title poll → recognized meeting platform + audio flowing
   ├─ (optional) F6 calendar: pre-arm 5 min before event start, pre-fill title/attendees
   └─ state.rs: start recording session → audio_v2 captures system audio

2. Transcription (real-time)
   ├─ audio_v2 → Whisper OR Parakeet ONNX engine → transcript segments (text + timestamps)
   ├─ segments streamed to UI as they arrive
   └─ segments persisted to `transcripts` / `transcript_chunks` tables

3. Meeting end
   ├─ state.rs: stop capture, finalize transcript
   ├─ F5 diarization (if enabled): sherpa-onnx diarize(full_audio)
   │     → (start, end, speaker) regions
   │     → merge onto transcript segments by timestamp overlap
   │     → persist speaker labels to `transcripts.speaker` (new column)
   ├─ summary/: build prompt from selected template (F1) → call configured LLM
   │     → persist structured summary to `summary_processes`
   └─ (optional) F8 Obsidian / F7 Notion: push summary on completion (opt-in)

4. After meeting
   ├─ F2/F3 export: render summary to PDF/DOCX from template structure
   ├─ F9 chat: index chunks into FTS5; Q&A via LLM with retrieval
   ├─ F11 notes editor: user adds manual markdown notes → `meeting_notes` table
   └─ F10: any new API tokens entered in settings → encrypted via OS keychain
```

---

## 4. Integration points (where our code touches upstream)

| Touch point | Upstream file | Our change | Merge risk |
|---|---|---|---|
| Tauri command registry | `lib.rs` | Register new commands for F1–F11 | Low — append-only |
| Recording state machine | `state.rs` | Hook F4 detector + F5 post-step | Medium — follow upstream event names |
| Settings read/write | `database/settings.rs` | Wrap in `security/` for F10 | Medium — central change, do in Phase 0 |
| Transcript pipeline | `summary/` + `transcript_pipeline.rs` | Insert F5 diarization merge | Low — new post-step |
| Template picker | `components/SummaryTemplatePicker.tsx` | Extend with F1 user templates | Low — additive |
| Export menu | `components/ExportMenu.tsx` | Add F2/F3 entries | Low — additive |
| Meeting detail view | `components/MeetingDetail.tsx` | Add F11 notes tab | Low — additive |
| Cargo manifest | `src-tauri/Cargo.toml` | Add `printpdf`, `docx-rs`, `keyring`, `sherpa-rs` | Low |
| Migrations | `src-tauri/migrations/` | New migration files (F1, F5, F10) | Low — sqlx versioned |
| Build config | `tauri.conf.json` | Disable updater, strip analytics | Low |
| Onboarding | `onboarding.rs` | Add diarization model download (F5) | Low — additive |

**Merge strategy:** keep additions in new modules; when an upstream file must change, isolate the change to a clearly commented block and re-apply on rebases (documented in `MIGRATION.md`).

---

## 5. Database schema additions

New migrations live in `frontend/src-tauri/migrations/` as numbered SQL files (`sqlx` applies them in order).

### Migration: F1 — custom templates
```sql
CREATE TABLE templates (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  name         TEXT NOT NULL,
  description  TEXT,
  schema_json  TEXT NOT NULL,          -- validated against upstream template schema
  is_builtin   INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
-- Builtin templates seeded with is_builtin=1 from the existing JSON files.
```

### Migration: F5 — speaker labels on transcripts

> **Note:** the upstream `transcripts` table already has a `speaker` column (migration `20251110000001_add_speaker_field.sql`), but it stores the **audio source** (`'mic'` / `'system'`), not speaker identity. We use `speaker_label` / `speaker_id` to avoid collision.

```sql
ALTER TABLE transcripts ADD COLUMN speaker_label TEXT;  -- "Speaker 1", "Alice", ...
ALTER TABLE transcripts ADD COLUMN speaker_id   INTEGER; -- stable index within a meeting
CREATE TABLE speaker_profiles (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  meeting_id   INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  speaker_id   INTEGER NOT NULL,
  display_name TEXT,
  embedding    BLOB,                  -- optional, for cross-meeting voice profiles (opt-in)
  UNIQUE(meeting_id, speaker_id)
);
```

### Migration: F6 — calendar linking
```sql
ALTER TABLE meetings ADD COLUMN calendar_event_id TEXT;
ALTER TABLE meetings ADD COLUMN calendar_source   TEXT; -- 'ics' | 'google' | 'outlook'
CREATE TABLE calendar_cache (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  source        TEXT NOT NULL,
  event_id      TEXT NOT NULL,
  title         TEXT,
  start_utc     TEXT NOT NULL,
  end_utc       TEXT NOT NULL,
  attendees     TEXT,                 -- JSON array
  UNIQUE(source, event_id)
);
```

### Migration: F7/F8 — integration state
```sql
CREATE TABLE integrations (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  provider    TEXT NOT NULL UNIQUE,   -- 'notion' | 'obsidian'
  config_json TEXT NOT NULL,          -- encrypted tokens referenced via security/ keyring
  enabled     INTEGER NOT NULL DEFAULT 0,
  updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### Migration: F9 — chat retrieval index
```sql
CREATE VIRTUAL TABLE transcript_fts USING fts5(
  meeting_id UNINDEXED,
  chunk_id   UNINDEXED,
  content,
  tokenize = 'porter unicode61'
);
-- Populated at meeting-save time from transcript_chunks.
```

### Migration: F10 — encrypted key storage
No schema change to `settings` columns (we keep the existing `*ApiKey` columns but store ciphertext). Added table for key metadata:
```sql
CREATE TABLE key_metadata (
  key_name     TEXT PRIMARY KEY,       -- e.g. 'openrouter_api_key'
  encrypted    INTEGER NOT NULL DEFAULT 0,
  keyring_id   TEXT,                   -- OS keychain entry id holding the data key
  rotated_at   TEXT
);
```
On app start, a one-time migration detects plaintext rows (where `encrypted=0`) and encrypts them with AES-256-GCM using a master key stored in the OS keychain, then flips `encrypted=1`.

### F11 — meeting notes
The upstream `meeting_notes` table already exists but is unused. No migration needed; we wire up reads/writes in `database/meeting_notes.rs`.

---

## 6. Security architecture (F10)

```
┌──────────────────────────────────────────────────────────┐
│  OS keychain (Windows Credential Manager / macOS Keychain │
│                / Linux Secret Service)                    │
│   entry: "personal-meetly/master-key"  → 32 random bytes │
└────────────────────────────┬─────────────────────────────┘
                             │  (retrieved at app start)
                             ▼
                      AES-256-GCM
                             │
        ┌────────────────────┴───────────────────────┐
        ▼                                            ▼
  settings table (DB)                       integrations table (DB)
  openrouter_api_key (ciphertext)           notion_token (ciphertext)
  oci_api_key (ciphertext)                  ...
        │                                            │
        ▼                                            ▼
  security/ decrypt on read                  security/ decrypt on read
  security/ encrypt on write                 security/ encrypt on write
```

- Master key never touches disk in plaintext; lives only in the OS keychain.
- Per-row nonce stored alongside ciphertext (12-byte GCM nonce prepended to the ciphertext blob).
- One-time migration on first run with the new build encrypts all existing plaintext keys.
- `settings.rs` getters/setters are the only sanctioned path; raw SQL reads of key columns are forbidden elsewhere (enforced by a lint + code review).

---

## 7. Build & distribution

- **Dev:** `cd frontend && pnpm install && pnpm tauri:dev`
- **Release:** CI pipeline (GitHub Actions or internal) builds per-OS binaries; Tauri auto-updater disabled (decision 4); updates delivered via our own internal channel later.
- **Model hosting (decision 2):** onboarding tries upstream HuggingFace / sherpa-onnx release URLs first, falls back to our own mirrored storage for reliability.
- **Telemetry (decision 3):** `analytics/` module removed; no analytics event emitted anywhere in our additions.

---

## 8. Privacy guarantees preserved

- All transcription, diarization, summarization, and chat run locally or via the user's own configured LLM endpoint — no cloud relay of audio or transcripts.
- Notion (F7) is the only feature that sends data out, and only on explicit per-meeting "Send to Notion" action (opt-in, token stored encrypted).
- Obsidian (F8) writes to a local vault folder only.
- No analytics, no auto-update beacon.

---

## 9. Open architectural questions (resolve in Phase 0)

1. **sherpa-rs maintenance status** — if `thewh1teagle/sherpa-rs` is archived, vendor a thin FFI wrapper around sherpa-onnx C++ instead.
2. **PT-BR embedding model** — smoke-test 3D-Speaker ERes2Net vs NeMo TitaNet on a Portuguese clip before locking F5's embedding model.
3. **Calendar OAuth redirect** — decide Tauri deep-link vs. local loopback HTTP server for Google/Outlook OAuth (F6 Phase 2/3).
4. **Cross-meeting voice profiles** — decide whether F5 stores embeddings persistently (opt-in) or resets per meeting; affects privacy posture.

_Next: `ROADMAP.md` tracks delivery; then Phase 0 begins (fork, build verify, F10, F11)._
