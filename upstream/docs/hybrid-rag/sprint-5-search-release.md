# Sprint 5: Search Surfaces And Release Hardening

## Status

In progress for Tasks 5.1-5.4 after the user's 2026-09-04 scope amendment.
Sprint 4 and Sprint 3 release acceptance remain mandatory gates for Task 5.5,
Sprint 5 close, and every release claim.

Task 5.4 was decomposed by user direction on 2026-09-04 into the sequential
Tasks 5.4a-5.4c. The split changes review and handoff boundaries, not the
approved Windows-only outcome or any release gate. Each subtask requires its
own implementation session, acceptance review, and execution-log entry.

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
retrieval, context, and Deep-mode contracts from Sprints 1-4. The reviewed
Sprint 3 implementation baseline is commits `62d7730` and `1047367`; it does
not close Sprint 3 release acceptance.

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

## Sprint 3 Release-Gate Inheritance (R40)

The user authorized Tasks 5.1-5.4 to proceed from the code-ready Sprint 4
baseline at `29df304` while Sprint 4 remains release-blocked. Task 5.5, Sprint
5 close, and release criteria additionally inherit Sprint 3's still-open gates:

- a valid independently authored Portuguese corpus;
- production-path quality and final provider-answer evidence;
- native Windows/R13 hermetic session evidence;
- exact-head GitHub Actions evidence.

V1-V10 and the currently rejected corpus fixtures/harnesses are not acceptance
evidence. Internal production testing without a corpus is diagnostic only. Task
5.5 and release close MUST NOT bypass these gates, and no later Fast/Deep result
may substitute for them.

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
- The single persisted `force_lexical_retrieval` setting is read at the shared
  Rust preparation/service boundary for every sidebar, Tauri, and MCP hybrid
  request, and for every initial/additional Deep retrieval; preserve typed
  `ForcedLexical` and do not add a second setting or diagnostics service.
- MCP timeout owns an internal deadline cancellation token passed through shared
  retrieval so queued/running scheduler and ONNX work terminates without a
  public MCP cancel API.

## Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---:|---|---|---|---|---|
| 5.1 | Sidebar search | Consume the approved Tauri hybrid contract for meeting-level sidebar search, relevance snippets/provenance, lexical fallback, cancellation, and stable snapshot IDs. | M | Pending `worker-m` | 5.2 | Frontend/Rust tests prove ranking, fallback, folder filters, request cancellation, dedupe, keyboard/a11y, and snapshot membership. | Switch sidebar invocation back to existing FTS command. |
| 5.2 | API and MCP | Add explicit cancellable Tauri and versioned bounded Fast-only MCP hybrid search/context contracts while preserving all lexical tools. | M | Pending `worker-m` | Sprint 4 approved contract; Task 4.1 shared ownership mechanism | Contract/execution tests prove surface classification, scope composition, provenance, source retention, shared cancellation/timeout bounds, compatibility, and no score ambiguity. | Remove additive commands/tools; existing lexical APIs remain. |
| 5.3 | Index UX | Add Settings status, progress, pause/resume, rebuild, force-lexical toggle, error/retry, model/license, and local-size UI. | M | Pending `worker-m` | 2.5, 3.4 | UI/backend tests prove controls, lexical-only state, kill switch, disk reporting, accessibility, and rebuild cannot delete primary data. | Remove additive UI/commands; background index continues or can be disabled. |
| 5.4a | Package authority | Verify and harden the pinned retrieval bundle staging and Tauri resource contract for Windows x64 without changing model or signing identity. | M | Pending distinct `worker-m` | 1.5, Sprint 2 | The exact manifest-managed model, tokenizer, and license set stages atomically; missing, corrupt, divergent, or extra content fails before packaging. | Revert Task 5.4a package-contract changes; ship lexical-only and make no hybrid package claim. |
| 5.4b | Packaged diagnostic | Add a safe installed-resource diagnostic that performs real tokenizer, embedding, reranker, and tiny hybrid-fixture inference and proves missing/corrupt resources degrade to lexical behavior. | L | Pending distinct `worker-l` | 5.4a accepted | The diagnostic resolves installed resources without a development override or network, returns typed bounded outcomes, and passes real inference and fallback tests. | Remove the additive diagnostic entry point; normal application and lexical retrieval remain unchanged. |
| 5.4c | Installed Windows smoke | Extend the active root Windows workflow to install MSI and NSIS artifacts, run the packaged diagnostic, preserve signing, and report package/cache size. | L | Pending distinct `worker-l` | 5.4b accepted | Both installed package formats pass the diagnostic from their installed layouts; workflow evidence records hashes, paths, sizes, and signing treatment. | Revert the additive workflow smoke/report steps; do not claim packaged hybrid support. |
| 5.5 | Release qualification | Run/fix scale, concurrency, crash, upgrade, deletion, corruption, privacy, evaluation, native, and rollback gates. | L | Pending `worker-l` | 5.1-5.3 and 5.4a-5.4c; inherited Sprint 3 release gates | All architecture release gates pass, including every inherited Sprint 3 gate, and final code/architecture reviews approve. | Disable semantic feature paths and retain FTS; restore pre-upgrade backup when release procedure requires. |

