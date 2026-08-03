import { useState, useCallback } from "react";
import { logger } from "@/lib/logger";

import { Meeting, Transcript, Summary, SummaryDataResponse, SummaryStatusResponse } from "@/types";
import { ModelConfig } from "@/components/ModelSettingsModal";
import {
    CurrentMeeting,
    useSidebar,
} from "@/components/Sidebar/SidebarProvider";
import { invoke as invokeTauri } from "@tauri-apps/api/core";
import { toast } from "sonner";
import Analytics from "@/lib/analytics";
import { isOllamaNotInstalledError } from "@/lib/utils";
import { BuiltInModelInfo } from "@/lib/builtin-ai";
import {
    detectAndCacheSummaryLanguage,
    readMeetingSummaryLanguage,
    readCachedDetectedSummaryLanguage,
} from "@/lib/summary-language-preferences";

async function resolveSummaryLanguage(
    meetingId: string,
    transcriptTexts: string[]
): Promise<string | null> {
    try {
        const perMeeting = await readMeetingSummaryLanguage(meetingId);
        if (perMeeting.language) return perMeeting.language;
    } catch (err) {
        logger.warn("Failed to load meeting summary language:", err);
        toast.warning("Could not load saved summary language", {
            description: "Using Auto for this generation.",
        });
    }

    try {
        const cachedDetected = await readCachedDetectedSummaryLanguage(meetingId);
        if (cachedDetected) return cachedDetected;
    } catch (err) {
        logger.warn("Failed to load cached detected summary language:", err);
    }

    try {
        const detection = await detectAndCacheSummaryLanguage(meetingId, transcriptTexts);
        if (detection.reason === "tie") {
            toast.warning("Bilingual transcript detected", {
                description: "Pick a summary language manually if Auto chooses the wrong fallback.",
            });
        }
        return detection.language;
    } catch (err) {
        logger.warn("Failed to detect transcript summary language:", err);
        return null;
    }
}

type SummaryStatus = "idle" | "processing" | "summarizing" | "regenerating" | "completed" | "error";

interface UseSummaryGenerationProps {
    meeting: Meeting;
    transcripts: Transcript[];
    modelConfig: ModelConfig;
    isModelConfigLoading: boolean;
    selectedTemplate: string;
    // ponytail: caller must supply the active row key; null means "no row
    // selected yet" → falls back to selectedTemplate for generation and to
    // backend "latest" semantics for get/cancel.
    activeTemplateId?: string | null;
    onMeetingUpdated?: () => Promise<void>;
    updateMeetingTitle: (title: string) => void;
    setAiSummary: (summary: Summary | null) => void;
    onOpenModelSettings?: () => void;
    // Refresh the page-level summaries list (zone-1 dropdown) when a poll
    // terminates so the status badge reflects the new DB row state. Without
    // this the badge keeps the stale pre-run status (e.g. "failed").
    onSummariesChanged?: () => void | Promise<void>;
}

