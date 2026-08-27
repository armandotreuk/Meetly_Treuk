# Handoff prompt — Meetily Hybrid RAG, Sprint 2 close-out

Paste everything below the line into the new orchestrator session.

---

You are continuing the Meetily **Hybrid RAG** program at Sprint 2 (Durable
Local Semantic Index). Read this whole brief before touching anything.

## Repository layout — read carefully, this trips people

- Git root is the **parent** of `upstream/`. Source root is `upstream/`.
- The only CI workflow GitHub Actions executes is the **root**
  `.github/workflows/build-windows.yml`. Anything under
  `upstream/.github/workflows/` is inert and never runs.
- The repo sits inside OneDrive. `cargo` fails with "output path is not a
  writable directory" unless you set a target dir outside OneDrive first:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
```

- Normative documents:
  - `upstream/docs/hybrid-rag/architecture.md` — source of truth
  - `upstream/docs/hybrid-rag/sprint-2-durable-local-index.md` — sprint record,
    including `## Sprint 2 Remediation` (the task specs) and the two
    post-remediation reviews

## Standing program rules

- Implementation happens in fresh worker sessions, one task at a time. Tasks
  sized `L` run alone. Tasks carrying a migration run alone.
- Architecture changes, new models, new dependencies, schema semantic changes,
  and runtime downloads all require **user approval** before implementation.
- Every completed task appends one immutable entry to the sprint record's
  `## Task Execution Log`, using the template at the bottom of that section.
- Never edit a review record. Append new ones.
- Logs, errors, and status must never carry raw meeting text, tokens, or
  vectors.

## State as of 2026-08-26

Sprint 2 tasks 2.1 through 2.5 are complete. Two rounds of review have run:
an earlier `gpt-5.6-sol` pair (fully remediated) and a later `claude-sonnet-5`
pair (the current open findings). The remediation spec for the second round is
`## Sprint 2 Remediation` in the sprint record: tasks `2.R1` through `2.R4`,
sequential, all touching `database/repositories/retrieval.rs`.

### Reconciliation completed

A worker reported `2.R1`–`2.R4` complete with 545 Rust tests passing and 2
ignored, but its work was not recoverable. Reconciliation checked the primary
tree, all Git branches and worktrees, reachable dangling Git objects, OneDrive
copies, and the accessible worker temporary directory. Both worker sessions
reported using the primary `upstream/` checkout and left no commit or patch.

The primary tree still has the pre-remediation markers:
`generation_id_for` is used for generation resumption,
`acknowledged_fast_hybrid_queries` gates GC, `PRAGMA wal_checkpoint` is in the
measurement path, `estimated_shadow_snapshot_bytes` is in the disk envelope,
and neither `dbstat` nor a staged-identity read is present. Treat the prior
worker report as unverified; it is not implementation or test evidence.

Fresh implementation of `2.R1` is authorized. Dispatch it in a new standalone
worker session, verify it in the primary tree, record its Task Execution Log
entry, then continue sequentially with `2.R2` through `2.R4`. Do not assume a
task is complete from a worker report alone.

## Hard constraint: do not invent a prior model bundle

A previous worker got blocked here and was right to stop.

`architecture.md` "Prior-Model Retention Across Upgrade" describes packaging a
current bundle alongside one immediately-prior bundle. **That contract is
forward-looking and is not implementable now.** Only one bundle has ever
existed — `meetily-retrieval-bundle-1` — so there is no prior bundle, no prior
model id, no prior ONNX or tokenizer artifacts, no prior hashes, and no prior
contract values.

If any instruction, plan, or inference leads you toward authoring a prior
bundle identity, artifact set, byte length, or SHA-256: **stop and report.**
Fabricating those would forge a provenance chain that Sprint 1's package and
provenance hardening exists specifically to prevent. The subsection states
this as a MUST NOT, and the remediation spec lists dual-bundle packaging under
"Deliberately Not In This Remediation".

The only part of the retention design in scope now is the identity-derivation
precondition inside `2.R1` — deriving the persisted model identity from the
full approved contract instead of `bundleId` alone.

## Resolved decision for 2.R1

The user approved **Option A** on 2026-08-26. `architecture.md` "Generation
Activation" and its Decision Log record the dated transitional clause: while
no semantic query surface exists, the successful-Fast-hybrid-query condition
is satisfied by one clean restart with the new generation active and
publication lag zero. The clause expires at Sprint 3 close.

Implement the Option A acceptance criteria in the sprint record. Do not
reopen this decision or implement Option B.

## Definition of done for Sprint 2 close

In order:

1. `2.R1`–`2.R4` verified present and passing in the primary tree.
2. Full Rust verification from `upstream/`:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

3. Frontend regression safety from `upstream/frontend/`: `npx tsc --noEmit`
   and `npx vitest run`.
4. Re-run and record the production-backend 250k benchmark from Sprint 1.
5. Append one Task Execution Log entry per completed remediation task.
6. Request fresh code and architecture reviews. Append them under
   `## Sprint Reviews`; never overwrite an existing record.

## Reporting rules

Report outcomes exactly as they happened. If a test fails, quote it. If a step
was skipped, say so. If you could not verify something, say you could not
verify it rather than describing it as done. Preserve unrelated working-tree
changes — several files in this repo carry user edits unrelated to this
program.
