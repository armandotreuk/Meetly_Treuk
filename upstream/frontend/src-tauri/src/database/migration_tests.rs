//! Focused tests for the semantic retrieval migration (Sprint 2 Task 2.1):
//! fresh installs get the full additive schema, and legacy databases are
//! backfilled without any tokenization or inference.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

async fn connect() -> SqlitePool {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .foreign_keys(true);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap()
}

async fn object_exists(pool: &SqlitePool, sql: &str) -> bool {
    let count: i64 = sqlx::query_scalar(sql).fetch_one(pool).await.unwrap();
    count > 0
}

async fn table_sql(pool: &SqlitePool, name: &str) -> Option<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .unwrap()
    .map(|(sql,)| sql)
}

#[tokio::test]
async fn fresh_migration_installs_semantic_schema_without_active_state() {
    let pool = connect().await;
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    for table in [
        "retrieval_models",
        "retrieval_generations",
        "retrieval_active_model",
        "search_source_state",
        "retrieval_documents",
        "retrieval_document_staging",
        "retrieval_meeting_state",
        "retrieval_index_state",
        "retrieval_index_changes",
    ] {
        assert!(
            object_exists(
                &pool,
                &format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '{table}'"
                )
            )
            .await,
            "{table} must exist"
        );
    }
    for trigger in [
        "retrieval_meeting_insert",
        "retrieval_meeting_title_update",
        "retrieval_transcript_insert",
        "retrieval_transcript_update",
        "retrieval_transcript_delete",
        "retrieval_summary_insert",
        "retrieval_summary_result_update",
        "retrieval_summary_delete",
        "retrieval_notes_insert",
        "retrieval_notes_update",
        "retrieval_notes_delete",
        "retrieval_meeting_folder_update",
        "retrieval_folder_rename",
        "retrieval_folder_delete",
        "retrieval_tombstone_before_meeting_delete",
    ] {
        assert!(
            object_exists(
                &pool,
                &format!("SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = '{trigger}'")
            )
            .await,
            "trigger {trigger} must exist"
        );
    }
    for index in [
        "search_source_state_fts_due",
        "retrieval_documents_by_meeting",
        "retrieval_document_staging_by_generation",
        "retrieval_meeting_state_due",
        "retrieval_index_changes_replay",
    ] {
        assert!(
            object_exists(
                &pool,
                &format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = '{index}'"
                )
            )
            .await,
            "index {index} must exist"
        );
    }

    // Document tables stay rowid; meeting state stays WITHOUT ROWID.
    let documents_sql = table_sql(&pool, "retrieval_documents").await.unwrap();
    assert!(!documents_sql.to_uppercase().contains("WITHOUT ROWID"));
    let staging_sql = table_sql(&pool, "retrieval_document_staging")
        .await
        .unwrap();
    assert!(!staging_sql.to_uppercase().contains("WITHOUT ROWID"));
    let state_sql = table_sql(&pool, "retrieval_meeting_state").await.unwrap();
    assert!(state_sql.to_uppercase().contains("WITHOUT ROWID"));

    // The forward-only heading migration added a nullable provenance column.
    let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(retrieval_documents)")
            .fetch_all(&pool)
            .await
            .unwrap();
    let heading = columns
        .iter()
        .find(|(_, name, _, _, _, _)| name == "heading")
        .expect("retrieval_documents.heading must exist");
    assert_eq!(
        heading.3, 0,
        "heading must stay nullable so pre-migration rows keep None"
    );
    assert_eq!(heading.5, 0, "heading is not a primary-key column");

    // No fixed byte-width rule on vectors anywhere.
    for name in ["retrieval_documents", "retrieval_document_staging"] {
        let sql = table_sql(&pool, name).await.unwrap().to_lowercase();
        assert!(
            !sql.contains("dimensions * 4"),
            "{name} must not hardcode an f32 byte width"
        );
    }

    // Generation states exclude 'active'; the singleton forbids a second row.
    sqlx::query("INSERT INTO retrieval_models (model_id, dimensions, vector_encoding, chunker_version, created_at) VALUES ('m', 2, 'int8', 1, 'now')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO retrieval_generations (generation_id, model_id, state, created_at) VALUES ('g', 'm', 'building', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    for state in ["building", "ready", "failed", "retired"] {
        sqlx::query("UPDATE retrieval_generations SET state = ? WHERE generation_id = 'g'")
            .bind(state)
            .execute(&pool)
            .await
            .unwrap();
    }
    let rejected =
        sqlx::query("UPDATE retrieval_generations SET state = 'active' WHERE generation_id = 'g'")
            .execute(&pool)
            .await;
    assert!(
        rejected.is_err(),
        "'active' must violate the generation CHECK"
    );
}

