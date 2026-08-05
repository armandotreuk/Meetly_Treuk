export function normalizeTemplateId(id: string | number, source?: string): string {
    const raw = String(id);
    if (/^\d+$/.test(raw)) {
        const numeric = Number(raw);
        if (Number.isSafeInteger(numeric) && numeric > 0) {
            return ["builtin", "bundled", "custom", "file"].includes(source ?? "")
                ? `file:${numeric}`
                : `db:${numeric}`;
        }
    }
    return raw;
}

export function databaseTemplateRowId(id: string | number): number {
    const raw = String(id).startsWith("db:") ? String(id).slice(3) : String(id);
    const parsed = Number(raw);
    if (!Number.isSafeInteger(parsed) || parsed < 1) {
        throw new Error("Invalid database template ID");
    }
    return parsed;
}
