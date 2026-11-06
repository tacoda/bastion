//! The `settings` commands, each a call to `bastion_server::api::settings` (D28; S3 removes them).

use super::prelude::*;

#[tauri::command]
pub async fn settings(cell: State<'_, FactoryCell>) -> Result<SettingsView, String> {
    api::settings::settings(&cell).await
}

#[tauri::command]
pub async fn settings_set(
    events: State<'_, Arc<Bus>>,
    cell: State<'_, FactoryCell>,
    settings: SettingsView,
) -> Result<(), String> {
    api::settings::settings_set(&events, &cell, settings).await
}

#[tauri::command]
pub async fn settings_preset(
    events: State<'_, Arc<Bus>>,
    cell: State<'_, FactoryCell>,
    preset: String,
) -> Result<(), String> {
    api::settings::settings_preset(&events, &cell, preset).await
}

#[tauri::command]
pub async fn settings_history(cell: State<'_, FactoryCell>) -> Result<Vec<VersionView>, String> {
    api::settings::settings_history(&cell).await
}

#[tauri::command]
pub async fn settings_restore(
    events: State<'_, Arc<Bus>>,
    cell: State<'_, FactoryCell>,
    version: u64,
) -> Result<(), String> {
    api::settings::settings_restore(&events, &cell, version).await
}

#[tauri::command]
pub async fn models(cell: State<'_, FactoryCell>) -> Result<Vec<ModelOption>, String> {
    api::settings::models(&cell).await
}

#[tauri::command]
pub async fn accounts(cell: State<'_, FactoryCell>) -> Result<Vec<AccountView>, String> {
    api::settings::accounts(&cell).await
}

#[tauri::command]
pub async fn issues(
    cell: State<'_, FactoryCell>,
    machine: State<'_, std::sync::Arc<dyn crate::machine::Machine>>,
    workspace: String,
) -> Result<Vec<IssueView>, String> {
    api::settings::issues(&cell, &machine, workspace).await
}

#[tauri::command]
pub async fn job_from_issue(
    cell: State<'_, FactoryCell>,
    machine: State<'_, std::sync::Arc<dyn crate::machine::Machine>>,
    workspace: String,
    number: u64,
) -> Result<String, String> {
    api::settings::job_from_issue(&cell, &machine, workspace, number).await
}
