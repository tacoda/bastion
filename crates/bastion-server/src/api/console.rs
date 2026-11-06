//! Commands for the engine console (PLAN.md A2).

use std::sync::Arc;

use bastion_protocol::{Console, LogPage, LogRequest};

use crate::console;
use crate::engine::Engine;

pub async fn console(engine: &Arc<dyn Engine>) -> Result<Console, String> {
    console::read(engine.as_ref()).await
}

pub async fn container_logs(
    engine: &Arc<dyn Engine>,
    request: LogRequest,
) -> Result<LogPage, String> {
    console::logs(engine.as_ref(), &request).await
}

pub async fn container_stop(engine: &Arc<dyn Engine>, name: String) -> Result<(), String> {
    console::set_running(engine.as_ref(), &name, false).await
}

pub async fn container_start(engine: &Arc<dyn Engine>, name: String) -> Result<(), String> {
    console::set_running(engine.as_ref(), &name, true).await
}
