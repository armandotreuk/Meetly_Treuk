# Task 1.R3a - Bounded Journal Publication Correctness

**Status:** Complete
**Owner:** implementation subagent (`ox-alpha`)
**Completed:** 2026-08-25
**Scope:** Sprint 1 post-remediation review item `1.R3a` only - correct the
vector benchmark harness's journal publication semantics and revalidate the
release evidence. No ANN, no application runtime/schema/migration/dependency/
model/package changes; no normative document edits.

## Root cause

`publish_pending` in `frontend/src-tauri/tests/vector_backend_benchmark.rs`
had two defects, confirmed by the 2026-08-25 Sprint review that reopened
Sprint 1:

1. **Unbounded publication race.** The pending journal read had no captured
   upper bound, and the durable finalize set
   `published_change_id = canonical_change_id`. A change committed after the
   publisher read its rows but before the finalize statement executed was
   therefore marked published while never having been applied to the
   in-memory overlay - a permanently lost update.
2. **Payload joined from current state.** Entries were read with a
   `LEFT JOIN bench_documents`, so a pending `upsert` whose document was
   subsequently deleted joined to a NULL vector and panicked inside
   `row.get::<Vec<u8>>`; had the NULL been swallowed instead, the upsert
   would have been silently skipped. The join also handed back whatever the
   row currently holds, which is not necessarily the payload produced by the
   entry being replayed.

A first repair bounded publication but still derived payloads from current
rows, so it had to fail closed (panic) when a document received a valid
concurrent commit past the captured bound. Review rejected that: valid
repeated-document/concurrent sequences must publish correctly. The accepted
design stores the payload with the entry itself.

## Fix (journal-carried payloads, one bounded replay path)

The benchmark-local mirror of the architecture's publication journal now
carries immutable upsert payloads, and replay reads only from it:

- `bench_index_changes` gains nullable `vector`, `dequant_scale`,
  `meeting_id` columns plus a `CHECK (operation = 'delete' OR payload IS
  NOT NULL)` invariant. Nullable payload columns on deletes mirror the
  approved production schema's nullable `source_revision`. This is a
  benchmark-mirror-only change: it lives entirely inside this test file's
  in-memory-per-run SQLite fixture and changes no application schema,
  migration, or runtime code. It mirrors the production journaling
  requirement - worker step 12 appends the journal entry in the same
  transaction that replaces the documents - by making each entry
  self-contained instead of dependent on later row state.
- `commit_updates` writes the vector bytes, dequantization scale, and
  meeting identity into every upsert journal entry atomically with the
  document replacement; delete entries carry no payload.
- `publish_pending` captures `canonical_change_id` as an upper bound FIRST,
  then delegates to `publish_through(db, overlay, bound)`.
- `publish_through` applies entries strictly through `(published, bound]`
  read from ONE consistent SQLite snapshot of the journal alone. Only the
  last bounded operation per doc is materialized (last-writer-wins): a
  trailing delete tombstones without touching any payload; a trailing upsert
  replays its own journal payload verbatim.
- `published_change_id` advances to the last applied change ID only
  (no-op when nothing applied); it never references `canonical_change_id`.
- The current-row JOIN and the `latest_per_doc` fail-closed workaround are
  removed: a document whose history continues past the bound simply publishes
  its captured payload; the newer commit stays unpublished until a subsequent
  pass.

All callers of `publish_pending` (crash-replay tests, update / delta /
crash-window / envelope phases) inherit the fix unchanged. The sparse-ID
repair (`base_scan_mask` per-row identity, `row_of_doc`) and the combined
envelope measurement are untouched and re-verified below.

## Files changed

- `frontend/src-tauri/tests/vector_backend_benchmark.rs` - benchmark-journal
  payload columns + CHECK, payload-writing `commit_updates`, bounded
  snapshot replay (`canonical_change_id`, `publish_through`, new
  `publish_pending`), module documentation, three deterministic regressions.
- `docs/hybrid-rag/task-1.r3a-journal-publication.md` - this report.

No other file changed; all pre-existing uncommitted work (Tasks
1.4/1.5/R1/R1a/R2/R3, main-agent records) preserved untouched. No commits.

## Regression evidence (deterministic, artifact-independent)

