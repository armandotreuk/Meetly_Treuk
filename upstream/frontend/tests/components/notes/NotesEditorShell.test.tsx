import { describe, expect, it, vi } from "vitest";
import React from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";

vi.mock("next/dynamic", async () => {
	const ReactModule = await import("react");
	return {
		default: () => (props: { onChange: (blocks: unknown[]) => void }) =>
			ReactModule.createElement("button", {
				"data-testid": "blocknote-editor",
				onClick: () => props.onChange([]),
			}),
	};
});

import { NotesEditorShell } from "@/components/notes/NotesEditorShell";

describe("NotesEditorShell", () => {
	it("renders BlockNote instead of a textarea", async () => {
		const container = document.createElement("div");
		const root = createRoot(container);
		const markdownEditorRef = { current: null };
		await act(async () => {
			root.render(
				React.createElement(NotesEditorShell, {
					notes: "note",
					initialBlocks: [],
					onBlocksChange: vi.fn(),
					markdownEditorRef,
					onBlur: vi.fn(),
					onManualSave: vi.fn(),
					isSaving: false,
					isDirty: false,
				})
			);
		});

		expect(container.querySelector("textarea")).toBeNull();
		expect(container.querySelector('[data-testid="blocknote-editor"]')).not.toBeNull();
		await act(async () => root.unmount());
	});

	it("saves with Ctrl+S through the window listener", async () => {
		const container = document.createElement("div");
		document.body.append(container);
		const root = createRoot(container);
		const onManualSave = vi.fn();
		await act(async () => {
			root.render(
				React.createElement(NotesEditorShell, {
					notes: "note",
					initialBlocks: [],
					onBlocksChange: vi.fn(),
					markdownEditorRef: { current: null },
					onBlur: vi.fn(),
					onManualSave,
					isSaving: false,
					isDirty: true,
				})
			);
		});

		(container.querySelector('[data-testid="blocknote-editor"]') as HTMLButtonElement).focus();
		await act(async () => window.dispatchEvent(new KeyboardEvent("keydown", { key: "s", ctrlKey: true })));
		expect(onManualSave).toHaveBeenCalledOnce();
		await act(async () => root.unmount());
		container.remove();
	});
});
