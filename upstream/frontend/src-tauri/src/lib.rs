use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;
// Removed unused import

// Performance optimization: Conditional logging macros for hot paths
#[cfg(debug_assertions)]
macro_rules! perf_debug {
    ($($arg:tt)*) => {
        log::debug!($($arg)*)
    };
}

#[cfg(not(debug_assertions))]
macro_rules! perf_debug {
    ($($arg:tt)*) => {};
}

#[cfg(debug_assertions)]
macro_rules! perf_trace {
    ($($arg:tt)*) => {
        log::trace!($($arg)*)
    };
}

#[cfg(not(debug_assertions))]
macro_rules! perf_trace {
    ($($arg:tt)*) => {};
}

// Make these macros available to other modules
#[allow(unused_imports)]
pub(crate) use perf_debug;
#[allow(unused_imports)]
pub(crate) use perf_trace;

// Re-export async logging macros for external use (removed due to macro conflicts)

// Declare audio module
pub mod anthropic;
pub mod api;
pub mod audio;
pub mod config;
pub mod console_utils;
pub mod database;
pub mod export;
pub mod groq;
pub mod mcp;
pub mod model_bundle;
pub mod notifications;
pub mod ollama;
pub mod onboarding;
pub mod openai;
pub mod openrouter;
pub mod parakeet_engine;
pub mod retrieval;
pub mod security;
pub mod state;
pub mod summary;
pub mod tray;
pub mod utils;
pub mod whisper_engine;

use audio::{list_audio_devices, trigger_audio_permission, AudioDevice};
use database::repositories::retrieval::{
    DerivedDiskMeasurementStatus, DerivedDiskUsage, ModelSpec, RetrievalRepository, VectorEncoding,
};
use log::{error as log_error, info as log_info};
use notifications::commands::NotificationManagerState;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::RwLock;

static RECORDING_FLAG: AtomicBool = AtomicBool::new(false);

/// Exit codes are the only diagnostic channel that survives the packaged
/// binary: release builds are `windows_subsystem = "windows"`, so stdout and
/// stderr are unattached and the printed status never reaches a CI log. Each
/// outcome therefore gets its own code and the workflow maps it back to text.
const SMOKE_EXIT_EXACT: i32 = 0;
const SMOKE_EXIT_UNAVAILABLE: i32 = 2;
const SMOKE_EXIT_RUNTIME: i32 = 3;
const SMOKE_EXIT_CONNECTION: i32 = 4;
const SMOKE_EXIT_MIGRATION: i32 = 5;
const SMOKE_EXIT_SETUP: i32 = 6;
const SMOKE_EXIT_MEASUREMENT: i32 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DbstatSmokeFailureStage {
    Runtime,
    Connection,
    Migration,
    Setup,
    Measurement,
}

#[derive(Debug, PartialEq, Eq)]
enum DbstatSmokeStatus {
    /// `dbstat` is linked in and returned an exact allocated-page measurement.
    /// The byte total is reported, never asserted: it tracks the derived
    /// schema's page layout and changes with any migration, while the property
    /// under test is only that an exact measurement was possible at all.
    Exact { bytes: u64 },
    /// The linked SQLite lacks `ENABLE_DBSTAT_VTAB`, which is the one condition
    /// this smoke exists to detect.
    Unavailable,
    /// The probe could not run to a verdict; the bounded stage is carried
    /// without exposing the underlying error.
    Failed { stage: DbstatSmokeFailureStage },
}

fn dbstat_smoke_status(
    measurement: Result<DerivedDiskUsage, DbstatSmokeFailureStage>,
) -> DbstatSmokeStatus {
    match measurement {
        Ok(usage) => match (usage.status, usage.bytes) {
            (DerivedDiskMeasurementStatus::Exact, Some(bytes)) => {
                DbstatSmokeStatus::Exact { bytes }
            }
            (DerivedDiskMeasurementStatus::Exact, None) => DbstatSmokeStatus::Failed {
                stage: DbstatSmokeFailureStage::Measurement,
            },
            (DerivedDiskMeasurementStatus::Unavailable, _) => DbstatSmokeStatus::Unavailable,
        },
        Err(stage) => DbstatSmokeStatus::Failed { stage },
    }
}

fn dbstat_smoke_exit_code(status: &DbstatSmokeStatus) -> i32 {
    match status {
        DbstatSmokeStatus::Exact { .. } => SMOKE_EXIT_EXACT,
        DbstatSmokeStatus::Unavailable => SMOKE_EXIT_UNAVAILABLE,
        DbstatSmokeStatus::Failed { stage } => match stage {
            DbstatSmokeFailureStage::Runtime => SMOKE_EXIT_RUNTIME,
            DbstatSmokeFailureStage::Connection => SMOKE_EXIT_CONNECTION,
            DbstatSmokeFailureStage::Migration => SMOKE_EXIT_MIGRATION,
            DbstatSmokeFailureStage::Setup => SMOKE_EXIT_SETUP,
            DbstatSmokeFailureStage::Measurement => SMOKE_EXIT_MEASUREMENT,
        },
    }
}

