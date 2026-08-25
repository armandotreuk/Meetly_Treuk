# Task 1.R2 - Package/Provenance Boundary Hardening

**Status:** Complete
**Owner:** implementation subagent (`ox-alpha`)
**Completed:** 2026-08-25
**Scope:** Review remediation only - fail the build/runtime parser closed for
the exact approved production contract, establish immutable redistribution
authority and applicable notices for both packaged models, give the staged
package exactly one signed authority with rejection of unexpected content,
repair single-publisher crash recovery, and prove all of it offline. No work on
`1.R3`, no runtime retrieval integration, no model substitution.

## Starting state

The two recorded dispatch failures left partial uncommitted `1.R2`-shaped work
in the tree alongside the retained Task 1.5/1.R1 files: a hardened Rust
validator (`model_bundle.rs`), the corrected checked-in manifest referencing a
composed MIT notice artifact, the notice file itself, and a hardened staging
script. That session treated the inherited state as input: it independently
re-verified every provenance claim against authoritative pinned upstream
sources (below), completed the missing package-authority change
(`tauri.conf.json` still packaged three duplicate retrieval resources),
republished the stale staged bundle under the corrected contract, and ran the
full verification battery.

A finishing pass on the same day closed the last unpinned byte in the signed
package — the committed `README.md` placeholder is now hash-pinned and
verified on copy and in every content check (see "Package authority") — added
the matching tampered-placeholder self-test case plus a real-flow negative
proof, and re-executed every row of the test-evidence table below. Nothing was
taken on faith from any earlier session.

## Immutable provenance evidence (fetched and confirmed 2026-08-25)

| Claim in the packaged notice/attribution | Authoritative source | Confirmed value |
|---|---|---|
| Upstream embedding license is MIT at the pinned revision | HF API `api/models/intfloat/multilingual-e5-base/revision/d128750597153bb5987e10b1c3493a34e5a4502a` | `cardData.license = "mit"`; tag `license:mit` |
| The Xenova export declares no separate license and names the upstream model as its sole base | HF API for `Xenova/multilingual-e5-base` @ `1ec9243030a27d1a115d5c340572074c125b58b2`; card README raw at that revision | `cardData.license = []` (empty); front matter `base_model: intfloat/multilingual-e5-base`; "with ONNX weights ... compatible with Transformers.js" (mechanical conversion) |
| Applicable copyright notice is `Copyright (c) Microsoft Corporation` | `raw.githubusercontent.com/microsoft/unilm/0e31c7c09737df491e7ff74ded19614b884c52b4/LICENSE` | File begins `The MIT License (MIT)` / `Copyright (c) Microsoft Corporation` |
| microsoft/unilm is the E5 development repository named by the model card's technical report | `raw.githubusercontent.com/microsoft/unilm/59d0483d235a338fcd57a1f90d4a199a39e0f401/e5/README.md` (HTTP 200) | "# E5 Text Embeddings" + citation of arXiv 2402.05672 (the report cited by the intfloat model card, whose arXiv tag is present in the API response) |
| Reranker license is Apache-2.0 at the pinned revision | `huggingface.co/cross-encoder/mmarco-mMiniLMv2-L12-H384-v1/raw/1427fd652930e4ba29e8149678df786c240d8825/README.md` | Front matter line 2: `license: apache-2.0` |

Redistribution authority conclusion (no blocker): the packaged ONNX bytes are
redistributed byte-unmodified from the Xenova export repo, whose card names
`intfloat/multilingual-e5-base` as its sole base and declares no license of its
own; the upstream model declares MIT at the pinned revision. The MIT grant is
conditioned on retaining the copyright and permission notice. Neither HF
repository ships a LICENSE file, so the applicable copyright notice is taken
from the E5 development repository's LICENSE at a pinned commit rather than
invented. The reranker ships the canonical Apache-2.0 text plus attribution;
Apache-2.0 §4 requires the license copy (packaged), retention of any upstream
NOTICE (none exists), and modification notices (we modify nothing).

## Exact license/notice decision

- The generic SPDX MIT template (`licenses/e5-base-MIT.txt`, with literal
  `<year> <copyright holders>`) was **replaced** by
  `licenses/e5-base-MIT-NOTICE.txt`: scope statement, both pinned repos and
  revisions, redistribution-authority chain, the actual copyright line
  (`Copyright (c) Microsoft Corporation`), and the canonical MIT permission
  text with placeholders substituted. SHA-256
  `b1470baa9d083bc6a3d3af31148fa4566873c2a3ecbea30f07c394e7993088ae`
  (3289 bytes), pinned in the manifest and enforced by the validator.
- mmarco due diligence: Apache-2.0 declared both as repository tag and in the
  pinned card front matter; no LICENSE/NOTICE file exists upstream to retain.
  The existing canonical-text + attribution packaging satisfies §4 as-is; the
  attribution additionally records that the unlicensed base model
  (`nreimers/mMiniLMv2-L12-H384-distilled-from-XLMR-Large`) is not packaged.
