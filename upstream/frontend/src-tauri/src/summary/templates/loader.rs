use super::defaults;
use super::types::Template;
use crate::database::repositories::templates::TemplatesRepository;
use once_cell::sync::Lazy;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::RwLock;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};
use tracing::{debug, info, warn};

// Global storage for the bundled templates directory path
static BUNDLED_TEMPLATES_DIR: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

#[cfg(test)]
static TEMPLATE_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[cfg(test)]
pub(crate) fn acquire_template_test_lock() -> MutexGuard<'static, ()> {
    TEMPLATE_TEST_LOCK
        .lock()
        .expect("template test lock should not be poisoned")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateOrigin {
    pub is_builtin: bool,
    pub source: &'static str,
}

pub const DATABASE_TEMPLATE_ID_PREFIX: &str = "db:";
pub const FILE_TEMPLATE_ID_PREFIX: &str = "file:";

pub fn database_template_id(id: i64) -> String {
    format!("{}{}", DATABASE_TEMPLATE_ID_PREFIX, id)
}

pub fn parse_database_template_id(template_id: &str) -> Option<i64> {
    template_id
        .strip_prefix(DATABASE_TEMPLATE_ID_PREFIX)
        .and_then(|id| id.parse::<i64>().ok())
        .filter(|id| *id > 0)
}

pub fn file_template_id(id: &str) -> String {
    if id.parse::<i64>().is_ok() {
        format!("{}{}", FILE_TEMPLATE_ID_PREFIX, id)
    } else {
        id.to_string()
    }
}

fn file_lookup_id(template_id: &str) -> &str {
    template_id
        .strip_prefix(FILE_TEMPLATE_ID_PREFIX)
        .unwrap_or(template_id)
}

/// Resolve the origin used by the template listing so shipped files remain read-only.
pub fn template_origin(template_id: &str) -> TemplateOrigin {
    if template_id.starts_with(DATABASE_TEMPLATE_ID_PREFIX) {
        return TemplateOrigin {
            is_builtin: false,
            source: "database",
        };
    }

    let file_id = file_lookup_id(template_id);
    if load_custom_template(file_id).is_some() {
        TemplateOrigin {
            is_builtin: false,
            source: "custom",
        }
    } else if load_bundled_template(file_id).is_some() {
        TemplateOrigin {
            is_builtin: true,
            source: "bundled",
        }
    } else if defaults::get_builtin_template(file_id).is_some() {
        TemplateOrigin {
            is_builtin: true,
            source: "builtin",
        }
    } else {
        TemplateOrigin {
            is_builtin: false,
            source: "file",
        }
    }
}

/// Set the bundled templates directory path (called once at app startup)
pub fn set_bundled_templates_dir(path: PathBuf) {
    info!("Bundled templates directory set to: {:?}", path);
    if let Ok(mut dir) = BUNDLED_TEMPLATES_DIR.write() {
        *dir = Some(path);
    }
}

/// Get the user's custom templates directory path
///
/// Returns the platform-specific application data directory for custom templates:
/// - macOS: ~/Library/Application Support/Meetily/templates/
/// - Windows: %APPDATA%\Meetily\templates\
/// - Linux: ~/.config/Meetily/templates/
fn get_custom_templates_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir()?;
    path.push("Meetily");
    path.push("templates");
    Some(path)
}

