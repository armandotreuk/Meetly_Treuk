# Sprint 2C: Close-Out Evidence And Envelope Decision

## Status

Blocked. Tasks `2C.1`, `2C.2`, and `2C.4` are complete, but `2C.3` cannot
safely produce qualifying full-application calibration evidence on this host.
Sprint closure remains blocked on that evidence and its dependent reviews.

## Goal

Resolve the verified residual R18 defects, prove the Windows installers expose
SQLite `dbstat`, and close Sprint 2 only when its activation-envelope contract
and independent reviews are approved.

## Scope

### In scope

- Correct the indexed lookup used by debug-time document-count reconciliation
  during meeting deletion.
- Preserve a bounded, stage-specific packaged-smoke failure verdict through the
  GUI-subsystem executable and root Windows workflow.
- Select and implement an approved resolution for the R13 activation RAM-gate
  blocker.
- Run the actual MSI and NSIS installed-package smoke after final source
  changes.
- Re-run Sprint 2 closure evidence and obtain fresh code and architecture
  reviews.

### Out of scope

- A second model bundle, prior-model retention packaging, or its RAM
  measurement.
- A persisted force-lexical kill switch, MCP semantic tooling, or Sprint 3
  query surfaces.
- Changing the approved model, chunking, vector encoding, or backend contract
  unless explicitly required by the user-selected R13 resolution.

## Current State And Evidence

- `migrations/20260827030000_add_retrieval_documents_meeting_lookup_index.sql`
  adds `retrieval_documents(meeting_id, generation_id)`. Fresh- and
  upgrade-migration query-plan regressions reject `SCAN retrieval_documents`.
