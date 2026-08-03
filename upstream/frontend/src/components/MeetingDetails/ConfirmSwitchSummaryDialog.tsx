"use client";

import { useState } from "react";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { AlertTriangle, Check } from "lucide-react";
import type { MeetingSummaryInfo } from "@/types";

interface ConfirmSwitchSummaryDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    summaries: MeetingSummaryInfo[];
    currentTemplateId: string | null;
    onConfirm: (newTemplateId: string) => void;
    pendingEditsExist: boolean;
    templateNames?: Record<string, string>;
}

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

export function ConfirmSwitchSummaryDialog({
    open,
    onOpenChange,
    summaries,
    currentTemplateId,
    onConfirm,
    pendingEditsExist,
    templateNames,
}: ConfirmSwitchSummaryDialogProps) {
    const [selected, setSelected] = useState<string | null>(currentTemplateId);

    const displayName = (id: string) =>
        (templateNames && templateNames[id]) || id;

    const handleConfirm = () => {
        if (!selected || selected === currentTemplateId) {
            onOpenChange(false);
            return;
        }
        onConfirm(selected);
    };

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-[450px]">
                <DialogHeader>
                    <DialogTitle>Switch Summary Template</DialogTitle>
                    <DialogDescription>
                        Choose which summary to view for this meeting.
                    </DialogDescription>
                </DialogHeader>

                {pendingEditsExist && (
                    <div className="flex items-start gap-2 rounded-md border border-amber-300 bg-amber-50 p-3 text-sm text-amber-800">
                        <AlertTriangle className="h-4 w-4 mt-0.5 shrink-0" />
                        <span>Unsaved changes will be lost if you switch.</span>
                    </div>
                )}

                <div className="max-h-72 overflow-y-auto space-y-1 py-1">
                    {summaries.length === 0 && (
                        <div className="text-sm text-muted-foreground py-4 text-center">
                            No summaries yet for this meeting.
                        </div>
                    )}
                    {summaries.map((s) => {
                        const isActive = s.template_id === currentTemplateId;
                        const isSelected = s.template_id === selected;
                        return (
                            <button
                                key={s.template_id}
                                type="button"
                                onClick={() => setSelected(s.template_id)}
                                className={`flex w-full items-center justify-between rounded-md border px-3 py-2 text-left text-sm transition-colors ${
                                    isSelected
                                        ? "border-primary bg-accent"
                                        : "border-border hover:bg-accent/50"
                                }`}
                            >
                                <span className="flex items-center gap-2">
                                    {isActive && (
                                        <Check className="h-4 w-4 text-primary shrink-0" />
                                    )}
                                    <span className="font-medium">
                                        {displayName(s.template_id)}
                                    </span>
                                </span>
                                <span
                                    className={`rounded-full px-2 py-0.5 text-xs font-medium ${
                                        STATUS_STYLES[s.status] ||
                                        "bg-gray-100 text-gray-700"
                                    }`}
                                >
                                    {s.status}
                                </span>
                            </button>
                        );
                    })}
                </div>

                <DialogFooter>
                    <Button variant="outline" onClick={() => onOpenChange(false)}>
                        Cancel
                    </Button>
                    <Button
                        onClick={handleConfirm}
                        disabled={!selected || selected === currentTemplateId}
                    >
                        Switch
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}