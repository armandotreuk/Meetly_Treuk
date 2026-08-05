import type { MeetingSummaryInfo } from "@/types";
import { normalizeTemplateId } from "@/lib/template-ids";

export const LEGACY_TEMPLATE_ID = "legacy";
export const LEGACY_TEMPLATE_DISPLAY_NAME = "Summary (original)";

export function isExportableSummary(status: string | undefined | null): boolean {
    return status === "completed";
}

export function findCompletedSummaryRow(
    rows: readonly MeetingSummaryInfo[],
    templateId: string
): MeetingSummaryInfo | undefined {
    const target = normalizeTemplateId(templateId);
    return rows.find((row) => normalizeTemplateId(row.template_id) === target && row.status === "completed");
}

export function buildExportPdfRequest(
    meetingId: string,
    templateId: string,
    templateSource?: string | null
): { meeting_id: string; template_id: string; template_source?: string } {
    const request: { meeting_id: string; template_id: string; template_source?: string } = {
        meeting_id: meetingId,
        template_id: normalizeTemplateId(templateId, templateSource ?? undefined),
    };
    if (templateSource) request.template_source = templateSource;
    return request;
}
