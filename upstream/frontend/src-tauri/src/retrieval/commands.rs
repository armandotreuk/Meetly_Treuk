//! Additive backend status/rebuild/pause contract for the retrieval query
//! index (Sprint 2B Task 2.5). No UI is introduced here; the Settings
//! surface arrives in Sprint 5 and consumes these same commands. Reports
//! carry counts, IDs, and measured sizes only - never raw meeting content.

use tauri::{AppHandle, Manager};

use crate::database::repositories::retrieval::RetrievalRepository;
use crate::retrieval::index::{self, RetrievalStatusReport};
use crate::retrieval::worker::RetrievalLifecycle;
use crate::state::AppState;

fn lifecycle_and_pool<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<(tauri::State<'_, RetrievalLifecycle>, sqlx::SqlitePool), String> {
    let lifecycle = app
        .try_state::<RetrievalLifecycle>()
        .ok_or_else(|| "retrieval lifecycle unavailable".to_string())?;
    let pool = app
        .try_state::<AppState>()
        .map(|state| state.db_manager.pool().clone())
        .ok_or_else(|| "database unavailable".to_string())?;
    Ok((lifecycle, pool))
}

#[tauri::command]
pub async fn retrieval_index_status(app: AppHandle) -> Result<RetrievalStatusReport, String> {
    let (lifecycle, pool) = lifecycle_and_pool(&app)?;
    index::index_status(
        &pool,
        lifecycle.index_service().as_ref(),
        lifecycle.index_paused(),
    )
    .await
}

/// Registers a distinct shadow generation for the active model. Healthy
/// active retrieval continues while it builds; activation happens only after
/// every gate passes.
#[tauri::command]
pub async fn retrieval_rebuild_index(app: AppHandle) -> Result<String, String> {
    let (_lifecycle, pool) = lifecycle_and_pool(&app)?;
    index::request_rebuild(&pool)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn retrieval_cancel_rebuild(
    app: AppHandle,
    generation_id: String,
) -> Result<bool, String> {
    let (_lifecycle, pool) = lifecycle_and_pool(&app)?;
    RetrievalRepository::cancel_building_generation(&pool, &generation_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn retrieval_set_index_paused(app: AppHandle, paused: bool) -> Result<(), String> {
    let (lifecycle, _pool) = lifecycle_and_pool(&app)?;
    lifecycle.set_index_paused(paused);
    Ok(())
}
