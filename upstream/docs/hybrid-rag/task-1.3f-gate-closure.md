# Task 1.3F — Gate-Stage Fidelity, Gate Admissibility, and Critical-Case Closure Evidence

| Field | Value |
|---|---|
| Status | **Complete** — all five deliverables produced; every remaining `1.3` blocker classified into category (a)/(b)/(c) with measured evidence. No gate threshold, metric definition, corpus case content, tuned constant, tuning objective, or held-out partition changed. |
| Date | 2026-08-24 |
| Owner | fresh `worker-l` session (`opencode-go/ox-alpha-free`, no substitution) |
| Baseline | commit `fd8a33c` (tests/ identical to instrument baseline `7318c0c`; verified `git diff 7318c0c HEAD -- frontend/src-tauri/tests/` is empty) |
| Harness | `frontend/src-tauri/tests/model_benchmark.rs`, `frontend/src-tauri/tests/retrieval_evaluation.rs`, fixtures unchanged |
| Method | Fidelity fixes applied to the benchmark simulation only; canonical release benchmark rerun before/after against the approved 1.2R corpus with staged pinned artifacts (hashes verified by the existing reference-inference contract). Supervised checks are assert-and-report only. Reference/critical cases were not touched by any tuning path; the feasibility probe is diagnostic existence evidence, not tuning. |

## 1. Scope discipline

Changed files (complete list): `frontend/src-tauri/tests/model_benchmark.rs`,
`frontend/src-tauri/tests/retrieval_evaluation.rs`, and this report. No
production file, PRD, `architecture.md`, README, ROADMAP, Cargo dependency
file, threshold, metric definition, fixture content, tuned constant, or tuning
objective was modified (`git status`/`git diff --stat` in §9).

Permanently retired `bge-reranker-base`: it is no longer loaded or executed
anywhere. Its recorded figures are reused where the PRD permits (manifest
expectation groups are kept as data; replay skips them with a printed
`[retired]` note). The Stage-D candidate set is now exactly the contracted
set: mmarco-quint8 (production candidate) and mmarco-f32 (quantization-cost
reference). The stability probe loads mmarco-quint8 instead of bge.

## 2. Deliverable 1 — Harness fidelity audit (architecture-designed exclusions)

### 2.1 Deleted-state handling — GAP CONFIRMED, FIX METRIC-INERT (proven)

**Gap.** `open_case_pool` inserted `meeting_fts` rows for `Deleted`
meetings (the evidence-insert loop was not state-guarded) while production
deletes FTS rows together with the meeting row
(`database/repositories/meeting.rs` deletes `meeting_fts WHERE meeting_id`).
Semantic docs already skipped Deleted meetings.

**Channel analysis (was any channel affected?).** Every
`FtsRepository::search_*` / `get_by_meeting_ids` path `JOIN meetings ON
fts.meeting_id = m.id`, so deleted rows could never be *returned*; they also
could not consume AND/OR limit slots (SQL LIMIT applies after the join). The
remaining theoretical channel is bm25 document-frequency shift from their
presence in the index. That channel is now **measured inside the canonical
run**: for each of the 15 deleted-family cases the full returned row sequence
was compared between the aligned builder and a reconstruction of the pre-fix
builder.

**Measured result:** `[fidelity-deleted] deleted-row alignment checked 15
cases; lexical ordering identical in 15 (divergent: 0)` — the pre-fix rows
were ranking-inert on this corpus. The alignment is therefore proven
metric-inert, and the builder now matches production cascade semantics
structurally.

### 2.2 Dirty-state handling — GAP CONFIRMED, FIXED

