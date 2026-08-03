// Lightweight wrapper around `console` that gates diagnostic output by
// environment. Use:
//   - logger.debug — verbose state output, suppressed in production
//   - logger.info  — notable lifecycle events, suppressed in production
//   - logger.warn  — recoverable problems, always emitted
//   - logger.error — failures, always emitted
//
// To change verbosity at runtime, set localStorage "meetily.logLevel".

export type LogLevel = "debug" | "info" | "warn" | "error" | "silent";

const ORDER: Record<LogLevel, number> = {
    debug: 10,
    info: 20,
    warn: 30,
    error: 40,
    silent: 100,
};

function isProduction(): boolean {
    return process.env.NODE_ENV === "production";
}

function readOverride(): LogLevel | null {
    if (typeof window === "undefined") return null;
    try {
        const stored = window.localStorage?.getItem("meetily.logLevel");
        if (stored && stored in ORDER) return stored as LogLevel;
    } catch {
        // ignore
    }
    return null;
}

let override: LogLevel | null = null;

function currentLevel(): LogLevel {
    if (override) return override;
    override = readOverride();
    return override ?? (isProduction() ? "warn" : "debug");
}

export const logger = {
    setLevel(level: LogLevel | null) {
        override = level;
    },
    getLevel(): LogLevel {
        return currentLevel();
    },
    debug(...args: unknown[]) {
        if (ORDER[currentLevel()] > ORDER.debug) return;
        // eslint-disable-next-line no-console
        console.log(...args);
    },
    info(...args: unknown[]) {
        if (ORDER[currentLevel()] > ORDER.info) return;
        // eslint-disable-next-line no-console
        console.info(...args);
    },
    warn(...args: unknown[]) {
        if (ORDER[currentLevel()] > ORDER.warn) return;
        // eslint-disable-next-line no-console
        console.warn(...args);
    },
    error(...args: unknown[]) {
        if (ORDER[currentLevel()] > ORDER.error) return;
        // eslint-disable-next-line no-console
        console.error(...args);
    },
};
