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

## Relationship To The Project Roadmap

This program is registered in the project `ROADMAP.md` under Phase 6. It is not
a competing plan, and `ROADMAP.md` is not "historical" — it is a live tracker
that sits above this directory for anything outside hybrid retrieval.

- `ROADMAP.md` Sprint 6A task 6.1 delivers contextual Chat entry points. This
  program depends on that surface work and does not duplicate it.
- `ROADMAP.md` defers "Semantic/hybrid search" until "a repeatable retrieval
  benchmark shows FTS5 misses important results at a material rate."
  **Sprint 1 Task 1.2 is that benchmark.** If Sprint 1 cannot demonstrate a
  baseline FTS failure on the reference and semantic categories, the deferral
  condition is unmet and this program is cancelled rather than continued.
- `docs/sprint-6-1-contextual-chat.md` closed on 2026-08-22 after its manual
  Windows/Tauri smoke passed. Its task `6.1.R10` defines the saved-meeting
  invariants Sprint 4.3 must preserve.

## Authority Order

When documents disagree, use this order:

1. The latest explicit user decision recorded in a sprint decision log.
2. `architecture.md` invariants and accepted decisions.
3. The currently approved sprint PRD.
4. A task execution entry.
5. `ROADMAP.md` for anything outside this program's retrieval scope.
6. Historical plans elsewhere in `docs/`.

Do not silently resolve a material conflict. Record it in the active sprint's
decision log and request user approval before changing scope, persistence,
privacy, model distribution, or an external contract.

## Execution Protocol

1. Obtain explicit user approval for the sprint PRD.
2. Mirror the approved task list in the opencode TODO list.
3. Propose a dependency-ready batch and obtain explicit batch approval.
4. Assign each implementation task to a new subagent session sized according
   to the task's `S`, `M`, or `L` rating. **Do not merge multiple tasks into
   one long-running session to save on context.** See "Caching Strategy"
   below — the saving is available without merging, and merging costs the
   isolation this protocol depends on.
5. Give the worker the architecture document, exact task section, file
   boundaries, acceptance checks, and required commands, assembled per
   "Caching Strategy" below.
6. Review the worker report and actual diff. Run the acceptance checks.
7. Append the immutable task execution entry before starting a dependent task.
8. Run code review at every sprint end. Run architecture review for every
   sprint in this program because all five affect high-risk architecture,
   persistence, model packaging, concurrency, or external contracts.
9. Request sprint-close approval before starting the next sprint.

The main agent owns product context, approvals, dependency ordering,
verification, and documentation. It does not implement a task delegated to a
worker. Workers must not absorb neighboring tasks or edit another sprint's PRD.

For this implementation, use a new `worker-l` session for every implementation
task regardless of its S/M/L complexity label. Every worker must use
`opencode-go/ox-alpha-free`; do not dispatch `worker-s` or `worker-m`, and do
not substitute another model.
Sprint reviews use the standard configured `reviewer` and `arch-reviewer`.
Complexity labels remain scope classifications, not agent-tier selections.

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

## Caching Strategy

Every worker dispatch resends `architecture.md` (~1,800 lines) plus the sprint
PRD in full, per the Execution Protocol's step 5 — this is intentional and
MUST NOT be trimmed per task, since the point of full-document dispatch is to
stop workers reinterpreting product decisions. Full resend is expensive if
done carelessly. It is nearly free if the prefix is structured for prompt
caching, which is keyed to **content**, not to session — a cache write from
one subagent's dispatch is readable by the next subagent's dispatch, as long
as the prefix bytes match and the read happens within the cache TTL. This is
why sessions are not merged for caching purposes: merging trades an
already-available saving for a real loss of isolation.

**Assemble every dispatch in this order:**

```text
[stable, byte-identical across every dispatch in a sprint]
  architecture.md (full)
  sprint PRD (full)
  cache_control breakpoint, ttl: "1h"
[volatile, per-task]
  exact task section
  file boundaries
  acceptance checks
  required commands
```

**Rules:**

- The stable block MUST be byte-identical across dispatches. Any variation
  before the breakpoint — a timestamp, a reordered field, a per-task
  annotation inserted into the architecture doc — invalidates the cache for
  the entire prefix, not just the changed part.
- Use an extended cache TTL (1 hour) rather than the 5-minute default. Batches
  are already dispatched close together under step 3's batch-approval flow, so
  a 1-hour TTL lets task 2 through N in an approved batch read the cache
  written by task 1.
- Prompt caching is per-model. All implementation tasks use
  `opencode-go/ox-alpha-free` through `worker-l`, so byte-identical stable
  prefixes can share that model's cache across distinct task sessions.
- Do not shrink or excerpt `architecture.md` per task to "save context." That
  reintroduces the reinterpretation risk this directory exists to prevent, for
  a saving the cache already provides without it.
- Do not merge multiple tasks into one continuous agent session to avoid
  resending the architecture document. A continuous session resends its own
  growing transcript — prior tasks' file reads, diffs, and dead ends — on
  every turn, which is typically larger than the architecture document by the
  time a sprint's later tasks run. It also blurs the per-task review and
  rollback boundary this protocol depends on: a bad turn in one task should be
  discardable without contaminating the next task's context.

## Verification Environment

