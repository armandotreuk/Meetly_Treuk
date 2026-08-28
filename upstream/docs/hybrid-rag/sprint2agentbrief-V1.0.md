# Meetily Hybrid RAG — Sprint 2 agent brief

Complete context and work order as of 2026-08-28. Paste everything below the
rule into a fresh orchestrator session.

**Superseded for close-out work.** Sprint 2's remaining work is tracked in
`sprint-2c-close-out.md`, which is authoritative for task state. This brief
covers program rules and settled decisions only; take the work order from the
2C PRD, not from section 5 below.

---

You are continuing the Meetily **Hybrid RAG** program at Sprint 2 (Durable
Local Semantic Index). Read this whole brief before touching anything.

# 1. Repository layout

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

- Normative documents:
  - `upstream/docs/hybrid-rag/architecture.md` — source of truth.
  - `upstream/docs/hybrid-rag/sprint-2-durable-local-index.md` — sprint record:
    task specs, `## Sprint 2 Remediation` (tasks `2.R1`-`2.R12`),
    `## Task Execution Log`, and `## Sprint Reviews`.

# 2. Standing program rules

- Implementation happens in fresh worker sessions, one task at a time. `L`
  tasks run alone. Tasks carrying a migration run alone.
- Architecture changes, new models, new dependencies, schema semantic changes,
  and runtime downloads require **user approval** before implementation.
- Every completed task appends one immutable entry to `## Task Execution Log`
  using the template at the end of that section. Never rewrite an existing
  entry.
- Never edit a review record. Append new ones.
- Logs, errors, and status must never carry raw meeting text, tokens, or
  vectors.
- Report the absolute working directory you operate in, and confirm your
  changes are committed. A previous worker reported completed work that was
  never written to disk and could not be reconciled; that must not recur.

# 3. Where the work stands

## Committed

```
6f0fd43  Docs: record successful installed package evidence
7f11887  CI: capture GUI smoke process exit codes
08611e3  CI: persist packaged smoke verdicts
60e763f  CI: preserve packaged dbstat verdicts
1bbfcbb  CI: locate packaged installers in Tauri target
fbdaa63  CI: retain packaged smoke failure details
fc18ff8  Fix: compile retrieval contract from checked-in manifest
456b794  Sprint 2C: index lookup and dbstat verdicts
0fe0442  Sprint 2: dbstat smoke contract, document-count integrity, R14 fixes
1130ad5  Sprint 2: update CI audit and knowledge graph
bc5a11c  Docs: Sprint 2 remediation log, envelope blocker, and 2.R12 spec
dc7e003  Sprint 2 remediation: tasks 2.R1-2.R11
c972107  Docs: Sprint 2 reviews, remediation spec, architecture amendments
85f6749  Sprint 2: durable local semantic index      (2.1-2.5, 511 tests)
c3e89c1  Docs: close Sprint 1
```

Branch: `sprint-2/durable-local-index`, off `main`. Pushed; HEAD equals
`origin/sprint-2/durable-local-index`.

## Uncommitted

Nothing of yours. On `6f0fd43` the full suite passes: 584 Rust tests (2
ignored), `cargo fmt --check`, and `git diff --check`.

Working-tree changes that are **not yours** and must be preserved untouched:
`upstream/docs/notes-chat-improvement-execution.md` and the three
`graphify-out/` directories (tool output; never commit them). Re-check
`git status` yourself; this list drifts.

Commit your own work per task. The earlier remediation round was lost because
it was never committed, and the recovery search found nothing to restore.

## Already handled — do not redo

- The `20260826000000` heading migration was edited in place after shipping.
  It is **restored to its committed text**, and a test now asserts that text
  stays intact. Its `CURRENT_TIMESTAMP` value is corrected by the forward-only
  `20260827020000_normalize_retrieval_meeting_state_timestamps.sql` instead.
- **Never edit a committed migration.** `database/manager.rs:43` runs
  `sqlx::migrate!("./migrations").run(&pool)`, which validates checksums of
  applied migrations and fails startup with `VersionMismatch`. Add a
  forward-only migration instead.

## Blocked

**Resolved, do not re-open.** `2.R9` measured the activation envelope at
1,482.7 MiB against the 1.30 GiB ceiling (1,395,864,371 bytes). `2.R12` stage 1
(lazy reranker) shipped, and `2.R13`'s recorded benchmark now measures the peak
at **1,170,399,232 bytes (1116.2 MiB)** — 215.0 MiB of margin, passing.

What remains blocked is only the *scope* question: the gate samples
whole-process RSS while the ceiling derives from retrieval-only arithmetic.
`2.R13` proved a retrieval-scoped gate is not implementable (ORT 1.22 as bound
exposes no per-session memory query), and `2C.3` then found the approved
full-application calibration unachievable on the current host. See
`sprint-2c-close-out.md`; this needs a user decision, not an implementation
attempt.

# 4. Settled decisions — do not re-open

**Garbage-collection gate: option A.** A transitional clause is recorded in
`architecture.md` "Generation Activation" and its Decision Log: while no
semantic query surface exists, the successful-Fast-hybrid-query condition is
satisfied by one clean restart with the new generation active and publication
lag zero. Implemented and tested. The clause expires at Sprint 3 close and is
marked in code with a `ponytail:` comment.

**Activation envelope remedy: cut residency, do not raise the ceiling.**
`ACTIVATION_RAM_CEILING_BYTES` (1,395,864,371) is unchanged and stays
unchanged. Rationale is in the sprint Decisions log dated 2026-08-27: the peak
is 63% warm ONNX sessions and only 25% snapshot overlap, and `2.R6` measured
the same activation at 573.3 MiB with no sessions resident.

