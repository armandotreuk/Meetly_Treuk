"use client";

import React, { useEffect } from "react";
import { Loader2, Save, X, StickyNote, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import dynamic from "next/dynamic";
import type { Block } from "@blocknote/core";
import type { MarkdownCapableEditor } from "@/lib/blocknote-markdown";
import { t } from "@/lib/i18n";

const Editor = dynamic(() => import("../BlockNoteEditor/Editor"), { ssr: false });

export interface NotesEditorShellProps {
	notes: string;
	initialBlocks: Block[] | null;
	onBlocksChange: (blocks: Block[]) => void;
	markdownEditorRef: React.MutableRefObject<MarkdownCapableEditor | null>;
	onBlur: () => void;
	onManualSave: () => void;
	isSaving: boolean;
	isDeleting?: boolean;
	isDirty: boolean;
	lastSavedAt?: string | null;
	width?: number;
	onClose?: () => void;
	showHeaderIcon?: boolean;
	errorState?: React.ReactNode;
	isLoading?: boolean;
	onDeleteNotes?: () => void;
}

export function NotesEditorShell({
	notes,
	initialBlocks,
	onBlocksChange,
	markdownEditorRef,
	onBlur,
	onManualSave,
	isSaving,
	isDeleting,
	isDirty,
	lastSavedAt,
	width,
	onClose,
	showHeaderIcon,
	errorState,
	isLoading,
	onDeleteNotes,
}: NotesEditorShellProps) {
	const effectiveWidth = width ?? 320;
	const isCompact = width !== undefined && width < 320;

	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
				event.preventDefault();
				if (isDirty && !isSaving) onManualSave();
			}
		};

		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [isDirty, isSaving, onManualSave]);

	if (isLoading) {
		return (
			<div className="flex items-center justify-center h-full">
				<Loader2 className="h-6 w-6 animate-spin text-gray-400" />
			</div>
		);
	}

	return (
		<div
			className="flex flex-col h-full bg-white border-l border-gray-200 shrink-0"
			style={{ width: effectiveWidth }}
		>
			<div className="flex items-center justify-between px-4 py-2 border-b border-gray-200 bg-gray-50">
				<div className="flex items-center gap-2">
					{showHeaderIcon && <StickyNote className="h-4 w-4 text-blue-600" />}
					<h3 className="text-sm font-semibold text-gray-700">{t("notes.header.title")}</h3>
				</div>
				<div className="flex items-center gap-2">
					{errorState ? (
						<span className="text-xs text-red-500">{t("notes.header.loadError")}</span>
					) : (
						<>
							<span aria-live="polite" className="flex items-center gap-1">
								{isSaving && (
									<span className="text-xs text-gray-400 flex items-center gap-1">
										<Loader2 className="h-3 w-3 animate-spin" aria-hidden="true" />
										{!isCompact && t("notes.status.saving")}
									</span>
								)}
								{isDirty && !isSaving && (
									<span className="text-xs text-amber-500">
										{!isCompact && t("notes.status.unsaved")}
									</span>
								)}
								{!isDirty && !isSaving && notes.length > 0 && (
									<span className="text-xs text-green-500">
										{!isCompact && (lastSavedAt ? t("notes.status.savedAt", { time: lastSavedAt }) : t("notes.status.saved"))}
									</span>
								)}
							</span>
							<Button
								variant="ghost"
								size="sm"
								onClick={onManualSave}
								disabled={!isDirty || isSaving}
								className={isCompact ? "h-7 w-7 p-0" : "h-7 text-xs"}
								title={t("notes.action.save")}
								aria-label={t("notes.action.saveAria")}
							>
								<Save className="h-3 w-3" />
								{!isCompact && <>{t("notes.action.save")}</>}
							</Button>
							{onDeleteNotes && (
								<Button
									variant="ghost"
									size="sm"
									onClick={onDeleteNotes}
									disabled={isSaving || isDeleting || !notes}
									className="h-7 w-7 p-0 text-gray-400 hover:text-red-600 hover:bg-red-50"
									title={t("notes.action.delete")}
									aria-label={t("notes.action.delete")}
								>
									<Trash2 className="h-3 w-3" />
								</Button>
							)}
						</>
					)}
					{onClose && (
						<Button
							variant="ghost"
							size="sm"
							onClick={onClose}
							className="h-7 w-7 p-0 text-gray-400 hover:text-gray-600"
							title={t("notes.action.hide")}
							aria-label={t("notes.action.hide")}
						>
							<X className="h-4 w-4" />
						</Button>
					)}
				</div>
			</div>
			{errorState ? (
				<div className="flex flex-1 items-center justify-center">{errorState}</div>
			) : (
				<ScrollArea className="flex-1">
					<div
						className="min-h-[60vh] p-4 text-sm text-gray-800"
						onBlur={(e) => {
							if (!e.currentTarget.contains(e.relatedTarget as Node)) onBlur();
						}}
					>
						<Editor
							key={initialBlocks ? JSON.stringify(initialBlocks) : "empty"}
							initialContent={initialBlocks ?? undefined}
							onChange={onBlocksChange}
							onReady={(blockEditor) => {
								markdownEditorRef.current = blockEditor;
							}}
						/>
					</div>
				</ScrollArea>
			)}
		</div>
	);
}
