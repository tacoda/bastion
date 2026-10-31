//! Bastion's Rust core. The pages reach the engine only through these commands
//! (PLAN.md D9), and the engine reaches the pages only as events (D10).

mod commands;
mod console;
mod doctor;
mod engine;
mod machine;
mod profile;
mod stack;

use std::sync::Arc;

use bastion_protocol::event;
use commands::app::{ProfilesFile, should_ask};
use engine::{Engine, IiiEngine};
use machine::{Machine, ThisMachine};
use profile::Profiles;
use stack::Stack;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

/// The checkout this build came from, which holds `worker-compose.yaml`.
// ponytail: a build-time path; packaging the stack replaces it (PLAN.md A11).
const CHECKOUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Ask the page before a quit while the stack runs. True means the quit waits.
fn hold_quit(app: &AppHandle) -> bool {
    let machine = app.state::<Arc<dyn Machine>>();
    if !should_ask(&app.state::<Stack>(), machine.inner().as_ref()) {
        return false;
    }
    let _ = app.emit(event::QUIT_REQUESTED, ());
    true
}

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
            let handle = app.handle().clone();
            engine.on_functions_changed(Arc::new(move || {
                let _ = handle.emit(event::CONSOLE_CHANGED, ());
            }))?;
            app.manage(engine);
            app.manage(Arc::new(ThisMachine) as Arc<dyn Machine>);
            app.manage(profiles);
            app.manage(ProfilesFile(path));
            app.manage(stack);
            Ok(())
        })
        .on_window_event(|window, e| {
            if let WindowEvent::CloseRequested { api, .. } = e
                && hold_quit(window.app_handle())
            {
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::profile,
            commands::app::quit,
            commands::console::console,
            commands::console::container_logs,
            commands::console::container_stop,
            commands::console::container_start,
            commands::engine::engine_status,
            commands::doctor::doctor,
            commands::stack::stack_status,
            commands::stack::stack_start,
            commands::stack::stack_stop
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, e| {
            // ⌘Q has no exit code; the `quit` command exits with one.
            if let RunEvent::ExitRequested {
                api, code: None, ..
            } = e
                && hold_quit(app)
            {
                api.prevent_exit();
            }
        });
}
