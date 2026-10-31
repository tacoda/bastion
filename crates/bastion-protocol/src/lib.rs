//! The Commands seam (PLAN.md §6.2): every command the view calls and every
//! event the app emits, with their types. The view and the app both depend on
//! this crate, and on nothing of each other's.

use serde::{Deserialize, Serialize};

/// Command names, so the view and the app cannot drift apart.
pub mod command {
    pub const ENGINE_STATUS: &str = "engine_status";
    pub const DOCTOR: &str = "doctor";
    pub const STACK_STATUS: &str = "stack_status";
    pub const STACK_START: &str = "stack_start";
    pub const STACK_STOP: &str = "stack_stop";
    pub const PROFILE: &str = "profile";
    /// Takes `{ stopStack: bool }`.
    pub const QUIT: &str = "quit";
}

/// Event names the app emits to the page (D10).
pub mod event {
    /// Carries an [`EngineStatus`](super::EngineStatus) each time the link changes.
    pub const ENGINE_CHANGED: &str = "engine-changed";
    /// The owner asked to quit while the stack runs: the page asks whether to stop it.
    pub const QUIT_REQUESTED: &str = "quit-requested";
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

/// The stack this profile runs, as `iii ps` sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum StackState {
    Stopped,
    Running,
    /// Another engine holds the profile's port.
    Taken {
        pid: u32,
        compose_file: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackStatus {
    pub state: StackState,
    pub compose_file: String,
    /// Where the compose daemon's output goes when the app started it.
    pub log: String,
}

/// Why the stack did not start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum StartRefused {
    /// A required Doctor check failed.
    Doctor {
        report: DoctorReport,
    },
    Taken {
        pid: u32,
        compose_file: Option<String>,
    },
    Failed {
        message: String,
    },
}

/// The profile the app runs with (PLAN.md D12). It holds no secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveProfile {
    pub name: String,
    pub kind: String,
    pub engine_url: String,
    pub checkout: String,
    /// The file the profiles live in.
    pub file: String,
}