## Dependency Order

`Sprint 4 code-ready baseline at 29df304 + user scope amendment -> 5.2 -> 5.1 -> 5.5`

`2.5 -> 5.3 -> 5.5`

`1.5 + Sprint 2 -> 5.4a -> 5.4b -> 5.4c -> 5.5`

Tasks `5.1` and `5.3` may run in one approved batch only if their TypeScript
types, command registrations, and settings/search components are disjoint.
Task `5.2` owns all serialized Tauri/MCP hybrid contracts and runs before its
sidebar consumer. Task `5.5` is L and runs alone. The original Task `5.4`
dropped from L to M when packaging was descoped to Windows x64 only, then was
split into three review units at user direction. Task `5.4a` is M; the
cross-cutting installed-resource diagnostic and signed-installer workflow Tasks
`5.4b` and `5.4c` are L and run alone. All three run sequentially because they
share the package authority and each subsequent task consumes the prior task's
accepted contract.
The Sprint 4 dependency here means its approved implementation contract; it does
not convert Sprint 3's open release gates into an implementation or evidence
waiver for Task 5.5.

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
- Read the single persisted `force_lexical_retrieval` setting at the shared Rust
  preparation/service boundary; when enabled, use the existing lexical fallback
  and preserve the typed `ForcedLexical` reason rather than adding a sidebar
  switch or diagnostics service.
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
- Enabling force-lexical affects the next sidebar request, survives restart, and
  disabling it restores hybrid behavior; the same setting and typed reason are
  used by every hybrid surface.
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
- Reuse the one Rust request-ownership/cancellation mechanism established by
  Sprint 4 Task 4.1, keyed so Chat and sidebar requests may coexist; do not add
  a parallel request registry. Require a request ID for interactive Tauri
  hybrid search/context and provide cancellation/ownership so superseded
  sidebar work stops in Rust, with terminal/error/timeout cleanup.
- Add versioned or separately named MCP hybrid search/context tools.
- Keep existing lexical MCP tools and descriptions.
- Update hybrid tool descriptions to state local semantic+lexical behavior and
  score/provenance semantics.
- Keep MCP Chat and hybrid search/context Fast-only in this release.
- Read the single persisted `force_lexical_retrieval` setting at the shared Rust
  preparation/service boundary for every Tauri and MCP hybrid request. When
  enabled, use the existing lexical fallback and preserve the typed
  `ForcedLexical` reason; do not add a second setting or diagnostics service.
- Give MCP hybrid tools strict candidate/context/time bounds and a server-side
  timeout. The timeout owns an internal deadline cancellation token passed
  through shared retrieval so queued/running scheduler and ONNX work terminates;
  do not claim a public MCP cancellation API.
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
- MCP Chat still uses shared Fast Chat preparation, cannot request Deep, and has
  a behavioral regression through that shared-preparation path.
- Superseded Tauri request IDs cancel queued/running retrieval without stale
  result publication; stale/replaced/cancelled progress is also suppressed and
  terminal/error/timeout cleanup leaves the shared registry bounded.
- MCP timeout cancellation reaches queued/running retrieval and ONNX work
  rather than merely dropping publication.
- Enabling force-lexical affects the next Tauri/sidebar/MCP hybrid request,
  survives restart, disabling restores hybrid behavior, and Fast/Deep Chat plus
  all hybrid surfaces preserve the typed `ForcedLexical` reason.
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
semantics, scope composition rules, reuse of the Task 4.1 ownership mechanism,
request-ID/cancellation behavior, MCP deadline-token timeout/bounds, shared
preparation regression, forced-lexical round-trip evidence, and external
rollback path.

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
- Keep this as the one persisted setting: the shared Rust preparation/service
  boundary reads it for Fast/Deep Chat and every sidebar/Tauri/MCP hybrid
  request, including all initial/additional Deep retrieval. Preserve the typed
  `ForcedLexical` reason; do not add a second setting or diagnostics service.
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
- The enable-next-request, restart, and disable-restore checks cover Fast and
  Deep Chat plus sidebar, Tauri, and MCP hybrid requests and preserve the typed
  `ForcedLexical` reason.
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

### 5.4 - Windows model packaging and installed smoke [Decomposed]

**Parent user outcome:** A user installing the supported Windows x64 MSI or
NSIS package receives the complete, trusted local retrieval bundle. The
installed application can run tokenizer, embedding, reranker, and a tiny local
hybrid query without downloading model data. If packaged retrieval resources
are missing or corrupt, the application still starts and search/Chat remain
usable through the existing typed lexical fallback.

