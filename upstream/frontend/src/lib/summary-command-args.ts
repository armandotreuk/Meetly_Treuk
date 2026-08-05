import type { SummaryRevision } from "@/types";

export interface SummaryGenerationIdentity {
    templateId: string;
    generation: string;
}

export function buildSummarySaveArgs({
    meetingId,
    summary,
    templateId,
    revision,
}: {
    meetingId: string;
    summary: unknown;
    templateId?: string | null;
    revision?: SummaryRevision | null;
}) {
    const resolvedTemplateId = templateId ?? revision?.templateId;
    if (!resolvedTemplateId || !revision) {
        throw new Error("A summary template and revision are required to save");
    }
    if (revision.templateId !== resolvedTemplateId) {
        throw new Error("The summary template does not match its revision");
    }

    return {
        meetingId,
        summary,
        templateId: resolvedTemplateId,
        expectedStartTime: revision.startTime ?? undefined,
        expectedUpdatedAt: revision.updatedAt,
    };
}

export function buildSummaryCancelArgs(
    meetingId: string,
    identity: SummaryGenerationIdentity | null | undefined
) {
    if (!identity?.templateId || !identity.generation) {
        throw new Error("A summary template and generation are required to cancel");
    }

    return {
        meetingId,
        templateId: identity.templateId,
        generation: identity.generation,
    };
}

export function buildSummaryFetchArgs(meetingId: string, identity: SummaryGenerationIdentity) {
    return {
        meetingId,
        templateId: identity.templateId,
        generation: identity.generation,
    };
}

export function buildSummaryLookupArgs(meetingId: string, templateId: string) {
    if (!templateId) throw new Error("A summary template is required to load a summary");
    return { meetingId, templateId };
}
