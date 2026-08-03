"use client";

import React, { useMemo, useState } from "react";
import { Dialog, DialogContent, DialogFooter, DialogTitle } from "@/components/ui/dialog";
import { VisuallyHidden } from "@/components/ui/visually-hidden";
import { Folder, FolderOpen, Inbox } from "lucide-react";
import type { MeetingFolder } from "@/types";

interface MoveToFolderModalProps {
    isOpen: boolean;
    // null = "Sem pasta" / root. Omit `excludeId` from options (e.g. the folder being moved).
    excludeId?: string | null;
    folders: MeetingFolder[];
    title: string;
    onCancel: () => void;
    onSelect: (folderId: string | null) => void;
}

export function MoveToFolderModal({
    isOpen,
    excludeId,
    folders,
    title,
    onCancel,
    onSelect,
}: MoveToFolderModalProps) {
    const { roots, descendants } = useMemo(() => {
        // Index folders by parent.
        const byParent = new Map<string | null, MeetingFolder[]>();
        const visible = folders.filter((f) => f.id !== excludeId);
        for (const f of visible) {
            const arr = byParent.get(f.parent_id) ?? [];
            arr.push(f);
            byParent.set(f.parent_id, arr);
        }
        // If excludeId is set, also drop its descendants (backend rejects cycles; UI hides them).
        const drop = new Set<string>();
        if (excludeId) {
            let frontier: string[] = [excludeId];
            while (frontier.length) {
                const next: string[] = [];
                for (const id of frontier) {
                    for (const f of byParent.get(id) ?? []) {
                        if (!drop.has(f.id)) {
                            drop.add(f.id);
                            next.push(f.id);
                        }
                    }
                }
                frontier = next;
            }
        }
        const keep = (folders: MeetingFolder[]) => folders.filter((f) => !drop.has(f.id));
        const roots = keep(byParent.get(null) ?? []);
        return { roots, descendants: drop };
    }, [folders, excludeId]);

    const [selected, setSelected] = useState<string | null>(null);
    React.useEffect(() => {
        if (isOpen) setSelected(null);
    }, [isOpen]);

    const renderFolder = (folder: MeetingFolder, depth: number): React.ReactNode => {
        const isSel = selected === folder.id;
        return (
            <div key={folder.id}>
                <button
                    onClick={() => setSelected(folder.id)}
                    className={`w-full flex items-center gap-2 px-3 py-2 rounded-md text-sm text-left transition-colors ${
                        isSel ? "bg-blue-100 text-blue-700" : "hover:bg-gray-100"
                    }`}
                    style={{ paddingLeft: `${depth * 16 + 12}px` }}
                >
                    {isSel ? (
                        <FolderOpen className="w-4 h-4 flex-shrink-0" />
                    ) : (
                        <Folder className="w-4 h-4 flex-shrink-0" />
                    )}
                    <span className="flex-1 truncate">{folder.name}</span>
                </button>
                {folders
                    .filter((f) => f.parent_id === folder.id && !descendants.has(f.id))
                    .map((f) => renderFolder(f, depth + 1))}
            </div>
        );
    };

    return (
        <Dialog open={isOpen} onOpenChange={(open) => !open && onCancel()}>
            <DialogContent className="sm:max-w-[420px] max-h-[80vh] flex flex-col">
                <VisuallyHidden>
                    <DialogTitle>{title}</DialogTitle>
                </VisuallyHidden>
                <h3 className="text-lg font-semibold mb-2">{title}</h3>
                <div className="flex-1 overflow-y-auto custom-scrollbar min-h-0 -mx-2">
                    <button
                        onClick={() => setSelected(null)}
                        className={`w-full flex items-center gap-2 px-3 py-2 mx-2 rounded-md text-sm text-left transition-colors ${
                            selected === null ? "bg-blue-100 text-blue-700" : "hover:bg-gray-100"
                        }`}
                    >
                        <Inbox className="w-4 h-4 flex-shrink-0" />
                        <span className="flex-1">Sem pasta</span>
                    </button>
                    {roots.map((f) => renderFolder(f, 1))}
                    {roots.length === 0 && (
                        <p className="text-xs text-gray-400 italic px-4 py-2">
                            Nenhuma pasta disponível.
                        </p>
                    )}
                </div>
                <DialogFooter>
                    <button
                        onClick={onCancel}
                        className="px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 rounded-md transition-colors"
                    >
                        Cancelar
                    </button>
                    <button
                        onClick={() => onSelect(selected)}
                        className="px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors"
                    >
                        Mover
                    </button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}