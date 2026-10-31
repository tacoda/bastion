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

pub trait Engine: Send + Sync {
    /// The engine's address, as the profile gave it.
    fn url(&self) -> &str;
    /// The link as it is now. The SDK reconnects by itself.
    fn link(&self) -> Link;
    /// Call one function and wait for its answer.
    fn call<'a>(&'a self, namespace: &'a str, function_id: &'a str, payload: Value) -> Call<'a>;
    /// Run `on_push` each time a worker joins or leaves, so its functions do.
    fn on_functions_changed(&self, on_push: OnPush) -> Result<(), String>;
}
