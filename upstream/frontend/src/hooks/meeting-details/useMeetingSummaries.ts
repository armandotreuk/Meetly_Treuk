import { useState, useEffect, useCallback } from "react";
import { logger } from "@/lib/logger";

import { invoke as invokeTauri } from "@tauri-apps/api/core";
import type { MeetingSummaryInfo } from "@/types";

export function useMeetingSummaries(meetingId: string | null | undefined) {
    const [summaries, setSummaries] = useState<MeetingSummaryInfo[]>([]);
    const [loading, setLoading] = useState<boolean>(false);
    const [error, setError] = useState<string | null>(null);

    const fetchSummaries = useCallback(async (id: string) => {
        setLoading(true);
        setError(null);
        try {
            const rows = await invokeTauri<MeetingSummaryInfo[]>(
                "api_list_meeting_summaries",
                { meetingId: id }
            );
            setSummaries(rows);
        } catch (e) {
            const msg = e instanceof Error ? e.message : String(e);
            logger.error("useMeetingSummaries: fetch failed", e);
            setError(msg);
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        if (!meetingId) {
            setSummaries([]);
            setError(null);
            setLoading(false);
            return;
        }
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

    return { summaries, loading, error, refresh };
}