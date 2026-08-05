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

/**
 * Export menu for the meeting summary.
 *
 * Currently offers a single "Export as PDF" entry, but the dropdown
 * is in place to grow into DOCX, Markdown, etc. without further UI
 * changes.
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

    const handleExportPdf = useCallback(async () => {
        if (isExporting) return;
        // ponytail: trust-boundary guard. The trigger is `disabled` when
        // `exportDisabled` is true (no active row), but the dropdown menu
        // item can still be activated via keyboard or a stale DOM event.
        // Refuse loudly rather than pass an empty template id to the backend
        // (which would either throw or, worse, export the wrong row).
        if (!templateId) {
            logger.warn("Skipping PDF export: no active template selected");
            toast.error("Select a template before exporting");
            return;
        }
        setIsExporting(true);
        try {
            Analytics.trackButtonClick("export_pdf", "meeting_details");

            const response = await invoke<ExportPdfResponse>("export_meeting_pdf", {
                request: buildExportPdfRequest(meetingId, templateId, templateSource),
            });

            // The backend returns the PDF as a `Vec<u8>` which Tauri's IPC
            // serialises to a JS `number[]`. Reassemble it into a Uint8Array
            // for the save call.
            const bytes = new Uint8Array(response.bytes);

            const savedPath = await invoke<string | null>("save_meeting_pdf", {
                bytes: Array.from(bytes),
                suggestedFilename: response.suggested_filename,
            });

            if (savedPath) {
                toast.success("PDF exported", {
                    description: savedPath,
                });
            } else {
                // User cancelled the dialog – stay silent.
            }
        } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            toast.error("Failed to export PDF", {
                description: message,
            });
            logger.error("PDF export failed:", err);
        } finally {
            setIsExporting(false);
        }
    }, [meetingId, templateId, templateSource, isExporting]);

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
                        void handleExportPdf();
                    }}
                    disabled={isExporting}
                >
                    <FileDown className="mr-2 h-4 w-4" />
                    <span>Export as PDF</span>
                </DropdownMenuItem>
                <DropdownMenuItem disabled>
                    <span className="mr-2 h-4 w-4 inline-block text-center text-gray-400">↻</span>
                    <span className="text-gray-400">Export as DOCX (coming soon)</span>
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
