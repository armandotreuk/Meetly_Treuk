# Task 1.3G — Corpus Patch and Contamination Gate Re-scope

| Field | Value |
|---|---|
| Status | **Complete** — the approved category-(c) corpus patches, source-state gate re-scope, enforcing admissibility checks, baseline re-record, and deleted-row alignment are implemented and verified. Model selection remains owned by the final Task `1.3` run. |
| Date | 2026-08-24 |
| Baseline | `e209b5d5c2272f8b71121cad2b2217fd88604047` |
| Scope | Tests/evidence only: `frontend/src-tauri/tests/` and this report. No production, PRD, architecture, README, ROADMAP, or execution-log file changed. |
| Method | Patch only the approved corpus surfaces, derive carrier state from fixture meeting/evidence state and text, gate retrieval-stage facts only, and retain answer-stage context presence as labeled information. No threshold, tuned constant, tuning objective, or held-out partition changed. |

## 1. Corpus-content boundary

`git diff e209b5d -- frontend/src-tauri/tests/fixtures/corpus` shows exactly
three content-line replacements across the two approved surfaces:

1. `pt-ref-chaves-acesso`: the target title changes from `Segurança de acesso
   — políticas de credenciais` to `Governança de chaves em ambientes —
   controle de acesso`. This is a plausible topic signal, not the decision
   answer and not a copy of the query sentence; the evidence keeps the
   trocar/chaves versus rotação/credenciais terminological gap.
2. `fixture-whatsapp-retention`: topical neighbour
   `mtg-onboarding-ativacao` now carries `apenas 3 dias` in explicitly
   superseded draft text.
3. `fixture-whatsapp-retention`: topical neighbour
   `mtg-pesquisa-disparos` now carries `apenas 4 dias` in explicitly discarded
   draft text.

No current note inside `mtg-whatsapp-retention` changed. No other corpus file
or case content changed. The remaining fixture changes are schema/policy, not
corpus content: `corpus_types.rs` derives source-state classification and
`evaluation_policy.json` re-records one baseline numerator. The two harnesses
implement assertions, reporting, gate partitioning, deleted-row alignment, and
the metadata-conforming embedding-family selection needed by the production-
channel assertion.

## 2. Patched-surface proof: before versus after

The supervised margin uses raw fixture text. Rank-1 admissibility uses only
lexical margin, title margin, and the contracted e5-base raw-vector rank;
`CONCEPT_LEXICON` is excluded.

| Surface | Before supervised margin | After supervised margin | Before rank-1 admissibility | After rank-1 admissibility |
|---|---|---|---|---|
| `fixture-whatsapp-retention` | winning lexical `+0.900` (`lexical +0.900`, `concept +0.000`, `title -0.350`) | unchanged: winning lexical `+0.900` (`+0.900/+0.000/-0.350`) | lexical `+0.900`, title `-0.350`, raw vector `1/4`, `any_positive_channel=true` | lexical `+0.900`, title `-0.350`, raw vector `1/5`, `any_positive_channel=true` |
| `pt-ref-chaves-acesso` | winning concept `+0.250`; production margins lexical `-1.333`, title `-1.000` | winning concept `+0.250`; production margins lexical `-1.333`, **title `+0.250`** | lexical `-1.333`, title `-1.000`, raw vector `5/9`, `any_positive_channel=false` | lexical `-1.333`, title `+0.250`, raw vector `4/10`, **`any_positive_channel=true`** |

The title-only chaves patch leaves FTS falsifiability intact because the
current baseline does not index titles:

```text
CASE fixture-whatsapp-retention ... status=EXPECTED_FTS_FAILURE_REFERENCE:PASS
CASE pt-ref-chaves-acesso ... status=EXPECTED_FTS_FAILURE_REFERENCE:PASS
```

The two WhatsApp facts are now real, independently carried retrieval-stage
facts:

```text
[SUPERVISED:co-residence] case=fixture-whatsapp-retention fact="apenas 3 dias" carriers=[mtg-onboarding-ativacao-ev] required_co_residence=false
[SUPERVISED:co-residence] case=fixture-whatsapp-retention fact="apenas 4 dias" carriers=[mtg-pesquisa-disparos-ev] required_co_residence=false
[SUPERVISED:evidence-admissibility] case=fixture-whatsapp-retention verdict=FEASIBLE_BY_ORDERING required_carrier_meetings=1/5 required_documents=2 required_le_window=true required_missing_from_pool=0 hydrated_pool_docs=8 retained_window=8 clean_docs_in_pool=8 forbidden_bearing_docs=0 constructive_retained_required=2/2 constructive_retained_forbidden=0
```

