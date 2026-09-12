"use client";

import React, {
    createContext,
    useCallback,
    useContext,
    useEffect,
    useMemo,
    useRef,
    useState,
} from "react";
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
    const [isExpanded, setIsExpanded] = useState(false);
    const returnFocusRef = useRef<HTMLElement | null>(null);
    const expandedPanelRef = useRef<HTMLDivElement | null>(null);
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
            if (!isRecording || nextScope.kind === "live_recording") {
                returnFocusRef.current =
                    document.activeElement instanceof HTMLElement ? document.activeElement : null;
                setIsExpanded(false);
                setChat({ scope: nextScope, label });
            }
        },
        [isRecording]
    );
    const closeChat = useCallback(() => {
        setChat(null);
        setIsExpanded(false);
        const trigger = returnFocusRef.current;
        returnFocusRef.current = null;
        if (typeof window.requestAnimationFrame === "function") {
            window.requestAnimationFrame(() => trigger?.focus());
        } else {
            trigger?.focus();
        }
    }, []);
    useEffect(() => {
        if (!chat || !isExpanded) return;
        const panel = expandedPanelRef.current;
        if (!panel) return;
        const getFocusableElements = () =>
            Array.from(
                panel.querySelectorAll<HTMLElement>(
                    'a[href], button:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
                )
            ).filter((element) => !element.hasAttribute("hidden"));
        const focusInitialElement = () => {
            const composer = panel.querySelector<HTMLTextAreaElement>("textarea:not([disabled])");
            (composer ?? getFocusableElements()[0] ?? panel).focus();
        };
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === "Escape") {
                event.preventDefault();
                closeChat();
                return;
            }
            if (event.key !== "Tab") return;
            const focusable = getFocusableElements();
            if (focusable.length === 0) {
                event.preventDefault();
                panel.focus();
                return;
            }
            const first = focusable[0];
            const last = focusable[focusable.length - 1];
            if (event.shiftKey && document.activeElement === first) {
                event.preventDefault();
                last.focus();
            } else if (!event.shiftKey && document.activeElement === last) {
                event.preventDefault();
                first.focus();
            }
        };
        focusInitialElement();
        document.addEventListener("keydown", onKeyDown);
        return () => document.removeEventListener("keydown", onKeyDown);
    }, [chat, isExpanded, closeChat]);
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
            <div
                className={
                    chat && !isExpanded ? "transition-[padding] duration-200 lg:pr-[27rem]" : ""
                }
            >
                {children}
            </div>
            {chat && (
                <>
                    {isExpanded && (
                        <div className="fixed inset-0 z-40 bg-black/20" aria-hidden="true" />
                    )}
                    <div
                        ref={expandedPanelRef}
                        role={isExpanded ? "dialog" : undefined}
                        aria-modal={isExpanded || undefined}
                        aria-label={isExpanded ? t("chat.header.title") : undefined}
                        tabIndex={isExpanded ? -1 : undefined}
                        className={
                            isExpanded
                                ? "fixed inset-4 z-50 overflow-hidden rounded-xl border border-gray-200 bg-white shadow-2xl"
                                : "fixed bottom-4 right-4 z-50 h-[min(38rem,calc(100dvh-2rem))] w-[calc(100vw-2rem)] max-w-[26rem] overflow-hidden rounded-xl border border-gray-200 bg-white shadow-2xl"
                        }
                    >
                        <ChatPanel
                            scope={chat.scope}
                            resolvedLabel={chat.label}
                            onClose={closeChat}
                            isExpanded={isExpanded}
                            onToggleExpanded={() => setIsExpanded((expanded) => !expanded)}
                        />
                    </div>
                </>
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
