# Sprint 5: Search Surfaces And Release Hardening

## Status

In progress for Tasks 5.1-5.4 after the user's 2026-09-04 scope amendment.
Sprint 4 and Sprint 3 release acceptance remain mandatory gates for Task 5.5,
Sprint 5 close, and every release claim.

Task 5.4 was decomposed by user direction on 2026-09-04 into the sequential
Tasks 5.4a-5.4c. The split changes review and handoff boundaries, not the
approved Windows-only outcome or any release gate. Each subtask requires its
own implementation session, acceptance review, and execution-log entry.

Verified implementation state on 2026-09-09 in `fix/sprint-5-review-r5`:
Windows CI7 and CI8 each ended after about 91–92 minutes with hosted-runner
communication loss during MSI installed smoke. Neither interruption establishes
an installer, retrieval, inference, teardown, or package-layout verdict. R10
then isolated each installed smoke onto a fresh downstream runner after the
build. Exact-head CI9 at `bc1dd943652b5c358412baa03158733878072a4c` passed
the package producer, fresh MSI and NSIS installed smokes, and the terminal
fail-closed evidence gate. Task 5.4c is accepted as Windows installed-package
evidence only; it does not close Sprint 5 or any release gate.

CI10 at exact HR-5.R10 source head `512daf9e158ea8207cf2c37a009cb61f9c32c305`
also passed Cargo Check, package production, both fresh installed smokes, and
the terminal evidence gate. This accepts title-top-k and Windows/package
integration at that commit only; it does not close Task 5.5, Sprint 5, signing
proof, or any release gate.

| Area | Current state | Remaining gate |
|---|---|---|
| 5.1-5.3 search/API/index UI | Implemented in `c8504a7`, `5bf5ced`, and `baf9b47`; subsequent cross-cutting R5 remediations are present. CI10 passed at the exact R10 source head. | Final Task 5.5/release qualification and its final exact-head CI. |
| HR-5.R10 correctness | Activation, snapshot/hydration, folder and public-ID fixes approved. The user-approved exact SQL title top-k is implemented, independently reviewed, and CI10-integrated at its exact source head: each scope/query partition orders by zero-scope-weight BM25 and stable string meeting ID before `LIMIT`; targeted retrieval regressions and `cargo check --lib` pass. | Final Task 5.5/release gates; title matching-set work remains explicitly linear and is not a candidate-limit work bound. |
| 5.4a package authority | Accepted after real Tauri expansion, staging/recovery tests, and independent review. | Installed-package evidence belongs to 5.4c. |
| 5.4b retrieval diagnostic | Accepted after pinned Rust 1.88 tests, real source-side package-layout inference, fallback tests, and independent review. | Source-layout inference is not MSI/NSIS installation evidence. |
| 5.4c installer CI | Accepted. CI9 at exact R10 head `bc1dd943652b5c358412baa03158733878072a4c` passed package production, fresh MSI and NSIS installed smokes, and terminal evidence gate. Both evidence records are schema 2/current-commit, use the approved 12-file / 430,993,263-byte bundle and manifest `8a375106…264ff4`, and pass isolated discovery, ownership, dbstat, real retrieval, teardown, residue, and signing policy. This records policy satisfaction, not a positive signature claim. | 5.4c is complete; Task 5.5 and all inherited release gates remain open. |
| 5.5 release qualification | Full release qualification not started; a limited non-corpus local baseline is recorded. Dependencies and inherited evidence remain open. | Valid independently authored Portuguese corpus, production-path quality and final provider-answer evidence, native Windows/R13 full loaded-application session, full qualification matrix, final reviewed-head Actions, final reviews, and user close approval. |

The latest explicit Rust 1.88 integration run passed 920 library tests (four
ignored), 18 model tests, 22 staged-bundle tests, cargo check, frontend
typecheck, and scoped formatting. The frontend suite passed 168 tests in 23
files. These checks provide local regression evidence, not release acceptance;
see the immutable execution entries for test and evidence limitations.

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

## Pre-implementation Baseline And Evidence

The touchpoints below describe the planning baseline, not a current status
report. Use the verified status above and the latest immutable execution and
review entries for the implemented state.

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

## Task Definitions And Planned Ownership

