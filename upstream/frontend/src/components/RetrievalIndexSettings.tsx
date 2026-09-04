"use client";

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertCircle, Database, Loader2, RefreshCw, Trash2 } from "lucide-react";
import { toast } from "sonner";

import { t } from "@/lib/i18n";
import { logger } from "@/lib/logger";
import { Alert, AlertDescription, AlertTitle } from "./ui/alert";
import { Button } from "./ui/button";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
} from "./ui/dialog";
import { Switch } from "./ui/switch";

interface RetrievalGenerationStatus {
    generation_id: string;
    model_id: string;
    state: string;
    document_count: number;
    tracked_meetings: number;
    current_meetings: number;
    retry_meetings: number;
    failed_meetings: number;
    canonical_change_id: number | null;
    published_change_id: number | null;
}

export interface RetrievalStatusReport {
    backend: string;
    indexed_scope: string;
    semantic_state: string;
    serving_state: string;
    shadow_state: string | null;
    operation_active: boolean;
    force_lexical_retrieval: boolean;
    model_load_failure: string | null;
    paused: boolean;
    active_generation_id: string | null;
    active_model_id: string | null;
    building_generations: RetrievalGenerationStatus[];
    document_count: number;
    tracked_meetings: number;
    current_meetings: number;
    retry_meetings: number;
    failed_meetings: number;
    canonical_change_id: number | null;
    published_change_id: number | null;
    activation_blockers: string[];
    resident_index_bytes: number;
    resident_process_bytes: number | null;
    activation_ram_ceiling_bytes: number;
    activation_ram_scope: string;
    derived_disk_bytes: number | null;
    derived_disk_is_estimate: boolean;
    derived_disk_measurement_status: string;
    derived_disk_estimate_bytes: number | null;
    derived_disk_gate_input_bytes: number | null;
    wal_file_size_bytes: number | null;
    derived_disk_steady_target_bytes: number;
    derived_disk_activation_limit_bytes: number;
    model: {
        embedding_name: string;
        embedding_revision: string;
        embedding_attribution: string;
        embedding_license: string;
        embedding_license_url: string;
        reranker_name: string;
        reranker_revision: string;
        reranker_attribution: string;
        reranker_license: string;
        reranker_license_url: string;
    };
    model_artifact_state: string;
}

type RetrievalAction = "force" | "pause" | "rebuild" | "retry" | "clear" | null;
type RetrievalConfirmation = "rebuild" | "clear" | null;

function formatBytes(bytes: number | null): string {
    if (bytes === null) return t("settings.retrieval.sizeUnavailable");
    if (bytes < 1024) return `${bytes} B`;
    const units = ["KiB", "MiB", "GiB"];
    let value = bytes;
    let unit = "B";
    for (const nextUnit of units) {
        value /= 1024;
        unit = nextUnit;
        if (value < 1024 || nextUnit === units[units.length - 1]) break;
    }
    return `${value.toFixed(1)} ${unit}`;
}

function stateLabel(state: string): string {
    const labels: Record<string, string> = {
        queued: t("settings.retrieval.state.queued"),
        ready: t("settings.retrieval.state.ready"),
        building: t("settings.retrieval.state.building"),
        complete: t("settings.retrieval.state.complete"),
        loading: t("settings.retrieval.state.loading"),
        catching_up: t("settings.retrieval.state.catchingUp"),
        paused: t("settings.retrieval.state.paused"),
        unavailable: t("settings.retrieval.state.unavailable"),
        model_unavailable: t("settings.retrieval.state.modelUnavailable"),
        model_mismatch: t("settings.retrieval.state.modelMismatch"),
        transitioning: t("settings.retrieval.state.transitioning"),
        failed: t("settings.retrieval.state.failed"),
    };
    return labels[state] ?? t("settings.retrieval.state.unavailable");
}

function artifactLabel(state: string): string {
    if (state === "verified") return t("settings.retrieval.artifacts.verified");
    if (state === "unavailable") return t("settings.retrieval.artifacts.unavailable");
    return t("settings.retrieval.artifacts.pending");
}

function scopeLabel(scope: string): string {
    return scope === "all_saved_meetings"
        ? t("settings.retrieval.scope.allSavedMeetings")
        : t("settings.retrieval.scope.unknown");
}

