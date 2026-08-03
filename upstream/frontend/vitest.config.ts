import { defineConfig } from "vitest/config";
import { resolve } from "node:path";

export default defineConfig({
    test: {
        environment: "happy-dom",
        globals: false,
        include: [
            "tests/**/*.{test,spec}.{ts,tsx,js,jsx,mjs}",
            "src/**/*.{test,spec}.{ts,tsx,js,jsx}",
        ],
        setupFiles: ["./tests/setup.ts"],
    },
    resolve: {
        alias: {
            "@": resolve(__dirname, "src"),
        },
    },
});
