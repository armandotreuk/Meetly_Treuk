# Sprint 1: Retrieval Quality Gates

## Status

Task `1.2R` completed independent verification on 2026-08-23 and replaces the
invalid Task `1.2` corpus. Its baseline is approved. The Batch 4 Task `1.3`
rerun completed with a blocked verdict and no production model pair selected.
On 2026-08-23 the user approved three unblocking decisions (RAM band for
e5-base pairings, permanent retirement of `bge-reranker-base`, creation of
Task `1.3F`) recorded in the decision log below. **Task `1.3F` is the next
dispatch (Batch 5).** Task `1.1` is unaffected; Tasks `1.4` and `1.5` remain
blocked on `1.3`, which is blocked on `1.3F`.

Task `1.3`'s *resource* findings survive and are retained — admissibility
arithmetic, measured pair RAM, derived disk, per-pair reranker latency,
quantization fidelity, and license screening. From the rerun, its *aggregate
quality* findings on the `1.2R` corpus are sound evidence (models now
discriminate; every aggregate gate passes); what remains unresolved is the
critical-case gate surface described below.

**The external prerequisite is satisfied:** Sprint 6.1 closed after the user
confirmed all six Windows/Tauri smoke checks passed on 2026-08-22.

### Why the Task 1.3 rerun blocked (2026-08-23)

The rerun's failure surface is not aggregate model quality — overall Recall@3
is 100% (135/135), semantic Recall@3 is 100% (30/30) against a 0/30 baseline,
and Evidence Recall@10 is 100% (209/209). Every failure concentrates on the
five critical cases plus one unevaluated gate, and cross-checking the rerun
report, the `1.2R` margin table, and `model_benchmark.rs` shows three
structural problems that no model swap can resolve:

1. **The three Critical-Recall@1 misses (`pt-ref-chaves-acesso`,
   `pt-ref-sla-suporte`, `pt-ref-nps-detrator`) are "solvable" only via a
   channel production does not have.** Each has a negative lexical margin, a
   negative-or-zero title margin, and wins only on the hand-authored
   `CONCEPT_LEXICON` channel — a solvability *diagnostic* with no production
   counterpart. Its production analog is the vector channel itself, and
   e5-base's raw vector ranks `chaves-acesso` at 5. The `1.2R` solvability
   proof covered "some channel has positive margin", not "a
   production-implementable channel can win at rank 1".
2. **Zero critical forbidden contamination was never proven achievable, and
   the 4/6 result is identical across all three rerankers — it is not a model
   property.** `1.2R` pinned forbidden claims to be retrieved (for baseline
   falsifiability) but no check asserts that *any* ordering keeps all required
   evidence inside the retained top-10 (`HYDRATED_MEETINGS=5`, `EVIDENCE_K=10`)
   while excluding every forbidden fact. Better retrieval mechanically worsens
   this metric (hybrid 30/121 vs baseline 25/121). This is the `1.2` lesson —
   a gate without an admissibility proof — recurring on a different gate.
3. **Citation/source precision is unevaluated** because the benchmark stops
   before ChatSource construction, and an unevaluated gate cannot support
   selection.

Task `1.3F` exists to close these three unknowns with evidence before the
final `1.3` selection run. It classifies each blocker item as (a) a harness
fidelity gap against the architecture's designed pipeline, (b) achievable but
not at the held-out-tuned constants, or (c) unachievable at the retrieval
stage by construction — each verdict having a different remedy owner.

### Why the first Task 1.3 run blocked (corpus defect, resolved by 1.2R)

The corpus is generated procedurally from four templates in
`frontend/src-tauri/tests/fixtures/corpus.rs`. Two of them are unwinnable by
construction:

- **Reference case.** The query is `quais os dias de comunicacao por whatsapp
  para o fluxo de retencao?`. The expected evidence shares almost no surface
  with it, while each of 30 distractors contains the query *verbatim* followed
  by a wrong answer. The target is lexically and semantically further from the
  query than every competitor. No retriever wins this; `bge` reaching rank 2 is
  approximately the ceiling.
- **Semantic cases.** Identical shape — expected evidence with near-zero query
  overlap against 20 verbatim-query distractors. This is why semantic Recall@3
  is `0% (0/30)` for the FTS baseline **and** for every hybrid configuration in
  both candidate pairs. The `+10 percentage point` semantic gate is not unmet;
  it is unreachable against this corpus.

Three confirming observations:

1. Three unrelated bi-encoder families — e5-small (384-d), e5-base (768-d),
   paraphrase-MiniLM — produced **byte-identical** aggregate metrics. Per
   `architecture.md` "Corpus Solvability", that is a corpus-defect signal.
2. Pair A's hybrid result equals the FTS baseline exactly (`Recall@3 90/135` in
   both). The vector channel changed nothing, because the tuner's objective was
   constant across all 144 configurations and fell through to its tie-break.
3. Size floors are met in letter only: ~10 distinct sentence shapes across 120
   cases; `similar_topic_distractor` is attached to every case unconditionally;
   all meetings share one title and one date; `ScopeKind::All` and
   `ScopeKind::Folder` resolve to the same allow-list; the 15 `Meeting`-scope
   cases permit exactly one meeting, making their Recall@1 free.

The harness's `oracle_results` did not catch this because it returns
`required_evidence_ids` directly — it verifies the scoring code, never whether a
retriever could produce the result.

## Goal

Remove known correctness defects and make every irreversible hybrid-RAG choice
from reproducible evidence. At sprint close, the project must have an approved
evaluation corpus, embedding/reranker model pair, chunk profile, vector backend,
resource envelope, and verified model-bundle manifest. Sprint 2 must not guess
any of these contracts.

## Architecture Authority

All work follows [`architecture.md`](architecture.md). A task stops and reports
a blocker when it cannot satisfy that document without changing an approved
product decision.

Every implementation task in this program receives a fresh `worker-l` session,
even when its complexity remains M, and must use `opencode-go/ox-alpha-free`.
Do not use `worker-s`, `worker-m`, or another implementation model. Sprint
reviews use the standard configured `reviewer` and `arch-reviewer`.

## Scope

### In Scope

- Fix the today-meeting-list intersection bug.
- Make generic context builders report exactly retained evidence.
- Create a private-safe Portuguese/English retrieval evaluation corpus and
  deterministic scoring harness.
- Benchmark and select one bundled multilingual embedding model and one local
  cross-encoder reranker.
- Benchmark exact vector search and a pure-Rust ANN candidate at 250,000
  documents when exact search misses the gate.
- Select a semantic chunk profile and all-summary-template policy.
- Define the pinned model manifest, licenses, hashes, and reproducible artifact
  fetch/verification process.
- Reconcile the declared Rust toolchain requirement with ONNX Runtime.

### Out Of Scope

- Production semantic schema or background indexing.
- Production vector retrieval.
- Chat hybrid integration.
- Shipping model binaries in the installer.
- Sidebar/MCP semantic behavior.
- Remote embedding APIs.

## Current State And Evidence

- `frontend/src-tauri/src/api/chat.rs:501-532` can compute both today filtering
  and meeting-list intent, but the list branch does not apply the today-ID set.
- `frontend/src-tauri/src/export/context.rs:139-208` returns only Markdown for
  broad context, while `api/chat.rs:573-585` constructs sources before final
  prompt truncation.
- `frontend/src-tauri/src/database/repositories/fts.rs:578-599` turns every raw
  query token, including high-frequency Portuguese function words, into FTS
  terms.
- `frontend/src-tauri/Cargo.toml:114` includes `ort 2.0.0-rc.10`, but there is
  no text tokenizer or retrieval benchmark.
- `frontend/src-tauri/Cargo.toml:9` declares Rust 1.77 while the resolved ORT
  wrapper declares a newer minimum Rust version.
- Existing model downloads do not provide the hash-verified atomic package
  contract required for bundled retrieval assets.
- The live user database diagnosis measured approximately 12,254 searchable
  rows; committed tests must use synthetic/private-safe fixtures, not that
  database or raw meeting content.

## Sprint Requirements

- The evaluation harness MUST score meeting ranking and evidence retention,
  not only final model wording.
- Evaluation fixtures MUST contain no private raw transcripts, API keys, paths,
  or personal identifiers.
- The reference WhatsApp case MUST retain its factual structure while using
  synthetic meeting IDs/names/content suitable for source control.
- The reference case and every semantic/paraphrase case MUST demonstrably
  **fail** under the current FTS-only baseline, asserted by the harness. See
  `architecture.md` "Baseline Failure Reproduction".
- The corpus MUST simultaneously satisfy `architecture.md` "Corpus Solvability"
  and "Baseline Failure Reproduction". Neither may be met at the other's
  expense: an unanswerable corpus fails the baseline trivially and measures
  nothing.
- The corpus MUST meet the floors in `architecture.md` "Corpus Size Floors",
  and every reported percentage MUST carry its denominator. Cases MUST be
  materially distinct; template-generated variants count once toward a floor,
  not once per emitted row.
- Model selection MUST first apply the admissibility filter in
  `architecture.md` "Resource Budget Arithmetic". Inadmissible candidates are
  not benchmarked.
- Model selection MUST record exact revisions, licenses, dimensions, vector
  encoding, tokenizer, pooling, normalization, prefixes, ONNX export source,
  artifact hashes, package size, RAM, latency, and quality metrics.
- Model selection MUST test Portuguese and English.
- Reranker selection MUST be evaluated separately from embedding recall, and
  MUST derive its candidate depth from the 900 ms p95 sub-budget rather than
  assuming the provisional 30-50 range is affordable.
- The vector-backend decision MUST use 12k, 50k, and 250k synthetic scales and
  MUST follow the Backend Decision Rule table in `architecture.md`. ANN is
  evaluated only for a latency miss.
- Derived disk cost per document MUST be measured and projected to 250k.
- Benchmark commands and corpus generation MUST be reproducible.
- The sprint MUST publish a user-approved numeric quality/resource gate table;
  qualitative phrases such as "improves" are not sufficient acceptance.
- The lexical core-term/high-frequency-word policy, RRF limits, meeting-
  aggregation constants, reranker depth policy, scheduler limits, and ORT
  thread cap MUST be recorded as measured decisions before Sprint 2.
- Platform gates apply to Windows x64 only.
- No task may add a remote service or native SQLite extension.

## Task List

