export function isSummaryInProgress(status: unknown): boolean {
    const normalized = typeof status === "string" ? status.trim().toLowerCase() : "";

    return (
        normalized === "pending" ||
        normalized === "processing" ||
        normalized === "summarizing" ||
        normalized === "regenerating"
    );
}

export function canAutoGenerateSummary({
    initialSummaryLoaded,
    isSummaryProcessing,
    hasCheckedAutoGen,
    hasTranscripts,
}: {
    initialSummaryLoaded: boolean;
    isSummaryProcessing: boolean;
    hasCheckedAutoGen: boolean;
    hasTranscripts: boolean;
}): boolean {
    return (
        initialSummaryLoaded &&
        hasTranscripts &&
        !isSummaryProcessing &&
        !hasCheckedAutoGen
    );
}
