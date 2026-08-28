//! Focused tests for the semantic retrieval migration (Sprint 2 Task 2.1):
//! fresh installs get the full additive schema, and legacy databases are
//! backfilled without any tokenization or inference.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::database::repositories::meeting::MeetingsRepository;

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

const RETRIEVAL_DOCUMENTS_MEETING_LOOKUP_INDEX: &str = "retrieval_documents_by_meeting_lookup";

async fn assert_meeting_lookup_index(pool: &SqlitePool) {
    let columns: Vec<(i64, String)> = sqlx::query_as(&format!(
        "SELECT seqno, name FROM pragma_index_info('{RETRIEVAL_DOCUMENTS_MEETING_LOOKUP_INDEX}') ORDER BY seqno"
    ))
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(
        columns,
        vec![(0, "meeting_id".into()), (1, "generation_id".into())]
    );

    let plan: Vec<String> = sqlx::query_as::<_, (i64, i64, i64, String)>(
        "EXPLAIN QUERY PLAN
         SELECT DISTINCT generation_id
         FROM retrieval_documents
         WHERE meeting_id = ?
         ORDER BY generation_id",
    )
    .bind("deleted-meeting")
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|(_, _, _, detail)| detail)
    .collect();
    // The invariant is that the lookup is index-driven, asserted by rejecting a
    // scan. Do not match the rest of EXPLAIN QUERY PLAN's prose: SQLite states
    // that output is not a stable interface, so a libsqlite3-sys bump that
    // reworded it would fail here as if the schema had regressed.
    assert!(
        plan.iter()
            .any(|detail| detail.contains(RETRIEVAL_DOCUMENTS_MEETING_LOOKUP_INDEX)),
        "affected-generation lookup must use the meeting lookup index: {plan:?}"
    );
    assert!(
        plan.iter()
            .all(|detail| !detail.contains("SCAN retrieval_documents")),
        "affected-generation lookup must not scan retrieval_documents: {plan:?}"
    );
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
        RETRIEVAL_DOCUMENTS_MEETING_LOOKUP_INDEX,
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

    assert_meeting_lookup_index(&pool).await;

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

    assert_meeting_lookup_index(&pool).await;

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
    // The heading migration has shipped, so its text is immutable: editing it
    // changes the sqlx checksum and fails startup with VersionMismatch on any
    // database that already applied it. This asserts the shipped text is
    // intact; its CURRENT_TIMESTAMP value is corrected by the forward-only
    // 20260827020000 migration instead.
    assert!(
        heading_migration.sql.contains("CURRENT_TIMESTAMP"),
        "shipped migration text must never be edited in place"
    );
    let normalization = migrator
        .migrations
        .iter()
        .find(|migration| migration.version == 20260827020000)
        .expect("forward-only timestamp normalization migration must exist");
    assert!(normalization
        .sql
        .contains("replace(updated_at, ' ', 'T') || '.000Z'"));
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

const LEGACY_IDENTITY_VERSION: i64 = 20260827000000;

