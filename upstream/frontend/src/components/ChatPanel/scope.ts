import type { ChatScope } from "@/types";

export async function createSearchSnapshotScope(meetings: { id: string }[]): Promise<ChatScope | null> {
    const result_ids = [...new Set(meetings.map((meeting) => meeting.id))].slice(0, 100);
    if (!result_ids.length) return null;
    const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify(result_ids)));
    const key = [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
    return { kind: "search_snapshot", key, data: { result_ids } };
}
