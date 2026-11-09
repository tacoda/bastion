//! Commands for the stack: status, start or attach, and stop.

use std::sync::Arc;

use bastion_protocol::{StackStatus, StartRefused};

use crate::engine::Engine;
use crate::machine::Machine;
use crate::stack::Stack;

/// One start or stop at a time, so a stop never races a start's spawn.
static BUSY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn stack_status(stack: &Stack, machine: &Arc<dyn Machine>) -> Result<StackStatus, String> {
    stack.status(machine.as_ref())
}

pub async fn stack_start(
    stack: &Stack,
    machine: &Arc<dyn Machine>,
    engine: &Arc<dyn Engine>,
) -> Result<StackStatus, StartRefused> {
    let _busy = BUSY.lock().await;
    stack.start(machine.as_ref(), engine.as_ref()).await
}

pub async fn stack_stop(stack: &Stack, machine: &Arc<dyn Machine>) -> Result<StackStatus, String> {
    let _busy = BUSY.lock().await;
    stack.stop(machine.as_ref())
}
