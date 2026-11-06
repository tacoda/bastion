//! The Doctor command.

use std::sync::Arc;

use bastion_protocol::DoctorReport;

use crate::doctor;
use crate::engine::Engine;
use crate::machine::Machine;

pub async fn doctor(
    machine: &Arc<dyn Machine>,
    engine: &Arc<dyn Engine>,
    checkout: &std::path::Path,
) -> Result<DoctorReport, ()> {
    Ok(doctor::run(machine.as_ref(), engine.as_ref(), checkout).await)
}
