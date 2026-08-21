"use client";

import { useEffect } from "react";

import type { PartialBlock, Block } from "@blocknote/core";
import { useCreateBlockNote } from "@blocknote/react";
import { BlockNoteView } from "@blocknote/shadcn";
import type { MarkdownCapableEditor } from "@/lib/blocknote-markdown";
import "@blocknote/shadcn/style.css";
import "@blocknote/core/fonts/inter.css";

interface EditorProps {
    initialContent?: Block[];
    onChange?: (blocks: Block[]) => void;
    onReady?: (editor: MarkdownCapableEditor) => void;
    editable?: boolean;
}

export default function Editor({ initialContent, onChange, onReady, editable = true }: EditorProps) {
    const editor = useCreateBlockNote({
        initialContent: initialContent as PartialBlock[] | undefined,
    });

    // Handle content changes
    useEffect(() => {
        if (!onChange) return;

        const handleChange = () => {
            onChange(editor.document);
        };

        const unsubscribe = editor.onChange(handleChange);

        return () => {
            if (typeof unsubscribe === "function") {
                unsubscribe();
            }
        };
    }, [editor, onChange]);

    useEffect(() => {
        onReady?.(editor);
    }, [editor, onReady]);

    return <BlockNoteView editor={editor} editable={editable} theme="light" />;
}
