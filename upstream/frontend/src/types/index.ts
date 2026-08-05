export interface Message {
    id: string;
    content: string;
    timestamp: string;
}

export interface Transcript {
    id: string;
    text: string;
    timestamp: string; // Wall-clock time (e.g., "14:30:05")
    sequence_id?: number;
    chunk_start_time?: number; // Legacy field
    is_partial?: boolean;
    confidence?: number;
    // NEW: Recording-relative timestamps for playback sync
    audio_start_time?: number; // Seconds from recording start (e.g., 125.3)
    audio_end_time?: number; // Seconds from recording start (e.g., 128.6)
    duration?: number; // Segment duration in seconds (e.g., 3.3)
}

export interface TranscriptUpdate {
    text: string;
    timestamp: string; // Wall-clock time for reference
    source: string;
    sequence_id: number;
    chunk_start_time: number; // Legacy field
    is_partial: boolean;
    confidence: number;
    // NEW: Recording-relative timestamps for playback sync
    audio_start_time: number; // Seconds from recording start
    audio_end_time: number; // Seconds from recording start
    duration: number; // Segment duration in seconds
}

export interface Block {
    id: string;
    type: string;
    content: string;
    color: string;
}

export interface Section {
    title: string;
    blocks: Block[];
}

export interface Summary {
    [key: string]: Section;
}

export interface ApiResponse {
    message: string;
    num_chunks: number;
    data: unknown[];
}

export interface SummaryResponse {
    status: string;
    summary: Summary;
    raw_summary?: string;
    usage?: {
        prompt_tokens: number;
        completion_tokens: number;
        total_tokens: number;
    };
}

// BlockNote-specific types
export type SummaryFormat = "legacy" | "markdown" | "blocknote";

export interface BlockNoteBlock {
    id: string;
    type: string;
    props?: Record<string, unknown>;
    content?: unknown[];
    children?: BlockNoteBlock[];
}

export interface SummaryDataResponse {
    markdown?: string;
    summary_json?: BlockNoteBlock[];
    // Legacy format fields
    MeetingName?: string;
    _section_order?: string[];
    [key: string]: unknown; // For legacy section data
}

// Status payload returned by `api_get_summary`. polled by SidebarProvider's
// startSummaryPolling and read directly by useSummaryGeneration when restoring
// after cancel/regen failure.
export interface SummaryStatusResponse {
    status: "idle" | "processing" | "summarizing" | "regenerating" | "completed" | "error" | "failed" | "cancelled";
    template_id?: string;
    start?: string | null;
    updated_at?: string | null;
    data?: SummaryDataResponse | null;
    error?: string;
    meetingName?: string;
}

// Pagination types for optimized transcript loading
export interface MeetingMetadata {
    id: string;
    title: string;
    created_at: string;
    updated_at: string;
    folder_id?: string | null;
    folder_path?: string;
}

// Lightweight summary descriptor for the per-meeting template dropdown list.
// Mirrors Rust `MeetingSummaryInfo` in `summary/commands.rs` (no serde
// rename_all → field names arrive as snake_case over Tauri IPC).
export interface MeetingSummaryInfo {
    template_id: string;
    status:
        | "idle"
        | "processing"
        | "summarizing"
        | "regenerating"
        | "completed"
        | "error"
        | "failed"
        | "cancelled";
    updated_at: string;
    generation?: string | null;
    error?: string | null;
}

export interface SummaryRevision {
    templateId: string;
    startTime: string | null;
    updatedAt: string;
}

// Canonical meeting record. Mirrors the Rust MeetingModel and adds
// in-memory only fields (transcripts) used by the meeting-details pages.
export interface Meeting {
    id: string;
    title: string;
    created_at: string;
    updated_at?: string;
    folder_path?: string | null;
    folder_id?: string | null;
    // Loaded on demand by the meeting-details views
    transcripts?: Transcript[];
}

export interface MeetingFolder {
    id: string;
    name: string;
    parent_id: string | null;
    created_at: string;
}

export interface PaginatedTranscriptsResponse {
    transcripts: Transcript[];
    total_count: number;
    has_more: boolean;
}

// Transcript segment data for virtualized display
export interface TranscriptSegmentData {
    id: string;
    timestamp: number; // audio_start_time in seconds
    endTime?: number; // audio_end_time in seconds
    text: string;
    confidence?: number;
}

// FTS5 full-text search results. Mirrors Rust `FtsSearchResult` in `fts.rs`;
// serde renames map snake_case → camelCase over Tauri IPC.
export interface FtsSearchResult {
    meeting_id: string;
    meeting_title: string;
    chunkType: string;
    chunkId: string;
    snippet: string;
    speaker?: string;
    timestampLabel?: string;
    folderId?: string;
    folderName: string;
    rank: number;
}

// Chat with Meetings types
export interface ChatMessage {
    role: "user" | "assistant";
    content: string;
    sources?: ChatSource[];
}

export interface ChatSource {
    meetingId: string;
    meetingTitle: string;
    chunkType: string;
    snippet: string;
    folderName: string;
}

export interface ChatResponse {
    answer: string;
    sources: ChatSource[];
}
