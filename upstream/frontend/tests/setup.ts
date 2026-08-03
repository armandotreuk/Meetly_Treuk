// Polyfill localStorage for happy-dom (needed by tests that use window.localStorage directly)
import { Window } from "happy-dom";

const window = new Window();
const localStorage = window.localStorage;

if (typeof globalThis.localStorage === "undefined") {
    Object.defineProperty(globalThis, "localStorage", {
        configurable: true,
        get: () => localStorage,
        set: () => {},
    });
}
