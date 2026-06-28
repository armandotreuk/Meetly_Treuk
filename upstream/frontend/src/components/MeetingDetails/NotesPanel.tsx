"use client";

import { useState, useEffect, useRef, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Loader2, Save } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';

interface MeetingNote {
    meeting_id: string;
    notes_markdown: string | null;
    notes_json: string | null;
    created_at: string;
    updated_at: string;
}

interface NotesPanelProps {
    meetingId: string;
}

export function NotesPanel({ meetingId }: NotesPanelProps) {
    const [notes, setNotes] = useState('');
    const [isLoading, setIsLoading] = useState(true);
    const [isSaving, setIsSaving] = useState(false);
    const [isDirty, setIsDirty] = useState(false);
    const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const lastSavedRef = useRef<string>('');

    useEffect(() => {
        let cancelled = false;

        const loadNotes = async () => {
            setIsLoading(true);
            try {
                const result = await invoke<MeetingNote | null>('get_meeting_notes', {
                    meetingId,
                });
                if (!cancelled && result) {
                    const markdown = result.notes_markdown || '';
                    setNotes(markdown);
                    lastSavedRef.current = markdown;
                } else if (!cancelled) {
                    setNotes('');
                    lastSavedRef.current = '';
                }
            } catch (error) {
                console.error('Failed to load meeting notes:', error);
                if (!cancelled) {
                    toast.error('Failed to load notes');
                }
            } finally {
                if (!cancelled) setIsLoading(false);
            }
        };

        loadNotes();

        return () => {
            cancelled = true;
            if (saveTimerRef.current) {
                clearTimeout(saveTimerRef.current);
            }
        };
    }, [meetingId]);

    const saveNotes = useCallback(async (markdown: string) => {
        if (markdown === lastSavedRef.current) return;

        setIsSaving(true);
        try {
            await invoke('save_meeting_notes', {
                meetingId,
                notesMarkdown: markdown,
                notesJson: null,
            });
            lastSavedRef.current = markdown;
            setIsDirty(false);
        } catch (error) {
            console.error('Failed to save meeting notes:', error);
            toast.error('Failed to save notes');
        } finally {
            setIsSaving(false);
        }
    }, [meetingId]);

    const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const value = e.target.value;
        setNotes(value);
        setIsDirty(true);

        if (saveTimerRef.current) {
            clearTimeout(saveTimerRef.current);
        }

        saveTimerRef.current = setTimeout(() => {
            saveNotes(value);
        }, 2000);
    };

    const handleManualSave = () => {
        if (saveTimerRef.current) {
            clearTimeout(saveTimerRef.current);
        }
        saveNotes(notes);
    };

    if (isLoading) {
        return (
            <div className="flex items-center justify-center h-full">
                <Loader2 className="h-6 w-6 animate-spin text-gray-400" />
            </div>
        );
    }

    return (
        <div className="flex flex-col h-full bg-white border-l border-gray-200">
            <div className="flex items-center justify-between px-4 py-2 border-b border-gray-200 bg-gray-50">
                <h3 className="text-sm font-semibold text-gray-700">Notes</h3>
                <div className="flex items-center gap-2">
                    {isSaving && (
                        <span className="text-xs text-gray-400 flex items-center gap-1">
                            <Loader2 className="h-3 w-3 animate-spin" />
                            Saving...
                        </span>
                    )}
                    {isDirty && !isSaving && (
                        <span className="text-xs text-amber-500">Unsaved</span>
                    )}
                    {!isDirty && !isSaving && notes.length > 0 && (
                        <span className="text-xs text-green-500">Saved</span>
                    )}
                    <Button
                        variant="ghost"
                        size="sm"
                        onClick={handleManualSave}
                        disabled={!isDirty || isSaving}
                        className="h-7 text-xs"
                    >
                        <Save className="h-3 w-3 mr-1" />
                        Save
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
                        saveNotes(notes);
                    }}
                    placeholder="Add your notes here..."
                    className="w-full h-full min-h-[60vh] p-4 text-sm text-gray-800 resize-none border-0 outline-none font-mono"
                    style={{ fontFamily: 'inherit' }}
                />
            </ScrollArea>
        </div>
    );
}
