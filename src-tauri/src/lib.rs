//! Bastion's Rust core. The pages reach the engine only through these commands
//! (PLAN.md D9), and the engine reaches the pages only as events (D10).

mod commands;
mod doctor;
mod engine;
mod machine;
mod profile;
mod stack;

use std::sync::Arc;

use engine::{Engine, IiiEngine};
use machine::{Machine, ThisMachine};
use profile::Profiles;
use stack::Stack;
use tauri::Manager;

/// The checkout this build came from, which holds `worker-compose.yaml`.
// ponytail: a build-time path; packaging the stack replaces it (PLAN.md A11).
const CHECKOUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path = app.path().app_config_dir()?.join(profile::FILE_NAME);
            let checkout = std::path::Path::new(CHECKOUT).canonicalize()?;
            let profiles = Profiles::load_or_create(&path, checkout)?;
            let active = profiles.active();
            let stack = Stack {
                checkout: active.checkout.clone(),
                port: stack::port(&active.engine_url)
                    .ok_or_else(|| format!("no port in engine_url {:?}", active.engine_url))?,
                log: app.path().app_log_dir()?.join("compose.log"),
            };
            let engine: Arc<dyn Engine> = Arc::new(IiiEngine::connect(&active.engine_url));
            commands::engine::watch(app.handle().clone(), engine.clone());
            app.manage(engine);
            app.manage(Arc::new(ThisMachine) as Arc<dyn Machine>);
            app.manage(profiles);
            app.manage(stack);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::engine::engine_status,
            commands::doctor::doctor,
            commands::stack::stack_status,
            commands::stack::stack_start,
            commands::stack::stack_stop
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
