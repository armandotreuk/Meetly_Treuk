import { useCallback, useEffect, useState } from "react";

import type { MeetingSummaryInfo } from "@/types";
import { normalizeTemplateId } from "@/lib/template-ids";
import {
    DEFAULT_SUMMARY_TEMPLATE_ID,
    resolveActiveSummaryTemplate,
} from "@/lib/summary-selection";

const STORAGE_KEY_PREFIX = "meeting.activeSummary.";
const LEGACY_STORAGE_KEY_PREFIX = "meetily:active-template:";

function storageKey(meetingId: string): string {
    return `${STORAGE_KEY_PREFIX}${meetingId}`;
}

function legacyStorageKey(meetingId: string): string {
    return `${LEGACY_STORAGE_KEY_PREFIX}${meetingId}`;
}

function readStored(meetingId: string): string | null {
    if (typeof window === "undefined") return null;
    try {
        const raw = window.localStorage.getItem(storageKey(meetingId))
            ?? window.localStorage.getItem(legacyStorageKey(meetingId));
        if (!raw) return null;
        let parsed: unknown = raw;
        try {
            parsed = JSON.parse(raw);
        } catch {
            // Accept the plain-string form as well as this hook's JSON form.
        }
        return typeof parsed === "string" ? normalizeTemplateId(parsed) : null;
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

export function useActiveSummaryTemplate(
    meetingId: string | null | undefined,
    summaries: MeetingSummaryInfo[] = [],
    summariesReady = true
) {
    const [activeTemplateId, setActiveTemplateIdState] = useState<string | null>(
        () => (meetingId ? readStored(meetingId) ?? DEFAULT_SUMMARY_TEMPLATE_ID : null)
    );
    const [resolvedMeetingId, setResolvedMeetingId] = useState<string | null>(null);

    useEffect(() => {
        if (!meetingId) {
            setActiveTemplateIdState(null);
            setResolvedMeetingId(null);
            return;
        }

        if (!summariesReady) {
            setActiveTemplateIdState(readStored(meetingId));
            setResolvedMeetingId(null);
            return;
        }

        const stored = readStored(meetingId);
        const exists =
            stored != null && summaries.some((s) => s.template_id === stored);
        if (stored != null && !exists) {
            writeStored(meetingId, null);
            setActiveTemplateIdState(resolveActiveSummaryTemplate(summaries));
        } else if (stored != null) {
            setActiveTemplateIdState(stored);
        } else {
            setActiveTemplateIdState(resolveActiveSummaryTemplate(summaries));
        }
        setResolvedMeetingId(meetingId);
        // ponytail: localStorage-only state, no cross-tab sync via the
        // `storage` event (unlike useRecentLanguages, which mirrors it).
        // Ceiling: two tabs of the same meeting can diverge on the active
        // template after one switches. Upgrade path: listen to `storage`
        // for STORAGE_KEY_PREFIX keys and call setActiveTemplateIdState.
    }, [meetingId, summaries, summariesReady]);

    const setActiveTemplateId = useCallback(
        (id: string | null) => {
            const normalizedId = id ? normalizeTemplateId(id) : null;
            if (!meetingId) {
                setActiveTemplateIdState(normalizedId);
                return;
            }
            writeStored(meetingId, normalizedId);
            setActiveTemplateIdState(normalizedId);
        },
        [meetingId]
    );

    return {
        activeTemplateId,
        setActiveTemplateId,
        ready: Boolean(meetingId && summariesReady && resolvedMeetingId === meetingId),
    };
}
