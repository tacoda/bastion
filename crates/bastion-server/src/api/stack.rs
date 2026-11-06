//! Commands for the stack: status, start or attach, and stop.

use std::sync::Arc;

use bastion_protocol::{StackStatus, StartRefused};

use crate::engine::Engine;
use crate::machine::Machine;
use crate::stack::Stack;

pub fn stack_status(stack: &Stack, machine: &Arc<dyn Machine>) -> Result<StackStatus, String> {
    stack.status(machine.as_ref())
}

pub async fn stack_start(
    stack: &Stack,
    machine: &Arc<dyn Machine>,
    engine: &Arc<dyn Engine>,
) -> Result<StackStatus, StartRefused> {
    stack.start(machine.as_ref(), engine.as_ref()).await
}

pub fn stack_stop(stack: &Stack, machine: &Arc<dyn Machine>) -> Result<StackStatus, String> {
    stack.stop(machine.as_ref())
}
