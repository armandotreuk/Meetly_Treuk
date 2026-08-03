import { useCallback } from "react";
import { logger } from "@/lib/logger";

import { invoke as invokeTauri } from "@tauri-apps/api/core";
import { Meeting } from "@/types";
import { toast } from "sonner";

interface UseMeetingOperationsProps {
    meeting: Meeting;
}

export function useMeetingOperations({ meeting }: UseMeetingOperationsProps) {
    // Open meeting folder in file explorer
    const handleOpenMeetingFolder = useCallback(async () => {
        try {
            await invokeTauri("open_meeting_folder", { meetingId: meeting.id });
        } catch (error) {
            logger.error("Failed to open meeting folder:", error);
            toast.error((error as string) || "Failed to open recording folder");
        }
    }, [meeting.id]);

    return {
        handleOpenMeetingFolder,
    };
}
