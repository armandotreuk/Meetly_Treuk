"use client";

import React, { useEffect, useCallback, useRef, useState } from "react";
import type { Block } from "@blocknote/core";
import { useCreateBlockNote } from "@blocknote/react";
import { logger } from "@/lib/logger";
import { useTranscripts } from "@/contexts/TranscriptContext";
import { useRecordingState } from "@/contexts/RecordingStateContext";

import { invoke } from "@tauri-apps/api/core";
import { useNotesEditor } from "@/components/notes/useNotesEditor";
import { NotesEditorShell } from "@/components/notes/NotesEditorShell";
import { registerRecordingNotesFlush } from "@/lib/recording-notes-flush";

// Legacy pre-scoping key, kept only to sweep up leftovers from older versions.
const LEGACY_DRAFT_KEY = "recording_notes_draft";

export interface RecordingNotesDraft {
	markdown: string;
	blocksJson: string | null;
}

export function persistDraft(key: string, payload: RecordingNotesDraft): boolean {
	try {
		sessionStorage.setItem(key, JSON.stringify(payload));
		return true;
	} catch {
		try {
			sessionStorage.setItem(
				key,
				JSON.stringify({ markdown: payload.markdown, blocksJson: null })
			);
			return true;
		} catch {
			return false;
		}
	}
}

interface RecordingNotesPanelProps {
	onClose: () => void;
	width?: number;
}

/**
 * Notes panel shown automatically on the recording screen. Notes are mirrored to
 * `notes.md` in the current recording folder in real time (debounced 2s) so they
 * survive an app crash mid-meeting. When the meeting is saved (stop or recovery),
	 * the Rust save path imports those files into the meeting_notes DB table
 * (see TranscriptsRepository::save_transcript).
 */
export function RecordingNotesPanel({ onClose, width }: RecordingNotesPanelProps) {
	// ponytail: meetingTitle is unique per recording (timestamped at start), so
	// keying the draft by it isolates sessions. Re-read when it changes (reload
	// sync) so the draft follows a late title sync instead of splitting. Gated on
	// hydration: until the reload-sync sets the real title, meetingTitle is the
	// shared default "+ New Call", which must never key (read or write) a draft
	// that could belong to another recording.
	const { meetingTitle } = useTranscripts();
	const { liveTranscriptScopeKey } = useRecordingState();
	const recordingScopeKeyRef = useRef(liveTranscriptScopeKey);
	recordingScopeKeyRef.current = liveTranscriptScopeKey;
	const draftKey =
		meetingTitle && meetingTitle !== "+ New Call"
			? `recording_notes_draft:${meetingTitle}`
			: null;
	const draftKeyRef = useRef(draftKey);
	draftKeyRef.current = draftKey;

	const saveDraftToDisk = useCallback(async (markdown: string, blocksJson: string | null) => {
		const key = draftKeyRef.current;
		if (key) persistDraft(key, { markdown, blocksJson });
		const recordingScopeKey = recordingScopeKeyRef.current;
		if (!recordingScopeKey) throw new Error("Recording notes scope is not ready");
		await invoke("save_recording_notes", { notes: markdown, blocksJson, recordingScopeKey });
	}, []);
	const parser = useCreateBlockNote({ initialContent: undefined });
	const [content, setContent] = useState<{ markdown: string; blocks: Block[] | null }>({
		markdown: "",
		blocks: null,
	});

	const editor = useNotesEditor({
		save: saveDraftToDisk,
		initialNotes: content.markdown,
		initialBlocks: content.blocks,
		onSaveError: (error) => {
			logger.error("Failed to save recording notes to folder:", error);
		},
	});

	const { flushPendingSave } = editor;
	const draftGenerationRef = useRef(0);

	useEffect(
		() => registerRecordingNotesFlush(liveTranscriptScopeKey, flushPendingSave),
		[liveTranscriptScopeKey, flushPendingSave]
	);

	useEffect(() => {
		// One-time sweep of the legacy global key (idempotent).
		sessionStorage.removeItem(LEGACY_DRAFT_KEY);

		// Only hydrate from the draft under the real title. While meetingTitle is
		// still the shared default the key is null, so never read (or write, see
		// handleChange) a draft that could belong to another recording; and when no
		// draft exists for this meeting, keep the current notes (a title sync must
		// not wipe text typed during the hydration window).
		let cancelled = false;
		if (draftKey) {
			const draft = sessionStorage.getItem(draftKey);
			if (draft !== null) {
				const stored = (() => {
					try {
						const parsed = JSON.parse(draft) as {
							markdown?: string;
							blocksJson?: string | null;
							blocks?: Block[];
						};
						const blocks = parsed.blocksJson
							? JSON.parse(parsed.blocksJson)
							: parsed.blocks;
						return typeof parsed.markdown === "string" || Array.isArray(blocks)
							? { markdown: parsed.markdown ?? "", blocks: Array.isArray(blocks) ? blocks : null }
							: null;
					} catch {
						return null;
					}
				})();
				if (stored) {
					if (stored.blocks) {
						setContent(stored);
					} else if (stored.markdown) {
						void parser
							.tryParseMarkdownToBlocks(stored.markdown)
							.then((blocks) => {
								if (!cancelled) setContent({ markdown: stored.markdown, blocks });
							})
							.catch((error) => logger.error("Failed to parse recording notes markdown:", error));
					}
				} else {
					void parser
						.tryParseMarkdownToBlocks(draft)
						.then((blocks) => {
							if (!cancelled) setContent({ markdown: draft, blocks });
						})
						.catch((error) => logger.error("Failed to parse recording notes markdown:", error));
				}
			}
		}

		return () => {
			cancelled = true;
			draftGenerationRef.current += 1;
			void flushPendingSave().catch(() => {});
		};
	}, [draftKey, flushPendingSave, parser]);

	const handleBlocksChange = (blocks: Block[]) => {
		editor.setBlocks(blocks);
		if (!draftKey) return;

		const generation = ++draftGenerationRef.current;
		const blocksJson = JSON.stringify(blocks);
		const markdown = editor.notesRef.current;
		if (!persistDraft(draftKey, { markdown, blocksJson })) {
			logger.error("Failed to persist recording notes session draft");
		}

		void editor.markdownEditorRef.current
			?.blocksToMarkdownLossy(blocks)
			.then((freshMarkdown) => {
				if (
					generation === draftGenerationRef.current &&
					!persistDraft(draftKey, { markdown: freshMarkdown, blocksJson })
				) {
					logger.error("Failed to persist recording notes session draft");
				}
			})
			.catch((error) => logger.error("Failed to convert recording notes draft to markdown:", error));
	};

	return (
		<NotesEditorShell
			notes={editor.notes}
			initialBlocks={content.blocks}
			onBlocksChange={handleBlocksChange}
			markdownEditorRef={editor.markdownEditorRef}
			onBlur={editor.handleBlur}
			onManualSave={editor.handleManualSave}
			isSaving={editor.isSaving}
			isDirty={editor.isDirty}
			lastSavedAt={editor.lastSavedAt}
			width={width}
			onClose={onClose}
			showHeaderIcon
		/>
	);
}
