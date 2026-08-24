# Task 1.2R — Corpus Re-Authoring and Solvability Invariant: Evidence Report

**Status:** Complete
**Owner:** fresh `worker-l` session (`opencode-go/ox-alpha-free`, no substitution)
**Completed:** 2026-08-23
**Authority:** `docs/hybrid-rag/architecture.md` (Baseline Failure Reproduction,
Corpus Solvability, Corpus Size Floors, gate table, Reference Acceptance Case);
`docs/hybrid-rag/sprint-1-quality-gates.md` Task 1.2R incl. the approved
two-part solvability clarification.

## Context

This was a retry: a prior `worker-l` session died mid-task and left an
uncommitted partial attempt (family-module corpus skeleton, solvability/margin
harness machinery, stale `expectedBaseline`, stale README). This session
audited every acceptance criterion from scratch and repaired the attempt
rather than trusting or wholesale rewriting it. A main-agent acceptance audit
then required a remediation pass (answer-key/supervised separation of the
structural check, supervised target-evidence distinctness key, meaningful
`scope.meeting_id` validation, corrected documentation, recorded privacy
command); all work is inside `frontend/src-tauri/tests/` plus this report; no
production file was touched.

## What the repaired corpus looks like

- 120 hand-authored cases assembled from literal family modules
  (`fixtures/corpus/*.rs`); builders handle schema only. Counts:
  **total=120, pt=60, en=60, critical=5**; every required overlapping category
  ≥15 (minimums observed: `scope_meeting` 15, `transcript_only` 16,
  `scope_folder` 16, `state_*` 15 each).
- Distinct normalized question/expected-evidence shapes:
  **120/120** (floor 96 / 80%), re-measured after the remediation described
  below. The supervised shape key hashes the normalized question plus only the
  required target evidence text — IDs label which evidence is target material
  — with digit-bearing tokens collapsed so `format!` ordinal variants cannot
  inflate the count.
- Two solvability invariants asserted by the harness, with a strict
  answer-key/supervised separation:
  1. `validate_structural_solvency` is genuinely answer-key-free. It reads
     only question raw text, scope schema, and meeting/titles/dates raw text:
     duplicate questions, distinct titles/dates per case, scope field
     contracts (Meeting scope must focus a meeting inside a permitted set of
     at least two; Folder scope requires `folder_id`; All/Snapshot/Today carry
     neither selector), folder scopes that exclude nothing in-corpus, semantic
     nonce discriminators, and raw candidate counts that reject
     verbatim-question and all-content-term clone walls (limit 2; measured
     corpus maximum is 2 — an answer plus one stale/draft restatement — while
     the Task 1.2 defect was a wall of 30). A grep of the function body for
     `expected_meeting_ids|required_evidence_ids|critical` returns zero
     matches.
  2. `corpus_supervised_labels_margin_coverage_and_distinctness_hold` is the
     only place expected IDs appear, as labels: per-case margins must beat the
     strongest distractor on lexical/concept/title channels computed from
     fixture text; the architecture's target-aware exception ("a verbatim or
     query-superset distractor is illegal *unless* the expected evidence also
     contains it") is enforced here via `supervised_coverage_violations`,
     where containment itself is decided from raw text; and the distinctness
     floor above is asserted here.
  Neither half can substitute for the other: the structural check proves the
  instrument is well-formed without knowing the answers; the supervised check
  proves a competent retriever could solve it and that no distractor wins only
  because the key was hidden from the structural pass.

## Five critical cases — named failure modes