1. `concurrent_commit_stays_unpublished_until_subsequent_publication` -
   batch 2 commits after the bound was captured and rewrites the SAME
   documents as batch 1 (newer upserts for docs 40-55) AND deletes doc 45,
   whose upsert is inside the bound. `publish_through(bound)` must - and
   does, without panicking - apply exactly the captured batch-1 payloads
   (doc 45 included), leave `published == bound < canonical`, and surface
   zero newer payloads/tombstones; the subsequent `publish_pending` applies
   the newer payloads, tombstones doc 45, and reaches `canonical ==
   published`. Expectations are built directly from `generate_row`
   constants and finally cross-checked byte-for-byte against freshly
   reloaded canonical storage (299 docs; doc 45 absent) - no shared helper
   with the publishing path.
2. `upsert_then_delete_publishes_tombstone_without_payload` - both variants
   (delete in a later commit; delete in the same commit as the upsert)
   publish without panicking, end tombstoned with `canonical == published`;
   reloaded storage lacks the documents and scans never return them.
3. `repeated_upserts_publish_final_canonical_vector` - three successive
   unpublished upserts settle on the FINAL journal payload: exact overlay
   equality plus independent hand-computed scoring
   (`dot_i8(v3,q)*s3*qs`) proving returned scores use the final vector.
4. Pre-existing `journal_replay_recovers_after_simulated_crash` and the 1.R3
   `sparse_doc_ids_survive_delete_compaction_and_crash_replay` pass unchanged
   through the new path (crash/reopen/replay correctness retained).

