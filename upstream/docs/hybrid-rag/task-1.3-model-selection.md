# Task 1.3 — Embedding, Reranker, and Chunk Selection Benchmark Report (2026-08-23 rerun)

| Field | Value |
|---|---|
| Status | **Blocked** — three budget-viable pairs fully evaluated on the card-multilingual-conforming `e5-base-int8` embedding family, and **every evaluated pair fails Critical Recall@1 plus Critical forbidden contamination (4/6)**; `e5-base-int8+bge-reranker-base-int8` additionally fails Reference Recall@1 (rank 2) and is metadata-nonconforming. Citation/source precision is UNEVALUATED by this simulation for all candidates. No production pair is selected; `benchmarkLeader` records non-production leaders only. |
| Date | 2026-08-23 (rerun against the approved Task 1.2R corpus; first-run quality findings voided 2026-08-22). Canonical release run recorded in the manifest and reproduced same-day with digit-identical quality tables. |
| Owner | fresh `worker-l` session (`opencode-go/ox-alpha-free`, no substitution) |
| Harness | `frontend/src-tauri/tests/model_benchmark.rs`, manifest `frontend/src-tauri/tests/fixtures/model_bundle_manifest.json` |
| Reference hardware | Windows x64, Intel Core Ultra 7 255HX (20 cores / 20 logical CPUs), 31.4 GiB RAM, ONNX Runtime CPU execution provider |
| Method | Pinned artifact hashes verified (SHA-256 match on the staged ONNX files, including the leader pins `9ddfd8b4…` e5-base int8 and `2059d8ef…` bge int8) before any inference. All quality metrics measured by actual tokenizer + embedding + reranker inference against the fixed 1.2R corpus. Tuning uses only the held-out partition (non-critical, non-reference; reference/critical never inspected). Every percentage carries its denominator. The canonical `--release` run's quality tables are deterministic and were confirmed by an independent same-day run: family metrics, per-pair full-corpus tables, critical ranks, gate verdicts, title ablations, concept-proxy table, fusion objective, and quantization/disk figures are digit-identical; latency/RAM probes vary with machine state as recorded in §9/§10. |

## 1. Rerun scope: retained vs re-measured

**Retained from the first run (not re-derived):** admissibility arithmetic and the full candidate estimate table (§3), isolated-process pair RAM peaks including the retained `e5-base-int8+bge-reranker-base-int8` figure of 1271.9 MiB (§9), derived disk per document, quantization-fidelity method, and license/ONNX-availability/portability screening including the documented-unavailable and rejected-before-benchmark lists (unchanged model metadata; see the manifest `biEncoderCandidates`/`crossEncoderCandidates`).

**Voided and re-measured here:** all quality metrics and category/PT/EN breakdowns for every candidate, both gate verdict tables, the locked fusion constants, all tuned gamma values, the meeting-aggregation constants, the chunk-profile conclusion, and the reranker selection output — now covering **all three budget-viable rerankers**, not just the previously viable subset.

**Coherence checks performed on retained evidence:** staged artifacts re-hashed against manifest pins (all match); FTS baseline re-derived in-run matches the approved record — semantic Recall@3 `0.00% (0/30)`, exact Recall@3 `100.00% (90/90)`; in-run e5-base embedding-session RSS delta ~552 MiB vs the isolated probe's +553.3 MiB retained; `pair_ram_probe` for `e5-base-int8+mmarco-quint8` reproduced the retained `1120.2 MiB` projected 250k peak exactly; derived disk 558 B/doc vs 555 B/doc retained; e5-small int8-vs-f32 session cosine agreement 0.9893 (min 0.9821) vs 0.9919 (min 0.9871) retained (the e5-base export has no staged f32 counterpart, so its fidelity is recorded as not separately measured).

## 2. Harness corrections required by the rerun (reviewer decisions inside scope)

Three defect classes were fixed inside `model_benchmark.rs` (plus manifest data repair); none alters metric definitions, gate thresholds, evaluation policy, or corpus content.

