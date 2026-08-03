'use client';

import React, { useState, useMemo, useEffect, useCallback, useRef } from 'react';
import { ChevronDown, ChevronRight, Settings, ChevronLeftCircle, ChevronRightCircle, Home, Mic, Square, NotebookPen, SearchIcon, X, Upload, FolderPlus, Inbox } from 'lucide-react';
import { useRouter, usePathname } from 'next/navigation';
import { useSidebar } from './SidebarProvider';
import type { CurrentMeeting } from '@/components/Sidebar/SidebarProvider';
import { ConfirmationModal } from '../ConfirmationModel/confirmation-modal';
import { ModelConfig } from '@/components/ModelSettingsModal';
import { SettingTabs } from '../SettingTabs';
import { TranscriptModelProps } from '@/components/TranscriptSettings';
import Analytics from '@/lib/analytics';
import { invoke } from '@tauri-apps/api/core';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useImportDialog } from '@/contexts/ImportDialogContext';
import { useConfig } from '@/contexts/ConfigContext';
import { FolderTreeItem } from './FolderTreeItem';
import { MeetingTreeItem, type DragPayload } from './MeetingTreeItem';
import { MoveToFolderModal } from './MoveToFolderModal';
import { useSidebarTree, type MeetingLike, type MeetingNode } from '@/hooks/useSidebarTree';

import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog"
import { VisuallyHidden } from "@/components/ui/visually-hidden"

import { MessageToast } from '../MessageToast';
import Logo from '../Logo';
import Info from '../Info';
import { ComplianceNotification } from '../ComplianceNotification';
import { Input } from '../ui/input';
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '../ui/input-group';

// "Sem pasta" virtual section: top of the tree, holds meetings with
// folder_id == null. Not renameable/deletable; dropping a meeting detaches
// it, dropping a folder moves it to root. Mirrors FolderTreeItem's
// drag-event contract (meetily-dragenter/leave/drop on a data-drop-target).
function UnfiledSection({ meetings, expanded, onToggle, onDropMeeting, onDropFolder, renderMeeting }: {
  meetings: MeetingLike[];
  expanded: boolean;
  onToggle: () => void;
  onDropMeeting: (meetingId: string) => void;
  onDropFolder: (folderId: string) => void;
  renderMeeting: (node: MeetingNode, depth: number) => React.ReactNode;
}) {
  const [isDropTarget, setIsDropTarget] = useState(false);
  const headerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = headerRef.current;
    if (!el) return;
    const onEnter = () => setIsDropTarget(true);
    const onLeave = () => setIsDropTarget(false);
    const onDrop = (e: Event) => {
      setIsDropTarget(false);
      const payload = (e as CustomEvent<{ payload: DragPayload }>).detail?.payload;
      if (!payload) return;
      if (payload.kind === 'meeting') onDropMeeting(payload.id);
      else onDropFolder(payload.id);
    };
    el.addEventListener('meetily-dragenter', onEnter);
    el.addEventListener('meetily-dragleave', onLeave);
    el.addEventListener('meetily-drop', onDrop as EventListener);
    return () => {
      el.removeEventListener('meetily-dragenter', onEnter);
      el.removeEventListener('meetily-dragleave', onLeave);
      el.removeEventListener('meetily-drop', onDrop as EventListener);
    };
  }, [onDropMeeting, onDropFolder]);

  return (
    <div>
      <div
        ref={headerRef}
        data-drop-target="unfiled"
        className={`flex items-center px-3 py-2 my-0.5 rounded-md text-sm cursor-pointer select-none ${
          isDropTarget ? 'bg-blue-100 ring-2 ring-blue-400' : 'hover:bg-gray-50'
        }`}
        onClick={onToggle}
      >
        <span className="flex-shrink-0 mr-1">
          {expanded ? (
            <ChevronDown className="w-4 h-4 text-gray-500" />
          ) : (
            <ChevronRight className="w-4 h-4 text-gray-500" />
          )}
        </span>
        <Inbox className="w-4 h-4 mr-2 flex-shrink-0 text-gray-600" />
        <span className="flex-1 truncate font-medium">Sem pasta</span>
        <span className="text-xs text-gray-400 mr-2">{meetings.length}</span>
      </div>
      {expanded && (
        <div>
          {meetings.map((m) =>
            renderMeeting({ kind: 'meeting', id: m.id, title: m.title, createdAt: m.created_at }, 1)
          )}
          {meetings.length === 0 && (
            <p className="text-xs text-gray-400 italic px-3 py-2" style={{ paddingLeft: '24px' }}>
              Arraste meetings aqui ou use Mover para...
            </p>
          )}
        </div>
      )}
    </div>
  );
}

