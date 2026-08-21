import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { persistDraft, RecordingNotesPanel } from "@/components/RecordingNotesPanel";
import { RecordingControls } from "@/components/RecordingControls";
import {
	flushRecordingNotes,
	registerRecordingNotesFlush,
	releaseRecordingNotesFlush,
} from "@/lib/recording-notes-flush";

const mocks = vi.hoisted(() => ({
	title: "Meeting 15_08_26_12_00_00",
	scopeKey: "live-scope-a",
	invoke: vi.fn(),
	convert: vi.fn(),
	parse: vi.fn(),
	setStatus: vi.fn(),
	toastError: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@/contexts/RecordingStateContext", () => ({
	RecordingStatus: { RECORDING: "recording" },
	useRecordingState: () => ({
		isPaused: false,
		setStatus: mocks.setStatus,
		liveTranscriptScopeKey: mocks.scopeKey,
	}),
}));
vi.mock("sonner", () => ({ toast: { error: mocks.toastError } }));
vi.mock("@/contexts/TranscriptContext", () => ({
	useTranscripts: () => ({ meetingTitle: mocks.title }),
}));
vi.mock("@blocknote/react", () => {
	const parser = { tryParseMarkdownToBlocks: mocks.parse };
	return { useCreateBlockNote: () => parser };
});
vi.mock("@/components/notes/NotesEditorShell", () => ({
	NotesEditorShell: (props: any) => {
		props.markdownEditorRef.current = { blocksToMarkdownLossy: mocks.convert };
		return (
			<div>
				<button onClick={() => props.onBlocksChange([{ id: "latest", type: "paragraph" }])}>
					Edit notes
				</button>
				<div data-testid="initial-blocks">{JSON.stringify(props.initialBlocks)}</div>
			</div>
		);
	},
}));

function deferred<T>() {
	let resolve!: (value: T) => void;
	const promise = new Promise<T>((done) => (resolve = done));
	return { promise, resolve };
}

async function editThenStop(importNotes = vi.fn()) {
	(document.querySelector("button") as HTMLButtonElement).click();
	await flushRecordingNotes(mocks.scopeKey);
	await importNotes();
	return importNotes;
}

describe("persistDraft", () => {
	afterEach(() => {
		vi.restoreAllMocks();
		sessionStorage.clear();
	});

	it("stores a rich draft that can be read back", () => {
		const payload = { markdown: "# Notes", blocksJson: '[{"type":"heading"}]' };

		expect(persistDraft("draft", payload)).toBe(true);
		expect(JSON.parse(sessionStorage.getItem("draft")!)).toEqual(payload);
	});

	it("retries with markdown only when the rich payload exceeds quota", () => {
		const setItem = sessionStorage.setItem.bind(sessionStorage);
		vi.spyOn(sessionStorage, "setItem").mockImplementation((key, value) => {
			if (value.includes('\"blocksJson\":\"')) throw new DOMException("QuotaExceededError");
			return setItem(key, value);
		});

		expect(persistDraft("draft", { markdown: "recoverable", blocksJson: "x".repeat(100) })).toBe(true);
		expect(JSON.parse(sessionStorage.getItem("draft")!)).toEqual({
			markdown: "recoverable",
			blocksJson: null,
		});
	});

	it("returns false without throwing when storage is unavailable", () => {
		vi.spyOn(sessionStorage, "setItem").mockImplementation(() => {
			throw new DOMException("QuotaExceededError");
		});

		expect(persistDraft("draft", { markdown: "notes", blocksJson: "[]" })).toBe(false);
	});

	it("keeps quota-fallback markdown parseable on reload", async () => {
		const setItem = sessionStorage.setItem.bind(sessionStorage);
		vi.spyOn(sessionStorage, "setItem").mockImplementation((key, value) => {
			if (value.includes('\"blocksJson\":\"')) throw new DOMException("QuotaExceededError");
			return setItem(key, value);
		});
		const parser = { tryParseMarkdownToBlocks: vi.fn().mockResolvedValue([{ type: "paragraph" }]) };

		persistDraft("draft", { markdown: "Reload me", blocksJson: "large-json" });
		const stored = JSON.parse(sessionStorage.getItem("draft")!);

		expect(await parser.tryParseMarkdownToBlocks(stored.markdown)).toEqual([{ type: "paragraph" }]);
		expect(parser.tryParseMarkdownToBlocks).toHaveBeenCalledWith("Reload me");
	});
});