1. **Meeting-scope lexical channel pinned the focus meeting.** The Task 1.2 defect class — `ScopeKind::Meeting` routed through the single-meeting FTS filter — had been corrected in the Task 1.2R baseline harness but not in the benchmark harness's own copy of the lexical channel. Left unfixed it produced a benchmark baseline of semantic Recall@3 43.33% (13/30), contradicting the user-approved 0/30 record, while inflating hybrid Meeting-scope metrics through the same pin. The channel now ranks AND→OR restricted to `scope.allowed_meeting_ids` (fetch x4, filter to the allow-list), mirroring `retrieval_evaluation.rs`; production `FtsRepository` calls are unchanged, and a fixture-schema assertion checks that a focused meeting sits inside its permitted set.
2. **Pair labels hardcoded `e5-small-int8`.** The contracted family is corpus-quality-dependent; on this corpus it is `e5-base-int8`, so pair keys/labels are derived from the contracted family, and the Batch-4 coherence test pins `benchmarkLeader` to the actual rerun leader identities (e5-base embedding; mmarco-mMiniLMv2-L12 reranker) together with artifact/hash equality against the candidate inventory.
3. **Batch 4 remediation — deterministic pair-order/recording contract.** The reference-expectation replay scored whatever query/evidence strings were stored in the manifest, while recording scored the harness's own strings. A PowerShell serialization pass had rewritten the manifest with triple-mangled UTF-8→CP1252 text in every non-ASCII reference string (e.g. `régua` → `rÃƒÆ'Ã‚Â©gua`) while leaving the recorded scores untouched — so replay silently re-scored garbage text and bge pair 0 drifted `actual=0.49032903` vs `recorded=0.9959985017776488`. Fix: the ordered five-pair list per reranker group is now single-sourced in code (`reference_rerank_pairs()`) and used by BOTH recording and replay, so batch composition (which dynamic-int8 activation scales depend on) is identical by construction; replay additionally asserts the manifest's stored texts byte-equal the code contract, turning any future encoding damage into a loud text-contract failure instead of a score drift. The corrupted manifest strings were repaired to exactly their pre-corruption values (verified reversible and cross-checked against checkpoint `1e41b6b`), scores unchanged, and the whole file re-serialized as readable repository JSON. Tolerances (`rerankerScoreAbs` 0.20 etc.) and assertions were not widened or weakened — the replay text-equality check is new and strictly stronger.

## 3. Admissibility pre-filter (RETAINED, 2026-08-22)

Formula: `dims x bytes_per_value x 250000 x 2 + embedding_session_artifact_bytes + reranker_session_artifact_bytes`. Envelope: <=1,073,741,824 B auto-pass; <=1,342,177,280 B approval band; above -> inadmissible, not benchmarked. These are ESTIMATES; §9 records measured simultaneous-session peaks. Full table in manifest `pairAdmissibilityEstimates`. Key rows:

| Pair | Total bytes | Band |
|---|---|---|
| e5-small [int8|int8] + bge-base [int8] | 588,879,901 | admissible |
| e5-base [int8|int8] + bge-base [int8] | 941,009,470 | admissible |
| e5-base [int8|int8] + mmarco [quint8_avx2] | 780,804,178 | admissible |
| para-MiniLM [int8|int8] + bge-base [int8] | 588,879,917 | admissible (max-seq non-conforming) |
| e5-small [f32|int8] + bge-base [int8] | 1,164,879,901 | approval-required |
| any + bge-base [f32] | 1,422,514,181 | **inadmissible** |
| e5-small + bge-reranker-large [f32] | 2,550,238,893 | **inadmissible** |

Rejected before benchmark (unchanged screening): bge-m3, LaBSE, mpnet-multilingual, distiluse-v2 (both portable exports lack the Dense(768->512)+normalize head — probe-verified), jina v2/v3 (CC-BY-NC), gte-multilingual (trust_remote_code); `nreimers/mMiniLMv2-L6-H384` ships no ONNX and no license field; no official L6 mmarco cross-encoder exists.

## 4. Candidate coverage on the 1.2R corpus (quality re-measured)

### Bi-encoder families (actual inference, full corpus, fused vector+lexical, no rerank)

| Family | Export | R@1 | R@3 | Evidence R@10 | MRR | Contract |
|---|---|---|---|---|---|---|
| e5-small (`intfloat/multilingual-e5-small` @614241f6, MIT; Xenova @761b726d) | dynamic int8, 384-d | 77.78% (105/135) | 95.56% (129/135) | 99.04% (207/209) | 0.9160 | conforming (seq 512) |
| **e5-base** (`intfloat/multilingual-e5-base` @d1287506, MIT; Xenova @1ec92430) | dynamic int8, 768-d | 79.26% (107/135) | **97.04% (131/135)** | 100.00% (209/209) | 0.9278 | conforming (seq 512); contracted |
| paraphrase-MiniLM (@e8f8c211, Apache-2.0) | dynamic int8, 384-d | 80.00% (108/135) | 96.30% (130/135) | 100.00% (209/209) | 0.9326 | **non-conforming**: max-seq 128 cannot honor required window profiles |

Unlike the void corpus, families no longer produce byte-identical metrics: the repaired corpus discriminates models, and the unique best family is **e5-base-int8**, which becomes the contracted harness family (the smallest-footprint tie-break no longer selects e5-small). All three remain NON-selected while the pair decision is blocked.

### Cross-encoders under the deterministic runtime policy (batch=1, depth=50)

Solo tail latency varied ~1.6–2.4x across same-day runs with machine state. Four runs are on record: two earlier elevated-latency runs, the canonical quiet-machine release run recorded in the manifest, and an independent same-day reproduction (§11 commands). Viability per candidate:

| Candidate | Solo p95 across runs | Depth-50 cost | Verdict |
|---|---|---|---|
| mmarco-mMiniLMv2-L12 quint8_avx2 (Apache-2.0; official ONNX; pt-trained -> conforming) | 13.4 / 17.2 ms elevated; **8.0 ms canonical** (repro 7.2 ms) | 400 ms canonical (repro 360 ms) | **viable in every run** — the only candidate that never hits the sub-budget wall. Held-out pairwise 85.00% (459/540), NDCG@10 0.9184 vs fused 0.8669 at tuned gamma 8 |
| bge-reranker-base int8 (MIT; card zh/en -> metadata NON-conforming) | 18.5 / 23.9 ms elevated (excluded at 926/1194 ms); **12.9 ms canonical** (repro 12.0 ms) | 645 ms canonical (repro 600 ms) | budget-excluded in the two elevated runs; **fully quality-evaluated in the canonical release run and repro** (§6/§10). Card zh/en metadata non-conformity stands regardless of quality |
| mmarco f32 | 20.5 / 22.5 ms elevated (excluded); **11.8 ms canonical** (repro 12.4 ms) | 590 ms canonical | budget-excluded in elevated runs; fully quality-evaluated in the canonical release run and repro. Held-out pairwise 85.19% (460/540), NDCG@10 0.9188 vs fused 0.8669 at gamma 0 (its own quantization-cost baseline) |
| bge-reranker-base fp16 | 92.9–133+ ms in every run | 4383–4646 ms | budget-excluded in every run |

The NDCG-leading candidate in the quiet-machine runs is bge-int8 (full-corpus NDCG 0.9009 vs 0.8926/0.8921); it is reported openly but cannot support selection: metadata-nonconforming AND failing gates (§10).

## 5. Held-out tuning: expanded title grid and locked constants

Partition unchanged (reference isolation intact): 105 non-critical, non-reference cases (120 expected-meeting entries); reference/critical never inspected.

Grid: k in {5,10,20,60} x w_vec in {0.5,1,2} x w_lex in {0.5,1,2} x alpha in {0,0.5} x **beta in {0, 0.25, 0.5, 1, 2}** = **360 fusion configurations**, then per-candidate gamma in {0,0.5,1,2,4,8}. Objective (lexicographic ascending): exact-term violations -> semantic R@3 misses -> overall R@3 misses -> MRR misses (micros); deterministic tie-break toward smaller constants.

Locked fusion for every evaluated pair: **k=5, w_vector=1, w_lexical=0.5, alpha=0.5, beta=0.25** (objective `[exact-viol 0, sem-miss 0, all-miss 0, mrr-miss 2166666]` over 360 configs). Tuned gamma: mmarco-quint8 **8** (objective `[0,0,0,0,2000000]`), bge-int8 **8** (`[0,0,0,0,500000]`), mmarco-f32 **0** (`[0,0,0,0,2166666]`). Shared runtime policy: reranker depth chat/search 50/25, batch 1, ORT intra-op 4.

## 6. Full-corpus quality results (canonical `--release` run; contracted e5-base-int8 at locked constants)

FTS baseline re-verified in-run on the same corpus: semantic Recall@3 `0.00% (0/30)`; exact Recall@3 `100.00% (90/90)` — matches the approved 1.2R record.

### Pair A — e5-base-int8 + bge-reranker-base-int8 (gamma 8; card-multilingual NON-conforming)

| Metric | Observed (denominators) |
|---|---|
| Meeting Recall@1 / @3 / @5 | 85.19% (115/135) / 100.00% (135/135) / 100.00% (135/135) |
| MRR | 0.9764 |
| Evidence Recall@10 | 100.00% (209/209) |
| Required-fact coverage | 100.00% (149/149) |
| Forbidden-fact contamination | 24.79% (30/121) |
| Portuguese / English | pt R@1 82.09% (55/67), R@3/R@5 67/67 · en R@1 88.24% (60/68), R@3/R@5 68/68 |
| Exact-term category | Recall@3 100.00% (90/90) — no regression |
| Semantic category | Recall@3 100.00% (30/30) vs baseline 0/30 |
| Critical Recall@1 | **40.00% (2/5)** — FAIL |
| Critical required facts / forbidden | facts 100.00% (9/9) PASS · forbidden **66.67% (4/6)** FAIL |
| Pairwise reranker accuracy | full corpus 74.02% (604/816); held-out 80.93% (437/540) |
| NDCG@10 final vs fused-order mean | 0.9009 vs 0.8382 — no degradation |

Per-critical-case meeting ranks (pair A): `fixture-whatsapp-retention` **rank 2**, `pt-ref-cobranca-regua` 1, `pt-ref-chaves-acesso` 3, `pt-ref-sla-suporte` 2, `pt-ref-nps-detrator` 1. The bge reranker lifts the echo-heavy neighbouring meeting over the pinned reference case (rank 2) — its recorded reference-probe weakness operating at corpus scale.

### Pair B — e5-base-int8 + mmarco-f32 (gamma 0; card-multilingual conforming)

| Metric | Observed (denominators) |
|---|---|
| Meeting Recall@1 / @3 / @5 | 82.22% (111/135) / 100.00% (135/135) / 100.00% (135/135) |
| MRR | 0.9569 |
| Evidence Recall@10 | 100.00% (209/209) |
| Required-fact coverage | 100.00% (149/149) |
| Forbidden-fact contamination | 24.79% (30/121) |
| Portuguese / English | pt R@1 76.12% (51/67), R@3/R@5 67/67 · en R@1 88.24% (60/68), R@3/R@5 68/68 |
| Exact-term category | Recall@3 100.00% (90/90) — no regression |
| Semantic category | Recall@3 100.00% (30/30) vs baseline 0/30 |
| Critical Recall@1 | **60.00% (3/5)** — FAIL (best of the three) |
| Critical required facts / forbidden | facts 100.00% (9/9) PASS · forbidden **66.67% (4/6)** FAIL |
| Pairwise reranker accuracy | full corpus 74.02% (604/816); held-out 85.19% (460/540) |
| NDCG@10 final vs fused-order mean | 0.8926 vs 0.8382 — no degradation |

Per-critical-case meeting ranks (pair B): `fixture-whatsapp-retention` 1, `pt-ref-cobranca-regua` 1, `pt-ref-chaves-acesso` 3, `pt-ref-sla-suporte` 3, `pt-ref-nps-detrator` 1.

### Pair C — e5-base-int8 + mmarco-quint8 (gamma 8; card-multilingual conforming; measured RAM leader pairing)

| Metric | Observed (denominators) |
|---|---|
| Meeting Recall@1 / @3 / @5 | 80.74% (109/135) / 99.26% (134/135) / 100.00% (135/135) |
| MRR | 0.9479 |
| Evidence Recall@10 | 100.00% (209/209) |
| Required-fact coverage | 100.00% (149/149) |
| Forbidden-fact contamination | 24.79% (30/121) |
| Portuguese / English | pt R@1 74.63% (50/67), R@3 98.51% (66/67), R@5 67/67 · en R@1 86.76% (59/68), R@3/R@5 68/68 |
| Exact-term category | Recall@3 100.00% (90/90) — no regression |
| Semantic category | Recall@3 100.00% (30/30) vs baseline 0/30 |
| Critical Recall@1 | **40.00% (2/5)** — FAIL |
| Critical required facts / forbidden | facts 100.00% (9/9) PASS · forbidden **66.67% (4/6)** FAIL |
| Pairwise reranker accuracy | full corpus 73.53% (600/816); held-out 85.00% (459/540) |
| NDCG@10 final vs fused-order mean | 0.8921 vs 0.8382 — no degradation |

Per-critical-case meeting ranks (pair C): `fixture-whatsapp-retention` 1, `pt-ref-cobranca-regua` 1, `pt-ref-chaves-acesso` 4, `pt-ref-sla-suporte` 3, `pt-ref-nps-detrator` 2. The three missing rank-1s are exactly the corpus's terminological-gap/stale-version/cross-section cases (`chaves-acesso` also has the worst raw vector rank, §8). Reference case under pairs B/C: rank 1, facts 2/2, forbidden 0/2.

### Chunk and summary policy evidence (re-measured on this corpus)

- Window profiles 256/48, 384/64, 512/96: each produced docs=1100 and identical fused meeting Recall@3 `97.04% (131/135)` and Evidence Recall@10 `100.00% (209/209)` (no-rerank fusion, contracted family). Profile choice remains cost-driven -> **384/64**.
- Summary policies: latest-summary-only equals all-labeled-summary-templates on single-template fixtures (identical metrics, forbidden included). Sprint 2 must re-evaluate against real multi-template summaries.
- Vector storage quantization cost (sampled every 3rd case): f32 = fp16 = int8 = `86.96% (60/69)` Evidence Recall@5 — zero measured recall cost.
- Derived disk: measured 78 B content/doc + 384 B vector(int8) + 96 B overhead = 558 B/doc -> 0.13 GiB steady / 0.26 GiB shadow-rebuild peak at 250k (envelopes 2 GiB / 3 GiB).

## 7. Mandatory title ablation (tuned beta=0.25 vs beta alone ablated to 0)

Full-corpus pass per setting; every other tuned constant fixed (k=5, w_vec=1, w_lex=0.5, alpha=0.5, gamma per pair, depth=50). The full `reference_whatsapp` category (15 cases) is compared category-wide; the pinned acceptance case `fixture-whatsapp-retention` is kept as its own row. Denominators: category R@1/R@3/R@5 n/15, EV@10/facts n/29, forbidden n/16.

### Pair A — bge-reranker-base-int8

| Metric | tuned beta=0.25 | beta=0 |
|---|---|---|
| Semantic Recall@3 | 100.00% (30/30) | 100.00% (30/30) |
| Reference category R@1 / R@3 / R@5 | 73.33% (11/15) / 100.00% (15/15) / 100.00% (15/15) | 73.33% (11/15) / 100.00% (15/15) / 100.00% (15/15) |
| Reference category EV@10 / facts / forbidden | 29/29 / 29/29 / 14/16 | 29/29 / 29/29 / 14/16 |
| Reference category MRR | 0.8444 | 0.8556 |
| Pinned WhatsApp case | rank 2, facts 2/2, forbidden 0/2 | rank 2, facts 2/2, forbidden 0/2 |
| Overall R@3 / R@5 / EV@10 / MRR | 135/135 / 135/135 / 209/209 / 0.9764 | 135/135 / 135/135 / 209/209 / 0.9736 |

Headline: **title-independent** across semantic, pinned reference case, critical subset, and full 15-case reference category (only aggregate MRR moves −0.0028).

### Pair B — mmarco-f32

| Metric | tuned beta=0.25 | beta=0 |
|---|---|---|
| Semantic Recall@3 | 100.00% (30/30) | 100.00% (30/30) |
| Reference category R@1 / R@3 / R@5 | 66.67% (10/15) / 100.00% (15/15) / 100.00% (15/15) | **46.67% (7/15)** / **93.33% (14/15)** / 100.00% (15/15) |
| Reference category EV@10 / facts / forbidden | 29/29 / 29/29 / 14/16 | 29/29 / 29/29 / 14/16 |
| Reference category MRR | 0.8000 | 0.6944 |
| Pinned WhatsApp case | rank 1, facts 2/2, forbidden 0/2 | rank 1, facts 2/2, forbidden 0/2 |
| Overall R@3 / R@5 / EV@10 / MRR | 135/135 / 135/135 / 209/209 / 0.9569 | **134/135** / 135/135 / 209/209 / 0.9354 |

Headline: **TITLE-DEPENDENT** — removing title scoring degrades the reference category R@1 10/15 -> 7/15 and R@3 15/15 -> 14/15, drops overall R@3 to 134/135, and moves overall MRR −0.0215. This pair's result must NOT be attributed solely to the embedding model.

### Pair C — mmarco-quint8

| Metric | tuned beta=0.25 | beta=0 |
|---|---|---|
| Semantic Recall@3 | 100.00% (30/30) | 100.00% (30/30) |
| Reference category R@1 / R@3 / R@5 | 53.33% (8/15) / 93.33% (14/15) / 100.00% (15/15) | 53.33% (8/15) / 93.33% (14/15) / 100.00% (15/15) |
| Reference category EV@10 / facts / forbidden | 29/29 / 29/29 / 14/16 | 29/29 / 29/29 / 14/16 |
| Reference category MRR | 0.7167 | 0.7167 |
| Pinned WhatsApp case | rank 1, facts 2/2, forbidden 0/2 | rank 1, facts 2/2, forbidden 0/2 |
| Overall R@3 / R@5 / EV@10 / MRR | 134/135 / 135/135 / 209/209 / 0.9479 | 134/135 / 135/135 / 209/209 / 0.9382 |

Headline: **title-independent** across semantic, pinned reference case, critical subset, and full 15-case reference category (only aggregate MRR moves −0.0097). The expanded grid still selected beta=0.25 as optimal on the held-out objective, so title input earns a small weight without carrying any gate result for this pair.

## 8. Concept-proxy disagreement evidence (supervised CONCEPT_LEXICON vs raw bi-encoder rank)

No production bi-encoder can be selected while the pair decision is blocked, so the per-case comparison is reported for the benchmark leader and labeled NON-selected. On this corpus all three leader roles coincide in one family — `e5-base-int8` is simultaneously the overall quality leader (best Recall@3), the best metadata-conforming family, and the contracted harness family — so one table covers the required minimum.

**Method.** Per case: the supervised proxy prediction is positive when the CONCEPT_LEXICON concept-channel margin over the strongest distractor is > 0 (same inverse-candidate-frequency arithmetic as the Task 1.2R margin check; expected IDs only label targets). Raw vector behavior is the expected meeting's best document rank under pure cosine ordering before any fusion (identical scoring to the pipeline's vector channel). Verdict is AGREE when both signals agree the target is retrievable (proxy-positive AND raw rank <= 3) or both say it is not.

**Summary for e5-base-int8 (NON-selected):** AGREE 35/120, DISAGREE 85 — proxy-positive/model-hit 35, **proxy-positive/model-miss 2**, **proxy-negative/model-hit 83**, proxy-negative/model-miss 0.

**Reading.** The lexicon proxy and the embedding model disagree far more than they agree, and the disagreement mass runs in the model's favor: the bi-encoder places the expected meeting inside raw top-3 for 118/120 cases (the exceptions are `pt-ref-chaves-acesso` at rank 5 and `pt-semantic-paraphrase-019` at rank 4), while the hand-built concept lexicon fails to predict success on 83 cases the embeddings solve outright. Zero cases are lost by both signals. The proxy remains a corpus-solvability diagnostic, not model evidence; this table keeps that disagreement visible instead of hiding it behind fusion/title scores. The two proxy-positive misses mark the corpus's genuinely hard cases (terminological gap; weak-paraphrase) and coincide with the critical-rank failures above.

| Case | Language | Concept margin | Vector rank mtg/ev | Verdict |
|---|---|---|---|---|
| fixture-whatsapp-retention | pt | +0.000 | 1/4 | DISAGREE |
| pt-ref-cobranca-regua | pt | +0.000 | 1/2 | DISAGREE |
| pt-ref-chaves-acesso | pt | +0.250 | 5/9 | DISAGREE |
| pt-ref-sla-suporte | pt | +0.500 | 2/6 | AGREE |
| pt-ref-nps-detrator | pt | +0.750 | 1/13 | AGREE |
| pt-ref-onboarding-primeiros-dias | pt | -1.000 | 1/3 | DISAGREE |
| pt-ref-renovacao-aprovacao | pt | -1.000 | 1/3 | DISAGREE |
| pt-ref-reativacao-inativos | pt | -1.000 | 1/7 | DISAGREE |
| pt-ref-trilhas-capacitacao | pt | +0.000 | 1/2 | DISAGREE |
| pt-ref-melhorias-produto | pt | +0.000 | 3/3 | DISAGREE |
| pt-ref-suporte-revendas | pt | -0.500 | 3/7 | DISAGREE |
| pt-ref-reembolso-limite | pt | -0.667 | 1/3 | DISAGREE |
| pt-ref-auxilio-remoto | pt | +0.000 | 1/4 | DISAGREE |
| pt-ref-licencas-disponiveis | pt | +0.000 | 1/4 | DISAGREE |
| pt-ref-revisao-logs | pt | +0.000 | 1/3 | DISAGREE |
| pt-semantic-paraphrase-031 | pt | +1.000 | 1/1 | AGREE |
| pt-semantic-paraphrase-016 | pt | +0.333 | 1/2 | AGREE |
| pt-semantic-paraphrase-017 | pt | -0.333 | 1/2 | DISAGREE |
| pt-semantic-paraphrase-018 | pt | +0.333 | 1/1 | AGREE |
| pt-semantic-paraphrase-019 | pt | +0.500 | 4/4 | DISAGREE |
| pt-semantic-paraphrase-020 | pt | -0.333 | 1/1 | DISAGREE |
| pt-semantic-paraphrase-021 | pt | +1.667 | 1/2 | AGREE |
| pt-semantic-paraphrase-022 | pt | -0.333 | 1/2 | DISAGREE |
| pt-semantic-paraphrase-023 | pt | +0.167 | 1/1 | AGREE |
| pt-semantic-paraphrase-024 | pt | +0.667 | 1/1 | AGREE |
| pt-semantic-paraphrase-025 | pt | -0.333 | 1/2 | DISAGREE |
| pt-semantic-paraphrase-026 | pt | +0.333 | 2/2 | AGREE |
| pt-semantic-paraphrase-027 | pt | -0.333 | 1/2 | DISAGREE |
| pt-semantic-paraphrase-028 | pt | +0.667 | 2/2 | AGREE |
| pt-semantic-paraphrase-029 | pt | +0.667 | 2/2 | AGREE |
| en-semantic-paraphrase-001 | en | -0.333 | 1/2 | DISAGREE |
| en-semantic-paraphrase-002 | en | +0.333 | 1/1 | AGREE |
| en-semantic-paraphrase-003 | en | -0.333 | 1/2 | DISAGREE |
| en-semantic-paraphrase-004 | en | +0.667 | 1/1 | AGREE |
| en-semantic-paraphrase-005 | en | +0.500 | 1/2 | AGREE |
| en-semantic-paraphrase-006 | en | +0.833 | 1/1 | AGREE |
| en-semantic-paraphrase-007 | en | +1.667 | 1/1 | AGREE |
| en-semantic-paraphrase-008 | en | -0.667 | 1/2 | DISAGREE |
| en-semantic-paraphrase-009 | en | -0.333 | 1/2 | DISAGREE |
| en-semantic-paraphrase-010 | en | -0.333 | 1/2 | DISAGREE |
| en-semantic-paraphrase-011 | en | -0.667 | 1/2 | DISAGREE |
| en-semantic-paraphrase-012 | en | +0.667 | 1/1 | AGREE |
| en-semantic-paraphrase-013 | en | -0.333 | 1/2 | DISAGREE |
| en-semantic-paraphrase-014 | en | +0.667 | 1/1 | AGREE |
| en-semantic-paraphrase-015 | en | -0.667 | 1/2 | DISAGREE |
| pt-followup-parcela-orcamento | pt | +0.500 | 1/2 | AGREE |
| pt-followup-edital-concurso | pt | +0.500 | 1/2 | AGREE |
| pt-followup-parecer-auditoria | pt | +0.000 | 1/2 | DISAGREE |
| pt-followup-vagas-estagio | pt | +1.000 | 1/2 | AGREE |
| pt-followup-comprovante-prazo | pt | +0.000 | 1/2 | DISAGREE |
| pt-followup-visita-planta | pt | +0.000 | 1/2 | DISAGREE |
| pt-followup-salas-congresso | pt | +0.000 | 1/1 | DISAGREE |
| en-followup-community-budget | en | +1.000 | 1/2 | AGREE |
| en-followup-workshop-registration | en | +0.000 | 1/2 | DISAGREE |
| en-followup-audit-partner | en | +1.000 | 1/2 | AGREE |
| en-followup-license-count | en | +0.000 | 1/2 | DISAGREE |
| en-followup-expense-cutoff | en | +1.000 | 1/1 | AGREE |
| en-followup-vendor-owner | en | +2.000 | 1/2 | AGREE |
| en-followup-annex-desks | en | +0.000 | 1/2 | DISAGREE |
| en-followup-staging-build | en | +0.000 | 1/1 | DISAGREE |
| pt-multi-aurora-boreal | pt | +1.000 | 1/1 | AGREE |
| en-multi-northwind-ironwood | en | +0.500 | 1/2 | AGREE |
| pt-multi-vesper-zenite | pt | +0.000 | 1/1 | DISAGREE |
| en-multi-bluefin-kestrel | en | +0.000 | 1/2 | DISAGREE |
| pt-multi-coral-sargaco | pt | +0.000 | 1/3 | DISAGREE |
| en-multi-larkspur-basalt | en | +0.000 | 1/3 | DISAGREE |
| pt-multi-jacaranda-guavira | pt | +0.000 | 1/1 | DISAGREE |
| en-multi-quartz-meadow | en | +0.000 | 1/1 | DISAGREE |
| pt-multi-onca-pintada | pt | +0.000 | 1/1 | DISAGREE |
| en-multi-harbor-lantern | en | +0.000 | 1/2 | DISAGREE |
| pt-multi-ipueira-taboca | pt | +0.000 | 1/1 | DISAGREE |
| en-multi-summit-valley | en | +0.000 | 1/1 | DISAGREE |
| pt-multi-cerrado-mangue | pt | +0.000 | 1/1 | DISAGREE |
| en-multi-fjord-dune | en | +0.000 | 1/2 | DISAGREE |
| en-multi-opal-topaz | en | +0.000 | 1/2 | DISAGREE |
| en-deleted-invoice-threshold | en | +0.000 | 1/1 | DISAGREE |
| en-deleted-onboarding-guide | en | +0.000 | 1/2 | DISAGREE |
| en-deleted-server-window | en | +1.000 | 1/1 | AGREE |
| en-deleted-refund-policy-code | en | +1.000 | 1/1 | AGREE |
| en-deleted-badge-colors | en | +0.000 | 1/1 | DISAGREE |
| en-deleted-quota-regional | en | +0.000 | 1/1 | DISAGREE |
| en-deleted-training-room-cap | en | +0.000 | 1/1 | DISAGREE |
| en-deleted-api-version-date | en | +0.000 | 1/2 | DISAGREE |
| en-deleted-expense-per-diem | en | +0.000 | 1/1 | DISAGREE |
| en-deleted-hiring-freeze-exempt | en | +2.000 | 1/1 | AGREE |
| en-deleted-partner-tier-names | en | +1.000 | 1/2 | AGREE |
| en-deleted-backup-frequency | en | +0.000 | 1/1 | DISAGREE |
| en-deleted-office-parking-pass | en | +0.000 | 1/1 | DISAGREE |
| en-deleted-release-notes-style | en | +0.000 | 1/2 | DISAGREE |
| en-deleted-table-reservation-limit | en | +1.000 | 1/1 | AGREE |
| pt-dirty-agenda-conselho | pt | +0.000 | 1/1 | DISAGREE |
| pt-dirty-venda-ferias | pt | +0.000 | 1/2 | DISAGREE |
| pt-dirty-conta-ativa | pt | +0.000 | 1/1 | DISAGREE |
| pt-dirty-rota-zona-sul | pt | +0.000 | 1/1 | DISAGREE |
| pt-dirty-desconto-volume | pt | +1.000 | 1/2 | AGREE |
| pt-dirty-abertura-loja | pt | +0.000 | 1/1 | DISAGREE |
| pt-dirty-prazo-estorno | pt | -0.500 | 1/1 | DISAGREE |
| pt-dirty-trilha-dados | pt | +0.000 | 1/1 | DISAGREE |
| en-dirty-holiday-calendar | en | +0.000 | 1/1 | DISAGREE |
| en-dirty-badge-photo | en | +0.000 | 1/1 | DISAGREE |
| en-dirty-snack-budget | en | +0.000 | 1/2 | DISAGREE |
| en-dirty-deskpool-release | en | +0.000 | 1/1 | DISAGREE |
| en-dirty-referral-bonus | en | +0.000 | 1/1 | DISAGREE |
| en-dirty-cycle-count-day | en | +0.000 | 1/2 | DISAGREE |
| en-dirty-support-banner | en | +1.000 | 1/1 | AGREE |
| pt-stale-politica-almoco | pt | +0.000 | 1/1 | DISAGREE |
| pt-stale-meta-vendas | pt | +1.000 | 1/2 | AGREE |
| pt-stale-feriado-municipal | pt | +0.000 | 1/1 | DISAGREE |
| pt-stale-limite-reembolso | pt | +0.000 | 1/1 | DISAGREE |
| pt-stale-versao-app | pt | +0.000 | 1/2 | DISAGREE |
| pt-stale-sala-reserva | pt | +0.000 | 1/1 | DISAGREE |
| pt-stale-desconto-aniversario | pt | +0.000 | 1/1 | DISAGREE |
| pt-stale-jornada-remota | pt | +0.000 | 1/2 | DISAGREE |
| en-stale-password-expiry | en | +0.000 | 1/1 | DISAGREE |
| en-stale-office-capacity | en | +0.000 | 2/2 | DISAGREE |
| en-stale-shipping-cutoff | en | +0.000 | 1/2 | DISAGREE |
| en-stale-training-allowance | en | +0.000 | 1/1 | DISAGREE |
| en-stale-meeting-length | en | +0.000 | 2/2 | DISAGREE |
| en-stale-invoice-discount | en | +0.000 | 1/1 | DISAGREE |
| en-stale-badge-access-doors | en | +0.000 | 1/1 | DISAGREE |

## 9. Resource evidence and coherence checks

| Metric | Value | Gate / coherence |
|---|---|---|
| Pair peak, e5-base-int8 + mmarco-quint8 (isolated fresh process, both sessions resident, post-inference RSS delta, incl. 192 MiB int8 vectors at 250k) | **1120.2 MiB** (embedding session +553.3 MiB; pair +928.2 MiB over base) | approval band — reproduced exactly by the 2026-08-23 probe |
| Pair peak, e5-small-int8 + mmarco-quint8 | 966.8 MiB | automatic pass (retained; not re-probed) |
| Pair peak, e5-small-int8 + bge-int8 | 1116.7 MiB | approval band (retained) |
| Pair peak, e5-base-int8 + bge-int8 | 1271.9 MiB | approval band, ~8 MiB under the 1.25 GiB hard fail (retained first-run probe; no new isolated probe taken in the rerun) |
| In-run e5-base session RSS delta (canonical run) | +552.2 MiB | coheres with the isolated probe (+553.3 MiB) |
| Derived disk/doc | 558 B -> 0.13 GiB steady / 0.26 GiB rebuild at 250k | PASS (retained 555 B coherent) |
| Query embed (solo, int8 session) | 2.5–4.5 ms mean across runs | within Fast budget |
| Reranker solo per-pair p95 | see §4 run table | depth-50 costs 400–645 ms for the three viable candidates in quiet-machine runs — PASS <=900 ms with materially less headroom than the retained first run; fp16 excluded in every run |
| Quantization storage cost | f32 = fp16 = int8 = `86.96% (60/69)` Evidence Recall@5 | zero measured recall cost |
| Embedding session fidelity | e5-small int8-vs-f32 sessions: mean cosine agreement 0.9893 (min 0.9821); retained 0.9919 (min 0.9871) | acceptable; e5-base has no staged f32 counterpart, fidelity not separately measurable |

Probe command for the pair figures above:

```powershell
$env:MEETLY_RAG_PAIR = "e5-base-int8/model_int8.onnx:mmarco-reranker/model_quint8_avx2.onnx"
$env:MEETLY_RAG_BENCH = "1"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark pair_ram_probe -- --nocapture
```

## 10. Gate verdict and blockers

Decision: **blocked-quality-gates** — no production pair selected.

Per-pair verdicts (every evaluated gate shown; a gate that is not evaluated here cannot support selection):

| Gate | A: e5-base+bge-int8 | B: e5-base+mmarco-f32 | C: e5-base+mmarco-quint8 |
|---|---|---|---|
| Reference Recall@1 (rank 1 + facts complete + zero forbidden) | **FAIL** (rank 2, facts 2/2, forb 0/2) | PASS (rank 1, facts 2/2, forb 0/2) | PASS (rank 1, facts 2/2, forb 0/2) |
| Critical Recall@1 = 100% | **FAIL** 40.00% (2/5) | **FAIL** 60.00% (3/5) | **FAIL** 40.00% (2/5) |
| Critical required-fact coverage = 100% | PASS 100.00% (9/9) | PASS 100.00% (9/9) | PASS 100.00% (9/9) |
| Critical forbidden contamination = 0 | **FAIL** 66.67% (4/6) | **FAIL** 66.67% (4/6) | **FAIL** 66.67% (4/6) |
| Exact-term no-regression (>= FTS 90/90) | PASS 100.00% (90/90) | PASS 100.00% (90/90) | PASS 100.00% (90/90) |
| Overall Recall@3 >= 95% | PASS 100.00% (135/135) | PASS 100.00% (135/135) | PASS 99.26% (134/135) |
| Overall Recall@5 >= 98% | PASS 100.00% (135/135) | PASS 100.00% (135/135) | PASS 100.00% (135/135) |
| Evidence Recall@10 >= 90% | PASS 100.00% (209/209) | PASS 100.00% (209/209) | PASS 100.00% (209/209) |
| Semantic +10pt Recall@3 over baseline (0/30) | PASS 100.00% (30/30) | PASS 100.00% (30/30) | PASS 100.00% (30/30) |
| NDCG non-degradation | PASS 0.9009 vs fused 0.8382 | PASS 0.8926 vs 0.8382 | PASS 0.8921 vs 0.8382 |
| Card-multilingual metadata | **NON-conforming** (zh/en card) | conforming | conforming |
| Latency (depth-50 sub-budget) | PASS in quiet runs; excluded in 2/4 same-day runs | PASS in quiet runs; excluded in 2/4 same-day runs | PASS in every run |
| Pair RAM at 250k | approval band (retained 1271.9 MiB) | approval band by arithmetic (1133067858 B); no isolated probe | approval band (measured 1120.2 MiB) |
| Citation/source precision | **UNEVALUATED** (no ChatSource construction) | **UNEVALUATED** | **UNEVALUATED** |

Blockers, in order of what a resolution would require:

1. **Critical Recall@1 fails for every pair** (`chaves-acesso`, `sla-suporte`, `nps-detrator` miss rank 1 under every reranker; best case mmarco-f32 3/5). **Critical forbidden contamination 4/6 fails for every pair**: the stale-version and cross-section critical cases keep surfacing their forbidden claims through fusion+rerank even though full-corpus forbidden sits at 24.79% (30/121). These are model/aggregation findings on a solvable corpus (raw vectors place all five critical targets inside top-5, §8), not corpus defects.
2. **bge-reranker-base remains independently blocked**: zh/en card metadata non-conformity for a PT+EN product (unchanged model-card property) plus its Reference-Rank-1 failure (rank 2 — it prefers the echo-heavy neighbouring meeting) and unstable same-day latency viability (excluded at depth 50 in 2 of 4 runs).
3. **RAM band.** Even if the quality gates were resolved, every conforming pairing sits in the explicit-approval band: e5-base+mmarco-quint8 measures 1120.2 MiB (>1 GiB auto-pass). The auto-pass alternative e5-small-int8+mmarco-quint8 (966.8 MiB) measures lower corpus quality (fused R@3 129/135 vs 131/135; not the contracted family, no reranked full-corpus evaluation).
4. **Citation/source precision is unevaluated** by this simulation for every candidate (no ChatSource construction or final prompt-budget source filtering), so it cannot support selection regardless of the other gates.
5. If every blocker above were cleared, selection would additionally require user approval of the tuned constants recorded here as an architecture addendum, and a clean-hardware latency re-probe (solo tails varied ~1.6–2.4x with machine state across same-day runs).

Tasks 1.4 and 1.5 remain blocked on this task.

## 11. Reproduction commands

From `upstream/`:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
$env:MEETLY_RAG_MODELS_DIR = "$env:TEMP\opencode\meetly-task13\models"   # default location; override if staged elsewhere

# offline-safe logic suite (no artifacts needed)
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark

# reference inference against staged artifacts (assert mode; add
# MEETLY_RAG_RECORD_EXPECTATIONS=1 to regenerate recorded_expectations.json)
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark reference_inference -- --nocapture

# CANONICAL full hybrid corpus + resource benchmark (--release is the evidence command)
$env:MEETLY_RAG_BENCH = "1"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark hybrid_corpus_and_resource_benchmark -- --nocapture
Remove-Item Env:MEETLY_RAG_BENCH -ErrorAction SilentlyContinue
```

