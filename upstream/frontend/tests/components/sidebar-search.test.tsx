import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FolderFilterTree } from "@/components/Sidebar/FolderFilterTree";
import { MeetingTreeItem } from "@/components/Sidebar/MeetingTreeItem";
import Sidebar from "@/components/Sidebar";
import { SidebarProvider } from "@/components/Sidebar/SidebarProvider";
import type { HybridMeetingResult, HybridSearchResponse, MeetingFolder } from "@/types";

const mocks = vi.hoisted(() => ({
    routerPush: vi.fn(),
    invoke: vi.fn(),
    openChat: vi.fn(),
}));

vi.mock("next/navigation", () => ({
    usePathname: () => "/",
    useRouter: () => ({ push: mocks.routerPush }),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
    emit: vi.fn(),
    listen: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("@/contexts/RecordingStateContext", () => ({
    useRecordingState: () => ({ isRecording: false }),
}));
vi.mock("@/contexts/ConfigContext", () => ({
    useConfig: () => ({ betaFeatures: { importAndRetranscribe: false } }),
}));
vi.mock("@/contexts/ImportDialogContext", () => ({
    useImportDialog: () => ({ openImportDialog: vi.fn() }),
}));
vi.mock("@/components/ChatPanel/ChatHost", () => ({
    useChatHost: () => ({ openChat: mocks.openChat }),
}));
vi.mock("@/lib/analytics", () => ({
    default: {
        trackBackendConnection: vi.fn(),
        trackButtonClick: vi.fn(),
        trackMeetingDeleted: vi.fn(),
        trackSettingsChanged: vi.fn(),
    },
}));

let root: Root;
let container: HTMLDivElement;

function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (reason?: unknown) => void;
    const promise = new Promise<T>((resolvePromise, rejectPromise) => {
        resolve = resolvePromise;
        reject = rejectPromise;
    });
    return { promise, resolve, reject };
}

function emptyHybridResponse(
    retrievalStatus: HybridSearchResponse["retrievalStatus"] = "hybrid"
): HybridSearchResponse {
    return {
        version: "v1",
        scope: { kind: "all" },
        retrievalStatus,
        results: [],
        total: 0,
    };
}

async function mountExpandedSidebar(): Promise<HTMLInputElement> {
    await act(async () => {
        root.render(
            <SidebarProvider searchInvoke={mocks.invoke}>
                <Sidebar />
            </SidebarProvider>
        );
        await Promise.resolve();
        await Promise.resolve();
    });

    const collapse = container.firstElementChild?.querySelector("button") as HTMLButtonElement;
    await act(async () => collapse.click());
    return container.querySelector(
        'input[aria-label="Search meeting content"]'
    ) as HTMLInputElement;
}

function setSearchInput(input: HTMLInputElement, value: string) {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
}

beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    mocks.routerPush.mockReset();
    mocks.invoke.mockReset();
    mocks.openChat.mockReset();
});

afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.useRealTimers();
});