**Prior-model bundle retention is forward-looking and not implementable.**
Only `meetily-retrieval-bundle-1` has ever existed. There is no prior bundle,
no prior model id, no prior artifacts, no prior hashes. If any plan leads you
toward authoring a prior-bundle identity, artifact set, byte length, or
SHA-256: **stop and report.** Fabricating those forges a provenance chain that
Sprint 1's package hardening exists to prevent. See `architecture.md`
"Prior-Model Retention Across Upgrade", which states this as a MUST NOT.

# 5. Work order

Do these in order. Commit after each numbered item.

## 5.1 Close three review findings — DONE, do not redo

All three shipped: (a) as `2.R16`, (b) recorded in `architecture.md` and the
2C task log, (c) as `2.R17` and `2C.2`. Retained below for context only.

**a. Detect `document_count` drift instead of clamping it.**
`database/repositories/meeting.rs` uses `MAX(document_count - N, 0)` in the
deletion path. The clamp hides any future counter bug at zero. The counter is
now maintained incrementally across two paths (replacement and deletion)
rather than recomputed, so drift is a live possibility with no detector. Add a
periodic or debug-time reconciliation that compares the counter against the
true row count and logs divergence. Do not remove the clamp.

**b. Record the derived work now inside the primary delete transaction.**
The deletion decrement runs a correlated `COUNT(*)` per generation inside
`delete_meeting_with_transaction`. It is indexed, bounded by one meeting's rows
times at most two generations, and cannot fail the delete — but the
architecture otherwise keeps derived work out of primary mutations. Record it
explicitly in the task log and in `architecture.md` rather than leaving it
unremarked.

**c. Assert `dbstat` availability in the packaged smoke test.**
With `ENABLE_DBSTAT_VTAB` absent the derived-disk gate fails closed and
semantic retrieval never activates. `database/repositories/retrieval.rs:4621`
asserts availability in a unit test against the same linked `libsqlite3-sys`
as the shipped binary, which is decent coverage, and the blocker message
already distinguishes "unavailable" from "too large". Extend the assertion to
the packaged smoke test (`architecture.md` "Packaged Smoke Tests"), because the
symptom of a packaging change would be silent permanent non-activation.

## 5.2 Implement 2.R12 — DONE, do not redo

Stage 1 shipped as `2.R14`; stage 2 was correctly skipped because stage 1
passed with margin; stage 3 became `2.R13`/`2C.D1`/`2C.3` and is the open
decision described under "Blocked" above. Retained below for context only.

Full spec: `sprint-2-durable-local-index.md`, section `### 2.R12 - Activation
envelope remedy: session residency and gate scope [M]`. Follow it as written.
Summary of its three stages:

1. **Lazy reranker session.** Sprint 2 has no production rerank consumer — the
   only `rerank_sync` caller outside `model.rs` is in the `index.rs` test
   module. Build the reranker on first rerank request, behind the same cache
   and `BundleIdentity`. Defer instantiation, never validation: its load-time
   I/O contract check must still run when it is built. Re-run the benchmark and
   record the embedding-only and reranker-only session weights.
2. **Session eviction across the activation window**, only if stage 1 leaves
   the peak at or above the ceiling. Its precondition is a measurement proving
   that dropping the handles actually returns RSS; ORT arena allocators may
   retain freed blocks. If RSS does not fall materially, **stop and report** —
   the remedy does not work and the decision returns to the user.
3. **Gate scope**, required regardless of where stages 1-2 land.
   `measure_resident_ram()` samples whole-process RSS while
   `ACTIVATION_RAM_CEILING_BYTES` derives from retrieval-only arithmetic.
   `2.R9` recorded that its benchmark excludes Whisper and webview residency
   that production includes. Either measure retrieval's own residency against a
   retrieval-scoped ceiling, or keep whole-process RSS and re-derive the
   ceiling as an explicit whole-process budget naming what else may be
   resident. Record the choice in `architecture.md` beside the ceiling.

## 5.3 Sprint close — tracked as `2C.5`

1. Full verification from `upstream/frontend/src-tauri/`:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

2. Release-gated envelope benchmark:

```powershell
$env:MEETLY_RAG_INDEX_BENCH = "1"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --lib retrieval::index::tests::bench_2r6_production_activation_envelope -- --nocapture
```

3. Frontend regression safety, from `upstream/frontend/`: `pnpm run typecheck`
   and `npx vitest run`.
4. Re-run and record the production-backend 250k benchmark from Sprint 1.
5. Append one Task Execution Log entry per completed task.
6. Request fresh code and architecture reviews. Append them under
   `## Sprint Reviews`; never overwrite an existing record.

# 6. Hard constraints

- Do not change `ACTIVATION_RAM_CEILING_BYTES`, the approved model, chunk,
  encoding, or backend contracts.
- Do not edit a committed migration.
- Do not fabricate a prior model bundle or any part of its identity.
- Do not re-open the settled decisions in section 4.
- Do not commit `graphify-out/` or the unrelated modified files in section 3.
- If a gate refuses, report it with its measurement. Do not work around a gate,
  relax a ceiling, or manufacture headroom to make a task pass.

# 7. Reporting

Report outcomes exactly as they happened. Quote failing test output. Say
explicitly when a step was skipped or could not be verified, rather than
describing it as done. State your absolute working directory and confirm what
you committed.
