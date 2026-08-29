"use client";

import { useState, useCallback } from "react";
import { logger } from "@/lib/logger";

import { invoke } from "@tauri-apps/api/core";
import { FileDown, Loader2 } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuLabel,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import Analytics from "@/lib/analytics";
import { buildExportPdfRequest } from "@/lib/export-summary";

interface ExportMenuProps {
    meetingId: string;
    /** Identifier of the template to render with. Null when no active row is selected. */
    templateId: string | null;
    /** Template listing, used to resolve source for raw numeric IDs. */
    availableTemplates?: Array<{ id: string; name: string; source?: string }>;
    /** Display name of the template, used in the menu label. */
    templateName?: string;
    /** Disabled state (e.g. summary not ready). */
    disabled?: boolean;
    /** Optional meeting title for analytics / log context. */
    meetingTitle?: string;
    /** Variant forwarded to the trigger button. */
    variant?: "outline" | "ghost" | "default";
    /** Size forwarded to the trigger button. */
    size?: "sm" | "default" | "lg" | "icon";
    /** Hide trigger text labels (icon-only) for narrow layouts. */
    compact?: boolean;
}

interface ExportPdfResponse {
    bytes: number[];
    suggested_filename: string;
    page_count: number;
}

interface ExportDocxResponse {
    bytes: number[];
    suggested_filename: string;
}

interface ExportMarkdownResponse {
    content: string;
    suggested_filename: string;
}

type ExportFormat = "pdf" | "docx" | "markdown";

/**
 * Export menu for the meeting summary. Offers PDF, DOCX, and Markdown.
 */
export function ExportMenu({
    meetingId,
    templateId,
    availableTemplates,
    templateName,
    disabled = false,
    meetingTitle,
    variant = "outline",
    size = "sm",
    compact = false,
}: ExportMenuProps) {
    const [isExporting, setIsExporting] = useState(false);

    const templateSource = availableTemplates?.find((t) => t.id === templateId)?.source;

    const handleExport = useCallback(
        async (format: ExportFormat) => {
            if (isExporting) return;
            // ponytail: trust-boundary guard. The trigger is `disabled` when
            // `exportDisabled` is true (no active row), but the dropdown menu
            // item can still be activated via keyboard or a stale DOM event.
            // Refuse loudly rather than pass an empty template id to the backend
            // (which would either throw or, worse, export the wrong row).
            if (!templateId) {
                logger.warn(`Skipping ${format} export: no active template selected`);
                toast.error("Select a template before exporting");
                return;
            }
            setIsExporting(true);
            try {
                const request = buildExportPdfRequest(meetingId, templateId, templateSource);
                let savedPath: string | null;
                let label: string;

                if (format === "pdf") {
                    Analytics.trackButtonClick("export_pdf", "meeting_details");
                    const response = await invoke<ExportPdfResponse>("export_meeting_pdf", { request });
                    savedPath = await invoke<string | null>("save_meeting_pdf", {
                        bytes: response.bytes,
                        suggestedFilename: response.suggested_filename,
                    });
                    label = "PDF";
                } else if (format === "docx") {
                    Analytics.trackButtonClick("export_docx", "meeting_details");
                    const response = await invoke<ExportDocxResponse>("export_meeting_docx", { request });
                    savedPath = await invoke<string | null>("save_meeting_docx", {
                        bytes: response.bytes,
                        suggestedFilename: response.suggested_filename,
                    });
                    label = "DOCX";
                } else {
                    Analytics.trackButtonClick("export_markdown", "meeting_details");
                    const response = await invoke<ExportMarkdownResponse>("export_meeting_markdown", {
                        request,
                    });
                    savedPath = await invoke<string | null>("save_meeting_markdown", {
                        content: response.content,
                        suggestedFilename: response.suggested_filename,
                    });
                    label = "Markdown";
                }

                if (savedPath) {
                    toast.success(`${label} exported`, {
                        description: savedPath,
                    });
                } else {
                    // User cancelled the dialog – stay silent.
                }
            } catch (err) {
                const message = err instanceof Error ? err.message : String(err);
                toast.error(`Failed to export ${format.toUpperCase()}`, {
                    description: message,
                });
                logger.error(`${format} export failed:`, err);
            } finally {
                setIsExporting(false);
            }
        },
        [meetingId, templateId, templateSource, isExporting]
    );

    return (
        <DropdownMenu>
            <DropdownMenuTrigger asChild>
                <Button
                    variant={variant}
                    size={size}
                    title={disabled ? "Generate a summary first" : "Export meeting summary"}
                    disabled={disabled || isExporting}
                >
                    {isExporting ? (
                        <>
                            <Loader2 className="animate-spin" />
                            <span className={compact ? "hidden" : "hidden lg:inline"}>Exporting...</span>
                        </>
                    ) : (
                        <>
                            <FileDown />
                            <span className={compact ? "hidden" : "hidden lg:inline"}>Export</span>
                        </>
                    )}
                </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
                <DropdownMenuLabel>
                    Export summary
                    {templateName ? (
                        <span className="block text-xs font-normal text-gray-500">
                            Template: {templateName}
                        </span>
                    ) : null}
                </DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                    onSelect={(event) => {
                        event.preventDefault();
                        void handleExport("pdf");
                    }}
                    disabled={isExporting}
                >
                    <FileDown className="mr-2 h-4 w-4" />
                    <span>Export as PDF</span>
                </DropdownMenuItem>
                <DropdownMenuItem
                    onSelect={(event) => {
                        event.preventDefault();
                        void handleExport("docx");
                    }}
                    disabled={isExporting}
                >
                    <FileDown className="mr-2 h-4 w-4" />
                    <span>Export as DOCX</span>
                </DropdownMenuItem>
                <DropdownMenuItem
                    onSelect={(event) => {
                        event.preventDefault();
                        void handleExport("markdown");
                    }}
                    disabled={isExporting}
                >
                    <FileDown className="mr-2 h-4 w-4" />
                    <span>Export as Markdown</span>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                {meetingTitle ? (
                    <DropdownMenuLabel className="text-xs font-normal text-gray-500 truncate">
                        {meetingTitle}
                    </DropdownMenuLabel>
                ) : null}
            </DropdownMenuContent>
        </DropdownMenu>
    );
}
