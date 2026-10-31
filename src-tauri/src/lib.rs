//! Bastion's Rust core. The pages reach the engine only through these commands
//! (PLAN.md D9), and the engine reaches the pages only as events (D10).

mod commands;
mod engine;

use std::sync::Arc;

use engine::{Engine, IiiEngine};
use tauri::Manager;

/// The local engine. A profile replaces this default in A1 (D12).
const ENGINE_URL: &str = "ws://127.0.0.1:49160";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let url = std::env::var("III_URL").unwrap_or_else(|_| ENGINE_URL.into());
            let engine: Arc<dyn Engine> = Arc::new(IiiEngine::connect(&url));
            commands::engine::watch(app.handle().clone(), engine.clone());
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::engine::engine_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
