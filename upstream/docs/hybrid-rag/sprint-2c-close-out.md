# Sprint 2C: Close-Out Evidence And Envelope Decision

## Status

Closed with user approval, 2026-08-29. Tasks `2C.1`, `2C.2`, `2C.R1`, `2C.6`,
`2C.R2`, `2C.R3`, `2C.R4`, and `2C.R5` are complete. `2C.3` is **closed as
accepted-with-obligation**, not delivered: the user accepted the existing
whole-process gate for Sprint 2 with the ceiling unchanged, and moved
full-application calibration into Sprint 3 as a close obligation recorded in
`architecture.md`. Run `43` establishes package evidence, but its Cargo Check
result was not persisted for the final gate. Current-head `2C.4` evidence is
complete through run `45`, including the correctly gated Cargo Check. Fresh
code and architecture reviews approve, and the user approved Sprint closure
through `2C.5`.

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

The Owner column carries each task's current state. The `## Task Execution Log` below is authoritative for what was actually done; this table is a summary and
must be updated whenever a log entry is appended.

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 2C.1 | Deletion reconciliation | Add the forward-only `(meeting_id, generation_id)` index and query-plan regression for affected-generation discovery. Preserve the bounded decrement and post-commit read-only reconciliation. | M, high risk | `worker-m` (complete) | Sprint PRD and migration-risk approval | Upgrade and fresh-migration tests prove the index exists; `EXPLAIN QUERY PLAN` shows an indexed meeting lookup rather than a scan; affected-generation deletion regression passes. | Revert the new migration and lookup regression; existing deletion semantics remain intact but the debug lookup scan returns. |
| 2C.2 | Packaged smoke diagnostics | Give every safe probe failure stage a bounded verdict that survives the GUI-subsystem executable; map it in the active root workflow; apply rustfmt. | M | `worker-m` (complete) | 2C.1 complete | Focused status/exit-code tests cover exact, unavailable, and every failure stage; release diagnostic exits correctly; workflow YAML/order checks and `cargo fmt --check` pass. | Revert the exit-code/workflow mapping and tests; `dbstat` activation remains fail-closed. |
| 2C.D1 | Envelope decision | Select one R13 resolution contract: full-application calibrated whole-process ceiling, runtime attribution infrastructure, upstream ORT API wait, sessions-excluded accounting, or an explicitly accepted conservative bound. | Decision | Main agent (complete) | None | Full-application calibrated whole-process scope is recorded with its ceiling consequences and rejected alternatives. | Keep R13 blocked; do not implement an unapproved approximation. |
| 2C.3 | Activation envelope | Calibrate the existing whole-process gate against the production application and revise its ceiling only from recorded full-application evidence. | L, high risk | `worker-l` (closed: accepted with Sprint 3 obligation) | 2C.D1 complete | The gate measures the documented full-application quantity, fails closed on unavailable terms, passes its agreed benchmark, and does not alter model/chunk/encoding/backend contracts without separate approval. | Revert the R13 implementation and architecture amendment; retain the current R12 fail-closed gate. |
| 2C.4 | Installed package evidence | Dispatch the root Windows workflow for the accepted 2C.1/2C.2 branch head and record actual MSI and NSIS silent-install, diagnostic, uninstall, cleanup, and pre-upload-gate outcomes. Rerun after any later R13 source change. | M | Main agent (complete; current-head re-dispatch complete) | 2C.2 complete | Both installed `meetily.exe --smoke-dbstat` runs pass; job summary records them; no smoke is skipped; CI URL and immutable addendum are recorded. | Revert only the diagnostic CI assertion if it prevents emergency packaging; semantic activation remains fail-closed without `dbstat`. |
| 2C.R1 | Review remediation | Apply the Final Code Review (R15) findings across the packaged smoke workflow, the diagnostic, the migration regression, and the three normative documents. | M | Main agent (complete) | 2C.4 complete | Full Rust suite, rustfmt, diff check, workflow YAML and PowerShell parse all pass; no finding left unaddressed or undocumented. | Revert the R15 commit; the prior behaviour returns along with its eleven findings. |
| 2C.6 | Activation observability | Surface the activation-refusal reason through the application log so a refused activation is distinguishable from a normal run. Read-only: no gate, ceiling, or measurement change. | S | Main agent (complete) | 2C.R1 complete | A refused activation emits one warn-level log line naming scope, measured bytes and ceiling; an admitted activation emits none; a test covers both; no meeting text, token, or vector appears in the line. | Revert the logging call; the gate is unchanged either way. |
| 2C.R2 | Review remediation | Make MSI/NSIS teardown exception-safe so every smoke persists its captured diagnostic verdict after an uninstall-launch failure. | S | Main agent (complete) | Final Code Review R11 | A teardown exception does not bypass residue checks or result persistence; nonzero diagnostic verdicts remain unchanged; a passing verdict downgrades to harness `1`. | Revert the nested teardown catches; installer diagnostics remain fail-closed. |
| 2C.R3 | Review remediation | Correct normative activation-observability text and capture warn emission in the refusal/admission regression. | S | Main agent (complete) | Final Code Review R11 | Architecture matches the status/log path; the test observes one exact RAM refusal line and no admitted line. | Revert the documentation correction and test logger; production activation behavior is unchanged. |
| 2C.R4 | CI evidence remediation | Persist a nonzero Cargo Check result despite GitHub Bash's default `errexit`, then fail the job through the existing final gate. | S | Main agent (complete) | Closure Architecture Review | A failed workspace check writes its output and exit code; the final gate fails rather than skips. | Revert the `errexit` guard; Cargo Check may again report false green. |
| 2C.R5 | Workspace-check sidecar staging | Build and stage the Tauri-required `llama-helper` sidecar before workspace Cargo Check. | S | Main agent (complete) | 2C.R4 evidence | The Cargo Check job reaches the workspace check with the target-qualified helper present and remains fail-closed on a helper-build failure. | Revert the staging step; Cargo Check again fails before Rust analysis begins. |
| 2C.5 | Sprint closure | Run final verification, append fresh code and architecture reviews, record deferrals, and request user Sprint 2 close approval. | M | Main agent (complete; user approved) | 2C.R2/R3/R4/R5 complete; 2C.4 re-dispatch complete; fresh code and architecture reviews approve | Full Rust suite, `cargo check`, rustfmt, diff check, typecheck, Vitest, and 250k benchmark pass; both reviews approve; user approved close. | Reopen only for a new post-close defect; preserve the accepted R13 Sprint 3 obligation. |

