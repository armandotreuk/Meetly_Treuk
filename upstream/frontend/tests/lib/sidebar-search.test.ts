import { afterEach, describe, expect, it, vi } from "vitest";
import { createSearchSnapshotScope } from "@/components/ChatPanel/scope";
import type { HybridSearchResponse } from "@/types";
import {
    buildSidebarSearchRows,
    createSidebarSearchController,
    SIDEBAR_SEARCH_MIN_QUERY_LENGTH,
    type SidebarSearchInvoke,
    type SidebarSearchState,
} from "@/lib/sidebar-search";

function response(
    results: HybridSearchResponse["results"],
    retrievalStatus: HybridSearchResponse["retrievalStatus"] = "hybrid",
    scope: HybridSearchResponse["scope"] = { kind: "all" }
): HybridSearchResponse {
    return {
        version: "v1",
        scope,
        retrievalStatus,
        results,
        total: results.length,
    };
}

function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (reason?: unknown) => void;
    const promise = new Promise<T>((resolvePromise, rejectPromise) => {
        resolve = resolvePromise;
        reject = rejectPromise;
    });
    return { promise, resolve, reject };
}

const hybridResult = {
    meetingId: "semantic",
    meetingTitle: "Stale response title",
    folderName: "root",
    folderId: "root",
    meetingRank: 1,
    retainedEvidenceIds: ["e1"],
    sources: [
        {
            meetingId: "semantic",
            meetingTitle: "Stale response title",
            folderName: "root",
            sourceKind: "transcript",
            snippet: "<mark>budget</mark> plan",
            evidenceIds: ["e1"],
        },
    ],
    provenance: [
        {
            evidenceId: "e1",
            channel: "semantic" as const,
            variant: "original" as const,
            channelRank: 1,
            querySlot: 0,
        },
        {
            evidenceId: "e2",
            channel: "lexical" as const,
            variant: "original" as const,
            matchMode: "or" as const,
            channelRank: 2,
            querySlot: 0,
        },
    ],
};

afterEach(() => {
    vi.useRealTimers();
});