const Sidebar: React.FC = () => {
  const router = useRouter();
  const pathname = usePathname();
  const {
    currentMeeting,
    setCurrentMeeting,
    isCollapsed,
    toggleCollapse,
    handleRecordingToggle,
    searchTranscripts,
    searchResults,
    isSearching,
    meetings,
    setMeetings,
    serverAddress,
    folders,
    createFolder,
    renameFolder,
    moveFolder,
    deleteFolder,
    moveMeetingToFolder
  } = useSidebar();

  // Get recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();
  const { openImportDialog } = useImportDialog();
  const { betaFeatures } = useConfig();
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(new Set(['unfiled']));
  const [searchQuery, setSearchQuery] = useState<string>('');
  const [showModelSettings, setShowModelSettings] = useState(false);
  const [modelConfig, setModelConfig] = useState<ModelConfig>({
    provider: 'ollama',
    model: '',
    whisperModel: '',
    apiKey: null,
    ollamaEndpoint: null
  });
  const [transcriptModelConfig, setTranscriptModelConfig] = useState<TranscriptModelProps>({
    provider: 'parakeet',
    model: 'parakeet-tdt-0.6b-v3-int8',
  });
  const [settingsSaveSuccess, setSettingsSaveSuccess] = useState<boolean | null>(null);

  // State for edit modal
  const [editModalState, setEditModalState] = useState<{ isOpen: boolean; meetingId: string | null; currentTitle: string }>({
    isOpen: false,
    meetingId: null,
    currentTitle: ''
  });
  const [editingTitle, setEditingTitle] = useState<string>('');

  // Folder tree modals
  const [moveModalState, setMoveModalState] = useState<{ isOpen: boolean; kind: 'meeting' | 'folder'; id: string | null }>({
    isOpen: false,
    kind: 'meeting',
    id: null
  });
  const [folderModalState, setFolderModalState] = useState<{
    isOpen: boolean;
    mode: 'create' | 'rename';
    folderId: string | null;
    parentId: string | null;
  }>({ isOpen: false, mode: 'create', folderId: null, parentId: null });
  const [folderNameInput, setFolderNameInput] = useState<string>('');
  const [deleteFolderModalState, setDeleteFolderModalState] = useState<{ isOpen: boolean; folderId: string | null }>({
    isOpen: false,
    folderId: null
  });

  // useEffect(() => {
  //   if (settingsSaveSuccess !== null) {
  //     const timer = setTimeout(() => {
  //       setSettingsSaveSuccess(null);
  //     }, 3000);
  //   }
  // }, [settingsSaveSuccess]);


  const [deleteModalState, setDeleteModalState] = useState<{ isOpen: boolean; itemId: string | null }>({ isOpen: false, itemId: null });

  useEffect(() => {
    // Note: Don't set hardcoded defaults - let DB be the source of truth
    const fetchModelConfig = async () => {
      // Only make API call if serverAddress is loaded
      if (!serverAddress) {
        console.log('Waiting for server address to load before fetching model config');
        return;
      }

      try {
        const data = await invoke('api_get_model_config') as any;
        if (data && data.provider !== null) {
          // Fetch API key if not included and provider requires it
          if (data.provider !== 'ollama' && !data.apiKey) {
            try {
              const apiKeyData = await invoke('api_get_api_key', {
                provider: data.provider
              }) as string;
              data.apiKey = apiKeyData;
            } catch (err) {
              console.error('Failed to fetch API key:', err);
            }
          }
          setModelConfig(data);
        }
      } catch (error) {
        console.error('Failed to fetch model config:', error);
      }
    };

    fetchModelConfig();
  }, [serverAddress]);


  useEffect(() => {
    // Note: Don't set hardcoded defaults - let DB be the source of truth
    const fetchTranscriptSettings = async () => {
      // Only make API call if serverAddress is loaded
      if (!serverAddress) {
        console.log('Waiting for server address to load before fetching transcript settings');
        return;
      }

      try {
        const data = await invoke('api_get_transcript_config') as any;
        if (data && data.provider !== null) {
          setTranscriptModelConfig(data);
        }
      } catch (error) {
        console.error('Failed to fetch transcript settings:', error);
      }
    };
    fetchTranscriptSettings();
  }, [serverAddress]);

  // Listen for model config updates from other components
  useEffect(() => {
    const setupListener = async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const unlisten = await listen<ModelConfig>('model-config-updated', (event) => {
        console.log('Sidebar received model-config-updated event:', event.payload);
        setModelConfig(event.payload);
      });

      return unlisten;
    };

    let cleanup: (() => void) | undefined;
    setupListener().then(fn => cleanup = fn);

    return () => {
      cleanup?.();
    };
  }, []);



  // Handle model config save
  const handleSaveModelConfig = async (config: ModelConfig) => {
    try {
      await invoke('api_save_model_config', {
        provider: config.provider,
        model: config.model,
        whisperModel: config.whisperModel,
        apiKey: config.apiKey,
        ollamaEndpoint: config.ollamaEndpoint,
      });

      setModelConfig(config);
      console.log('Model config saved successfully');
      setSettingsSaveSuccess(true);

      // Emit event to sync other components
      const { emit } = await import('@tauri-apps/api/event');
      await emit('model-config-updated', config);

      // Track settings change
      await Analytics.trackSettingsChanged('model_config', `${config.provider}_${config.model}`);
    } catch (error) {
      console.error('Error saving model config:', error);
      setSettingsSaveSuccess(false);
    }
  };

  const handleSaveTranscriptConfig = async (updatedConfig?: TranscriptModelProps) => {
    try {
      const configToSave = updatedConfig || transcriptModelConfig;
      const payload = {
        provider: configToSave.provider,
        model: configToSave.model,
        apiKey: configToSave.apiKey ?? null
      };
      console.log('Saving transcript config with payload:', payload);

      await invoke('api_save_transcript_config', {
        provider: payload.provider,
        model: payload.model,
        apiKey: payload.apiKey,
      });


      setSettingsSaveSuccess(true);

      // Track settings change
      const transcriptConfigToSave = updatedConfig || transcriptModelConfig;
      await Analytics.trackSettingsChanged('transcript_config', `${transcriptConfigToSave.provider}_${transcriptConfigToSave.model}`);
    } catch (error) {
      console.error('Failed to save transcript config:', error);
      setSettingsSaveSuccess(false);
    }
  };

  // Handle search input changes
  const handleSearchChange = useCallback(async (value: string) => {
    setSearchQuery(value);

    // If search query is empty, just return to normal view
    if (!value.trim()) return;

    // Search through transcripts
    await searchTranscripts(value);
  }, [searchTranscripts]);

  // Folder tree (pastas lógicas): unfiled bucket + recursive folder roots.
  const { unfiled, roots } = useSidebarTree(folders, meetings);

  const folderNameById = useMemo(() => new Map(folders.map((f) => [f.id, f.name])), [folders]);

  // Global search is flat (decision #19): transcript matches ∪ title matches,
  // rendered without the tree.
  const searchFilteredMeetings = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return [];
    const matchedIds = new Set(searchResults.map((r) => r.id));
    return meetings.filter((m) => matchedIds.has(m.id) || m.title.toLowerCase().includes(q));
  }, [meetings, searchQuery, searchResults]);

  // Folder action handlers (backend errors surface as toasts)
  const handleMoveMeeting = useCallback(async (meetingId: string, folderId: string | null) => {
    try {
      await moveMeetingToFolder(meetingId, folderId);
    } catch (error) {
      toast.error('Falha ao mover meeting', {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  }, [moveMeetingToFolder]);

  const handleMoveFolder = useCallback(async (folderId: string, newParentId: string | null) => {
    try {
      await moveFolder(folderId, newParentId);
    } catch (error) {
      // Backend rejects cycles with a descriptive message
      toast.error('Falha ao mover pasta', {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  }, [moveFolder]);

  const openCreateFolderModal = (parentId: string | null) => {
    setFolderModalState({ isOpen: true, mode: 'create', folderId: null, parentId });
    setFolderNameInput('');
  };

  const openRenameFolderModal = (folderId: string, currentName: string) => {
    setFolderModalState({ isOpen: true, mode: 'rename', folderId, parentId: null });
    setFolderNameInput(currentName);
  };

  const handleFolderModalConfirm = async () => {
    const name = folderNameInput.trim();
    if (!name) {
      toast.error('Nome da pasta não pode ser vazio');
      return;
    }
    try {
      if (folderModalState.mode === 'create') {
        await createFolder(name, folderModalState.parentId);
        // Reveal the new subfolder inside its parent
        if (folderModalState.parentId) {
          const parentId = folderModalState.parentId;
          setExpandedFolders((prev) => new Set(prev).add(parentId));
        }
        toast.success('Pasta criada');
      } else if (folderModalState.folderId) {
        await renameFolder(folderModalState.folderId, name);
        toast.success('Pasta renomeada');
      }
      setFolderModalState((s) => ({ ...s, isOpen: false }));
    } catch (error) {
      toast.error('Falha ao salvar pasta', {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleDeleteFolderConfirm = async () => {
    const id = deleteFolderModalState.folderId;
    setDeleteFolderModalState({ isOpen: false, folderId: null });
    if (!id) return;
    try {
      await deleteFolder(id);
      toast.success('Pasta excluída', { description: 'Meetings movidos para Sem pasta' });
    } catch (error) {
      toast.error('Falha ao excluir pasta', {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleMoveModalSelect = async (folderId: string | null) => {
    const { kind, id } = moveModalState;
    setMoveModalState((s) => ({ ...s, isOpen: false }));
    if (!id) return;
    if (kind === 'meeting') await handleMoveMeeting(id, folderId);
    else await handleMoveFolder(id, folderId);
  };


  const handleDelete = async (itemId: string) => {
    console.log('Deleting item:', itemId);
    const payload = {
      meetingId: itemId
    };

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('api_delete_meeting', {
        meetingId: itemId,
      });
      console.log('Meeting deleted successfully');
      const updatedMeetings = meetings.filter((m: CurrentMeeting) => m.id !== itemId);
      setMeetings(updatedMeetings);

      // Track meeting deletion
      Analytics.trackMeetingDeleted(itemId);

      // Show success toast
      toast.success("Meeting deleted successfully", {
        description: "All associated data has been removed"
      });

      // If deleting the active meeting, navigate to home
      if (currentMeeting?.id === itemId) {
        setCurrentMeeting({ id: 'intro-call', title: '+ New Call' });
        router.push('/');
      }
    } catch (error) {
      console.error('Failed to delete meeting:', error);
      toast.error("Failed to delete meeting", {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleDeleteConfirm = () => {
    if (deleteModalState.itemId) {
      handleDelete(deleteModalState.itemId);
    }
    setDeleteModalState({ isOpen: false, itemId: null });
  };

  // Handle modal editing of meeting names
  const handleEditStart = (meetingId: string, currentTitle: string) => {
    setEditModalState({
      isOpen: true,
      meetingId: meetingId,
      currentTitle: currentTitle
    });
    setEditingTitle(currentTitle);
  };

  const handleEditConfirm = async () => {
    const newTitle = editingTitle.trim();
    const meetingId = editModalState.meetingId;

    if (!meetingId) return;

    // Prevent empty titles
    if (!newTitle) {
      toast.error("Meeting title cannot be empty");
      return;
    }

    try {
      await invoke('api_save_meeting_title', {
        meetingId: meetingId,
        title: newTitle,
      });

      // Update local state
      const updatedMeetings = meetings.map((m: CurrentMeeting) =>
        m.id === meetingId ? { ...m, title: newTitle } : m
      );
      setMeetings(updatedMeetings);

      // Update current meeting if it's the one being edited
      if (currentMeeting?.id === meetingId) {
        setCurrentMeeting({ id: meetingId, title: newTitle });
      }

      // Track the edit
      Analytics.trackButtonClick('edit_meeting_title', 'sidebar');

      toast.success("Meeting title updated successfully");

      // Close modal and reset state
      setEditModalState({ isOpen: false, meetingId: null, currentTitle: '' });
      setEditingTitle('');
    } catch (error) {
      console.error('Failed to update meeting title:', error);
      toast.error("Failed to update meeting title", {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleEditCancel = () => {
    setEditModalState({ isOpen: false, meetingId: null, currentTitle: '' });
    setEditingTitle('');
  };

  const toggleFolder = (folderId: string) => {
    // Normal toggle behavior for all folders
    const newExpanded = new Set(expandedFolders);
    if (newExpanded.has(folderId)) {
      newExpanded.delete(folderId);
    } else {
      newExpanded.add(folderId);
    }
    setExpandedFolders(newExpanded);
  };

  // Expose setShowModelSettings to window for Rust tray to call
  useEffect(() => {
    (window as any).openSettings = () => {
      setShowModelSettings(true);
    };

    // Cleanup on unmount
    return () => {
      delete (window as any).openSettings;
    };
  }, []);

  const renderCollapsedIcons = () => {
    if (!isCollapsed) return null;

    const isHomePage = pathname === '/';
    const isMeetingPage = pathname?.includes('/meeting-details');
    const isSettingsPage = pathname === '/settings';

    return (
      <TooltipProvider>
        <div className="flex flex-col items-center space-y-4 mt-4">
          <Logo isCollapsed={isCollapsed} />

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => router.push('/')}
                className={`p-2 rounded-lg transition-colors duration-150 ${isHomePage ? 'bg-gray-100' : 'hover:bg-gray-100'
                  }`}
              >
                <Home className="w-5 h-5 text-gray-600" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Home</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={handleRecordingToggle}
                disabled={isRecording}
                className={`p-2 ${isRecording ? 'bg-red-500 cursor-not-allowed' : 'bg-red-500 hover:bg-red-600'} rounded-full transition-colors duration-150 shadow-sm`}
              >
                {isRecording ? (
                  <Square className="w-5 h-5 text-white" />
                ) : (
                  <Mic className="w-5 h-5 text-white" />
                )}
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>{isRecording ? "Recording in progress..." : "Start Recording"}</p>
            </TooltipContent>
          </Tooltip>

          {betaFeatures.importAndRetranscribe && (
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  onClick={() => openImportDialog()}
                  className="p-2 rounded-lg transition-colors duration-150 hover:bg-blue-100 bg-blue-50"
                >
                  <Upload className="w-5 h-5 text-blue-600" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="right">
                <p>Import Audio</p>
              </TooltipContent>
            </Tooltip>
          )}

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => {
                  if (isCollapsed) toggleCollapse();
                  toggleFolder('unfiled');
                }}
                className={`p-2 rounded-lg transition-colors duration-150 ${isMeetingPage ? 'bg-gray-100' : 'hover:bg-gray-100'
                  }`}
              >
                <NotebookPen className="w-5 h-5 text-gray-600" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Meeting Notes</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => router.push('/settings')}
                className={`p-2 rounded-lg transition-colors duration-150 ${isSettingsPage ? 'bg-gray-100' : 'hover:bg-gray-100'
                  }`}
              >
                <Settings className="w-5 h-5 text-gray-600" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Settings</p>
            </TooltipContent>
          </Tooltip>

          <Info isCollapsed={isCollapsed} />
        </div>
      </TooltipProvider>
    );
  };

  // Find matching transcript snippet for a meeting item
  const findMatchingSnippet = (itemId: string) => {
    if (!searchQuery.trim() || !searchResults.length) return null;
    return searchResults.find(result => result.id === itemId);
  };

  // Shared meeting row renderer: used by the tree (FolderTreeItem children),
  // the unfiled section, and flat search results. MeetingTreeItem owns the
  // drag-source behavior, date sub-line, and rename/delete/move actions.
  const renderMeeting = (node: MeetingNode, depth: number) => (
    <MeetingTreeItem
      key={node.id}
      meetingId={node.id}
      title={node.title}
      depth={depth}
      currentMeetingId={currentMeeting?.id}
      createdAt={node.createdAt}
      onEditMeeting={handleEditStart}
      onRequestDeleteMeeting={(id) => setDeleteModalState({ isOpen: true, itemId: id })}
      onRequestMoveMeeting={(id) => setMoveModalState({ isOpen: true, kind: 'meeting', id })}
    />
  );

  return (
    <div className="fixed top-0 left-0 h-screen z-40">
      {/* Floating collapse button */}
      <button
        onClick={toggleCollapse}
        className="absolute -right-6 top-20 z-50 p-1 bg-white hover:bg-gray-100 rounded-full shadow-lg border"
        style={{ transform: 'translateX(50%)' }}
      >
        {isCollapsed ? (
          <ChevronRightCircle className="w-6 h-6" />
        ) : (
          <ChevronLeftCircle className="w-6 h-6" />
        )}
      </button>

      <div
        className={`h-screen bg-white border-r shadow-sm flex flex-col transition-all duration-300 ${isCollapsed ? 'w-16' : 'w-64'
          }`}
      >
        {/*  Header with traffic light spacing */}
        <div className="flex-shrink-0 h-22 flex items-center">

          {/* Title container */}



          <div className="flex-1">
            {!isCollapsed && (
              <div className="p-3">
                {/* <span className="text-lg text-center border rounded-full bg-blue-50 border-white font-semibold text-gray-700 mb-2 block items-center">
                  <span>Meetily</span>
                </span> */}
                <Logo isCollapsed={isCollapsed} />

                <div className="relative mb-1">
                  <InputGroup >
                    <InputGroupInput placeholder='Search meeting content...' value={searchQuery}
                      onChange={(e) => handleSearchChange(e.target.value)}
                    />
                    <InputGroupAddon>
                      <SearchIcon />
                    </InputGroupAddon>
                    {searchQuery &&
                      <InputGroupAddon align={'inline-end'}>
                        <InputGroupButton
                          onClick={() => handleSearchChange('')}
                        >
                          <X />
                        </InputGroupButton>
                      </InputGroupAddon>
                    }
                  </InputGroup>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Main content - scrollable area */}
        <div className="flex-1 flex flex-col min-h-0">
          {/* Fixed navigation items */}
          <div className="flex-shrink-0">
            {!isCollapsed && (
              <div
                onClick={() => router.push('/')}
                className="p-3  text-lg font-semibold items-center hover:bg-gray-100 h-10   flex mx-3 mt-3 rounded-lg cursor-pointer"
              >
                <Home className="w-4 h-4 mr-2" />
                <span>Home</span>
              </div>
            )}
          </div>

          {/* Content area */}
          <div className="flex-1 flex flex-col min-h-0">
            {renderCollapsedIcons()}
            {/* Meeting Notes folder header - fixed */}
            {!isCollapsed && (
              <div className="flex-shrink-0">
                <div className="flex items-center transition-all duration-150 p-3 text-lg font-semibold h-10 mx-3 mt-3 rounded-lg group">
                  <NotebookPen className="w-4 h-4 mr-2 text-gray-600" />
                  <span className="text-gray-700">Meeting Notes</span>
                  {searchQuery && isSearching && (
                    <span className="ml-2 text-xs text-blue-500 animate-pulse">Searching...</span>
                  )}
                  <button
                    onClick={() => openCreateFolderModal(null)}
                    className="ml-auto text-gray-400 hover:text-blue-600 p-1 rounded-md hover:bg-blue-50 opacity-0 group-hover:opacity-100 transition-opacity duration-150"
                    aria-label="Nova pasta"
                    title="Nova pasta"
                  >
                    <FolderPlus className="w-4 h-4" />
                  </button>
                </div>
              </div>
            )}

            {/* Scrollable meeting items: flat results when searching, folder tree otherwise */}
            {!isCollapsed && (
              <div className="flex-1 overflow-y-auto custom-scrollbar min-h-0">
                <div className="mx-3">
                  {searchQuery.trim() ? (
                    <>
                      {searchFilteredMeetings.map((m) => (
                        <MeetingTreeItem
                          key={m.id}
                          meetingId={m.id}
                          title={m.title}
                          depth={0}
                          currentMeetingId={currentMeeting?.id}
                          createdAt={m.created_at}
                          snippetContext={findMatchingSnippet(m.id)?.matchContext ?? null}
                          folderName={m.folder_id ? folderNameById.get(m.folder_id) ?? null : null}
                          onEditMeeting={handleEditStart}
                          onRequestDeleteMeeting={(id) => setDeleteModalState({ isOpen: true, itemId: id })}
                          onRequestMoveMeeting={(id) => setMoveModalState({ isOpen: true, kind: 'meeting', id })}
                        />
                      ))}
                      {searchFilteredMeetings.length === 0 && !isSearching && (
                        <p className="text-xs text-gray-400 italic px-3 py-2">Nenhum resultado.</p>
                      )}
                    </>
                  ) : (
                    <>
                      <UnfiledSection
                        meetings={unfiled}
                        expanded={expandedFolders.has('unfiled')}
                        onToggle={() => toggleFolder('unfiled')}
                        onDropMeeting={(id) => handleMoveMeeting(id, null)}
                        onDropFolder={(id) => handleMoveFolder(id, null)}
                        renderMeeting={renderMeeting}
                      />
                      {roots.map((folder) => (
                        <FolderTreeItem
                          key={folder.id}
                          folder={folder}
                          depth={0}
                          expanded={expandedFolders}
                          onToggle={toggleFolder}
                          currentMeetingId={currentMeeting?.id}
                          onEditMeeting={handleEditStart}
                          onRequestDeleteMeeting={(id) => setDeleteModalState({ isOpen: true, itemId: id })}
                          onMoveMeeting={handleMoveMeeting}
                          onMoveFolder={handleMoveFolder}
                          onCreateSubfolder={openCreateFolderModal}
                          onRenameFolder={openRenameFolderModal}
                          onRequestDeleteFolder={(id) => setDeleteFolderModalState({ isOpen: true, folderId: id })}
                          onRequestMoveFolder={(id) => setMoveModalState({ isOpen: true, kind: 'folder', id })}
                          renderMeeting={renderMeeting}
                        />
                      ))}
                    </>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Footer */}
        {!isCollapsed && (

          <div className="flex-shrink-0 p-2 border-t border-gray-100">
            <button
              onClick={handleRecordingToggle}
              disabled={isRecording}
              className={`w-full flex items-center justify-center px-3 py-2 text-sm font-medium text-white ${isRecording ? 'bg-red-300 cursor-not-allowed' : 'bg-red-500 hover:bg-red-600'} rounded-lg transition-colors shadow-sm`}
            >
              {isRecording ? (
                <>
                  <Square className="w-4 h-4 mr-2" />
                  <span>Recording in progress...</span>
                </>
              ) : (
                <>
                  <Mic className="w-4 h-4 mr-2" />
                  <span>Start Recording</span>
                </>
              )}
            </button>

            {betaFeatures.importAndRetranscribe && (
              <button
                onClick={() => openImportDialog()}
                className="w-full flex items-center justify-center px-3 py-2 mt-1 text-sm font-medium text-gray-700 bg-blue-100 hover:bg-blue-200 rounded-lg transition-colors shadow-sm"
              >
                <Upload className="w-4 h-4 mr-2" />
                <span>Import Audio</span>
              </button>
            )}

            <button
              onClick={() => router.push('/settings')}
              className="w-full flex items-center justify-center px-3 py-1.5 mt-1 mb-1 text-sm font-medium text-gray-700 bg-gray-200 hover:bg-gray-300 rounded-lg transition-colors shadow-sm"
            >
              <Settings className="w-4 h-4 mr-2" />
              <span>Settings</span>
            </button>
            <Info isCollapsed={isCollapsed} />
            <div className="w-full flex items-center justify-center px-3 py-1 text-xs text-gray-400">
              v0.4.0
            </div>
          </div>
        )}
      </div>

      {/* Confirmation Modal for Delete */}
      <ConfirmationModal
        isOpen={deleteModalState.isOpen}
        text="Are you sure you want to delete this meeting? This action cannot be undone."
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteModalState({ isOpen: false, itemId: null })}
      />

      {/* Edit Meeting Title Modal */}
      <Dialog open={editModalState.isOpen} onOpenChange={(open) => {
        if (!open) handleEditCancel();
      }}>
        <DialogContent className="sm:max-w-[425px]">
          <VisuallyHidden>
            <DialogTitle>Edit Meeting Title</DialogTitle>
          </VisuallyHidden>
          <div className="py-4">
            <h3 className="text-lg font-semibold mb-4">Edit Meeting Title</h3>
            <div className="space-y-4">
              <div>
                <label htmlFor="meeting-title" className="block text-sm font-medium text-gray-700 mb-2">
                  Meeting Title
                </label>
                <input
                  id="meeting-title"
                  type="text"
                  value={editingTitle}
                  onChange={(e) => setEditingTitle(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      handleEditConfirm();
                    } else if (e.key === 'Escape') {
                      handleEditCancel();
                    }
                  }}
                  className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                  placeholder="Enter meeting title"
                  autoFocus
                />
              </div>
            </div>
          </div>
          <DialogFooter>
            <button
              onClick={handleEditCancel}
              className="px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 rounded-md transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleEditConfirm}
              className="px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors"
            >
              Save
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Move meeting/folder modal */}
      <MoveToFolderModal
        isOpen={moveModalState.isOpen}
        excludeId={moveModalState.kind === 'folder' ? moveModalState.id : null}
        folders={folders}
        title={moveModalState.kind === 'folder' ? 'Mover pasta para...' : 'Mover meeting para...'}
        onCancel={() => setMoveModalState((s) => ({ ...s, isOpen: false }))}
        onSelect={handleMoveModalSelect}
      />

      {/* Delete folder confirmation */}
      <ConfirmationModal
        isOpen={deleteFolderModalState.isOpen}
        text="Excluir esta pasta? Subpastas serão excluídas junto e todos os meetings voltarão para Sem pasta."
        onConfirm={handleDeleteFolderConfirm}
        onCancel={() => setDeleteFolderModalState({ isOpen: false, folderId: null })}
      />

      {/* Create/rename folder modal */}
      <Dialog open={folderModalState.isOpen} onOpenChange={(open) => {
        if (!open) setFolderModalState((s) => ({ ...s, isOpen: false }));
      }}>
        <DialogContent className="sm:max-w-[425px]">
          <VisuallyHidden>
            <DialogTitle>{folderModalState.mode === 'create' ? 'Nova Pasta' : 'Renomear Pasta'}</DialogTitle>
          </VisuallyHidden>
          <div className="py-4">
            <h3 className="text-lg font-semibold mb-4">
              {folderModalState.mode === 'create' ? 'Nova Pasta' : 'Renomear Pasta'}
            </h3>
            <div>
              <label htmlFor="folder-name" className="block text-sm font-medium text-gray-700 mb-2">
                Nome da pasta
              </label>
              <input
                id="folder-name"
                type="text"
                value={folderNameInput}
                onChange={(e) => setFolderNameInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    handleFolderModalConfirm();
                  } else if (e.key === 'Escape') {
                    setFolderModalState((s) => ({ ...s, isOpen: false }));
                  }
                }}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                placeholder="Nome da pasta"
                autoFocus
              />
            </div>
          </div>
          <DialogFooter>
            <button
              onClick={() => setFolderModalState((s) => ({ ...s, isOpen: false }))}
              className="px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 rounded-md transition-colors"
            >
              Cancelar
            </button>
            <button
              onClick={handleFolderModalConfirm}
              className="px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors"
            >
              Salvar
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default Sidebar;
