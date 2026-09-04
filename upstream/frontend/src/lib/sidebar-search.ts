import type { HybridMeetingResult, HybridRetrievalStatus, HybridSearchResponse } from "@/types";
import { t } from "@/lib/i18n";

export const SIDEBAR_SEARCH_DEBOUNCE_MS = 250;
export const SIDEBAR_SEARCH_MIN_QUERY_LENGTH = 1;
export const SIDEBAR_SEARCH_RESULT_LIMIT = 50;

export type SidebarSearchNotice = "forced_lexical" | "lexical_fallback";
export type SidebarSearchErrorCode = "invalid_request" | "timeout" | "unavailable";

export interface SidebarSearchMeeting {
    id: string;
    title: string;
    created_at?: string;
    folder_id?: string | null;
    folder_name?: string | null;
    has_notes?: boolean;
}

export interface SidebarSearchRow {
    meeting: SidebarSearchMeeting;
    snippet: string | null;
    provenance: string | null;
}

export interface SidebarSearchState {
    phase: "idle" | "loading" | "ready";
    response: HybridSearchResponse | null;
    notice: SidebarSearchNotice | null;
    error: SidebarSearchErrorCode | null;
}

export type SidebarSearchInvoke = (
    command: string,
    args: Record<string, unknown>
) => Promise<unknown>;

export interface SidebarSearchController {
    search: (query: string, folderId?: string | null) => void;
    cancel: () => void;
    dispose: () => void;
}

interface SidebarSearchControllerOptions {
    invoke: SidebarSearchInvoke;
    onState: (state: SidebarSearchState) => void;
    debounceMs?: number;
    requestIdFactory?: (generation: number) => string;
}

function createSidebarSearchInstanceId(): string {
    const cryptoApi = globalThis.crypto;
    if (typeof cryptoApi?.randomUUID === "function") return cryptoApi.randomUUID();
    if (typeof cryptoApi?.getRandomValues === "function") {
        const values = new Uint32Array(4);
        cryptoApi.getRandomValues(values);
        return Array.from(values, (value) => value.toString(16).padStart(8, "0")).join("");
    }
    return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function idleState(): SidebarSearchState {
    return {
        phase: "idle",
        response: null,
        notice: null,
        error: null,
    };
}

function readyState(
    response: HybridSearchResponse | null,
    notice: SidebarSearchNotice | null,
    error: SidebarSearchErrorCode | null
): SidebarSearchState {
    return {
        phase: "ready",
        response,
        notice,
        error,
    };
}

function responseNotice(status: HybridRetrievalStatus): SidebarSearchNotice | null {
    if (status === "forced_lexical") return "forced_lexical";
    if (status === "lexical_fallback") return "lexical_fallback";
    return null;
}

function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "";
}

export function isSidebarSearchCancellation(error: unknown): boolean {
    return /cancelled|canceled|superseded|invalidated/i.test(errorText(error));
}

export function classifySidebarSearchError(error: unknown): SidebarSearchErrorCode {
    const message = errorText(error).toLowerCase();
    if (message.includes("timed out") || message.includes("timeout")) return "timeout";
    if (message.includes("invalid hybrid") || message.includes("invalid request")) {
        return "invalid_request";
    }
    return "unavailable";
}

function isHybridSearchResponse(value: unknown): value is HybridSearchResponse {
    if (!value || typeof value !== "object") return false;
    const response = value as Partial<HybridSearchResponse>;
    return (
        response.version === "v1" &&
        (response.retrievalStatus === "hybrid" ||
            response.retrievalStatus === "forced_lexical" ||
            response.retrievalStatus === "lexical_fallback") &&
        isHybridScope(response.scope) &&
        typeof response.total === "number" &&
        Number.isFinite(response.total) &&
        Array.isArray(response.results) &&
        response.results.every(isHybridMeetingResult)
    );
}

function isHybridScope(value: unknown): value is HybridSearchResponse["scope"] {
    if (!value || typeof value !== "object") return false;
    const scope = value as {
        kind?: unknown;
        meetingId?: unknown;
        folderId?: unknown;
        meetingIds?: unknown;
    };
    if (scope.kind === "all") return true;
    if (scope.kind === "meeting") return typeof scope.meetingId === "string";
    if (scope.kind === "folder") return typeof scope.folderId === "string";
    return (
        scope.kind === "allowed_meeting_ids" &&
        Array.isArray(scope.meetingIds) &&
        scope.meetingIds.every((meetingId) => typeof meetingId === "string")
    );
}

