use crate::database::repositories::templates::{Template as DbTemplate, TemplatesRepository};
use crate::state::AppState;
use crate::summary::templates;
use serde::{Deserialize, Serialize};
use tauri::{Manager, Runtime};
use tracing::{info, warn};

/// Template metadata for UI display
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

    /// Only user-created database templates have an editor-backed mutation path.
    pub is_editable: bool,
}

/// Detailed template structure for preview/debugging
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

    /// Source of the template: "builtin", "bundled", "custom", or "database"
    pub source: String,

    /// Whether this template can be updated or deleted through the database API.
    pub is_editable: bool,

    /// Full validated schema, used for safe duplication and database editing.
    pub schema_json: String,
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

    let file_infos: Vec<TemplateInfo> =
        file_templates.into_iter().map(file_template_info).collect();

    // Add database templates (user-created templates from DB)
    let db_templates = if let Some(app_state) = app.try_state::<AppState>() {
        let pool = app_state.db_manager.pool();
        TemplatesRepository::list_all(pool)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let template_infos = merge_template_infos(file_infos, db_templates);

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
    template_source: Option<String>,
) -> Result<TemplateDetails, String> {
    info!(
        "api_get_template_details called for template_id: {}",
        template_id
    );

    let source_hint = template_source.as_deref();
    let explicit_database_id = templates::parse_database_template_id(&template_id);
    let legacy_numeric_id = template_id.parse::<i64>().ok().filter(|id| *id > 0);
    let file_only = explicit_database_id.is_none()
        && legacy_numeric_id.is_none()
        && matches!(source_hint, Some("builtin" | "bundled" | "custom" | "file"));
    let database_only = explicit_database_id.is_some()
        || (source_hint == Some("database") && legacy_numeric_id.is_none());
    let mut resolved: Option<(templates::Template, bool, String, bool, String, String)> = None;
    let database_id = explicit_database_id.or(legacy_numeric_id);

    // Explicit database IDs and legacy numeric IDs are checked against the
    // database before file lookup, so a colliding file cannot shadow a stored
    // summary reference. An explicit file source remains file-only.
    if !file_only {
        if let (Some(app_state), Some(id)) = (app.try_state::<AppState>(), database_id) {
            let pool = app_state.db_manager.pool();
            if let Some(db_template) = TemplatesRepository::get_by_id(pool, id)
                .await
                .map_err(|e| format!("Failed to load template: {}", e))?
            {
                let parsed = templates::validate_and_parse_template(&db_template.schema_json)
                    .map_err(|e| format!("Stored template has invalid schema: {}", e))?;
                resolved = Some((
                    parsed,
                    db_template.is_builtin != 0,
                    "database".to_string(),
                    db_template.is_builtin == 0,
                    db_template.schema_json,
                    templates::database_template_id(db_template.id),
                ));
            }
        }
    }

    if resolved.is_none() && !database_only {
        let file_id = if file_only || legacy_numeric_id.is_some() {
            templates::file_template_id(&template_id)
        } else {
            template_id.clone()
        };
        if let Ok(parsed) = templates::get_template(&file_id) {
            let origin = templates::template_origin(&file_id);
            let schema_json = serde_json::to_string(&parsed)
                .map_err(|e| format!("Failed to serialize template schema: {}", e))?;
            resolved = Some((
                parsed,
                origin.is_builtin,
                origin.source.to_string(),
                false,
                schema_json,
                templates::file_template_id(&file_id),
            ));
        } else {
            return Err(format!("Template '{}' could not be loaded", template_id));
        }
    }

    let (template, is_builtin, source, is_editable, schema_json, resolved_id) =
        resolved.ok_or_else(|| format!("Template '{}' not found", template_id))?;

    let section_titles: Vec<String> = template
        .sections
        .iter()
        .map(|section| section.title.clone())
        .collect();

    let details = TemplateDetails {
        id: resolved_id,
        name: template.name,
        description: template.description,
        sections: section_titles,
        is_builtin,
        source,
        is_editable,
        schema_json,
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

fn file_template_info((id, name, description): (String, String, String)) -> TemplateInfo {
    let id = templates::file_template_id(&id);
    let origin = templates::template_origin(&id);
    TemplateInfo {
        id,
        name,
        description,
        is_builtin: origin.is_builtin,
        source: origin.source.to_string(),
        is_editable: false,
    }
}

fn database_template_info(template: DbTemplate) -> TemplateInfo {
    let is_builtin = template.is_builtin != 0;
    TemplateInfo {
        id: templates::database_template_id(template.id),
        name: template.name,
        description: template.description,
        is_builtin,
        source: "database".to_string(),
        is_editable: !is_builtin,
    }
}

fn database_builtin_matches_file(template: &DbTemplate, file: &TemplateInfo) -> bool {
    // The database copy is identified by the stable built-in registry ID, not
    // by its display name or schema. A synced row may legitimately be stale
    // while a custom file override is active.
    let stable_file_id = file.id.strip_prefix("file:").unwrap_or(&file.id);
    template.stable_id.as_deref() == Some(stable_file_id)
        && templates::defaults::list_builtin_template_ids()
            .into_iter()
            .any(|id| id == stable_file_id)
}

fn merge_template_infos(
    mut file_infos: Vec<TemplateInfo>,
    db_templates: Vec<DbTemplate>,
) -> Vec<TemplateInfo> {
    for db_template in db_templates {
        // User templates are identified by their database ID, not by display
        // name. Only equivalent built-in copies are suppressed.
        if db_template.is_builtin != 0
            && file_infos
                .iter()
                .any(|file| database_builtin_matches_file(&db_template, file))
        {
            continue;
        }
        file_infos.push(database_template_info(db_template));
    }

    file_infos.sort_by(|a, b| match (a.is_builtin, b.is_builtin) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)),
    });
    file_infos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_builtins_are_marked_read_only() {
        let info = file_template_info((
            "daily_standup".to_string(),
            "Daily Standup".to_string(),
            "Daily updates".to_string(),
        ));
        assert!(info.is_builtin);
        assert_ne!(info.source, "file");
        assert!(!info.is_editable);
    }

    #[test]
    fn numeric_file_templates_are_not_editable() {
        let info = file_template_info((
            "42".to_string(),
            "File Template".to_string(),
            "Read-only file template".to_string(),
        ));
        assert!(!info.is_builtin);
        assert!(!info.is_editable);
    }

    fn db_template(id: i64, name: &str, schema_json: &str, is_builtin: i64) -> DbTemplate {
        db_template_with_stable(id, name, schema_json, is_builtin, None)
    }

    fn db_template_with_stable(
        id: i64,
        name: &str,
        schema_json: &str,
        is_builtin: i64,
        stable_id: Option<&str>,
    ) -> DbTemplate {
        DbTemplate {
            id,
            name: name.to_string(),
            description: "Description".to_string(),
            stable_id: stable_id.map(str::to_string),
            schema_json: schema_json.to_string(),
            is_builtin,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        }
    }

    #[test]
    fn listing_keeps_same_name_user_templates_and_deduplicates_builtins() {
        let file = file_template_info((
            "standard_meeting".to_string(),
            "Standard Meeting Notes".to_string(),
            "File built-in".to_string(),
        ));
        let infos = merge_template_infos(
            vec![file.clone()],
            vec![
                db_template(10, &file.name, "custom schema 1", 0),
                db_template(11, &file.name, "custom schema 2", 0),
                db_template_with_stable(
                    12,
                    "Standard Meeting",
                    templates::defaults::STANDARD_MEETING,
                    1,
                    Some("standard_meeting"),
                ),
            ],
        );

        assert_eq!(
            infos
                .iter()
                .filter(|info| !info.is_builtin && info.source == "database")
                .count(),
            2
        );
        assert!(infos.iter().any(|info| info.id == "db:10"));
        assert_eq!(infos.iter().filter(|info| info.is_builtin).count(), 1);
        assert!(!infos.iter().any(|info| info.id == "12"));
    }

    #[test]
    fn custom_file_override_deduplicates_synced_builtin_by_stable_id() {
        let file = TemplateInfo {
            id: "standard_meeting".to_string(),
            name: "Custom Standard Meeting".to_string(),
            description: "Override".to_string(),
            is_builtin: false,
            source: "custom".to_string(),
            is_editable: false,
        };
        let infos = merge_template_infos(
            vec![file],
            vec![db_template_with_stable(
                12,
                "Renamed built-in",
                templates::defaults::STANDARD_MEETING,
                1,
                Some("standard_meeting"),
            )],
        );

        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].source, "custom");
        assert_eq!(infos[0].id, "standard_meeting");
    }

    #[test]
    fn stale_synced_builtin_is_deduplicated_even_when_schema_differs() {
        let file = TemplateInfo {
            id: "standard_meeting".to_string(),
            name: "Custom Standard Meeting".to_string(),
            description: "Override".to_string(),
            is_builtin: false,
            source: "custom".to_string(),
            is_editable: false,
        };
        let stale_schema =
            templates::defaults::STANDARD_MEETING.replace("Standard Meeting", "Stale synced copy");
        let infos = merge_template_infos(
            vec![file],
            vec![db_template_with_stable(
                42,
                "Stale display name",
                &stale_schema,
                1,
                Some("standard_meeting"),
            )],
        );

        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "standard_meeting");
        assert_eq!(infos[0].source, "custom");
    }

    #[test]
    fn listing_keeps_legacy_numeric_file_and_database_ids_distinct() {
        let file = file_template_info((
            "42".to_string(),
            "File Template".to_string(),
            "Read-only file template".to_string(),
        ));
        let infos = merge_template_infos(
            vec![file],
            vec![db_template(42, "Database Template", "database schema", 0)],
        );

        assert!(infos
            .iter()
            .any(|info| info.id == "file:42" && info.source != "database"));
        assert!(infos
            .iter()
            .any(|info| info.id == "db:42" && info.source == "database"));
    }

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
