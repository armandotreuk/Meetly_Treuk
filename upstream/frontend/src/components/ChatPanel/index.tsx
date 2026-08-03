"use client";

import React, { useState, useRef, useEffect } from "react";
import { logger } from "@/lib/logger";
import { invoke } from "@tauri-apps/api/core";
import { Send, Loader2, X, MessageSquare } from "lucide-react";
import { ChatMessage } from "./ChatMessage";
import type { ChatMessage as ChatMessageType, ChatResponse } from "@/types";

interface ChatPanelProps {
    onClose: () => void;
}

export function ChatPanel({ onClose }: ChatPanelProps) {
    const [messages, setMessages] = useState<ChatMessageType[]>([]);
    const [input, setInput] = useState("");
    const [isLoading, setIsLoading] = useState(false);
    const messagesEndRef = useRef<HTMLDivElement>(null);
    const inputRef = useRef<HTMLInputElement>(null);

    // Auto-scroll to bottom on new messages
    useEffect(() => {
        messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }, [messages]);

    // Focus input on mount
    useEffect(() => {
        inputRef.current?.focus();
    }, []);

    const handleSend = async () => {
        const query = input.trim();
        if (!query || isLoading) return;

        const userMessage: ChatMessageType = { role: "user", content: query };
        setMessages((prev) => [...prev, userMessage]);
        setInput("");
        setIsLoading(true);

        try {
            // Build history from previous messages (last 10)
            const history = messages.slice(-10).map((m) => ({
                role: m.role,
                content: m.content,
            }));

            const response = (await invoke("api_chat_with_meetings", {
                query,
                history,
                authToken: null,
            })) as ChatResponse;

            const assistantMessage: ChatMessageType = {
                role: "assistant",
                content: response.answer,
                sources: response.sources,
            };
            setMessages((prev) => [...prev, assistantMessage]);
        } catch (error) {
            logger.error("Chat error:", error);
            const errorMessage: ChatMessageType = {
                role: "assistant",
                content: `Error: ${error instanceof Error ? error.message : String(error)}`,
            };
            setMessages((prev) => [...prev, errorMessage]);
        } finally {
            setIsLoading(false);
            inputRef.current?.focus();
        }
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            handleSend();
        }
    };

    return (
        <div className="flex flex-col h-full bg-white border-t border-gray-200">
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-gray-200 bg-gray-50">
                <div className="flex items-center gap-2">
                    <MessageSquare className="h-4 w-4 text-blue-600" />
                    <span className="text-sm font-medium text-gray-700">Chat with Meetings</span>
                </div>
                <button
                    onClick={onClose}
                    className="p-1 rounded hover:bg-gray-200 text-gray-400 hover:text-gray-600"
                >
                    <X className="h-4 w-4" />
                </button>
            </div>

            {/* Messages */}
            <div className="flex-1 overflow-y-auto p-4 space-y-4">
                {messages.length === 0 && (
                    <div className="text-center text-gray-400 text-sm py-8">
                        Ask questions about your meetings.
                        <br />
                        The AI will search your transcripts, summaries, and notes.
                    </div>
                )}
                {messages.map((msg, i) => (
                    <ChatMessage
                        key={i}
                        role={msg.role}
                        content={msg.content}
                        sources={msg.sources}
                    />
                ))}
                {isLoading && (
                    <div className="flex justify-start">
                        <div className="w-7 h-7 rounded-full bg-blue-100 flex items-center justify-center">
                            <Loader2 className="h-4 w-4 text-blue-600 animate-spin" />
                        </div>
                        <div className="ml-2 bg-gray-100 rounded-lg px-3 py-2 text-sm text-gray-500">
                            Searching meetings...
                        </div>
                    </div>
                )}
                <div ref={messagesEndRef} />
            </div>

            {/* Input */}
            <div className="border-t border-gray-200 p-3">
                <div className="flex gap-2">
                    <input
                        ref={inputRef}
                        type="text"
                        value={input}
                        onChange={(e) => setInput(e.target.value)}
                        onKeyDown={handleKeyDown}
                        placeholder="Ask about your meetings..."
                        disabled={isLoading}
                        className="flex-1 px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:bg-gray-50 disabled:text-gray-400"
                    />
                    <button
                        onClick={handleSend}
                        disabled={!input.trim() || isLoading}
                        className="px-3 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:bg-gray-300 disabled:cursor-not-allowed"
                    >
                        <Send className="h-4 w-4" />
                    </button>
                </div>
            </div>
        </div>
    );
}