#[tokio::test]
async fn legacy_backfill_seeds_existing_meetings_without_inference() {
    // Simulate a database from before the semantic retrieval feature: apply
    // every migration except the semantic ones, insert legacy data as it
    // existed before this task, then run the semantic migrations (including
    // the forward-only heading follow-up) in order.
    let pool = connect().await;
    let migrator = sqlx::migrate!("./migrations");
    const SEMANTIC_BASE_VERSION: i64 = 20260825000000;

    let mut conn = pool.acquire().await.unwrap();
    for migration in migrator.migrations.iter() {
        if migration.version < SEMANTIC_BASE_VERSION {
            sqlx::raw_sql(&migration.sql)
                .execute(&mut *conn)
                .await
                .unwrap();
        }
    }

    sqlx::query("INSERT INTO meeting_folders (id, name, parent_id, created_at) VALUES ('f', 'Legacy', NULL, '2026-01-01T00:00:00Z')")
        .execute(&mut *conn)
        .await
        .unwrap();
    for id in ["legacy-a", "legacy-b"] {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_id) VALUES (?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'f')")
            .bind(id)
            .bind(format!("Legacy {id}"))
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('lt', 'legacy-a', 'old transcript text', '09:00')")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at, result) VALUES ('legacy-a', 'std', 'completed', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '{\"markdown\":\"old summary\"}')")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO meeting_notes (meeting_id, notes_markdown, created_at, updated_at) VALUES ('legacy-b', 'old notes', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
        .execute(&mut *conn)
        .await
        .unwrap();

    for migration in migrator.migrations.iter() {
        if migration.version >= SEMANTIC_BASE_VERSION {
            sqlx::raw_sql(&migration.sql)
                .execute(&mut *conn)
                .await
                .unwrap();
        }
    }
    drop(conn);

    // Every legacy meeting is seeded with pending FTS repair work.
    let seeded: Vec<(String, i64, i64, i64)> =
        sqlx::query_as("SELECT meeting_id, source_revision, fts_projection_revision, fts_indexed_revision FROM search_source_state ORDER BY meeting_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(seeded.len(), 2);
    for row in &seeded {
        assert_eq!((row.1, row.2, row.3), (1, 1, 0));
    }
    assert!(seeded.iter().any(|row| row.0 == "legacy-a"));

    // The backfill enqueues only; no derived or model artifacts were produced.
    for table in [
        "retrieval_models",
        "retrieval_generations",
        "retrieval_documents",
        "retrieval_document_staging",
        "retrieval_index_changes",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "{table} must be empty after a pure backfill");
    }

    // The forward-only heading follow-up applied cleanly on top of the
    // legacy database with its nullable contract intact.
    let heading_column: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('retrieval_documents')
         WHERE name = 'heading' AND \"notnull\" = 0",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(heading_column, 1);
}

