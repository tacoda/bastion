//! Bastion's Rust core. The pages reach the engine only through these commands
//! (PLAN.md D9), and the engine reaches the pages only as events (D10).

mod audit_log;
mod charter;
mod commands;
mod console;
mod doctor;
mod engine;
mod eval_store;
mod evals;
mod factory;
mod gate;
mod machine;
mod profile;
mod stack;
mod store;

use std::sync::Arc;

use bastion_protocol::{Link, event};
use commands::app::{ProfilesFile, should_ask};
use commands::factory::FactoryCell;
use engine::{Engine, IiiEngine};
use factory::Factory;
use machine::{Machine, ThisMachine};
use profile::Profiles;
use stack::Stack;
use store::SqlStore;
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

/// Once the engine is up: open the store, start the factory, and resume its jobs.
fn start_factory(
    app: AppHandle,
    engine: Arc<dyn Engine>,
    machine: Arc<dyn Machine>,
    cell: FactoryCell,
) -> Result<(), String> {
    let scratch = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("bundles");
    tauri::async_runtime::spawn(async move {
        loop {
            if engine.link() == Link::Up {
                match SqlStore::open(engine.clone()).await {
                    Ok(store) => {
                        let emitter = app.clone();
                        let on_change = Arc::new(move |id: &str| {
                            let _ = emitter.emit(event::JOB_CHANGED, id);
                        });
                        let factory =
                            Factory::new(engine, Arc::new(store), machine, on_change, scratch);
                        if let Err(e) = factory.start() {
                            eprintln!("the factory did not start: {e}");
                        }
                        let _ = cell.set(factory);
                        // Pages that loaded before the factory read again.
                        let _ = app.emit(event::JOB_CHANGED, "");
                        return;
                    }
                    Err(e) => eprintln!("the store did not open: {e}"),
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    Ok(())
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
    commands::engine::watch(app.handle().clone(), engine.clone());
    let handle = app.handle().clone();
    engine.on_functions_changed(Arc::new(move || {
        let _ = handle.emit(event::CONSOLE_CHANGED, ());
    }))?;
    let machine: Arc<dyn Machine> = Arc::new(ThisMachine);
    let cell = FactoryCell::default();
    start_factory(
        app.handle().clone(),
        engine.clone(),
        machine.clone(),
        cell.clone(),
    )?;
    app.manage(cell);
    app.manage(engine);
    app.manage(machine);
    app.manage(profiles);
    app.manage(ProfilesFile(path));
    app.manage(stack);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(setup)
        .on_window_event(|window, e| match e {
            WindowEvent::CloseRequested { api, .. } if hold_quit(window.app_handle()) => {
                api.prevent_close();
            }
            // A hidden webview can miss pushes, so every page reads again.
            WindowEvent::Focused(true) => {
                let _ = window.emit(event::JOB_CHANGED, "");
            }
            _ => {}
        })
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
            commands::govern::capability_move,
            commands::govern::audit,
            commands::evals::evals,
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