export function useSummaryGeneration({
    meeting,
    transcripts,
    modelConfig,
    isModelConfigLoading,
    selectedTemplate,
    activeTemplateId,
    onMeetingUpdated,
    updateMeetingTitle,
    setAiSummary,
    onOpenModelSettings,
    onSummariesChanged,
}: UseSummaryGenerationProps) {
    const [summaryStatus, setSummaryStatus] = useState<SummaryStatus>("idle");
    const [summaryError, setSummaryError] = useState<string | null>(null);

    const { startSummaryPolling, stopSummaryPolling } = useSidebar();

    // Helper to get status message
    const getSummaryStatusMessage = useCallback((status: SummaryStatus) => {
        switch (status) {
            case "processing":
                return "Processing transcript...";
            case "summarizing":
                return "Generating summary...";
            case "regenerating":
                return "Regenerating summary...";
            case "completed":
                return "Summary completed";
            case "error":
                return "Error generating summary";
            default:
                return "";
        }
    }, []);

    // Unified summary processing logic
    const processSummary = useCallback(
        async ({
            transcriptText,
            transcriptTexts,
            customPrompt = "",
            isRegeneration = false,
        }: {
            transcriptText: string;
            transcriptTexts?: string[];
            customPrompt?: string;
            isRegeneration?: boolean;
        }) => {
            setSummaryStatus(isRegeneration ? "regenerating" : "processing");
            setSummaryError(null);

            try {
                if (!transcriptText.trim()) {
                    throw new Error("No transcript text available. Please add some text first.");
                }

                logger.debug("Processing transcript with template:", selectedTemplate);

                // Calculate time since recording
                const timeSinceRecording =
                    (Date.now() - new Date(meeting.created_at).getTime()) / 60000; // minutes

                // Track summary generation started
                await Analytics.trackSummaryGenerationStarted(
                    modelConfig.provider,
                    modelConfig.model,
                    transcriptText.length,
                    timeSinceRecording
                );

                // Track custom prompt usage if present
                if (customPrompt.trim().length > 0) {
                    await Analytics.trackCustomPromptUsed(customPrompt.trim().length);
                }

                // Show toast notification for generation start
                toast.info(`${isRegeneration ? "Regenerating" : "Generating"} summary...`, {
                    description: `Using ${modelConfig.provider}/${modelConfig.model}`,
                    duration: 3000,
                });

                // Resolve explicit metadata override first; Auto detects the transcript language.
                const summaryLanguage = await resolveSummaryLanguage(
                    meeting.id,
                    transcriptTexts?.length ? transcriptTexts : [transcriptText]
                );

                // Process transcript and get process_id
                const result = await invokeTauri<{ process_id: string }>("api_process_transcript", {
                    text: transcriptText,
                    model: modelConfig.provider,
                    modelName: modelConfig.model,
                    meetingId: meeting.id,
                    chunkSize: 40000,
                    overlap: 1000,
                    customPrompt: customPrompt,
                    templateId: activeTemplateId ?? selectedTemplate,
                    summaryLanguage,
                });

                const process_id = result.process_id;
                logger.debug("Process ID:", process_id);

                // Start global polling via context.
                // ponytail: 4th arg = row key polled. Must match the row
                // `api_process_transcript` just wrote (activeTemplateId ??
                // selectedTemplate); mismatch would silently fall back to
                // "latest" and poll a different row.
                startSummaryPolling(meeting.id, process_id, async (pollingResult) => {
                    logger.debug("Summary status:", pollingResult);

                    // ponytail: one refresh on terminal statuses keeps the
                    // zone-1 dropdown badge in sync with the DB row. The
                    // non-terminal iterations below (processing/summarizing/
                    // regenerating) skip this, so we don't spam the IPC every
                    // 5s. Fire-and-forget; badge update is independent of the
                    // local summaryStatus set below.
                    if (
                        pollingResult.status === "completed" ||
                        pollingResult.status === "error" ||
                        pollingResult.status === "failed" ||
                        pollingResult.status === "cancelled" ||
                        pollingResult.status === "idle"
                    ) {
                        void Promise.resolve(onSummariesChanged?.()).catch((e) =>
                            logger.warn("onSummariesChanged refresh failed:", e)
                        );
                    }

                    // Handle cancellation
                    if (pollingResult.status === "cancelled") {
                        logger.debug("Summary generation was cancelled");

                        // Reload summary from database (backend has already restored from backup)
                        try {
                            const existingSummary = await invokeTauri<SummaryStatusResponse>(
                                "api_get_summary",
                                {
                                    meetingId: meeting.id,
                                    templateId: activeTemplateId ?? undefined,
                                }
                            );

                            if (existingSummary?.data) {
                                logger.debug("Restored previous summary after cancellation");
                                setAiSummary(existingSummary.data as unknown as Summary);
                                setSummaryStatus("completed");
                            } else {
                                setSummaryStatus("idle");
                            }
                        } catch (error) {
                            logger.error("Failed to reload summary after cancellation:", error);
                            setSummaryStatus("idle");
                        }

                        setSummaryError(null);
                        return;
                    }

                    // Handle errors
                    if (pollingResult.status === "error" || pollingResult.status === "failed") {
                        logger.error("Backend returned error:", pollingResult.error);
                        const errorMessage =
                            pollingResult.error ||
                            `Summary ${isRegeneration ? "regeneration" : "generation"} failed`;

                        // If this was a regeneration, try to restore previous summary from database
                        if (isRegeneration) {
                            try {
                                const existingSummary = await invokeTauri<SummaryStatusResponse>(
                                    "api_get_summary",
                                    {
                                        meetingId: meeting.id,
                                        templateId: activeTemplateId ?? undefined,
                                    }
                                );

                                if (existingSummary?.data) {
                                    logger.debug(
                                        "Restored previous summary after regeneration failure"
                                    );
                                    setAiSummary(existingSummary.data as unknown as Summary);
                                    setSummaryStatus("completed");
                                    setSummaryError(null);

                                    // Show error toast with restoration message
                                    toast.error(`Failed to regenerate summary`, {
                                        description: `${errorMessage}. Your previous summary has been restored.`,
                                    });

                                    await Analytics.trackSummaryGenerationCompleted(
                                        modelConfig.provider,
                                        modelConfig.model,
                                        false,
                                        undefined,
                                        errorMessage
                                    );
                                    return;
                                }
                            } catch (error) {
                                logger.error("Failed to reload summary after error:", error);
                            }
                        }

                        // Continue with normal error handling if not regeneration or reload failed
                        setSummaryError(errorMessage);
                        setSummaryStatus("error");

                        // Check if this is a "model is required" error
                        const isModelRequiredError =
                            errorMessage.includes("model is required") ||
                            errorMessage.includes('"model":"required"') ||
                            (errorMessage.toLowerCase().includes("model") &&
                                errorMessage.toLowerCase().includes("required"));

                        // Show error toast
                        toast.error(
                            `Failed to ${isRegeneration ? "regenerate" : "generate"} summary`,
                            {
                                description: errorMessage.includes("Connection refused")
                                    ? "Could not connect to LLM service. Please ensure Ollama or your configured LLM provider is running."
                                    : errorMessage,
                            }
                        );

                        // Auto-open model settings modal if model is missing
                        if (isModelRequiredError && onOpenModelSettings) {
                            logger.debug(
                                "🔧 Model required error detected, opening model settings..."
                            );
                            onOpenModelSettings();
                        }

                        await Analytics.trackSummaryGenerationCompleted(
                            modelConfig.provider,
                            modelConfig.model,
                            false,
                            undefined,
                            errorMessage
                        );
                        return;
                    }

                    // Handle successful completion
                    if (pollingResult.status === "completed" && pollingResult.data) {
                        logger.debug("Summary generation completed:", pollingResult.data);

                        // Update meeting title if available
                        const meetingName =
                            pollingResult.data.MeetingName || pollingResult.meetingName;
                        if (meetingName) {
                            updateMeetingTitle(meetingName);
                        }

                        // Check if backend returned markdown format (new flow)
                        if (pollingResult.data.markdown) {
                            logger.debug("Received markdown format from backend");
                            setAiSummary({
                                markdown: pollingResult.data.markdown,
                            } as unknown as Summary);
                            setSummaryStatus("completed");

                            // Show success toast
                            toast.success("Summary generated successfully!", {
                                description: "Your meeting summary is ready",
                                duration: 4000,
                            });

                            if (meetingName && onMeetingUpdated) {
                                await onMeetingUpdated();
                            }

                            await Analytics.trackSummaryGenerationCompleted(
                                modelConfig.provider,
                                modelConfig.model,
                                true
                            );
                            return;
                        }

                        // Legacy format handling
                        const summarySections = Object.entries(pollingResult.data).filter(
                            ([key]) => key !== "MeetingName"
                        );
                        const allEmpty = summarySections.every(
                            ([, section]) =>
                                !(section as any).blocks || (section as any).blocks.length === 0
                        );

                        if (allEmpty) {
                            logger.error("Summary completed but all sections empty");
                            setSummaryError(
                                "Summary generation completed but returned empty content."
                            );
                            setSummaryStatus("error");

                            await Analytics.trackSummaryGenerationCompleted(
                                modelConfig.provider,
                                modelConfig.model,
                                false,
                                undefined,
                                "Empty summary generated"
                            );
                            return;
                        }

                        // Remove MeetingName from data before formatting
                        const { MeetingName, ...summaryData } = pollingResult.data;

                        // Format legacy summary data
                        const formattedSummary: Summary = {};
                        const sectionKeys =
                            pollingResult.data._section_order || Object.keys(summaryData);

                        for (const key of sectionKeys) {
                            try {
                                const section = summaryData[key];
                                if (
                                    section &&
                                    typeof section === "object" &&
                                    "title" in section &&
                                    "blocks" in section
                                ) {
                                    const typedSection = section as {
                                        title?: string;
                                        blocks?: Array<Record<string, unknown>>;
                                    };

                                    if (Array.isArray(typedSection.blocks)) {
                                        formattedSummary[key] = {
                                            title: typedSection.title || key,
                                            blocks: typedSection.blocks.map((block) => ({
                                                ...block,
                                                id: (block.id as string) ?? key,
                                                type: (block.type as string) ?? "text",
                                                color: "default",
                                                content:
                                                    typeof block.content === "string"
                                                        ? block.content.trim()
                                                        : "",
                                            })),
                                        };
                                    } else {
                                        formattedSummary[key] = {
                                            title: typedSection.title || key,
                                            blocks: [],
                                        };
                                    }
                                }
                            } catch (error) {
                                logger.warn(`Error processing section ${key}:`, error);
                            }
                        }

                        setAiSummary(formattedSummary);
                        setSummaryStatus("completed");

                        // Show success toast
                        toast.success("Summary generated successfully!", {
                            description: "Your meeting summary is ready",
                            duration: 4000,
                        });

                        await Analytics.trackSummaryGenerationCompleted(
                            modelConfig.provider,
                            modelConfig.model,
                            true
                        );

                        if (meetingName && onMeetingUpdated) {
                            await onMeetingUpdated();
                        }
                    }
                }, activeTemplateId ?? selectedTemplate);
            } catch (error) {
                logger.error(
                    `Failed to ${isRegeneration ? "regenerate" : "generate"} summary:`,
                    error
                );
                const errorMessage = error instanceof Error ? error.message : "Unknown error";
                setSummaryError(errorMessage);
                setSummaryStatus("error");
                // Note: We don't clear the summary here because the backend has already restored from backup

                toast.error(`Failed to ${isRegeneration ? "regenerate" : "generate"} summary`, {
                    description: errorMessage,
                });

                await Analytics.trackSummaryGenerationCompleted(
                    modelConfig.provider,
                    modelConfig.model,
                    false,
                    undefined,
                    errorMessage
                );
            }
        },
        [
            meeting.id,
            meeting.created_at,
            modelConfig,
            selectedTemplate,
            activeTemplateId,
            startSummaryPolling,
            setAiSummary,
            updateMeetingTitle,
            onMeetingUpdated,
            onSummariesChanged,
        ]
    );

    // Helper function to fetch ALL transcripts for summary generation
    const fetchAllTranscripts = useCallback(async (meetingId: string): Promise<Transcript[]> => {
        try {
            logger.debug("📊 Fetching all transcripts for meeting:", meetingId);

            // First, get total count by fetching first page
            const firstPage = (await invokeTauri("api_get_meeting_transcripts", {
                meetingId,
                limit: 1,
                offset: 0,
            })) as { transcripts: Transcript[]; total_count: number; has_more: boolean };

            const totalCount = firstPage.total_count;
            logger.debug(`📊 Total transcripts in database: ${totalCount}`);

            if (totalCount === 0) {
                return [];
            }

            // Fetch all transcripts in one call
            const allData = (await invokeTauri("api_get_meeting_transcripts", {
                meetingId,
                limit: totalCount,
                offset: 0,
            })) as { transcripts: Transcript[]; total_count: number; has_more: boolean };

            logger.debug(`✅ Fetched ${allData.transcripts.length} transcripts from database`);
            return allData.transcripts;
        } catch (error) {
            logger.error("❌ Error fetching all transcripts:", error);
            toast.error("Failed to fetch transcripts for summary generation");
            return [];
        }
    }, []);

    const buildSummaryTranscriptPayload = useCallback((allTranscripts: Transcript[]) => {
        const formatTime = (seconds: number | undefined, fallbackTimestamp: string): string => {
            if (seconds === undefined) {
                return fallbackTimestamp;
            }
            const totalSecs = Math.floor(seconds);
            const mins = Math.floor(totalSecs / 60);
            const secs = totalSecs % 60;
            return `[${mins.toString().padStart(2, "0")}:${secs.toString().padStart(2, "0")}]`;
        };

        return {
            transcriptText: allTranscripts
                .map((t) => `${formatTime(t.audio_start_time, t.timestamp)} ${t.text}`)
                .join("\n"),
            transcriptTexts: allTranscripts.map((t) => t.text),
        };
    }, []);

    // Public API: Generate summary from transcripts
    const handleGenerateSummary = useCallback(
        async (customPrompt: string = "") => {
            // Check if model config is still loading
            if (isModelConfigLoading) {
                logger.debug("⏳ Model configuration is still loading, please wait...");
                toast.info("Loading model configuration, please wait...");
                return;
            }

            // CHANGE: Fetch ALL transcripts from database, not from pagination state
            logger.debug("📊 Fetching all transcripts for summary generation...");
            const allTranscripts = await fetchAllTranscripts(meeting.id);

            if (!allTranscripts.length) {
                const error_msg = "No transcripts available for summary";
                logger.debug(error_msg);
                toast.error(error_msg);
                return;
            }

            logger.debug(`✅ Proceeding with ${allTranscripts.length} transcripts`);

            logger.debug("🚀 Starting summary generation with config:", {
                provider: modelConfig.provider,
                model: modelConfig.model,
                template: selectedTemplate,
            });

            // Check if Ollama provider has models available
            if (modelConfig.provider === "ollama") {
                try {
                    const endpoint = modelConfig.ollamaEndpoint || null;
                    const models = (await invokeTauri("get_ollama_models", { endpoint })) as any[];

                    if (!models || models.length === 0) {
                        toast.error(
                            "No Ollama models found. Please download gemma3:1b from Model Settings.",
                            { duration: 5000 }
                        );
                        return;
                    }
                } catch (error) {
                    logger.error("Error checking Ollama models:", error);
                    const errorMessage = error instanceof Error ? error.message : String(error);

                    if (isOllamaNotInstalledError(errorMessage)) {
                        // Ollama is not installed - show specific message with download link
                        toast.error("Ollama is not installed", {
                            description: "Please download and install Ollama to use local models.",
                            duration: 7000,
                            action: {
                                label: "Download",
                                onClick: () =>
                                    invokeTauri("open_external_url", {
                                        url: "https://ollama.com/download",
                                    }),
                            },
                        });
                    } else {
                        // Other error - generic message
                        toast.error(
                            "Failed to check Ollama models. Please ensure Ollama is running and download a model from Settings.",
                            { duration: 5000 }
                        );
                    }
                    return;
                }
            }

            // Check if built-in AI provider has models available
            if (modelConfig.provider === "builtin-ai") {
                try {
                    const selectedModel = modelConfig.model;

                    if (!selectedModel) {
                        toast.error("No built-in AI model selected", {
                            description: "Please select a model in settings",
                            duration: 5000,
                        });
                        if (onOpenModelSettings) {
                            onOpenModelSettings();
                        }
                        return;
                    }

                    // Check model readiness with filesystem refresh
                    const isReady = await invokeTauri<boolean>("builtin_ai_is_model_ready", {
                        modelName: selectedModel,
                        refresh: true,
                    });

                    if (!isReady) {
                        // Get detailed model status
                        const modelInfo = await invokeTauri<BuiltInModelInfo | null>(
                            "builtin_ai_get_model_info",
                            {
                                modelName: selectedModel,
                            }
                        );

                        if (modelInfo) {
                            const status = modelInfo.status;

                            if (status.type === "downloading") {
                                toast.info("Model download in progress", {
                                    description: `${selectedModel} is downloading (${status.progress}%). Please wait until download completes.`,
                                    duration: 5000,
                                });
                                return;
                            }

                            if (status.type === "not_downloaded") {
                                toast.error("Built-in AI model not downloaded", {
                                    description: `${selectedModel} needs to be downloaded. Please download it in model settings.`,
                                    duration: 7000,
                                });
                                if (onOpenModelSettings) {
                                    onOpenModelSettings();
                                }
                                return;
                            }

                            if (status.type === "corrupted" || status.type === "error") {
                                const errorDesc =
                                    status.type === "error"
                                        ? status.Error || "The model file has an error"
                                        : "The model file is corrupted";
                                toast.error("Built-in AI model not available", {
                                    description: `${errorDesc}. Please check model settings.`,
                                    duration: 7000,
                                });
                                if (onOpenModelSettings) {
                                    onOpenModelSettings();
                                }
                                return;
                            }
                        }

                        // Fallback if we couldn't get model info
                        toast.error("Built-in AI model not ready", {
                            description: "Please ensure the model is downloaded in settings",
                            duration: 5000,
                        });
                        if (onOpenModelSettings) {
                            onOpenModelSettings();
                        }
                        return;
                    }

                    // Model is ready, continue to backend call
                } catch (error) {
                    logger.error("Error validating built-in AI model:", error);
                    toast.error("Failed to validate built-in AI model", {
                        description: error instanceof Error ? error.message : String(error),
                        duration: 5000,
                    });
                    return;
                }
            }

            const summaryPayload = buildSummaryTranscriptPayload(allTranscripts);

            await processSummary({
                ...summaryPayload,
                customPrompt,
            });
        },
        [
            meeting.id,
            fetchAllTranscripts,
            buildSummaryTranscriptPayload,
            processSummary,
            modelConfig,
            isModelConfigLoading,
            selectedTemplate,
            activeTemplateId,
        ]
    );

    // Public API: Regenerate summary from the current saved transcript
    const handleRegenerateSummary = useCallback(async () => {
        const allTranscripts = await fetchAllTranscripts(meeting.id);

        if (!allTranscripts.length) {
            logger.error("No transcripts available for regeneration");
            toast.error("No transcripts available for summary regeneration");
            return;
        }

        await processSummary({
            ...buildSummaryTranscriptPayload(allTranscripts),
            isRegeneration: true,
        });
    }, [meeting.id, fetchAllTranscripts, buildSummaryTranscriptPayload, processSummary]);

    // Public API: Stop ongoing summary generation
    const handleStopGeneration = useCallback(async () => {
        logger.debug("Stopping summary generation for meeting:", meeting.id);

        try {
            // Call backend to cancel the summary generation
            await invokeTauri("api_cancel_summary", {
                meetingId: meeting.id,
                templateId: activeTemplateId ?? undefined,
            });
            logger.debug("✓ Backend cancellation request sent for meeting:", meeting.id);
        } catch (error) {
            logger.error("Failed to cancel summary generation:", error);
            // Continue with frontend cleanup even if backend call fails
        }

        // Stop polling
        stopSummaryPolling(meeting.id);

        // Reset status to idle
        setSummaryStatus("idle");
        setSummaryError(null);

        // Show toast notification
        toast.info("Summary generation stopped", {
            description: "You can generate a new summary anytime",
            duration: 3000,
        });
    }, [meeting.id, activeTemplateId, stopSummaryPolling]);

    return {
        summaryStatus,
        summaryError,
        handleGenerateSummary,
        handleRegenerateSummary,
        handleStopGeneration,
        getSummaryStatusMessage,
    };
}
