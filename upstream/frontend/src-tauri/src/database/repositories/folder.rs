// Logical folders for grouping meetings (multi-level, in-DB only).
// Disk layout (folder_path on meetings) is never touched here.

use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

use crate::database::models::MeetingFolderModel;
use crate::database::repositories::fts::FtsRepository;

pub struct FolderRepository;

impl FolderRepository {
    /// All folders, alphabetical within each level. Caller builds the tree client-side.
    pub async fn get_all(pool: &SqlitePool) -> Result<Vec<MeetingFolderModel>, SqlxError> {
        sqlx::query_as::<_, MeetingFolderModel>(
            "SELECT id, name, parent_id, created_at FROM meeting_folders ORDER BY name ASC",
        )
        .fetch_all(pool)
        .await
    }

    pub async fn create(
        pool: &SqlitePool,
        name: &str,
        parent_id: Option<&str>,
    ) -> Result<MeetingFolderModel, SqlxError> {
        if name.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "folder name cannot be empty".to_string(),
            ));
        }
        let id = format!("folder-{}", Uuid::new_v4());
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO meeting_folders (id, name, parent_id, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name.trim())
        .bind(parent_id)
        .bind(now)
        .execute(pool)
        .await?;

        info!(
            "Created folder id={} name={:?} parent={:?}",
            id, name, parent_id
        );
        Self::get_by_id(pool, &id)
            .await?
            .ok_or_else(|| SqlxError::Protocol(format!("folder {} vanished after insert", id)))
    }

    pub async fn get_by_id(
        pool: &SqlitePool,
        id: &str,
    ) -> Result<Option<MeetingFolderModel>, SqlxError> {
        sqlx::query_as::<_, MeetingFolderModel>(
            "SELECT id, name, parent_id, created_at FROM meeting_folders WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    pub async fn get_subtree_ids(pool: &SqlitePool, id: &str) -> Result<Vec<String>, SqlxError> {
        Ok(sqlx::query_scalar(
            r#"
            WITH RECURSIVE subtree(id) AS (
                SELECT id FROM meeting_folders WHERE id = ?
                UNION ALL
                SELECT f.id FROM meeting_folders f JOIN subtree s ON f.parent_id = s.id
            )
            SELECT id FROM subtree
            "#,
        )
        .bind(id)
        .fetch_all(pool)
        .await?)
    }

    pub async fn rename(pool: &SqlitePool, id: &str, name: &str) -> Result<bool, SqlxError> {
        if name.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "folder name cannot be empty".to_string(),
            ));
        }
        let r = sqlx::query("UPDATE meeting_folders SET name = ? WHERE id = ?")
            .bind(name.trim())
            .bind(id)
            .execute(pool)
            .await?;
        if r.rows_affected() > 0 {
            // Update FTS folder_name for all meetings in this folder
            if let Err(e) = FtsRepository::sync_folder(pool, id).await {
                error!("Failed to sync FTS for folder {}: {}", id, e);
            }
        }
        Ok(r.rows_affected() > 0)
    }

    /// Move folder `id` under `new_parent_id` (None = root). Rejects cycles:
    /// new_parent cannot be `id` itself or any descendant of `id` (which would
    /// make the tree unreachable). Detects by walking UP from new_parent; if we
    /// hit `id`, it's a cycle. ponytail: SQLite recursive CTE is stdlib graph
    /// walk; no need for an in-memory tree.
    pub async fn move_folder(
        pool: &SqlitePool,
        id: &str,
        new_parent_id: Option<&str>,
    ) -> Result<(), String> {
        if id.trim().is_empty() {
            return Err("folder id cannot be empty".to_string());
        }

        // Reject moving a folder into itself.
        if let Some(pid) = new_parent_id {
            if pid == id {
                return Err("Cannot move a folder into itself".to_string());
            }
        }

        let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
        let mut tx = conn.begin().await.map_err(|e| e.to_string())?;

        // Cycle check: climb ancestors from new_parent; if any equals `id`, reject.
        if let Some(pid) = new_parent_id {
            let cycle: Option<(String,)> = sqlx::query_as(
                r#"
                WITH RECURSIVE ancestors(id) AS (
                    SELECT parent_id FROM meeting_folders WHERE id = ?
                    UNION ALL
                    SELECT f.parent_id FROM meeting_folders f
                    JOIN ancestors a ON f.id = a.id
                    WHERE f.parent_id IS NOT NULL
                )
                SELECT id FROM ancestors WHERE id = ?
                "#,
            )
            .bind(pid)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

            if cycle.is_some() {
                let _ = tx.rollback().await;
                return Err("Cannot move a folder into one of its own subfolders".to_string());
            }
        }

        let res = sqlx::query("UPDATE meeting_folders SET parent_id = ? WHERE id = ?")
            .bind(new_parent_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        if res.rows_affected() == 0 {
            let _ = tx.rollback().await;
            return Err(format!("folder {} not found", id));
        }

        tx.commit().await.map_err(|e| e.to_string())?;
        info!("Moved folder {} under {:?}", id, new_parent_id);
        Ok(())
    }

    /// Delete folder + all descendants in one transaction; all meetings that lived
    /// anywhere in the subtree end up with folder_id = NULL (= "Sem pasta").
    /// ponytail: explicit cascade; FKs aren't enforced at runtime (see delete_meeting_with_transaction).
    pub async fn delete_with_cascade(pool: &SqlitePool, id: &str) -> Result<bool, SqlxError> {
        if id.trim().is_empty() {
            return Err(SqlxError::Protocol("folder id cannot be empty".to_string()));
        }

        // Transactional part runs in its own scope so the acquired
        // connection returns to the pool before the best-effort FTS queries
        // below; holding it across them starves a max-1 pool.
        let subtree: Vec<(String,)> = {
            let mut conn = pool.acquire().await?;
            let mut tx = conn.begin().await?;

            // Collect id + all descendants.
            let subtree: Vec<(String,)> = sqlx::query_as(
                r#"
                WITH RECURSIVE subtree(id) AS (
                    SELECT id FROM meeting_folders WHERE id = ?
                    UNION ALL
                    SELECT f.id FROM meeting_folders f
                    JOIN subtree s ON f.parent_id = s.id
                )
                SELECT id FROM subtree
                "#,
            )
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;

            if subtree.is_empty() {
                tx.rollback().await?;
                return Ok(false);
            }

            // Detach all meetings in the subtree -> "Sem pasta".
            for (fid,) in &subtree {
                sqlx::query("UPDATE meetings SET folder_id = NULL WHERE folder_id = ?")
                    .bind(fid)
                    .execute(&mut *tx)
                    .await?;
            }

            // Delete folder rows.
            for (fid,) in &subtree {
                sqlx::query("DELETE FROM meeting_folders WHERE id = ?")
                    .bind(fid)
                    .execute(&mut *tx)
                    .await?;
            }

            tx.commit().await?;
            subtree
        };
        info!(
            "Deleted folder {} and {} descendants; affected meetings -> Sem pasta",
            id,
            subtree.len() - 1
        );

        // Clear folder_name in FTS for all affected folder_ids (best-effort)
        for (fid,) in &subtree {
            if let Err(e) =
                sqlx::query("UPDATE meeting_fts SET folder_name = '' WHERE folder_id = ?")
                    .bind(fid)
                    .execute(pool)
                    .await
            {
                error!(
                    "Failed to clear FTS folder_name for deleted folder {}: {}",
                    fid, e
                );
            }
        }

        Ok(true)
    }

    /// Attach meeting to folder (None = "Sem pasta"). Validates folder exists.
    pub async fn set_meeting_folder(
        pool: &SqlitePool,
        meeting_id: &str,
        folder_id: Option<&str>,
    ) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut tx = pool.begin().await?;

        if let Some(fid) = folder_id {
            let exists: Option<(i64,)> =
                sqlx::query_as("SELECT 1 FROM meeting_folders WHERE id = ?")
                    .bind(fid)
                    .fetch_optional(&mut *tx)
                    .await?;
            if exists.is_none() {
                tx.rollback().await?;
                error!("set_meeting_folder: folder {} not found", fid);
                return Ok(false);
            }
        }

        let res = sqlx::query("UPDATE meetings SET folder_id = ? WHERE id = ?")
            .bind(folder_id)
            .bind(meeting_id)
            .execute(&mut *tx)
            .await?;

        if res.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }

        tx.commit().await?;

        // Refresh FTS for the meeting to update folder_id and folder_name (best-effort)
        if let Err(e) = FtsRepository::refresh_meeting(pool, meeting_id).await {
            error!(
                "Failed to refresh FTS for meeting {} after folder change: {}",
                meeting_id, e
            );
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect(":memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE meeting_folders (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                parent_id TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE meetings (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                folder_path TEXT,
                folder_id TEXT
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("create schema");
        pool
    }

    async fn seed_meeting(pool: &SqlitePool, id: &str) {
        let now = Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(format!("meeting {}", id))
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await
            .expect("seed meeting");
    }

    async fn meeting_folder_id(pool: &SqlitePool, id: &str) -> Option<String> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT folder_id FROM meetings WHERE id = ?")
                .bind(id)
                .fetch_optional(pool)
                .await
                .expect("read meeting folder_id");
        row.and_then(|(v,)| v)
    }

    #[tokio::test]
    async fn cycle_rejected() {
        let pool = setup().await;
        let a = FolderRepository::create(&pool, "A", None).await.unwrap();
        let b = FolderRepository::create(&pool, "B", Some(&a.id))
            .await
            .unwrap();
        // Try to move A under B -> A is ancestor of B, must reject.
        let err = FolderRepository::move_folder(&pool, &a.id, Some(&b.id))
            .await
            .expect_err("expected cycle rejection");
        assert!(err.contains("its own subfolders"));
    }

    #[tokio::test]
    async fn delete_cascade_unfiles_meetings() {
        let pool = setup().await;
        let parent = FolderRepository::create(&pool, "Work", None).await.unwrap();
        let child = FolderRepository::create(&pool, "Project X", Some(&parent.id))
            .await
            .unwrap();
        seed_meeting(&pool, "m-1").await;
        seed_meeting(&pool, "m-2").await;
        assert!(
            FolderRepository::set_meeting_folder(&pool, "m-1", Some(&parent.id))
                .await
                .unwrap()
        );
        assert!(
            FolderRepository::set_meeting_folder(&pool, "m-2", Some(&child.id))
                .await
                .unwrap()
        );

        let ok = FolderRepository::delete_with_cascade(&pool, &parent.id)
            .await
            .unwrap();
        assert!(ok, "delete should report success");

        // Both meetings must now be in "Sem pasta" (folder_id == NULL).
        assert_eq!(meeting_folder_id(&pool, "m-1").await, None);
        assert_eq!(meeting_folder_id(&pool, "m-2").await, None);
        // Folder rows gone.
        assert_eq!(FolderRepository::get_all(&pool).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn move_to_root_works() {
        let pool = setup().await;
        let a = FolderRepository::create(&pool, "A", None).await.unwrap();
        let b = FolderRepository::create(&pool, "B", Some(&a.id))
            .await
            .unwrap();
        // Move B to root (parent None) — allowed.
        FolderRepository::move_folder(&pool, &b.id, None)
            .await
            .unwrap();
        let folders = FolderRepository::get_all(&pool).await.unwrap();
        let b_row = folders.iter().find(|f| f.id == b.id).unwrap();
        assert!(b_row.parent_id.is_none());
    }

    #[tokio::test]
    async fn set_meeting_to_unknown_folder_no_op() {
        let pool = setup().await;
        seed_meeting(&pool, "m-1").await;
        let ok = FolderRepository::set_meeting_folder(&pool, "m-1", Some("nonexistent"))
            .await
            .unwrap();
        assert!(!ok, "should refuse unknown folder id");
    }

    /// Regression: the post-commit best-effort FTS updates must run with the
    /// transaction connection already released, or a max-1 pool waits out its
    /// whole acquire timeout inside delete_with_cascade.
    #[tokio::test]
    async fn delete_with_cascade_returns_promptly_on_one_connection_pool() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;

        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("connect one-connection sqlite");
        sqlx::query(
            r#"
            CREATE TABLE meeting_folders (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                parent_id TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE meetings (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                folder_path TEXT,
                folder_id TEXT
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("create schema");
        seed_meeting(&pool, "m-1").await;
        let folder = FolderRepository::create(&pool, "Work", None).await.unwrap();
        assert!(
            FolderRepository::set_meeting_folder(&pool, "m-1", Some(&folder.id))
                .await
                .unwrap()
        );

        let deleted = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            FolderRepository::delete_with_cascade(&pool, &folder.id),
        )
        .await
        .expect("delete_with_cascade must not starve a max-1 pool")
        .expect("delete succeeds");
        assert!(deleted);
    }
}
