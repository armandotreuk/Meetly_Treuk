import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSearchSnapshotScope } from "@/components/ChatPanel/scope";
import { ChatHost, LiveChatLauncher, useChatHost } from "@/components/ChatPanel/ChatHost";
import { ChatMessage } from "@/components/ChatPanel/ChatMessage";
import type { ChatScope } from "@/types";

const mocks = vi.hoisted(() => ({
    invoke: vi.fn(),
    listeners: new Map<string, (event: { payload: any }) => void>(),
    listen: vi.fn(),
    unlisten: vi.fn(),
    routerPush: vi.fn(),
    isRecording: false,
    liveTranscriptScopeKey: null as string | null,
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
    listen: mocks.listen,
}));
vi.mock("next/navigation", () => ({ useRouter: () => ({ push: mocks.routerPush }) }));
vi.mock("@/contexts/RecordingStateContext", () => ({ useRecordingState: () => ({ isRecording: mocks.isRecording, liveTranscriptScopeKey: mocks.liveTranscriptScopeKey }) }));

function Launcher({ scope, label }: { scope: ChatScope; label: string }) {
    const { openChat } = useChatHost();
    return <button onClick={() => openChat(scope)}>{label}</button>;
}

function LabeledLauncher({ scope, label, buttonLabel }: { scope: ChatScope; label?: string; buttonLabel: string }) {
    const { openChat } = useChatHost();
    return <button onClick={() => openChat(scope, label)}>{buttonLabel}</button>;
}

function Promoter({ liveScopeKey, meetingId }: { liveScopeKey: string; meetingId: string }) {
    const { promoteLiveChat } = useChatHost();
    return <button onClick={() => promoteLiveChat(liveScopeKey, meetingId)}>promote</button>;
}

function PromotionPreparer() {
    const { prepareLiveChatPromotion } = useChatHost();
    return <button onClick={() => prepareLiveChatPromotion()}>prepare promotion</button>;
}

let root: Root;
let container: HTMLDivElement;

async function flush() {
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });
}

beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    mocks.listeners.clear();
    mocks.listen.mockReset();
    mocks.listen.mockImplementation(async (name: string, callback: (event: { payload: any }) => void) => {
        mocks.listeners.set(name, callback);
        return mocks.unlisten;
    });
    mocks.unlisten.mockClear();
    mocks.invoke.mockReset();
    mocks.routerPush.mockReset();
    mocks.isRecording = false;
    mocks.liveTranscriptScopeKey = null;
    mocks.invoke.mockImplementation((command: string, args?: any) => {
        if (command === "api_get_chat_model_config") return Promise.resolve({});
        if (command === "api_chat_get_or_create_scoped_conversation") return Promise.resolve({ id: `conversation-${args.scope.key}` });
        if (command === "api_chat_get_messages") return Promise.resolve([]);
        return Promise.resolve();
    });
});

afterEach(() => {
    act(() => root.unmount());
    container.remove();
});

describe("createSearchSnapshotScope", () => {
    it("uses bounded unique chunk ids and a deterministic identity", async () => {
        const results = Array.from({ length: 102 }, (_, index) => ({ id: index === 1 ? "meeting-0" : `meeting-${index}` }));
        const first = await createSearchSnapshotScope(results);
        const second = await createSearchSnapshotScope(results);
        expect(first).toEqual(second);
        expect(first).toMatchObject({ kind: "search_snapshot", data: { result_ids: expect.arrayContaining(["meeting-0", "meeting-100"]) } });
        expect(first?.data.result_ids).toHaveLength(100);
    });

    it("keeps title-only meetings from the rendered search list in the snapshot", async () => {
        const scope = await createSearchSnapshotScope([{ id: "fts-match" }, { id: "title-only" }]);
        expect(scope?.data.result_ids).toEqual(["fts-match", "title-only"]);
    });
});

