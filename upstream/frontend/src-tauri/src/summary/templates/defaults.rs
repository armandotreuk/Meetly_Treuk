/// Embedded default templates using compile-time inclusion
///
/// These templates are bundled into the binary and serve as fallbacks
/// when custom templates are not available.

/// Daily standup template for engineering/product teams
pub const DAILY_STANDUP: &str = include_str!("../../../templates/daily_standup.json");

/// Standard meeting notes template
pub const STANDARD_MEETING: &str = include_str!("../../../templates/standard_meeting.json");

/// Project sync template
pub const PROJECT_SYNC: &str = include_str!("../../../templates/project_sync.json");

/// Retrospective template
pub const RETROSPECTIVE: &str = include_str!("../../../templates/retrospective.json");

/// Psychiatric session template
pub const PSYCHIATRIC_SESSION: &str = include_str!("../../../templates/psychatric_session.json");

/// Sales/marketing client call template
pub const SALES_MARKETING_CLIENT_CALL: &str =
    include_str!("../../../templates/sales_marketing_client_call.json");

use serde_json::Value;

/// Registry of all built-in templates
///
/// Maps template identifiers to their embedded JSON content
pub fn get_builtin_templates() -> Vec<(&'static str, &'static str)> {
    vec![
        ("daily_standup", DAILY_STANDUP),
        ("standard_meeting", STANDARD_MEETING),
        ("project_sync", PROJECT_SYNC),
        ("retrospective", RETROSPECTIVE),
        ("psychatric_session", PSYCHIATRIC_SESSION),
        ("sales_marketing_client_call", SALES_MARKETING_CLIENT_CALL),
    ]
}

/// Get a built-in template by identifier
///
/// # Arguments
/// * `id` - Template identifier (e.g., "daily_standup", "standard_meeting")
///
/// # Returns
/// The template JSON content if found, None otherwise
pub fn get_builtin_template(id: &str) -> Option<&'static str> {
    match id {
        "daily_standup" => Some(DAILY_STANDUP),
        "standard_meeting" => Some(STANDARD_MEETING),
        "project_sync" => Some(PROJECT_SYNC),
        "retrospective" => Some(RETROSPECTIVE),
        "psychatric_session" => Some(PSYCHIATRIC_SESSION),
        "sales_marketing_client_call" => Some(SALES_MARKETING_CLIENT_CALL),
        _ => None,
    }
}

/// List all built-in template identifiers
pub fn list_builtin_template_ids() -> Vec<&'static str> {
    vec![
        "daily_standup",
        "standard_meeting",
        "project_sync",
        "retrospective",
        "psychatric_session",
        "sales_marketing_client_call",
    ]
}

/// Get the display name for a built-in template
pub fn get_builtin_template_name(id: &str) -> Option<&'static str> {
    match id {
        "daily_standup" => Some("Daily Standup"),
        "standard_meeting" => Some("Standard Meeting"),
        "project_sync" => Some("Project Sync / Status Update"),
        "retrospective" => Some("Retrospective (Agile)"),
        "psychatric_session" => Some("Psychiatric Session Note (SOAP + AI Hybrid)"),
        "sales_marketing_client_call" => Some("Client / Sales Meeting"),
        _ => None,
    }
}

/// Get the description for a built-in template
pub fn get_builtin_template_description(id: &str) -> Option<&'static str> {
    match id {
        "daily_standup" => Some("Time-boxed daily updates for engineering/product teams"),
        "standard_meeting" => Some("A standard template for general meetings, focusing on key outcomes and actions"),
        "project_sync" => Some("Weekly or bi-weekly project status meeting focusing on milestones and risks"),
        "retrospective" => Some("Sprint retrospective template for continuous improvement"),
        "psychatric_session" => Some("AI-assisted psychiatric progress note template based on SOAP, with clinical metadata and AI summary"),
        "sales_marketing_client_call" => Some("Capture client goals, deliverables, and next steps"),
        _ => None,
    }
}

/// Parse a template JSON and return the name and description from the JSON itself
/// This is useful when the template is loaded from a file rather than the built-in list
pub fn parse_template_metadata(json_content: &str) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(json_content).ok()?;
    let name = value.get("name")?.as_str()?.to_string();
    let description = value.get("description")?.as_str()?.to_string();
    Some((name, description))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_templates_valid_json() {
        for (id, content) in get_builtin_templates() {
            let result = serde_json::from_str::<serde_json::Value>(content);
            assert!(
                result.is_ok(),
                "Built-in template '{}' contains invalid JSON: {:?}",
                id,
                result.err()
            );
        }
    }

    #[test]
    fn test_get_builtin_template() {
        assert!(get_builtin_template("daily_standup").is_some());
        assert!(get_builtin_template("standard_meeting").is_some());
        assert!(get_builtin_template("project_sync").is_some());
        assert!(get_builtin_template("retrospective").is_some());
        assert!(get_builtin_template("psychatric_session").is_some());
        assert!(get_builtin_template("sales_marketing_client_call").is_some());
        assert!(get_builtin_template("nonexistent").is_none());
    }
}
