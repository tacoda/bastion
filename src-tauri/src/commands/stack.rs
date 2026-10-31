//! Commands for the stack: status, start or attach, and stop.

use std::sync::Arc;

use bastion_protocol::{StackStatus, StartRefused};
use tauri::State;

use crate::engine::Engine;
use crate::machine::Machine;
use crate::stack::Stack;

#[tauri::command]
pub fn stack_status(
    stack: State<'_, Stack>,
    machine: State<'_, Arc<dyn Machine>>,
) -> Result<StackStatus, String> {
    stack.status(machine.inner().as_ref())
}

#[tauri::command]
pub async fn stack_start(
    stack: State<'_, Stack>,
    machine: State<'_, Arc<dyn Machine>>,
    engine: State<'_, Arc<dyn Engine>>,
) -> Result<StackStatus, StartRefused> {
    stack
        .start(machine.inner().as_ref(), engine.inner().as_ref())
        .await
}

#[tauri::command]
pub fn stack_stop(
    stack: State<'_, Stack>,
    machine: State<'_, Arc<dyn Machine>>,
) -> Result<StackStatus, String> {
    stack.stop(machine.inner().as_ref())
}