describe("mounted Sidebar search rows", () => {
    it("keeps meeting navigation and row actions as separate keyboard controls", async () => {
        const onEditMeeting = vi.fn();
        const onRequestDeleteMeeting = vi.fn();
        const onRequestMoveMeeting = vi.fn();
        await act(async () =>
            root.render(
                <MeetingTreeItem
                    meetingId="meeting-1"
                    title="Planning"
                    depth={0}
                    snippetContext="budget plan"
                    provenanceLabel="Hybrid · Transcript"
                    hasNotes
                    onEditMeeting={onEditMeeting}
                    onRequestDeleteMeeting={onRequestDeleteMeeting}
                    onRequestMoveMeeting={onRequestMoveMeeting}
                />
            )
        );

        const buttons = Array.from(container.querySelectorAll("button"));
        const navigation = buttons.find(
            (button) => button.getAttribute("aria-label") === "Planning"
        );
        expect(navigation).toBeDefined();
        expect(container.querySelector("button button")).toBeNull();
        expect(container.querySelector('[role="button"]')).toBeNull();
        expect(navigation?.tabIndex).toBe(0);
        expect(container.textContent).toContain("budget plan");
        expect(container.textContent).toContain("Hybrid · Transcript");

        navigation?.focus();
        expect(document.activeElement).toBe(navigation);
        await act(async () => navigation?.click());
        expect(mocks.routerPush).toHaveBeenCalledWith("/meeting-details?id=meeting-1");

        const edit = container.querySelector(
            'button[aria-label="Edit meeting title"]'
        ) as HTMLButtonElement;
        edit.focus();
        expect(document.activeElement).toBe(edit);
        await act(async () => edit.click());
        expect(onEditMeeting).toHaveBeenCalledWith("meeting-1", "Planning");
    });

    it("mounts folder filters with localized accessible pressed and clear states", async () => {
        const folders: MeetingFolder[] = [
            {
                id: "folder-1",
                name: "Planning",
                parent_id: null,
                created_at: "2026-09-04T00:00:00Z",
            },
        ];
        const onSelect = vi.fn();
        await act(async () =>
            root.render(<FolderFilterTree folders={folders} selected={null} onSelect={onSelect} />)
        );

        const filter = container.querySelector(
            'button[aria-label="Filter by folder Planning"]'
        ) as HTMLButtonElement;
        expect(filter).toBeDefined();
        expect(filter.getAttribute("aria-pressed")).toBe("false");
        await act(async () => filter.click());
        expect(onSelect).toHaveBeenCalledWith("folder-1");

        await act(async () =>
            root.render(
                <FolderFilterTree folders={folders} selected="folder-1" onSelect={onSelect} />
            )
        );
        const activeFilter = container.querySelector(
            'button[aria-label="Filter by folder Planning"]'
        ) as HTMLButtonElement;
        const clear = container.querySelector(
            'button[aria-label="Clear folder filter"]'
        ) as HTMLButtonElement;
        expect(activeFilter.getAttribute("aria-pressed")).toBe("true");
        expect(clear.textContent).toBe("clear");
        await act(async () => clear.click());
        expect(onSelect).toHaveBeenLastCalledWith(null);
    });

    it("drives the real provider/sidebar lifecycle and snapshots authoritative order", async () => {
        vi.useFakeTimers();
        const old = deferred<unknown>();
        const newer = deferred<unknown>();
        const result = (
            id: string,
            title: string,
            folderId: string | null
        ): HybridMeetingResult => ({
            meetingId: id,
            meetingTitle: title,
            folderName: folderId ? "Project" : "",
            folderId,
            meetingRank: 1,
            retainedEvidenceIds: [`e-${id}`],
            sources: [
                {
                    meetingId: id,
                    meetingTitle: title,
                    folderName: folderId ? "Project" : "",
                    sourceKind: "transcript",
                    snippet: `snippet for ${id}`,
                    evidenceIds: [`e-${id}`],
                },
            ],
            provenance: [
                {
                    evidenceId: `e-${id}`,
                    channel: "semantic",
                    variant: "original",
                    channelRank: 1,
                    querySlot: 0,
                },
            ],
        });
        const response = (
            results: HybridMeetingResult[],
            scope: HybridSearchResponse["scope"] = { kind: "all" }
        ): HybridSearchResponse => ({
            version: "v1",
            scope,
            retrievalStatus: "hybrid",
            results,
            total: results.length,
        });
        mocks.invoke.mockImplementation((command: string, args: Record<string, unknown>) => {
            if (command === "api_get_meetings") {
                return Promise.resolve([
                    { id: "cached", title: "Stale cache", folder_id: "other", has_notes: false },
                ]);
            }
            if (command === "api_get_folders") {
                return Promise.resolve([
                    {
                        id: "project",
                        name: "Project",
                        parent_id: null,
                        created_at: "2026-09-04T00:00:00Z",
                    },
                ]);
            }
            if (command === "api_search_hybrid") {
                const scope = args.scope as { kind?: unknown };
                if (scope.kind === "folder") {
                    return Promise.resolve(
                        response(
                            [result("folder-result", "Authoritative folder result", "project")],
                            { kind: "folder", folderId: "project" }
                        )
                    );
                }
                if (args.query === "old") return old.promise;
                return newer.promise;
            }
            return Promise.resolve();
        });

        await act(async () => {
            root.render(
                <SidebarProvider searchInvoke={mocks.invoke}>
                    <Sidebar />
                </SidebarProvider>
            );
            await Promise.resolve();
            await Promise.resolve();
        });

        const collapse = container.firstElementChild?.querySelector("button") as HTMLButtonElement;
        await act(async () => collapse.click());
        const input = container.querySelector(
            'input[aria-label="Search meeting content"]'
        ) as HTMLInputElement;
        expect(input).toBeDefined();
        const setSearch = (value: string) => {
            Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(
                input,
                value
            );
            input.dispatchEvent(new Event("input", { bubbles: true }));
        };

        await act(async () => {
            setSearch("old");
            vi.advanceTimersByTime(250);
        });
        expect(mocks.invoke).toHaveBeenCalledWith(
            "api_search_hybrid",
            expect.objectContaining({ query: "old" })
        );

        await act(async () => {
            setSearch("newer");
            vi.advanceTimersByTime(250);
        });
        expect(mocks.invoke).toHaveBeenCalledWith("api_cancel_hybrid_request", expect.anything());
        expect(mocks.invoke).toHaveBeenCalledWith(
            "api_search_hybrid",
            expect.objectContaining({ query: "newer" })
        );

        await act(async () => {
            newer.resolve(response([result("server-only", "Authoritative newer", null)]));
            await Promise.resolve();
            await Promise.resolve();
        });
        await act(async () => {
            old.resolve(response([result("old-result", "Old result", null)]));
            await Promise.resolve();
            await Promise.resolve();
        });
        expect(container.textContent).toContain("Authoritative newer");
        expect(container.textContent).not.toContain("Old result");

        const folderFilter = container.querySelector(
            'button[aria-label="Filter by folder Project"]'
        ) as HTMLButtonElement;
        await act(async () => {
            folderFilter.click();
            vi.advanceTimersByTime(250);
            await Promise.resolve();
            await Promise.resolve();
        });
        expect(mocks.invoke).toHaveBeenCalledWith(
            "api_search_hybrid",
            expect.objectContaining({
                query: "newer",
                scope: { kind: "folder", folderId: "project" },
            })
        );

        const ask = Array.from(container.querySelectorAll("button")).find(
            (button) => button.textContent === "Ask about these results"
        );
        await act(async () => {
            ask?.click();
            await Promise.resolve();
            await Promise.resolve();
        });
        expect(mocks.openChat).toHaveBeenCalledWith(
            expect.objectContaining({
                kind: "search_snapshot",
                data: { result_ids: ["folder-result"] },
            })
        );
    });

    it("announces pending loading and no-result states from the mounted Sidebar", async () => {
        vi.useFakeTimers();
        const pending = deferred<unknown>();
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "api_get_meetings" || command === "api_get_folders") {
                return Promise.resolve([]);
            }
            if (command === "api_search_hybrid") return pending.promise;
            return Promise.resolve();
        });

        const input = await mountExpandedSidebar();
        await act(async () => {
            setSearchInput(input, "alpha");
            vi.advanceTimersByTime(250);
            await Promise.resolve();
        });

        const loading = Array.from(container.querySelectorAll('[role="status"]')).find((node) =>
            node.textContent?.includes("Searching meetings")
        ) as HTMLElement;
        expect(loading).toBeDefined();
        expect(loading.getAttribute("aria-live")).toBe("polite");
        expect(loading.getAttribute("aria-atomic")).toBe("true");

        await act(async () => {
            pending.resolve(emptyHybridResponse());
            await Promise.resolve();
            await Promise.resolve();
        });
        const noResults = Array.from(container.querySelectorAll('[role="status"]')).find((node) =>
            node.textContent?.includes("No results")
        ) as HTMLElement;
        expect(noResults).toBeDefined();
        expect(noResults.getAttribute("aria-live")).toBe("polite");
        expect(noResults.getAttribute("aria-atomic")).toBe("true");
    });

    it("announces degraded fallback without exposing backend details", async () => {
        vi.useFakeTimers();
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "api_get_meetings" || command === "api_get_folders") {
                return Promise.resolve([]);
            }
            if (command === "api_search_hybrid") {
                return Promise.resolve(emptyHybridResponse("lexical_fallback"));
            }
            return Promise.resolve();
        });

        const input = await mountExpandedSidebar();
        await act(async () => {
            setSearchInput(input, "alpha");
            vi.advanceTimersByTime(250);
            await Promise.resolve();
            await Promise.resolve();
        });

        const fallback = Array.from(container.querySelectorAll('[role="status"]')).find((node) =>
            node.textContent?.includes("Semantic search unavailable")
        ) as HTMLElement;
        expect(fallback).toBeDefined();
        expect(fallback.getAttribute("aria-live")).toBe("polite");
        expect(fallback.getAttribute("aria-atomic")).toBe("true");
    });

    it("announces a generic private error without exposing the query or backend detail", async () => {
        vi.useFakeTimers();
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "api_get_meetings" || command === "api_get_folders") {
                return Promise.resolve([]);
            }
            if (command === "api_search_hybrid") {
                return Promise.reject(new Error("database secret for hidden query"));
            }
            return Promise.resolve();
        });

        const input = await mountExpandedSidebar();
        await act(async () => {
            setSearchInput(input, "private query");
            vi.advanceTimersByTime(250);
            await Promise.resolve();
            await Promise.resolve();
        });

        const error = Array.from(container.querySelectorAll('[role="status"]')).find((node) =>
            node.textContent?.includes("Search unavailable")
        ) as HTMLElement;
        expect(error).toBeDefined();
        expect(error.textContent).toBe("Search unavailable");
        expect(error.textContent).not.toContain("private query");
        expect(error.textContent).not.toContain("database secret");
        expect(error.getAttribute("aria-live")).toBe("polite");
        expect(error.getAttribute("aria-atomic")).toBe("true");
    });
});
