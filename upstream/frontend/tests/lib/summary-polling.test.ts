import { describe, expect, it } from "vitest";

import {
    isCurrentSummaryPoll,
    shouldApplySummaryPollResult,
    summaryPollKey,
} from "@/lib/summary-polling";

describe("summary polling identity", () => {
    it("distinguishes meetings, templates, and generations", () => {
        const oldRun = { meetingId: "meeting-1", templateId: "db:42", generation: "old" };
        const newRun = { ...oldRun, generation: "new" };

        expect(summaryPollKey(oldRun)).not.toBe(summaryPollKey(newRun));
        expect(isCurrentSummaryPoll(summaryPollKey(newRun), oldRun)).toBe(false);
        expect(isCurrentSummaryPoll(summaryPollKey(newRun), newRun)).toBe(true);
    });

    it("does not apply an exact-generation idle response to newer state", () => {
        expect(shouldApplySummaryPollResult({ status: "idle" }, { templateId: "db:42" })).toBe(
            false
        );
        expect(
            shouldApplySummaryPollResult(
                { status: "completed", template_id: "file:42" },
                { templateId: "db:42" }
            )
        ).toBe(false);
        expect(
            shouldApplySummaryPollResult(
                { status: "completed", template_id: "db:42" },
                { templateId: "db:42" }
            )
        ).toBe(true);
    });
});