The original worker assignments below describe implementation boundaries;
they are not a completion tracker. Current acceptance is recorded above.

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
| 2026-09-05 | Raise the sidebar inference minimum from one character to three (`SIDEBAR_SEARCH_MIN_QUERY_LENGTH` / `SEARCH_MIN_MODEL_QUERY_CHARS`), and keep the 2026-09-04 rationale satisfied by matching titles locally by substring in every retrieval state rather than by query length. | At one character the guard is the empty-query check under another name, so Task 5.1's "minimum query length" mitigation bounded nothing; the original rationale was short-name title matching, which the client-side title union now preserves at any length, including lengths below the minimum. | Keep the approved minimum at one and accept unbounded cross-encoder inference per debounced keystroke; rely on debounce alone. | OpenCode under the user's delegated decision authority through 07:00 UTC-03, per the 2026-09-08 row below; supersedes the 2026-09-04 row above, which the user approved. |
| 2026-09-02 | Carry Sprint 3's open release gates into Task 5.5 and release close while retaining commits `62d7730` and `1047367` as the reviewed implementation baseline. | R40 separates implementation dependencies from release acceptance; valid corpus, production-path quality/provider-answer, native Windows/R13 hermetic session, and exact-head Actions evidence remain mandatory. | Treat Sprint 4/5 implementation results or broad architecture wording as release evidence. | User-authorized R40 |
| 2026-09-02 | Reuse one Rust ownership/cancellation mechanism and Chat publication fence for sidebar/Tauri/MCP work, including internal MCP deadline cancellation. | Prevents parallel registries, stale progress, and timeouts that merely drop results while preserving Fast-only MCP compatibility. | Add another request registry or public MCP cancel API. | User-authorized R40 |
| 2026-09-02 | Carry the single persisted `force_lexical_retrieval` decision through all Deep rounds and sidebar/Tauri/MCP hybrid requests. | Shared-boundary reads, typed `ForcedLexical`, and next-request/restart/disable-restore checks keep rollback consistent without a second service. | Per-surface settings or diagnostics. | User-authorized R40 |
| 2026-09-04 | Permit Tasks 5.1-5.4 to proceed from code-ready Sprint 4 baseline `29df304` while retaining every Sprint 4/Sprint 3 release gate for Task 5.5, Sprint 5 close, and release claims. | The user explicitly authorized implementation to continue; separating code readiness from release acceptance preserves the inherited evidence gates. | Require Sprint 4 release closure before all Sprint 5 implementation. | User |
| 2026-09-04 | Decompose Task 5.4 into sequential Tasks 5.4a package authority, 5.4b packaged diagnostic, and 5.4c installed MSI/NSIS CI smoke. | Artifact trust, runtime inference/fallback, and signed installer evidence have distinct failure modes and acceptance evidence. Separate handoffs prevent source-only checks from being mistaken for installed-package proof and isolate signing-sensitive workflow changes. | Keep one broad Task 5.4 implementation session; split only the CI step. | User |
| 2026-09-04 | Reclassify Tasks 5.4b and 5.4c as L and assign each directly to a distinct `worker-l` session after its dependency is accepted. | The installed-resource diagnostic and signed-installer workflow are cross-cutting native/package evidence changes and require higher-risk implementation/review ownership. A worker owns one task; it does not delegate nested worker sessions. | Retain M `worker-m` ownership; use one worker-l as a delegating manager. | User |
| 2026-09-08 | Approve a three-character sidebar model-inference minimum (`SIDEBAR_SEARCH_MIN_QUERY_LENGTH` / `SEARCH_MIN_MODEL_QUERY_CHARS`). | A one-character minimum did not reduce per-keystroke cross-encoder work; exact title matching remains available without model inference for shorter non-empty queries, while the three-character guard bounds interactive ONNX cost. | Restore a one-character model-inference minimum; rely only on debounce. | OpenCode under the user's delegated decision authority through 07:00 UTC-03 |
| 2026-09-08 | Add an additive `retrieval_title_fts` mirror for authoritative bounded hybrid title lookup. | The current ID-ordered title scan is incomplete above its arbitrary row limit and is unsuitable for MCP/Context. A separately maintained FTS5 title mirror supports normalized all-core-term lookup without changing the existing public `meeting_fts` lexical commands, semantic document set, vector inputs, or title provenance semantics. | Keep a capped scan with an incomplete-result flag; add title rows to existing `meeting_fts`; add title vectors. | OpenCode under the user's delegated decision authority through 07:00 UTC-03 |
| 2026-09-08 | Adopt exact SQL score-and-ID title top-k and document its linear matching-set work, amending the strict candidate-limit database-work requirement for this channel. | The synthetic 250k-title diagnostic measured about 436 ms versus 5,960 ms for snapshot-paged scans with identical exact top-k. Output/retained memory remain bounded, but every match is scored; preserve scope/snapshot/hydration/cancellation and all other release gates. | Retain the strict work requirement and leave title search pending a new index design. | User |

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

### Task HR-5.R5 - R5 independent-review remediation: indexed title mirror, lag fail-closed, single-transaction terminalization, three-character authorization

**Status:** Complete (implementation and mandatory verification; pending independent review; no release claim and no gate re-opened)
**Owner:** `worker-m` (HR-5.R5)
**Completed:** 2026-09-08
**Implemented:**
- Authoritative indexed title lookup: the Search/Chat/Context title channel no
  longer scans `meetings` in ID order under a 10,000-row budget. It runs one
  FTS5 `MATCH` seek against the new additive `retrieval_title_fts` mirror for
  every purpose and scope (Tauri sidebar, Chat/Context, and MCP share the same
  service), joined to current `meetings` by mirrored rowid so identity, title
  text, and folder membership are always the live rows. Deleted meetings, old
  titles, and out-of-scope folder membership can never be served. Ranking is
  unchanged in observable behavior: every match contains all distinct core
  terms, so the retired (overlap desc, meeting id asc) order collapses to
  meeting id ascending, applied by SQL `ORDER BY`/`LIMIT` with the per-variant
  candidate cap.
- Migration `20260908000000_add_retrieval_title_fts.sql`: additive FTS5
  virtual table (`title`, `unicode61` tokenizer, the same tokenizer family as
  `meeting_fts`), one backfill from `meetings`, and three triggers
  (insert/title-update/delete) that maintain the mirror inside the writer's
  own transaction. No title rows enter `meeting_fts`, no public lexical FTS
  command changes, no semantic document or vector changes, no model identity
  or package artifact changes.
- `update_publication_lag` fail-closed: a failed `publication_lag` read and a
  missing index-state row for the active generation now call
  `mark_lag_unknown` instead of the swallowed zero, so `index_status` cannot
  report ready off an unverifiable state. Regression
  `unverifiable_publication_lag_never_reports_caught_up` injects all three
  failures deterministically (row deleted, backing table dropped, pool
  closed).
- Single-transaction shadow terminalization:
  `mark_shadow_generation_failed` now checks the non-active pointer, the
  outstanding-work predicate (pending/retry rows below current source
  revisions), and performs the failed write inside one `BEGIN IMMEDIATE`;
  `record_item_failure` invokes it directly. Work created before the write
  commits keeps the generation building; user mutation and retry semantics
  after a genuinely terminal failed state are unchanged. Regression
  `terminalization_never_marks_a_generation_failed_over_work_created_while_it_waited`
  drives the real repository flow against a file-backed pool where a barrier
  connection holds the SQLite write lock while a user mutation commits.
- Three-character model-inference guard preserved as approved
  (`SIDEBAR_SEARCH_MIN_QUERY_LENGTH = 3` / `SEARCH_MIN_MODEL_QUERY_CHARS = 3`)
  and the 2026-09-05 decision row revised from pending to authorized by the
  user-delegated 2026-09-08 decision recorded above.
**Implementation:**
- Files: `frontend/src-tauri/migrations/20260908000000_add_retrieval_title_fts.sql`
  (new), `frontend/src-tauri/src/retrieval/service.rs`, `frontend/src-tauri/src/retrieval/tests.rs`,
  `frontend/src-tauri/src/retrieval/index.rs`, `frontend/src-tauri/src/retrieval/worker.rs`,
  `frontend/src-tauri/src/database/repositories/retrieval.rs`, `frontend/src-tauri/src/mcp/server.rs`,
  this doc, `docs/notes-chat-improvement-execution.md`.
