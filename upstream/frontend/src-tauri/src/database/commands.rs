use log::{error, info};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

use super::manager::DatabaseManager;
use crate::state::AppState;
use tauri::State;

#[derive(Serialize)]
pub struct DatabaseCheckResult {
    pub exists: bool,
    pub size: u64,
}

/// Check if this is the first launch (no database exists yet)
#[tauri::command]
pub async fn check_first_launch(app: AppHandle) -> Result<bool, String> {
    DatabaseManager::is_first_launch(&app)
        .await
        .map_err(|e| format!("Failed to check first launch: {}", e))
}

/// Open a dialog to select a folder or file for legacy database import
#[tauri::command]
pub async fn select_legacy_database_path(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    info!("Opening dialog to select legacy database location");

    let file_path = app
        .dialog()
        .file()
        .add_filter("Database Files", &["db"])
        .blocking_pick_file();

    if let Some(path) = file_path {
        let path_str = path.to_string();
        info!("User selected path: {}", path_str);
        Ok(Some(path_str))
    } else {
        info!("User cancelled file selection");
        Ok(None)
    }
}

/// Detect legacy database from a selected path (root repo, backend folder, or db file)
#[tauri::command]
pub async fn detect_legacy_database(selected_path: String) -> Result<Option<String>, String> {
    let path = PathBuf::from(&selected_path);

    info!("Detecting legacy database from path: {}", selected_path);

    // Case 1: User selected the .db file directly
    if path.is_file() {
        if let Some(extension) = path.extension() {
            if extension == "db" {
                info!("Direct .db file selected: {}", selected_path);
                return Ok(Some(selected_path));
            }
        }
    }

    // Case 2: User selected directory containing meeting_minutes.db
    if path.is_dir() {
        let direct_db = path.join("meeting_minutes.db");
        if direct_db.exists() && direct_db.is_file() {
            let db_path = direct_db.to_string_lossy().to_string();
            info!("Found database in selected directory: {}", db_path);
            return Ok(Some(db_path));
        }

        // Case 3: User selected root repo (check backend subdirectory)
        let backend_db = path.join("backend").join("meeting_minutes.db");
        if backend_db.exists() && backend_db.is_file() {
            let db_path = backend_db.to_string_lossy().to_string();
            info!("Found database in backend subdirectory: {}", db_path);
            return Ok(Some(db_path));
        }
    }

    info!("No legacy database found at path: {}", selected_path);
    Ok(None)
}

/// Check for legacy database in the default app data directory
#[tauri::command]
pub async fn check_default_legacy_database(app: AppHandle) -> Result<Option<String>, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    let legacy_db = app_data_dir.join("meeting_minutes.db");
    info!("Checking for default legacy database at: {:?}", legacy_db);

    if legacy_db.exists() && legacy_db.is_file() {
        let path_str = legacy_db.to_string_lossy().to_string();
        info!("Found default legacy database: {}", path_str);
        Ok(Some(path_str))
    } else {
        info!("No default legacy database found");
        Ok(None)
    }
}

/// Check if the Homebrew database exists and return its size
/// This is specifically for detecting old Python backend installations
#[tauri::command]
pub async fn check_homebrew_database(path: String) -> Result<Option<DatabaseCheckResult>, String> {
    let db_path = PathBuf::from(&path);

    info!("Checking for Homebrew database at: {}", path);

    // Check if file exists and is a regular file
    if db_path.exists() && db_path.is_file() {
        // Get file metadata to check size
        match std::fs::metadata(&db_path) {
            Ok(metadata) => {
                let size = metadata.len();
                info!("Found Homebrew database: {} ({} bytes)", path, size);

                // Only consider it valid if it has content (not empty)
                if size > 0 {
                    Ok(Some(DatabaseCheckResult { exists: true, size }))
                } else {
                    info!("Database file exists but is empty");
                    Ok(None)
                }
            }
            Err(e) => {
                error!("Failed to read database metadata: {}", e);
                Ok(None)
            }
        }
    } else {
        info!("No database found at Homebrew location");
        Ok(None)
    }
}

