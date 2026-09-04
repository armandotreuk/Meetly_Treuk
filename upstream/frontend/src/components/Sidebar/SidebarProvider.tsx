'use client';

import React, { createContext, useContext, useState, useEffect } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import Analytics from '@/lib/analytics';
import { invoke } from '@tauri-apps/api/core';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { usePanelResize } from '@/hooks/usePanelResize';
import type { MeetingFolder, HybridSearchResponse } from '@/types';
import { buildSummaryCancelArgs } from '@/lib/summary-command-args';
import { shouldApplySummaryPollResult, summaryPollKey } from '@/lib/summary-polling';
import {
  createSidebarSearchController,
  type SidebarSearchController,
  type SidebarSearchErrorCode,
  type SidebarSearchInvoke,
  type SidebarSearchNotice,
  type SidebarSearchState,
} from '@/lib/sidebar-search';


interface SidebarItem {
  id: string;
  title: string;
  type: 'folder' | 'file';
  children?: SidebarItem[];
}

export interface CurrentMeeting {
  id: string;
  title: string;
  created_at?: string;
  folder_id?: string | null;
  has_notes?: boolean;
}

interface SidebarContextType {
  currentMeeting: CurrentMeeting | null;
  setCurrentMeeting: (meeting: CurrentMeeting | null) => void;
  sidebarItems: SidebarItem[];
  isCollapsed: boolean;
  toggleCollapse: () => void;
  meetings: CurrentMeeting[];
  setMeetings: (meetings: CurrentMeeting[]) => void;
  isMeetingActive: boolean;
  setIsMeetingActive: (active: boolean) => void;
  handleRecordingToggle: () => void;
  searchTranscripts: (query: string, folderId?: string | null) => Promise<void>;
  cancelSidebarSearch: () => void;
  searchResponse: HybridSearchResponse | null;
  searchNotice: SidebarSearchNotice | null;
  searchError: SidebarSearchErrorCode | null;
  searchPhase: SidebarSearchState['phase'];
  isSearching: boolean;
  setServerAddress: (address: string) => void;
  serverAddress: string;
  transcriptServerAddress: string;
  setTranscriptServerAddress: (address: string) => void;
  // Summary polling management
  activeSummaryPolls: Map<string, NodeJS.Timeout>;
  startSummaryPolling: (meetingId: string, processId: string, templateId: string, generation: string, onUpdate: (result: any) => void) => void;
  stopSummaryPolling: (meetingId: string, templateId?: string, generation?: string) => void;
  // Refetch meetings from backend
  refetchMeetings: () => Promise<void>;
  // Meeting folders (pastas lógicas). Actions throw on backend error so
  // callers can surface the message (e.g. cycle rejection) in a toast.
  folders: MeetingFolder[];
  refetchFolders: () => Promise<void>;
  createFolder: (name: string, parentId?: string | null) => Promise<MeetingFolder>;
  renameFolder: (id: string, name: string) => Promise<void>;
  moveFolder: (id: string, newParentId: string | null) => Promise<void>;
  deleteFolder: (id: string) => Promise<void>;
  moveMeetingToFolder: (meetingId: string, folderId: string | null) => Promise<void>;
  // Panel-resize (in-flight): sidebar width in px and active-drag flag
  sidebarWidth: number;
  sidebarDragging: boolean;
  resizeHandleProps: { onMouseDown: (e: React.MouseEvent) => void };

}

const SidebarContext = createContext<SidebarContextType | null>(null);

export const useSidebar = () => {
  const context = useContext(SidebarContext);
  if (!context) {
    throw new Error('useSidebar must be used within a SidebarProvider');
  }
  return context;
};

export interface SidebarProviderProps {
  children: React.ReactNode;
  searchInvoke?: SidebarSearchInvoke;
}

