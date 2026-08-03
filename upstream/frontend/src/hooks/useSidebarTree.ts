import { useMemo } from "react";
import type { MeetingFolder, Meeting } from "@/types";

// Any object with the meeting fields the tree cares about. Accepts both
// full Meeting records (from the meeting-details views) and the slim
// CurrentMeeting shape exposed by SidebarProvider (created_at optional at
// the type level; always present at runtime after fetchMeetings).
export type MeetingLike = Pick<Meeting, "id" | "title" | "folder_id"> & {
    created_at?: string;
};

export interface MeetingNode {
    kind: "meeting";
    id: string;
    title: string;
    createdAt?: string;
}

export interface FolderNode {
    kind: "folder";
    id: string;
    name: string;
    parentId: string | null;
    children: TreeNode[];
}

export type TreeNode = FolderNode | MeetingNode;

export interface SidebarTree {
    // Virtual "Sem pasta" section: meetings with folder_id == null.
    unfiled: MeetingLike[];
    // Top-level folders (parent_id == null).
    roots: FolderNode[];
}

// ponytail: builds tree in O(n) via two Map passes; the recursive build is
// memoized. Ceiling: ~thousand folders or meetings; below that linear is
// imperceptible. Upgrade path if it grows: incremental tree update on the
// mutated subtree only.
export function useSidebarTree(folders: MeetingFolder[], meetings: MeetingLike[]): SidebarTree {
    return useMemo(() => {
        // Index meetings by folder_id (null => unfiled bucket).
        const unfiled: MeetingLike[] = [];
        const meetingsByFolder = new Map<string, MeetingLike[]>();
        for (const m of meetings) {
            if (m.folder_id) {
                const arr = meetingsByFolder.get(m.folder_id) ?? [];
                arr.push(m);
                meetingsByFolder.set(m.folder_id, arr);
            } else {
                unfiled.push(m);
            }
        }

        // Index folders by parent_id.
        const foldersByParent = new Map<string | null, MeetingFolder[]>();
        for (const f of folders) {
            const arr = foldersByParent.get(f.parent_id) ?? [];
            arr.push(f);
            foldersByParent.set(f.parent_id, arr);
        }

        const buildFolderNode = (folder: MeetingFolder): FolderNode => {
            const childFolders = foldersByParent.get(folder.id) ?? [];
            const childMeetings = meetingsByFolder.get(folder.id) ?? [];
            const children: TreeNode[] = [
                ...childFolders.map(buildFolderNode),
                ...childMeetings.map<TreeNode>((m) => ({
                    kind: "meeting",
                    id: m.id,
                    title: m.title,
                    createdAt: m.created_at,
                })),
            ];
            return {
                kind: "folder",
                id: folder.id,
                name: folder.name,
                parentId: folder.parent_id,
                children,
            };
        };

        const roots = (foldersByParent.get(null) ?? []).map(buildFolderNode);

        return { unfiled, roots };
    }, [folders, meetings]);
}
