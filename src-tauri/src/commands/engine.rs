//! The `engine` commands, each a call to `bastion_server::api::engine` (D28; S3 removes them).

use super::prelude::*;

#[tauri::command]
pub async fn engine_status(engine: State<'_, Arc<dyn Engine>>) -> Result<EngineStatus, ()> {
    api::engine::engine_status(&engine).await
}