**User-visible boundaries:**

- No model download, network fallback, setup wizard, or new model choice is
  introduced. Retrieval assets are package-owned and available offline.
- The approved model identities, versions, licenses, and local-only behavior do
  not change.
- Missing or corrupt semantic resources produce the existing truthful
  unavailable/degraded state; they do not crash startup or imply readiness.
- Installer identity and signing behavior do not change.
- This work supports Windows x64 only. It makes no macOS ARM64 or Linux x64
  package claim.
- Package and cache sizes are build/release evidence, not user telemetry. No
  raw query, meeting text, tokens, embeddings, or local paths enter public logs.

**Existing technical authority:**

- `frontend/src-tauri/resources/retrieval/model-bundle.manifest.json` is the
  checked-in publication authority for bundle ID
  `meetily-retrieval-bundle-1`, `intfloat/multilingual-e5-base` embedding, and
  `cross-encoder/mmarco-mMiniLMv2-L12-H384-v1` reranking. Its pinned revisions,
  byte lengths, SHA-256 digests, tensor contracts, and license records must not
  be replaced or relaxed in this task.
- `frontend/src-tauri/scripts/stage-retrieval-models.ps1` is the approved fetch,
  cache, byte-length/SHA-256 verification, crash recovery, and atomic
  publication path. `resources/retrieval/bundle` is its sole package output;
  arbitrary or stale files are rejected.
- `frontend/src-tauri/tauri.conf.json` already packages
  `resources/retrieval/bundle`. Its Windows signing command is
  `scripts/sign-windows.ps1`; no subtask may disable, bypass, replace, or fake
  that command or its credentials.
- `frontend/src-tauri/src/retrieval/model.rs::bundle_dir` maps Tauri's installed
  resource directory to `resources/retrieval/bundle`.
  `RetrievalModels::get_or_load` parses the approved manifest, verifies all
  artifacts before ONNX loading, warms the bounded embedding session, and
  lazily loads the reranker.
- `frontend/src-tauri/src/main.rs` currently reserves first-argument packaged
  diagnostics, including `--smoke-dbstat`. A retrieval diagnostic must preserve
  that exact first-argument safety rule and use distinct documented exit codes.
- The only active platform workflow is repository-root
  `.github/workflows/build-windows.yml`. It already stages/verifies the bundle,
  runs source-tree reference inference, builds both package formats, installs
  both for `--smoke-dbstat`, preserves the existing signer, and uploads the
  installers. Nested workflows under `upstream/.github/workflows/` are inert
  for this fork and are out of scope.

**Parent success criteria:**

- Both installed Windows x64 package formats resolve the exact packaged
  manifest and managed files from their installed resource layout.
- Both installed packages execute real tokenizer, embedding, reranker, and
  tiny hybrid-fixture inference with finite, dimensionally correct,
  reference-compatible output and no network access.
- Missing/corrupt runtime resources fail closed before ONNX consumes them and
  preserve application startup plus typed lexical fallback.
- Missing license, missing artifact, digest/length mismatch, divergent manifest
  copy, and unmanifested package content all fail before package creation.
- The package contains one retrieval bundle and does not copy model resources
  into mutable app data.
- MSI/NSIS byte sizes, retrieval bundle bytes, model-cache effect, manifest
  digest, installed resource path shape, and diagnostic outcomes are recorded.
- Signing configuration and installer identity are unchanged unless the user
  separately approves a signing change.
- Installed-package success is based on workflow/native-run evidence, never a
  successful `cargo test`, source-tree inference, or archive listing alone.

#### 5.4a - Pinned bundle staging and package authority [M]

**Subagent user outcome:** Every Windows installer build receives one complete,
license-attributed, checksum-verified retrieval bundle, and a bad package input
is rejected before an installer can be produced.

**Implementation boundary:**

- Own only pre-package artifact authority, staging, verification, and Tauri
  resource inclusion. Do not add the installed inference diagnostic or modify
  MSI/NSIS install-smoke steps; those belong to 5.4b and 5.4c.
- Reuse `stage-retrieval-models.ps1`; do not create another downloader,
  manifest, model cache, or package directory.
- Confirm the script's manifest traversal covers embedding model, embedding
  tokenizer, reranker model, reranker tokenizer, and both managed license
  entries. Preserve atomic same-volume publication and recovery behavior.
- Keep `resources/retrieval/bundle` as the only packaged retrieval root and the
  checked-in manifest outside that root as build authority only.
- Never commit downloaded ONNX/tokenizer artifacts if repository policy keeps
  them staged/cache-backed. Do not introduce runtime downloads.
- Preserve the exact approved bundle/model identities and all manifest hashes.
  A model, quantization, revision, license, checksum, or artifact-path change is
  a separate user-approved architecture decision.