| Case | Named mode | Asserted shape |
|---|---|---|
| `fixture-whatsapp-retention` | Evidence completeness (pinned reference) | Meeting rank passes/near-passes (≤3) while neither the full schedule `dias 1, 3, 7, 10 e 15` nor the MPV/non-MPV day-one distinction survives fragment retrieval; superseded/partial neighbour cadences surface `3 dias`/`4 dias` fragments instead. Pinned block asserts rank≤3 ∧ incomplete evidence/facts ∧ fragments present ∧ schedule absent. |
| `pt-ref-cobranca-regua` | Superseded-draft contamination + section loss | Approved cadence (2, 8, 20, 35) and SMS-channel step live in separate sections; the discarded draft (5, 15) and finance-cadence neighbours push the SMS section out of the bounded top-10 → evidence/fact incompleteness with rank passing. |
| `pt-ref-chaves-acesso` | Terminological gap | Question says trocar/chaves; the decision says rotação periódica de credenciais. Inventory/access neighbours own the surface vocabulary; target ranks below the top 3. |
| `pt-ref-sla-suporte` | Stale-version contamination | Renewed agreement avoids the question nouns; the stale-derived legacy summary still circulates with the retired figure and is pinned to be retrieved *and* to surface its forbidden claim in retained text. |
| `pt-ref-nps-detrator` | Cross-section join + snippet loss | Threshold and callback commitments sit in different sections; the long callback note is truncated by the bounded snippet window so the commitment phrase is "present in storage but absent from the snippet shown to the model" — the exact recorded production mode. |

## New baseline (replaces void 1.2 figures)

| Metric | 1.2R (new, denominators) | Task 1.2 (void) |
|---|---|---|
| Meeting Recall@1 | 53.33% (72/135) | 75/135 |
| Meeting Recall@3 | 71.11% (96/135) | 90/135 |
| Meeting Recall@5 | 91.85% (124/135) | 90/135 |
| MRR | 0.695833 (120 cases) | 0.625 |
| Evidence Recall@10 | 86.60% (181/209) | 90/150 |
| Required-fact coverage | 87.25% (130/149) | 90/150 |
| Forbidden-fact contamination | 20.66% (25/121) | 105/135 |
| Citation/source precision | 100% (471/471) | 300/300 |

Exact-term/number/name no-regression holds: **PASS 100% (90/90)**.
Reference-category cases: 15/15 under-served (harness-asserted). Semantic
category: **Recall@3 0.00% (0/30)** under the FTS baseline — maximally
under-served while individually solvable (margins above).

## Cancellation verdict

The cancellation condition does **not** trigger. On a corpus that competent
retrievers can solve (positive margin everywhere, structural checks pass),
the current FTS-only baseline demonstrably fails the entire reference
category (15/15 under-served, asserted per case) and the entire semantic
category (0/30 Recall@3). The `ROADMAP.md` deferral condition — a repeatable
benchmark showing FTS5 misses important results at a material rate — is now
met by real FTS gaps, not by an unanswerable instrument. Proceed to the
Task 1.3 rerun; do not cancel.

## 120-case margin table (channel/margin over strongest distractor)

Winning channel distribution: lexical 46, concept 22, title 52. Every case
strictly positive.

