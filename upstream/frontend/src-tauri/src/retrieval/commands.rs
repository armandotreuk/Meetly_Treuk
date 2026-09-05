//! Additive backend status/rebuild/pause contract for the retrieval query
//! index (Sprint 2B Task 2.5). No UI is introduced here; the Settings
//! surface arrives in Sprint 5 and consumes these same commands. Reports
//! carry counts, IDs, and measured sizes only - never raw meeting content.

use tauri::{AppHandle, Manager};

use crate::database::repositories::retrieval::RetrievalRepository;
use crate::retrieval::index::{self, RetrievalStatusReport};
use crate::retrieval::worker::{LifecycleOperation, RetrievalLifecycle};
use crate::state::AppState;
use sqlx::SqlitePool;

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

pub(crate) async fn ensure_no_active_operation(
    lifecycle: &RetrievalLifecycle,
    pool: &SqlitePool,
) -> Result<(), String> {
    if lifecycle.index_paused() {
        return Err("retrieval operation is paused".to_string());
    }
    let shadows = RetrievalRepository::shadow_generation_statuses(pool)
        .await
        .map_err(|error| error.to_string())?;
    if shadows.iter().any(index::shadow_operation_active) {
        return Err("retrieval operation already active".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn retrieval_index_status(app: AppHandle) -> Result<RetrievalStatusReport, String> {
    let (lifecycle, pool) = lifecycle_and_pool(&app)?;
    // Bounded like the service-side consistency retry in `index::index_status`:
    // rapid pause toggling must not spin this command through repeated full
    // status reads. The last attempt's report is returned regardless.
    let mut attempt = 0;
    loop {
        let paused = lifecycle.index_paused();
        let report = index::index_status(&pool, lifecycle.index_service().as_ref(), paused).await?;
        attempt += 1;
        if paused == lifecycle.index_paused() || attempt >= index::STATUS_CONSISTENCY_ATTEMPTS {
            return Ok(report);
        }
        tokio::task::yield_now().await;
    }
}

/// Registers a distinct shadow generation for the active model. Healthy
/// active retrieval continues while it builds; activation happens only after
/// every gate passes.
#[tauri::command]
pub async fn retrieval_rebuild_index(app: AppHandle) -> Result<String, String> {
    let (lifecycle, pool) = lifecycle_and_pool(&app)?;
    let _reservation = lifecycle
        .reserve_operation(LifecycleOperation::Rebuild, None)
        .await?;
    ensure_no_active_operation(&lifecycle, &pool).await?;
    index::request_rebuild(&pool)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn retrieval_retry_rebuild(app: AppHandle, generation_id: String) -> Result<(), String> {
    let (lifecycle, pool) = lifecycle_and_pool(&app)?;
    let _reservation = lifecycle
        .reserve_operation(LifecycleOperation::Retry, None)
        .await?;
    ensure_no_active_operation(&lifecycle, &pool).await?;
    if !RetrievalRepository::retry_failed_generation(&pool, &generation_id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("retrieval rebuild is not ready to retry".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn retrieval_cancel_rebuild(
    app: AppHandle,
    generation_id: String,
) -> Result<bool, String> {
    let (lifecycle, pool) = lifecycle_and_pool(&app)?;
    lifecycle
        .cancel_rebuild(&pool, &generation_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn retrieval_set_index_paused(app: AppHandle, paused: bool) -> Result<(), String> {
    let (lifecycle, _pool) = lifecycle_and_pool(&app)?;
    lifecycle.set_index_paused_command(paused).await
}

#[tauri::command]
pub async fn retrieval_clear_index(app: AppHandle) -> Result<(), String> {
    let (lifecycle, pool) = lifecycle_and_pool(&app)?;
    lifecycle
        .clear_index(&pool)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::retrieval::{ModelSpec, VectorEncoding};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    #[tokio::test]
    async fn active_shadow_blocks_mutating_command_until_terminal_failure() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::from_str("sqlite::memory:")
                    .unwrap()
                    .foreign_keys(true),
            )
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        RetrievalRepository::register_model(
            &pool,
            &ModelSpec {
                model_id: "command-test-model".to_string(),
                dimensions: 2,
                vector_encoding: VectorEncoding::Int8,
                chunker_version: 1,
                dequantization_scale: Some(1.0 / 127.0),
                dequantization_zero_point: Some(0),
            },
        )
        .await
        .unwrap();
        RetrievalRepository::register_generation(&pool, "command-shadow", "command-test-model")
            .await
            .unwrap();
        let lifecycle = RetrievalLifecycle::default();

        assert!(ensure_no_active_operation(&lifecycle, &pool).await.is_err());
        RetrievalRepository::mark_shadow_generation_failed(&pool, "command-shadow")
            .await
            .unwrap();
        assert!(ensure_no_active_operation(&lifecycle, &pool).await.is_ok());

        lifecycle.set_index_paused(true);
        assert!(ensure_no_active_operation(&lifecycle, &pool).await.is_err());
    }
}
