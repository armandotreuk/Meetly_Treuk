# Sprint 5: Search Surfaces And Release Hardening

## Status

Planned, blocked by Sprint 4 approval and completion.

Revised 2026-08-21 after pre-implementation critique: packaging descoped to
Windows x64, derived-disk gate added, kill-switch UI added, and a sidebar
reranking guard added. Estimate: 8-12 working days.

## Goal

Extend reviewed hybrid retrieval to every approved search/context surface, give
users safe local index controls and diagnostics, package both models in the
supported Windows x64 desktop build, and close the program only after
250,000-document, crash, upgrade, deletion, privacy, and installed-application
validation passes.

## Architecture Authority

All work follows [`architecture.md`](architecture.md) and the reviewed runtime,
retrieval, context, and Deep-mode contracts from Sprints 1-4.

## Scope

### In Scope

- Meeting-level hybrid sidebar search with lexical fallback.
- Stable search-snapshot creation from displayed meeting IDs.
- Additive Tauri hybrid search and context commands.
- Additive/versioned MCP hybrid search and context tools.
- Retrieval index status, progress, pause/resume, rebuild, force-lexical, and
  error UI.
- Bundled model resources and license attribution in the Windows x64 package.
- Installed-package tokenizer, embedding, reranker, and hybrid-query smoke tests
  on Windows x64.
- 12k/50k/250k scale and concurrency validation.
- Crash/restart, dirty update, model upgrade, deletion, cache/sidecar corruption,
  and fallback validation.
- Final privacy, accessibility, source, documentation, and rollback review.

### Out Of Scope

- Removing existing FTS/Tauri/MCP lexical contracts.
- Remote embeddings.
- GPU ONNX execution.
- New cloud telemetry.
- MCP authentication.
- Vector indexing live unsaved transcripts.
- A general search UI redesign unrelated to hybrid ranking.
- **macOS ARM64 and Linux x64 packaging.** Deferred with the platforms; see
  `architecture.md` "Platform Scope". Do not add root-level workflows for those
  targets in this sprint, and do not claim support for them.

## Current State And Evidence

- `frontend/src/components/Sidebar/SidebarProvider.tsx:233-252` retains current
  search results for the sidebar.
- `frontend/src/components/Sidebar/index.tsx:328-359` derives search membership
  for Chat snapshot launch; result rendering is around `:777-805`.
- `frontend/src-tauri/src/api/api.rs:596-630` exposes lexical FTS search and
  rebuild commands.
- `frontend/src-tauri/src/api/chat.rs:1787-1804` exposes lexical context build.
- `frontend/src-tauri/src/mcp/server.rs:133-172` exposes lexical search/context
  tools with BM25/full-text semantics.
- `frontend/src-tauri/src/mcp/server.rs:174-233` uses shared Chat preparation.
- `frontend/src/app/settings/page.tsx:30-39,132-156` defines current settings
  navigation/content.
- `frontend/src/components/shared/DownloadProgressToast.tsx` provides a global
  progress-event UI pattern, although retrieval indexing is not a download.
- The repository root `.github/workflows/build-windows.yml` builds the Windows
  package but does not install it and execute ORT inference. It is the only
  active workflow in this fork; `upstream/.github/workflows/build-macos.yml`
  and `build-linux.yml` are nested where GitHub Actions never reads them and
  have never run.
- Sprints 2-4 provide backend status/control, hybrid search internals, context
  retention, and all persisted Chat behavior.

## Sprint Requirements

- Sidebar and context surfaces use the same reviewed retrieval service, not a
  parallel vector implementation.
- Existing explicit FTS/BM25 commands/tools remain available.
- Public scores have unambiguous names/semantics.
- Search snapshots continue storing ordered meeting IDs, never raw renderer
  snippets as trusted Chat context.
- Settings rebuild affects only derived semantic state.
- Model artifacts and licenses are present in signed/packaged applications.
- Installed-package inference is tested, not inferred from `cargo test`.
- Release scale includes index plus active model sessions in RAM measurements.
- Every semantic failure preserves lexical functionality.

## Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---:|---|---|---|---|---|
| 5.1 | Sidebar search | Consume the approved Tauri hybrid contract for meeting-level sidebar search, relevance snippets/provenance, lexical fallback, cancellation, and stable snapshot IDs. | M | Pending `worker-m` | 5.2 | Frontend/Rust tests prove ranking, fallback, folder filters, request cancellation, dedupe, keyboard/a11y, and snapshot membership. | Switch sidebar invocation back to existing FTS command. |
| 5.2 | API and MCP | Add explicit cancellable Tauri and versioned bounded Fast-only MCP hybrid search/context contracts while preserving all lexical tools. | M | Pending `worker-m` | Sprint 4 | Contract/execution tests prove surface classification, scope composition, provenance, source retention, cancellation/bounds, compatibility, and no score ambiguity. | Remove additive commands/tools; existing lexical APIs remain. |
| 5.3 | Index UX | Add Settings status, progress, pause/resume, rebuild, force-lexical toggle, error/retry, model/license, and local-size UI. | M | Pending `worker-m` | 2.5, 3.4 | UI/backend tests prove controls, lexical-only state, kill switch, disk reporting, accessibility, and rebuild cannot delete primary data. | Remove additive UI/commands; background index continues or can be disabled. |
| 5.4 | Packaging | Bundle, sign, attribute, install, and smoke-test embedding/reranker resources on **Windows x64**. | M | Pending `worker-m` | 1.5, Sprint 2 | The installed Windows package loads/tokenizes/embeds/reranks/queries and fails over when resources are unavailable. | Remove resources and ship lexical-only build; never claim hybrid availability. |
| 5.5 | Release qualification | Run/fix scale, concurrency, crash, upgrade, deletion, corruption, privacy, evaluation, native, and rollback gates. | L | Pending `worker-l` | 5.1-5.4 | All architecture release gates pass and final code/architecture reviews approve. | Disable semantic feature paths and retain FTS; restore pre-upgrade backup when release procedure requires. |

## Dependency Order

`Sprint 4 -> 5.2 -> 5.1 -> 5.5`

`2.5 -> 5.3 -> 5.5`

`1.5 + Sprint 2 -> 5.4 -> 5.5`

Tasks `5.1` and `5.3` may run in one approved batch only if their TypeScript
types, command registrations, and settings/search components are disjoint.
Task `5.2` owns all serialized Tauri/MCP hybrid contracts and runs before its
sidebar consumer. Task `5.5` is L and runs alone. Task `5.4` dropped from L to
M when packaging was descoped to Windows x64 only.

## Task Specifications

### 5.1 - Hybrid sidebar search [M]

**Outcome:** Visible meeting search benefits from semantic recall and local
reranking while preserving immediate lexical fallback and stable Chat snapshot
membership.

**Likely touchpoints:**

- `frontend/src/components/Sidebar/SidebarProvider.tsx`
- `frontend/src/components/Sidebar/index.tsx`
- Existing sidebar search hooks/tests
- `frontend/src/types/index.ts`
- Additive hybrid search command adapter
- Retrieval service search-purpose path

**Required implementation:**

- Invoke the Task 5.2 meeting-level hybrid search command after the existing
  debounce and folder-filter logic.
- Return one result per meeting with current title/folder, best retained
  snippet/source metadata, and explicit retrieval provenance.
- Do not expose raw vector, BM25, RRF, or reranker scores as one ambiguous
  public `rank` value.
- Preserve deterministic result ordering and stable meeting-ID dedupe.
- Preserve exact authoritative title-only matches as a lexical candidate
  channel in active, building, failed, and semantic-unavailable states.
- Resolve folder restrictions authoritatively in Rust.
- Use existing FTS results when semantic state is building, unavailable, or
  failed.
- Keep search-snapshot launch based on the ordered visible meeting IDs.
- Preserve loading/empty/error behavior, keyboard navigation, and accessibility.
- Avoid displaying internal model/index errors as raw Rust messages.
- Generate a stable request ID per search generation and cancel the prior
  backend request before/when a newer sidebar query supersedes it.
- **Bound the cost of reranking on every debounced keystroke.** Sidebar search
  runs the cross-encoder far more often than Chat does, and an empty-query
  guard alone is not sufficient. Required guards:
  - Use the `RetrievalPurpose::Search` shallower reranking depth approved in
    Sprint 1, not the Chat depth.
  - Do not run model inference below an approved minimum query length.
  - Cancel the previous request's in-flight reranking, not just its result
    publication, so superseded keystrokes stop consuming the ONNX permit.
  - Respect the shared scheduler's interactive priority so sidebar typing
    cannot starve an active Chat request.

**Acceptance criteria:**

- Semantic paraphrase fixture finds the expected meeting absent from weak FTS
  results.
