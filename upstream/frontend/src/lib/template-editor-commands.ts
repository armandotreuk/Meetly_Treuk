// Tauri converts Rust snake_case command parameters to these camelCase IPC
// keys. Keep the names centralized so editor calls cannot drift independently.
export const TEMPLATE_EDITOR_TAURI_ARGS = {
    templateId: "templateId",
    templateSource: "templateSource",
    templateJson: "templateJson",
    schemaJson: "schemaJson",
} as const;
