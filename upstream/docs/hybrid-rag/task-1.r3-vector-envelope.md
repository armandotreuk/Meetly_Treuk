# Task 1.R3 - Exact-Vector Sparse-ID Repair and Measured Transient Envelope

**Status:** Complete
**Owner:** implementation subagent (`ox-alpha`)
**Completed:** 2026-08-25
**Scope:** Sprint 1 review remediation `1.R3` only - repair the exact
benchmark's document-ID-to-row handling so every caller is correct after
deletes, compaction, SQLite reload, and journal replay; encode the distinct
approved steady-state (1.25 GiB band) and user-approved transient (1.30 GiB,
exactly active+shadow) ceilings; replace the raw-payload arithmetic proxy with
a real same-process 250k measurement of active snapshot + building shadow +
delta/tombstones + both selected ONNX sessions resident. No work on other
tasks, no runtime retrieval behavior, no ANN, no model/package/provenance
changes beyond consuming the already-staged bundle for measurement.

## Review findings remediated

1. **Sparse-ID crash/compaction proof defect (code review, should-fix).**
   `base_scan_mask` wrote overlay keys - which are stable *document IDs* -
   into a mask indexed by *row position*. That is only correct while
   `doc_id == row`; compaction and SQLite reload preserve sparse IDs while
   shifting row positions, so the old code masked unrelated live rows and
   left shadowed/tombstoned rows scorable. The crash test compared two
   overlays through the same flawed helper, so the mistake was invisible.
   The same ID-vs-row confusion existed in `verify_no_leak`
   (`meeting_of_doc[doc as usize]`).
2. **Superseded transient verdict encoded in the harness (code review,
   should-fix).** The combined-RAM block labeled any figure above 1.25 GiB
   as `FAIL`, contradicting the approved 2026-08-24 contract that permits
   exactly the two-snapshot rebuild state up to 1.30 GiB.
3. **Rebuild peak was arithmetic, not a measured simultaneous state (both
   reviews, blocker).** The harness loaded no model sessions, observed a
   ~700 MiB process peak without them, then added a retained Task 1.3
   session figure to raw vector payload sizes and called the sum a peak;
   SQLite row/BLOB transients and snapshot metadata/capacity were absent
   from the approval arithmetic.

## Root-cause fix (one shared repair)

- `base_scan_mask` now decides per **row**, keyed on that row's own
  `snap.doc_ids[row]` identity against the overlay's tombstone/upsert sets -
  never by indexing with a document ID. Correct for dense and sparse IDs
  alike, at every call site (warm phases, delta-penalty phases, crash-window
  replay, compaction comparisons).
- Added `Snapshot::row_of_doc` / `Snapshot::meeting_of_doc_id`
  (binary search over the sorted `doc_ids`) as the single ID-to-row
  translation; `verify_no_leak` now resolves returned documents' meetings
  through it instead of `meeting_of_doc[doc as usize]`.
- Every caller of the touched helpers was grepped and inherits the fix; no
  caller outside this test target exists.

## Independent sparse-ID regression

New deterministic test `sparse_doc_ids_survive_delete_compaction_and_crash_replay`
(runs without artifacts or env vars):

- Inserts 600 docs; commits three canonical batches (24 upserts on docs
  100-123, deletes of docs 50-52 and 400-402) whose publication never runs;
  closes and reopens the database; replays the journal into a fresh overlay
  over the reloaded snapshot whose IDs no longer equal row positions.
- Builds a canonical expected-document map from the journal semantics and
  scores it directly (`dot_i8` + sort + truncate) - the expected side shares
  no mask/index helper with the implementation under test.
- Asserts post-replay top-150 equality for 12 queries, then compacts and
  asserts stable identities (deleted docs absent, docs 53/54/55/304/123
  present), doc-id ordering, count exactness, and post-compaction result
  equality against the same canonical expectation; finally proves narrow
  scope over the sparse rebuilt snapshot leaks nothing via the fixed meeting
  lookup.

**Mutation proof:** temporarily restoring the old ID-indexed mask makes the
test fail immediately - the replayed results then contain a duplicate doc 101
(stale base row scored alongside its overlay replacement) and lose live docs
(125, 126). Restored fix passes.

## Encoded limits (approved contract)