- Inspect the root Windows workflow's existing stage call, but edit the workflow
  in this subtask only if a pre-build package-integrity gate cannot otherwise be
  enforced. Leave installed smoke orchestration to 5.4c.
- Preserve `tauri.conf.json` identifier, targets, icons, external binaries, and
  Windows `signCommand` exactly.

**Success criteria:**

- A clean staged bundle contains exactly the manifest-managed artifacts, its
  byte-identical manifest copy, and the already pinned allowed placeholder.
- Missing model/tokenizer/license, one-byte corruption, wrong byte length,
  unsafe/duplicate path, divergent manifest, unexpected file, ambiguous crash
  backup, and tampered placeholder each fail closed.
- A valid sole crash backup is fully reverified before restoration.
- Tauri includes the staged bundle once at the stable runtime path, with no
  duplicate model tree in app data or another package resource root.
- The staging/build path reaches no unpinned URL and verifies cache hits exactly
  like fresh downloads.
- Source diff proves signing command and package identity are unchanged.

**Required verification:**

```powershell
./frontend/src-tauri/scripts/stage-retrieval-models.ps1 -SelfTest
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib model_bundle
$env:MEETLY_RAG_VERIFY_STAGED_BUNDLE = "1"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib staged_production_bundle -- --nocapture
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
git diff --check
```

The staged-production check may fetch/cache the approved artifacts through the
existing script. If network or cache access is unavailable, report that exact
limitation; the offline self-test does not substitute for real bundle evidence.

**Rollback:** Revert only 5.4a package-contract changes, remove staged resources
from the build output, retain lexical behavior, and make no packaged hybrid
claim. Never weaken integrity checks to make packaging succeed.

**Subagent handoff report:** List every changed file, exact managed artifact
paths, bundle/manifest identity and digest, staging/cache behavior, negative
integrity tests, signing/config diff result, commands and outputs, omissions,
and blockers. Append an immutable `5.4a` execution entry; do not mark parent
5.4 complete.

#### 5.4b - Installed retrieval diagnostic and fallback proof [L]

**Subagent user outcome:** Maintainers can ask the installed executable to prove
that its own packaged retrieval resources work offline, while a normal user can
still open and use the app lexically when those resources are unavailable.

**Implementation boundary:**

- Own the additive safe diagnostic entry point, installed resource resolution,
  real inference fixture, typed exit/result contract, and runtime missing/
  corrupt fallback tests. Do not edit installer orchestration in the root
  workflow; 5.4c consumes this accepted diagnostic.
- Extend the first-argument dispatch pattern in
  `frontend/src-tauri/src/main.rs`; never match the flag elsewhere in argv.
- Resolve the production smoke bundle through the same installed Tauri resource
  path contract as normal startup. The passing installed diagnostic must not use
  `MEETLY_RAG_BUNDLE_DIR`, `CARGO_MANIFEST_DIR`, the source tree, a developer
  cache, or an app-data copy.
- Reuse `model_bundle` validation and `RetrievalModels`; do not implement a
  second manifest parser, tokenizer, ONNX session loader, or ranking service.
- Execute known bounded tokenizer, embedding, and reranker reference cases,
  then a tiny local hybrid query through the production retrieval/ranking path.
  Fixture content must be synthetic, local, deterministic, and unrelated to the
  excluded evaluation corpora.
- Prove no network retrieval is attempted. Do not add a download fallback or
  rely on network blocking as the implementation.
- Use privacy-safe output: stage name, typed status, dimensions/counts, finite
  verdicts, bundle/manifest digest, and exit code only. Never print raw fixture
  text, token IDs, vectors, absolute user paths, queries, or model internals.
- Give each failure stage a stable distinct non-zero exit code so CI can
  distinguish harness/resource/tokenizer/embedding/reranker/hybrid failures.
- Missing or corrupt model resources in normal startup remain typed semantic
  unavailable/lexical fallback. A diagnostic may fail non-zero, but must not
  turn normal application startup into a hard model dependency.

**Success criteria:**

- A real complete package-layout fixture passes manifest verification,
  tokenizer reference checks, embedding inference, reranker inference, and one
  tiny hybrid query with retained source/provenance invariants.
- The diagnostic loads both approved ONNX sessions from the package resource
  tree and verifies finite, expected-dimensional output.
- Missing manifest, missing artifact/license, length/hash corruption, tokenizer
  failure, embedding failure, reranker failure, and hybrid failure map to
  distinct bounded outcomes without panic or startup side effects.
- Normal application construction with missing/corrupt semantic resources
  reaches the existing typed lexical fallback and does not copy or download a
  replacement.
- Tests prove the installed-success path rejects source/development overrides
  and performs zero network calls by construction.
