//! The runtime the server's tasks run on. One process-wide `tokio` runtime,
//! made on first use, so a task can start from any thread, including the
//! engine's callbacks.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Handle, Runtime};
use tokio::task::JoinHandle;

fn handle() -> &'static Handle {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| Runtime::new().expect("a tokio runtime"))
        .handle()
}

pub fn spawn<F>(task: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    handle().spawn(task)
}

/// Run a future to its end from outside the runtime, as a test does.
pub fn block_on<F: Future>(task: F) -> F::Output {
    handle().block_on(task)
}