```text
steady state    <= 1 GiB      PASS automatic
                <= 1.25 GiB   PASS inside approved 1-1.25 GiB e5-base band
                >  1.25 GiB   FAIL (asserted)
transient       <= 1 GiB      PASS automatic
                <= 1.30 GiB   PASS inside user-approved 2026-08-24 ceiling,
                              valid ONLY for exactly active+shadow two snapshots
                >  1.30 GiB   FAIL -> [blocked-resource-envelope] (asserted)
```

A true third resident snapshot or any peak above 1.30 GiB remains blocking;
the transient ceiling is not a steady-state band. Both verdicts are asserted
in-process, so an over-ceiling run fails the command instead of printing a
contradictory label. An approved sub-ceiling transient peak is reported as
PASS, fixing the review's "approved state labeled failure" finding.

## Same-process 250k envelope measurement (replaces arithmetic proxy)

`combined_envelope_measurement` runs LAST in the gated release matrix so the
monotonic Windows process peak counters attribute their maximum to the state
that dominates it:

1. Loads BOTH selected ONNX sessions from the staged production bundle -
   e5-base dynamic-int8 (`models/embedding/model_int8.onnx`) and mmarco
   quint8_avx2 (`models/reranker/model_quint8_avx2.onnx`) - each with its
   bundled tokenizer, using the production-shaped session pattern from the
   Task 1.3 reference harness (CPU EP, Level3 optimization, intra-op 4).
   Sessions stay alive through the entire peak window.
2. Warms each session once (batch-1 x 512 synthetic ids, deterministic) so
   ORT arena/workspace residency is materialized before the peak window.
3. Holds the reader-held active snapshot (250,000 docs) plus the live
   delta/tombstone overlay (4,094.3 KiB after the delta/crash phases), then
   builds the shadow snapshot with a new production-shaped streaming loader
   (`load_snapshot_streaming`: exact-capacity reservation from `COUNT(*)`,
   bounded 4,096-row chunks) instead of one `fetch_all`, so only the real
   snapshot capacity plus a bounded per-chunk SQLite transient is carried.
4. Reads `K32GetProcessMemoryInfo`: peak/current working set governs (same
   metric family as the retained Task 1.3 evidence); private commit is
   recorded alongside because working sets can be trimmed under OS pressure
   while commit charge tracks actual charges.
5. Drops the shadow and reads the post-activation steady-state residency.

Fail-closed behavior: the gated run validates the staged bundle BEFORE any
measurement (`MEETLY_RAG_BUNDLE_DIR`, default
`resources/retrieval/bundle`). Absent directory fails instantly; a corrupt/
misconfigured artifact fails at session load - both panic with
`[blocked-resource-envelope]` messages naming the env var or artifact. The
deterministic suite remains fully artifact-independent.