| ID | Feature | Task | Size | Owner | Dependencies | Acceptance check | Rollback |
|---|---|---|---|---|---|---|---|
| 1.1 | Retrieval correctness | Fix today/list intersection, generic retained-source parity, and existing raw lexical query logging before semantic expansion. | M | `worker-l` (`ses_fd6904208ffef0bDNhO4huicTS`) | None | Passed: 41 Chat tests, 11 context tests, privacy-log regression, Cargo check, rustfmt, and diff check. | Revert localized resolver/context/log changes; no data change. |
| 1.2 | Evaluation | Add a private-safe multilingual corpus, deterministic metrics, baseline runner, and the reference regression. | M | `worker-l` (`ses_fd69041d9ffe3Prh5s1KjcPHNL`) | None | **Superseded by `1.2R`:** harness and metrics retained; generated corpus and recorded baseline void. | Test/fixture tooling only; remove without production effect. |
| 1.2R | Evaluation | Re-author the corpus as materially distinct hand-written cases; add the solvability invariant and its harness assertion. | M | `worker-l` (`ses_fd317c5a5ffe20snhqLcOtBV3h`) | 1.2 | Passed: 120-case solvable corpus, answer-key-free structural checks, supervised raw-text margins, deterministic baseline, privacy scan, compatibility tests, rustfmt, Cargo check, and diff check. Baseline awaits user approval before `1.3`. | Restore the prior corpus/harness; test tooling only, no production effect. |
| 1.3F | Gate closure | Audit harness fidelity against the architecture pipeline, prove or refute gate admissibility for the failing critical gates, add citation-precision simulation, and deliver a per-blocker verdict table. | L | Pending `worker-l` | 1.2R, 1.3 rerun evidence | Verdict table classifies every blocker item with evidence; admissibility invariant added in report mode; citation precision measured for pairs B/C; no gate, threshold, corpus-content, or constants change. | Tests/evidence only; restore committed `tests/` state; no production effect. |
| 1.3 | Model selection | Benchmark and select the bundled multilingual embedding and reranker pair plus chunk policy. | L | `worker-l` (`ses_fd06da77cffeQmAh22R2sPO3Fz`); first run `ses_fd65db999ffe5gwCX1YNm12F5w` | 1.2R, 1.3F | **Blocked after rerun — final selection run pending `1.3F`.** All three budget-viable pairs fail Critical Recall@1 and critical forbidden contamination; source precision is unevaluated. No production pair selected. | No production default changes before approval; remove benchmark artifacts. |
| 1.4 | Vector backend | Benchmark exact search and, only if needed, a pure-Rust HNSW candidate at 250k scale. | L | Pending `worker-l` | 1.3 | Report selects exact or exact+ANN and demonstrates the architecture performance/RAM gates. | Benchmark-only dependency/code can be removed; no persisted format ships. |
| 1.5 | Model supply chain | Implement the small bundle manifest and reproducible hash/license verification pipeline; reconcile Rust MSRV. | M | Pending `worker-l` | 1.3 | Valid artifacts pass; tampered/missing artifacts fail before packaging; toolchain contract is explicit and checked. | Remove additive manifest/fetch verification; no runtime behavior yet. |

## Dependency Order

`1.2 -> 1.2R -> 1.3F -> 1.3 -> 1.4`

`1.3 -> 1.5`

`1.1` and `1.2` are independent if the evaluation harness is kept outside
`api/chat.rs` inline tests. Tasks `1.3F`, `1.3`, and `1.4` are L and run
alone. Task `1.5` may start after `1.3`, but should not run concurrently with
`1.4` if both need to change `Cargo.toml`, benchmark targets, or model
artifact scripts.

`1.2R` is a hard blocker for `1.3`. No model, encoding, chunk, fusion, or
reranker-depth decision may be made against the superseded corpus, and the
`1.3` rerun may not begin until `1.2R`'s new baseline is recorded and approved.

`1.3F` is a hard blocker for the final `1.3` selection run. The two failing
critical gates must first be proven achievable (or re-specified with user
approval on `1.3F` evidence), and the citation-precision gate must become
measurable. Running selection again before that repeats the `1.2` mistake:
benchmarking against an instrument whose gates have no admissibility proof.

## Task Specifications

### 1.1 - Retrieval correctness prerequisites [M]

**Outcome:** Existing lexical Chat has correct date/list semantics and an
evidence-retention contract that hybrid retrieval can reuse.

**Likely touchpoints:**

- `frontend/src-tauri/src/api/chat.rs`
- `frontend/src-tauri/src/export/context.rs`
- Existing Rust tests in those modules

**Required implementation:**

- When a query requests both today's meetings and a meeting list, pass the
  computed today meeting IDs into title-list resolution or otherwise intersect
  titles before formatting.
- Preserve folder and search-snapshot scope restrictions during that
  intersection.
- Generalize the generic context builder to return Markdown plus stable
  retained chunk/evidence IDs.
- Build `ChatSource` values after generic context retention, not from the full
  pre-truncation retrieval vector.
- Account for final prompt budgeting. If `assemble_prompt` can remove evidence
  after the context builder, move or expose budgeting so retained IDs represent
  the actual final prompt.
- Preserve single-meeting source parity and live-source behavior.
- Replace full-query INFO logging in `api_search_fts`,
  `api_search_transcripts`, and any remaining current lexical search path with
  privacy-safe query length/mode/result counts.
- Do not introduce semantic types or dependencies in this task.

**Acceptance criteria:**

- A today+list query in all scope lists only meetings from the current local
  date.
- The same behavior holds inside a recursive folder scope.
- Search-snapshot membership remains an allow-list.
- Generic context truncation emits only sources whose content appears in the
  context delivered to the model.
- Final prompt overhead cannot silently remove a source-backed chunk while the
  source remains emitted.
- Existing saved-meeting source-parity tests still pass.
- Captured/static log assertions prove raw Portuguese/English query text does
  not reach INFO logs in existing lexical Tauri/Chat paths.
- No schema, IPC, or persisted-source compatibility change is introduced.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib export::context::tests
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** State exactly where the final source-retention
decision now occurs and identify every Chat caller affected.

### 1.2 - Evaluation corpus and harness [M]

**Outcome:** Retrieval quality can be compared deterministically before and
after each architecture increment.

**Likely touchpoints:**

- New fixture directory under `frontend/src-tauri/tests/fixtures/`
- New integration test or benchmark/evaluation target under
  `frontend/src-tauri/tests/` or `frontend/src-tauri/benches/`
- Minimal dev-only dependencies only when standard library/Serde cannot cover
  the requirement

**Required fixture schema:**

Each case records:

```text
case ID
language
question
optional history/rewritten query
scope and allowed meeting IDs
synthetic meetings and evidence documents
expected meeting IDs and order constraints
required evidence IDs or required facts
forbidden facts when distractors conflict
answer mode category
```

**Required case categories:**

- Portuguese WhatsApp schedule reference case.
- Exact numbers/dates/names.
- Portuguese and English paraphrases with weak lexical overlap.
- Similar-topic distractor meetings.
- Summary-only, notes-only, and transcript-only answers.
- Multi-meeting synthesis.
- Follow-up query rewriting.
- Folder, all, meeting, snapshot, and today allow-lists.
- Deleted, dirty, and stale-derived candidates.

**Required metrics:**

- Meeting Recall@1, Recall@3, Recall@5.
- Mean reciprocal rank.
- Evidence Recall@K.
- Required-fact coverage from retained evidence.
- Forbidden-fact contamination.
- Citation/source precision when a context builder is available.
- Stage latency hooks, even if model stages are absent initially.

**Acceptance criteria:**

- Fixtures contain no copied private transcript text or real user identifiers.
- Baseline FTS results are recorded and reproducible.
- **The reference case fails under the FTS-only baseline in the same mode as
  the observed production failure — isolated numeric fragments, incomplete
  schedule, missing MPV distinction — and the harness asserts that failure as a
  passing test.** A fixture the current retrieval already answers is
  unrepresentative and must be strengthened until the baseline fails.
- **Every case in the semantic/paraphrase category is likewise shown to be
  under-served by the baseline.** Exact-term, number, and name categories are
  exempt and instead assert baseline success, since their gate is
  no-regression.
- The reference case expects the complete day schedule and MPV distinction.
- The harness fails when the correct meeting is moved below the required rank.
- The harness fails when required evidence is removed despite correct meeting
  ranking.
- Corpus loading, metric computation, and output ordering are deterministic.
- **The corpus meets the size floors in `architecture.md`: at least 120 cases
  total, at least 15 per required category, at least 40 Portuguese and 40
  English, and at least 5 designated reference/critical cases.** A metric
  computed below its floor is reported as indicative only and cannot close a
  sprint.
- The task publishes numeric pass/fail gates with corpus counts for every
  metric required by `architecture.md`; every percentage is reported with its
  denominator. Default floors may change only with explicit user approval based
  on baseline evidence.
- The task publishes the evaluated lexical normalization/core-term policy and
  high-frequency-word list/algorithm; it must cover Portuguese and English and
  preserve exact names/numbers.
- One documented command runs the evaluation locally.

**Required verification:**

Run from the repository's `upstream/` directory.

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

`frontend/src-tauri/tests/` does not exist yet; this task creates it. The
integration target **MUST be named `retrieval_evaluation`** so the command
above is stable — Sprints 2 through 5 hardcode it. If a different name is
unavoidable, the worker must update every sprint document that references it in
the same change, and record the rename in this sprint's decision log.

**Worker report additions:** List every case ID/category with its baseline
pass/fail status, and explain how the reference private case was syntheticized
while preserving the failure mode.

### 1.2R - Corpus re-authoring and solvability invariant [M]

**Outcome:** The evaluation corpus measures retrieval quality instead of
measuring its own impossibility. Gates become reachable by a competent retriever
and unreachable by an incompetent one.

**Supersedes:** the corpus produced by Task `1.2`. The `1.2` harness, metrics,
scoring, mutation tests, and privacy audit are sound and are **retained**; only
the fixture content and the missing solvability assertion are in scope. Do not
rewrite `retrieval_evaluation.rs` wholesale.

**Likely touchpoints:**

- `frontend/src-tauri/tests/fixtures/corpus.rs` — replaced
- `frontend/src-tauri/tests/fixtures/evaluation_policy.json` — new baseline
- `frontend/src-tauri/tests/retrieval_evaluation.rs` — solvability assertion,
  distinctness assertion, updated `expectedBaseline`
