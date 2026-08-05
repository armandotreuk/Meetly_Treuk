import type { MeetingSummaryInfo } from "@/types";
import { normalizeTemplateId } from "@/lib/template-ids";

export const DEFAULT_SUMMARY_TEMPLATE_ID = "standard_meeting";

function newestFirst(left: MeetingSummaryInfo, right: MeetingSummaryInfo): number {
    const byDate = right.updated_at.localeCompare(left.updated_at);
    return byDate || left.template_id.localeCompare(right.template_id);
}

export function resolveActiveSummaryTemplate(
    summaries: MeetingSummaryInfo[],
    storedTemplateId?: string | null,
    defaultTemplateId = DEFAULT_SUMMARY_TEMPLATE_ID
): string {
    const normalizedStoredId = storedTemplateId
        ? normalizeTemplateId(storedTemplateId)
        : null;
    if (
        normalizedStoredId &&
        summaries.some((summary) => normalizeTemplateId(summary.template_id) === normalizedStoredId)
    ) {
        return normalizedStoredId;
    }

    const newest = [...summaries].sort(newestFirst)[0];
    return newest ? normalizeTemplateId(newest.template_id) : defaultTemplateId;
}

export function fallbackAfterSummaryDelete(
    summaries: MeetingSummaryInfo[],
    deletedTemplateId: string,
    defaultTemplateId = DEFAULT_SUMMARY_TEMPLATE_ID
): string {
    const deletedId = normalizeTemplateId(deletedTemplateId);
    const index = summaries.findIndex(
        (summary) => normalizeTemplateId(summary.template_id) === deletedId,
    );
    if (index < 0) return defaultTemplateId;

    return (summaries[index + 1] && normalizeTemplateId(summaries[index + 1].template_id))
        ?? (summaries[index - 1] && normalizeTemplateId(summaries[index - 1].template_id))
        ?? defaultTemplateId;
}

export function summaryResponseMatchesTemplate(
    response: { template_id?: string | null },
    templateId: string
): boolean {
    if (!response.template_id) return true;
    return normalizeTemplateId(response.template_id) === normalizeTemplateId(templateId);
}
