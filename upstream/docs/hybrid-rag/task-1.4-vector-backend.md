# Task 1.4 - Vector Backend Benchmark (exact, 768-d int8)

**Status:** Complete
**Owner:** `worker-l` continuation session (`openrouter/stealth/ox-alpha`)
**Completed:** 2026-08-24
**Scope:** Benchmark exact vector search at 12k/50k/250k under the selected
production encoding (768-d int8 storage with recorded dequantization scales,
per the approved Task 1.3 bundle) and decide exact-vs-ANN by the architecture
Backend Decision Rule. Benchmark/test code plus this report only; no
production retrieval/schema/corpus/gate/model/manifest/weight changes and no
architecture/sprint/README edits.

## Backend Decision

**Ship exact search. Do not evaluate ANN.**

Under the Backend Decision Rule table (`architecture.md` "Vector Search
Backend"), the measured result at 250k is **both gates pass**:

- **Latency gate** (vector-stage p95 < 500 ms): worst observed warm-global p95
  at 250k is **59.4 ms** (max 61.6 ms) - roughly 8.4x under the gate. Every
  scope, candidate depth, concurrency, and delta-overlay phase passes.
- **RAM gate** (steady-state retrieval RAM): combined arithmetic at 250k gives
  **1113.4 MiB**, inside the user-approved 1-1.25 GiB e5-base band and
  corroborating the frozen Task 1.3 projection (1118.3 MiB).

Rule row 1 ("Both gates pass -> Ship exact search. Do not evaluate ANN.")
applies directly. No ANN dependency, sidecar, or graph code was added, and no
recall-vs-exact measurement is applicable. A pure-Rust HNSW evaluation is
untriggered: its only trigger is a latency miss with passing RAM, which did
not occur.

One envelope finding requiring an orchestrator/user ruling is recorded below
("Shadow-overlap envelope finding"); it concerns the transient shadow-rebuild
moment, not the backend choice, and ANN is not a permitted remedy for it in
any case.

## Harness

`frontend/src-tauri/tests/vector_backend_benchmark.rs` (new, untracked until
accepted). Structure:

- **Deterministic generation.** Per-row seeded XorShift64 (`row_seed(doc)`),
  unit-normalized f32 vector symmetrically quantized to int8 with a recorded
  per-vector dequantization scale; `score(q,d) = dot_i8(q,d) * q.scale *
  d.scale` approximates cosine. Generation is prefix-stable and batched
  (200-row SQLite flushes) - no unbounded allocation. Pinned by
  `generation_is_deterministic_and_unit_norm`.
- **Exact backend model.** Immutable contiguous base snapshot + BTreeMap
  upsert delta + BTreeSet tombstones (the architecture's "Exact Option").
  Pre-scan mask drops out-of-scope and delta-shadowed/tombstoned rows before
  scoring, so they cannot enter candidates. Compaction rebuilds
  base-minus-tombstones-plus-upserts preserving stable doc identities.
- **Canonical SQLite persistence mirror.** `bench_documents`,
  `bench_index_changes` journal, singleton `bench_index_state` with
  canonical/published change IDs; updates commit documents + journal +
  canonical advance in one transaction; a publisher replays pending journal
  into the overlay then durably advances published_change_id (worker steps
  8-13 / crash-window semantics).
- **Interactive scheduler.** Condvar scheduler with `SCAN_PERMITS = 2`
  concurrent scans, `QUEUE_CAP = 8` queued interactive requests, fast-fail
  rejection past the cap, and an index-worker probe honoring the
  interactive-pause rule within `INTERACTIVE_PAUSE_BUDGET_MS = 250`.

### Scheduling-probe fix applied this session

The worker-impact phase requested a pause and spin-waited for it even in the
`no-pause` arm, while the probe thread only ever sets `paused` when
`honor_pause` - a guaranteed infinite hang in the `honor == false` arm (the
previously missing `[worker-impact]` evidence). Fix at the scheduling probe,
one condition, no policy change:

```diff
-                if round == 1 && qi == queries.len() / 2 {
+                if honor && round == 1 && qi == queries.len() / 2 {
```

The downstream latency-budget assertion was already `if honor`-guarded. The
file was also passed through `cargo fmt` (formatting only; the new file had
pre-existing style drift).

## Canonical command

```powershell
$env:CARGO_TARGET_DIR = "C:\Users\arman\cargo-target"
$env:MEETLY_RAG_VECTOR_BENCH = "1"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test vector_backend_benchmark full_matrix_benchmark -- --nocapture
Remove-Item Env:MEETLY_RAG_VECTOR_BENCH -ErrorAction SilentlyContinue
```

Hardware line printed by the run: Windows x64, 20 logical CPUs, encoding=int8
(dim=768, per-vector dequant scale), top-k=150. Full matrix finished in
108.0 s; `test result: ok. 1 passed`. Raw output preserved verbatim below;
figures quoted in this report are from this final run (an earlier identical
run before the fmt pass reproduced the same conclusions: no-pause p95 55.4 /
pause-on-interactive p95 58.6, pause 3 ms).

## Measured matrix

### n = 12,000

| Phase | Result |
|---|---|
| Deterministic generation | generate 53 ms, sqlite insert 175 ms |
| Cold load | **40 ms** for 12,000 docs (8.8 MiB vector payload) |
| Warm global | p50 1.9 / p95 2.2 / max 3.2 ms (192 samples) - PASS |
| Warm folder-quarter | p50 0.5 / p95 0.6 ms - PASS |
| Warm snapshot-5 | p50 0.0 / p95 0.0 ms - PASS |
| Candidate limits | k=50: 1.9/2.2 - k=100: 1.2/1.6 - k=150: 1.2/1.7 ms |
| Recall | recall@150 = 1.0000 exact-by-construction; identical to brute force on 8/64 sampled queries |
| Concurrency (single-digit threads) | 2 scanners x 96 queries: p50 1.1 / p95 1.5 ms, wall 113 ms, max_active 2 |
| Update (delta path) | 128-doc batch: sqlite commit 5.3 ms, replay+apply 7.96 ms, durable published-id commit 0.1 ms; overlay 97.5 KiB; base digest unchanged: true |
| Compaction | rebuilt 12,000-doc base (from 12,000 + 128 delta) in 7 ms |
| Peak RSS / disk | 38.0 MiB peak working set; 9.4 MiB on disk (825 B/doc) |

### n = 50,000

| Phase | Result |
|---|---|
| Deterministic generation | generate 118 ms, sqlite insert 610 ms |
| Cold load | **106 ms** for 50,000 docs (36.6 MiB payload) |
| Warm global | p50 9.7 / p95 12.3 / max 13.0 ms - PASS |
| Warm folder-quarter | p50 2.3 / p95 3.1 ms - PASS |
| Warm snapshot-5 | p50 0.1 / p95 0.1 ms - PASS |
| Candidate limits | k=50: 10.7/12.7 - k=100: 10.7/12.5 - k=150: 10.8/12.6 ms |
| Recall | recall@150 = 1.0000; identical to brute force on 8/64 sampled queries |
| Concurrency | 2 x 96: p50 10.4 / p95 11.6 ms, wall 999 ms, max_active 2 |
| Update (delta path) | commit 8.8 ms, apply 0.93 ms, published-id commit 0.1 ms; overlay 97.5 KiB; base digest unchanged: true |
| Compaction | rebuilt 50,000-doc base in 49 ms |
| Peak RSS / disk | 122.9 MiB; 39.2 MiB (822 B/doc) |

### n = 250,000 (release-gate scale)

| Phase | Result |
|---|---|
| Deterministic generation | generate 1,137 ms, sqlite insert 4,087 ms |
| Cold load | **916 ms** for 250,000 docs (183.1 MiB payload) |
| Disk | 195.9 MiB (**821 B/doc**); projected 250k: **0.19 GiB steady / 0.38 GiB two retained generations** (envelopes 2 GiB / 3 GiB - PASS) |
| Warm global | p50 **53.1** / p95 **59.4** / max 61.6 ms (192 samples) - PASS (< 500 ms gate) |
| Warm folder-quarter | p50 12.9 / p95 15.4 ms - PASS |
| Warm snapshot-5 | p50 0.2 / p95 0.2 ms - PASS |
| Scope safety | zero out-of-scope documents across 64 queries x 3 rounds x 2 narrow scopes (+ dedicated leak tests) |
| Candidate limits | k=50: 53.5/58.4 - k=100: 52.4/58.8 - k=150: 53.1/59.1 ms (depth-insensitive; 150 candidates affordable) |
| Recall | recall@150 = 1.0000 exact-by-construction; verified identical to brute force on 8/64 sampled queries |
| Bounded concurrency | 2 scanners x 96 queries: p50 52.2 / p95 55.5 ms, wall 5,000 ms, **max_active 2** (permit bound held) |
| Update (base+delta/tombstone) | 128-doc batch: sqlite commit 11.2 ms, replay+apply 1.71 ms, durable published-id commit 0.1 ms; overlay 97.5 KiB; **base digest unchanged: true** (updates never touch/copy the base) |
| Compaction | rebuilt 250,000-doc base (from 250,000 + 128 delta) in **249 ms** |

#### Delta-size penalty toward the compaction threshold

| Delta fraction | Docs | Apply time | Overlay size | Warm-global p95 during overlay |
|---|---|---|---|---|
| 0.5% | 1,250 | 188 ms | 1,048 KiB | 58.9 ms - PASS |
| 1.0% | 2,500 | 233 ms | 2,000 KiB | 60.0 ms - PASS |
| 2.0% | 5,000 | 323 ms | 3,903 KiB | 58.3 ms - PASS |

Scan latency is insensitive to overlay size in this range; a compaction
threshold anywhere up to 2% delta (5,000 docs) is latency-safe. Measured
apply cost stays sub-second at all three fractions.

#### Reader-held old snapshot + new/shadow load

Reader-held old snapshot (250,000 docs, 183.1 MiB) while the new/shadow
snapshot loaded from canonical SQLite in 885 ms; process peak working set
701.5 MiB (model sessions are intentionally not loaded in this harness -
their retained Task 1.3 residency enters the combined arithmetic instead).

#### Crash-window semantics (canonical commit -> in-memory publication)

Four 64-upsert + 1-delete batches under a continuously scanning reader:
sqlite commit 2.0-2.3 ms each; in-memory publication completed 0.35 / 0.48 /
0.80 / 22.95 ms after the respective commits. Between commit and publication
queries keep serving the old snapshot; canonical being ahead of published is
the crash window, and restart replay closes it durably
(`journal_replay_recovers_after_simulated_crash`: reopen -> replay ->
canonical == published -> search results identical to expected overlay).
Reporting note: the harness prints the hammer thread's total completed scans
("under 1 concurrent scans" - one 50 ms scan spans the whole measurement
window), i.e. scan count completed under concurrent load, not a concurrency
level; the concurrency level is the separate always-running scanner loop.

#### Index-worker scheduling impact (previously missing evidence)

| Policy | Query p50 | Query p95 | Interactive pause observed |
|---|---|---|---|
| no-pause (busy worker ignores signal) | 47.1 ms | 56.4 ms | n/a |
| pause-on-interactive | 52.4 ms | 61.1 ms | **3 ms** (budget 250 ms) |

A busy single-owner index worker costs ~5 ms p50 / ~2 ms p95 of interactive
scan latency; honoring the interactive-pause rule removes the worker mid-run
with the pause observed in 3 ms, ~83x inside the 250 ms budget, at negligible
query-latency cost. The `honor`-guarded budget assertion passes. Focused
deterministic coverage: `index_worker_pauses_within_250ms_of_interactive_waiter`.

### Combined RAM arithmetic at 250k (retained Task 1.3 session figures)

```
[ram-parts] base snapshot 183.1 MiB, delta+tombstones 4094.3 KiB,
            sessions (Task 1.3 retained) 926.3 MiB
[ram-envelope] steady-state (active base + delta + sessions):
               1113.4 MiB -> PASS inside approved 1-1.25 GiB e5-base band
[ram-envelope] shadow overlap (reader-held old base + new base + delta + sessions):
               1296.5 MiB -> FAIL above the 1.25 GiB hard line
[rss] process peak working set at n=250000: 701.5 MiB
[disk-shadow] two retained generations measured at this scale:
              0.38 GiB vs 3 GiB rebuild-peak envelope
```

Steady state corroborates the approved Task 1.3 projection (1118.3 MiB) to
within 5 MiB. Disk is far inside both envelopes (0.19/0.38 GiB vs 2/3 GiB).

## Shadow-overlap envelope finding (escalate - do not resolve silently)

The strictest simultaneous-holding arithmetic - reader-held old base + new
shadow base + delta + both model sessions resident during a shadow
rebuild/activation moment - computes to **1296.5 MiB, 16.5 MiB (1.3%) above
the 1.25 GiB hard line**, and the harness prints FAIL for it verbatim. Facts
relevant to ruling on it:

1. It is a transient rebuild/activation state, not steady state; the
   steady-state figure (1113.4 MiB) is inside the approved band.
2. The normative peak formula counts the 2x shadow-overlap factor *and* a
   separate reader-held-old-snapshot term; if "old + new" is exactly what the
   2x factor models, the extra reader-held term double-counts one base copy
   and the honest peak is the steady-state figure. Under the strictest
   reading (three copies addressable) it exceeds the line as printed.
3. Measured process peak working set during the entire run (without loaded
   sessions) was 701.5 MiB; the sessions term is the retained Task 1.3
   measurement, not re-measured here by design (frozen selection).
4. Permitted levers are already in the architecture and none involve ANN:
   memory-mapping the base snapshot (would remove one or both base copies
   from resident RAM during rebuild), bounding rebuild-time session
   residency, or an approved envelope clarification.

This is recorded as the task's one open item for the orchestrator/user. It
does not affect the exact-vs-ANN decision: ANN adds memory and is never a
RAM remedy under the decision rule.

## Required-evidence index (Task 1.4 matrix items)

| Required item | Where measured |
|---|---|
| 12k / 50k / 250k scales | three `run_scale` phases above |
| Selected production dimension/encoding | 768-d int8 + dequant scales (approved Task 1.3 contract) |
| Cold load / warm query | `[cold-load]` / `[warm-*]` lines |
| Global all-meetings query | `[warm-global]` |
| Narrow folder/snapshot allow-list query | `[warm-folder-quarter]`, `[warm-snapshot-5]`, leak tests |
| Exact base+delta/tombstone update and compaction cost | `[update]`, `[compaction]`, delta-penalty table |
| Reader-held old + new/shadow snapshot RAM | `[shadow-overlap]` + combined arithmetic |
| Active model-session RAM | retained Task 1.3 residency (926.3 MiB) in `[ram-parts]`; not re-measured per frozen selection |
| Crash window commit->publication | `[crash-window]` + replay test |
| Single and bounded concurrent queries | sequential phases + `[concurrency]` (2 scanners, max_active 2) + queue-overflow rejection test |
| Queue/permit/concurrency/compaction/scheduling values | SCAN_PERMITS=2, QUEUE_CAP=8, pause budget 250 ms (observed 3 ms), candidate limits {50,100,150} (150 affordable, depth-insensitive), compaction safe to >=2% delta, update batch 128 |
| Peak RSS and on-disk size | `[rss]` per scale; `[disk]` per scale; 821-825 B/doc |
| Deterministic generation | seeded per-row generation + determinism/unit-norm test |
| Recall against exact NN for ANN | N/A - ANN not evaluated (rule row 1) |
| Update/rebuild cost for ANN | N/A - ANN not evaluated |

## Verification

```powershell
$env:CARGO_TARGET_DIR = "C:\Users\arman\cargo-target"
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --test vector_backend_benchmark
#   ok. 9 passed; 0 failed (incl. the gated full-matrix SKIP without the env var)
cargo test --manifest-path "frontend/src-tauri/Cargo.toml" --lib
#   ok. 394 passed; 2 ignored
cargo check --manifest-path "frontend/src-tauri/Cargo.toml"           # pass
cargo fmt --manifest-path "frontend/src-tauri/Cargo.toml" --check     # pass
git diff --check                                                      # pass
pnpm --dir frontend run typecheck                                     # pass
npx vitest run                                                        # 20 files / 95 tests passed
```

Privacy/model-weight scan over the new test file and this report: no API-key/
token/private-marker patterns; no absolute user paths (only a
`$env:LOCALAPPDATA` reference in a reproducibility doc comment); no model
artifact extensions (`.onnx`/`.bin`/weights) referenced or staged; git
carries only `.rs`/`.md` changes - no binaries, no model weights. The file
contains synthetic vectors only; no transcript/meeting text exists in it.

## Rollback

- Delete `frontend/src-tauri/tests/vector_backend_benchmark.rs` (untracked)
  and this report. Nothing else in the tree changed: tracked diff is exactly
  the pre-existing main-agent `architecture.md`/sprint/README amendments,
  untouched by this session.
- No production code, schema, dependency, manifest, or persisted-data effect;
  no model artifacts were downloaded or staged.

## Decisions and follow-ups

- Backend decision (ship exact; no ANN) must be recorded in
  `architecture.md` by a dated, user-approved addendum before Sprint 2 - the
  orchestrator's step; this session is boundary-barred from editing it.
- The shadow-overlap envelope finding above needs a ruling (accept transient,
  count the formula's terms as overlapping, or adopt memory-mapped base in
  Sprint 2).
- Recorded scheduler starting values for the Sprint 2 implementation:
  permits 2, queue 8, pause 250 ms, candidate limit 150, update batch 128,
  compaction threshold anywhere <= 2% delta (all latency-safe with large
  margins).