- Whisper CUDA/Vulkan feature selection does not alter retrieval ORT inputs or
  reference outputs; Metal remains out of scope on Windows.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::model::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib model_bundle
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
pnpm --dir "frontend" run typecheck
git diff --check
```

Run the diagnostic against a real package-layout directory when available and
record whether that is a source-side package-layout test or an actually
installed executable. Only 5.4c can satisfy the MSI/NSIS installed criterion.

**Rollback:** Remove the additive retrieval diagnostic and its tests. Do not
change normal startup, resource verification, or lexical fallback behavior.

**Subagent handoff report:** Provide the exact flag, first-argument rule, exit
code table, resource-root derivation, production functions reused, fixture and
network-isolation design, real inference output summary, fallback proof,
changed files, verification, omissions, and blockers. Append an immutable
`5.4b` execution entry; do not claim installed MSI/NSIS success.

#### 5.4c - MSI/NSIS installed-artifact CI smoke [L]

**Subagent user outcome:** Every supported Windows package is installed and
tested as users receive it, so a resource-layout, signing, or ONNX packaging
failure blocks publication rather than appearing after installation.

**Implementation boundary:**

- Own repository-root `.github/workflows/build-windows.yml` installed smoke,
  package/cache size reporting, artifact evidence, and final package gate. Do
  not redesign the stager or diagnostic accepted in 5.4a/5.4b.
- Extend the existing MSI and NSIS installation/teardown pattern. Continue to
  run `--smoke-dbstat`; add the accepted 5.4b diagnostic against the executable
  discovered under each actual install root.
- Keep bounded process timeouts, preserve each diagnostic's exit code, sanitize
  public failure output, uninstall both packages, and fail the final gate only
  for smokes that actually ran. Earlier build failures must not be relabeled as
  smoke failures.
- Run after artifact staging, source reference inference, and Tauri package
  build, and before installer upload/publication.
- Preserve workflow triggers, Windows x64 CPU target, package targets, artifact
  names, application identifier, and signing behavior. Never add dummy
  certificates, bypass flags, unsigned fallback publication, or secret output.
- Record MSI bytes, NSIS bytes, staged retrieval bundle bytes, model-cache
  bytes/hit state, and build-output/cache impact using native filesystem
  measurements. These are evidence only; do not introduce telemetry.
- Produce a concise step-summary record containing commit SHA, package kind,
  installed relative resource path shape, manifest digest, diagnostic status,
  and size figures. Do not print absolute runner paths or fixture content.

**Success criteria:**

- MSI is silently installed into an isolated root, its installed executable
  runs both dbstat and retrieval diagnostics within timeout, then uninstall and
  residue checks pass.
- NSIS is silently installed into an isolated root, its installed executable
  runs both diagnostics within timeout, then uninstall and residue checks pass.
- Each retrieval diagnostic resolves only that package's installed resource
  tree and passes tokenizer, embedding, reranker, and hybrid fixture inference
  without network access.
- A diagnostic-specific failure remains distinguishable from installer,
  executable-discovery, timeout, and teardown failures.
- Missing/corrupt package inputs fail before package creation; installed
  resource failures fail the smoke and remain normal-runtime lexical fallback.
- MSI/NSIS sizes, staged bundle size, cache impact, manifest digest, exact
  commit SHA, workflow run URL, and diagnostic summaries are recorded.
- The workflow diff preserves the current signing command and does not expose
  secrets. If signing credentials are unavailable on a runner, report the
  package/signing evidence limitation rather than bypassing signing.
- A successful intermediate workflow run is Task 5.4c evidence only. It does
  not close the inherited exact-head release gate unless it is also the final
  reviewed release head used by Task 5.5.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
pnpm --dir "frontend" run typecheck
git diff --check
```

The acceptance check also requires a real run of repository-root
`.github/workflows/build-windows.yml` for the exact reviewed 5.4c commit, with
both installed package diagnostics and teardown passing. A local package build,
workflow syntax review, source-tree test, or diagnostic against `target/` does
not substitute for this evidence.

**Rollback:** Revert only the additive retrieval smoke, size-report, and gate
steps. Preserve the existing dbstat smoke and installer upload behavior. Make
no packaged hybrid claim until the installed test is restored.

**Subagent handoff report:** Provide workflow run/commit link, MSI/NSIS
installer and installed relative paths, sanitized exit-code outcomes, timeout
and teardown results, package/bundle/cache byte counts, manifest digest,
signing treatment, changed files, local checks, omissions, and blockers. Append
an immutable `5.4c` execution entry and state whether parent 5.4 is accepted or
still waiting on native evidence.

**5.4 review gates:** Review 5.4a before dispatching 5.4b, and review 5.4b before
dispatching 5.4c. Task 5.4c requires code review plus architecture/release-rigor
review because it changes signed-package evidence and the active root workflow.
No subtask may mark Task 5.5, Sprint 5, or the release complete.

**Common subagent constraints:**

