import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
    ACTIVE_POLL_INTERVAL_MS,
    IDLE_POLL_INTERVAL_MS,
    RetrievalIndexSettings,
    type RetrievalStatusReport,
} from "@/components/RetrievalIndexSettings";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const status: RetrievalStatusReport = {
    backend: "exact",
    indexed_scope: "all_saved_meetings",
    semantic_state: "ready",
    serving_state: "ready",
    shadow_state: null,
    operation_active: false,
    force_lexical_retrieval: false,
    model_load_failure: null,
    paused: false,
    active_generation_id: "generation-1",
    active_model_id: "model-1",
    building_generations: [],
    document_count: 12,
    tracked_meetings: 4,
    current_meetings: 4,
    retry_meetings: 0,
    failed_meetings: 0,
    canonical_change_id: 2,
    published_change_id: 2,
    activation_blockers: [],
    resident_index_bytes: 1024,
    resident_process_bytes: 2048,
    activation_ram_ceiling_bytes: 4096,
    activation_ram_scope: "whole-process RSS",
    derived_disk_bytes: 2048,
    derived_disk_is_estimate: false,
    derived_disk_measurement_status: "exact",
    derived_disk_estimate_bytes: null,
    derived_disk_gate_input_bytes: 2048,
    wal_file_size_bytes: null,
    derived_disk_steady_target_bytes: 4096,
    derived_disk_activation_limit_bytes: 8192,
    model: {
        embedding_name: "intfloat/multilingual-e5-base",
        embedding_revision: "embedding-revision",
        embedding_attribution: "Embedding attribution",
        embedding_license: "MIT",
        embedding_license_url: "https://example.com/embedding-license",
        reranker_name: "cross-encoder/mmarco",
        reranker_revision: "reranker-revision",
        reranker_attribution: "Reranker attribution",
        reranker_license: "Apache-2.0",
        reranker_license_url: "https://example.com/reranker-license",
    },
    model_artifact_state: "verified",
};

function shadowStatus(
    overrides: Partial<RetrievalStatusReport["building_generations"][number]> = {}
) {
    return {
        generation_id: "generation-shadow",
        model_id: "model-1",
        state: "building",
        document_count: 3,
        tracked_meetings: 4,
        current_meetings: 1,
        retry_meetings: 0,
        failed_meetings: 0,
        canonical_change_id: 3,
        published_change_id: 1,
        ...overrides,
    };
}

let root: Root;
let container: HTMLDivElement;
let currentStatus: RetrievalStatusReport;

beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    mocks.invoke.mockReset();
    currentStatus = status;
    mocks.invoke.mockImplementation((command: string) => {
        if (command === "retrieval_index_status") return Promise.resolve(currentStatus);
        return Promise.resolve();
    });
});

afterEach(() => {
    act(() => root.unmount());
    container.remove();
    document.body.querySelectorAll("[data-radix-portal]").forEach((portal) => portal.remove());
    vi.useRealTimers();
});

