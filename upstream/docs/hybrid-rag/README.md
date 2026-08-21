# Hybrid RAG Implementation Documents

This directory is the source of truth for Meetily's local hybrid retrieval
implementation. It separates stable architecture from sprint execution records
so implementation agents do not reinterpret product decisions while coding.

## Document Map

| Document | Purpose | Change authority |
|---|---|---|
| [`architecture.md`](architecture.md) | Normative system design, invariants, data contracts, failure behavior, quality gates, and accepted product decisions. | User approval is required for a material architecture change. |
| [`sprint-1-quality-gates.md`](sprint-1-quality-gates.md) | Evaluation corpus, prerequisite correctness, model selection, vector-backend benchmark, and bundle manifest. | Living PRD and execution record for Sprint 1. |
| [`sprint-2-durable-local-index.md`](sprint-2-durable-local-index.md) | Source revisions, durable FTS/semantic repair, chunking, bundled ONNX engines, background indexing, and query snapshots. | Living PRD and execution record for Sprint 2. |
| [`sprint-3-broad-hybrid-chat.md`](sprint-3-broad-hybrid-chat.md) | Hybrid retrieval, meeting ranking, local reranking, authoritative hydration, and folder/all Chat rollout. | Living PRD and execution record for Sprint 3. |
| [`sprint-4-deep-saved-scopes.md`](sprint-4-deep-saved-scopes.md) | Fast/Deep modes, bounded iterative retrieval, and remaining persisted Chat scopes. | Living PRD and execution record for Sprint 4. |
| [`sprint-5-search-release.md`](sprint-5-search-release.md) | Sidebar, Tauri and MCP surfaces, diagnostics, packaging, scale validation, and release gates. | Living PRD and execution record for Sprint 5. |

## Authority Order

When documents disagree, use this order:

1. The latest explicit user decision recorded in a sprint decision log.
2. `architecture.md` invariants and accepted decisions.
3. The currently approved sprint PRD.
4. A task execution entry.
5. Historical plans elsewhere in `docs/`.

Do not silently resolve a material conflict. Record it in the active sprint's
decision log and request user approval before changing scope, persistence,
privacy, model distribution, or an external contract.

## Execution Protocol

1. Obtain explicit user approval for the sprint PRD.
2. Mirror the approved task list in the opencode TODO list.
3. Propose a dependency-ready batch and obtain explicit batch approval.
4. Assign each implementation task to a new subagent session sized according
   to the task's `S`, `M`, or `L` rating.
5. Give the worker the architecture document, exact task section, file
   boundaries, acceptance checks, and required commands.
6. Review the worker report and actual diff. Run the acceptance checks.
7. Append the immutable task execution entry before starting a dependent task.
8. Run code review at every sprint end. Run architecture review for every
   sprint in this program because all five affect high-risk architecture,
   persistence, model packaging, concurrency, or external contracts.
9. Request sprint-close approval before starting the next sprint.

The main agent owns product context, approvals, dependency ordering,
verification, and documentation. It does not implement a task delegated to a
worker. Workers must not absorb neighboring tasks or edit another sprint's PRD.

## Required Worker Report

Every worker must return:

- Changed files.
- Implemented behavior.
- Explicit omissions.
- Verification commands and results.
- Architecture decisions made within the approved boundary.
- Spillover findings and blockers.
- Rollback notes.

An implementation is not complete because code exists. It is complete only
after its acceptance checks pass and its sprint execution entry is written.

## Verification Environment

Sprint command blocks use PowerShell because the current workspace is Windows.
They set `CARGO_TARGET_DIR` under `%LOCALAPPDATA%` to avoid OneDrive build-path
failures without hardcoding a user name. Linux/macOS workers must use an
equivalent local path outside synchronized storage, for example:

```bash
export CARGO_TARGET_DIR="${TMPDIR:-/tmp}/meetily-cargo-target"
```

Workers run the equivalent command for their platform and record the exact
command. Platform-specific packaged checks in Sprint 5 cannot be replaced by a
Windows-only result.

## Program Status

**Status:** Awaiting architecture and Sprint 1 approval.

No implementation task may start from these documents until the user approves
the relevant sprint and the first task batch.
