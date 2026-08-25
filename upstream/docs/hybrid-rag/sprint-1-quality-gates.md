# Sprint 1: Retrieval Quality Gates

## Status

Task `1.2R` completed independent verification on 2026-08-23 and replaces the
invalid Task `1.2` corpus. Its baseline is approved. Batch 5 Task `1.3F`
completed independent verification on 2026-08-24 and closed the harness-fidelity
and citation-measurement gaps. Its evidence proves the critical contamination
gate is unachievable as staged and one rank-1 case is unachievable as authored.
**On 2026-08-24 the user resolved category (c) with the hybrid remedy** (corpus
patches for `pt-ref-chaves-acesso` and the two vacuous WhatsApp forbidden
facts; carrier-source-state re-scope of the contamination gate, recorded as an
approved `architecture.md` amendment). **Batch 6 Task `1.3G` completed
independent verification on 2026-08-24**: both corpus surfaces are patched with
all four proof families asserted by the harness, the re-scoped gate is enforced,
the baseline is re-recorded, and every critical case now has a positive
production-implementable rank-1 channel with a feasible retrieval-stage
contamination gate.

**The instrument is closed. Batch 7's final Task `1.3` run is complete with a
documented stop, not a model selection.** Amendment 5 admitted the ten
non-critical reference siblings and added their Recall@1 term, while keeping
the five critical/pinned cases fully isolated. Its held-out objective earned
`k=5`, `w_vector=1`, `w_lexical=0.5`, `alpha=0.5`, `beta=1`, and `gamma=0`, but
the resulting production candidate scores Critical Recall@1 **`2/5`**. The
diagnostic probe still finds **78/2160** (mmarco-quint8) and **79/2160**
(mmarco-f32) jointly passing configurations; the earned configuration is
outside that set at strictly better held-out objective value. This is stronger
than the anticipated `3/5` stop evidence: the passing region is not reached by
generalizable signal, and no constants were selected by inspecting critical
results. `pt-ref-sla-suporte` remains the amendment-4 open item (rank 3).
**The user resolved that stop on 2026-08-24 by splitting the critical gate**
(approved `architecture.md` amendment): critical *hydration-window membership*
is the Sprint 1 model-selection gate and passes `5/5`; critical *Recall@1*
keeps its 100% threshold and moves to Sprint 3 as a release gate, owned by
Task `3.2` for `pt-ref-sla-suporte` and `pt-ref-nps-detrator` and by Task
`3.6` for `pt-ref-chaves-acesso`. **Task `1.3` is therefore complete and
`e5-base-int8` + `mmarco-quint8` is the approved production pair** at the
earned constants above, int8 vector encoding, measured 1118.3 MiB inside the
pre-approved band. Tasks `1.4` and `1.5` are unblocked. Task `1.1` is
unaffected. No further Sprint 1 instrument iteration is permitted.

**Batch 8 Task `1.4` completed independent verification on 2026-08-24.** Exact
search is selected at 250k (p95 `48.2 ms`, recall@150 `1.0000`); ANN was not
evaluated because its latency trigger did not occur. The user approved the
measured two-snapshot rebuild peak (`1296.5 MiB`) under a 1.30 GiB transient
ceiling while retaining the 1.25 GiB steady-state cap. Task `1.5` is next and
runs alone.

**Batch 9 Task `1.5` implementation completed on 2026-08-25, but Sprint review
returned changes requested.** Its local manifest, staging, and Rust 1.88 work
is retained. **Batch 10 `1.R1` is complete:** the active root workflow now
stages, verifies, and reference-tests the production bundle on Rust 1.88.0.
**Batch 11 `1.R2` is complete:** the production package/provenance boundary is
fail-closed, with full backup integrity recovery. `1.R3` remains before Sprint
close to remeasure the exact backend's transient envelope.

