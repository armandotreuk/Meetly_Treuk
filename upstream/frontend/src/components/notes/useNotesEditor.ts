"use client";

import { useState, useRef, useCallback, useEffect } from "react";
import type { Block } from "@blocknote/core";
import { blocksToMarkdownSafely, type MarkdownCapableEditor } from "@/lib/blocknote-markdown";

export interface UseNotesEditorOptions {
	save: (markdown: string, blocksJson: string | null) => Promise<void>;
	onSaveError?: (error: unknown) => void;
	initialNotes?: string;
	initialBlocks?: Block[] | null;
}

export interface UseNotesEditorReturn {
	notes: string;
	setNotes: React.Dispatch<React.SetStateAction<string>>;
	notesRef: React.MutableRefObject<string>;
	blocksRef: React.MutableRefObject<Block[] | null>;
	markdownEditorRef: React.MutableRefObject<MarkdownCapableEditor | null>;
	setBlocks: (blocks: Block[]) => void;
	resetContent: (notes: string, blocks: Block[] | null) => void;
	isDirty: boolean;
	setIsDirty: React.Dispatch<React.SetStateAction<boolean>>;
	isSaving: boolean;
	lastSavedRef: React.MutableRefObject<string>;
	lastSavedAt: string | null;
	handleChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void;
	handleManualSave: () => void;
	handleBlur: () => void;
	flushPendingSave: () => Promise<void>;
	cancelPendingSave: () => void;
}