Quality tables are deterministic: the canonical `--release` run recorded in the manifest and an independent same-day reproduction produced digit-identical results for every quality metric, gate verdict, ablation cell, and the concept-proxy table. **Debug-profile caveat (disclosed, guard unchanged):** running the full benchmark without `--release` on loaded machine state halts at the harness's designed guard — reranker candidates then exceed the 900 ms depth-50 sub-budget under debug-build overhead (mmarco-quint8 p95 18.4–18.9 ms x 50 = 921–946 ms) and the harness refuses to fabricate viable-candidate evidence (`no reranker candidate fits the 900 ms deterministic sub-budget`). Latency gates are only meaningful in release builds; the guard itself is unchanged.

## 12. Rollback

All changes are tests/evidence-only; no production file, schema, API, or persisted-data surface is touched, and nothing was committed.

- `git checkout 1e41b6b -- frontend/src-tauri/tests/model_benchmark.rs frontend/src-tauri/tests/retrieval_evaluation.rs frontend/src-tauri/tests/fixtures/model_bundle_manifest.json` restores the checkpointed harness/manifest (checkpoint `1e41b6b` predates this task; note it also predates the 1.3 rerun content, so restoring loses the rerun evidence record).
- Delete `frontend/src-tauri/tests/fixtures/concept_lexicon.rs` (created by the mechanical lexicon extraction shared by both harnesses).
- Restore the previous version of this report from review records if needed (file is untracked).
- Staged model artifacts live outside git under `%TEMP%\opencode\meetly-task13\models`; removing them changes nothing in the repo.

## 13. Omissions and spillover

- Per dispatch constraints, the Task Execution Log entry for this rerun is
  maintained by the orchestrator in `sprint-1-quality-gates.md`; the unrelated
  Notes/Chat execution record is not part of this program.
- The stale `fixtures/corpus.rs` header comment that located the margin lexicon in `retrieval_evaluation.rs` was corrected to point at `fixtures/concept_lexicon.rs` (comment-only; no corpus behavior change).
- Latency viability of bge-int8/mmarco-f32 is machine-state-dependent (viable in quiet runs, excluded in loaded runs). Any future selection needs one clean-hardware re-probe per §10.5; this report does not resolve it.
- mmarco-f32's title-dependent ablation headline (§7) means its numbers must not be quoted as pure embedding-model quality; recorded here because the sprint decision requires the tuned/beta-0 pair, not because either f32 variant is a selection candidate.
