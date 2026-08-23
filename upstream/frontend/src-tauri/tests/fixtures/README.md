# Retrieval evaluation fixtures

Run the deterministic FTS baseline and print all numeric metrics, denominators,
category counts, per-case statuses, lexical policy, and observational latency
hooks from the repository root:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test retrieval_evaluation -- --nocapture
```

`corpus.rs` assembles 120 hand-authored, private-safe synthetic cases from the
family modules under `fixtures/corpus/`. Questions, target evidence, wrong
facts, and distractors are literal material per case; builders handle schema
only. Categories intentionally overlap: source kind and scope are orthogonal to
lookup mode, and state cases also protect exact retrieval. No fixture was
copied from a user database. The WhatsApp reference case keeps only the
reported factual shape (days 1, 3, 7, 10, and 15 plus distinct MPV/non-MPV
day-one actions) and uses synthetic IDs, titles, folders, and prose.

Two solvability invariants are asserted by the harness, with a strict
answer-key/supervised separation:

- `corpus_structural_solvency_invariants_hold_without_the_answer_key` runs
  `validate_structural_solvency`, which reads only question, scope-schema, and
  meeting raw text: duplicate questions, distinct titles/dates, scope field
  contracts (including Meeting scope focusing inside a permitted set of at
  least two meetings), folder scopes that exclude nothing, semantic nonce
  discriminators, and raw candidate counts that reject verbatim-question and
  all-content-term clone walls. It never reads expected/required IDs.
- `corpus_supervised_labels_margin_coverage_and_distinctness_hold` is where
  expected IDs appear, as labels only: margins must be positive per case on
  lexical/concept/title channels computed from fixture text; a verbatim or
  query-superset distractor is rejected unless the labeled target raw text has
  equivalent coverage; and distinct shapes are counted over normalized
  question plus required target evidence text (numeric tokens collapsed), with
  the >=96/120 floor asserted here.

`evaluation_policy.json` pins the current FTS baseline, numeric gates, the
candidate lexical core-term policy, Portuguese/English high-frequency lists,
and the exact deterministic baseline snapshot recorded for the re-authored
corpus (superseding the void Task 1.2 figures). Wall-clock latency samples are
reported but deliberately excluded from that snapshot.

Citation/source precision uses the production generic context builder with a
fixed 1,200-character evaluation budget. Sources are identified by the same
JSON `(meeting_id, source_kind, chunk_id)` tuple used by production and are
scored only against identities the builder reports as retained.