- Exact name/number fixture remains correct through hybrid FTS contribution.
- Folder-filtered search never displays another folder/subtree meeting.
- Each meeting appears once with a current title/folder.
- Title-only fixtures match current sidebar behavior during active and every
  lexical-fallback state.
- Snapshot captures exactly the displayed ordered IDs up to the existing cap.
- Lexical fallback is automatic and visibly usable while index builds/fails.
- Empty query does not run model inference.
- **A query below the approved minimum length does not run model inference.**
- **A superseded search cancels its in-flight reranking and releases the model
  permit, proven by a test rather than only by discarded results.**
- **Sidebar reranking uses the `Search` purpose depth**, and a test asserts it
  does not use the deeper Chat depth.
- Typing rapidly while a Chat request is streaming does not delay that stream
  beyond the approved scheduler policy.
- Debounce/cancellation prevents stale older results replacing newer results.
- Search controls/results remain keyboard and screen-reader accessible.

**Required verification:**

```powershell
pnpm --dir "frontend" run typecheck
pnpm --dir "frontend" exec vitest run
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Record result contract, fallback UX, debounce/
cancellation ownership, folder behavior, and snapshot identity proof.

### 5.2 - Additive Tauri and MCP hybrid contracts [M]

**Outcome:** Local callers and MCP clients can explicitly request hybrid search
and retained hybrid context without breaking existing BM25 consumers.

**Likely touchpoints:**

- `frontend/src-tauri/src/api/api.rs`
- `frontend/src-tauri/src/api/chat.rs` context command area
- `frontend/src-tauri/src/lib.rs` command registration
- `frontend/src-tauri/src/mcp/server.rs`
- Shared serialized result/source types
- MCP/Tauri command tests

**Required implementation:**

- Keep `api_search_fts` and current lexical context behavior unchanged.
- Keep `api_search_transcripts` as an explicitly classified legacy transcript
  lexical command; do not silently convert it to meeting-level semantic search.
- Add clearly named hybrid search and hybrid context commands using the shared
  retrieval service.
- Define meeting/evidence provenance and retained source IDs explicitly.
- Accept exactly one tagged scope through backend validation. Reject conflicting
  folder/allowed-ID/query-folder combinations and enforce existing ID bounds;
  never accept raw renderer-provided evidence.
- Require a request ID for interactive Tauri hybrid search/context and provide
  cancellation/ownership so superseded sidebar work stops in Rust.
- Add versioned or separately named MCP hybrid search/context tools.
- Keep existing lexical MCP tools and descriptions.
- Update hybrid tool descriptions to state local semantic+lexical behavior and
  score/provenance semantics.
- Keep MCP Chat and hybrid search/context Fast-only in this release.
- Give MCP hybrid tools strict candidate/context/time bounds and server-side
  timeout. Do not claim MCP cancellation support.
- Enforce limits and local scope before serialization.
- Do not expose embeddings or private diagnostics.

**Acceptance criteria:**

- Existing lexical Tauri and MCP tests/consumers retain BM25 behavior.
- `api_search_transcripts`, `api_search_fts`, `api_build_context`, persisted
  Chat, sidebar, and MCP each have an explicit compatibility classification.
- Hybrid search returns meeting-level ranked results with unambiguous fields.
- Hybrid context returns only evidence retained under its context budget.
- Folder and allowed-ID inputs cannot widen scope.
- Invalid/oversized input fails with a stable safe error.
- MCP tool definitions and execution tests cover hybrid success, fallback,
  limit, and error semantics.
- MCP Chat still uses shared Fast Chat preparation and cannot request Deep.
- Superseded Tauri request IDs cancel queued/running retrieval without stale
  result publication.
- No provider/API key or raw content appears in logs.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib mcp::server::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

Add an executable MCP tool-routing test if current coverage only serializes
definitions. Record final tool names/versions and JSON examples.

**Worker report additions:** Provide compatibility table, exact score/provenance
semantics, scope composition rules, request-ID/cancellation behavior, MCP
timeout/bounds, and external rollback path.

### 5.3 - Semantic index Settings and diagnostics [M]

**Outcome:** Users can understand and safely recover local semantic indexing
without touching files or risking meeting data.

**Likely touchpoints:**

- `frontend/src/app/settings/page.tsx`
- Existing Chat model/settings component or a focused retrieval-status component
- `frontend/src/types/index.ts`
- Retrieval status/control Tauri commands from Sprint 2
- Frontend tests

**Required implementation:**

- Place retrieval/index controls in the most consistent existing Settings area;
  avoid a new top-level tab unless the content cannot fit Chat settings clearly.
- Show bundled model name/revision and license attribution/link.
- Show active/building/paused/failed/lexical-only status.
- Show indexed versus total meetings and background progress.
- Show estimated derived index disk/RAM information when available, **presented
  against the approved disk envelope** so a user can see when it is being
  approached rather than only an unanchored number.
- Provide pause, resume, rebuild, and retry controls.
- **Provide the `force_lexical_retrieval` toggle** delivered in Sprint 3.4.
  Explain it in user terms — searching and Chat keep working using exact word
  matching instead of meaning-based matching — and make clear that it does not
  delete the index and can be turned off at any time. This is the user's own
  rollback for a bad retrieval result and must be discoverable, not buried.
- Show forced-lexical state distinctly from a semantic failure state, so a user
  never mistakes their own setting for a broken index.
- Confirm rebuild clearly states that transcripts, summaries, notes, recordings,
  conversations, and FTS are not deleted.
- Disable conflicting actions while active and expose accessible live status.
- Map safe backend errors to actionable copy; do not expose raw paths/stacks.
- Do not provide model download/delete controls because models are bundled.

**Acceptance criteria:**

- Every backend status state renders deterministically.
- Progress is announced accessibly without excessive screen-reader chatter.
- Pause/resume/retry invokes the correct command and updates state.
- Rebuild requires confirmation and deletes only semantic derived state.
- Simulated rebuild failure preserves primary data and offers retry.
- Lexical-only state explains that search/Chat still work with lower semantic
  quality.
- **The force-lexical toggle round-trips: enabling it changes retrieval on the
  next request, it survives restart, disabling it restores hybrid behavior, and
  neither transition pauses or invalidates the index.**
- **User-forced lexical state is visually and textually distinct from a
  semantic failure state.**
- Disk usage is shown against its envelope, with a clear indication when the
  approved figure is exceeded.
- Model/license attribution is present.
- Layout works on desktop and narrow Settings views.

**Required verification:**

```powershell
pnpm --dir "frontend" run typecheck
pnpm --dir "frontend" exec vitest run
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Record Settings placement rationale, state/event
contract, rebuild safety wording, and accessibility checks.