- Approach: reuse the repository's existing `BEGIN IMMEDIATE` patterns and the
  FTS5/trigger idiom of the semantic-retrieval migration; the title lookup is a
  service-level `QueryBuilder` query per scope (folder recursive-CTE, meeting/allowed-ID
  `IN` list from request-start membership, all) with cancellation checks and
  the existing `record_candidate` provenance path.
**Not implemented:**
- No title rows in `meeting_fts`; no new public lexical commands; no
  semantic/vector/model/package changes; no change to force-lexical, lexical
  fallback, deletion/scope/privacy/cancellation behavior, or the 5.4 package
  constraints.
**Why not implemented:**
- Explicitly excluded by the 2026-09-08 authorized decision (alternative
  rejected: title rows in `meeting_fts`, title vectors).
**Verification:**
- `cargo check` - pass (only the pre-existing `retrieval/model.rs` warning).
- `cargo test --lib` - 892 passed / 0 failed / 2 ignored (one recorded
  wall-clock Deep-deadline flake on the first pass passed unchanged on the
  clean re-run; the flake predates this task and is in `retrieval/agent`).
- `cargo fmt --check` - pass.
- `pnpm run typecheck` - pass; `pnpm exec vitest run` - 168 passed / 23 files.
- `git diff --check` - pass.
- New/updated regressions: `title_lookup_finds_a_match_beyond_the_retired_ten_thousand_row_scan_cap`,
  `title_mirror_lookup_tracks_updates_deletes_and_scope` (diacritics, repeated
  terms, title update, folder move, deletion, meeting/folder/all scope),
  `folder_title_top_k_is_bounded_deterministic_and_page_order_independent`
  (rewritten to the all-core-term gate),
  `unverifiable_publication_lag_never_reports_caught_up`,
  `terminalization_never_marks_a_generation_failed_over_work_created_while_it_waited`,
  `hybrid_search_mcp_route_finds_a_matching_title_beyond_the_retired_scan_cap`
  (MCP route, 10,000+ filler meetings, title provenance `channelRank` 1).
**Rollback:**
- Forward-only migration per policy: restore the verified pre-upgrade database
  backup before running a pre-migration binary. Code-only revert of the
  service/repo/worker commits restores the previous scan/lag/terminalization
  behavior; the mirror table and triggers are additive and inert for the old
  code path.
**Decisions and follow-ups:**
- The title lookup uses all-core-term (implicit AND) semantics for every
  purpose, per the authorized decision. Production callers (Search/Chat/Context,
  Tauri and MCP) all pass `CoreTermLanguage::Unknown`, which is exactly the
  gate the retired scan enforced for Search; partial-overlap title matching
  for stated-language library callers is retired with the scan. The
  three-character minimum preserves short-query lexical/title behavior.
- Deterministic ranking, cancellation checks, per-variant caps, folder
  recursive-CTE membership, and allowed-ID bounds are unchanged from the scan
  contract; the lookup cost is proportional to the exact-match set, never a
  full table scan or candidate overfetch.
- Inherited gates unchanged: independently authored corpus, production-path
  quality/provider-answer evidence, native Windows/R13 hermetic session,
  exact-head Actions evidence, installed-package smoke. No release claim.

### Task HR-5.R6 - R5.R5 changes-requested remediation: purpose-complete ranked title lookup, string-ID mirror, bounded ranked fetch, atomic fenced terminal failure, malformed-lag fail-closed

**Status:** Complete, then changes-requested by Review R5.R6 - the scoped-work, tie-determinism, and Chat/Context-falsifiability claims below were corrected by Task HR-5.R7 (2026-09-08), which Review R5.R7 then partially superseded (see Task HR-5.R8); see that entry for the reviewed state.
**Owner:** `worker-m` (HR-5.R6)
**Completed:** 2026-09-08
**Implemented:**
- Purpose-complete title lookup: the `CoreTermLanguage::Unknown`/purpose
  bypass that skipped the title channel for Chat and Context is removed. The
  channel now runs for Search, Chat, and Context on the shared service, so
  Tauri and MCP alike get authoritative title candidates everywhere; the
  unknown-language all-core-term gate is unchanged. The approved
  search-only model-inference minimum (`SEARCH_MIN_MODEL_QUERY_CHARS`) is
  untouched: the title lookup is pure indexed SQL and never touches the model
  runtime.
- Mirror identity redesign: `retrieval_title_fts` stores the stable meeting
  string ID (`meeting_id UNINDEXED`) instead of mirroring `meetings.rowid`;
  the lookup joins live `meetings` on `m.id = meeting_id` for current
  identity, title, and folder membership. The uncommitted migration
  `20260908000000_add_retrieval_title_fts.sql` was amended in place (no
  database outside this tree applied an earlier revision, so no repair
  migration is stacked). Triggers enforce exactly one live mirror row per
  meeting across backfill (one row per `meetings.id` PK), insert,
  title-update (delete-then-insert), delete, and restart; a meeting deleted
  mid-flight leaves zero rows and can never be served through the live join.
- Bounded ranked fetch: the lookup is
  `... WHERE retrieval_title_fts MATCH ? [AND scope] ORDER BY rank LIMIT ?`
  over FTS5's `rank` (negated bm25, lower is better). This selects the FTS5
  rank-ordered cursor strategy (observed as `VIRTUAL TABLE INDEX 32:` in
  `EXPLAIN QUERY PLAN`), so rows stream best-ranked first, scope filters
  apply per row, and the per-variant LIMIT terminates the scan after the cap
  - with NO `USE TEMP B-TREE FOR ORDER BY`, which the previous
  `ORDER BY m.id` form required to sort the whole matched set. The bounded
  fetch is re-ordered in memory by (rank, meeting id ascending) - a
  documented deterministic response order; exactly-equal ranks straddling the
  fetch cut resolve by the mirror's index order, so the result is a
  deterministic function of the database state. Visiting matching index
  entries is intrinsic to ranking them; non-matching rows are never scanned.
