# Task 1.3 - Final Selection Run (Batch 7, post-`1.3G`, amendment 5)

| Field | Value |
|---|---|
| Status | **Complete — documented stop (Critical Recall@1 2/5 at held-out-tuned constants)** |
| Date | 2026-08-24 |
| Owner | `worker-l` (`openrouter/stealth/ox-alpha`) |
| Instrument | Post-`1.3G` corpus, gates, re-recorded baseline, and `1.3F` fidelity fixes; commit `6bba48b` |
| Candidate set | e5-base-int8 + mmarco-quint8 (production candidate); e5-base-int8 + mmarco-f32 (quantization-cost reference only). `bge-reranker-base` permanently retired (2026-08-23 user decision) and NOT evaluated. |

This report is the single deliverable of the Batch 7 final Task 1.3 selection run
authorized by amendment 5 (2026-08-24). It changes nothing about the corpus,
gates, models, or production code; it implements exactly the amendment-5 tuning
partition and objective, retunes every held-out constant from scratch on that
instrument, evaluates every gate with full constants disclosure, corroborates
the feasibility probe, and reports one result.

**Executive summary.** The amendment-5 retune finds a configuration whose
held-out objective is *perfect* on its first five components
(`[exact-viol=0, sem-miss=0, ref-r1-miss=0, ndcg-nondeg=0, all-miss=0]`; only
the MRR tie-break term is nonzero) — no point of the entire 2160-configuration
grid scores better on the held-out distribution. Under that best-generalizing
configuration the pair passes every gate except Critical Recall@1, which lands
at **2/5** (`fixture-whatsapp-retention` and `pt-ref-cobranca-regua` rank 1;
`pt-ref-chaves-acesso` rank 2, `pt-ref-sla-suporte` rank 3,
`pt-ref-nps-detrator` rank 2). Meanwhile the diagnostic probe confirms
78/2160 (quint8) and 79/2160 (f32) configurations still pass all three joint
gate conditions, and the tuned configuration sits OUTSIDE that passing set:
reaching it costs measurable held-out quality (best-passing key pays +2
semantic misses and +2 overall misses for quint8). This is direct evidence
that the passing region is not reachable from generalizable signal. Per the
binding amendment, this run stops and reports; the choice between a gate split
and a dated exception against Sprint 3 Task `3.6` belongs to the user.
Amendment 4 additionally escalates `pt-ref-sla-suporte` (rank 3 < 1).

## 1. What this run changed in the harness (benchmark surface only)

File touched: `frontend/src-tauri/tests/model_benchmark.rs`. No production
code, schema, dependency, model weight, architecture document, PRD, execution
log, or gate/corpus content was modified.

1. **Partition (amendment 5).** The held-out filter previously excluded every
   case carrying `reference_whatsapp` plus every critical case. It now excludes
   ONLY critical cases plus the pinned reference case
   (`excluded_from_tuning`: `case.critical || case.id ==
   "fixture-whatsapp-retention"`), admitting exactly the 10 non-critical
   reference-category cases into tuning. Regression assertion
   `amendment5_partition_excludes_only_critical_and_pinned_cases` proves:
   exactly 5 excluded cases, they are precisely the five designated
   critical/pinned IDs, every critical case stays outside tuning, exactly 10
   admitted reference-category cases, tune-partition size 115 of 120.
2. **Objective (amendment 5).** New shared accumulator `TuneObjective` builds
   the lexicographic key used by both the fusion grid and the per-candidate
   gamma grid:

   ```text
   key = [ exact-term R@3 violations,
           semantic-category R@3 misses,
           reference-category R@1 misses (10 admitted siblings),
           overall R@3 misses,
           MRR deficit (micros) ]            (fusion stage)

   rerank_key = [ exact-term R@3 violations,
                  semantic-category R@3 misses,
                  reference-category R@1 misses,
                  NDCG non-degradation flag (0/1),
                  overall R@3 misses,
                  MRR deficit (micros) ]    (gamma stage)
   ```

   The reference-R@1 miss term sits after semantic misses and before overall
   R@3 misses, as required. Regression assertion
   `amendment5_objective_orders_reference_r1_misses_between_semantic_and_overall`
   proves the scope (each category term counts only its own cases;
   reference-R@1 counts only rank != 1) and the ordering (one semantic miss
   outranks any number of reference misses; eliminating one reference miss
   outranks any reduction of overall misses), and would fail if either changed.