/// Seeds exactly what the previous binary persisted under the legacy
/// bundle-id model identity: one model row, a fully published ready
/// generation pointing at it, its documents, per-meeting state, and journal.
async fn seed_legacy_identity_database(pool: &sqlx::Pool<sqlx::Sqlite>) {
    let mut conn = pool.acquire().await.unwrap();
    let migrator = sqlx::migrate!("./migrations");
    for migration in migrator.migrations.iter() {
        if migration.version < LEGACY_IDENTITY_VERSION {
            sqlx::raw_sql(&migration.sql)
                .execute(&mut *conn)
                .await
                .unwrap();
        }
    }
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('legacy-meeting', 'Legacy', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO retrieval_models (model_id, dimensions, vector_encoding, chunker_version, dequantization_scale, dequantization_zero_point, created_at)
         VALUES ('meetily-retrieval-bundle-1', 768, 'int8', 1, ?, 0, '2026-08-20T00:00:00Z')",
    )
    .bind(1.0_f64 / 127.0)
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO retrieval_generations (generation_id, model_id, state, created_at) VALUES ('gen-legacy', 'meetily-retrieval-bundle-1', 'ready', '2026-08-21T00:00:00Z')")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO retrieval_index_state (generation_id, backend, state, document_count, canonical_change_id, published_change_id, updated_at)
         VALUES ('gen-legacy', 'exact', 'ready', 1, 2, 2, '2026-08-21T00:00:00Z')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO retrieval_meeting_state (generation_id, meeting_id, indexed_source_revision, state, updated_at)
         VALUES ('gen-legacy', 'legacy-meeting', 1, 'ready', '2026-08-21T00:00:00Z')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO retrieval_documents (generation_id, document_id, meeting_id, source_kind, ordinal, content, content_hash, dimensions, vector_encoding, vector, source_revision, updated_at)
         VALUES ('gen-legacy', 'doc-legacy-0', 'legacy-meeting', 'summary', 0, 'derived legacy text', X'010203', 768, 'int8', X'7F', 1, '2026-08-21T00:00:00Z')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO retrieval_index_changes (generation_id, meeting_id, operation, source_revision, created_at)
         VALUES ('gen-legacy', 'legacy-meeting', 'upsert', 1, '2026-08-21T00:00:00Z')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO retrieval_index_changes (generation_id, meeting_id, operation, source_revision, created_at)
         VALUES ('gen-legacy', 'legacy-meeting', 'upsert', 1, '2026-08-21T00:00:01Z')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
}