- `frontend/src-tauri/src/lib.rs` emits bounded exact, unavailable, runtime,
  SQLite-connection, migration, deterministic-setup, and measurement verdicts;
  the Windows workflow preserves them through the GUI-subsystem package path.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` passes.
- `sprint-2-durable-local-index.md:1177-1218` records R13 as blocked: current
  ONNX Runtime bindings cannot measure native session residency in a
  retrieval-scoped, non-undercounting way.
- The root workflow only auto-runs on `main` and `dev`
  (`.github/workflows/build-windows.yml:3-8`), so release evidence was manually
  dispatched. Run `41` passed its checked-in contract, staged bundle, reference
  inference, release build, installed MSI and NSIS smokes, and package uploads.
- Current verification is green: 584 Rust tests passed (2 ignored),
  `cargo check --lib --tests`, frontend typecheck, 95 Vitest tests, workflow
  YAML parsing, range `git diff --check`, and debug `--smoke-dbstat` all pass.

## Requirements And Acceptance Criteria

- Deleting a meeting in a debug/test build must find affected generations with
  an indexed `meeting_id` lookup and keep the existing `MAX(document_count -
  N, 0)` clamp and post-commit non-mutating reconciliation.
- Packaged smoke outcomes must distinguish exact `dbstat`, unavailable
  `dbstat`, and each bounded probe failure stage without relying on a GUI
  process console or exposing database/user content.
- No task may claim MSI or NSIS coverage until the installed executable passes
  in an actual root-workflow run.
- R13 must be resolved by an explicitly approved contract; no heuristic,
  unmeasurable session estimate, or ceiling change may be introduced by
  implication.
- Sprint close requires all tasks below accepted, the full verification suite,
  the gated 250k production benchmark, fresh approving reviews, and user
  sprint-close approval.

## Technical Approach

Use an additive forward-only SQLite index ordered by the deletion predicate,
then add a query-plan regression around the existing affected-generation
lookup. Keep the existing diagnostic's narrow non-GUI entrypoint and transport
only a bounded stage classification in its exit code; the root workflow maps
that code to a public CI sentence. Reuse the existing R13 proof and implement
only the option the user approves. Run native installer evidence once, after
the final source change, rather than treating static workflow inspection as
package verification.

## Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 2C.1 | Deletion reconciliation | Add the forward-only `(meeting_id, generation_id)` index and query-plan regression for affected-generation discovery. Preserve the bounded decrement and post-commit read-only reconciliation. | M, high risk | Pending `worker-m` | Sprint PRD and migration-risk approval | Upgrade and fresh-migration tests prove the index exists; `EXPLAIN QUERY PLAN` shows an indexed meeting lookup rather than a scan; affected-generation deletion regression passes. | Revert the new migration and lookup regression; existing deletion semantics remain intact but the debug lookup scan returns. |
| 2C.2 | Packaged smoke diagnostics | Give every safe probe failure stage a bounded verdict that survives the GUI-subsystem executable; map it in the active root workflow; apply rustfmt. | M | Pending `worker-m` | 2C.1 complete | Focused status/exit-code tests cover exact, unavailable, and every failure stage; release diagnostic exits correctly; workflow YAML/order checks and `cargo fmt --check` pass. | Revert the exit-code/workflow mapping and tests; `dbstat` activation remains fail-closed. |
| 2C.D1 | Envelope decision | Select one R13 resolution contract: full-application calibrated whole-process ceiling, runtime attribution infrastructure, upstream ORT API wait, sessions-excluded accounting, or an explicitly accepted conservative bound. | Decision | Main agent | Complete | Full-application calibrated whole-process scope is recorded with its ceiling consequences and rejected alternatives. | Keep R13 blocked; do not implement an unapproved approximation. |
| 2C.3 | Activation envelope | Calibrate the existing whole-process gate against the production application and revise its ceiling only from recorded full-application evidence. | L, high risk | Pending `worker-l` | 2C.D1 complete | The gate measures the documented full-application quantity, fails closed on unavailable terms, passes its agreed benchmark, and does not alter model/chunk/encoding/backend contracts without separate approval. | Revert the R13 implementation and architecture amendment; retain the current R12 fail-closed gate. |
| 2C.4 | Installed package evidence | Dispatch the root Windows workflow for the accepted 2C.1/2C.2 branch head and record actual MSI and NSIS silent-install, diagnostic, uninstall, cleanup, and pre-upload-gate outcomes. Rerun after any later R13 source change. | M | Main agent | 2C.2 complete | Both installed `meetily.exe --smoke-dbstat` runs pass; job summary records them; no smoke is skipped; CI URL and immutable addendum are recorded. | Revert only the diagnostic CI assertion if it prevents emergency packaging; semantic activation remains fail-closed without `dbstat`. |
| 2C.5 | Sprint closure | Run final verification, append fresh code and architecture reviews, record deferrals, and request user Sprint 2 close approval. | M | Main agent and reviewers | 2C.4 complete | Full Rust suite, `cargo check`, rustfmt, diff check, typecheck, Vitest, and 250k benchmark pass; both reviews approve; user approves close. | Do not close Sprint 2; preserve the accepted tasks and keep the remaining finding open. |

## Dependency Order

`2C.1 -> 2C.2 -> 2C.4`; `2C.D1 -> 2C.3`; `2C.3 + 2C.4 -> 2C.5`

Task `2C.1` runs alone because it adds a migration. Task `2C.3` runs alone
because it changes the activation-envelope authority. `2C.4` can establish the
independent installed-`dbstat` contract now; it must rerun when a future R13
source change creates the final closure head.

## Risks And Mitigations

- **Migration performance:** index creation changes existing databases. Use the
  established forward-only migration path, test both upgrade and fresh
  databases, and require explicit migration-risk approval.
- **Diagnostic ambiguity:** exit codes must name only safe technical stages;
  never emit SQL, paths, meeting text, tokens, or vectors.
- **Native packaging:** static YAML validation cannot prove WiX/NSIS install
  behavior. Treat real Windows CI as the sole acceptance evidence.
- **R13 scope drift:** calibration must load the same approved application
  components the release keeps resident; a retrieval-only benchmark cannot set
  the whole-process ceiling.
- **Review regression:** do not edit prior review or execution entries; append
  new review records and dated decision-log addenda only.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-27 | Created Sprint 2C as a separate close-out PRD. | The original Sprint 2 record is an immutable execution history; R18 verification produced new work and remaining closure gates. | Edit prior R18 or claim it complete without a new plan. | User requested sprint planning |
| 2026-08-27 | Approved the Sprint 2C PRD and its additive-index migration risk. | The verified deletion lookup scan and formatting failure require a bounded correction before close-out evidence can be trusted. | Defer the scan until a later sprint; accept an unformatted close candidate. | User |
| 2026-08-27 | Keep R13 as an explicit decision gate. | Existing bindings cannot meet all required measurement properties; selecting an approximation would be an architecture change. | Implicitly retain whole-process RSS, use snapshots only, or raise the ceiling without approval. | Pending user |
| 2026-08-27 | Select a full-application calibrated whole-process gate for R13 and delegate all Sprint 2C batch approvals to the main agent. | Current ONNX Runtime bindings cannot attribute native session residency without undercounting. Whole-process calibration is the smallest available non-undercounting measurement; its ceiling may change only after the production application is measured. | Runtime attribution infrastructure; wait for an upstream API; sessions-excluded accounting; a fixed session bound. | User delegation |
| 2026-08-28 | Reject the first `2C.3` calibration implementation; retain the existing ceiling as authoritative until remediation passes fresh code and architecture review. | Independent reviews found incomplete runs could set a ceiling, nonzero calibration exits became success, the harness omitted claimed production state, and a regular environment variable could bypass the gate. | Accept unreviewable one-host evidence; relabel the retrieval benchmark as full application. | Main agent under user delegation |
| 2026-08-28 | Mark `2C.3` blocked and remove its incomplete implementation. | A safe isolated full-app run cannot make the audio stack resident without contending on live WASAPI/CPAL streams; the required Whisper artifact is absent on this host; no existing API proves WebView residency through its production path. The partial code was uncompilable and could not yield valid evidence. | Retain an environment-controlled gate bypass; accept a retrieval-only or partial-component measurement; raise the ceiling from unverifiable evidence. | Main agent under user delegation |
| 2026-08-28 | Run `2C.4` ahead of blocked `2C.3`. | Installed-package `dbstat` coverage does not depend on the RAM gate and can be tested on the accepted 2C.1/2C.2 head. It is explicitly repeated if R13 later changes packaged source. | Delay all independent evidence until external calibration prerequisites exist. | Main agent under user delegation |
| 2026-08-28 | Repair fresh-checkout package evidence before accepting `2C.4`. | CI proved the compile-time manifest probe used an unstaged path, the smoke looked in the wrong Tauri target directory, and GUI-process exit handling lost the bounded verdict. The workflow now uses the checked-in manifest, actual bundle directory, persisted runner verdicts, and explicit process exit codes. | Accept local staged artifacts as package evidence; weaken or remove the installed-package gate. | Main agent under user delegation |

## Task Execution Log

<!-- Add one immutable entry per completed, blocked, or cancelled task. -->

### 2C.D1 - Envelope decision

**Status:** Complete
**Owner:** Main agent, under user delegation
**Completed:** 2026-08-27
**Implemented:**
- Selected calibration of the existing whole-process activation gate against the production application as the R13 resolution direction.
**Implementation:**
- Files: this file; the existing Sprint 2 architecture amendment will be made by `2C.3`.
- Approach: use the current measurable whole-process quantity rather than inventing an unobservable retrieval-scoped session term.
**Not implemented:**
- No ceiling change or runtime-gate code before `2C.3` records full-application calibration evidence.
**Why not implemented:**
- The exact ceiling must be derived from the selected production measurement, not assumed from a retrieval-only harness.
**Verification:**
- R13 blocker proof confirms existing public/runtime facilities cannot measure the native ONNX session term without undercounting.
**Rollback:**
- Restore the R13 blocked decision and leave the current R12 fail-closed whole-process gate unchanged.
**Decisions and follow-ups:**
- The user delegated authority for this decision and all Sprint 2C task batches. `2C.3` must still demonstrate real full-application calibration before changing the ceiling.

### 2C.1 - Index deletion lookup

**Status:** Complete
**Owner:** `worker-m` (`ses_fb9c488d3ffe1VC9YUvXMFs6oT`)
**Completed:** 2026-08-27
**Implemented:**
- Added the forward-only `retrieval_documents(meeting_id, generation_id)` lookup index without changing or dropping existing indexes or rows.
- Added fresh- and upgrade-migration regressions that inspect the index columns and require the exact affected-generation query plan to use the new covering index rather than scan `retrieval_documents`.
- Updated the isolated deletion fixture to represent the production index layout; the bounded decrement and post-commit debug reconciliation remain unchanged.
**Implementation:**
- Files: `frontend/src-tauri/migrations/20260827030000_add_retrieval_documents_meeting_lookup_index.sql`, `frontend/src-tauri/src/database/migration_tests.rs`, `frontend/src-tauri/src/database/repositories/meeting.rs`, this file.
- Approach: order the additive index by the deletion predicate's equality column, then its distinct/order-by generation column. The regression rejects `SCAN retrieval_documents` so future schema changes cannot silently reintroduce the write-lock scan.
**Not implemented:**
- No data rewrite, index removal, replacement-path reconciliation, public API, dependency, or logging change.
**Why not implemented:**
- The defect is the debug-time lookup's missing leading index column; broader counter or publication changes would add unrelated scope.
**Verification:**
- `$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"; cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::repositories::meeting::tests -- --nocapture` - pass: 2 passed.
- `$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"; cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib database::migration_tests -- --nocapture` - pass: 6 passed, including fresh and upgrade query-plan assertions.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml" --lib --tests` - pass; pre-existing `retrieval/model.rs` unnecessary-parentheses warning remains.
- `rustfmt --edition 2021 --check "frontend/src-tauri/src/database/migration_tests.rs" "frontend/src-tauri/src/database/repositories/meeting.rs"` - pass.
- `git diff --check` - pass.
- Full `cargo fmt --check` remains failing only on the pre-existing R18 import wrapping in `frontend/src-tauri/src/lib.rs`; `2C.2` owns that correction.
**Rollback:**
- Restoring the prior query leaves the additive index harmless. Do not delete an applied forward-only migration; add a later migration to drop the index only if a measured replacement makes it obsolete.
**Decisions and follow-ups:**
- `2C.2` is now dependency-ready. The migration applies only the smallest index needed to remove the verified write-lock scan.

