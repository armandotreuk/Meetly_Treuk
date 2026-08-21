"use client";

import { useCallback, useEffect, useRef, useState } from "react";

interface UsePanelResizeOptions {
    initial: number;
    min: number;
    /** Max width as a fraction of the window inner width (e.g. 0.6 = 60%). */
    maxFraction: number;
    /** "left" panel grows when dragging right; "right" panel grows when dragging left. */
    side: "left" | "right";
    /** Persist and restore the panel width under this localStorage key when provided. */
    storageKey?: string;
}

interface UsePanelResizeResult {
    width: number;
    isDragging: boolean;
    handleProps: { onMouseDown: (e: React.MouseEvent) => void };
}

export function loadClampedWidth(
    storageKey: string | undefined,
    initial: number,
    min: number,
    maxFraction: number
): number {
    if (!storageKey) return initial;
    try {
        const raw = localStorage.getItem(storageKey);
        if (raw == null) return initial;
        const stored = Number(raw);
        if (!Number.isFinite(stored)) return initial;
        const max = typeof window !== "undefined" ? Math.floor(window.innerWidth * maxFraction) : Number.MAX_SAFE_INTEGER;
        return Math.max(min, Math.min(max, stored));
    } catch {
        return initial;
    }
}

/**
 * Horizontal drag-to-resize for a panel anchored to one side of the window.
 * Optionally persists the width in localStorage. Clamps width to [min,
 * window.innerWidth * maxFraction] so the panel always stays inside the window.
 */
export function usePanelResize({
    initial,
    min,
    maxFraction,
    side,
    storageKey,
}: UsePanelResizeOptions): UsePanelResizeResult {
    const [width, setWidth] = useState(() => loadClampedWidth(storageKey, initial, min, maxFraction));
    const [isDragging, setIsDragging] = useState(false);
    const startRef = useRef<{ x: number; w: number }>({ x: 0, w: initial });

    const handleMouseDown = useCallback(
        (e: React.MouseEvent) => {
            e.preventDefault();
            startRef.current = { x: e.clientX, w: width };
            setIsDragging(true);
        },
        [width]
    );

    useEffect(() => {
        if (!isDragging) return;

        const onMove = (e: MouseEvent) => {
            const max = Math.floor(window.innerWidth * maxFraction);
            const delta = e.clientX - startRef.current.x;
            const next = side === "left" ? startRef.current.w + delta : startRef.current.w - delta;
            const clamped = Math.max(min, Math.min(max, next));
            setWidth(clamped);
            if (storageKey) {
                try {
                    // ponytail: synchronous localStorage write per drag tick (cheap; at most ~60fps).
                    //           upgrade: persist only on mouseup to halve writes if it ever shows on a perf trace.
                    window.localStorage.setItem(storageKey, String(clamped));
                } catch {
                    return;
                }
            }
        };
        const onUp = () => setIsDragging(false);

        const prevCursor = document.body.style.cursor;
        const prevUserSelect = document.body.style.userSelect;
        document.body.style.cursor = "col-resize";
        document.body.style.userSelect = "none";

        window.addEventListener("mousemove", onMove);
        window.addEventListener("mouseup", onUp);

        return () => {
            document.body.style.cursor = prevCursor;
            document.body.style.userSelect = prevUserSelect;
            window.removeEventListener("mousemove", onMove);
            window.removeEventListener("mouseup", onUp);
        };
    }, [isDragging, min, maxFraction, side, storageKey]);

    return { width, isDragging, handleProps: { onMouseDown: handleMouseDown } };
}