async fn smoke_dbstat_measurement() -> Result<DerivedDiskUsage, DbstatSmokeFailureStage> {
    let options = database::manager::sqlite_connect_options("sqlite::memory:")
        .map_err(|_| DbstatSmokeFailureStage::Connection)?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|_| DbstatSmokeFailureStage::Connection)?;
    sqlx::query("PRAGMA page_size = 4096")
        .execute(&pool)
        .await
        .map_err(|_| DbstatSmokeFailureStage::Setup)?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|_| DbstatSmokeFailureStage::Migration)?;
    RetrievalRepository::register_model(
        &pool,
        &ModelSpec {
            model_id: "smoke-model".to_string(),
            dimensions: 1,
            vector_encoding: VectorEncoding::F32,
            chunker_version: 1,
            dequantization_scale: None,
            dequantization_zero_point: None,
        },
    )
    .await
    .map_err(|_| DbstatSmokeFailureStage::Setup)?;
    RetrievalRepository::register_generation(&pool, "smoke-generation", "smoke-model")
        .await
        .map_err(|_| DbstatSmokeFailureStage::Setup)?;
    let measurement = RetrievalRepository::derived_disk_usage(&pool)
        .await
        .map_err(|_| DbstatSmokeFailureStage::Measurement)?;
    pool.close().await;
    Ok(measurement)
}

pub fn run_dbstat_smoke() -> i32 {
    let measurement = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(smoke_dbstat_measurement()),
        Err(_) => Err(DbstatSmokeFailureStage::Runtime),
    };
    let status = dbstat_smoke_status(measurement);
    match &status {
        DbstatSmokeStatus::Exact { bytes } => {
            println!("smoke-dbstat: status=exact bytes={bytes}");
        }
        DbstatSmokeStatus::Unavailable => {
            eprintln!("smoke-dbstat: status=unavailable");
        }
        DbstatSmokeStatus::Failed { stage } => {
            eprintln!("smoke-dbstat: status=failed stage={stage:?}");
        }
    }
    dbstat_smoke_exit_code(&status)
}

// Global language preference storage (default to "auto-translate" for automatic translation to English)
static LANGUAGE_PREFERENCE: std::sync::LazyLock<StdMutex<String>> =
    std::sync::LazyLock::new(|| StdMutex::new("auto-translate".to_string()));

// Global custom vocabulary prompt storage (flattened from user's newline-separated words/phrases)
static VOCABULARY_PROMPT: std::sync::LazyLock<StdMutex<String>> =
    std::sync::LazyLock::new(|| StdMutex::new(String::new()));

#[derive(Debug, Deserialize)]
struct RecordingArgs {
    save_path: String,
}

#[derive(Debug, Serialize, Clone)]
struct TranscriptionStatus {
    chunks_in_queue: usize,
    is_processing: bool,
    last_activity_ms: u64,
}

#[tauri::command]
async fn start_recording<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
) -> Result<(), String> {
    log_info!("🔥 CALLED start_recording with meeting: {:?}", meeting_name);
    log_info!(
        "📋 Backend received parameters - mic: {:?}, system: {:?}, meeting: {:?}",
        mic_device_name,
        system_device_name,
        meeting_name
    );

    if is_recording().await {
        return Err("Recording already in progress".to_string());
    }

    // Call the actual audio recording system with meeting name
    match audio::recording_commands::start_recording_with_devices_and_meeting(
        app.clone(),
        mic_device_name,
        system_device_name,
        meeting_name.clone(),
    )
    .await
    {
        Ok(_) => {
            RECORDING_FLAG.store(true, Ordering::SeqCst);
            tray::update_tray_menu(&app);

            log_info!("Recording started successfully");

            // Show recording started notification through NotificationManager
            // This respects user's notification preferences
            let notification_manager_state = app.state::<NotificationManagerState<R>>();
            if let Err(e) = notifications::commands::show_recording_started_notification(
                &app,
                &notification_manager_state,
                meeting_name.clone(),
            )
            .await
            {
                log_error!("Failed to show recording started notification: {}", e);
            } else {
                log_info!("Successfully showed recording started notification");
            }

            Ok(())
        }
        Err(e) => {
            log_error!("Failed to start audio recording: {}", e);
            Err(format!("Failed to start recording: {}", e))
        }
    }
}

