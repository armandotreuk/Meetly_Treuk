"use client";

import React, { useState, useRef, useEffect, useCallback } from "react";
import { logger } from "@/lib/logger";
import { t } from "@/lib/i18n";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { Send, Square, Loader2, X, MessageSquare, Trash2 } from "lucide-react";
import { useRouter } from "next/navigation";
import { ChatMessage } from "./ChatMessage";
import type {
    ChatMessage as ChatMessageType,
    ChatStreamStartPayload,
    ChatStreamChunkPayload,
    ChatStreamDonePayload,
    ChatStreamErrorPayload,
    ChatStreamAbortPayload,
    ChatMeetingDeletedPayload,
    ChatConversation,
    ChatMessageRow,
    ChatSource,
    ChatScope,
    ChatRetrievalMode,
    ChatPreparationProgressPayload,
} from "@/types";

interface ChatPanelProps {
    scope: ChatScope;
    resolvedLabel?: string;
    onClose: () => void;
}

// ponytail: deletions recorded per panel epoch are capped; overflowing the
// cap suppresses source rendering for the rest of that epoch (see
// sanitizeSources) instead of risking a re-rendered deleted source via an
// in-flight response. Raise the cap only together with a real LRU that
// invalidates responses on eviction.
const MAX_LOCALLY_DELETED_MEETING_IDS = 64;

function parseSources(sourcesJson: string): ChatSource[] | undefined {
    try {
        return JSON.parse(sourcesJson) as ChatSource[];
    } catch {
        return undefined;
    }
}

// ponytail: custom-openai is Custom; refine localhost endpoints as Local when api_get_chat_model_config exposes the endpoint.
function classifyProvider(provider: string | null): "local" | "cloud" | "custom" {
    if (
        provider === "ollama" ||
        provider === "builtin-ai" ||
        provider === "local-llama" ||
        provider === "localllama"
    )
        return "local";
    if (
        provider === "openai" ||
        provider === "claude" ||
        provider === "anthropic" ||
        provider === "groq" ||
        provider === "openrouter"
    )
        return "cloud";
    return "custom";
}

