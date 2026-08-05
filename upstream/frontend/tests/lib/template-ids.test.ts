import { describe, expect, it } from "vitest";

import { databaseTemplateRowId, normalizeTemplateId } from "@/lib/template-ids";

describe("normalizeTemplateId", () => {
    it("canonicalizes legacy numeric database IDs", () => {
        expect(normalizeTemplateId(42)).toBe("db:42");
        expect(normalizeTemplateId("42")).toBe("db:42");
    });

    it("preserves built-in string IDs", () => {
        expect(normalizeTemplateId("daily_standup")).toBe("daily_standup");
    });

    it("keeps a numeric file ID in the file namespace when source is known", () => {
        expect(normalizeTemplateId(42, "bundled")).toBe("file:42");
    });

    it("parses source-safe database IDs for mutation commands", () => {
        expect(databaseTemplateRowId("db:42")).toBe(42);
        expect(() => databaseTemplateRowId("42-file")).toThrow();
    });
});
