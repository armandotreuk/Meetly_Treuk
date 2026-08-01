# Meeting Date Display — Sprint Log

**Feature:** Display the meeting date/time in two places:
- **Meeting-details page header** — full format (`"July 31, 2026 at 2:30 PM"`), editable title re-enabled
- **Sidebar meeting list** — short format (`"Jul 31, 2:30 PM"`), under each meeting title

**Source plan:** `docs/fts5-search-mcp-plan.md` (sibling) and the chat plan-revision above.

**Locked decisions:**
- Locale = browser default (`undefined` — adapts per user)
- FTS search results: deferred until FTS5 branch lands (show date only if the result type carries `created_at`, else omit)
- Rust serialization: `m.created_at.0.to_rfc3339()` (explicit ISO 8601; `DateTimeUtc.0` unwraps `DateTime<Utc>`)
- `EditableTitle` re-enabled in the meeting-details header

**Sprint breakdown:**

| Sprint | Scope | Files |
|--------|-------|-------|
| 1 | Foundational plumbing: shared date helper + Rust `created_at` exposure | `lib/utils.ts`, `api/api.rs` |
| 2 | Sidebar data layer: `CurrentMeeting` + tree types carry `created_at` | `SidebarProvider.tsx`, `useSidebarTree.ts` |
| 3 | Sidebar UI: pass `createdAt` through `MeetingTreeItem` and all call sites | `MeetingTreeItem.tsx`, `Sidebar/index.tsx` |
| 4 | Meeting-details header: `EditableTitle` + date line; end-to-end smoke test | `SummaryPanel.tsx` |

**Process rules:**
- Every task ends with a subagent code review.
- Every sprint ends with a subagent sprint-end code review.
- Sprint log updated at the end of each sprint with: what was implemented, why, and any deviations/decisions.
- Pause and ask the user before starting the next sprint.

---

## Sprint 0 — Document Creation

**Implemented:** Created this sprint log (`docs/meeting-date-display-sprint-log.md`) as the running record.

**Why:** Provide a single place to capture decisions, scope, and rationale so future maintainers (and reviewers) can understand the feature without re-deriving it from the plan. A separate document from the plan keeps the plan clean as the source-of-truth and the log as the implementation journal.

---

## Sprint 1 — Foundational Plumbing

**Implemented:**
1. New helper `formatMeetingDate(iso, format)` in `frontend/src/lib/utils.ts` with `"full"` and `"short"` formats and a `""` sentinel for nullish / empty / unparseable input.
2. Rust API `Meeting` struct in `frontend/src-tauri/src/api/api.rs` now exposes `pub created_at: String`, populated by `m.created_at.0.to_rfc3339()` (unwrapping the `DateTimeUtc` newtype and emitting an ISO 8601 string).
3. Added test file `frontend/tests/lib/utils.test.ts` covering the nullish/empty/unparseable contract and the two format branches.

**Why:** The helper centralizes date formatting in one place (the repo had no shared date helper, only inline `toLocaleDateString` patterns). The Rust change is the minimum required to surface `created_at` over Tauri IPC — `MeetingModel` already had the field, it was being stripped at the API layer. RFC3339 is the explicit ISO 8601 format the frontend `new Date()` parses reliably and the `Meeting.created_at: string` TS interface expects.

**Decisions / deviations:**
- `"full"` uses `hour: "numeric"` (no leading zero) per the agreed spec `"July 31, 2026 at 2:30 PM"`. An earlier draft had `"2-digit"` which produced `"02:30 PM"`; caught by subagent code review and fixed.
- Drive-by: the pre-existing `isOllamaNotInstalledError` was re-indented / re-quoted / semi-coloned by the editor to match `.prettierrc.json`. Kept intentionally rather than reverting; the file is now prettier-compliant.
- Test was added after the task-level review flagged its absence (per AGENTS.md "non-trivial logic leaves ONE runnable check behind"). Test co-located with other lib tests in `frontend/tests/lib/` per the existing convention.

**Files changed:**
- `frontend/src/lib/utils.ts` (helper added; `isOllamaNotInstalledError` reformatted)
- `frontend/src-tauri/src/api/api.rs` (struct + mapping)
- `frontend/tests/lib/utils.test.ts` (new file)

**Sprint-end verdict:** PASS. Wire boundary verified end-to-end (DB → Rust API → TS interface → helper). Sprint 2/3 entry points (SidebarProvider, useSidebarTree) confirmed un-touched. Ready to proceed.

---

## Sprint 2 — Sidebar Data Layer

**Implemented:**
1. `frontend/src/components/Sidebar/SidebarProvider.tsx` — `CurrentMeeting` gained optional `created_at?: string`; `fetchMeetings` invoke cast and transform widened to propagate the field.
2. `frontend/src/hooks/useSidebarTree.ts` — `MeetingLike` widened to `Pick<Meeting, "id" | "title" | "folder_id"> & { created_at?: string }`; `MeetingNode` gained `createdAt?: string`; `buildFolderNode` maps `createdAt: m.created_at`.
3. `frontend/src/app/meeting-details/page.tsx:150` — opportunistic: when opening a meeting, `setCurrentMeeting` now also passes `created_at: metadata.created_at`.
4. `frontend/src/hooks/useRecordingStop.ts:358-363` — opportunistic: when a recording stops, the `setCurrentMeeting` call in the success branch now also passes `created_at: meetingData.created_at`. (The catch branch at `:366` has no source for the date and was left alone — `refetchMeetings()` re-populates `state.meetings` immediately before this block, so the gap closes within one render.)

