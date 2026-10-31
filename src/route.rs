//! Where the window is. Pages navigate through the `Nav` in context.

use leptos::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Jobs,
    NewRequest,
    Job(String),
    Engine,
    Settings,
}

pub type Nav = WriteSignal<Route>;

pub fn nav() -> Nav {
    expect_context::<Nav>()
}

/// A status mark and its color, as a run of steps shows them.
pub fn mark(state: &str) -> (&'static str, &'static str) {
    match state {
        "done" | "landed" => ("✓", "text-ok"),
        "running" | "provisioning" => ("●", "text-link"),
        "waiting" | "needs_you" => ("!", "text-warn"),
        "failed" => ("✕", "text-bad"),
        "stopped" => ("■", "text-muted"),
        "queued" => ("◌", "text-muted"),
        _ => ("○", "text-muted"),
    }
}

/// "3 min ago", from seconds since the epoch.
pub fn ago(at: u64) -> String {
    let now = (js_sys::Date::now() / 1000.0) as u64;
    match now.saturating_sub(at) {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

/// "in 12 min", to a time in seconds since the epoch.
pub fn until(at: u64) -> String {
    let now = (js_sys::Date::now() / 1000.0) as u64;
    match at.saturating_sub(now) {
        0 => "now".into(),
        s if s < 60 => "in under a minute".into(),
        s => format!("in {} min", s.div_ceil(60)),
    }
}