| Case | Channel | Margin | Lexical | Concept | Title |
|---|---|---|---|---|---|
| fixture-whatsapp-retention | lexical | +0.900 | +0.900 | 0.000 | −0.350 |
| pt-ref-cobranca-regua | lexical | +1.500 | +1.500 | 0.000 | +1.000 |
| pt-ref-chaves-acesso | concept | +0.250 | −1.333 | +0.250 | −1.000 |
| pt-ref-sla-suporte | concept | +0.500 | −1.000 | +0.500 | −1.000 |
| pt-ref-nps-detrator | concept | +0.750 | −1.000 | +0.750 | −0.333 |
| pt-ref-onboarding-primeiros-dias | title | +0.167 | −2.000 | −1.000 | +0.167 |
| pt-ref-renovacao-aprovacao | title | +0.500 | −1.000 | −1.000 | +0.500 |
| pt-ref-reativacao-inativos | title | +0.667 | −1.500 | −1.000 | +0.667 |
| pt-ref-trilhas-capacitacao | title | +0.667 | −1.000 | 0.000 | +0.667 |
| pt-ref-melhorias-produto | title | +0.667 | −2.333 | 0.000 | +0.667 |
| pt-ref-suporte-revendas | title | +0.667 | −2.000 | −0.500 | +0.667 |
| pt-ref-reembolso-limite | title | +0.500 | −2.000 | −0.667 | +0.500 |
| pt-ref-auxilio-remoto | title | +0.250 | −3.000 | 0.000 | +0.250 |
| pt-ref-licencas-disponiveis | title | +0.583 | −1.000 | 0.000 | +0.583 |
| pt-ref-revisao-logs | title | +0.833 | +0.333 | 0.000 | +0.833 |
| pt-semantic-paraphrase-031 | concept | +1.000 | −1.000 | +1.000 | 0.000 |
| pt-semantic-paraphrase-016 | concept | +0.333 | −0.500 | +0.333 | 0.000 |
| pt-semantic-paraphrase-017 | title | +1.500 | −1.000 | −0.333 | +1.500 |
| pt-semantic-paraphrase-018 | concept | +0.333 | −1.000 | +0.333 | 0.000 |
| pt-semantic-paraphrase-019 | concept | +0.500 | −1.000 | +0.500 | 0.000 |
| pt-semantic-paraphrase-020 | title | +1.000 | −1.000 | −0.333 | +1.000 |
| pt-semantic-paraphrase-021 | concept | +1.667 | −1.000 | +1.667 | 0.000 |
| pt-semantic-paraphrase-022 | title | +1.500 | −1.000 | −0.333 | +1.500 |
| pt-semantic-paraphrase-023 | concept | +0.167 | −1.000 | +0.167 | 0.000 |
| pt-semantic-paraphrase-024 | concept | +0.667 | −1.000 | +0.667 | +0.500 |
| pt-semantic-paraphrase-025 | title | +1.000 | −1.000 | −0.333 | +1.000 |
| pt-semantic-paraphrase-026 | concept | +0.333 | −1.000 | +0.333 | 0.000 |
| pt-semantic-paraphrase-027 | title | +1.500 | −1.000 | −0.333 | +1.500 |
| pt-semantic-paraphrase-028 | concept | +0.667 | −1.000 | +0.667 | 0.000 |
| pt-semantic-paraphrase-029 | concept | +0.667 | −1.000 | +0.667 | 0.000 |
| en-semantic-paraphrase-001 | title | +1.000 | −1.000 | −0.333 | +1.000 |
| en-semantic-paraphrase-002 | concept | +0.333 | −1.000 | +0.333 | 0.000 |
| en-semantic-paraphrase-003 | title | +1.500 | −1.000 | −0.333 | +1.500 |
| en-semantic-paraphrase-004 | concept | +0.667 | −1.000 | +0.667 | 0.000 |
| en-semantic-paraphrase-005 | concept | +0.500 | −1.000 | +0.500 | 0.000 |
| en-semantic-paraphrase-006 | concept | +0.833 | −1.000 | +0.833 | 0.000 |
| en-semantic-paraphrase-007 | concept | +1.667 | −1.000 | +1.667 | 0.000 |
| en-semantic-paraphrase-008 | title | +2.000 | −1.000 | −0.667 | +2.000 |
| en-semantic-paraphrase-009 | title | +2.500 | −2.000 | −0.333 | +2.500 |
| en-semantic-paraphrase-010 | title | +1.000 | −1.000 | −0.333 | +1.000 |
| en-semantic-paraphrase-011 | title | +1.000 | −1.000 | −0.667 | +1.000 |
| en-semantic-paraphrase-012 | concept | +0.667 | −1.000 | +0.667 | 0.000 |
| en-semantic-paraphrase-013 | title | +1.000 | −1.000 | −0.333 | +1.000 |
| en-semantic-paraphrase-014 | concept | +0.667 | −1.000 | +0.667 | 0.000 |
| en-semantic-paraphrase-015 | title | +1.500 | −1.000 | −0.667 | +1.500 |
| pt-followup-parcela-orcamento | lexical | +3.000 | +3.000 | +0.500 | +0.500 |
| pt-followup-edital-concurso | lexical | +2.000 | +2.000 | +0.500 | +1.000 |
| pt-followup-parecer-auditoria | lexical | +3.500 | +3.500 | 0.000 | +1.000 |
| pt-followup-vagas-estagio | lexical | +3.500 | +3.500 | +1.000 | +1.500 |
| pt-followup-comprovante-prazo | lexical | +4.000 | +4.000 | 0.000 | 0.000 |
| pt-followup-visita-planta | lexical | +4.000 | +4.000 | 0.000 | +1.500 |
| pt-followup-salas-congresso | lexical | +4.000 | +4.000 | 0.000 | +1.000 |
| en-followup-community-budget | lexical | +5.000 | +5.000 | +1.000 | +1.000 |
| en-followup-workshop-registration | lexical | +3.000 | +3.000 | 0.000 | +1.000 |
| en-followup-audit-partner | lexical | +3.000 | +3.000 | +1.000 | +1.000 |
| en-followup-license-count | lexical | +3.500 | +3.500 | 0.000 | +1.000 |
| en-followup-expense-cutoff | lexical | +4.000 | +4.000 | +1.000 | +1.500 |
| en-followup-vendor-owner | lexical | +3.000 | +3.000 | +2.000 | +1.500 |
| en-followup-annex-desks | lexical | +2.000 | +2.000 | 0.000 | +0.500 |
| en-followup-staging-build | lexical | +3.000 | +3.000 | 0.000 | +1.000 |
| pt-multi-aurora-boreal | concept | +1.000 | +0.500 | +1.000 | −0.500 |
| en-multi-northwind-ironwood | lexical | +2.500 | +2.500 | +0.500 | +0.750 |
| pt-multi-vesper-zenite | title | +0.500 | +0.500 | 0.000 | +0.500 |
| en-multi-bluefin-kestrel | lexical | +0.500 | +0.500 | 0.000 | 0.000 |
| pt-multi-coral-sargaco | title | +0.500 | +0.500 | 0.000 | +0.500 |
| en-multi-larkspur-basalt | title | +0.500 | +0.500 | 0.000 | +0.500 |
| pt-multi-jacaranda-guavira | title | +1.000 | +0.500 | 0.000 | +1.000 |
| en-multi-quartz-meadow | title | +1.000 | +0.500 | 0.000 | +1.000 |
| pt-multi-onca-pintada | title | +0.500 | +0.500 | 0.000 | +0.500 |
| en-multi-harbor-lantern | title | +1.000 | +0.500 | 0.000 | +1.000 |
| pt-multi-ipueira-taboca | title | +0.500 | +0.500 | 0.000 | +0.500 |
| en-multi-summit-valley | lexical | +1.500 | +1.500 | 0.000 | +0.500 |
| pt-multi-cerrado-mangue | title | +0.500 | +0.500 | 0.000 | +0.500 |
| en-multi-fjord-dune | lexical | +0.500 | +0.500 | 0.000 | 0.000 |
| en-multi-opal-topaz | lexical | +1.500 | +1.500 | 0.000 | +1.000 |
| en-deleted-invoice-threshold | title | +0.500 | 0.000 | 0.000 | +0.500 |
| en-deleted-onboarding-guide | lexical | +2.000 | +2.000 | 0.000 | +1.000 |
| en-deleted-server-window | lexical | +3.000 | +3.000 | +1.000 | +1.000 |
| en-deleted-refund-policy-code | lexical | +2.000 | +2.000 | +1.000 | +1.000 |
| en-deleted-badge-colors | title | +0.500 | 0.000 | 0.000 | +0.500 |
| en-deleted-quota-regional | title | +0.500 | 0.000 | 0.000 | +0.500 |
| en-deleted-training-room-cap | lexical | +2.000 | +2.000 | 0.000 | +1.000 |
| en-deleted-api-version-date | lexical | +1.000 | +1.000 | 0.000 | +0.500 |
| en-deleted-expense-per-diem | lexical | +2.000 | +2.000 | 0.000 | +1.000 |
| en-deleted-hiring-freeze-exempt | lexical | +3.000 | +3.000 | +2.000 | +0.500 |
| en-deleted-partner-tier-names | lexical | +3.000 | +3.000 | +1.000 | +0.500 |
| en-deleted-backup-frequency | lexical | +0.500 | +0.500 | 0.000 | −0.500 |
| en-deleted-office-parking-pass | lexical | +2.000 | +2.000 | 0.000 | +1.000 |
| en-deleted-release-notes-style | lexical | +1.000 | +1.000 | 0.000 | +0.500 |
| en-deleted-table-reservation-limit | lexical | +2.000 | +2.000 | +1.000 | 0.000 |
| pt-dirty-agenda-conselho | title | +1.000 | +1.000 | 0.000 | +1.000 |
| pt-dirty-venda-ferias | title | +1.000 | 0.000 | 0.000 | +1.000 |
| pt-dirty-conta-ativa | title | +0.500 | −1.000 | 0.000 | +0.500 |
| pt-dirty-rota-zona-sul | lexical | +2.000 | +2.000 | 0.000 | +1.000 |
| pt-dirty-desconto-volume | lexical | +2.000 | +2.000 | +1.000 | 0.000 |
| pt-dirty-abertura-loja | lexical | +1.000 | +1.000 | 0.000 | +0.500 |
| pt-dirty-prazo-estorno | lexical | +1.000 | +1.000 | −0.500 | +0.500 |
| pt-dirty-trilha-dados | lexical | +3.000 | +3.000 | 0.000 | +0.500 |
| en-dirty-holiday-calendar | title | +0.500 | 0.000 | 0.000 | +0.500 |
| en-dirty-badge-photo | title | +1.000 | +1.000 | 0.000 | +1.000 |
| en-dirty-snack-budget | title | +1.000 | +1.000 | 0.000 | +1.000 |
| en-dirty-deskpool-release | lexical | +2.000 | +2.000 | 0.000 | +1.000 |
| en-dirty-referral-bonus | lexical | +1.000 | +1.000 | 0.000 | +0.500 |
| en-dirty-cycle-count-day | title | +1.000 | −1.000 | 0.000 | +1.000 |
| en-dirty-support-banner | lexical | +2.000 | +2.000 | +1.000 | +1.000 |
| pt-stale-politica-almoco | title | +0.500 | 0.000 | 0.000 | +0.500 |
| pt-stale-meta-vendas | concept | +1.000 | +1.000 | +1.000 | 0.000 |
| pt-stale-feriado-municipal | lexical | +1.000 | +1.000 | 0.000 | 0.000 |
| pt-stale-limite-reembolso | lexical | +1.000 | +1.000 | 0.000 | 0.000 |
| pt-stale-versao-app | lexical | +1.000 | +1.000 | 0.000 | 0.000 |
| pt-stale-sala-reserva | title | +0.500 | 0.000 | 0.000 | +0.500 |
| pt-stale-desconto-aniversario | title | +0.500 | 0.000 | 0.000 | +0.500 |
| pt-stale-jornada-remota | title | +0.500 | 0.000 | 0.000 | +0.500 |
| en-stale-password-expiry | title | +0.500 | 0.000 | 0.000 | +0.500 |
| en-stale-office-capacity | title | +0.500 | 0.000 | 0.000 | +0.500 |
| en-stale-shipping-cutoff | lexical | +1.000 | +1.000 | 0.000 | +0.500 |
| en-stale-training-allowance | title | +0.500 | 0.000 | 0.000 | +0.500 |
| en-stale-meeting-length | title | +1.000 | 0.000 | 0.000 | +1.000 |
| en-stale-invoice-discount | title | +0.500 | 0.000 | 0.000 | +0.500 |
| en-stale-badge-access-doors | title | +0.500 | 0.000 | 0.000 | +0.500 |

