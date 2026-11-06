//! The `doctor` commands, each a call to `bastion_server::api::doctor` (D28; S3 removes them).

use super::prelude::*;

#[tauri::command]
pub async fn doctor(
    machine: State<'_, Arc<dyn Machine>>,
    engine: State<'_, Arc<dyn Engine>>,
    profiles: State<'_, Profiles>,
) -> Result<DoctorReport, ()> {
    api::doctor::doctor(&machine, &engine, &profiles.active().checkout).await
}
