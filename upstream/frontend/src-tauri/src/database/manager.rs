use crate::database::templates_sync::sync_builtin_templates;
use sqlx::{
    migrate::MigrateDatabase,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Result, Sqlite, SqlitePool, Transaction,
};
use std::fs;
use std::path::Path;
use std::str::FromStr;

const INITIAL_SCHEMA_TABLES: &[&str] = &[
    "meetings",
    "transcripts",
    "summary_processes",
    "transcript_chunks",
    "settings",
    "transcript_settings",
];

struct LegacyMigrationChecksumRepair {
    version: i64,
    legacy_checksum_hex: &'static str,
    required_tables: &'static [&'static str],
    required_columns: &'static [(&'static str, &'static str)],
}

// These are the exact checksums written by the pre-fork personal Meetily
// releases. A repair is allowed only when the existing database already has
// the schema effect of that migration. The current embedded migration checksum
// is then written as metadata; migration SQL is never replayed here.
const LEGACY_MIGRATION_CHECKSUM_REPAIRS: &[LegacyMigrationChecksumRepair] = &[
    LegacyMigrationChecksumRepair {
        version: 20250916100000,
        legacy_checksum_hex: "298F47AFAA1FE9B3312BC1D9D23D0BC780BDD12F54206DF690B5AA7C1117BC3D1C40D99918D7266E5522BD86EF8D7793",
        required_tables: INITIAL_SCHEMA_TABLES,
        required_columns: &[],
    },
    LegacyMigrationChecksumRepair {
        version: 20250920155811,
        legacy_checksum_hex: "E006A52615896541ACCE6EA2BA1E918109976D78792B70A41ECC94F07ED255DBD79038A6022F1C7C5E9E8803D0B2D3D0",
        required_tables: &[],
        required_columns: &[("settings", "openRouterApiKey")],
    },
    LegacyMigrationChecksumRepair {
        version: 20251006000000,
        legacy_checksum_hex: "9AC46A88E49D20E3B665D79912134C14106460675DD8AA70ECB3EB25E934BD9FDA102A6C2A932078F44671C30C8640A4",
        required_tables: &[],
        required_columns: &[
            ("meetings", "folder_path"),
            ("transcripts", "audio_start_time"),
            ("transcripts", "audio_end_time"),
            ("transcripts", "duration"),
        ],
    },
    LegacyMigrationChecksumRepair {
        version: 20251010153942,
        legacy_checksum_hex: "EB12BAAE2C08A0487C28AE64EB18C1F115CB110C5BC6BA127322B1952E06367B1E26C45C0642C26EA153E7729EDB2CC2",
        required_tables: &[],
        required_columns: &[("settings", "ollamaEndpoint")],
    },
    LegacyMigrationChecksumRepair {
        version: 20251101000000,
        legacy_checksum_hex: "07D6ED23B60C0AF3DE7CB1891C9C3A79C0310C9D6E857E0441E1615F7574CB62F44329CC318AF386F3D23A9B7C8A3950",
        required_tables: &[],
        required_columns: &[
            ("summary_processes", "result_backup"),
            ("summary_processes", "result_backup_timestamp"),
        ],
    },
    LegacyMigrationChecksumRepair {
        version: 20251105120000,
        legacy_checksum_hex: "E91730CEB5DCA556DA33C49DAA79DB820B8D499F8A246E6397168A1B56B3B15A4001DC20DC74B7877206E8BC2213FA2F",
        required_tables: &["licensing"],
        required_columns: &[
            ("settings", "customOpenAIConfig"),
            ("licensing", "encrypted_key"),
            ("licensing", "signature_hash"),
            ("licensing", "soft_expiry_date"),
        ],
    },
    LegacyMigrationChecksumRepair {
        version: 20251110000000,
        legacy_checksum_hex: "F11C86BF0EF57C6DAEC61A7F1DE4F403C5FE6D9AAFDF1677229DB777B711935BE633279A37EFCB8E6555B1438FB0BEA8",
        required_tables: &[],
        required_columns: &[("licensing", "grace_period")],
    },
    LegacyMigrationChecksumRepair {
        version: 20251110000001,
        legacy_checksum_hex: "C5D436603D3FC94728C62EB1CBEB22245AD345DFABEFDC2D29E679204AF24C5C966C8B4B1CAEED4BDE1F2A706C24613E",
        required_tables: &[],
        required_columns: &[("transcripts", "speaker")],
    },
    LegacyMigrationChecksumRepair {
        version: 20251223000000,
        legacy_checksum_hex: "D9024F5483DA5ADCFE3CAE4FCCEA9C5BE755D9E46F25AA666B4D40A466DD3FC2039E1A2998D7C5AC63C4777EB9392377",
        required_tables: &["meeting_notes"],
        required_columns: &[
            ("meeting_notes", "meeting_id"),
            ("meeting_notes", "notes_markdown"),
            ("meeting_notes", "notes_json"),
        ],
    },
    LegacyMigrationChecksumRepair {
        version: 20251229000000,
        legacy_checksum_hex: "806E0CFCE84A9A7A8CBC7BD19C33A040963913B57A6F28932B5FF3FBDAA111AFF290FC5FD8D5B4491B61B8AA69B5A6A9",
        required_tables: &[],
        required_columns: &[("settings", "geminiApiKey")],
    },
];

