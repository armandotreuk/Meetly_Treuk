import { describe, expect, test } from "vitest";
import {
    getDownloadTotalMb,
    getSummaryModelSizeLabel,
    getSummaryModelSizeMb,
    resolveOnboardingSummaryModelStatus,
} from "../../src/lib/onboarding-summary-model";

describe("onboarding summary model", () => {
    test("undownloaded selected Qwen model stays not ready", () => {
        expect(
            JSON.stringify(
                resolveOnboardingSummaryModelStatus({
                    selectedModel: "qwen3.5:4b",
                    recommendedModel: "qwen3.5:4b",
                    selectedModelReady: false,
                })
            )
        ).toBe(
            JSON.stringify({
                selectedSummaryModel: "qwen3.5:4b",
                summaryModelDownloaded: false,
            })
        );
    });

    test("explicit selected model wins over a different recommendation", () => {
        expect(
            JSON.stringify(
                resolveOnboardingSummaryModelStatus({
                    selectedModel: "gemma3:1b",
                    recommendedModel: "qwen3.5:4b",
                    selectedModelReady: true,
                })
            )
        ).toBe(
            JSON.stringify({
                selectedSummaryModel: "gemma3:1b",
                summaryModelDownloaded: true,
            })
        );
    });

    test("recommended Qwen is selected when none is chosen", () => {
        expect(
            JSON.stringify(
                resolveOnboardingSummaryModelStatus({
                    selectedModel: "",
                    recommendedModel: "qwen3.5:2b",
                    selectedModelReady: true,
                })
            )
        ).toBe(
            JSON.stringify({
                selectedSummaryModel: "qwen3.5:2b",
                summaryModelDownloaded: true,
            })
        );
    });

    test("getSummaryModelSizeMb returns model sizes in MB", () => {
        expect(getSummaryModelSizeMb("qwen3.5:2b")).toBe(1221);
        expect(getSummaryModelSizeMb("qwen3.5:4b")).toBe(2614);
        expect(getSummaryModelSizeMb("gemma3:1b")).toBe(1019);
        expect(getSummaryModelSizeMb("unknown:model")).toBe(0);
    });

    test("getSummaryModelSizeLabel formats sizes for UI", () => {
        expect(getSummaryModelSizeLabel("qwen3.5:2b")).toBe("~1.2 GiB");
        expect(getSummaryModelSizeLabel("qwen3.5:4b")).toBe("~2.6 GiB");
        expect(getSummaryModelSizeLabel("unknown:model")).toBe("");
    });

    test("getDownloadTotalMb prefers existing local size", () => {
        expect(getDownloadTotalMb(0, "qwen3.5:4b")).toBe(2614);
        expect(getDownloadTotalMb(undefined, "qwen3.5:2b")).toBe(1221);
        expect(getDownloadTotalMb(512, "qwen3.5:4b")).toBe(512);
    });
});
