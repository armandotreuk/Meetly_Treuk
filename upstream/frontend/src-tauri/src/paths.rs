//! Persistent-path policy shared by the native application.
//!
//! The normal package retains Tauri's platform-native app-data resolution.
//! The R13 validation package is intentionally different: its persistent
//! state must stay beneath the dedicated D: validation root, regardless of
//! platform resolver or product-name behaviour in individual plugins.

use std::path::PathBuf;
#[cfg(not(feature = "r13-validation"))]
use tauri::Manager;
use tauri::{AppHandle, Runtime};

#[cfg(feature = "r13-validation")]
pub const R13_VALIDATION_APP_DATA_ROOT: &str = r"D:\Meetly-R13-Validation\roaming-app-data";

/// Resolve the native application's persistent data directory.
pub fn app_data_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    #[cfg(feature = "r13-validation")]
    {
        let _ = app;
        Ok(PathBuf::from(R13_VALIDATION_APP_DATA_ROOT))
    }

    #[cfg(not(feature = "r13-validation"))]
    {
        app.path().app_data_dir()
    }
}

/// The R13 notification component has no app handle, so it uses this same
/// explicit root instead of its historical hard-coded `meetily` directory.
#[cfg(feature = "r13-validation")]
pub fn r13_validation_app_data_dir() -> PathBuf {
    PathBuf::from(R13_VALIDATION_APP_DATA_ROOT)
}

/// Fallback for components that predate app-handle injection. In an R13
/// package this must never resolve to the user's generic system data folder.
pub fn fallback_data_dir() -> Option<PathBuf> {
    #[cfg(feature = "r13-validation")]
    {
        Some(r13_validation_app_data_dir())
    }

    #[cfg(not(feature = "r13-validation"))]
    {
        dirs::data_dir()
            .or_else(dirs::home_dir)
            .map(|path| path.join("Meetily"))
    }
}
