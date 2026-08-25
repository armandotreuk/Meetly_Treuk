# Task 1.R1a - Correct the active root Windows CI staging invocation

**Status:** Complete
**Completed:** 2026-08-25
**Scope:** Sprint 1 follow-up `1.R1a` only - the smallest path/working-directory
correction to repository-root `.github/workflows/build-windows.yml` so the
staging step actually resolves on a runner checkout and the release gates run in
order before the Tauri build. No source, manifest, package-script, staging-
script, or nested-workflow changes; no `1.R2`/`1.R3`/`1.R3a` work.

## Root cause

Both post-remediation reviews named this sole blocker
(`SPR1-HR-AR-POST-20260825`, `SPR1-HR-CR-POST-20260825b`,
`sprint-1-quality-gates.md:2526-2537,2616-2639`): GitHub Actions runs each step
at the checkout root unless `working-directory` is set, and every neighbouring
bash/Rust step enters `upstream/` explicitly - but the staging step was
`shell: pwsh` with no directory change, invoking
`./frontend/src-tauri/scripts/stage-retrieval-models.ps1`. That path exists only
under `upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1`, so
PowerShell failed ("term is not recognized") before any fetch; the job never
reached the ten-artifact assertion, staged reference inference, or the Tauri
build, leaving `architecture.md`'s Packaging And Platform Gates
(`docs/hybrid-rag/architecture.md:1708-1715`) unsatisfied by active CI.

## Fix

One line, `.github/workflows/build-windows.yml` (staging step `run:`):

```yaml
run: ./upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1
```

Chosen over `working-directory: upstream` (the reviews' alternative): it is the
same-size diff without introducing a YAML key used nowhere else in the file, and
it matches the file's only other `pwsh` step ("Verify active toolchain matches
rust-toolchain.toml"), which already uses checkout-root-relative paths
(`upstream/rust-toolchain.toml`). Behavior is identical either way because the
script derives every path from `$MyInvocation.MyCommand.Path` and
`$env:LOCALAPPDATA`; it has no CWD dependency.

### What CI will now execute (build job, unchanged order)

1. Validate checked-in production manifest contract -
   `cd upstream && cargo test --lib model_bundle`
   (`.github/workflows/build-windows.yml:132-136`).
2. **Stage and verify retrieval model bundle** - `shell: pwsh` at the checkout
   root executes `./upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1`
   (fetch pinned URLs into the `%LOCALAPPDATA%\meetily\model-cache` cache,
   verify length+SHA-256, stage outside resources, re-verify the package,
   publish atomically to `resources/retrieval/bundle`)
   (`.github/workflows/build-windows.yml:138-140`).
3. Assert staged bundle contains all verified artifacts -
   `MEETLY_RAG_VERIFY_STAGED_BUNDLE=1` gated test
   (`.github/workflows/build-windows.yml:142-148`).
4. Reference inference on staged production bundle -
   `MEETLY_RAG_BUNDLE_DIR=<workspace>/upstream/frontend/src-tauri/resources/retrieval/bundle`
   (`.github/workflows/build-windows.yml:150-156`).
5. llama-helper sidecar build and Tauri build/packaging, unchanged.

## Verified commands and results

Run from the repository root (= checkout-root analog), 2026-08-25:

| Command | Result |
|---|---|
| `npx --yes yaml-lint .github/workflows/build-windows.yml` | PASS |
| Checkout-root PowerShell executing exactly the corrected CI invocation, offline mode: `& ./upstream/frontend/src-tauri/scripts/stage-retrieval-models.ps1 -SelfTest` | PASS - `SELFTEST PASS`; all self-test families (sole-backup recovery, foreign/corrupt/missing-artifact/README-less rejection, ambiguous refusal, clean-package control) prove the relative path resolves from the checkout root and the script executes |
| Static path/order assertions (repo-root PowerShell over the workflow text): corrected `upstream/frontend/...` path present; no `run: ./frontend/src-tauri/scripts/` remnant; strict name-offset ordering of the three gate steps before the build | PASS - stage@4597 < ten-artifact@4754 < reference-inference@5060 < tauri-build@7656 |
| `git diff --check` | PASS (pre-existing CRLF warnings only, on untouched files, same as recorded in `task-1.r1-active-ci.md`) |

### What cannot be executed locally versus what CI will execute

- Locally proven end to end for the defect itself: YAML validity, path
  resolution from the checkout root, and script executability (via `-SelfTest`:
  temp dirs only, no network, no model downloads).
- Not executable locally: the real GitHub-hosted run - `actions/cache`
  restore/save round trip, the runner-side Hugging Face revision-resolve fetches
  (~396 MiB on a cold cache), the downstream ten-artifact assertion, reference
  inference, llama-helper/Tauri builds, and artifact upload. Those mechanisms
  carry their own prior evidence (`1.R1`/`1.R2` reports) and none of their code
  changed here. Per the review requirement, Sprint 1 stays open until one actual
  root `build-windows` run passes through staging, both gates, and Tauri
  packaging with attached run URL/logs.
- A full local staging rehearsal (real fetch + republish) was deliberately not
  performed; the task authorized `-SelfTest` to avoid the network fetch.

## Files changed

- `.github/workflows/build-windows.yml` (one line: staging-step `run:` path)

Untouched as instructed: nested `upstream/.github/**`, all sources, manifests,
package scripts, the staging script, every doc except this report, and all
`1.R3`/`1.R3a` work. All pre-existing uncommitted changes preserved.

Spillover recorded here, not acted on (out of this task's doc scope): the
post-remediation code review also asked to correct
`task-1.r1-active-ci.md:53` (the report line transcribing the wrong path) in the
same change; that file may not be edited under this task's constraints and is
left for the main-agent log pass.

## Rollback

Revert the single-line edit to restore
`run: ./frontend/src-tauri/scripts/stage-retrieval-models.ps1`. Nothing else
depends on the path form; no runtime effect.

## Blockers

None blocking. Residual risk unchanged from `1.R1`: the corrected invocation
runs for the first time on a real runner only when CI next executes, and HF
revision-resolve URL availability remains staging's network dependency exactly
as accepted in Task 1.5.