### 5.4 - Windows model packaging and installed smoke [M]

**Outcome:** The supported Windows x64 installed application includes trusted
retrieval assets and can execute real tokenizer/embedding/reranker inference.

**Platform note:** this task covers Windows x64 only. macOS ARM64 and Linux x64
are deferred per `architecture.md` "Platform Scope" because this fork has no
active CI for them. Do not create root-level workflows for those targets here,
and do not edit the inert workflows under `upstream/.github/workflows/`
expecting them to run.

**Likely touchpoints:**

- `frontend/src-tauri/tauri.conf.json`
- The repository-root `.github/workflows/build-windows.yml`
- Artifact fetch/verification scripts from Sprint 1
- Windows smoke-test helper/command
- License/attribution resources

**Required implementation:**

- Fetch pinned model artifacts in the Windows build workflow through the
  approved verifier.
- Place resources at stable Tauri resource paths, chosen so a later
  macOS/Linux enablement is additive rather than a rework.
- Include model/tokenizer/license files in the installer/package.
- Preserve app signing behavior.
- Install package artifacts in CI or a platform runner where supported, then
  invoke a headless/safe diagnostic that loads both sessions and executes known
  reference inference.
- Verify a known local hybrid query over a tiny fixture.
- Verify model resource path handling after installation, not only in Cargo
  output.
- Simulate unavailable/corrupt resource in a dedicated test build/path and
  prove lexical fallback does not prevent startup.
- Record installer/package size and build-cache impact.
- Ensure CUDA/Vulkan/Metal Whisper features do not change ORT retrieval outputs.

**Acceptance criteria:**

- Windows x64 NSIS/MSI package passes installed tokenizer, embedding, reranker,
  and hybrid fixture inference.
- Artifact corruption/missing license fails before package creation.
- Runtime missing/corrupt model produces semantic-unavailable/FTS fallback,
  not application startup failure.
- Package contains exact manifest hashes and license attribution.
- Model resources are not duplicated unnecessarily in app data.
- Package-size report is recorded and approved.
- Release documentation states that hybrid retrieval is verified on Windows
  x64 only and makes no macOS or Linux claim.