- Start from pushed branch `sprint-2/durable-local-index` at or after Task 5.3
  commit `baf9b47`; report the exact input and output commit IDs.
- The Git repository root is above the application subtree: the active workflow
  is `D:\Personal Meetly\.github\workflows\build-windows.yml`, while application
  paths in this PRD are relative to `D:\Personal Meetly\upstream`.
- Use one new, distinct `worker-m` session for 5.4a and one new, distinct
  `worker-l` session for each of 5.4b and 5.4c. Do not let one worker absorb a
  later subtask, stage, commit, push, or make a release claim.
- Preserve unrelated dirty work. Never inspect, edit, stage, execute, or cite as
  evidence `retrieval_evaluation*`, `evaluation_policy*`, rejected/validation
  corpus fixtures or reports (including V1-V10 and independent variants),
  `frontend/src-tauri/tests/debug_mt.rs`, or `.opencode/`.
- Whole-crate formatting may remain blocked only by already documented excluded
  fixture whitespace. Run touched-file `rustfmt --edition 2021 --check` and
  `git diff --check`; never format or stage excluded files to make a global gate
  pass.
- Use privacy-safe logs and test output. Artifact-relative paths and approved
  public model identities are allowed; absolute user/runner paths, secrets, raw
  content, tokens, vectors, and queries are not.
- A local or corpus-free test can prove package mechanics but cannot become
  quality/release evidence. Keep the independent corpus, production answer,
  native Windows/R13, and exact-final-head Actions gates open for Task 5.5.

### 5.5 - Release qualification and program close [L]

**Outcome:** Hybrid RAG is demonstrably correct, recoverable, private, on the
supported Windows x64 package, and within approved scale/resource limits.

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
- Model/chunker upgrade shadow build and activation, only when this release
  introduces a second approved bundle/chunker identity. Otherwise record the
  prior-model retention row as not applicable; never fabricate a prior bundle.
- Corrupt vector row/cache/sidecar/model resource.
- Initial and partial backfill lexical-only behavior.
- Every Chat scope in Fast/Deep, live direct path, sidebar, Tauri, and MCP.
- **Forced lexical-only retrieval across every surface, its persistence across
  restart, and clean restoration of hybrid behavior when disabled.** The shared
  Rust boundary reads the one persisted setting for every initial/additional
  Deep retrieval and sidebar/Tauri/MCP hybrid request and preserves typed
  `ForcedLexical`; no second setting or diagnostics service is permitted.
- One shared Rust ownership/cancellation mechanism reused from Task 4.1,
  including stale/replaced/cancelled progress, terminal/error/timeout cleanup,
  and bounded registry lifetime.
- MCP server timeout cancellation of queued/running shared retrieval and ONNX
  work through its internal deadline token, without a public MCP cancel claim.
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
- Windows x64 installed package smoke evidence from accepted Tasks 5.4a-5.4c.

**Acceptance criteria:**

- All `architecture.md` correctness, privacy, availability, scope, performance,
  packaging, evaluation, and reference-case gates pass.
- Task 5.5 and release close also require valid evidence for the independently
  authored Portuguese corpus, production-path quality and final provider-answer
  evidence, native Windows/R13 hermetic session evidence, and exact-head GitHub
  Actions evidence. V1-V10 and currently rejected corpus fixtures/harnesses are
  not acceptance evidence; corpus-free internal production testing is diagnostic
  only. No Fast/Deep result may bypass these gates.
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
- If this release introduces a second approved bundle/chunker identity, the
  prior-bundle package, identity derivation, combined RAM envelope, fallback,
  and upgrade tests pass. Otherwise the upgrade-retention qualification is
  recorded not applicable by architecture authority.
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
The execution entry MUST report each inherited Sprint 3 gate separately; a
task-local evaluation, native check, or Fast/Deep result cannot substitute for a
missing gate.

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
- **Parallel cancellation state or unfenced progress:** reuse Task 4.1's single
  Rust ownership/cancellation mechanism and Chat publication fence, with bounded
  cleanup/lifetime tests.
- **MCP timeout only drops results:** pass the server-owned internal deadline
  token through shared retrieval so queued and ONNX work is cancelled.
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
- **Forced-lexical drift:** one shared-boundary persisted setting, typed
  `ForcedLexical`, and enable-next-request/restart/disable-restore coverage on
  Fast/Deep and every hybrid surface.
- **Release-gate laundering:** Task 5.5 and release close inherit the four
  named Sprint 3 gates; rejected fixtures and corpus-free diagnostics cannot
  become evidence.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-21 | Preserve existing lexical Tauri/MCP tools and add hybrid contracts. | Concrete external consumers may depend on BM25 score semantics. | Silently change existing tools to hybrid. | Main agent, pending sprint approval |