/// Import legacy database and initialize the database manager
#[tauri::command]
pub async fn import_and_initialize_database(
    app: AppHandle,
    legacy_db_path: String,
) -> Result<(), String> {
    info!(
        "Starting import of legacy database from: {}",
        legacy_db_path
    );

    // Import and get initialized manager
    let db_manager = DatabaseManager::import_legacy_database(&app, &legacy_db_path)
        .await
        .map_err(|e| {
            error!("Failed to import legacy database: {}", e);
            format!("Failed to import database: {}", e)
        })?;

    // Update app state with the new manager
    app.manage(AppState { db_manager });

    info!("Legacy database imported and initialized successfully");

    // Start MCP server now that AppState is available (first-launch path)
    crate::mcp::server::spawn_from_app(&app);

    // Emit event to notify frontend that database is ready
    app.emit("database-initialized", ())
        .map_err(|e| format!("Failed to emit database-initialized event: {}", e))?;

    Ok(())
}

/// Initialize a fresh database (for users who don't want to import)
#[tauri::command]
pub async fn initialize_fresh_database(app: AppHandle) -> Result<(), String> {
    info!("Initializing fresh database");

    let db_manager = DatabaseManager::new_from_app_handle(&app)
        .await
        .map_err(|e| {
            error!("Failed to initialize fresh database: {}", e);
            format!("Failed to initialize database: {}", e)
        })?;

    // Update app state with the new manager
    app.manage(AppState {
        db_manager: db_manager.clone(),
    });

    // Start MCP server now that AppState is available (first-launch path)
    crate::mcp::server::spawn_from_app(&app);

    // Set default model configuration for fresh installs
    let pool = db_manager.pool();

    let default_summary_model =
        crate::summary::summary_engine::commands::get_recommended_summary_model_for_current_system(
        )
        .unwrap_or("qwen3.5:2b");

    // Default Summary Model: Built-in AI (Qwen recommendation for this system)
    if let Err(e) = crate::database::repositories::setting::SettingsRepository::save_model_config(
        pool,
        "builtin-ai",
        default_summary_model,
        "large-v3", // Default whisper model (unused for builtin but required)
        None,
    )
    .await
    {
        error!("Failed to set default summary model config: {}", e);
    }

    // Default Transcription Model: Parakeet
    if let Err(e) =
        crate::database::repositories::setting::SettingsRepository::save_transcript_config(
            pool,
            "parakeet",
            crate::config::DEFAULT_PARAKEET_MODEL,
        )
        .await
    {
        error!("Failed to set default transcription model config: {}", e);
    }

    info!("Fresh database initialized successfully with default models");

    // Emit event to notify frontend that database is ready
    app.emit("database-initialized", ())
        .map_err(|e| format!("Failed to emit database-initialized event: {}", e))?;

    Ok(())
}

/// Get the database directory path
#[tauri::command]
pub async fn get_database_directory(app: AppHandle) -> Result<String, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    Ok(app_data_dir.to_string_lossy().to_string())
}

/// Open the database folder in the system file explorer
#[tauri::command]
pub async fn open_database_folder(app: AppHandle) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    // Ensure directory exists before trying to open it
    if !app_data_dir.exists() {
        std::fs::create_dir_all(&app_data_dir)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    let folder_path = app_data_dir.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    info!("Opened database folder: {}", folder_path);
    Ok(())
}

// F11: Meeting notes commands