#[derive(Clone)]
pub struct DatabaseManager {
    pool: SqlitePool,
}

/// The one place SQLite connection options are configured. Diagnostics that
/// need a throwaway database (the packaged `--smoke-dbstat` probe) open it
/// through here so they exercise the same connection contract the application
/// does, instead of drifting from it.
pub(crate) fn sqlite_connect_options(path: &str) -> Result<SqliteConnectOptions> {
    Ok(SqliteConnectOptions::from_str(path)?.foreign_keys(true))
}

/// Reconciles the known pre-fork personal migration checksums only after the
/// already-applied schema effects are verified. This updates SQLx metadata in
/// one transaction; it never replays migration SQL or changes user records.
async fn repair_known_legacy_migration_checksums(pool: &SqlitePool) -> Result<()> {
    let migrations_table_exists: Option<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await?;
    if migrations_table_exists.is_none() {
        return Ok(());
    }

    let migrator = sqlx::migrate!("./migrations");
    let mut pending = Vec::new();
    for repair in LEGACY_MIGRATION_CHECKSUM_REPAIRS {
        let applied_checksum: Option<String> = sqlx::query_scalar(
            "SELECT hex(checksum) FROM _sqlx_migrations WHERE version = ? AND success = 1",
        )
        .bind(repair.version)
        .fetch_optional(pool)
        .await?;
        if applied_checksum.as_deref() != Some(repair.legacy_checksum_hex) {
            continue;
        }

        for table in repair.required_tables {
            let exists: Option<String> = sqlx::query_scalar(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
            )
            .bind(table)
            .fetch_optional(pool)
            .await?;
            if exists.is_none() {
                return Err(sqlx::Error::Protocol(format!(
                    "refusing legacy migration checksum repair for {} because required table {table} is missing",
                    repair.version
                )));
            }
        }
        for (table, column) in repair.required_columns {
            let exists: Option<String> =
                sqlx::query_scalar("SELECT name FROM pragma_table_info(?) WHERE name = ?")
                    .bind(table)
                    .bind(column)
                    .fetch_optional(pool)
                    .await?;
            if exists.is_none() {
                return Err(sqlx::Error::Protocol(format!(
                    "refusing legacy migration checksum repair for {} because required column {table}.{column} is missing",
                    repair.version
                )));
            }
        }
        if migrator
            .iter()
            .all(|migration| migration.version != repair.version)
        {
            return Err(sqlx::Error::Protocol(format!(
                "legacy migration {} is missing from the embedded migrator",
                repair.version
            )));
        }
        pending.push(repair);
    }

    if pending.is_empty() {
        return Ok(());
    }

    let mut transaction = pool.begin().await?;
    for repair in pending {
        let expected = migrator
            .iter()
            .find(|migration| migration.version == repair.version)
            .expect("embedded migration was verified before the transaction");
        let repaired = sqlx::query(
            "UPDATE _sqlx_migrations SET checksum = ? WHERE version = ? AND success = 1 AND hex(checksum) = ?",
        )
        .bind(expected.checksum.as_ref())
        .bind(repair.version)
        .bind(repair.legacy_checksum_hex)
        .execute(&mut *transaction)
        .await?;
        if repaired.rows_affected() != 1 {
            return Err(sqlx::Error::Protocol(
                "legacy migration checksum changed during compatibility repair".into(),
            ));
        }
    }
    transaction.commit().await?;
    log::warn!("Repaired known pre-fork migration checksums after validating their schema effects");
    Ok(())
}

