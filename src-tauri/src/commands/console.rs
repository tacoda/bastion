//! The `console` commands, each a call to `bastion_server::api::console` (D28; S3 removes them).

use super::prelude::*;

#[tauri::command]
pub async fn console(engine: State<'_, Arc<dyn Engine>>) -> Result<Console, String> {
    api::console::console(&engine).await
}

#[tauri::command]
pub async fn container_logs(
    engine: State<'_, Arc<dyn Engine>>,
    request: LogRequest,
) -> Result<LogPage, String> {
    api::console::container_logs(&engine, request).await
}

#[tauri::command]
pub async fn container_stop(
    engine: State<'_, Arc<dyn Engine>>,
    name: String,
) -> Result<(), String> {
    api::console::container_stop(&engine, name).await
}

#[tauri::command]
pub async fn container_start(
    engine: State<'_, Arc<dyn Engine>>,
    name: String,
) -> Result<(), String> {
    api::console::container_start(&engine, name).await
}
