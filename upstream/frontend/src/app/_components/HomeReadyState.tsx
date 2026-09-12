import React from "react";
import { CalendarClock, Mic, ShieldCheck, Upload } from "lucide-react";
import type { CurrentMeeting } from "@/components/Sidebar/SidebarProvider";
import { formatMeetingDate } from "@/lib/utils";
import { t } from "@/lib/i18n";

interface HomeReadyStateProps {
    meetings: CurrentMeeting[];
    modelName: string;
    hasMicrophone: boolean;
    isCheckingMicrophone: boolean;
    canImportAudio: boolean;
    onStartRecording: () => void;
    onCheckMicrophone: () => void;
    onImportAudio: () => void;
    onOpenMeeting: (meetingId: string) => void;
}

export function HomeReadyState({
    meetings,
    modelName,
    hasMicrophone,
    isCheckingMicrophone,
    canImportAudio,
    onStartRecording,
    onCheckMicrophone,
    onImportAudio,
    onOpenMeeting,
}: HomeReadyStateProps) {
    const recentMeetings = [...meetings]
        .sort(
            (left, right) =>
                new Date(right.created_at ?? 0).getTime() - new Date(left.created_at ?? 0).getTime()
        )
        .slice(0, 4);

    return (
        <section
            className="mx-auto mt-4 w-full max-w-3xl space-y-6 px-1 pb-28"
            aria-labelledby="home-ready-title"
        >
            <div className="rounded-2xl border border-blue-100 bg-gradient-to-br from-blue-50 to-white p-6 shadow-sm md:p-8">
                <div className="flex flex-col gap-5 sm:flex-row sm:items-start sm:justify-between">
                    <div className="max-w-xl">
                        <div className="mb-3 inline-flex items-center gap-2 rounded-full bg-white px-3 py-1 text-xs font-medium text-blue-700 shadow-sm">
                            <ShieldCheck className="h-3.5 w-3.5" aria-hidden="true" />
                            {t("home.privacy")}
                        </div>
                        <h1
                            id="home-ready-title"
                            className="text-2xl font-semibold tracking-tight text-gray-900"
                        >
                            {t("home.title")}
                        </h1>
                        <p className="mt-2 text-sm leading-6 text-gray-600">
                            {t("home.description")}
                        </p>
                    </div>
                    <div className="rounded-xl border border-blue-100 bg-white px-3 py-2 text-xs text-gray-600 shadow-sm">
                        <span className="block font-medium text-gray-800">
                            {t("home.transcriptionProfile")}
                        </span>
                        <span className="mt-0.5 block break-all">{modelName}</span>
                    </div>
                </div>
                <div className="mt-6 flex flex-wrap gap-3">
                    <button
                        type="button"
                        onClick={hasMicrophone ? onStartRecording : onCheckMicrophone}
                        disabled={isCheckingMicrophone}
                        className="inline-flex items-center gap-2 rounded-lg bg-blue-600 px-4 py-2.5 text-sm font-medium text-white shadow-sm transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:bg-blue-300"
                    >
                        <Mic className="h-4 w-4" aria-hidden="true" />
                        {hasMicrophone ? t("home.startRecording") : t("home.checkAudio")}
                    </button>
                    {canImportAudio && (
                        <button
                            type="button"
                            onClick={onImportAudio}
                            className="inline-flex items-center gap-2 rounded-lg border border-gray-300 bg-white px-4 py-2.5 text-sm font-medium text-gray-700 transition-colors hover:border-blue-300 hover:text-blue-700"
                        >
                            <Upload className="h-4 w-4" aria-hidden="true" />
                            {t("home.importAudio")}
                        </button>
                    )}
                </div>
            </div>

            <section
                className="rounded-2xl border border-gray-200 bg-white p-5 shadow-sm"
                aria-labelledby="recent-meetings-title"
            >
                <div className="mb-3 flex items-center gap-2">
                    <CalendarClock className="h-4 w-4 text-blue-600" aria-hidden="true" />
                    <h2 id="recent-meetings-title" className="text-sm font-semibold text-gray-900">
                        {t("home.recentMeetings")}
                    </h2>
                </div>
                {recentMeetings.length === 0 ? (
                    <p className="text-sm text-gray-500">{t("home.noRecentMeetings")}</p>
                ) : (
                    <ul className="divide-y divide-gray-100">
                        {recentMeetings.map((meeting) => (
                            <li key={meeting.id}>
                                <button
                                    type="button"
                                    onClick={() => onOpenMeeting(meeting.id)}
                                    className="flex w-full items-center justify-between gap-3 py-3 text-left transition-colors hover:text-blue-700"
                                >
                                    <span className="min-w-0 truncate text-sm font-medium text-gray-800">
                                        {meeting.title}
                                    </span>
                                    <time
                                        className="shrink-0 text-xs text-gray-500"
                                        dateTime={meeting.created_at}
                                    >
                                        {formatMeetingDate(meeting.created_at, "short")}
                                    </time>
                                </button>
                            </li>
                        ))}
                    </ul>
                )}
            </section>
        </section>
    );
}
