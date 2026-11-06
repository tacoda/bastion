//! Bastion's window (PLAN.md D27, D28). It starts or attaches to the local
//! server and tells the page where it is; the page does the rest over HTTP.

mod commands;
mod local;
mod profile;

use bastion_protocol::event;
use commands::app::ProfilesFile;
use profile::Profiles;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

/// The checkout this build came from, which holds `worker-compose.yaml`.
// ponytail: a build-time path; packaging the stack replaces it (PLAN.md A11).
const CHECKOUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Load the profile, then start or attach to its local server.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path().app_config_dir()?.join(profile::FILE_NAME);
    let checkout = std::path::Path::new(CHECKOUT).canonicalize()?;
    let profiles = Profiles::load_or_create(&path, checkout)?;
    let data = app.path().app_data_dir()?.join("server");
    let target = local::start(profiles.active(), &data)?;
    app.manage(target);
    app.manage(profiles);
    app.manage(ProfilesFile(path));
    Ok(())
}

/// The page decides a quit: it asks only while the stack runs.
fn ask_page(app: &AppHandle) {
    let _ = app.emit(event::QUIT_REQUESTED, ());
}

fn on_window(window: &tauri::Window, e: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = e {
        api.prevent_close();
        ask_page(window.app_handle());
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(setup)
        .on_window_event(on_window)
        .invoke_handler(tauri::generate_handler![
            commands::app::profile,
            commands::app::target,
            commands::app::quit,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, e| {
            // ⌘Q has no exit code; the `quit` command exits with one.
            if let RunEvent::ExitRequested {
                api, code: None, ..
            } = e
            {
                api.prevent_exit();
                ask_page(app);
            }
        });
}