## Design decisions the reviewer needs

1. **Meeting-scope baseline path (focused harness change).** The prior
   harness ran `ScopeKind::Meeting` through the single-meeting FTS filter,
   which pinned retrieval to the target and made Recall@1 free and
   falsifiability unreachable for the 15 PT semantic cases — exactly the
   defect class 1.2R bans. The Meeting arm now runs the same AND→OR ranked
   search restricted to `scope.allowed_meeting_ids` (production
   `FtsRepository` calls unchanged; Snapshot/Today hydration untouched).
   This is what lets meeting scope permit several meetings with earned
   Recall@1, per the task's explicit requirement.
2. **Strict answer-key/supervised separation.** The structural check is now
   genuinely answer-key-free: it counts raw verbatim/all-term candidates and
   rejects clone walls (limit 2), plus scope-schema contracts — it never reads
   `expected_meeting_ids`/`required_evidence_ids`. The architecture's
   target-aware exception ("unless the expected evidence also contains them")
   moved into the supervised check, where IDs label targets, raw text decides
   containment and coverage, and a covering distractor without equivalent
   target coverage fails with a `[SUPERVISED:coverage]` message; margin and
   distinctness assertions live in the same supervised test
   (`corpus_supervised_labels_margin_coverage_and_distinctness_hold`), whose
   distinctness key hashes only normalized question + required target evidence
   text with numeric tokens collapsed. Failure messages carry
   `[SUPERVISED:…]` prefixes so the two layers are never confused.
