"use client";

import React from "react";
import { Folder, X } from "lucide-react";
import type { MeetingFolder } from "@/types";

interface FolderFilterTreeProps {
    folders: MeetingFolder[];
    selected: string | null;
    onSelect: (name: string | null) => void;
}

export function FolderFilterTree({ folders, selected, onSelect }: FolderFilterTreeProps) {
    if (folders.length === 0) return null;

    return (
        <div className="flex flex-wrap gap-1 px-1 mt-1">
            {folders.map((f) => {
                const active = selected === f.name;
                return (
                    <button
                        key={f.id}
                        onClick={() => onSelect(active ? null : f.name)}
                        aria-pressed={active}
                        aria-label={`Filtrar por pasta ${f.name}`}
                        className={`flex items-center gap-1 px-2 py-0.5 text-xs rounded-full border transition-colors ${
                            active
                                ? "bg-blue-100 border-blue-300 text-blue-700"
                                : "bg-gray-50 border-gray-200 text-gray-600 hover:bg-gray-100"
                        }`}
                    >
                        <Folder className="w-2.5 h-2.5" />
                        <span className="truncate max-w-[80px]">{f.name}</span>
                        {active && <X className="w-2.5 h-2.5" />}
                    </button>
                );
            })}
            {selected && (
                <button
                    onClick={() => onSelect(null)}
                    aria-label="Limpar filtro de pasta"
                    className="text-xs text-gray-400 hover:text-gray-600 underline"
                >
                    limpar
                </button>
            )}
        </div>
    );
}
