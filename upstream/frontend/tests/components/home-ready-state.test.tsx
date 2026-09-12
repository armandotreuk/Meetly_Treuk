import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HomeReadyState } from "@/app/_components/HomeReadyState";
import type { CurrentMeeting } from "@/components/Sidebar/SidebarProvider";

let root: Root;
let container: HTMLDivElement;

const meetings: CurrentMeeting[] = [
    { id: "oldest", title: "Oldest meeting", created_at: "2026-09-01T09:00:00.000Z" },
    { id: "second", title: "Second meeting", created_at: "2026-09-02T09:00:00.000Z" },
    { id: "third", title: "Third meeting", created_at: "2026-09-03T09:00:00.000Z" },
    { id: "fourth", title: "Fourth meeting", created_at: "2026-09-04T09:00:00.000Z" },
    { id: "newest", title: "Newest meeting", created_at: "2026-09-05T09:00:00.000Z" },
];

function renderReadyState(overrides: Partial<React.ComponentProps<typeof HomeReadyState>> = {}) {
    const props: React.ComponentProps<typeof HomeReadyState> = {
        meetings,
        modelName: "large-v3-turbo-q8_0",
        hasMicrophone: true,
        isCheckingMicrophone: false,
        canImportAudio: true,
        onStartRecording: vi.fn(),
        onCheckMicrophone: vi.fn(),
        onImportAudio: vi.fn(),
        onOpenMeeting: vi.fn(),
        ...overrides,
    };
    act(() => root.render(<HomeReadyState {...props} />));
    return props;
}

beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
});

afterEach(() => {
    act(() => root.unmount());
    container.remove();
});

describe("HomeReadyState", () => {
    it("shows the newest four meetings and opens the selected meeting", () => {
        const props = renderReadyState();
        const meetingButtons = Array.from(container.querySelectorAll("li button"));

        expect(meetingButtons.map((button) => button.textContent)).toHaveLength(4);
        expect(meetingButtons.map((button) => button.textContent).join(" ")).toContain(
            "Newest meeting"
        );
        expect(meetingButtons.map((button) => button.textContent).join(" ")).not.toContain(
            "Oldest meeting"
        );

        act(() => meetingButtons[0].click());
        expect(props.onOpenMeeting).toHaveBeenCalledWith("newest");
    });

    it("uses the microphone check when no microphone is available", () => {
        const props = renderReadyState({ hasMicrophone: false });
        const action = Array.from(container.querySelectorAll("button")).find((button) =>
            button.textContent?.includes("Check microphone")
        ) as HTMLButtonElement;

        act(() => action.click());
        expect(props.onCheckMicrophone).toHaveBeenCalledOnce();
        expect(props.onStartRecording).not.toHaveBeenCalled();
    });

    it("starts recording and respects the import beta gate", () => {
        const props = renderReadyState({ canImportAudio: false });
        const start = Array.from(container.querySelectorAll("button")).find((button) =>
            button.textContent?.includes("Start recording")
        ) as HTMLButtonElement;

        act(() => start.click());
        expect(props.onStartRecording).toHaveBeenCalledOnce();
        expect(container.textContent).not.toContain("Import audio");
    });
});