export function SidebarProvider({ children, searchInvoke }: SidebarProviderProps) {
  const [currentMeeting, setCurrentMeeting] = useState<CurrentMeeting | null>({ id: 'intro-call', title: '+ New Call' });
  const [isCollapsed, setIsCollapsed] = useState(true);
  const [meetings, setMeetings] = useState<CurrentMeeting[]>([]);
  const [sidebarItems, setSidebarItems] = useState<SidebarItem[]>([]);
  const [isMeetingActive, setIsMeetingActive] = useState(false);
  const [searchResponse, setSearchResponse] = useState<HybridSearchResponse | null>(null);
  const [searchNotice, setSearchNotice] = useState<SidebarSearchNotice | null>(null);
  const [searchError, setSearchError] = useState<SidebarSearchErrorCode | null>(null);
  const [searchPhase, setSearchPhase] = useState<SidebarSearchState['phase']>('idle');
  const [isSearching, setIsSearching] = useState(false);
  const [serverAddress, setServerAddress] = useState('');
  const [transcriptServerAddress, setTranscriptServerAddress] = useState('');
  const [activeSummaryPolls, setActiveSummaryPolls] = useState<Map<string, NodeJS.Timeout>>(new Map());
  const activeSummaryPollsRef = React.useRef<Map<string, NodeJS.Timeout>>(new Map());
  const activeSummaryPollKeysRef = React.useRef<Set<string>>(new Set());
  const [folders, setFolders] = useState<MeetingFolder[]>([]);

  // Use recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();

  const searchController = React.useMemo<SidebarSearchController>(
    () =>
      createSidebarSearchController({
        invoke: searchInvoke ?? ((command, args) => invoke(command, args)),
        onState: (state: SidebarSearchState) => {
          setIsSearching(state.phase === 'loading');
          setSearchResponse(state.response);
          setSearchNotice(state.notice);
          setSearchError(state.error);
          setSearchPhase(state.phase);
        },
      }),
    [searchInvoke]
  );

  // ponytail: sidebar resize — initial 256 matches `w-64`, min 200, max 40% of viewport.
  const { width: sidebarWidth, isDragging: sidebarDragging, handleProps: resizeHandleProps } = usePanelResize({
    initial: 256,
    min: 200,
    maxFraction: 0.4,
    side: 'left',
    storageKey: 'meedly:sidebar-width',
  });

  const pathname = usePathname();
  const router = useRouter();

  // Extract fetchMeetings as a reusable function
  const fetchMeetings = React.useCallback(async () => {
    if (serverAddress) {
      try {
        const meetings = await invoke('api_get_meetings') as Array<{ id: string, title: string, created_at?: string, folder_id?: string | null, has_notes?: boolean }>;
        const transformedMeetings = meetings.map((meeting: any) => ({
          id: meeting.id,
          title: meeting.title,
          created_at: meeting.created_at,
          folder_id: meeting.folder_id ?? null,
          has_notes: meeting.has_notes ?? false
        }));
        setMeetings(transformedMeetings);
        Analytics.trackBackendConnection(true);
      } catch (error) {
        console.error('Error fetching meetings:', error);
        setMeetings([]);
        Analytics.trackBackendConnection(false, error instanceof Error ? error.message : 'Unknown error');
      }
    }
  }, [serverAddress]);

  const fetchFolders = React.useCallback(async () => {
    if (!serverAddress) return;
    try {
      const folders = await invoke('api_get_folders') as MeetingFolder[];
      setFolders(folders);
    } catch (error) {
      console.error('Error fetching folders:', error);
      setFolders([]);
    }
  }, [serverAddress]);

  useEffect(() => {
    fetchMeetings();
    fetchFolders();
  }, [serverAddress, fetchMeetings, fetchFolders]);

  // Folder actions: optimistic local state, backend error propagates to caller.
  const createFolder = React.useCallback(async (name: string, parentId: string | null = null) => {
    const folder = await invoke('api_create_folder', { name, parentId }) as MeetingFolder;
    setFolders(prev => [...prev, folder]);
    return folder;
  }, []);

  const renameFolder = React.useCallback(async (id: string, name: string) => {
    await invoke('api_rename_folder', { id, name });
    setFolders(prev => prev.map(f => (f.id === id ? { ...f, name } : f)));
  }, []);

  const moveFolder = React.useCallback(async (id: string, newParentId: string | null) => {
    await invoke('api_move_folder', { id, newParentId });
    setFolders(prev => prev.map(f => (f.id === id ? { ...f, parent_id: newParentId } : f)));
  }, []);

  const deleteFolder = React.useCallback(async (id: string) => {
    await invoke('api_delete_folder', { id });
    // Backend cascade detaches subfolders + meetings; refetch both to stay true.
    await Promise.all([fetchFolders(), fetchMeetings()]);
  }, [fetchFolders, fetchMeetings]);

  const moveMeetingToFolder = React.useCallback(async (meetingId: string, folderId: string | null) => {
    await invoke('api_set_meeting_folder', { meetingId, folderId });
    setMeetings(prev => prev.map(m => (m.id === meetingId ? { ...m, folder_id: folderId } : m)));
  }, []);

  useEffect(() => {
    const fetchSettings = async () => {
      setServerAddress('http://localhost:5167');
      setTranscriptServerAddress('http://127.0.0.1:8178/stream');
    };
    fetchSettings();
  }, []);

  const baseItems: SidebarItem[] = [
    {
      id: 'meetings',
      title: 'Meeting Notes',
      type: 'folder' as const,
      children: [
        ...meetings.map(meeting => ({ id: meeting.id, title: meeting.title, type: 'file' as const }))
      ]
    },
  ];


  const toggleCollapse = () => {
    setIsCollapsed(!isCollapsed);
  };

  // Update current meeting when on home page
  useEffect(() => {
    if (pathname === '/') {
      setCurrentMeeting({ id: 'intro-call', title: '+ New Call' });
    }
    setSidebarItems(baseItems);
  }, [pathname]);

  // Update sidebar items when meetings change
  useEffect(() => {
    setSidebarItems(baseItems);
  }, [meetings]);

  // Function to handle recording toggle from sidebar
  const handleRecordingToggle = () => {
    if (!isRecording) {
      // Check if already on home page
      if (pathname === '/') {
        // Already on home - trigger recording directly via custom event
        console.log('Triggering recording from sidebar (already on home page)');
        window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'));
      } else {
        // Not on home - navigate and use auto-start mechanism
        console.log('Navigating to home page with auto-start flag');
        sessionStorage.setItem('autoStartRecording', 'true');
        router.push('/');
      }

      // Track recording initiation from sidebar
      Analytics.trackButtonClick('start_recording', 'sidebar');
    }
    // The actual recording start/stop is handled in the Home component
  };

  const searchTranscripts = React.useCallback(
    async (query: string, folderId: string | null = null) => {
      searchController.search(query, folderId);
    },
    [searchController]
  );

  useEffect(() => {
    return () => searchController.dispose();
  }, [searchController]);

  // Summary polling management
  const startSummaryPolling = React.useCallback((
    meetingId: string,
    processId: string,
    templateId: string,
    generation: string,
    onUpdate: (result: any) => void,
  ) => {
    const key = summaryPollKey({ meetingId, templateId, generation });

    // Summary generation is serialized for a meeting, but the map key still
    // includes the row and generation so an old response cannot reach a new
    // row after a restart.
    for (const [existingKey, interval] of activeSummaryPollsRef.current) {
      if (existingKey.startsWith(`${meetingId}\u0000`)) {
        clearInterval(interval);
        activeSummaryPollsRef.current.delete(existingKey);
        activeSummaryPollKeysRef.current.delete(existingKey);
      }
    }
    setActiveSummaryPolls(prev => {
      const next = new Map(prev);
      for (const existingKey of next.keys()) {
        if (existingKey.startsWith(`${meetingId}\u0000`)) next.delete(existingKey);
      }
      return next;
    });

    console.log(`📊 Starting polling for meeting ${meetingId}, process ${processId}, template ${templateId}, generation ${generation}`);

    let pollCount = 0;
    let latestRequest = 0;
    const MAX_POLLS = 200;
    let pollInterval: NodeJS.Timeout;
    const finish = () => {
      clearInterval(pollInterval);
      if (activeSummaryPollsRef.current.get(key) === pollInterval) {
        activeSummaryPollsRef.current.delete(key);
      }
      activeSummaryPollKeysRef.current.delete(key);
      setActiveSummaryPolls(prev => {
        const next = new Map(prev);
        if (next.get(key) === pollInterval) next.delete(key);
        return next;
      });
    };

    activeSummaryPollKeysRef.current.add(key);
    pollInterval = setInterval(async () => {
      if (!activeSummaryPollKeysRef.current.has(key)) return;
      pollCount++;

      if (pollCount >= MAX_POLLS) {
        console.warn(`⏱️ Polling timeout for ${meetingId} / ${templateId}`);
        finish();
        void invoke('api_cancel_summary', buildSummaryCancelArgs(meetingId, { templateId, generation })).catch((error) => {
          console.warn(`Failed to cancel timed-out summary for ${meetingId}:`, error);
        });
        onUpdate({
          status: 'error',
          template_id: templateId,
          start: generation,
          error: 'Summary generation timed out after 15 minutes. Please try again or check your model configuration.'
        });
        return;
      }

      const requestId = ++latestRequest;
      try {
        const result = await invoke('api_get_summary', {
          meetingId,
          templateId,
          generation,
        }) as any;

        // An interval can have an in-flight request after it was replaced or
        // stopped. Ignore that response before it can update React state.
        if (!activeSummaryPollKeysRef.current.has(key) || requestId !== latestRequest) return;
        if (!shouldApplySummaryPollResult(result, { templateId })) {
          finish();
          return;
        }

        console.log(`📊 Polling update for ${meetingId} / ${templateId}:`, result.status);
        onUpdate(result);

        if (result.status === 'completed' || result.status === 'error' || result.status === 'failed' || result.status === 'cancelled') {
          console.log(`Polling completed for ${meetingId} / ${templateId}, status: ${result.status}`);
          finish();
        } else if (result.status === 'idle' && pollCount > 1) {
          console.log(`Process ${generation} disappeared for ${meetingId} / ${templateId}, stopping poll`);
          finish();
        }
      } catch (error) {
        if (!activeSummaryPollKeysRef.current.has(key) || requestId !== latestRequest) return;
        console.error(`Polling error for ${meetingId} / ${templateId}:`, error);
        onUpdate({
          status: 'error',
          template_id: templateId,
          start: generation,
          error: error instanceof Error ? error.message : 'Unknown error'
        });
        finish();
      }
    }, 5000);

    activeSummaryPollsRef.current.set(key, pollInterval);
    setActiveSummaryPolls(prev => new Map(prev).set(key, pollInterval));
  }, []);

  const stopSummaryPolling = React.useCallback((meetingId: string, templateId?: string, generation?: string) => {
    const prefix = templateId && generation
      ? `${meetingId}\u0000${templateId}\u0000${generation}`
      : `${meetingId}\u0000`;
    for (const [key, interval] of activeSummaryPollsRef.current) {
      if (key === prefix || (!templateId && key.startsWith(prefix))) {
        clearInterval(interval);
        activeSummaryPollsRef.current.delete(key);
        activeSummaryPollKeysRef.current.delete(key);
      }
    }
    setActiveSummaryPolls(prev => {
      const next = new Map(prev);
      for (const key of next.keys()) {
        if (key === prefix || (!templateId && key.startsWith(prefix))) next.delete(key);
      }
      return next;
    });
  }, []);

  // Cleanup all polling intervals on unmount
  useEffect(() => {
    return () => {
      console.log('🧹 Cleaning up all summary polling intervals');
      activeSummaryPollsRef.current.forEach(interval => clearInterval(interval));
      activeSummaryPollsRef.current.clear();
      activeSummaryPollKeysRef.current.clear();
    };
  }, []);



  return (
    <SidebarContext.Provider value={{
      currentMeeting,
      setCurrentMeeting,
      sidebarItems,
      isCollapsed,
      toggleCollapse,
      meetings,
      setMeetings,
      isMeetingActive,
      setIsMeetingActive,
      handleRecordingToggle,
       searchTranscripts,
       cancelSidebarSearch: searchController.cancel,
       searchResponse,
       searchNotice,
       searchError,
       searchPhase,
      isSearching,
      setServerAddress,
      serverAddress,
      transcriptServerAddress,
      setTranscriptServerAddress,
      activeSummaryPolls,
      startSummaryPolling,
      stopSummaryPolling,
      refetchMeetings: fetchMeetings,
      folders,
      refetchFolders: fetchFolders,
      createFolder,
      renameFolder,
      moveFolder,
      deleteFolder,
      moveMeetingToFolder,
      sidebarWidth,
      sidebarDragging,
      resizeHandleProps,

    }}>
      {children}
    </SidebarContext.Provider>
  );
}
