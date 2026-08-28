# Handoff — Meetily Hybrid RAG, Sprint 2C close-out

Current as of 2026-08-28, branch head `2C.R1`. Paste everything below the rule
into a fresh worker session. One task per session.

---

You are continuing the Meetily **Hybrid RAG** program at Sprint 2C, the
close-out sprint for Sprint 2 (Durable Local Semantic Index). Read this whole
brief before touching anything.

# 1. Repository layout — this trips people

- Git root is the **parent** of `upstream/`. Source root is `upstream/`.
- Rust crate root: `upstream/frontend/src-tauri/`.
- The only CI workflow GitHub Actions executes is the **root**
  `.github/workflows/build-windows.yml`. Anything under
  `upstream/.github/workflows/` is inert and never runs. Do not "fix" it.
- The repo lives inside OneDrive. Cargo fails with "output path is not a
  writable directory" unless you redirect the target dir first:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
```

- Normative documents, in precedence order:
  1. `upstream/docs/hybrid-rag/architecture.md` — source of truth.
  2. `upstream/docs/hybrid-rag/sprint-2c-close-out.md` — the live sprint
     record. Task specs are under `## Remaining Task Specs`; state is in
     `## Task List` and `## Task Execution Log`.
  3. `upstream/docs/hybrid-rag/sprint-2-durable-local-index.md` — the Sprint 2
     history. Read-only context; do not add tasks there.
- `upstream/docs/hybrid-rag/sprint2agentbrief-V1.0.md` is **superseded** for
  close-out work. Its program rules still hold; its work order does not.

# 2. Standing program rules

- Implementation happens in fresh worker sessions, one task at a time. `L`
  tasks run alone. Tasks carrying a migration run alone.
- Architecture changes, new models, new dependencies, schema semantic changes,
  and runtime downloads require **user approval** before implementation.
- Every completed task appends one immutable entry to `## Task Execution Log`
  using the template at the end of that section. Never rewrite an existing
  entry. Never edit a review record; append new ones.
- **Never edit a committed migration.** `database/manager.rs` runs
  `sqlx::migrate!("./migrations").run(&pool)`, which validates checksums and
  fails startup with `VersionMismatch` if shipped migration text changes. Two
  of the three migration incidents in this project's history were exactly this.
  Add a forward-only migration instead.
- Logs, errors, and status must never carry raw meeting text, tokens, or
  vectors.
- Report the absolute working directory you operate in, and confirm what you
  committed. A previous worker reported completed work that was never written
  to disk and could not be recovered.
- If a gate refuses, report it with its measurement. Do not work around a gate,
  relax a ceiling, or manufacture headroom to make a task pass.

# 3. Where the work stands

Branch `sprint-2/durable-local-index`, off `main`, pushed. `2C.1`, `2C.2`,
`2C.4` and `2C.R1` are complete. `2C.3` is closed by user decision (see §4).

Green on the current head: 584 Rust tests pass (2 ignored), `cargo fmt
--check`, `git diff --check`, and debug `--smoke-dbstat` all pass.

Working-tree changes that are **not yours** and must be preserved untouched:
`upstream/docs/notes-chat-improvement-execution.md` and the `graphify-out/`
directories (tool output; never commit them). Re-check `git status` yourself —
this list drifts.

# 4. Settled decisions — do not re-open

**The activation RAM gate is accepted as-is.** It samples whole-process RSS
against a ceiling derived from retrieval-only arithmetic. This mismatch is
known, recorded, and deliberate. `ACTIVATION_RAM_CEILING_BYTES` stays exactly
`1,395,864,371`. `2.R13` proved a retrieval-scoped gate unimplementable against
ORT 1.22 as bound; `2C.3` proved full-application calibration unproducible on
the development host. Calibration is now a **Sprint 3 close obligation**
recorded in `architecture.md`. If a plan leads you toward changing the ceiling,
widening the gate, or adding a bypass: **stop and report.**

**The `retrieval_documents(meeting_id, generation_id)` index stays.** It serves
only a `debug_assertions` query and release builds carry it unused. That is
accepted; removing it costs a third migration for bounded waste. Revisit at
Sprint 3 close, not now.

**Garbage-collection gate: option A.** A transitional clause in
`architecture.md` "Generation Activation" satisfies the successful-Fast-hybrid-
query condition with one clean restart while no query surface exists. It
expires at Sprint 3 close.

**Prior-model bundle retention is forward-looking and not implementable.** Only
`meetily-retrieval-bundle-1` has ever existed. There is no prior bundle, model
id, artifact set, or hash. If any plan leads you toward authoring one: **stop
and report.** Fabricating it forges a provenance chain that Sprint 1's package
hardening exists to prevent.

# 5. Your task

## Next up: `2C.6` — Activation refusal observability [S]

Full spec: `sprint-2c-close-out.md`, `## Remaining Task Specs`, section
`### 2C.6`. Follow it as written. In one sentence: when the activation gate
refuses, nothing is logged — successful activation logs at
`retrieval/index.rs:1622` but the refusal path only writes to a variable
(`pending_blockers`) that no code reads. Make the refusal reason reach the
application log, and change nothing else.

This is deliberately small and read-only with respect to behaviour. Do not
widen it into gate work, a Tauri command, or a UI surface.

## Then, in order

- **`2C.4` re-dispatch** (main agent / user, not a worker task). `2C.R1`
  changed the MSI and NSIS smoke steps, so CI run `41` no longer covers the
  shipped workflow. The root workflow does **not** auto-run on this branch —
  it triggers only on pushes to `main`/`dev`, PRs to `main`, and manual
  dispatch. It must be dispatched manually against the head that includes
  `2C.6`, and both installed-package smokes must pass with none skipped.
- **`2C.5` — sprint closure.** Full verification suite, the release-gated
  envelope benchmark, frontend typecheck and Vitest, the 250k production
  benchmark, one execution-log entry per task, then fresh code and architecture
  reviews appended under `## Sprint Reviews`, then user close approval.

# 6. Reporting

Report outcomes exactly as they happened. Quote failing test output. Say
explicitly when a step was skipped or could not be verified, rather than
describing it as done. State your absolute working directory and confirm what
you committed.
