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

        // The agent announces initial retrieval twice: on entry with 0/0, then
        // with the retained counts. Only the second form may claim completion
        // — the first covers the longest silent phase of Deep preparation.
        await act(async () => progress({ payload: { streamId, stage: "initial_retrieval", completed: 0, total: 0 } }));
        expect(container.querySelector('[role="status"]')?.textContent).toContain("Searching your meetings");
        expect(container.querySelector('[role="status"]')?.textContent).not.toContain("complete");
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

    it("clears rendered sources when a timeout-race deletion aborts before command completion", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        let resolveStream!: () => void;
        mocks.invoke.mockImplementation((command: string, args?: any) => {
            if (command === "api_get_chat_model_config") return Promise.resolve({});
            if (command === "api_chat_get_or_create_scoped_conversation") return Promise.resolve({ id: `conversation-${args.scope.key}` });
            if (command === "api_chat_get_messages") return Promise.resolve([]);
            if (command === "api_chat_with_scoped_conversation_stream") return new Promise<void>((resolve) => { resolveStream = resolve; });
            return Promise.resolve();
        });
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

        // Sources render at start; a chunk makes the row streaming.
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources: [{ meetingId: "deleted-1", meetingTitle: "Deleted", chunkType: "transcript", snippet: "private", folderName: "" }] } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "partial answer" } }));
        expect(container.textContent).toContain("partial answer");
        expect(container.textContent).toContain("Deleted");
        expect(container.querySelector('[aria-label="Stop generating"]')).not.toBeNull();

        // The privacy-safe abort event (identity + reason only) scrubs the
        // in-flight row and restores a usable send state.
        await act(async () => mocks.listeners.get("chat-stream-abort")!({ payload: { streamId, reason: "referenced_meeting_deleted" } }));
        expect(container.textContent).not.toContain("private");
        expect(container.textContent).not.toContain("partial answer");
        expect(container.querySelector('[aria-label="Stop generating"]')).toBeNull();
        expect(container.querySelector('[aria-label="Stop generating"]'), "stop button must be gone").toBeNull();
        expect(container.querySelector("textarea")?.disabled, "textarea must be enabled").toBe(false);
        // A new send is possible: typing re-enables the send button.
        const restoredTextarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(restoredTextarea, "follow-up");
            restoredTextarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        const sendButton = container.querySelector('[aria-label="Send message"]') as HTMLButtonElement;
        expect(sendButton, "send button must exist").not.toBeNull();
        expect(sendButton.disabled, "send button must be enabled").toBe(false);
        await act(async () => resolveStream());
        expect(container.textContent).not.toContain("partial answer");
        expect(mocks.invoke.mock.calls.some(([command, args]) => command === "api_chat_save_message" && args.role === "assistant")).toBe(false);
        // A stale/replaced done for a DIFFERENT stream is suppressed by the
        // identity fence (same-stream events cannot occur after abort: the
        // backend suppresses every terminal publication and the listeners
        // are unregistered).
        await act(async () => mocks.listeners.get("chat-stream-done")!({ payload: { streamId: "replaced-stream", answer: "replaced answer", sources: [] } }));
        expect(container.textContent).not.toContain("replaced answer");
        expect(mocks.invoke.mock.calls.some(([command, args]) => command === "api_chat_save_message" && args.content === "replaced answer")).toBe(false);
    });

    it("scrubs the active row on a safe terminal revalidation error and ignores stale cleanup", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        let resolveStream!: () => void;
        mocks.invoke.mockImplementation((command: string, args?: any) => {
            if (command === "api_get_chat_model_config") return Promise.resolve({});
            if (command === "api_chat_get_or_create_scoped_conversation") return Promise.resolve({ id: `conversation-${args.scope.key}` });
            if (command === "api_chat_get_messages") return Promise.resolve([]);
            if (command === "api_chat_with_scoped_conversation_stream") return new Promise<void>((resolve) => { resolveStream = resolve; });
            return Promise.resolve();
        });
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

        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources: [{ meetingId: "meeting-1", meetingTitle: "Private meeting", chunkType: "transcript", snippet: "private source", folderName: "" }] } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "private partial" } }));
        await act(async () => mocks.listeners.get("chat-stream-error")!({ payload: { streamId: "replaced-stream", error: "database: private", safeCleanup: true } }));
        expect(container.textContent).toContain("private partial");
        expect(container.textContent).toContain("Private meeting");
        expect(container.querySelector('[aria-label="Stop generating"]')).not.toBeNull();

        await act(async () => mocks.listeners.get("chat-stream-error")!({ payload: { streamId, error: "The chat context could not be revalidated safely.", safeCleanup: true } }));
        expect(container.textContent).not.toContain("Private meeting");
        expect(container.textContent).not.toContain("private partial");
        expect(container.querySelector('[aria-label="Stop generating"]')).toBeNull();
        expect(container.querySelector("textarea")?.disabled).toBe(false);
        const alert = container.querySelector('[role="alert"]') as HTMLElement;
        expect(alert.textContent).toContain("The chat context could not be revalidated safely.");
        expect(alert.textContent).not.toContain("database");
        const saved = mocks.invoke.mock.calls.find(([command, args]) => command === "api_chat_save_message" && args.role === "assistant");
        expect(saved?.[1]).toMatchObject({ sources: null, isError: true });
        expect(document.activeElement).toBe(container.querySelector("textarea"));
        await act(async () => resolveStream());
        await flush();
        expect(container.querySelectorAll('[role="alert"]')).toHaveLength(1);
        expect(mocks.invoke.mock.calls.filter(([command, args]) => command === "api_chat_save_message" && args.role === "assistant")).toHaveLength(1);
    });

    it("scrubs only the deleted meeting's sources from loaded messages on the deletion notification", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        mocks.invoke.mockImplementation((command: string, args?: any) => {
            if (command === "api_get_chat_model_config") return Promise.resolve({});
            if (command === "api_chat_get_or_create_scoped_conversation") return Promise.resolve({ id: `conversation-${args.scope.key}` });
            if (command === "api_chat_get_messages") return Promise.resolve([
                { id: "row-1", conversation_id: "conversation-all", role: "assistant", content: "history answer", sources_json: "{\"unexpected\":\"shape\"}", is_error: false, created_at: "2026-09-03T00:00:00Z" },
            ]);
            return Promise.resolve();
        });
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        expect(container.textContent).toContain("history answer");
        const textarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "question");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        await flush();
        const streamId = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream")![1].streamId;

        // A completed loaded message carrying sources from two meetings,
        // alongside a history message whose sources array is malformed.
        const sources = [
            { meetingId: "kept-1", meetingTitle: "Kept meeting", chunkType: "transcript", snippet: "kept snippet", folderName: "" },
            { meetingId: "deleted-1", meetingTitle: "Deleted meeting", chunkType: "transcript", snippet: "private deleted snippet", folderName: "" },
        ];
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "completed answer" } }));
        await act(async () => mocks.listeners.get("chat-stream-done")!({ payload: { streamId, answer: "completed answer", sources } }));
        expect(container.textContent).toContain("completed answer");
        expect(container.querySelector('[aria-label="Open meeting Deleted meeting"]')).not.toBeNull();
        expect(container.querySelector('[aria-label="Open meeting Kept meeting"]')).not.toBeNull();
        const assistantSaves = () => mocks.invoke.mock.calls.filter(([command, args]) => command === "api_chat_save_message" && args.role === "assistant").length;
        const savesBefore = assistantSaves();

        // A malformed notification (missing identity) changes nothing.
        await act(async () => mocks.listeners.get("chat-meeting-deleted")!({ payload: {} }));
        expect(container.querySelector('[aria-label="Open meeting Deleted meeting"]')).not.toBeNull();

        // The committed-deletion notification scrubs only that meeting's
        // sources immediately, keeping the answer text and other sources.
        await act(async () => mocks.listeners.get("chat-meeting-deleted")!({ payload: { meetingId: "deleted-1" } }));
        expect(container.textContent).toContain("completed answer");
        expect(container.textContent).toContain("Kept meeting");
        expect(container.textContent).not.toContain("Deleted meeting");
        expect(container.textContent).not.toContain("private deleted snippet");
        expect(container.querySelector('[aria-label="Open meeting Deleted meeting"]')).toBeNull();
        expect(container.querySelector('[aria-label="Open meeting Kept meeting"]')).not.toBeNull();
        expect(container.querySelector("textarea")?.disabled).toBe(false);
        expect(container.querySelectorAll('[role="alert"]')).toHaveLength(0);
        expect(container.textContent).toContain("history answer");
        // The scrub is renderer-local: no re-persistence of any sources.
        expect(assistantSaves()).toBe(savesBefore);

        // A stale/duplicate notification is an idempotent no-op.
        await act(async () => mocks.listeners.get("chat-meeting-deleted")!({ payload: { meetingId: "deleted-1" } }));
        expect(container.querySelector('[aria-label="Open meeting Kept meeting"]')).not.toBeNull();
        expect(container.querySelector('[aria-label="Open meeting Deleted meeting"]')).toBeNull();
        expect(container.textContent).toContain("completed answer");
    });

    it("keeps conversation, input, and sending usable while the deletion listener never registers", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        mocks.listen.mockImplementation((name: string, callback: (event: { payload: any }) => void) => {
            if (name === "chat-meeting-deleted") {
                // Registration never settles: nothing may await it.
                return new Promise(() => {});
            }
            mocks.listeners.set(name, callback);
            return mocks.unlisten;
        });
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        // Conversation creation and input are NOT blocked by the pending
        // registration, and the listener callback was never installed.
        expect(mocks.invoke.mock.calls.some(([command]) => command === "api_chat_get_or_create_scoped_conversation")).toBe(true);
        expect(mocks.listeners.has("chat-meeting-deleted")).toBe(false);
        expect(container.querySelector("textarea")?.disabled).toBe(false);
        const textarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "question");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        await flush();
        const streamId = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream")![1].streamId;
        const sources = [{ meetingId: "m-gone", meetingTitle: "Gone meeting", chunkType: "transcript", snippet: "private gone snippet", folderName: "" }];
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "pending answer" } }));
        await act(async () => mocks.listeners.get("chat-stream-done")!({ payload: { streamId, answer: "pending answer", sources } }));
        // Text renders source-free while the subscription is unconfirmed; no
        // raw listener error surfaces.
        expect(container.textContent).toContain("pending answer");
        expect(container.textContent).not.toContain("Gone meeting");
        expect(container.textContent).not.toContain("private gone snippet");
        expect(container.querySelectorAll('[aria-label="Open meeting Gone meeting"]').length).toBe(0);
        expect(container.textContent).not.toContain("ipc down");
        expect(container.querySelector('[aria-label="Stop generating"]')).toBeNull();
        expect(container.querySelector("textarea")?.disabled).toBe(false);
    });

    it("filters a locally deleted meeting's sources from a load result that resolves after the deletion", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        let resolveConversation!: (conversation: any) => void;
        let resolveLoad!: (rows: any[]) => void;
        mocks.invoke.mockImplementation((command: string, args?: any) => {
            if (command === "api_get_chat_model_config") return Promise.resolve({});
            if (command === "api_chat_get_or_create_scoped_conversation") return new Promise((resolve) => { resolveConversation = resolve; });
            if (command === "api_chat_get_messages") return new Promise((resolve) => { resolveLoad = resolve; });
            return Promise.resolve();
        });
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        // Registration is confirmed active BEFORE the rows are read.
        await act(async () => resolveConversation({ id: "conversation-all" }));
        await flush();
        // The deletion commits while the load is in flight: the recorded id
        // must filter the load result that resolves afterwards.
        await act(async () => mocks.listeners.get("chat-meeting-deleted")!({ payload: { meetingId: "m-gone" } }));
        await act(async () => resolveLoad([
            { id: "row-1", role: "assistant", content: "loaded answer", sources_json: JSON.stringify([
                { meetingId: "m-gone", meetingTitle: "Gone meeting", chunkType: "transcript", snippet: "private gone snippet", folderName: "" },
                { meetingId: "m-kept", meetingTitle: "Kept meeting", chunkType: "transcript", snippet: "kept snippet", folderName: "" },
            ]), is_error: false },
        ]));
        await flush();
        expect(container.textContent).toContain("loaded answer");
        expect(container.textContent).toContain("Kept meeting");
        expect(container.textContent).not.toContain("Gone meeting");
        expect(container.textContent).not.toContain("private gone snippet");
        expect(container.querySelector('[aria-label="Open meeting Kept meeting"]')).not.toBeNull();
        expect(container.querySelector('[aria-label="Open meeting Gone meeting"]')).toBeNull();
    });

    it("degrades to source-free rendering when the deletion listener registration fails", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        let resolveLoad!: (rows: any[]) => void;
        mocks.listen.mockImplementation((name: string, callback: (event: { payload: any }) => void) => {
            if (name === "chat-meeting-deleted") {
                // Registration failure is fail-closed and never surfaced raw.
                return Promise.reject(new Error("ipc down: raw listener details"));
            }
            mocks.listeners.set(name, callback);
            return mocks.unlisten;
        });
        mocks.invoke.mockImplementation((command: string, args?: any) => {
            if (command === "api_get_chat_model_config") return Promise.resolve({});
            if (command === "api_chat_get_or_create_scoped_conversation") return Promise.resolve({ id: `conversation-${args.scope.key}` });
            if (command === "api_chat_get_messages") return new Promise((resolve) => { resolveLoad = resolve; });
            return Promise.resolve();
        });
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        // No listener exists: a committed deletion could not be observed, so
        // a source-bearing load result must render without any sources.
        expect(mocks.listeners.has("chat-meeting-deleted")).toBe(false);
        await act(async () => resolveLoad([
            { id: "row-1", role: "assistant", content: "loaded answer", sources_json: JSON.stringify([
                { meetingId: "m-gone", meetingTitle: "Gone meeting", chunkType: "transcript", snippet: "private gone snippet", folderName: "" },
            ]), is_error: false },
        ]));
        await flush();
        expect(container.textContent).toContain("loaded answer");
        expect(container.textContent).not.toContain("Gone meeting");
        expect(container.textContent).not.toContain("private gone snippet");
        expect(container.querySelectorAll('[aria-label="Open meeting Gone meeting"]').length).toBe(0);
        // The raw listener error is never surfaced; the panel stays usable.
        expect(container.textContent).not.toContain("ipc down");
        const textarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "question");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        await flush();
        const streamId = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream")![1].streamId;
        const sources = [{ meetingId: "m-gone", meetingTitle: "Gone meeting", chunkType: "transcript", snippet: "private gone snippet", folderName: "" }];
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "stream answer" } }));
        await act(async () => mocks.listeners.get("chat-stream-done")!({ payload: { streamId, answer: "stream answer", sources } }));
        expect(container.textContent).toContain("stream answer");
        expect(container.querySelectorAll('[aria-label="Open meeting Gone meeting"]').length).toBe(0);
        expect(container.querySelector('[aria-label="Stop generating"]')).toBeNull();
        expect(container.querySelector("textarea")?.disabled).toBe(false);
    });

    it("bounds locally retained deletions and degrades source rendering on overflow", async () => {
        const all: ChatScope = { kind: "all", key: "all" };
        const meeting: ChatScope = { kind: "meeting", key: "meeting-1" };
        await act(async () => root.render(<ChatHost><Launcher scope={all} label="all" /><Launcher scope={meeting} label="meeting" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        // Saturate the per-epoch cap (64) and force one eviction.
        for (let index = 0; index < 65; index++) {
            await act(async () => mocks.listeners.get("chat-meeting-deleted")!({ payload: { meetingId: `del-${index}` } }));
        }
        // A response carrying an evicted id and unknown ids cannot restore
        // any source for the rest of this generation: answer text renders,
        // no source chips do.
        const textarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "saturated");
            textarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        await flush();
        const streamId = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream")![1].streamId;
        const saturatedSources = [
            { meetingId: "del-0", meetingTitle: "Evicted meeting", chunkType: "transcript", snippet: "evicted snippet", folderName: "" },
            { meetingId: "m-fresh", meetingTitle: "Fresh meeting", chunkType: "transcript", snippet: "fresh snippet", folderName: "" },
        ];
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources: saturatedSources } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "saturated answer" } }));
        await act(async () => mocks.listeners.get("chat-stream-done")!({ payload: { streamId, answer: "saturated answer", sources: saturatedSources } }));
        expect(container.textContent).toContain("saturated answer");
        expect(container.querySelectorAll('[aria-label="Open meeting Evicted meeting"]').length).toBe(0);
        expect(container.querySelectorAll('[aria-label="Open meeting Fresh meeting"]').length).toBe(0);

        // A scope change bounds the epoch: recorded deletions and the
        // degradation expire with the old generation, so non-deleted sources
        // render again (safety permits preserving other metadata).
        await act(async () => (Array.from(container.querySelectorAll("button")).find((button) => button.textContent === "meeting") as HTMLButtonElement).click());
        await flush();
        const meetingTextarea = container.querySelector("textarea")!;
        await act(async () => {
            Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(meetingTextarea, "after switch");
            meetingTextarea.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
        await flush();
        const nextStreamId = mocks.invoke.mock.calls.filter(([command]) => command === "api_chat_with_scoped_conversation_stream")[1]![1].streamId;
        const freshSources = [{ meetingId: "m-fresh", meetingTitle: "Fresh meeting", chunkType: "transcript", snippet: "fresh snippet", folderName: "" }];
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId: nextStreamId, sources: freshSources } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId: nextStreamId, text: "switched answer" } }));
        await act(async () => mocks.listeners.get("chat-stream-done")!({ payload: { streamId: nextStreamId, answer: "switched answer", sources: freshSources } }));
        expect(container.textContent).toContain("switched answer");
        expect(container.querySelectorAll('[aria-label="Open meeting Fresh meeting"]').length).toBe(1);
        // The evicted id from the previous epoch stays suppressed server-side
        // and is no longer rendered anywhere.
        expect(container.querySelectorAll('[aria-label="Open meeting Evicted meeting"]').length).toBe(0);
    });

    it("ignores deletion events delivered after the panel unmounts", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        const staleCallback = mocks.listeners.get("chat-meeting-deleted")!;
        expect(staleCallback).toBeDefined();

        // Close the persisted panel via a recording start (established
        // harness behavior), then deliver the event to the stale callback.
        mocks.isRecording = true;
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await flush();
        expect(() => staleCallback({ payload: { meetingId: "m-late" } })).not.toThrow();

        // A freshly opened panel instance inherits nothing from the stale
        // callback: a completed message carrying that meeting's source stays.
        mocks.isRecording = false;
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
        const sources = [{ meetingId: "m-late", meetingTitle: "Late meeting", chunkType: "transcript", snippet: "late snippet", folderName: "" }];
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "late answer" } }));
        await act(async () => mocks.listeners.get("chat-stream-done")!({ payload: { streamId, answer: "late answer", sources } }));
        expect(container.textContent).toContain("late answer");
        expect(container.querySelector('[aria-label="Open meeting Late meeting"]')).not.toBeNull();
    });

    it("scrubs the deleted meeting from active and completed rows idempotently", async () => {
        const scope: ChatScope = { kind: "all", key: "all" };
        await act(async () => root.render(<ChatHost><Launcher scope={scope} label="all" /></ChatHost>));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());
        await flush();
        const typeAndSend = async (text: string) => {
            const textarea = container.querySelector("textarea")!;
            await act(async () => {
                Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, text);
                textarea.dispatchEvent(new Event("input", { bubbles: true }));
            });
            await act(async () => (container.querySelector('[aria-label="Send message"]') as HTMLButtonElement).click());
            await flush();
        };
        await typeAndSend("first");
        const sources = [
            { meetingId: "m-gone", meetingTitle: "Gone meeting", chunkType: "transcript", snippet: "private gone snippet", folderName: "" },
            { meetingId: "m-kept", meetingTitle: "Kept meeting", chunkType: "transcript", snippet: "kept snippet", folderName: "" },
        ];
        let streamId = mocks.invoke.mock.calls.find(([command]) => command === "api_chat_with_scoped_conversation_stream")![1].streamId;
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "first answer" } }));
        await act(async () => mocks.listeners.get("chat-stream-done")!({ payload: { streamId, answer: "first answer", sources } }));

        // Second turn: an active streaming row with the same source set.
        await typeAndSend("second");
        streamId = mocks.invoke.mock.calls.filter(([command]) => command === "api_chat_with_scoped_conversation_stream")[1]![1].streamId;
        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "partial second" } }));
        expect(container.querySelector('[aria-label="Stop generating"]')).not.toBeNull();

        // One deletion scrubs BOTH rows' deleted-meeting sources while
        // keeping answer text, the other source, and the active stream.
        await act(async () => mocks.listeners.get("chat-meeting-deleted")!({ payload: { meetingId: "m-gone" } }));
        expect(container.textContent).toContain("first answer");
        expect(container.textContent).toContain("partial second");
        expect(container.textContent).toContain("Kept meeting");
        expect(container.textContent).not.toContain("Gone meeting");
        expect(container.textContent).not.toContain("private gone snippet");
        expect(container.querySelectorAll('[aria-label="Open meeting Kept meeting"]').length).toBe(2);
        expect(container.querySelectorAll('[aria-label="Open meeting Gone meeting"]').length).toBe(0);
        expect(container.querySelector('[aria-label="Stop generating"]')).not.toBeNull();

        // A duplicate event is an idempotent no-op.
        await act(async () => mocks.listeners.get("chat-meeting-deleted")!({ payload: { meetingId: "m-gone" } }));
        expect(container.querySelectorAll('[aria-label="Open meeting Kept meeting"]').length).toBe(2);
        expect(container.querySelectorAll('[aria-label="Open meeting Gone meeting"]').length).toBe(0);
        expect(container.textContent).toContain("first answer");
        expect(container.textContent).toContain("partial second");
    });

    it("restores the active panel when deletion aborts before stream start", async () => {
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

        await act(async () => mocks.listeners.get("chat-stream-abort")!({ payload: { streamId, reason: "referenced_meeting_deleted" } }));
        expect(container.querySelector('[aria-label="Stop generating"]')).toBeNull();
        expect(container.querySelector("textarea")?.disabled).toBe(false);
        expect(container.querySelectorAll('[aria-label="Meeting source"]').length).toBe(0);
    });

    it("ignores a stale source-less abort and clears the current source-less stream", async () => {
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

        await act(async () => mocks.listeners.get("chat-stream-start")!({ payload: { streamId, sources: [] } }));
        await act(async () => mocks.listeners.get("chat-stream-chunk")!({ payload: { streamId, text: "source-less partial" } }));
        await act(async () => mocks.listeners.get("chat-stream-abort")!({ payload: { streamId: "replaced-stream", reason: "referenced_meeting_deleted" } }));
        expect(container.textContent).toContain("source-less partial");
        expect(container.querySelector('[aria-label="Stop generating"]')).not.toBeNull();

        await act(async () => mocks.listeners.get("chat-stream-abort")!({ payload: { streamId, reason: "referenced_meeting_deleted" } }));
        expect(container.textContent).not.toContain("source-less partial");
        expect(container.querySelector('[aria-label="Stop generating"]')).toBeNull();
        expect(container.querySelector("textarea")?.disabled).toBe(false);
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
        // Intercept the first STREAM listener registration (the mount-scoped
        // deletion listener registers separately and is not the stale-stream
        // subject of this test).
        mocks.listen.mockImplementation((name: string, callback: (event: { payload: any }) => void) => {
            if (name !== "chat-stream-start" || staleCallback) {
                mocks.listeners.set(name, callback);
                return mocks.unlisten;
            }
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