3. **Title-channel solvability.** Titles are not FTS-indexed, so giving
   target meetings topical titles (and stripping exclusive query tokens from
   rival titles) supplies the margin check's discriminating channel without
   changing any baseline outcome. This mirrors real corpora, where a meeting
   about a topic is titled by that topic.
4. **EN semantic parity scaffolds moved Folder→All.** With only three
   in-folder competitors, Folder-scope semantic cases could never satisfy
   rank>3. All scope lets all three echoing rivals outrank the zero-overlap
   paraphrase target (rank ≥4 or absence). `scope_folder` remains exactly at
   its floor (15) via the other families; verified by the printed category
   counts.
5. **Snippet-loss reconstruction (nps-detrator).** The callback commitment
   lives beyond the 48-token snippet window / 400-character hydration slice,
   reproducing the recorded production failure "present in storage but not in
   the snippet shown to the model" while the meeting itself stays findable.
6. **Privacy-audit false positive fixed by rename**, not by weakening the
   marker list: case id `en-dirty-hotdesk-return` contained the substring
   `sk-`; renamed to `en-dirty-deskpool-release`.
7. **Scope schema contract gives `scope.meeting_id` a real job.** Meeting
   scope must focus (`meeting_id`) a meeting that is inside its permitted set
   of at least two; Folder scope requires `folder_id` and forbids
   `meeting_id`; All/Snapshot/Today forbid both selectors. This removes the
   "never read" warning by validation, not by an allow attribute.
