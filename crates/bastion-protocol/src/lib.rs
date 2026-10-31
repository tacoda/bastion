//! The Commands seam (PLAN.md §6.2): every command the view calls and every
//! event the app emits, with their types. The view and the app both depend on
//! this crate, and on nothing of each other's.

use serde::{Deserialize, Serialize};

/// Command names, so the view and the app cannot drift apart.
pub mod command {
    pub const ENGINE_STATUS: &str = "engine_status";
    pub const DOCTOR: &str = "doctor";
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

/// The outcome of one Doctor check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Pass,
    /// Failed, but the stack can start without it.
    Warn,
    Fail,
    /// Not run, because what it needs is not there yet (the engine, say).
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub required: bool,
    pub state: CheckState,
    pub detail: String,
    /// What to do about a `Warn` or a `Fail`.
    pub fix: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}

impl DoctorReport {
    /// The stack may start: no required check failed.
    pub fn ready(&self) -> bool {
        !self
            .checks
            .iter()
            .any(|c| c.required && c.state == CheckState::Fail)
    }
}