function isHybridMeetingResult(value: unknown): value is HybridMeetingResult {
    if (!value || typeof value !== "object") return false;
    const result = value as Partial<HybridMeetingResult>;
    return (
        typeof result.meetingId === "string" &&
        typeof result.meetingTitle === "string" &&
        typeof result.folderName === "string" &&
        (result.folderId === null || typeof result.folderId === "string") &&
        typeof result.meetingRank === "number" &&
        Number.isFinite(result.meetingRank) &&
        Array.isArray(result.retainedEvidenceIds) &&
        result.retainedEvidenceIds.every((evidenceId) => typeof evidenceId === "string") &&
        Array.isArray(result.sources) &&
        result.sources.every(isHybridSource) &&
        Array.isArray(result.provenance) &&
        result.provenance.every(isHybridProvenance)
    );
}

function isHybridSource(value: unknown): value is HybridMeetingResult["sources"][number] {
    if (!value || typeof value !== "object") return false;
    const source = value as Partial<HybridMeetingResult["sources"][number]>;
    return (
        typeof source.meetingId === "string" &&
        typeof source.meetingTitle === "string" &&
        typeof source.folderName === "string" &&
        typeof source.sourceKind === "string" &&
        typeof source.snippet === "string" &&
        Array.isArray(source.evidenceIds) &&
        source.evidenceIds.every((evidenceId) => typeof evidenceId === "string")
    );
}

function isHybridProvenance(value: unknown): value is HybridMeetingResult["provenance"][number] {
    if (!value || typeof value !== "object") return false;
    const provenance = value as Partial<HybridMeetingResult["provenance"][number]>;
    return (
        typeof provenance.evidenceId === "string" &&
        (provenance.channel === "lexical" ||
            provenance.channel === "title" ||
            provenance.channel === "semantic") &&
        (provenance.variant === "original" ||
            provenance.variant === "rewritten" ||
            provenance.variant === "core_terms") &&
        (provenance.matchMode === undefined ||
            provenance.matchMode === "and" ||
            provenance.matchMode === "or") &&
        typeof provenance.channelRank === "number" &&
        Number.isFinite(provenance.channelRank) &&
        typeof provenance.querySlot === "number" &&
        Number.isFinite(provenance.querySlot)
    );
}

export function createSidebarSearchController({
    invoke,
    onState,
    debounceMs = SIDEBAR_SEARCH_DEBOUNCE_MS,
    requestIdFactory,
}: SidebarSearchControllerOptions): SidebarSearchController {
    const instanceId = createSidebarSearchInstanceId();
    const makeRequestId =
        requestIdFactory ?? ((generation: number) => `sidebar-${instanceId}-${generation}`);
    let disposed = false;
    let generation = 0;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let activeRequestId: string | null = null;

    const cancelRequest = (requestId: string) => {
        void invoke("api_cancel_hybrid_request", { requestId }).catch(() => undefined);
    };

    const isCurrent = (currentGeneration: number, requestId: string) =>
        !disposed && generation === currentGeneration && activeRequestId === requestId;

    const run = async (
        currentGeneration: number,
        query: string,
        folderId: string | null,
        requestId: string
    ) => {
        if (disposed || generation !== currentGeneration) return;
        activeRequestId = requestId;

        try {
            const responseValue = await invoke("api_search_hybrid", {
                query,
                scope: folderId !== null ? { kind: "folder", folderId } : { kind: "all" },
                limit: SIDEBAR_SEARCH_RESULT_LIMIT,
                requestId,
            });
            if (!isCurrent(currentGeneration, requestId)) return;
            if (!isHybridSearchResponse(responseValue)) {
                throw new Error("Invalid hybrid response");
            }
            onState(readyState(responseValue, responseNotice(responseValue.retrievalStatus), null));
        } catch (error) {
            if (!isCurrent(currentGeneration, requestId)) return;
            if (isSidebarSearchCancellation(error)) {
                onState(idleState());
                return;
            }

            onState(readyState(null, null, classifySidebarSearchError(error)));
        } finally {
            if (isCurrent(currentGeneration, requestId)) activeRequestId = null;
        }
    };

    const search = (query: string, folderId: string | null = null) => {
        disposed = false;
        generation += 1;
        const currentGeneration = generation;
        if (timer !== null) {
            clearTimeout(timer);
            timer = null;
        }
        if (activeRequestId !== null) {
            cancelRequest(activeRequestId);
            activeRequestId = null;
        }

        const trimmedQuery = query.trim();
        if (Array.from(trimmedQuery).length < SIDEBAR_SEARCH_MIN_QUERY_LENGTH) {
            onState(idleState());
            return;
        }

        const requestId = makeRequestId(currentGeneration);
        onState({ ...idleState(), phase: "loading" });
        timer = setTimeout(() => {
            timer = null;
            void run(currentGeneration, trimmedQuery, folderId, requestId);
        }, debounceMs);
    };

    const dispose = () => {
        disposed = true;
        generation += 1;
        if (timer !== null) {
            clearTimeout(timer);
            timer = null;
        }
        if (activeRequestId !== null) {
            cancelRequest(activeRequestId);
            activeRequestId = null;
        }
    };

    const cancel = () => {
        generation += 1;
        if (timer !== null) {
            clearTimeout(timer);
            timer = null;
        }
        if (activeRequestId !== null) {
            cancelRequest(activeRequestId);
            activeRequestId = null;
        }
    };

    return { search, cancel, dispose };
}

