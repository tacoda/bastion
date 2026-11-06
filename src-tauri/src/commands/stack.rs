//! The `stack` commands, each a call to `bastion_server::api::stack` (D28; S3 removes them).

use super::prelude::*;

#[tauri::command]
pub fn stack_status(
    stack: State<'_, Stack>,
    machine: State<'_, Arc<dyn Machine>>,
) -> Result<StackStatus, String> {
    api::stack::stack_status(&stack, &machine)
}

#[tauri::command]
pub async fn stack_start(
    stack: State<'_, Stack>,
    machine: State<'_, Arc<dyn Machine>>,
    engine: State<'_, Arc<dyn Engine>>,
) -> Result<StackStatus, StartRefused> {
    api::stack::stack_start(&stack, &machine, &engine).await
}

#[tauri::command]
pub fn stack_stop(
    stack: State<'_, Stack>,
    machine: State<'_, Arc<dyn Machine>>,
) -> Result<StackStatus, String> {
    api::stack::stack_stop(&stack, &machine)
}
