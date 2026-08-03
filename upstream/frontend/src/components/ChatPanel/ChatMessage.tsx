"use client";

import React from "react";
import { User, Bot, ExternalLink } from "lucide-react";
import type { ChatSource } from "@/types";

interface ChatMessageProps {
    role: "user" | "assistant";
    content: string;
    sources?: ChatSource[];
}

export function ChatMessage({ role, content, sources }: ChatMessageProps) {
    const isUser = role === "user";

    return (
        <div className={`flex gap-3 ${isUser ? "justify-end" : "justify-start"}`}>
            {!isUser && (
                <div className="shrink-0 w-7 h-7 rounded-full bg-blue-100 flex items-center justify-center">
                    <Bot className="h-4 w-4 text-blue-600" />
                </div>
            )}
            <div
                className={`max-w-[85%] rounded-lg px-3 py-2 text-sm ${
                    isUser
                        ? "bg-blue-600 text-white"
                        : "bg-gray-100 text-gray-900"
                }`}
            >
                <div className="whitespace-pre-wrap break-words">{content}</div>
                {!isUser && sources && sources.length > 0 && (
                    <div className="mt-2 pt-2 border-t border-gray-200">
                        <div className="text-xs text-gray-500 mb-1">Sources:</div>
                        <div className="flex flex-wrap gap-1">
                            {sources.map((src, i) => (
                                <span
                                    key={i}
                                    className="inline-flex items-center gap-1 text-xs bg-gray-200 text-gray-700 rounded px-1.5 py-0.5"
                                >
                                    <ExternalLink className="h-3 w-3" />
                                    {src.meetingTitle}
                                    {src.folderName && (
                                        <span className="text-gray-400">/ {src.folderName}</span>
                                    )}
                                </span>
                            ))}
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