export function useNotesEditor(options: UseNotesEditorOptions): UseNotesEditorReturn {
	const [notes, setNotes] = useState(options.initialNotes ?? "");
	const [isSaving, setIsSaving] = useState(false);
	const [isDirty, setIsDirty] = useState(false);
	const [lastSavedAt, setLastSavedAt] = useState<string | null>(null);
	const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
	const lastSavedRef = useRef<string>(options.initialNotes ?? "");
	const notesRef = useRef<string>(options.initialNotes ?? "");
	const blocksRef = useRef<Block[] | null>(options.initialBlocks ?? null);
	const markdownEditorRef = useRef<MarkdownCapableEditor | null>(null);
	const lastSavedBlocksRef = useRef<string | null>(
		options.initialBlocks ? JSON.stringify(options.initialBlocks) : null
	);
	const saveGenRef = useRef(0);
	const isSavingRef = useRef(false);
	const activeSavePromiseRef = useRef<Promise<void> | null>(null);
	const saveErrorRef = useRef<{ gen: number; error: unknown } | null>(null);
	const skipFlushRef = useRef(false);
	const pendingSaveRef = useRef<{
		markdown: string;
		blocks: Block[] | null;
		blocksJson: string | null;
		gen: number;
	} | null>(null);

	const onSaveErrorRef = useRef(options.onSaveError);
	onSaveErrorRef.current = options.onSaveError;

	const wrappedSave = useCallback(
		() => {
			const gen = ++saveGenRef.current;
			const blocks = blocksRef.current;
			const intent = {
				markdown: notesRef.current,
				blocks,
				blocksJson: blocks ? JSON.stringify(blocks) : null,
				gen,
			};
			if (isSavingRef.current) {
				pendingSaveRef.current = intent;
				return activeSavePromiseRef.current ?? Promise.resolve();
			}

			isSavingRef.current = true;
			setIsSaving(true);
			const savePromise = (async () => {
				try {
					let current: typeof intent | null = intent;
					while (current) {
						if (current.gen === saveGenRef.current) {
							const markdownResult = current.blocks
								? await blocksToMarkdownSafely(
										markdownEditorRef.current ?? {
											blocksToMarkdownLossy: async () => current!.markdown,
										},
										current.blocks,
										{
											source: "useNotesEditor",
											fallbackMarkdown: current.markdown,
										}
									)
								: { markdown: current.markdown, ok: true };
							const markdown = markdownResult.markdown ?? "";

							if (
								current.gen === saveGenRef.current &&
								(markdown !== lastSavedRef.current ||
									current.blocksJson !== lastSavedBlocksRef.current)
							) {
								try {
									await options.save(markdown, current.blocksJson);
									if (current.gen === saveGenRef.current) {
										saveErrorRef.current = null;
										lastSavedRef.current = markdown;
										lastSavedBlocksRef.current = current.blocksJson;
										notesRef.current = markdown;
										setNotes(markdown);
										setIsDirty(false);
										setLastSavedAt(
											new Date().toLocaleTimeString([], {
												hour: "2-digit",
												minute: "2-digit",
											})
										);
									}
								} catch (error) {
									if (current.gen === saveGenRef.current) {
										saveErrorRef.current = { gen: current.gen, error };
									}
									if (current.gen === saveGenRef.current && onSaveErrorRef.current) {
										onSaveErrorRef.current(error);
									}
								}
							}
						}

						current = pendingSaveRef.current;
						pendingSaveRef.current = null;
					}
				} finally {
					isSavingRef.current = false;
					setIsSaving(false);
				}
			})();
			activeSavePromiseRef.current = savePromise;
			void savePromise.then(
				() => {
					if (activeSavePromiseRef.current === savePromise) activeSavePromiseRef.current = null;
				},
				() => {
					if (activeSavePromiseRef.current === savePromise) activeSavePromiseRef.current = null;
				}
			);
			return savePromise;
		},
		[options.save]
	);

	const flushPendingSave = useCallback(async () => {
		let savePromise = activeSavePromiseRef.current ?? Promise.resolve();
		let retried = false;
		if (saveTimerRef.current) {
			clearTimeout(saveTimerRef.current);
			saveTimerRef.current = null;
			if (!skipFlushRef.current) savePromise = wrappedSave();
		} else if (saveErrorRef.current?.gen === saveGenRef.current && !skipFlushRef.current) {
			savePromise = wrappedSave();
			retried = true;
		}
		await savePromise;
		if (saveErrorRef.current?.gen === saveGenRef.current && !skipFlushRef.current && !retried) {
			await wrappedSave();
		}
		if (saveErrorRef.current?.gen === saveGenRef.current) throw saveErrorRef.current.error;
	}, [wrappedSave]);

	// ponytail: cancelPendingSave drops the pending debounce WITHOUT saving —
	// the one place that intentionally discards unsaved content is notes
	// deletion, where saving the deleted text back would be a bug.
	const cancelPendingSave = useCallback(() => {
		if (saveTimerRef.current) {
			clearTimeout(saveTimerRef.current);
			saveTimerRef.current = null;
		}
		pendingSaveRef.current = null;
		skipFlushRef.current = true;
		++saveGenRef.current;
	}, []);

	useEffect(() => {
		if (options.initialNotes !== undefined) {
			setNotes(options.initialNotes);
			notesRef.current = options.initialNotes;
			lastSavedRef.current = options.initialNotes;
			blocksRef.current = options.initialBlocks ?? null;
			lastSavedBlocksRef.current = options.initialBlocks ? JSON.stringify(options.initialBlocks) : null;
		}
		return () => {
			void flushPendingSave().catch(() => {});
		};
	}, [options.initialNotes, options.initialBlocks, flushPendingSave]);

	// ponytail: beforeunload can't await async Tauri IPC in a webview — the flush may
	// not complete before teardown. Best-effort only; the real protection is the
	// unmount/re-key flush above plus the backend notes.md import.
	useEffect(() => {
		const flush = () => void flushPendingSave().catch(() => {});
		window.addEventListener("beforeunload", flush);
		return () => window.removeEventListener("beforeunload", flush);
	}, [flushPendingSave]);

	const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
		const value = e.target.value;
		setNotes(value);
		setIsDirty(true);
		skipFlushRef.current = false;
		notesRef.current = value;
		if (isSavingRef.current) ++saveGenRef.current;

		if (saveTimerRef.current) {
			clearTimeout(saveTimerRef.current);
		}
		saveTimerRef.current = setTimeout(() => {
			saveTimerRef.current = null;
			void wrappedSave();
		}, 2000);
	};

	const setBlocks = useCallback(
		(blocks: Block[]) => {
			blocksRef.current = blocks;
			setIsDirty(true);
			skipFlushRef.current = false;
			if (isSavingRef.current) ++saveGenRef.current;
			if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
			saveTimerRef.current = setTimeout(() => {
				saveTimerRef.current = null;
				void wrappedSave();
			}, 2000);
		},
		[wrappedSave]
	);

	const resetContent = useCallback((markdown: string, blocks: Block[] | null) => {
		setNotes(markdown);
		notesRef.current = markdown;
		lastSavedRef.current = markdown;
		blocksRef.current = blocks;
		lastSavedBlocksRef.current = blocks ? JSON.stringify(blocks) : null;
		setIsDirty(false);
		skipFlushRef.current = false;
	}, []);

	const handleManualSave = () => {
		if (saveTimerRef.current) {
			clearTimeout(saveTimerRef.current);
			saveTimerRef.current = null;
		}
		void wrappedSave();
	};

	const handleBlur = () => {
		if (saveTimerRef.current) {
			clearTimeout(saveTimerRef.current);
			saveTimerRef.current = null;
		}
		void wrappedSave();
	};

	return {
		notes,
		setNotes,
		notesRef,
		blocksRef,
		markdownEditorRef,
		setBlocks,
		resetContent,
		isDirty,
		setIsDirty,
		isSaving,
		lastSavedRef,
		lastSavedAt,
		handleChange,
		handleManualSave,
		handleBlur,
		flushPendingSave,
		cancelPendingSave,
	};
}
