"use client";

import { useState, useEffect, useCallback } from "react";
import { logger } from "@/lib/logger";

import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import {
    Plus,
    Trash2,
    Save,
    FileText,
    Loader2,
    AlertCircle,
    CheckCircle,
    Copy,
} from "lucide-react";
import {
    Dialog,
    DialogContent,
    DialogHeader,
    DialogTitle,
    DialogTrigger,
} from "@/components/ui/dialog";

interface Template {
    id: string;
    name: string;
    description: string;
    is_builtin: boolean;
    source: string;
}

interface TemplateDetails {
    id: string;
    name: string;
    description: string;
    sections: string[];
    is_builtin: boolean;
}

export function TemplateEditor() {
    const [templates, setTemplates] = useState<Template[]>([]);
    const [selectedTemplate, setSelectedTemplate] = useState<TemplateDetails | null>(null);
    const [isLoading, setIsLoading] = useState(true);
    const [isSaving, setIsSaving] = useState(false);
    const [showCreateDialog, setShowCreateDialog] = useState(false);
    const [editingTemplate, setEditingTemplate] = useState<TemplateDetails | null>(null);

    // Form state
    const [formName, setFormName] = useState("");
    const [formDescription, setFormDescription] = useState("");
    const [formSchemaJson, setFormSchemaJson] = useState("");

    // Load templates on mount
    const loadTemplates = useCallback(async () => {
        setIsLoading(true);
        try {
            const result = await invoke<Template[]>("api_list_templates");
            setTemplates(result);
        } catch (error) {
            logger.error("Failed to load templates:", error);
            toast.error("Failed to load templates");
        } finally {
            setIsLoading(false);
        }
    }, []);

    useEffect(() => {
        loadTemplates();
    }, [loadTemplates]);

    const handleViewTemplate = async (template: Template) => {
        try {
            const details = await invoke<TemplateDetails>("api_get_template_details", {
                templateId: template.id,
            });
            setSelectedTemplate(details);
            if (!template.is_builtin) {
                setEditingTemplate(details);
                setFormName(details.name);
                setFormDescription(details.description);
                // Fetch full schema JSON from database
                if (template.source === "database") {
                    // We need to get the full schema - for now use a placeholder
                    const dbTemplates = await invoke<any[]>("list_templates");
                    const dbTemplate = dbTemplates.find((t) => t.id === template.id);
                    if (dbTemplate) {
                        setFormSchemaJson(dbTemplate.schema_json);
                    }
                }
            }
        } catch (error) {
            logger.error("Failed to load template details:", error);
            toast.error("Failed to load template details");
        }
    };

    const handleCreateNew = () => {
        setEditingTemplate(null);
        setFormName("");
        setFormDescription("");
        setFormSchemaJson(
            JSON.stringify(
                {
                    name: "",
                    description: "",
                    sections: [
                        {
                            title: "Summary",
                            instruction: "Provide a brief summary of the meeting",
                            format: "paragraph",
                        },
                    ],
                },
                null,
                2
            )
        );
        setShowCreateDialog(true);
    };

    const handleEditTemplate = (template: Template) => {
        if (template.is_builtin) {
            toast.error("Cannot edit built-in templates");
            return;
        }
        handleViewTemplate(template);
        setShowCreateDialog(true);
    };

    const handleDeleteTemplate = async (template: Template) => {
        if (template.is_builtin) {
            toast.error("Cannot delete built-in templates");
            return;
        }
        if (!confirm(`Delete template "${template.name}"? This cannot be undone.`)) return;

        try {
            await invoke("delete_template", { id: parseInt(template.id) });
            toast.success("Template deleted");
            loadTemplates();
            if (selectedTemplate?.id === template.id) {
                setSelectedTemplate(null);
                setEditingTemplate(null);
            }
        } catch (error) {
            logger.error("Failed to delete template:", error);
            toast.error("Failed to delete template");
        }
    };

    const handleSaveTemplate = async () => {
        if (!formName.trim()) {
            toast.error("Template name is required");
            return;
        }

        // Validate JSON
        let parsedSchema;
        try {
            parsedSchema = JSON.parse(formSchemaJson);
        } catch (e) {
            toast.error("Invalid JSON in schema");
            return;
        }

        // Validate against template schema
        try {
            await invoke("api_validate_template", { templateJson: formSchemaJson });
        } catch (error) {
            toast.error(`Template validation failed: ${error}`);
            return;
        }

        setIsSaving(true);
        try {
            if (editingTemplate) {
                // Update existing template
                await invoke("update_template", {
                    id: parseInt(editingTemplate.id),
                    name: formName,
                    description: formDescription,
                    schema_json: formSchemaJson,
                });
                toast.success("Template updated");
            } else {
                // Create new template
                await invoke("create_template", {
                    name: formName,
                    description: formDescription,
                    schema_json: formSchemaJson,
                });
                toast.success("Template created");
            }
            setShowCreateDialog(false);
            loadTemplates();
        } catch (error) {
            logger.error("Failed to save template:", error);
            toast.error("Failed to save template");
        } finally {
            setIsSaving(false);
        }
    };

    const handleDuplicateTemplate = async (template: Template) => {
        try {
            const details = await invoke<TemplateDetails>("api_get_template_details", {
                templateId: template.id,
            });
            setEditingTemplate(null);
            setFormName(`${details.name} (Copy)`);
            setFormDescription(details.description);
            // Get full schema from database
            const dbTemplates = await invoke<any[]>("list_templates");
            const dbTemplate = dbTemplates.find((t) => t.id === template.id);
            setFormSchemaJson(
                dbTemplate?.schema_json ||
                    JSON.stringify(
                        {
                            name: details.name,
                            description: details.description,
                            sections: details.sections.map((s) => ({
                                title: s,
                                instruction: "",
                                format: "paragraph",
                            })),
                        },
                        null,
                        2
                    )
            );
            setShowCreateDialog(true);
        } catch (error) {
            logger.error("Failed to duplicate template:", error);
            toast.error("Failed to duplicate template");
        }
    };

    const handleCopySchemaJson = () => {
        navigator.clipboard.writeText(formSchemaJson);
        toast.success("Schema copied to clipboard");
    };

    const handleFormatJson = () => {
        try {
            const parsed = JSON.parse(formSchemaJson);
            setFormSchemaJson(JSON.stringify(parsed, null, 2));
        } catch {
            toast.error("Invalid JSON to format");
        }
    };

    return (
        <div className="h-full flex flex-col">
            {/* Toolbar */}
            <div className="flex items-center justify-between p-4 border-b border-gray-200 bg-white">
                <h2 className="text-lg font-semibold">Custom Templates</h2>
                <div className="flex items-center gap-2">
                    <Button variant="outline" size="sm" onClick={handleCreateNew}>
                        <Plus className="w-4 h-4 mr-1" />
                        New Template
                    </Button>
                </div>
            </div>

            {/* Main Content */}
            <div className="flex-1 flex overflow-hidden">
                {/* Template List */}
                <div className="w-72 border-r border-gray-200 bg-gray-50 overflow-y-auto p-4">
                    {isLoading ? (
                        <div className="flex items-center justify-center h-full">
                            <Loader2 className="w-6 h-6 animate-spin text-gray-400" />
                        </div>
                    ) : templates.length === 0 ? (
                        <div className="text-center text-gray-500 py-8">
                            <FileText className="w-12 h-12 mx-auto mb-2 opacity-50" />
                            <p>No templates found</p>
                            <p className="text-sm">Click "New Template" to create one</p>
                        </div>
                    ) : (
                        <div className="space-y-2">
                            {templates.map((template) => (
                                <div
                                    key={template.id}
                                    className={`p-3 rounded-lg cursor-pointer transition-colors ${
                                        selectedTemplate?.id === template.id
                                            ? "bg-blue-50 border border-blue-200"
                                            : "hover:bg-white hover:border-gray-200 border"
                                    }`}
                                    onClick={() => handleViewTemplate(template)}
                                >
                                    <div className="flex items-start justify-between gap-2">
                                        <div className="flex-1 min-w-0">
                                            <div className="flex items-center gap-2">
                                                <span className="font-medium truncate">
                                                    {template.name}
                                                </span>
                                                {template.is_builtin && (
                                                    <span className="text-xs px-1.5 py-0.5 bg-blue-100 text-blue-700 rounded">
                                                        Built-in
                                                    </span>
                                                )}
                                                {template.source === "database" &&
                                                    !template.is_builtin && (
                                                        <span className="text-xs px-1.5 py-0.5 bg-green-100 text-green-700 rounded">
                                                            Custom
                                                        </span>
                                                    )}
                                            </div>
                                            <p className="text-xs text-gray-500 truncate mt-1">
                                                {template.description}
                                            </p>
                                        </div>
                                        {!template.is_builtin && (
                                            <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                                                <Button
                                                    variant="ghost"
                                                    size="icon"
                                                    className="h-6 w-6"
                                                    onClick={(e) => {
                                                        e.stopPropagation();
                                                        handleDuplicateTemplate(template);
                                                    }}
                                                    title="Duplicate"
                                                >
                                                    <Copy className="w-3.5 h-3.5" />
                                                </Button>
                                                <Button
                                                    variant="ghost"
                                                    size="icon"
                                                    className="h-6 w-6 text-red-500 hover:text-red-700"
                                                    onClick={(e) => {
                                                        e.stopPropagation();
                                                        handleDeleteTemplate(template);
                                                    }}
                                                    title="Delete"
                                                >
                                                    <Trash2 className="w-3.5 h-3.5" />
                                                </Button>
                                            </div>
                                        )}
                                    </div>
                                </div>
                            ))}
                        </div>
                    )}
                </div>

                {/* Template Editor / Preview */}
                <div className="flex-1 flex flex-col overflow-hidden">
                    {selectedTemplate ? (
                        <>
                            <div className="p-4 border-b border-gray-200 bg-white">
                                <div className="flex items-center justify-between">
                                    <div>
                                        <h3 className="font-semibold">{selectedTemplate.name}</h3>
                                        <p className="text-sm text-gray-500">
                                            {selectedTemplate.description}
                                        </p>
                                    </div>
                                    <div className="flex items-center gap-2">
                                        {selectedTemplate.is_builtin && (
                                            <span className="text-xs px-2 py-1 bg-blue-100 text-blue-700 rounded-full">
                                                Built-in (read-only)
                                            </span>
                                        )}
                                        {!editingTemplate && !selectedTemplate.is_builtin && (
                                            <Button
                                                variant="outline"
                                                size="sm"
                                                onClick={() =>
                                                    handleEditTemplate({
                                                        ...selectedTemplate,
                                                        is_builtin: false,
                                                    } as any)
                                                }
                                            >
                                                Edit
                                            </Button>
                                        )}
                                    </div>
                                </div>
                            </div>

                            {editingTemplate ? (
                                /* Edit Mode */
                                <div className="flex-1 p-4 overflow-y-auto">
                                    <div className="max-w-3xl mx-auto space-y-6">
                                        <div>
                                            <Label htmlFor="template-name">Template Name</Label>
                                            <Input
                                                id="template-name"
                                                value={formName}
                                                onChange={(e) => setFormName(e.target.value)}
                                                placeholder="My Custom Template"
                                                className="mt-1"
                                            />
                                        </div>

                                        <div>
                                            <Label htmlFor="template-description">
                                                Description
                                            </Label>
                                            <Textarea
                                                id="template-description"
                                                value={formDescription}
                                                onChange={(e) => setFormDescription(e.target.value)}
                                                placeholder="Brief description of what this template is for"
                                                rows={2}
                                                className="mt-1"
                                            />
                                        </div>

                                        <div>
                                            <div className="flex items-center justify-between">
                                                <Label htmlFor="template-schema">
                                                    Schema (JSON)
                                                </Label>
                                                <div className="flex items-center gap-2">
                                                    <Button
                                                        variant="ghost"
                                                        size="icon"
                                                        onClick={handleFormatJson}
                                                        title="Format JSON"
                                                    >
                                                        <FileText className="w-4 h-4" />
                                                    </Button>
                                                    <Button
                                                        variant="ghost"
                                                        size="icon"
                                                        onClick={handleCopySchemaJson}
                                                        title="Copy JSON"
                                                    >
                                                        <Copy className="w-4 h-4" />
                                                    </Button>
                                                </div>
                                            </div>
                                            <Textarea
                                                id="template-schema"
                                                value={formSchemaJson}
                                                onChange={(e) => setFormSchemaJson(e.target.value)}
                                                placeholder='{"name": "...", "description": "...", "sections": [...]}'
                                                rows={20}
                                                className="mt-1 font-mono text-sm"
                                                spellCheck={false}
                                            />
                                            <p className="text-xs text-gray-500 mt-1">
                                                Define sections with title, instruction, format
                                                (paragraph|list|string), and optional item_format.
                                            </p>
                                        </div>

                                        <div className="flex justify-end gap-2 pt-4 border-t border-gray-200">
                                            <Button
                                                variant="outline"
                                                onClick={() => {
                                                    setShowCreateDialog(false);
                                                    setEditingTemplate(null);
                                                }}
                                            >
                                                Cancel
                                            </Button>
                                            <Button
                                                onClick={handleSaveTemplate}
                                                disabled={isSaving}
                                            >
                                                {isSaving ? (
                                                    <Loader2 className="w-4 h-4 animate-spin mr-2" />
                                                ) : (
                                                    <Save className="w-4 h-4 mr-2" />
                                                )}
                                                {editingTemplate
                                                    ? "Update Template"
                                                    : "Create Template"}
                                            </Button>
                                        </div>
                                    </div>
                                </div>
                            ) : (
                                /* Preview Mode */
                                <div className="flex-1 p-4 overflow-y-auto">
                                    <div className="max-w-3xl mx-auto space-y-6">
                                        <div className="prose prose-sm max-w-none">
                                            <h3>Sections</h3>
                                            <ol className="list-decimal list-inside space-y-4">
                                                {selectedTemplate.sections.map(
                                                    (sectionTitle, index) => (
                                                        <li
                                                            key={index}
                                                            className="p-4 bg-gray-50 rounded-lg border border-gray-200"
                                                        >
                                                            <span className="font-medium">
                                                                {sectionTitle}
                                                            </span>
                                                        </li>
                                                    )
                                                )}
                                            </ol>
                                        </div>

                                        <div className="flex justify-end gap-2 pt-4 border-t border-gray-200">
                                            {!selectedTemplate.is_builtin && (
                                                <Button
                                                    variant="outline"
                                                    onClick={() =>
                                                        handleEditTemplate({
                                                            ...selectedTemplate,
                                                            is_builtin: false,
                                                        } as any)
                                                    }
                                                >
                                                    Edit Template
                                                </Button>
                                            )}
                                            <Button
                                                variant="outline"
                                                onClick={() =>
                                                    navigator.clipboard.writeText(
                                                        JSON.stringify(
                                                            {
                                                                name: selectedTemplate.name,
                                                                description:
                                                                    selectedTemplate.description,
                                                                sections:
                                                                    selectedTemplate.sections.map(
                                                                        (s) => ({
                                                                            title: s,
                                                                            instruction: "",
                                                                            format: "paragraph",
                                                                        })
                                                                    ),
                                                            },
                                                            null,
                                                            2
                                                        )
                                                    )
                                                }
                                            >
                                                <Copy className="w-4 h-4 mr-2" />
                                                Copy as JSON
                                            </Button>
                                        </div>
                                    </div>
                                </div>
                            )}
                        </>
                    ) : (
                        <div className="flex-1 flex items-center justify-center text-gray-500">
                            <div className="text-center">
                                <FileText className="w-16 h-16 mx-auto mb-4 opacity-30" />
                                <p>Select a template from the list to view or edit</p>
                                <p className="text-sm">
                                    Or click "New Template" to create a custom one
                                </p>
                            </div>
                        </div>
                    )}
                </div>
            </div>

            {/* Create/Edit Dialog */}
            <Dialog open={showCreateDialog} onOpenChange={setShowCreateDialog}>
                <DialogContent className="max-w-4xl max-h-[90vh]">
                    <DialogHeader>
                        <DialogTitle>
                            {editingTemplate ? "Edit Template" : "Create New Template"}
                        </DialogTitle>
                    </DialogHeader>
                    <div className="max-w-3xl mx-auto space-y-6 py-4">
                        <div>
                            <Label htmlFor="dialog-template-name">Template Name</Label>
                            <Input
                                id="dialog-template-name"
                                value={formName}
                                onChange={(e) => setFormName(e.target.value)}
                                placeholder="My Custom Template"
                                className="mt-1"
                                autoFocus
                            />
                        </div>

                        <div>
                            <Label htmlFor="dialog-template-description">Description</Label>
                            <Textarea
                                id="dialog-template-description"
                                value={formDescription}
                                onChange={(e) => setFormDescription(e.target.value)}
                                placeholder="Brief description of what this template is for"
                                rows={2}
                                className="mt-1"
                            />
                        </div>

                        <div>
                            <div className="flex items-center justify-between">
                                <Label htmlFor="dialog-template-schema">Schema (JSON)</Label>
                                <div className="flex items-center gap-2">
                                    <Button
                                        variant="ghost"
                                        size="icon"
                                        onClick={handleFormatJson}
                                        title="Format JSON"
                                    >
                                        <FileText className="w-4 h-4" />
                                    </Button>
                                    <Button
                                        variant="ghost"
                                        size="icon"
                                        onClick={handleCopySchemaJson}
                                        title="Copy JSON"
                                    >
                                        <Copy className="w-4 h-4" />
                                    </Button>
                                </div>
                            </div>
                            <Textarea
                                id="dialog-template-schema"
                                value={formSchemaJson}
                                onChange={(e) => setFormSchemaJson(e.target.value)}
                                placeholder='{"name": "...", "description": "...", "sections": [...]}'
                                rows={25}
                                className="mt-1 font-mono text-sm"
                                spellCheck={false}
                            />
                            <p className="text-xs text-gray-500 mt-1">
                                Define sections with title, instruction, format
                                (paragraph|list|string), and optional item_format.
                                <br />
                                Example item_format:{" "}
                                <code className="bg-gray-100 px-1 rounded">
                                    | **Owner** | **Task** | **Due** |\n| --- | --- | --- |
                                </code>
                            </p>
                        </div>

                        <div className="flex justify-end gap-2 pt-4 border-t border-gray-200">
                            <Button
                                variant="outline"
                                onClick={() => {
                                    setShowCreateDialog(false);
                                    setEditingTemplate(null);
                                }}
                            >
                                Cancel
                            </Button>
                            <Button onClick={handleSaveTemplate} disabled={isSaving}>
                                {isSaving ? (
                                    <Loader2 className="w-4 h-4 animate-spin mr-2" />
                                ) : (
                                    <Save className="w-4 h-4 mr-2" />
                                )}
                                {editingTemplate ? "Update Template" : "Create Template"}
                            </Button>
                        </div>
                    </div>
                </DialogContent>
            </Dialog>
        </div>
    );
}