- Atomic fenced terminal failure:
  `record_terminal_work_failure_and_maybe_terminalize` performs, in ONE
  `BEGIN IMMEDIATE`: read the meeting's current source revision under the
  writer lock; only when it equals the exact revision the failed work
  targeted, record the terminal failure; inspect the outstanding-work
  predicate; and conditionally terminalize a non-active building generation
  (shared private `terminalize_idle_generation_tx`, also used by
  `mark_shadow_generation_failed`). If the source moved past the failed work
  before the lock was acquired, the stale failure is REJECTED, the work is
  requeued pending for the newer revision (attempt budget reset), and nothing
  is terminalized over the newer revision. A meeting deleted mid-flight
  records nothing (its work row is cascade-removed) and leaves only the idle
  terminalization decision, so a fully consumed shadow never stalls in
  `building`. `record_item_failure` invokes the operation and runs
  `suppress_terminal_failure` only AFTER a confirmed applicable record - a
  rejected stale record never suppresses serving.
- Malformed lag fail-closed: `update_publication_lag` treats a published bound
  ahead of canonical, negative bounds, an overflowing/invalid delta (via
  `checked_sub` + sign check), a missing index-state row, and read failures
  as `mark_lag_unknown` - never clamped to zero/ready.
**Implementation:**
- Files: `frontend/src-tauri/migrations/20260908000000_add_retrieval_title_fts.sql`
  (amended), `frontend/src-tauri/src/retrieval/service.rs`, `frontend/src-tauri/src/retrieval/tests.rs`,
  `frontend/src-tauri/src/retrieval/index.rs`, `frontend/src-tauri/src/retrieval/worker.rs`,
  `frontend/src-tauri/src/database/repositories/retrieval.rs`, `frontend/src-tauri/src/api/chat.rs`,
  `frontend/src-tauri/src/mcp/server.rs`, this doc, `docs/notes-chat-improvement-execution.md`.
- Approach: reuse the repository's `BEGIN IMMEDIATE` patterns and FTS5/trigger
  idioms; the ranked lookup SQL is built by one shared `build_title_lookup`
  helper that the plan-shape regression explains with an `EXPLAIN QUERY PLAN`
  prefix, so the asserted plan IS the production statement.
**Not implemented:**
- No title rows in `meeting_fts`; no public lexical command, semantic
  document, vector, model identity, or package artifact changes; no change to
  force-lexical, lexical fallback, deletion/scope/privacy/cancellation
  behavior, or the Task 3.3 contract that title candidates are selection
  signals, never citations, on the Chat/Context paths.
**Why not implemented:**
- Explicitly out of the authorized decision scope and guarded by existing
  reviewed contracts.
**Verification:**
- `cargo check` - pass (only the pre-existing `retrieval/model.rs` warning).
- `cargo test --lib` - 897 passed / 0 failed / 2 ignored.
- `cargo fmt --check` - pass; `git diff --check` - pass.
- `pnpm run typecheck` - pass; `pnpm exec vitest run` - 168 passed / 23 files.
- New/updated regressions: production Chat route (`api/chat.rs`, 10,000+
  fillers, title-matched meeting cited, title-only match not cited, folder
  fencing), production Context route (`execute_hybrid_context`, beyond-cap
  selection, no title citations, folder fencing), Tauri Search +
  `handle_jsonrpc` MCP routes with beyond-cap matches, title provenance, and
  meeting/allowed-ID/folder fencing (`retrieval/tests.rs`), common-term bound
  proof with `EXPLAIN QUERY PLAN` assertions (rank-ordered cursor strategy
  present, no matched-set temp b-tree sort - reverting to `ORDER BY m.id`
  fails it), mirror one-live-row enforcement after title update, delete, and
  same-id re-insert, real `record_item_failure` concurrent same-meeting
  mutation barrier (file-backed pool, stale record rejected, work requeued,
  serving NOT suppressed, active generation untouched), real
  `record_item_failure` applicable flow (record confirmed, serving suppressed
  after confirmation, idle non-active generation terminalized), and lag
  corruption/failure injection (published ahead of canonical, negative
  bounds, missing row, dropped table, closed pool).
**Rollback:**
- Forward-only migration per policy: restore the verified pre-upgrade database
  backup before running a pre-migration binary. Code-only revert restores the
  prior lookup/lag/terminalization behavior; the mirror table and triggers
  are additive and inert for the old code path.
**Decisions and follow-ups:**
- The equal-rank fetch cut resolves by the mirror's index order (documented);
  resolving arbitrary rank ties by meeting id without a matched-set sort is
  not possible in FTS5, and the ranked form is the review-directed trade.
- `mark_shadow_generation_failed` remains the standalone idle-gated
  terminalize primitive sharing `terminalize_idle_generation_tx`; the
  production failure path uses the atomic record-and-terminalize operation.
- Inherited gates unchanged: independently authored corpus, production-path
  quality/provider-answer evidence, native Windows/R13 hermetic session,
  exact-head Actions evidence, installed-package smoke. No release claim.


### Task HR-5.R7 - R5.R6 changes-requested remediation: in-index scoped ranked title lookup, identity-key tie order, activation-grade bound validation, falsifiable Chat/Context routes, upgrade/restart migration proof

**Status:** Complete, then changes-requested by Review R5.R7 - the scope-column bm25 coupling, the derived ranking-key collision policy, and the missing final-gate revalidation below were corrected by Task HR-5.R8 (2026-09-08); see that entry for the reviewed state.
**Owner:** `worker-m` (HR-5.R7)
**Completed:** 2026-09-08
**Implemented:**
- In-index scope restriction: the mirror gains an indexed `scope` column
  (identity token `m` + lowercase hex of the stable meeting string ID, plus
  the direct-folder token `f` + hex); the MATCH expression ANDs
  column-filtered `{title}` terms with column-filtered `{scope}` tokens, so
  the FTS intersection is the in-scope candidate set - a narrow folder with
  10,000 out-of-scope common matches returns only its own rows. Meeting and
  allowed-ID scopes intersect identity tokens (the approved 100-ID bound); a
  folder scope intersects the subtree's DIRECT folder tokens expanded from
  the LIVE `meeting_folders` table at query time, bounded by
  `MAX_TITLE_SCOPE_KEYS` (512, fail-closed) with a 65,536-byte cap on the
  assembled expression (fail-closed). Descendant folders are correct through
  the query-time expansion; a re-parented subtree needs no mirror
  maintenance; a folder deletion rewrites its direct meetings' tokens ahead
  of the ON DELETE SET NULL action.
