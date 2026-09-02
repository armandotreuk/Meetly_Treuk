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

Use a new implementation session for each task. Match the worker to the
task's S/M/L complexity and let explicit user direction override that default.
Do not merge tasks into one growing session merely to retain context.
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

## Dispatch Context

Dispatch the smallest complete task contract. Include the exact task section,
the relevant architecture invariants and decisions, file boundaries,
acceptance checks, required commands, current worktree constraints, and a
clear report format. Link to the source documents rather than pasting their
full history; workers read the named sections before editing.

Do not replay old reviews, superseded benchmark output, or unrelated sprint
plans into an implementation prompt. A fresh session is still required per
task, but its context should contain the current decision and runnable evidence
only. This preserves task isolation while preventing historical execution logs
from crowding out the code and acceptance contract.

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

**Current:** Sprints 1 and 2 are closed with user approval. Sprint 3 is in
progress: Task 3.1 is complete, Task 3.2 has production-path implementation
evidence and awaits the user's final review, and Task 3.3 is the authorized next
batch. Task 3.5 owns current-bundle release performance/R13 evidence. Task 3.6
is conditional on a reviewed terminological-gap miss; the 5/5 critical Recall@1
threshold, scope/privacy/fallback invariants, and Windows x64 release scope are
unchanged.

Historical Sprint 1 execution record:

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
7. **Batch 7:** final Task `1.3` is complete with the amendment-5 documented
   stop. The 115-case tuning partition admitted ten non-critical reference
   siblings while preserving all five critical/pinned cases; its objective
   earned `k=5`, `w_vector=1`, `w_lexical=0.5`, `alpha=0.5`, `beta=1`,
   `gamma=0`, but Critical Recall@1 is `2/5`. Quint8 still has `78/2160`
   diagnostic joint-passing configurations, but the earned constants are
   outside that region at better held-out objective value. Constants were not
   chosen by inspecting critical cases, and no further Sprint 1
   corpus/gate/partition/objective iteration may occur.
8. **Resolution (2026-08-24):** the user split the critical gate rather than
   granting a dated exception. Critical hydration-window membership (`5/5`,
   with critical facts `9/9` and zero retrieval-stage contamination) is the
   Sprint 1 gate; critical Recall@1 keeps its 100% threshold as a Sprint 3
   release gate. The bi-encoder already ranks four of five critical targets
   first, so the residual failure is ordinal position produced by fusion and
   aggregation — Sprint 3's stages. Debt is attributed by measured cause:
    `pt-ref-sla-suporte` and `pt-ref-nps-detrator` to Task `3.2`,
    `pt-ref-chaves-acesso` to Task `3.6`. **Task `1.3` is complete; the pair,
    encoding, constants, chunk profile, and title-dependence qualification are
     recorded in the approved `architecture.md` addendum. Tasks `1.4` and `1.5`
     are unblocked.**
9. **Batch 8 Task `1.4`:** exact vector search is selected under the approved
   768-d int8 bundle. At 250k it passes global p95 `48.2 ms` (500 ms gate),
   exact recall@150 `1.0000`, scope isolation, update/journal recovery,
   bounded concurrency, and a 2 ms interactive-pause response. Steady RAM is
   initial arithmetic figures were superseded by Task `1.R3`'s same-process
   result: `61.1 ms` p95, `1134.8 MiB` steady state, and `1317.9 MiB` active+
   shadow+delta+session peak. ANN was not evaluated because exact latency and
   RAM pass.
10. **Batch 9 Task `1.5`:** the manifest locks the approved e5-base int8 and
    mmarco quint8 bundle, with separate tokenizer contracts, exact hashes,
    artifact provenance, and MIT/Apache-2.0 resources. Fresh-cache staging
    verifies and atomically publishes all ten artifacts (411 MiB) before the
    Tauri build; the runtime validator/verifier passes 19 focused tests and is
    not yet called by startup. Rust 1.88.0 is declared locally. **Review blocks
    Sprint 1 close:** `1.R1` moved the gate to active root CI and added staged
    reference inference; `1.R2` closed package/provenance and recovery hardening;
    `1.R3` closed sparse-ID and production-shaped vector rebuild measurement.
     `1.R1a` corrected the active CI staging path and `1.R3a` corrected bounded
     journal publication with self-contained upsert payloads. Independent
     release evidence passes at p95 `61.1 ms`, recall@150 `1.0000`, steady
     `1133.8 MiB`, and peak `1316.9 MiB`; the conservative governing peak is
     `1319.9 MiB`. Final reviews approve; a hosted Windows build remains
     required.
11. **Deferred to Sprint 3 Task `3.6`:** single-turn query expansion, the
   architecturally correct remedy for `pt-ref-chaves-acesso`'s terminological
   gap. It is registered with its approach left as an open architecture
   question (hand-authored lexicon vs. local LLM expansion vs.
   pseudo-relevance feedback) and is blocked on a user decision.
12. **User decision on Deep-as-default** is deferred to Sprint 4 close and is
    not a Sprint 1 blocker. The open question remains recorded in
    `architecture.md`.

`architecture.md` now carries a **"Corpus Solvability"** section as the
normative counterweight to "Baseline Failure Reproduction". A corpus may not
satisfy falsifiability by being unanswerable.

No task outside the approved batch may start without its own dependency-ready
batch approval.