impl DatabaseManager {
    pub async fn new(tauri_db_path: &str, backend_db_path: &str) -> Result<Self> {
        if let Some(parent_dir) = Path::new(tauri_db_path).parent() {
            if !parent_dir.exists() {
                fs::create_dir_all(parent_dir).map_err(|e| sqlx::Error::Io(e))?;
            }
        }

        if !Path::new(tauri_db_path).exists() {
            if Path::new(backend_db_path).exists() {
                log::info!(
                    "Copying database from {} to {}",
                    backend_db_path,
                    tauri_db_path
                );
                fs::copy(backend_db_path, tauri_db_path).map_err(|e| sqlx::Error::Io(e))?;
            } else {
                log::info!("Creating database at {}", tauri_db_path);
                Sqlite::create_database(tauri_db_path).await?;
            }
        }

        let pool = SqlitePoolOptions::new()
            .connect_with(sqlite_connect_options(tauri_db_path)?)
            .await?;

        repair_known_legacy_migration_checksums(&pool).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;

        // Sync built-in templates to database
        if let Err(e) = sync_builtin_templates(&pool).await {
            log::warn!("Failed to sync built-in templates: {}", e);
        }

        Ok(DatabaseManager { pool })
    }

    // NOTE: So for the first time users they needs to start the application
    // after they can just delete the existing .sqlite file and then copy the existing .db file to
    // the current app dir, So the system detects legacy db and copy it and starts with that data
    // (Newly created .sqlite with the copied content from .db)
    pub async fn new_from_app_handle(app_handle: &tauri::AppHandle) -> Result<Self> {
        // Resolve the app's data directory
        let app_data_dir =
            crate::paths::app_data_dir(app_handle).expect("failed to get app data dir");
        if !app_data_dir.exists() {
            fs::create_dir_all(&app_data_dir).map_err(|e| sqlx::Error::Io(e))?;
        }

        // Define database paths
        let tauri_db_path = app_data_dir
            .join("meeting_minutes.sqlite")
            .to_string_lossy()
            .to_string();
        // Legacy backend DB path (for auto-migration if exists)
        let backend_db_path = app_data_dir
            .join("meeting_minutes.db")
            .to_string_lossy()
            .to_string();

        // WAL file paths for defensive cleanup
        let wal_path = app_data_dir.join("meeting_minutes.sqlite-wal");
        let shm_path = app_data_dir.join("meeting_minutes.sqlite-shm");

        log::info!("Tauri DB path: {}", tauri_db_path);
        log::info!("Legacy backend DB path: {}", backend_db_path);

        // Try to open database with defensive WAL handling
        match Self::new(&tauri_db_path, &backend_db_path).await {
            Ok(db_manager) => {
                log::info!("Database opened successfully");
                Ok(db_manager)
            }
            Err(e) => {
                // Check if error is due to corrupted WAL file
                let error_msg = e.to_string();
                if error_msg.contains("malformed") || error_msg.contains("corrupt") {
                    log::warn!("Database appears corrupted, likely due to orphaned WAL file. Attempting recovery...");
                    log::warn!("Error details: {}", error_msg);

                    // Delete potentially corrupted WAL/SHM files
                    if wal_path.exists() {
                        match fs::remove_file(&wal_path) {
                            Ok(_) => log::info!("Removed orphaned WAL file: {:?}", wal_path),
                            Err(e) => log::warn!("Failed to remove WAL file: {}", e),
                        }
                    }
                    if shm_path.exists() {
                        match fs::remove_file(&shm_path) {
                            Ok(_) => log::info!("Removed orphaned SHM file: {:?}", shm_path),
                            Err(e) => log::warn!("Failed to remove SHM file: {}", e),
                        }
                    }

                    // Retry connection without WAL files
                    log::info!("Retrying database connection after WAL cleanup...");
                    match Self::new(&tauri_db_path, &backend_db_path).await {
                        Ok(db_manager) => {
                            log::info!("Database opened successfully after WAL recovery");
                            Ok(db_manager)
                        }
                        Err(retry_err) => {
                            log::error!(
                                "Database connection failed even after WAL cleanup: {}",
                                retry_err
                            );
                            Err(retry_err)
                        }
                    }
                } else {
                    // Not a WAL-related error, propagate original error
                    log::error!("Database connection failed: {}", error_msg);
                    Err(e)
                }
            }
        }
    }

    /// Check if this is the first launch (sqlite database doesn't exist yet)
    pub async fn is_first_launch(app_handle: &tauri::AppHandle) -> Result<bool> {
        let app_data_dir =
            crate::paths::app_data_dir(app_handle).expect("failed to get app data dir");

        let tauri_db_path = app_data_dir.join("meeting_minutes.sqlite");

        Ok(!tauri_db_path.exists())
    }

