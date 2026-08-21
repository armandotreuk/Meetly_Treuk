//! Tauri commands for logical meeting folders.
//!
//! Folders exist only in DB; disk layout (meetings.folder_path) is untouched.

use tauri::{AppHandle, Runtime, State};

use crate::{
    database::{models::MeetingFolderModel, repositories::folder::FolderRepository},
    state::AppState,
};

#[tauri::command]
pub async fn api_get_folders<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<Vec<MeetingFolderModel>, String> {
    let pool = state.db_manager.pool();
    FolderRepository::get_all(pool)
        .await
        .map_err(|e| format!("Failed to load folders: {}", e))
}

#[tauri::command]
pub async fn api_create_folder<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, AppState>,
    name: String,
    parent_id: Option<String>,
) -> Result<MeetingFolderModel, String> {
    let pool = state.db_manager.pool();
    FolderRepository::create(pool, &name, parent_id.as_deref())
        .await
        .map_err(|e| format!("Failed to create folder: {}", e))
}

#[tauri::command]
pub async fn api_rename_folder<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    FolderRepository::rename(pool, &id, &name)
        .await
        .map_err(|e| format!("Failed to rename folder: {}", e))
}

#[tauri::command]
pub async fn api_move_folder<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
    new_parent_id: Option<String>,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    FolderRepository::move_folder(pool, &id, new_parent_id.as_deref()).await
}

#[tauri::command]
pub async fn api_delete_folder<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    FolderRepository::delete_with_cascade(pool, &id)
        .await
        .map_err(|e| format!("Failed to delete folder: {}", e))
}

#[tauri::command]
pub async fn api_set_meeting_folder<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, AppState>,
    meeting_id: String,
    folder_id: Option<String>,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    FolderRepository::set_meeting_folder(pool, &meeting_id, folder_id.as_deref())
        .await
        .map_err(|e| format!("Failed to set meeting folder: {}", e))
}
