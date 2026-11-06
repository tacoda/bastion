//! The app's own commands: the active profile, the window's server, and quit.

use std::path::PathBuf;

use bastion_protocol::{ActiveProfile, Target};
use tauri::{AppHandle, State};

use crate::profile::{Kind, Profiles};

/// Where `profiles.toml` was read from.
pub struct ProfilesFile(pub PathBuf);

#[tauri::command]
pub fn profile(profiles: State<'_, Profiles>, file: State<'_, ProfilesFile>) -> ActiveProfile {
    let p = profiles.active();
    ActiveProfile {
        name: p.name.clone(),
        kind: match p.kind {
            Kind::Local => "local".into(),
        },
        engine_url: p.engine_url.clone(),
        checkout: p.checkout.display().to_string(),
        file: file.0.display().to_string(),
    }
}

/// The server this window talks to, with its token (D28).
#[tauri::command]
pub fn target(target: State<'_, Target>) -> Target {
    target.inner().clone()
}

/// Quit the app. The server and its stack keep running unless the page
/// stopped the stack first (D27).
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