**Batch 12 `1.R3` is complete:** sparse document IDs survive crash replay and
compaction, and the measured same-process active/shadow/delta/ONNX-session peak
is 1317.9 MiB on independent rerun, within the approved 1.30 GiB transient
ceiling. **Batch 13 `1.R1a` and `1.R3a` are complete:** the active CI staging
path resolves from the checkout root, and bounded journal publication replays
self-contained upsert payloads correctly across same-document concurrent
 updates. Final code and architecture reviews approve. **Hosted closure evidence
 (2026-08-25):** root Windows workflow run
 [`#31`](https://github.com/armandotreuk/Meetly_Treuk/actions/runs/32907148572)
 at `8a7d566` passed both jobs: pinned-toolchain cargo check; llama-helper
 sidecar build; manifest contract; staged-bundle verification; staged
 tokenizer/embedding/reranker reference inference; full Tauri CPU packaging;
 and MSI/NSIS artifact uploads. The hosted-root requirement is complete.

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

### What Task 1.3F established (2026-08-24)

- Fidelity fixes for deleted, dirty/stale semantic rows, and authoritative
  hydration reduce overall forbidden contamination from `30/121` to `15/121`
  and close `pt-ref-nps-detrator` at rank 1 without changing corpus content or
  gates.
- Citation/source precision is now production-typed, mutation-tested, and
  passes for both retained pairs at `602/602`.
- `pt-ref-sla-suporte` is category (b): its raw vector channel ranks the target
  first, while held-out-tuned fusion/aggregation ranks it second.
- `pt-ref-chaves-acesso` is category (c): lexical/title margins are negative
  and the raw vector channel ranks it fifth; only the non-production concept
  proxy makes it solvable at rank 1.
- Four hittable forbidden facts are trapped in non-required current notes
  inside their required meetings. Because each hydrated pool has only 6-8
  documents, below `EVIDENCE_K=10`, every ordering retains the carrier. The two
  remaining WhatsApp facts have no document carrier. Both retained rerankers
  have `0/2160` configurations that jointly pass Critical Recall@1, zero
  critical contamination, and exact-term no-regression.

Category (a) fixes are accepted into the final instrument. Category (b) remains
eligible for the final held-out retune. Category (c) requires an explicit user
decision before the final Task `1.3` run; the orchestrator may not patch the
corpus or re-stage a gate implicitly.

**Category (c) resolution (2026-08-24, user decision):** the hybrid remedy,
with four binding amendments recorded in the decision log — (1) the
contamination gate keeps a real retrieval-stage half, re-scoped to forbidden
facts carried by superseded/stale/deleted sources, while current-content
contradictions move to the defined-but-deferred answer-stage non-assertion
gate (`architecture.md` amendment, approved); (2) the new WhatsApp carriers
must sit in superseded sources or neighbour meetings so they do not recreate
the trapped-carrier structure; (3) the `pt-ref-chaves-acesso` patch keeps its
terminological gap and must be proven by the production-channel rank-1
admissibility check turning positive while the FTS baseline still fails;
(4) `pt-ref-sla-suporte` stays category (b) and escalates back to the user if
the final held-out retune still lands it below rank 1. Task `1.3G` implements
(1)-(3) plus the baseline re-record and baseline-harness alignment.

### What Task 1.3G established, and the one remaining gap (2026-08-24)

Verified independently by the orchestrator (corpus diff is exactly three
content lines across the two approved surfaces; `retrieval_evaluation` passes
6/6 including both new mutation tests):

- Both WhatsApp forbidden facts now have real carriers in *neighbour* meetings
  as explicitly superseded drafts, and the case reports
  `FEASIBLE_BY_ORDERING` with `forbidden_bearing_docs=0`. Amendment 2 holds:
  no new trapped carrier was created.
- The re-scoped contamination gate is falsifiable and does real work: the FTS
  baseline fails it at `17/107` while the hybrid pipeline reaches `1/107`
  overall and `0/2` on criticals.
- The baseline re-record moves exactly one numerator (`25/121` → `26/121`),
  attributed to the one new carrier reaching retained context; the chaves
  title is FTS-invisible and deleted-row alignment measured inert (`15/15`).
- Every critical case now has a positive production-implementable rank-1
  channel, and admissibility is enforcing rather than report-only.

**The remaining gap is the tuning objective, not the models.** The held-out
partition (`model_benchmark.rs` `tune_idx`) excludes all 15 reference-category
cases plus every critical case, and the objective's four terms — exact-term
violations, semantic Recall@3 misses, overall Recall@3 misses, MRR — contain
no term that rewards solving a terminological-gap, stale-version, or
cross-section case. The tuner therefore optimizes on a distribution from which
the phenomena the critical gates measure have been removed, and is then graded
on them. With 78-79 configurations of the same pair passing every gate
jointly, the passing region demonstrably exists and the objective cannot see
it. The final `1.3` run is amended accordingly (see its "Final-run amendments
(2026-08-24, post-`1.3G`)").

Four items recorded for the register, none blocking:

1. The `1.3G` report omits the tuned constants. Because chaves's only positive
   production channel is `title` and `1.3F` tuned `title_beta` to `0`, the run
   may have gated a case on a channel the fused configuration switched off.
   The final run MUST print `k`, `w_vector`, `w_lexical`, `alpha`, `beta`, and
   per-candidate `gamma` beside the gate table.
2. `0/2160` → `79/2160` is not attributable to the corpus patch alone: the
   probe's gate definition also changed (all-facts `0/6` → retrieval-stage
   `0/2`). The two figures are not directly comparable.
3. 74 of the 107 retrieval-stage forbidden facts have no fixture carrier at
   all — the same vacuity `1.3F` found, fixed only for the two critical facts
   that were in the approved scope. Percentage forms of retrieval-stage
   contamination are therefore diluted and must not be read as rates; the
   critical gate is count-based and unaffected.
4. `1.3G` added a `metadata_conforming` filter to embedding-family selection.
   It formalizes existing intent (paraphrase-MiniLM's max-seq 128 was already
   non-conforming) and is correct, but it was not in the `1.3G` specification.

Recorded plainly: the chaves title now carries three of the question's content
terms, so that case's discriminating signal is title-token overlap rather than
semantic bridging. The terminological gap survives where it is documented (the
evidence text: *trocar/chaves* versus *rotação/credenciais*) and baseline
falsifiability is intact because titles are not FTS-indexed, but the case
tests something narrower than it did. The architecturally correct remedy —
single-turn query expansion — does not exist anywhere in this program and is
registered as Sprint 3 Task `3.6`.

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
| 1.3F | Gate closure | Audit harness fidelity against the architecture pipeline, prove or refute gate admissibility for the failing critical gates, add citation-precision simulation, and deliver a per-blocker verdict table. | L | `worker-l` (`ses_fce6f62e3ffekuqxK1x52fEwoT`) | 1.2R, 1.3 rerun evidence | **Passed:** all five deliverables, constructive admissibility report, production-typed citation precision `602/602` for pairs B/C, falsifiability mutation, `0/2160` feasibility result, privacy and boundary checks. | Tests/evidence only; restore committed `tests/` state; no production effect. |
| 1.3G | Corpus patch + gate re-scope | Implement the 2026-08-24 category (c) decision: patch `pt-ref-chaves-acesso` and the two vacuous WhatsApp forbidden carriers; re-scope critical contamination by carrier source state; promote the admissibility invariant to enforcing for retrieval-stage facts; re-record the baseline and align the baseline harness. | M | `worker-l` (primary `ses_fcbf894c0ffe0XwFSDMSavxqDj`; residual fix `ses_fcb41049cffefT5nm7rG3j6sNE`) | 1.3F | **Passed:** rank-1 admissibility positive for `chaves-acesso` on the title channel with baseline still failing; both WhatsApp facts carried in superseded sources and `FEASIBLE_BY_ORDERING`; classification `107/14` pinned by test; enforcing admissibility plus five rejecting mutations; new baseline recorded (`26/121`); structural/margin/distinctness/mutation/privacy checks pass. | Restore fixtures/harness from `e209b5d`; test tooling only, no production effect. |
| 1.3 | Model selection | Benchmark and select the bundled multilingual embedding and reranker pair plus chunk policy. | L | `worker-l` (`ses_fcaf0a683ffeJQbDb2utDAyIQW`); earlier runs `ses_fd06da77cffeQmAh22R2sPO3Fz`, `ses_fd65db999ffe5gwCX1YNm12F5w` | 1.2R, 1.3F, 1.3G | **Complete — pair selected (2026-08-24).** Amendment-5 retune with the five critical/pinned cases isolated earned fully disclosed constants; every Sprint 1 gate passes, including critical hydration `5/5`, critical facts `9/9`, critical retrieval-stage contamination `0/2`, citation precision `602/602`. Critical Recall@1 `2/5` is retained at 100% as a Sprint 3 release gate per the approved gate split. Selected: `e5-base-int8` + `mmarco-quint8`, int8 encoding, 1118.3 MiB in the approved band. | Revert the approved addendum and re-block `1.4`/`1.5`; no production default ships until Sprint 2. |
| 1.4 | Vector backend | Benchmark exact search and, only if needed, a pure-Rust HNSW candidate at 250k scale. | L | Complete after `1.R3` review remediation | 1.3 | **Complete:** exact selected; no ANN trigger. Post-`1.R3a` 250k matrix: p95 `61.1 ms`, recall@150 `1.0000`, steady `1133.8 MiB`, and conservative governing active+shadow+delta+sessions peak `1319.9 MiB` within 1.30 GiB. | Delete benchmark/report harness; no production backend ships yet. |
| 1.5 | Model supply chain | Implement the small bundle manifest and reproducible hash/license verification pipeline; reconcile Rust MSRV. | M | Complete after `1.R1`/`1.R2` review remediation | 1.3, 1.4 | Root CI exercises Rust 1.88.0, staged-bundle verification, and reference inference; strict package/provenance/recovery checks pass. | Remove additive manifest/fetch verification; no runtime behavior yet. |
| 1.R1 | Active CI gate | Move the toolchain, staging, semantic manifest validation, and reference inference to the active root Windows workflow. | L | `worker-l` (`ses_fc6bd168effeAUd4Tj4WGqpoyj`) | 1.5 | **Complete:** root CI reads/asserts Rust 1.88.0, stages/verifies ten artifacts, and executes staged tokenizer/embedding/reranker inference before Tauri build. | Revert root workflow and harness gate adapter; no runtime behavior changes. |
| 1.R2 | Package/provenance boundary | Fail closed for the exact selected bundle, resolve exporter attribution, reject unexpected package content, and recover a lone interrupted publish. | M | `worker-l` (`ses_fc634146bffevaVpdPBU0R2NE6`) | 1.5, 1.R1 | **Complete:** 21 strict parser/verifier tests and eight script self-test families pass; immutable export/notice evidence is recorded. | Revert Task 1.R2 package/verifier changes; no runtime retrieval behavior. |
| 1.R3 | Exact benchmark envelope | Repair sparse-ID replay semantics and measure active+shadow+delta+selected sessions in one 250k process. | L | `worker-l` (`ses_fc60bbe8bffehPVuVVU0jP9OyO`) | 1.4, 1.5, 1.R2 | **Complete:** 10 deterministic tests, including a mutation-proven sparse-ID regression; independent 250k p95 `61.1 ms`, recall@150 `1.0000`, and combined peak `1317.9 MiB` pass. | Revert benchmark/report changes; no production backend ships. |
| 1.R1a | CI staging path | Correct the checkout-root path for the active Windows staging step. | S | `worker-l` (`ses_fc5b84814ffe609Kxu6ZtJI0wi`) | 1.R1 | **Complete:** root YAML lint and the exact checkout-root `-SelfTest` invocation pass; staging, verification, inference, and packaging retain their required order. | Restore the prior single workflow path; no runtime effect. |
| 1.R3a | Bounded journal publication | Make the benchmark-local journal self-contained and publish only through a captured canonical bound. | M | `worker-l` (`ses_fc5b8478fffepoG91Zm7Cjreoj`) | 1.R3 | **Complete:** 13 deterministic tests cover same-document concurrent commits, repeated upserts, upsert/delete tombstones, and crash replay; independent 250k matrix passes. | Restore the prior benchmark fixture/report; no production backend ships. |

## Dependency Order

`1.2 -> 1.2R -> 1.3F -> 1.3G -> 1.3 -> 1.4`

`1.3 -> 1.5`

`1.1` and `1.2` are independent if the evaluation harness is kept outside
`api/chat.rs` inline tests. Tasks `1.3F`, `1.3`, and `1.4` are L and run
alone; `1.3G` is M and runs alone because it changes the shared instrument.
Task `1.5` may start after `1.3`, but should not run concurrently with
`1.4` if both need to change `Cargo.toml`, benchmark targets, or model
artifact scripts.

`1.3G` is a hard blocker for the final `1.3` selection run: it changes corpus
content and the contamination gate definition, so every quality figure the
final run produces must be measured against the post-`1.3G` instrument and its
re-recorded baseline.

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

### 1.3G - Corpus patch and contamination gate re-scope [M]

**Outcome:** The instrument implements the user's 2026-08-24 category (c)
decision. Every gate the final `1.3` run is measured against is proven
admissible on production-implementable channels, and the corpus keeps its
realism (contradictory notes inside real meetings) instead of being sanitized
around an unachievable gate.

**Authority:** the 2026-08-24 user decision (hybrid remedy, four amendments —
see "Category (c) resolution" above) and the approved `architecture.md`
amendments: the re-scoped "Forbidden-fact contamination (retrieval stage)"
gate row, the "Answer-stage non-assertion (deferred evaluation)" row, the
"Gate admissibility" row, and the new Corpus Solvability admissibility
bullet. This task implements an approved gate re-scope; it is not authorized
to change any other threshold or metric definition.

**Hard boundaries:**

- Corpus content changes are limited to exactly two surfaces:
  `pt-ref-chaves-acesso` and the WhatsApp forbidden-fact carriers. Every
  other case — including `pt-ref-sla-suporte`, which stays category (b) — is
  untouched.
- No model inference beyond re-running the existing diagnostics; no tuning;
  reference/critical cases stay outside every tuning path.
- No production files, PRDs, or further `architecture.md` edits.
- Work inside `frontend/src-tauri/tests/` plus a new report
  `docs/hybrid-rag/task-1.3g-corpus-gate-patch.md`.

**Required implementation:**

1. **`pt-ref-chaves-acesso` patch (amendment 3).** Keep the terminological
   gap — the question says trocar/chaves, the decision says rotação periódica
   de credenciais; that tension is the point of the case. Give the target a
   legitimately learnable discriminating signal on a production channel, the
   way real corpora do (a meeting deciding credential rotation is plausibly
   titled with that topic, or carries an adjacent sentence sharing part of
   the question's vocabulary). Required proof, all asserted:
   - The FTS baseline still fails the case (falsifiability preserved).
   - The answer-key-free structural checks still pass.
   - The supervised raw-text margin is positive on the lexical or title
     channel — the `CONCEPT_LEXICON` may no longer be the case's only
     positive channel.
   - The `[SUPERVISED:rank1-admissibility]` production-channel check reports
     `any_positive_channel=true` for this case.
2. **WhatsApp forbidden carriers (amendment 2).** Author real carrier text
   for `"apenas 3 dias"` and `"apenas 4 dias"` in superseded/draft sources or
   topical neighbour meetings — NOT in current notes inside
   `fixture-whatsapp-retention`. The trapped-carrier structure `1.3F` proved
   unachievable must not be recreated. Required proof: post-patch,
   `[SUPERVISED:evidence-admissibility]` reports `FEASIBLE_BY_ORDERING` for
   the case with both facts carried (`UNHITTABLE_BY_CONSTRUCTION` no longer
   appears), and the pinned reference case still fails the baseline in its
   recorded evidence-completeness shape.
3. **Gate re-scope by carrier source state (amendment 1).** Classify every
   forbidden fact by its carrier's source state, computed from fixture text:
   *retrieval-stage* (all carriers are superseded, stale-derived, or deleted
   sources) versus *answer-stage* (any carrier is current authoritative
   content inside an expected meeting). The critical contamination gate
   evaluates to 0 over retrieval-stage facts only. The four current-note
   facts (`"dias 5 e 15"`, `"renovação mensal"`, `"em um dia inteiro"`,
   `"cupom como resposta padrão"`) become answer-stage facts: reported with
   denominators in every run, never gated in Sprint 1, and printed with their
   classification so Sprint 3/4 inherit an explicit list. Overall
   contamination remains reported as before (informational).
4. **Promote the admissibility invariant to enforcing for retrieval-stage
   facts.** A retrieval-stage forbidden fact that is not
   `FEASIBLE_BY_ORDERING`, or a critical case without a positive
   production-implementable channel, now FAILS the supervised test instead of
   printing a report line. Answer-stage facts are exempt from the ordering
   requirement by definition and remain report-only.
5. **Baseline re-record and harness alignment.** Re-record `expectedBaseline`
   in `evaluation_policy.json` from the patched corpus (the patch changes
   fixture content, so the approved figures are superseded). In the same
   change, align `retrieval_evaluation.rs::setup_case` deleted-meeting FTS
   insertion with production cascade semantics — the `1.3F` spillover item —
   since the re-record unpins the numbers that froze it. Report old versus
   new baseline side by side with denominators.

**Explicit non-goals:** no model pair selection, no constants promotion, no
chunk policy, no changes to any other case's content, no threshold changes
beyond the approved re-scope, no answer-stage evaluation (defined, deferred,
Sprint 3/4).

**Acceptance criteria:**

- All five implementation points proven by harness assertions, not report
  prose; every percentage carries its denominator.
- Distinct-shape floor, corpus size floors, mutation tests, and the privacy
  scan still pass on the patched corpus.
- `cargo test --test retrieval_evaluation` and `--test model_benchmark` pass;
  the canonical release benchmark runs clean end to end and prints the new
  fact classifications.
- `git diff` confirms the corpus-content boundary (two surfaces only).

**Required verification:**

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
$env:MEETLY_RAG_MODELS_DIR = "$env:TEMP\opencode\meetly-task13\models"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation -- --nocapture
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark
$env:MEETLY_RAG_BENCH = "1"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark hybrid_corpus_and_resource_benchmark -- --nocapture
Remove-Item Env:MEETLY_RAG_BENCH -ErrorAction SilentlyContinue
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check
git diff --check
```

**Worker report additions:** before/after supervised margins and
rank1-admissibility lines for the two patched surfaces; the full forbidden-
fact classification table (fact, carriers, source states, retrieval-stage or
answer-stage) for at least all critical facts plus per-class counts over all
121; old-versus-new baseline table; and the post-patch critical gate outlook
(informational — selection stays with the final `1.3` run).

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

**Post-`1.3G` amendments (2026-08-24):**

- The final run measures against the post-`1.3G` instrument and its
  re-recorded baseline; `1.3F`'s category (a) fidelity fixes are retained.
- Critical contamination is evaluated under the re-scoped definition
  (retrieval-stage facts only); answer-stage facts are reported with
  denominators as informational output, never as a Sprint 1 gate result.
- Held-out constants are retuned from scratch on the final instrument; no
  constant from any earlier run carries forward.
- `pt-ref-sla-suporte` escalation rule (amendment 4): if the final held-out
  retune still ranks it below 1, report it as an explicit open item for the
  user — it is not a silent gate failure and not a reason to touch the corpus
  or constants outside the held-out objective.

**Final-run amendments (2026-08-24, post-`1.3G`) — amendment 5:**

`1.3G` proved the gates reachable by this pair (78-79 of 2160 configurations
pass jointly) while the held-out objective selects a `3/5` configuration. The
cause is a distribution-shifted split, not model capability. The final run
therefore changes the tuning partition and objective, and nothing else:

- **Partition.** Admit the **10 non-critical reference-category cases** into
  the held-out tuning partition. The 5 designated critical/pinned cases
  (`fixture-whatsapp-retention`, `pt-ref-cobranca-regua`,
  `pt-ref-chaves-acesso`, `pt-ref-sla-suporte`, `pt-ref-nps-detrator`) remain
  fully isolated and MUST NOT be inspected by any tuning path. The current
  filter excludes every case carrying the `reference_whatsapp` category; it
  must exclude only `critical` cases and the pinned reference case.
- **Objective.** Add a reference-category Recall@1 miss term over those 10
  admitted cases, ranked after semantic misses and before overall Recall@3
  misses. Record the full lexicographic key in the report.
- **Rationale to carry into the report.** This trades a small reduction in the
  gate's independence — the admitted cases are structural siblings of the gate
  cases — for a train/test split drawn from the same distribution, which is
  the sound arrangement. Selecting constants by inspecting critical-case
  results remains forbidden: a configuration that passes only because it was
  chosen on the gate is overfitting, and would retroactively invalidate the
  benchmark.
- **Mandatory constants disclosure.** Print `k`, `w_vector`, `w_lexical`,
  `alpha`, `beta`, and per-candidate `gamma` beside every gate table, plus the
  objective vector of the winning configuration. A gate result reported
  without its constants is not reviewable.
- **Feasibility corroboration.** Re-run the diagnostic probe and report both
  the passing-configuration count and whether the tuned configuration falls
  inside the passing set. If it does not, state the objective distance between
  them.
- **If the retune still yields `3/5`:** stop and report. Do not adjust the
  partition again, do not select from the passing set by inspection, and do
  not weaken a gate. That outcome is evidence that the passing region is not
  reachable from generalizable signal, and the decision — gate split versus
  dated exception against Sprint 3 Task `3.6` — belongs to the user.

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
| 2026-08-24 | Accept Task `1.3F` as complete evidence and keep the final Task `1.3` run blocked for a user decision on category (c). | Independent checks pass; citation precision and two fidelity findings are closed, but `pt-ref-chaves-acesso` has no production-implementable rank-1 channel and the critical contamination gate is unachievable as staged. Running selection before choosing corpus patches or gate re-staging would knowingly benchmark against an impossible instrument. | Rerun selection with the impossible gates; patch the corpus or gates without user approval; treat all findings as model failures. | Main agent |
| 2026-08-24 | Resolve category (c) with the hybrid remedy: patch `pt-ref-chaves-acesso` and the two vacuous WhatsApp carriers; re-scope the contamination gate by carrier source state. | Requiring retrieval to erase a current note inside the correct meeting asks the wrong layer to censor authoritative content — hydration includes current notes wholesale by design, and the recorded production failure was an *answer* asserting the wrong value. Corpus-only patching would sanitize the realistic contradictory-notes structure the corpus exists to reproduce; gate-restaging-only would abandon rank-1 where evidence shows it achievable (4/5 post-fix) and leave two facts measuring nothing. | Option 2 (corpus patches only); option 3 (gate re-staging only); keep the gates as staged and accept a permanently blocked program. | User |
| 2026-08-24 | Bind four amendments to the hybrid remedy. | (1) Retrieval keeps a real contamination gate over superseded/stale/deleted carriers — achievable and already measured (30/121 → 15/121 when the stale path closed); the answer-stage non-assertion gate is defined now, measured only when an answer pipeline exists (Sprint 3/4), and Sprint 1 may not claim it. (2) New WhatsApp carriers go in superseded/neighbour sources so the patch cannot recreate the trapped-carrier structure. (3) The `chaves-acesso` patch keeps its terminological gap and is accepted only when the production-channel rank-1 admissibility check turns positive with the baseline still failing. (4) `pt-ref-sla-suporte` stays category (b) and escalates to the user if the final retune leaves it below rank 1. | Adopt the remedy without proof obligations; patch `sla-suporte` too; drop the retrieval-stage half of the gate entirely. | User |
| 2026-08-24 | Reroute the Batch 6 worker to the same Ox Alpha model via `openrouter/stealth/ox-alpha` after `opencode-go` returned provider `Insufficient balance` for the fresh `1.3G` dispatch. | This is a transport change preserving the model pin, not a fallback; the alternative was pausing Sprint 1 on billing. The route completed the residual classifier correction successfully this time (unlike its empty-output failure during Batch 4). | Keep retrying the unavailable `opencode-go` endpoint; substitute another model. | User |
| 2026-08-24 | Accept Task `1.3G` as complete and unblock the final Task `1.3` selection run. | Independent reruns pass (`retrieval_evaluation` 6/6, `model_benchmark` 9/9), the corpus-content diff touches exactly the two approved surfaces (three content lines), all four proof families are harness assertions with rejecting mutations, the re-scoped critical contamination gate passes `0/2`, and every critical case has a positive production-implementable rank-1 channel. Remaining critical-rank misses are model/aggregation evidence owned by the final run. | Keep `1.3` blocked despite the closed instrument; reopen the corpus decision; accept the patch without independent verification. | Main agent |
| 2026-08-24 | Approve the `architecture.md` amendments: carrier-source-state gate re-scope, answer-stage non-assertion row, gate-admissibility requirement, and the Corpus Solvability admissibility bullet. | Gate definitions live in `architecture.md` and require user approval to change; the admissibility requirement generalizes the lesson that consumed two L-sized benchmark tasks — a zero-tolerance gate without an existence proof on production-implementable channels is an unfalsifiable trap, not a gate. | Record the re-scope only in this sprint document; leave admissibility as a sprint convention rather than an architecture invariant. | User |
| 2026-08-24 | Add Task `1.3G` (M) as a hard blocker for the final `1.3` selection run, and fold the baseline re-record plus baseline-harness deleted-row alignment into it. | The remedy changes corpus content and a gate definition, so every figure the final run produces must come from the post-patch instrument and its re-recorded baseline; the `1.3F` spillover alignment rides along because the re-record unpins the frozen numbers. | Fold the patch into the final `1.3` run itself; dispatch the alignment separately; skip re-recording the baseline. | User |
| 2026-08-24 | Accept Task `1.3G` after orchestrator verification, and record four non-blocking register items. | The corpus diff is three content lines on the two approved surfaces, `retrieval_evaluation` passes 6/6 independently, the re-scoped gate is falsifiable (baseline `17/107` fails, hybrid `1/107`), and the baseline moved by exactly one attributable numerator. The register items — undisclosed tuned constants, the non-comparable `0`→`79/2160` figures, 74 carrierless facts diluting percentages, and the unspecified `metadata_conforming` filter — affect reporting and interpretation, not correctness. | Reject for the unspecified family filter; require a further corpus pass for the carrierless facts. | User |
| 2026-08-24 | Amend the final `1.3` run (amendment 5): admit the 10 non-critical reference-category cases into the held-out tuning partition and add a reference-category Recall@1 term to the objective; keep the 5 critical/pinned cases fully isolated. | `1.3G` measures `78-79/2160` configurations of the existing pair passing every gate jointly while the tuned configuration scores `3/5`. The partition excludes all 15 reference cases and the objective has no term for gap-type retrieval, so the tuner optimizes on a distribution with the graded phenomena removed. Admitting non-gate siblings converts a distribution-shifted split into a same-distribution one; the 5 designated cases stay isolated. | Select constants from the passing set by inspection (overfitting on the gate); change nothing and accept `3/5`; benchmark a larger embedding model. | User |
| 2026-08-24 | Forbid selecting constants by inspecting critical-case results, in this and every later run, and require constants disclosure beside every gate table. | A configuration that passes only because it was chosen on the gate makes the gate self-fulfilling and would retroactively invalidate the whole benchmark program. A gate result reported without its constants is not reviewable — `1.3G` omitted them while gating a case whose only positive channel may have carried zero weight. | Permit selection from the diagnostic passing set; keep constants disclosure optional. | User |
| 2026-08-24 | Bound the final `1.3` run: if the retune still yields Critical Recall@1 `3/5`, stop and report rather than iterating further on the instrument. | Five instrument-side tasks have run (`1.2`, `1.2R`, `1.3F`, `1.3G`, and the final retune). `1.3F` and `1.3G` each found something real, but the remaining question is narrow enough to answer once; a `3/5` outcome is itself evidence that the passing region is not reachable from generalizable signal, and the resulting choice — gate split versus dated exception against Sprint 3 Task `3.6` — belongs to the user. | Allow further partition or objective iteration; permit a gate change inside the run. | User |
| 2026-08-24 | Register single-turn query expansion as Sprint 3 Task `3.6` rather than adding it to Sprint 1. | It is the architecturally correct remedy for `pt-ref-chaves-acesso` and does not exist anywhere in this program — the implemented rewrite path is follow-up-only and `chaves` is single-turn. But it is a genuine architecture decision (hand-authored lexicon reintroduces the non-production `CONCEPT_LEXICON` pattern; LLM expansion puts a provider round-trip inside the retrieval path of a local-first product; pseudo-relevance feedback would likely drift toward the distractors that own the surface vocabulary), Sprint 1 excludes production retrieval behavior, and building it to fix a gate case then grading the models on that case repeats the overfitting problem one stage upstream. Sprint 3 already carries the query-variant plumbing. | Build expansion inside Sprint 1 alongside the final run; defer it without registering a task. | User |
| 2026-08-24 | Resolve the documented stop by splitting the critical gate (option 1, refined), not by a dated exception (option 2). | The final run shows the raw bi-encoder ranking the expected meeting **first for four of the five critical cases** (only `chaves` at 4): the demotions come from fusion and meeting aggregation, which Sprint 3 Task 3.2 builds and tunes and which Sprint 1 cannot fix by choosing a different embedding pair. All five critical meetings sit inside the hydration window (ranks 1,1,2,3,2) with critical facts 9/9 and zero retrieval-stage contamination, so the product outcome is already correct and the residual failure is ordinal position. Option 2 was rejected on its own premise: query expansion addresses only `chaves`; `sla-suporte` and `nps-detrator` already have raw vector rank 1, so a Task `3.6`-contingent exception would expire unredeemed. | Option 2 (dated exception contingent on Task `3.6`); keep Recall@1 as a Sprint 1 gate and leave selection blocked; lower the threshold below 100%. | User |
| 2026-08-24 | Approve `intfloat/multilingual-e5-base` (dynamic int8, 768-d, MIT) + `cross-encoder/mmarco-mMiniLMv2-L12-H384-v1` (quint8_avx2, Apache-2.0) with int8 vector storage as the production pair, at the earned constants `k=5, w_vector=1, w_lexical=0.5, alpha=0.5, beta=1, gamma=0`, support cap 3, chat depth 50, search depth 25, batch 1, ORT intra-op 4; chunk profile 384/64. | Every Sprint 1 gate passes under the split: critical hydration 5/5, critical facts 9/9, critical retrieval-stage contamination 0/2, pinned reference rank 1, exact-term 90/90, overall R@3 135/135, R@5 135/135, EV@10 209/209, semantic 30/30 vs 0/30 baseline, NDCG non-degraded, citation precision 602/602, RAM 1118.3 MiB inside the pre-approved band, reranker depth-50 cost 720 ms inside the 900 ms sub-budget. Constants are the held-out objective's output with the five critical/pinned cases never inspected. | Select the f32 reranker variant (reference-only, no quality gain); defer selection pending Sprint 3 fusion work. | User |
| 2026-08-24 | Record the title dependence of the selected configuration in the architecture addendum. | The mandatory ablation shows reference-category Recall@1 falling 12/15 → 7/15 and overall MRR 0.9681 → 0.9451 when title scoring is removed, while semantic Recall@3 holds at 30/30. The embedding channel is independently strong, but reference-family gate performance is materially title-assisted and must not be attributed solely to the embedding model. | Report the tuned aggregate only and omit the dependence. | User |
| 2026-08-24 | Accept final Task `1.3` as a documented-stop outcome; select no production pair and request the gate-split versus dated-exception decision. | Independent release rerun reproduces the amendment-5 tuning result: the five critical/pinned cases remain isolated, objective `[0,0,0,0,0,2166666]` earns `k=5`, `w_vector=1`, `w_lexical=0.5`, `alpha=0.5`, `beta=1`, `gamma=0`, yet Critical Recall@1 is `2/5`. The quint8 feasibility probe proves `78/2160` joint-passing configurations exist but the earned configuration lies outside them at strictly better held-out objective value; selecting a passing configuration by inspection remains forbidden. `pt-ref-sla-suporte` is explicit amendment-4 open item at rank 3. | Iterate the corpus/gates/partition/objective again; select from the passing set by inspecting critical outcomes; approve a pair despite the failed critical gate. | Main agent |
| 2026-08-24 | Approve the Task `1.4` two-snapshot rebuild accounting and its 1.30 GiB transient ceiling; select exact search and do not evaluate ANN. | The 2x overlap is exactly active plus shadow snapshots; a reader's `Arc` adds no third vector allocation. At 250k, exact search has p95 `48.2 ms` and recall@150 `1.0000`; steady-state RAM is `1113.4 MiB` in the approved 1-1.25 GiB band and the measured two-snapshot rebuild peak is `1296.5 MiB`, within the explicitly approved 1.30 GiB transient ceiling. ANN is not its permitted latency remedy because exact latency passes, and would add memory. Any true third snapshot or peak above 1.30 GiB remains blocking. | Treat the reader handle as an unmeasured third snapshot; leave the transient peak ungoverned; evaluate ANN despite an exact latency pass. | User |
| 2026-08-25 | Accept Task `1.5` and replace the stale Rust 1.77 declaration with the exact Rust 1.88.0 toolchain contract. | The manifest encodes the approved selected pair, separate tokenizer contracts, ten length/hash-pinned artifacts, and license attribution. Fresh-cache staging fetches only immutable revision URLs, verifies each artifact, and atomically publishes the complete package. Independent focused tests (19/19), staging verification, typecheck, Cargo check, rustfmt, diff check, and workflow YAML lint pass. The locked graph's effective floor is Rust 1.88, above ORT's own 1.81 floor, so 1.77 and floating CI `stable` were false contracts. | Retain Rust 1.77; use floating CI stable; defer hash verification to Sprint 2; allow a partial bundle. | Main agent |
| 2026-08-25 | Rescind Task `1.5` acceptance pending review remediation `1.R1`-`1.R3`. | Sprint review proved that the edited CI workflow is nested under `upstream/` and inert; the active root workflow remains on floating stable and does not stage, semantically validate, or run reference inference. Review also found incomplete production-contract/provenance enforcement and unproven vector rebuild RAM/recovery evidence. The local implementation evidence remains valid but cannot satisfy the release gate alone. | Close Sprint 1 on local checks; treat nested workflow edits as CI coverage; defer the findings to Sprint 2. | Main agent |
| 2026-08-25 | Accept review remediation `1.R1`: activate the root Windows CI release gate. | Independent staged-bundle verification passes all ten artifacts and the existing reference harness passes tokenizer, embedding, and reranker inference against the staged production bundle. The actual root workflow now reads/asserts Rust 1.88.0 in both jobs and runs contract validation, staging, bundle verification, and inference before Tauri build. | Retain the inert nested workflow as CI authority; use hash staging without inference; defer the root workflow fix. | Main agent |
| 2026-08-25 | Accept review remediation `1.R2`: harden the approved package/provenance boundary. | The parser rejects every selected-contract/provenance substitution in its 21 focused tests; source model/export attribution and exact MIT notice are pinned; the staged bundle is its only packaged authority; and the script self-test proves full-integrity backup recovery plus rejection/preservation of corrupt, missing, ambiguous, divergent, or unmanifested packages. | Keep generic schema-only manifest validation; retain duplicate packaged resources; restore backups without integrity validation; retain a generic MIT template. | Main agent |
| 2026-08-25 | Accept review remediation `1.R3`: repair sparse-ID exact search and replace arithmetic RAM evidence with a true combined measurement. | The new canonical sparse-ID regression fails under the former ID-as-row mask and passes after the shared row-identity repair. The independent 250k release matrix holds active snapshot, streamed shadow, delta/tombstones, and both warmed selected ONNX sessions in one process: p95 `61.1 ms`, recall@150 `1.0000`, steady `1134.8 MiB`, and peak `1317.9 MiB`, 13.3 MiB below the approved 1.30 GiB transient ceiling. | Retain the ID-indexed mask; use retained session arithmetic; keep the superseded 1.25 GiB transient failure label; evaluate ANN. | Main agent |
| 2026-08-25 | Reopen Sprint review for `1.R1a` and `1.R3a`. | Post-remediation review proves the root workflow invokes the staging script without its `upstream/` prefix, blocking its required CI gate. It also identifies a benchmark publication race that can mark concurrently committed journal entries published before application, plus an upsert/delete sequence that lacks a payload. Both invalidate release evidence until corrected and retested. | Close Sprint 1 on local checks; defer these correctness/CI defects to Sprint 2. | Main agent |
| 2026-08-25 | Accept review corrections `1.R1a` and `1.R3a` pending final re-review. | The root workflow now invokes the staging script through its checkout-root-relative `upstream/...` path; YAML lint and the exact invocation's offline recovery self-test pass. The benchmark journal now carries immutable upsert payloads, so bounded replay remains correct when a later same-document commit changes or deletes the current row. Independent verification passes all 13 deterministic tests and the 250k release matrix: p95 `61.1 ms`, recall@150 `1.0000`, steady `1133.8 MiB`, and peak `1316.9 MiB`. The highest valid recorded post-fix peak remains `1319.9 MiB`, 11.3 MiB below the 1.30 GiB cap. | Retain a bound-only fix that derives upsert payload from current document state; accept the old workflow path; lower the cap or evaluate ANN. | Main agent |

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

### 1.3F - Gate-stage fidelity, admissibility, and critical-case closure

**Status:** Complete
**Owner:** `worker-l` (`ses_fce6f62e3ffekuqxK1x52fEwoT`)
**Completed:** 2026-08-24
**Implemented:**
- Aligned deleted, dirty/stale, and authoritative-hydration behavior with the
  architecture simulation; deleted-row removal was proven ranking-inert across
  all 15 affected cases.
- Added an assert-and-report supervised constructive admissibility invariant,
  production-channel rank-1 evidence, production-typed citation/source
  precision, a falsifiability mutation check, and the diagnostic 2160-point
  per-reranker feasibility probe.
- Published a complete ten-item category (a)/(b)/(c) verdict table in
  `docs/hybrid-rag/task-1.3f-gate-closure.md`.
**Implementation:**
- Files: `frontend/src-tauri/tests/model_benchmark.rs`,
  `frontend/src-tauri/tests/retrieval_evaluation.rs`, and
  `docs/hybrid-rag/task-1.3f-gate-closure.md`.
- Approach: fix simulation fidelity without changing gates or corpus content;
  prove gate existence constructively in the supervised layer; measure source
  parity using real `ChatSource` values after scope and budget retention; use
  the critical grid only as diagnostic existence evidence, never tuning.
**Not implemented:**
- No corpus patch, gate/threshold/metric change, production code, selected
  model, promoted constants, artifact bundle, or model weights.
**Why not implemented:**
- Category (c) remedies require a user corpus-patch-versus-gate-restaging
  decision. Task `1.3F` was approved to report that evidence, not enact it.
**Verification:**
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation` - independently pass, 5 tests and no target warnings.
- `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark -- --nocapture` - independently pass, 8 tests including the citation mutation.
- Release `hybrid_corpus_and_resource_benchmark` - worker pass twice with digit-identical quality tables; citation precision `602/602` for each retained pair, feasibility `0/2160` for each reranker, verdict `blocked-quality-gates`.
- Rust library suite - pass, 394 tests; 2 ignored. Frontend Vitest - pass, 95 tests. Typecheck and Cargo check - pass.
- `cargo fmt --check` and `git diff --check` - pass.
- Focused privacy scan - pass; matches are scanner/report literals only. No model weights entered Git.
**Rollback:**
- Restore `frontend/src-tauri/tests/model_benchmark.rs` and
  `frontend/src-tauri/tests/retrieval_evaluation.rs` from `7318c0c`, then
  remove `docs/hybrid-rag/task-1.3f-gate-closure.md`. Production and persisted
  data are unaffected; staged artifacts remain outside Git.
**Decisions and follow-ups:**
- Accept category (a): `pt-ref-nps-detrator` and citation precision are closed.
- Carry category (b) `pt-ref-sla-suporte` into the final held-out retune.
- Keep final Task `1.3`, Tasks `1.4`, and `1.5` blocked until the user chooses
  corpus patches or gate re-staging for category (c).

### 1.3G - Corpus patch and contamination gate re-scope

**Status:** Complete
**Owner:** `worker-l` (primary `ses_fcbf894c0ffe0XwFSDMSavxqDj`, `opencode-go/ox-alpha-free`; residual classifier fix `ses_fcb41049cffefT5nm7rG3j6sNE`, `openrouter/stealth/ox-alpha`; first dispatch attempt `ses_fcc04d762ffeil2zISYe19N0AH` failed on provider insufficient balance)
**Completed:** 2026-08-24
**Implemented:**
- Patched exactly the two approved corpus surfaces: the `pt-ref-chaves-acesso`
  target title (`Governança de chaves em ambientes — controle de acesso`,
  terminological gap preserved) and real superseded/draft carriers for
  `"apenas 3 dias"` / `"apenas 4 dias"` in topical-neighbour meetings outside
  the expected meeting.
- Added fixture-derived carrier-source-state classification
  (`classify_forbidden_fact`) with fact-level precedence — deleted,
  expected-authoritative, indexed-only stale, explicit superseded marker,
  otherwise reject — and re-scoped the critical contamination gate to
  retrieval-stage facts only; answer-stage facts are reported with denominators
  and never gated in Sprint 1 (`107/14` split pinned by test).
- Promoted admissibility to enforcing with five rejecting mutations
  (no-positive-channel title revert, trapped carrier, removed carrier,
  expected-current reclassification, differing-texts-both-carrying rejection)
  and aligned `setup_case` deleted-meeting FTS insertion with production
  cascade semantics.
- Re-recorded the baseline honestly: only forbidden contamination moved,
  `25/121` to `26/121` (+1 from the new superseded WhatsApp carrier); all other
  approved figures unchanged.
**Implementation:**
- Files: `frontend/src-tauri/tests/fixtures/corpus/reference_a.rs`,
  `reference_b.rs` (three content lines), `fixtures/corpus_types.rs`,
  `fixtures/evaluation_policy.json`, `tests/retrieval_evaluation.rs`,
  `tests/model_benchmark.rs`, and report
  `docs/hybrid-rag/task-1.3g-corpus-gate-patch.md`.
- Approach: patch only approved surfaces; derive gate scope from fixture state;
  prove every requirement with executable assertions rather than prose; keep
  reference/critical cases outside all tuning paths.
**Not implemented:**
- No model selection, constants promotion, chunk policy, answer-stage
  evaluation, threshold change beyond the approved re-scope, or production edit.
**Why not implemented:**
- Explicit non-goals of the approved task; selection belongs to the final
  Task `1.3` run and answer-stage measurement to Sprint 3/4.
**Verification:**
- `cargo test ... --test retrieval_evaluation -- --nocapture` - independently pass, 6 tests including the mutation suite.
- `cargo test ... --test model_benchmark` - independently pass, 9 tests including rank1-admissibility and citation mutations.
- Release canonical benchmark - worker pass twice plus one post-fix confirmation run; decision `blocked-quality-gates` (Critical R@1 3/5 remains model evidence); critical retrieval-stage contamination PASS `0/2`; citation precision `602/602`; feasibility now `79/2160` f32 / `78/2160` quint8.
- Frontend typecheck - pass. Vitest - pass, 95 tests. Cargo check - pass.
- `cargo fmt --check` and `git diff --check` - pass.
- Privacy scan - pass; 9 matches, all classified as scanner/validator literals or regex false positives.
**Rollback:**
- Restore the six files from `e209b5d` and delete
  `docs/hybrid-rag/task-1.3g-corpus-gate-patch.md`. Production and persisted
  data unaffected; staged artifacts remain outside Git.
**Decisions and follow-ups:**
- The final Task `1.3` run is cleared to dispatch on this instrument; it must
  apply the amendment-4 escalation if `pt-ref-sla-suporte` stays below rank 1.
- Orchestrator acceptance review (2026-08-24) verified the corpus diff is three
  content lines on the two approved surfaces and re-ran
  `retrieval_evaluation` (6/6) independently. Accepted.
- Four items recorded, none blocking (detail in this document's "What Task
  1.3G established"): the report omits the tuned constants, so the final run
  must disclose them; `0/2160` → `79/2160` is not comparable across the gate
  re-definition; 74 of 107 retrieval-stage facts remain carrierless, so
  percentage forms are diluted; and the in-scope-but-unspecified
  `metadata_conforming` family filter is accepted as correct.
- Follow-up owned by the final `1.3` run: amendment 5 (tuning partition and
  objective) — `1.3G`'s `78-79/2160` feasibility result localizes the
  remaining failure to constant selection.
- Follow-up owned by Sprint 3 Task `3.6`: single-turn query expansion, the
  architecturally correct remedy for `pt-ref-chaves-acesso`'s terminological
  gap, which no stage in this program currently implements.

### 1.3 - Final model selection (amendment 5)

**Status:** Complete - documented stop; no production pair selected
**Owner:** `worker-l` (`ses_fcaf0a683ffeJQbDb2utDAyIQW`, `openrouter/stealth/ox-alpha`)
**Completed:** 2026-08-24
**Implemented:**
- Applied amendment 5 only: tuning excludes precisely the five critical/pinned
  cases and admits the ten non-critical reference-category siblings; two
  regression tests pin the partition and the objective's lexicographic order.
- Added the reference-category Recall@1 miss term after semantic misses and
  before overall Recall@3 misses in both fusion and gamma tuning, with the full
  objective key and all constants printed beside every gate table.
- Extended the feasibility probe to compare the held-out-tuned configuration
  against the full-corpus passing set without using that set for selection.
- Produced `docs/hybrid-rag/task-1.3-final-selection.md`, including the clean
  release latency reprobe, citation/source precision, title ablation, resource
  evidence, candidate provenance, and explicit amendment-4 escalation.
**Implementation:**
- Files: `frontend/src-tauri/tests/model_benchmark.rs` and
  `docs/hybrid-rag/task-1.3-final-selection.md`.
- Approach: retune solely on the 115-case amendment-5 partition and grade the
  isolated critical cases only after constants are earned.
**Not implemented:**
- No production model selection, corpus/gate/partition iteration beyond
  amendment 5, production implementation, dependency change, or manifest
  promotion.
**Why not implemented:**
- The earned production-candidate configuration fails Critical Recall@1 at
  `2/5`; amendment 5 forbids selecting a diagnostic passing configuration by
  inspection and requires a documented stop rather than another iteration.
**Verification:**
- Independent `cargo test --test model_benchmark` - pass, 11/11 including both amendment-5 regressions.
- Independent `cargo test --test retrieval_evaluation` - pass, 6/6.
- Independent release `hybrid_corpus_and_resource_benchmark` - pass; quint8
  earned constants `k=5`, `w_vector=1`, `w_lexical=0.5`, `alpha=0.5`, `beta=1`,
  `gamma=0`; objective `[0,0,0,0,0,2166666]`; Critical Recall@1 `2/5`,
  retrieval-stage contamination `0/2`, citation precision `602/602`,
  feasibility `78/2160`, tuned configuration outside the passing set.
- Worker ran library suite (394 pass, 2 ignored), Cargo checks, formatter,
  diff check, release RAM probe, and privacy/model-weight scan; all passed.
**Rollback:**
- Restore `frontend/src-tauri/tests/model_benchmark.rs` from `6bba48b` and
  remove `docs/hybrid-rag/task-1.3-final-selection.md`. Production, corpus,
  gates, manifest, and persisted data are unaffected.
**Decisions and follow-ups:**
- User decision required: split the critical gate or grant a dated exception
  against Sprint 3 Task `3.6`; do not run another Sprint 1 instrument iteration.
- `pt-ref-sla-suporte` remains an explicit amendment-4 open item (rank 3).

### 1.3 - Amendment: critical-gate split and pair selection

**Status:** Amendment (the final-selection entry above remains immutable and is
not edited; this entry records the user decision that resolved its stop.)
**Owner:** Main agent
**Recorded:** 2026-08-24
**Decision:**
- The user resolved the documented stop by **splitting the critical gate**
  (`architecture.md` amendment, approved 2026-08-24), not by a dated exception.
  Critical *hydration-window membership* is the Sprint 1 model-selection gate
  and passes `5/5`; critical *Recall@1* keeps its 100% threshold and becomes a
  **Sprint 3 release gate**.
- Sprint 3 debt is attributed by measured cause, not assigned wholesale:
  `pt-ref-sla-suporte` (raw vector rank 1, fused rank 3) and
  `pt-ref-nps-detrator` (raw vector rank 1, fused rank 2) belong to Task `3.2`
  fusion/aggregation/reranking; `pt-ref-chaves-acesso` (raw vector rank 4,
  terminological gap) belongs to Task `3.6` query expansion.
- Option 2 was rejected on its premise: query expansion addresses only
  `chaves`. The other two misses have raw vector rank 1, so a Task
  `3.6`-contingent exception would have expired unredeemed.
**Effect on the record:**
- Task `1.3` status changes from *documented stop* to **Complete — pair
  selected**. The selected pair, encoding, constants, chunk profile, runtime
  limits, resource evidence, and title-dependence qualification are recorded
  in `architecture.md` "Approved Sprint 1 Bundle And Runtime Contract" before
  Sprint 2 implementation.
- Tasks `1.4` and `1.5` are unblocked.
- Critical Recall@1 `2/5` is not waived. It is re-measured at Sprint 3 close
  and must pass before release.
**Not changed:**
- No corpus, gate threshold, tuned constant, partition, or objective was
  altered by this decision. Nothing was selected by inspecting critical-case
  results; the constants remain the held-out objective's output.
**Follow-ups:**
- Sprint 3 Task `3.2` inherits the critical Recall@1 release gate plus the
  recorded finding that a lexicographic-minimizing objective is stricter than a
  threshold gate; it must gate on thresholds and optimize inside the feasible
  set.
- Sprint 3 Task `3.6` inherits `pt-ref-chaves-acesso` and remains blocked on
  the user's expansion-approach decision.
- Unverified and stated as such: the feasibility probe checked only three
  conditions for its 78 passing configurations (critical Recall@1, critical
  retrieval-stage contamination, exact-term no-regression). Whether any of them
  passes *every* gate was never evaluated, so no claim that a fully-passing
  configuration exists is on the record.
- The selected configuration's title dependence (reference-category Recall@1
  12/15 at `beta=1` versus 7/15 at `beta=0`) is recorded in the architecture
  addendum and cannot be attributed solely to the embedding model.

### 1.4 - Vector backend benchmark

**Status:** Complete - exact backend selected
**Owner:** `worker-l` (`ses_fc993e5b7ffe423l47gsPHwsn9`, `openrouter/stealth/ox-alpha`)
**Completed:** 2026-08-24
**Implemented:**
- Added a deterministic, bounded exact-vector benchmark at 12k, 50k, and 250k
  selected 768-d int8 vectors, with cold/warm, global/narrow scope, candidate
  depth, update/delta/tombstone, compaction, crash-window, concurrency, and
  scheduler measurements.
- Proved exact top-150 equality with brute force, scope isolation, immutable
  base updates, journal replay after a simulated crash, scheduler capacity, and
  the 250 ms interactive worker-pause budget.
- Corrected the no-pause scheduling probe deadlock: it now requests and waits
  for a pause only from the pause-honoring probe arm.
- Selected exact search; ANN was not evaluated because exact p95 is `48.2 ms`
  at 250k against the 500 ms gate, with recall@150 `1.0000`.
**Implementation:**
- Files: `frontend/src-tauri/tests/vector_backend_benchmark.rs`,
  `docs/hybrid-rag/task-1.4-vector-backend.md`, and the worker's cross-program
  execution note in `docs/notes-chat-improvement-execution.md`.
- Approach: model the approved exact base+delta+tombstone and SQLite journal
  contract in an isolated benchmark harness; make the backend decision before
  considering any ANN dependency.
**Not implemented:**
- No ANN evaluation/dependency, production backend, schema/migration, model,
  manifest, or runtime retrieval implementation.
**Why not implemented:**
- Backend Decision Rule row 1 applies. Exact passes its latency and RAM gates;
  ANN has no trigger and would add memory.
**Verification:**
- Independent `cargo test --test vector_backend_benchmark` - pass, 9/9.
- Independent release `full_matrix_benchmark` - pass: 250k global p95
  `48.2 ms`, folder p95 `10.6 ms`, snapshot p95 `0.1 ms`, two-scanner p95
  `44.9 ms`, pause observed in `2 ms`, and exact recall@150 `1.0000`.
- RAM/disk: steady `1113.4 MiB` is in the approved band; measured active plus
  shadow peak `1296.5 MiB` is in the approved 1.30 GiB transient ceiling;
  disk `0.19/0.38 GiB` is inside the 2/3 GiB envelopes.
- Worker also passed Rust library tests (394 pass, 2 ignored), Cargo check,
  formatter, diff check, frontend typecheck/Vitest, and privacy/model-weight
  scan.
**Rollback:**
- Delete `frontend/src-tauri/tests/vector_backend_benchmark.rs` and
  `docs/hybrid-rag/task-1.4-vector-backend.md`; remove the matching
  `HR-1.4` note in `docs/notes-chat-improvement-execution.md`. No production
  state or dependency changed.
**Decisions and follow-ups:**
- Sprint 2 implements exact base+delta+tombstone search with the approved
  limits: 150 candidates, two scan permits, queue 8, pause 250 ms, update
  batch 128, and compaction at or before 2% delta; re-measure in production.
- Any actual third vector snapshot or rebuild peak above 1.30 GiB blocks
  activation pending a user-approved remedy.

### 1.5 - Bundle manifest, artifact verification, and MSRV

**Status:** Blocked
**Owner:** `worker-l` (`ses_fc8fee5d7ffeeJczknaY5KJfR7`)
**Completed:** Not applicable - 2026-08-25 dispatch failed before implementation.
**Implemented:**
- None.
**Implementation:**
- Files: None.
- Approach: The user-approved isolated Worker-L dispatch was attempted twice
  against the configured `opencode-go/ox-alpha-free` provider.
**Not implemented:**
- All Task `1.5` manifest, verification, CI, and MSRV work.
**Why not implemented:**
- Both attempts failed before the worker session could start: Console Go returned
  `Upstream request failed: Endpoint is unavailable`.
**Verification:**
- Worker-L launch retry - failed before any repository command or edit.
**Rollback:**
- Not applicable; no repository changes were made by either failed dispatch.
**Decisions and follow-ups:**
- No fallback model was used. A new dispatch requires the configured provider
  to recover or an explicit user-approved fallback.

### 1.5 - Bundle manifest, artifact verification, and MSRV

**Status:** Complete
**Owner:** `worker-l` (`ses_fc75c4aa5ffertNOLMzRPRF904`, `opencode-go/ox-alpha-free`)
**Completed:** 2026-08-25
**Implemented:**
- Added the selected-pair production manifest, separate complete embedding and
  reranker tokenizer contracts, exact artifacts/licenses, provenance, and
  attribution. The ten artifacts are length/SHA-256 pinned.
- Added a Windows-only cache/stage/publish script. It fetches model/tokenizer
  artifacts only from immutable revision URLs, verifies all artifacts as one
  package outside Tauri resources, then atomically publishes the bundle.
- Added a strict Rust manifest parser and lazy pre-load artifact verifier; 19
  focused tests cover valid, corrupt, missing, unknown-version, contract, and
  provenance cases. Application startup does not call it yet.
- Set the true Rust 1.88 floor in both manifests, pinned `rust-toolchain.toml`
  to 1.88.0, and made Windows CI install/assert that exact version instead of
  floating `stable`.
**Implementation:**
- Files: `frontend/src-tauri/resources/retrieval/`,
  `frontend/src-tauri/scripts/stage-retrieval-models.ps1`,
  `frontend/src-tauri/src/model_bundle.rs`, `rust-toolchain.toml`, Cargo
  manifests/lockfile, Tauri resources, Windows workflow, `.gitignore`, and
  `docs/hybrid-rag/task-1.5-model-supply-chain.md`.
- Approach: use a small checked-in contract and local hash verification, not a
  runtime downloader; use a build cache and temporary sibling staging directory
  so invalid/incomplete artifacts never enter the signed resource directory.
**Not implemented:**
- No runtime download, startup integration, ONNX session construction, vector
  retrieval, schema/migration, ANN, or macOS/Linux workflow claim.
**Why not implemented:**
- These belong to later approved Sprint 2+ work. Task `1.5` prepares and
  verifies the supply chain without activating retrieval behavior.
**Verification:**
- Fresh-cache `stage-retrieval-models.ps1` - pass: eight model/tokenizer
  artifacts fetched from pinned URLs; two checked-in license texts verified;
  all ten artifacts re-verified and atomically published (411 MiB).
- Independent `cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib model_bundle` - pass, 19/19.
- Independent `pnpm --dir "frontend" run typecheck`, Cargo check with the
  LOCALAPPDATA target directory, rustfmt check, `git diff --check`, and Windows
  workflow YAML lint - pass.
**Rollback:**
- Revert the Task `1.5` files and resource entries; delete the local
  `%LOCALAPPDATA%\meetily\model-cache` cache. Nothing calls the verifier or
  consumes the staged bundle yet.
**Decisions and follow-ups:**
- Tokenizer JSON bytes are identical but tokenizer revisions/configs differ, so
  contracts remain separate. `sentencepiece.bpe.model` is intentionally absent:
  the approved runtime contract uses tokenizer.json; add it only if Sprint 2
  requires a slow-tokenizer fallback.
- The initial Worker-L dispatch remains recorded above as an infrastructure
  outage; the fresh session completed after the provider recovered.

### 1.R1 - Active root CI release gate

**Status:** Complete
**Owner:** `worker-l` (`ses_fc6bd168effeAUd4Tj4WGqpoyj`, `opencode-go/ox-alpha-free`)
**Completed:** 2026-08-25
**Implemented:**
- Moved the exact Rust 1.88.0 install/assertion, manifest validation, bundle
  cache/staging, ten-artifact verification, and reference-inference gate to
  the active repository-root Windows workflow.
- Adapted the existing benchmark reference harness to read the staged production
  bundle layout under `MEETLY_RAG_BUNDLE_DIR`; bundle mode fails rather than
  skipping when artifacts are absent and runs the approved tokenizer, embedding,
  and quint8 reranker only.
**Implementation:**
- Files: root `.github/workflows/build-windows.yml`,
  `frontend/src-tauri/tests/model_benchmark.rs`,
  `frontend/src-tauri/src/model_bundle.rs`, and
  `docs/hybrid-rag/task-1.r1-active-ci.md`.
- Approach: reuse Task `1.3` reference expectations and inference implementation
  with a production-layout adapter, rather than create a second ONNX test.
**Not implemented:**
- No installed-package inference, macOS/Linux workflow, model change, runtime
  retrieval behavior, package/provenance remediation (`1.R2`), or vector
  benchmark work (`1.R3`).
**Why not implemented:**
- Those are separately bounded review follow-ups or Sprint 5 gates.
**Verification:**
- Independent `MEETLY_RAG_VERIFY_STAGED_BUNDLE=1` test - pass, ten staged
  artifacts verify by length and SHA-256.
- Independent `MEETLY_RAG_BUNDLE_DIR=<staged bundle>` reference-inference
  test - pass, tokenizer/embedding/reranker reference expectations reproduced.
- Root workflow YAML lint and `git diff --check` - pass.
**Rollback:**
- Revert the root workflow gate and the two test-only adapter changes; delete
  the local model cache. No application runtime path changed.
**Decisions and follow-ups:**
- The nested `upstream/.github` workflow remains inert and untouched.
- `1.R2` and `1.R3` remain required before re-review/Sprint close.

### 1.R2 - Package/provenance boundary

**Status:** Blocked
**Owner:** `worker-l` (`ses_fc6961588ffeU3IS7BWeayB3rI`)
**Completed:** Not applicable - 2026-08-25 dispatch failed before implementation.
**Implemented:**
- None.
**Implementation:**
- Files: None.
- Approach: The approved Worker-L dispatch was attempted twice against the
  configured `opencode-go/ox-alpha-free` provider.
**Not implemented:**
- All `1.R2` selected-contract, provenance, package-authority, and recovery
  remediation.
**Why not implemented:**
- Both attempts failed before a worker session started: Console Go returned
  `Upstream request failed: Endpoint is unavailable`.
**Verification:**
- Worker-L launch retry - failed before any repository command or edit.
**Rollback:**
- Not applicable; neither failed dispatch changed the repository.
**Decisions and follow-ups:**
- No fallback model was used. Redispatch after the configured provider recovers
  or obtain explicit user approval for a fallback.

### 1.R2 - Package/provenance boundary

**Status:** Complete
**Owner:** `worker-l` (`ses_fc634146bffevaVpdPBU0R2NE6`, `opencode-go/ox-alpha-free`)
**Completed:** 2026-08-25
**Implemented:**
- Bound manifest parsing to the exact approved models, exports, preprocessing,
  tensor contracts, source revisions, artifact set, and license authority.
- Replaced the generic e5 MIT template with a pinned evidence-backed notice for
  the Xenova export's upstream E5 provenance; retained the pinned mmarco
  Apache-2.0 attribution and text.
- Made the staged bundle the sole packaged retrieval authority. It rejects
  unexpected content, divergent manifests, and a missing/tampered required
  README placeholder.
- Added full-integrity single-backup recovery: every managed artifact, the
  manifest copy, and placeholder pin must verify before restoration.
**Implementation:**
- Files: `frontend/src-tauri/resources/retrieval/`,
  `frontend/src-tauri/src/model_bundle.rs`,
  `frontend/src-tauri/scripts/stage-retrieval-models.ps1`,
  `frontend/src-tauri/tauri.conf.json`, and
  `docs/hybrid-rag/task-1.r2-package-provenance.md`.
- Approach: use immutable primary-source evidence and one reusable package
  integrity gate for staging, publication, and recovery.
**Not implemented:**
- No runtime retrieval/startup integration, installed-package inference,
  macOS/Linux workflow, or Task `1.R3` vector benchmark changes.
**Why not implemented:**
- These are separate Sprint 2/Sprint 5 work or the remaining independent
  review remediation.
**Verification:**
- Independent `stage-retrieval-models.ps1 -SelfTest` - pass: intact recovery,
  missing/corrupt/ambiguous backup rejection, unmanifested/divergent/tampered
  content rejection, and clean-package control.
- Independent `cargo test --lib model_bundle` - pass, 21/21.
- Independent staging, staged bundle/reference inference, typecheck, Cargo
  check, rustfmt, active-workflow YAML lint, and diff checks - pass.
**Rollback:**
- Revert the R2 manifest/verifier/script/resource changes and delete the local
  bundle/cache. No application runtime consumes them yet.
**Decisions and follow-ups:**
- The actual Microsoft E5 notice is now retained through a pinned provenance
  chain; update its artifact/hash only with a corresponding approved model or
  rights-holder change.
- `1.R3` remains before the final review and Sprint-close request.

### 1.R3 - Exact benchmark envelope

**Status:** Complete
**Owner:** `worker-l` (`ses_fc60bbe8bffehPVuVVU0jP9OyO`, `opencode-go/ox-alpha-free`)
**Completed:** 2026-08-25
**Implemented:**
- Fixed the shared overlay mask to evaluate each base row by its stable document
  ID, not an assumed ID-as-row position; fixed sibling scope-meeting lookup.
- Added a canonical sparse-ID crash-replay/compaction regression that does not
  share the production mask/index helper; restoring the old helper fails it.
- Replaced vector/session arithmetic with a fail-closed staged-bundle run that
  warms and holds both selected ONNX sessions while streaming the shadow over
  the active snapshot and live delta/tombstone overlay.
- Enforced the distinct 1.25 GiB steady and 1.30 GiB two-snapshot transient
  ceilings in the benchmark output/assertions.
**Implementation:**
- Files: `frontend/src-tauri/tests/vector_backend_benchmark.rs` and
  `docs/hybrid-rag/task-1.r3-vector-envelope.md`.
- Approach: stream shadow rows into exact-capacity allocations and govern the
  combined state with Windows process working-set/commit counters.
**Not implemented:**
- No ANN, model/package/workflow change, runtime backend, schema/migration, or
  cap relaxation.
**Why not implemented:**
- Exact p95 and the measured combined envelope pass; ANN is neither triggered
  nor a permitted RAM remedy.
**Verification:**
- Independent deterministic benchmark suite - pass, 10/10 including the
  sparse-ID regression.
- Independent 250k release matrix - pass: p95 `61.1 ms`, recall@150 `1.0000`,
  steady `1134.8 MiB`, and combined active+shadow+delta+sessions `1317.9 MiB`
  versus the 1.30 GiB cap; interactive pause observed in 2 ms.
- Worker also proved missing/corrupt staged bundle failure, plus Cargo check,
  rustfmt, diff check, typecheck, and Vitest.
**Rollback:**
- Restore the untracked benchmark harness/report to their pre-R3 state; no
  production state or dependency changed.
**Decisions and follow-ups:**
- The independent rerun's `1317.9 MiB` is the governing observed result, above
  the worker run's 1316.3 MiB but still 13.3 MiB inside the approved transient
  cap. Sprint 2 must re-measure at its actual allocation/batching behavior.
- All review remediation tasks are complete; final reviews remain required.

### 1.R1a - Active CI staging path

**Status:** Complete
**Owner:** `worker-l` (`ses_fc5b84814ffe609Kxu6ZtJI0wi`, `opencode-go/ox-alpha-free`)
**Completed:** 2026-08-25
**Implemented:**
- Corrected the active root workflow staging invocation to
  `./upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1`.
- Corrected the earlier R1 report to describe the same checkout-root path.
**Implementation:**
- Files: root `.github/workflows/build-windows.yml`,
  `docs/hybrid-rag/task-1.r1-active-ci.md`, and this execution record.
- Approach: use the repository-relative script path rather than adding a new
  per-step working directory; the script already derives its own paths.
**Not implemented:**
- No model, script, manifest, package, runtime, or nested-workflow changes.
**Why not implemented:**
- The defect was a single active-workflow path error; changing another boundary
  would not improve the CI gate.
**Verification:**
- Root workflow YAML lint - pass.
- Exact checkout-root `stage-retrieval-models.ps1 -SelfTest` - pass, including
  sole-backup recovery and invalid-package rejection families.
- Static gate ordering/path assertions and `git diff --check` - pass.
**Rollback:**
- Restore the prior workflow invocation; no runtime state or packaged contract
  changes.
**Decisions and follow-ups:**
- A GitHub-hosted `build-windows` run remains required evidence for the runner
  cache, network staging, downstream inference, and packaging path.

### 1.R3a - Bounded journal publication correctness

**Status:** Complete
**Owner:** `worker-l` (`ses_fc5b8478fffepoG91Zm7Cjreoj`, `opencode-go/ox-alpha-free`)
**Completed:** 2026-08-25
**Implemented:**
- Added immutable vector, scale, and meeting payload columns to benchmark-local
  upsert journal entries, with a SQLite `CHECK` that rejects payload-less
  upserts.
- Captures a canonical upper bound before publishing; replay reads only journal
  entries in `(published, bound]` and advances only through the last applied ID.
- Added regressions for same-document concurrent updates, repeated upserts,
  upsert/delete tombstones, and retained crash-replay behavior.
**Implementation:**
- Files: `frontend/src-tauri/tests/vector_backend_benchmark.rs`,
  `docs/hybrid-rag/task-1.r3a-journal-publication.md`, and this execution
  record.
- Approach: make every journal replay entry self-contained so later document
  mutations cannot alter or invalidate bounded publication.
**Not implemented:**
- No production schema, migration, runtime backend, ANN, model, package, or
  resource-cap change.
**Why not implemented:**
- This corrects only the benchmark fixture that supplies Sprint 2's journal
  contract; production persistence belongs to Sprint 2.
**Verification:**
- Independent deterministic vector suite - pass, 13/13.
- Independent 250k release matrix - pass: p95 `61.1 ms`, recall@150 `1.0000`,
  steady `1133.8 MiB`, and combined peak `1316.9 MiB`.
- Rust format/check and `git diff --check` - pass; the highest valid recorded
  R3a peak remains `1319.9 MiB`, inside the 1.30 GiB cap by 11.3 MiB.
**Rollback:**
- Restore the prior benchmark fixture/report; no production state, dependency,
  migration, or runtime behavior exists to roll back.
**Decisions and follow-ups:**
- Final code and architecture reviews approve. Sprint 2 must carry
  journal-carried immutable upsert payloads or an equivalently correct,
  revision-addressable replay contract; Sprint 1 still needs its hosted root
  Windows workflow evidence before close.

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

**Reviewer:** `reviewer` (`SPR1-HR-CR-20260825`)
**Verdict:** Changes requested
**Findings:**
- **Blocker — the release/CI changes are in an inert nested workflow.** Git's
  repository root is the parent of `upstream/`, so the edited
  `upstream/.github/workflows/build-windows.yml:498-513,652-662` is not loaded
  by GitHub Actions. The active workflow remains
  `../.github/workflows/build-windows.yml:88-91,247-250` on floating `stable`
  and has no model staging step. Consequently CI does not exercise Rust 1.88.0
  and a clean release checkout can package only the ignored bundle placeholder,
  not the verified model artifacts. This directly contradicts the repository-
  root workflow authority recorded at `docs/hybrid-rag/architecture.md:65-70`
  and `docs/hybrid-rag/sprint-1-quality-gates.md:1309-1313`.
- **Blocker — the mandatory Windows reference-inference gate is absent.** Even
  the edited inert workflow goes directly from hash staging at
  `.github/workflows/build-windows.yml:660-662` to sidecar/Tauri builds; it
  never runs tokenizer, embedding, or reranker inference from the staged
  resource layout. The normative pre-Sprint-2 gate requires all three and
  stable reference outputs (`docs/hybrid-rag/architecture.md:1686-1697`). Hash
  verification proves provenance, not ONNX/tokenizer compatibility.
- **Blocker — the claimed 1.30 GiB rebuild peak was not measured in the
  required simultaneous state.** The harness explicitly omits both model
  sessions (`frontend/src-tauri/tests/vector_backend_benchmark.rs:11-14`),
  observes a 703.5 MiB process peak while loading two snapshots without them,
  then reports 1296.5 MiB by adding the retained session figure to raw vector
  payload sizes only (`:1383-1396,1501-1531`). `load_snapshot` also uses
  `fetch_all` before constructing the contiguous snapshot (`:689-718`), so
  transient SQLite rows/BLOBs and snapshot metadata/capacity are real parts of
  the measured path but absent from the approval arithmetic. The narrow rebuild
  ceiling and Task 1.4 matrix require old snapshot + shadow + delta + active
  sessions together (`docs/hybrid-rag/architecture.md:94-152`;
  `docs/hybrid-rag/sprint-1-quality-gates.md:1232-1237,1274-1277`). Exact
  latency still passes and ANN remains the wrong RAM remedy, but activation at
  the approved scale is not proven.
- **Should-fix — manifest validation is not fail-closed against incompatible
  bundle contracts.** `frontend/src-tauri/src/model_bundle.rs:161-213` accepts
  arbitrary nonempty model IDs/revisions/prefixes/pooling, either normalization
  and truncation side, and any listed quantization; `:300-317` does not bind an
  ONNX repo/revision to approved artifacts or pinned source URLs. The exact-
  production assertions at `:1080-1133` are tests only and are not run by the
  release workflow. In addition, staging copies every unmanifested file from a
  prior bundle into the signed package without hashing it
  (`frontend/src-tauri/scripts/stage-retrieval-models.ps1:121-133`). A changed
  but schema-valid manifest or stale extra resource can therefore pass the
  build/package path instead of being rejected as an incompatible package.
- **Should-fix — embedding-export redistribution provenance is incomplete.**
  The packaged ONNX artifact comes from the separate
  `Xenova/multilingual-e5-base` export
  (`frontend/src-tauri/resources/retrieval/model-bundle.manifest.json:9-12,54-61`),
  while the only embedding license entry attributes the upstream
  `intfloat/multilingual-e5-base` model (`:125-136`). The packaged MIT text
  still contains generic `<year> <copyright holders>` placeholders
  (`frontend/src-tauri/resources/retrieval/licenses/e5-base-MIT.txt:1-12`). A
  hash-pinned generic template does not establish the export repository's
  redistribution authority or retain an applicable copyright notice.
- **Should-fix — the exact-backend crash/compaction proof uses document IDs as
  snapshot row indexes.** `base_scan_mask` writes `mask[doc_id]` at
  `frontend/src-tauri/tests/vector_backend_benchmark.rs:212-225`, although
  compaction and SQLite reload preserve sparse IDs while changing row
  positions (`:327-370,689-718`). The crash test deletes IDs 50-52, reloads the
  shortened snapshot, then compares two overlays using the same flawed mask
  (`:970-1032`), so it can hide unrelated rows and still pass. That does not
  prove the recorded deletion/replay strategy; add a sparse-ID regression and
  compare replay results with canonical expected documents independently of
  the implementation under test.
- **Should-fix — the canonical benchmark still encodes the superseded 1.25 GiB
  transient verdict.** `frontend/src-tauri/tests/vector_backend_benchmark.rs:1520-1529`
  reports the measured 1296.5 MiB two-snapshot state as `FAIL`, while the
  approved contract permits this specific state through 1.30 GiB
  (`docs/hybrid-rag/architecture.md:149-152`). The review rerun reproduced that
  failure label. Encode and assert the distinct steady-state and approved
  transient ceilings so benchmark output cannot contradict the sprint record.
**Required follow-ups:**
- `1.R1` — move the toolchain assertion, model cache/staging, and required
  reference-inference gate into the actual repository-root Windows workflow;
  verify a clean checkout produces a bundle containing all ten artifacts.
- `1.R2` — make build/runtime manifest validation reject non-approved or
  incoherent model/preprocessing/provenance contracts and unexpected staged
  files; record the ONNX export's license authority and package the applicable
  notice/attribution; add release-path mutation tests.
- `1.R3` — repair the exact benchmark's ID-to-row handling and transient RAM
  gate, measure active + shadow + delta + both sessions in one production-shaped
  process, add sparse-ID post-compaction/crash-replay tests, and rerun the
  release matrix before relying on its RAM/recovery evidence.
**Remaining risks:** Fresh-cache network acquisition and installed-package
resource discovery were not rerun in this review. The ignored 411 MiB local
bundle is correctly excluded by `.gitignore:80-84`, but only the corrected
active CI clean-checkout test can prove release packaging. No runtime consumer
calls `parse_manifest`/`verify_artifacts` yet (`frontend/src-tauri/src/lib.rs:49`);
that deliberate Sprint 2 integration remains a fail-closed implementation risk.
**Verification:** Rust 1.88.0; focused manifest tests 19/19; vector tests 9/9;
release 12k/50k/250k matrix passed with 250k warm-global p95 53.2 ms and exact
recall@150 1.0000 (but emitted the transient-RAM `FAIL` above); Cargo check and
rustfmt passed; frontend typecheck passed; Vitest 20 files/95 tests passed; `git
diff --check` passed with existing line-ending warnings.
**Files reviewed:** `.github/workflows/build-windows.yml`, `.gitignore`,
`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
`frontend/src-tauri/Cargo.toml`, `frontend/src-tauri/src/lib.rs`,
`frontend/src-tauri/src/model_bundle.rs`,
`frontend/src-tauri/scripts/stage-retrieval-models.ps1`,
`frontend/src-tauri/tauri.conf.json`,
`frontend/src-tauri/tests/vector_backend_benchmark.rs`,
`frontend/src-tauri/resources/retrieval/model-bundle.manifest.json`,
`frontend/src-tauri/resources/retrieval/licenses/e5-base-MIT.txt`,
`frontend/src-tauri/resources/retrieval/licenses/mmarco-mMiniLMv2-Apache-2.0.txt`,
`frontend/src-tauri/resources/retrieval/bundle/README.md`,
`docs/hybrid-rag/architecture.md`, `docs/hybrid-rag/sprint-1-quality-gates.md`,
`docs/hybrid-rag/task-1.4-vector-backend.md`, and
`docs/hybrid-rag/task-1.5-model-supply-chain.md`; active-workflow context:
`../.github/workflows/build-windows.yml`. Unrelated user/main documentation
changes in `docs/hybrid-rag/README.md` and
`docs/notes-chat-improvement-execution.md` were preserved and excluded.

### Architecture Review

**Required because:** Model supply chain, new dependencies, retrieval algorithm,
cross-platform native runtime, and a decision that constrains every later
sprint.

**Reviewer:** `arch-reviewer` (`Sprint-1-Hybrid-RAG-architecture-review`)
**Verdict:** Changes requested
**Findings:**
- **BLOCKER — the mandatory pre-Sprint-2 Windows reference-inference gate is
  not present in CI.** The architecture requires the Windows runner to execute
  tokenizer, embedding, and reranker inference from the staged resource layout
  and compare platform-neutral reference results before Sprint 2
  (`docs/hybrid-rag/architecture.md:1681-1697`). The workflow only stages and
  hashes the bundle, then proceeds to the application build
  (`.github/workflows/build-windows.yml:652-664`); it never runs the existing
  Task 1.3 reference-inference harness. Local Task 1.3 evidence does not satisfy
  the explicit CI-runner gate. Cheapest correction: reuse that focused
  reference-inference check against `resources/retrieval/bundle` immediately
  after staging; do not add a second inference implementation.
- **BLOCKER — the 1.30 GiB rebuild result is arithmetic, not a measurement of
  the required simultaneously resident state.** The benchmark explicitly does
  not load either model session (`frontend/src-tauri/tests/vector_backend_benchmark.rs:11-14`),
  loads all SQLite rows with `fetch_all` while constructing the shadow
  (`frontend/src-tauri/tests/vector_backend_benchmark.rs:689-717`), and then
  computes `1296.5 MiB` from two raw vector payloads plus a retained session
  number (`frontend/src-tauri/tests/vector_backend_benchmark.rs:1501-1531`). Its
  observed shadow-load process peak is instead reported without sessions
  (`docs/hybrid-rag/task-1.4-vector-backend.md:161-166,195-204`). This omits
  snapshot metadata/capacity and transient SQL row/BLOB allocations at the
  narrow 1.30 GiB margin, contrary to the combined-envelope contract
  (`docs/hybrid-rag/architecture.md:94-152`) and Task 1.4 matrix
  (`docs/hybrid-rag/sprint-1-quality-gates.md:1232-1237,1274-1277`). Exact
  latency still clearly passes and ANN remains the wrong RAM remedy, but the
  rebuild activation limit handed to Sprint 2 is unproven. Cheapest correction:
  stream the shadow load into the production-shaped contiguous allocation and
  measure active snapshot + shadow + delta + both resident sessions in one
  process; record and govern any additional transient allocation rather than
  calling raw-payload arithmetic a measured peak.
- **HIGH — the package path does not enforce the approved bundle contract.**
  The staging script validates version, paths, lengths, hashes, and duplicates
  only (`frontend/src-tauri/scripts/stage-retrieval-models.ps1:46-63`), while
  the exact model/revision/tensor/tokenizer assertions live only in a Rust unit
  test (`frontend/src-tauri/src/model_bundle.rs:1080-1133`) that the Windows
  packaging workflow does not run. A manifest changed to a different
  hash-matching model can therefore be signed even though Sprint 2 is forbidden
  to substitute the approved pair (`docs/hybrid-rag/architecture.md:507-539`).
  Cheapest correction: run the existing focused production-manifest test in
  the packaging job before staging; keep the script responsible for bytes and
  the Rust validator responsible for the semantic contract.
- **HIGH — redistribution evidence does not yet close the artifact trust
  boundary.** The packaged embedding ONNX bytes come from the separate
  `Xenova/multilingual-e5-base` export
  (`frontend/src-tauri/resources/retrieval/model-bundle.manifest.json:9-12,54-61`),
  but the sole embedding license entry attributes only the upstream
  `intfloat/multilingual-e5-base` model
  (`frontend/src-tauri/resources/retrieval/model-bundle.manifest.json:125-136`).
  Its packaged MIT notice still contains literal `<year> <copyright holders>`
  placeholders (`frontend/src-tauri/resources/retrieval/licenses/e5-base-MIT.txt:1-12`).
  Hashing a generic template proves integrity, not that the required upstream
  copyright notice and the exporter artifact's redistribution provenance were
  retained. Record the ONNX export's license/provenance and package the actual
  applicable notice/attribution before approving the bundle.
- **MEDIUM — publication and package layout create avoidable recovery and
  authority ambiguity.** Staging copies the manifest and licenses into the
  bundle (`frontend/src-tauri/scripts/stage-retrieval-models.ps1:108-135`), but
  Tauri also packages the source manifest and licenses as separate resources
  (`frontend/src-tauri/tauri.conf.json:96-102`), leaving two signed copies for
  Sprint 2 to choose between. In addition, a prior valid backup is deleted on
  the next run before recovery (`frontend/src-tauri/scripts/stage-retrieval-models.ps1:65-69`),
  despite the two-rename missing-directory window
  (`frontend/src-tauri/scripts/stage-retrieval-models.ps1:137-150`). Use one
  packaged bundle authority; if `bundle` is absent and one backup exists,
  restore it before cleanup. This keeps the accepted single-publisher design
  without adding a journal.
- **Assumptions/risks:** the exact Rust 1.88.0 declarations are aligned across
  both manifests, `rust-toolchain.toml`, and Windows CI
  (`Cargo.toml:8-12`, `frontend/src-tauri/Cargo.toml:1-10`,
  `rust-toolchain.toml:1-4`, `.github/workflows/build-windows.yml:498-513`). The
  verifier is correctly not on startup, but Sprint 2 must invoke it exactly
  once off Tokio worker threads immediately before first ORT session creation,
  and convert failure to semantic-unavailable/FTS fallback
  (`frontend/src-tauri/src/model_bundle.rs:265-285`;
  `docs/hybrid-rag/architecture.md:1376-1394`). Reviewed staged binaries were
  treated as generated build outputs: all ten present files matched manifest
  byte lengths and SHA-256 locally, and the focused model-bundle suite passed
  19/19; no CI run evidence was assumed.
- **Reviewed files:** `.github/workflows/build-windows.yml`, `.gitignore`,
  `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
  `docs/hybrid-rag/{README.md,architecture.md,sprint-1-quality-gates.md,task-1.4-vector-backend.md,task-1.5-model-supply-chain.md}`,
  `docs/notes-chat-improvement-execution.md`,
  `frontend/src-tauri/{Cargo.toml,tauri.conf.json,scripts/stage-retrieval-models.ps1,src/lib.rs,src/model_bundle.rs,tests/vector_backend_benchmark.rs}`,
  `frontend/src-tauri/resources/retrieval/model-bundle.manifest.json`, both
  checked-in license files, `bundle/README.md`, and the ignored staged bundle's
  ten manifest-managed artifacts (length/hash verification only for large
  binary/tokenizer outputs).

### Post-Remediation Architecture Review

**Required because:** This re-review determines whether the Sprint 1 model
supply-chain, exact-backend, recovery, resource-envelope, and Sprint 2 handoff
remediations close the two prior review records.

**Reviewer:** `arch-reviewer` (`SPR1-HR-AR-POST-20260825`)
**Verdict:** Changes requested
**Findings:**
- **BLOCKER — the active root workflow contains the right gate but invokes the
  staging script from the wrong repository-relative path.** GitHub Actions runs
  a step at the checkout root unless `working-directory` is set. The active
  workflow calls `./frontend/src-tauri/scripts/stage-retrieval-models.ps1`
  without changing directory (`../.github/workflows/build-windows.yml:138-140`),
  but the script is under `upstream/frontend/...`; the following Rust gates do
  explicitly enter `upstream` (`../.github/workflows/build-windows.yml:142-156`).
  A checkout-root path check confirms the invoked path is absent and the
  `upstream/...` path exists. Therefore active CI stops before staging and never
  reaches staged tokenizer/embedding/reranker inference, so the prior Windows
  CI blocker is not yet resolved despite the correct exact-toolchain and
  fail-closed inference machinery being present (`../.github/workflows/build-windows.yml:88-111,150-156`).
- **RESOLVED — package authority, contract, provenance, and recovery now form
  one fail-closed boundary.** Tauri packages only the bundle directory
  (`frontend/src-tauri/tauri.conf.json:96-100`); manifest parsing binds the
  exact selected models, exports, preprocessing, artifact paths/URLs, and
  license authorities (`frontend/src-tauri/src/model_bundle.rs:289-516`); the
  staged/published/recovered package shares one integrity gate and rejects
  foreign, divergent, missing, corrupt, or placeholder-tampered content
  (`frontend/src-tauri/scripts/stage-retrieval-models.ps1:82-130,321-330,396-415`).
  The immutable E5 export/upstream chain and applicable Microsoft MIT notice
  are retained in the managed bundle (`frontend/src-tauri/resources/retrieval/model-bundle.manifest.json:125-148`;
  `frontend/src-tauri/resources/retrieval/licenses/e5-base-MIT-NOTICE.txt:4-61`).
  The offline recovery self-test and all 21 focused Rust verifier tests passed
  in this review.
- **RESOLVED — exact sparse-ID and two-snapshot evidence now matches the
  approved backend semantics.** Base masking keys every row by stable document
  ID, and the independent crash/replay/compaction regression scores a canonical
  expected map without sharing the mask helper
  (`frontend/src-tauri/tests/vector_backend_benchmark.rs:161-184,238-254,1150-1297`).
  The production-shaped envelope keeps active snapshot, streamed shadow,
  delta/tombstones, and both warmed selected sessions alive in one process and
  asserts separate 1.25 GiB steady/1.30 GiB transient limits
  (`frontend/src-tauri/tests/vector_backend_benchmark.rs:1750-1754,1890-2010`).
  The independent recorded peak is 1317.9 MiB, with exactly two snapshots and
  13.3 MiB margin (`docs/hybrid-rag/task-1.r3-vector-envelope.md:94-169`). The
  deterministic suite passed 10/10 in this review.
- **RESOLVED WITH NARROW HANDOFF RISK — Sprint 2 has an authoritative exact
  backend and rollback contract.** The architecture requires exact immutable
  base + delta + tombstones, canonical journal replay, production
  remeasurement, and activation blocking above 1.30 GiB
  (`docs/hybrid-rag/architecture.md:934-958`); Sprint 2 requires pre-load bundle
  verification/fallback and a production 250k rerun
  (`docs/hybrid-rag/sprint-2-durable-local-index.md:274-333,482-569`). Rollback
  remains lexical-first and derived-state-only, with old-binary rollback
  correctly requiring a verified pre-upgrade database backup
  (`docs/hybrid-rag/architecture.md:1742-1769`). Residual risk: the Sprint 2
  file still uses generic “exact or exact+ANN” wording
  (`docs/hybrid-rag/sprint-2-durable-local-index.md:494-505`); architecture
  authority makes ANN unavailable, but the task should be read together with
  the Sprint 1 addendum before dispatch.

**Required follow-ups:**
- Correct the active root staging step to run from `upstream/` (or invoke the
  `upstream/frontend/...` path), then execute the actual root Windows workflow
  through staged-bundle verification, reference inference, and Tauri packaging.
- Do not change the model/runtime/package design for this blocker; the cheaper
  correction is one workflow path/working-directory fix. Sprint 1 remains open
  until the active run passes.

**Assumptions/risks:** No successful GitHub-hosted run was available to assume;
local staged inference and the previously recorded 250k release reruns remain
evidence for their own mechanisms, not evidence that the active workflow can
reach them. The 13.3 MiB transient margin is narrow, so Sprint 2's required
production remeasurement remains activation-blocking. Runtime invocation of
`parse_manifest`/`verify_artifacts` remains deliberately owned by Sprint 2 and
must convert failure to semantic-unavailable/FTS fallback.

**Reviewed files:** full `docs/hybrid-rag/architecture.md`; full
`docs/hybrid-rag/sprint-1-quality-gates.md`; both prior review records in that
sprint file; full reports `task-1.4-vector-backend.md`,
`task-1.5-model-supply-chain.md`, `task-1.r1-active-ci.md`,
`task-1.r2-package-provenance.md`, and `task-1.r3-vector-envelope.md`; Sprint 2
handoff `sprint-2-durable-local-index.md`; all tracked and untracked Sprint 1
implementation changes, including root and nested Windows workflows, Cargo
manifests/lock/toolchain, `.gitignore`, Tauri resource configuration,
`src/lib.rs`, `src/model_bundle.rs`, the staging script, production manifest,
both license/notice files, bundle README, model/reference benchmark changes,
exact-vector benchmark, Hybrid RAG README, and cross-program execution note.

### Post-Remediation Code Review

**Required because:** Re-verify that the `1.R1`/`1.R2`/`1.R3` remediation
resolves every blocker and should-fix raised by both prior review records
before Sprint 1 can close and Sprint 2 can begin.

**Reviewer:** `reviewer` (`SPR1-HR-CR-POST-20260825b`, main-agent session
`e762b0ec-7ac3-454d-a05c-49ad443b817d`)
**Verdict:** Changes requested
**Findings:**
- **BLOCKER — the active root workflow still invokes the staging script from a
  path that does not exist on the runner.** GitHub Actions runs each step from
  the checkout root unless `working-directory` is set, and every other Rust/
  bash step in this file explicitly enters `upstream/` before invoking a
  crate-relative path
  (`.github/workflows/build-windows.yml:135,147,155,166,217,264-266,345`). The
  staging step at `.github/workflows/build-windows.yml:138-140` is
  `shell: pwsh` with no `cd` and no `working-directory`, and calls
  `./frontend/src-tauri/scripts/stage-retrieval-models.ps1`; the script exists
  only under `upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1`,
  as `Task 1.5`, `1.R1`, and `1.R2` all confirm. From the checkout root, a
  local `ls` reproduces the failure (`No such file or directory`). PowerShell
  will fail with "term is not recognized" before any HF fetch happens; the job
  never reaches the ten-artifact assertion, the reference-inference gate, or
  the Tauri build. This is the identical bug the prior architecture re-review
  named as its sole blocker (`SPR1-HR-AR-POST-20260825`,
  `docs/hybrid-rag/sprint-1-quality-gates.md:2523-2534`), and the
  `task-1.r1-active-ci.md:53` command line records the same wrong path — the
  `1.R1` worker never noticed it, and no later remediation touched it. Cheapest
  correction: add `working-directory: upstream` to the staging step (or wrap
  the script call in a multi-line pwsh block that runs `Set-Location upstream`
  first), then execute one full active-workflow run through the ten-artifact
  assertion, the reference-inference gate, and the Tauri build. Do not change
  the script, manifest, or benchmark to accommodate the mispath.
- **RESOLVED — prior CI-authority blocker (partial):** the actual root
  `.github/workflows/build-windows.yml:88-111,132-136,142-156` now reads the
  pinned channel from `upstream/rust-toolchain.toml`, installs it via
  `dtolnay/rust-toolchain@master` with `toolchain: <parsed>`, and rejects any
  active `rustc` version that drifts from the file; both jobs enforce the same
  assertion. The check-in `rust-toolchain.toml:1-4` pins `1.88.0` and the
  workspace/member manifests both declare `rust-version = "1.88"`. The nested
  `upstream/.github/workflows/build-windows.yml` remains inert but is no
  longer authoritative; only the staging-invocation blocker above prevents
  the release gate from actually reaching its correct downstream steps.
- **RESOLVED — prior "reference-inference gate absent" blocker
  (implementation-side):** the active workflow contains a fail-closed
  reference-inference step
  (`.github/workflows/build-windows.yml:150-156`) that reuses the Task 1.3
  harness against the staged bundle via `MEETLY_RAG_BUNDLE_DIR`. The
  `upstream/frontend/src-tauri/tests/model_benchmark.rs:822-857` handler
  correctly asserts the directory exists when the env var is set and drives
  the same production-shape tokenizer/embedding/reranker path
  (`model_benchmark.rs:521-570,649-670,742-763`) with the packaged
  `models/embedding/model_int8.onnx` and `models/reranker/model_quint8_avx2.onnx`
  layout. The gate's effect is real; it is only unreachable today because of
  the upstream staging blocker above.
- **RESOLVED — arithmetic-not-measurement RAM blocker.** The R3 harness loads
  BOTH selected ONNX sessions from the staged bundle before any measurement
  (`upstream/frontend/src-tauri/tests/vector_backend_benchmark.rs:1897-1938`),
  warms each at batch-1×512, holds the reader-held active snapshot and live
  delta, streams the shadow via `load_snapshot_streaming`
  (`vector_backend_benchmark.rs:751-798`), and reads Windows peak working set
  through `K32GetProcessMemoryInfo` at the "combined holding" sample
  (`vector_backend_benchmark.rs:1960-1990`). The 1.30 GiB ceiling is asserted
  in-process (`vector_backend_benchmark.rs:1983-1990`), the 1.25 GiB steady
  cap is asserted after the shadow is dropped
  (`vector_backend_benchmark.rs:2002-2006`), and both verdict labels are
  encoded (`vector_backend_benchmark.rs:895-925`). The independent 250k rerun
  documented at `docs/hybrid-rag/task-1.r3-vector-envelope.md:127-169`
  produces `1317.9 MiB` peak / `1134.8 MiB` steady inside the approved
  ceilings.
- **RESOLVED — sparse-ID should-fix.** `base_scan_mask`
  (`upstream/frontend/src-tauri/tests/vector_backend_benchmark.rs:244-254`)
  iterates `(row, doc)` pairs and asks the overlay by stable document ID,
  never by row index. `Snapshot::row_of_doc`/`meeting_of_doc_id`
  (`vector_backend_benchmark.rs:178-184`) route every ID-to-row translation
  through binary search, and `verify_no_leak`
  (`vector_backend_benchmark.rs:1593-1615`) consumes `meeting_of_doc_id`
  instead of `meeting_of_doc[doc as usize]`. The independent regression
  `sparse_doc_ids_survive_delete_compaction_and_crash_replay`
  (`vector_backend_benchmark.rs:1150-1297`) scores expected outputs from a
  canonical expected-document map without touching `base_scan_mask`, and
  compares them to the mask+scan pipeline after replay and after compaction;
  R3's mutation proof confirms restoring the old ID-indexed mask fails it.
- **RESOLVED — superseded 1.25 GiB transient verdict.** Distinct
  `BAND_MAX_BYTES` (1.25 GiB steady) and `TRANSIENT_MAX_BYTES` (1.30 GiB
  transient) constants govern separate assertions and printed verdicts
  (`vector_backend_benchmark.rs:14-19,71-75,895-925`); the previous label that
  called an approved state `FAIL` is gone.
- **RESOLVED — manifest fail-closed / package-authority should-fix.**
  `parse_manifest` runs `validate_approved_contract` after schema validation
  (`upstream/frontend/src-tauri/src/model_bundle.rs:186-292,295-517`), binding
  every field of the selected pair — model IDs and revisions, ONNX export
  repo/revision/quantization, dimensions, prefixes, pooling, normalization,
  tokenizer type/truncation, artifact paths, exact per-artifact pinned
  revision-resolve URLs, and both license authorities — to the approved
  constants at the top of the file. The 20-case
  `selected_contract_substitutions_fail_parsing` mutation
  (`model_bundle.rs:1442-1581`) asserts that every substitution fails
  parsing itself, not merely a downstream check. The active workflow runs
  this validator before staging (`.github/workflows/build-windows.yml:132-136`),
  and the packaging config now surfaces exactly one retrieval resource —
  `resources/retrieval/bundle` — with the duplicate manifest/licenses entries
  removed (`upstream/frontend/src-tauri/tauri.conf.json:95-102`).
- **RESOLVED — embedding-export redistribution provenance.** The generic MIT
  template is gone; the packaged notice
  `licenses/e5-base-MIT-NOTICE.txt` (3289 bytes, sha256 pinned in the
  manifest) documents the intfloat→Xenova→microsoft/unilm chain, carries the
  actual `Copyright (c) Microsoft Corporation`, and is enforced by both the
  manifest validator (`upstream/frontend/src-tauri/src/model_bundle.rs:62-68,458-483`)
  and the checked-in-manifest test asserting the notice contains no template
  placeholders (`upstream/frontend/src-tauri/src/model_bundle.rs:1656-1683`).
- **RESOLVED — publication/recovery ambiguity.** The staging script keeps a
  single reusable `Assert-PackageIntegrity` gate that verifies every
  candidate package (staged, backup, published) by byte-length, SHA-256,
  byte-identical manifest copy against the checked-in publication authority,
  the pinned README placeholder, and rejection of unmanifested content
  (`upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1:82-119`);
  `Restore-CrashedPublication` runs BEFORE stale-dir cleanup and applies the
  same gate before renaming a sole backup into `bundle/`
  (`stage-retrieval-models.ps1:121-132,321-330`). Ambiguous multi-backup
  states are refused, and the 10-family `-SelfTest`
  (`stage-retrieval-models.ps1:134-295`) proves intact recovery,
  missing/corrupt/foreign/README-less backup rejection, ambiguous-recovery
  refusal, unmanifested/tampered-placeholder/divergent-manifest rejection,
  and the clean-package control.
**Should-fix (non-blocking) noted during this review:**
- The `1.R1` report (`upstream/docs/hybrid-rag/task-1.r1-active-ci.md:53`)
  transcribes the same wrong path the workflow uses. Update the report line
  alongside the workflow fix so a future reviewer does not treat the report
  as evidence that the invocation is correct.
- `MEETLY_RAG_VERIFY_STAGED_BUNDLE`/`MEETLY_RAG_BUNDLE_DIR` failure paths do
  the right thing in the harness but there is no negative CI job asserting
  fail-closed behavior on the runner itself (i.e., a scheduled run with a
  deliberately empty `bundle/` should still exit non-zero on the
  ten-artifact assertion and reference-inference gate). Adding one after the
  primary staging fix lands would prove the whole gate chain end-to-end and
  guard against future silent regressions.
**Required follow-ups:**
- Fix the active root workflow's staging step to invoke the script from
  `upstream/` (either `working-directory: upstream` on the step, or a
  multi-line pwsh block that changes directory first). Execute one full
  active `build-windows` run through the ten-artifact assertion, reference
  inference, and Tauri packaging, and attach the run's URL/logs as evidence.
- Correct `task-1.r1-active-ci.md:53` to reference the corrected invocation
  in the same change.
- Verify that fresh cache and warm cache both reach a published bundle
  containing manifest + README + ten artifacts on the runner (no local-only
  substitute).
**Remaining risks:**
- The measured transient rebuild peak has a ~14.9 MiB margin against the
  1.30 GiB ceiling. Sprint 2 MUST re-measure under production allocation and
  batch shapes; any true third resident snapshot or a larger reranker depth
  can push it over. R3's assertion already fails loudly if that happens.
- Nothing calls `parse_manifest`/`verify_artifacts` at application startup
  yet (`upstream/frontend/src-tauri/src/lib.rs:49`); Sprint 2 must invoke
  the verifier exactly once off Tokio worker threads immediately before the
  first ORT session, and convert failure to `semantic-unavailable`/FTS
  fallback per `architecture.md`.
- Sprint 2's task description still carries generic "exact or exact+ANN"
  wording; architecture authority forbids ANN as a RAM remedy, but the
  Sprint 1 addendum should be re-read before Sprint 2 dispatch (this is a
  Sprint 2 hygiene item, not a Sprint 1 blocker).
- Session residency measurement is Windows-specific (K32GetProcessMemoryInfo);
  the macOS/Linux targets remain deferred with the platforms themselves, so
  the measurement basis is aligned with the release scope.
**Verification performed in this review (documentation and static checks
only — no fresh benchmark or CI run):**
- Read full `upstream/docs/hybrid-rag/architecture.md`, full
  `sprint-1-quality-gates.md` including the Code Review, Architecture Review,
  and Post-Remediation Architecture Review records; read task reports 1.4,
  1.5, 1.R1, 1.R2, 1.R3 end-to-end.
- Inspected every uncommitted Sprint 1 tracked/untracked change: root and
  nested `build-windows.yml`, `upstream/rust-toolchain.toml`, workspace and
  Tauri `Cargo.toml`, `Cargo.lock`, `upstream/.gitignore`, `tauri.conf.json`,
  `src/lib.rs`, `src/model_bundle.rs`,
  `scripts/stage-retrieval-models.ps1`, `tests/model_benchmark.rs`,
  `tests/vector_backend_benchmark.rs`,
  `resources/retrieval/model-bundle.manifest.json`, both packaged license
  notices, and the `bundle/README.md` placeholder. Unrelated user
  documentation edits under `docs/hybrid-rag/README.md` and
  `docs/notes-chat-improvement-execution.md` were preserved and excluded.
- Reproduced the staging-invocation defect by resolving
  `./frontend/src-tauri/scripts/stage-retrieval-models.ps1` against the
  checkout root: the path does not exist; the same file resolves under
  `upstream/frontend/src-tauri/scripts/`. This mirrors the mechanism the
  prior architecture re-review identified.
- No source, script, workflow, manifest, resource, or test file was edited
  by this review; only this record is appended.
**Files reviewed:** `.github/workflows/build-windows.yml`,
`upstream/.github/workflows/build-windows.yml`, `upstream/.gitignore`,
`upstream/Cargo.toml`, `upstream/Cargo.lock`, `upstream/rust-toolchain.toml`,
`upstream/frontend/src-tauri/Cargo.toml`,
`upstream/frontend/src-tauri/src/lib.rs`,
`upstream/frontend/src-tauri/src/model_bundle.rs`,
`upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1`,
`upstream/frontend/src-tauri/tauri.conf.json`,
`upstream/frontend/src-tauri/tests/vector_backend_benchmark.rs`,
`upstream/frontend/src-tauri/tests/model_benchmark.rs`,
`upstream/frontend/src-tauri/resources/retrieval/model-bundle.manifest.json`,
`upstream/frontend/src-tauri/resources/retrieval/licenses/e5-base-MIT-NOTICE.txt`,
`upstream/frontend/src-tauri/resources/retrieval/licenses/mmarco-mMiniLMv2-Apache-2.0.txt`,
`upstream/frontend/src-tauri/resources/retrieval/bundle/README.md`,
`upstream/docs/hybrid-rag/architecture.md`,
`upstream/docs/hybrid-rag/sprint-1-quality-gates.md`,
`upstream/docs/hybrid-rag/task-1.4-vector-backend.md`,
`upstream/docs/hybrid-rag/task-1.5-model-supply-chain.md`,
`upstream/docs/hybrid-rag/task-1.r1-active-ci.md`,
`upstream/docs/hybrid-rag/task-1.r2-package-provenance.md`, and
`upstream/docs/hybrid-rag/task-1.r3-vector-envelope.md`. Unrelated user/main
documentation changes in `upstream/docs/hybrid-rag/README.md` and
`upstream/docs/notes-chat-improvement-execution.md` were preserved and
excluded from the review scope.

### Post-Remediation Architecture Review

**Required because:** Model supply chain, packaging authority, exact-backend
semantics, resource-envelope contract, and the Sprint 2 handoff are decisions
that constrain every later sprint; this re-review determines whether the
`1.R1`/`1.R2`/`1.R3` remediation actually closes the two prior review records
against `architecture.md`'s normative requirements.

**Reviewer:** `arch-reviewer` (`SPR1-HR-AR-POST2-20260825`, main-agent session
`e762b0ec-7ac3-454d-a05c-49ad443b817d`)
**Verdict:** Changes requested
**Findings:**
- **BLOCKER — the mandatory pre-Sprint-2 Windows reference-inference gate
  remains architecturally unreachable in active CI.** `architecture.md`
  requires the Windows runner to fetch/verify model artifacts and execute
  tokenizer, embedding, and reranker inference against the staged resource
  layout before Sprint 2 (`docs/hybrid-rag/architecture.md:1712-1719`), and
  the Task 1.5 spec pins the same authority
  (`docs/hybrid-rag/sprint-1-quality-gates.md:1318-1322`). The active root
  workflow now contains the correct gate machinery — pinned Rust 1.88.0 with
  assertion, model cache, manifest-contract validation, staging, ten-artifact
  assertion, and reference inference against `MEETLY_RAG_BUNDLE_DIR`
  (`.github/workflows/build-windows.yml:88-156`) — but the staging step at
  `.github/workflows/build-windows.yml:138-140` is `shell: pwsh` with no
  `cd`/`working-directory` and calls
  `./frontend/src-tauri/scripts/stage-retrieval-models.ps1`, while every
  neighbouring bash/Rust step in the same job explicitly enters `upstream/`
  before invoking a crate-relative path
  (`.github/workflows/build-windows.yml:135,147,155,166,217,264-266,345`).
  The script exists only at
  `upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1`, so
  PowerShell fails with "term is not recognized" before the ten-artifact
  assertion and reference inference can run, and before the Tauri build
  begins. This is the same defect the prior architecture re-review named as
  its sole blocker (`SPR1-HR-AR-POST-20260825`,
  `docs/hybrid-rag/sprint-1-quality-gates.md:2523-2534`), and it is
  transcribed verbatim in the `1.R1` report
  (`upstream/docs/hybrid-rag/task-1.r1-active-ci.md:53`) — no downstream task
  touched it. Cheapest correction: add `working-directory: upstream` to the
  step (or wrap the invocation in a multi-line pwsh block whose first line
  is `Set-Location upstream`); do not weaken the script, manifest, or gate.
  Sprint 1 remains open until an actual root `build-windows` run passes
  through staging, ten-artifact assertion, reference inference, and Tauri
  packaging with attached run URL/logs.
- **RESOLVED — exact-toolchain contract now enforced at the CI authority.**
  Both root-workflow jobs read `channel` from `upstream/rust-toolchain.toml`
  with POSIX sed, install `dtolnay/rust-toolchain@master` at that channel,
  then assert `rustc --version` matches; drift fails the job
  (`.github/workflows/build-windows.yml:88-111,306-329`). The pinned channel
  is `1.88.0` (`upstream/rust-toolchain.toml:1-4`) and both manifests declare
  `rust-version = "1.88"` (`upstream/Cargo.toml:8-11`,
  `upstream/frontend/src-tauri/Cargo.toml:5-10`), matching the Task 1.5
  MSRV evidence and the `architecture.md` toolchain contract addendum
  (`docs/hybrid-rag/architecture.md:1730-1740`). No floating `stable`
  remains in the active workflow.
- **RESOLVED — singular signed bundle authority.** Tauri packages exactly
  one retrieval resource, `resources/retrieval/bundle`
  (`upstream/frontend/src-tauri/tauri.conf.json:95-102`); the duplicated
  manifest and licenses resource entries flagged by the prior architecture
  review are gone. `architecture.md` "Package Authority And Provenance"
  requires the staged bundle to be the only packaged retrieval authority
  and to reject unmanifested content
  (`docs/hybrid-rag/architecture.md:499-506`); the staging script enforces
  exactly that through one reusable `Assert-PackageIntegrity` gate applied
  to staged, backup, and published states
  (`upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1:82-119,321-330,395-415`).
- **RESOLVED — fail-closed manifest contract binding the approved pair.**
  `parse_manifest` now runs `validate_approved_contract` after schema
  validation and binds every architecture-mandated identity — bundle ID,
  chunker version, both model IDs and revisions, ONNX export
  repo/revision/quantization, dimensions, prefixes, pooling, normalization,
  tokenizer type/truncation/revision-to-export-revision equality, exact
  artifact path sets per component, per-artifact pinned revision-resolve
  URLs, and both license authorities
  (`upstream/frontend/src-tauri/src/model_bundle.rs:186-292,295-517`). The
  20-case `selected_contract_substitutions_fail_parsing` mutation
  (`upstream/frontend/src-tauri/src/model_bundle.rs:1442-1581`) proves that
  every substitution named by `architecture.md`'s approved bundle contract
  (`docs/hybrid-rag/architecture.md:525-539`) fails parsing itself, not just
  a downstream assertion. The active workflow runs the validator via
  `cargo test --lib model_bundle` before staging
  (`.github/workflows/build-windows.yml:132-136`); the parser is not on the
  startup path yet (correct for Sprint 1) and will be wired off Tokio worker
  threads in Sprint 2.
- **RESOLVED — immutable provenance and applicable notice.** The generic
  MIT template is gone; the packaged notice is
  `licenses/e5-base-MIT-NOTICE.txt` (3289 bytes, sha256 pinned in the
  manifest) documenting the intfloat→Xenova→microsoft/unilm chain with the
  actual `Copyright (c) Microsoft Corporation`. The manifest validator
  enforces the notice path, source URL, resource, attribution, and SPDX
  values against the constants at the top of the file
  (`upstream/frontend/src-tauri/src/model_bundle.rs:62-68,458-483`); a
  separate checked-in-manifest test asserts the on-disk notice contains no
  `<year>`/`<copyright holders>` placeholders and carries the required
  copyright and export attributions
  (`upstream/frontend/src-tauri/src/model_bundle.rs:1656-1683`). This meets
  the `architecture.md` "Package Authority And Provenance" chain of custody
  (`docs/hybrid-rag/architecture.md:499-515`).
- **RESOLVED — validated recovery with full package integrity.**
  `Restore-CrashedPublication` runs BEFORE stale-dir cleanup and applies
  the same `Assert-PackageIntegrity` gate as staging/publish before
  renaming a sole backup into `bundle/`
  (`upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1:121-132,321-330`);
  multiple backups refuse ambiguous recovery. The 10-family `-SelfTest`
  proves intact recovery, missing/corrupt/foreign/README-less backup
  rejection, ambiguous-recovery refusal, unmanifested/tampered-placeholder/
  divergent-manifest rejection, and the clean-package control
  (`upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1:134-295`).
  The design remains single-publisher without a journal, consistent with
  `architecture.md`'s recovery contract.
- **RESOLVED — exact backend sparse-ID semantics.** `base_scan_mask` now
  iterates `(row, doc)` pairs from `snap.doc_ids` and asks the overlay by
  stable document ID
  (`upstream/frontend/src-tauri/tests/vector_backend_benchmark.rs:244-254`);
  `Snapshot::row_of_doc`/`Snapshot::meeting_of_doc_id` centralize every
  ID-to-row translation
  (`upstream/frontend/src-tauri/tests/vector_backend_benchmark.rs:178-184`);
  `verify_no_leak` consumes `meeting_of_doc_id`
  (`upstream/frontend/src-tauri/tests/vector_backend_benchmark.rs:1593-1615`).
  The independent regression
  `sparse_doc_ids_survive_delete_compaction_and_crash_replay`
  (`upstream/frontend/src-tauri/tests/vector_backend_benchmark.rs:1150-1297`)
  scores the expected side from a canonical expected-document map (no shared
  mask/index helper) after crash-window replay and after compaction, and R3
  reports a documented mutation-check proof that restoring the old
  ID-indexed mask fails the regression
  (`docs/hybrid-rag/task-1.r3-vector-envelope.md:52-75`). This preserves
  `architecture.md`'s "Exact Option" invariants that stable identity
  survives compaction and reload
  (`docs/hybrid-rag/architecture.md:899-917`).
- **RESOLVED — actual active+shadow+delta+both-session envelope with valid
  two-snapshot semantics.** The R3 harness loads BOTH selected ONNX
  sessions from the staged bundle before any measurement, warms each at
  batch-1×512, holds the reader-held active snapshot plus the live
  delta/tombstone overlay, streams the shadow via
  `load_snapshot_streaming` with exact-capacity reservation and bounded
  4,096-row chunks, and reads the Windows process peak working set at the
  combined-holding sample; the 1.30 GiB transient ceiling and the 1.25 GiB
  steady band are asserted in-process
  (`upstream/frontend/src-tauri/tests/vector_backend_benchmark.rs:751-798,1897-2010`).
  The `[envelope-transient]` line explicitly states "exactly two snapshots
  held" and the ceiling is guarded by
  `TRANSIENT_MAX_BYTES`/`BAND_MAX_BYTES` constants
  (`upstream/frontend/src-tauri/tests/vector_backend_benchmark.rs:14-19,71-75,895-925`).
  The independent 250k rerun records `1317.9 MiB` peak and `1134.8 MiB`
  steady with a `13.3 MiB` margin
  (`docs/hybrid-rag/task-1.r3-vector-envelope.md:127-169`), matching the
  approved 2026-08-24 architecture amendment
  (`docs/hybrid-rag/architecture.md:944-958`). A true third resident
  snapshot or any higher peak still fails loudly as
  `[blocked-resource-envelope]`.
- **RESOLVED WITH NARROW HANDOFF RISK — Sprint 2 exact backend contract and
  rollback story.** `architecture.md`'s "Approved Exact Backend Contract"
  hands Sprint 2 an immutable base + delta + tombstones + publication
  journal, measured initial limits (150 candidates, 2 scan permits, queue
  8, 250 ms pause, 128-doc update batch, ≤2% compaction threshold), and
  the requirement to re-measure under production allocation and to block
  activation above 1.30 GiB (`docs/hybrid-rag/architecture.md:934-958`).
  Sprint 2's task list must be read alongside this addendum before
  dispatch; the prior architecture re-review flagged that Sprint 2's file
  still carries generic "exact or exact+ANN" wording
  (`docs/hybrid-rag/sprint-1-quality-gates.md:2569-2573`). Rollback stays
  ordered lexical-first, then pause, then rebuild, then disable-semantic
  ship, then binary rollback against a verified pre-upgrade DB backup
  (`docs/hybrid-rag/architecture.md:1742-1769`); no destructive path was
  introduced. Runtime invocation of `parse_manifest`/`verify_artifacts`
  remains deferred to Sprint 2 and must be called exactly once off Tokio
  worker threads immediately before first ORT session creation, converting
  failure to semantic-unavailable/FTS fallback
  (`upstream/frontend/src-tauri/src/model_bundle.rs:530-552`;
  `docs/hybrid-rag/architecture.md:1400-1416`).
**Required follow-ups:**
- Fix the active root workflow's staging step to run from `upstream/`
  (`working-directory: upstream` on the step, or a multi-line pwsh block
  that changes directory first); execute one full active `build-windows`
  run through staging, ten-artifact assertion, reference inference, and
  Tauri packaging on a runner, and attach the run URL/logs as evidence.
- Correct `upstream/docs/hybrid-rag/task-1.r1-active-ci.md:53` in the same
  change so the report cannot be cited as evidence that the invocation is
  correct.
- Do not change the model/runtime/package/manifest/benchmark contracts for
  this blocker; the failure is one workflow path/working-directory fix.
**Assumptions/risks:**
- The measured `1317.9 MiB` transient peak has a `13.3 MiB` margin against
  the approved `1.30 GiB` ceiling; Sprint 2 MUST re-measure under
  production allocation and reranker batch shapes and the assertion will
  fail loudly if a true third snapshot or a larger reranker depth pushes
  the peak over.
- No successful GitHub-hosted `build-windows` run is available as evidence
  yet; local reference-inference and 250k reruns prove their own mechanisms
  but do not prove the active workflow can reach them until the blocker is
  fixed.
- Sprint 2 must own the run-once, off-Tokio-thread invocation of
  `parse_manifest`/`verify_artifacts` before first ORT session creation and
  the failure-to-FTS fallback conversion.
- macOS/Linux workflows remain deferred with the platforms themselves; the
  Windows-only measurement basis is aligned with the release scope.
- The prior review's Sprint 2 documentation-wording risk still applies: the
  Sprint 2 file's "exact or exact+ANN" phrasing must be read against the
  Sprint 1 addendum that makes ANN unavailable as a RAM remedy.
**Reviewed files:** `.github/workflows/build-windows.yml`,
`upstream/.github/workflows/build-windows.yml`, `upstream/.gitignore`,
`upstream/Cargo.toml`, `upstream/Cargo.lock`, `upstream/rust-toolchain.toml`,
`upstream/frontend/src-tauri/Cargo.toml`,
`upstream/frontend/src-tauri/src/lib.rs`,
`upstream/frontend/src-tauri/src/model_bundle.rs`,
`upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1`,
`upstream/frontend/src-tauri/tauri.conf.json`,
`upstream/frontend/src-tauri/tests/vector_backend_benchmark.rs`,
`upstream/frontend/src-tauri/tests/model_benchmark.rs`,
`upstream/frontend/src-tauri/resources/retrieval/model-bundle.manifest.json`,
`upstream/frontend/src-tauri/resources/retrieval/licenses/e5-base-MIT-NOTICE.txt`,
`upstream/frontend/src-tauri/resources/retrieval/licenses/mmarco-mMiniLMv2-Apache-2.0.txt`,
`upstream/frontend/src-tauri/resources/retrieval/bundle/README.md`, full
`upstream/docs/hybrid-rag/architecture.md`, full
`upstream/docs/hybrid-rag/sprint-1-quality-gates.md` including the Code
Review, Architecture Review, Post-Remediation Architecture Review, and
Post-Remediation Code Review records above, and reports
`upstream/docs/hybrid-rag/task-1.4-vector-backend.md`,
`upstream/docs/hybrid-rag/task-1.5-model-supply-chain.md`,
`upstream/docs/hybrid-rag/task-1.r1-active-ci.md`,
`upstream/docs/hybrid-rag/task-1.r2-package-provenance.md`, and
`upstream/docs/hybrid-rag/task-1.r3-vector-envelope.md`. Unrelated user/main
documentation changes in `upstream/docs/hybrid-rag/README.md` and
`upstream/docs/notes-chat-improvement-execution.md` were preserved and
excluded from the review scope.

### Final Post-Correction Code Review

**Required because:** This is the final Sprint 1 code review after accepted
corrective tasks `1.R1a` and `1.R3a`, covering every uncommitted Sprint 1
change and the prior review findings.

**Reviewer:** `reviewer` (`SPR1-HR-CR-FINAL-20260825`)
**Verdict:** Approve
**Findings:**
- **Blocker:** None.
- **Should-fix:** None. The active repository-root workflow now invokes the
  staging script through the correct checkout-root-relative path, then performs
  staged artifact verification and reference inference before either sidecar or
  Tauri packaging (`.github/workflows/build-windows.yml:132-159,209-220`). The
  invocation itself passes the exact checkout-root `-SelfTest`; no hosted run is
  claimed.
- **Journal correctness:** The benchmark schema requires every upsert journal
  row to carry vector, scale, and meeting payload (`frontend/src-tauri/tests/vector_backend_benchmark.rs:552-562`), and `commit_updates` writes document state,
  immutable journal payload, and canonical advancement in one SQLite
  transaction (`frontend/src-tauri/tests/vector_backend_benchmark.rs:619-675`).
  `publish_pending` captures canonical first; `publish_through` reads only
  `(published, bound]` journal rows from one transaction snapshot, applies the
  last bounded operation per document, and advances published only to the last
  applied row (`frontend/src-tauri/tests/vector_backend_benchmark.rs:687-793`).
  The regressions independently cover same-document commits beyond the bound,
  repeated upserts, same/cross-transaction upsert-delete trails, crash replay,
  and sparse IDs (`frontend/src-tauri/tests/vector_backend_benchmark.rs:1163-1585`).
- **Regression context:** The approved model contract remains fail-closed and
  hash-verifies every managed artifact (`frontend/src-tauri/src/model_bundle.rs:186-292,530-550`); staging/recovery share the complete package-integrity gate
  (`frontend/src-tauri/scripts/stage-retrieval-models.ps1:82-130,321-415`);
  Tauri has one retrieval resource authority
  (`frontend/src-tauri/tauri.conf.json:96-100`); and Rust declarations remain
  aligned at 1.88/1.88.0 (`Cargo.toml:8-12`,
  `frontend/src-tauri/Cargo.toml:1-10`, `rust-toolchain.toml:1-4`). No new
  privacy, analytics, secret-logging, or remote runtime path was introduced.
- **Documentation:** The governing conservative peak is consistently recorded
  as `1319.9 MiB`; the latest independent matrix is correctly distinguished as
  p95 `61.1 ms`, recall@150 `1.0000`, steady `1133.8 MiB`, and peak
  `1316.9 MiB` (`docs/hybrid-rag/sprint-1-quality-gates.md:364,1516,2341-2344`;
  `docs/hybrid-rag/architecture.md:938-946`). Earlier `1.4`, `1.R3`, and worker
  `1.R3a` measurements remain labeled as historical runs rather than replacing
  the governing/latest-independent figures.

**Evidence gap (not a source-code defect):** The corrected root workflow is
uncommitted and unpushed, so GitHub-hosted staging, cache restore/fetch, staged
verification, reference inference, and Tauri packaging cannot yet have run.
Per the approved Sprint requirement, Sprint 1 close still requires one
successful hosted root `build-windows` execution and attached run evidence; the
absence of that run does not change this code-review verdict
(`docs/hybrid-rag/task-1.r1a-ci-path.md:72-84`).

**Residual risks:** The conservative rebuild result has only about `11.3 MiB`
of headroom, so Sprint 2's production-shaped remeasurement remains
activation-blocking. The journal here is benchmark-local; Sprint 2 must retain
self-contained payloads or an equivalently immutable revision-addressed replay
contract and a single serialized publisher. Runtime invocation of bundle
verification and semantic-unavailable/FTS fallback also remains intentionally
owned by Sprint 2.

**Verification:** Reviewer rerun: deterministic vector suite 13/13, exact
checkout-root staging `-SelfTest`, `cargo fmt --check`, `cargo check`, and `git
diff --check` all passed (only pre-existing CRLF warnings). Accepted independent
evidence additionally records the release-gated 250k matrix passing at p95
`61.1 ms`, recall@150 `1.0000`, steady `1133.8 MiB`, and peak `1316.9 MiB`.

**Files reviewed:** all uncommitted Sprint 1 tracked and untracked changes:
root and nested Windows workflows; `.gitignore`; Cargo manifests, lockfile, and
toolchain; `src/lib.rs`, `src/model_bundle.rs`, staging script, Tauri config,
model/vector benchmarks, production manifest/licenses/bundle placeholder; and
all changed/new Hybrid RAG reports, README, architecture, quality-gate, and
cross-program execution documentation.

**Follow-up tasks created:** None. The hosted workflow run is an outstanding
Sprint approval-gate action, not a corrective source task.

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