**Gap.** `build_case_docs` embedded `indexed_text` semantic documents for
Dirty meetings into the *searchable* vector channel. Per the architecture
failure matrix ("Meeting dirty → Exclude stale semantic rows for that
meeting; allow current FTS/hydration") and the activation rules, production
never admits semantic rows for a meeting whose indexed revision is behind its
source revision: the searchable state is "no semantic contribution", with the
meeting still reachable through FTS/hydration. Embedding the draft text
simulates an index state production never has (the fixture's dirty evidence
carries `indexed_text == authoritative_text`, so the defect was *admission*,
not text choice).

**Fix.** Documents of Dirty/StaleDerived meetings carry
`vector_indexed=false`: they are excluded from the vector channel and from
raw-vector diagnostics, remain reachable through the lexical mapping (map by
chunk id), and are reranked on their indexed chunk text (reranking precedes
hydration, and a lexical candidate's index chunk is what production scores).

### 2.3 Hydration fidelity — GAP CONFIRMED, FIXED

**Gap.** `score_case_hybrid` retained matched documents using the *indexed*
chunk text. Per architecture "Authoritative Hydration", hydration loads
current authoritative content and its hash verification omits stale semantic
evidence, so retained context must contain the regenerated/current text, not
the stale derived projection.

**Fix.** Retention and fact/forbidden checks read `authoritative_text`.
Lexical hits on stale summaries therefore hydrate to the regenerated text
(production-faithful); the stale value cannot reach the prompt through a
stale derived row. Ranking behavior (FTS snippet order pre-repair) is
intentionally preserved — that transient window is what the corpus pins.

### 2.4 `rewritten_query` — REPORTED (no gap)

Cases carrying a rewritten query: exactly the 15 `follow_up_rewrite`
follow-up family cases (pronoun questions whose rewrite carries the antecedent
nouns). Production supplies a rewrite through the existing follow-up query
path, which triggers on conversational history — matching the fixtures, which
all carry history. The motivating terminological-gap critical case
(`pt-ref-chaves-acesso`) is single-turn with no history: production would NOT
synthesize a rewritten query for it, so the fixture faithfully represents
production, and its terminological gap stands as a real single-turn challenge
(consumed by the Deliverable-2 rank-1 admissibility check below).

### 2.5 Before/after metric deltas (recorded rerun → post-fix canonical run)

Baseline re-verified in-run and unchanged: semantic Recall@3 `0.00% (0/30)`,
exact Recall@3 `100.00% (90/90)` — matches the approved 1.2R record. All
before-figures are the recorded canonical rerun (manifest +
task-1.3-model-selection.md §6); after-figures are the post-fix release run
(digit-identical across two same-day runs; see §8).

| Metric | Before (pair A/B/C) | After (B/C) | Denominator |
|---|---|---|---|
| Overall Recall@1 | 85.19% / 82.22% / 80.74% (115,111,109/135) | 80.00% / 80.00% (108/135) | 135 |
| Overall Recall@3 | 100% / 100% / 99.26% (135,135,134/135) | 99.26% both (134/135) | 135 |
| Overall Recall@5 | 100% all (135/135) | 100% both (135/135) | 135 |
| Overall forbidden contamination | 24.79% all (30/121) | 12.40% both (15/121) | 121 |
| Evidence Recall@10 | 100% all (209/209) | 100% both (209/209) | 209 |
| Required-fact coverage | 100% all (149/149) | 100% both (149/149) | 149 |
| Exact-term Recall@3 | 100% all (90/90) | 100% both (90/90) | 90 |
| Semantic Recall@3 | 100% all (30/30) | 100% both (30/30) | 30 |
| Critical Recall@1 | 40% / 60% / 40% (2,3,2/5) | 60% both (3/5) | 5 |
| Critical required facts | 100% all (9/9) | 100% both (9/9) | 9 |
| Critical forbidden contamination | 66.67% all (4/6) | 66.67% both (4/6) | 6 |
| NDCG@10 final vs fused mean | 0.9009/0.8926/0.8921 vs 0.8382 | 0.8929/0.8924 vs 0.8549 | 120-case mean |
| Citation/source precision | UNEVALUATED | **100.00% both (602/602)** | 602 |
| Family fused R@3 (no rerank) | e5-base 97.04% (131/135) | e5-base 97.04% (131/135) | 135 |

Delta attribution: the deleted-row alignment moved nothing (§2.1 proof); the
dirty/stale exclusion + hydration-faithful retention account for all movement
— overall forbidden halved (stale projections can no longer contaminate),
Critical Recall@1 rose to 3/5 for both pairs (`pt-ref-nps-detrator` 2→rank 1),
and one non-critical reference-category sibling moved below top-3 (overall
R@3 135→134; the affected case lies in the non-critical reference sibling
set, still above the ≥95% gate). Pair A is not re-measured: bge is retired.

Held-out constants were legitimately re-solved by the unchanged objective on
the fixed harness: locked fusion `k=5, w_vector=1, w_lexical=0.5,
support_alpha=0.5, title_beta=0` (objective `[0,0,0,2166666]` over 360
configs), tuned gamma 0 for both rerankers. These remain non-production
search outputs exactly as before; nothing was promoted.

## 3. Deliverable 2 — Gate-admissibility invariant (supervised, report mode)

Added to `retrieval_evaluation.rs`'s supervised layer (invoked from
`corpus_supervised_labels_margin_coverage_and_distinctness_hold`, which still
passes — assert-and-report only). Output lines are labeled `[SUPERVISED:…]`.

### 3.1 Evidence admissibility + co-residence per critical case

Method: documents = per-evidence units (matches the simulator's per-section
chunk granularity; window splitting could only fragment further). Forbidden
presence is evaluated on authoritative text (what hydration retains).
Existence proof is CONSTRUCTIVE under the benchmark's fixed retention
semantics (`HYDRATED_MEETINGS=5`, `EVIDENCE_K=10`): meetings carrying
required evidence hydrate FIRST, clean alternatives follow ordered by fewest
forbidden-bearing documents; the pooled candidate retained ordering is then
constructed GLOBALLY across all hydrated documents — required clean
documents first, other clean documents next, forbidden-bearing documents
last (stable sort, so ties keep carrier-first meeting order and fixture
order) — so a large first carrier meeting cannot push a later meeting's
required document out of the retained window. The verdict is derived from
that concrete global ordering: feasible only when it retains every required
document AND zero forbidden-bearing documents — which additionally requires
required carrier meetings to fit the hydration cap, distinct required
documents to fit the retained window (`required_le_window`), and enough
clean documents to fill the whole window ahead of any forbidden-bearing
document.

```
[SUPERVISED:co-residence] case=fixture-whatsapp-retention fact="apenas 3 dias"
    UNHITTABLE_BY_CONSTRUCTION: no document carries this fact (indexed or authoritative)
[SUPERVISED:co-residence] case=fixture-whatsapp-retention fact="apenas 4 dias"
    UNHITTABLE_BY_CONSTRUCTION
[SUPERVISED:evidence-admissibility] case=fixture-whatsapp-retention verdict=FEASIBLE_BY_ORDERING
    required_carrier_meetings=1/5 required_documents=2 required_le_window=true
    required_missing_from_pool=0 hydrated_pool_docs=8 retained_window=8
    clean_docs_in_pool=8 forbidden_bearing_docs=0
    constructive_retained_required=2/2 constructive_retained_forbidden=0

[SUPERVISED:co-residence] case=pt-ref-cobranca-regua fact="dias 5 e 15"
    carriers=[cobr-rascunho] required_co_residence=false
[SUPERVISED:evidence-admissibility] case=pt-ref-cobranca-regua verdict=UNACHIEVABLE_HYDRATION_WINDOW
    required_carrier_meetings=1/5 required_documents=2 required_le_window=true
    required_missing_from_pool=0 hydrated_pool_docs=8 retained_window=8
    clean_docs_in_pool=7 forbidden_bearing_docs=1
    constructive_retained_required=2/2 constructive_retained_forbidden=1

[SUPERVISED:co-residence] case=pt-ref-chaves-acesso fact="renovação mensal"
    carriers=[chv-rascunho] required_co_residence=false
[SUPERVISED:evidence-admissibility] case=pt-ref-chaves-acesso verdict=UNACHIEVABLE_HYDRATION_WINDOW
    required_carrier_meetings=1/5 required_documents=2 required_le_window=true
    required_missing_from_pool=0 hydrated_pool_docs=7 retained_window=7
    clean_docs_in_pool=6 forbidden_bearing_docs=1
    constructive_retained_required=2/2 constructive_retained_forbidden=1

[SUPERVISED:co-residence] case=pt-ref-sla-suporte fact="em um dia inteiro"
    carriers=[sla-antigo] required_co_residence=false
[SUPERVISED:evidence-admissibility] case=pt-ref-sla-suporte verdict=UNACHIEVABLE_HYDRATION_WINDOW
    required_carrier_meetings=1/5 required_documents=1 required_le_window=true
    required_missing_from_pool=0 hydrated_pool_docs=6 retained_window=6
    clean_docs_in_pool=5 forbidden_bearing_docs=1
    constructive_retained_required=1/1 constructive_retained_forbidden=1

[SUPERVISED:co-residence] case=pt-ref-nps-detrator fact="cupom como resposta padrão"
    carriers=[nps-cupom] required_co_residence=false
[SUPERVISED:evidence-admissibility] case=pt-ref-nps-detrator verdict=UNACHIEVABLE_HYDRATION_WINDOW
    required_carrier_meetings=1/5 required_documents=2 required_le_window=true
    required_missing_from_pool=0 hydrated_pool_docs=8 retained_window=8
    clean_docs_in_pool=7 forbidden_bearing_docs=1
    constructive_retained_required=2/2 constructive_retained_forbidden=1
```

Findings:
- **No required-document co-residence exists** — no forbidden fact rides on
  required evidence, so the strict "unachievable by co-residence" condition
  never fires.
- **The four hittable facts each live in ONE non-required note inside the
  EXPECTED meeting** (`cobr-rascunho`, `chv-rascunho`, `sla-antigo`,
  `nps-cupom`). Because the hydrated pool (6-8 docs) is smaller than the
  configured `EVIDENCE_K=10` cap, the effective retained window equals the
  whole pool; every ordering that hydrates the expected meeting — which
  required-fact coverage forces — also retains the carrier. Under the
  benchmark's staged retention semantics the zero-contamination gate is
  **unachievable at the retrieval stage for these four facts**, regardless of
  model or constants.
- The two WhatsApp facts are vacuous: their carrier strings appear only in
  the answer key, never in any document. They inflate the denominator (6) and
  can never hit; the real contamination surface is 4 facts, and the observed
  4/6 is **all hittable facts hitting**.
- For `pt-ref-sla-suporte`, the stale-derived carrier (`mtg-sla-legado`) was
  a second exposure that Deliverable 1's hydration fix eliminated: post-fix,
  the authoritative-text carrier list is `[sla-antigo]` only — the stale
  derived summary no longer appears among carriers. Note the
  architecture itself would surface `sla-antigo`: hydration includes current
  notes wholesale, so a "discarded draft" note inside the expected meeting
  reaches the model by design.

### 3.2 Rank-1 admissibility over production-implementable channels

Computed in `model_benchmark.rs` for the production-candidate e5-base-int8
family; channels are lexical margin, title margin, and the measured raw
bi-encoder vector rank. The `CONCEPT_LEXICON` is excluded.

```
[SUPERVISED:rank1-admissibility] channels = lexical/title/raw vector only:
case=fixture-whatsapp-retention   lexical=+0.900 title=-0.350 vector_rank(mtg/ev)=1/4   any_positive_channel=true
case=pt-ref-cobranca-regua        lexical=+1.500 title=+1.000 vector_rank(mtg/ev)=1/2   any_positive_channel=true
case=pt-ref-chaves-acesso         lexical=-1.333 title=-1.000 vector_rank(mtg/ev)=5/9   any_positive_channel=false
case=pt-ref-sla-suporte           lexical=-1.000 title=-1.000 vector_rank(mtg/ev)=1/5   any_positive_channel=true
case=pt-ref-nps-detrator          lexical=-1.000 title=-0.333 vector_rank(mtg/ev)=1/13  any_positive_channel=true
```

- `pt-ref-chaves-acesso` is the only critical case with **no
  production-implementable winning channel**: lexical and title margins are
  negative and the raw e5-base vector ranks the target fifth. Its 1.2R
  solvability rests solely on the hand-authored concept proxy, confirming the
  PRD premise.
- `pt-ref-sla-suporte` and `pt-ref-nps-detrator` have a positive raw vector
  channel (ranks 1); their earlier rank misses are fusion/aggregation
  outcomes, not channel absence. Post-fix, `nps-detrator` reaches rank 1 and
  `sla-suporte` rank 2 under both contracted pairs.

## 4. Deliverable 3 — Citation/source precision simulation (pairs B and C)

Implemented the architecture's source-emission stage inside Stage F per pair
using the production `app_lib::api::chat::ChatSource` type (imported into the
integration harness; no production edit). For each case the simulation:

1. retains evidence after hydration-set selection, profile exclusion, scope
   revalidation against `scope.allowed_meeting_ids`, and the final
   `EVIDENCE_K` prompt budget — in that order;
2. constructs one `ChatSource` per retained document with `meeting_id`,
   `meeting_title`, `chunk_type`, `folder_name`, `source_kind`, and a
   `snippet` populated from the authoritative retained (hydrated) text —
   never the pre-hydration indexed chunk;
3. assembles the final retained context string (the lowercased concatenation
   of retained authoritative text that required-fact checks read);
4. measures precision by **snippet containment against that actual context**
   (`context.contains(snippet)`), not identity equality between two slices of
   one tuple list.

**Falsifiability self-check** (`citation_precision_rejects_pre_budget_and_stale_snippets`,
deterministic, artifact-free): the shared `chat_source_precision` helper is
asserted to REJECT three mutation classes — a pre-budget snippet whose text
never reached the final context, a stale pre-hydration snippet superseded by
the regenerated authoritative text, and an empty-snippet emission — while
accepting conformant sources `(2,2)`. The metric can therefore fail; it is
not tautological.

**Assumptions recorded explicitly:**
1. `EVIDENCE_K=10` over hydrated meetings is the prompt-budget proxy; sources
   are attached at exactly the point the broad contract requires (after
   final truncation), which is why the conformant pipeline cannot emit a
   pre-budget source — the mutation check proves the measurement would catch
   one.
2. Profile documents are never citable (excluded upstream of emission).
3. Fixtures are static, so mid-query deletion/move cannot be exercised; scope
   revalidation runs mechanically at emission (membership recomputed from the
   authoritative scope) and would drop any source whose meeting fell out.
4. Snippets are full authoritative section texts; production may truncate for
   display, but containment is evaluated against the same text the prompt
   carries, so display-side truncation cannot create false failures here.

**Measured result (both pairs, post-fix harness):**

| Pair | Citation/source precision | Denominator |
|---|---|---|
| B: e5-base-int8 + mmarco-f32 | **PASS 100.00% (602/602)** | 602 emitted ChatSource values = sum over 120 cases of retained documents (min(EVIDENCE_K, hydrated non-profile docs)); unchanged from the earlier identity-based formulation because both emit one source per retained document |
| C: e5-base-int8 + mmarco-quint8 | **PASS 100.00% (602/602)** | same |

The previously unevaluated gate now produces a number through real
`ChatSource` construction and containment measurement; the pair verdict logic
consumes it (a failing precision would block selection).

## 5. Deliverable 4 — Constants-feasibility probe (diagnostic, NOT tuning)

The existing 360-configuration fusion grid (k∈{5,10,20,60} × w_vec∈{0.5,1,2}
× w_lex∈{0.5,1,2} × alpha∈{0,0.5} × beta∈{0,0.25,0.5,1,2}) times the gamma
grid {0,0.5,1,2,4,8} = 2160 configurations per reranker, evaluated on the
full corpus INCLUDING reference/critical cases purely as existence evidence,
on the post-Deliverable-1 harness. Passing requires Critical Recall@1 5/5
AND critical forbidden contamination 0 AND exact-term Recall@3 no-regression
against the FTS baseline (≥ 90/90).

| Reranker | Passing configurations |
|---|---|
| mmarco-quint8 | **0/2160 — none exists** |
| mmarco-f32 | **0/2160 — none exists** |

No passing configuration exists, hence no examples. This corroborates §3.1
structurally: with four forbidden facts trapped inside their expected
meetings under the staged retention window, no fusion/aggregation/rerank
weighting can satisfy the joint gates. As mandated, the probe promoted
nothing; the tuned constants remain the held-out objective's output.

## 6. Deliverable 5 — Verdict table (no blank cells)

Categories: (a) fidelity-gap-fixed · (b) achievable-but-not-at-tuned-constants
· (c) unachievable-at-retrieval-stage(-as-staged). Remedy owners: harness /
corpus patch / gate re-staging (user approval) / model.

| # | Blocker item | Verdict | Measured evidence (denominator) | Remedy owner |
|---|---|---|---|---|
| 1 | Rank miss `pt-ref-chaves-acesso` (rank 4 both pairs) | **(c)** as authored: no production-implementable channel wins rank 1 | lexical −1.333, title −1.000, e5-base raw vector rank 5/9; only the non-production `CONCEPT_LEXICON` is positive; single-turn fixture matches production (no rewrite would exist) | Corpus patch (re-author so a real channel discriminates) or re-stage to a measured achievable top-k gate (current final rank 4; raw vector rank 5) — user approval |
| 2 | Rank miss `pt-ref-sla-suporte` (rank 2 both pairs, was 3) | **(b)** raw vector channel wins (rank 1/5); tuned fusion/aggregation lands rank 2; probe shows no joint-passing config but the channel is positive | `[SUPERVISED:rank1-admissibility]` vector_rank=1; post-fix pair ranks 2/2; feasibility 0/2160 (joint) | Model/fusion: final `1.3` held-out retune on the fixed harness; escalate to user only if it persists |
| 3 | Rank miss `pt-ref-nps-detrator` (now rank 1 both pairs, was 2) | **(a)** fixed by Deliverable-1 fidelity (dirty/stale rows excluded from the vector channel changed aggregation inputs) | rerun C rank 2 → post-fix rank 1 (both pairs, denominators n/5) | None — closed by harness fidelity |
| 4 | Forbidden `"apenas 3 dias"` (whatsapp) | **(c)** vacuous: no carrier text exists in any document | co-residence scan: UNHITTABLE_BY_CONSTRUCTION; contributes 0 hits to the observed 4/6 | Corpus patch (pin a real superseded-draft carrier) — user approval |
| 5 | Forbidden `"apenas 4 dias"` (whatsapp) | **(c)** vacuous: same | same as #4 | Corpus patch — user approval |
| 6 | Forbidden `"dias 5 e 15"` (cobranca) | **(c)** as staged: carrier `cobr-rascunho` is a non-required note INSIDE the expected meeting; hydrated pool 8 docs < `EVIDENCE_K=10`, so the effective window retains the whole pool under every ordering | pool 8, window 8, bearing 1, non-bearing 7 → UNACHIEVABLE_HYDRATION_WINDOW; probe corroboration 0/2160 | Gate re-staging (scope forbidden facts to superseded/stale sources or add instruction-level guard) or corpus patch (move carrier out of expected meeting) — user approval |
| 7 | Forbidden `"renovação mensal"` (chaves) | **(c)** as staged: carrier `chv-rascunho` in expected meeting | pool 7, window 7, bearing 1 → UNACHIEVABLE_HYDRATION_WINDOW; 0/2160 | Same as #6 — user approval |
| 8 | Forbidden `"em um dia inteiro"` (sla) | **(c)** as staged, with a partial **(a)**: the stale-derived carrier (`mtg-sla-legado`) was eliminated by the hydration fix; the remaining carrier `sla-antigo` is a current note in the expected meeting (which production hydration includes wholesale by design) | post-fix carriers=[sla-antigo] only; pool 6, window 6, bearing 1 → UNACHIEVABLE_HYDRATION_WINDOW; overall contamination 30/121→15/121 shows the stale path closed | Same as #6 — user approval |
| 9 | Forbidden `"cupom como resposta padrão"` (nps) | **(c)** as staged: carrier `nps-cupom` ("idea discarded") in expected meeting | pool 8, window 8, bearing 1 → UNACHIEVABLE_HYDRATION_WINDOW; 0/2160 | Same as #6 — user approval |
| 10 | Citation/source precision (unevaluated) | **(a)** fixed: production `ChatSource` values constructed per the broad-retrieval contract; gate measured by snippet containment against the final retained context and passes; falsifiability proven by mutation self-check | 100.00% (602/602) pair B; 100.00% (602/602) pair C | None — closed; consumable by the final `1.3` run |

Reading: the three rank misses split (a)/(b)/(c) cleanly; the contamination
gate fails not because retrieval is bad but because, as staged, it is
unsatisfiable — four of six facts are structurally trapped (one of them only
partially, after the stale-path fix), and two are vacuous. Better retrieval
mechanically worsens this metric (hybrid 30/121 vs baseline 25/121 pre-fix),
which is the `1.2` lesson recurring: this gate needed an admissibility proof
before models were benchmarked against it.

## 7. Privacy result

Focused privacy scan over `frontend/src-tauri/tests/**` plus this report
(same marker set as the 1.2R audit: `c:\users\`, `/users/`, `@gmail.`,
`@outlook.`, `sk-`, `api_key`, `bearer `, `onedrive\`): tests/ contribute 10
matches — all of which are the scanner's own literals inside
`validate_private_safe`, the existing `blocked-risk-approval` decision
string, and a documentation path reference; this report contributes 5
matches — its own quotation of the marker list and command in this paragraph
and §8. Identical profile to the approved 1.2R result ("matches are
scanner/documentation literals"). No fixture or corpus content carries any
marker; the harness's in-corpus private-marker validation continues to pass.
No model weights were committed; staged artifacts remain under
`%TEMP%\opencode\meetly-task13\models`.

## 8. Reproducibility

From `upstream/`:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
$env:MEETLY_RAG_MODELS_DIR = "$env:TEMP\opencode\meetly-task13\models"

cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark

# CANONICAL full-corpus + resource evidence (release):
$env:MEETLY_RAG_BENCH = "1"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark hybrid_corpus_and_resource_benchmark -- --nocapture
Remove-Item Env:MEETLY_RAG_BENCH -ErrorAction SilentlyContinue
```

Quality tables are deterministic: after the fidelity fixes and again after
the acceptance-audit corrections (production `ChatSource`-based precision,
strengthened constructive admissibility proof), the canonical release run was
re-executed on the final tree with digit-identical quality output (critical
ranks, gate verdicts, contamination 4/6, precision 602/602 via context
containment, feasibility 0/2160 both rerankers, family/concept-proxy
tables). Solo reranker latency varied with machine state across runs (quint8
p95 7.1→12.1→13.5 ms; f32 viable at 10.9–16.0 ms, budget-excluded in one
loaded intermediate run) — consistent with the recorded §4 sensitivity; a
clean-hardware latency re-probe remains mandatory before any selection.

Resource coherence: e5-base embedding session RSS delta +553.9 MiB (retained
probe +553.3); `pair_ram_probe` for e5-base-int8+mmarco-quint8 measured a
projected 250k peak of **1117.3 MiB** (retained figure 1120.2 MiB) — inside
the user-approved 1-1.25 GiB band (2026-08-23 decision). Derived disk 558
B/doc → 0.13 GiB steady / 0.26 GiB rebuild at 250k (envelopes 2/3 GiB).
Quantization storage cost f32=fp16=int8 = 86.96% (60/69) Evidence Recall@5 —
zero measured recall cost.

Decision output remains `blocked-quality-gates` — correct: Task 1.3F closes
instrument unknowns and does not select a model or force gates to pass.

## 9. Rollback (against committed baseline `7318c0c`)

All changes are tests/evidence-only; no production surface is touched.

```powershell
git checkout 7318c0c -- frontend/src-tauri/tests/model_benchmark.rs frontend/src-tauri/tests/retrieval_evaluation.rs
```

Then delete this report file. Staged model artifacts live outside git and
need no rollback. The two harnesses return exactly to the `7318c0c` /
`fd8a33c` state (verified `git diff 7318c0c HEAD -- frontend/src-tauri/tests/`
empty before this task).

Files changed by this task:

```
M  frontend/src-tauri/tests/model_benchmark.rs   (+509/-51 region incl. fmt)
M  frontend/src-tauri/tests/retrieval_evaluation.rs (+145)
A  docs/hybrid-rag/task-1.3f-gate-closure.md     (this report)
```

## 10. Omissions and spillover

- The Task Execution Log entry for `1.3F` belongs to the main orchestrator;
  per dispatch constraints this worker did not edit
  `sprint-1-quality-gates.md` or the unrelated Notes/Chat execution record.
- Spillover (report-only, not implemented): the Task 1.2R baseline harness
  (`retrieval_evaluation.rs::setup_case`) has the same latent pattern of
  inserting FTS rows for Deleted meetings. It was deliberately left untouched
  — the approved baseline pins those numbers, and the JOIN makes the rows
  invisible except possibly through bm25 IDF. If the baseline is ever
  re-recorded, align it to production cascade semantics then.
- Cosmetic: when the held-out tuner locks `beta=0`, the title-ablation header
  prints "tuned beta=0 vs beta=0" (a degenerate self-comparison). The
  ablation remains meaningful whenever beta>0; left as-is to avoid touching
  reporting logic without need.
- The feasibility probe isolates JOINT gate passage (as specified). It does
  not attribute per-case rank feasibility; per-case channel evidence for that
  lives in §3.2.