- This sprint decision log and task execution entry

**Required implementation:**

- Replace procedural generation with hand-authored cases. `format!` interpolation
  of an ordinal into a shared sentence is the defect being removed; it MUST NOT
  reappear in any category that carries a discriminating signal.
- Author each case so its expected evidence beats that case's distractors on at
  least one channel, per `architecture.md` "Corpus Solvability".
- Remove every distractor that contains the query verbatim or a superset of its
  content terms. Distractors MUST be plausible topical neighbours — meetings a
  user could reasonably confuse with the target — not query copies.
- Add an answer-key-free structural assertion over fixture text that rejects
  query-copy/superset distractors, nonce semantic discriminators, duplicated
  templates, and non-varying ranking attributes.
- Add a separate margin assertion that may use expected IDs only to label the
  target. Compute target and strongest-distractor channel scores entirely from
  raw fixture text; IDs may not contribute to scores or bypass retrieval. State
  the discriminating channel and margin per case in the report.
- Add a distinctness assertion: no two cases may share a normalized question or
  expected-evidence template. Report the count of distinct sentence shapes
  alongside the case count.
- Give meetings distinct titles and dates so title overlap and recency inputs
  are exercised. Make folder allow-lists actually exclude meetings so
  `ScopeKind::Folder` differs from `ScopeKind::All`.
- Give `ScopeKind::Meeting` cases more than one permitted meeting, so their
  Recall@1 is earned.
- Replace nonce discriminators (`Sintetico42`, `Cedar42`, `Atlas42`) in semantic
  cases with real Portuguese and English paraphrase relations. Nonce tokens are
  acceptable **only** in exact-term/number/name cases, where verbatim matching
  is the property under test.
- Re-record `expectedBaseline` in `evaluation_policy.json` from the new corpus.
  The prior figures (`R@1 75/135`, `R@3 90/135`, `MRR 0.625`) are void.
- Preserve the `retrieval_evaluation` target name and the existing three
  mutation tests unchanged.

**Reference case reconstruction — read this before authoring it:**

The production failure recorded in `architecture.md` "Reference Acceptance Case"
was an **evidence-completeness failure, not a meeting-ranking failure**: the
baseline surfaced isolated numeric fragments and an incomplete schedule, and
reduced the answer to "3 and 4 days". Task `1.2` re-encoded it as a ranking
failure and manufactured 30 verbatim-query distractor *meetings* to force it,
which is both unrepresentative and unwinnable.

The reconstruction MUST instead:

- Let the correct meeting be findable — it discusses the topic and carries the
  topic's terms, as the real one did.
- Place the complete schedule (`1, 3, 7, 10 and 15`) and the MPV/non-MPV day-one
  distinction in **separate sections of that meeting**, far enough apart that a
  bounded fragment-level retrieval can return one without the other.
- Source the misleading "3 days"/"4 days" figures from partial or superseded
  content in the topical neighbourhood — an earlier draft section, a related
  meeting — not from a wall of query clones.
- Fail the baseline on **Evidence Recall and required-fact coverage**, with
  meeting rank passing or near-passing. Assert that specific failure shape, not
  merely "the baseline fails".

The remaining four critical cases follow the same discipline: each names the
failure mode it reproduces and asserts that mode specifically.

**Acceptance criteria:**

- Every case satisfies "Corpus Solvability": answer-key-free structural checks
  pass, and a separate raw-text margin check proves the expected evidence beats
  its strongest distractor on at least one declared channel.
- The reference and semantic-category cases still fail the FTS baseline,
  asserted by the harness — falsifiability and solvability hold **together**.
- The reference case fails the baseline on evidence completeness, with its
  failure shape asserted specifically.
- Distinct sentence shapes are at least 80% of the case count; the corpus meets
  all "Corpus Size Floors" on materially distinct cases.
- Semantic cases carry a real paraphrase relation; no nonce token is the
  discriminating signal in any semantic case.
- Meeting titles and dates vary within every case; folder scope excludes at
  least one in-corpus meeting; no `ScopeKind::Meeting` case has a single-meeting
  allow-list.
- The new baseline is recorded, reproducible, and reported with denominators.
- Exact-term/number/name cases still pass the baseline (no-regression contract
  unchanged).
- The three existing mutation tests still fail the corpus when rank, evidence,
  or retained sources are degraded.
- Fixtures contain no private transcript text, identifiers, keys, or paths.

**Explicit non-goals:** no model inference, no changes to metrics definitions or
gate thresholds, no production code changes. If the new baseline suggests a gate
threshold is wrong, **report it — do not change it.** Gate changes require user
approval on baseline evidence.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** For every case, state the discriminating channel
and its margin over the strongest distractor. State the distinct-shape count
against the case count. State the new baseline with denominators beside the
superseded `1.2` figures. Name any gate the new baseline calls into question,
without changing it.

### 1.3F - Gate-stage fidelity, gate admissibility, and critical-case closure evidence [L]

**Outcome:** Every remaining `1.3` blocker is classified with evidence into
one of three verdicts, each with a different remedy owner, so the final `1.3`
selection run happens exactly once against a closed instrument.

**You must not re-derive:** The `1.3` rerun (`task-1.3-model-selection.md`) is
blocked on exactly two quality gates — Critical Recall@1 (misses:
`pt-ref-chaves-acesso`, `pt-ref-sla-suporte`, `pt-ref-nps-detrator`) and
critical forbidden contamination (`4/6`) — with identical contamination
results across all three rerankers, plus citation/source precision being
unevaluated. The `1.2R` margin table shows the three missing critical cases
win only on the `CONCEPT_LEXICON` channel (lexical and title margins
negative), and the lexicon has no production counterpart. Nobody has proven
the contamination gate is achievable by ANY ordering of this corpus. Your job
is to close those unknowns with evidence — not to select a model, and not to
make gates pass.

**Standing user decisions you inherit (2026-08-23, decision log):** the
1-1.25 GiB RAM band is approved for e5-base pairings; `bge-reranker-base` is
permanently retired (do not evaluate it beyond reusing recorded figures); the
candidate set is e5-base-int8 + mmarco-quint8 (production candidate) with
mmarco-f32 as quantization-cost reference only.

**Hard boundaries:**

- Do NOT change any gate threshold, metric definition, corpus case content,
  tuned fusion constant, tuning objective, or the held-out partition
  (reference/critical cases stay uninspected by every tuning path).
- Do NOT touch production files, sprint PRDs, or `architecture.md`.
- Where a finding implies a corpus patch or a gate re-staging, REPORT it with
  evidence; do not implement it. Both require user approval.
- Work only inside `frontend/src-tauri/tests/` plus a new report
  `docs/hybrid-rag/task-1.3f-gate-closure.md`.
- Model inference is allowed only for the diagnostics below, using the
  contracted e5-base-int8 and the mmarco rerankers.

**Deliverable 1 — Harness fidelity audit (architecture-designed exclusions).**
Audit `model_benchmark.rs` for pipeline stages the architecture mandates but
the simulation omits or half-implements, and fix the simulation to
architecture fidelity (fidelity fixes are in scope; gate changes are not).
Verify at minimum:

- *Deleted-state handling:* FTS rows are inserted for `Deleted` meetings in
  the benchmark's pool builder (evidence-insert loop is not state-guarded)
  while semantic docs skip them; production cascades FTS deletion with the
  meeting. Determine whether any channel — lexical rank slots, interleave
  limits, IDF/candidate counts — is affected even where `map_lexical` drops
  the rows, and align to production.
- *Dirty-state handling:* per the architecture failure-mode table, a dirty
  meeting excludes its stale semantic rows while current FTS/hydration remain
  allowed. Confirm the simulation embeds what production would actually have
  indexed for dirty fixtures; if it embeds `authoritative_text` for dirty
  meetings' semantic docs, it simulates an index state production never has.
- *Hydration fidelity:* per `architecture.md` "Authoritative Hydration",
  content-hash verification omits stale semantic evidence while the meeting
  stays eligible through lexical/current data. Confirm the retained-evidence
  construction in `score_case_hybrid` reflects this for stale/dirty fixtures.
- *`rewritten_query`:* report which corpus cases carry one and whether the
  production query-preparation stage would supply one where the fixture does
  not (the terminological-gap critical case is the motivating example).

Report every gap found, the fix, and the metric deltas it causes
(before/after tables, all denominators). Re-run the canonical benchmark after
fixes.

**Deliverable 2 — Gate-admissibility invariant (extends the 1.2R supervised
harness).** Add to `retrieval_evaluation.rs`'s supervised layer (answer-key
use is legitimate there) a per-case existence check for the two failing
gates, analogous to the `1.2R` margin check:

- *Evidence admissibility:* for each critical case, does at least one
  ordering of the case's indexable documents place all required evidence
  inside the retained top-10 (given `HYDRATED_MEETINGS=5`, `EVIDENCE_K=10`,
  profile docs excluded) while NO retained text contains a forbidden fact?
  Compute it constructively (e.g., required docs first, forbidden-bearing
  docs last, respecting the hydrated-meeting constraint), and record for each
  forbidden fact whether it co-resides in a document that is itself required
  evidence — if so, the gate is unachievable at the retrieval stage by
  construction.
- *Rank-1 admissibility:* for each critical case, report the margin per
  production-implementable channel only — lexical, title, and the measured
  e5-base raw vector rank. The `CONCEPT_LEXICON` is explicitly excluded from
  this check.

This check MUST NOT gate the corpus yet: assert-and-report mode, failures
printed, test passes. Its verdicts feed a user decision, not an automatic
corpus edit.

**Deliverable 3 — Citation/source precision simulation.** Extend the
benchmark with the architecture's source-emission stage: construct
`ChatSource` entries from retained evidence exactly as the broad-retrieval
contract specifies (scope revalidation before source emission; profile docs
never cited; prompt-budget filtering), and score source precision (every
emitted source present in the final retained context) for pairs B and C. This
gate must produce a number. Record the stage's assumptions explicitly.

