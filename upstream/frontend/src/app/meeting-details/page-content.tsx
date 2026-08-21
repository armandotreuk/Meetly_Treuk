"use client";
import { useState, useEffect, useRef } from "react";
import { logger } from "@/lib/logger";

import { motion } from "framer-motion";
import { Meeting, Summary, SummaryResponse, TranscriptSegmentData, MeetingSummaryInfo, SummaryRevision } from "@/types";
import { useSidebar } from "@/components/Sidebar/SidebarProvider";
import Analytics from "@/lib/analytics";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { TranscriptPanel } from "@/components/MeetingDetails/TranscriptPanel";
import { SummaryPanel } from "@/components/MeetingDetails/SummaryPanel";
import { NotesPanel } from "@/components/MeetingDetails/NotesPanel";
import { ModelConfig } from "@/components/ModelSettingsModal";
import { StickyNote, MessageSquare } from "lucide-react";

// Custom hooks
import { useMeetingData } from "@/hooks/meeting-details/useMeetingData";
import { useSummaryGeneration } from "@/hooks/meeting-details/useSummaryGeneration";
import { useTemplates } from "@/hooks/meeting-details/useTemplates";
import { useCopyOperations } from "@/hooks/meeting-details/useCopyOperations";
import { useMeetingOperations } from "@/hooks/meeting-details/useMeetingOperations";
import { useConfig } from "@/contexts/ConfigContext";
import { usePanelResize } from "@/hooks/usePanelResize";
import { togglePanelOnShortcut } from "@/lib/panel-shortcuts";
import { t } from "@/lib/i18n";
import { useChatHost } from "@/components/ChatPanel/ChatHost";