- Identity-deterministic equal-rank selection: the mirror rowid is a stable
  ranking key derived from the full meeting ID (two 31-polynomial folds of
  its UTF-8 bytes combined into one 63-bit integer, computed by the backfill
  and triggers via one recursive CTE). The rank-ordered cursor breaks
  equal-rank ties by stable identity, never insertion history; the response
  re-sorts the bounded fetch by (rank, meeting id ascending). Key collisions
  are impossible inputs that fail closed at the mutating statement (FTS5
  rowid uniqueness), proven with the deterministic pair `a` and `U+0003
  U+0004` (both fold to 97). The scope column always carries exactly three
  tokens (identity, folder when present, padding) so document length is
  uniform and bm25 stays title-driven.
- Activation-grade bound validation: `validated_publication_delta` is the
  single validator; the activation gate blocks malformed stored pairs
  (negative, published ahead of canonical, unrepresentable delta) and
  missing rows, the post-install lag setter routes malformed pairs to
  `mark_lag_unknown`, and the status refresh shares the same validator.
- Falsifiable Chat/Context title routes: the target's summary, notes, and
  transcript share NO token with the query, so only the title channel can
  select the meeting; its authoritative content is cited and the title
  itself is never a citation (frozen Task 3.3 boundary); folder and
  allowed-ID scopes fence other matches.
- Upgrade/restart migration proof: a pre-migration populated file database is
  upgraded by the title migration, the backfill seeds exactly one correctly
  scoped row per legacy meeting, a reopen asserts persistence, and title
  update, folder removal, and meeting deletion are followed by the mirror.
- Execution record repair: control characters replaced, HR-5.R6 claims
  narrowed via an explicit supersession banner, and this entry records the
  corrected contracts.
**Implementation:**
- Files: `frontend/src-tauri/migrations/20260908000000_add_retrieval_title_fts.sql`
  (amended in place), `frontend/src-tauri/src/retrieval/service.rs`,
  `frontend/src-tauri/src/retrieval/tests.rs`,
  `frontend/src-tauri/src/retrieval/hydration.rs`,
  `frontend/src-tauri/src/retrieval/index.rs`,
  `frontend/src-tauri/src/api/chat.rs`,
  `frontend/src-tauri/src/mcp/server.rs`,
  `frontend/src-tauri/src/database/migration_tests.rs`,
  `docs/hybrid-rag/sprint-5-search-release.md`,
  `docs/notes-chat-improvement-execution.md`.
- Approach: scope keys and the ranking key live in the additive mirror and
  are maintained transactionally by triggers; the shared `build_title_lookup`
  helper emits one scope-independent statement and the plan-shape regression
  explains the identical SQL.
**Not implemented:**
- No title rows in `meeting_fts`; no public lexical command, semantic
  document, vector, model identity, or package artifact changes; no change to
  force-lexical, lexical fallback, deletion/scope/privacy/cancellation
  behavior, or the Task 3.3 title selection/citation boundary.
**Why not implemented:**
- Explicitly out of the authorized decision scope and guarded by existing
  reviewed contracts.
**Verification:**
- `cargo check` - pass (only the pre-existing `retrieval/model.rs` warning).
- `cargo test --lib` - 902 passed / 0 failed / 2 ignored.
- `cargo fmt --check` - pass; `git diff --check` - pass.
- `pnpm run typecheck` - pass; `pnpm exec vitest run` - 168 passed / 23 files.
- New/updated regressions: narrow-folder bounded lookup over 10,000
  out-of-scope common matches with `EXPLAIN QUERY PLAN` assertions for all
  four scopes (rank-ordered cursor strategy present; no temp B-tree sort; no
  scope CTE - reverting to a post-filter or `ORDER BY m.id` fails them);
  meeting/allowed-ID exactness; equal-rank selection following the stable
  identity key across insertion orders; key-collision fail-closed; oversized
  scope key and >512-folder subtree fail-closed; mirror maintenance under
  folder move/delete/re-parent; falsifiable production Chat and Context
  routes (All/Folder/Allowed); activation blocking on malformed stored bounds
  with a repaired-bounds control; legacy upgrade/restart/backfill migration
  coverage.
**Rollback:**
- Forward-only migration per policy: restore the verified pre-upgrade
  database backup before running a pre-migration binary. Code-only revert
  restores the prior lookup/lag/terminalization behavior; the mirror table
  and triggers are additive and inert for the old code path.
**Decisions and follow-ups:**
- The scope-token/identity-key design is a durable schema trade: one additive
  FTS table carries the scope and ranking keys (maintained transactionally by
  triggers) so query work is bounded by the requested scope and candidate
  limit; the review's rejected alternative (a hard scan budget with
  incomplete results) was not used.
- bm25 ordering is unchanged relative to title content (uniform three-token
  scope shape), and equal-rank ties resolve by stable identity, never
  insertion history.
- Inherited gates unchanged: independently authored corpus, production-path
  quality/provider-answer evidence, native Windows/R13 hermetic session,
  exact-head Actions evidence, installed-package smoke. No release claim.

### Task HR-5.R8 - R5.R7 changes-requested remediation: final-gate publication-bound revalidation, zero-weight scope ranking, collision-free meeting-ID tie-break

**Status:** Historical implementation; superseded by R5.R8 and R5.R9 changes-requested reviews. The activation and work-bound claims below were not accepted. Current remediation is HR-5.R10.
**Owner:** `worker-m` (HR-5.R8)
**Completed:** 2026-09-08
**Implemented:**
- Final-gate publication-bound revalidation: `activate_generation_if_ready`
  now reads BOTH publication bounds inside its `BEGIN IMMEDIATE` and
  validates them with the shared `validated_publication_delta` (moved to the
  repository module as the single validator used by the status refresh, the
  activation preflight, and this final gate). Negative bounds, a published
  bound ahead of canonical, an unrepresentable delta, or a missing
  index-state row block the activation: no ready flip, no active-pointer
  move, the prior generation keeps serving. This closes the TOCTOU between
  the preflight and the durable transition.
