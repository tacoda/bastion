//! Commands for the engine console (PLAN.md A2).

use std::sync::Arc;

use bastion_protocol::{Console, LogPage, LogRequest};
use tauri::State;

use crate::console;
use crate::engine::Engine;

#[tauri::command]
pub async fn console(engine: State<'_, Arc<dyn Engine>>) -> Result<Console, String> {
    console::read(engine.inner().as_ref()).await
}

#[tauri::command]
pub async fn container_logs(
    engine: State<'_, Arc<dyn Engine>>,
    request: LogRequest,
) -> Result<LogPage, String> {
    console::logs(engine.inner().as_ref(), &request).await
}

#[tauri::command]
pub async fn container_stop(
    engine: State<'_, Arc<dyn Engine>>,
    name: String,
) -> Result<(), String> {
    console::set_running(engine.inner().as_ref(), &name, false).await
}

#[tauri::command]
pub async fn container_start(
    engine: State<'_, Arc<dyn Engine>>,
    name: String,
) -> Result<(), String> {
    console::set_running(engine.inner().as_ref(), &name, true).await
}