3. **Constants disclosure.** Every gate table is preceded by a `[constants]`
   line printing `k`, `w_vector`, `w_lexical`, `alpha`, `beta`, per-candidate
   `gamma`, depth/batch, plus the winning full objective vector. The
   `[tune-fusion]` and `[reranker]` lines print the same keys. No constants-free
   gate table remains.
4. **Feasibility corroboration.** `constants_feasibility_probe` now also
   accumulates the held-out objective over the amendment-5 partition for every
   probed configuration, detects whether the tuned configuration is inside the
   passing set, and if not prints the componentwise objective distance between
   the tuned key and the best-objective passing configuration's key.
5. **Decision semantics (final-run roles).** The printed decision now keys on
   the fixed production candidate (`e5-base-int8+mmarco-quint8`) rather than
   "any conforming pair"; the f32 pairing cannot select. A pass inside the
   pre-approved e5-base band would print `complete-band-approved`; a quality
   failure prints `blocked-quality-gates`.

Both new regression tests run offline (no artifacts required) inside the normal
`cargo test --test model_benchmark` suite.

## 2. Reproduction commands

From `upstream/`; artifacts staged at
`%TEMP%\opencode\meetly-task13\models` (verified present; the staged
e5-base-int8 export hash matches the manifest pin
`sha256-9ddfd8b45086dabc59a7e1bb00463225dace8954962418b240840f2153bc87da`,
so no re-staging was needed):

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
$env:MEETLY_RAG_MODELS_DIR = "$env:TEMP\opencode\meetly-task13\models"
$env:MEETLY_RAG_BENCH = "1"
# Canonical final-selection evidence run (release build):
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark hybrid_corpus_and_resource_benchmark -- --nocapture
# Isolated pair RAM probe:
$env:MEETLY_RAG_PAIR = "e5-base-int8/model_int8.onnx:mmarco-reranker/model_quint8_avx2.onnx"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark pair_ram_probe -- --nocapture
Remove-Item Env:MEETLY_RAG_PAIR, Env:MEETLY_RAG_BENCH -ErrorAction SilentlyContinue
```

Raw canonical output retained at
`%TEMP%\opencode\meetly-task13\final-selection-run1.log`.

## 3. Held-out retune result (from scratch; no constant carried forward)

Tuning ran on the amendment-5 partition (115 cases: 105 former held-out cases +
the 10 non-critical reference siblings). The five designated critical/pinned
cases were never inspected by any tuning path; they appear below only as
post-hoc graded output of the tuner's chosen configuration.

**Earned constants (both candidates, identical):**

| Constant | Value |
|---|---|
| RRF `k` | 5 |
| `w_vector` | 1 |
| `w_lexical` | 0.5 |
| `alpha` (support) | 0.5 |
| `beta` (title) | 1 |
| `gamma` (reranker channel; quint8 and f32) | 0 |
| support cap | 3 |
| chat rerank depth | 50 (derived: floor(900 ms / p95) capped at 50) |
| search depth / batch / intra-op | 25 / 1 / 4 |

**Winning objective vector** (gamma-stage key,
`[exact-viol, sem-miss, ref-r1-miss, ndcg-nondeg, all-miss, mrr-miss-u]`):
`[0, 0, 0, 0, 0, 2166666]`. The first five components are zero: across the
whole grid no configuration beats it on any prior component; among ties the
minimum MRR deficit (2166666 micros summed over the partition's 130 expected
meetings) selected the winner. Fusion-stage key over the same partition:
`[0, 0, 0, 0, 2166666]` over 360 configurations.

The expanded title grid (`β ∈ {0, 0.25, 0.5, 1, 2}`) was tuned jointly as
required; the objective selected β=1 — title scoring ON, chosen by the data,
not assumed.

## 4. Gate tables (constants disclosed beside each)

### 4.1 Production candidate: e5-base-int8 + mmarco-quint8

Constants: `k=5 w_vector=1 w_lexical=0.5 alpha=0.5 beta=1 gamma=0
support_cap=3 chat-depth=50 batch=1`; winning objective vector
`[0, 0, 0, 0, 0, 2166666]`.

| Gate | Result | Measured value (denominator included) |
|---|---|---|
| Reference Recall@1 (pinned case) | **PASS** | rank 1; facts 2/2; retrieval-stage forbidden 0/2 |
| Critical Recall@1 | **FAIL** | 40.00% (2/5) |
| Critical required-fact coverage | PASS | 100.00% (9/9) |
| Critical retrieval-stage forbidden contamination | PASS | 0.00% (0/2) |
| Answer-stage non-assertion | DEFERRED (not Sprint 1) | context presence informational 100.00% (4/4) |
| Exact-term no-regression | PASS | 100.00% (90/90), baseline 90/90 |
| Overall Recall@3 ≥ 95% | PASS | 100.00% (135/135) |
| Overall Recall@5 ≥ 98% | PASS | 100.00% (135/135) |
| Evidence Recall@10 ≥ 90% | PASS | 100.00% (209/209) |
| Semantic +10 pt R@3 over baseline | PASS | 100.00% (30/30) vs baseline 0/30 |
| NDCG non-degradation | PASS | final 0.8924 vs fused-order 0.8546 |
| Citation/source precision (1.3F simulation) | PASS | 100.00% (602/602) |

Overall metrics: R@1 83.70% (113/135), R@3 100% (135/135), R@5 100%
(135/135), MRR 0.9681, EV@10 209/209, fact coverage 149/149, forbidden
contamination 12.40% (15/121). By stage: retrieval-stage 0.93% (1/107);
answer-stage context presence (informational, never gated in Sprint 1)
14/14. PT: R@1 53/67, R@3 67/67. EN: R@1 60/68, R@3 68/68.

Per-critical-case ranks under the tuned configuration:

| Case | Expected meeting | Rank | R@1 |
|---|---|---|---|
| `fixture-whatsapp-retention` | `mtg-whatsapp-retention` | 1 | hit |
| `pt-ref-cobranca-regua` | `mtg-cobranca-vencidas` | 1 | hit |
| `pt-ref-chaves-acesso` | `mtg-pt-ref-chaves-acesso` | 2 | miss |
| `pt-ref-sla-suporte` | `mtg-pt-ref-sla-suporte` | 3 | **miss — amendment-4 escalation** |
| `pt-ref-nps-detrator` | `mtg-pt-ref-nps-detrator` | 2 | miss |

**Amendment-4 escalation (explicit user open item):** `pt-ref-sla-suporte`
remains below rank 1 (rank 3) at the final held-out-tuned constants. Its raw
vector channel ranks the target first (`1/3` meeting/evidence in the
concept-proxy table below); fusion/aggregation demotes it. This is reported as
an open item, not silently failed, and no constant or corpus text was moved to
fix it outside the held-out objective.

### 4.2 Quantization-cost reference: e5-base-int8 + mmarco-f32

Same locked fusion constants and γ=0; winning objective vector identical
`[0, 0, 0, 0, 0, 2166666]`. Every gate result matches the production
candidate's table except NDCG non-degradation margin (final 0.8929 vs fused
0.8546) and Critical Recall@1 also FAIL at 40.00% (2/5) with the same three
misses. Role: quantization-cost reference only — it cannot select, and its
near-identical quality confirms quint8 costs ~nothing (tune-partition NDCG@10
0.9046 quint8 vs 0.9049 f32; pairwise 78.81% (532/675) vs 79.26% (535/675)).

### 4.3 Title ablation (mandatory tuned-β vs β=0 pair; all other constants fixed)

| Metric | tuned β=1 | β=0 |
|---|---|---|
| Semantic R@3 | 100.00% (30/30) | 100.00% (30/30) |
| Reference category (15 cases) R@1 | 12/15 | 7/15 |
| Reference category R@3 | 15/15 | 14/15 |
| Reference category EV@10 / facts / forbidden | 29/29, 29/29, 14/16 | 29/29, 29/29, 14/16 |
| Reference-category MRR | 0.8889 | 0.7056 |
| Pinned WhatsApp case | rank 1, facts 2/2, forbidden 0/2 | rank 1, facts 2/2, forbidden 0/2 |
| Overall R@3 / R@5 / EV@10 | 135/135, 135/135, 209/209 | 134/135, 135/135, 209/209 |
| Overall MRR | 0.9681 | 0.9451 |

**Headline finding (title-dependent, per the 2026-08-23 rule):** removing title
scoring collapses reference-category R@1 from 12/15 to 7/15 and drops overall
MRR 0.9681 → 0.9451 while semantic R@3 holds at 100%. The embedding channel is
strong (semantic 30/30 even at β=0), but the reference-family performance that
carries the gates is materially title-assisted. The result must not be
attributed solely to the embedding model. Both candidates show the identical
pattern.

### 4.4 Raw bi-encoder vs supervised CONCEPT_LEXICON (production embedding e5-base-int8)

Summary: AGREE 35/120, DISAGREE 85 (proxy-positive/model-hit 35,
proxy-positive/model-miss 2, proxy-negative/model-hit 83, proxy-negative/
model-miss 0). The lexicon is a solvability proxy, not model evidence: it is
negative on 85 cases the model actually solves, and its two positive-prediction
misses are visible above. Critical/reference rows (meeting/evidence vector
ranks):

| Case | Concept margin | Vector rank mtg/ev | Verdict |
|---|---|---|---|
| fixture-whatsapp-retention | +0.000 | 1/5 | DISAGREE |
| pt-ref-cobranca-regua | +0.000 | 1/2 | DISAGREE |
| pt-ref-chaves-acesso | +0.250 | 4/10 | DISAGREE |
| pt-ref-sla-suporte | +0.500 | 1/5 | AGREE |
| pt-ref-nps-detrator | +0.750 | 1/13 | AGREE |

(The full 120-row table for both benchmark-leader embeddings is printed by the
canonical command; the second leader table, paraphrase-minilm, shows
chaves-acesso raw-vector meeting rank 3 — the terminological gap remains
partially bridgeable by a stronger/larger-context encoder, which is the Sprint
3 Task `3.6` territory, not a constant.)

Note how disagreement stays visible: `sla-suporte`'s positive concept margin
matches a raw-rank-1 vector hit, yet fused aggregation ranks it 3rd — the
failure is in fusion/aggregation weighting, exactly what the lexicographic
objective could not trade away without losing held-out quality elsewhere.

## 5. Feasibility corroboration (diagnostic probe, NOT tuning)

Probe = 360-point fusion grid × 6-point γ grid = 2160 configurations per
reranker, evaluated on the full corpus purely as existence evidence; the tuned
configuration remains exclusively the held-out objective's output.

| Candidate | Passing configurations | Tuned config inside passing set? | Objective distance (tuned minus best-passing, componentwise) |
|---|---|---|---|
| mmarco-quint8 | **78/2160** | **NO** | `[0, -2, 0, 0, -2, -2999998]` vs best-passing key `[0, 2, 0, 0, 2, 5166664]` at `k=20 wv=0.5 wl=0.5 α=0.5 β=0.5 γ=8` |
| mmarco-f32 | **79/2160** | **NO** | `[0, -1, 0, 0, -1, -2249998]` vs best-passing key `[0, 1, 0, 0, 1, 4416664]` at `k=10 wv=0.5 wl=0.5 α=0.5 β=0.5 γ=8` |

Reading: the tuned configuration is strictly better on the held-out objective
(negative deltas mean the tuned config pays fewer semantic/overall misses and
less MRR deficit than ANY gate-passing configuration). Conversely, every gate-
passing configuration sacrifices held-out measurable quality. After amendment 5
widened the partition and added the gap-type retrieval term, the passing region
still is not reachable from generalizable signal. Per the binding stop rule,
this ends instrument iteration: the remaining decision is the user's.

## 6. Clean-hardware latency re-probe (release build)

Machine state at run time (2026-08-24 ~16:15 local, immediately before the
canonical release run): Intel Core Ultra 7 255HX (20 logical CPUs), 31.4 GiB
RAM, 10.0 GiB free, Windows x64, ORT CPU-only, intra-op 4. Instantaneous CPU
load sampled 5×3 s before launch: 32%, 36%, 27%, 32%, 37% — ordinary desktop
background (ms-teams, OneDrive sync, Wispr Flow, Edge webviews, Task Manager)
was active; the machine was NOT perfectly idle and this is recorded honestly.
No build/index jobs were running during measurement.

Command: the canonical `cargo test --release … hybrid_corpus_and_resource_
benchmark -- --nocapture` above (fresh release build; deterministic batch-1
policy, 50-pair head, warm-up excluded from timing windows).

Measured solo per-pair latency (250 timed pairs = 50 docs × 5 reps):

| Candidate | load | solo p50 | solo p95 | session RAM delta | Depth cost @50 | 900 ms verdict |
|---|---|---|---|---|---|---|
| mmarco-quint8 | 1146 ms | 10.8 ms | **14.4 ms** | +306.3 MiB | 720 ms ≤ 900 ms | **PASS** |
| mmarco-f32 | 2314 ms | 11.1 ms | **16.5 ms** | +637.1 MiB | 825 ms ≤ 900 ms | **PASS** |

Derived policy: chat depth 50 (=RERANK_SET ceiling), search depth 25, batch 1.
This supersedes all earlier stale latency figures; neither candidate is
budget-excluded in this run.

## 7. Resource envelope evidence

- **Pair RAM (isolated process probe, this run):** projected 250k peak with
  int8 vectors = **1118.3 MiB** (embedding session +554.2 MiB, both sessions
  resident after inference +926.3 MiB over process base). Consistent with the
  recorded 1120.2 MiB. Sits in the **1–1.25 GiB band pre-approved for e5-base
  pairings (user decisions 2026-08-23/24)**; recorded against that approval, so
  a RAM-band result is not a blocker.
- **Admissibility pre-filter (retained arithmetic, unchanged):** 768-d int8
  vectors at 250k × 2 overlap = 384 MB; e5-base-int8 + mmarco-quint8 estimate
  band `approval-required` (≤1.25 GiB), approved as above; 768-d f32 remains
  inadmissible (>1.25 GiB hard fail).
- **Derived disk (measured this run):** 78 B content/doc +384 B int8 vector
  +96 B overhead = 558 B/doc → projected 250k steady **0.13 GiB** (envelope
  2 GiB), shadow-rebuild peak ×2 **0.26 GiB** (envelope 3 GiB). PASS.
- **Quantized vector storage recall cost:** vector-channel Evidence Recall@5
  (sampled every 3rd case) f32 = fp16 = int8 = 86.96% (60/69) — zero measured
  cost, consistent with the retained first-run finding.
- **Embedding export fidelity:** e5-small dynamic-int8 vs f32 sessions (only
  family with a staged f32 counterpart): mean cosine agreement 0.9894, min
  0.9825; query embed 5.6 ms (int8) vs 8.0 ms (f32). The e5-base leader has no
  staged f32 session; its fidelity is not separately claimed (as recorded in
  the manifest).

## 8. Chunk profile and summary policy (measured this run)

- Window profiles 256/48, 384/64, 512/96: identical fused meeting R@3
  97.04% (131/135) and EV@10 100.00% (209/209), docs=1100 each. The profiles
  remain indistinguishable on this corpus; 384/64 stays the default-profile
  basis (largest context within the leader's 512 limit with workable doc
  count). Any final chunk-policy promotion inherits this evidence as-is.
- latest-summary-only vs all-labeled-summary-templates: identical aggregate
  metrics (R@3 131/135, EV@10 209/209, forbidden 15/121). Single-template
  fixtures still cannot separate them; the distinction remains deferred until
  fixtures carry multi-template summaries.

## 9. Model provenance, licenses, hashes (immutable manifest pins)

Production candidate bundle (unchanged from the pinned manifest):

| Field | Embedding | Reranker |
|---|---|---|
| Model ID | `intfloat/multilingual-e5-base` | `cross-encoder/mmarco-mMiniLMv2-L12-H384-v1` |
| Revision | `d128750597153bb5987e10b1c3493a34e5a4502a` | `1427fd652930e4ba29e8149678df786c240d8825` |
| ONNX export revision | `1ec9243030a27d1a115d5c340572074c125b58b2` (portable mirror dynamic-int8) | quint8_avx2 + f32 exports, pinned immutable revision |
| License | MIT | apache-2.0 |
| Dimensions / max seq | 768 / 512 | 512 pair encoding, `<s>q</s></s>d</s>`, LongestFirst |
| Prefixes / pooling / norm | query `query: `, doc `passage: `; mean pooling; L2 | logits[.,0], sigmoid transform, label index 0 |
| Staged artifact hash | `model_int8.onnx` sha256-`9ddfd8b45086dabc59a7e1bb00463225dace8954962418b240840f2153bc87da` (verified against staged file this run) | quint8/f32 files verified byte-stable from prior pinned staging (sizes 118,620,016 / 470,883,696 B) |

Card metadata multilingual-conforming: yes for both halves (mMARCO PT training
data; PT+EN product). Rejected-before-benchmark and documented-unavailable
candidates (jina v2/v3 unclear redistribution, gte base no license field,
bge-large dominated, L6 variant unlicensed community ONNX only, mxbai CC-BY-NC,
gte-reranker CC-BY-NC, MiniLM-L6-v2 English-only family) are retained verbatim
in `model_bundle_manifest.json` and are not re-derived here. `BAAI/
bge-reranker-base` is permanently retired (zh/en card nonconformity; 2026-08-23
user decision) and was NOT executed in this run.

## 10. Verification performed

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo check --manifest-path "frontend/src-tauri/Cargo.toml" --tests     # pass
cargo test  --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark        # 11 passed (incl. 2 new amendment-5 regression tests)
cargo test  --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation   # 6 passed
cargo test  --manifest-path "frontend/src-tauri/Cargo.toml" --lib       # 394 passed; 2 ignored
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"             # pass
cargo fmt   --manifest-path "frontend/src-tauri/Cargo.toml" --check     # pass
git diff --check                                                        # pass
# canonical release benchmark + pair_ram_probe: pass (sections 4-7; log retained)
```

