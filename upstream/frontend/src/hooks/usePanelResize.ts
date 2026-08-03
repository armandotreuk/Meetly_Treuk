"use client";

import { useCallback, useEffect, useRef, useState } from "react";

interface UsePanelResizeOptions {
    initial: number;
    min: number;
    /** Max width as a fraction of the window inner width (e.g. 0.6 = 60%). */
    maxFraction: number;
    /** "left" panel grows when dragging right; "right" panel grows when dragging left. */
    side: "left" | "right";
}

interface UsePanelResizeResult {
    width: number;
    isDragging: boolean;
    handleProps: { onMouseDown: (e: React.MouseEvent) => void };
}

/**
 * Horizontal drag-to-resize for a panel anchored to one side of the window.
 * No persistence — resets to `initial` on remount. Clamps width to
 * [min, window.innerWidth * maxFraction] so the panel always stays inside the window.
 */
export function usePanelResize({
    initial,
    min,
    maxFraction,
    side,
}: UsePanelResizeOptions): UsePanelResizeResult {
    const [width, setWidth] = useState(initial);
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
            setWidth(Math.max(min, Math.min(max, next)));
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
    }, [isDragging, min, maxFraction, side]);

    return { width, isDragging, handleProps: { onMouseDown: handleMouseDown } };
}