#[tauri::command]
pub async fn get_meeting_notes(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<super::repositories::meeting_notes::MeetingNote>, String> {
    let pool = state.db_manager.pool();
    super::repositories::meeting_notes::MeetingNotesRepository::get_notes(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to get meeting notes: {}", e))
}

#[tauri::command]
pub async fn save_meeting_notes(
    state: State<'_, AppState>,
    meeting_id: String,
    notes_markdown: Option<String>,
    notes_json: Option<String>,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    super::repositories::meeting_notes::MeetingNotesRepository::save_notes(
        pool,
        &meeting_id,
        notes_markdown.as_deref(),
        notes_json.as_deref(),
    )
    .await
    .map_err(|e| format!("Failed to save meeting notes: {}", e))?;

    // Export notes.md to the meeting folder (fire-and-forget on IO error — DB is the source of truth).
    let folder_path: Option<Option<String>> =
        sqlx::query_scalar::<_, Option<String>>("SELECT folder_path FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_optional(pool)
            .await
            .unwrap_or(None);
    if let Some(Some(path)) = folder_path {
        let notes_md = std::path::Path::new(&path).join("notes.md");
        let content = notes_markdown.clone().unwrap_or_default();
        if let Err(e) = std::fs::write(&notes_md, content.as_bytes()) {
            log::warn!(
                "Failed to export notes.md to {} for meeting {}: {}",
                notes_md.display(),
                meeting_id,
                e
            );
        }
    }

    // Update FTS index — best-effort; a failure here doesn't invalidate
    // the notes data we just committed.
    if let Err(e) =
        super::repositories::fts::FtsRepository::refresh_meeting(pool, &meeting_id).await
    {
        error!("Failed to refresh FTS for meeting {}: {}", meeting_id, e);
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_meeting_notes(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<(), String> {
    let pool = state.db_manager.pool();

    // Look up folder before deleting the DB row.
    let folder_path: Option<Option<String>> =
        sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    super::repositories::meeting_notes::MeetingNotesRepository::delete_notes(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to delete meeting notes: {}", e))?;

    // Best-effort removal of the exported file.
    if let Some(Some(path)) = folder_path {
        let notes_md = std::path::Path::new(&path).join("notes.md");
        let _ = std::fs::remove_file(&notes_md);
    }

    // Update FTS index — best-effort; a failure here doesn't invalidate
    // the notes deletion we just committed.
    if let Err(e) =
        super::repositories::fts::FtsRepository::refresh_meeting(pool, &meeting_id).await
    {
        error!("Failed to refresh FTS for meeting {}: {}", meeting_id, e);
    }

    Ok(())
}

// F1: Template commands

#[tauri::command]
pub async fn list_templates(
    state: State<'_, AppState>,
) -> Result<Vec<super::repositories::templates::Template>, String> {
    let pool = state.db_manager.pool();
    super::repositories::templates::TemplatesRepository::list_all(pool)
        .await
        .map_err(|e| format!("Failed to list templates: {}", e))
}

#[tauri::command]
pub async fn list_user_templates(
    state: State<'_, AppState>,
) -> Result<Vec<super::repositories::templates::Template>, String> {
    let pool = state.db_manager.pool();
    super::repositories::templates::TemplatesRepository::list_user_templates(pool)
        .await
        .map_err(|e| format!("Failed to list user templates: {}", e))
}

#[tauri::command]
pub async fn list_builtin_templates(
    state: State<'_, AppState>,
) -> Result<Vec<super::repositories::templates::Template>, String> {
    let pool = state.db_manager.pool();
    super::repositories::templates::TemplatesRepository::list_builtin_templates(pool)
        .await
        .map_err(|e| format!("Failed to list builtin templates: {}", e))
}

async fn ensure_database_template_mutation(
    pool: &sqlx::SqlitePool,
    id: &str,
    source: Option<&str>,
) -> Result<i64, String> {
    let database_id = crate::summary::templates::parse_database_template_id(id)
        .or_else(|| {
            (source == Some("database"))
                .then(|| id.parse::<i64>().ok().filter(|id| *id > 0))
                .flatten()
        })
        .ok_or_else(|| {
            if source == Some("database") {
                "Invalid database template ID".to_string()
            } else {
                "Database template mutations require an explicit database template ID".to_string()
            }
        })?;

    if matches!(source, Some(value) if value != "database") {
        return Err("File-based templates are read-only".to_string());
    }

    let template =
        super::repositories::templates::TemplatesRepository::get_by_id(pool, database_id)
            .await
            .map_err(|e| format!("Failed to get template: {}", e))?
            .ok_or_else(|| "Database template not found".to_string())?;
    if template.is_builtin != 0 {
        return Err("Cannot modify built-in template".to_string());
    }
    Ok(database_id)
}

fn validate_create_template_source(source: Option<&str>) -> Result<(), String> {
    if matches!(source, Some(value) if value != "database") {
        return Err("File-based templates are read-only".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_template(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Option<super::repositories::templates::Template>, String> {
    let pool = state.db_manager.pool();
    super::repositories::templates::TemplatesRepository::get_by_id(pool, id)
        .await
        .map_err(|e| format!("Failed to get template: {}", e))
}

#[tauri::command]
pub async fn create_template(
    state: State<'_, AppState>,
    name: String,
    description: String,
    schema_json: String,
    template_source: Option<String>,
) -> Result<super::repositories::templates::Template, String> {
    validate_create_template_source(template_source.as_deref())?;
    let pool = state.db_manager.pool();
    super::repositories::templates::TemplatesRepository::create(
        pool,
        super::repositories::templates::CreateTemplateRequest {
            name,
            description,
            schema_json,
        },
    )
    .await
    .map_err(|e| format!("Failed to create template: {}", e))
}

#[tauri::command]
pub async fn update_template(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    description: Option<String>,
    schema_json: Option<String>,
    template_source: Option<String>,
) -> Result<super::repositories::templates::Template, String> {
    let pool = state.db_manager.pool();
    let id = ensure_database_template_mutation(pool, &id, template_source.as_deref()).await?;
    super::repositories::templates::TemplatesRepository::update(
        pool,
        id,
        super::repositories::templates::UpdateTemplateRequest {
            name,
            description,
            schema_json,
        },
    )
    .await
    .map_err(|e| format!("Failed to update template: {}", e))
}

#[tauri::command]
pub async fn delete_template(
    state: State<'_, AppState>,
    id: String,
    template_source: Option<String>,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let id = ensure_database_template_mutation(pool, &id, template_source.as_deref()).await?;
    super::repositories::templates::TemplatesRepository::delete(pool, id)
        .await
        .map_err(|e| format!("Failed to delete template: {}", e))
}

#[cfg(test)]
mod template_mutation_tests {
    use super::{ensure_database_template_mutation, validate_create_template_source};
    use sqlx::SqlitePool;

    #[test]
    fn non_database_sources_are_rejected() {
        assert!(validate_create_template_source(Some("custom")).is_err());
        assert!(validate_create_template_source(Some("bundled")).is_err());
        assert!(validate_create_template_source(Some("builtin")).is_err());
        assert!(validate_create_template_source(Some("database")).is_ok());
        assert!(validate_create_template_source(None).is_ok());
    }

    #[tokio::test]
    async fn database_source_hint_cannot_mutate_a_builtin_row() {
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
        .expect("create template schema");
        sqlx::query(
            "INSERT INTO templates (id, name, description, schema_json, is_builtin, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(42_i64)
        .bind("Builtin")
        .bind("Builtin")
        .bind("{}")
        .bind(1_i64)
        .bind("now")
        .bind("now")
        .execute(&pool)
        .await
        .expect("insert builtin template");

        assert!(
            ensure_database_template_mutation(&pool, "db:42", Some("database"))
                .await
                .is_err()
        );
        assert!(ensure_database_template_mutation(&pool, "db:42", None)
            .await
            .is_err());
        assert!(ensure_database_template_mutation(&pool, "42", None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn explicit_database_mutation_ignores_a_colliding_file_template() {
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
        .expect("create template schema");
        sqlx::query(
            "INSERT INTO templates (id, name, description, schema_json, is_builtin, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(77_i64)
        .bind("Database template")
        .bind("Database template")
        .bind("{}")
        .bind(0_i64)
        .bind("now")
        .bind("now")
        .execute(&pool)
        .await
        .expect("insert database template");

        let dir = tempfile::tempdir().expect("create bundled template directory");
        std::fs::write(
            dir.path().join("77.json"),
            r#"{
                "name": "File template",
                "description": "File template",
                "sections": [{
                    "title": "Summary",
                    "instruction": "Summarize",
                    "format": "paragraph"
                }]
            }"#,
        )
        .expect("write colliding file template");
        let _lock = crate::summary::templates::acquire_template_test_lock();
        crate::summary::templates::set_bundled_templates_dir(dir.path().to_path_buf());

        let id = ensure_database_template_mutation(&pool, "db:77", Some("database"))
            .await
            .expect("DB namespace must target the database row directly");
        assert_eq!(id, 77);
        assert!(
            ensure_database_template_mutation(&pool, "file:77", Some("file"))
                .await
                .is_err()
        );
    }
}