**Deliverable 4 — Constants-feasibility probe (diagnostic, NOT tuning).**
Over the existing 360-configuration fusion grid times the gamma grid, report
whether ANY configuration passes Critical Recall@1 5/5 and critical forbidden
0/6 (on the post-Deliverable-1 harness) without regressing the exact-term
gate. Output: the count of passing configurations and three examples, or
"none exists". This is feasibility evidence for the orchestrator and user;
the tuned constants remain the held-out objective's output and MUST NOT be
replaced by a grid point chosen on critical cases.

**Deliverable 5 — Verdict table (the report's centerpiece).** One row per
blocker item: the 3 critical rank misses, each of the 6 critical forbidden
facts (naming which cases hold the 4 hits), and citation precision. Columns:
verdict — (a) fidelity-gap-fixed / (b) achievable-but-not-at-tuned-constants /
(c) unachievable-at-retrieval-stage — the evidence line, and the recommended
remedy owner (harness / corpus patch / gate re-staging needing user approval /
model). No blank cells.

**Acceptance criteria:**

- All five deliverables present; the verdict table has no blank cells and
  every claim cites a measured number with its denominator.
- The admissibility invariant exists in the supervised harness in report
  mode, with `[SUPERVISED:…]`-style labeled output.
- Citation/source precision is measured for pairs B and C.
- No gate threshold, metric definition, corpus case content, or tuned
  constant changed; `git diff` confirms the boundary.
- Fixture and report privacy scan passes (same command as `1.2R`).

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
$env:MEETLY_RAG_MODELS_DIR = "$env:TEMP\opencode\meetly-task13\models"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark
# canonical evidence run
$env:MEETLY_RAG_BENCH = "1"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark hybrid_corpus_and_resource_benchmark -- --nocapture
Remove-Item Env:MEETLY_RAG_BENCH -ErrorAction SilentlyContinue
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

Re-stage model artifacts from the pinned manifest revisions/hashes if
`%TEMP%\opencode\meetly-task13\models` has been cleared.

**Worker report additions:** the verdict table; per-fidelity-gap before/after
metric tables; the admissibility result per critical case including the
co-residence analysis per forbidden fact; the feasibility-probe result; and
rollback notes against the committed `tests/` baseline.

### 1.3 - Embedding, reranker, and chunk selection [L]

**Outcome:** One exact, redistributable model pair and chunking contract are
approved for production implementation.

**Rerun scope (2026-08-22).** This task has run once. It is a **rerun against
the `1.2R` corpus**, not a re-implementation. The harness
(`tests/model_benchmark.rs`), candidate manifest, artifact staging, and
reproduction commands are sound and are retained.

*Retained from the first run — do not re-derive:*

- Admissibility pre-filter arithmetic and the full candidate estimate table.
- Measured pair RAM: 966.8 MiB (e5-small-int8 + mmarco-quint8), 1116.7 MiB
  (e5-small-int8 + bge-int8), and the e5-base pairings.
- Derived disk: 555 B/doc → 0.13 GiB steady, 0.26 GiB rebuild at 250k.
- Per-pair reranker latency and the depth-50 ceiling under the 900 ms
  sub-budget; the fp16/f32 budget exclusions.
- Quantization fidelity: int8 = fp16 = f32 at zero measured recall cost; int8
  vs f32 session cosine agreement 0.9919.
- License, ONNX-availability, and portability screening, including the
  documented-unavailable and rejected-before-benchmark candidate lists.

*Void — must be re-measured against the `1.2R` corpus:*

- All quality metrics for both pairs, all category and PT/EN breakdowns, and
  both gate verdict tables.
- The locked fusion constants (`k=5, w_vec=0.5, w_lex=0.5, α=0, β=0`), all
  tuned γ values, and the meeting-aggregation constants. Per `architecture.md`
  Decision Log 2026-08-22, these are artifacts of a degenerate search and carry
  no evidentiary weight.
- The chunk-profile conclusion that 256/48, 384/64, and 512/96 are equivalent.
  That result followed from template-identical fixture text, not from a
  measured property of the profiles.
- The reranker selection rule's output. Re-rank the budget-viable candidates on
  the new corpus before naming a leader.

*Mandatory title/concept diagnostics for the rerun:*

- Expand the held-out title-weight grid from `β ∈ {0, 1}` to
  **`β ∈ {0, 0.25, 0.5, 1, 2}`**. Tune it with the other fusion constants; do
  not assume either title-off or unit title weight is optimal.
- For every candidate pair, report semantic and reference metrics twice: once
  at the tuned `β` and once with only `β` ablated to `0`. This mandatory pair
  holds every other tuned constant fixed. If the embedding contribution is
  visible only when title scoring is enabled, make that the headline finding,
  not a footnote or an unqualified model-quality pass.
- For every case, compare the selected bi-encoder's raw vector rank for the
  expected meeting/evidence with the supervised `CONCEPT_LEXICON` prediction.
  Report case ID, concept margin, vector rank, and agreement/disagreement. The
  lexicon is a corpus-solvability proxy, not model evidence; disagreement must
  remain visible rather than being hidden by title or fusion scores.

*Unchanged and still blocking regardless of corpus:* `BAAI/bge-reranker-base`
declares zh/en on its model card and is metadata-nonconforming for a PT+EN
product. A better corpus does not resolve this. If bge leads on quality again,
report the conformity blocker separately and do not treat the quality result as
resolving it.

**Final selection run (post-`1.3F`) scope amendments (2026-08-23):**

- The final run may not begin until `1.3F`'s verdict table is recorded and
  any corpus patch or gate re-staging it recommends has been user-decided.
- `bge-reranker-base` is permanently retired (user decision 2026-08-23):
  zh/en metadata nonconformity, latency exclusion in 2 of 4 same-day runs,
  and reference-case rank 2. Do not re-evaluate it. The candidate set is
  e5-base-int8 + mmarco-quint8 (production candidate) with mmarco-f32 as
  quantization-cost reference only.
- The 1-1.25 GiB RAM band is pre-approved for e5-base pairings (user decision
  2026-08-23; measured 1120.2 MiB for e5-base+mmarco-quint8). A RAM-band
  result is no longer a blocker for e5-base selection; record the measured
  figure against the approval in the report.
- Citation/source precision must be evaluated using the `1.3F` simulation; an
  unevaluated gate cannot support selection.
- A clean-hardware latency re-probe (quiet machine state, release build) is
  required before selection, per the rerun report §10.5.

Re-staging note: artifacts were staged to `%TEMP%\opencode\meetly-task13\models`
and have likely been cleared. Re-stage from the pinned revisions and hashes in
the manifest before rerunning.

**Likely touchpoints:**

- Reproducible benchmark target/scripts under `frontend/src-tauri/`
- `frontend/src-tauri/Cargo.toml` dev/benchmark dependencies if necessary
- This sprint decision log and task execution entry
- Small candidate metadata/manifests; no committed large model binaries

**Mandatory pre-filter — apply before benchmarking anything:**

Compute admissibility from `architecture.md` "Resource Budget Arithmetic" for
every candidate pair:

```text
dimensions * bytes_per_value * 250000 * 2   (shadow-activation overlap)
  + embedding_session_bytes
  + reranker_session_bytes
  <= 1 GiB     -> admissible
  <= 1.25 GiB  -> admissible only with explicit user risk approval
  >  1.25 GiB  -> inadmissible; DO NOT BENCHMARK
```

Record the computed figure for every candidate considered, including the ones
this filter eliminates. Two consequences are given, not findings to rediscover:

- A 768-dimension **f32** bi-encoder is inadmissible at the 250,000-document
  gate. It may be reconsidered only paired with an approved quantized encoding.
- Vector encoding is part of model selection, not a later optimization. Report
  `f32`, `fp16`, and `int8` variants of a candidate as distinct entries with
  their own quality and recall measurements.

If the filter eliminates every candidate that could meet the quality gates,
stop and raise an architecture decision. Permitted levers are lower
dimensionality, quantization, memory-mapping the base snapshot, or an approved
reduction of the 250,000-document scale gate. Adding an ANN index is not a
lever.

**Required candidates:**

- At least two multilingual bi-encoder families suitable for Portuguese and
  English, **from the admissible set**.
- At least two multilingual cross-encoder/reranker candidates when licensing
  and ONNX availability permit.
- At least the transcript window profiles defined in `architecture.md`.
- Latest-summary-only versus labeled all-summary-template evidence.
- For any candidate whose f32 form is inadmissible, its quantized form if the
  export exists.

**Required measurements:**

- Evaluation metrics from Task 1.2 for embedding retrieval.
- Reranker pairwise accuracy or NDCG on the reranker subset.
- Query and document inference latency.
- **Per-pair cross-encoder latency, and the maximum candidate depth that fits
  the 900 ms p95 reranking sub-budget.** If the affordable depth is below the
  depth that meets the quality gates, report the conflict rather than silently
  choosing one; it requires an approved gate change.
- Batch throughput.
- Peak model-session RAM.
- Artifact and installed resource size.
- Embedding dimensions, vector encoding, and expected 250k vector memory,
  including the 2x shadow-activation overlap.
- Measured derived disk cost per document and the projected 250k footprint
  against the 2 GiB steady-state envelope.
- Quantization recall cost when a quantized encoding is proposed.
- Portuguese/English regression breakdown.
- Held-out hybrid simulations selecting RRF `k`/channel weights, concept/title/
  support meeting-aggregation constants, reranker candidate count, and reranker
  batch size without tuning only the reference case.
- Title ablation on semantic and reference subsets at tuned `β` versus `β=0`,
  with all other selected constants fixed, using the expanded title-weight grid
  above.
- Per-case selected-bi-encoder rank versus `CONCEPT_LEXICON` margin, including
  explicit agreement/disagreement classification.
- CPU behavior on Windows x64 reference hardware. Other platforms are deferred;
  do not block on unavailable runners.
- License and redistribution evidence.
- Tokenizer, prefix, pooling, normalization, and quantization fidelity against
  a trusted reference implementation.

**Implementation constraints:**

- Use the existing ORT major/version contract unless evidence shows it cannot
  load an approved model.
- Do not select a model whose exact tokenizer/preprocessing cannot be
  reproduced in Rust.
- Do not infer redistribution rights from an upstream repository being public.
- Do not commit model weights.
- Record failed candidates and why they were rejected.

**Acceptance criteria:**

- One embedding and one reranker model/revision are selected, **with an
  explicit vector encoding**.
- The admissibility pre-filter was applied and its computed figure is recorded
  for every candidate considered, including eliminated ones.
- The selected pair's projected peak RAM at 250k, including 2x snapshot
  overlap and both model sessions, is at or below 1 GiB, or has explicit user
  risk approval within the 1-1.25 GiB band.
- The pair meets the Task 1.2 approved numeric semantic-category delta/floor
  without violating exact-term/number no-regression gates after planned fusion.
- Semantic and reference gate evidence includes the mandatory tuned-`β` and
  `β=0` pair. A title-dependent pass is reported as title-dependent and is not
  attributed solely to the embedding model.
- The reference case retrieves the correct meeting and complete evidence under
  at least one approved chunk profile.
- All required manifest fields are known and immutable.
- Licenses permit bundling in the distributed application.
- RRF/channel, meeting-aggregation, ORT intra-op thread values, and **reranker
  candidate depth derived from the 900 ms sub-budget** are selected with
  numeric evidence and recorded in an approved architecture addendum. If an
  adaptive depth policy is adopted it is deterministic, evaluated by the
  corpus, and never varied by wall-clock timing.
- Projected derived disk at 250k is recorded against the 2 GiB envelope.
- A reference sentence/query produces stable expected outputs for future
  packaged smoke tests, recorded platform-neutrally.
- The **Windows x64** development/CI runner executes actual tokenizer,
  embedding, and reranker reference inference before Sprint 2. macOS and Linux
  runners are deferred with the platforms; see `architecture.md` "Platform
  Scope".
- Embedding and reranker preprocessing are recorded independently, including
  tokenizer revision, pair formatting, input/output names/dtypes, truncation,
  output-label interpretation, and score transform.
- Architecture review approves the selection evidence.

**Required verification:**

The worker defines reproducible commands for each selected target and then
runs at minimum:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Include a candidate comparison table, exact model
IDs/revisions, license URLs/files, artifact hashes if available, rejected
alternatives, the recommended chunk policy, the tuned-title/title-off metric
pair, and the per-case bi-encoder/`CONCEPT_LEXICON` disagreement table.

### 1.4 - Vector backend benchmark [L]

**Outcome:** The project has a measured search backend for 250,000 documents,
not an assumed dependency choice.

**Likely touchpoints:**

- Reproducible benchmark target and synthetic corpus generator
- Temporary or dev-only pure-Rust ANN dependency only if exact misses its gate
- Sprint decision log

**Required benchmark matrix:**

- 12,000, 50,000, and 250,000 vectors.
- Selected production dimension and encoding.
- Cold load and warm query.
- Global all-meetings query.
- Narrow folder/snapshot allow-list query.
- Update/delta and full rebuild cost if ANN is evaluated.
- Exact base+delta/tombstone update and compaction cost.
- Reader-held old snapshot plus new/shadow snapshot/model-session peak RAM.
- Crash window between canonical SQLite commit and memory/sidecar publication.
- Single and bounded concurrent queries.
- Candidate limits, vector-scan concurrency, interactive queue size/permits,
  exact/ANN compaction threshold, and index-worker scheduling impact.
- Peak RSS and on-disk size.
- Recall against exact nearest neighbors for ANN.

**Decision rule:**

The latency gate and the RAM gate have different remedies. An ANN index stores
a proximity graph *in addition to* the vectors it indexes: it reduces query
latency and **increases** memory. Selecting ANN in response to a RAM failure
makes the failure worse. Follow the Backend Decision Rule table in
`architecture.md`:

| Measured result at 250k | Permitted remedy |
|---|---|
| Both gates pass | Ship exact search. Do not evaluate ANN. |
| Latency p95 misses, RAM passes | Evaluate a pure-Rust HNSW-style index. This is the only ANN trigger. |
| RAM misses, latency passes | Quantized `vector_encoding`, lower-dimension model, or memory-mapped base. **Do not evaluate ANN for this failure.** |
| Both miss | Stop. Architecture decision required. Do not silently reduce scale, quality, or corpus. |

Additional constraints:

- An ANN candidate's graph memory counts toward the retrieval RAM envelope. A
  candidate that pushes peak RAM above the approved band is rejected regardless
  of its latency benefit.
- Reject any ANN candidate that cannot be persisted/rebuilt safely on Windows
  x64, or whose measured recall harms the evaluation corpus.
- Because Task 1.3 applies the memory pre-filter, a RAM miss here means the
  pre-filter arithmetic was wrong. Record that as a finding, do not work around
  it silently.

**Acceptance criteria:**

- Benchmark generation is deterministic and does not allocate unbounded test
  data.
- Results include p50/p95, load time, RAM, disk, and recall.
- Narrow scope filtering cannot return out-of-scope documents.
- The selected backend has a documented update and crash-recovery strategy.
- Meeting updates do not synchronously copy/rebuild all 250k base vectors.
- Peak old/new snapshot, delta/tombstone, ANN graph when selected, and active
  model-session RAM is at or below 1 GiB, or a measured 1-1.25 GiB result is
  explicitly approved before selection; above 1.25 GiB fails without a product
  scope change.
- Measured derived disk at 250k is at or below the 2 GiB steady-state and 3 GiB
  rebuild-peak envelope, or has explicit approval.
- Any added ANN library has acceptable license, maintenance, and Windows x64
  build evidence.
- Vector candidate limits, scan permits, interactive queue limit, index
  compaction threshold, and scheduler policy are recorded in the architecture
  addendum; unresolved values block Sprint 2.
- The final backend decision is recorded in `architecture.md` by a dated,
  approved addendum before Sprint 2 implementation.

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

The benchmark command and output artifact are mandatory worker-report items.

### 1.5 - Bundle manifest, artifact verification, and MSRV [M]

**Outcome:** Retrieval model artifacts can be fetched reproducibly and rejected
before packaging when altered, missing, unlicensed, or incompatible.

**Likely touchpoints:**

- Small checked-in manifest under `frontend/src-tauri/` resources/config
- Build or CI script for pinned artifact acquisition and hash verification
- The repository-root `.github/workflows/build-windows.yml` — **this is the
  only active workflow in this fork.** The macOS and Linux workflows under
  `upstream/.github/workflows/` are nested where GitHub Actions never reads
  them; do not edit them expecting CI to run, and do not treat their existence
  as platform coverage.
- Root/frontend Rust toolchain or manifest metadata
- License-resource configuration

**Required implementation:**

- Encode every field required by `architecture.md`.
- Encode separate complete tokenizer/preprocessing/input/output contracts for
  embedding and reranker models, unless both explicitly reference an identical
  shared tokenizer identity.
- Fetch into a build cache/staging location, never directly into a final signed
  package directory.
- Verify byte length and SHA-256 before publication.
- Verify all required artifacts as one package.
- Include exact license text/attribution in packaged resources.
- Add runtime lazy length/SHA-256 verification before first model load.
- Define whether CI or a release-preparation command fetches artifacts.
- Resolve the mismatch between the project's declared Rust 1.77 and the
  resolved ORT wrapper's newer MSRV using an explicit tested toolchain policy.
- Add no runtime model download behavior.

**Acceptance criteria:**

- Valid pinned artifacts pass on the Windows x64 workflow path. No macOS or
  Linux workflow path is required or claimed this release.
- One-byte corruption fails verification.
- Missing tokenizer/model/license fails verification.
- Unknown manifest version fails closed.
- Wrong input/output name, dtype, label index, pair format, or tokenizer
  reference fails validation.
- Model files remain excluded from normal Git history.
- The selected Rust toolchain is explicit and exercised by CI.
- No application startup code depends on model artifacts yet.

**Required verification:**

```powershell
# Worker supplies the final artifact verification command.
pnpm --dir "frontend" run typecheck
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** Record artifact provenance, cache behavior,
license packaging, CI impact, and the exact Rust toolchain decision.

## Sprint Acceptance Criteria

- All task acceptance sets pass (`1.1`, `1.2R`, `1.3F`, `1.3`, `1.4`, `1.5`;
  `1.2` is superseded by `1.2R`).
- The corpus satisfies solvability and falsifiability simultaneously, both
  asserted by the harness, on materially distinct cases.
- `architecture.md` contains approved addenda for selected models, vector
  encoding, chunking, vector backend, reranker depth, limits, and toolchain.
- The evaluation corpus meets its size floors and the reference/semantic cases
  demonstrably fail the FTS baseline.
- Evaluation baseline and selected-model results are reproducible.
- The selected pair's projected 250k RAM and disk figures are recorded against
  their envelopes.
- No private corpus content is committed.
- No production semantic schema or runtime retrieval is introduced early.
- Full Rust library tests, typecheck, Vitest, Cargo check, rustfmt, and diff
  checks pass.
- Code review and architecture review return Approved with no unresolved
  blocker or should-fix finding.

**Cancellation condition:** if Task 1.2 cannot demonstrate that the current
FTS-only baseline fails the reference and semantic categories, the `ROADMAP.md`
deferral condition for semantic search is unmet. The correct outcome is to
cancel this program and keep the Sprint 1.1 correctness fixes, not to proceed
with a retrieval rewrite that has no measured problem to solve.

## Risks And Mitigations

- **Benchmark overfitting:** require multilingual categories and held-out cases,
  not only the reported WhatsApp question.
- **Unrepresentative synthetic fixture:** the same agent authors the fixture and
  must beat it. Mitigated by requiring the fixture to fail the FTS baseline in
  the observed failure mode, asserted as a passing test.
- **Unanswerable synthetic fixture (materialized in `1.2`; mitigated by
  `1.2R`):** the falsifiability requirement alone is satisfiable by making the
  corpus impossible, which passes the letter of the rule and measures nothing.
  Mitigated by `architecture.md` "Corpus Solvability", asserted from fixture
  text without the answer key, and by treating identical metrics across
  unrelated model families as a corpus-defect signal.
- **Template-inflated sample size:** procedural generation satisfies the size
  floors nominally while leaving the effective N at the template count.
  Mitigated by the distinctness assertion and the 80% distinct-shape floor.
- **Constants tuned on a degenerate objective:** when a search objective's
  leading terms are constant across all configurations, the grid resolves on its
  tie-break and produces values that look measured. Mitigated by voiding the
  `1.3` constants explicitly rather than carrying them into Sprint 2.
- **Statistically meaningless gates:** percentage thresholds at small N are
  noise. Mitigated by corpus size floors and mandatory denominators.
- **Gate without an admissibility proof (materialized twice: `1.2` corpus
  solvability, `1.3` rerun critical contamination):** a gate can be
  unachievable by construction while looking like a quality failure, wasting
  L-sized benchmark tasks against it. Mitigated by `1.3F`'s per-gate
  admissibility invariant in the supervised harness: every zero-tolerance
  gate must carry an existence proof that some retrieval ordering satisfies
  it before a model is benchmarked against it.
- **Solvability proven on a non-production channel:** the `CONCEPT_LEXICON`
  margin can certify a case "solvable" that no implementable channel
  (lexical, title, vector) can win. Mitigated by `1.3F`'s rank-1
  admissibility check, which excludes the lexicon and reports
  production-implementable margins only.
- **Model license ambiguity:** treat unclear redistribution as a blocker.
- **Reference mismatch:** compare Rust tokenization/outputs with trusted model
  reference vectors.
- **Installer growth:** report exact package size even though no strict total
  asset limit was selected.
- **Memory arithmetic discovered late:** mitigated by applying the
  admissibility pre-filter in 1.3 before benchmarking, not after.
- **ANN complexity without need:** exact must fail the *latency* gate before ANN
  is selected; a RAM miss is never an ANN trigger.
- **Reranker exceeding the Fast budget:** measured per-pair latency and a
  derived depth ceiling, rather than assuming 30-50 pairs are affordable.
- **Private fixture leakage:** use synthetic text and review fixture diffs.
- **CI artifact drift:** immutable revisions plus SHA-256, never floating URLs.

## Decisions And Change Log

| Date | Decision or change | Rationale | Alternatives considered | Approved by |
|---|---|---|---|---|
| 2026-08-21 | Sprint 1 is a quality gate and does not ship semantic retrieval. | Prevent foundational model/index guesses from leaking into production architecture. | Implement a chosen model immediately. | User |
| 2026-08-21 | Use synthetic source-controlled evaluation fixtures. | Preserve privacy while retaining factual retrieval structure. | Commit a copy of the live user database. | User |
| 2026-08-21 | Exact search must be benchmarked before ANN. | Current scale is small and deletion is cheaper than a speculative subsystem. | Adopt sqlite-vec/HNSW immediately. | User |
| 2026-08-21 | Apply the memory admissibility filter before benchmarking any model. | A 768-dim f32 candidate is inadmissible at 250k by arithmetic alone; discovering that through two L-sized benchmark tasks wastes the sprint. | Benchmark on quality first and check memory afterward. | User |
| 2026-08-21 | ANN is triggered by a latency miss only, never a RAM miss. | An HNSW graph is stored in addition to the vectors and cannot reduce a footprint it adds to. | Keep the original undifferentiated "scale gate" rule. | User |
| 2026-08-21 | Vector encoding is part of model selection; quantized variants are distinct candidates. | The RAM envelope makes encoding a first-order quality decision, not a later optimization. | Select f32 and treat quantization as contingency. | User |
| 2026-08-21 | Require the reference fixture to fail the FTS baseline, asserted by the harness. | Otherwise the program's headline gate cannot fail and therefore cannot inform anything. | Record the baseline without asserting failure. | User |
| 2026-08-21 | Set corpus size floors and require denominators on every percentage. | At N=20 a single case moves Recall@3 by 5 points and "+10 points" is noise. | Record counts without setting floors. | User |
| 2026-08-21 | Derive reranker candidate depth from a 900 ms sub-budget. | 30-50 cross-encoder pairs on CPU plausibly consume the entire 2 s Fast budget alone. | Assume the provisional 30-50 range is affordable. | User |
| 2026-08-21 | Windows x64 only for all platform gates in this sprint. | The macOS/Linux workflows in this fork are nested under `upstream/` and never execute; the original gate was unsatisfiable. | Add root-level macOS/Linux workflows as a Sprint 0 prerequisite. | User |
| 2026-08-21 | Pin the evaluation target name to `retrieval_evaluation`. | Four later sprints hardcode the command; "the name may differ" would break them all. | Let the worker choose and update references later. | User |
| 2026-08-21 | Add an explicit program cancellation condition. | If the FTS baseline does not fail, the `ROADMAP.md` deferral condition is unmet and the rewrite has no measured problem to solve. | Proceed regardless of baseline evidence. | User |
| 2026-08-22 | Pin every implementation and review subagent to `openai/gpt-5.6-sol`; use distinct `worker-l` sessions for all tasks. | The user explicitly required the main agent's model for the entire implementation and forbade model changes at that point. | Use size-tier worker models. | User |
| 2026-08-22 | Approve Batch 1 containing Tasks `1.1` and `1.2`, each in a distinct `worker-l` session. | Both tasks have no dependencies and are parallel-safe when the evaluation harness stays outside `api/chat.rs` inline tests. | Dispatch either task alone or revise boundaries. | User |
| 2026-08-22 | Supersede the Sol-only model pin: use only fresh `worker-l` sessions for implementation, with the standard model configured for that subagent. | The user explicitly revised the dispatch policy after the first two workers returned no report. Task scope and Batch 1 approval are unchanged. | Keep the prior Sol-only pin or use size-tier workers. | User |
| 2026-08-22 | Pause implementation before Task `1.3`; do not approve proposed Batch 2. | The user selected Pause implementation at the Batch 2 dispatch gate. Batch 1 remains complete and Tasks `1.3`-`1.5` remain pending. | Approve Task `1.3` or review its boundaries first. | User |
| 2026-08-22 | Require `opencode-go/ox-alpha-free` for every `worker-l` implementation session with no model substitution. | The user explicitly pinned the implementation agent model. This supersedes the earlier standard-configured-model policy without resuming paused implementation. | Keep the configurable standard model or permit fallbacks. | User |
| 2026-08-22 | Resume implementation and approve single-task Batch 2 containing Task `1.3`. | Task `1.2` is complete, verified, and logged; Task `1.3` is dependency-ready and must run alone because it is L-sized and establishes shared model/chunk contracts. | Keep implementation paused or review Task `1.3` boundaries again. | User |
| 2026-08-22 | Record Task `1.3` as blocked with no production model pair selected. | Independent audit and reruns confirm the PT+EN-conforming e5-small+mmarco pair passes RAM/latency but degrades NDCG and misses quality gates; the NDCG-leading BGE pair is metadata-nonconforming and falls in the RAM approval band. | Silently weaken a gate, treat a benchmark leader as selected, or start dependent Tasks `1.4`/`1.5`. | Main agent |
| 2026-08-22 | Task `1.3` blocker addendum: exhaust held-out fusion and meeting-aggregation constants before requesting an architecture decision. | A 144-configuration fusion grid plus six reranker weights per viable pair, tuned on 105 non-critical/non-reference cases, still leaves the conforming pair at 66.67% Recall@3 with degraded NDCG and the nonconforming BGE pair at 77.78% Recall@3 with reference rank 2. The blocked verdict stands. | Treat fixed channel weights or only gamma 0/1 as exhaustive, or tune constants against the reference cases. | Main agent |
| 2026-08-22 | Reattribute the Task `1.3` block: the Task `1.2` corpus is invalid, and `1.3`'s quality findings are uninterpretable rather than adverse. | The reference and semantic templates place the query verbatim inside every distractor while the expected evidence shares almost no surface with it. No retriever can win those cases. Three unrelated bi-encoder families returning byte-identical metrics confirms the corpus, not the model, is the deciding variable. | Accept the blocked verdict as a model finding and choose between Pair A and Pair B. | User |
| 2026-08-22 | Add Task `1.2R` to re-author the corpus; make it a hard blocker for `1.3`. | The instrument must be repaired before any irreversible model, encoding, chunk, or fusion contract is set from its output. | Lower the gates to fit the observed numbers, or approve Pair B on RAM/metadata waivers. | User |
| 2026-08-22 | Reject the gate-lowering and approve-Pair-B options recorded in the `1.3` report §5. | Both ratify a measuring instrument now known to be invalid, and would lock a production contract for four downstream sprints on uninterpretable evidence. | Adjust the affected quality gates with the existing evidence; waive bge's zh/en metadata and RAM band. | User |
| 2026-08-22 | Retain `1.3`'s resource findings; void its quality findings and tuned constants. | RAM, disk, latency, quantization fidelity, and licensing are properties of the models and hardware, independent of fixture content. Quality metrics and constants tuned on a degenerate objective are not. | Void the entire task and rerun it from scratch. | User |
| 2026-08-22 | Do not invoke the cancellation condition yet. | The `ROADMAP.md` deferral condition is currently *unproven in both directions*: `1.2` showed FTS failing a corpus that everything fails, which is not evidence about FTS. `1.2R` resolves it honestly either way — real FTS gaps mean proceed, no gaps mean cancel with justification. | Cancel now on the existing blocked verdict, keeping only the `1.1` fixes. | User |
| 2026-08-22 | Record `BAAI/bge-reranker-base` zh/en metadata nonconformity as an independent blocker that `1.2R` does not resolve. | It is a property of the model card, not of the corpus. A corpus fix must not be allowed to look as though it cleared this. | Treat a post-rerun quality win as resolving the conformity question. | User |
| 2026-08-22 | Approve single-task Batch 3 containing Task `1.2R`, with a two-part solvability assertion. | The task is dependency-ready and must repair the shared corpus before `1.3` can rerun. Structural defects are checked without the answer key; expected IDs only label targets for raw-text margin scoring. | Keep the task paused or require an impossible unsupervised expected-target assertion. | User |
| 2026-08-23 | Record title concentration in `1.2R` and require title ablation plus concept-proxy disagreement evidence in the `1.3` rerun. | Title is the strongest supervised solvability channel for 52/120 cases overall and 23/45 reference/semantic cases. A passing fused gate could therefore conceal weak embedding behavior unless tuned-`β` and `β=0` results are paired and raw bi-encoder ranks are compared with the `CONCEPT_LEXICON` proxy per case. | Treat titles as ordinary fusion input and report only the tuned aggregate; retain the two-point `β ∈ {0,1}` grid; assume the handcrafted concept proxy predicts the selected model. | User |
| 2026-08-23 | Approve the Task `1.2R` baseline as the authority for the Task `1.3` rerun. | Independent verification passed on the solvable 120-case corpus and the tests-only checkpoint `1e41b6b` makes subsequent benchmark changes diffable. Approved figures: R@1 72/135, R@3 96/135, R@5 124/135, MRR 0.695833, Evidence R@10 181/209, fact coverage 130/149, forbidden contamination 25/121, source precision 471/471. | Keep model work blocked and revise `1.2R`; approve a gate or model pair together with the baseline. | User |
| 2026-08-23 | Approve single-task Batch 4 containing only the Task `1.3` rerun in a fresh `worker-l` session using `opencode-go/ox-alpha-free` with no fallback. | `1.2R` is complete and approved; the retained harness and staged artifacts are available. The rerun must retune void quality constants on the fixed corpus, use the expanded title grid, publish title ablation and concept-proxy disagreement evidence, and select no pair unless every gate passes. | Keep Task `1.3` blocked or merge dependent Tasks `1.4`/`1.5` into the model rerun. | User |
| 2026-08-23 | Route the same Ox Alpha model through `openrouter/stealth/ox-alpha` for the Batch 4 remediation, superseding only the `opencode-go` provider route. | `opencode-go/ox-alpha-free` returned repeated provider `network_error` responses and left remediation incomplete. OpenRouter exposes the exact Ox Alpha model; changing transport preserves the model pin rather than authorizing a fallback. | Keep retrying the unavailable `opencode-go` endpoint; substitute a different model. | User |
| 2026-08-23 | Restore the Batch 4 worker route to `opencode-go/ox-alpha-free`. | OpenRouter recognized `stealth/ox-alpha` but every worker and direct probe completed with empty output, so that route could not perform or report work. The model pin remains Ox Alpha and no fallback is authorized. | Keep the nonfunctional OpenRouter route; substitute another model. | User |
| 2026-08-23 | Record the Batch 4 Task `1.3` rerun as blocked with no production pair selected. | Every budget-viable pair fails Critical Recall@1 and critical forbidden contamination `4/6`; bge also fails the pinned Reference Recall@1 and remains zh/en metadata-nonconforming. Citation/source precision is unevaluated, all conforming e5-base pairings require RAM-band approval, and latency viability varies with machine state. | Select the best aggregate pair despite failed/unevaluated gates; weaken critical gates; treat title-assisted aggregate recall as sufficient. | Main agent |
| 2026-08-23 | Reattribute the `1.3` rerun block: the remaining failures are instrument-closure gaps, not model findings. | The three Critical-R@1 misses win only on the non-production `CONCEPT_LEXICON` channel (lexical/title margins negative; e5-base raw vector rank 5 on `chaves-acesso`); critical contamination `4/6` is identical across all three rerankers and has no admissibility proof; citation precision is structurally unevaluated. No model swap changes any of these. | Dispatch another selection rerun with new candidates; accept the blocked verdict as final; weaken the critical gates. | User |
| 2026-08-23 | Approve the 1-1.25 GiB RAM band for e5-base pairings (measured 1120.2 MiB for e5-base+mmarco-quint8). | Every conforming pairing sits in the explicit-approval band; the auto-pass alternative (e5-small, 966.8 MiB) is measurably weaker (fused R@3 129/135 vs 131/135). The band exists exactly for this trade; approving it removes a standing blocker from every future verdict. | Require the automatic <=1 GiB pass and accept e5-small's weaker quality; defer the band decision to selection time. | User |
| 2026-08-23 | Permanently retire `BAAI/bge-reranker-base` from the candidate set. | zh/en card metadata nonconformity for a PT+EN product, latency exclusion in 2 of 4 same-day runs, and pinned reference-case rank 2 — none of which further benchmarking changes. Candidate set narrows to e5-base-int8 + mmarco-quint8 (production) with mmarco-f32 as quantization-cost reference. | Keep bge as a comparison candidate; waive the metadata gate on quality evidence. | User |
| 2026-08-23 | Add Task `1.3F` as a hard blocker for the final `1.3` selection run. | The two failing critical gates need per-case verdicts — harness-fidelity gap, achievable-not-at-tuned-constants, or unachievable-by-construction — plus a citation-precision simulation, before selection can run exactly once against a closed instrument. Corpus patches and gate re-staging remain user decisions on `1.3F` evidence. | Rerun selection directly; patch the corpus or re-stage gates now without evidence; treat the critical failures as final model findings. | User |
| 2026-08-23 | Authorize the `1.3F` constants-feasibility probe as diagnostic evidence, explicitly not tuning. | Distinguishing "no configuration can pass the critical gates" from "the held-out objective misses a passing region" changes the remedy owner. The probe reports existence only; tuned constants remain the held-out objective's output and reference/critical cases stay outside every tuning path. | Omit the probe and keep verdict category (b) unmeasurable; allow tuning on critical cases. | User |
| 2026-08-23 | Record commit `7318c0c` (on top of checkpoint `1e41b6b`) as the committed instrument baseline for `1.3F`. | The evaluation corpus, harnesses, and manifest are the program's measuring instrument; earlier worker sessions modified them with no tracked history (`1.2R` itself could not roll back). The tree is now fully committed; `1.3F` and later tasks diff and restore against `7318c0c`. | Continue with an untracked `tests/` directory and checkpoint stashes. | User |

## Task Execution Log

<!-- Append one immutable entry per completed, blocked, or cancelled task. -->

### 1.1 - Retrieval correctness prerequisites

**Status:** Complete
**Owner:** `worker-l` (`ses_fd6904208ffef0bDNhO4huicTS`)
**Completed:** 2026-08-22
**Implemented:**
- Intersected today/list title resolution with all, recursive-folder, and
  frozen-snapshot allow-lists.
- Made generic context return stable retained evidence identities and construct
  broad sources only after final prompt overhead is budgeted.
- Replaced raw lexical query logging with Unicode length, mode, scope, limit,
  authorization-presence, and result-count fields.
**Implementation:**
- Files: `frontend/src-tauri/src/api/api.rs`,
  `frontend/src-tauri/src/api/chat.rs`,
  `frontend/src-tauri/src/database/repositories/fts.rs`, and
  `frontend/src-tauri/src/export/context.rs`.
- Approach: `prepare_chat_inputs_for_scope` computes the final evidence budget,
  calls `build_context_markdown_with_limit`, and filters `ChatSource` values by
  its `(meeting_id, chunk_type, chunk_id)` identities before `assemble_prompt`.
**Not implemented:**
- Semantic types, model dependencies, schema, IPC, persisted-source changes,
  or changes to saved-meeting/live source behavior.
**Why not implemented:**
- Explicitly outside Task 1.1.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::chat::tests` - pass, 41 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib export::context::tests` - pass, 11 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib api::api::tests::lexical_search_info_fields_never_contain_raw_query_text` - pass, 1 test.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; unrelated `ROADMAP.md` line-ending warning only.
**Rollback:**
- Revert the four Rust files; no migration or data repair is required.
**Decisions and follow-ups:**
- All persisted streaming/non-streaming and MCP Chat callers inherit the shared
  preparation fix. String-returning lexical context APIs keep their contracts.

### 1.2 - Evaluation corpus and harness

**Status:** Complete
**Owner:** `worker-l` (`ses_fd69041d9ffe3Prh5s1KjcPHNL`)
**Completed:** 2026-08-22
**Implemented:**
- Added 120 deterministic synthetic cases: 60 Portuguese, 60 English, five
  critical references, and at least 15 cases in every required overlapping
  category.
- Reproduced the WhatsApp schedule failure with days 1, 3, 7, 10, and 15 plus
  the MPV/non-MPV day-one distinction, without private source text.
- Added deterministic FTS metrics, numeric gates, lexical policy, latency hooks,
  baseline-failure/no-regression assertions, and rank/evidence/source mismatch
  mutation checks.
**Implementation:**
- Files: `frontend/src-tauri/tests/retrieval_evaluation.rs` and
  `frontend/src-tauri/tests/fixtures/{corpus.rs,evaluation_policy.json,README.md}`.
- Approach: exercise the production `FtsRepository` against isolated in-memory
  SQLite FTS5 databases and the production generic context builder at a 1,200
  Unicode-character budget. The deterministic baseline is Recall@1 `75/135`,
  Recall@3 `90/135`, Recall@5 `90/135`, MRR `0.625`, Evidence Recall@10
  `90/150`, required-fact coverage `90/150`, forbidden contamination `105/135`,
  and retained-source precision `300/300`.
**Not implemented:**
- Embedding/reranker inference, chunk selection, hybrid deltas, production
  semantic runtime, or RAM/disk/backend benchmarks.
**Why not implemented:**
- These belong to Tasks 1.3 and 1.4.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation` - pass, 3 tests including degraded rank, missing evidence, and unretained-source mutations.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; unrelated `ROADMAP.md` line-ending warning only.
- Fixture secret/path scan and built-in private-marker validation - pass; matches are scanner literals only.
**Rollback:**
- Remove the integration target and its three fixture files; production behavior
  and persisted data are unaffected.
**Decisions and follow-ups:**
- All 15 reference variants and all 30 semantic/paraphrase cases fail the FTS
  baseline as required; all 75 exact-term cases pass the no-regression contract.
- A worker-added entry in the unrelated Notes/Chat execution record was removed;
  this sprint document remains authoritative.

### 1.3 - Embedding, reranker, and chunk selection

**Status:** Blocked
**Owner:** `worker-l` (`ses_fd65db999ffe5gwCX1YNm12F5w`)
**Completed:** 2026-08-22
**Implemented:**
- Added a reproducible Windows x64 benchmark harness, candidate manifest, and
  evidence report without committing model weights.
- Applied admissibility filtering, ran real tokenizer/ONNX inference, evaluated
  three bi-encoder families and two reranker families/precisions, measured
  deterministic batch-1 latency and pair RAM, and compared all required chunk
  and summary policies against the Task 1.2 corpus.
- Recorded platform-neutral reference outputs, artifact hashes, license and
  preprocessing contracts, vector encoding, 250k RAM/disk projections, and
  benchmark-leader constants explicitly not approved for production.
**Implementation:**
- Files: `frontend/src-tauri/tests/model_benchmark.rs`,
  `frontend/src-tauri/tests/fixtures/model_bundle_manifest.json`,
  `frontend/src-tauri/tests/fixtures/corpus_types.rs`,
  `frontend/src-tauri/tests/retrieval_evaluation.rs`,
  `frontend/src-tauri/Cargo.toml`, `Cargo.lock`, and
  `docs/hybrid-rag/task-1.3-model-selection.md`.
- Approach: stage immutable model artifacts outside git, execute them through
  the existing ORT major contract plus a benchmark-only Hugging Face tokenizer,
  run production FTS and deterministic hybrid simulations, and keep blocked
  evidence under `benchmarkLeader` rather than a production `selected` contract.
**Not implemented:**
- No production model pair, semantic runtime, vector backend, bundle supply
  chain, schema, API, or model weights.
**Why not implemented:**
- The PT+EN-conforming e5-small-int8 + mmarco-quint8 pair passes the automatic
  RAM envelope at 966.8 MiB and the latency/license/portability gates, but
  degrades NDCG and fails semantic/reference quality gates. The NDCG-leading
  BGE pair is metadata-nonconforming for Portuguese and measures 1116.7 MiB,
  requiring separate risk approval even if its model-contract blocker changed.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation` - pass, 3 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark` - pass, 7 offline-safe/coherence tests with no target warnings.
- Staged-artifact reference inference and full release hybrid/resource benchmark - pass as reproducible commands; decision is `blocked-quality-gates`.
- `MEETLY_RAG_PAIR=e5-small-int8/...:mmarco-reranker/... pair_ram_probe` - independently measured 966.8 MiB projected peak at 250k.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib` - pass, 394 tests; 2 ignored.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; unrelated `ROADMAP.md` line-ending warning only.
**Rollback:**
- Remove the benchmark target, candidate manifest/report, and shared corpus type
  file; restore the Task 1.2 local type definitions and Cargo dependency/lock
  changes. Production runtime and persisted data are unaffected.
**Decisions and follow-ups:**
- No production pair is selected. Tasks `1.4` and `1.5` remain blocked on this
  task. Continue only after the user approves an architecture resolution.

### 1.2 / 1.3 - Amendment: corpus invalidated

**Status:** Amendment (the `1.2` and `1.3` entries above remain immutable and
are not edited; this entry records what later evidence changed about them.)
**Owner:** Main agent
**Recorded:** 2026-08-22
**Finding:**
- The `1.2` corpus is invalid. Its reference and semantic templates place the
  query verbatim inside every distractor while the expected evidence shares
  almost no surface with the query, making those categories unwinnable by any
  retriever. See this document's "Why Task 1.3 blocked".
- Consequently the `1.2` entry's claim that "all 15 reference variants and all
  30 semantic/paraphrase cases fail the FTS baseline as required" is true as
  written but does not carry the meaning it was recorded to carry: they fail
  for every retriever, not because FTS is lexical.
- The `1.3` entry's blocked verdict stands, but its stated cause — no model pair
  satisfies the quality gates — is superseded. The gates were unreachable.
**Effect on the record:**
- `1.2` status changes from Complete to **Superseded by `1.2R`**.
- `1.3` status remains Blocked; its cause is reattributed and its rerun scope is
  defined in the task specification above.
- `1.2`'s recorded baseline (`R@1 75/135`, `R@3 90/135`, `MRR 0.625`,
  `Evidence R@10 90/150`) is void and is re-recorded by `1.2R`.