export function ChatPanel({ scope, resolvedLabel, onClose }: ChatPanelProps) {
    const router = useRouter();
    const [messages, setMessages] = useState<ChatMessageType[]>([]);
    const [input, setInput] = useState("");
    const [isLoading, setIsLoading] = useState(false);
    const [isStreaming, setIsStreaming] = useState(false);
    const [retrievalMode, setRetrievalMode] = useState<ChatRetrievalMode>("deep");
    const [preparationProgress, setPreparationProgress] =
        useState<ChatPreparationProgressPayload | null>(null);
    const [conversationId, setConversationId] = useState<string | null>(null);
    const [loadError, setLoadError] = useState<string | null>(null);
    const conversationIdRef = useRef<string | null>(null);
    const messagesEndRef = useRef<HTMLDivElement>(null);
    const inputRef = useRef<HTMLTextAreaElement>(null);
    const unlistenersRef = useRef<UnlistenFn[]>([]);
    const streamIdRef = useRef<string | null>(null);
    // Locally observed deletions, generation-tagged (R74): meeting id to the
    // scope generation during which the deletion was observed. Bounded by
    // MAX_LOCALLY_DELETED_MEETING_IDS; entries from older generations are
    // purged on scope change because older responses can no longer apply.
    const locallyDeletedMeetingsRef = useRef<Map<string, number>>(new Map());
    // Set when the cap overflowed during a generation: every response of that
    // generation may carry an evicted id, so source rendering degrades until
    // the next scope change bounds the epoch.
    const deletedSourcesEvictedInRef = useRef<number | null>(null);
    // True only once the deletion subscription is confirmed registered. Until
    // then (or if registration failed), rendering source/navigation data is
    // unsafe: a committed deletion could not be observed. Fail-closed.
    const deletionSubscriptionActiveRef = useRef(false);
    const [modelLabel, setModelLabel] = useState<string | null>(null);
    const [providerKind, setProviderKind] = useState<string | null>(null);
    const scopeRef = useRef(scope);
    const scopeGenerationRef = useRef(0);
    const scopeIdentityRef = useRef("");
    const scopeIdentity = JSON.stringify(scope);
    if (scopeIdentityRef.current !== scopeIdentity) {
        scopeIdentityRef.current = scopeIdentity;
        scopeGenerationRef.current += 1;
    }
    scopeRef.current = scope;
    const scopeLabel =
        resolvedLabel
            ? scope.kind === "meeting"
                ? t("chat.scope.meetingNamed", { title: resolvedLabel })
                : scope.kind === "folder"
                  ? t("chat.scope.folderNamed", { name: resolvedLabel })
                  : resolvedLabel
            : scope.kind === "all"
              ? t("chat.scope.allMeetings")
              : scope.kind === "meeting"
                ? t("chat.scope.thisMeeting")
                : scope.kind === "folder"
                  ? t("chat.scope.folder")
                  : scope.kind === "live_recording"
                    ? t("chat.scope.liveRecording")
                    : t("chat.scope.searchResults");
    const cleanupListeners = useCallback(() => {
        unlistenersRef.current.forEach((unlisten) => unlisten());
        unlistenersRef.current = [];
    }, []);

    // Drops sources that cannot be rendered safely from a source array before
    // it reaches message state. Fail-closed (R74/R75): the caller passes
    // trust=true only when the deletion subscription was confirmed active
    // BEFORE the underlying data was read - otherwise (pending or failed
    // registration) ALL source/navigation data is suppressed, because a
    // committed deletion could not have been observed. Trusted results are
    // additionally stripped once the recorded-deletion cap overflowed during
    // the current generation (an evicted id could ride in on an in-flight
    // response) and are filtered against the recorded deletions. Malformed
    // (non-array) input is normalized to undefined. Idempotent; never
    // surfaces a raw error.
    const sanitizeSources = useCallback((sources: ChatSource[] | undefined, trusted: boolean) => {
        if (!trusted) return undefined;
        const evictedIn = deletedSourcesEvictedInRef.current;
        if (evictedIn !== null && evictedIn <= scopeGenerationRef.current) return undefined;
        if (!Array.isArray(sources)) return undefined;
        const recorded = locallyDeletedMeetingsRef.current;
        const filtered = sources.filter((source) => !recorded.has(source?.meetingId));
        return filtered.length === sources.length ? sources : filtered;
    }, []);

    useEffect(() => {
        let cancelled = false;
        invoke<{ provider?: string | null; model?: string | null }>("api_get_chat_model_config")
            .then((config) => {
                if (cancelled) return;
                setProviderKind(config?.provider ?? null);
                if (config?.provider && config.model)
                    setModelLabel(`${config.provider} / ${config.model}`);
            })
            .catch((error) => logger.error("Failed to load chat model config:", error));
        return () => {
            cancelled = true;
        };
    }, []);

    useEffect(() => {
        conversationIdRef.current = conversationId;
    }, [conversationId]);

    // Committed-deletion notification (panel lifetime): the backend emits one
    // identity-only event after a meeting deletion commits. The callback
    // records the stable id synchronously (idempotent, bounded with
    // generation-tagged eviction) and scrubs the rows already in state; every
    // later load/update filters through sanitizeSources with trust captured
    // BEFORE its data was read. Registration never blocks conversation
    // creation, sending, or rendering (R75): until it settles - or forever if
    // it fails - every result is applied source-free, and a LATE successful
    // registration re-enables sources only for loads/streams whose data is
    // read after it, never for stripped stale results. After unmount the
    // callback is a no-op.
    useEffect(() => {
        let cancelled = false;
        let unlisten: UnlistenFn | undefined;
        void (async () => {
            const fn = await listen<ChatMeetingDeletedPayload>("chat-meeting-deleted", (event) => {
                const meetingId = event.payload?.meetingId;
                if (cancelled || typeof meetingId !== "string" || !meetingId) return;
                const recorded = locallyDeletedMeetingsRef.current;
                if (!recorded.has(meetingId)) {
                    recorded.set(meetingId, scopeGenerationRef.current);
                    while (recorded.size > MAX_LOCALLY_DELETED_MEETING_IDS) {
                        let oldestId: string | null = null;
                        let oldestGeneration = Infinity;
                        for (const [id, recordedGeneration] of recorded) {
                            if (recordedGeneration < oldestGeneration) {
                                oldestGeneration = recordedGeneration;
                                oldestId = id;
                            }
                        }
                        if (oldestId === null) break;
                        recorded.delete(oldestId);
                        deletedSourcesEvictedInRef.current = scopeGenerationRef.current;
                    }
                }
                setMessages((prev) =>
                    prev.map((message) => {
                        if (!Array.isArray(message.sources)) return message;
                        if (!message.sources.some((source) => source?.meetingId === meetingId)) {
                            return message;
                        }
                        return {
                            ...message,
                            sources: message.sources.filter(
                                (source) => source?.meetingId !== meetingId
                            ),
                        };
                    })
                );
            });
            if (cancelled) fn();
            else {
                unlisten = fn;
                deletionSubscriptionActiveRef.current = true;
            }
        })().catch((error) => {
            // Fail closed: deletionSubscriptionActiveRef stays false, so
            // source/navigation rendering stays suppressed for this panel's
            // lifetime. The raw error is never surfaced to the UI.
            logger.error("Failed to listen for meeting deletions:", error);
        });
        return () => {
            cancelled = true;
            unlisten?.();
        };
    }, []);

    useEffect(() => {
        let cancelled = false;
        const generation = scopeGenerationRef.current;
        // Generation change bounds the deletion epoch (R74): responses from
        // older generations can no longer apply (generation guards below and
        // isCurrentRequest), so their recorded deletions expire with them,
        // and any cap-overflow degradation from an older generation lifts.
        for (const [id, recordedGeneration] of locallyDeletedMeetingsRef.current) {
            if (recordedGeneration < generation) locallyDeletedMeetingsRef.current.delete(id);
        }
        const evictedIn = deletedSourcesEvictedInRef.current;
        if (evictedIn !== null && evictedIn < generation) deletedSourcesEvictedInRef.current = null;
        const oldStreamId = streamIdRef.current;
        if (oldStreamId) void invoke("api_cancel_chat_stream", { streamId: oldStreamId });
        streamIdRef.current = null;
        cleanupListeners();
        setIsLoading(false);
        setIsStreaming(false);
        setRetrievalMode("deep");
        setPreparationProgress(null);
        setLoadError(null);
        conversationIdRef.current = null;
        setConversationId(null);
        setMessages([]);

        const loadConversation = async () => {
            try {
                const conversation = await invoke<ChatConversation>(
                    "api_chat_get_or_create_scoped_conversation",
                    { scope, title: null }
                );
                if (cancelled || generation !== scopeGenerationRef.current) return;
                const conversationId = conversation.id;
                // Trust boundary = read start (R75): a load whose rows are
                // read while the deletion subscription is unconfirmed may
                // predate an unobservable deletion, so it renders source-free
                // even if the subscription succeeds before application. A
                // load read while active is filtered against the recorded
                // deletions at application.
                const sourcesTrusted = deletionSubscriptionActiveRef.current;
                const rows = await invoke<ChatMessageRow[]>("api_chat_get_messages", {
                    conversationId,
                });
                if (cancelled || generation !== scopeGenerationRef.current) return;
                setConversationId(conversationId);
                setMessages(
                    rows.map((row) => ({
                        role: row.role,
                        content: row.content,
                        sources: sanitizeSources(
                            row.sources_json ? parseSources(row.sources_json) : undefined,
                            sourcesTrusted
                        ),
                        isError: row.is_error,
                    }))
                );
            } catch (error) {
                logger.error("Failed to load chat conversation:", error);
                // Map to the typed deleted-meeting condition by EXACT code
                // equality (the backend emits "deleted_meeting_thread|<text>"),
                // with a privacy-safe localized fallback for everything else.
                // Raw backend/database error text is never rendered.
                if (!cancelled && generation === scopeGenerationRef.current) {
                    const message = error instanceof Error ? error.message : String(error);
                    const separator = message.indexOf("|");
                    const code = separator === -1 ? message : message.slice(0, separator);
                    setLoadError(
                        code === "deleted_meeting_thread"
                            ? t("chat.orphan.disclosure")
                            : t("chat.load.failed")
                    );
                }
            }
        };

        void loadConversation();
        return () => {
            cancelled = true;
            conversationIdRef.current = null;
        };
    }, [scope, cleanupListeners, sanitizeSources]);

    // Auto-scroll to bottom on new messages
    useEffect(() => {
        messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }, [messages]);

    // Focus input on mount
    useEffect(() => {
        inputRef.current?.focus();
    }, []);

    // Clean up event listeners on unmount
    useEffect(() => {
        return () => {
            scopeGenerationRef.current += 1;
            void invoke("api_cancel_chat_stream", { streamId: null }).catch((error) =>
                logger.error("Failed to cancel chat stream on close:", error)
            );
            unlistenersRef.current.forEach((unlisten) => unlisten());
            unlistenersRef.current = [];
        };
    }, []);

    const saveMessage = useCallback((conversationId: string, message: ChatMessageType) => {
        if (conversationIdRef.current !== conversationId) return;
        void invoke("api_chat_save_message", {
            conversationId,
            role: message.role,
            content: message.content,
            sources: message.sources ?? null,
            isError: message.isError ?? false,
        }).catch((error) => logger.error("Failed to save chat message:", error));
    }, []);

    const sendQuery = async (query: string) => {
        if (!query || isLoading || isStreaming || !conversationId) return;
        const requestGeneration = scopeGenerationRef.current;
        const streamConversationId = conversationId;
        const isCurrentScope = () =>
            requestGeneration === scopeGenerationRef.current &&
            conversationIdRef.current === streamConversationId;
        let liveTranscriptConsent = false;
        if (scope.kind === "live_recording") {
            const currentConfig = await invoke<{ provider?: string | null }>(
                "api_get_chat_model_config"
            ).catch(() => null);
            if (!isCurrentScope()) return;
            if (classifyProvider(currentConfig?.provider ?? providerKind) !== "local") {
                if (!window.confirm(t("chat.live.disclosure"))) return;
                liveTranscriptConsent = true;
            }
        }

        if (!isCurrentScope()) return;

        const userMessage: ChatMessageType = { role: "user", content: query };
        setMessages((prev) => [...prev, userMessage]);
        saveMessage(conversationId, userMessage);
        setInput("");
        if (inputRef.current) inputRef.current.style.height = "auto";
        setIsLoading(true);
        setPreparationProgress(null);

        // Build history from previous messages (last 10), excluding error bubbles
        const history = messages
            .filter((m) => !m.isError)
            .slice(-10)
            .map((m) => ({
                role: m.role,
                content: m.content,
            }));

        const streamId = crypto.randomUUID();
        streamIdRef.current = streamId;
        const isCurrentRequest = () => isCurrentScope() && streamIdRef.current === streamId;
        // Trust boundary = send start (R75): a stream begun while the
        // deletion subscription is unconfirmed renders source-free for its
        // whole lifecycle, even if registration succeeds mid-stream.
        const streamSourcesTrusted = deletionSubscriptionActiveRef.current;

        cleanupListeners();

        try {
            const unlisteners = unlistenersRef.current;

            const unlistenStart = await listen<ChatStreamStartPayload>(
                "chat-stream-start",
                (event) => {
                    if (!isCurrentRequest() || event.payload.streamId !== streamId) return;
                    setIsLoading(false);
                    setIsStreaming(true);
                    setMessages((prev) => [
                        ...prev,
                        {
                            role: "assistant",
                            content: "",
                            sources: sanitizeSources(event.payload.sources, streamSourcesTrusted),
                            isStreaming: true,
                        },
                    ]);
                }
            );
            if (!isCurrentRequest()) {
                unlistenStart();
                return;
            }
            unlisteners.push(unlistenStart);

            const unlistenProgress = await listen<ChatPreparationProgressPayload>(
                "chat-preparation-progress",
                (event) => {
                    if (!isCurrentRequest() || event.payload.streamId !== streamId) return;
                    setPreparationProgress(event.payload);
                }
            );
            if (!isCurrentRequest()) {
                unlistenProgress();
                return;
            }
            unlisteners.push(unlistenProgress);

            const unlistenChunk = await listen<ChatStreamChunkPayload>(
                "chat-stream-chunk",
                (event) => {
                    if (!isCurrentRequest() || event.payload.streamId !== streamId) return;
                    setPreparationProgress(null);
                    setMessages((prev) => {
                        const last = prev[prev.length - 1];
                        if (!last || last.role !== "assistant" || last.isError) {
                            return prev;
                        }
                        const updated: ChatMessageType = {
                            ...last,
                            content: last.content + event.payload.text,
                            isStreaming: true,
                        };
                        return [...prev.slice(0, -1), updated];
                    });
                }
            );
            if (!isCurrentRequest()) {
                unlistenChunk();
                return;
            }
            unlisteners.push(unlistenChunk);

            const unlistenDone = await listen<ChatStreamDonePayload>(
                "chat-stream-done",
                (event) => {
                    if (!isCurrentRequest() || event.payload.streamId !== streamId) return;
                    const sources = sanitizeSources(event.payload.sources, streamSourcesTrusted);
                    setIsStreaming(false);
                    setPreparationProgress(null);
                    cleanupListeners();
                    saveMessage(streamConversationId, {
                        role: "assistant",
                        content: event.payload.answer,
                        sources,
                    });
                    setMessages((prev) => {
                        const last = prev[prev.length - 1];
                        if (!last || last.role !== "assistant" || last.isError) {
                            return prev;
                        }
                        const updated: ChatMessageType = {
                            ...last,
                            content: event.payload.answer || last.content,
                            sources,
                            isStreaming: false,
                        };
                        return [...prev.slice(0, -1), updated];
                    });
                    inputRef.current?.focus();
                }
            );
            if (!isCurrentRequest()) {
                unlistenDone();
                return;
            }
            unlisteners.push(unlistenDone);

            const unlistenError = await listen<ChatStreamErrorPayload>(
                "chat-stream-error",
                (event) => {
                    if (!isCurrentRequest() || event.payload.streamId !== streamId) return;
                    setIsLoading(false);
                    setIsStreaming(false);
                    setPreparationProgress(null);
                    cleanupListeners();
                    logger.error("Chat stream error:", event.payload.error);
                    const errorMessage: ChatMessageType = {
                        role: "assistant",
                        content: t("chat.error.message", { message: event.payload.error }),
                        isError: true,
                    };
                    saveMessage(streamConversationId, errorMessage);
                    setMessages((prev) => {
                        const last = prev[prev.length - 1];
                        if (event.payload.safeCleanup && last && last.role === "assistant" && last.isStreaming) {
                            return [...prev.slice(0, -1), errorMessage];
                        }
                        if (last && last.role === "assistant" && last.isStreaming && last.content) {
                            // Finalize the partial answer that already reached the user.
                            const updated: ChatMessageType = {
                                ...last,
                                isStreaming: false,
                            };
                            return [...prev.slice(0, -1), updated];
                        }
                        return [...prev, errorMessage];
                    });
                    inputRef.current?.focus();
                }
            );
            if (!isCurrentRequest()) {
                unlistenError();
                return;
            }
            unlisteners.push(unlistenError);

            // Deletion invalidation: the backend suppressed every terminal
            // source/done/error publication; this privacy-safe abort event
            // (stable identity + reason only) tells the panel to scrub the
            // rendered source chips / partial answer and restore a usable
            // send state.
            const unlistenAbort = await listen<ChatStreamAbortPayload>(
                "chat-stream-abort",
                (event) => {
                    if (!isCurrentRequest() || event.payload.streamId !== streamId) return;
                    setIsLoading(false);
                    setIsStreaming(false);
                    setPreparationProgress(null);
                    cleanupListeners();
                    setMessages((prev) => {
                        const last = prev[prev.length - 1];
                        if (last && last.role === "assistant" && last.isStreaming) {
                            // Remove the in-flight assistant row entirely: its
                            // sources/partial text may quote deleted content.
                            return prev.slice(0, -1);
                        }
                        return prev;
                    });
                    inputRef.current?.focus();
                }
            );
            if (!isCurrentRequest()) {
                unlistenAbort();
                return;
            }
            unlisteners.push(unlistenAbort);

            if (!isCurrentRequest()) return;
            await invoke("api_chat_with_scoped_conversation_stream", {
                query,
                history,
                authToken: null,
                streamId,
                conversationId: streamConversationId,
                liveTranscriptConsent,
                mode: scope.kind === "live_recording" ? "fast" : retrievalMode,
            });
        } catch (error) {
            if (!isCurrentRequest()) return;
            setIsLoading(false);
            setIsStreaming(false);
            setPreparationProgress(null);
            cleanupListeners();
            logger.error("Chat error:", error);
            const errorMessage: ChatMessageType = {
                role: "assistant",
                content: t("chat.error.message", {
                    message: error instanceof Error ? error.message : String(error),
                }),
                isError: true,
            };
            saveMessage(streamConversationId, errorMessage);
            setMessages((prev) => [...prev, errorMessage]);
            inputRef.current?.focus();
        }
    };

    const handleSend = () => sendQuery(input.trim());

    const handleStop = async () => {
        const streamId = streamIdRef.current;
        if (!streamId || (!isLoading && !isStreaming)) return;
        try {
            await invoke("api_cancel_chat_stream", { streamId });
        } catch (error) {
            logger.error("Failed to cancel chat stream:", error);
        } finally {
            if (streamIdRef.current === streamId) {
                streamIdRef.current = null;
                setIsLoading(false);
                setIsStreaming(false);
                setPreparationProgress(null);
                cleanupListeners();
                inputRef.current?.focus();
            }
        }
    };

    const handleClear = async () => {
        if (!conversationId || isBusy || !confirm(t("chat.clear.confirm"))) return;
        try {
            await invoke("api_chat_clear_conversation", { conversationId });
            const newConversation = await invoke<ChatConversation>(
                "api_chat_get_or_create_scoped_conversation",
                { scope: scopeRef.current, title: null }
            );
            setConversationId(newConversation.id);
            setMessages([]);
        } catch (error) {
            logger.error("Failed to clear chat:", error);
        }
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            handleSend();
        }
    };

    const handleInput = (e: React.FormEvent<HTMLTextAreaElement>) => {
        const textarea = e.currentTarget;
        textarea.style.height = "auto";
        textarea.style.height = `${Math.min(textarea.scrollHeight, 120)}px`;
        setInput(textarea.value);
    };

    const isBusy = isLoading || isStreaming;
    const isLiveScope = scope.kind === "live_recording";
    const selectedRetrievalMode: ChatRetrievalMode = isLiveScope ? "fast" : retrievalMode;
    const providerCategory = classifyProvider(providerKind);
    const providerBadge =
        providerCategory === "local"
            ? {
                  color: "text-green-700",
                  dot: "bg-green-500",
                  title: t("chat.provider.localTooltip"),
              }
            : providerCategory === "cloud"
              ? {
                    color: "text-blue-600",
                    dot: "bg-blue-500",
                    title: t("chat.provider.cloudTooltip"),
                }
              : {
                    color: "text-gray-500",
                    dot: "bg-gray-400",
                    title: t("chat.provider.customTooltip"),
                };
    const suggestedPrompts =
        scope.kind === "meeting" || scope.kind === "live_recording"
            ? [
                  t("chat.suggested.actionItems"),
                  t("chat.suggested.keyDecisions"),
                  t("chat.suggested.attendees"),
                  t("chat.suggested.openQuestions"),
              ]
            : scope.kind === "all"
              ? [
                    t("chat.suggested.todayActionItems"),
                    t("chat.suggested.todaySummary"),
                    t("chat.suggested.weeklyDecisions"),
                    t("chat.suggested.commonTopics"),
                ]
              : [
                    t("chat.suggested.globalActionItems"),
                    t("chat.suggested.recentMeetings"),
                    t("chat.suggested.weeklyDecisions"),
                    t("chat.suggested.commonTopics"),
                ];

    return (
        <div className="flex flex-col h-full bg-white border-t border-gray-200">
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-gray-200 bg-gray-50">
                <div className="flex items-center gap-2">
                    <MessageSquare className="h-4 w-4 text-blue-600" aria-hidden="true" />
                    <div>
                        <div className="text-sm font-medium text-gray-700">
                            {t("chat.header.title")}
                        </div>
                        <div className="flex items-center gap-2">
                            <button
                                onClick={() => router.push("/settings")}
                                aria-label={t("chat.header.configureModelAria")}
                                className="text-xs text-gray-400 hover:text-blue-600"
                            >
                                {modelLabel ?? t("chat.header.configureModel")}
                            </button>
                            {modelLabel && (
                                <span
                                    className={`flex items-center gap-1 text-xs ${providerBadge.color}`}
                                    title={providerBadge.title}
                                >
                                    <span
                                        className={`inline-block h-1.5 w-1.5 rounded-full ${providerBadge.dot}`}
                                        aria-hidden
                                    />
                                    {t(`chat.provider.${providerCategory}`)}
                                </span>
                            )}
                        </div>
                    </div>
                </div>
                <div className="flex items-center gap-2">
                    <span className="text-xs text-gray-500" aria-label={t("chat.scope.aria")}>
                        {scopeLabel}
                    </span>
                    <button
                        onClick={handleClear}
                        disabled={!conversationId || isBusy}
                        aria-label={t("chat.clear.aria")}
                        className="p-1 rounded hover:bg-gray-200 text-gray-400 hover:text-gray-600 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                        <Trash2 className="h-4 w-4" />
                    </button>
                    <button
                        onClick={onClose}
                        aria-label={t("chat.closeAria")}
                        className="p-1 rounded hover:bg-gray-200 text-gray-400 hover:text-gray-600"
                    >
                        <X className="h-4 w-4" />
                    </button>
                </div>
            </div>

            <div className="flex items-start gap-3 border-b border-gray-200 px-4 py-2">
                <div className="flex items-center gap-2">
                    <label
                        htmlFor="chat-retrieval-mode"
                        className="text-xs font-medium text-gray-600"
                    >
                        {t("chat.mode.label")}
                    </label>
                    <select
                        id="chat-retrieval-mode"
                        value={selectedRetrievalMode}
                        onChange={(event) =>
                            setRetrievalMode(event.currentTarget.value as ChatRetrievalMode)
                        }
                        disabled={isLiveScope || isBusy}
                        aria-label={t("chat.mode.label")}
                        aria-describedby="chat-retrieval-mode-help"
                        className="rounded border border-gray-300 bg-white px-2 py-1 text-xs text-gray-700 focus:border-transparent focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:cursor-not-allowed disabled:bg-gray-50 disabled:text-gray-400"
                    >
                        <option value="fast">{t("chat.mode.fast")}</option>
                        <option value="deep">{t("chat.mode.deep")}</option>
                    </select>
                </div>
                <p id="chat-retrieval-mode-help" className="text-xs text-gray-500">
                    {isLiveScope
                        ? t("chat.mode.liveDescription")
                        : selectedRetrievalMode === "deep"
                          ? t("chat.mode.deepDescription")
                          : t("chat.mode.fastDescription")}
                </p>
            </div>

            {/* Messages */}
            <div className="flex-1 overflow-y-auto p-4 space-y-4">
                {loadError && (
                    <div
                        role="status"
                        aria-live="polite"
                        className="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800"
                    >
                        {loadError}
                    </div>
                )}
                {messages.length === 0 && (
                    <div className="text-center text-gray-400 text-sm py-8">
                        {t("chat.empty.description")}
                        <br />
                        {t("chat.empty.searchDescription")}
                        <div
                            className="mt-4 flex flex-wrap justify-center gap-2"
                            aria-label={t("chat.suggested.aria")}
                        >
                            {suggestedPrompts.map((suggestedPrompt) => (
                                <button
                                    key={suggestedPrompt}
                                    onClick={() => sendQuery(suggestedPrompt)}
                                    disabled={isBusy || !conversationId}
                                    aria-label={t("chat.suggested.askAria", {
                                        prompt: suggestedPrompt,
                                    })}
                                    className="rounded-full border border-gray-300 px-3 py-1 text-xs text-gray-700 hover:border-blue-500 hover:text-blue-600 disabled:cursor-not-allowed disabled:opacity-50"
                                >
                                    {suggestedPrompt}
                                </button>
                            ))}
                        </div>
                    </div>
                )}
                {messages.map((msg, i) => (
                    <ChatMessage
                        key={i}
                        role={msg.role}
                        content={msg.content}
                        sources={msg.sources}
                        isStreaming={msg.isStreaming}
                        isError={msg.isError}
                        onSourceClick={(meetingId) =>
                            router.push(`/meeting-details?id=${meetingId}`)
                        }
                    />
                ))}
                {(isLoading || preparationProgress !== null) && (
                    <div className="flex justify-start" role="status" aria-live="polite" aria-atomic="true">
                        <div className="w-7 h-7 rounded-full bg-blue-100 flex items-center justify-center">
                            <Loader2 className="h-4 w-4 text-blue-600 animate-spin" />
                        </div>
                        <div className="ml-2 bg-gray-100 rounded-lg px-3 py-2 text-sm text-gray-500">
                            {preparationProgress?.stage === "initial_retrieval"
                                ? // The agent announces this stage twice: once
                                  // on entry with 0/0, then again with the
                                  // retained source counts. Only the second
                                  // form may claim completion - the first
                                  // covers the longest silent phase of Deep
                                  // preparation.
                                  preparationProgress.total === 0
                                    ? t("chat.preparation.initialRetrievalStarted")
                                    : t("chat.preparation.initialRetrieval", {
                                          completed: preparationProgress.completed,
                                          total: preparationProgress.total,
                                      })
                                : preparationProgress?.stage === "planner_round"
                                  ? t("chat.preparation.plannerRound", {
                                        completed: preparationProgress.completed,
                                        total: preparationProgress.total,
                                    })
                                  : preparationProgress?.stage === "additional_search"
                                    ? t("chat.preparation.additionalSearch", {
                                          completed: preparationProgress.completed,
                                          total: preparationProgress.total,
                                      })
                                    : preparationProgress?.stage === "answer_generation"
                                      ? t("chat.preparation.answerGeneration", {
                                            completed: preparationProgress.completed,
                                            total: preparationProgress.total,
                                        })
                                      : t("chat.searching")}
                        </div>
                    </div>
                )}
                <div ref={messagesEndRef} />
            </div>

            {/* Input */}
            <div className="border-t border-gray-200 p-3">
                <div className="flex gap-2">
                    <textarea
                        ref={inputRef}
                        rows={1}
                        value={input}
                        onInput={handleInput}
                        onKeyDown={handleKeyDown}
                        placeholder={t("chat.input.placeholder")}
                        disabled={isBusy || !conversationId}
                        className="max-h-[120px] flex-1 resize-none overflow-y-auto rounded-lg border border-gray-300 px-3 py-2 text-sm focus:border-transparent focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:bg-gray-50 disabled:text-gray-400"
                    />
                    {isBusy ? (
                        <button
                            onClick={handleStop}
                            aria-label={t("chat.stop.aria")}
                            className="px-3 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700"
                        >
                            <Square className="h-4 w-4" />
                        </button>
                    ) : (
                        <button
                            onClick={handleSend}
                            disabled={!input.trim() || isBusy || !conversationId}
                            aria-label={t("chat.sendAria")}
                            className="px-3 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:bg-gray-300 disabled:cursor-not-allowed"
                        >
                            <Send className="h-4 w-4" />
                        </button>
                    )}
                </div>
            </div>
        </div>
    );
}
