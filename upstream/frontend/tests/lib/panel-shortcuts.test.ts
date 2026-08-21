import { describe, expect, it, vi } from "vitest";
import { togglePanelOnShortcut } from "@/lib/panel-shortcuts";

describe("togglePanelOnShortcut", () => {
	it("toggles outside text editing but not inside it", () => {
		const toggle = vi.fn();
		const shortcut = () => new KeyboardEvent("keydown", { key: "N", ctrlKey: true, shiftKey: true });

		expect(togglePanelOnShortcut(shortcut(), "n", toggle)).toBe(true);
		expect(toggle).toHaveBeenCalledTimes(1);

		const input = document.createElement("input");
		document.body.append(input);
		input.focus();
		expect(togglePanelOnShortcut(shortcut(), "n", toggle)).toBe(false);
		expect(toggle).toHaveBeenCalledTimes(1);
		input.remove();

		const editor = document.createElement("div");
		editor.setAttribute("contenteditable", "true");
		editor.tabIndex = -1;
		document.body.append(editor);
		editor.focus();
		expect(togglePanelOnShortcut(shortcut(), "n", toggle)).toBe(false);
		expect(toggle).toHaveBeenCalledTimes(1);
		editor.remove();
	});
});