/// Load a template from the bundled resources directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
fn load_bundled_template(template_id: &str) -> Option<String> {
    let bundled_dir = BUNDLED_TEMPLATES_DIR.read().ok()?.clone()?;
    let template_path = bundled_dir.join(format!("{}.json", template_id));

    debug!("Checking for bundled template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!(
                "Loaded bundled template '{}' from {:?}",
                template_id, template_path
            );
            Some(content)
        }
        Err(e) => {
            debug!("No bundled template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load a template from the user's custom templates directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
fn load_custom_template(template_id: &str) -> Option<String> {
    let custom_dir = get_custom_templates_dir()?;
    let template_path = custom_dir.join(format!("{}.json", template_id));

    debug!("Checking for custom template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!(
                "Loaded custom template '{}' from {:?}",
                template_id, template_path
            );
            Some(content)
        }
        Err(e) => {
            debug!("No custom template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load and parse a template by identifier
///
/// This function implements a fallback strategy:
/// 1. Check user's custom templates directory
/// 2. Check bundled resources directory (app templates)
/// 3. Fall back to built-in embedded templates
/// 4. Return error if not found in any location
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "daily_standup", "standard_meeting")
///
/// # Returns
/// Parsed and validated Template struct
pub fn get_template(template_id: &str) -> Result<Template, String> {
    info!("Loading template: {}", template_id);

    let numeric_id = template_id.parse::<i64>().ok();
    if template_id.starts_with(DATABASE_TEMPLATE_ID_PREFIX) || numeric_id.is_some() {
        return Err(format!(
            "Ambiguous template '{}'; use db:<id> or file:<id>",
            template_id
        ));
    }

    let file_id = file_lookup_id(template_id);

    // Try custom template first, then bundled, then built-in
    let json_content = if let Some(custom_content) = load_custom_template(file_id) {
        debug!("Using custom template for '{}'", template_id);
        custom_content
    } else if let Some(bundled_content) = load_bundled_template(file_id) {
        debug!("Using bundled template for '{}'", template_id);
        bundled_content
    } else if let Some(builtin_content) = defaults::get_builtin_template(file_id) {
        debug!("Using built-in template for '{}'", template_id);
        builtin_content.to_string()
    } else {
        return Err(format!(
            "Template '{}' not found. Available templates: {}",
            template_id,
            list_template_ids().join(", ")
        ));
    };

    // Parse and validate
    validate_and_parse_template(&json_content)
}

fn get_explicit_file_template(template_id: &str) -> Result<Template, String> {
    let file_id = file_lookup_id(template_id);
    let json_content = load_custom_template(file_id)
        .or_else(|| load_bundled_template(file_id))
        .ok_or_else(|| format!("File template '{}' not found", template_id))?;
    validate_and_parse_template(&json_content)
}

async fn load_database_template(pool: &SqlitePool, id: i64) -> Result<Option<Template>, String> {
    let record = TemplatesRepository::get_by_id(pool, id)
        .await
        .map_err(|e| format!("Failed to load database template: {}", e))?;

    record
        .map(|record| {
            validate_and_parse_template(&record.schema_json)
                .map_err(|e| format!("Stored template has invalid schema: {}", e))
        })
        .transpose()
}

/// Resolve a template without allowing a file to shadow a database row.
/// New database IDs use the `db:` prefix; unprefixed positive numeric IDs are
/// retained as a legacy persisted-summary format.
pub async fn resolve_template(pool: &SqlitePool, template_id: &str) -> Result<Template, String> {
    let explicit_database_id = parse_database_template_id(template_id);
    let legacy_database_id = if explicit_database_id.is_none() {
        template_id.parse::<i64>().ok().filter(|id| *id > 0)
    } else {
        None
    };

    if let Some(id) = explicit_database_id.or(legacy_database_id) {
        if let Some(template) = load_database_template(pool, id).await? {
            return Ok(template);
        }

        if explicit_database_id.is_some() {
            // An explicit database namespace is strict. A deleted DB row is
            // handled by callers that can render archived summary content;
            // it must never silently select a colliding file template.
            return Err(format!("Database template 'db:{}' not found", id));
        }

        // An unprefixed numeric ID is an old ambiguous reference. Resolve the
        // database row first, then the file namespace when no row exists.
        return get_template(&file_template_id(template_id));
    }

    get_template(template_id)
}

/// Resolve a template when the caller has an explicit source from the listing.
pub async fn resolve_template_with_source(
    pool: &SqlitePool,
    template_id: &str,
    source: Option<&str>,
) -> Result<Template, String> {
    // The `db:` namespace is unambiguous and cannot be redirected to a file
    // by a stale or contradictory source hint.
    if let Some(id) = parse_database_template_id(template_id) {
        return load_database_template(pool, id)
            .await?
            .ok_or_else(|| format!("Database template '{}' not found", template_id));
    }

    // Raw numeric IDs predate source-safe IDs and remain DB-first even when a
    // caller supplied a stale source hint.
    if template_id
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .is_some()
    {
        return resolve_template(pool, template_id).await;
    }

    match source {
        Some("database") => {
            let id = template_id
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| format!("Invalid database template ID '{}'", template_id))?;
            load_database_template(pool, id)
                .await?
                .ok_or_else(|| format!("Database template '{}' not found", template_id))
        }
        Some("builtin") => get_template(template_id),
        Some("bundled" | "custom") => get_template(&file_template_id(template_id)),
        Some("file") => get_explicit_file_template(template_id),
        Some(other) => Err(format!("Unknown template source '{}'", other)),
        None => resolve_template(pool, template_id).await,
    }
}

/// Validate and parse template JSON
///
/// # Arguments
/// * `json_content` - Raw JSON string
///
/// # Returns
/// Parsed and validated Template struct
pub fn validate_and_parse_template(json_content: &str) -> Result<Template, String> {
    let template: Template = serde_json::from_str(json_content)
        .map_err(|e| format!("Failed to parse template JSON: {}", e))?;

    template.validate()?;

    Ok(template)
}

/// List all available template identifiers
///
/// Returns a combined list of:
/// - Built-in template IDs
/// - Bundled template IDs (from app resources)
/// - Custom template IDs (from user's data directory)
pub fn list_template_ids() -> Vec<String> {
    let mut ids: Vec<String> = defaults::list_builtin_template_ids()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // Add bundled templates if directory is set
    if let Ok(bundled_dir_lock) = BUNDLED_TEMPLATES_DIR.read() {
        if let Some(bundled_dir) = bundled_dir_lock.as_ref() {
            if bundled_dir.exists() {
                match std::fs::read_dir(bundled_dir) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            if let Some(filename) = entry.file_name().to_str() {
                                if filename.ends_with(".json") {
                                    let id = file_template_id(filename.trim_end_matches(".json"));
                                    if !ids.contains(&id) {
                                        ids.push(id);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read bundled templates directory: {}", e);
                    }
                }
            }
        }
    }

    // Add custom templates if directory exists
    if let Some(custom_dir) = get_custom_templates_dir() {
        if custom_dir.exists() {
            match std::fs::read_dir(&custom_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if let Some(filename) = entry.file_name().to_str() {
                            if filename.ends_with(".json") {
                                let id = file_template_id(filename.trim_end_matches(".json"));
                                if !ids.contains(&id) {
                                    ids.push(id);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read custom templates directory: {}", e);
                }
            }
        }
    }

    ids.sort();
    ids
}

/// List all available templates with their metadata
///
/// Returns a list of (id, name, description) tuples
pub fn list_templates() -> Vec<(String, String, String)> {
    let mut templates = Vec::new();

    for id in list_template_ids() {
        match get_template(&id) {
            Ok(template) => {
                templates.push((id, template.name, template.description));
            }
            Err(e) => {
                warn!("Failed to load template '{}': {}", id, e);
            }
        }
    }

    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_template_origin_is_builtin() {
        let origin = template_origin("daily_standup");
        assert!(origin.is_builtin);
        assert_ne!(origin.source, "file");
    }

    #[test]
    fn database_ids_are_source_safe() {
        assert_eq!(database_template_id(42), "db:42");
        assert_eq!(parse_database_template_id("db:42"), Some(42));
        assert_eq!(parse_database_template_id("42"), None);
        assert_eq!(parse_database_template_id("db:0"), None);
        assert_eq!(parse_database_template_id("db:-1"), None);
        assert_eq!(file_template_id("42"), "file:42");
        assert_eq!(file_template_id("daily_standup"), "daily_standup");
        assert!(get_template("42").is_err());
    }

    #[tokio::test]
    async fn resolve_template_loads_database_template_ids() {
        let pool = SqlitePool::connect(":memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE templates (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                stable_id TEXT,
                schema_json TEXT NOT NULL,
                is_builtin INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create templates schema");

        let schema = r#"{
            "name": "Database Template",
            "description": "Loaded from the database",
            "sections": [{
                "title": "Summary",
                "instruction": "Summarize the meeting",
                "format": "paragraph"
            }]
        }"#;
        sqlx::query(
            "INSERT INTO templates (id, name, description, schema_json, is_builtin, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(42_i64)
        .bind("Database Template")
        .bind("Loaded from the database")
        .bind(schema)
        .bind(0_i64)
        .bind("now")
        .bind("now")
        .execute(&pool)
        .await
        .expect("insert database template");

        let template = resolve_template(&pool, "db:42")
            .await
            .expect("resolve database template");
        assert_eq!(template.name, "Database Template");
        assert_eq!(template.description, "Loaded from the database");
        assert_eq!(template.sections.len(), 1);

        // Existing summary rows keep their unprefixed numeric ID.
        let legacy_template = resolve_template(&pool, "42")
            .await
            .expect("resolve legacy database template");
        assert_eq!(legacy_template.name, "Database Template");
    }

    #[tokio::test]
    async fn legacy_numeric_database_id_is_not_shadowed_by_a_file_template() {
        let _lock = acquire_template_test_lock();
        let pool = SqlitePool::connect(":memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE templates (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                stable_id TEXT,
                schema_json TEXT NOT NULL,
                is_builtin INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create templates schema");

        let database_schema = r#"{
            "name": "Database Template",
            "description": "Database source",
            "sections": [{
                "title": "Summary",
                "instruction": "Summarize the meeting",
                "format": "paragraph"
            }]
        }"#;
        let file_schema = database_schema.replace("Database Template", "File Template");
        sqlx::query(
            "INSERT INTO templates (id, name, description, schema_json, is_builtin, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(42_i64)
        .bind("Database Template")
        .bind("Database source")
        .bind(database_schema)
        .bind(0_i64)
        .bind("now")
        .bind("now")
        .execute(&pool)
        .await
        .expect("insert database template");

        let dir = tempfile::tempdir().expect("create bundled template directory");
        std::fs::write(dir.path().join("42.json"), file_schema)
            .expect("write colliding file template");
        set_bundled_templates_dir(dir.path().to_path_buf());
        assert!(list_template_ids().contains(&"file:42".to_string()));

        let template = resolve_template(&pool, "42")
            .await
            .expect("resolve legacy database template");
        assert_eq!(template.name, "Database Template");
        assert_eq!(template_origin("42").source, "bundled");

        let explicit = resolve_template(&pool, "db:42")
            .await
            .expect("resolve explicit database template");
        assert_eq!(explicit.name, "Database Template");

        let hinted = resolve_template_with_source(&pool, "db:42", Some("custom"))
            .await
            .expect("explicit database namespace must win over a file hint");
        assert_eq!(hinted.name, "Database Template");

        let file = resolve_template_with_source(&pool, "file:42", Some("file"))
            .await
            .expect("explicit file namespace must resolve the file");
        assert_eq!(file.name, "File Template");
    }

    #[tokio::test]
    async fn legacy_numeric_id_falls_back_to_file_when_database_row_is_missing() {
        let _lock = acquire_template_test_lock();
        let pool = SqlitePool::connect(":memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE templates (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                stable_id TEXT,
                schema_json TEXT NOT NULL,
                is_builtin INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create templates schema");

        let dir = tempfile::tempdir().expect("create bundled template directory");
        std::fs::write(
            dir.path().join("987654.json"),
            r#"{
                "name": "Archived File Template",
                "description": "File fallback",
                "sections": [{
                    "title": "Summary",
                    "instruction": "Keep it",
                    "format": "paragraph"
                }]
            }"#,
        )
        .expect("write numeric file template");
        set_bundled_templates_dir(dir.path().to_path_buf());

        let template = resolve_template(&pool, "987654")
            .await
            .expect("legacy numeric ID should fall back to file");
        assert_eq!(template.name, "Archived File Template");
        let explicit = resolve_template(&pool, "db:987654").await;
        assert!(
            explicit.is_err(),
            "explicit db IDs must not fall through to files"
        );

        let file = resolve_template_with_source(&pool, "file:987654", Some("bundled"))
            .await
            .expect("explicit file ID should resolve the file");
        assert_eq!(file.name, "Archived File Template");
    }

    #[test]
    fn test_get_builtin_template() {
        let template = get_template("daily_standup");
        assert!(template.is_ok());

        let template = template.unwrap();
        assert_eq!(template.name, "Daily Standup");
        assert!(!template.sections.is_empty());
    }

    #[test]
    fn test_get_nonexistent_template() {
        let result = get_template("nonexistent_template");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_template_ids() {
        let ids = list_template_ids();
        assert!(ids.contains(&"daily_standup".to_string()));
        assert!(ids.contains(&"standard_meeting".to_string()));
    }

    #[test]
    fn test_validate_invalid_json() {
        let result = validate_and_parse_template("invalid json");
        assert!(result.is_err());
    }
}
