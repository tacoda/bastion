//! The app's own commands: the active profile, and quitting with or without
//! the stack (PLAN.md D13).

use std::path::PathBuf;
use std::sync::Arc;

use bastion_protocol::{ActiveProfile, StackState};
use tauri::{AppHandle, State};

use crate::machine::Machine;
use crate::profile::{Kind, Profiles};
use crate::stack::Stack;

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

#[tauri::command]
pub fn quit(
    app: AppHandle,
    stack: State<'_, Stack>,
    machine: State<'_, Arc<dyn Machine>>,
    stop_stack: bool,
) -> Result<(), String> {
    if stop_stack {
        stack.stop(machine.inner().as_ref())?;
    }
    app.exit(0);
    Ok(())
}

/// A quit asks the owner first only while this profile's stack runs.
pub fn should_ask(stack: &Stack, machine: &dyn Machine) -> bool {
    matches!(
        stack.status(machine).map(|s| s.state),
        Ok(StackState::Running)
    )
}