export default function PageContent({
    meeting,
    summaryData,
    isSummaryProcessing = false,
    onSummaryProcessingChange,
    shouldAutoGenerate = false,
    onAutoGenerateComplete,
    onMeetingUpdated,
    onRefetchTranscripts,
    activeTemplateId,
    summaries,
    setActiveTemplateId,
    onSummariesChanged,
    summaryRevision,
    onSummaryRevisionChange,
    // Pagination props for efficient transcript loading
    segments,
    hasMore,
    isLoadingMore,
    totalCount,
    loadedCount,
    onLoadMore,
}: {
    meeting: Meeting;
    summaryData: Summary | null;
    isSummaryProcessing?: boolean;
    onSummaryProcessingChange?: (processing: boolean) => void;
    shouldAutoGenerate?: boolean;
    onAutoGenerateComplete?: () => void;
    onMeetingUpdated?: () => Promise<void>;
    onRefetchTranscripts?: () => Promise<void>;
    activeTemplateId?: string | null;
    // Multi-template summaries (Sprint D) — page-level hook state, threaded down.
    summaries?: MeetingSummaryInfo[];
    setActiveTemplateId?: (templateId: string | null) => void;
    onSummariesChanged?: () => void | Promise<void>;
    summaryRevision?: SummaryRevision | null;
    onSummaryRevisionChange?: (revision: SummaryRevision | null) => void;
    // Pagination props
    segments?: TranscriptSegmentData[];
    hasMore?: boolean;
    isLoadingMore?: boolean;
    totalCount?: number;
    loadedCount?: number;
    onLoadMore?: () => void;
}) {
    logger.debug("📄 PAGE CONTENT: Initializing with data:", {
        meetingId: meeting.id,
        summaryDataKeys: summaryData ? Object.keys(summaryData) : null,
        transcriptsCount: meeting.transcripts?.length,
    });

    // State
    const [customPrompt, setCustomPrompt] = useState<string>("");
    const [isRecording] = useState(false);
    const [summaryResponse] = useState<SummaryResponse | null>(null);
    const [showNotes, setShowNotes] = useState(false);
    const { openChat } = useChatHost();

    // Meeting-details notes panel — resizable from its left edge.
    const notesResize = usePanelResize({
        initial: 320,
        min: 240,
        maxFraction: 0.6,
        side: "right",
        storageKey: "meedly:meeting-notes-width",
    });

    // Transcript — resizable from its right edge (between transcript and summary).
    const transcriptResize = usePanelResize({
        initial: 360,
        min: 240,
        maxFraction: 0.5,
        side: "left",
        storageKey: "meedly:meeting-transcript-width",
    });

    // ponytail: ResizeObserver to detect the SummaryPanel's rendered width so we can toggle
    // compact button labels. Hysteresis avoids flicker when the user drags across the 480px
    // threshold: enter compact at <480, exit compact only at >=500. Upgrade path if this
    // pattern repeats = extract useElementWidth + hysteresis hook.
    const [summaryContainer, setSummaryContainer] = useState<HTMLDivElement | null>(null);
    const [summaryWidth, setSummaryWidth] = useState<number | null>(null);
    const [isSummaryCompact, setIsSummaryCompact] = useState(false);
    useEffect(() => {
        if (!summaryContainer) return;
        const ro = new ResizeObserver(([entry]) => setSummaryWidth(entry.contentRect.width));
        ro.observe(summaryContainer);
        return () => ro.disconnect();
    }, [summaryContainer]);
    useEffect(() => {
        if (summaryWidth === null) return;
        if (!isSummaryCompact && summaryWidth < 480) setIsSummaryCompact(true);
        else if (isSummaryCompact && summaryWidth >= 500) setIsSummaryCompact(false);
    }, [summaryWidth, isSummaryCompact]);

    // Ref to store the modal open function from SummaryGeneratorButtonGroup
    const openModelSettingsRef = useRef<(() => void) | null>(null);

    // Sidebar context
    const { serverAddress } = useSidebar();

    // Get model config from ConfigContext
    const { modelConfig, setModelConfig } = useConfig();

    // Custom hooks
    const templates = useTemplates();
    // ponytail: `activeTemplateId` is the single source of truth for which
    // summary row is active (S2 claim 6: "Never silently defaults to
    // `standard_meeting`"). The previous `?? templates.selectedTemplate`
    // substitution routed save/generation/export to `standard_meeting`
    // whenever `activeTemplateId` was null (orphaned-active path, delete that
    // nulls the active row, or pre-`summariesReady` mount). `useTemplates`
    // seeds `selectedTemplate` with `"standard_meeting"`, so that fallback was
    // the silent default the claim forbids. Downstream consumers now guard
    // null explicitly: `buildSummarySaveArgs` already throws; generation and
    // export skip when null. `templates.selectedTemplate` is only the S1
    // single-template picker value and stays independent of the active row.
    // Ceiling: while `activeTemplateId` is null the panel offers no row to
    // save/generate/export against — that is the intended "no row selected"
    // state, not a degraded default.
    const effectiveTemplateId = activeTemplateId;
    const meetingData = useMeetingData({
        meeting,
        summaryData,
        activeTemplateId: effectiveTemplateId,
        summaryRevision,
        onSummaryRevisionChange,
        onMeetingUpdated,
    });

    // Normalize once: the in-memory transcripts field on Meeting is optional
    const transcripts = meetingData.transcripts ?? [];

    // Callback to register the modal open function
    const handleRegisterModalOpen = (openFn: () => void) => {
        logger.debug("📝 Registering modal open function in PageContent");
        openModelSettingsRef.current = openFn;
    };

    // Callback to trigger modal open (called from error handler)
    const handleOpenModelSettings = () => {
        logger.debug("🔔 Opening model settings from PageContent");
        if (openModelSettingsRef.current) {
            openModelSettingsRef.current();
        } else {
            logger.warn("⚠️ Modal open function not yet registered");
        }
    };

    // Save model config to backend database and sync via event
    const handleSaveModelConfig = async (config?: ModelConfig) => {
        if (!config) return;
        try {
            await invoke("api_save_model_config", {
                provider: config.provider,
                model: config.model,
                whisperModel: config.whisperModel,
                apiKey: config.apiKey ?? null,
                ollamaEndpoint: config.ollamaEndpoint ?? null,
            });

            // Emit event so ConfigContext and other listeners stay in sync
            const { emit } = await import("@tauri-apps/api/event");
            await emit("model-config-updated", config);

            toast.success("Model settings saved successfully");
        } catch (error) {
            logger.error("Failed to save model config:", error);
            toast.error("Failed to save model settings");
        }
    };

    const summaryGeneration = useSummaryGeneration({
        meeting,
        transcripts,
        modelConfig: modelConfig,
        isModelConfigLoading: false,
        selectedTemplate: templates.selectedTemplate,
        activeTemplateId: effectiveTemplateId,
        summaryRevision,
        isExternallyProcessing: isSummaryProcessing,
        onExternalProcessingChange: onSummaryProcessingChange,
        onMeetingUpdated,
        updateMeetingTitle: meetingData.updateMeetingTitle,
        setAiSummary: meetingData.setAiSummary,
        onOpenModelSettings: handleOpenModelSettings,
        onSummariesChanged,
        onSummaryRevisionChange,
    });

    const activeSummaryRow = summaries?.find(
        (summary) => summary.template_id === effectiveTemplateId,
    );
    const localGenerationIsActive =
        summaryGeneration.summaryStatus === "processing" ||
        summaryGeneration.summaryStatus === "summarizing" ||
        summaryGeneration.summaryStatus === "regenerating";
    const selectedSummaryStatus = activeSummaryRow?.status;
    const panelSummaryStatus =
        isSummaryProcessing || localGenerationIsActive
            ? summaryGeneration.summaryStatus
            : selectedSummaryStatus === "failed" || selectedSummaryStatus === "error"
              ? "error"
              : selectedSummaryStatus === "completed"
                ? "completed"
                : selectedSummaryStatus === "processing" || selectedSummaryStatus === "summarizing" || selectedSummaryStatus === "regenerating"
                  ? "processing"
                  : summaryGeneration.summaryStatus;

    const handleTemplateSelect = (templateId: string, templateName: string) => {
        // Existing single-template controls are the S1 selection path. Keep
        // the chosen row key alongside the prompt selection for saves and
        // generation; the page-level multi-row picker remains S2.
        setActiveTemplateId?.(templateId);
        if (templateId !== activeTemplateId) onSummaryRevisionChange?.(null);
        templates.handleTemplateSelection(templateId, templateName);
    };

    const copyOperations = useCopyOperations({
        meeting,
        transcripts,
        meetingTitle: meetingData.meetingTitle,
        aiSummary: meetingData.aiSummary,
        blockNoteSummaryRef: meetingData.blockNoteSummaryRef,
    });

    const meetingOperations = useMeetingOperations({
        meeting,
    });

    // Track page view
    useEffect(() => {
        Analytics.trackPageView("meeting_details");
    }, []);

    // ponytail: Ctrl/Cmd+Shift+N/M avoid Ctrl/Cmd+J downloads and Ctrl/Cmd+Shift+J DevTools, though host applications may reserve them.
    useEffect(() => {
        const onKey = (event: KeyboardEvent) => {
            if (togglePanelOnShortcut(event, "n", () => setShowNotes((show) => !show))) return;
            togglePanelOnShortcut(event, "m", () => openChat({ kind: "meeting", key: meeting.id }, meeting.title));
        };
        window.addEventListener("keydown", onKey);
        return () => window.removeEventListener("keydown", onKey);
    }, [meeting.id, openChat]);

    // Auto-generate summary when flag is set
    useEffect(() => {
        let cancelled = false;

        const autoGenerate = async () => {
            if (shouldAutoGenerate && transcripts.length > 0 && !cancelled) {
                logger.debug(
                    `🤖 Auto-generating summary with ${modelConfig.provider}/${modelConfig.model}...`
                );
                await summaryGeneration.handleGenerateSummary("");

                // Notify parent that auto-generation is complete (only if not cancelled)
                if (onAutoGenerateComplete && !cancelled) {
                    onAutoGenerateComplete();
                }
            }
        };

        autoGenerate();

        // Cleanup: cancel if component unmounts or meeting changes
        return () => {
            cancelled = true;
        };
    }, [shouldAutoGenerate, meeting.id]); // Re-run if meeting changes

    return (
        <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.3, ease: "easeOut" }}
            className="flex flex-col h-screen bg-gray-50"
        >
            <div className="flex flex-1 overflow-hidden">
                <TranscriptPanel
                    transcripts={transcripts}
                    customPrompt={customPrompt}
                    onPromptChange={setCustomPrompt}
                    onCopyTranscript={copyOperations.handleCopyTranscript}
                    onOpenMeetingFolder={meetingOperations.handleOpenMeetingFolder}
                    isRecording={isRecording}
                    disableAutoScroll={true}
                    // Pagination props for efficient loading
                    usePagination={true}
                    segments={segments}
                    hasMore={hasMore}
                    isLoadingMore={isLoadingMore}
                    totalCount={totalCount}
                    loadedCount={loadedCount}
                    onLoadMore={onLoadMore}
                    // Retranscription props
                    meetingId={meeting.id}
                    meetingFolderPath={meeting.folder_path}
                    onRefetchTranscripts={onRefetchTranscripts}
                    width={transcriptResize.width}
                />
                {/* Draggable handle between transcript and summary (controls transcript width) */}
                <div
                    {...transcriptResize.handleProps}
                    className="w-1.5 shrink-0 cursor-col-resize hover:bg-blue-200 active:bg-blue-400"
                    title="Resize transcript"
                />
                <SummaryPanel
                    meeting={meeting}
                    meetingTitle={meetingData.meetingTitle}
                    onTitleChange={meetingData.handleTitleChange}
                    isEditingTitle={meetingData.isEditingTitle}
                    onStartEditTitle={() => meetingData.setIsEditingTitle(true)}
                    onFinishEditTitle={() => meetingData.setIsEditingTitle(false)}
                    isTitleDirty={meetingData.isTitleDirty}
                    summaryRef={meetingData.blockNoteSummaryRef}
                    isSaving={meetingData.isSaving}
                    onSaveAll={meetingData.saveAllChanges}
                    onCopySummary={copyOperations.handleCopySummary}
                    onOpenFolder={meetingOperations.handleOpenMeetingFolder}
                    aiSummary={meetingData.aiSummary}
                     summaryStatus={panelSummaryStatus}
                    isExternallyProcessing={isSummaryProcessing}
                    transcripts={transcripts}
                    modelConfig={modelConfig}
                    setModelConfig={setModelConfig}
                    onSaveModelConfig={handleSaveModelConfig}
                    onGenerateSummary={summaryGeneration.handleGenerateSummary}
                    onStopGeneration={summaryGeneration.handleStopGeneration}
                    customPrompt={customPrompt}
                    summaryResponse={summaryResponse}
                    onSaveSummary={meetingData.handleSaveSummary}
                    onSummaryChange={meetingData.handleSummaryChange}
                    onDirtyChange={meetingData.setIsSummaryDirty}
                    summaryError={summaryGeneration.summaryError}
                     onRegenerateSummary={summaryGeneration.handleRegenerateSummary}
                     getSummaryStatusMessage={summaryGeneration.getSummaryStatusMessage}
                     availableTemplates={templates.availableTemplates}
                     selectedTemplate={templates.selectedTemplate}
                     onTemplateSelect={handleTemplateSelect}
                     summaries={summaries}
                     activeTemplateId={effectiveTemplateId}
                     onActiveTemplateChange={setActiveTemplateId}
                     onSummariesChanged={onSummariesChanged}
                      pendingEditsExist={meetingData.isSummaryDirty}
                     isModelConfigLoading={false}
                    onOpenModelSettings={handleRegisterModalOpen}
                    containerRef={setSummaryContainer}
                    compact={isSummaryCompact}
                />
                {showNotes && (
                    <>
                        <div
                            {...notesResize.handleProps}
                            className="w-1.5 shrink-0 cursor-col-resize hover:bg-blue-200 active:bg-blue-400"
                            title="Resize notes"
                        />
                        <NotesPanel
                            meetingId={meeting.id}
                            width={notesResize.width}
                            onClose={() => setShowNotes(false)}
                        />
                    </>
                )}
            </div>
            {/* F11: Notes toggle button — only shown when notes are hidden so it never overlaps the panel's Save/X buttons */}
            {!showNotes && (
                <button
                    onClick={() => setShowNotes(true)}
                    className="absolute top-4 right-4 z-10 p-2 rounded-lg shadow-sm bg-white text-gray-400 hover:text-gray-600"
                    title="Show notes (Ctrl/Cmd+Shift+N)"
                    aria-label={t("app.meetingDetails.showNotesAria")}
                >
                    <StickyNote className="h-4 w-4" />
                </button>
            )}
{/* Chat toggle button */}
            <button
                onClick={() => openChat({ kind: "meeting", key: meeting.id }, meeting.title)}
                className="absolute top-4 right-12 z-10 p-2 rounded-lg shadow-sm bg-white text-gray-400 hover:text-gray-600"
                title={t("app.meetingDetails.showChatTitle")}
                aria-label={t("app.meetingDetails.showChatAria")}
            >
                <MessageSquare className="h-4 w-4" />
            </button>
        </motion.div>
    );
}