### 2C.2 - Packaged smoke verdicts

**Status:** Complete
**Owner:** `worker-m` (`ses_fb9b9b817ffePBOyx4r0X84L5S`)
**Completed:** 2026-08-28
**Implemented:**
- Replaced the generic pre-verdict failure code with bounded runtime, SQLite-connection, migration, deterministic-setup, and measurement stage codes.
- Mapped every diagnostic and package-harness code in the active MSI/NSIS workflow gate without relying on output from the GUI-subsystem executable.
- Removed executable-path and diagnostic-output printing from the installer smoke and formatted the R18 imports.
**Implementation:**
- Files: `.github/workflows/build-windows.yml`, `frontend/src-tauri/src/lib.rs`, this file.
- Approach: discard underlying errors at the process boundary and transport only a stable failure-stage enum through the exit code. Code `1` remains a workflow harness failure; codes `0` and `2` retain exact and unavailable semantics; codes `3` through `7` identify the probe stage.
**Not implemented:**
- No dependency, console-attachment behavior, migration, public API, user-database access, or installed-package success claim.
**Why not implemented:**
- Exit codes are the only diagnostic channel available to the packaged GUI binary. A console attachment would add an unapproved direct platform dependency and is unnecessary for bounded CI diagnosis.
**Verification:**
- `$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"; cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib dbstat_smoke -- --nocapture` - pass: 4 passed.
- `$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"; cargo run --manifest-path "frontend/src-tauri/Cargo.toml" -- --smoke-dbstat` - pass: `smoke-dbstat: status=exact bytes=69632`.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml" --lib --tests` - pass; pre-existing `retrieval/model.rs` unnecessary-parentheses warning remains.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- System Python YAML parsing plus a static assertion for codes `0` through `7` and smoke-before-upload ordering - pass.
- `git diff --check` - pass.
**Rollback:**
- Revert the bounded stage enum, exit-code mapping, workflow text, and focused tests. Semantic activation remains fail-closed when `dbstat` is unavailable.
**Decisions and follow-ups:**
- Actual MSI and NSIS execution remains `2C.4`; a local debug diagnostic cannot establish packaged release coverage. An unrelated notes execution record changed concurrently and is preserved without treating it as Sprint 2C authority.