8. **Diagnostics:** the structural check reports all failing cases per run;
   under-served failures include best_rank/evidence/facts and the retrieved
   order; the baseline report prints before the deterministic-snapshot assert.
9. **Gates untouched.** The baseline naturally FAILs the future hybrid gates
   (Critical Recall@1 2/5, Overall Recall@3 96/135 < 95%, Evidence Recall@10
   181/209 < 90%) — those are informational until Task 1.3 reruns; per the
   task's non-goals no threshold was changed. One observation for review,
   reported without action: the semantic delta gate (+10 points over 0/30)
   and the 95% overall Recall@3 default will be decided by the 1.3 rerun on
   this corpus.

## Verification (all from `upstream/`)

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation -- --nocapture
# ok. 5 passed; 0 failed  (determinism double-run, floors/schema, private-safe,
# pinned reference/semantic/exact expectations, answer-key-free structural
# solvency, supervised margin/coverage/distinctness, three mutation classes)
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check            # clean
git diff --check                                                             # clean
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test model_benchmark
# ok. 7 passed; 0 failed (shared fixture files still compile/pass for 1.3)
```

Focused privacy scan. Canonical command (rg):

```text
rg -n -i "(c:\\users\\|/users/|@gmail\.|@outlook\.|sk-[a-z0-9]{4}|api_key|bearer |onedrive\\)" frontend/src-tauri/tests docs/hybrid-rag/task-1.2r-corpus.md
```

The worker's PowerShell equivalent and the main-agent focused scan over
`frontend/src-tauri/tests` plus this report found only benign matches —

- `retrieval_evaluation.rs:601-607`: the harness's own `forbidden_markers`
  scanner literals (`"/users/"`, `"@gmail."`, `"@outlook."`, `"api_key"`,
  `"bearer "`, `"oneDrive\\"`) — the privacy audit itself;
- `model_benchmark.rs:3104,3184`: the Task 1.3 decision literal
  `"blocked-risk-approval"` matched by the `sk-[a-z0-9]{4}` arm ("sk-risk");
- this report's scanner command/literal documentation and the
  `en-dirty-hotdesk-return` → `en-dirty-deskpool-release` rename.

No private transcript text, identifiers, keys, paths, or secrets in any
fixture or report.

## Files changed

- `frontend/src-tauri/tests/retrieval_evaluation.rs`
- `frontend/src-tauri/tests/fixtures/corpus.rs`
- `frontend/src-tauri/tests/fixtures/corpus/reference_a.rs`
- `frontend/src-tauri/tests/fixtures/corpus/reference_b.rs`
- `frontend/src-tauri/tests/fixtures/corpus/reference_c.rs`
- `frontend/src-tauri/tests/fixtures/corpus/reference_d.rs`
- `frontend/src-tauri/tests/fixtures/corpus/semantic_pt.rs`
- `frontend/src-tauri/tests/fixtures/corpus/semantic_en.rs`
- `frontend/src-tauri/tests/fixtures/corpus/multi.rs`
- `frontend/src-tauri/tests/fixtures/corpus/states_dirty.rs`
- `frontend/src-tauri/tests/fixtures/corpus/states_stale.rs`
- `frontend/src-tauri/tests/fixtures/evaluation_policy.json`
- `frontend/src-tauri/tests/fixtures/README.md`
- `docs/hybrid-rag/task-1.2r-corpus.md` (this report)
- formatting-only (`rustfmt`, no behavior change):
  `tests/model_benchmark.rs`, `tests/fixtures/corpus_types.rs`,
  `tests/fixtures/corpus/{follow_up,multi,states_deleted}.rs`

## Omissions / notes for the record

- Per the dispatch constraints this report does not edit
  `sprint-1-quality-gates.md` or `notes-chat-improvement-execution.md`;
  transcribe the Status/Implemented/Verification/Rollback block above into
  the sprint document's Task Execution Log (that document is authoritative;
  the 1.2 entry already recorded that stray worker entries in the Notes/Chat
  execution record get removed).
- The prior partial attempt left no git history to roll back to (`tests/` is
  entirely untracked). **Rollback:** restore the pre-1.2R fixture/harness
  content from the previous session's artifacts and revert the JSON baseline
  to the void 1.2 figures; test tooling only, no production effect, no data
  repair. Nothing here affects production runtime, schema, IPC, or persisted
  sources.
- Deterministic-snapshot caveat carried over from 1.2: BM25 tie order can in
  principle vary across SQLite builds; the double-run determinism assertion
  pins behaviour on the verification machine.
