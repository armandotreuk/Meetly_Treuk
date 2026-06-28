# Migration Log — Personal Meetly vs Upstream

> Records the base commit, upstream-incompatible changes, and findings from code inspection.

---

## Base commit

- **Upstream repo:** https://github.com/Zackriya-Solutions/meetily
- **Base commit:** `0281737d87d26352fb0adc78c8c0975f691b23d1` (v0.4.0, "Merge pull request #502 from Zackriya-Solutions/meetily/release/v0.4.0")
- **Clone date:** 2026-06-28
- **Clone location:** `upstream/`

---

## Build environment

| Tool | Version | Status |
|---|---|---|
| Rust | 1.96.0 (stable, x86_64-pc-windows-msvc) | ✅ installed via rustup |
| Node.js | 24.16.0 | ✅ pre-installed |
| pnpm | 11.9.0 | ✅ via `corepack pnpm` |
| CMake | 11.13.0 | ✅ at `C:/Program Files/CMake/bin/cmake.exe` (not in PATH — use `CMAKE` env var) |
| VS 2026 Community | 18.0 | ✅ C++ workload (VC/Tools) present |
| Git | 2.54.0 | ✅ |

### ⚠️ Smart App Control (SAC) blocker

SAC is in enforced mode (`VerifiedAndReputablePolicyState: 1`). It blocks all unsigned executables, including Rust build scripts. **Local Rust compilation is impossible** until SAC is turned off or builds are deferred to CI. Frontend (Next.js) builds fine.

See `ROADMAP.md` → "Blocker: Smart App Control" for options.

### Target directory

`CARGO_TARGET_DIR` is set to `C:/Users/arman/meetily-build-target` via `upstream/.cargo/config.toml` to keep build artifacts out of OneDrive.

---

## Upstream code findings (2026-06-28)

### Module structure (confirmed in `lib.rs` lines 38–56)

Active modules: `analytics`, `api`, `audio`, `config`, `console_utils`, `database`, `notifications`, `ollama`, `onboarding`, `openai`, `anthropic`, `groq`, `openrouter`, `parakeet_engine`, `state`, `summary`, `tray`, `utils`, `whisper_engine`

**`audio_v2/` is dead code** — exists on disk but NOT declared in `lib.rs`, not referenced in any `.rs` file. Do not use as a base for F4/F5; use `audio/` or create new top-level modules.

### Database

- `database/` module has a `repositories/` subdirectory: `meeting.rs`, `setting.rs`, `summary.rs`, `transcript.rs`, `transcript_chunk.rs` — follow this pattern for new repositories (F11: `meeting_notes.rs`).
- `SettingsRepository` in `setting.rs` handles `api_key: Option<String>` for both model config and transcript config — this is where F10 encryption wraps reads/writes.
- 10 migrations in `migrations/`. Notable:
  - `20251101000000_add_pro_license_custom_openai.sql` — licensing schema (harmless, 0 rows in user's DB)
  - `20251110000001_add_speaker_field.sql` — adds `speaker` column to `transcripts` (stores `'mic'`/`'system'` audio source, NOT speaker identity)
  - `20251223000000_add_meeting_notes.sql` — `meeting_notes` table (exists, unused, F11 wires it up)
  - `20251229000000_add_gemini_api_key.sql` — another plaintext API key column (needs F10 encryption)

### Frontend

- `pnpm build` succeeds — 11 static pages: `/`, `/meeting-details`, `/notes/[id]`, `/settings`, `/_not-found`
- **`/notes/[id]` is a static demo** — hardcoded sample data (`team-sync-dec-26`, `product-review`, `project-ideas`, `action-items`), NOT connected to the database. F11 replaces this with a DB-backed implementation.
- **`BlockNoteEditor/Editor.tsx` already exists** — a working BlockNote rich text editor (accepts `initialContent: Block[]`, `onChange: (blocks: Block[]) => void`, `editable: boolean`). F11 wraps this component.
- **`MeetingDetails/` has tab-style panels** — `SummaryPanel.tsx`, `TranscriptPanel.tsx`, etc. F11 adds a `NotesPanel.tsx` here.
- Analytics components to strip (decision 3): `AnalyticsConsentSwitch.tsx`, `AnalyticsDataModal.tsx`, `AnalyticsProvider.tsx`
- Updater components to strip (decision 4): `UpdateCheckProvider.tsx`, `UpdateDialog.tsx`, `UpdateNotification.tsx`
- `TranscriptView.tsx` / `VirtualizedTranscriptView.tsx` — where F5 speaker labels render

### Tauri config (`tauri.conf.json`)

Three places to disable the auto-updater (decision 4):
1. Line 66: remove `"updater:default"` from capabilities permissions
2. Line 91: change `"createUpdaterArtifacts": true` → `false`
3. Lines 113-120: remove the entire `"plugins": { "updater": { ... } }` block

### Cargo.toml key dependencies

- `posthog-rs = "0.3.7"` — telemetry crate to strip (decision 3)
- `tauri-plugin-updater = "2.3.0"` — auto-updater plugin to remove (decision 4)
- `silero_rs` — Silero VAD already present (useful for F5)
- `ort = "2.0.0-rc.10"` — ONNX Runtime already present (shared with F5 sherpa-onnx)
- `whisper-rs = "0.13.2"` with `["raw-api"]` on Windows (CPU-only default)
- `whatlang = "0.16.4"` — language detection (useful for PT-BR detection)
- `ffmpeg-sidecar` — audio processing
- `sqlx = "0.8"` with sqlite — database
- `nnnoiseless = "0.5"` — RNNoise noise suppression
- `ebur128 = "0.1"` — loudness normalization
- `symphonia = "0.5.4"` — audio decoding (AAC, MP4, MP3, FLAC, OGG, Vorbis, PCM, WAV)

### Templates

Located at `frontend/src-tauri/templates/` (7 JSON files), bundled as Tauri resources via `tauri.conf.json` `"resources": ["templates/*.json"]`. NOT under `src/` — F1's template registry reads from here for builtins and from the DB for user templates.