describe("ChatHost scoped panel", () => {
    it("closes persisted chat on recording start, keeps live chat, and ignores persisted launchers", async () => {
        const persisted: ChatScope = { kind: "all", key: "all" };
        const live: ChatScope = { kind: "live_recording", key: "live-1" };
        const renderHost = () => root.render(<ChatHost><Launcher scope={persisted} label="persisted" /><Launcher scope={live} label="live" /></ChatHost>);
        await act(async () => renderHost());
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "persisted") as HTMLButtonElement).click());
        await flush();
        expect(container.querySelector("textarea")).not.toBeNull();

        mocks.isRecording = true;
        await act(async () => renderHost());
        await flush();
        expect(container.querySelector("textarea")).toBeNull();
        expect(mocks.invoke).toHaveBeenCalledWith("api_cancel_chat_stream", { streamId: null });

        const persistedLoads = mocks.invoke.mock.calls.filter(([command]) => command === "api_chat_get_or_create_scoped_conversation").length;
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "persisted") as HTMLButtonElement).click());
        await flush();
        expect(mocks.invoke.mock.calls.filter(([command]) => command === "api_chat_get_or_create_scoped_conversation")).toHaveLength(persistedLoads);
        expect(container.querySelector("textarea")).toBeNull();

        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "live") as HTMLButtonElement).click());
        await flush();
        expect(container.querySelector("textarea")).not.toBeNull();
        expect(mocks.invoke).toHaveBeenCalledWith("api_chat_get_or_create_scoped_conversation", { scope: live, title: null });
    });

    it("does not reuse a stale live panel across recording restarts", async () => {
        const live: ChatScope = { kind: "live_recording", key: "live-1" };
        mocks.isRecording = true;
        mocks.liveTranscriptScopeKey = "live-1";
        await act(async () => root.render(<ChatHost><Launcher scope={live} label="live" /></ChatHost>));
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "live") as HTMLButtonElement).click());
        await flush();
        expect(container.querySelector("textarea")).not.toBeNull();

        // Recording stops; the open live panel is retained by design.
        mocks.isRecording = false;
        await act(async () => root.render(<ChatHost><Launcher scope={live} label="live" /></ChatHost>));
        await flush();
        expect(container.querySelector("textarea")).not.toBeNull();

        // Recording restarts with a fresh key: the stale panel must not be reused.
        mocks.isRecording = true;
        mocks.liveTranscriptScopeKey = "live-2";
        await act(async () => root.render(<ChatHost><Launcher scope={live} label="live" /></ChatHost>));
        await flush();
        expect(container.querySelector("textarea")).toBeNull();
    });

    it("shows the live launcher only during recording with a stable key", async () => {
        await act(async () => root.render(<ChatHost><LiveChatLauncher isRecording={false} recordingScopeKey="live-1" /></ChatHost>));
        expect(container.textContent).not.toContain("Ask about this recording");
        await act(async () => root.render(<ChatHost><LiveChatLauncher isRecording recordingScopeKey={null} /></ChatHost>));
        expect(container.textContent).not.toContain("Ask about this recording");
        await act(async () => root.render(<ChatHost><LiveChatLauncher isRecording recordingScopeKey="live-1" /></ChatHost>));
        expect(container.textContent).toContain("Ask about this recording");
    });

    it("sends live content only with explicit consent for a non-local provider", async () => {
        const provider = "claude";
        mocks.invoke.mockImplementation((command: string, args?: any) => {
            if (command === "api_get_chat_model_config") return Promise.resolve({ provider, model: "model" });
            if (command === "api_chat_get_or_create_scoped_conversation") return Promise.resolve({ id: `conversation-${args.scope.key}` });
            if (command === "api_chat_get_messages") return Promise.resolve([]);
            return Promise.resolve();
        });
        const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
        await act(async () => root.render(<ChatHost><Launcher scope={{ kind: "live_recording", key: "live-1" }} label="live" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        const textarea = container.querySelector("textarea")!;
        const mode = container.querySelector("#chat-retrieval-mode") as HTMLSelectElement;
        expect(mode.disabled).toBe(true);
        expect(mode.value).toBe("fast");
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "question");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        expect(confirm).toHaveBeenCalledOnce();
        expect(mocks.invoke.mock.calls.some(([command]) => command === "api_chat_with_scoped_conversation_stream")).toBe(false);
        confirm.mockReturnValue(true);
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        await flush();
        expect(confirm).toHaveBeenCalledTimes(2);
        const request = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream");
        expect(request?.[1]).toMatchObject({ liveTranscriptConsent: true, mode: "fast" });
        confirm.mockRestore();
    });

    it("defaults to Deep and sends the selected mode without changing the conversation", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();

        const mode = container.querySelector("#chat-retrieval-mode") as HTMLSelectElement;
        expect(mode.value).toBe("deep");
        await act(async () => {
            mode.value = "fast";
            mode.dispatchEvent(new Event("change", { bubbles: true }));
        });
        expect(mode.value).toBe("fast");

        const textarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "question");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        await flush();

        const request = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream");
        expect(request?.[1]).toMatchObject({ mode: "fast" });
        expect(mocks.invoke.mock.calls.filter(([command]) => command === "api_chat_get_or_create_scoped_conversation")).toHaveLength(1);
    });

    it("sends Deep explicitly for a new interactive request", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();

        const textarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "question");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        await flush();

        const request = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream");
        expect(request?.[1]).toMatchObject({ mode: "deep" });
    });

    it("renders only privacy-safe preparation progress and cancels during preparation", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        const textarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "question");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        await flush();
        const streamId = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream")![1].streamId;
        const progress = mocks.listeners.get("chat-preparation-progress")!;

        await act(async () => progress({ payload: { streamId, stage: "initial_retrieval", completed: 2, total: 3 } }));
        expect(container.querySelector('[role="status"]')?.textContent).toContain("2 of 3");
        expect(container.querySelector('[role="status"]')?.textContent).not.toContain("question");
        await act(async () => progress({ payload: { streamId, stage: "planner_round", completed: 1, total: 2 } }));
        expect(container.querySelector('[role="status"]')?.textContent).toContain("round 1 of 2");
        expect(container.querySelector('[role="status"]')?.textContent).not.toContain("question");
        await act(async () => progress({ payload: { streamId, stage: "additional_search", completed: 2, total: 2 } }));
        expect(container.querySelector('[role="status"]')?.textContent).toContain("Additional search: 2 of 2");
        expect(container.querySelector('[role="status"]')?.textContent).not.toContain("question");
        await act(async () => progress({ payload: { streamId, stage: "initial_retrieval", completed: 2, total: 3 } }));
        await act(async () => progress({ payload: { streamId: "stale", stage: "answer_generation", completed: 0, total: 1 } }));
        expect(container.querySelector('[role="status"]')?.textContent).not.toContain("Preparing answer");
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources: [] } }));
        expect(container.querySelector('[role="status"]')?.textContent).toContain("2 of 3");
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "answer" } }));
        expect(container.querySelector('[role="status"]')).toBeNull();

        await act(async () => (container.querySelector('[aria-label="Stop generating"]') as HTMLButtonElement).click());
        expect(mocks.invoke).toHaveBeenCalledWith("api_cancel_chat_stream", { streamId });
        expect(container.querySelector('[aria-label="Stop generating"]')).toBeNull();
    });

    it("promotes an open live host to the saved meeting without mixing scopes", async () => {
        const live: ChatScope = { kind: "live_recording", key: "live-1" };
        await act(async () => root.render(<ChatHost><Launcher scope={live} label="live" /><Promoter liveScopeKey="live-1" meetingId="meeting-1" /></ChatHost>));
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "live") as HTMLButtonElement).click());
        await flush();
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "promote") as HTMLButtonElement).click());
        await flush();
        expect(mocks.invoke).toHaveBeenCalledWith("api_chat_promote_live_recording", { liveScopeKey: "live-1", meetingId: "meeting-1" });
        expect(mocks.invoke).toHaveBeenCalledWith("api_chat_get_or_create_scoped_conversation", { scope: { kind: "meeting", key: "meeting-1" }, title: null });
    });

    it("cancels the current stream before live save promotion begins", async () => {
        await act(async () => root.render(<ChatHost><PromotionPreparer /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        expect(mocks.invoke).toHaveBeenCalledWith("api_cancel_chat_stream", { streamId: null });
    });

    it("discloses an orphaned meeting thread only for the exact typed condition", async () => {
        const backendDisclosure = "This meeting's chat thread is no longer available because the meeting was deleted. Earlier answers may still quote deleted content.";
        mocks.invoke.mockImplementation((command: string, args?: any) => {
            if (command === "api_get_chat_model_config") return Promise.resolve({});
            if (command === "api_chat_get_or_create_scoped_conversation" && args.scope.kind === "meeting")
                return Promise.reject(new Error(`deleted_meeting_thread|${backendDisclosure}`));
            return Promise.resolve();
        });
        await act(async () => root.render(<ChatHost><Launcher scope={{ kind: "meeting", key: "deleted-1" }} label="deleted" /></ChatHost>));
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "deleted") as HTMLButtonElement).click());
        await flush();
        const banner = container.querySelector('[role="status"]') as HTMLElement;
        // The typed deleted-meeting condition maps to the localized orphan
        // disclosure, not the raw backend error text.
        expect(banner.textContent).toContain("This meeting was deleted.");
        expect(banner.textContent).toContain("may still quote deleted content");
        expect(banner.textContent).not.toContain(backendDisclosure);
        // Without a conversation the panel stays read-only instead of failing silently.
        expect(container.querySelector("textarea")?.disabled).toBe(true);
    });

    it("keeps near-collision errors on the privacy-safe generic fallback", async () => {
        mocks.invoke.mockImplementation((command: string, args?: any) => {
            if (command === "api_get_chat_model_config") return Promise.resolve({});
            if (command === "api_chat_get_or_create_scoped_conversation")
                return Promise.reject(new Error("a meeting was deleted by another process: internal row 0xdeadbeef"));
            return Promise.resolve();
        });
        await act(async () => root.render(<ChatHost><Launcher scope={{ kind: "meeting", key: "meeting-1" }} label="meeting" /></ChatHost>));
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "meeting") as HTMLButtonElement).click());
        await flush();
        const banner = container.querySelector('[role="status"]') as HTMLElement;
        // A near-collision message must NOT be classified as the orphan
        // condition: the code segment differs from the stable constant.
        expect(banner.textContent).toContain("Chat could not be loaded");
        expect(banner.textContent).toContain("Check your model configuration");
        expect(banner.textContent).not.toContain("This meeting was deleted.");
        expect(banner.textContent).not.toContain("0xdeadbeef");
    });

    it("shows a privacy-safe generic failure for arbitrary load errors", async () => {
        mocks.invoke.mockImplementation((command: string, args?: any) => {
            if (command === "api_get_chat_model_config") return Promise.resolve({});
            if (command === "api_chat_get_or_create_scoped_conversation")
                return Promise.reject(new Error("database is locked: internal row 0xdeadbeef"));
            return Promise.resolve();
        });
        await act(async () => root.render(<ChatHost><Launcher scope={{ kind: "all", key: "all" }} label="all" /></ChatHost>));
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "all") as HTMLButtonElement).click());
        await flush();
        const banner = container.querySelector('[role="status"]') as HTMLElement;
        expect(banner.textContent).toContain("Chat could not be loaded");
        expect(banner.textContent).not.toContain("0xdeadbeef");
        expect(banner.textContent).not.toContain("database is locked");
    });

    it("renders live sources without meeting navigation and keeps stored sources navigable", async () => {
        await act(async () => root.render(<><ChatMessage role="assistant" content="live" sources={[{ meetingId: "live-1", meetingTitle: "Live recording", chunkType: "live_transcript", snippet: "now", folderName: "", sourceKind: "live_recording" }]} onSourceClick={mocks.routerPush} /><ChatMessage role="assistant" content="saved" sources={[{ meetingId: "meeting-1", meetingTitle: "Planning", chunkType: "transcript", snippet: "then", folderName: "" }]} onSourceClick={mocks.routerPush} /></>));
        const liveSource = container.querySelector('[aria-label="Live recording transcript source"]') as HTMLElement;
        expect(liveSource.tagName).toBe("SPAN");
        liveSource.click();
        expect(mocks.routerPush).not.toHaveBeenCalled();
        (container.querySelector('[aria-label="Open meeting Planning"]') as HTMLButtonElement).click();
        expect(mocks.routerPush).toHaveBeenCalledWith("meeting-1");
    });

    it("passes each launcher scope to the single scoped conversation host", async () => {
        const scopes: ChatScope[] = [
            { kind: "all", key: "all" },
            { kind: "meeting", key: "meeting-1" },
            { kind: "folder", key: "folder-1" },
            { kind: "search_snapshot", key: "snapshot", data: { result_ids: ["chunk-1"] } },
        ];
        await act(async () => root.render(<ChatHost>{scopes.map((scope) => <Launcher key={scope.key} scope={scope} label={scope.kind} />)}</ChatHost>));

        for (const scope of scopes) {
            await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === scope.kind) as HTMLButtonElement).click());
            await flush();
            expect(mocks.invoke).toHaveBeenCalledWith("api_chat_get_or_create_scoped_conversation", { scope, title: null });
        }
        expect(container.querySelectorAll('[aria-label="Search scope"]')).toHaveLength(1);
    });

    it("keeps the complete cross-context path scoped through live promotion", async () => {
        const scopes: ChatScope[] = [
            { kind: "all", key: "all" },
            { kind: "meeting", key: "meeting-1" },
            { kind: "folder", key: "folder-1" },
            { kind: "search_snapshot", key: "snapshot", data: { result_ids: ["chunk-1"] } },
            { kind: "live_recording", key: "live-1" },
        ];
        await act(async () => root.render(<ChatHost>{scopes.map((scope) => <Launcher key={scope.key} scope={scope} label={scope.kind} />)}<Promoter liveScopeKey="live-1" meetingId="meeting-1" /></ChatHost>));

        for (const scope of scopes) {
            await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === scope.kind) as HTMLButtonElement).click());
            await flush();
        }
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "promote") as HTMLButtonElement).click());
        await flush();

        expect(mocks.invoke).toHaveBeenCalledWith("api_chat_promote_live_recording", { liveScopeKey: "live-1", meetingId: "meeting-1" });
        expect(mocks.invoke).toHaveBeenCalledWith("api_chat_get_or_create_scoped_conversation", { scope: { kind: "meeting", key: "meeting-1" }, title: null });
    });

    it("cancels and isolates an old stream when the host scope changes", async () => {
        const all: ChatScope = { kind: "all", key: "all" };
        const meeting: ChatScope = { kind: "meeting", key: "meeting-1" };
        await act(async () => root.render(<ChatHost><Launcher scope={all} label="all" /><Launcher scope={meeting} label="meeting" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        const textarea = container.querySelector("textarea")!;
        await act(async () => {
            textarea.dispatchEvent(new InputEvent("input", { bubbles: true, data: "question", inputType: "insertText" }));
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "question");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.getAttribute("aria-label") === "Send message") as HTMLButtonElement).click());
        await flush();
        const streamCall = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream")!;
        const streamId = streamCall[1].streamId;
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "meeting") as HTMLButtonElement).click());
        await flush();
        expect(mocks.invoke).toHaveBeenCalledWith("api_cancel_chat_stream", { streamId });
        expect(mocks.unlisten).toHaveBeenCalled();

        await act(async () => mocks.listeners.get("chat-stream-done")!({ payload: { streamId, answer: "old answer", sources: [] } }));
        expect(mocks.invoke.mock.calls.filter(([command, args]) => command === "api_chat_save_message" && args.content === "old answer")).toHaveLength(0);
        expect(container.textContent).not.toContain("old answer");
    });

    it("unregisters a delayed stale listener and skips its stream invocation", async () => {
        const all: ChatScope = { kind: "all", key: "all" };
        const meeting: ChatScope = { kind: "meeting", key: "meeting-1" };
        let resolveListener!: (unlisten: () => void) => void;
        let staleCallback!: (event: { payload: any }) => void;
        const lateUnlisten = vi.fn();
        mocks.listen.mockImplementationOnce((_name: string, callback: (event: { payload: any }) => void) => {
            staleCallback = callback;
            return new Promise((resolve) => { resolveListener = resolve; });
        });

        await act(async () => root.render(<ChatHost><Launcher scope={all} label="all" /><Launcher scope={meeting} label="meeting" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        const textarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "old question");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "meeting") as HTMLButtonElement).click());
        await flush();

        await act(async () => resolveListener(lateUnlisten));
        await flush();
        expect(lateUnlisten).toHaveBeenCalledOnce();
        expect(mocks.invoke.mock.calls.some(([command]) => command === "api_chat_with_scoped_conversation_stream")).toBe(false);

        await act(async () => staleCallback({ payload: { streamId: "stale", answer: "old terminal", sources: [] } }));
        expect(container.textContent).not.toContain("old terminal");
        expect(mocks.invoke.mock.calls.some(([command, args]) => command === "api_chat_save_message" && args.content === "old terminal")).toBe(false);
    });

    it("filters stream events by stream and conversation identity", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        const textarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "question");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.getAttribute("aria-label") === "Send message") as HTMLButtonElement).click());
        await flush();
        const streamId = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream")![1].streamId;
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId: "other", sources: [] } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId: "other", text: "wrong" } }));
        await act(async () => mocks.listeners.get("chat-preparation-progress")!({ payload: { streamId: "other", stage: "initial_retrieval", completed: 1, total: 1 } }));
        expect(container.textContent).not.toContain("wrong");
        expect(container.textContent).not.toContain("Initial retrieval complete");
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources: [] } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "right" } }));
        expect(container.textContent).toContain("right");
    });

    it("renders the resolved meeting/folder name when a label is provided, else falls back to the generic label", async () => {
        const cases = [
            { scope: { kind: "meeting", key: "meeting-1" }, label: "Quarterly Review", expected: "Meeting: Quarterly Review" },
            { scope: { kind: "folder", key: "folder-1" }, label: "Projects", expected: "Folder: Projects" },
            { scope: { kind: "meeting", key: "meeting-2" }, label: undefined, expected: "This meeting" },
            { scope: { kind: "folder", key: "folder-2" }, label: undefined, expected: "This folder" },
        ];
        for (const { scope, label, expected } of cases) {
            await act(async () => root.render(<ChatHost><LabeledLauncher scope={scope} label={label} buttonLabel={`open-${scope.key}`} /></ChatHost>));
            await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === `open-${scope.key}`) as HTMLButtonElement).click());
            await flush();
            const scopeBadge = container.querySelector('[aria-label="Search scope"]') as HTMLElement;
            expect(scopeBadge.textContent).toBe(expected);
        }
        // The label is display-only: the persisted conversation identity sent to the backend stays label-free.
        expect(mocks.invoke.mock.calls.filter(([command]) => command === "api_chat_get_or_create_scoped_conversation")).toHaveLength(cases.length);
        expect(mocks.invoke.mock.calls.filter(([command, args]) => command === "api_chat_get_or_create_scoped_conversation" && "label" in args.scope)).toHaveLength(0);
    });
});
