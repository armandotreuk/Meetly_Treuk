import { describe, expect, it } from "vitest";

import { canAutoGenerateSummary, isSummaryInProgress } from "@/lib/summary-status";

describe("isSummaryInProgress", () => {
    it("recognizes all processing states", () => {
        expect(isSummaryInProgress("pending")).toBe(true);
        expect(isSummaryInProgress(" PENDING ")).toBe(true);
        expect(isSummaryInProgress("processing")).toBe(true);
        expect(isSummaryInProgress("summarizing")).toBe(true);
        expect(isSummaryInProgress("regenerating")).toBe(true);
    });

    it("does not classify terminal or unknown states as processing", () => {
        expect(isSummaryInProgress("completed")).toBe(false);
        expect(isSummaryInProgress("failed")).toBe(false);
        expect(isSummaryInProgress(undefined)).toBe(false);
    });

    it("waits for the initial summary check before allowing auto-generation", () => {
        expect(
            canAutoGenerateSummary({
                initialSummaryLoaded: false,
                isSummaryProcessing: false,
                hasCheckedAutoGen: false,
                hasTranscripts: true,
            })
        ).toBe(false);
        expect(
            canAutoGenerateSummary({
                initialSummaryLoaded: true,
                isSummaryProcessing: true,
                hasCheckedAutoGen: false,
                hasTranscripts: true,
            })
        ).toBe(false);
        expect(
            canAutoGenerateSummary({
                initialSummaryLoaded: true,
                isSummaryProcessing: false,
                hasCheckedAutoGen: false,
                hasTranscripts: true,
            })
        ).toBe(true);
    });
});