**Required verification:**

Run the project platform workflows/commands defined by the task. At minimum:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::model::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
pnpm --dir "frontend" run typecheck
git diff --check
```

CI/package links, installed paths, hashes, and diagnostic output are mandatory
execution-log evidence.

**Worker report additions:** Provide per-platform artifact layout, installed
smoke commands/results, signing impact, package sizes, and fallback proof.

### 5.5 - Release qualification and program close [L]

**Outcome:** Hybrid RAG is demonstrably correct, recoverable, private,
cross-platform, and within approved scale/resource limits.

**Likely touchpoints:**

- Evaluation/benchmark/crash test harnesses
- Minimal production corrections required by approved gates
- All sprint decision/execution logs
- User-facing/release documentation if behavior needs disclosure

**Required qualification matrix:**

- 12k, 50k, and 250k document vector/index loads and queries.
- Fast and Deep quality/latency/resource metrics.
- Concurrent bounded search plus background indexing.
- Shared scheduler priority/queue/ORT-thread behavior and queued cancellation.
- Recording/transcription while index worker pauses/throttles.
- Crash during chunking, embedding, SQLite replacement, sidecar/cache publish,
  and model-generation activation.
- Restart/resume after each crash point.
- Meeting edit during embedding.
- Meeting deletion before/after vector publication.
- Deleted-meeting Chat source scrub while answer text remains.
- Folder move without re-embedding and authoritative scope filtering.
- Model/chunker upgrade shadow build and activation.
- Corrupt vector row/cache/sidecar/model resource.
- Initial and partial backfill lexical-only behavior.
- Every Chat scope in Fast/Deep, live direct path, sidebar, Tauri, and MCP.
- **Forced lexical-only retrieval across every surface, its persistence across
  restart, and clean restoration of hybrid behavior when disabled.**
- **Derived disk at 12k/50k/250k in steady state and during a shadow rebuild,
  measured against the 2 GiB / 3 GiB envelope.**
- **Reranking stage p95 measured separately against its 900 ms sub-budget, for
  both the Chat and Search purposes.**
- **Deep preparation p95, total provider round-trips per turn, and progress
  event delivery.**
- Sustained sidebar typing while a Chat stream is active, proving reranking
  cancellation and scheduler priority.
- Source parity and persisted old-source compatibility.
- Privacy-safe logs and no runtime embedding network traffic.
- Windows x64 installed package smoke evidence from Task 5.4.

**Acceptance criteria:**

- All `architecture.md` correctness, privacy, availability, scope, performance,
  packaging, evaluation, and reference-case gates pass.
- Peak retrieval RAM at 250k is at most 1 GiB on reference hardware, including
  the ANN graph when selected, or has the explicit required approval for a
  measured 1-1.25 GiB result; above 1.25 GiB fails without a product scope
  change.
- Derived disk at 250k is at most 2 GiB steady state and 3 GiB during shadow
  rebuild, or has explicit approval.
- Vector-stage, reranking-stage, Fast preparation, and Deep preparation p95 all
  meet their approved thresholds, each reported as its own figure.
- ANN recall, when selected, meets the Sprint 1 quality gate.
- The retrieval kill switch works on the installed package and is documented as
  the first-line rollback.
- Crashes never corrupt primary meeting content or activate partial semantic
  state.
- Deleted/out-of-scope meetings never appear from stale vectors.
- Every semantic failure has a verified lexical fallback.
- No raw query/content/embedding appears in logs.
- Audio/transcription scheduler qualification shows no new drop/overflow
  warning and no more than 10% p95 throughput degradation versus paused index.
- Full Rust/frontend suites, typecheck, formatting, diff, evaluation,
  benchmarks, and installed native smokes pass.
- Code review and architecture review are Approved with no unresolved blocker
  or should-fix finding.
- Deferred work and known ceilings are documented explicitly.

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

Run every approved scale/packaging/native command and attach summarized results
to the execution entry.

**Worker report additions:** Provide the complete gate matrix with pass/fail,
measured metrics, fixes made, omissions, residual risks, and rollback drill.

## Sprint Acceptance Criteria

- Sidebar, explicit Tauri hybrid commands, hybrid MCP tools, and Chat use the
  shared reviewed retrieval service.
- Existing lexical APIs/tools remain compatible.
- Users can inspect, pause, retry, safely rebuild, and force lexical-only
  retrieval from Settings.
- The Windows x64 package executes installed local inference.
- 250k scale, crash, update, deletion, upgrade, corruption, scope, privacy,
  fallback, disk, and source gates pass.
- Final evaluation passes every approved numeric quality/category gate, with
  corpus sizes reported alongside every percentage.
- Program documentation contains exact model/backend limits, the Windows-only
  platform scope, and operational recovery behavior including the kill switch.
- Code/architecture reviews and user sprint-close approval are complete.

## Risks And Mitigations

- **External consumer breakage:** additive commands/tools and explicit score
  fields.
- **Sidebar stale result race:** request cancellation/generation ownership.
- **Unsafe rebuild UI:** derived-only backend contract plus confirmation/tests.
- **Installer failure/size:** pinned CI artifacts, package reports, installed
  smoke.
- **Unverified platform claims:** Windows-only scope stated in release
  documentation; no macOS or Linux support asserted without installed-package
  inference on that target.
- **Scale memory spike:** measure active model sessions plus old/new snapshots
  plus the ANN graph, not vectors alone.
- **Derived disk growth:** measured against an explicit envelope at every
  scale, including the shadow-rebuild peak.
- **Sidebar reranking cost:** minimum query length, `Search` purpose depth,
  in-flight cancellation, and scheduler priority verified under sustained
  typing.
- **Recovery claim without proof:** crash injection and restart tests.
- **Privacy regression:** local network/log audit and no remote telemetry.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-21 | Preserve existing lexical Tauri/MCP tools and add hybrid contracts. | Concrete external consumers may depend on BM25 score semantics. | Silently change existing tools to hybrid. | Main agent, pending sprint approval |
| 2026-08-21 | Put index controls in existing Settings structure unless content proves a new tab necessary. | Minimize navigation/UI expansion. | Add a Search tab immediately. | Main agent, pending sprint approval |
| 2026-08-21 | Require installed-package inference, not package file inspection. | ORT resource/dylib failures appear only after installation. | Treat successful `tauri build` as proof. | Main agent, pending sprint approval |
| 2026-08-21 | Descope packaging to Windows x64 and drop Task 5.4 from L to M. | The macOS/Linux workflows in this fork are nested under `upstream/` and never execute; the original gate could not be satisfied. | Add root-level macOS/Linux workflows and keep the three-platform gate. | Main agent, **requires user approval — product scope change** |
| 2026-08-21 | Surface the force-lexical kill switch in Settings as a first-class control. | It is the user's own rollback from a bad retrieval result and is useless if undiscoverable. | Keep it as a hidden or developer-only setting. | Main agent, pending sprint approval |
| 2026-08-21 | Add derived-disk qualification at every scale, including the rebuild peak. | Derived text plus vectors plus two retained generations plausibly reach ~2 GiB with no prior ceiling anywhere in the program. | Report disk as an unanchored metric. | Main agent, pending sprint approval |
| 2026-08-21 | Guard sidebar reranking with a minimum query length, `Search` depth, and in-flight cancellation. | Sidebar runs the cross-encoder per debounced keystroke; an empty-query check alone does not bound that cost. | Rely on debounce and the empty-query guard. | Main agent, pending sprint approval |

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

**Required because:** External Tauri/MCP contracts, sidebar behavior, destructive
derived-state controls, signed cross-platform model packaging, 250k scale,
failure recovery, privacy, and final release claims.

**Reviewer:** Pending
**Verdict:** Pending
**Findings:** Pending

## Approval Gates

- Sprint 4 close must be approved first.
- User approval of this PRD is required before Sprint 5 TODO creation.
- Task 5.2 external contracts require a dedicated approved batch unless proven
  safe with another task.
- Task 5.5 is L and runs alone.
- Package-size, supported-platform, resource-limit, remote behavior, or lexical
  compatibility changes require explicit scope/risk approval.
- Adding macOS or Linux back to the release scope requires a root-level build
  workflow for that target, the Sprint 1 reference-inference gate executed on
  it, and the Task 5.4 installed smoke executed on it. It is a scope change,
  not a task-level decision.
- Binary rollback after the semantic migration requires a verified pre-upgrade
  database backup; do not test/claim old-binary startup against a newer migrated
  database unless migrator policy was separately approved.
- Final program close requires user approval after both reviews and the full
  release gate report.