**Mutation proof:** temporarily restoring the old finalize
(`SET published_change_id = canonical_change_id`) fails regression 1
immediately (`published=32, expected 16`: "concurrently committed entries
were marked published by a pass bounded below them"); restored fix passes
13/13. Payload dependence on current rows is now structurally impossible:
the CHECK constraint rejects payload-less upsert entries at write time.

## Full staged 250k matrix (corrected state, release build)

Canonical command (from `upstream/`):

```powershell
$env:CARGO_TARGET_DIR = "$env:LOCALAPPDATA\meetily-cargo-target"
$env:MEETLY_RAG_VECTOR_BENCH = "1"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test vector_backend_benchmark full_matrix_benchmark -- --nocapture
Remove-Item Env:MEETLY_RAG_VECTOR_BENCH -ErrorAction SilentlyContinue
```

`test result: ok. 1 passed` in 106.70 s (2026-08-25, Windows x64, 20 logical
CPUs). Verbatim key lines at n=250000:

```text
[cold-load] 899 ms for 250000 docs from canonical SQLite (183.1 MiB vector payload)
[warm-global] p50 48.1 ms / p95 65.1 ms / max 67.9 ms (192 samples) -> PASS (< 500 ms gate)
[warm-folder-quarter] p50 9.0 ms / p95 16.3 ms -> PASS ; [warm-snapshot-5] p95 0.3 ms -> PASS
[scope] narrow scopes returned zero out-of-scope documents across 64 queries x 3 rounds x 2 scopes
[recall] recall@150=1.0000 exact-by-construction; verified identical to brute force on 8/64 sampled queries
[concurrency] 2 scanners x 96 queries: per-query p50 44.3 ms / p95 59.9 ms (PASS) , wall 4423 ms, max_active 2
[update] 128-doc batch: sqlite commit 12.6 ms, replay+apply 0.96 ms, durable published-id commit 0.1 ms; overlay 97.5 KiB; base digest unchanged: true
[compaction] rebuilt base of 250000 docs (from 250000 + 128 delta) in 237 ms
[warm-delta-0.005/0.01/0.02] p95 63.7 / 62.2 / 61.4 ms -> PASS (overlay to 3903.0 KiB)
[crash-window] batches 0-3: sqlite commit 1.0-3.7 ms, publication completed 0.54-11.47 ms later under concurrent scans
[worker-impact] pause-on-interactive p95 63.2 ms, interactive pause observed in 2 ms (budget 250 ms)
[disk-shadow] two retained generations measured at this scale: 0.40 GiB vs 3 GiB rebuild-peak envelope
```

Combined envelope (same-process measurement, staged bundle consumed
read-only):

```text
[envelope-sessions] e5-base dynamic-int8 + mmarco quint8_avx2 resident ... embedding +551.9 MiB, reranker +376.8 MiB, both +928.8 MiB over process base
[envelope-parts] active snapshot 250000 docs (183.1 MiB), delta+tombstones 4094.3 KiB
[envelope-transient] shadow (249996 docs, 183.1 MiB) streamed in 1279 ms ... exactly two snapshots held
[envelope-peak] measured process peak working set 1319.9 MiB (current 1319.9 MiB; private commit 1318.0 MiB current / peak)
[envelope-verdict] PASS inside user-approved 1.30 GiB transient ceiling (exactly active+shadow)
[envelope-steady] measured working set after shadow release: 1136.8 MiB -> PASS inside approved 1-1.25 GiB band
[rss] process peak working set at n=250000: 1319.9 MiB
```

### Resource comparison (caps unchanged)

| Figure | 1.R3 (pre-fix) | 1.R3a final run | Verdict |
|---|---|---|---|
| Transient two-snapshot peak | 1316.3 / 1317.9 MiB | **1319.9 MiB** | PASS, ~11.3 MiB under the 1.30 GiB ceiling |
| Steady state after shadow release | 1133.2 MiB | **1136.8 MiB** | PASS inside 1-1.25 GiB band |
| Warm-global p95 @250k | 51.1 / 61.1 ms | **65.1 ms** | PASS (< 500 ms gate) |
| recall@150 | 1.0000 | **1.0000** | unchanged |
| Derived disk, two retained generations @250k | 0.38 GiB | **0.40 GiB** | PASS (3 GiB rebuild-peak envelope); growth is the journaled upsert payloads |

Journaling the payload adds ~2 bytes/doc-equivalent of disk at the measured
update volume and shifts the process peak by ~3 MiB (within observed
run-to-run variance). Exact search stays selected; ANN remains unevaluated
(no trigger). No cap or verdict changes.

### Fail-closed staged-bundle proof

`MEETLY_RAG_BUNDLE_DIR=Z:\definitely-not-a-bundle` gated run fails instantly
before any measurement:

```text
staged retrieval bundle not found at Z:\definitely-not-a-bundle; set MEETLY_RAG_BUNDLE_DIR ...
test result: FAILED. finished in 0.00s
```

## Verification summary (all executed 2026-08-25, Windows x64)

| Command (from `upstream/`) | Result |
|---|---|
| `cargo test --manifest-path frontend/src-tauri/Cargo.toml --test vector_backend_benchmark` | PASS - 13/13 deterministic incl. extended concurrency regression |
| Mutation check: old `published = canonical` finalize temporarily restored | Regression 1 FAILS as designed (`32 != 16`); fix restored, suite green |
| Release full matrix + combined envelope with staged bundle | PASS - `ok. 1 passed` in 106.70 s; outputs quoted above |
| `MEETLY_RAG_BUNDLE_DIR=<nonexistent>` gated run (rebuilt binary) | FAILS fast before any measurement (fail-closed proven) |
| `cargo check --manifest-path frontend/src-tauri/Cargo.toml` | PASS |
| `cargo fmt --manifest-path frontend/src-tauri/Cargo.toml --check` (after `fmt`) | PASS |
| `git diff --check` | PASS (pre-existing CRLF warnings on untouched files only) |

TypeScript/frontend sources are untouched (Rust test target + docs only);
no typecheck/Vitest impact exists to explain beyond that scope boundary.

## No-goals

- No ANN evaluation or dependency (latency passes; RAM passed; ANN is never a
  RAM remedy under the Backend Decision Rule).
- No application runtime/schema/migration changes: only the benchmark file's
  own per-run SQLite fixture gained journal payload columns, mirroring the
  production journaling duty without touching any shipped schema.
- No model/package/provenance/workflow/corpus/gate changes; caps and verdicts
  unchanged; no normative documents edited (register/status entries remain
  the orchestrator's, consistent with Task 1.R3 precedent).
- No real-threaded concurrency test: the race is reproduced deterministically
  via captured-bound interleaving.

## Rollback

Both files are untracked working-tree state: restore
`frontend/src-tauri/tests/vector_backend_benchmark.rs` from the pre-1.R3a
copy and delete this report. Nothing else changed; no persisted data,
dependency, schema, or runtime effect. The staged bundle and cache are owned
by Tasks 1.5/R1/R2 and untouched.

## Blockers / risks

None blocking. Residual risks stated openly:

- The benchmark journal now duplicates update payloads for the lifetime of
  each run's database (bounded by the harness's update volume; measured
  effect +0.02 GiB on two retained generations at 250k). Production journal
  design (payload vs source_revision as-of joins) is a Sprint 2 decision;
  the architecture schema's nullable `source_revision` already anticipates
  revision-keyed joins.
- Transient margin at 250k remains ~11 MiB (~0.9%) above the measured peak;
  the harness asserts and fails loudly as `[blocked-resource-envelope]` if
  exceeded. Permitted levers remain memory-mapping or bounding rebuild-time
  session residency; ANN is not a lever.
