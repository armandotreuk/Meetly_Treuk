"use client";

import { useState, useEffect, useRef, useCallback } from "react";
import { logger } from "@/lib/logger";

import { invoke } from "@tauri-apps/api/core";
import { Loader2, Save } from "lucide-react";
import { StickyNote, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";

const DRAFT_KEY = "recording_notes_draft";

interface RecordingNotesPanelProps {
    onClose: () => void;
    width?: number;
}

/**
 * Notes panel shown automatically on the recording screen. Notes are mirrored to
 * `notes.md` in the current recording folder in real time (debounced 2s) so they
 * survive an app crash mid-meeting, and also kept in sessionStorage as the bridge
 * to persist them to the meetings_notes DB table on stop (see useRecordingStop).
 */
export function RecordingNotesPanel({ onClose, width }: RecordingNotesPanelProps) {
    const [notes, setNotes] = useState("");
    const [isSaving, setIsSaving] = useState(false);
    const [isDirty, setIsDirty] = useState(false);
    const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const lastSavedRef = useRef<string>("");

    // ponytail: threshold 320px hardcoded; upgrade path if reused = extract useCompactPanelHeader.
    const isCompact = width !== undefined && width < 320;

    useEffect(() => {
        const draft = sessionStorage.getItem(DRAFT_KEY) ?? "";
        setNotes(draft);
        lastSavedRef.current = draft;

        return () => {
            if (saveTimerRef.current) {
                clearTimeout(saveTimerRef.current);
            }
        };
    }, []);

    const saveDraftToDisk = useCallback(async (markdown: string) => {
        if (markdown === lastSavedRef.current) return;
        setIsSaving(true);
        try {
            await invoke("save_recording_notes", { notes: markdown });
            lastSavedRef.current = markdown;
            setIsDirty(false);
        } catch (error) {
            logger.error("Failed to save recording notes to folder:", error);
        } finally {
            setIsSaving(false);
        }
    }, []);

    const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const value = e.target.value;
        setNotes(value);
        setIsDirty(true);
        sessionStorage.setItem(DRAFT_KEY, value);

        if (saveTimerRef.current) {
            clearTimeout(saveTimerRef.current);
        }
        saveTimerRef.current = setTimeout(() => {
            void saveDraftToDisk(value);
        }, 2000);
    };

    const handleManualSave = () => {
        if (saveTimerRef.current) {
            clearTimeout(saveTimerRef.current);
        }
        void saveDraftToDisk(notes);
    };

    return (
        <div
            className="flex flex-col h-full bg-white border-l border-gray-200 shrink-0"
            style={width !== undefined ? { width } : { width: 320 }}
        >
            <div className="flex items-center justify-between px-4 py-2 border-b border-gray-200 bg-gray-50">
                <div className="flex items-center gap-2">
                    <StickyNote className="h-4 w-4 text-blue-600" />
                    <h3 className="text-sm font-semibold text-gray-700">Notes</h3>
                </div>
                <div className="flex items-center gap-2">
                    {isSaving && (
                        <span className="text-xs text-gray-400 flex items-center gap-1">
                            <Loader2 className="h-3 w-3 animate-spin" />
                            {!isCompact && "Saving..."}
                        </span>
                    )}
                    {isDirty && !isSaving && (
                        <span className="text-xs text-amber-500">
                            {!isCompact && "Unsaved"}
                        </span>
                    )}
                    {!isDirty && !isSaving && notes.length > 0 && (
                        <span className="text-xs text-green-500">
                            {!isCompact && "Saved"}
                        </span>
                    )}
                    <Button
                        variant="ghost"
                        size="sm"
                        onClick={handleManualSave}
                        disabled={!isDirty || isSaving}
                        className={isCompact ? "h-7 w-7 p-0" : "h-7 text-xs"}
                        title="Save"
                    >
                        <Save className="h-3 w-3" />
                        {!isCompact && <>Save</>}
                    </Button>
                    <Button
                        variant="ghost"
                        size="sm"
                        onClick={onClose}
                        className="h-7 w-7 p-0 text-gray-400 hover:text-gray-600"
                        title="Hide notes"
                    >
                        <X className="h-4 w-4" />
                    </Button>
                </div>
            </div>
            <ScrollArea className="flex-1">
                <textarea
                    value={notes}
                    onChange={handleChange}
                    onBlur={() => {
                        if (saveTimerRef.current) {
                            clearTimeout(saveTimerRef.current);
                        }
                        void saveDraftToDisk(notes);
                    }}
                    placeholder="Add your notes here..."
                    className="w-full h-full min-h-[60vh] p-4 text-sm text-gray-800 resize-none border-0 outline-none"
                    style={{ fontFamily: "inherit" }}
                />
            </ScrollArea>
        </div>
    );
}