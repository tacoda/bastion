//! The `factory` commands, each a call to `bastion_server::api::factory` (D28; S3 removes them).

use super::prelude::*;

#[tauri::command]
pub async fn workspaces(cell: State<'_, FactoryCell>) -> Result<Vec<Workspace>, String> {
    api::factory::workspaces(&cell).await
}

#[tauri::command]
pub async fn workspace_add(
    cell: State<'_, FactoryCell>,
    machine: State<'_, Arc<dyn Machine>>,
    workspace: Workspace,
) -> Result<(), String> {
    api::factory::workspace_add(&cell, &machine, workspace).await
}

#[tauri::command]
pub async fn jobs(cell: State<'_, FactoryCell>) -> Result<Vec<JobSummary>, String> {
    api::factory::jobs(&cell).await
}

#[tauri::command]
pub async fn job(cell: State<'_, FactoryCell>, id: String) -> Result<JobDetail, String> {
    api::factory::job(&cell, id).await
}

#[tauri::command]
pub async fn job_submit(
    cell: State<'_, FactoryCell>,
    workspace: String,
    request: String,
    pipeline: Option<Pipeline>,
) -> Result<String, String> {
    api::factory::job_submit(&cell, workspace, request, pipeline).await
}

#[tauri::command]
pub async fn job_approve(cell: State<'_, FactoryCell>, id: String) -> Result<(), String> {
    api::factory::job_approve(&cell, id).await
}

#[tauri::command]
pub async fn job_comment(
    cell: State<'_, FactoryCell>,
    id: String,
    text: String,
) -> Result<(), String> {
    api::factory::job_comment(&cell, id, text).await
}

#[tauri::command]
pub async fn job_stop(cell: State<'_, FactoryCell>, id: String) -> Result<(), String> {
    api::factory::job_stop(&cell, id).await
}

#[tauri::command]
pub async fn dials(cell: State<'_, FactoryCell>) -> Result<DialsView, String> {
    api::factory::dials(&cell).await
}

#[tauri::command]
pub async fn dials_set(cell: State<'_, FactoryCell>, dials: DialsView) -> Result<(), String> {
    api::factory::dials_set(&cell, dials).await
}