- Both license artifacts remain checked in under `resources/retrieval/licenses/`
  as exact sources (staging prefers them over network) **and** ship inside the
  bundle as manifest-managed artifacts.

## Fail-closed production contract

`parse_manifest` now ends with `validate_approved_contract`: after generic
schema validation, every field must equal the exact approved Sprint 1 bundle -
bundle ID/chunker version; embedding model id/revision/export repo/export
revision/quantization/dimensions/max-seq/query prefix/document prefix/pooling/
normalization/tokenizer type/truncation side; reranker model id/revision/
quantization/pair format/output label index/score transform; exact managed
artifact path sets per component; every artifact source URL bound to its
component's pinned revision-resolve URL (so an ONNX repo/revision cannot be
swapped while keeping schema-valid artifacts); and the full MIT/Apache license
authority records (SPDX, notice path, attribution text, resource URL, source
URL). A substitution therefore fails parsing/validation itself - including in
CI, where `cargo test --lib model_bundle` runs before staging - not merely a
test assertion. Deliberate generality remains only below the approved contract
(schema-level checks that make the failure messages precise).

## Package authority

- `tauri.conf.json` packages exactly one retrieval resource:
  `resources/retrieval/bundle`. The separate `model-bundle.manifest.json` and
  `licenses` entries were removed; the checked-in copies beside the bundle are
  build inputs/provenance sources only, never signed package content. Sprint 2
  has one manifest and one license set to load.
- The published bundle contains exactly: its manifest copy, the committed
  `README.md` placeholder, and the ten manifest-managed artifacts. Verified
  after republishing (stale prior-bundle files such as the old
  `e5-base-MIT.txt` are gone).
- The `README.md` placeholder is **required and verified**, not optional: it is
  deliberately part of the published bundle, so the shared integrity gate
  fails closed when it is absent and pins its byte length and SHA-256 against
  drift or tampering. Update the pin in the same change as the committed file.
- One reusable gate (`Assert-PackageIntegrity`) verifies every candidate
  package directory — staged package, recoverable backup, published bundle:
  every manifest-managed artifact present with exact byte length and SHA-256;
  the package's manifest copy present and **byte-identical to the checked-in
  publication manifest**; required pinned placeholders intact; no unexpected
  unmanifested file. The staging script builds staging only from
  cache/manifest/README and never copies arbitrary prior-bundle files.
  `-SelfTest` proves sole-backup restoration before cleanup plus rejection of
  a backup holding unexpected content, missing a managed artifact, carrying a
  corrupted managed artifact, lacking the README, ambiguous backups, an
  unmanifested staged file, a tampered placeholder, and a divergent manifest
  copy.

## Crash recovery

Single-publisher publication keeps the documented two same-volume renames.
Before stale-dir cleanup, if `bundle/` is absent and exactly one
`.bundle-backup-*` exists, it must pass the full `Assert-PackageIntegrity`
gate — managed artifacts verified by length/SHA-256, manifest-copy byte
identity against the checked-in authority, README presence/pin, and the
unexpected-file scan — before it is renamed into `bundle/`. A backup that is
missing a managed artifact, carries a corrupt one, holds foreign content, or
lacks the README fails closed and is preserved, never deleted before recovery;
multiple backups refuse ambiguous recovery. If the second rename fails, the
backup is renamed back in the same run; a crash between renames is recovered
on the next run. No journal was added.

## Test evidence (all run 2026-08-25, Windows x64; every row below was
## independently re-executed end to end by the completing session)

