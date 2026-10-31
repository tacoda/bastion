//! The Doctor command.

use std::sync::Arc;

use bastion_protocol::DoctorReport;
use tauri::State;

use crate::doctor;
use crate::engine::Engine;
use crate::machine::Machine;
use crate::profile::Profiles;

#[tauri::command]
pub async fn doctor(
    machine: State<'_, Arc<dyn Machine>>,
    engine: State<'_, Arc<dyn Engine>>,
    profiles: State<'_, Profiles>,
) -> Result<DoctorReport, ()> {
    let checkout = profiles.active().checkout.clone();
    Ok(doctor::run(machine.inner().as_ref(), engine.inner().as_ref(), &checkout).await)
}