## Canonical command and output (2026-08-25, Windows x64, 20 logical CPUs)

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "meetily-cargo-target"
$env:MEETLY_RAG_VECTOR_BENCH = "1"
cargo test --release --manifest-path "frontend/src-tauri/Cargo.toml" --test vector_backend_benchmark full_matrix_benchmark -- --nocapture
Remove-Item Env:MEETLY_RAG_VECTOR_BENCH -ErrorAction SilentlyContinue
```

`test result: ok. 1 passed` in 90.21 s. Verbatim envelope lines:

```text
=== Combined 250k rebuild envelope (same-process measurement) ===
[envelope-sessions] e5-base dynamic-int8 + mmarco quint8_avx2 resident (sessions + bundled tokenizers, arena warmed) from ...\resources\retrieval\bundle: embedding +549.5 MiB, reranker +377.6 MiB, both +927.1 MiB over process base
[envelope-parts] active snapshot 250000 docs (183.1 MiB), delta+tombstones 4094.3 KiB
[envelope-transient] shadow (249996 docs, 183.1 MiB) streamed in 619 ms while active+delta+both sessions stayed resident; exactly two snapshots held (active 250000 docs + shadow 249996 docs)
[envelope-peak] measured process peak working set 1316.3 MiB (current 1316.3 MiB; private commit 1313.7 MiB current / 1313.7 MiB peak)
[envelope-verdict] transient two-snapshot rebuild peak vs limits -> PASS inside user-approved 1.30 GiB transient ceiling (exactly active+shadow)
[envelope-steady] measured working set after shadow release: 1133.2 MiB (private commit 1130.2 MiB) -> PASS inside approved 1-1.25 GiB e5-base band
[disk-shadow] two retained generations measured at this scale: 0.38 GiB vs 3 GiB rebuild-peak envelope
[rss] process peak working set at n=250000: 1316.3 MiB
```

### 250k component accounting and cap verdict

| Component | Measured |
|---|---|
| Model sessions + bundled tokenizers (arena warmed) | +927.1 MiB over process base (embedding +549.5, reranker +377.6) |
| Active snapshot | 250,000 docs / 183.1 MiB vectors |
| Building shadow snapshot | 249,996 docs / 183.1 MiB vectors (post-crash-window canonical state) |
| Delta + tombstones | 4,094.3 KiB |
| Snapshot metadata/capacity + SQLite transients | included in the process counters (streaming keeps per-chunk transient bounded) |
| **Measured combined transient peak** | **1316.3 MiB (current == peak; private commit 1313.7)** |
| **Transient cap verdict** | **PASS - inside the user-approved 1.30 GiB (1331.2 MiB) ceiling, margin ~14.9 MiB** |
| **Measured steady state (shadow released)** | **1133.2 MiB -> PASS inside approved 1-1.25 GiB band** |

Prior evidence cross-checks: measured session residency 927.1 MiB reproduces
the retained Task 1.3 pair figure (926.3 MiB) within 0.8 MiB, proving the
same measurement basis; measured steady state 1133.2 MiB corroborates the
Task 1.4 arithmetic (1113.4 MiB) and the frozen Task 1.3 projection
(1118.3 MiB). The measured true peak (1316.3 MiB) sits close to the prior
arithmetic estimate (1296.5 MiB) and inside the approved ceiling - the
arithmetic was honest but is no longer the evidence.

Preserved latency/recall evidence at 250k (exact search stays selected; ANN
remains forbidden - RAM did not miss, so ANN is not a permitted remedy under
the Backend Decision Rule):

```text
[cold-load] 536 ms for 250000 docs from canonical SQLite (183.1 MiB vector payload)
[warm-global] p50 41.2 ms / p95 51.1 ms / max 53.2 ms (192 samples) -> PASS (< 500 ms gate)
[warm-folder-quarter]/[warm-snapshot-5] pass; zero out-of-scope documents across 64 queries x 3 rounds x 2 scopes
[recall] recall@150=1.0000 exact-by-construction; identical to brute force on 8/64 sampled queries
[concurrency] 2 scanners x 96 queries: p50 39.8 / p95 44.9 ms, max_active 2
[update] base digest unchanged: true (updates never copy the base)
[compaction] rebuilt 250,000-doc base in 166 ms; delta fractions 0.5/1/2% apply in 153/196/249 ms
[crash-window] publication completed 0.39-0.52 ms after each canonical commit under concurrent scans
[worker-impact] pause-on-interactive p95 52.9 ms, interactive pause observed in 2 ms (budget 250 ms)
[disk-shadow] 0.38 GiB vs 3 GiB rebuild-peak envelope
```

## Sparse-ID proof summary

- New regression compares replay and compaction results against canonical
  expected documents scored independently (no shared index helper).
- Mutation check: reintroducing the ID-indexed mask fails the test with a
  visible duplicate-doc/stale-row signature; the fix restores equality.
- Existing tests (`journal_replay_recovers_after_simulated_crash`,
  `overlay_tombstone_semantics_match_compacted_rebuild`,
  `scope_allow_list_never_leaks_out_of_scope`,
  `update_path_never_touches_base_snapshot`,
  `exact_scan_matches_brute_force_recall`) all still pass through the single
  repaired helper.

## Model-session residency proof

`[envelope-sessions]` measures in-process deltas around each load: embedding
+549.5 MiB and reranker +377.6 MiB (sessions + bundled tokenizers, arena
warmed at batch-1 x 512). Both sessions remain alive across the streamed
shadow build; `[envelope-transient]` states "active+delta+both sessions
stayed resident". The 927.1 MiB total matches the retained Task 1.3
measurement basis within 0.8 MiB. Session input/output names are validated
against the approved tensor contract at load time; anything else fails
closed.

## Files changed

- `frontend/src-tauri/tests/vector_backend_benchmark.rs` - sparse-ID root
  fix + helpers, independent sparse-ID regression, streaming shadow loader,
  full process-memory counters, staged-bundle session loader (fail-closed),
  measured combined envelope with asserted steady/transient ceilings,
  removal of the retained-figure arithmetic block and superseded 1.25 GiB
  transient label, module documentation update.
- `docs/hybrid-rag/task-1.r3-vector-envelope.md` - this report.

No production code, schema/migration, dependency, manifest, package,
workflow, corpus/gate, or model-artifact change. All pre-existing uncommitted
Task 1.4/1.5/R1/R2/main-agent changes preserved untouched.

## Verification (all executed 2026-08-25, Windows x64)

| Command (from `upstream/`) | Result |
|---|---|
| `$env:CARGO_TARGET_DIR=<LOCALAPPDATA>\meetily-cargo-target; cargo test ... --test vector_backend_benchmark` | PASS - 10/10 deterministic tests incl. the new sparse-ID regression (artifact-independent) |
| Mutation check: old ID-indexed mask restored temporarily | FAILS the new regression as designed (duplicate doc 101, lost live docs); fix restored, test green |
| Release full matrix + combined envelope with staged bundle | PASS - `ok. 1 passed` in 90.21 s; outputs quoted above |
| `MEETLY_RAG_BUNDLE_DIR=<nonexistent>` gated run | FAILS fast before any measurement: "staged retrieval bundle not found at ...; set MEETLY_RAG_BUNDLE_DIR ..." |
| `MEETLY_RAG_BUNDLE_DIR=<dir with bogus .onnx>` gated run | FAILS closed at session load: "[blocked-resource-envelope] staged embedding session failed to load: ..." |
| `cargo check --manifest-path frontend/src-tauri/Cargo.toml` | PASS |
| `cargo fmt --manifest-path frontend/src-tauri/Cargo.toml --check` | PASS |
| `git diff --check` | PASS (pre-existing CRLF warnings only, untouched files) |
| `pnpm --dir frontend run typecheck` | PASS |
| `npx vitest run` (frontend) | PASS - 20 files / 95 tests |

## Platform allocator caveat

Windows working-set figures can be trimmed under memory pressure, and commit
charge can retain freed heap pages briefly; the harness therefore governs on
peak working set (the metric family all retained Sprint 1 figures used) while
printing current/peak private commit as corroboration (1313.7 MiB peak commit
vs 1316.3 MiB peak WS - agreement within 0.2%). Current == peak at the
combined-holding sample confirms the lifetime maximum occurred in exactly the
required state. ORT activation arenas grow with production batch shapes; the
batch-1x512 warmup bounds them deterministically here, and the architecture
already requires Sprint 2 to re-measure limits under its real allocation
behavior.

## No-goals

- No ANN evaluation or dependency (latency passes; RAM passed; ANN is never
  a RAM remedy).
- No runtime retrieval/startup behavior, no model downloads, no schema/
  migrations, no package/provenance edits (the staged bundle is consumed
  read-only for measurement).
- No change to normative architecture/README/sprint documents or
  `docs/notes-chat-improvement-execution.md` (out of bounds for this task;
  register/status entries are the orchestrator's).
- No third-snapshot path, no cap relaxation, no silent skip when the staged
  bundle is required.

## Rollback

Restore `frontend/src-tauri/tests/vector_backend_benchmark.rs` from the
pre-R3 working-tree state (file is untracked; keep a copy before applying)
and delete this report. Nothing else changed; no persisted data, dependency,
or runtime effect. The staged bundle and cache are owned by Tasks 1.5/R1/R2
and are untouched.

## Blockers / risks

None blocking. Residual risks stated openly:

- The transient margin at the approved 250k scale is ~15 MiB (~1.1%). A
  larger reranker depth, bigger embedding batches, or allocator variance on
  other machines could push a future re-measurement above 1.30 GiB; the
  harness now asserts and would fail loudly as `blocked-resource-envelope`.
  Permitted levers remain memory-mapping the base snapshot or bounding
  rebuild-time session residency; ANN is not a lever.
- Session residency includes tokenizers parsed by the `tokenizers` crate
  (~250 MB each resident); Sprint 2 could reduce this with lazy/shared
  tokenizer loading, but that is production optimization territory, not this
  task.
- Peak-counter attribution relies on the combined phase running last in the
  matrix; reordering phases could attribute earlier spikes to the envelope
  reading (conservative direction only).