**Why:** The sidebar tree is the *only* path that renders meetings in the left panel. The data needed to reach `MeetingTreeItem` end-to-end, and the tree-builder is the bottleneck — it dropped every field except `id` and `title`. `created_at` must propagate `MeetingModel → Meeting → fetchMeetings → CurrentMeeting → MeetingLike → MeetingNode` for Sprint 3 to render the date under each meeting title. Opportunistic calls (3 and 4) close the data gap for the *currently-open* meeting so the date appears immediately when a meeting is opened, not just on the next refetch.

**Decisions / deviations:**
- `MeetingLike` was first drafted as `Pick<Meeting, "id" | "title" | "created_at" | "folder_id">`, which made `created_at` *required* via `Pick` from the `Meeting` interface. The task-level review caught a real `tsc` error against the existing `CurrentMeeting.created_at?: string` (optional) at the call site in `Sidebar/index.tsx:355`. Fixed to `Pick<Meeting, "id" | "title" | "folder_id"> & { created_at?: string }` — the intersection keeps `id`/`title`/`folder_id` required (preserving the existing tree-builder contract) while making `created_at` optional to match `CurrentMeeting`. Comment on the type was updated to document why the optionality exists.
- No test was added for Sprint 2. Reasoning (per the sprint-end review): the only logic is a one-line field pass at `useSidebarTree.ts:75` and a transform at `SidebarProvider.tsx:139-153`. The type system is the test for type-only plumbing. If Sprint 3 reveals a regression in the `createdAt` propagation, a `useSidebarTree` test will be added at that point.