| 2026-08-21 | Put index controls in existing Settings structure unless content proves a new tab necessary. | Minimize navigation/UI expansion. | Add a Search tab immediately. | Main agent, pending sprint approval |
| 2026-08-21 | Require installed-package inference, not package file inspection. | ORT resource/dylib failures appear only after installation. | Treat successful `tauri build` as proof. | Main agent, pending sprint approval |
| 2026-08-21 | Descope packaging to Windows x64 and drop Task 5.4 from L to M. | The macOS/Linux workflows in this fork are nested under `upstream/` and never execute; the original gate could not be satisfied. | Add root-level macOS/Linux workflows and keep the three-platform gate. | User |
| 2026-08-21 | Surface the force-lexical kill switch in Settings as a first-class control. | It is the user's own rollback from a bad retrieval result and is useless if undiscoverable. | Keep it as a hidden or developer-only setting. | Main agent, pending sprint approval |
| 2026-08-21 | Add derived-disk qualification at every scale, including the rebuild peak. | Derived text plus vectors plus two retained generations plausibly reach ~2 GiB with no prior ceiling anywhere in the program. | Report disk as an unanchored metric. | Main agent, pending sprint approval |
| 2026-08-21 | Guard sidebar reranking with a minimum query length, `Search` depth, and in-flight cancellation. | Sidebar runs the cross-encoder per debounced keystroke; an empty-query check alone does not bound that cost. | Rely on debounce and the empty-query guard. | Main agent, pending sprint approval |
| 2026-09-04 | Set the approved sidebar inference minimum to one non-empty Unicode character. | Preserve exact title matching for short names while avoiding model inference for empty input. | Require two or more characters; rely only on debounce. | User |
| 2026-09-05 | Raise the sidebar inference minimum from one character to three (`SIDEBAR_SEARCH_MIN_QUERY_LENGTH` / `SEARCH_MIN_MODEL_QUERY_CHARS`), and keep the 2026-09-04 rationale satisfied by matching titles locally by substring in every retrieval state rather than by query length. | At one character the guard is the empty-query check under another name, so Task 5.1's "minimum query length" mitigation bounded nothing; the original rationale was short-name title matching, which the client-side title union now preserves at any length, including lengths below the minimum. | Keep the approved minimum at one and accept unbounded cross-encoder inference per debounced keystroke; rely on debounce alone. | **Pending user approval** - supersedes the 2026-09-04 row above, which the user approved. |
| 2026-09-02 | Carry Sprint 3's open release gates into Task 5.5 and release close while retaining commits `62d7730` and `1047367` as the reviewed implementation baseline. | R40 separates implementation dependencies from release acceptance; valid corpus, production-path quality/provider-answer, native Windows/R13 hermetic session, and exact-head Actions evidence remain mandatory. | Treat Sprint 4/5 implementation results or broad architecture wording as release evidence. | User-authorized R40 |
| 2026-09-02 | Reuse one Rust ownership/cancellation mechanism and Chat publication fence for sidebar/Tauri/MCP work, including internal MCP deadline cancellation. | Prevents parallel registries, stale progress, and timeouts that merely drop results while preserving Fast-only MCP compatibility. | Add another request registry or public MCP cancel API. | User-authorized R40 |
| 2026-09-02 | Carry the single persisted `force_lexical_retrieval` decision through all Deep rounds and sidebar/Tauri/MCP hybrid requests. | Shared-boundary reads, typed `ForcedLexical`, and next-request/restart/disable-restore checks keep rollback consistent without a second service. | Per-surface settings or diagnostics. | User-authorized R40 |
| 2026-09-04 | Permit Tasks 5.1-5.4 to proceed from code-ready Sprint 4 baseline `29df304` while retaining every Sprint 4/Sprint 3 release gate for Task 5.5, Sprint 5 close, and release claims. | The user explicitly authorized implementation to continue; separating code readiness from release acceptance preserves the inherited evidence gates. | Require Sprint 4 release closure before all Sprint 5 implementation. | User |
| 2026-09-04 | Decompose Task 5.4 into sequential Tasks 5.4a package authority, 5.4b packaged diagnostic, and 5.4c installed MSI/NSIS CI smoke. | Artifact trust, runtime inference/fallback, and signed installer evidence have distinct failure modes and acceptance evidence. Separate handoffs prevent source-only checks from being mistaken for installed-package proof and isolate signing-sensitive workflow changes. | Keep one broad Task 5.4 implementation session; split only the CI step. | User |
| 2026-09-04 | Reclassify Tasks 5.4b and 5.4c as L and assign each directly to a distinct `worker-l` session after its dependency is accepted. | The installed-resource diagnostic and signed-installer workflow are cross-cutting native/package evidence changes and require higher-risk implementation/review ownership. A worker owns one task; it does not delegate nested worker sessions. | Retain M `worker-m` ownership; use one worker-l as a delegating manager. | User |

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

