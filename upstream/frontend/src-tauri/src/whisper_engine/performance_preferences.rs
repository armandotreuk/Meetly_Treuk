use super::acceleration::WhisperCompiledBackend;
use crate::audio::{GpuType, HardwareProfile};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const AUTO_THREAD_LIMIT: usize = 0;
const MAX_THREAD_LIMIT: usize = 32;

static CONFIGURED_THREAD_LIMIT: AtomicUsize = AtomicUsize::new(AUTO_THREAD_LIMIT);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionPerformancePreferences {
    /// None lets Meetily select a conservative thread count for the current machine.
    pub cpu_thread_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionHardwareStatus {
    pub compiled_backend: String,
    pub detected_gpu: String,
    pub gpu_runtime_status: String,
    pub cpu_logical_cores: usize,
    pub recommended_cpu_threads: usize,
    pub configured_cpu_threads: Option<usize>,
    pub effective_cpu_threads: usize,
    pub message: String,
}

fn max_thread_limit() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(1, MAX_THREAD_LIMIT)
}

fn stored_thread_limit() -> Option<usize> {
    match CONFIGURED_THREAD_LIMIT.load(Ordering::Relaxed) {
        AUTO_THREAD_LIMIT => None,
        limit => Some(limit),
    }
}

fn set_stored_thread_limit(limit: Option<usize>) {
    CONFIGURED_THREAD_LIMIT.store(limit.unwrap_or(AUTO_THREAD_LIMIT), Ordering::Relaxed);
}

pub(crate) fn resolve_thread_count_with_limit(
    configured: Option<usize>,
    adaptive: Option<usize>,
    max_available: usize,
) -> usize {
    let max_available = max_available.clamp(1, MAX_THREAD_LIMIT);
    configured.or(adaptive).unwrap_or(4).clamp(1, max_available)
}

/// The current preference is read at each decode so a saved setting takes effect
/// for the next transcription without rebuilding or restarting the application.
pub(crate) fn effective_thread_count(adaptive: Option<usize>) -> usize {
    resolve_thread_count_with_limit(stored_thread_limit(), adaptive, max_thread_limit())
}

fn gpu_type_name(gpu_type: GpuType) -> &'static str {
    match gpu_type {
        GpuType::None => "None",
        GpuType::Metal => "Metal",
        GpuType::Cuda => "CUDA",
        GpuType::Vulkan => "Vulkan",
        GpuType::OpenCL => "OpenCL",
    }
}