describe("RecordingNotesPanel stop races", () => {
	let root: Root;
	let container: HTMLDivElement;
	const mount = async () => {
		container = document.createElement("div");
		document.body.appendChild(container);
		root = createRoot(container);
		await act(async () => root.render(<RecordingNotesPanel onClose={() => {}} />));
	};

	beforeEach(() => {
		mocks.title = "Meeting 15_08_26_12_00_00";
		mocks.scopeKey = "live-scope-a";
		mocks.invoke.mockReset().mockResolvedValue(undefined);
		mocks.convert.mockReset().mockResolvedValue("latest markdown");
		mocks.parse.mockReset().mockResolvedValue([]);
		mocks.setStatus.mockReset();
		mocks.toastError.mockReset();
	});

	afterEach(() => {
		if (root) act(() => root.unmount());
		container?.remove();
		sessionStorage.clear();
		releaseRecordingNotesFlush(mocks.scopeKey);
		vi.useRealTimers();
	});

	it("flushes the latest component edit before post-stop import", async () => {
		const order: string[] = [];
		mocks.invoke.mockImplementation(async (command) => {
			if (command === "save_recording_notes") order.push("disk");
		});
		const importNotes = vi.fn(async () => order.push("import"));
		await mount();

		await act(async () => void (await editThenStop(importNotes)));

		expect(mocks.invoke).toHaveBeenCalledWith("save_recording_notes", {
			notes: "latest markdown",
			blocksJson: JSON.stringify([{ id: "latest", type: "paragraph" }]),
			recordingScopeKey: mocks.scopeKey,
		});
		expect(order).toEqual(["disk", "import"]);
	});

	it("waits for an in-flight markdown conversion before crossing the stop boundary", async () => {
		const conversion = deferred<string>();
		mocks.convert.mockReturnValue(conversion.promise);
		await mount();
		act(() => (container.querySelector("button") as HTMLButtonElement).click());

		let stopped = false;
		const stop = flushRecordingNotes(mocks.scopeKey).then(() => (stopped = true));
		await Promise.resolve();
		expect(stopped).toBe(false);
		conversion.resolve("converted latest");
		await act(async () => stop);

		expect(stopped).toBe(true);
		expect(mocks.invoke).toHaveBeenCalledWith("save_recording_notes", expect.objectContaining({
			notes: "converted latest",
		}));
	});

	it("cancels and flushes a stop within the two-second debounce", async () => {
		vi.useFakeTimers();
		const order: string[] = [];
		mocks.invoke.mockImplementation(async () => void order.push("disk"));
		await mount();
		act(() => (container.querySelector("button") as HTMLButtonElement).click());
		await vi.advanceTimersByTimeAsync(1900);
		expect(mocks.invoke).not.toHaveBeenCalled();

		await act(async () => {
			await flushRecordingNotes(mocks.scopeKey);
			order.push("manager-teardown");
		});
		await vi.advanceTimersByTimeAsync(200);

		expect(order).toEqual(["disk", "manager-teardown"]);
		expect(mocks.invoke).toHaveBeenCalledTimes(1);
	});

	it("keeps a tray-stop flush alive after the panel unmounts", async () => {
		vi.useFakeTimers();
		const conversion = deferred<string>();
		const order: string[] = [];
		mocks.convert.mockReturnValue(conversion.promise);
		mocks.invoke.mockImplementation(async () => void order.push("disk"));
		await mount();
		act(() => (container.querySelector("button") as HTMLButtonElement).click());
		await vi.advanceTimersByTimeAsync(1900);

		await act(async () => root.render(<div />));
		order.push("manager-teardown");
		const postStop = flushRecordingNotes(mocks.scopeKey).then(() => void order.push("import"));
		await Promise.resolve();
		expect(order).toEqual(["manager-teardown"]);

		conversion.resolve("latest tray notes");
		await act(async () => postStop);

		expect(order).toEqual(["manager-teardown", "disk", "import"]);
		expect(mocks.invoke).toHaveBeenCalledWith("save_recording_notes", expect.objectContaining({
			notes: "latest tray notes",
			recordingScopeKey: mocks.scopeKey,
		}));
	});

	it("keeps detached flushes isolated and retryable by recording scope", async () => {
		const flushA = vi
			.fn<() => Promise<void>>()
			.mockRejectedValueOnce(new Error("temporary disk failure"))
			.mockResolvedValue(undefined);
		const flushB = vi.fn<() => Promise<void>>().mockResolvedValue(undefined);
		const unregisterA = registerRecordingNotesFlush("scope-a", flushA);
		unregisterA();
		registerRecordingNotesFlush("scope-b", flushB);
		await vi.waitFor(() => expect(flushA).toHaveBeenCalledTimes(1));

		await flushRecordingNotes("scope-a");

		expect(flushA).toHaveBeenCalledTimes(2);
		expect(flushB).not.toHaveBeenCalled();
		releaseRecordingNotesFlush("scope-a");
		releaseRecordingNotesFlush("scope-b");
	});

	it("hydrates a real-title draft after mounting under the placeholder title", async () => {
		const blocks = [{ id: "hydrated", type: "paragraph" }];
		sessionStorage.setItem(
			"recording_notes_draft:Real Meeting",
			JSON.stringify({ markdown: "hydrated", blocksJson: JSON.stringify(blocks) })
		);
		mocks.title = "+ New Call";
		await mount();
		expect(container.querySelector("[data-testid='initial-blocks']")!.textContent).toBe("null");

		mocks.title = "Real Meeting";
		await act(async () => root.render(<RecordingNotesPanel onClose={() => {}} />));

		expect(container.querySelector("[data-testid='initial-blocks']")!.textContent).toBe(JSON.stringify(blocks));
	});

	it("keeps recording active when notes fail to flush twice before stop", async () => {
		const onRecordingStop = vi.fn();
		const onStopInitiated = vi.fn();
		mocks.invoke.mockImplementation(async (command) => {
			if (command === "save_recording_notes") throw new Error("disk unavailable");
		});
		container = document.createElement("div");
		document.body.appendChild(container);
		root = createRoot(container);
		await act(async () => root.render(
			<>
				<RecordingNotesPanel onClose={() => {}} />
				<RecordingControls
					isRecording
					barHeights={[]}
					onRecordingStop={onRecordingStop}
					onRecordingStart={async () => {}}
					onTranscriptReceived={() => {}}
					onStopInitiated={onStopInitiated}
					isRecordingDisabled={false}
					isParentProcessing={false}
				/>
			</>
		));

		const buttons = container.querySelectorAll("button");
		await act(async () => buttons[0].click());
		await act(async () => (container.querySelectorAll("button")[2] as HTMLButtonElement).click());
		await vi.waitFor(() => expect(
			mocks.invoke.mock.calls.filter(([command]) => command === "save_recording_notes")
		).toHaveLength(2));

		expect(mocks.invoke).not.toHaveBeenCalledWith("stop_recording", expect.anything());
		expect(onRecordingStop).not.toHaveBeenCalled();
		expect(mocks.toastError).toHaveBeenCalledWith(
			"Couldn't save notes before stopping. Try again or stop without saving."
		);
		expect(mocks.setStatus).toHaveBeenCalledWith("recording");

		expect((container.querySelectorAll("button")[2] as HTMLButtonElement).disabled).toBe(false);
	});
});