### 2C.3 - Full-application activation-envelope calibration

**Status:** Blocked
**Owner:** `worker-l` (`ses_fb8d8df1bffe2ouL0jeBNA5j6u`)
**Recorded:** 2026-08-28
**Implemented:**
- Removed the timed-out partial calibration module, CLI path, startup branches, gate telemetry, and all calibration bypasses. The normal startup path and `ACTIVATION_RAM_CEILING_BYTES = 1,395,864,371` are restored unchanged.
**Implementation:**
- Files: removed incomplete changes from `frontend/src-tauri/src/retrieval/calibration.rs`, `retrieval/index.rs`, `retrieval/mod.rs`, `lib.rs`, and `main.rs`; this file.
- Approach: preserve the existing fail-closed R12/R13 authority rather than shipping a calibration path that could accept unresident components, lose a nonzero exit, access user state, or derive a ceiling from an invalid measurement.
**Not implemented:**
- No full-application calibration harness, RAM-ceiling change, architecture amendment, production gate change, package smoke claim, or external model provisioning.
**Why not implemented:**
- On this host, loading the real audio stack requires opening live WASAPI/CPAL streams and contending with user audio; the approved Whisper artifact is absent; and the codebase has no production-path WebView-residency proof. A safe release run also needs hermetic temporary app/model/database roots, atomic report retention, phase deadlines and cleanup, exact component-residency checks, and gate-current-RSS telemetry. The incomplete implementation could not truthfully establish those conditions.
**Verification:**
- `$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"; cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib` - pass: 584 passed, 2 ignored.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml" --lib --tests` - pass; pre-existing `retrieval/model.rs` unnecessary-parentheses warning remains.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the existing workflow LF-to-CRLF notice remains.
- Source search confirms no calibration CLI, environment bypass, or revised ceiling remains in the restored tree.
**Rollback:**
- None needed: no production calibration or ceiling change shipped. The prior fail-closed gate remains authoritative.
**Decisions and follow-ups:**
- Qualifying external evidence requires a release build with the approved staged retrieval bundle and explicit Whisper loadout, no production instance, a hermetic temporary app-data/model/database root, actual component-residency checks, atomic report persistence, and a current-RSS sample at active+shadow coexistence. It must not bypass the production gate or derive a ceiling until the evidence is independently reviewed.
- `2C.5` remains blocked by this task. `2C.4` may establish independent installed-`dbstat` evidence now and must rerun after any future R13 source change.

