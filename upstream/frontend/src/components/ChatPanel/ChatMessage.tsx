"use client";

import React, { useEffect, useRef, useState } from "react";
import { User, Bot, Check, Copy, ExternalLink } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ChatSource } from "@/types";
import { t } from "@/lib/i18n";

interface ChatMessageProps {
    role: "user" | "assistant";
    content: string;
    sources?: ChatSource[];
    isStreaming?: boolean;
    isError?: boolean;
    onSourceClick?: (meetingId: string) => void;
}

export function ChatMessage({ role, content, sources, isStreaming, isError, onSourceClick }: ChatMessageProps) {
    const isUser = role === "user";
    const [copied, setCopied] = useState(false);
    const copyTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    useEffect(() => () => {
        if (copyTimeoutRef.current) clearTimeout(copyTimeoutRef.current);
    }, []);

    const handleCopy = async () => {
        try {
            await navigator.clipboard.writeText(content);
            setCopied(true);
            if (copyTimeoutRef.current) clearTimeout(copyTimeoutRef.current);
            copyTimeoutRef.current = setTimeout(() => setCopied(false), 2000);
        } catch {
            return;
        }
    };

    return (
        <div className={`flex gap-3 ${isUser ? "justify-end" : "justify-start"}`}>
            {!isUser && (
                <div className={`shrink-0 w-7 h-7 rounded-full flex items-center justify-center ${isError ? "bg-red-100" : "bg-blue-100"}`}>
                    <Bot className={`h-4 w-4 ${isError ? "text-red-600" : "text-blue-600"}`} />
                </div>
            )}
            <div
                className={`relative max-w-[85%] rounded-lg px-3 py-2 text-sm ${
                    isUser
                        ? "bg-blue-600 text-white"
                        : isError
                          ? "bg-red-100 text-red-900"
                          : "bg-gray-100 text-gray-900"
                }`}
            >
                {!isUser && !isStreaming && !isError && (
                    <button
                        onClick={handleCopy}
                        aria-label={copied ? t("chat.message.copied") : t("chat.message.copy")}
                        title={copied ? t("chat.message.copied") : t("chat.message.copy")}
                        className="absolute right-1 top-1 rounded p-1 text-gray-400 hover:bg-gray-200 hover:text-gray-600"
                    >
                        {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
                        <span aria-live="polite" className="sr-only">{copied ? t("chat.message.copied") : ""}</span>
                    </button>
                )}
                <div className="break-words">
                    {isUser ? content : (
                        <ReactMarkdown
                            remarkPlugins={[remarkGfm]}
                            components={{
                                h1: ({ children }) => <h1 className="mb-2 text-lg font-semibold">{children}</h1>,
                                h2: ({ children }) => <h2 className="mb-2 text-base font-semibold">{children}</h2>,
                                h3: ({ children }) => <h3 className="mb-1 font-semibold">{children}</h3>,
                                p: ({ children }) => <p className="mb-2 last:mb-0">{children}</p>,
                                ul: ({ children }) => <ul className="mb-2 list-disc space-y-1 pl-5 last:mb-0">{children}</ul>,
                                ol: ({ children }) => <ol className="mb-2 list-decimal space-y-1 pl-5 last:mb-0">{children}</ol>,
                                code: ({ children, className }) => <code className={`${className ?? ""} rounded bg-gray-200 px-1 py-0.5 text-xs`}>{children}</code>,
                                pre: ({ children }) => <pre className="mb-2 overflow-x-auto rounded bg-gray-100 p-2 last:mb-0">{children}</pre>,
                            }}
                        >
                            {content}
                        </ReactMarkdown>
                    )}
                    {!isUser && isStreaming && (
                        <span className="inline-block w-2 h-4 ml-0.5 align-middle bg-current animate-pulse" />
                    )}
                </div>
                {!isUser && sources && sources.length > 0 && (
                    <div className="mt-2 pt-2 border-t border-gray-200">
                        <div className="text-xs text-gray-500 mb-1">{t("chat.message.sources")}</div>
                        <div className="flex flex-wrap gap-1">
                            {sources.map((src, i) => {
                                const isLive = src.sourceKind === "live_recording" || src.chunkType === "live_transcript";
                                const SourceTag = isLive ? "span" : "button";
                                return <SourceTag
                                    key={i}
                                    onClick={isLive ? undefined : () => {
                                        if (window.getSelection()?.toString()) return;
                                        onSourceClick?.(src.meetingId);
                                    }}
                                    aria-label={isLive ? t("chat.message.liveSourceAria") : t("chat.message.openMeetingAria", { title: src.meetingTitle })}
                                    title={src.snippet ? `${src.meetingTitle}: ${src.snippet}` : src.meetingTitle}
                                    className={`inline-flex items-center gap-1 rounded bg-gray-200 px-1.5 py-0.5 text-xs text-gray-700 ${isLive ? "" : "cursor-pointer hover:bg-gray-300"}`}
                                >
                                    {!isLive && <ExternalLink className="h-3 w-3" />}
                                    {src.meetingTitle}
                                    {src.folderName && (
                                        <span className="text-gray-400">/ {src.folderName}</span>
                                    )}
                                </SourceTag>;
                            })}
                        </div>
                    </div>
                )}
            </div>
            {isUser && (
                <div className="shrink-0 w-7 h-7 rounded-full bg-blue-600 flex items-center justify-center">
                    <User className="h-4 w-4 text-white" />
                </div>
            )}
        </div>
    );
}