describe("sidebar search lifecycle", () => {
    it("debounces, cancels superseded requests, and rejects late responses", async () => {
        vi.useFakeTimers();
        const old = deferred<unknown>();
        const newer = deferred<unknown>();
        const states: SidebarSearchState[] = [];
        const invoke = vi.fn((command: string, args: Record<string, unknown>) => {
            if (command === "api_cancel_hybrid_request") return Promise.resolve();
            if (command === "api_search_hybrid") {
                return args.query === "old" ? old.promise : newer.promise;
            }
            return Promise.resolve([]);
        }) as SidebarSearchInvoke;
        const controller = createSidebarSearchController({
            invoke,
            onState: (state) => states.push(state),
            requestIdFactory: (generation) => `test-${generation}`,
        });

        controller.search("old");
        vi.advanceTimersByTime(249);
        expect(invoke).not.toHaveBeenCalledWith("api_search_hybrid", expect.anything());
        vi.advanceTimersByTime(1);
        expect(invoke).toHaveBeenCalledWith(
            "api_search_hybrid",
            expect.objectContaining({
                query: "old",
                requestId: "test-1",
            })
        );

        controller.search("newer");
        expect(invoke).toHaveBeenCalledWith("api_cancel_hybrid_request", {
            requestId: "test-1",
        });
        vi.advanceTimersByTime(250);
        expect(invoke).toHaveBeenCalledWith(
            "api_search_hybrid",
            expect.objectContaining({
                query: "newer",
                requestId: "test-2",
            })
        );

        newer.resolve(response([hybridResult]));
        await Promise.resolve();
        await Promise.resolve();
        old.resolve(response([{ ...hybridResult, meetingId: "old-result" }]));
        await Promise.resolve();
        await Promise.resolve();

        expect(states.at(-1)?.response?.results[0].meetingId).toBe("semantic");
        controller.dispose();
    });

    it("uses the selected folder in the server request and cancels on folder changes", () => {
        vi.useFakeTimers();
        const invoke = vi.fn((command: string) => {
            if (command === "api_cancel_hybrid_request") return Promise.resolve();
            return Promise.resolve(response([]));
        }) as SidebarSearchInvoke;
        const controller = createSidebarSearchController({
            invoke,
            onState: vi.fn(),
            requestIdFactory: (generation) => `test-${generation}`,
        });

        controller.search("budget", "folder-a");
        vi.advanceTimersByTime(250);
        expect(invoke).toHaveBeenCalledWith(
            "api_search_hybrid",
            expect.objectContaining({
                scope: { kind: "folder", folderId: "folder-a" },
            })
        );

        controller.search("budget", "folder-b");
        expect(invoke).toHaveBeenCalledWith("api_cancel_hybrid_request", {
            requestId: "test-1",
        });
        controller.dispose();
    });

    it("dispatches one-character queries after debounce and keeps empty input idle", () => {
        vi.useFakeTimers();
        const invoke = vi.fn((command: string) => {
            if (command === "api_cancel_hybrid_request") return Promise.resolve();
            return Promise.resolve(response([]));
        }) as SidebarSearchInvoke;
        const controller = createSidebarSearchController({
            invoke,
            onState: vi.fn(),
            requestIdFactory: (generation) => `test-${generation}`,
        });

        expect(SIDEBAR_SEARCH_MIN_QUERY_LENGTH).toBe(1);
        controller.search("");
        vi.advanceTimersByTime(250);
        expect(invoke).not.toHaveBeenCalledWith("api_search_hybrid", expect.anything());
        controller.search("a");
        vi.advanceTimersByTime(249);
        expect(invoke).not.toHaveBeenCalledWith("api_search_hybrid", expect.anything());
        vi.advanceTimersByTime(1);
        expect(invoke).toHaveBeenCalledWith(
            "api_search_hybrid",
            expect.objectContaining({ query: "a", requestId: "test-2" })
        );
        vi.advanceTimersByTime(250);
        controller.dispose();
        expect(invoke).toHaveBeenCalledWith("api_cancel_hybrid_request", { requestId: "test-2" });
    });

    it("keeps delayed cancellation scoped to the controller that created the request", () => {
        vi.useFakeTimers();
        const cancelRelease = deferred<unknown>();
        const requestIds: string[] = [];
        const cancelledIds: string[] = [];
        const invoke = vi.fn((command: string, args: Record<string, unknown>) => {
            if (command === "api_search_hybrid") {
                requestIds.push(String(args.requestId));
                return new Promise(() => undefined);
            }
            if (command === "api_cancel_hybrid_request") {
                cancelledIds.push(String(args.requestId));
                return cancelRelease.promise;
            }
            return Promise.resolve();
        }) as SidebarSearchInvoke;

        const previous = createSidebarSearchController({ invoke, onState: vi.fn(), debounceMs: 0 });
        previous.search("old");
        vi.advanceTimersByTime(1);
        previous.dispose();

        const current = createSidebarSearchController({ invoke, onState: vi.fn(), debounceMs: 0 });
        current.search("new");
        vi.advanceTimersByTime(1);

        expect(requestIds).toHaveLength(2);
        expect(requestIds[0]).not.toBe(requestIds[1]);
        expect(cancelledIds).toEqual([requestIds[0]]);

        current.dispose();
        expect(cancelledIds).toEqual(requestIds);
        cancelRelease.resolve(undefined);
    });

    it.each(["forced_lexical", "lexical_fallback"] as const)(
        "treats typed %s status as usable results",
        async (retrievalStatus) => {
            const states: SidebarSearchState[] = [];
            const invoke = vi.fn((command: string) => {
                if (command === "api_search_hybrid") {
                    return Promise.resolve(response([hybridResult], retrievalStatus));
                }
                return Promise.resolve([]);
            }) as SidebarSearchInvoke;
            const controller = createSidebarSearchController({
                invoke,
                debounceMs: 0,
                onState: (state) => states.push(state),
            });

            controller.search("budget");
            await new Promise((resolve) => setTimeout(resolve, 0));

            expect(states.at(-1)).toMatchObject({
                phase: "ready",
                notice: retrievalStatus,
                error: null,
            });
            expect(invoke).not.toHaveBeenCalledWith("api_search_fts", expect.anything());
            controller.dispose();
        }
    );

    it("keeps generic hybrid command failures distinct and hides backend error text", async () => {
        const states: SidebarSearchState[] = [];
        const invoke = vi.fn((command: string) => {
            if (command === "api_search_hybrid")
                return Promise.reject("database path / secret query");
            return Promise.resolve();
        }) as SidebarSearchInvoke;
        const controller = createSidebarSearchController({
            invoke,
            debounceMs: 0,
            onState: (state) => states.push(state),
        });

        controller.search("budget");
        await new Promise((resolve) => setTimeout(resolve, 0));

        expect(states.at(-1)).toMatchObject({
            phase: "ready",
            notice: null,
            error: "unavailable",
        });
        expect(invoke).not.toHaveBeenCalledWith("api_search_fts", expect.anything());
        expect(JSON.stringify(states.at(-1))).not.toContain("database path");
        controller.dispose();
    });

    it("rejects malformed hybrid payloads as invalid requests", async () => {
        const states: SidebarSearchState[] = [];
        const invoke = vi.fn((command: string) => {
            if (command === "api_search_hybrid") {
                return Promise.resolve({
                    version: "v1",
                    scope: { kind: "all" },
                    retrievalStatus: "hybrid",
                    results: [{ meetingId: "missing-fields" }],
                    total: 1,
                });
            }
            return Promise.resolve();
        }) as SidebarSearchInvoke;
        const controller = createSidebarSearchController({
            invoke,
            debounceMs: 0,
            onState: (state) => states.push(state),
        });

        controller.search("budget");
        await new Promise((resolve) => setTimeout(resolve, 0));

        expect(states.at(-1)).toMatchObject({
            phase: "ready",
            error: "invalid_request",
            response: null,
        });
        expect(invoke).not.toHaveBeenCalledWith("api_search_fts", expect.anything());
        controller.dispose();
    });

    it("returns a typed safe error when a scoped hybrid request cannot fall back", async () => {
        const states: SidebarSearchState[] = [];
        const invoke = vi.fn((command: string) => {
            if (command === "api_search_hybrid")
                return Promise.reject("Hybrid retrieval is unavailable");
            return Promise.resolve([]);
        }) as SidebarSearchInvoke;
        const controller = createSidebarSearchController({
            invoke,
            debounceMs: 0,
            onState: (state) => states.push(state),
        });

        controller.search("budget", "folder-a");
        await new Promise((resolve) => setTimeout(resolve, 0));

        expect(states.at(-1)).toMatchObject({ phase: "ready", error: "unavailable" });
        expect(JSON.stringify(states.at(-1))).not.toContain("Hybrid retrieval is unavailable");
        controller.dispose();
    });

    it("treats backend invalidation as cancellation without lexical fallback", async () => {
        const states: SidebarSearchState[] = [];
        const invoke = vi.fn((command: string) => {
            if (command === "api_search_hybrid") {
                return Promise.reject("Hybrid result was invalidated");
            }
            if (command === "api_search_fts") {
                return Promise.reject("FTS fallback must not run");
            }
            return Promise.resolve();
        }) as SidebarSearchInvoke;
        const controller = createSidebarSearchController({
            invoke,
            debounceMs: 0,
            onState: (state) => states.push(state),
        });

        controller.search("budget");
        await new Promise((resolve) => setTimeout(resolve, 0));

        expect(states.at(-1)).toMatchObject({ phase: "idle", response: null });
        expect(invoke).not.toHaveBeenCalledWith("api_search_fts", expect.anything());
        controller.dispose();
    });
});