#[tokio::test]
async fn legacy_bundle_identity_migrates_in_place_without_reindexing() {
    let pool = connect().await;
    let migrated_model_id = crate::retrieval::worker::bundled_model_identity();
    assert_ne!(
        migrated_model_id, "meetily-retrieval-bundle-1",
        "the derived identity must not equal the legacy bundle id"
    );
    seed_legacy_identity_database(&pool).await;

    let mut conn = pool.acquire().await.unwrap();
    let migration = sqlx::migrate!("./migrations")
        .migrations
        .iter()
        .find(|migration| migration.version == LEGACY_IDENTITY_VERSION)
        .unwrap();
    sqlx::raw_sql(&migration.sql)
        .execute(&mut *conn)
        .await
        .unwrap();
    // Forward-only guard rail: replaying the statements is a clean no-op once
    // the legacy row is gone (the production migrator never reruns a version).
    sqlx::raw_sql(&migration.sql)
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    // Insert new identity, repoint generations, remove legacy identity.
    let model_row: Option<(i64, String, i64, f64, i64)> = sqlx::query_as(
        "SELECT dimensions, vector_encoding, chunker_version, dequantization_scale, dequantization_zero_point
         FROM retrieval_models WHERE model_id = ?",
    )
    .bind(&migrated_model_id)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_eq!(
        model_row,
        Some((768, "int8".into(), 1, 1.0 / 127.0, 0)),
        "the derived identity must carry the identical storage contract"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM retrieval_models")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1,
        "the legacy model row must be gone"
    );

    let generation: (String,) = sqlx::query_as(
        "SELECT model_id FROM retrieval_generations WHERE generation_id = 'gen-legacy'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(generation.0, migrated_model_id);

    // Documents, per-meeting state, bounds, journal, and pointer untouched...
    let document: (String, String, i64) = sqlx::query_as(
        "SELECT document_id, content, length(vector) FROM retrieval_documents WHERE generation_id = 'gen-legacy'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        document,
        ("doc-legacy-0".into(), "derived legacy text".into(), 1)
    );
    let meeting_state: (String, i64) = sqlx::query_as(
        "SELECT state, indexed_source_revision FROM retrieval_meeting_state
          WHERE generation_id = 'gen-legacy' AND meeting_id = 'legacy-meeting'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(meeting_state, ("ready".into(), 1));
    let bounds: (i64, i64) = sqlx::query_as(
        "SELECT canonical_change_id, published_change_id FROM retrieval_index_state WHERE generation_id = 'gen-legacy'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bounds, (2, 2));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM retrieval_index_changes")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
    assert!(
        sqlx::query_as::<_, (String,)>("SELECT generation_id FROM retrieval_active_model")
            .fetch_optional(&pool)
            .await
            .unwrap()
            .is_none()
    );

    // ...and because per-meeting state stayed current, the migrated
    // generation owes ZERO work: no re-indexing was triggered.
    let due =
        crate::database::repositories::retrieval::RetrievalRepository::list_due_generation_work(
            &pool,
            "gen-legacy",
            "2099-12-31T00:00:00Z",
            10,
        )
        .await
        .unwrap();
    assert!(due.is_empty(), "migration must cause zero reindexing");
}

#[tokio::test]
async fn fresh_databases_run_the_legacy_identity_rewrite_as_a_noop() {
    let pool = connect().await;
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let models: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM retrieval_models")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        models, 0,
        "a fresh database has no legacy identity to rewrite"
    );
}

#[tokio::test]
async fn terminal_tombstone_repair_migrates_populated_databases_without_acknowledging_tails() {
    const REPAIR_VERSION: i64 = 20260827010000;
    let pool = connect().await;
    let migrator = sqlx::migrate!("./migrations");

    // Run the database through the shipped trigger, but stop before the
    // forward-only repair so this fixture contains the upgrade defect.
    let mut conn = pool.acquire().await.unwrap();
    for migration in migrator.migrations.iter() {
        if migration.version < REPAIR_VERSION {
            sqlx::raw_sql(&migration.sql)
                .execute(&mut *conn)
                .await
                .unwrap();
        }
    }
    sqlx::query(
        "INSERT INTO retrieval_models (model_id, dimensions, vector_encoding, chunker_version, dequantization_scale, dequantization_zero_point, created_at)
         VALUES ('model', 1, 'int8', 1, 1.0, 0, 'before')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    for (generation_id, state) in [
        ("gen-ready", "ready"),
        ("gen-building", "building"),
        ("gen-retired", "retired"),
        ("gen-failed", "failed"),
    ] {
        sqlx::query(
            "INSERT INTO retrieval_generations (generation_id, model_id, state, document_count, created_at)
             VALUES (?, 'model', ?, 1, 'before')",
        )
        .bind(generation_id)
        .bind(state)
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO retrieval_index_state (generation_id, backend, state, document_count, canonical_change_id, published_change_id, updated_at)
             VALUES (?, 'exact', ?, 1, 0, 0, 'before')",
        )
        .bind(generation_id)
        .bind(state)
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO retrieval_active_model (singleton, generation_id, activated_at)
         VALUES (1, 'gen-ready', 'before')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO meetings (id, title, created_at, updated_at)
         VALUES ('doomed', 'Doomed', 'before', 'before')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO transcripts (id, meeting_id, transcript, timestamp)
         VALUES ('transcript', 'doomed', 'primary text', '10:00')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO summary_processes (meeting_id, template_id, status, created_at, updated_at)
         VALUES ('doomed', 'template', 'pending', 'before', 'before')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO meeting_notes (meeting_id, notes_markdown, created_at, updated_at)
         VALUES ('doomed', 'primary notes', 'before', 'before')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();

    let mut published_bounds = Vec::new();
    for generation_id in ["gen-ready", "gen-building", "gen-retired", "gen-failed"] {
        let change_id = sqlx::query(
            "INSERT INTO retrieval_index_changes (generation_id, meeting_id, operation, source_revision, created_at)
             VALUES (?, 'doomed', 'upsert', 1, 'before')",
        )
        .bind(generation_id)
        .execute(&mut *conn)
        .await
        .unwrap()
        .last_insert_rowid();
        sqlx::query(
            "UPDATE retrieval_index_state
             SET canonical_change_id = ?, published_change_id = ?
             WHERE generation_id = ?",
        )
        .bind(change_id)
        .bind(change_id)
        .bind(generation_id)
        .execute(&mut *conn)
        .await
        .unwrap();
        published_bounds.push((generation_id, change_id));

        sqlx::query(
            "INSERT INTO retrieval_documents (generation_id, document_id, meeting_id, source_kind, ordinal, content, content_hash, dimensions, vector_encoding, vector, source_revision, updated_at)
             VALUES (?, ?, 'doomed', 'transcript', 0, 'derived', X'01', 1, 'int8', X'01', 1, 'before')",
        )
        .bind(generation_id)
        .bind(format!("doc-{generation_id}"))
        .execute(&mut *conn)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO retrieval_document_staging (job_id, generation_id, meeting_id, source_revision, document_id, payload)
             VALUES (?, ?, 'doomed', 1, ?, X'01')",
        )
        .bind(format!("job-{generation_id}"))
        .bind(generation_id)
        .bind(format!("staged-{generation_id}"))
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    drop(conn);

    // The old trigger appends a delete tail to all four index states. The
    // normal meeting deletion path still owns the primary-data transaction.
    assert!(MeetingsRepository::delete_meeting(&pool, "doomed")
        .await
        .unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meetings WHERE id = 'doomed'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    for table in [
        "transcripts",
        "summary_processes",
        "meeting_notes",
        "search_source_state",
        "retrieval_documents",
        "retrieval_document_staging",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COUNT(*) FROM {table} WHERE meeting_id = 'doomed'"
            ))
            .fetch_one(&pool)
            .await
            .unwrap(),
            0,
            "primary/derived rows for the deleted meeting must cascade"
        );
    }

    let before_repair: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT generation_id, canonical_change_id, published_change_id
         FROM retrieval_index_state ORDER BY generation_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(before_repair
        .iter()
        .all(|(_, canonical, published)| canonical > published));

    let repair = migrator
        .migrations
        .iter()
        .find(|migration| migration.version == REPAIR_VERSION)
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    sqlx::raw_sql(&repair.sql)
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    // Only live generations retain the new delete tail. Terminal tails are
    // discarded as obsolete; their published bounds remain exactly unchanged.
    for generation_id in ["gen-ready", "gen-building"] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM retrieval_index_changes
                 WHERE generation_id = ? AND operation = 'delete'",
            )
            .bind(generation_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
    }
    sqlx::query(
        "INSERT INTO meetings (id, title, created_at, updated_at)
         VALUES ('future', 'Future', 'after', 'after')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(MeetingsRepository::delete_meeting(&pool, "future")
        .await
        .unwrap());
    for generation_id in ["gen-ready", "gen-building"] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM retrieval_index_changes
                 WHERE generation_id = ? AND operation = 'delete'",
            )
            .bind(generation_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        let bounds: (i64, i64) = sqlx::query_as(
            "SELECT canonical_change_id, published_change_id
             FROM retrieval_index_state WHERE generation_id = ?",
        )
        .bind(generation_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(bounds.0 > bounds.1);
    }
    for generation_id in ["gen-retired", "gen-failed"] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM retrieval_index_changes
                 WHERE generation_id = ? AND operation = 'delete'",
            )
            .bind(generation_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
    }

    for (generation_id, published) in published_bounds.into_iter().filter(|(generation_id, _)| {
        *generation_id == "gen-retired" || *generation_id == "gen-failed"
    }) {
        let bounds: (i64, i64) = sqlx::query_as(
            "SELECT canonical_change_id, published_change_id
             FROM retrieval_index_state WHERE generation_id = ?",
        )
        .bind(generation_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(bounds, (published, published));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM retrieval_index_changes
                 WHERE generation_id = ? AND change_id > ?",
            )
            .bind(generation_id)
            .bind(published)
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert!(
            crate::database::repositories::retrieval::RetrievalRepository::delete_generation(
                &pool,
                generation_id,
            )
            .await
            .unwrap(),
            "repair must make obsolete terminal state reclaimable"
        );
    }
}
