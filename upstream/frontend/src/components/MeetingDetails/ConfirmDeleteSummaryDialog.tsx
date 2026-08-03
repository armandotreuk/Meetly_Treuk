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
import { Loader2, Trash2 } from "lucide-react";
import { invoke as invokeTauri } from "@tauri-apps/api/core";
import { toast } from "sonner";

interface ConfirmDeleteSummaryDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    meetingId: string;
    templateId: string;
    templateDisplayName?: string;
    onDeleted: () => void;
}

export function ConfirmDeleteSummaryDialog({
    open,
    onOpenChange,
    meetingId,
    templateId,
    templateDisplayName,
    onDeleted,
}: ConfirmDeleteSummaryDialogProps) {
    const [isDeleting, setIsDeleting] = useState(false);
    const name = templateDisplayName || templateId;

    const handleDelete = async () => {
        setIsDeleting(true);
        try {
            // ponytail: backend cancel-then-deletes (commands.rs), so no client-side cancel needed.
            await invokeTauri("api_delete_meeting_summary", { meetingId, templateId });
            onDeleted();
            onOpenChange(false);
        } catch (err) {
            const msg =
                typeof err === "string"
                    ? err
                    : err instanceof Error
                      ? err.message
                      : String(err);
            toast.error("Failed to delete summary", { description: msg });
        } finally {
            setIsDeleting(false);
        }
    };

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-[420px]">
                <DialogHeader>
                    <DialogTitle>Delete Summary</DialogTitle>
                    <DialogDescription>
                        Delete summary for &ldquo;{name}&rdquo;? This cannot be undone.
                    </DialogDescription>
                </DialogHeader>

                <DialogFooter>
                    <Button
                        variant="outline"
                        onClick={() => onOpenChange(false)}
                        disabled={isDeleting}
                    >
                        Cancel
                    </Button>
                    <Button
                        variant="destructive"
                        onClick={handleDelete}
                        disabled={isDeleting}
                    >
                        {isDeleting ? (
                            <>
                                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                                Deleting...
                            </>
                        ) : (
                            <>
                                <Trash2 className="h-4 w-4 mr-2" />
                                Delete
                            </>
                        )}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}