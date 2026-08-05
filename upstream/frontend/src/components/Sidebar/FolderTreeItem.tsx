"use client";

import React, { useRef, useEffect } from "react";
import {
    ChevronDown,
    ChevronRight,
    Folder,
    FolderOpen,
    MoreVertical,
    FolderInput,
    Pencil,
    Trash2,
    FolderPlus,
} from "lucide-react";
import type { FolderNode, MeetingNode } from "@/hooks/useSidebarTree";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
    DragPayload,
    getDragState,
    setDragState,
    setDragDropTarget,
    DRAG_THRESHOLD_PX,
    makeGhost,
    moveGhost,
    removeGhost,
    findDropTargetAt,
} from "./MeetingTreeItem";

interface FolderTreeItemProps {
    folder: FolderNode;
    depth: number;
    expanded: Set<string>;
    onToggle: (id: string) => void;
    currentMeetingId?: string;
    onEditMeeting: (meetingId: string, currentTitle: string) => void;
    onRequestDeleteMeeting: (meetingId: string) => void;
    onMoveMeeting: (meetingId: string, folderId: string | null) => void;
    onMoveFolder: (folderId: string, newParentId: string | null) => void;
    onCreateSubfolder: (parentId: string) => void;
    onRenameFolder: (folderId: string, currentName: string) => void;
    onRequestDeleteFolder: (folderId: string) => void;
    onRequestMoveFolder: (folderId: string) => void;
    renderMeeting: (node: MeetingNode, depth: number) => React.ReactNode;
}