export function RetrievalIndexSettings() {
    const [status, setStatus] = useState<RetrievalStatusReport | null>(null);
    const [statusLoading, setStatusLoading] = useState(true);
    const [statusError, setStatusError] = useState(false);
    const [action, setAction] = useState<RetrievalAction>(null);
    const [confirmation, setConfirmation] = useState<RetrievalConfirmation>(null);
    const statusRequestRef = useRef(0);
    const mountedRef = useRef(false);

    const loadStatus = useCallback(async (showLoading = false) => {
        if (!mountedRef.current) return;
        const requestId = ++statusRequestRef.current;
        if (showLoading) setStatusLoading(true);
        try {
            const next = await invoke<RetrievalStatusReport>("retrieval_index_status");
            if (!mountedRef.current || requestId !== statusRequestRef.current) return;
            setStatus(next);
            setStatusError(false);
        } catch (error) {
            if (!mountedRef.current || requestId !== statusRequestRef.current) return;
            logger.error("Failed to load retrieval index status:", error);
            setStatusError(true);
        } finally {
            if (mountedRef.current && requestId === statusRequestRef.current) {
                setStatusLoading(false);
            }
        }
    }, []);

    useEffect(() => {
        mountedRef.current = true;
        void loadStatus(true);
        const interval = window.setInterval(() => void loadStatus(), 2000);
        return () => {
            mountedRef.current = false;
            statusRequestRef.current += 1;
            window.clearInterval(interval);
        };
    }, [loadStatus]);

    const runAction = useCallback(
        async (nextAction: Exclude<RetrievalAction, null>, operation: () => Promise<unknown>) => {
            setAction(nextAction);
            try {
                await operation();
                await loadStatus();
                toast.success(t("settings.retrieval.actionComplete"));
            } catch (error) {
                logger.error(`Retrieval index action failed (${nextAction}):`, error);
                toast.error(t("settings.retrieval.actionFailed"));
            } finally {
                setAction(null);
            }
        },
        [loadStatus]
    );

    const shadowGeneration = useMemo(() => {
        if (!status) return null;
        return status.building_generations[0] ?? null;
    }, [status]);
    const shadowState = status?.shadow_state ?? null;
    const operationActive = status?.operation_active ?? false;
    const controlsDisabled = action !== null || operationActive;
    const progressSource = shadowGeneration ?? status;
    const progress = progressSource
        ? progressSource.tracked_meetings > 0
            ? Math.min(
                  100,
                  Math.round(
                      (progressSource.current_meetings / progressSource.tracked_meetings) * 100
                  )
              )
            : shadowState === "complete" || status?.semantic_state === "ready"
              ? 100
              : 0
        : 0;
    const diskBytes = status?.derived_disk_bytes ?? status?.derived_disk_estimate_bytes ?? null;
    const diskEnvelopeState =
        status?.derived_disk_gate_input_bytes === null ||
        status?.derived_disk_gate_input_bytes === undefined
            ? "unavailable"
            : status.derived_disk_gate_input_bytes >= status.derived_disk_activation_limit_bytes
              ? "exceeded"
              : "within";
    const hasIndexError =
        (status?.model_load_failure !== null && status?.model_load_failure !== undefined) ||
        (status?.failed_meetings ?? 0) > 0 ||
        (status?.retry_meetings ?? 0) > 0 ||
        (status?.activation_blockers.length ?? 0) > 0 ||
        status?.building_generations.some(
            (generation) =>
                generation.state === "failed" ||
                generation.failed_meetings > 0 ||
                generation.retry_meetings > 0
        ) === true;

    if (statusLoading && !status) {
        return (
            <section
                className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm"
                aria-busy="true"
            >
                <div className="flex items-center gap-2 text-sm text-gray-600">
                    <Loader2 className="h-4 w-4 animate-spin" />
                    {t("settings.retrieval.loading")}
                </div>
            </section>
        );
    }

    return (
        <section
            className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm"
            aria-labelledby="retrieval-index-heading"
            aria-busy={statusLoading}
        >
            <div className="flex items-start justify-between gap-4">
                <div>
                    <div className="mb-2 flex items-center gap-2">
                        <Database className="h-5 w-5 text-gray-700" />
                        <h3
                            id="retrieval-index-heading"
                            className="text-lg font-semibold text-gray-900"
                        >
                            {t("settings.retrieval.title")}
                        </h3>
                    </div>
                    <p className="text-sm text-gray-600">{t("settings.retrieval.description")}</p>
                </div>
                <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void loadStatus(true)}
                    disabled={statusLoading || action !== null}
                    aria-label={t("settings.retrieval.refreshAria")}
                >
                    <RefreshCw className={statusLoading ? "animate-spin" : undefined} />
                    {t("settings.retrieval.refresh")}
                </Button>
            </div>

            {statusError || !status ? (
                <Alert variant="destructive" className="mt-5">
                    <AlertCircle />
                    <AlertTitle>{t("settings.retrieval.statusFailedTitle")}</AlertTitle>
                    <AlertDescription className="flex flex-wrap items-center justify-between gap-3">
                        <span>{t("settings.retrieval.statusFailed")}</span>
                        <Button
                            variant="outline"
                            size="sm"
                            onClick={() => void loadStatus(true)}
                            disabled={statusLoading}
                        >
                            {t("settings.retrieval.retry")}
                        </Button>
                    </AlertDescription>
                </Alert>
            ) : (
                <>
                    <div
                        className="mt-5 rounded-md bg-gray-50 p-4"
                        role="status"
                        aria-live="polite"
                    >
                        <div className="flex flex-wrap items-center justify-between gap-2">
                            <span className="font-medium text-gray-900">
                                {t("settings.retrieval.statusLabel")}
                            </span>
                            <span className="font-semibold text-blue-700">
                                {status.force_lexical_retrieval
                                    ? t("settings.retrieval.state.lexicalOnly")
                                    : stateLabel(shadowState ?? status.semantic_state)}
                            </span>
                        </div>
                        <p className="mt-1 text-sm text-gray-600">
                            {status.force_lexical_retrieval
                                ? t("settings.retrieval.state.lexicalOnlyDescription")
                                : shadowState
                                  ? t("settings.retrieval.shadowDescription", {
                                        state: stateLabel(shadowState),
                                    })
                                  : stateLabel(status.semantic_state)}
                        </p>
                        {shadowState && status.force_lexical_retrieval && (
                            <p className="mt-1 text-xs text-gray-500">
                                {t("settings.retrieval.shadowDescription", {
                                    state: stateLabel(shadowState),
                                })}
                            </p>
                        )}
                        {shadowState &&
                            status.active_generation_id &&
                            !status.force_lexical_retrieval && (
                                <p className="mt-1 text-xs text-gray-500">
                                    {t("settings.retrieval.shadowServing", {
                                        state: stateLabel(status.serving_state),
                                    })}
                                </p>
                            )}
                    </div>

                    <div className="mt-5 grid gap-4 sm:grid-cols-2">
                        <div className="rounded-md border border-gray-200 p-4">
                            <div className="flex items-center justify-between gap-2 text-sm">
                                <span className="font-medium text-gray-900">
                                    {t("settings.retrieval.progressLabel")}
                                </span>
                                <span className="text-gray-600">
                                    {progressSource?.current_meetings ?? 0} /{" "}
                                    {progressSource?.tracked_meetings ?? 0}
                                </span>
                            </div>
                            <progress
                                className="mt-3 h-2 w-full accent-blue-600"
                                max={100}
                                value={progress}
                                aria-label={t("settings.retrieval.progressAria", { progress })}
                            />
                            <p className="mt-2 text-xs text-gray-500">
                                {t("settings.retrieval.documents", {
                                    count: progressSource?.document_count ?? status.document_count,
                                })}
                            </p>
                            <p className="mt-1 text-xs text-gray-500">
                                {t("settings.retrieval.scope", {
                                    scope: scopeLabel(status.indexed_scope),
                                })}
                            </p>
                        </div>

                        <div className="rounded-md border border-gray-200 p-4">
                            <span className="font-medium text-gray-900">
                                {t("settings.retrieval.localSize")}
                            </span>
                            <p className="mt-2 text-sm text-gray-600">
                                {diskBytes === null
                                    ? t("settings.retrieval.sizeUnavailable")
                                    : t("settings.retrieval.sizeValue", {
                                          size: formatBytes(diskBytes),
                                          target: formatBytes(
                                              status.derived_disk_steady_target_bytes
                                          ),
                                      })}
                            </p>
                            <p className="mt-1 text-xs text-gray-500">
                                {status.derived_disk_is_estimate
                                    ? t("settings.retrieval.sizeEstimate")
                                    : t("settings.retrieval.sizeExact")}
                            </p>
                            <p className="mt-1 text-xs text-gray-500">
                                {diskEnvelopeState === "exceeded"
                                    ? t("settings.retrieval.diskExceeded", {
                                          limit: formatBytes(
                                              status.derived_disk_activation_limit_bytes
                                          ),
                                      })
                                    : diskEnvelopeState === "within"
                                      ? t("settings.retrieval.diskWithinEnvelope", {
                                            limit: formatBytes(
                                                status.derived_disk_activation_limit_bytes
                                            ),
                                        })
                                      : t("settings.retrieval.diskEnvelopeUnavailable")}
                            </p>
                            <p className="mt-3 font-medium text-gray-900">
                                {t("settings.retrieval.ram")}
                            </p>
                            <p className="mt-1 text-sm text-gray-600">
                                {status.resident_process_bytes === null
                                    ? t("settings.retrieval.ramUnavailable")
                                    : t("settings.retrieval.ramValue", {
                                          value: formatBytes(status.resident_process_bytes),
                                          ceiling: formatBytes(status.activation_ram_ceiling_bytes),
                                      })}
                            </p>
                            <p className="mt-1 text-xs text-gray-500">
                                {t("settings.retrieval.ramScope", {
                                    scope: status.activation_ram_scope,
                                })}
                            </p>
                        </div>
                    </div>

                    <div className="mt-5 rounded-md border border-gray-200 p-4">
                        <div className="flex flex-wrap items-center justify-between gap-3">
                            <div>
                                <p className="font-medium text-gray-900">
                                    {t("settings.retrieval.pauseTitle")}
                                </p>
                                <p className="mt-1 text-sm text-gray-600">
                                    {t("settings.retrieval.pauseDescription")}
                                </p>
                            </div>
                            <Button
                                variant="outline"
                                onClick={() =>
                                    void runAction("pause", () =>
                                        invoke("retrieval_set_index_paused", {
                                            paused: !status.paused,
                                        })
                                    )
                                }
                                disabled={action !== null}
                            >
                                {action === "pause" && <Loader2 className="animate-spin" />}
                                {status.paused
                                    ? t("settings.retrieval.resume")
                                    : t("settings.retrieval.pause")}
                            </Button>
                        </div>
                    </div>

                    <div className="mt-5 rounded-md border border-gray-200 p-4">
                        <div className="flex items-center justify-between gap-4">
                            <div>
                                <p className="font-medium text-gray-900">
                                    {t("settings.retrieval.forceLexicalTitle")}
                                </p>
                                <p className="mt-1 text-sm text-gray-600">
                                    {t("settings.retrieval.forceLexicalDescription")}
                                </p>
                            </div>
                            <Switch
                                checked={status.force_lexical_retrieval}
                                onCheckedChange={(enabled) =>
                                    void runAction("force", () =>
                                        invoke("api_chat_set_force_lexical_retrieval", { enabled })
                                    )
                                }
                                disabled={action !== null}
                                aria-label={t("settings.retrieval.forceLexicalAria")}
                            />
                        </div>
                    </div>

                    <div className="mt-5 rounded-md border border-gray-200 p-4">
                        <div className="flex flex-wrap items-center justify-between gap-3">
                            <div>
                                <p className="font-medium text-gray-900">
                                    {t("settings.retrieval.rebuildTitle")}
                                </p>
                                <p className="mt-1 text-sm text-gray-600">
                                    {t("settings.retrieval.rebuildDescription")}
                                </p>
                            </div>
                            <Button
                                onClick={() => setConfirmation("rebuild")}
                                disabled={controlsDisabled}
                            >
                                {action === "rebuild" && <Loader2 className="animate-spin" />}
                                {t("settings.retrieval.rebuild")}
                            </Button>
                        </div>
                    </div>

                    {hasIndexError && (
                        <Alert variant="destructive" className="mt-5">
                            <AlertCircle />
                            <AlertTitle>{t("settings.retrieval.errorTitle")}</AlertTitle>
                            <AlertDescription className="flex flex-wrap items-center justify-between gap-3">
                                <span>{t("settings.retrieval.errorDescription")}</span>
                                <Button
                                    variant="outline"
                                    size="sm"
                                    onClick={() =>
                                        void runAction("retry", () =>
                                            shadowGeneration?.state === "failed"
                                                ? invoke("retrieval_retry_rebuild", {
                                                      generationId: shadowGeneration.generation_id,
                                                  })
                                                : invoke("retrieval_rebuild_index")
                                        )
                                    }
                                    disabled={controlsDisabled}
                                >
                                    {action === "retry" && <Loader2 className="animate-spin" />}
                                    {t("settings.retrieval.retry")}
                                </Button>
                            </AlertDescription>
                        </Alert>
                    )}

                    <div className="mt-5 rounded-md border border-gray-200 p-4">
                        <p className="font-medium text-gray-900">
                            {t("settings.retrieval.modelTitle")}
                        </p>
                        <div className="mt-3 grid gap-4 text-sm sm:grid-cols-2">
                            <div>
                                <p className="font-medium text-gray-700">
                                    {t("settings.retrieval.embedding")}
                                </p>
                                <p className="mt-1 break-all text-gray-900">
                                    {status.model.embedding_name}
                                </p>
                                <p className="mt-1 break-all text-xs text-gray-500">
                                    {t("settings.retrieval.revision", {
                                        revision: status.model.embedding_revision,
                                    })}
                                </p>
                                <p className="mt-2 text-xs text-gray-500">
                                    {status.model.embedding_attribution}
                                </p>
                                <a
                                    className="mt-2 inline-block text-blue-700 underline"
                                    href={status.model.embedding_license_url}
                                    target="_blank"
                                    rel="noreferrer"
                                >
                                    {t("settings.retrieval.license", {
                                        license: status.model.embedding_license,
                                    })}
                                </a>
                            </div>
                            <div>
                                <p className="font-medium text-gray-700">
                                    {t("settings.retrieval.reranker")}
                                </p>
                                <p className="mt-1 break-all text-gray-900">
                                    {status.model.reranker_name}
                                </p>
                                <p className="mt-1 break-all text-xs text-gray-500">
                                    {t("settings.retrieval.revision", {
                                        revision: status.model.reranker_revision,
                                    })}
                                </p>
                                <p className="mt-2 text-xs text-gray-500">
                                    {status.model.reranker_attribution}
                                </p>
                                <a
                                    className="mt-2 inline-block text-blue-700 underline"
                                    href={status.model.reranker_license_url}
                                    target="_blank"
                                    rel="noreferrer"
                                >
                                    {t("settings.retrieval.license", {
                                        license: status.model.reranker_license,
                                    })}
                                </a>
                            </div>
                        </div>
                        <p className="mt-3 text-xs text-gray-500">
                            {t("settings.retrieval.artifacts.status", {
                                state: artifactLabel(status.model_artifact_state),
                            })}
                        </p>
                    </div>

                    <div className="mt-5 flex justify-end">
                        <Button
                            variant="destructive"
                            onClick={() => setConfirmation("clear")}
                            disabled={controlsDisabled}
                        >
                            <Trash2 />
                            {t("settings.retrieval.clear")}
                        </Button>
                    </div>
                </>
            )}

            <Dialog
                open={confirmation !== null}
                onOpenChange={(open) => {
                    if (!open) setConfirmation(null);
                }}
            >
                <DialogContent>
                    <DialogHeader>
                        <DialogTitle>
                            {confirmation === "rebuild"
                                ? t("settings.retrieval.rebuildConfirmTitle")
                                : t("settings.retrieval.clearTitle")}
                        </DialogTitle>
                        <DialogDescription>
                            {confirmation === "rebuild"
                                ? t("settings.retrieval.rebuildConfirmDescription")
                                : t("settings.retrieval.clearDescription")}
                        </DialogDescription>
                    </DialogHeader>
                    <DialogFooter>
                        <Button
                            variant="outline"
                            onClick={() => setConfirmation(null)}
                            disabled={action !== null}
                        >
                            {t("settings.retrieval.cancel")}
                        </Button>
                        <Button
                            variant="destructive"
                            onClick={() => {
                                if (controlsDisabled) {
                                    setConfirmation(null);
                                    return;
                                }
                                const confirmed = confirmation;
                                setConfirmation(null);
                                if (confirmed === "rebuild") {
                                    void runAction("rebuild", () =>
                                        invoke("retrieval_rebuild_index")
                                    );
                                } else if (confirmed === "clear") {
                                    void runAction("clear", () => invoke("retrieval_clear_index"));
                                }
                            }}
                            disabled={controlsDisabled || confirmation === null}
                        >
                            {action === confirmation && <Loader2 className="animate-spin" />}
                            {confirmation === "rebuild"
                                ? t("settings.retrieval.rebuildConfirm")
                                : t("settings.retrieval.clearConfirm")}
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>
        </section>
    );
}