- `1.3`'s resource findings are retained; its quality findings and tuned
  constants are void.
**Not changed:**
- Task `1.1` is unaffected and remains Complete.
- The `1.2` harness, metrics, scoring, mutation tests, and privacy audit remain
  in use; only fixture content and the missing solvability assertion are
  replaced.
**Follow-ups:**
- Dispatch `1.2R`, then rerun `1.3`. Tasks `1.4` and `1.5` remain blocked.

### 1.2R - Corpus re-authoring and solvability invariant

**Status:** Complete; baseline approved
**Owner:** `worker-l` (`ses_fd317c5a5ffe20snhqLcOtBV3h`)
**Completed:** 2026-08-23
**Implemented:**
- Replaced the invalid generated corpus with 120 materially distinct cases: 60
  Portuguese, 60 English, five critical, and at least 15 in every required
  overlapping category.
- Added separate answer-key-free structural checks and supervised raw-text
  margin, coverage, and distinctness checks. Expected IDs only label targets
  and never contribute to scores or bypass retrieval.
- Reproduced the reference failure as evidence incompleteness while keeping all
  semantic cases individually solvable and under-served by the FTS baseline.
**Implementation:**
- Files: `frontend/src-tauri/tests/retrieval_evaluation.rs`,
  `frontend/src-tauri/tests/fixtures/corpus.rs`,
  `frontend/src-tauri/tests/fixtures/corpus/*.rs`,
  `frontend/src-tauri/tests/fixtures/evaluation_policy.json`,
  `frontend/src-tauri/tests/fixtures/README.md`, and
  `docs/hybrid-rag/task-1.2r-corpus.md`.