- Zero-weight scope ranking: every candidate is scored with the explicit
  per-query weight vector `bm25(retrieval_title_fts, 1.0, 1.0, 0.0)` - the
  scope column's weight is zero - so scope-token document frequencies cannot
  influence title relevance, order, or the bounded top-k. Probe-verified: a
  candidate's score is bit-identical across a 50x change in its folder
  token's document frequency. The three-token scope shape is kept so the
  ranking document length stays uniform across rows.
- Collision-free tie-break: the derived 63-bit ranking-key scheme (whose
  31-polynomial fold collided on ordinary IDs `Aa`/`BB`) is removed. Mirror
  rows carry no derived key; the rowid is FTS5-assigned and meaningless.
  Deterministic selection comes from a bounded rowid-window batch scan
  (`TITLE_SCAN_BATCH_ROWS` = 400; the rowid range constraint is pushed into
  FTS5, so each in-scope match is visited exactly once) feeding a bounded
  top-k heap ordered by (bm25 title score, meeting ID ascending) - the
  meeting ID itself is the bijective, collision-free tie-break, making the
  selection a deterministic function of database content independent of
  insertion history, and no schema-valid TEXT meeting ID can fail insertion,
  upgrade, or backfill.
**Implementation:**
- Files: `frontend/src-tauri/migrations/20260908000000_add_retrieval_title_fts.sql`
  (amended in place), `frontend/src-tauri/src/database/repositories/retrieval.rs`,
  `frontend/src-tauri/src/retrieval/index.rs`,
  `frontend/src-tauri/src/retrieval/service.rs`,
  `frontend/src-tauri/src/retrieval/tests.rs`,
  `docs/hybrid-rag/sprint-5-search-release.md`,
  `docs/notes-chat-improvement-execution.md`.
- Approach: the centralized validator lives in the repository module beside
  the final gate; the lookup's batch query and the bounded heap live in the
  title channel, with the plan-shape regression explaining the identical
  SQL.
**Not implemented:**
- No title rows in `meeting_fts`; no public lexical command, semantic
  document, vector, model identity, or package artifact changes; no change to
  force-lexical, lexical fallback, deletion/scope/privacy/cancellation
  behavior, or the Task 3.3 title selection/citation boundary.
**Why not implemented:**
- Explicitly out of the authorized decision scope and guarded by existing
  reviewed contracts.
**Verification:**
- `cargo check` - pass (only the pre-existing `retrieval/model.rs` warning).
- `cargo test --lib` - 904 passed / 0 failed / 2 ignored.
- `cargo fmt --check` - pass; `git diff --check` - pass.
- `pnpm run typecheck` - pass; `pnpm exec vitest run` - 168 passed / 23 files.
- New/updated regressions: the final-gate race regression
  (`activation_final_gate_blocks_malformed_bounds_after_preflight` - bounds
  corrupted directly before the gate: refusal, building state, prior
  generation serving, repaired-bounds commit control); the uneven
  parent/child folder-frequency regression
  (`title_scope_token_document_frequencies_do_not_change_ranking`) that
  fails under default bm25 weights; the adversarial-ID test
  (`title_arbitrary_ids_are_collision_free_and_rank_deterministically` - `Aa`,
  `BB`, `U+0003 U+0004`, `a` coexist and rank by ID); reverse-insertion
  deterministic top-k
  (`title_equal_rank_selection_follows_the_stable_identity_key`); the
  10,000-out-of-scope common-title plan/bound test updated to the batch SQL
  for all four scopes; and the legacy upgrade/reopen backfill coverage.
**Rollback:**
- Forward-only migration per policy: restore the verified pre-upgrade
  database backup before running a pre-migration binary. Code-only revert
  restores the prior lookup/lag/activation behavior; the mirror table and
  triggers are additive and inert for the old code path.
**Decisions and follow-ups:**
- The meeting ID itself is the tie-break (bijective over the ID space), so
  no collision class exists and no valid ID can be rejected; determinism is
  by content (score, ID), not insertion history.
- The rank-ordered cursor was replaced by the rowid-window batch scan +
  client heap: both visit exactly the in-scope match set, but only the batch
  design admits an arbitrary-ID collision-free tie-break. Per-request FTS
  work stays bounded by the in-scope match set; memory by the candidate cap
  plus one batch.
- The FTS5 rank-configuration INSERT does not persist across database reopen
  (probe-verified), so the weights are supplied per query through the
  `bm25()` auxiliary function instead of a stored configuration.
- Inherited gates unchanged: independently authored corpus, production-path
  quality/provider-answer evidence, native Windows/R13 hermetic session,
  exact-head Actions evidence, installed-package smoke. No release claim.

### Task 5.5 - initial local qualification baseline

**Status:** Blocked (local baseline and synthetic activation/disk scale rows recorded; no release or sprint-close claim)
**Owner:** `worker-l` (Task 5.5 initial qualification pass)
**Completed:** 2026-09-09
**Implemented:**
- No production-runtime correction. This entry records only the reproducible,
  non-corpus local qualification subset and its test-only harness extension.
- The guarded source-path activation-envelope test now accepts only the three
  approved synthetic corpus sizes (`12000`, `50000`, and `250000`), defaulting
  to its prior 250k behavior when unset. Its exact-axis query cap is now bounded
  by the deterministic number of exact synthetic matches, so smaller approved
  rows never validate zero-similarity filler rows as exact matches.
- Benchmark database cleanup now drops the closed pool before bounded removal,
  retries the three database files, and has a drop guard for assertion-failure
  paths. A successful run fails if its own temporary database remains.
- All three guarded local source-path active-plus-shadow activation and exact
  derived-disk-envelope rows passed using the approved staged bundle.
**Not implemented:**
- P95 latency, concurrency, crash/restart, recording, provider-answer,
  package-smoke, and the remaining full-matrix qualifications.
**Why not implemented:**
- They require dedicated controlled data, native/package sessions, or external
  evidence not available to this local baseline.
**Verification:**
- `cargo check --locked --manifest-path frontend/src-tauri/Cargo.toml --lib`
  - pass.
