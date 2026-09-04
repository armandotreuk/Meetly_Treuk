"use client";

import React, { useRef, useEffect } from "react";
import { useRouter } from "next/navigation";
import { File, MoreVertical, Pencil, Trash2, FolderInput } from "lucide-react";
import { useSidebar } from "./SidebarProvider";
import { formatMeetingDate } from "@/lib/utils";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

// ponytail: global drag-drop coordinator module. Avoids React Context round-trips
// for transient drag state. Ceiling: one drag at a time (matches UI). Upgrade
// path if needed: React Context + reducer.
export type DragPayload = { kind: "meeting" | "folder"; id: string } | null;
export interface DragState {
    payload: DragPayload;
    sourceEl: HTMLElement | null;
    ghostEl: HTMLElement | null;
    dropTargetEl: HTMLElement | null;
}
const DRAG_KEY = "__meetily_drag_state__";

export function getDragState(): DragState | null {
    return (window as unknown as Record<string, DragState | null>)[DRAG_KEY] ?? null;
}
export function setDragState(state: DragState | null) {
    (window as unknown as Record<string, DragState | null>)[DRAG_KEY] = state;
}
export function setDragDropTarget(el: HTMLElement | null) {
    const s = getDragState();
    if (s) s.dropTargetEl = el;
}

export const DRAG_THRESHOLD_PX = 6;

export function makeGhost(sourceEl: HTMLElement, label: string): HTMLElement {
    const ghost = sourceEl.cloneNode(true) as HTMLElement;
    ghost.style.position = "fixed";
    ghost.style.pointerEvents = "none";
    ghost.style.opacity = "0.7";
    ghost.style.zIndex = "9999";
    ghost.style.background = "white";
    ghost.style.border = "1px solid #3b82f6";
    ghost.style.borderRadius = "6px";
    ghost.style.padding = "8px 12px";
    ghost.style.boxShadow = "0 4px 12px rgba(0,0,0,0.15)";
    ghost.style.width = `${sourceEl.offsetWidth}px`;
    ghost.style.whiteSpace = "nowrap";
    ghost.style.overflow = "hidden";
    ghost.style.textOverflow = "ellipsis";
    ghost.innerText = label;
    document.body.appendChild(ghost);
    return ghost;
}

export function moveGhost(ghost: HTMLElement, x: number, y: number) {
    ghost.style.left = `${x + 8}px`;
    ghost.style.top = `${y + 8}px`;
}

export function removeGhost(ghost: HTMLElement | null) {
    if (ghost && ghost.parentNode) ghost.parentNode.removeChild(ghost);
}

export function findDropTargetAt(x: number, y: number): HTMLElement | null {
    let el = document.elementFromPoint(x, y);
    while (el && el !== document.documentElement) {
        if (el.hasAttribute && el.hasAttribute("data-drop-target")) {
            return el as HTMLElement;
        }
        el = el.parentElement;
    }
    return null;
}

interface MeetingTreeItemProps {
    meetingId: string;
    title: string;
    depth: number;
    currentMeetingId?: string;
    snippetContext?: string | null;
    chunkType?: string | null;
    provenanceLabel?: string | null;
    folderName?: string | null;
    createdAt?: string;
    hasNotes?: boolean;
    onEditMeeting: (meetingId: string, currentTitle: string) => void;
    onRequestDeleteMeeting: (meetingId: string) => void;
    onRequestMoveMeeting: (meetingId: string) => void;
}

