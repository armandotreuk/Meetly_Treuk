"use client";

import { ModelConfig, ModelSettingsModal } from "@/components/ModelSettingsModal";
import { logger } from "@/lib/logger";

import { OllamaModel } from "@/contexts/ConfigContext";
import { Dialog, DialogContent, DialogTrigger, DialogTitle } from "@/components/ui/dialog";
import { VisuallyHidden } from "@/components/ui/visually-hidden";
import { Button } from "@/components/ui/button";
import { ButtonGroup } from "@/components/ui/button-group";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuLabel,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Sparkles, Settings, Loader2, FileText, Check, Square, Trash2, AlertTriangle } from "lucide-react";
import Analytics from "@/lib/analytics";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useState, useEffect, ReactNode } from "react";
import { formatDistanceToNow } from "date-fns";
import { isOllamaNotInstalledError } from "@/lib/utils";
import { BuiltInModelInfo } from "@/lib/builtin-ai";
import type { MeetingSummaryInfo } from "@/types";
import { ConfirmSwitchSummaryDialog } from "./ConfirmSwitchSummaryDialog";
import { ConfirmDeleteSummaryDialog } from "./ConfirmDeleteSummaryDialog";
import { ChooseTemplateForLegacyDialog } from "./ChooseTemplateForLegacyDialog";

// Sentinel template_id used by the migration backfill for pre-multi-template
// summaries (see summary/commands.rs).
const LEGACY_TEMPLATE_ID = "legacy";
const LEGACY_DISPLAY_NAME = "Summary (original)";

const GENERATING_STATUSES: MeetingSummaryInfo["status"][] = [
    "processing",
    "summarizing",
    "regenerating",
];

// ponytail: duplicated from ConfirmSwitchSummaryDialog (Sprint C output, not
// exportable). If a third consumer appears, extract to a shared module.
const STATUS_STYLES: Record<MeetingSummaryInfo["status"], string> = {
    idle: "bg-gray-100 text-gray-700",
    processing: "bg-blue-100 text-blue-700",
    summarizing: "bg-blue-100 text-blue-700",
    regenerating: "bg-blue-100 text-blue-700",
    completed: "bg-green-100 text-green-700",
    error: "bg-red-100 text-red-700",
    failed: "bg-red-100 text-red-700",
    cancelled: "bg-gray-100 text-gray-500",
};

interface SummaryGeneratorButtonGroupProps {
    languageSlot?: ReactNode;
    modelConfig: ModelConfig;
    setModelConfig: (config: ModelConfig | ((prev: ModelConfig) => ModelConfig)) => void;
    onSaveModelConfig: (config?: ModelConfig) => Promise<void>;
    onGenerateSummary: (customPrompt: string) => Promise<void>;
    onStopGeneration: () => void;
    customPrompt: string;
    summaryStatus: "idle" | "processing" | "summarizing" | "regenerating" | "completed" | "error";
    availableTemplates: Array<{ id: string; name: string; description: string }>;
    onTemplateSelect: (templateId: string, templateName: string) => void;
    hasTranscripts?: boolean;
    isModelConfigLoading?: boolean;
    onOpenModelSettings?: (openFn: () => void) => void;
    compact?: boolean;
    // Multi-template summaries (Sprint D, items 22/23/29). State is owned by
    // the page-level useMeetingSummaries/useActiveSummaryTemplate instances
    // and threaded down so summary fetching stays in sync.
    meetingId?: string;
    summaries?: MeetingSummaryInfo[];
    activeTemplateId?: string | null;
    onActiveTemplateChange?: (templateId: string | null) => void;
    onSummariesChanged?: () => void | Promise<void>;
    pendingEditsExist?: boolean;
    // Legacy callers still pass these; accept and ignore.
    selectedTemplate?: string;
    hasSummary?: boolean;
}