**Working directory:** every verification block in these documents runs from
the repository's `upstream/` directory. The relative path
`frontend/src-tauri/Cargo.toml` resolves only from there. Workers MUST `cd` to
`upstream/` first and record the directory they used.

**Build target directory:** sprint command blocks use PowerShell because the
current workspace is Windows. They set `CARGO_TARGET_DIR` under
`%LOCALAPPDATA%` to avoid OneDrive build-path failures without hardcoding a
user name:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
```

Note that `upstream/.cargo/config.toml` does **not** set `CARGO_TARGET_DIR` —
it sets only `WHISPER_DONT_GENERATE_BINDINGS`. `MIGRATION.md` claims otherwise
and is stale. Setting the variable explicitly in every command block is
therefore required, not redundant. Other project documents name different target
paths; within this program the `%LOCALAPPDATA%` path above is authoritative.

**Platform:** this program targets Windows x64 only. See `architecture.md`
"Platform Scope" for why macOS and Linux are deferred and what re-enabling them
would require.

## Program Status

**Status:** Sprint 1 remediation in progress. Batch 6 Task `1.3G` completed
independent verification on 2026-08-24, closing the category (c) resolution.
**The final Task `1.3` selection run (Batch 7) is next** on the closed
post-`1.3G` instrument. Sprint 6.1 is closed.

Current execution state:

1. **Batch 1:** Task 1.1 is complete and unaffected. Task 1.2 is **superseded
   by Task `1.2R`** — its harness and metrics are retained, its corpus is not.
2. **Batch 2:** Task 1.3's first run is blocked. Its resource findings (RAM,
   disk, latency, quantization fidelity, licensing) are retained; its quality
   findings and tuned fusion/aggregation constants are void. Tasks 1.4 and 1.5
   remain blocked behind Task `1.3`.
3. **Batch 3:** Task `1.2R` is complete. Its baseline is Recall@1 `72/135`,
   Recall@3 `96/135`, Recall@5 `124/135`, MRR `0.695833`, Evidence Recall@10
   `181/209`, fact coverage `130/149`, forbidden contamination `25/121`, and
   source precision `471/471`; the user approved these figures on 2026-08-23.
4. **Batch 4:** Task `1.3` rerun is complete with verdict
   `blocked-quality-gates`. Every evaluated pair fails Critical Recall@1 and
   critical forbidden contamination; citation/source precision is unevaluated.
   No production model pair or constants are approved. The block is
   reattributed (2026-08-23) to instrument-closure gaps, not model findings:
   the failing critical cases are provably winnable only via the
   non-production `CONCEPT_LEXICON` channel, the contamination gate has no
   admissibility proof, and citation precision is structurally unmeasurable
   in the current simulation.
5. **Batch 5:** Task `1.3F` is complete. Citation precision passes `602/602`
   for both retained pairs and fidelity fixes close one critical rank miss,
   but the admissibility proof finds four forbidden carriers structurally
   retained and two forbidden facts with no carrier; both rerankers have
   `0/2160` jointly passing configurations. `pt-ref-chaves-acesso` also has no
   production-implementable rank-1 channel. Standing decisions: the e5-base
   RAM band is approved and `bge-reranker-base` is retired.
6. **Batch 6:** Task `1.3G` is complete. The corpus patch touches exactly the
   two approved surfaces; carrier-source-state classification (`107` retrieval-
   stage / `14` answer-stage) is fixture-derived and test-pinned; admissibility
   is enforced with rejecting mutations; the baseline re-record moved only
   forbidden contamination `25/121` → `26/121`. Every critical case now has a
   positive production-implementable rank-1 channel and the re-scoped critical
   contamination gate passes `0/2`. The final `1.3` selection run follows on
   this closed instrument.
7. **Batch 7 (next):** the final Task `1.3` selection run, carrying
   **amendment 5**. `1.3G` measured `79/2160` (mmarco-f32) and `78/2160`
   (mmarco-quint8) configurations of the existing e5-base-int8 pair that
   jointly pass every gate, while the held-out objective selects a `3/5`
   configuration — so the remaining failure is constant selection, not model
   capability or corpus content. The run admits the 10 non-critical
   reference-category cases into the tuning partition (the 5 critical/pinned
   cases stay isolated), adds a reference-category Recall@1 objective term,
   and must disclose all tuned constants. Selecting constants by inspecting
   critical-case results is forbidden. If the retune still yields `3/5`, the
   run stops and reports; the resulting choice belongs to the user.
8. **Deferred to Sprint 3 Task `3.6`:** single-turn query expansion, the
   architecturally correct remedy for `pt-ref-chaves-acesso`'s terminological
   gap. It is registered with its approach left as an open architecture
   question (hand-authored lexicon vs. local LLM expansion vs.
   pseudo-relevance feedback) and is blocked on a user decision.
9. **User decision on Deep-as-default** is deferred to Sprint 4 close and is
   not a Sprint 1 blocker. The open question remains recorded in
   `architecture.md`.

`architecture.md` now carries a **"Corpus Solvability"** section as the
normative counterweight to "Baseline Failure Reproduction". A corpus may not
satisfy falsifiability by being unanswerable.

No task outside the approved batch may start without its own dependency-ready
batch approval.
