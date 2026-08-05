import { useState, useEffect, useCallback } from "react";
import { logger } from "@/lib/logger";

import { invoke as invokeTauri } from "@tauri-apps/api/core";
import type { MeetingSummaryInfo } from "@/types";
import { normalizeTemplateId } from "@/lib/template-ids";

export function useMeetingSummaries(meetingId: string | null | undefined) {
    const [summaries, setSummaries] = useState<MeetingSummaryInfo[]>([]);
    const [loading, setLoading] = useState<boolean>(Boolean(meetingId));
    const [loadedMeetingId, setLoadedMeetingId] = useState<string | null>(null);
    const [error, setError] = useState<string | null>(null);

    const fetchSummaries = useCallback(async (id: string) => {
        setLoading(true);
        setError(null);
        try {
            const rows = await invokeTauri<MeetingSummaryInfo[]>(
                "api_list_meeting_summaries",
                { meetingId: id }
            );
            setSummaries(
                rows.map((row) => ({
                    ...row,
                    template_id: normalizeTemplateId(row.template_id),
                }))
            );
        } catch (e) {
            const msg = e instanceof Error ? e.message : String(e);
            logger.error("useMeetingSummaries: fetch failed", e);
            setError(msg);
        } finally {
            setLoading(false);
            setLoadedMeetingId(id);
        }
    }, []);

    useEffect(() => {
        if (!meetingId) {
            setSummaries([]);
            setError(null);
            setLoading(false);
            setLoadedMeetingId(null);
            return;
        }
        setSummaries([]);
        setLoading(true);
        setLoadedMeetingId(null);
        void fetchSummaries(meetingId);
    }, [meetingId, fetchSummaries]);

    const refresh = useCallback(async () => {
        if (!meetingId) return;
        await fetchSummaries(meetingId);
        // ponytail: one-shot fetch, no SWR/react-query cache; callers MUST
        // call refresh() after every mutation (delete/switch/generate) to
        // reflect server state. Ceiling: a stale list between writes,
        // acceptable since item 18/26 layer polling/refetch on top.
    }, [meetingId, fetchSummaries]);

    return {
        summaries,
        loading,
        error,
        refresh,
        ready: Boolean(meetingId && loadedMeetingId === meetingId),
    };
}
