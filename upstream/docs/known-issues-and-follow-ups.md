# Known Issues and Follow-ups

Status: working backlog

Last updated: 2026-08-03

This document records known gaps, deferred work, and risks discovered while restoring multi-template summaries, PDF export, and the existing feature set. A reported fix is not considered release-ready until it has passed the independent sprint review.

## Current Decision Gate

The latest S1 implementation agent reports that the following hardening is implemented, but the final independent S1 review has not run yet:

- Per-template and per-generation cancellation, polling, save, and delete fencing.
- Canonical `db:<id>` and legacy numeric summary identity compatibility.
- Explicit `file:<id>` identities for numeric file templates.
- Failed `PENDING` process recovery and timeout cancellation.
- Legacy PDF fallback preservation, including `MeetingNotes.sections`.
- Stable built-in override deduplication.
- Correct `folder_id` propagation and `open_meeting_folder` projection.

The latest reported verification is 318 Rust tests passed, 13 focused frontend tests passed, and TypeScript typecheck passed. These results still require independent review before S1 is accepted.

## High Impact Items

### S1 hardening requires independent review

The identity and concurrency changes affect persisted summaries, generation cancellation, polling, save behavior, and PDF export. Review must specifically test:

- Two templates generating for the same meeting at the same time.
- Cancelling one template without cancelling another.
- A stale worker completing after a retry.
- A stale editor saving after a newer generation.
- Canonical and legacy numeric summary aliases coexisting.
- A database template and numeric file template with the same numeric ID.
- Deleted database/file templates with preserved summary content.
- Failed initialization and timeout recovery from `PENDING`.

### Multi-template UI is not wired yet

The backend and supporting components exist, but the meeting-details page still needs the complete user path:

- Call `useMeetingSummaries` and `useActiveSummaryTemplate`.
- Load the exact summary for the selected template.
- Pass active-template state through `PageContent` and `SummaryPanel`.
- List, switch, delete, and generate multiple summaries.
- Save edits to the selected summary instead of defaulting to `standard_meeting`.
- Preserve dirty-switch confirmation and generation polling.

Planned sprint: `S2-TEMPLATES`.

### PDF export is still not visible in the active UI

The Rust export commands and `MeetingDetails/ExportMenu.tsx` exist, but the menu must be rendered from the active summary toolbar and receive the selected template identity.

Planned sprint: `S3-PDF`.

## Confirmed Feature Gaps

### FTS5 sidebar search

The FTS5 migration, repository, and Tauri command exist. `SidebarProvider` still uses the legacy transcript search path, and `api_search_fts` has no active frontend caller.

Planned sprint: `S4-INTEGRATION`.

### Sidebar resizing

Sidebar resize state exists, but `SidebarProvider` explicitly does not attach the resize handle props. The feature remains incomplete.

Planned sprint: `S4-INTEGRATION`.

### DOCX export

PDF export is the supported format. The DOCX implementation remains a disabled placeholder and should not be presented as available until implemented.

### Test harness failures

The full frontend test command still reports two pre-existing suite failures:

- An empty `onboarding-summary-model.test.mjs` suite.
- A `bun:test` import in `summary-language-preferences.test.js`.

These should be cleaned up or explicitly excluded before release gates are considered fully green.

## Verification Gaps

- No complete end-to-end test currently exercises Tauri IPC from the rendered meeting-details page through SQLite and back.
- Multi-template migration behavior is not covered by a real upgrade test against a copied production database.
- Summary alias compatibility is not covered by a production-shaped fixture containing both legacy numeric and canonical rows.
- PDF output needs manual verification with accented text, legacy sections, deleted templates, and multiple selected templates.
- Folder persistence needs manual verification after refetch and application restart.
- CUDA release deployment must be repeated only after the final reviewed code is committed.
- A production database backup is required before migration/deployment.

## Later Improvements

Priority order after the current restoration:

1. Complete S2 multi-template page and summary editing flow.
2. Complete S3 selected-template PDF export flow.
3. Restore FTS5 sidebar search and folder-aware result filtering.
4. Attach and test sidebar resizing.
5. Add real SQLite migration and alias compatibility fixtures.
6. Add Tauri IPC integration tests for template CRUD and export.
7. Add end-to-end meeting-details tests for switching, saving, deleting, polling, and export.
8. Resolve the pre-existing frontend test harness failures.
9. Implement DOCX export or remove its disabled UI entry.
10. Audit orphaned components (`LegacyDatabaseImport`, `BluetoothPlaybackWarning`, `ConsoleToggle`, `useAudioPlayer`, and `ModelDownloadProgress`) and either wire or delete them.

## Scope Creep (S4 Review Finding)

### Summary polling refactoring mixed into S4 diff

The S4 review found that `SidebarProvider.tsx` contains a major rewrite of `startSummaryPolling` and `stopSummaryPolling` that belongs to S2 (multi-template summary generation), not S4 (FTS5 + resize). Specifically, the diff introduces:

- **New imports**: `buildSummaryCancelArgs`, `shouldApplySummaryPollResult`, `summaryPollKey` — extracted utility modules created for multi-template identity handling.
- **New refs**: `activeSummaryPollsRef`, `activeSummaryPollKeysRef` — replaces the prior polling state with a keyed ref-based architecture.
- **`startSummaryPolling` signature change**: 5 params `(meetingId, processId, templateId, generation, onUpdate)` instead of S2's originally planned 4 params. The `generation` parameter is an S1 identity feature.
- **Null-separated compound keys**: `summaryPollKey` builds `meetingId\u0000templateId\u0000generation` — a new keying scheme not described in any S2 plan document.
- **Refs-based dependency arrays**: `React.useCallback(..., [])` with no deps, relying on refs for mutable state — a pattern change beyond S2's scope.
- **`stopSummaryPolling` signature change**: `(meetingId, templateId?, generation?)` with targeted prefix matching — new capability not in the original S2 plan.

**Root cause**: The S2 multi-template implementation (Sprint B, items 24–27 per `multi-template-summaries-progress.md:250`) described a narrow "add 4th arg" change. The actual diff in `SidebarProvider.tsx` went further, rewriting the entire polling lifecycle to support per-template/per-generation identity tracking. This was committed as part of S4 rather than committed separately as an S2 follow-up.

**Impact**: The S4 commit is larger than intended. It bundles S2 polling infrastructure with S4 FTS5 search and sidebar resize changes, making review and rollback harder.

**Status**: Both `useSummaryGeneration.ts` and `meeting-details/page.tsx` correctly call the updated signatures. The code is internally consistent and functional — the issue is commit hygiene, not correctness.

**Future cleanup**: A future commit could split the S2 polling refactoring (keyed refs, compound keys, `summaryPollKey`/`shouldApplySummaryPollResult` utilities) into its own commit with an S2 scope label, leaving the S4 commit with only FTS5 and resize changes.

## Review Policy

Every sprint ends with an independent code review before the next sprint starts. Review findings must be fixed or explicitly recorded as deferred scope.

Each implementation or review subagent must keep its session below 180000 tokens. At 150000 tokens, it must stop, summarize its state and unresolved work, and hand the remaining work to a fresh subagent.
