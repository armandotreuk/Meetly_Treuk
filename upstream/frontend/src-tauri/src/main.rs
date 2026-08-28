#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use env_logger;
use log;

fn main() {
    // Only the first argument selects the packaged diagnostic. Tauri forwards
    // argv for single-instance activation and deep links, so matching the flag
    // anywhere in the list would let a forwarded payload exit the app instead
    // of opening the requested content.
    if std::env::args().nth(1).as_deref() == Some("--smoke-dbstat") {
        std::process::exit(app_lib::run_dbstat_smoke());
    }

    std::env::set_var("RUST_LOG", "info");
    env_logger::init();

    // Async logger will be initialized lazily when first needed (after Tauri runtime starts)
    log::info!("Starting application...");
    app_lib::run();
}