## Dependency Order

`2C.1 -> 2C.2 -> 2C.4 -> 2C.R1 -> 2C.6 -> 2C.R2/R3 -> 2C.R4 -> 2C.R5 -> 2C.4(re-dispatched)`;
`2C.D1 -> 2C.3` (closed by decision); `2C.4(re-dispatched) -> 2C.5`

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
| 2026-08-28 | Accept the whole-process activation gate as-is for Sprint 2; close `2C.3` as accepted-with-obligation rather than delivered; move full-application calibration into Sprint 3. | Both remedies were proven unavailable, not deferred: `2.R13` proved a retrieval-scoped gate unimplementable against ORT 1.22 as bound, and `2C.3` proved full-application calibration unproducible on this host. The mismatch errs fail-safe and semantic retrieval has no query surface in Sprint 2, so no user can reach the defect. Sprint 3 is the first build where a real loaded application runs a semantic query, making it a better calibration site than any synthetic harness. Recorded in `architecture.md` "Close-out acceptance and its Sprint 3 obligation". | Build a CI calibration rig (a cloud runner is not a user machine and yields weaker evidence); wait for an upstream ORT memory API (no timeline); snapshots-only accounting (omits the dominant term); a fixed conservative session bound (a guess that can undercount). | User |
| 2026-08-28 | Require activation-refusal observability before the release that first exposes semantic retrieval; add it as `2C.6`. | The refusal reason is stored in `pending_blockers` and read by nothing. Without it, the Sprint 3 calibration obligation is unmeasurable and a permanently-refusing gate is indistinguishable from a working one. | Ship the accepted gate with no observability and rely on Sprint 3 dogfooding to notice. | User |
| 2026-08-28 | Retain the `retrieval_documents(meeting_id, generation_id)` index; revisit at Sprint 3 close. | Measured on SQLite 3.49.1, the release deletion decrement is a covering-index search on the pre-existing index with or without the new one, so release builds carry it unused. Removal costs a third forward-only migration where two of three prior migration incidents broke startup, and restores the scan `2C.1` removed. Bounded waste beats an unbounded migration risk. | Drop the index via a new migration and re-index only the test fixture. | User |
| 2026-08-28 | Accept the `2C.R1` review remediation and require `2C.4` to be re-dispatched against the resulting head. | `2C.R1` changed the MSI and NSIS smoke steps, so the run `41` evidence no longer covers the shipped workflow. The sprint's own rule forbids claiming installer coverage without a real run of the current source. | Accept run `41` as still-valid evidence for changed workflow logic. | User |

## Remaining Task Specs

### 2C.6 - Activation refusal observability [S]

**Outcome.** When the activation gate refuses to switch on a semantic
generation, the reason reaches the application log. A refused activation and a
normal run stop being indistinguishable.

