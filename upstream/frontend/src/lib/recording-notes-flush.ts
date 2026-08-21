interface FlushEntry {
	registered: (() => Promise<void>) | null;
	detached: (() => Promise<void>) | null;
	last: Promise<void> | null;
}

const entries = new Map<string, FlushEntry>();

function captureFlush(entry: FlushEntry, flush: () => Promise<void>): Promise<void> {
	const promise = flush();
	entry.last = promise;
	// The stop path still receives the original rejecting promise; this passive
	// handler only prevents an unhandled rejection during renderer unmount.
	void promise.catch(() => {});
	return promise;
}

// ponytail: keyed side-channel avoids prop-drilling editor state into recording
// controls. The stable native scope key prevents delayed recording A work from
// being consumed by recording B.
export function registerRecordingNotesFlush(
	recordingScopeKey: string | null,
	flush: () => Promise<void>
): () => void {
	if (!recordingScopeKey) return () => {};
	const entry: FlushEntry = { registered: flush, detached: null, last: null };
	entries.set(recordingScopeKey, entry);
	return () => {
		if (entries.get(recordingScopeKey) !== entry || entry.registered !== flush) return;
		entry.registered = null;
		entry.detached = flush;
		captureFlush(entry, flush);
	};
}

export function flushRecordingNotes(recordingScopeKey: string | null): Promise<void> {
	if (!recordingScopeKey) return Promise.resolve();
	const entry = entries.get(recordingScopeKey);
	if (!entry) return Promise.resolve();
	const flush = entry.registered ?? entry.detached;
	return flush ? captureFlush(entry, flush) : entry.last ?? Promise.resolve();
}

export function releaseRecordingNotesFlush(recordingScopeKey: string | null): void {
	if (recordingScopeKey) entries.delete(recordingScopeKey);
}