export function SummaryGeneratorButtonGroup({
    modelConfig,
    setModelConfig,
    onSaveModelConfig,
    onGenerateSummary,
    onStopGeneration,
    customPrompt,
    summaryStatus,
    availableTemplates,
    onTemplateSelect,
    hasTranscripts = true,
    isModelConfigLoading = false,
    onOpenModelSettings,
    languageSlot,
    compact = false,
    meetingId = "",
    summaries = [],
    activeTemplateId = null,
    onActiveTemplateChange = () => {},
    onSummariesChanged = () => {},
    pendingEditsExist = false,
}: SummaryGeneratorButtonGroupProps) {
    const [isCheckingModels, setIsCheckingModels] = useState(false);
    const [settingsDialogOpen, setSettingsDialogOpen] = useState(false);
    const [templateMenuOpen, setTemplateMenuOpen] = useState(false);
    const [switchDialogOpen, setSwitchDialogOpen] = useState(false);
    const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
    const [legacyDialogOpen, setLegacyDialogOpen] = useState(false);

    // Expose the function to open the modal via callback registration
    useEffect(() => {
        if (onOpenModelSettings) {
            // Register our open dialog function with the parent by calling the callback
            // This allows the parent to store a reference to this function
            const openDialog = () => {
                logger.debug("📱 Opening model settings dialog via callback");
                setSettingsDialogOpen(true);
            };

            // Call the parent's callback with our open function
            // Note: This assumes onOpenModelSettings accepts a function parameter
            // We'll need to adjust the signature
            onOpenModelSettings(openDialog);
        }
    }, [onOpenModelSettings]);

    if (!hasTranscripts) {
        return null;
    }

    const checkBuiltInAIModelsAndGenerate = async () => {
        setIsCheckingModels(true);
        try {
            const selectedModel = modelConfig.model;

            // Check if specific model is configured
            if (!selectedModel) {
                toast.error("No built-in AI model selected", {
                    description: "Please select a model in settings",
                    duration: 5000,
                });
                setSettingsDialogOpen(true);
                return;
            }

            // Check model readiness (with filesystem refresh)
            const isReady = await invoke<boolean>("builtin_ai_is_model_ready", {
                modelName: selectedModel,
                refresh: true,
            });

            if (isReady) {
                // Model is available, proceed with generation
                onGenerateSummary(customPrompt);
                return;
            }

            // Model not ready - check detailed status
            const modelInfo = await invoke<BuiltInModelInfo | null>("builtin_ai_get_model_info", {
                modelName: selectedModel,
            });

            if (!modelInfo) {
                toast.error("Model not found", {
                    description: `Could not find information for model: ${selectedModel}`,
                    duration: 5000,
                });
                setSettingsDialogOpen(true);
                return;
            }

            // Handle different model states
            const status = modelInfo.status;

            if (status.type === "downloading") {
                toast.info("Model download in progress", {
                    description: `${selectedModel} is downloading (${status.progress}%). Please wait until download completes.`,
                    duration: 5000,
                });
                return;
            }

            if (status.type === "not_downloaded") {
                toast.error("Model not downloaded", {
                    description: `${selectedModel} needs to be downloaded before use. Opening model settings...`,
                    duration: 5000,
                });
                setSettingsDialogOpen(true);
                return;
            }

            if (status.type === "corrupted") {
                toast.error("Model file corrupted", {
                    description: `${selectedModel} file is corrupted. Please delete and re-download.`,
                    duration: 7000,
                });
                setSettingsDialogOpen(true);
                return;
            }

            if (status.type === "error") {
                toast.error("Model error", {
                    description: status.Error || "An error occurred with the model",
                    duration: 5000,
                });
                setSettingsDialogOpen(true);
                return;
            }

            // Fallback
            toast.error("Model not available", {
                description: "The selected model is not ready for use",
                duration: 5000,
            });
            setSettingsDialogOpen(true);
        } catch (error) {
            logger.error("Error checking built-in AI models:", error);
            toast.error("Failed to check model status", {
                description: error instanceof Error ? error.message : String(error),
                duration: 5000,
            });
        } finally {
            setIsCheckingModels(false);
        }
    };

    const checkOllamaModelsAndGenerate = async () => {
        // Handle built-in AI provider
        if (modelConfig.provider === "builtin-ai") {
            await checkBuiltInAIModelsAndGenerate();
            return;
        }

        // Only check for Ollama provider
        if (modelConfig.provider !== "ollama") {
            onGenerateSummary(customPrompt);
            return;
        }

        setIsCheckingModels(true);
        try {
            const endpoint = modelConfig.ollamaEndpoint || null;
            const models = (await invoke("get_ollama_models", { endpoint })) as OllamaModel[];

            if (!models || models.length === 0) {
                // No models available, show message and open settings
                toast.error(
                    "No Ollama models found. Please download gemma2:2b from Model Settings.",
                    { duration: 5000 }
                );
                setSettingsDialogOpen(true);
                return;
            }

            // Models are available, proceed with generation
            onGenerateSummary(customPrompt);
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
                            invoke("open_external_url", { url: "https://ollama.com/download" }),
                    },
                });
            } else {
                // Other error - generic message
                toast.error(
                    "Failed to check Ollama models. Please check if Ollama is running and download a model.",
                    { duration: 5000 }
                );
            }
            setSettingsDialogOpen(true);
        } finally {
            setIsCheckingModels(false);
        }
    };

    const isGenerating =
        summaryStatus === "processing" ||
        summaryStatus === "summarizing" ||
        summaryStatus === "regenerating";

    const templateNameFor = (templateId: string) =>
        templateId === LEGACY_TEMPLATE_ID
            ? LEGACY_DISPLAY_NAME
            : (availableTemplates.find((t) => t.id === templateId)?.name ?? templateId);

    const activeRow = summaries.find((s) => s.template_id === activeTemplateId) ?? null;
    const hasActiveRow = activeRow != null;

    // ponytail: zone lists + name map recomputed per render; summaries and
    // templates are both tiny lists (<20 rows), so memoization is noise.
    const zone2Templates = availableTemplates.filter(
        (t) => t.id !== LEGACY_TEMPLATE_ID && !summaries.some((s) => s.template_id === t.id)
    );
    const templateNames: Record<string, string> = {
        [LEGACY_TEMPLATE_ID]: LEGACY_DISPLAY_NAME,
    };
    for (const t of availableTemplates) templateNames[t.id] = t.name;

    // activeTemplateId points at a row that was deleted elsewhere (and is not
    // a selectable next-generation template either).
    const isOrphanedActive =
        activeTemplateId != null &&
        !hasActiveRow &&
        (activeTemplateId === LEGACY_TEMPLATE_ID ||
            !availableTemplates.some((t) => t.id === activeTemplateId));

    // Lock template switching/deletion while any row is generating; the
    // primary button stays enabled (it is "Regenerate" / Stop).
    const isLocked =
        isGenerating || summaries.some((s) => GENERATING_STATUSES.includes(s.status));

    const handleSelectSummaryRow = (templateId: string) => {
        if (templateId === activeTemplateId) return;
        if (pendingEditsExist) {
            setSwitchDialogOpen(true);
            return;
        }
        onActiveTemplateChange(templateId);
    };

    const handlePrimaryClick = () => {
        Analytics.trackButtonClick("generate_summary", "meeting_details");
        // Item 29: legacy rows are a read-only archive — never regenerate
        // into them; ask for a real template instead.
        if (activeTemplateId === LEGACY_TEMPLATE_ID) {
            setLegacyDialogOpen(true);
            return;
        }
        checkOllamaModelsAndGenerate();
    };

    return (
        <>
        <ButtonGroup>
            {/* Generate Summary or Stop button */}
            {isGenerating ? (
                <Button
                    variant="outline"
                    size="sm"
                    className="bg-gradient-to-r from-red-50 to-orange-50 hover:from-red-100 hover:to-orange-100 border-red-200 xl:px-4"
                    onClick={() => {
                        Analytics.trackButtonClick("stop_summary_generation", "meeting_details");
                        onStopGeneration();
                    }}
                    title="Stop summary generation"
                >
                    <Square className="xl:mr-2" size={18} fill="currentColor" />
                    <span className={compact ? "hidden" : "hidden lg:inline xl:inline"}>Stop</span>
                </Button>
            ) : (
                <Button
                    variant="outline"
                    size="sm"
                    className="bg-gradient-to-r from-blue-50 to-purple-50 hover:from-blue-100 hover:to-purple-100 border-blue-200 xl:px-4"
                    onClick={handlePrimaryClick}
                    disabled={isCheckingModels || isModelConfigLoading}
                    title={
                        isModelConfigLoading
                            ? "Loading model configuration..."
                            : isCheckingModels
                              ? "Checking models..."
                              : hasActiveRow
                                ? "Regenerate AI Summary"
                                : "Generate AI Summary"
                    }
                >
                    {isCheckingModels || isModelConfigLoading ? (
                        <>
                            <Loader2 className="animate-spin xl:mr-2" size={18} />
                            <span className={compact ? "hidden" : "hidden xl:inline"}>Processing...</span>
                        </>
                    ) : (
                        <>
                            <Sparkles className="xl:mr-2" size={18} />
                            <span className={compact ? "hidden" : "hidden lg:inline xl:inline"}>
                                {hasActiveRow ? "Regenerate" : "Generate summary"}
                            </span>
                        </>
                    )}
                </Button>
            )}

            {languageSlot}

            {/* Settings button */}
            <Dialog open={settingsDialogOpen} onOpenChange={setSettingsDialogOpen}>
                <DialogTrigger asChild>
                    <Button variant="outline" size="sm" title="Summary Settings">
                        <Settings />
                        <span className={compact ? "hidden" : "hidden lg:inline"}>AI Model</span>
                    </Button>
                </DialogTrigger>
                <DialogContent aria-describedby={undefined}>
                    <VisuallyHidden>
                        <DialogTitle>Model Settings</DialogTitle>
                    </VisuallyHidden>
                    <ModelSettingsModal
                        onSave={async (config) => {
                            await onSaveModelConfig(config);
                            setSettingsDialogOpen(false);
                        }}
                        modelConfig={modelConfig}
                        setModelConfig={setModelConfig}
                        skipInitialFetch={true}
                        layout="dialog"
                    />
                </DialogContent>
            </Dialog>

            {/* Template selector dropdown: Zone 1 = existing summary rows,
                Zone 2 = templates with no summary yet */}
            {(summaries.length > 0 || availableTemplates.length > 0) && (
                <DropdownMenu open={templateMenuOpen} onOpenChange={setTemplateMenuOpen}>
                    <DropdownMenuTrigger asChild>
                        <Button
                            variant="outline"
                            size="sm"
                            disabled={isLocked}
                            title={
                                isOrphanedActive
                                    ? "Previously selected summary no longer exists"
                                    : "Select summary template"
                            }
                        >
                            <FileText />
                            <span className={compact ? "hidden" : "hidden lg:inline"}>
                                {activeTemplateId ? templateNameFor(activeTemplateId) : "Template"}
                            </span>
                            {activeRow && (
                                <span
                                    className={`hidden lg:inline rounded-full px-1.5 py-0.5 text-xs font-medium ${
                                        STATUS_STYLES[activeRow.status] ||
                                        "bg-gray-100 text-gray-700"
                                    }`}
                                >
                                    {activeRow.status}
                                </span>
                            )}
                            {isOrphanedActive && (
                                <AlertTriangle className="h-4 w-4 text-amber-500" />
                            )}
                        </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end" className="min-w-[16rem]">
                        {summaries.length > 0 && (
                            <>
                                <DropdownMenuLabel>Existing summaries</DropdownMenuLabel>
                                {summaries.map((s) => (
                                    <DropdownMenuItem
                                        key={s.template_id}
                                        onClick={() => handleSelectSummaryRow(s.template_id)}
                                        className="flex items-center justify-between gap-2"
                                    >
                                        <span className="flex min-w-0 flex-col">
                                            <span className="flex items-center gap-1.5">
                                                <span className="truncate">
                                                    {templateNameFor(s.template_id)}
                                                </span>
                                                {activeTemplateId === s.template_id && (
                                                    <Check className="h-4 w-4 shrink-0 text-green-600" />
                                                )}
                                            </span>
                                            <span className="text-xs text-muted-foreground">
                                                {formatDistanceToNow(new Date(s.updated_at), {
                                                    addSuffix: true,
                                                })}
                                            </span>
                                        </span>
                                        <span className="flex shrink-0 items-center gap-1.5">
                                            <span
                                                className={`rounded-full px-1.5 py-0.5 text-xs font-medium ${
                                                    STATUS_STYLES[s.status] ||
                                                    "bg-gray-100 text-gray-700"
                                                }`}
                                            >
                                                {s.status}
                                            </span>
                                            <button
                                                type="button"
                                                title={`Delete ${templateNameFor(s.template_id)}`}
                                                disabled={isLocked}
                                                onClick={(e) => {
                                                    e.stopPropagation();
                                                    setTemplateMenuOpen(false);
                                                    setDeleteTarget(s.template_id);
                                                }}
                                                className="rounded p-1 text-gray-400 hover:bg-red-50 hover:text-red-600 disabled:opacity-40"
                                            >
                                                <Trash2 className="h-3.5 w-3.5" />
                                            </button>
                                        </span>
                                    </DropdownMenuItem>
                                ))}
                            </>
                        )}
                        {summaries.length > 0 && zone2Templates.length > 0 && (
                            <DropdownMenuSeparator />
                        )}
                        {zone2Templates.length > 0 && (
                            <>
                                <DropdownMenuLabel>Available templates</DropdownMenuLabel>
                                {zone2Templates.map((template) => (
                                    <DropdownMenuItem
                                        key={template.id}
                                        onClick={() => {
                                            onActiveTemplateChange(template.id);
                                            onTemplateSelect(template.id, template.name);
                                        }}
                                        title={template.description}
                                        className="flex items-center justify-between gap-2"
                                    >
                                        <span>{template.name}</span>
                                        {activeTemplateId === template.id && (
                                            <Check className="h-4 w-4 text-green-600" />
                                        )}
                                    </DropdownMenuItem>
                                ))}
                            </>
                        )}
                    </DropdownMenuContent>
                </DropdownMenu>
            )}
        </ButtonGroup>

        <ConfirmSwitchSummaryDialog
            open={switchDialogOpen}
            onOpenChange={setSwitchDialogOpen}
            summaries={summaries}
            currentTemplateId={activeTemplateId}
            pendingEditsExist={pendingEditsExist}
            templateNames={templateNames}
            onConfirm={(newTemplateId) => {
                onActiveTemplateChange(newTemplateId);
                setSwitchDialogOpen(false);
            }}
        />
        <ConfirmDeleteSummaryDialog
            open={deleteTarget != null}
            onOpenChange={(open) => {
                if (!open) setDeleteTarget(null);
            }}
            meetingId={meetingId}
            templateId={deleteTarget ?? ""}
            templateDisplayName={deleteTarget ? templateNameFor(deleteTarget) : undefined}
            onDeleted={() => {
                if (deleteTarget === activeTemplateId) onActiveTemplateChange(null);
                void onSummariesChanged();
            }}
        />
        <ChooseTemplateForLegacyDialog
            open={legacyDialogOpen}
            onOpenChange={setLegacyDialogOpen}
            availableTemplates={availableTemplates}
            onChoose={(templateId) => {
                onActiveTemplateChange(templateId);
                setLegacyDialogOpen(false);
            }}
        />
        </>
    );
}