**Why this is required.** The user accepted a knowingly conservative activation
gate for Sprint 2 (see the decision log and `architecture.md` "Close-out
acceptance and its Sprint 3 obligation"). That acceptance is conditional on
being able to observe the gate in the field: Sprint 3 owes a real refusal rate,
and a gate that never admits is a defect even though nothing crashes. Today
`retrieval/index.rs:1630` stores every blocker in `pending_blockers` and
`pending_activation_blockers()` at `:748` is called by nothing - not a Tauri
command, not a log line, not a test outside the module. Successful activation
already logs at `retrieval/index.rs:1622`; only the failure path is silent.

**Likely touchpoints.**
- `frontend/src-tauri/src/retrieval/index.rs` - the activation loop that ends
  at `set_pending_blockers`, around `:1474`-`:1631`.

**Required implementation.**
- Emit exactly one `log::warn!` when the activation pass ends with a non-empty
  blocker set, naming every blocker it collected. Do not log per iteration: the
  loop can reject several candidate generations in one pass and a per-candidate
  line would be noise.
- Emit nothing when `reported_blockers` is empty. Successful activation is
  already logged at `:1622` and must not gain a second line.
- Cover every blocker source, not only the RAM gate: coverage blockers
  (`:1503`, `:1596`), model mismatch (`:1483`, `:1545`), the RAM gate
  (`:1571`), and the derived-disk gate (`:1607`) all reach the same set and all
  produce a silently non-activating index.
- Do not change `ram_gate_blocker`, `ACTIVATION_RAM_CEILING_BYTES`,
  `ACTIVATION_RAM_SCOPE`, the measurement, the gate's admit/refuse decision, or
  any blocker string. This task is read-only with respect to behaviour.
- Do not add a Tauri command or any UI surface. Sprint 3 owns query surfaces.

**Acceptance criteria.**
- A refused activation produces one warn-level line carrying the scope, the
  measured byte value, and the ceiling for a RAM refusal, and the equivalent
  detail for the other blocker kinds.
- An admitted activation produces no new line.
- The blocker strings already contain only generation ids, model ids, byte
  counts, and revision numbers. Confirm no meeting text, token, or vector can
  reach the line, and state how you confirmed it.

**Required verification.**
- A test asserting the refusal path populates the blockers it logs, and a test
  asserting the admitted path leaves the set empty. Prefer extending the
  existing `ram_gate_admits_below_and_blocks_at_above_or_unavailable` coverage
  over inventing a new fixture.
- Full `cargo test --lib`, `cargo fmt --check`, `git diff --check`.

**Worker report additions.**
- Quote the exact log line produced for a RAM refusal.
- State whether `pending_activation_blockers()` remains uncalled, and if you
  removed it, say so.

**Rollback.** Revert the logging call. The gate behaves identically either way.

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

### 2C.4 - Current-head package evidence addendum

**Status:** Complete
**Owner:** Main agent, under user delegation
**Completed:** 2026-08-28
**Implemented:**
- Re-dispatched the root Windows workflow after `2C.R1`, `2C.6`, `2C.R2`, and
  `2C.R3` changed package smoke and close-out source.
**Implementation:**
- Files: no source change; this file.
- Approach: dispatch the active root workflow with its release input against the
  pushed final package source rather than treating the prior successful run as
  coverage for changed scripts.
**Not implemented:**
- No package, diagnostic, gate, ceiling, model, or migration change.
**Why not implemented:**
- Run `42` covers the prior head only. Current workflow evidence is an execution
  requirement, not a reason to change product behavior.
**Verification:**
- Root Windows workflow run `43` on commit
  `25f64c012a0268a6e7aea525d8cdc85fe35bcbc1`
  - pass: [Build Windows (CI)](https://github.com/armandotreuk/Meetly_Treuk/actions/runs/33215212797).
- The completed run passed the checked-in manifest and staged-bundle contracts,
  reference inference, release build, installed MSI smoke, installed NSIS smoke,
  final bounded-verdict gate, and package uploads. Its workspace Cargo Check
  exited nonzero, but Bash `errexit` prevented the step from exporting that
  result; the final Cargo Check gate skipped and the green job does not certify
  that check.
**Rollback:**
- No source rollback applies. A later package-source change requires a new
  dispatch before release evidence can be claimed.
**Decisions and follow-ups:**
- `2C.4` must be re-dispatched after `2C.R4` before it can be considered a
  Sprint closure dependency. Fresh approving reviews and user Sprint-close
  approval then remain required by `2C.5`.

### 2C.R1 - Final Code Review (R15) remediation

**Status:** Complete
**Owner:** Main agent, under user delegation (session `e762b0ec-7ac3-454d-a05c-49ad443b817d`)
**Completed:** 2026-08-28
**Implemented:**
- Closed ten of the eleven Final Code Review (R15) findings below; the
  eleventh is recorded as a user decision, not an omission.
- Stopped the packaged smoke teardown from overwriting a diagnostic verdict. A
  teardown problem now downgrades only a passing run to harness code `1`; a
  verdict the diagnostic already returned survives. Exit `2` - the one
  condition this smoke exists to detect - can no longer be relabelled as a
  flaky installer.
- Extended the MSI residue check to every root the executable search covers, so
  a Program Files install that fails to uninstall is no longer masked by the
  empty temporary directory the step creates for itself.
- Bounded the diagnostic invocation with an explicit 120-second wait and kill
  path, replacing an unbounded wait that could hold a runner until the job
  timeout.
- Reduced the caught-exception log line to the exception class, removing the
  absolute paths PowerShell embeds in exception messages.
- Preserved the underlying `sqlx::Error` behind `#[cfg(debug_assertions)]`
  instead of discarding it at the point of failure, so a local run still names
  what broke while the packaged verdict stays a bare stage code.
- Replaced a tautological assertion over two constants with a loop that
  actually proves `unavailable` never collapses into any pre-verdict stage.
- Narrowed the query-plan regression to the index name, dropping a match on
  `EXPLAIN QUERY PLAN` prose that SQLite does not treat as a stable interface.
- Brought three normative documents back in line with the shipped contract:
  the exit-code table in `architecture.md`, the self-contradicting task table
  in this file, and the eight-commit-stale work order in the agent brief.
**Implementation:**
- Files: `.github/workflows/build-windows.yml`,
  `frontend/src-tauri/src/lib.rs`,
  `frontend/src-tauri/src/database/migration_tests.rs`,
  `docs/hybrid-rag/architecture.md`,
  `docs/hybrid-rag/sprint2agentbrief-V1.0.md`, this file.
- Approach: hoist the installer search roots to step scope so teardown can
  re-scan the same roots it searched; gate every teardown downgrade behind an
  exit code of zero; a generic stage mapper that logs in debug builds and
  returns the bounded enum in all builds.
**Not implemented:**
- Finding 5, the `retrieval_documents(meeting_id, generation_id)` index. The
  finding is correct and was verified by running both query plans through
  SQLite 3.49.1: the release-path decrement is a covering-index search on the
  pre-existing index either way, so release builds carry the new index unused.
- No change to `ACTIVATION_RAM_CEILING_BYTES`, the gate, the approved
  model/chunk/encoding/backend contracts, or any migration.
**Why not implemented:**
- Removing the index needs a third forward-only migration in a codebase where
  two of three migration incidents broke application startup, and it restores
  the write-lock scan `2C.1` was commissioned to remove. The user accepted the
  bounded cost and deferred the question to Sprint 3 close, when the added
  query surfaces make it decidable on evidence. Recorded in the decision log
  above and in `architecture.md`.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib` - pass:
  584 passed, 0 failed, 2 ignored; identical to the pre-change baseline.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; the pre-existing workflow CRLF notice remains.
- `cargo run --manifest-path "frontend/src-tauri/Cargo.toml" -- --smoke-dbstat`
  - pass: `smoke-dbstat: status=exact bytes=69632`.
- Workflow YAML parses; both smoke steps parse under the PowerShell language
  parser; smoke steps still precede the upload steps. The final gate's residual
  parse errors are its GitHub Actions expression placeholders, which Actions
  substitutes before PowerShell runs.
- Teardown verdict precedence simulated across six cases, including the
  finding-1 scenario (dbstat unavailable plus failed uninstall), which now
  persists `2` rather than collapsing to `1`.
**Rollback:**
- Revert the `2C.R1` commit. The prior behaviour returns together with all
  eleven findings; no persisted data, schema, or model contract is involved.
**Decisions and follow-ups:**
- The MSI and NSIS smoke steps changed, so run `41` no longer covers the
  shipped workflow. `2C.4` MUST be re-dispatched against this head before
  `2C.5`; the root workflow does not auto-run on this branch and needs a manual
  `workflow_dispatch`.
- `2C.6` is now dependency-ready and is the last implementation task before
  sprint close.

### 2C.6 - Activation refusal observability

**Status:** Complete
**Owner:** Main agent, under user delegation
**Completed:** 2026-08-28
**Implemented:**
- Added one `warn`-level line after an activation pass collects one or more
  blockers. It reports the complete safe blocker set once, rather than one line
  per rejected candidate.
- Extended the measured RAM-gate integration test to assert the refusal payload
  contains the scope, ceiling, and measured value, and that a later admitted
  activation leaves no blockers.
**Implementation:**
- Files: `frontend/src-tauri/src/retrieval/index.rs`, this file.
- Approach: log the existing `reported_blockers` collection immediately before
  persisting it to `pending_blockers`; all coverage, model, RAM, and disk
  blockers already enter this same collection.
**Not implemented:**
- No gate, ceiling, measurement, blocker string, Tauri command, status-API, or
  UI change.
**Why not implemented:**
- This task makes the accepted fail-closed state observable only. Sprint 3 owns
  the full-application calibration and refusal-rate obligation.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib measured_ram_gate_blocks_activation_until_measurement_admits -- --nocapture`
  - pass: 1 passed.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib`
  - pass: 584 passed, 2 ignored.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` and
  `git diff --check`
  - pass.
- The exact RAM-refusal line at the approved ceiling is
  `Semantic generation activation refused: generation gen-ram: measured whole-process RSS 1395864371 bytes meets or exceeds the 1395864371 byte activation ceiling`.
- Existing blocker construction carries only generation/model IDs, counts,
  revisions, measurements, and fixed gate text; no meeting text, token, or
  vector enters `reported_blockers`. `pending_activation_blockers()` already
  remains used by `index_status`; this task did not add a new consumer.
**Rollback:**
- Revert the logging call. Activation admission and refusal behavior remain
  unchanged.
**Decisions and follow-ups:**
- Re-dispatch `2C.4` against the head containing this task before `2C.5`; the
  root workflow is manually dispatched for this branch.

### 2C.R2 - Exception-safe package smoke teardown

**Status:** Complete
**Owner:** Main agent, under user delegation
**Completed:** 2026-08-28
**Implemented:**
- Wrapped the MSI and NSIS uninstall process launch in local teardown catches.
  An exception now marks teardown failed without skipping cleanup, residue
  checks, or result-file persistence.
**Implementation:**
- Files: `.github/workflows/build-windows.yml`, this file.
- Approach: retain a captured nonzero diagnostic code; only a teardown problem
  after an otherwise passing diagnostic changes `0` to harness code `1`.
**Not implemented:**
- No installer path, diagnostic exit-code contract, database diagnostic,
  activation gate, or package-success bypass change.
**Why not implemented:**
- The review found `finally` could exit before persisting a safe diagnostic
  verdict when `Start-Process` itself failed. The local catches are the smallest
  correction that preserves both verdict precedence and subsequent cleanup.
**Verification:**
- `npx --yes yaml-lint ".github/workflows/build-windows.yml"`
  - pass.
- Full current-head MSI/NSIS evidence is re-dispatched as `2C.4` after this
  source change; no local static check claims installed-package coverage.
**Rollback:**
- Revert the two local teardown catches. Diagnostic execution remains
  fail-closed, but an uninstall-launch exception again bypasses persistence.
**Decisions and follow-ups:**
- `2C.4` must execute against this corrected workflow before closure.

### 2C.R3 - Activation observability review remediation

**Status:** Complete
**Owner:** Main agent, under user delegation
**Completed:** 2026-08-28
**Implemented:**
- Corrected the normative architecture text to describe the existing status
  reader and one warn-level refusal emission.
- Added a test-only logger capture that verifies one exact `gen-ram` RAM
  refusal line and no line after admitted activation.
**Implementation:**
- Files: `docs/hybrid-rag/architecture.md`, `frontend/src-tauri/src/retrieval/index.rs`, this file.
- Approach: capture only the deterministic test generation's fixed safe message,
  so parallel tests cannot contribute unrelated activation logs.
**Not implemented:**
- No production logging configuration, gate, ceiling, measurement, blocker,
  status-API, Tauri command, or UI change.
**Why not implemented:**
- The review required a runnable assertion for the only new observable behavior,
  not another production logging abstraction.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib measured_ram_gate_blocks_activation_until_measurement_admits -- --nocapture`
  - pass: 1 passed.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib`
  - pass: 584 passed, 2 ignored.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` and
  `git diff --check`
  - pass.
**Rollback:**
- Revert the architecture correction and test-only logger capture. The
  production refusal line remains unchanged.
**Decisions and follow-ups:**
- Fresh code and architecture reviews follow the re-dispatched `2C.4` evidence.

### 2C.R4 - Cargo Check verdict persistence

**Status:** Complete
**Owner:** Main agent, under user delegation
**Completed:** 2026-08-28
**Implemented:**
- Disabled Bash `errexit` only while workspace Cargo Check runs, then restored
  it after capturing and exporting Cargo's exit code.
**Implementation:**
- Files: `.github/workflows/build-windows.yml`, this file.
- Approach: retain the existing output artifact and final failure gate; prevent
  the intermediate script from exiting before either receives Cargo's verdict.
**Not implemented:**
- No Cargo command, dependency, Rust toolchain, package diagnostic, installer
  smoke, gate, ceiling, model, schema, or source-code change.
**Why not implemented:**
- In run `43`, workspace Cargo Check exited nonzero, but GitHub Bash's default
  `-e` stopped the script before it wrote `cargo_check_exit_code`. The final
  gate therefore skipped. Package-smoke evidence remains valid, but its green
  job conclusion cannot certify Cargo Check.
**Verification:**
- `cargo check --workspace --message-format=short`
  - pass locally.
- `npx --yes yaml-lint ".github/workflows/build-windows.yml"`
  - pass.
- `git diff --check -- .github/workflows/build-windows.yml upstream/docs/hybrid-rag/sprint-2c-close-out.md`
  - pass.
- The root workflow must be re-dispatched after this workflow-source change;
  its final gate is the CI proof that a remote check failure is surfaced.
**Rollback:**
- Revert the `set +e` / `set -e` pair. Cargo Check would again report false
  green when the check fails before its result is exported.
**Decisions and follow-ups:**
- `2C.4` is pending re-dispatch. Do not claim Cargo Check evidence from run
  `43`.

### 2C.R5 - Workspace-check sidecar staging

**Status:** Complete
**Owner:** Main agent, under user delegation
**Completed:** 2026-08-29
**Implemented:**
- Built the debug `llama-helper` sidecar and copied it to Tauri's required
  target-qualified external-binary path before workspace Cargo Check.
**Implementation:**
- Files: `.github/workflows/build-windows.yml`, this file.
- Approach: reuse the package job's existing helper build and staging contract
  in the independent check job, with its own `target` directory.
**Not implemented:**
- No Tauri configuration, Rust code, Cargo package selection, dependency,
  diagnostic, installer smoke, activation gate, ceiling, model, or schema
  change.
**Why not implemented:**
- Run `44` correctly surfaced that Cargo Check's Tauri build script requires
  `binaries/llama-helper-x86_64-pc-windows-msvc.exe`, but the check job had not
  created it. Removing `meetily` from workspace checking would hide the real
  package contract instead of satisfying it.
**Verification:**
- Root Windows workflow run `44` on commit
  `8e820124d0aa96bf16fad4ba76a9f0d5e8a98e70`
  - [Cargo Check correctly failed](https://github.com/armandotreuk/Meetly_Treuk/actions/runs/33252380074/job/99100178538): Tauri failed only because the required sidecar path did not exist, and its final failure gate reported the nonzero result as intended.
- `cargo build -p llama-helper`
  - pass locally; produced `debug/llama-helper.exe` in the configured target.
- `npx --yes yaml-lint ".github/workflows/build-windows.yml"` and targeted
  `git diff --check`
  - pass.
- `2C.4` must be re-dispatched after this workflow-source change.
**Rollback:**
- Revert this staging step. Cargo Check will again stop at Tauri's missing
  external-binary validation.
**Decisions and follow-ups:**
- Preserve `cargo check --workspace`; the next root-workflow run must prove
  both the helper-stage prerequisite and final Cargo Check result.

### 2C.4 - Final current-head package evidence addendum

**Status:** Complete
**Owner:** Main agent, under user delegation
**Completed:** 2026-08-29
**Implemented:**
- Re-dispatched the root release workflow after the two Cargo Check remediation
  commits and recorded the resulting package and check-job evidence.
**Implementation:**
- Files: this file.
- Approach: use the active root workflow on the pushed workflow head rather
  than infer package coverage from prior runs or static inspection.
**Not implemented:**
- No package, diagnostic, gate, ceiling, model, migration, or application
  source change.
**Why not implemented:**
- Installed-package and Windows workspace-check evidence must execute against
  the exact workflow source that supplies their prerequisites and final gates.
**Verification:**
- Root Windows workflow run `45` on commit
  `d02f4c834ddf2c0511ef683af51a35ee7325dab7`
  - pass: [Build Windows (CI)](https://github.com/armandotreuk/Meetly_Treuk/actions/runs/33253136355).
- `Cargo Check (Windows)` job passed: the helper-stage and workspace-check steps
  passed; the output uploaded; its final failure gate skipped only because the
  exported exit code was `0`.
- `Build Windows x64 (CPU)` job passed: release build, installed MSI smoke,
  installed NSIS smoke, final smoke gate, and MSI/NSIS/executable uploads all
  passed.
- MSI and NSIS installed `--smoke-dbstat` diagnostics each returned `0`; the
  final gate recorded `MSI packaged dbstat smoke: PASS` and `NSIS packaged
  dbstat smoke: PASS`. Uploaded installer artifact IDs are `9715454735` (MSI)
  and `9715456981` (NSIS).
**Rollback:**
- No source rollback applies. Any later workflow or package-source change must
  repeat this evidence before Sprint closure.
**Decisions and follow-ups:**
- `2C.4` is no longer a Sprint closure dependency. Fresh architecture approval
  and user Sprint-close approval remain required by `2C.5`.

### 2C.5 - Sprint closure

**Status:** Complete
**Owner:** Main agent, under user delegation
**Completed:** 2026-08-29
**Implemented:**
- Recorded the completed verification, current-head package evidence, fresh
  code and architecture approvals, and explicit user Sprint-close approval.
**Implementation:**
- Files: this file.
- Approach: close only after all task acceptance checks, run-45 evidence, and
  independent reviews were recorded; do not reinterpret the accepted R13
  obligation as delivered calibration.
**Not implemented:**
- No R13 calibration, activation-gate or ceiling change, model, chunking,
  vector, backend, package, schema, or product-source change.
**Why not implemented:**
- Sprint 2 closure accepts the existing fail-closed envelope contract and
  carries full-application calibration into Sprint 3 rather than changing it
  without new authority.
**Verification:**
- Run `45` passed both Windows package and Cargo Check jobs, including installed
  MSI/NSIS `--smoke-dbstat` diagnostics, final gates, and uploads.
- Full Rust suite, local workspace Cargo Check, rustfmt, YAML lint, targeted
  diff check, frontend typecheck, Vitest, and the 250k benchmark passed as
  recorded in this close-out record.
- Final code review R13 and architecture review R14 approved with no findings.
- User explicitly approved Sprint 2 closure on 2026-08-29.
**Rollback:**
- Reopen only for a new post-close defect. Retain the accepted R13 Sprint 3
  calibration/refusal-rate close obligation.
**Decisions and follow-ups:**
- Sprint 2 is closed. Sprint 3 remains responsible for full-application R13
  calibration and refusal-rate evidence.

## Sprint Reviews

<!-- Append fresh code and architecture review results after task verification. -->

### Final Code Review (R15)

**Reviewer:** `claude-opus-5` (session `e762b0ec-7ac3-454d-a05c-49ad443b817d`), 2026-08-28
**Scope:** `0fe0442..6f0fd43` - the Sprint 2C range: the packaged smoke workflow
steps and their verdict gate, the `--smoke-dbstat` stage enum, the
`(meeting_id, generation_id)` migration and its query-plan regression, the
manifest-path correction, and both close-out documents.
**Verdict:** Changes requested - ten findings resolved in `2C.R1` above, one
resolved by user decision.

**Findings (severity order):**
1. **Blocker - the packaged smoke teardown overwrites the diagnostic verdict.**
   Setting the harness code on any uninstall or cleanup problem discards a
   verdict the try-block already captured, so exit `2` - dbstat unavailable,
   the one condition the smoke exists to detect - is reported as a flaky
   installer. `.github/workflows/build-windows.yml:344`, and `:421`/`:424` for
   NSIS.
2. **Blocker - `architecture.md` documents only exit codes `0`/`2`/`3`.**
   `2C.2` split `3` into five stage codes and the workflow maps `1`, so the
   source of truth contradicts both the shipped binary and its gate.
   `docs/hybrid-rag/architecture.md:1971`.
3. **Blocker - the agent brief is eight commits stale and re-orders shipped
   work.** It reports the branch unpushed, omits eight commits, and presents
   `5.1` and `5.2` as open when both shipped; a fresh session would
   re-implement the lazy reranker and re-open a resolved envelope blocker.
   `docs/hybrid-rag/sprint2agentbrief-V1.0.md:59`.
4. **Should-fix - the MSI cleanup check ignores the Program Files fallback.**
   The residue test covers only the temporary directory the step created for
   itself, so a failed uninstall from the fallback location reports PASS.
   `.github/workflows/build-windows.yml:315`.
5. **Should-fix - the new index serves only a `debug_assertions` query.**
   Verified against SQLite 3.49.1: the release decrement is a covering-index
   search on the pre-existing index either way, so release builds maintain a
   second B-tree nothing reads.
   `migrations/20260827030000_add_retrieval_documents_meeting_lookup_index.sql:1`.
6. **Should-fix - the underlying error is discarded at its source.** The unit
   mapping destroys the `sqlx::Error` before any build can print it, including
   debug runs that have a console. `frontend/src-tauri/src/lib.rs:159`.
7. **Should-fix - a test is named for a separation it never exercises.** Its
   second assertion compares two integer literals and calls no function under
   test. `frontend/src-tauri/src/lib.rs:1126`.
8. **Should-fix - the task table contradicts the execution log.** Owner cells
   read "Pending" for tasks the log records Complete, in a document that is the
   normal work order for a fresh worker.
   `docs/hybrid-rag/sprint-2c-close-out.md:91`.
9. Correctness (minor) - the query-plan regression matches
   `EXPLAIN QUERY PLAN` prose, which SQLite does not treat as a stable
   interface. `frontend/src-tauri/src/database/migration_tests.rs:68`.
10. Conventions - the catch handler prints the raw exception message,
    reintroducing the absolute paths the same commit removed and that this
    document's risk register forbids.
    `.github/workflows/build-windows.yml:338`.
11. Correctness (minor) - the diagnostic wait cannot be bounded, so a hung
    diagnostic holds the runner until the job timeout rather than returning a
    verdict. `.github/workflows/build-windows.yml:327`.

**Verification:** `cargo test --lib` (584 passed, 2 ignored), `cargo fmt
--check`, and `git diff --check` all passed on the reviewed tree before any
change, so every finding is a latent defect rather than a broken build. The
250k production benchmark and the frontend suites were not re-run for this
review; they belong to `2C.5`.

**Required follow-ups:** all addressed in `2C.R1`, except finding 5, which the
user resolved by decision (retain the index, revisit at Sprint 3 close) and
which is recorded in the decision log above and in `architecture.md`.

### Final Code Review (R11)

**Reviewer:** `openai/gpt-5.6-sol`, 2026-08-28
**Scope:** `0fe0442..51e9c7c` - Sprint 2C close-out through `2C.6`.
**Verdict:** Changes requested - resolved by `2C.R2` and `2C.R3` above.

**Findings (severity order):**
1. **Blocker - installer teardown was not exception-safe.** An uninstall
   `Start-Process` exception could escape `finally` before a captured diagnostic
   verdict was persisted, masking code `2` and skipping residue checks.
   `.github/workflows/build-windows.yml:351-383`, `:452-480`.
2. **Should-fix - the normative observability text contradicted the shipped
   status/log behavior.** `architecture.md:1116-1129` still said blockers were
   unread and unlogged.
3. **Should-fix - the `2C.6` regression did not observe the warning side
   effect.** Deleting `log::warn!` left the state-only assertion green.

**Verification:** Focused activation and `dbstat` tests passed. Run `42` passed
the happy teardown path but could not exercise an uninstall-launch exception.
No gate, ceiling, model, schema, raw-content logging, or external-transfer
change was found.

**Required follow-ups:** `2C.R2`, `2C.R3`, then re-dispatch `2C.4` for the
current workflow head before fresh closure reviews.

### Post-remediation Code Review

**Reviewer:** `openai/gpt-5.6-sol`, 2026-08-28
**Scope:** `51e9c7c..25f64c0` - R11 remediation.
**Verdict:** Approve.

**Findings:** None. The review confirmed uninstall-launch exceptions preserve
an existing diagnostic code and continue cleanup/residue checks; the refusal
line remains safe and gate-read-only; and the test logger is one-time,
mutex-protected, and filtered to the unique test generation.

**Verification:** Run `43` passed on `25f64c012a0268a6e7aea525d8cdc85fe35bcbc1`.
Focused activation coverage, full Rust tests, and diff validation passed.

### Closure Architecture Review

**Reviewer:** `openai/gpt-5.6-sol`, 2026-08-28
**Scope:** Sprint 2C authority and final remediation head `25f64c0`.
**Verdict:** Changes requested - documentation-only finding.

**Findings:** The authoritative close-out record still treated `2C.4`
re-dispatch as outstanding and recorded only obsolete run `41`, despite current
head run `43` having passed MSI/NSIS smoke, final verdict gate, package uploads,
and Cargo Check. No architecture or source-code finding was identified.

**Required follow-ups:** This immutable run-43 addendum and task-summary update,
then a fresh architecture review.

### Closure Architecture Re-Review

**Reviewer:** `openai/gpt-5.6-sol`, 2026-08-28
**Scope:** Current close-out head after the run-43 addendum.
**Verdict:** Changes requested - resolved by `2C.R4` above; fresh approval
remains pending CI evidence.

**Findings:** Run `43`'s workspace Cargo Check exited `101`, but GitHub Bash
ran with `-e` and stopped the script before `cargo_check_exit_code` was
exported. The final failure gate skipped, so the workflow's green conclusion
cannot be used as Cargo Check evidence. The run-43 addendum also incorrectly
claimed that check passed.

**Required follow-ups:** Re-dispatch `2C.4` after `2C.R4`; record the actual
Cargo Check result and obtain fresh code and architecture approvals before
requesting Sprint-close approval.

### Final Code Review (R13)

**Reviewer:** `openai/gpt-5.6-sol`, 2026-08-29
**Scope:** `25f64c0..d02f4c8` - Cargo Check result persistence and workspace
sidecar staging.
**Verdict:** Approve.

**Findings:** None. The check job exports Cargo's result despite Bash `-e`,
keeps the separate nonzero final gate authoritative, and builds the
target-qualified sidecar Tauri requires before checking the whole workspace.
The new prerequisite is fail-closed on a helper build, directory, or copy
failure; no package, gate, model, schema, secret, or product-code contract
changed.

**Verification:** Run `44` exercised the intended nonzero gate; run `45` then
passed the helper stage, workspace Cargo Check, and output upload, with the
final Cargo Check failure step skipped only on exit `0`. YAML lint and targeted
diff validation passed. The subsequent run-45 package job also passed, as
recorded in the current-head evidence addendum above.

### Final Architecture Review (R14)

**Reviewer:** `openai/gpt-5.6-sol`, 2026-08-29
**Scope:** `25f64c0..2d6f8a4` - final Cargo Check remediations and complete
Sprint 2C closure record.
**Verdict:** Approve.

**Findings:** None. The remediation is confined to the active root Windows
workflow and its close-out record. R13 remains accepted with its Sprint 3
calibration obligation, and the fixed whole-process ceiling, fail-closed gate,
approved backend, model, chunking, and vector-encoding contracts are unchanged.
The workspace check retains all packages, stages Tauri's required helper, and
persists nonzero results for its authoritative final gate.

**Verification:** Run `45` accurately covers tested workflow head
`d02f4c834ddf2c0511ef683af51a35ee7325dab7`: Cargo Check passed helper staging,
the workspace check, and output upload; its failure step skipped only at exit
`0`. The package job passed both installed smokes, the final gate, and all
three uploads. Documentation-only head `2d6f8a4` introduces no untested
workflow or product delta.

**Residual evidence note:** Raw Actions logs require authenticated access;
immutable run/job/step results and artifact metadata remain retained and match
the recorded evidence. No architecture or product-contract gap remains.
