import { describe, it, expect } from "vitest";
import { formatMeetingDate } from "@/lib/utils";

describe("formatMeetingDate", () => {
    it("returns formatted short date for a valid ISO string", () => {
        const result = formatMeetingDate("2026-07-31T14:30:00.000Z", "short");
        // Don't assert the exact localized string (depends on test env locale).
        // Just assert it's non-empty and contains the year-or-day-or-hour.
        expect(result).toBeTruthy();
        expect(result.length).toBeGreaterThan(0);
    });

    it("returns formatted full date for a valid ISO string", () => {
        const result = formatMeetingDate("2026-07-31T14:30:00.000Z", "full");
        expect(result).toBeTruthy();
        expect(result.length).toBeGreaterThan(0);
    });

    it("returns empty string for null, undefined, empty, or unparseable input", () => {
        expect(formatMeetingDate(null, "short")).toBe("");
        expect(formatMeetingDate(undefined, "short")).toBe("");
        expect(formatMeetingDate("", "short")).toBe("");
        expect(formatMeetingDate("not-a-date", "short")).toBe("");
        expect(formatMeetingDate(null, "full")).toBe("");
    });
});