### 2C.4 - Installed package evidence

**Status:** Complete
**Owner:** Main agent, under user delegation
**Completed:** 2026-08-28
**Implemented:**
- Repaired the fresh-checkout compile-time manifest probe to use the checked-in
  production manifest rather than the later-staged bundle directory.
- Corrected Windows package discovery to use Tauri's actual
  `upstream/frontend/target/<profile>` output directory.
- Made installer smoke verdict handoff reliable for GUI-subsystem binaries by
  collecting explicit `Start-Process -Wait -PassThru` exit codes and persisting
  bounded results for the final workflow gate.
**Implementation:**
- Files: `.github/workflows/build-windows.yml`,
  `frontend/src-tauri/src/retrieval/chunking.rs`,
  `frontend/src-tauri/src/retrieval/model.rs`, this file.
- Approach: package the release first, silently install each real installer,
  invoke the installed executable's non-GUI diagnostic, silently uninstall and
  clean up, then let one final gate map only its bounded exit code.
**Not implemented:**
- No R13 calibration, RAM-ceiling change, model/chunk/encoding/backend change,
  package success bypass, or user-database diagnostic output.
**Why not implemented:**
- Installed-package `dbstat` evidence is independent of the unavailable R13
  calibration environment and must not alter that activation-gate authority.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib dbstat_smoke`
  - pass: 4 passed.
- Root Windows workflow run `41` on commit `7f11887f730f9480b78254d33e2847d27b08f2c4`
  - pass: [Build Windows (CI)](https://github.com/armandotreuk/Meetly_Treuk/actions/runs/33171251338).
- The successful run completed the checked-in manifest contract, staged-bundle
  verification, reference inference, release build, installed MSI smoke,
  installed NSIS smoke, final bounded-verdict gate, MSI upload, NSIS upload,
  executable upload, and Cargo Check (Windows), with no smoke skipped.
**Rollback:**
- Revert the package-evidence workflow and manifest-path changes together. The
  semantic activation gate remains fail-closed when `dbstat` evidence is absent.
**Decisions and follow-ups:**
- Any future R13 source change must repeat this same installed-package evidence
  before `2C.5` can seek Sprint closure.

## Sprint Reviews

<!-- Append fresh code and architecture review results after task verification. -->
