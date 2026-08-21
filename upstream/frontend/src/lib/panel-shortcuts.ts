export function togglePanelOnShortcut(event: KeyboardEvent, key: string, toggle: () => void) {
	if (
		document.activeElement?.closest("input, textarea, [contenteditable]") ||
		!(event.ctrlKey || event.metaKey) ||
		!event.shiftKey ||
		event.key.toLowerCase() !== key
	)
		return false;

	event.preventDefault();
	toggle();
	return true;
}