- Approach: hand-authored literal case families use shared schema builders;
  production FTS and context code remain unchanged. The new deterministic
  baseline is Recall@1 `72/135`, Recall@3 `96/135`, Recall@5 `124/135`, MRR
  `0.695833`, Evidence Recall@10 `181/209`, fact coverage `130/149`, forbidden
  contamination `25/121`, and source precision `471/471`.
- Tests-only checkpoint: `1e41b6b` (`frontend/src-tauri/tests/`).
**Not implemented:**
- No production retrieval, model, chunk, fusion, schema, API, persisted-data,
  or model-weight change. No Task `1.3` quality result or constant was retained.
**Why not implemented:**
- These are outside the approved single-task remediation batch. Task `1.3`
  must rerun against this baseline only after explicit user approval.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation -- --nocapture` - pass, 5 tests.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark` - pass, 7 tests against the shared repaired corpus.
- `cargo check --manifest-path "frontend/src-tauri/Cargo.toml"` - pass.
- `cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check` - pass.
- `git diff --check` - pass; unrelated `ROADMAP.md` line-ending warning only.
- Focused privacy scan - pass; matches are scanner/documentation literals and
  the existing `blocked-risk-approval` decision string only.
**Rollback:**
- Restore the pre-`1.2R` fixture/harness files and remove the new family modules
  and report. Production behavior and persisted data are unaffected.
