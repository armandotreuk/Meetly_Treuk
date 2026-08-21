import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { usePanelResize } from "@/hooks/usePanelResize";

describe("usePanelResize persistence", () => {
    const mounted: Array<{ root: ReturnType<typeof createRoot>; container: HTMLDivElement }> = [];

    afterEach(async () => {
        for (const { root, container } of mounted.splice(0)) {
            await act(async () => root.unmount());
            container.remove();
        }
        vi.restoreAllMocks();
    });

    function mount(options: Parameters<typeof usePanelResize>[0]) {
        let result: ReturnType<typeof usePanelResize> | undefined;
        let firstWidth: number | undefined;
        function TestComponent() {
            result = usePanelResize(options);
            firstWidth ??= result.width;
            return null;
        }

        const container = document.createElement("div");
        document.body.appendChild(container);
        const root = createRoot(container);
        mounted.push({ root, container });
        act(() => root.render(React.createElement(TestComponent)));
        return { getResult: () => result!, getFirstWidth: () => firstWidth! };
    }

    const options = {
        initial: 320,
        min: 240,
        maxFraction: 0.6,
        side: "right" as const,
    };

    it("restores a stored width on mount", () => {
        Object.defineProperty(window, "innerWidth", { configurable: true, value: 2000 });
        const getItem = vi.spyOn(window.localStorage, "getItem").mockReturnValue("500");
        const mountedHook = mount({ ...options, storageKey: "meedly:test" });

        expect(getItem).toHaveBeenCalledWith("meedly:test");
        expect(mountedHook.getFirstWidth()).toBe(500);
    });

    it("persists a width changed by dragging", () => {
        const setItem = vi.spyOn(window.localStorage, "setItem").mockImplementation(() => {});
        const getResult = mount({ ...options, storageKey: "meedly:test" }).getResult;

        act(() => getResult().handleProps.onMouseDown({ preventDefault: vi.fn(), clientX: 100 } as never));
        act(() => window.dispatchEvent(new MouseEvent("mousemove", { clientX: 150 })));

        expect(setItem).toHaveBeenCalledWith("meedly:test", "270");
    });

    it("does not access localStorage without a storage key", () => {
        const getItem = vi.spyOn(window.localStorage, "getItem");
        const setItem = vi.spyOn(window.localStorage, "setItem").mockImplementation(() => {});
        const getResult = mount(options).getResult;

        act(() => getResult().handleProps.onMouseDown({ preventDefault: vi.fn(), clientX: 100 } as never));
        act(() => window.dispatchEvent(new MouseEvent("mousemove", { clientX: 150 })));

        expect(getItem).not.toHaveBeenCalled();
        expect(setItem).not.toHaveBeenCalled();
    });

    it("clamps a stored width to the current window", () => {
        Object.defineProperty(window, "innerWidth", { configurable: true, value: 500 });
        vi.spyOn(window.localStorage, "getItem").mockReturnValue("500");
        const getResult = mount({ ...options, storageKey: "meedly:test" }).getResult;

        expect(getResult().width).toBe(300);
    });

    it("clamps an oversized stored width before the first render", () => {
        Object.defineProperty(window, "innerWidth", { configurable: true, value: 2000 });
        vi.spyOn(window.localStorage, "getItem").mockReturnValue("99999");
        const mountedHook = mount({ ...options, storageKey: "meedly:test" });

        expect(mountedHook.getFirstWidth()).toBe(1200);
    });

    it("falls back to initial when localStorage read throws", () => {
        vi.spyOn(window.localStorage, "getItem").mockImplementation(() => {
            throw new Error("blocked");
        });
        const getResult = mount({ ...options, storageKey: "meedly:test" }).getResult;

        expect(getResult().width).toBe(320);
    });
});
