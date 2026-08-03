import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { logger } from "../../src/lib/logger";

describe("logger", () => {
    let originalEnv: string | undefined;
    let originalLevel: string | null;
    let consoleLog: ReturnType<typeof vi.spyOn>;
    let consoleInfo: ReturnType<typeof vi.spyOn>;
    let consoleWarn: ReturnType<typeof vi.spyOn>;
    let consoleError: ReturnType<typeof vi.spyOn>;

    beforeEach(() => {
        originalEnv = process.env.NODE_ENV;
        originalLevel = window.localStorage.getItem("meetily.logLevel");
        window.localStorage.removeItem("meetily.logLevel");
        logger.setLevel(null);
        consoleLog = vi.spyOn(console, "log").mockImplementation(() => {});
        consoleInfo = vi.spyOn(console, "info").mockImplementation(() => {});
        consoleWarn = vi.spyOn(console, "warn").mockImplementation(() => {});
        consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    });

    afterEach(() => {
        vi.restoreAllMocks();
        if (originalEnv === undefined) delete process.env.NODE_ENV;
        else process.env.NODE_ENV = originalEnv;
        if (originalLevel) window.localStorage.setItem("meetily.logLevel", originalLevel);
    });

    test("debug and info pass through at debug level", () => {
        logger.setLevel("debug");
        logger.debug("d");
        logger.info("i");
        expect(consoleLog).toHaveBeenCalledWith("d");
        expect(consoleInfo).toHaveBeenCalledWith("i");
    });

    test("debug is suppressed at warn level", () => {
        logger.setLevel("warn");
        logger.debug("d");
        logger.info("i");
        logger.warn("w");
        logger.error("e");
        expect(consoleLog).not.toHaveBeenCalled();
        expect(consoleInfo).not.toHaveBeenCalled();
        expect(consoleWarn).toHaveBeenCalledWith("w");
        expect(consoleError).toHaveBeenCalledWith("e");
    });

    test("silent level suppresses everything", () => {
        logger.setLevel("silent");
        logger.error("boom");
        expect(consoleError).not.toHaveBeenCalled();
    });
});