**Decisions and follow-ups:**
- The Task `1.2` baseline is void and replaced by the figures above.
- The user approved this baseline and single-task Batch 4 on 2026-08-23. Rerun
  Task `1.3` in a fresh session; do not inherit the void quality constants.

### 1.3 - Embedding, reranker, and chunk selection rerun

**Status:** Blocked
**Owner:** `worker-l` (`ses_fd06da77cffeQmAh22R2sPO3Fz`)
**Completed:** 2026-08-23
**Implemented:**
- Reran actual tokenizer, embedding, and reranker inference against the approved
  `1.2R` corpus for three budget-viable pairs, with the expanded title-weight
  grid, per-pair title ablation, and per-case concept-proxy disagreement report.
- Corrected the benchmark's Meeting-scope lexical parity and dynamic contracted
  pair labels, then repaired the reference-inference contract so record/replay
  share byte-identical ordered pairs and reject manifest text corruption.
- Recorded e5-base-int8 and mmarco-quint8 as non-production benchmark leaders;
  no model pair or tuned constant was promoted to a production contract.
**Implementation:**
- Files: `frontend/src-tauri/tests/model_benchmark.rs`,
  `frontend/src-tauri/tests/retrieval_evaluation.rs` (mechanical shared-lexicon
  import only), `frontend/src-tauri/tests/fixtures/concept_lexicon.rs`,
  `frontend/src-tauri/tests/fixtures/corpus.rs` (comment only),
  `frontend/src-tauri/tests/fixtures/model_bundle_manifest.json`, and
  `docs/hybrid-rag/task-1.3-model-selection.md`.
