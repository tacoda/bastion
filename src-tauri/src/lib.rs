//! Bastion's Rust core. The pages reach the engine only through these commands
//! (PLAN.md D9), and the engine reaches the pages only as events (D10).

mod commands;
mod profile;

use bastion_server::events::Bus;
use bastion_server::server::Server;
use bastion_server::{engine, machine, stack};
use tokio::sync::broadcast::error::RecvError;

use std::sync::Arc;

use bastion_protocol::{Change, event};
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

/// Where the factory keeps its bundles, and the first-run rules and grants
/// the stack carries (D26).
fn factory_inputs(app: &AppHandle) -> Result<(String, std::path::PathBuf), String> {
    let scratch = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("bundles");
    let seed = std::fs::read_to_string(format!("{CHECKOUT}/seed/governance.sql"))
        .map_err(|e| format!("seed/governance.sql: {e}"))?;
    Ok((seed, scratch))
}

/// Load the profile, connect to the engine, and start the factory once it is up.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
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
    let machine: Arc<dyn Machine> = Arc::new(ThisMachine);
    let server = Server::new(engine.clone(), machine.clone())?;
    bridge(app.handle().clone(), server.events.clone());
    let (seed, scratch) = factory_inputs(app.handle())?;
    server.start_factory(seed, scratch);
    app.manage(server.factory.clone());
    app.manage(server.events.clone());
    app.manage(engine);
    app.manage(machine);
    app.manage(profiles);
    app.manage(ProfilesFile(path));
    app.manage(stack);
    Ok(())
}

/// Pass the server's changes to the page as the Tauri events it listens for,
/// until S3 has the page follow `GET /events` itself.
fn bridge(app: AppHandle, events: Arc<Bus>) {
    let mut changes = events.subscribe();
    bastion_server::rt::spawn(async move {
        loop {
            let _ = match changes.recv().await {
                Ok((_, Change::Job { id })) => app.emit(event::JOB_CHANGED, id),
                Ok((_, Change::Engine { status })) => app.emit(event::ENGINE_CHANGED, status),
                Ok((_, Change::Console)) => app.emit(event::CONSOLE_CHANGED, ()),
                // Missed changes: every page reads again.
                Err(RecvError::Lagged(_)) => app.emit(event::JOB_CHANGED, ""),
                Err(RecvError::Closed) => return,
            };
        }
    });
}

/// Ask before closing while the stack runs, and read every page again on focus.
fn on_window(window: &tauri::Window, e: &WindowEvent) {
    match e {
        WindowEvent::CloseRequested { api, .. } if hold_quit(window.app_handle()) => {
            api.prevent_close();
        }
        // A hidden webview can miss pushes, so every page reads again.
        WindowEvent::Focused(true) => {
            let _ = window.emit(event::JOB_CHANGED, "");
        }
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(setup)
        .on_window_event(on_window)
        .invoke_handler(tauri::generate_handler![
            commands::app::profile,
            commands::app::quit,
            commands::console::console,
            commands::console::container_logs,
            commands::console::container_stop,
            commands::console::container_start,
            commands::engine::engine_status,
            commands::factory::workspaces,
            commands::factory::workspace_add,
            commands::factory::jobs,
            commands::factory::job,
            commands::factory::job_submit,
            commands::factory::job_approve,
            commands::factory::job_comment,
            commands::factory::job_stop,
            commands::factory::dials,
            commands::factory::dials_set,
            commands::govern::held_allow,
            commands::govern::held_refuse,
            commands::govern::constraints,
            commands::govern::constraint_add,
            commands::govern::constraint_move,
            commands::govern::capabilities,
            commands::govern::charters,
            commands::govern::factory_capabilities,
            commands::govern::capability_move,
            commands::govern::audit,
            commands::evals::evals,
            commands::settings::settings,
            commands::settings::settings_set,
            commands::settings::settings_preset,
            commands::settings::settings_history,
            commands::settings::settings_restore,
            commands::settings::models,
            commands::settings::accounts,
            commands::settings::issues,
            commands::settings::job_from_issue,
            commands::evals::eval,
            commands::evals::eval_create,
            commands::evals::eval_apply,
            commands::govern::audit_export,
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
