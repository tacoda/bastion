//! The `govern` commands, each a call to `bastion_server::api::govern` (D28; S3 removes them).

use super::prelude::*;

#[tauri::command]
pub async fn held_allow(cell: State<'_, FactoryCell>, held: HeldCallRef) -> Result<(), String> {
    api::govern::held_allow(&cell, held).await
}

#[tauri::command]
pub async fn held_refuse(
    cell: State<'_, FactoryCell>,
    held: HeldCallRef,
    feedback: String,
) -> Result<(), String> {
    api::govern::held_refuse(&cell, held, feedback).await
}

#[tauri::command]
pub async fn constraints(cell: State<'_, FactoryCell>) -> Result<Vec<ConstraintView>, String> {
    api::govern::constraints(&cell).await
}

#[tauri::command]
pub async fn constraint_add(
    cell: State<'_, FactoryCell>,
    constraint: ConstraintView,
) -> Result<(), String> {
    api::govern::constraint_add(&cell, constraint).await
}

#[tauri::command]
pub async fn constraint_move(
    cell: State<'_, FactoryCell>,
    id: String,
    up: bool,
) -> Result<(), String> {
    api::govern::constraint_move(&cell, id, up).await
}

#[tauri::command]
pub async fn capabilities(cell: State<'_, FactoryCell>) -> Result<Vec<CapabilityView>, String> {
    api::govern::capabilities(&cell).await
}

#[tauri::command]
pub async fn capability_move(
    cell: State<'_, FactoryCell>,
    stage: String,
    function: String,
    up: bool,
) -> Result<(), String> {
    api::govern::capability_move(&cell, stage, function, up).await
}

#[tauri::command]
pub async fn audit(cell: State<'_, FactoryCell>) -> Result<AuditView, String> {
    api::govern::audit(&cell).await
}

#[tauri::command]
pub async fn audit_export(
    app: AppHandle,
    cell: State<'_, FactoryCell>,
    machine: State<'_, Arc<dyn Machine>>,
) -> Result<String, String> {
    api::govern::audit_export(
        &app.path().app_data_dir().map_err(|e| e.to_string())?,
        &cell,
        &machine,
    )
    .await
}

#[tauri::command]
pub async fn charters(
    cell: State<'_, FactoryCell>,
    machine: State<'_, Arc<dyn Machine>>,
) -> Result<Vec<WorkspaceCharterView>, String> {
    api::govern::charters(&cell, &machine).await
}

#[tauri::command]
pub async fn factory_capabilities(
    cell: State<'_, FactoryCell>,
) -> Result<Vec<FactoryCapabilityView>, String> {
    api::govern::factory_capabilities(&cell).await
}