- Approach: retain approved resource evidence, remeasure all corpus-dependent
  quality evidence in release mode, tune only on the held-out partition, and
  keep all blocked leaders under non-production manifest fields.
**Not implemented:**
- No production model selection, runtime, schema, API, vector backend, bundle,
  model weights, or approved fusion/chunk constants.
**Why not implemented:**
- Every evaluated pair fails Critical Recall@1 and critical forbidden
  contamination `4/6`. BGE additionally fails pinned Reference Recall@1 and is
  zh/en metadata-nonconforming. Citation/source precision is unevaluated, all
  conforming e5-base pairings need RAM-band approval, and latency viability is
  machine-state-sensitive.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation` - pass, 5 tests; approved baseline unchanged.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark -- --nocapture` - pass, 7 tests; independently rerun by the main agent.
- Release `hybrid_corpus_and_resource_benchmark` - pass with verdict `blocked-quality-gates`; a second run produced digit-identical quality tables.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib` - pass, 394 tests; 2 ignored.
- `cargo check` and `cargo fmt --check` - pass.
- `git diff --check` - pass; unrelated `ROADMAP.md` line-ending warning only.
**Rollback:**
- Restore checkpoint `1e41b6b` for the three checkpointed test files, remove
  `fixtures/concept_lexicon.rs`, and restore the prior report. Production and
  persisted data are unaffected; staged model artifacts remain outside git.
**Decisions and follow-ups:**
- Tasks `1.4` and `1.5` remain blocked. Any continuation requires a new user
  architecture decision; do not choose the best aggregate pair by weakening or
  ignoring critical, source, metadata, RAM, or latency gates.

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

**Reviewer:** Pending
**Verdict:** Pending
**Findings:** Pending
**Required follow-ups:** Pending

### Architecture Review

**Required because:** Model supply chain, new dependencies, retrieval algorithm,
cross-platform native runtime, and a decision that constrains every later
sprint.

**Reviewer:** Pending
**Verdict:** Pending
**Findings:** Pending

## Approval Gates

- **The Windows-only platform scope and this PRD were approved by the user on
  2026-08-22.**
- **Sprint 6.1 closed on 2026-08-22** after all six manual Windows/Tauri smoke
  checks passed. Its task `6.1.R10` defines saved-meeting invariants that Sprint
  4.3 must preserve.
- Sprint 1 TODOs were authorized by the user's PRD approval on 2026-08-22.
- User approval of each dependency-ready batch is required before dispatch.
- User approval is required for the exact production model pair, its vector
  encoding, and any ANN dependency before Sprint 2.
- Explicit user risk approval is required if the selected pair lands in the
  1-1.25 GiB RAM band or exceeds the derived-disk envelope.
- Sprint-close approval is required before Sprint 2 begins.
