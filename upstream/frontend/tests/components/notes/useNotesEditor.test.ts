import { describe, expect, it, vi } from "vitest";
import React from "react";
import { createRoot } from "react-dom/client";
import { act } from "react";
import { useNotesEditor } from "@/components/notes/useNotesEditor";

describe("useNotesEditor", () => {
	it("flushes the latest notes on unmount via notesRef", async () => {
		const saves: string[] = [];
		const save = async (markdown: string) => {
			saves.push(markdown);
		};

		let editor: ReturnType<typeof useNotesEditor> | null = null;

		function TestComp({ initialNotes }: { initialNotes?: string }) {
			const e = useNotesEditor({ save, initialNotes });
			editor = e;
			return React.createElement("div", null, e.notes);
		}

		const container = document.createElement("div");
		document.body.appendChild(container);
		const root = createRoot(container);

		await act(async () => {
			root.render(React.createElement(TestComp, { initialNotes: "hello" }));
		});

		await act(async () => {
			editor!.handleChange({
				target: { value: "world" },
			} as React.ChangeEvent<HTMLTextAreaElement>);
		});

		await act(async () => {
			root.unmount();
		});

		await act(async () => {
			await Promise.resolve();
		});

		expect(saves).toContain("world");
		expect(saves[saves.length - 1]).toBe("world");
	});

	it("debounces save by 2s and cancels on manual save", async () => {
		vi.useFakeTimers();
		const saves: string[] = [];
		const save = async (markdown: string) => {
			saves.push(markdown);
		};

		let editor: ReturnType<typeof useNotesEditor> | null = null;

		function TestComp({ initialNotes }: { initialNotes?: string }) {
			const e = useNotesEditor({ save, initialNotes });
			editor = e;
			return React.createElement("div", null, e.notes);
		}

		const container = document.createElement("div");
		document.body.appendChild(container);
		const root = createRoot(container);

		await act(async () => {
			root.render(React.createElement(TestComp, { initialNotes: "a" }));
		});

		await act(async () => {
			editor!.handleChange({
				target: { value: "b" },
			} as React.ChangeEvent<HTMLTextAreaElement>);
		});

		// Before 2s, no save yet
		vi.advanceTimersByTime(500);
		expect(saves).toEqual([]);

		// Manual save should cancel the pending timer and fire immediately
		await act(async () => {
			editor!.handleManualSave();
		});
		expect(saves).toEqual(["b"]);

		// Advancing past the original 2s window should not fire a duplicate
		vi.advanceTimersByTime(2000);
		expect(saves).toEqual(["b"]);

		await act(async () => {
			root.unmount();
		});

		vi.useRealTimers();
	});

	it("cancelPendingSave clears the pending debounce without saving", async () => {
		vi.useFakeTimers();
		const saves: string[] = [];
		const save = async (markdown: string) => {
			saves.push(markdown);
		};

		let editor: ReturnType<typeof useNotesEditor> | null = null;

		function TestComp({ initialNotes }: { initialNotes?: string }) {
			const e = useNotesEditor({ save, initialNotes });
			editor = e;
			return React.createElement("div", null, e.notes);
		}

		const container = document.createElement("div");
		document.body.appendChild(container);
		const root = createRoot(container);

		await act(async () => {
			root.render(React.createElement(TestComp, { initialNotes: "a" }));
		});

		await act(async () => {
			editor!.handleChange({
				target: { value: "b" },
			} as React.ChangeEvent<HTMLTextAreaElement>);
		});

		await act(async () => {
			editor!.cancelPendingSave();
		});
		expect(editor!.isDirty).toBe(true);

		vi.advanceTimersByTime(2000);
		expect(saves).toEqual([]);

		await act(async () => {
			root.unmount();
		});
		expect(saves).toEqual([]);

		vi.useRealTimers();
	});

	it("saves BlockNote JSON when markdown conversion falls back", async () => {
		vi.useFakeTimers();
		const saves: Array<{ markdown: string; blocksJson: string | null }> = [];
		const blocks = [{ id: "block-1", type: "paragraph", props: {}, content: [] }] as any;
		let editor: ReturnType<typeof useNotesEditor> | null = null;

		function TestComp() {
			const e = useNotesEditor({
				save: async (markdown, blocksJson) => {
					saves.push({ markdown, blocksJson });
				},
				initialNotes: "fallback markdown",
			});
			editor = e;
			return React.createElement("div");
		}

		const container = document.createElement("div");
		document.body.appendChild(container);
		const root = createRoot(container);
		await act(async () => {
			root.render(React.createElement(TestComp));
		});
		editor!.markdownEditorRef.current = {
			blocksToMarkdownLossy: async () => Promise.reject(new Error("conversion failed")),
		};

		await act(async () => {
			editor!.setBlocks(blocks);
			await vi.advanceTimersByTimeAsync(2000);
		});

		expect(saves).toEqual([{ markdown: "fallback markdown", blocksJson: JSON.stringify(blocks) }]);
		await act(async () => {
			root.unmount();
		});
		vi.useRealTimers();
	});

	it("serializes saves and keeps only the latest queued content", async () => {
		let resolveFirst!: () => void;
		let resolveSecond!: () => void;
		const first = new Promise<void>((resolve) => (resolveFirst = resolve));
		const second = new Promise<void>((resolve) => (resolveSecond = resolve));
		const save = vi
			.fn<(markdown: string) => Promise<void>>()
			.mockReturnValueOnce(first)
			.mockReturnValueOnce(second);
		let editor: ReturnType<typeof useNotesEditor> | null = null;

		function TestComp() {
			const e = useNotesEditor({ save, initialNotes: "a" });
			editor = e;
			return React.createElement("div", null, e.notes);
		}

		const container = document.createElement("div");
		const root = createRoot(container);
		await act(async () => root.render(React.createElement(TestComp)));
		await act(async () => {
			editor!.handleChange({ target: { value: "b" } } as React.ChangeEvent<HTMLTextAreaElement>);
			editor!.handleManualSave();
		});
		await act(async () => {
			editor!.handleChange({ target: { value: "c" } } as React.ChangeEvent<HTMLTextAreaElement>);
			editor!.handleManualSave();
			editor!.handleChange({ target: { value: "d" } } as React.ChangeEvent<HTMLTextAreaElement>);
			editor!.handleManualSave();
		});

		expect(save).toHaveBeenCalledTimes(1);
		expect(save).toHaveBeenNthCalledWith(1, "b", null);
		await act(async () => resolveFirst());
		expect(save).toHaveBeenCalledTimes(2);
		expect(save).toHaveBeenNthCalledWith(2, "d", null);
		await act(async () => resolveSecond());
		expect(editor!.lastSavedRef.current).toBe("d");
		expect(editor!.isDirty).toBe(false);
		await act(async () => root.unmount());
	});

	it("awaits an in-flight save before deletion so persistence stays deleted", async () => {
		vi.useFakeTimers();
		let resolveSave!: () => void;
		const pendingSave = new Promise<void>((resolve) => (resolveSave = resolve));
		const mockDb = new Map<string, { markdown: string; json: string | null }>();
		const save = vi.fn(async (markdown: string, json: string | null) => {
			await pendingSave;
			mockDb.set("meeting-1", { markdown, json });
		});
		const deleteNotes = vi.fn(async () => {
			mockDb.delete("meeting-1");
		});
		let editor: ReturnType<typeof useNotesEditor> | null = null;

		function TestComp() {
			const e = useNotesEditor({ save, initialNotes: "a" });
			editor = e;
			return React.createElement("div", null, e.notes);
		}

		const container = document.createElement("div");
		const root = createRoot(container);
		await act(async () => root.render(React.createElement(TestComp)));
		await act(async () => {
			editor!.handleChange({ target: { value: "b" } } as React.ChangeEvent<HTMLTextAreaElement>);
			await vi.advanceTimersByTimeAsync(2000);
		});
		let flush!: Promise<void>;
		act(() => {
			editor!.handleChange({ target: { value: "c" } } as React.ChangeEvent<HTMLTextAreaElement>);
			editor!.cancelPendingSave();
			flush = editor!.flushPendingSave();
		});

		expect(mockDb.size).toBe(0);
		expect(deleteNotes).not.toHaveBeenCalled();
		await act(async () => {
			resolveSave();
			await flush;
			expect(mockDb.get("meeting-1")).toEqual({ markdown: "b", json: null });
			await deleteNotes();
			editor!.resetContent("", null);
		});
		expect(deleteNotes).toHaveBeenCalledOnce();
		expect(mockDb.size).toBe(0);
		expect(save).toHaveBeenCalledTimes(1);
		expect(editor!.notes).toBe("");
		expect(editor!.lastSavedRef.current).toBe("");
		await act(async () => root.unmount());
		vi.useRealTimers();
	});
});
