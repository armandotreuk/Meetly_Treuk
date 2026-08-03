"use client";

import { useState, useEffect, useCallback } from "react";
import { logger } from "@/lib/logger";

import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { ModelConfig, ModelSettingsModal } from "@/components/ModelSettingsModal";

export function ChatModelSettings() {
    const [modelConfig, setModelConfig] = useState<ModelConfig>({
        provider: "ollama",
        model: "llama3.2:latest",
        whisperModel: "large-v3",
        apiKey: null,
        ollamaEndpoint: null,
    });

    const fetchModelConfig = useCallback(async () => {
        try {
            const data = await invoke<ModelConfig>("api_get_chat_model_config");
            if (data && data.provider !== null) {
                if (data.provider !== "ollama" && data.provider !== "builtin-ai" && !data.apiKey) {
                    try {
                        const apiKeyData = (await invoke("api_get_api_key", {
                            provider: data.provider,
                        })) as string;
                        data.apiKey = apiKeyData;
                    } catch (err) {
                        logger.error("Failed to fetch API key:", err);
                    }
                }
                if (data.provider === "custom-openai") {
                    try {
                        const customConfig = await invoke<{
                            displayName?: string;
                            endpoint?: string;
                            model?: string;
                            apiKey?: string;
                            maxTokens?: number;
                            temperature?: number;
                            topP?: number;
                        }>("api_get_custom_openai_config");
                        if (customConfig) {
                            data.customOpenAIDisplayName = customConfig.displayName || null;
                            data.customOpenAIEndpoint = customConfig.endpoint || null;
                            data.customOpenAIModel = customConfig.model || null;
                            data.customOpenAIApiKey = customConfig.apiKey || null;
                            data.maxTokens = customConfig.maxTokens || null;
                            data.temperature = customConfig.temperature || null;
                            data.topP = customConfig.topP || null;
                            data.model = customConfig.model || data.model;
                        }
                    } catch (err) {
                        logger.error("Failed to fetch custom OpenAI config:", err);
                    }
                }
                setModelConfig(data);
            }
        } catch (error) {
            logger.error("Failed to fetch chat model config:", error);
            toast.error("Failed to load chat model settings");
        }
    }, []);

    useEffect(() => {
        fetchModelConfig();
    }, [fetchModelConfig]);

    useEffect(() => {
        let cleanup: (() => void) | undefined;
        const setupListener = async () => {
            const { listen } = await import("@tauri-apps/api/event");
            const unlisten = await listen<ModelConfig>("chat-model-config-updated", (event) => {
                setModelConfig(event.payload);
            });
            return unlisten;
        };
        setupListener().then((fn) => (cleanup = fn));
        return () => { cleanup?.(); };
    }, []);

    const handleSaveModelConfig = async (config: ModelConfig) => {
        try {
            await invoke("api_save_chat_model_config", {
                provider: config.provider,
                model: config.model,
                apiKey: config.apiKey,
                ollamaEndpoint: config.ollamaEndpoint,
            });

            setModelConfig(config);

            const { emit } = await import("@tauri-apps/api/event");
            await emit("chat-model-config-updated", config);

            toast.success("Chat model settings saved");
        } catch (error) {
            logger.error("Error saving chat model config:", error);
            toast.error("Failed to save chat model settings");
        }
    };

    return (
        <div className="flex flex-col gap-4">
            <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
                <h3 className="text-lg font-semibold mb-4">Chat Model Configuration</h3>
                <p className="text-sm text-gray-600 mb-6">
                    Configure the AI model used for &quot;Chat with Meetings&quot;. API keys are shared globally — if you already configured a key for Summary, it works here too.
                </p>

                <ModelSettingsModal
                    modelConfig={modelConfig}
                    setModelConfig={setModelConfig}
                    onSave={handleSaveModelConfig}
                    skipInitialFetch={true}
                />
            </div>
        </div>
    );
}