- `cargo fmt --manifest-path frontend/src-tauri/Cargo.toml --check` - pass.
- Direct `node node_modules/typescript/bin/tsc --noEmit` from `frontend` -
  pass; this is not `pnpm --dir frontend run typecheck`.
- `git diff --check` - pass.
- The pre-selector serial library baseline passed 921 / 0 / 4 ignored in
  115.37 s. The current selector change has focused selector and scale-test
  coverage below; no broad current-head library-suite result is claimed because
  that invocation is not safely separable under this qualification's evidence
  exclusions.
- `cargo test --locked --manifest-path frontend/src-tauri/Cargo.toml --lib
  retrieval::agent::tests::initial_failure_after_crossing_the_boundary_keeps_the_underlying_error
  -- --test-threads=1` - pass: 1 passed / 0 failed / 924 filtered in 1.69 s.
  This was the provisional boundary test observed during the initial sampled
  serial output; it is not a hanging test.
- `cargo test --locked --manifest-path frontend/src-tauri/Cargo.toml --lib
  retrieval::index::tests::bench_corpus_selector_accepts_only_approved_scale_rows`
  - pass: 1 / 0 / 925 filtered. `cargo check --locked --manifest-path
  frontend/src-tauri/Cargo.toml --lib`, touched-file `rustfmt --edition 2021
  --check`, and `git diff --check` also passed after the selector change.
- `MEETLY_RAG_INDEX_BENCH=1` with each approved
  `MEETLY_RAG_INDEX_BENCH_DOCUMENTS` value ran `cargo test --release --locked
  --manifest-path frontend/src-tauri/Cargo.toml --lib
  retrieval::index::tests::bench_2r6_production_activation_envelope --
  --nocapture`, with all three rows passing inside the 20-minute watchdog:

  | Documents | Test time after release compile | Active snapshot | Exact derived disk steady / active+shadow | Active+shadow process peak / window | Reranker validation |
  |---:|---:|---:|---:|---:|---:|
  | 12,000 | 3.98 s after 1m 04s compile | 132 ms | 13.3 MiB / 26.5 MiB | 734.5 MiB / 152 ms | 678 ms; 1,042.7 MiB process peak |
  | 50,000 | 10.95 s after 1m 04s compile | 751 ms | 55.0 MiB / 110.1 MiB | 743.7 MiB / 872 ms | 721 ms; 1,078.0 MiB process peak |
  | 250,000 | 56.86 s after 1m 28s compile | 9,068 ms | 275.5 MiB / 551.0 MiB | 1,113.2 MiB / 9,699 ms | 719 ms; 1,272.8 MiB process peak |

  The exact `dbstat` 250k row measured 288,849,920 bytes steady and
  577,785,856 bytes active-plus-shadow, leaving 1,858,633,728 bytes below the
  2 GiB steady target and 2,643,439,616 bytes below the 3 GiB activation limit.
  Its process peak was 1,167,290,368 bytes, leaving 228,574,003 bytes under
  the test's 1.30 GiB transient ceiling. Temporary benchmark databases from
  the three post-fix rows were removed, and no cargo, compiler, or benchmark
  process remained. Three 32,231,888-byte synthetic database files from the
  earlier pre-fix failed run remain outside the worktree because this execution
  environment forbids their manual deletion; they are not qualification
  evidence and no process holds them.
- `pnpm --dir frontend run typecheck` - unavailable in this worktree: pnpm
  requested a non-interactive modules-directory replacement. No install or
  replacement was authorized for this baseline.
- Vitest was not run because this qualification pass does not establish a
  safely separable non-corpus invocation.
**Rollback:**
- None; no product code changed.
**Decisions and follow-ups:**
- A valid independently authored Portuguese corpus, production-path quality and
  final provider-answer evidence, a native Windows/R13 full loaded-application
  session, p95 and the remaining Task 5.5 matrix, installed-package smoke,
  final reviewed-head Actions, final reviews, and user closure remain open.
- No non-completing library test was found: the serial log ends with the full
  passing harness summary. The longer serial wall time is diagnostic only and
  does not replace any scale or performance qualification.
- The three activation/disk-envelope rows do not establish corpus quality,
  Fast/Deep quality or p95 latency, reference-hardware performance, package
  behavior, or release acceptance.

## Sprint Reviews

### Code Review

**Reviewer:** `anthropic/claude-opus-5` (Claude Code, `/code-review xhigh`), four rounds
**Verdict:** Implementation findings resolved through HR-5.R4; subsequent
reviews through R5.R9 returned changes-requested. HR-5.R10 now corrects the
absolute activation watermark and title snapshot/publication races, with
independent scoped approval and passing boundary regressions. Candidate-
bounded title work has not been achieved; an explicit user design decision
is pending. Sprint close remains blocked by the separate release gates.

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
- **R5.R4** (the two follow-ups selected from the R5.R3 judgement calls): 4
  findings, all in the HR-5.R3/HR-5.R3b corrections themselves - the
  client-side title scan was unbounded and now ran on every search, and the
  reserved head counted candidates instead of rendered rows, so repeats could
  push every other missed title match past the final slice. All 4 fixed in
  Task HR-5.R4, both behavioural ones proven by negative control.
- **R5.R5** (independent review of the user commits `2d640c6..f5fa329`):
  changes-requested - 6 findings: (1) the unknown-language/purpose bypass
  dropped the title channel for Chat and Context entirely; (2) the title
  mirror used `meetings.rowid` as identity, which is not the stable meeting
  ID and is not preserved across vacuum-style rebuilds; (3) the lookup
  ordered the whole matched set by `m.id` (`USE TEMP B-TREE FOR ORDER BY`),
  so a common title term still forced a full matched-set sort; (4) the
  terminal work failure was still split across an unfenced failure record and
  a separate terminalize, and `suppress_terminal_failure` ran before the
  record was confirmed applicable; (5) `update_publication_lag` clamped
  malformed lag rows (negative bounds, published ahead of canonical) to zero;
  (6) the docs recorded HR-5.R5 as implemented-and-pending without the review
  verdict. All 6 fixed in Task HR-5.R6; the R5.R6 review then returned
  changes-requested on that remediation (see R5.R6 below).
