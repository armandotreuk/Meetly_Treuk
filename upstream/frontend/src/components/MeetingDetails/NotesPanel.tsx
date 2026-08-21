"use client";

import { useState, useEffect, useCallback } from "react";
import type { Block } from "@blocknote/core";
import { useCreateBlockNote } from "@blocknote/react";
import { logger } from "@/lib/logger";

import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { useNotesEditor } from "@/components/notes/useNotesEditor";
import { NotesEditorShell } from "@/components/notes/NotesEditorShell";
import { useSidebar } from "@/components/Sidebar/SidebarProvider";
import { t } from "@/lib/i18n";

interface MeetingNote {
	meeting_id: string;
	notes_markdown: string | null;
	notes_json: string | null;
	created_at: string;
	updated_at: string;
}

interface NotesPanelProps {
	meetingId: string;
	width?: number;
	onClose?: () => void;
}

export function NotesPanel({ meetingId, width, onClose }: NotesPanelProps) {
	const [isLoading, setIsLoading] = useState(true);
	const [loadError, setLoadError] = useState<string | null>(null);
	const [reloadKey, setReloadKey] = useState(0);
	const [isDeleting, setIsDeleting] = useState(false);
	const { refetchMeetings } = useSidebar();

	const saveNotes = useCallback(
		async (markdown: string, blocksJson: string | null) => {
			await invoke("save_meeting_notes", {
				meetingId,
				notesMarkdown: markdown,
				notesJson: blocksJson,
			});
			// ponytail: refetch after each save so the sidebar's has-notes dot stays
			// live; the list is a local SQLite read (~30 rows), cheap at the 2s
			// debounce cadence.
			await refetchMeetings();
		},
		[meetingId, refetchMeetings]
	);

	const parser = useCreateBlockNote({ initialContent: undefined });
	const [content, setContent] = useState<{ markdown: string; blocks: Block[] | null }>({
		markdown: "",
		blocks: null,
	});
	const editor = useNotesEditor({
		save: saveNotes,
		initialNotes: content.markdown,
		initialBlocks: content.blocks,
		onSaveError: (error) => {
			logger.error("Failed to save meeting notes:", error);
			toast.error(t("notes.toast.saveFailed"));
		},
	});

	const { notesRef, lastSavedRef, flushPendingSave } = editor;

	useEffect(() => {
		let cancelled = false;

		const loadNotes = async () => {
			setIsLoading(true);
			setLoadError(null);
			lastSavedRef.current = "";
			try {
				const result = await invoke<MeetingNote | null>("get_meeting_notes", {
					meetingId,
				});
				if (!cancelled && result) {
					const markdown = result.notes_markdown || "";
					let blocks: Block[] | null = null;
					if (result.notes_json) {
						try {
							const parsed = JSON.parse(result.notes_json);
							blocks = Array.isArray(parsed) ? parsed : null;
						} catch {
							blocks = null;
						}
					}
					// ponytail: notes_json is the rich-content source of truth; markdown only hydrates legacy rows.
					if (!blocks && markdown) {
						try {
							blocks = await parser.tryParseMarkdownToBlocks(markdown);
						} catch (error) {
							logger.error("Failed to parse meeting notes markdown:", error);
						}
					}
					if (!cancelled) setContent({ markdown, blocks });
				} else if (!cancelled) {
					setContent({ markdown: "", blocks: null });
				}
			} catch (error) {
				logger.error("Failed to load meeting notes:", error);
				if (!cancelled) {
					setLoadError(error instanceof Error ? error.message : String(error));
					toast.error(t("notes.toast.loadFailed"));
				}
			} finally {
				if (!cancelled) setIsLoading(false);
			}
		};

		loadNotes();

		return () => {
			cancelled = true;
			void flushPendingSave().catch(() => {});
		};
	}, [meetingId, reloadKey, notesRef, lastSavedRef, flushPendingSave, parser]);

	const handleRetryLoad = () => {
		setReloadKey((key) => key + 1);
	};

	const handleDeleteNotes = async () => {
		if (isDeleting || !confirm(t("notes.delete.confirm"))) return;
		// ponytail: cancel before the IPC invalidates an active save and drops its
		// latest-only queue so deleted content cannot be re-saved afterward.
		editor.cancelPendingSave();
		setIsDeleting(true);
		try {
			await editor.flushPendingSave();
			await invoke("delete_meeting_notes", { meetingId });
			editor.resetContent("", null);
			setContent({ markdown: "", blocks: null });
			await refetchMeetings();
			toast.success(t("notes.toast.deleted"));
		} catch (error) {
			logger.error("Failed to delete meeting notes:", error);
			toast.error(t("notes.toast.deleteFailed"));
		} finally {
			setIsDeleting(false);
		}
	};

	return (
		<NotesEditorShell
			notes={editor.notes}
			initialBlocks={content.blocks}
			onBlocksChange={editor.setBlocks}
			markdownEditorRef={editor.markdownEditorRef}
			onBlur={editor.handleBlur}
			onManualSave={editor.handleManualSave}
			isSaving={editor.isSaving}
			isDeleting={isDeleting}
			isDirty={editor.isDirty}
			lastSavedAt={editor.lastSavedAt}
			width={width}
			onClose={onClose}
			onDeleteNotes={handleDeleteNotes}
			isLoading={isLoading}
			errorState={
				loadError ? (
					<div className="text-center px-6">
						<p className="text-sm text-red-600">{t("notes.header.loadError")}</p>
						<p className="text-xs text-gray-500 mt-1">{loadError}</p>
						<Button
							variant="outline"
							size="sm"
							className="mt-4"
							onClick={handleRetryLoad}
							autoFocus
						>
							{t("notes.retry")}
						</Button>
					</div>
				) : undefined
			}
		/>
	);
}
