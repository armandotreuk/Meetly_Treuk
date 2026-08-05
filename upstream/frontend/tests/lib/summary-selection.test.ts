import { describe, expect, it } from "vitest";

import {
    fallbackAfterSummaryDelete,
    resolveActiveSummaryTemplate,
    summaryResponseMatchesTemplate,
} from "@/lib/summary-selection";
import { buildSummaryLookupArgs } from "@/lib/summary-command-args";

const row = (template_id: string, updated_at: string) => ({
    template_id,
    status: "completed" as const,
    updated_at,
});

describe("page-level summary selection", () => {
    it("uses the stored row, then newest row, then the standard new-meeting template", () => {
        const summaries = [
            row("daily_standup", "2026-08-03T10:00:00Z"),
            row("db:42", "2026-08-03T12:00:00Z"),
        ];

        expect(resolveActiveSummaryTemplate(summaries, "daily_standup")).toBe("daily_standup");
        expect(resolveActiveSummaryTemplate(summaries, "removed-template")).toBe("db:42");
        expect(resolveActiveSummaryTemplate([])).toBe("standard_meeting");
    });

    it("loads and accepts only the exact selected template", () => {
        expect(buildSummaryLookupArgs("meeting-1", "file:42")).toEqual({
            meetingId: "meeting-1",
            templateId: "file:42",
        });
        expect(summaryResponseMatchesTemplate({ template_id: "db:42" }, "db:42")).toBe(true);
        expect(summaryResponseMatchesTemplate({ template_id: "daily_standup" }, "db:42")).toBe(
            false,
        );
    });

    it("selects the next listed row after deleting the active row", () => {
        const summaries = [
            row("standard_meeting", "2026-08-03T12:00:00Z"),
            row("daily_standup", "2026-08-03T11:00:00Z"),
            row("db:42", "2026-08-03T10:00:00Z"),
        ];

        expect(fallbackAfterSummaryDelete(summaries, "standard_meeting")).toBe("daily_standup");
        expect(fallbackAfterSummaryDelete(summaries, "daily_standup")).toBe("db:42");
        expect(fallbackAfterSummaryDelete([summaries[0]], "standard_meeting")).toBe(
            "standard_meeting",
        );
    });
});