Privacy scan of the modified harness: no secrets, keys, user paths, or private
transcript text introduced; `git ls-files` contains no model-weight artifacts
(`.onnx`/`.bin`/`.gguf`). Frontend typecheck/Vitest not affected (no frontend
files touched).

## 11. Open items for the user (decision required)

1. **Primary (binding stop rule).** Critical Recall@1 at the earned constants
   is 2/5 while 78–79/2160 configurations pass all three joint conditions and
   the tuned configuration is provably outside that passing set at better
   held-out quality. Choose: (a) split the critical gate from the general
   gate set, or (b) grant a dated exception against Sprint 3 Task `3.6`
   (single-turn query expansion) with this evidence attached. Selecting
   constants by inspecting critical results remains forbidden and was not done.
2. **Amendment-4 escalation.** `pt-ref-sla-suporte` stays below rank 1 (rank 3
   fused vs rank 1 raw-vector). It is an explicit open item, not a silent gate
   failure.
3. **Title dependence.** Reference-family gate performance is title-assisted
   (β=1 earns 12/15 vs 7/15 at β=0). If selection proceeds later, record this
   dependence in any approved addendum.
4. Non-blocking register items inherited from `1.3G` (constants disclosure now
   implemented; probe-count comparability; carrierless-fact dilution) stand as
   recorded there.

