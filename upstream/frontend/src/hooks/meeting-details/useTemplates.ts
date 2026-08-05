import { useState, useEffect, useCallback } from "react";
import { logger } from "@/lib/logger";
import { normalizeTemplateId } from "@/lib/template-ids";

import { invoke as invokeTauri } from "@tauri-apps/api/core";
import { toast } from "sonner";
import Analytics from "@/lib/analytics";

export function useTemplates() {
    const [availableTemplates, setAvailableTemplates] = useState<
        Array<{
            id: string;
            name: string;
            description: string;
            source?: string;
        }>
    >([]);
    const [selectedTemplate, setSelectedTemplate] = useState<string>("standard_meeting");

    // Fetch available templates on mount
    useEffect(() => {
        const fetchTemplates = async () => {
            try {
                const templates = (await invokeTauri("api_list_templates")) as Array<{
                    id: string | number;
                    name: string;
                    description: string;
                    source?: string;
                }>;
                logger.debug("Available templates:", templates);
                setAvailableTemplates(
                    templates.map((template) => ({
                        id: normalizeTemplateId(template.id, template.source),
                        name: template.name,
                        description: template.description,
                        source: template.source,
                    }))
                );
            } catch (error) {
                logger.error("Failed to fetch templates:", error);
            }
        };
        fetchTemplates();
    }, []);

    // Handle template selection
    const handleTemplateSelection = useCallback((templateId: string, templateName: string) => {
        setSelectedTemplate(templateId);
        toast.success("Template selected", {
            description: `Using "${templateName}" template for summary generation`,
        });
        Analytics.trackFeatureUsed("template_selected");
    }, []);

    return {
        availableTemplates,
        selectedTemplate,
        handleTemplateSelection,
    };
}
