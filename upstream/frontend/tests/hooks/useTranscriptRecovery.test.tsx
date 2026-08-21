import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useTranscriptRecovery } from "@/hooks/useTranscriptRecovery";

const mocks = vi.hoisted(() => ({
    getMeetingMetadata: vi.fn(),
    getTranscripts: vi.fn(),
    markMeetingSaved: vi.fn(),
    saveMeeting: vi.fn(),
    invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@/services/indexedDBService", () => ({
    indexedDBService: {
        getMeetingMetadata: mocks.getMeetingMetadata,
        getTranscripts: mocks.getTranscripts,
        markMeetingSaved: mocks.markMeetingSaved,
    },
}));
vi.mock("@/services/storageService", () => ({ storageService: { saveMeeting: mocks.saveMeeting } }));
vi.mock("@/lib/summary-language-preferences", () => ({ applyPinnedSummaryLanguageToMeeting: vi.fn() }));
vi.mock("sonner", () => ({ toast: { warning: vi.fn() } }));

function Recovery() {
    const { recoverMeeting } = useTranscriptRecovery();
    return <button onClick={() => recoverMeeting("recovery-1")}>recover</button>;
}

describe("useTranscriptRecovery live chat repair", () => {
    let root: Root;
    let container: HTMLDivElement;

    beforeEach(() => {
        container = document.createElement("div");
        document.body.appendChild(container);
        root = createRoot(container);
        mocks.getMeetingMetadata.mockResolvedValue({
            meetingId: "recovery-1",
            title: "Recovered",
            startTime: 1,
            lastUpdated: 2,
            transcriptCount: 1,
            savedToSQLite: false,
            liveChatScopeKey: "live-1",
        });
        mocks.getTranscripts.mockResolvedValue([{ text: "hello", timestamp: "now", sequenceId: 0 }]);
        mocks.saveMeeting.mockResolvedValue({ meeting_id: "meeting-1" });
        mocks.markMeetingSaved.mockResolvedValue(undefined);
    });

    afterEach(() => {
        act(() => root.unmount());
        container.remove();
        vi.clearAllMocks();
    });

    it("carries durable live linkage into restart recovery before clearing the checkpoint", async () => {
        await act(async () => root.render(<Recovery />));
        await act(async () => (container.querySelector("button") as HTMLButtonElement).click());

        expect(mocks.saveMeeting).toHaveBeenCalledWith("Recovered", [expect.objectContaining({ text: "hello" })], null, "live-1");
        expect(mocks.markMeetingSaved).toHaveBeenCalledWith("recovery-1");
        expect(mocks.saveMeeting.mock.invocationCallOrder[0]).toBeLessThan(mocks.markMeetingSaved.mock.invocationCallOrder[0]);
    });
});