**Reviewer:** `anthropic/claude-opus-5` (Claude Code, `/code-review xhigh`), three rounds
**Verdict:** Implementation findings resolved; sprint close still blocked by the
release-qualification gates below, which this review did not and cannot clear.

**Findings:**

- **R5.R1** (full Sprint 5 range `29df304..2f767a2`): 15 findings - 3 blockers
  (a single terminal per-meeting failure ended the whole shadow generation; the
  approved sidebar minimum query length was set to 1, making it a no-op; a
  hybrid command error left the sidebar with no lexical fallback at all), 10
  should-fix, 2 cleanup. All 15 fixed in Task HR-5.R1.
- **R5.R2** (review of the HR-5.R1 remediation diff): 5 findings, all in the
  remediation itself - 1 blocker (the new title gate compared a deduplicated
  overlap against a raw term count, so any repeated query token disabled the
  title channel), 3 should-fix, 1 conventions. All 5 fixed in Task HR-5.R2.
- **R5.R3** (full Sprint 5 range again, after HR-5.R2): 15 findings - 2
  blockers (`matchMode` serialized as `null`, which the sidebar's own response
  validator rejects, so every hybrid response containing a semantic or title
  provenance entry - that is, every hybrid response - fell back to "Search
  unavailable"; and the Search title channel full-scanning the `meetings`
  table on every debounced keystroke, ~977 sequential queries at the 250k
  gate), 9 should-fix, 4 cleanup/altitude/conventions. All 15 fixed in Task
  HR-5.R3, with both blockers proven by negative control before the fix.

Both rounds, their per-finding corrections, verification output, and the
environment/flake caveats are recorded in
[`notes-chat-improvement-execution.md`](../notes-chat-improvement-execution.md)
under `R5.R1`, `HR-5.R1`, `R5.R2`, `HR-5.R2`, `R5.R3`, and `HR-5.R3`.

**Verification after remediation:** `cargo check` pass; `cargo test --lib` 889
passed / 0 failed / 2 ignored; `cargo fmt --check` pass; `pnpm run typecheck`
pass; `pnpm exec vitest run` 164 passed / 23 files; `git diff --check` pass.

**Open item from R5.R3:** the 2026-09-05 Decisions row raising the sidebar
inference minimum from one character to three supersedes a user-approved row
and is marked pending user approval.

**Required follow-ups:** Task 5.4 packaging, Task 5.5 release qualification, and
sprint close remain blocked by their own unchanged evidence gates (independently
authored corpus, production-path quality and final provider-answer evidence,
native Windows hermetic-session evidence, exact-head GitHub Actions evidence,
and the installed-package smoke). No release claim follows from this review.

### Architecture Review

**Required because:** External Tauri/MCP contracts, sidebar behavior, destructive
derived-state controls, signed Windows x64 model packaging, 250k scale,
failure recovery, privacy, and final release claims.

**Reviewer:** Pending
**Verdict:** Pending
**Findings:** Pending

## Approval Gates

- The user approved a scope amendment on 2026-09-04 allowing Tasks 5.1-5.4 to
  proceed from code-ready Sprint 4 baseline `29df304`. This is not Sprint 4
  close approval and does not waive any inherited release gate.
- Sprint 3 release acceptance remains open and is mandatory for Task 5.5 and
  release close: valid independently authored Portuguese corpus, production-
  path quality and final provider-answer evidence, native Windows/R13 hermetic
  session evidence, and exact-head GitHub Actions evidence.
- V1-V10 and currently rejected corpus fixtures/harnesses are not acceptance
  evidence; corpus-free internal production testing is diagnostic only. Task
  5.5 and release close MUST NOT bypass the inherited gates or substitute a
  later Fast/Deep result for them.
- User approval of this PRD is required before Sprint 5 TODO creation.
- Task 5.2 external contracts require a dedicated approved batch unless proven
  safe with another task.
- Task 5.5 is L and runs alone.
- Tasks 5.4a, 5.4b, and 5.4c require separate sequential batch approvals and
  distinct worker sessions. A subtask is dependency-ready only after the prior
  subtask's checks, execution entry, and review are accepted.
- Tasks 5.4b and 5.4c are L and run alone under their directly assigned
  `worker-l` sessions; 5.4a remains M under `worker-m`.
- Package-size, supported-platform, resource-limit, remote behavior, or lexical
  compatibility changes require explicit scope/risk approval.
- Adding macOS or Linux back to the release scope requires a root-level build
  workflow for that target, the Sprint 1 reference-inference gate executed on
  it, and the Tasks 5.4a-5.4c installed smoke executed on it. It is a scope
  change,
  not a task-level decision.
- Binary rollback after the semantic migration requires a verified pre-upgrade
  database backup; do not test/claim old-binary startup against a newer migrated
  database unless migrator policy was separately approved.
- Final program close requires user approval after both reviews and the full
  release gate report.