#[tauri::command]
async fn stop_recording<R: Runtime>(app: AppHandle<R>, args: RecordingArgs) -> Result<(), String> {
    log_info!("Attempting to stop recording...");

    // Check the actual audio recording system state instead of the flag
    if !audio::recording_commands::is_recording().await {
        log_info!("Recording is already stopped");
        return Ok(());
    }

    // Call the actual audio recording system to stop
    match audio::recording_commands::stop_recording(
        app.clone(),
        audio::recording_commands::RecordingArgs {
            save_path: args.save_path.clone(),
        },
    )
    .await
    {
        Ok(_) => {
            RECORDING_FLAG.store(false, Ordering::SeqCst);
            tray::update_tray_menu(&app);

            // Create the save directory if it doesn't exist
            if let Some(parent) = std::path::Path::new(&args.save_path).parent() {
                if !parent.exists() {
                    log_info!("Creating directory: {:?}", parent);
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        let err_msg = format!("Failed to create save directory: {}", e);
                        log_error!("{}", err_msg);
                        return Err(err_msg);
                    }
                }
            }

            // Show recording stopped notification through NotificationManager
            // This respects user's notification preferences
            let notification_manager_state = app.state::<NotificationManagerState<R>>();
            if let Err(e) = notifications::commands::show_recording_stopped_notification(
                &app,
                &notification_manager_state,
            )
            .await
            {
                log_error!("Failed to show recording stopped notification: {}", e);
            } else {
                log_info!("Successfully showed recording stopped notification");
            }

            Ok(())
        }
        Err(e) => {
            log_error!("Failed to stop audio recording: {}", e);
            // Still update the flag even if stopping failed
            RECORDING_FLAG.store(false, Ordering::SeqCst);
            tray::update_tray_menu(&app);
            Err(format!("Failed to stop recording: {}", e))
        }
    }
}

#[tauri::command]
async fn is_recording() -> bool {
    audio::recording_commands::is_recording().await
}

#[tauri::command]
fn get_transcription_status() -> TranscriptionStatus {
    TranscriptionStatus {
        chunks_in_queue: 0,
        is_processing: false,
        last_activity_ms: 0,
    }
}

#[tauri::command]
fn read_audio_file(file_path: String) -> Result<Vec<u8>, String> {
    match std::fs::read(&file_path) {
        Ok(data) => Ok(data),
        Err(e) => Err(format!("Failed to read audio file: {}", e)),
    }
}

#[tauri::command]
async fn save_transcript(file_path: String, content: String) -> Result<(), String> {
    log_info!("Saving transcript to: {}", file_path);

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&file_path).parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }
    }

    // Write content to file
    std::fs::write(&file_path, content)
        .map_err(|e| format!("Failed to write transcript: {}", e))?;

    log_info!("Transcript saved successfully");
    Ok(())
}

// Audio level monitoring commands
#[tauri::command]
async fn start_audio_level_monitoring<R: Runtime>(
    app: AppHandle<R>,
    device_names: Vec<String>,
) -> Result<(), String> {
    log_info!(
        "Starting audio level monitoring for devices: {:?}",
        device_names
    );

    audio::simple_level_monitor::start_monitoring(app, device_names)
        .await
        .map_err(|e| format!("Failed to start audio level monitoring: {}", e))
}

#[tauri::command]
async fn stop_audio_level_monitoring() -> Result<(), String> {
    log_info!("Stopping audio level monitoring");

    audio::simple_level_monitor::stop_monitoring()
        .await
        .map_err(|e| format!("Failed to stop audio level monitoring: {}", e))
}

#[tauri::command]
async fn is_audio_level_monitoring() -> bool {
    audio::simple_level_monitor::is_monitoring()
}

// Analytics commands stripped (decision 3: no telemetry)

// Whisper commands are now handled by whisper_engine::commands module

#[tauri::command]
async fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    list_audio_devices()
        .await
        .map_err(|e| format!("Failed to list audio devices: {}", e))
}

#[tauri::command]
async fn trigger_microphone_permission() -> Result<bool, String> {
    trigger_audio_permission()
        .map_err(|e| format!("Failed to trigger microphone permission: {}", e))
}

#[tauri::command]
async fn start_recording_with_devices<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
) -> Result<(), String> {
    start_recording_with_devices_and_meeting(app, mic_device_name, system_device_name, None).await
}

#[tauri::command]
async fn start_recording_with_devices_and_meeting<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
) -> Result<(), String> {
    log_info!("🚀 CALLED start_recording_with_devices_and_meeting - Mic: {:?}, System: {:?}, Meeting: {:?}",
             mic_device_name, system_device_name, meeting_name);

    // Clone meeting_name for notification use later
    let meeting_name_for_notification = meeting_name.clone();

    // Call the recording module functions that support meeting names
    let recording_result = match (mic_device_name.clone(), system_device_name.clone()) {
        (None, None) => {
            log_info!(
                "No devices specified, starting with defaults and meeting: {:?}",
                meeting_name
            );
            audio::recording_commands::start_recording_with_meeting_name(app.clone(), meeting_name)
                .await
        }
        _ => {
            log_info!(
                "Starting with specified devices: mic={:?}, system={:?}, meeting={:?}",
                mic_device_name,
                system_device_name,
                meeting_name
            );
            audio::recording_commands::start_recording_with_devices_and_meeting(
                app.clone(),
                mic_device_name,
                system_device_name,
                meeting_name,
            )
            .await
        }
    };

    match recording_result {
        Ok(_) => {
            log_info!("Recording started successfully via tauri command");

            // Show recording started notification through NotificationManager
            // This respects user's notification preferences
            let notification_manager_state = app.state::<NotificationManagerState<R>>();
            if let Err(e) = notifications::commands::show_recording_started_notification(
                &app,
                &notification_manager_state,
                meeting_name_for_notification.clone(),
            )
            .await
            {
                log_error!("Failed to show recording started notification: {}", e);
            }

            Ok(())
        }
        Err(e) => {
            log_error!("Failed to start recording via tauri command: {}", e);
            Err(e)
        }
    }
}

