"use client";

import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ChatScope } from "@/types";
import { ChatPanel } from ".";
import { invoke } from "@tauri-apps/api/core";
import { MessageSquare } from "lucide-react";
import { t } from "@/lib/i18n";
import { useRecordingState } from "@/contexts/RecordingStateContext";

interface ChatHostValue {
    openChat: (scope: ChatScope, label?: string) => void;
    promoteLiveChat: (
        liveScopeKey: string,
        meetingId: string,
        alreadyPromoted?: boolean
    ) => Promise<void>;
    prepareLiveChatPromotion: () => Promise<void>;
}

const ChatHostContext = createContext<ChatHostValue | null>(null);

export function useChatHost() {
    const context = useContext(ChatHostContext);
    if (!context) throw new Error("useChatHost must be used within ChatHost");
    return context;
}

// ponytail: label is display-only (resolved meeting title / folder name); it is never
// sent to the backend or part of the persisted conversation identity, so it lives beside
// the scope instead of inside it (the backend ChatScope is deny_unknown_fields).
interface OpenChatState {
    scope: ChatScope;
    label?: string;
}

export function ChatHost({ children }: { children: React.ReactNode }) {
    const [chat, setChat] = useState<OpenChatState | null>(null);
    const { isRecording, liveTranscriptScopeKey } = useRecordingState();
    useEffect(() => {
        if (isRecording)
            setChat((current) =>
                current?.scope.kind === "live_recording" &&
                current.scope.key === liveTranscriptScopeKey
                    ? current
                    : null
            );
    }, [isRecording, liveTranscriptScopeKey]);
    const openChat = useCallback(
        (nextScope: ChatScope, label?: string) => {
            if (!isRecording || nextScope.kind === "live_recording")
                setChat({ scope: nextScope, label });
        },
        [isRecording]
    );
    const promoteLiveChat = useCallback(
        async (liveScopeKey: string, meetingId: string, alreadyPromoted = false) => {
            if (!alreadyPromoted)
                await invoke("api_chat_promote_live_recording", { liveScopeKey, meetingId });
            setChat((current) =>
                current?.scope.kind === "live_recording" && current.scope.key === liveScopeKey
                    ? { scope: { kind: "meeting", key: meetingId } }
                    : current
            );
        },
        []
    );
    const prepareLiveChatPromotion = useCallback(
        () => invoke<void>("api_cancel_chat_stream", { streamId: null }),
        []
    );
    const value = useMemo(
        () => ({ openChat, promoteLiveChat, prepareLiveChatPromotion }),
        [openChat, promoteLiveChat, prepareLiveChatPromotion]
    );
    return (
        <ChatHostContext.Provider value={value}>
            {children}
            {chat && (
                <div className="fixed bottom-0 right-0 z-30 h-80 w-full max-w-3xl border-l border-t border-gray-200 shadow-xl">
                    <ChatPanel scope={chat.scope} resolvedLabel={chat.label} onClose={() => setChat(null)} />
                </div>
            )}
        </ChatHostContext.Provider>
    );
}

export function LiveChatLauncher({
    isRecording,
    recordingScopeKey,
}: {
    isRecording: boolean;
    recordingScopeKey: string | null;
}) {
    const { openChat } = useChatHost();
    if (!isRecording || !recordingScopeKey) return null;
    return (
        <button
            onClick={() => openChat({ kind: "live_recording", key: recordingScopeKey })}
            className="fixed top-14 right-4 z-20 flex items-center gap-2 rounded-lg bg-white px-3 py-2 text-sm text-blue-600 shadow-sm hover:text-blue-700"
            aria-label={t("chat.live.launcher")}
        >
            <MessageSquare className="h-4 w-4" />
            {t("chat.live.launcher")}
        </button>
    );
}