function responseMatchesScope(response: HybridSearchResponse, folderId: string | null): boolean {
    if (folderId === null) return response.scope.kind === "all";
    return response.scope.kind === "folder" && response.scope.folderId === folderId;
}

function displaySnippet(snippet: string | undefined): string | null {
    if (!snippet) return null;
    const cleaned = snippet.replace(/<\/?mark>/gi, "").trim();
    return cleaned || null;
}

function sourceKindLabel(sourceKind: string | undefined): string | null {
    if (sourceKind === "transcript") return t("app.sidebar.provenance.transcript");
    if (sourceKind === "summary") return t("app.sidebar.provenance.summary");
    if (sourceKind === "notes" || sourceKind === "note") return t("app.sidebar.provenance.notes");
    return null;
}

export function bestHybridSource(result: HybridMeetingResult) {
    const firstRetainedId = result.retainedEvidenceIds?.[0];
    return (
        result.sources?.find((source) =>
            firstRetainedId ? source.evidenceIds?.includes(firstRetainedId) : false
        ) ??
        result.sources?.[0] ??
        null
    );
}

export function formatHybridProvenance(result: HybridMeetingResult): string | null {
    const channels = new Set(result.provenance?.map((entry) => entry.channel));
    let channel: string | null = null;
    if (channels.has("semantic") && (channels.has("lexical") || channels.has("title"))) {
        channel = t("app.sidebar.provenance.hybrid");
    } else if (channels.has("semantic")) {
        channel = t("app.sidebar.provenance.meaning");
    } else if (channels.has("lexical")) {
        channel = t("app.sidebar.provenance.keyword");
    } else if (channels.has("title")) {
        channel = t("app.sidebar.provenance.title");
    }
    const kind = sourceKindLabel(bestHybridSource(result)?.sourceKind);
    return [channel, kind].filter(Boolean).join(" · ") || null;
}

export function buildSidebarSearchRows(
    meetings: SidebarSearchMeeting[],
    query: string,
    folderId: string | null,
    response: HybridSearchResponse | null
): SidebarSearchRow[] {
    const normalizedQuery = query.trim().toLowerCase();
    if (!normalizedQuery) return [];

    const currentMeetings = new Map(meetings.map((meeting) => [meeting.id, meeting]));
    const rows: SidebarSearchRow[] = [];
    const seen = new Set<string>();

    const add = (
        meeting: SidebarSearchMeeting,
        snippet: string | null,
        provenance: string | null
    ) => {
        if (seen.has(meeting.id)) return;
        seen.add(meeting.id);
        rows.push({ meeting, snippet, provenance });
    };

    if (!response || !responseMatchesScope(response, folderId)) return [];
    for (const result of response.results.slice(0, SIDEBAR_SEARCH_RESULT_LIMIT)) {
        const cached = currentMeetings.get(result.meetingId);
        const meeting: SidebarSearchMeeting = {
            id: result.meetingId,
            title: result.meetingTitle,
            folder_id: result.folderId,
            folder_name: result.folderName,
            created_at: cached?.created_at,
            has_notes: cached?.has_notes,
        };
        const source = bestHybridSource(result);
        add(meeting, displaySnippet(source?.snippet), formatHybridProvenance(result));
    }

    return rows.slice(0, SIDEBAR_SEARCH_RESULT_LIMIT);
}