#[tauri::command]
async fn set_language_preference(language: String) -> Result<(), String> {
    let mut lang_pref = LANGUAGE_PREFERENCE
        .lock()
        .map_err(|e| format!("Failed to set language preference: {}", e))?;
    log_info!("Setting language preference to: {}", language);
    *lang_pref = language;
    Ok(())
}

// Internal helper function to get language preference (for use within Rust code)
pub fn get_language_preference_internal() -> Option<String> {
    LANGUAGE_PREFERENCE.lock().ok().map(|lang| lang.clone())
}

// Internal helper to read the flattened vocabulary prompt for whisper initial_prompt and summary glossary
pub fn get_vocabulary_prompt_internal() -> String {
    VOCABULARY_PROMPT
        .lock()
        .ok()
        .map(|p| p.clone())
        .unwrap_or_default()
}

// ponytail: flattens the user's newline-separated word list into a single whisper initial_prompt sentence.
// Ceiling: whisper's initial_prompt context window is ~224 tokens, so very long lists silently fall off the end.
// Upgrade path: per-template vocabularies or a term-prioritization strategy if recognition of rare words suffers.
pub fn build_whisper_prompt_from_vocabulary(raw: &str) -> String {
    const MAX_PROMPT_LEN: usize = 200;
    let mut words: Vec<&str> = raw
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if words.is_empty() {
        return String::new();
    }
    words.dedup();
    let mut prompt = String::from("The following conversation may include these specific terms: ");
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            prompt.push_str(", ");
        }
        if prompt.len() + w.len() > MAX_PROMPT_LEN {
            break;
        }
        prompt.push_str(w);
    }
    prompt.push('.');
    prompt
}

#[tauri::command]
async fn api_get_custom_vocabulary<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    let storage = app
        .try_state::<state::AppState>()
        .ok_or_else(|| "App state not available".to_string())?;
    crate::database::repositories::setting::SettingsRepository::get_custom_vocabulary(
        storage.db_manager.pool(),
    )
    .await
    .map(|v| v.unwrap_or_default())
    .map_err(|e| format!("DB error: {}", e))
}

#[tauri::command]
async fn api_save_custom_vocabulary<R: Runtime>(
    app: AppHandle<R>,
    vocabulary: String,
) -> Result<(), String> {
    let storage = app
        .try_state::<state::AppState>()
        .ok_or_else(|| "App state not available".to_string())?;
    crate::database::repositories::setting::SettingsRepository::save_custom_vocabulary(
        storage.db_manager.pool(),
        &vocabulary,
    )
    .await
    .map_err(|e| format!("DB error: {}", e))?;
    let prompt = build_whisper_prompt_from_vocabulary(&vocabulary);
    if let Ok(mut p) = VOCABULARY_PROMPT.lock() {
        *p = prompt;
    }
    Ok(())
}