## 3. Forbidden-fact classification
Classification uses fact-level evidence/text-aware precedence: deleted sources
remain deleted; current authoritative text in an expected meeting is answer-stage
regardless of dirty/stale index state; `StaleDerived` requires exactly that the
fact is present in indexed text and absent from authoritative text, so meeting
dirty/stale state or unrelated whole-text differences never override fact-level
authoritative presence; explicit drafts are superseded, and unmarked current
topical carriers fail with an error.

### 3.1 All critical facts

| Case | Fact | Carrier(s) and derived source state | Class |
|---|---|---|---|
| `fixture-whatsapp-retention` | `apenas 3 dias` | `mtg-onboarding-ativacao/mtg-onboarding-ativacao-ev` — superseded/draft | retrieval-stage |
| `fixture-whatsapp-retention` | `apenas 4 dias` | `mtg-pesquisa-disparos/mtg-pesquisa-disparos-ev` — superseded/draft | retrieval-stage |
| `pt-ref-cobranca-regua` | `dias 5 e 15` | `mtg-cobranca-vencidas/cobr-rascunho` — current authoritative expected | answer-stage deferred |
| `pt-ref-chaves-acesso` | `renovação mensal` | `mtg-pt-ref-chaves-acesso/chv-rascunho` — current authoritative expected | answer-stage deferred |
| `pt-ref-sla-suporte` | `em um dia inteiro` | `mtg-pt-ref-sla-suporte/sla-antigo` — current authoritative expected; `mtg-sla-legado/sla-stale` — stale-derived | answer-stage deferred because any current expected carrier controls |
| `pt-ref-nps-detrator` | `cupom como resposta padrão` | `mtg-pt-ref-nps-detrator/nps-cupom` — current authoritative expected | answer-stage deferred |

### 3.2 All-fact denominators

```text
[SUPERVISED:forbidden-classification-counts] retrieval-stage=107/121 answer-stage-deferred=14/121 total=121/121
```

Of the 107 retrieval-stage facts, 33 have fixture carriers: three superseded
facts, 15 deleted-source facts, and 15 stale-derived facts; the other
74 are carrierless negative labels. All 14 answer-stage facts have a current
authoritative expected-meeting carrier. The supervised test pins the exact
`(107, 14)` split, while the supervised retrieval and canonical runs print
every fact, carrier, state, and class.

## 4. Harness assertions and falsifiability audit

Every required proof is executable rather than report-only:

| Requirement | Enforcing assertion or mutation |
|---|---|
| Baseline still fails both patched cases | `current_fts_baseline_is_deterministic_and_falsifiable` calls `validate_baseline_expectations`; both cases print `EXPECTED_FTS_FAILURE_REFERENCE:PASS`, then the exact baseline snapshot is asserted. |
| Answer-key-free structural solvability | `corpus_structural_solvency_invariants_hold_without_the_answer_key` calls `validate_structural_solvency` without expected/required IDs. |
| Positive chaves production channel | `corpus_supervised_labels_margin_coverage_and_distinctness_hold` asserts lexical or title margin `> 0`; observed title margin is `+0.250`. |
| Chaves rank-1 admissibility | The canonical e5-base loop calls `enforce_rank1_admissibility(...).expect(...)` for every critical case; chaves reports `any_positive_channel=true`. `rank1_admissibility_rejects_a_no_positive_channel_mutation` first accepts the patched title, restores the old title, and asserts rejection. |
| Real WhatsApp carriers and source states | `carrier_state_and_ordering_admissibility_mutations_are_rejected` asserts both facts have non-empty carriers, retrieval-stage class, and only `Superseded` state. |
| WhatsApp ordering feasibility | The same test requires the real case to return `Ok`; the supervised suite requires all critical retrieval-stage verdicts to be `FEASIBLE_BY_ORDERING`. |
| Carrier-state classification and denominators | `classify_forbidden_fact` derives states and rejects unclassified current topical carriers; the supervised suite asserts exactly `107` retrieval-stage and `14` answer-stage facts. Mutations assert authoritative carriers in expected Dirty and StaleDerived meetings are answer-stage/`CurrentAuthoritative`, while an indexed-only stale occurrence is retrieval-stage/`StaleDerived`. |
| Enforcing admissibility and violating mutations | The former report-only helper now returns `Err` for any critical retrieval-stage empty carrier or non-feasible ordering, and the suite `.expect`s success. Mutations trap a carrier in required evidence and remove a carrier; both are asserted to fail. |
| Deleted-row production alignment | `setup_case` no longer inserts deleted meeting FTS rows; canonical diagnostics report `15/15` deleted cases with identical lexical ordering and zero divergences. |
| Corpus floors/distinctness/privacy | Existing corpus/private checks remain assertions; supervised distinct shape is `120/120` against floor `96/120`. |