| Command (from `upstream/` unless noted) | Result |
|---|---|
| `powershell ...stage-retrieval-models.ps1 -SelfTest` | PASS - 8 proof families: sole-backup full-integrity recovery; unexpected-content rejection; missing-managed-artifact rejection; corrupted-managed-artifact (hash) rejection; missing-README rejection; ambiguous backups refused; clean staged package accepted; unmanifested/tampered-placeholder/divergent-manifest-copy rejection. Every rejected backup is verified preserved |
| Recovery integrity gap closure: `Restore-CrashedPublication` now runs the same `Assert-PackageIntegrity` gate as staging/post-publish, so a lone backup with a missing or corrupt managed model/tokenizer/license can never be renamed into `bundle/` | covered by self-test families 3 and 4 above |
| Tampered-placeholder negative proof on the real flow: edit published `bundle/README.md`, run staging | FAILS closed (exit 1, pin message, nothing renamed); restoring the committed bytes and rerunning publishes cleanly (exit 0) |
| Warm-cache staging rerun after the recovery-integrity refactor landed | PASS - 10/10 verified from cache/checked-in, shared integrity gate passed on staging and post-publish, atomically published (exit 0) |
| Staged bundle contents after republish | exactly 12 files: manifest copy + README + 10 artifacts; no stale files |
| `cargo test --lib model_bundle` (`CARGO_TARGET_DIR=%LOCALAPPDATA%\meetily-cargo-target`) | PASS - 21/21 incl. 20-case `selected_contract_substitutions_fail_parsing` (model/revision/export/quantization/dimensions/prefixes/pooling/normalization/truncation/tokenizer type/provenance drift/license authority/attribution) and non-approved-license/missing-notice cases |
| `$env:MEETLY_RAG_VERIFY_STAGED_BUNDLE=1 cargo test --lib staged_production_bundle` | PASS - ten artifacts presence+length+SHA-256 against the republished bundle |
| `$env:MEETLY_RAG_BUNDLE_DIR=<staged bundle> cargo test --test model_benchmark reference_inference_is_stable_finite_and_dimensional` | PASS - tokenizer/embedding/reranker inference vs recorded expectations (rerun this session) |
| Fresh-cache staging: `...stage-retrieval-models.ps1 -CacheRoot <empty temp>` | PASS - 8 artifacts fetched from pinned revision URLs, 2 licenses served checked-in, package re-verified, atomically published (exit 0); temp cache deleted |
| `npx --yes yaml-lint ../.github/workflows/build-windows.yml` (repo root) | PASS |
| `pnpm --dir frontend run typecheck` | PASS |
| `npx vitest run` (frontend) | PASS - 20 files / 95 tests |
| `cargo check --manifest-path frontend/src-tauri/Cargo.toml` | PASS |
| `cargo fmt --manifest-path frontend/src-tauri/Cargo.toml --check` | PASS |
| `git diff --check` | PASS (pre-existing CRLF warnings only; exit 0) |

All rows record 2026-08-25 Windows-x64 executions. The recovery-gap-fix pass
re-executed rows 1, 4-8, and 13-15 on top of the refactored script; row 3's
copy-loop pin check is unchanged by this fix, and rows 9-12 (fresh-cache
network path, YAML lint, typecheck, Vitest) have no code-path overlap with it,
so their earlier same-day results stand.

Provenance re-confirmation: the completing session independently re-fetched
every authoritative source in the table above (HF API at the pinned intfloat
revision returns tag `license:mit`; the Xenova card at the pinned export
revision declares no license and `base_model: intfloat/multilingual-e5-base`;
the pinned unilm LICENSE begins `The MIT License (MIT)` /
`Copyright (c) Microsoft Corporation`; the pinned unilm `e5/README.md` hosts
E5 and cites arXiv 2402.05672; the pinned mmarco card front matter reads
`license: apache-2.0`). No claim in this report rests on a single session's
assertion.

## Active CI behavior (root `.github/workflows/build-windows.yml`)

No change required. The Task 1.R1 gate already runs semantic validation
(`cargo test --lib model_bundle`, which now includes the fail-closed
approved-contract gate) **before** staging, then staging, the ten-artifact
assertion, and reference inference, all ahead of the Tauri build. The corrected
manifest manages the same ten artifacts in the same layout, so the existing
count assertion remains exact; the cache key follows the manifest hash
automatically.

## Omitted scope

- `1.R3` vector benchmark RAM/recovery remeasurement - untouched.
- No runtime retrieval integration; nothing calls the verifier at startup yet
  (Sprint 2 call site).
- Nested `upstream/.github`, architecture/README/sprint documents,
  `docs/notes-chat-improvement-execution.md` (explicitly out of bounds; the
  sprint register entry for `1.R2` is left to the orchestrator), Task 1.4
  benchmark files.
- macOS/Linux workflows remain out of platform scope.

## Rollback

Restore the prior working-tree state: revert `tauri.conf.json`'s resources list
(re-adds the two duplicate entries), restore the pre-R2
`resources/retrieval/model-bundle.manifest.json` + generic
`licenses/e5-base-MIT.txt`, revert `src/model_bundle.rs` /
`scripts/stage-retrieval-models.ps1`, delete the staged `bundle/` contents and
`%LOCALAPPDATA%\meetily\model-cache\`. Nothing consumes the bundle at runtime,
so removal has no application effect. The provenance findings above stand
independently of any code rollback.

## Blockers / risks

None blocking. Residual risks stated openly:

- The Microsoft-corporation notice rests on the model card linking unilm as the
  E5 development repository; if a rights holder ever publishes a different
  copyright attribution for multilingual-e5-base, the notice artifact and its
  pinned hash must be regenerated (single-file change, validator enforces the
  update).
- The reranker base model's lack of upstream license declaration remains a
  documented residual risk inherited from selection; the packaged reranker is
  redistributed under the cross-encoder repo's own Apache-2.0 declaration.
- First CI run will cold-fetch ~396 MiB because the manifest hash changed;
  subsequent runs hit the cache.
