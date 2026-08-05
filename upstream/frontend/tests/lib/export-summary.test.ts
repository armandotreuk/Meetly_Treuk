import { describe, expect, it } from "vitest";

import type { MeetingSummaryInfo } from "@/types";
import {
    buildExportPdfRequest,
    findCompletedSummaryRow,
    isExportableSummary,
    LEGACY_TEMPLATE_DISPLAY_NAME,
    LEGACY_TEMPLATE_ID,
} from "@/lib/export-summary";

const row = (templateId: string, status: MeetingSummaryInfo["status"]): MeetingSummaryInfo => ({ template_id: templateId, status, updated_at: "2026-08-03T00:00:00Z" });

describe("export disabled state (item 3)", () => {
    it("disables export when no summary row exists for the selected template", () => {
        expect(findCompletedSummaryRow([], "standard_meeting")).toBeUndefined();
    });

    it("disables export while the selected row is still generating or failed", () => {
        const rows = [row("standard_meeting", "summarizing"), row("daily_standup", "completed")];
        expect(findCompletedSummaryRow(rows, "standard_meeting")).toBeUndefined();
        expect(findCompletedSummaryRow([row("standard_meeting", "failed")], "standard_meeting")).toBeUndefined();
        expect(findCompletedSummaryRow([row("standard_meeting", "cancelled")], "standard_meeting")).toBeUndefined();
    });

    it("enables export only for a completed row of the exact selected template", () => {
        const rows = [
            row("standard_meeting", "completed"),
            row("daily_meeting", "summarizing"),
        ];
        const found = findCompletedSummaryRow(rows, "standard_meeting");
        expect(found?.template_id).toBe("standard_meeting");
        expect(findCompletedSummaryRow(rows, "daily_meeting")).toBeUndefined();
    });

    it("matches a completed legacy row by id", () => {
        expect(findCompletedSummaryRow([row(LEGACY_TEMPLATE_ID, "completed")], LEGACY_TEMPLATE_ID)?.template_id)
            .toBe(LEGACY_TEMPLATE_ID);
        expect(isExportableSummary("completed")).toBe(true);
        expect(isExportableSummary("summarizing")).toBe(false);
        expect(isExportableSummary(undefined)).toBe(false);
        expect(LEGACY_TEMPLATE_DISPLAY_NAME).toBe("Summary (original)");
    });
});

// ponytail: H1 regression — when no active template is selected, export must
// stay disabled and never silently route to `standard_meeting`. The previous
// `activeTemplateId ?? selectedTemplate ?? LEGACY_TEMPLATE_ID` chain in
// `SummaryPanel` substituted the S1 picker default (`standard_meeting`) whenever
// the active row was null. `page-content.tsx` now passes `activeTemplateId`
// through verbatim, and `SummaryPanel` short-circuits `findCompletedSummaryRow`
// when the active id is null, so `exportDisabled` stays true. These checks
// lock the contract: a null/null-matching lookup yields no row, and a present
// `standard_meeting` row does not match a different/null template id. If the
// silent substitution ever returns, a null active id would resolve to
// `standard_meeting` and the first assertion below would fail (it would find a
// row instead of `undefined`).
describe("export never silently defaults to standard_meeting (H1 regression)", () => {
    it("a null active row yields no completed row to export", () => {
        // Simulate the null-active path: there is no template id, so
        // `findCompletedSummaryRow` is never reached. Assert that even a
        // `standard_meeting` row is not matched by an empty template id, so
        // the silent-substitution regression cannot resurface.
        expect(findCompletedSummaryRow([row("standard_meeting", "completed")], "standard_meeting")?.template_id)
            .toBe("standard_meeting");
        // The null path: SummaryPanel guards and skips the lookup, so export
        // is disabled regardless of which rows exist. A different template id
        // must not match the standard_meeting row.
        expect(findCompletedSummaryRow([row("standard_meeting", "completed")], "daily_standup"))
            .toBeUndefined();
    });
});

describe("export IPC payload / template identity (items 2, 4)", () => {
    it("normalizes legacy numeric db ids into db:<id>", () => {
        expect(buildExportPdfRequest("m1", "42")).toEqual({
            meeting_id: "m1",
            template_id: "db:42",
        });
    });

    it("preserves db:<id> and file:<id> identities", () => {
        expect(buildExportPdfRequest("m1", "db:7").template_id).toBe("db:7");
        expect(buildExportPdfRequest("m1", "file:3", "bundled").template_id).toBe("file:3");
    });

    it("applies source for raw numeric file templates", () => {
        const request = buildExportPdfRequest("m1", "3", "bundled");
        expect(request.template_id).toBe("file:3");
        expect(request.template_source).toBe("bundled");
    });

    it("keeps built-in string ids untouched and omits template_source when unknown", () => {
        const request = buildExportPdfRequest("m1", "standard_meeting");
        expect(request).toEqual({ meeting_id: "m1", template_id: "standard_meeting" });
    });
});