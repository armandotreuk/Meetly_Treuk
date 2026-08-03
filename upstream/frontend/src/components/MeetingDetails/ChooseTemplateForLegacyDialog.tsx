"use client";

import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

interface ChooseTemplateForLegacyDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    availableTemplates: Array<{ id: string; name: string; description: string }>;
    onChoose: (templateId: string) => void;
}

// Item 29: the backfilled "legacy" summary row is a read-only archive, so
// "Regenerate" on it reroutes here. Picking a template only sets it active;
// generation still requires a second click on the primary button.
export function ChooseTemplateForLegacyDialog({
    open,
    onOpenChange,
    availableTemplates,
    onChoose,
}: ChooseTemplateForLegacyDialogProps) {
    const templates = availableTemplates.filter((t) => t.id !== "legacy");

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-[450px]">
                <DialogHeader>
                    <DialogTitle>Choose a template</DialogTitle>
                    <DialogDescription>
                        The original summary is kept as a read-only archive. Pick a template
                        for the new summary, then click &ldquo;Generate summary&rdquo; to run
                        it.
                    </DialogDescription>
                </DialogHeader>

                <div className="max-h-72 overflow-y-auto space-y-1 py-1">
                    {templates.length === 0 && (
                        <div className="text-sm text-muted-foreground py-4 text-center">
                            No templates available.
                        </div>
                    )}
                    {templates.map((t) => (
                        <button
                            key={t.id}
                            type="button"
                            title={t.description}
                            onClick={() => onChoose(t.id)}
                            className="flex w-full items-center justify-between rounded-md border border-border px-3 py-2 text-left text-sm transition-colors hover:bg-accent/50"
                        >
                            <span className="font-medium">{t.name}</span>
                        </button>
                    ))}
                </div>

                <DialogFooter>
                    <Button variant="outline" onClick={() => onOpenChange(false)}>
                        Cancel
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