- **R5.R6** (review of the HR-5.R6 remediation diff): changes-requested - 5
  findings: (1) scoped lookups still rank-scanned the global FTS match set
  before applying the scope predicate (about 400,000 VM steps at 10,000
  out-of-scope matches for `LIMIT 3`) and equal-rank selection followed FTS
  insertion history; (2) malformed publication bounds still passed the
  activation gates (only the status refresh validated them); (3) the new
  Chat/Context title-route tests were not falsifiable (target content carried
  the query terms); (4) no legacy pre-migration upgrade/restart/backfill
  coverage existed; (5) the execution record contained control characters and
  overstated claims. All 5 fixed in Task HR-5.R7; the R5.R7 review then
  returned changes-requested on that remediation (see R5.R7 below).
- **R5.R7** (review of the HR-5.R7 remediation diff): changes-requested - 3
  findings: (1) the final transactional activation gate read only the
  canonical bound, so malformed state introduced after the preflight could
  still be durably marked ready and activated (TOCTOU); (2) scope tokens
  shared the default bm25 weights, so a rare folder token's document
  frequency could inflate a meeting's score and change the bounded top-k;
  (3) the derived 63-bit ranking-key scheme rejected schema-valid meeting
  IDs on collision (ordinary IDs `Aa` and `BB` both fold to 2112) and could
  brick the upgrade backfill. HR-5.R8 addressed these but its subsequent
  R5.R8/R5.R9 reviews found a wrong-dimension activation comparison,
  exhaustive title work, cross-page mutation races, and overstated records.

Both rounds, their per-finding corrections, verification output, and the
environment/flake caveats are recorded in
[`notes-chat-improvement-execution.md`](../notes-chat-improvement-execution.md)
under `R5.R1`, `HR-5.R1`, `R5.R2`, `HR-5.R2`, `R5.R3`, `HR-5.R3`,
`HR-5.R3b`, `R5.R4`, `HR-5.R4`, `R5.R5`, `HR-5.R6`, `R5.R6`, `HR-5.R7`,
`R5.R7`, and `HR-5.R8`.

**Verification after remediation:** `cargo check` pass; `cargo test --lib` 889
passed / 0 failed / 2 ignored; `cargo fmt --check` pass; `pnpm run typecheck`
pass; `pnpm exec vitest run` 168 passed / 23 files; `git diff --check` pass.

**Verification after HR-5.R6 (2026-09-08, this working tree):** `cargo check`
pass (only the pre-existing `retrieval/model.rs` warning); `cargo test --lib`
897 passed / 0 failed / 2 ignored; `cargo fmt --check` pass; `pnpm run
typecheck` pass; `pnpm exec vitest run` 168 passed / 23 files; `git diff
--check` pass.

**Verification after HR-5.R8 (2026-09-08, this working tree):** `cargo check`
pass (only the pre-existing `retrieval/model.rs` warning); `cargo test --lib`
904 passed / 0 failed / 2 ignored; `cargo fmt --check` pass; `pnpm run
typecheck` pass; `pnpm exec vitest run` 168 passed / 23 files; `git diff
--check` pass. The previously recorded timing-flake class (manual pause,
MCP deadline capacity, Deep deadline tests - none touched by the HR-5.R8
diff) has surfaced single-test failures under full-suite load across
repeated runs; each passes unchanged in isolation and on clean re-runs.

**Open items:** (1) the 2026-09-05 three-character minimum-length decision row
is authorized by the user-delegated 2026-09-08 decision recorded above; the
guard stays approved at three characters. (2) Meeting titles are indexed in
the additive `retrieval_title_fts` mirror (migration `20260908000000`) keyed
by the stable meeting string ID and maintained transactionally by triggers.
The migration's rowid-window comment records the historical R8 implementation
and is deliberately immutable now that its source is committed remotely:
editing even a comment changes SQLx's checksum for databases that applied it.
The current lookup is instead one scoped exact SQLite
`ORDER BY bm25(...), m.id COLLATE BINARY LIMIT` statement per existing
disjoint scope group, followed by a cap-sized Rust merge under one read
snapshot. SQLite scores/sorts every matching in-scope row before `LIMIT`, so
the approved output/memory bound is not a candidate-bounded database-work
claim. Snapshot consistency and current-title hydration fences are tested.
The public `meeting_fts` lexical contract, the semantic document set, and the
vectors are unchanged, and the client-side substring union remains as the
bounded presentation-layer complement, not a completeness safety net.
HR-5.R10 correctness/boundary review is approved; its exact-SQL implementation
is independently reviewed, targeted integration checks pass, and CI10 passed
at its exact source head. The separate final Task 5.5/release gates remain
open. Maximum-length public scopes use disjoint bounded MATCH groups under the
same snapshot and cap-sized merge, preserving exact score/ID order.

**Package-authority handoff (2026-09-08):** Task 5.4a remediation
HR-5.4a.R3 is independently approved (review R5.4a.R4). Actual Tauri
resource expansion, eight filesystem-backed helper regressions, complete
stager recovery/rollback SelfTest, 22 application bundle tests with real
staged artifacts, cargo check, and warm-cache publication passed. The
manifest/model/signing identities are unchanged. Task 5.4b is also accepted
after independent review: the additive installed-resource diagnostic passes
real source-side package-layout inference (six embedding references, five
reranker pairs, two retained sources), normal lexical fallback and bounded
failure checks. Fresh pinned Rust 1.88 verification passed 920 library tests
with four ignored; the real diagnostic was explicitly run separately. A
file-symlink test skipped for missing Windows privilege is disclosed, not
counted as executed evidence. Task 5.4c's separate code and architecture reviews
requested a registration-based installation-ownership correction before any
real installer run. That correction now passes 205 assertions in both worker
and primary runs, including native MSI metadata extraction, hidden process
mechanics, fail-closed registration preflight and ownership-proved cleanup.
The local all-user MSI inventory probe is permission-limited and not counted
as passing native inventory evidence. YAML/Bash syntax checks also passed.
Independent code and architecture re-reviews now approve implementation
commit/dispatch. Actual installed MSI/NSIS and parent 5.4 acceptance remain
open. The execution log records
the complete verification and exclusions.

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