fn nvidia_driver_available() -> Option<bool> {
    #[cfg(target_os = "windows")]
    {
        return std::process::Command::new("nvidia-smi")
            .arg("-L")
            .output()
            .ok()
            .map(|output| output.status.success());
    }

    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

fn gpu_runtime_status(backend: WhisperCompiledBackend, nvidia_driver: Option<bool>) -> String {
    match (backend, nvidia_driver) {
        (WhisperCompiledBackend::Cuda, Some(true)) => {
            "NVIDIA driver detected; GPU activation is verified when a model loads.".to_string()
        }
        (WhisperCompiledBackend::Cuda, Some(false)) => {
            "NVIDIA driver not detected; CUDA acceleration cannot start.".to_string()
        }
        (WhisperCompiledBackend::Cuda, None) => {
            "CUDA runtime availability has not been verified on this platform.".to_string()
        }
        (WhisperCompiledBackend::Cpu, _) => {
            "This package has no GPU backend compiled in.".to_string()
        }
        _ => "GPU runtime availability is verified when a model loads.".to_string(),
    }
}

fn status_message(
    backend: WhisperCompiledBackend,
    gpu_detected: bool,
    nvidia_driver: Option<bool>,
) -> String {
    match (backend, gpu_detected) {
        (WhisperCompiledBackend::Cpu, true) => {
            "A GPU is detected, but this CPU package cannot enable it. Install the matching CUDA or Vulkan package, then restart Meetily.".to_string()
        }
        (WhisperCompiledBackend::Cpu, false) => {
            "This package uses the CPU. A GPU package must be installed and its compatible driver available before GPU acceleration can be used.".to_string()
        }
        (WhisperCompiledBackend::Cuda, _) if nvidia_driver == Some(false) => {
            "CUDA is compiled into this package, but no NVIDIA driver was detected. Install a compatible driver, then restart Meetily.".to_string()
        }
        (backend, _) => format!(
            "{} is compiled into this package. GPU acceleration is selected when the package is built and is verified when a model loads; it is not a live setting.",
            backend.as_str()
        ),
    }
}

fn load_preferences<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<TranscriptionPerformancePreferences, String> {
    let store = app
        .store("transcription_performance.json")
        .map_err(|error| {
            format!("Failed to access transcription performance preferences: {error}")
        })?;

    match store.get("preferences") {
        Some(value) => serde_json::from_value(value.clone()).map_err(|error| {
            format!("Failed to read transcription performance preferences: {error}")
        }),
        None => Ok(TranscriptionPerformancePreferences::default()),
    }
}

/// Called at setup so a persisted limit is in effect before the first recording.
pub fn initialize_preferences<R: Runtime>(app: &AppHandle<R>) {
    match load_preferences(app) {
        Ok(preferences) => set_stored_thread_limit(preferences.cpu_thread_limit),
        Err(error) => log::warn!("{error}; using automatic transcription thread selection"),
    }
}

#[tauri::command]
pub async fn get_transcription_performance_preferences<R: Runtime>(
    app: AppHandle<R>,
) -> Result<TranscriptionPerformancePreferences, String> {
    let preferences = load_preferences(&app)?;
    set_stored_thread_limit(preferences.cpu_thread_limit);
    Ok(preferences)
}

#[tauri::command]
pub async fn set_transcription_performance_preferences<R: Runtime>(
    app: AppHandle<R>,
    preferences: TranscriptionPerformancePreferences,
) -> Result<(), String> {
    if let Some(limit) = preferences.cpu_thread_limit {
        let maximum = max_thread_limit();
        if !(1..=maximum).contains(&limit) {
            return Err(format!(
                "CPU thread limit must be between 1 and {maximum} on this machine"
            ));
        }
    }

    let store = app
        .store("transcription_performance.json")
        .map_err(|error| {
            format!("Failed to access transcription performance preferences: {error}")
        })?;
    store.set(
        "preferences",
        serde_json::to_value(&preferences).map_err(|error| {
            format!("Failed to save transcription performance preferences: {error}")
        })?,
    );
    store.save().map_err(|error| {
        format!("Failed to persist transcription performance preferences: {error}")
    })?;
    set_stored_thread_limit(preferences.cpu_thread_limit);
    Ok(())
}

#[tauri::command]
pub async fn get_transcription_hardware_status() -> Result<TranscriptionHardwareStatus, String> {
    let profile = HardwareProfile::detect();
    let recommended = resolve_thread_count_with_limit(
        None,
        profile.get_whisper_config().max_threads,
        max_thread_limit(),
    );
    let backend = WhisperCompiledBackend::current();
    let nvidia_driver = nvidia_driver_available();

    Ok(TranscriptionHardwareStatus {
        compiled_backend: backend.as_str().to_string(),
        detected_gpu: gpu_type_name(profile.gpu_type).to_string(),
        gpu_runtime_status: gpu_runtime_status(backend, nvidia_driver),
        cpu_logical_cores: max_thread_limit(),
        recommended_cpu_threads: recommended,
        configured_cpu_threads: stored_thread_limit(),
        effective_cpu_threads: effective_thread_count(profile.get_whisper_config().max_threads),
        message: status_message(backend, profile.has_gpu_acceleration, nvidia_driver),
    })
}

#[cfg(test)]
mod tests {
    use super::resolve_thread_count_with_limit;

    #[test]
    fn automatic_limit_uses_adaptive_value() {
        assert_eq!(resolve_thread_count_with_limit(None, Some(8), 20), 8);
    }

    #[test]
    fn manual_limit_is_bounded_by_machine_capacity() {
        assert_eq!(resolve_thread_count_with_limit(Some(40), Some(8), 12), 12);
        assert_eq!(resolve_thread_count_with_limit(Some(0), Some(8), 12), 1);
    }
}
