import { describe, expect, it } from "vitest";

import {
    buildSummaryCancelArgs,
    buildSummaryFetchArgs,
    buildSummarySaveArgs,
} from "@/lib/summary-command-args";

describe("summary command identities", () => {
    const revision = {
        templateId: "db:42",
        startTime: "2026-08-03T12:00:00Z",
        updatedAt: "2026-08-03T12:05:00Z",
    };

    it("keeps the selected non-default template and revision on manual saves", () => {
        expect(
            buildSummarySaveArgs({
                meetingId: "meeting-1",
                summary: { markdown: "edited" },
                templateId: "db:42",
                revision,
            })
        ).toEqual({
            meetingId: "meeting-1",
            summary: { markdown: "edited" },
            templateId: "db:42",
            expectedStartTime: revision.startTime,
            expectedUpdatedAt: revision.updatedAt,
        });
    });

    it("does not rewrite canonical, legacy numeric, or file identities", () => {
        for (const templateId of ["db:42", "42", "file:42"]) {
            expect(
                buildSummarySaveArgs({
                    meetingId: "meeting-1",
                    summary: { markdown: "edited" },
                    templateId,
                    revision: { ...revision, templateId },
                }).templateId,
            ).toBe(templateId);
        }
    });

    it("does not invent a default template or unfenced save revision", () => {
        expect(() =>
            buildSummarySaveArgs({
                meetingId: "meeting-1",
                summary: { markdown: "edited" },
            })
        ).toThrow();
        expect(() =>
            buildSummarySaveArgs({
                meetingId: "meeting-1",
                summary: { markdown: "edited" },
                templateId: "standard_meeting",
                revision,
            })
        ).toThrow();
    });

    // ponytail: H1 regression — a null/undefined `activeTemplateId` must never
    // route the save payload to `standard_meeting`. `page-content.tsx` stopped
    // substituting `templates.selectedTemplate` (which seeds as
    // `"standard_meeting"`); the save path's trust boundary is
    // `buildSummarySaveArgs`. Save is allowed only when the revision pins a
    // template (`resolvedTemplateId = templateId ?? revision?.templateId`); an
    // all-null orphaned-active state (no active id AND no revision) must throw
    // rather than invent `standard_meeting`. If this guard ever regresses, a
    // post-delete null with no revision would silently write a
    // `standard_meeting` row the user never selected.
    it("rejects a null/undefined active template with no revision instead of saving as standard_meeting", () => {
        expect(() =>
            buildSummarySaveArgs({
                meetingId: "meeting-1",
                summary: { markdown: "edited" },
                templateId: null,
                revision: null,
            })
        ).toThrow();
        expect(() =>
            buildSummarySaveArgs({
                meetingId: "meeting-1",
                summary: { markdown: "edited" },
                templateId: undefined,
            })
        ).toThrow();
    });

    it("keeps the revision's template id when the active id is null (no silent standard_meeting)", () => {
        // A null active id with a pinned revision must save under the
        // revision's template, never fall back to `standard_meeting`.
        expect(
            buildSummarySaveArgs({
                meetingId: "meeting-1",
                summary: { markdown: "edited" },
                templateId: null,
                revision,
            }).templateId,
        ).toBe("db:42");
    });

    it("carries the exact template and generation through cancel and reload args", () => {
        const identity = { templateId: "file:42", generation: "2026-08-03T12:00:00Z" };
        expect(buildSummaryCancelArgs("meeting-1", identity)).toEqual({
            meetingId: "meeting-1",
            templateId: "file:42",
            generation: identity.generation,
        });
        expect(buildSummaryFetchArgs("meeting-1", identity)).toEqual({
            meetingId: "meeting-1",
            templateId: "file:42",
            generation: identity.generation,
        });
        expect(() => buildSummaryCancelArgs("meeting-1", null)).toThrow();
    });
});