describe("RetrievalIndexSettings", () => {
    it("measures derived disk against the envelope that applies", async () => {
        vi.useFakeTimers();
        // Between the steady-state target and the shadow-rebuild limit: in
        // steady state this is over budget, during a rebuild it is not.
        const overSteady = { ...status, derived_disk_gate_input_bytes: 6144 };
        mocks.invoke.mockImplementation((command: string) =>
            command === "retrieval_index_status" ? Promise.resolve(overSteady) : Promise.resolve()
        );
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });
        expect(container.textContent).toContain("Steady-state envelope exceeded (4.0 KiB target)");

        const rebuilding = {
            ...overSteady,
            semantic_state: "building",
            shadow_state: "building",
            operation_active: true,
            building_generations: [shadowStatus({ current_meetings: 2 })],
        };
        mocks.invoke.mockImplementation((command: string) =>
            command === "retrieval_index_status" ? Promise.resolve(rebuilding) : Promise.resolve()
        );
        await act(async () => {
            await vi.advanceTimersByTimeAsync(IDLE_POLL_INTERVAL_MS);
        });
        expect(container.textContent).toContain("Within activation envelope (8.0 KiB limit)");
    });

    it("quotes one envelope: the size line and the envelope line cannot disagree", async () => {
        vi.useFakeTimers();
        const rebuilding = {
            ...status,
            derived_disk_bytes: 6144,
            derived_disk_gate_input_bytes: 6144,
            shadow_state: "building",
            operation_active: true,
            building_generations: [shadowStatus({ current_meetings: 2 })],
        };
        mocks.invoke.mockImplementation((command: string) =>
            command === "retrieval_index_status" ? Promise.resolve(rebuilding) : Promise.resolve()
        );
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });

        // During a rebuild the applicable ceiling is the activation limit, so
        // the measured size must be quoted against it, not against the
        // steady-state target the line below it is no longer using.
        expect(container.textContent).toContain("6.0 KiB · rebuild limit 8.0 KiB");
        expect(container.textContent).not.toContain("steady-state target 4.0 KiB");
        expect(container.textContent).toContain("Within activation envelope (8.0 KiB limit)");
    });

    it("treats derived disk exactly at the approved ceiling as within it", async () => {
        // The approved gate is "at most", so equality is inside the envelope.
        const atCeiling = {
            ...status,
            derived_disk_gate_input_bytes: status.derived_disk_steady_target_bytes,
        };
        mocks.invoke.mockImplementation((command: string) =>
            command === "retrieval_index_status" ? Promise.resolve(atCeiling) : Promise.resolve()
        );
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });
        expect(container.textContent).not.toContain("Steady-state envelope exceeded");
        expect(container.textContent).toContain("Within steady-state envelope");
    });

    it("does not report a healthy build with transient retries as a broken index", async () => {
        vi.useFakeTimers();
        // `retry` is the worker's ordinary backoff state; only terminal
        // failures are an index error.
        const retrying = {
            ...status,
            retry_meetings: 2,
            semantic_state: "building",
            shadow_state: "building",
            operation_active: true,
            building_generations: [shadowStatus({ current_meetings: 1, retry_meetings: 2 })],
        };
        mocks.invoke.mockImplementation((command: string) =>
            command === "retrieval_index_status" ? Promise.resolve(retrying) : Promise.resolve()
        );
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });
        expect(container.textContent).not.toContain("Semantic indexing needs attention");

        const failing = {
            ...retrying,
            building_generations: [shadowStatus({ current_meetings: 1, failed_meetings: 1 })],
        };
        mocks.invoke.mockImplementation((command: string) =>
            command === "retrieval_index_status" ? Promise.resolve(failing) : Promise.resolve()
        );
        await act(async () => {
            await vi.advanceTimersByTimeAsync(ACTIVE_POLL_INTERVAL_MS);
        });
        expect(container.textContent).toContain("Semantic indexing needs attention");
    });

    it("exposes status, the persisted lexical kill switch, and derived-only clear confirmation", async () => {
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });

        expect(container.textContent).toContain("Ready");
        expect(container.textContent).toContain("4 / 4");
        expect(container.textContent).toContain("Scope: all saved meetings");
        expect(container.textContent).toContain("intfloat/multilingual-e5-base");
        expect(container.textContent).toContain("License: MIT");
        // Steady state is measured against the steady-state target, not
        // the higher shadow-rebuild activation limit.
        expect(container.textContent).toContain("Within steady-state envelope (4.0 KiB target)");
        expect(container.textContent).toContain("Resident memory");
        expect(container.querySelector('[role="status"]')?.getAttribute("aria-live")).toBe(
            "polite"
        );
        expect(container.querySelector("progress")?.getAttribute("aria-label")).toContain("100%");

        const forceLexical = container.querySelector('[role="switch"]') as HTMLButtonElement;
        expect(forceLexical.getAttribute("aria-checked")).toBe("false");
        await act(async () => forceLexical.click());
        expect(mocks.invoke).toHaveBeenCalledWith("api_chat_set_force_lexical_retrieval", {
            enabled: true,
        });

        const rebuild = Array.from(container.querySelectorAll("button")).find(
            (button) => button.textContent?.trim() === "Rebuild"
        ) as HTMLButtonElement;
        await act(async () => rebuild.click());
        expect(document.body.textContent).toContain("recordings, conversations, keyword search");
        const confirmRebuild = Array.from(document.body.querySelectorAll("button")).find((button) =>
            button.textContent?.includes("Rebuild index")
        ) as HTMLButtonElement;
        await act(async () => confirmRebuild.click());
        expect(mocks.invoke).toHaveBeenCalledWith("retrieval_rebuild_index");

        const clear = Array.from(container.querySelectorAll("button")).find((button) =>
            button.textContent?.includes("Clear local semantic index")
        ) as HTMLButtonElement;
        await act(async () => clear.click());
        expect(document.body.textContent).toContain("Meetings, transcripts, summaries, notes");

        const confirm = Array.from(document.body.querySelectorAll("button")).find((button) =>
            button.textContent?.includes("Clear index")
        ) as HTMLButtonElement;
        await act(async () => confirm.click());
        expect(mocks.invoke).toHaveBeenCalledWith("retrieval_clear_index");
    });

    it.each([
        ["queued", "Queued", 0, true],
        ["building", "Building", 2, true],
        ["paused", "Paused", 2, true],
        ["failed", "Needs attention", 3, false],
        ["complete", "Complete", 4, false],
    ])(
        "renders the shadow lifecycle state %s from backend progress",
        async (shadowState, label, currentMeetings, operationActive) => {
            currentStatus = {
                ...status,
                semantic_state: shadowState === "complete" ? "complete" : shadowState,
                shadow_state: shadowState,
                operation_active: operationActive,
                building_generations: [
                    shadowStatus({
                        state:
                            shadowState === "complete"
                                ? "ready"
                                : shadowState === "failed"
                                  ? "failed"
                                  : "building",
                        current_meetings: currentMeetings,
                        failed_meetings: shadowState === "failed" ? 1 : 0,
                    }),
                ],
            };
            await act(async () => {
                root.render(<RetrievalIndexSettings />);
                await Promise.resolve();
            });

            expect(container.textContent).toContain(label);
            expect(container.textContent).toContain(`${currentMeetings} / 4`);
            const rebuild = Array.from(container.querySelectorAll("button")).find(
                (button) => button.textContent?.trim() === "Rebuild"
            ) as HTMLButtonElement;
            expect(rebuild.disabled).toBe(operationActive);
            const forceLexical = container.querySelector('[role="switch"]') as HTMLButtonElement;
            expect(forceLexical.disabled).toBe(false);
            const clear = Array.from(container.querySelectorAll("button")).find((button) =>
                button.textContent?.includes("Clear local semantic index")
            ) as HTMLButtonElement;
            expect(clear.disabled).toBe(operationActive);
        }
    );

    it.each([
        ["loading", "Loading"],
        ["catching_up", "Catching up"],
        ["transitioning", "Switching index"],
        ["unavailable", "Unavailable"],
        ["model_mismatch", "Model mismatch"],
    ])("renders serving state %s", async (servingState, label) => {
        currentStatus = {
            ...status,
            semantic_state: servingState,
            serving_state: servingState,
        };
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });

        expect(container.textContent).toContain(label);
    });

    it("uses shadow progress and only re-enables conflicting controls after polling reaches terminal state", async () => {
        vi.useFakeTimers();
        currentStatus = {
            ...status,
            semantic_state: "building",
            shadow_state: "building",
            operation_active: true,
            building_generations: [shadowStatus({ current_meetings: 2 })],
        };
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });

        expect(container.textContent).toContain("2 / 4");
        expect((container.querySelector("progress") as HTMLProgressElement).value).toBe(50);
        const rebuild = () =>
            Array.from(container.querySelectorAll("button")).find(
                (button) => button.textContent?.trim() === "Rebuild"
            ) as HTMLButtonElement;
        expect(rebuild().disabled).toBe(true);
        expect((container.querySelector('[role="switch"]') as HTMLButtonElement).disabled).toBe(
            false
        );

        currentStatus = { ...status };
        await act(async () => {
            await vi.advanceTimersByTimeAsync(IDLE_POLL_INTERVAL_MS);
        });
        expect(rebuild().disabled).toBe(false);
    });

    it("ignores an older poll response after a newer lifecycle response", async () => {
        vi.useFakeTimers();
        const resolveStatus: Array<(value: RetrievalStatusReport) => void> = [];
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "retrieval_index_status") {
                return new Promise<RetrievalStatusReport>((resolve) => resolveStatus.push(resolve));
            }
            return Promise.resolve();
        });
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });
        expect(resolveStatus).toHaveLength(1);

        await act(async () => {
            await vi.advanceTimersByTimeAsync(IDLE_POLL_INTERVAL_MS);
        });
        expect(resolveStatus).toHaveLength(2);
        const building = {
            ...status,
            semantic_state: "building",
            shadow_state: "building",
            operation_active: true,
            building_generations: [shadowStatus({ current_meetings: 2 })],
        };
        await act(async () => {
            resolveStatus[1](building);
            await Promise.resolve();
        });
        expect(container.textContent).toContain("Building");
        expect(
            Array.from(container.querySelectorAll("button")).find(
                (button) => button.textContent?.trim() === "Rebuild"
            )
        ).toHaveProperty("disabled", true);

        await act(async () => {
            resolveStatus[0](status);
            await Promise.resolve();
        });
        expect(container.textContent).toContain("2 / 4");
        expect(container.textContent).toContain("Building");
        expect(
            Array.from(container.querySelectorAll("button")).find(
                (button) => button.textContent?.trim() === "Rebuild"
            )
        ).toHaveProperty("disabled", true);
    });

    it("ignores a deferred status response after unmount", async () => {
        let resolveStatus!: (value: RetrievalStatusReport) => void;
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "retrieval_index_status") {
                return new Promise<RetrievalStatusReport>((resolve) => {
                    resolveStatus = resolve;
                });
            }
            return Promise.resolve();
        });
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });
        expect(container.textContent).toContain("Loading retrieval status...");
        await act(async () => root.unmount());
        await act(async () => {
            resolveStatus(status);
            await Promise.resolve();
        });
        expect(container.textContent).toBe("");
    });

    it("keeps the shadow lifecycle visible without claiming semantic serving in forced lexical mode", async () => {
        currentStatus = {
            ...status,
            force_lexical_retrieval: true,
            semantic_state: "building",
            shadow_state: "building",
            operation_active: true,
            building_generations: [shadowStatus({ current_meetings: 2 })],
        };
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });

        expect(container.textContent).toContain("Keyword-only retrieval");
        expect(container.textContent).toContain("Background semantic index: Building.");
        expect(container.textContent).not.toContain("Current semantic search remains available");
        const forceLexical = container.querySelector('[role="switch"]') as HTMLButtonElement;
        expect(forceLexical.disabled).toBe(false);
        await act(async () => forceLexical.click());
        expect(mocks.invoke).toHaveBeenCalledWith("api_chat_set_force_lexical_retrieval", {
            enabled: false,
        });
    });

    it("surfaces a shadow failure and retries that generation without exposing backend details", async () => {
        currentStatus = {
            ...status,
            semantic_state: "failed",
            shadow_state: "failed",
            operation_active: false,
            building_generations: [
                shadowStatus({ state: "failed", current_meetings: 3, failed_meetings: 1 }),
            ],
        };
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });

        expect(container.textContent).toContain("Needs attention");
        expect(container.textContent).toContain("3 / 4");
        expect(container.textContent).not.toContain("generation-shadow");
        const retry = Array.from(container.querySelectorAll("button")).find(
            (button) => button.textContent?.trim() === "Retry"
        ) as HTMLButtonElement;
        await act(async () => retry.click());
        expect(mocks.invoke).toHaveBeenCalledWith("retrieval_retry_rebuild", {
            generationId: "generation-shadow",
        });
    });

    it("keeps pause/resume available while a shadow is active and refreshes its state", async () => {
        currentStatus = {
            ...status,
            semantic_state: "paused",
            shadow_state: "paused",
            paused: true,
            operation_active: true,
            building_generations: [shadowStatus({ current_meetings: 1 })],
        };
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });

        const pause = Array.from(container.querySelectorAll("button")).find(
            (button) => button.textContent?.trim() === "Resume indexing"
        ) as HTMLButtonElement;
        expect(pause.disabled).toBe(false);
        await act(async () => pause.click());
        expect(mocks.invoke).toHaveBeenCalledWith("retrieval_set_index_paused", {
            paused: false,
        });
    });

    it("renders loading and recovers from a failed status poll", async () => {
        let rejectStatus!: (error: Error) => void;
        let attempts = 0;
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "retrieval_index_status") {
                attempts += 1;
                if (attempts === 1) {
                    return new Promise<RetrievalStatusReport>((_, reject) => {
                        rejectStatus = reject;
                    });
                }
                return Promise.resolve(status);
            }
            return Promise.resolve();
        });
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
        });
        expect(container.textContent).toContain("Loading retrieval status...");
        rejectStatus(new Error("private status failure"));
        await act(async () => {
            await Promise.resolve();
        });
        expect(container.textContent).toContain("Status unavailable");
        const retry = Array.from(container.querySelectorAll("button")).find(
            (button) => button.textContent?.trim() === "Retry"
        ) as HTMLButtonElement;
        await act(async () => retry.click());
        await Promise.resolve();
        expect(container.textContent).toContain("Ready");
    });

    it("shows unavailable artifacts with generic retry copy and keeps lexical mode distinct", async () => {
        currentStatus = {
            ...status,
            semantic_state: "model_unavailable",
            model_load_failure: "private artifact path",
            model_artifact_state: "unavailable",
        };
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });

        expect(container.textContent).toContain("Model unavailable");
        expect(container.textContent).toContain("Unavailable");
        expect(container.textContent).not.toContain("private artifact path");
        const retry = Array.from(container.querySelectorAll("button")).find(
            (button) => button.textContent?.trim() === "Retry"
        ) as HTMLButtonElement;
        await act(async () => retry.click());
        expect(mocks.invoke).toHaveBeenCalledWith("retrieval_rebuild_index");
    });

    it("cancels rebuild and clear confirmations without invoking either command", async () => {
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });
        const rebuild = Array.from(container.querySelectorAll("button")).find(
            (button) => button.textContent?.trim() === "Rebuild"
        ) as HTMLButtonElement;
        await act(async () => rebuild.click());
        const cancel = Array.from(document.body.querySelectorAll("button")).find(
            (button) => button.textContent?.trim() === "Cancel"
        ) as HTMLButtonElement;
        await act(async () => cancel.click());
        expect(mocks.invoke).not.toHaveBeenCalledWith("retrieval_rebuild_index");

        const clear = Array.from(container.querySelectorAll("button")).find((button) =>
            button.textContent?.includes("Clear local semantic index")
        ) as HTMLButtonElement;
        await act(async () => clear.click());
        const clearCancel = Array.from(document.body.querySelectorAll("button")).find(
            (button) => button.textContent?.trim() === "Cancel"
        ) as HTMLButtonElement;
        await act(async () => clearCancel.click());
        expect(mocks.invoke).not.toHaveBeenCalledWith("retrieval_clear_index");
    });

    it("keeps rebuild and clear failures generic", async () => {
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "retrieval_index_status") return Promise.resolve(status);
            if (command === "retrieval_clear_index") {
                return Promise.reject(new Error("private database path"));
            }
            return Promise.resolve();
        });
        await act(async () => {
            root.render(<RetrievalIndexSettings />);
            await Promise.resolve();
        });
        const clear = Array.from(container.querySelectorAll("button")).find((button) =>
            button.textContent?.includes("Clear local semantic index")
        ) as HTMLButtonElement;
        await act(async () => clear.click());
        const confirm = Array.from(document.body.querySelectorAll("button")).find((button) =>
            button.textContent?.includes("Clear index")
        ) as HTMLButtonElement;
        await act(async () => confirm.click());
        await Promise.resolve();
        expect(container.textContent).not.toContain("private database path");
        expect(mocks.invoke).toHaveBeenCalledWith("retrieval_clear_index");
    });
});
