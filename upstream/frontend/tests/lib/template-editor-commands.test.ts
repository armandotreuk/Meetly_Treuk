import { describe, expect, it } from "vitest";

import { TEMPLATE_EDITOR_TAURI_ARGS } from "@/lib/template-editor-commands";

describe("TemplateEditor Tauri argument names", () => {
    it("keeps Rust snake_case arguments on Tauri's camelCase wire format", () => {
        expect(TEMPLATE_EDITOR_TAURI_ARGS).toEqual({
            templateId: "templateId",
            templateSource: "templateSource",
            templateJson: "templateJson",
            schemaJson: "schemaJson",
        });
    });
});