#[tokio::test]
async fn heading_migration_requeues_headingless_live_rows_without_touching_failures() {
    let pool = connect().await;
    let migrator = sqlx::migrate!("./migrations");
    const SEMANTIC_BASE_VERSION: i64 = 20260825000000;
    const HEADING_VERSION: i64 = 20260826000000;

    let mut conn = pool.acquire().await.unwrap();
    for migration in migrator.migrations.iter() {
        if migration.version < SEMANTIC_BASE_VERSION {
            sqlx::raw_sql(&migration.sql)
                .execute(&mut *conn)
                .await
                .unwrap();
        }
    }
    sqlx::raw_sql(
        &migrator
            .migrations
            .iter()
            .find(|migration| migration.version == SEMANTIC_BASE_VERSION)
            .unwrap()
            .sql,
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('live', 'Live', 'now', 'now'), ('failed', 'Failed', 'now', 'now')")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO retrieval_models (model_id, dimensions, vector_encoding, chunker_version, created_at) VALUES ('model', 1, 'int8', 1, 'now')")
        .execute(&mut *conn)
        .await
        .unwrap();
    for (generation_id, state) in [("building", "building"), ("ready", "ready")] {
        sqlx::query("INSERT INTO retrieval_generations (generation_id, model_id, state, created_at) VALUES (?, 'model', ?, 'now')")
            .bind(generation_id)
            .bind(state)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE search_source_state SET source_revision = 7, fts_projection_revision = 7, changed_at = 'now' WHERE meeting_id = 'live'")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("UPDATE search_source_state SET source_revision = 9, fts_projection_revision = 9, changed_at = 'now' WHERE meeting_id = 'failed'")
        .execute(&mut *conn)
        .await
        .unwrap();
    for generation_id in ["building", "ready"] {
        sqlx::query("INSERT INTO retrieval_meeting_state (generation_id, meeting_id, indexed_source_revision, state, updated_at) VALUES (?, 'live', 7, 'ready', 'before')")
            .bind(generation_id)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO retrieval_meeting_state (generation_id, meeting_id, indexed_source_revision, state, attempt_count, next_attempt_at, last_error, updated_at) VALUES ('ready', 'failed', 9, 'failed', 4, 'later', 'terminal', 'before')")
        .execute(&mut *conn)
        .await
        .unwrap();
    for (generation_id, meeting_id) in
        [("building", "live"), ("ready", "live"), ("ready", "failed")]
    {
        sqlx::query("INSERT INTO retrieval_documents (generation_id, document_id, meeting_id, source_kind, ordinal, content, content_hash, dimensions, vector_encoding, vector, source_revision, updated_at) VALUES (?, ?, ?, 'summary', 0, 'content', X'01', 1, 'int8', X'02', 7, 'before')")
            .bind(generation_id)
            .bind(format!("{generation_id}-{meeting_id}"))
            .bind(meeting_id)
            .execute(&mut *conn)
            .await
            .unwrap();
    }

    let heading_migration = migrator
        .migrations
        .iter()
        .find(|migration| migration.version == HEADING_VERSION)
        .unwrap();
    let migration_result = sqlx::raw_sql(&heading_migration.sql)
        .execute(&mut *conn)
        .await;
    assert!(migration_result.is_ok(), "heading migration must succeed");
    drop(conn);

    let live_rows: Vec<(String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT generation_id, state, indexed_source_revision, next_attempt_at
         FROM retrieval_meeting_state WHERE meeting_id = 'live' ORDER BY generation_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        live_rows,
        vec![
            ("building".into(), "pending".into(), 0, None),
            ("ready".into(), "pending".into(), 0, None),
        ]
    );

    let failed_row: (String, i64, i64, Option<String>, Option<String>, String) = sqlx::query_as(
        "SELECT state, indexed_source_revision, attempt_count, next_attempt_at, last_error, updated_at
         FROM retrieval_meeting_state WHERE generation_id = 'ready' AND meeting_id = 'failed'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        failed_row,
        (
            "failed".into(),
            9,
            4,
            Some("later".into()),
            Some("terminal".into()),
            "before".into()
        )
    );

    let heading_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM retrieval_documents WHERE heading IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(heading_count, 3);

    let due =
        crate::database::repositories::retrieval::RetrievalRepository::list_due_generation_work(
            &pool, "ready", "now", 10,
        )
        .await
        .unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].meeting_id, "live");
    assert_eq!(due[0].indexed_source_revision, 0);
}
