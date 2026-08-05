export interface SummaryPollIdentity {
    meetingId: string;
    templateId: string;
    generation: string;
}

export function summaryPollKey(identity: SummaryPollIdentity): string {
    return [identity.meetingId, identity.templateId, identity.generation].join("\u0000");
}

export function isCurrentSummaryPoll(
    activeKey: string | null | undefined,
    identity: SummaryPollIdentity
): boolean {
    return activeKey === summaryPollKey(identity);
}

export function shouldApplySummaryPollResult(
    result: { status?: string; template_id?: string },
    identity: Pick<SummaryPollIdentity, "templateId">
): boolean {
    if (result.template_id && result.template_id !== identity.templateId) return false;
    // An exact-generation backend read returns idle with no template when the
    // row was replaced/deleted. That is not evidence that a newer UI row is idle.
    return !(result.status === "idle" && !result.template_id);
}