export function FolderTreeItem(props: FolderTreeItemProps) {
    const {
        folder,
        depth,
        expanded,
        onToggle,
        currentMeetingId,
        onEditMeeting,
        onRequestDeleteMeeting,
        onMoveMeeting,
        onMoveFolder,
        onCreateSubfolder,
        onRenameFolder,
        onRequestDeleteFolder,
        onRequestMoveFolder,
        renderMeeting,
    } = props;

    const isExpanded = expanded.has(folder.id);
    const paddingLeft = `${depth * 12 + 12}px`;
    const [isDropTarget, setIsDropTarget] = React.useState(false);
    const headerRef = useRef<HTMLDivElement>(null);
    const dragStateRef = useRef<{
        startX: number;
        startY: number;
        dragging: boolean;
    } | null>(null);

    // Drop target: listen for the custom events dispatched by the source's drag loop.
    useEffect(() => {
        const el = headerRef.current;
        if (!el) return;
        const onEnter = () => setIsDropTarget(true);
        const onLeave = () => setIsDropTarget(false);
        const onDrop = (e: Event) => {
            setIsDropTarget(false);
            const detail = (e as CustomEvent<{ payload: DragPayload }>).detail;
            const payload = detail?.payload;
            if (!payload) return;
            if (payload.kind === "meeting") {
                onMoveMeeting(payload.id, folder.id);
            } else if (payload.kind === "folder") {
                if (payload.id === folder.id) return;
                onMoveFolder(payload.id, folder.id);
            }
        };
        el.addEventListener("meetily-dragenter", onEnter);
        el.addEventListener("meetily-dragleave", onLeave);
        el.addEventListener("meetily-drop", onDrop as EventListener);
        return () => {
            el.removeEventListener("meetily-dragenter", onEnter);
            el.removeEventListener("meetily-dragleave", onLeave);
            el.removeEventListener("meetily-drop", onDrop as EventListener);
        };
    }, [folder.id, onMoveMeeting, onMoveFolder]);

    // Drag source: track mouse and emit the same custom events.
    useEffect(() => {
        const root = headerRef.current;
        if (!root) return;

        const onMouseMove = (e: MouseEvent) => {
            const ds = dragStateRef.current;
            if (!ds) return;
            const dx = e.clientX - ds.startX;
            const dy = e.clientY - ds.startY;
            if (!ds.dragging) {
                if (Math.hypot(dx, dy) < DRAG_THRESHOLD_PX) return;
                ds.dragging = true;
                document.body.style.userSelect = "none";
                document.body.style.webkitUserSelect = "none";
                const ghost = makeGhost(root, folder.name);
                setDragState({
                    payload: { kind: "folder", id: folder.id },
                    sourceEl: root,
                    ghostEl: ghost,
                    dropTargetEl: null,
                });
            }
            const s = getDragState();
            if (s?.ghostEl) moveGhost(s.ghostEl, e.clientX, e.clientY);
            const target = findDropTargetAt(e.clientX, e.clientY);
            const prev = s?.dropTargetEl ?? null;
            if (target !== prev) {
                if (prev) {
                    prev.classList.remove("drag-over-highlight");
                    prev.dispatchEvent(new CustomEvent("meetily-dragleave"));
                }
                if (target) {
                    target.classList.add("drag-over-highlight");
                    target.dispatchEvent(new CustomEvent("meetily-dragenter"));
                }
                setDragDropTarget(target);
            }
        };

        const onMouseUp = (e: MouseEvent) => {
            const s = getDragState();
            if (s) {
                if (s.dropTargetEl) {
                    s.dropTargetEl.classList.remove("drag-over-highlight");
                    s.dropTargetEl.dispatchEvent(
                        new CustomEvent("meetily-drop", {
                            detail: { payload: s.payload, clientX: e.clientX, clientY: e.clientY },
                        })
                    );
                }
                removeGhost(s.ghostEl);
                setDragState(null);
                document.body.style.userSelect = "";
                document.body.style.webkitUserSelect = "";
            }
            // Suppress the synthetic click that follows a completed drag.
            if (dragStateRef.current?.dragging) {
                const ref = dragStateRef.current;
                setTimeout(() => {
                    if (dragStateRef.current === ref) dragStateRef.current = null;
                }, 0);
            } else {
                dragStateRef.current = null;
            }
            document.removeEventListener("mousemove", onMouseMove);
            document.removeEventListener("mouseup", onMouseUp);
        };

        const onClickCapture = (e: MouseEvent) => {
            const s = dragStateRef.current;
            if (s?.dragging) {
                e.stopPropagation();
                e.preventDefault();
            }
        };

        const onMouseDown = (e: MouseEvent) => {
            if (e.button !== 0) return;
            // Text selection is blocked via `select-none` on the row container.
            dragStateRef.current = { startX: e.clientX, startY: e.clientY, dragging: false };
            document.addEventListener("mousemove", onMouseMove);
            document.addEventListener("mouseup", onMouseUp);
        };

        root.addEventListener("mousedown", onMouseDown);
        root.addEventListener("click", onClickCapture, true);
        return () => {
            root.removeEventListener("mousedown", onMouseDown);
            root.removeEventListener("click", onClickCapture, true);
            document.removeEventListener("mousemove", onMouseMove);
            document.removeEventListener("mouseup", onMouseUp);
        };
    }, [folder.id, folder.name]);

    const folderMeetings: MeetingNode[] = folder.children.filter(
        (c): c is MeetingNode => c.kind === "meeting"
    );
    const childFolders: FolderNode[] = folder.children.filter(
        (c): c is FolderNode => c.kind === "folder"
    );

    return (
        <div>
            <div
                ref={headerRef}
                data-drop-target="folder"
                data-folder-id={folder.id}
                className={`flex items-center px-3 py-2 my-0.5 rounded-md text-sm cursor-pointer group select-none ${
                    isDropTarget ? "bg-blue-100 ring-2 ring-blue-400" : "hover:bg-gray-50"
                }`}
                style={{ paddingLeft }}
                onClick={() => onToggle(folder.id)}
            >
                <span className="flex-shrink-0 mr-1">
                    {isExpanded ? (
                        <ChevronDown className="w-4 h-4 text-gray-500" />
                    ) : (
                        <ChevronRight className="w-4 h-4 text-gray-500" />
                    )}
                </span>
                {isExpanded ? (
                    <FolderOpen className="w-4 h-4 mr-2 flex-shrink-0 text-blue-600" />
                ) : (
                    <Folder className="w-4 h-4 mr-2 flex-shrink-0 text-gray-600" />
                )}
                <span className="flex-1 truncate font-medium">{folder.name}</span>
                <span className="text-xs text-gray-400 mr-2">{folderMeetings.length}</span>
                <div className="flex items-center opacity-0 group-hover:opacity-100 transition-opacity duration-150">
                    <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                            <button
                                onClick={(e) => e.stopPropagation()}
                                className="hover:text-blue-600 p-1 rounded-md hover:bg-blue-50 flex-shrink-0"
                                aria-label="Folder actions"
                            >
                                <MoreVertical className="w-4 h-4" />
                            </button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-48">
                            <DropdownMenuItem
                                onSelect={() => onCreateSubfolder(folder.id)}
                            >
                                <FolderPlus className="w-4 h-4 mr-2" />
                                Nova subpasta
                            </DropdownMenuItem>
                            <DropdownMenuItem
                                onSelect={() => onRenameFolder(folder.id, folder.name)}
                            >
                                <Pencil className="w-4 h-4 mr-2" />
                                Renomear
                            </DropdownMenuItem>
                            <DropdownMenuItem
                                onSelect={() => onRequestMoveFolder(folder.id)}
                            >
                                <FolderInput className="w-4 h-4 mr-2" />
                                Mover para...
                            </DropdownMenuItem>
                            <DropdownMenuSeparator />
                            <DropdownMenuItem
                                onSelect={() => onRequestDeleteFolder(folder.id)}
                                className="text-red-600 focus:text-red-700"
                            >
                                <Trash2 className="w-4 h-4 mr-2" />
                                Excluir
                            </DropdownMenuItem>
                        </DropdownMenuContent>
                    </DropdownMenu>
                </div>
            </div>
            {isExpanded && (
                <div>
                    {childFolders.map((cf) => (
                        <FolderTreeItem
                            key={cf.id}
                            folder={cf}
                            depth={depth + 1}
                            expanded={expanded}
                            onToggle={onToggle}
                            currentMeetingId={currentMeetingId}
                            onEditMeeting={onEditMeeting}
                            onRequestDeleteMeeting={onRequestDeleteMeeting}
                            onMoveMeeting={onMoveMeeting}
                            onMoveFolder={onMoveFolder}
                            onCreateSubfolder={onCreateSubfolder}
                            onRenameFolder={onRenameFolder}
                            onRequestDeleteFolder={onRequestDeleteFolder}
                            onRequestMoveFolder={onRequestMoveFolder}
                            renderMeeting={renderMeeting}
                        />
                    ))}
                    {folderMeetings.map((m) => renderMeeting(m, depth + 1))}
                    {folderMeetings.length === 0 && childFolders.length === 0 && (
                        <p
                            className="text-xs text-gray-400 italic px-3 py-2"
                            style={{ paddingLeft: `${(depth + 1) * 12 + 12}px` }}
                        >
                            Arraste meetings aqui ou use Mover para...
                        </p>
                    )}
                </div>
            )}
        </div>
    );
}