## 5. Baseline re-record

The old authority is `e209b5d` / Task `1.2R`; the new values come from the
final patched tree. This is the complete expected-baseline table.

| Metric | Old | New | Delta and attribution |
|---|---|---|---|
| Meeting Recall@1 | 53.33% (`72/135`) | 53.33% (`72/135`) | none |
| Meeting Recall@3 | 71.11% (`96/135`) | 71.11% (`96/135`) | none |
| Meeting Recall@5 | 91.85% (`124/135`) | 91.85% (`124/135`) | none |
| MRR | `0.695833` (`120` cases) | `0.695833` (`120` cases) | none |
| Evidence Recall@10 | 86.60% (`181/209`) | 86.60% (`181/209`) | none |
| Required-fact coverage | 87.25% (`130/149`) | 87.25% (`130/149`) | none |
| Forbidden-fact contamination | 20.66% (`25/121`) | 21.49% (`26/121`) | `+1/121`; the newly real `apenas 3 dias` superseded carrier appears in retained baseline context. The `apenas 4 dias` carrier does not add a retained hit. |
| Citation/source precision | 100.00% (`471/471`) | 100.00% (`471/471`) | none |
| Exact/name/number Recall@3 | 100.00% (`90/90`) | 100.00% (`90/90`) | none |
| Semantic Recall@3 | 0.00% (`0/30`) | 0.00% (`0/30`) | none |
| Retrieval-stage contamination | not defined in old gate | 15.89% (`17/107`) | new approved partition |
| Answer-stage context presence | not defined in old gate | 64.29% (`9/14`) | new informational/deferred partition |

The chaves title is invisible to FTS. Deleted-row alignment is measured
ranking-inert (`15/15` identical, `0` divergent). Therefore the single
baseline movement is fully attributable to the new WhatsApp carrier. The
policy snapshot changes only `forbiddenContamination.numerator`, `25` to `26`.

## 6. Post-patch critical gate outlook — informational only

The final canonical run completed for both contracted e5-base + mmarco
reranker variants. No model or constants were selected or promoted here.

| Metric | mmarco-f32 | mmarco-quint8 |
|---|---|---|
| Critical per-case ranks | WhatsApp `1`, cobrança `1`, chaves `4`, SLA `2`, NPS `1` | same |
| Critical Recall@1 | **FAIL** 60.00% (`3/5`) | **FAIL** 60.00% (`3/5`) |
| Critical required facts | PASS 100.00% (`9/9`) | PASS 100.00% (`9/9`) |
| Critical retrieval-stage contamination | **PASS** 0.00% (`0/2`) | **PASS** 0.00% (`0/2`) |
| Critical answer-stage context presence | deferred/not evaluated 100.00% (`4/4`) | deferred/not evaluated 100.00% (`4/4`) |
| Overall Recall@3 / @5 | PASS 99.26% (`134/135`) / 100.00% (`135/135`) | same |
| Evidence Recall@10 | PASS 100.00% (`209/209`) | PASS 100.00% (`209/209`) |
| Overall retrieval-stage contamination | 0.93% (`1/107`) | 0.93% (`1/107`) |
| Overall answer-stage context presence | informational 100.00% (`14/14`) | informational 100.00% (`14/14`) |
| Citation/source precision | PASS 100.00% (`602/602`) | PASS 100.00% (`602/602`) |
| Joint-feasible diagnostic configurations | `79/2160` | `78/2160` |

The admissibility defect is closed: every critical case has a positive
production-implementable rank-1 channel and the critical retrieval-stage
contamination gate is feasible and passes. The tuned pair still misses chaves
and SLA at rank 1, so the canonical decision remains
`blocked-quality-gates`. That is model/aggregation evidence for the final
Task `1.3`, not a corpus or gate-admissibility blocker.

## 7. Verification and reproducibility

From `upstream/`:

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

Results on the final code tree:

- retrieval evaluation: **6 passed, 0 failed**;
- ordinary model benchmark: **9 passed, 0 failed**;
- canonical release benchmark: **1 passed, 0 failed**, 121/121 fact
  classifications printed, decision `blocked-quality-gates`;
- frontend typecheck: pass;
- Vitest: **20 files, 95 tests passed**;
- Tauri `cargo check`: pass;
- rustfmt and diff checks: pass after this report (final rerun below).

## 8. Privacy

Canonical focused scan, with the same expression and scope shape as `1.2R`
and `1.3F`:

```powershell
rg -n -i "(c:\\users\\|/users/|@gmail\.|@outlook\.|sk-[a-z0-9]{4}|api_key|bearer |onedrive\\)" frontend/src-tauri/tests docs/hybrid-rag/task-1.3g-corpus-gate-patch.md
```

Result: **pass, 9 matched lines, all classified**:

| Match location | Lines | Classification |
|---|---:|---|
| this report | 199 | The canonical scanner command itself. |
| `retrieval_evaluation.rs` | 640–646 (6 matched lines) | Existing private-marker validator literals: one home-path form, two email-domain forms, two credential/header forms, and one synchronized-folder form. These are the strings the test searches for, not fixture data. |
| `model_benchmark.rs` | 4284, 4364 | Regex false positives inside the existing risk decision label: the final two letters of `risk` plus the label's hyphen and first four letters of the next word satisfy the token-prefix alternative. One line defines the label and one compares it. |

No corpus/fixture content, secret, personal address, or user path matched. The
in-harness `validate_private_safe` assertion also passed. Model weights remain
outside Git.

## 9. Exact file inventory

| File | Change class |
|---|---|
| `frontend/src-tauri/tests/fixtures/corpus/reference_a.rs` | Approved corpus content: two WhatsApp superseded/draft carriers only. |
| `frontend/src-tauri/tests/fixtures/corpus/reference_b.rs` | Approved corpus content: chaves target title only. |
| `frontend/src-tauri/tests/fixtures/corpus_types.rs` | In-scope fixture schema/derived source-state classifier. |
| `frontend/src-tauri/tests/fixtures/evaluation_policy.json` | In-scope baseline snapshot: forbidden numerator `25` → `26`. |
| `frontend/src-tauri/tests/retrieval_evaluation.rs` | In-scope baseline alignment, stage metrics/reporting, enforcing assertions, and mutations. |
| `frontend/src-tauri/tests/model_benchmark.rs` | In-scope canonical stage metrics/gates, production-channel assertion/mutation, classifications, and metadata-conforming family selection. |
| `docs/hybrid-rag/task-1.3g-corpus-gate-patch.md` | This evidence report. |

## 10. Rollback against `e209b5d`

All changes are tests/evidence-only. Exact rollback:

```powershell
git checkout e209b5d -- frontend/src-tauri/tests/fixtures/corpus/reference_a.rs frontend/src-tauri/tests/fixtures/corpus/reference_b.rs frontend/src-tauri/tests/fixtures/corpus_types.rs frontend/src-tauri/tests/fixtures/evaluation_policy.json frontend/src-tauri/tests/model_benchmark.rs frontend/src-tauri/tests/retrieval_evaluation.rs
Remove-Item "docs/hybrid-rag/task-1.3g-corpus-gate-patch.md"
```

No model artifact is tracked; staged weights remain outside Git under the
temporary Task `1.3` model directory.

## 11. Omissions and spillover

- Per dispatch, the main orchestrator owns `sprint-1-quality-gates.md` task
  status and the Notes/Chat execution log; neither was edited.
- No answer-stage generation/non-assertion evaluation was added. Its explicit
  14-fact set and context-presence denominators are deferred to Sprint 3/4.
- No pair selection, constant promotion, production implementation, chunk
  policy, threshold change, or unrelated corpus cleanup was performed.
- The canonical final-Task decision remains blocked on Critical Recall@1
  `3/5`; this report does not reinterpret that result as a Task `1.3G`
  failure because all approved admissibility and contamination proofs pass.
- The evidence-aware classifier correction did not alter the real fixture
  split or executable gate output: counts remain `107/121` and `14/121`.
  One non-critical dirty topical draft is now correctly labeled superseded
  instead of stale-derived; its retrieval-stage class is unchanged.
  `reference_b.rs` and this report both use `Governança de chaves em ambientes
  — controle de acesso`; the current-tree canonical rerun confirms chaves rank
  `4`, with feasibility `79/2160` f32 and `78/2160` quint8.