pub fn run() {
    log::set_max_level(log::LevelFilter::Info);

    let mut builder = tauri::Builder::default();

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            log_info!(
                "Second app instance requested with args: {:?}, cwd: {:?}",
                args,
                cwd
            );

            tray::focus_main_window(app);
        }));
    }

    builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(whisper_engine::parallel_commands::ParallelProcessorState::new())
        .manage(Arc::new(RwLock::new(
            None::<notifications::manager::NotificationManager<tauri::Wry>>,
        )) as NotificationManagerState<tauri::Wry>)
        .manage(audio::init_system_audio_state())
        .manage(summary::summary_engine::ModelManagerState(Arc::new(tokio::sync::Mutex::new(None))))
        .manage(api::chat::ChatStreamState::new())
        .setup(|_app| {
            log::info!("Application setup complete");

            // ponytail: dev-only DevTools via MEETILY_DEVTOOLS=1. Ceiling: release
            // builds without the env var skip this; upgrade path is a settings toggle.
            if std::env::var("MEETILY_DEVTOOLS").map(|v| v == "1").unwrap_or(false) {
                if let Some(window) = _app.get_webview_window("main") {
                    let _ = window.open_devtools();
                }
            }

            // Initialize system tray
            if let Err(e) = tray::create_tray(_app.handle()) {
                log::error!("Failed to create system tray: {}", e);
            }

            // Initialize notification system with proper defaults
            log::info!("Initializing notification system...");
            let app_for_notif = _app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let notif_state = app_for_notif.state::<NotificationManagerState<tauri::Wry>>();
                match notifications::commands::initialize_notification_manager(app_for_notif.clone()).await {
                    Ok(manager) => {
                        // Set default consent and permissions on first launch
                        if let Err(e) = manager.set_consent(true).await {
                            log::error!("Failed to set initial consent: {}", e);
                        }
                        if let Err(e) = manager.request_permission().await {
                            log::error!("Failed to request initial permission: {}", e);
                        }

                        // Store the initialized manager
                        let mut state_lock = notif_state.write().await;
                        *state_lock = Some(manager);
                        log::info!("Notification system initialized with default permissions");
                    }
                    Err(e) => {
                        log::error!("Failed to initialize notification manager: {}", e);
                    }
                }
            });

            // Set models directory to use app_data_dir (unified storage location)
            whisper_engine::commands::set_models_directory(&_app.handle());

            // Initialize Whisper engine on startup
            tauri::async_runtime::spawn(async {
                if let Err(e) = whisper_engine::commands::whisper_init().await {
                    log::error!("Failed to initialize Whisper engine on startup: {}", e);
                }
            });

            // Set Parakeet models directory
            parakeet_engine::commands::set_models_directory(&_app.handle());

            // Initialize Parakeet engine on startup
            tauri::async_runtime::spawn(async {
                if let Err(e) = parakeet_engine::commands::parakeet_init().await {
                    log::error!("Failed to initialize Parakeet engine on startup: {}", e);
                }
            });

            // Initialize ModelManager for summary engine (async, non-blocking)
            let app_handle_for_model_manager = _app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match summary::summary_engine::commands::init_model_manager_at_startup(&app_handle_for_model_manager).await {
                    Ok(_) => log::info!("ModelManager initialized successfully at startup"),
                    Err(e) => {
                        log::warn!("Failed to initialize ModelManager at startup: {}", e);
                        log::warn!("ModelManager will be lazy-initialized on first use");
                    }
                }
            });

            // Trigger system audio permission request on startup (similar to microphone permission)
            // #[cfg(target_os = "macos")]
            // {
            //     tauri::async_runtime::spawn(async {
            //         if let Err(e) = audio::permissions::trigger_system_audio_permission() {
            //             log::warn!("Failed to trigger system audio permission: {}", e);
            //         }
            //     });
            // }

            // Create the one detached retrieval lifecycle before any database
            // exists; it idempotently attaches after each database
            // installation path below, is shared with MCP by clone, and must
            // be shut down before the database pool closes.
            let retrieval_bundle_root = _app
                .handle()
                .path()
                .resource_dir()
                .ok()
                .map(|dir| retrieval::model::bundle_dir(&dir));
            _app.manage(retrieval::worker::RetrievalLifecycle::new(
                retrieval::worker::LifecycleConfig::production(retrieval_bundle_root),
            ));

            // Initialize database (handles first launch detection and conditional setup)
            tauri::async_runtime::block_on(async {
                database::setup::initialize_database_on_startup(&_app.handle()).await
            })
            .expect("Failed to initialize database");

            // Start MCP HTTP JSON-RPC server for external agents (normal startup path)
            crate::mcp::server::spawn_from_app(&_app.handle());

            // F10: Initialize security module (OS keyring master key)
            log::info!("Initializing security module (encrypted key storage)...");
            if let Err(e) = security::init() {
                log::warn!("Failed to initialize security module: {} — API keys will be stored in plaintext", e);
            }

            // Initialize bundled templates directory for dynamic template discovery
            log::info!("Initializing bundled templates directory...");
            if let Ok(resource_path) = _app.handle().path().resource_dir() {
                let templates_dir = resource_path.join("templates");
                log::info!("Setting bundled templates directory to: {:?}", templates_dir);
                summary::templates::set_bundled_templates_dir(templates_dir);
            } else {
                log::warn!("Failed to resolve resource directory for templates");
            }

            // Load saved custom vocabulary from DB into the in-memory global so Whisper + summary pick it up
            let app_for_vocab = _app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Some(state) = app_for_vocab.try_state::<state::AppState>() {
                    match crate::database::repositories::setting::SettingsRepository::get_custom_vocabulary(
                        state.db_manager.pool(),
                    )
                    .await
                    {
                        Ok(Some(raw)) if !raw.is_empty() => {
                            let prompt = build_whisper_prompt_from_vocabulary(&raw);
                            if let Ok(mut p) = VOCABULARY_PROMPT.lock() {
                                *p = prompt;
                            }
                            log::info!("Loaded custom vocabulary from DB into memory");
                        }
                        _ => {}
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    if let Err(e) = window.hide() {
                        log::error!("Failed to hide main window on close request: {}", e);
                    } else {
                        log::info!("Main window hidden to tray on close request");
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            is_recording,
            get_transcription_status,
            read_audio_file,
            save_transcript,
            whisper_engine::commands::whisper_init,
            whisper_engine::commands::whisper_get_available_models,
            whisper_engine::commands::whisper_load_model,
            whisper_engine::commands::whisper_get_current_model,
            whisper_engine::commands::whisper_is_model_loaded,
            whisper_engine::commands::whisper_has_available_models,
            whisper_engine::commands::whisper_validate_model_ready,
            whisper_engine::commands::whisper_transcribe_audio,
            whisper_engine::commands::whisper_get_models_directory,
            whisper_engine::commands::whisper_download_model,
            whisper_engine::commands::whisper_cancel_download,
            whisper_engine::commands::whisper_delete_corrupted_model,
            // Parakeet engine commands
            parakeet_engine::commands::parakeet_init,
            parakeet_engine::commands::parakeet_get_available_models,
            parakeet_engine::commands::parakeet_load_model,
            parakeet_engine::commands::parakeet_get_current_model,
            parakeet_engine::commands::parakeet_is_model_loaded,
            parakeet_engine::commands::parakeet_has_available_models,
            parakeet_engine::commands::parakeet_validate_model_ready,
            parakeet_engine::commands::parakeet_transcribe_audio,
            parakeet_engine::commands::parakeet_get_models_directory,
            parakeet_engine::commands::parakeet_download_model,
            parakeet_engine::commands::parakeet_retry_download,
            parakeet_engine::commands::parakeet_cancel_download,
            parakeet_engine::commands::parakeet_delete_corrupted_model,
            parakeet_engine::commands::open_parakeet_models_folder,
            // Parallel processing commands
            whisper_engine::parallel_commands::initialize_parallel_processor,
            whisper_engine::parallel_commands::start_parallel_processing,
            whisper_engine::parallel_commands::pause_parallel_processing,
            whisper_engine::parallel_commands::resume_parallel_processing,
            whisper_engine::parallel_commands::stop_parallel_processing,
            whisper_engine::parallel_commands::get_parallel_processing_status,
            whisper_engine::parallel_commands::get_system_resources,
            whisper_engine::parallel_commands::check_resource_constraints,
            whisper_engine::parallel_commands::calculate_optimal_workers,
            whisper_engine::parallel_commands::prepare_audio_chunks,
            whisper_engine::parallel_commands::test_parallel_processing_setup,
            get_audio_devices,
            trigger_microphone_permission,
            start_recording_with_devices,
            start_recording_with_devices_and_meeting,
            start_audio_level_monitoring,
            stop_audio_level_monitoring,
            is_audio_level_monitoring,
            // Recording pause/resume commands
            audio::recording_commands::pause_recording,
            audio::recording_commands::resume_recording,
            audio::recording_commands::is_recording_paused,
            audio::recording_commands::get_recording_state,
            audio::recording_commands::get_meeting_folder_path,
            audio::recording_commands::save_recording_notes,
            // Reload sync commands (retrieve transcript history and meeting name)
            audio::recording_commands::get_transcript_history,
            audio::recording_commands::get_recording_meeting_name,
            // Device monitoring commands (AirPods/Bluetooth disconnect/reconnect)
            audio::recording_commands::poll_audio_device_events,
            audio::recording_commands::get_reconnection_status,
            audio::recording_commands::attempt_device_reconnect,
            // Playback device detection (Bluetooth warning)
            audio::recording_commands::get_active_audio_output,
            // Audio recovery commands (for transcript recovery feature)
            audio::incremental_saver::recover_audio_from_checkpoints,
            audio::incremental_saver::cleanup_checkpoints,
            audio::incremental_saver::has_audio_checkpoints,
            console_utils::show_console,
            console_utils::hide_console,
            console_utils::toggle_console,
            ollama::get_ollama_models,
            ollama::pull_ollama_model,
            ollama::delete_ollama_model,
            ollama::get_ollama_model_context,
            openai::openai::get_openai_models,
            anthropic::anthropic::get_anthropic_models,
            groq::groq::get_groq_models,
            api::api_get_meetings,
            api::folders::api_get_folders,
            api::folders::api_create_folder,
            api::folders::api_rename_folder,
            api::folders::api_move_folder,
            api::folders::api_delete_folder,
            api::folders::api_set_meeting_folder,
            api::api_search_transcripts,
            api::api_search_fts,
            api::api_rebuild_fts_index,
            api::api_build_context,
            api::api_chat_with_meetings,
            api::api_chat_with_scoped_conversation,
            api::api_chat_with_meetings_stream,
            api::api_chat_with_scoped_conversation_stream,
            api::api_cancel_chat_stream,
            api::api_chat_create_conversation,
            api::api_chat_get_conversation,
            api::api_chat_get_or_create_scoped_conversation,
            api::api_chat_get_messages,
            api::api_chat_save_message,
            api::api_chat_clear_conversation,
            api::api_chat_promote_live_recording,
            api::api_chat_discard_live_recording,
            api::api_get_profile,
            api::api_save_profile,
            api::api_update_profile,
            api::api_get_model_config,
            api::api_save_model_config,
            api::api_get_chat_model_config,
            api::api_save_chat_model_config,
            api::api_get_api_key,
            // api::api_get_auto_generate_setting,
            // api::api_save_auto_generate_setting,
            api::api_get_transcript_config,
            api::api_save_transcript_config,
            api::api_get_transcript_api_key,
            api_get_custom_vocabulary,
            api_save_custom_vocabulary,
            api::api_delete_meeting,
            api::api_get_meeting,
            api::api_get_meeting_metadata,
            api::api_get_meeting_transcripts,
            api::api_save_meeting_title,
            api::api_save_transcript,
            api::open_meeting_folder,
            api::test_backend_connection,
            api::debug_backend_connection,
            api::open_external_url,
            // Custom OpenAI commands
            api::api_save_custom_openai_config,
            api::api_get_custom_openai_config,
            api::api_test_custom_openai_connection,
            // Summary commands
            summary::commands::api_process_transcript,
            summary::commands::api_get_summary,
            summary::commands::api_save_meeting_summary,
            summary::commands::api_get_meeting_summary_language,
            summary::commands::api_save_meeting_summary_language,
            summary::commands::api_get_meeting_detected_summary_language,
            summary::commands::api_save_meeting_detected_summary_language,
            summary::commands::api_detect_transcript_summary_language,
            summary::commands::api_cancel_summary,
            summary::commands::api_list_meeting_summaries,
            summary::commands::api_delete_meeting_summary,
            // Template commands
            summary::template_commands::api_list_templates,
            summary::template_commands::api_get_template_details,
            summary::template_commands::api_validate_template,
            // Built-in AI commands
            summary::summary_engine::commands::builtin_ai_list_models,
            summary::summary_engine::commands::builtin_ai_get_model_info,
            summary::summary_engine::commands::builtin_ai_download_model,
            summary::summary_engine::commands::builtin_ai_cancel_download,
            summary::summary_engine::commands::builtin_ai_delete_model,
            summary::summary_engine::commands::builtin_ai_is_model_ready,
            summary::summary_engine::commands::builtin_ai_get_available_summary_model,
            summary::summary_engine::commands::builtin_ai_get_recommended_model,
            openrouter::get_openrouter_models,
            audio::recording_preferences::get_recording_preferences,
            audio::recording_preferences::set_recording_preferences,
            audio::recording_preferences::get_default_recordings_folder_path,
            audio::recording_preferences::open_recordings_folder,
            audio::recording_preferences::select_recording_folder,
            audio::recording_preferences::get_available_audio_backends,
            audio::recording_preferences::get_current_audio_backend,
            audio::recording_preferences::set_audio_backend,
            audio::recording_preferences::get_audio_backend_info,
            // Language preference commands
            set_language_preference,
            // Notification system commands
            notifications::commands::get_notification_settings,
            notifications::commands::set_notification_settings,
            notifications::commands::request_notification_permission,
            notifications::commands::show_notification,
            notifications::commands::show_test_notification,
            notifications::commands::is_dnd_active,
            notifications::commands::get_system_dnd_status,
            notifications::commands::set_manual_dnd,
            notifications::commands::set_notification_consent,
            notifications::commands::clear_notifications,
            notifications::commands::is_notification_system_ready,
            notifications::commands::initialize_notification_manager_manual,
            notifications::commands::test_notification_with_auto_consent,
            notifications::commands::get_notification_stats,
            // System audio capture commands
            audio::system_audio_commands::start_system_audio_capture_command,
            audio::system_audio_commands::list_system_audio_devices_command,
            audio::system_audio_commands::check_system_audio_permissions_command,
            audio::system_audio_commands::start_system_audio_monitoring,
            audio::system_audio_commands::stop_system_audio_monitoring,
            audio::system_audio_commands::get_system_audio_monitoring_status,
            // Screen Recording permission commands
            audio::permissions::check_screen_recording_permission_command,
            audio::permissions::request_screen_recording_permission_command,
            audio::permissions::trigger_system_audio_permission_command,
            // Database import commands
            database::commands::check_first_launch,
            database::commands::select_legacy_database_path,
            database::commands::detect_legacy_database,
            database::commands::check_default_legacy_database,
            database::commands::check_homebrew_database,
            database::commands::import_and_initialize_database,
            database::commands::initialize_fresh_database,
            // Database and Models path commands
            database::commands::get_database_directory,
            database::commands::open_database_folder,
            // F11: Meeting notes commands
            database::commands::get_meeting_notes,
            database::commands::save_meeting_notes,
            database::commands::delete_meeting_notes,
            // F1: Template commands
            database::commands::list_templates,
            database::commands::list_user_templates,
            database::commands::list_builtin_templates,
            database::commands::get_template,
            database::commands::create_template,
            database::commands::update_template,
            database::commands::delete_template,
            // F2: PDF export
            export::commands::export_meeting_pdf,
            export::commands::save_meeting_pdf,
            whisper_engine::commands::open_models_folder,
            // Onboarding commands
            onboarding::get_onboarding_status,
            onboarding::save_onboarding_status_cmd,
            onboarding::reset_onboarding_status_cmd,
            onboarding::complete_onboarding,
            // System settings commands
            #[cfg(target_os = "macos")]
            utils::open_system_settings,
            // Retranscription commands
            audio::retranscription::start_retranscription_command,
            audio::retranscription::cancel_retranscription_command,
            audio::retranscription::is_retranscription_in_progress_command,
            // Import audio commands
            audio::import::select_and_validate_audio_command,
            audio::import::validate_audio_file_command,
            audio::import::start_import_audio_command,
            audio::import::cancel_import_command,
            audio::import::is_import_in_progress_command,
            // Retrieval index status/rebuild/pause contract (Task 2.5)
            retrieval::commands::retrieval_index_status,
            retrieval::commands::retrieval_rebuild_index,
            retrieval::commands::retrieval_cancel_rebuild,
            retrieval::commands::retrieval_set_index_paused,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            match event {
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => {
                    tray::focus_main_window(_app_handle);
                }
                tauri::RunEvent::Exit => {
                    log::info!("Application exiting, cleaning up resources...");
                    tauri::async_runtime::block_on(async {
                        // Cancel and join retrieval model work BEFORE the
                        // database pool closes so nothing publishes after
                        // teardown.
                        if let Some(lifecycle) =
                            _app_handle.try_state::<retrieval::worker::RetrievalLifecycle>()
                        {
                            lifecycle.shutdown().await;
                        }

                        // Clean up database connection and checkpoint WAL
                        if let Some(app_state) = _app_handle.try_state::<state::AppState>() {
                            log::info!("Starting database cleanup...");
                            if let Err(e) = app_state.db_manager.cleanup().await {
                                log::error!("Failed to cleanup database: {}", e);
                            } else {
                                log::info!("Database cleanup completed successfully");
                            }
                        } else {
                            log::warn!("AppState not available for database cleanup (likely first launch)");
                        }

                        // Clean up sidecar
                        log::info!("Cleaning up sidecar...");
                        if let Err(e) = summary::summary_engine::force_shutdown_sidecar().await {
                            log::error!("Failed to force shutdown sidecar: {}", e);
                        }
                    });
                    log::info!("Application cleanup complete");
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{
        build_whisper_prompt_from_vocabulary, dbstat_smoke_exit_code, dbstat_smoke_status,
        smoke_dbstat_measurement, DbstatSmokeFailureStage, DerivedDiskMeasurementStatus,
        DerivedDiskUsage, SMOKE_EXIT_CONNECTION, SMOKE_EXIT_EXACT, SMOKE_EXIT_MEASUREMENT,
        SMOKE_EXIT_MIGRATION, SMOKE_EXIT_RUNTIME, SMOKE_EXIT_SETUP, SMOKE_EXIT_UNAVAILABLE,
    };

    #[test]
    fn flattens_newline_separated_words() {
        let raw = "Kubernetes\nARIA\nTLDR";
        let prompt = build_whisper_prompt_from_vocabulary(raw);
        assert!(prompt.contains("Kubernetes"));
        assert!(prompt.contains("ARIA"));
        assert!(prompt.contains("TLDR"));
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(build_whisper_prompt_from_vocabulary(""), "");
        assert_eq!(build_whisper_prompt_from_vocabulary("  \n  \n"), "");
    }

    #[test]
    fn respects_length_cap() {
        let long = (0..50)
            .map(|i| format!("SupercalifragilisticWord{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = build_whisper_prompt_from_vocabulary(&long);
        assert!(prompt.len() <= 220); // cap + small slack for the trailing period
    }

    /// Any exact measurement passes: the byte total tracks the derived
    /// schema's page layout, so pinning it would turn the next migration into
    /// a release-blocking "packaged dbstat smoke failed".
    #[test]
    fn dbstat_smoke_accepts_any_exact_measurement() {
        for bytes in [4_096, 65_536, 1_048_576] {
            assert_eq!(
                dbstat_smoke_exit_code(&dbstat_smoke_status(Ok(DerivedDiskUsage::exact(bytes)))),
                SMOKE_EXIT_EXACT
            );
        }
    }

    #[test]
    fn dbstat_smoke_separates_unavailable_from_pre_verdict_failures() {
        assert_eq!(
            dbstat_smoke_exit_code(&dbstat_smoke_status(Ok(DerivedDiskUsage::unavailable(1)))),
            SMOKE_EXIT_UNAVAILABLE
        );
        assert_ne!(SMOKE_EXIT_EXACT, SMOKE_EXIT_UNAVAILABLE);
    }

    #[test]
    fn dbstat_smoke_maps_every_pre_verdict_stage_to_a_distinct_code() {
        let mappings = [
            (DbstatSmokeFailureStage::Runtime, SMOKE_EXIT_RUNTIME),
            (DbstatSmokeFailureStage::Connection, SMOKE_EXIT_CONNECTION),
            (DbstatSmokeFailureStage::Migration, SMOKE_EXIT_MIGRATION),
            (DbstatSmokeFailureStage::Setup, SMOKE_EXIT_SETUP),
            (DbstatSmokeFailureStage::Measurement, SMOKE_EXIT_MEASUREMENT),
        ];
        let mut seen = Vec::new();
        for (stage, expected_code) in mappings {
            let code = dbstat_smoke_exit_code(&dbstat_smoke_status(Err(stage)));
            assert_eq!(code, expected_code);
            assert_ne!(code, SMOKE_EXIT_EXACT);
            assert_ne!(code, SMOKE_EXIT_UNAVAILABLE);
            assert!(!seen.contains(&code));
            seen.push(code);
        }
    }

    #[tokio::test]
    async fn dbstat_smoke_uses_the_migrated_database_path() {
        let measurement = smoke_dbstat_measurement().await.unwrap();
        assert_eq!(measurement.status, DerivedDiskMeasurementStatus::Exact);
        assert!(measurement.bytes.is_some_and(|bytes| bytes > 0));
    }
}