    /// Import a legacy database from the specified path and initialize
    pub async fn import_legacy_database(
        app_handle: &tauri::AppHandle,
        legacy_db_path: &str,
    ) -> Result<Self> {
        let app_data_dir =
            crate::paths::app_data_dir(app_handle).expect("failed to get app data dir");

        if !app_data_dir.exists() {
            fs::create_dir_all(&app_data_dir).map_err(|e| sqlx::Error::Io(e))?;
        }

        // Copy legacy database to app data directory as meeting_minutes.db
        let target_legacy_path = app_data_dir.join("meeting_minutes.db");
        log::info!(
            "Copying legacy database from {} to {}",
            legacy_db_path,
            target_legacy_path.display()
        );

        fs::copy(legacy_db_path, &target_legacy_path).map_err(|e| sqlx::Error::Io(e))?;

        // Now use the standard initialization which will detect and migrate the legacy db
        Self::new_from_app_handle(app_handle).await
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn with_transaction<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Transaction<'_, Sqlite>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut tx = self.pool.begin().await?;
        let result = f(&mut tx).await;

        match result {
            Ok(val) => {
                tx.commit().await?;
                Ok(val)
            }
            Err(err) => {
                tx.rollback().await?;
                Err(err)
            }
        }
    }

    /// Cleanup database connection and checkpoint WAL
    /// This should be called on application shutdown to ensure:
    /// - All WAL changes are written to the main database file
    /// - The .wal and .shm files are deleted
    /// - Connection pool is gracefully closed
    pub async fn cleanup(&self) -> Result<()> {
        log::info!("Starting database cleanup...");

        // Force checkpoint of WAL to main database file and remove WAL file
        // TRUNCATE mode: checkpoints all pages AND deletes the WAL file
        match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
        {
            Ok(_) => log::info!("WAL checkpoint completed successfully"),
            Err(e) => log::warn!("WAL checkpoint failed (non-fatal): {}", e),
        }

        // Close the connection pool gracefully
        self.pool.close().await;
        log::info!("Database connection pool closed");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_repair(version: i64) -> &'static LegacyMigrationChecksumRepair {
        LEGACY_MIGRATION_CHECKSUM_REPAIRS
            .iter()
            .find(|repair| repair.version == version)
            .unwrap()
    }

    async fn set_legacy_checksum(
        pool: &SqlitePool,
        repair: &LegacyMigrationChecksumRepair,
        success: bool,
    ) {
        sqlx::query(&format!(
            "UPDATE _sqlx_migrations SET checksum = X'{}', success = {} WHERE version = {}",
            repair.legacy_checksum_hex,
            i64::from(success),
            repair.version
        ))
        .execute(pool)
        .await
        .unwrap();
    }

    async fn migration_checksum_hex(pool: &SqlitePool, version: i64) -> String {
        sqlx::query_scalar("SELECT hex(checksum) FROM _sqlx_migrations WHERE version = ?")
            .bind(version)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn repairs_only_the_known_complete_legacy_migration_set() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let migrator = sqlx::migrate!("./migrations");
        migrator.run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('legacy-meeting', 'Preserve me', '2025-01-01', '2025-01-01')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('legacy-transcript', 'legacy-meeting', 'important user transcript', '2025-01-01')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for repair in LEGACY_MIGRATION_CHECKSUM_REPAIRS {
            set_legacy_checksum(&pool, repair, true).await;
        }

        repair_known_legacy_migration_checksums(&pool)
            .await
            .unwrap();
        migrator.run(&pool).await.unwrap();

        for repair in LEGACY_MIGRATION_CHECKSUM_REPAIRS {
            assert_ne!(
                migration_checksum_hex(&pool, repair.version).await,
                repair.legacy_checksum_hex
            );
        }
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT title FROM meetings WHERE id = 'legacy-meeting'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "Preserve me"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT transcript FROM transcripts WHERE id = 'legacy-transcript'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "important user transcript"
        );
    }

    #[tokio::test]
    async fn refuses_to_repair_an_incomplete_legacy_schema() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let repair = legacy_repair(20250920155811);
        sqlx::query("DROP TABLE settings")
            .execute(&pool)
            .await
            .unwrap();
        set_legacy_checksum(&pool, repair, true).await;

        assert!(repair_known_legacy_migration_checksums(&pool)
            .await
            .is_err());
        assert_eq!(
            migration_checksum_hex(&pool, repair.version).await,
            repair.legacy_checksum_hex
        );
    }

    #[tokio::test]
    async fn leaves_nonlegacy_or_unsuccessful_migrations_untouched() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let repair = legacy_repair(20250920155811);
        let original = migration_checksum_hex(&pool, repair.version).await;

        repair_known_legacy_migration_checksums(&pool)
            .await
            .unwrap();
        assert_eq!(
            migration_checksum_hex(&pool, repair.version).await,
            original
        );

        set_legacy_checksum(&pool, repair, false).await;
        repair_known_legacy_migration_checksums(&pool)
            .await
            .unwrap();
        assert_eq!(
            migration_checksum_hex(&pool, repair.version).await,
            repair.legacy_checksum_hex
        );
    }
}
