//! The Engine seam (PLAN.md §6.2). This module is the only code in the app that
//! imports `iii-sdk` (D19). Everything else holds an `Arc<dyn Engine>`.

#[cfg(test)]
pub mod fake;
mod iii;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bastion_protocol::Link;
use serde_json::Value;

pub use iii::IiiEngine;

/// The namespace the engine registers its own functions in, such as `engine::*`.
pub const DEFAULT_NAMESPACE: &str = "default";

pub type Call<'a> = Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>>;

/// Run on each push from the engine. It must return quickly.
pub type OnPush = Arc<dyn Fn() + Send + Sync>;

/// Handle one trigger event and return the handler's answer (`{}` for a
/// plain event, a decision for a hook). It must return quickly.
pub type OnEvent = Arc<dyn Fn(Value) -> Value + Send + Sync>;

pub type Bytes<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>>;

/// How long a call waits by default. Slow calls, such as booting a VM, pass
/// their own timeout to `call_for`.
pub const CALL_TIMEOUT_MS: u64 = 5_000;

pub trait Engine: Send + Sync {
    /// The engine's address, as the profile gave it.
    fn url(&self) -> &str;
    /// The link as it is now. The SDK reconnects by itself.
    fn link(&self) -> Link;
    /// Call one function and wait for its answer, up to `CALL_TIMEOUT_MS`.
    fn call<'a>(&'a self, namespace: &'a str, function_id: &'a str, payload: Value) -> Call<'a> {
        self.call_for(namespace, function_id, payload, CALL_TIMEOUT_MS)
    }
    fn call_for<'a>(
        &'a self,
        namespace: &'a str,
        function_id: &'a str,
        payload: Value,
        timeout_ms: u64,
    ) -> Call<'a>;
    /// Run `on_event` for each event of `trigger_type` that matches `config`.
    /// Returns the binding's id for `unbind`.
    fn bind(&self, trigger_type: &str, config: Value, on_event: OnEvent) -> Result<String, String>;
    fn unbind(&self, id: &str);
    /// Read every byte of a content channel, such as `sandbox::fs::read` returns.
    fn read_channel(&self, content: Value) -> Bytes<'_>;
    /// Run `on_push` each time a worker joins or leaves, so its functions do.
    fn on_functions_changed(&self, on_push: OnPush) -> Result<(), String>;
}
