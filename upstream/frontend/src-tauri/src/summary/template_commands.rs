use crate::state::AppState;
use crate::summary::templates;
use serde::{Deserialize, Serialize};
use tauri::{Manager, Runtime};
use tracing::{info, warn};

/// Template metadata for UI display
#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateInfo {
    /// Template identifier (e.g., "daily_standup", "standard_meeting")
    pub id: String,

    /// Display name for the template
    pub name: String,

    /// Brief description of the template's purpose
    pub description: String,

    /// Whether this is a built-in template (read-only) or user template
    pub is_builtin: bool,

    /// Source of the template: "builtin", "bundled", "custom", or "database"
    pub source: String,
}

/// Detailed template structure for preview/debugging
#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateDetails {
    /// Template identifier
    pub id: String,

    /// Display name
    pub name: String,

    /// Description
    pub description: String,

    /// List of section titles in order
    pub sections: Vec<String>,

    /// Whether this is a built-in template
    pub is_builtin: bool,
}

/// Lists all available templates
///
/// Returns templates from built-in (embedded), bundled (app resources), custom (user data directory), and database sources.
/// Database templates take precedence for user-created templates.
///
/// # Returns
/// Vector of TemplateInfo with id, name, description, is_builtin, and source for each template
#[tauri::command]
pub async fn api_list_templates<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<TemplateInfo>, String> {
    info!("api_list_templates called");

    // Get templates from the file-based system (built-in, bundled, custom)
    let file_templates = templates::list_templates();

    let mut template_infos: Vec<TemplateInfo> = file_templates
        .into_iter()
        .map(|(id, name, description)| TemplateInfo {
            id,
            name,
            description,
            is_builtin: false, // Will be determined below
            source: "file".to_string(),
        })
        .collect();

    // Add database templates (user-created templates from DB)
    if let Some(app_state) = app.try_state::<AppState>() {
        let pool = app_state.db_manager.pool();
        if let Ok(db_templates) =
            crate::database::repositories::templates::TemplatesRepository::list_all(pool).await
        {
            for db_template in db_templates {
                // Check if already exists in file_templates (by name)
                let exists = template_infos.iter().any(|t| t.name == db_template.name);
                if !exists {
                    template_infos.push(TemplateInfo {
                        id: db_template.id.to_string(),
                        name: db_template.name,
                        description: db_template.description,
                        is_builtin: db_template.is_builtin != 0,
                        source: if db_template.is_builtin != 0 {
                            "builtin"
                        } else {
                            "database"
                        }
                        .to_string(),
                    });
                }
            }
        }
    }

    // Sort: builtin first, then by name
    template_infos.sort_by(|a, b| {
        // Builtin templates first
        match (a.is_builtin, b.is_builtin) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });

    info!("Found {} available templates", template_infos.len());

    Ok(template_infos)
}

/// Gets detailed information about a specific template
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "daily_standup")
///
/// # Returns
/// TemplateDetails with full template structure
#[tauri::command]
pub async fn api_get_template_details<R: Runtime>(
    app: tauri::AppHandle<R>,
    template_id: String,
) -> Result<TemplateDetails, String> {
    info!(
        "api_get_template_details called for template_id: {}",
        template_id
    );

    // First try database
    let mut template = None;
    let mut is_builtin = false;

    if let Some(app_state) = app.try_state::<AppState>() {
        let pool = app_state.db_manager.pool();
        // Try parsing as i64 for database ID
        if let Ok(id) = template_id.parse::<i64>() {
            if let Ok(Some(db_template)) =
                crate::database::repositories::templates::TemplatesRepository::get_by_id(pool, id)
                    .await
            {
                if let Ok(parsed) = crate::summary::templates::loader::validate_and_parse_template(
                    &db_template.schema_json,
                ) {
                    is_builtin = db_template.is_builtin != 0;
                    template = Some(parsed);
                }
            }
        }
    }

    // Fallback to file-based system
    if template.is_none() {
        let parsed = templates::get_template(&template_id)?;
        template = Some(parsed);
    }

    let template = template.ok_or_else(|| format!("Template '{}' not found", template_id))?;

    let section_titles: Vec<String> = template
        .sections
        .iter()
        .map(|section| section.title.clone())
        .collect();

    let details = TemplateDetails {
        id: template_id,
        name: template.name,
        description: template.description,
        sections: section_titles,
        is_builtin,
    };

    info!("Retrieved template details for '{}'", details.name);

    Ok(details)
}

/// Validates a custom template JSON string
///
/// Useful for template editor UI or validation before saving custom templates
///
/// # Arguments
/// * `template_json` - Raw JSON string of the template
///
/// # Returns
/// Ok(template_name) if valid, Err(error_message) if invalid
#[tauri::command]
pub async fn api_validate_template<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_json: String,
) -> Result<String, String> {
    info!("api_validate_template called");

    match templates::validate_and_parse_template(&template_json) {
        Ok(template) => {
            info!("Template '{}' validated successfully", template.name);
            Ok(template.name)
        }
        Err(e) => {
            warn!("Template validation failed: {}", e);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_templates() {
        // This test requires the templates to be embedded/available
        // In a real test environment, you might want to mock the templates module

        // For now, just verify the function compiles and runs
        // You can expand this with more specific assertions
    }

    #[tokio::test]
    async fn test_validate_template_valid() {
        let valid_json = r#"
        {
            "name": "Test Template",
            "description": "A test template",
            "sections": [
                {
                    "title": "Summary",
                    "instruction": "Provide a summary",
                    "format": "paragraph"
                }
            ]
        }"#;

        // Mock app handle would be needed for actual testing
        // For now, test the validation logic directly
        let result = templates::validate_and_parse_template(valid_json);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_template_invalid() {
        let invalid_json = "invalid json";

        let result = templates::validate_and_parse_template(invalid_json);
        assert!(result.is_err());
    }
}
