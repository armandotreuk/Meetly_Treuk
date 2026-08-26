use log::info;
use tauri::{AppHandle, Emitter, Manager};

use super::manager::DatabaseManager;
use crate::retrieval::worker::RetrievalLifecycle;
use crate::state::AppState;

/// Initialize database on app startup
/// Handles first launch detection and conditional initialization
pub async fn initialize_database_on_startup(app: &AppHandle) -> Result<(), String> {
    // Check if this is the first launch (no database exists yet)
    let is_first_launch = DatabaseManager::is_first_launch(app)
        .await
        .map_err(|e| format!("Failed to check first launch status: {}", e))?;

    if is_first_launch {
        info!("First launch detected - will notify window when ready");

        // Delay event emission to ensure window is ready and React listeners are registered
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            app_handle
                .emit("first-launch-detected", ())
                .expect("Failed to emit first-launch-detected event");
            info!("Emitted first-launch-detected after delay");
        });
    } else {
        // Normal flow - initialize database immediately
        let db_manager = DatabaseManager::new_from_app_handle(app)
            .await
            .map_err(|e| format!("Failed to initialize database manager: {}", e))?;

        app.manage(AppState { db_manager });
        attach_retrieval_worker(app);
        info!("Database initialized successfully");
    }

    Ok(())
}

/// Idempotently starts the shared retrieval index worker once database state
/// exists. Called after AppState installation in all three paths (normal
/// startup, fresh creation, legacy import); duplicate calls are no-ops.
pub fn attach_retrieval_worker<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(lifecycle) = app.try_state::<RetrievalLifecycle>() else {
        log::warn!("Retrieval lifecycle not managed; index worker not started");
        return;
    };
    let Some(app_state) = app.try_state::<AppState>() else {
        log::warn!("AppState not available; index worker not started");
        return;
    };
    let pool = app_state.db_manager.pool().clone();
    lifecycle.attach_database(pool);
}
