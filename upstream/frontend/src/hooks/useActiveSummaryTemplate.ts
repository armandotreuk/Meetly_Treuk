import { useCallback, useEffect, useState } from "react";

import type { MeetingSummaryInfo } from "@/types";

const STORAGE_KEY_PREFIX = "meetily:active-template";

function storageKey(meetingId: string): string {
    return `${STORAGE_KEY_PREFIX}:${meetingId}`;
}

function readStored(meetingId: string): string | null {
    if (typeof window === "undefined") return null;
    try {
        const raw = window.localStorage.getItem(storageKey(meetingId));
        if (!raw) return null;
        const parsed = JSON.parse(raw);
        return typeof parsed === "string" ? parsed : null;
    } catch {
        return null;
    }
}

function writeStored(meetingId: string, value: string | null): void {
    if (typeof window === "undefined") return;
    try {
        const key = storageKey(meetingId);
        if (value) window.localStorage.setItem(key, JSON.stringify(value));
        else window.localStorage.removeItem(key);
    } catch {
        // localStorage may be unavailable (incognito/quota); non-critical.
    }
}

function pickFallback(summaries: MeetingSummaryInfo[]): string | null {
    if (summaries.length === 0) return null;
    if (summaries.length === 1) return summaries[0].template_id;

    const completed = summaries
        .filter((s) => s.status === "completed")
        .sort((a, b) => (a.updated_at < b.updated_at ? 1 : -1));
    if (completed.length > 0) return completed[0].template_id;

    const newest = [...summaries].sort((a, b) =>
        a.updated_at < b.updated_at ? 1 : -1
    );
    return newest[0].template_id;
}

export function useActiveSummaryTemplate(
    meetingId: string | null | undefined,
    summaries: MeetingSummaryInfo[] = []
) {
    const [activeTemplateId, setActiveTemplateIdState] = useState<string | null>(
        () => (meetingId ? readStored(meetingId) : null)
    );

    useEffect(() => {
        if (!meetingId) {
            setActiveTemplateIdState(null);
            return;
        }
        const stored = readStored(meetingId);
        const exists =
            stored != null && summaries.some((s) => s.template_id === stored);
        if (stored != null && !exists) {
            writeStored(meetingId, null);
            setActiveTemplateIdState(pickFallback(summaries));
            return;
        }
        if (stored != null) {
            setActiveTemplateIdState(stored);
            return;
        }
        setActiveTemplateIdState(pickFallback(summaries));
        // ponytail: localStorage-only state, no cross-tab sync via the
        // `storage` event (unlike useRecentLanguages, which mirrors it).
        // Ceiling: two tabs of the same meeting can diverge on the active
        // template after one switches. Upgrade path: listen to `storage`
        // for STORAGE_KEY_PREFIX keys and call setActiveTemplateIdState.
    }, [meetingId, summaries]);

    const setActiveTemplateId = useCallback(
        (id: string | null) => {
            if (!meetingId) {
                setActiveTemplateIdState(id);
                return;
            }
            writeStored(meetingId, id);
            setActiveTemplateIdState(id);
        },
        [meetingId]
    );

    return { activeTemplateId, setActiveTemplateId };
}