**Deferred items (Sprint 3):**
- `frontend/src/components/Sidebar/index.tsx:698` `renderMeetingNode` uses an inline type `{ kind: "meeting"; id: string; title: string }` instead of importing `MeetingNode`. Currently latent (TypeScript's bivariant function-parameter checking makes the narrower `node` parameter acceptable for a `MeetingNode` slot, and the body only reads `id`/`title`). When Sprint 3 needs to forward `createdAt`, the parameter type should be widened to `MeetingNode` directly (one-line import).
- `useRecordingStop.ts:366-369` (catch branch): no `created_at` source on this path. The `refetchMeetings()` at line 353 re-populates the `meetings[]` list, so the gap closes within one render. Not worth a separate metadata fetch in the error path. Leave as-is.

**Files changed:**
- `frontend/src/components/Sidebar/SidebarProvider.tsx`
- `frontend/src/hooks/useSidebarTree.ts`
- `frontend/src/app/meeting-details/page.tsx` (opportunistic)
- `frontend/src/hooks/useRecordingStop.ts` (opportunistic)

**Sprint-end verdict:** PASS. End-to-end data flow verified across 8 hops. Optionality correct at every layer. No UI/format leaks. No regressions. Ready to proceed to Sprint 3.

---

## Sprint 3 — Sidebar UI (short format)

**Implemented:**
1. `frontend/src/components/Sidebar/MeetingTreeItem.tsx`:
   - Added import: `import { formatMeetingDate } from "@/lib/utils";`
   - Widened props: `createdAt?: string;` (line 89).
   - Destructured `createdAt` in the function signature (line 103).
   - Derived value: `const formattedDate = isIntro ? "" : formatMeetingDate(createdAt, "short");` (line 112). One short-circuit for the intro row (it has no date) and one call into the helper (which returns `""` for nullish/empty/unparseable).
   - Rendered: `{formattedDate && (<div className="mt-1 ml-8 text-xs text-gray-500">{formattedDate}</div>)}` (after the `snippetContext` block, line ~310).
2. `frontend/src/components/Sidebar/index.tsx`:
   - Added: `import type { MeetingNode } from "@/hooks/useSidebarTree";`
   - Widened `renderMeetingNode` parameter from inline `{ kind: "meeting"; id: string; title: string }` to `node: MeetingNode`. Passes `createdAt={node.createdAt}` (line 711).
   - Flat search-results list (`flatSearchResults.map(...)`): added `createdAt={m.created_at}` (line 857). `m` is `CurrentMeeting`.
   - `unfiled.map((m) => ...)` list: added `createdAt={m.created_at}` (line 916). `m` is `MeetingLike`.
   - Hardcoded `+ New Call` intro item (lines 899-908): **intentionally NOT updated** — no `created_at` source. The `isIntro` guard inside `MeetingTreeItem` already prevents a date from rendering.

**Why:** This is the visible user-facing change for the sidebar. The data path from Sprint 2 ends at `MeetingNode.createdAt?: string` (and at `CurrentMeeting.created_at?` for the search-results list). Sprint 3 is purely presentation: pass the field through, format it short, render under the title.

**Decisions / deviations:**
- **Prop type chosen as `string` (not `string | null`).** First draft was `createdAt?: string | null;` — over-broad. The data path never produces `null` (Rust emits RFC3339 via `to_rfc3339()` which is always a string), so the optional `string` is the precise contract. The TS reviewer caught this as a non-blocking note and the cleanup was applied immediately in the same sprint.
- **Render order chosen as: snippet → date → chips.** Snippet is the most salient in search results (yellow background), so it goes first. Date is contextual metadata below it. Chips (chunkType/folderName) trail.
- **No new test added.** The change is type plumbing + one helper call; the existing test at `frontend/tests/lib/utils.test.ts` already covers `formatMeetingDate` for the cases that matter (`null`, `undefined`, empty, unparseable, real ISO, both formats). Adding a UI-render test for a date sub-line would require a React test harness and a snapshot of the `MeetingTreeItem` markup — out of proportion to the change. The end-to-end smoke test in Sprint 4 is the verification.

**Deferred items (Sprint 4):**
- `SidebarProvider.setCurrentMeeting` is also called at `Sidebar/index.tsx:213` and `:477` without `created_at`. Sprint 2 already opportunistically added the field at the two paths where a *real* meeting is being opened (`meeting-details/page.tsx:150` and `useRecordingStop.ts:358`). The remaining two callsites are: (a) deletion fallback to the intro row at `:213` — no `created_at` needed, and (b) `handleEditConfirm` at `:477` (title rename) — the helper doesn't currently have `created_at` in scope. Sprint 4 should add a small `created_at` lookup (or a separate `setCurrentMeetingEx` that takes the full meeting) to keep the in-memory current meeting consistent after renames.
- `SummaryPanel.tsx` (Sprint 4 work) should consider using a semantic `<time dateTime={...}>` element instead of a plain `<p>` for the header date, per the sprint-end reviewer's a11y note.

**Files changed (2):**
- `frontend/src/components/Sidebar/MeetingTreeItem.tsx`
- `frontend/src/components/Sidebar/index.tsx`

**Sprint-end verdict:** PASS. All 4 `MeetingTreeItem` call sites are correctly wired: 3 pass `createdAt`, 1 (intro) intentionally omits it. Optionality consistent across all 5 type layers (`Meeting → CurrentMeeting → MeetingLike → MeetingNode → MeetingTreeItemProps`). Guards correctly hide the sub-line for: intro row, missing `created_at`, empty/unparseable input. SnippetContext and chip rows still render independently. No regressions. Ready to proceed to Sprint 4.

---

## Sprint 4 — Header (full format) + smoke test

**Implemented:**
1. `frontend/src/components/MeetingDetails/SummaryPanel.tsx`:
   - Added import: `import { formatMeetingDate } from "@/lib/utils";`
   - Hoisted before `return`: `const formattedFullDate = formatMeetingDate(meeting.created_at, "full");`
   - Uncommented `<EditableTitle>` block (was previously disabled).
   - Added the date sub-line directly below the title:
     ```tsx
     {formattedFullDate && (
         <time
             dateTime={meeting.created_at}
             className="text-sm text-gray-500 mt-2 block"
         >
             {formattedFullDate}
         </time>
     )}
     ```

**Why:** This is the second of two visible user-facing changes. Sprint 1-3 wired the data through to the sidebar list (short format). Sprint 4 completes the feature by re-enabling the editable title (was commented out) and adding the matching full-format date in the meeting-details header.

**Decisions / deviations from the original sprint plan:**
- **`<time dateTime={meeting.created_at}>` instead of `<p>`.** Sprint 3's reviewer suggested this for a11y. Applied in this sprint. `meeting.created_at` is the raw RFC3339 string, which is exactly what `<time dateTime>` wants.
- **`mt-2` (8px) instead of `mt-1` (4px).** Sprint 4 reviewer flagged that `mt-1` reads as "attached" rather than "subordinate" under a `text-2xl`-class title. `mt-2` is the comfortable read. One-character change.
- **`block` class on `<time>`.** `<time>` is inline by default; the previous `<p>` was block-level. Adding `block` preserves the original block layout (margins work, full-width) and means the visual diff vs. the original commented-out region is minimized.
- **Hoisted const instead of double-call.** First draft called `formatMeetingDate` twice (once in the guard, once in the content). Replaced with a single `const formattedFullDate = ...` hoisted to the top of the return. Reads cleaner and matches the codebase pattern (cf. `staleStatusOrigin`, `displaySummaryError`, `languageSlot` at lines 294-327 of the same file).
- **Prettier reformatted** the file: 6 of 8 touched files needed re-formatting (`SummaryPanel.tsx`, `MeetingTreeItem.tsx`, `Sidebar/index.tsx`, `SidebarProvider.tsx`, `useSidebarTree.ts`, `meeting-details/page.tsx`). This is a tooling issue, not a real diff — but worth recording that the formatter moved some lines around. After prettier, the typecheck and tests still pass.

**Deferred items (post-feature):**
- **Locale-specific "at" in `"full"` format.** `formatMeetingDate` uses `toLocaleDateString(undefined, …)`. In en-US, the "full" format renders as `"July 31, 2026 at 2:30 PM"`; in pt-BR it renders as `"31 de julho de 2026, 14:30"` (no "at"). The original sprint spec locked the format to browser default locale, so this is by design. If we later want a literal "at" between date and time in non-English locales, the helper would need a manual join. No action this sprint.
- **`SidebarProvider.setCurrentMeeting` at `Sidebar/index.tsx:477` (rename handler)** still doesn't carry `created_at`. After a rename, the `currentMeeting` in memory has the new title but the original `created_at`. The header date is rendered from `meeting.created_at` (not `currentMeeting.created_at`), so the visible date is unaffected. `currentMeeting.created_at` is only used for the sidebar's `MeetingTreeItem` calls (which read from the `meetings[]` state, not `currentMeeting`). Net: no visible bug, just an inconsistency in the in-memory current-meeting state. Documented but not fixed.
- **Polished "What's New" line in user guide** (`docs/features-user-guide.md`) could be added. Out of scope for this sprint.

**Verification (Task 4.3):**
- `tsc --noEmit` — passes
- `vitest run` — 5 test files, 19 tests, all pass (including `tests/lib/utils.test.ts` covering `formatMeetingDate` for nullish/empty/unparseable/real-ISO/both-formats)
- `next lint --dir src --dir tests` — no warnings on changed files (only pre-existing warnings in unrelated files: `useRecordingStart.ts`, `useRecordingStateSync.ts`, `useRecordingStop.ts`, `analytics.ts`, `recordingNotification.tsx`, `blocknote-markdown.test.ts`)
- `prettier --check` — all 8 touched files use Prettier code style
- **Manual visual check by the user is still recommended** before shipping (create a meeting, verify both header + sidebar show dates in their respective formats).

**Files changed (1):**
- `frontend/src/components/MeetingDetails/SummaryPanel.tsx`

**Sprint-end verdict:** PASS. End-to-end data flow verified across 11 hops. Header shows full format; sidebar shows short format. Intro row never shows a date (correctly guarded at the `MeetingTreeItem` level). Missing/unparseable dates silently hide the sub-line via the `&&` guard. No regressions to `EditableTitle`, popovers, drag-and-drop, or the summary button group. `tsc`, `vitest`, and `next lint` all clean on the changed files. Feature is ready to ship.

---

## Final Summary

**Feature:** Display meeting `created_at` in the sidebar (short) and meeting-details header (full).

**Sprint count:** 4 (Sprint 0: log creation; Sprint 1: data + helper; Sprint 2: state plumbing; Sprint 3: sidebar UI; Sprint 4: header UI + verification).

**Total files changed:** 10
- Backend: `frontend/src-tauri/src/api/api.rs`
- Types: `frontend/src/types/index.ts` (no change — pre-existing field)
- State: `frontend/src/components/Sidebar/SidebarProvider.tsx`, `frontend/src/hooks/useSidebarTree.ts`, `frontend/src/app/meeting-details/page.tsx`, `frontend/src/hooks/useRecordingStop.ts`
- UI: `frontend/src/components/Sidebar/MeetingTreeItem.tsx`, `frontend/src/components/Sidebar/index.tsx`, `frontend/src/components/MeetingDetails/SummaryPanel.tsx`
- Util: `frontend/src/lib/utils.ts`
- Tests: `frontend/tests/lib/utils.test.ts` (no change to existing tests, only new assertions were added in Sprint 1)

**Format contract locked:**
- `"full"` = e.g., `"July 31, 2026 at 2:30 PM"` (en-US; browser-default locale)
- `"short"` = e.g., `"Jul 31, 2:30 PM"` (en-US; browser-default locale)
- Helper: `formatMeetingDate(iso: string | null | undefined, format: "full" | "short"): string` — returns `""` for nullish/empty/unparseable.

**Verification (per-sprint + final):**
- Every sprint ended with subagent review (PASS) and a sprint log entry.
- Final verification: `tsc --noEmit` ✓, `vitest run` ✓ (5 files, 19 tests, all pass), `next lint` ✓ (clean on changed files), `prettier --check` ✓ (8 files clean).

**Open follow-ups (post-feature, non-blocking):**
- Locale-specific rendering of "at" in full format (intentional per spec).
- `setCurrentMeeting` after rename at `Sidebar/index.tsx:477` doesn't carry `created_at` (no visible bug).
- Optional: a "What's New" line in `docs/features-user-guide.md`.
- Optional: manual visual check by the user before merging.

**Ready to ship.**

---

## Sprint 5 — Polish (post-feature cleanup)

**Implemented:**
1. `docs/features-user-guide.md`:
   - Added Section 7 "Meeting Dates in Sidebar & Header" describing where the date appears (sidebar short / header full / search results short), the intro-row exception, locale handling, and 6 smoke-test cases.
   - Added a "Meeting dates" row in the Quick Reference table.
2. `frontend/src/components/Sidebar/index.tsx` (lines 469-477):
   - Updated `handleEditConfirm` so the in-memory `currentMeeting` carries `created_at` after a rename. Looks up the meeting in `meetings[]` and threads the field through. This closes the "rename leaves `currentMeeting.created_at` undefined" inconsistency flagged in Sprint 4.

**Why:** The two open follow-ups from the Final Summary that were cheap to fix. Section 7 of the user guide makes the feature discoverable for the user. The `setCurrentMeeting` rename fix removes a real inconsistency (the `currentMeeting` state is the "in-memory" representation of the open meeting; it should match the meeting's actual fields, not just `id` and `title`).

**Decisions / deviations:**
- **Locale example format.** First draft included time-of-day in the locale examples (`31 de julho de 2026, 14:30` in pt-BR). Sprint 5 reviewer caught this as wrong: `toLocaleString` in pt-BR actually produces `31 de julho de 2026 às 14:30` (uses `às` not comma, and the time depends on the user's timezone — the test used `T14:30:00Z` which is UTC and renders in local time). Switched to date-only examples (`31 de julho de 2026` in pt-BR, `31 juillet 2026` in fr-FR) to avoid the timezone trap and the locale-specific "às"/"à" differences. Date-only is universally recognizable and matches what the helper does for the date part.
- **No new test added.** The rename fix is a one-line lookup. The behavior is exercised every time the user renames a meeting (the `meetings.find()` returns the right `created_at`). A unit test for this would require setting up the entire `useSidebar` provider with state and mocks — out of proportion to the change. Manual smoke test is sufficient.
- **No code in `SidebarProvider.setCurrentMeeting` itself changed.** The fix is at the call site (Sidebar/index.tsx:469-477) because that's where the new title and the in-memory `meetings[]` are both available. Touching the provider would have been the wrong layer (it would have required changing the provider's signature or adding a new variant, which is what the original Sprint 4 deferred-item comment speculated about — but it's not needed when the call site can do the lookup trivially).

**Deferred items (post-Sprint 5):**
- Locale-specific rendering of "at" in full format — intentional per spec (browser default locale).
- Manual visual check by the user before merging.

**Files changed (2):**
- `docs/features-user-guide.md`
- `frontend/src/components/Sidebar/index.tsx`

**Verification (post-Sprint 5):**
- `tsc --noEmit` ✓
- `vitest run` ✓ (5 files, 19 tests, all pass)
- `prettier --check` ✓ (already-clean from Sprint 4; Sprint 5 changes did not introduce any new formatting issues)

**Sprint-end verdict:** PASS. Feature is now feature-complete, documented, and consistent. The only remaining open follow-up is the manual visual check, which requires the user to launch the app.

---

## Final Final Summary

**Feature:** Display meeting `created_at` in the sidebar (short) and meeting-details header (full).

**Sprint count:** 5 (Sprint 0: log creation; Sprint 1: data + helper; Sprint 2: state plumbing; Sprint 3: sidebar UI; Sprint 4: header UI + verification; Sprint 5: polish).

**Total files changed:** 11
- Backend: `frontend/src-tauri/src/api/api.rs`
- State: `frontend/src/components/Sidebar/SidebarProvider.tsx`, `frontend/src/hooks/useSidebarTree.ts`, `frontend/src/app/meeting-details/page.tsx`, `frontend/src/hooks/useRecordingStop.ts`
- UI: `frontend/src/components/Sidebar/MeetingTreeItem.tsx`, `frontend/src/components/Sidebar/index.tsx`, `frontend/src/components/MeetingDetails/SummaryPanel.tsx`
- Util: `frontend/src/lib/utils.ts`
- Tests: `frontend/tests/lib/utils.test.ts`
- Docs: `docs/features-user-guide.md`

**Format contract:**
- `"full"` = e.g., `"July 31, 2026 at 2:30 PM"` (en-US; browser-default locale)
- `"short"` = e.g., `"Jul 31, 2:30 PM"` (en-US; browser-default locale)
- Helper: `formatMeetingDate(iso: string | null | undefined, format: "full" | "short"): string` — returns `""` for nullish/empty/unparseable.

**Verification chain:**
- Every sprint ended with subagent review (PASS for Sprints 1, 2, 3, 4, 5; Sprint 5 caught and fixed a real locale-example bug in the user guide).
- Final verification: `tsc --noEmit` ✓, `vitest run` ✓ (5 files, 19 tests), `next lint` ✓ (clean on changed files), `prettier --check` ✓ (8+ files clean).

**Open follow-ups (intentional, non-blocking):**
- Locale-specific rendering of "at" in full format (intentional per spec).
- Manual visual check by the user before merging.

**Feature complete and ready to ship.**

---

## Sprint 5 Followup — Blocker caught in final review

**Discovered in:** End-of-feature subagent review (Task 5.5). Reviewer returned a `FAIL` verdict with a real, reachable bug.

**The bug:** `frontend/src/hooks/meeting-details/useMeetingData.ts` had two paths (`handleSaveMeetingTitle` at lines 67-71 and `updateMeetingTitle` at lines 161-165) that built fresh object literals `{ id, title }` for `setMeetings` and `setCurrentMeeting`, dropping `created_at` (and `folder_id`). When the user renamed a meeting in the meeting-details header (the `EditableTitle` re-enabled in Sprint 4), the sidebar's date sub-line would disappear for that meeting because `m.created_at` was `undefined` in the updated `meetings[]` array.

**Why Sprint 5 missed it:** Sprint 5 fixed the *sidebar's* rename path at `Sidebar/index.tsx:469-477` but did not grep for parallel rename paths in the *meeting-details page* hooks. AGENTS.md says: "Bug fix = root cause, not symptom: a report names a symptom. Grep every caller of the function you touch and fix the shared function once." I should have grepped all `setCurrentMeeting`/`setMeetings` callsites when Sprint 2 first widened the type. The final review caught it.

**Fix applied:** Replaced the fresh object literals with object spreads and added `created_at` to the `setCurrentMeeting` calls.

### Fix 1 (lines 67-75, in `handleSaveMeetingTitle`)

Before:
```ts
const updatedMeetings = sidebarMeetings.map((m: CurrentMeeting) =>
    m.id === meeting.id ? { id: m.id, title: meetingTitle } : m
);
setMeetings(updatedMeetings);
setCurrentMeeting({ id: meeting.id, title: meetingTitle });
```

After:
```ts
const updatedMeetings = sidebarMeetings.map((m: CurrentMeeting) =>
    m.id === meeting.id ? { ...m, title: meetingTitle } : m
);
setMeetings(updatedMeetings);
setCurrentMeeting({
    id: meeting.id,
    title: meetingTitle,
    created_at: meeting.created_at,
});
```

### Fix 2 (lines 161-172, in `updateMeetingTitle`)

Same pattern, same fix. `setMeetingTitle(newTitle);` line is unchanged.

**Why spread instead of adding the field by name:** `sidebarMeetings: CurrentMeeting[]` and `CurrentMeeting` has `id`, `title`, `created_at?`, `folder_id?`. Spreading `{...m, title: meetingTitle}` is robust against future field additions to `CurrentMeeting` and follows the principle of preserving all fields by default. This is what `setMeetings` and `setCurrentMeeting` should always do — the bug was that the previous code built a literal with only the changed field, which works for a single-rename but is fragile to schema changes.

**Audit of all `setCurrentMeeting` / `setMeetings` callsites (Task 5.5b):**

| File | Line | Status | Notes |
|------|------|--------|-------|
| `SidebarProvider.tsx` | 99, 213 | OK | `useState` init / reset to intro-call stub (no date by design) |
| `SidebarProvider.tsx` | 154 | OK | `fetchMeetings` builds all 4 fields |
| `SidebarProvider.tsx` | 158 | OK | `setMeetings([])` |
| `SidebarProvider.tsx` | 457-459 | OK | `setMeetings(prev => prev.map(m => m.id === meetingId ? { ...m, folder_id } : m))` — spread |
| `Sidebar/index.tsx` | 405 | OK | `setMeetings(meetings.filter(...))` — preserves fields |
| `Sidebar/index.tsx` | 417 | OK | Reset to intro-call stub (no date by design) |
| `Sidebar/index.tsx` | 467 | OK | `setMeetings(meetings.map(... ? { ...m, title } : m))` — spread |
| `Sidebar/index.tsx` | 472-476 | OK | Carries `created_at: meeting?.created_at` (Sprint 5) |
| `meeting-details/page.tsx` | 150-154 | OK | Carries `created_at: metadata.created_at` (Sprint 2) |
| `useRecordingStop.ts` | 358-362 | OK | Carries `created_at: meetingData.created_at` (Sprint 2) |
| `useRecordingStop.ts` | 367-370 | OK | Catch-branch fallback; no `created_at` source. `refetchMeetings()` re-populates `meetings[]` immediately before. No user-visible bug. |
| `useMeetingData.ts` | 67-75, 161-172 | **FIXED** | Was dropping `created_at` on title save |
| `useNavigation.ts` | 11 | Latent | Has the same stale-literal pattern, but **zero callers** in the source tree. Not live. |

**Files changed (1):**
- `frontend/src/hooks/meeting-details/useMeetingData.ts`

**Verification (post-fix):**
- `tsc --noEmit` ✓
- `vitest run` ✓ (5 files, 19 tests, all pass)
- `prettier --check` ✓ (file reformatted with prettier; no functional change)
- Subagent review (Task 5.5d) ✓ PASS

**Final final verdict:** PASS. Feature is now feature-complete, documented, consistent, and the blocker is resolved. The only remaining open follow-up is the manual visual check by the user before merging.

---

## Closing Notes

**Why the blocker slipped through:** Sprint 5's review was a polish pass on Sprint 5's two changes (user guide + sidebar rename). It did not call back to Sprint 2's change to `setCurrentMeeting`/`setMeetings` types — which is exactly the kind of cross-sprint regression that the AGENTS.md "Grep every caller" rule is designed to prevent. Lesson: when widening a type (Sprint 2 added `created_at?` to `CurrentMeeting`), every callsite of the related setters must be re-grepped for "did the new field get dropped in a fresh object literal?". The final review caught it because the reviewer treated the entire feature as a single end-to-end unit, not as five independent sprints.

**Latent issues (out of scope, non-blocking):**
- `useNavigation.ts:11` has the same stale-literal pattern but zero callers. Either delete the file or fix it. Defer.
- All `setCurrentMeeting` literals drop `folder_id` (the field is never read from `currentMeeting`, only from `meetings[]`). Defer.

**The meeting-date-display feature is complete and ready to merge.**

---

## Sprint 6 — Surgery & Ship

**Why this sprint exists:** The user said "ship it" with a working tree that contains the meeting-date-display feature (Sprints 1-5) PLUS 250+ other modified files and 60+ untracked files from concurrent feature work in flight (FTS5, MCP, Folders, Chat, custom vocab, multi-template summaries, etc.). Initial attempts to `git add` the 13 meeting-date-display files produced 4,398 inserted / 2,507 deleted lines — most of which were unrelated work mixed into the same files. The right call was a surgical revert-to-HEAD-then-re-apply for each tracked file, leaving the unrelated in-flight work unstaged.

**Already done in Sprint 6.0 (pre-plan, the work that exposed the problem):**
1. Created `enhance/meeting-date-display` branch.
2. Reverted 8 tracked files to HEAD to start clean:
   - `frontend/src-tauri/src/api/api.rs`
   - `frontend/src/components/Sidebar/SidebarProvider.tsx`
   - `frontend/src/app/meeting-details/page.tsx`
   - `frontend/src/hooks/useRecordingStop.ts`
   - `frontend/src/components/Sidebar/index.tsx`
   - `frontend/src/components/MeetingDetails/SummaryPanel.tsx`
   - `frontend/src/lib/utils.ts`
   - `frontend/src/hooks/meeting-details/useMeetingData.ts`
3. Surgically re-applied 4 of those 8 (verified clean diffs):
   - `api.rs` — added `pub created_at: String,` to `Meeting` struct + `created_at: m.created_at.0.to_rfc3339(),` to the `api_get_meetings` mapping.
   - `SidebarProvider.tsx` — added `created_at?: string` to `CurrentMeeting` interface + propagated through `fetchMeetings`.
   - `page.tsx` — added `created_at: metadata.created_at` to `setCurrentMeeting` after a meeting is opened.
   - `useRecordingStop.ts` — added `created_at: meetingData.created_at` to `setCurrentMeeting` in the success branch.

**Remaining sub-sprints (4 + 1 = 5):**

| Sub-sprint | File | What | Status |
|------------|------|------|--------|
| 6.1 | `lib/utils.ts` + `tests/lib/utils.test.ts` | Re-add `formatMeetingDate` helper + 2 tests | pending |
| 6.2 | `hooks/meeting-details/useMeetingData.ts` | Re-apply Sprint 5 fix (spread instead of literal at 2 spots) | pending |
| 6.3 | `components/MeetingDetails/SummaryPanel.tsx` | Re-apply Sprint 4 (EditableTitle uncommented + `<time>` date line) | pending |
| 6.4 | `components/Sidebar/index.tsx` | Re-apply Sprint 3+5 (~300 lines: import, renderMeetingNode, 3 callsites, rename fix) | pending |
| 6.5 | All 13 files | Stage, commit, push, final review | pending |

**Sprint 6 process:**
- For each sub-sprint: subagent re-applies the changes per the original sprint plan; subagent reviews the resulting diff; orchestrator updates this log.
- For the big sub-sprint (6.4): the subagent gets the full Sprint 3 plan as context to recreate the new code in the reverted file.

**Out of scope (left for a future commit):**
- The 250+ other modified files (FTS5, MCP, Folders, Chat, custom vocab, multi-template summaries, etc.) — these are in-flight features not yet ready to commit.
- `useNavigation.ts:11` stale-literal — zero callers, deferred.
- All `setCurrentMeeting` literals drop `folder_id` — `currentMeeting.folder_id` never read, deferred.

---

### Sub-sprint 6.1: `lib/utils.ts` (Sprint 1) + tests

**Implemented:**
- `upstream/frontend/src/lib/utils.ts` — appended `formatMeetingDate(iso, format)` helper at the end of the file. 33 lines added, no other code modified. Used the file's existing convention (2-space indent, single quotes, `pattern =>` arrow param style).
- `upstream/frontend/tests/lib/utils.test.ts` — re-created (it was untracked in the working tree) with 3 vitest tests covering the helper.

**Sprint-end review (subagent):** First pass returned FAIL — review caught that running prettier on `lib/utils.ts` after the revert reformatted the entire file (2→4 space indent, single→double quotes, etc.) instead of just appending the function. The Sprint 1 reviewer's "Drive-by" was the prettier reformatting, but for the Sprint 6 commit we want a strictly additive diff. **Fix:** reverted to HEAD again and re-applied the function in the file's exact HEAD style (2-space, single quotes, no parens). Result: 41-line diff, all of which are the new function (1 blank line + 33 lines of code + the closing brace is the new line 65).

**Verification (post-fix):**
- `git diff upstream/frontend/src/lib/utils.ts` — clean additive diff, no modifications to existing code.
- `vitest run tests/lib/utils.test.ts` — 3/3 tests pass.
- `tsc --noEmit` — no new errors in `lib/utils.ts` or `utils.test.ts` (pre-existing errors in unrelated files are out of scope).

**Sprint 6.1 verdict:** PASS. Ready to commit.

---

### Sub-sprint 6.2: `useMeetingData.ts` (Sprint 5 fix)

**Implemented:**
- `upstream/frontend/src/hooks/meeting-details/useMeetingData.ts` — re-applied the Sprint 5 fix at both call sites:
  - In `handleSaveMeetingTitle` (around line 60-70): replaced `{ id: m.id, title: meetingTitle }` with `{ ...m, title: meetingTitle }` and added `created_at: meeting.created_at` to the `setCurrentMeeting` call.
  - In `updateMeetingTitle` (around line 150-160): same pattern for the AI-suggested title rename path.

**Sprint-end review (subagent):** PASS. Exactly 4 hunks, no other changes. `CurrentMeeting` fields all preserved by the spread. `setCurrentMeeting` carries `created_at` at both sites. Dep arrays unchanged. No new TypeScript errors. Blocker resolved.

**Verification:**
- `git diff upstream/frontend/src/hooks/meeting-details/useMeetingData.ts` — clean 4-hunk diff.
- `tsc --noEmit` — no new errors in this file.

**Sprint 6.2 verdict:** PASS. Ready to commit.

---

### Sub-sprint 6.3: `SummaryPanel.tsx` (Sprint 4)

**Implemented:**
- `upstream/frontend/src/components/MeetingDetails/SummaryPanel.tsx` — re-applied the Sprint 4 changes:
  - Added `import { formatMeetingDate } from '@/lib/utils';` (line 21).
  - Hoisted `const formattedFullDate = formatMeetingDate(meeting.created_at, 'full');` at the top of the top-level return block (line 256).
  - Uncommented the `<EditableTitle>` block (was previously disabled, lines 256-262).
  - Added the date sub-line as `<time dateTime={meeting.created_at} className="text-sm text-gray-500 mt-2 block">` directly below the `<EditableTitle>` (lines 263-270).

**Sprint-end review (subagent):** PASS. Exactly 2 hunks, +13/-2, only `SummaryPanel.tsx` touched. All 5 `<EditableTitle>` props present. `summaryGeneratorButtonGroup` block at line 279 still correctly gated. Hoisted const is at top level, not in a nested function (safe for re-renders). `mt-2` (8px) is a deliberate spacing change from the original `mt-1` (4px), per the spec. No regressions.

**Style note:** The subagent matched the file's actual style (2-space indent, single quotes) per AGENTS.md's "Mimic code style" rule. My initial spec said 4-space + double quotes, but the file uses 2-space + single quotes. The subagent's choice was correct.

**Verification:**
- `git diff upstream/frontend/src/components/MeetingDetails/SummaryPanel.tsx` — clean 2-hunk diff.
- `tsc --noEmit` — no new errors in this file.

**Sprint 6.3 verdict:** PASS. Ready to commit.

---

### Sub-sprint 6.4: `Sidebar/index.tsx` (Sprint 3+5)

**Implemented (smaller-scoped approach):**

The original Sprint 3 plan called for restructuring `Sidebar/index.tsx` to use a new `MeetingTreeItem` component and a folder-tree structure. That restructure depends on the Folders feature (`useSidebarTree`, `FolderTreeItem`, `MoveToFolderModal`, `FolderFilterTree`) which is incomplete — the component files exist as untracked but there's no `useFolders` hook or folder state management in the context. A previous subagent attempt to do the full restructure went off-scope: it invented local folder state with `window.prompt`, cast `meetings as any`, and expanded `setMoveToModal` calls. All of that was reverted.

The final approach is the smallest change that delivers the meeting-date-display sidebar feature without pulling in incomplete Folders work:

1. **Import added** (line 13): `import { formatMeetingDate } from '@/lib/utils';`
2. **Hoisted consts in `renderItem`** (lines 561-563):
   ```ts
   const meeting = isMeetingItem ? meetings.find((m) => m.id === item.id) : null;
   const dateStr = meeting ? formatMeetingDate(meeting.created_at, 'short') : '';
   ```
3. **Date sub-line in JSX** (line 650):
   ```tsx
   {dateStr && <div className="mt-1 ml-8 text-xs text-gray-500">{dateStr}</div>}
   ```

**Why this approach:**
- **Smallest possible diff**: 8 insertions / 0 deletions, one file. No new components, no new state, no tree structure.
- **No incomplete dependencies**: `MeetingTreeItem` is tightly coupled to the Folders feature (drag/drop, move-to-folder, folder/chunk-type badges). Pulling it in brings unfinished state. The existing `renderItem` function in HEAD already renders meetings inline — we just add the date sub-line there.
- **Reuses the existing pattern**: `meetings.find()` is O(n) per row, which mirrors the existing `findMatchingSnippet` two lines below (line 562). Consistent with the codebase.
- **Aligns perfectly**: `ml-8` (32px) = icon `w-6` (24px) + `mr-2` (8px), so the date aligns exactly with the title text. Same classes as the transcript match snippet block below it.

**What was NOT done (intentionally, out of scope):**
- No `MeetingTreeItem` component used (it's coupled to Folders).
- No tree structure (`useSidebarTree`, `FolderTreeItem`, `MoveToFolderModal` not used).
- No folder state management added (that belongs to the Folders feature).
- No FTS search-results row added (that belongs to the FTS feature).
- No changes to the folder rendering branch in `renderItem`.
- No new hooks or effects.

**Sprint-end review (subagent):** PASS. Exactly 3 hunks, 8 insertions / 0 deletions, one file. `tsc --noEmit` reports zero errors in this file. All 11 pre-existing errors are in other in-flight feature files (`activeTemplateId`, `sidebarWidth`, `selectedTemplate`, `SummaryStatusResponse`). The scope discipline paid off: this is the smallest diff that delivers the feature, reuses the existing helper and pattern, and invents nothing.

**Verification:**
- `git diff upstream/frontend/src/components/Sidebar/index.tsx` — clean 3-hunk diff, +8/-0.
- `tsc --noEmit` — no new errors in this file.

**Sprint 6.4 verdict:** PASS. Ready to commit.