## 12. Rollback

Restore `frontend/src-tauri/tests/model_benchmark.rs` from commit `6bba48b`
(`git checkout 6bba48b -- frontend/src-tauri/tests/model_benchmark.rs`) and
delete this report file. Production code, persisted data, corpus, gates, and
manifest are untouched by this task; staged model artifacts remain outside Git.

---

## FINAL RESULT (exactly one)

**Documented stop — branch 2.** The final held-out retune under amendment 5
earns `k=5, w_vector=1, w_lexical=0.5, alpha=0.5, beta=1, gamma(quint8)=0,
support_cap=3, depth=50, batch=1` for the production candidate
**e5-base-int8 + mmarco-quint8** (vector encoding int8-storage; measured
1118.3 MiB projected peak inside the pre-approved band), with winning held-out
objective vector `[0, 0, 0, 0, 0, 2166666]`. That configuration passes every
Sprint 1 gate except **Critical Recall@1 = 2/5** (misses:
`pt-ref-chaves-acesso` rank 2, `pt-ref-sla-suporte` rank 3 — amendment-4 open
item, `pt-ref-nps-detrator` rank 2), while the feasibility probe measures
78/2160 passing configurations and proves the tuned configuration lies outside
the passing set at strictly better held-out objective value (componentwise
distance `[0,-2,0,0,-2,-2999998]` to the best passing configuration). The
passing region is therefore not reachable from generalizable signal on this
instrument. Per the binding amendment, no further partition/objective/gate
iteration occurs and nothing is selected by inspection: **no production pair
is approved by this run; the gate-split versus dated-exception decision
belongs to the user.**