describe("sidebar search result policy", () => {
    it("uses current scoped server rows without appending stale local title matches", () => {
        const meetings = [{ id: "semantic", title: "Stale cached title", folder_id: "other" }];
        const rows = buildSidebarSearchRows(
            meetings,
            "budget",
            "root",
            response(
                [
                    hybridResult,
                    {
                        ...hybridResult,
                        meetingId: "server-only",
                        meetingTitle: "Authoritative title",
                        folderName: "child",
                        folderId: "child",
                    },
                ],
                "hybrid",
                {
                    kind: "folder",
                    folderId: "root",
                }
            )
        );

        expect(rows.map((row) => row.meeting.id)).toEqual(["semantic", "server-only"]);
        expect(rows[0]).toMatchObject({
            meeting: {
                title: "Stale response title",
                folder_id: "root",
                folder_name: "root",
            },
            snippet: "budget plan",
            provenance: "Hybrid · Transcript",
        });
        expect(rows[1].meeting).toMatchObject({
            title: "Authoritative title",
            folder_id: "child",
            folder_name: "child",
        });
        expect(rows.map((row) => row.provenance)).not.toContain("database path / secret query");
    });

    it.each(["hybrid", "lexical_fallback"] as const)(
        "keeps title-only %s candidates in server order and snapshots displayed IDs",
        async (retrievalStatus) => {
            const meetings = [
                { id: "title-second", title: "Alpha second" },
                { id: "content", title: "Other" },
                { id: "title-first", title: "Alpha first" },
            ];
            const titleResult = (meetingId: string, meetingTitle: string, meetingRank: number) => ({
                meetingId,
                meetingTitle,
                folderName: "",
                folderId: null,
                meetingRank,
                retainedEvidenceIds: [`title:${meetingId}`],
                sources: [
                    {
                        meetingId,
                        meetingTitle,
                        folderName: "",
                        sourceKind: "title",
                        snippet: "",
                        evidenceIds: [`title:${meetingId}`],
                    },
                ],
                provenance: [
                    {
                        evidenceId: `title:${meetingId}`,
                        channel: "title" as const,
                        variant: "core_terms" as const,
                        channelRank: meetingRank,
                        querySlot: 0,
                    },
                ],
            });
            const rows = buildSidebarSearchRows(
                meetings,
                "alpha",
                null,
                response(
                    [
                        titleResult("title-second", "Alpha second", 1),
                        titleResult("content", "Other", 2),
                        titleResult("title-first", "Alpha first", 3),
                    ],
                    retrievalStatus
                )
            );

            expect(rows.map((row) => row.meeting.id)).toEqual([
                "title-second",
                "content",
                "title-first",
            ]);
            const firstScope = await createSearchSnapshotScope(rows.map(({ meeting }) => meeting));
            const secondScope = await createSearchSnapshotScope(rows.map(({ meeting }) => meeting));
            expect(firstScope).toEqual(secondScope);
            expect(firstScope).toMatchObject({
                kind: "search_snapshot",
                data: { result_ids: ["title-second", "content", "title-first"] },
            });
        }
    );

    it("returns empty rows for empty or no-result searches", () => {
        expect(buildSidebarSearchRows([], "", null, null)).toEqual([]);
        expect(
            buildSidebarSearchRows([{ id: "m1", title: "No match" }], "budget", null, response([]))
        ).toEqual([]);
    });
});