export function MeetingTreeItem({
    meetingId,
    title,
    depth,
    currentMeetingId,
    snippetContext,
    chunkType,
    provenanceLabel,
    folderName,
    createdAt,
    hasNotes,
    onEditMeeting,
    onRequestDeleteMeeting,
    onRequestMoveMeeting,
}: MeetingTreeItemProps) {
    const router = useRouter();
    const isActive = currentMeetingId === meetingId;
    const paddingLeft = `${depth * 12 + 12}px`;
    const isIntro = meetingId.startsWith("intro-call");
    const formattedDate = isIntro ? "" : formatMeetingDate(createdAt, "short");
    const rootRef = useRef<HTMLDivElement>(null);
    const dragStateRef = useRef<{
        startX: number;
        startY: number;
        dragging: boolean;
    } | null>(null);

    useEffect(() => {
        const root = rootRef.current;
        if (!root) return;

        const onMouseMove = (e: MouseEvent) => {
            const ds = dragStateRef.current;
            if (!ds) return;
            const dx = e.clientX - ds.startX;
            const dy = e.clientY - ds.startY;
            if (!ds.dragging) {
                if (Math.hypot(dx, dy) < DRAG_THRESHOLD_PX) return;
                ds.dragging = true;
                // Lock text selection globally for the duration of the drag.
                document.body.style.userSelect = "none";
                document.body.style.webkitUserSelect = "none";
                const ghost = makeGhost(root, title);
                setDragState({
                    payload: { kind: "meeting", id: meetingId },
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
                // Restore text selection after the drag finishes.
                document.body.style.userSelect = "";
                document.body.style.webkitUserSelect = "";
            }
            // Keep dragStateRef alive for one tick so the synthetic click event
            // (which fires after mouseup) can see `dragging === true` and be
            // suppressed by onClickCapture. Then clear.
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

        const onMouseDown = (e: MouseEvent) => {
            if (isIntro) return;
            // Only left-button drags.
            if (e.button !== 0) return;
            // Don't preventDefault here — that would also block focus on inner
            // buttons (Pencil, MoreVertical, Trash2). Text selection is blocked
            // via `select-none` on the row container instead.
            dragStateRef.current = { startX: e.clientX, startY: e.clientY, dragging: false };
            document.addEventListener("mousemove", onMouseMove);
            document.addEventListener("mouseup", onMouseUp);
        };

        const onClickCapture = (e: MouseEvent) => {
            // If a drag just completed, swallow the synthetic click so it doesn't
            // trigger router.push on the source item.
            const s = dragStateRef.current;
            if (s?.dragging) {
                e.stopPropagation();
                e.preventDefault();
            }
        };

        root.addEventListener("mousedown", onMouseDown);
        root.addEventListener("click", onClickCapture, true);
        return () => {
            root.removeEventListener("mousedown", onMouseDown);
            root.removeEventListener("click", onClickCapture, true);
            document.removeEventListener("mousemove", onMouseMove);
            document.removeEventListener("mouseup", onMouseUp);
        };
    }, [meetingId, title, isIntro]);

    const handleClick = () => {
        const basePath = isIntro ? "/" : `/meeting-details?id=${meetingId}`;
        router.push(basePath);
    };

    return (
        <div
            ref={rootRef}
            className={`flex flex-col w-full group px-3 py-2 my-0.5 rounded-md text-sm cursor-pointer select-none ${
                isActive
                    ? "bg-blue-100 text-blue-700 font-medium"
                    : snippetContext
                      ? "bg-yellow-50"
                      : "hover:bg-gray-50"
            }`}
            style={{ paddingLeft }}
        >
            <div className="flex items-start w-full">
                <button
                    type="button"
                    className="flex min-w-0 flex-1 flex-col text-left"
                    onClick={handleClick}
                    aria-label={title}
                    tabIndex={0}
                >
                    <div className="flex items-center w-full">
                        <div className="flex-shrink-0 flex items-center justify-center w-6 h-6 rounded-full mr-2 bg-blue-100">
                            <PlusOrFile isIntro={isIntro} />
                        </div>
                        <span className="flex-1 min-w-0 flex items-center gap-1.5">
                            <span className="break-words">{title}</span>
                            {hasNotes && (
                                <span
                                    className="w-2 h-2 rounded-full bg-green-500 flex-shrink-0"
                                    title="Has notes"
                                    role="img"
                                    aria-label="Has notes"
                                />
                            )}
                        </span>
                    </div>
                    {snippetContext && (
                        <div className="mt-1 ml-8 text-xs text-gray-500 bg-yellow-50 p-1.5 rounded border border-yellow-100 line-clamp-2">
                            <span className="font-medium text-yellow-600">Match:</span>{" "}
                            {snippetContext}
                        </div>
                    )}
                    {formattedDate && (
                        <div className="mt-1 ml-8 text-xs text-gray-500">{formattedDate}</div>
                    )}
                    {(chunkType || provenanceLabel || folderName) && (
                        <div className="mt-1 ml-8 flex gap-1 flex-wrap">
                            {chunkType && (
                                <span className="px-1.5 py-0.5 text-[10px] rounded bg-indigo-100 text-indigo-700 border border-indigo-200">
                                    {chunkType}
                                </span>
                            )}
                            {provenanceLabel && (
                                <span className="px-1.5 py-0.5 text-[10px] rounded bg-purple-100 text-purple-700 border border-purple-200">
                                    {provenanceLabel}
                                </span>
                            )}
                            {folderName && (
                                <span className="px-1.5 py-0.5 text-[10px] rounded bg-gray-100 text-gray-600 border border-gray-200">
                                    {folderName}
                                </span>
                            )}
                        </div>
                    )}
                </button>
                {!isIntro && (
                    <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity duration-150">
                        <button
                            type="button"
                            onClick={(e) => {
                                e.stopPropagation();
                                onEditMeeting(meetingId, title);
                            }}
                            className="hover:text-blue-600 p-1 rounded-md hover:bg-blue-50 flex-shrink-0"
                            aria-label="Edit meeting title"
                        >
                            <Pencil className="w-4 h-4" />
                        </button>
                        <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                                <button
                                    type="button"
                                    onClick={(e) => e.stopPropagation()}
                                    className="hover:text-blue-600 p-1 rounded-md hover:bg-blue-50 flex-shrink-0"
                                    aria-label="Meeting actions"
                                >
                                    <MoreVertical className="w-4 h-4" />
                                </button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end" className="w-48">
                                <DropdownMenuItem onSelect={() => onRequestMoveMeeting(meetingId)}>
                                    <FolderInput className="w-4 h-4 mr-2" />
                                    Mover para...
                                </DropdownMenuItem>
                                <DropdownMenuSeparator />
                                <DropdownMenuItem onSelect={() => onEditMeeting(meetingId, title)}>
                                    <Pencil className="w-4 h-4 mr-2" />
                                    Renomear
                                </DropdownMenuItem>
                                <DropdownMenuItem
                                    onSelect={() => onRequestDeleteMeeting(meetingId)}
                                    className="text-red-600 focus:text-red-700"
                                >
                                    <Trash2 className="w-4 h-4 mr-2" />
                                    Excluir
                                </DropdownMenuItem>
                            </DropdownMenuContent>
                        </DropdownMenu>
                        <button
                            type="button"
                            onClick={(e) => {
                                e.stopPropagation();
                                onRequestDeleteMeeting(meetingId);
                            }}
                            className="hover:text-red-600 p-1 rounded-md hover:bg-red-50 flex-shrink-0"
                            aria-label="Delete meeting"
                        >
                            <Trash2 className="w-4 h-4" />
                        </button>
                    </div>
                )}
            </div>
        </div>
    );
}

function PlusOrFile({ isIntro }: { isIntro: boolean }) {
    if (isIntro) {
        return (
            <svg
                xmlns="http://www.w3.org/2000/svg"
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                className="w-3.5 h-3.5 text-blue-600"
            >
                <path d="M5 12h14" />
                <path d="M12 5v14" />
            </svg>
        );
    }
    return <File className="w-3.5 h-3.5 text-gray-600" />;
}
