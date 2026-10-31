//! The Commands seam (PLAN.md §6.2): every command the view calls and every
//! event the app emits, with their types. The view and the app both depend on
//! this crate, and on nothing of each other's.

use serde::{Deserialize, Serialize};

/// Command names, so the view and the app cannot drift apart.
pub mod command {
    pub const ENGINE_STATUS: &str = "engine_status";
}

/// Event names the app emits to the page (D10).
pub mod event {
    /// Carries an [`EngineStatus`](super::EngineStatus) each time the link changes.
    pub const ENGINE_CHANGED: &str = "engine-changed";
}

/// The app's link to the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    Connecting,
    Up,
    Reconnecting,
    Down,
}

/// What the page shows about the engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStatus {
    pub url: String,
    pub link: Link,
    /// The count of registered functions, when the link is up and the engine answered.
    pub functions: Option<usize>,
}
