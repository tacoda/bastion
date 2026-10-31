//! The stack routine (PLAN.md A1, D13): start, attach to, and stop the compose
//! project in the profile's checkout. `iii ps --json` is the source of truth.

use std::path::PathBuf;
use std::time::Duration;

use bastion_protocol::{StackState, StackStatus, StartRefused};
use serde::Deserialize;

use crate::doctor::{self, COMPOSE_FILE};
use crate::engine::Engine;
use crate::machine::Machine;

/// How long a started daemon has to show up in `iii ps`.
const START_WAIT: Duration = Duration::from_secs(10);
const START_POLL: Duration = Duration::from_millis(250);

/// Where this profile's stack lives.
pub struct Stack {
    pub checkout: PathBuf,
    /// The engine port the profile points at.
    pub port: u16,
    pub log: PathBuf,
}

#[derive(Deserialize)]
struct Process {
    pid: u32,
    kind: String,
    #[serde(default)]
    ports: Vec<u16>,
    compose_file: Option<String>,
}

impl Stack {
    pub fn compose_file(&self) -> PathBuf {
        self.checkout.join(COMPOSE_FILE)
    }

    pub fn status(&self, machine: &dyn Machine) -> Result<StackStatus, String> {
        Ok(StackStatus {
            state: self.state(machine)?,
            compose_file: self.compose_file().display().to_string(),
            log: self.log.display().to_string(),
        })
    }

    fn state(&self, machine: &dyn Machine) -> Result<StackState, String> {
        let out = machine.run("iii", &["ps", "--json"], None)?;
        if !out.success {
            return Err(format!("iii ps: {}", out.text.trim()));
        }
        let processes: Vec<Process> =
            serde_json::from_str(&out.text).map_err(|e| format!("iii ps --json: {e}"))?;
        let ours = self.compose_file().display().to_string();
        if processes
            .iter()
            .any(|p| p.kind == "compose" && p.compose_file.as_deref() == Some(ours.as_str()))
        {
            return Ok(StackState::Running);
        }
        Ok(processes
            .into_iter()
            .find(|p| p.kind == "engine" && p.ports.contains(&self.port))
            .map_or(StackState::Stopped, |p| StackState::Taken {
                pid: p.pid,
                compose_file: p.compose_file,
            }))
    }

    /// Attach to a running stack, or run Doctor and start one.
    pub async fn start(
        &self,
        machine: &dyn Machine,
        engine: &dyn Engine,
    ) -> Result<StackStatus, StartRefused> {
        let failed = |message: String| StartRefused::Failed { message };
        match self.state(machine).map_err(failed)? {
            StackState::Running => return self.status(machine).map_err(failed),
            StackState::Taken { pid, compose_file } => {
                return Err(StartRefused::Taken { pid, compose_file });
            }
            StackState::Stopped => {}
        }
        let report = doctor::run(machine, engine, &self.checkout).await;
        if !report.ready() {
            return Err(StartRefused::Doctor { report });
        }
        let file = self.compose_file().display().to_string();
        machine
            .spawn(
                "iii",
                &["compose", "--up", "-f", &file],
                &self.checkout,
                &self.log,
            )
            .map_err(failed)?;
        let mut waited = Duration::ZERO;
        while waited < START_WAIT {
            if self.state(machine).map_err(failed)? == StackState::Running {
                return self.status(machine).map_err(failed);
            }
            tokio::time::sleep(START_POLL).await;
            waited += START_POLL;
        }
        Err(failed(format!(
            "the compose daemon did not appear within {}s; see {}",
            START_WAIT.as_secs(),
            self.log.display()
        )))
    }

    pub fn stop(&self, machine: &dyn Machine) -> Result<StackStatus, String> {
        let file = self.compose_file().display().to_string();
        let out = machine.run("iii", &["kill", "--compose", &file], Some(&self.checkout))?;
        if !out.success {
            return Err(format!("iii kill: {}", out.text.trim()));
        }
        self.status(machine)
    }
}

/// The port in an engine URL such as `ws://127.0.0.1:49160`.
pub fn port(url: &str) -> Option<u16> {
    url.rsplit(':').next()?.trim_end_matches('/').parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeEngine;
    use crate::machine::fake::FakeMachine;
    use bastion_protocol::Link;

    const COMPOSE: &str = "containers:\n  a:\n    worker: package://a\n    version: \"1.2.3\"\n";

    fn stack() -> Stack {
        Stack {
            checkout: "/repo".into(),
            port: 49160,
            log: "/logs/compose.log".into(),
        }
    }

    fn ps(rows: &str) -> String {
        format!("[{rows}]")
    }

    const OURS: &str =
        r#"{"pid":7,"kind":"compose","ports":[49160],"compose_file":"/repo/worker-compose.yaml"}"#;
    const OTHER: &str = r#"{"pid":9,"kind":"engine","ports":[49160],"compose_file":"/elsewhere/worker-compose.yaml"}"#;

    fn ready_machine() -> FakeMachine {
        FakeMachine::new()
            .run_ok("iii", "0.25.0")
            .run_ok("gh", "ok")
            .file("/repo/worker-compose.yaml", COMPOSE)
    }

    fn start(machine: &FakeMachine) -> Result<StackStatus, StartRefused> {
        let engine = FakeEngine::new(Link::Down);
        tauri::async_runtime::block_on(stack().start(machine, &engine))
    }

    #[test]
    fn port_comes_from_the_engine_url() {
        assert_eq!(port("ws://127.0.0.1:49160"), Some(49160));
        assert_eq!(port("ws://box"), None);
    }

    #[test]
    fn a_running_stack_is_attached_not_started_twice() {
        let machine = FakeMachine::new().run_ok("iii", &ps(OURS));
        assert_eq!(start(&machine).unwrap().state, StackState::Running);
        assert!(!machine.called("spawn"));
    }

    #[test]
    fn a_port_held_by_another_project_is_refused_with_its_owner() {
        let machine = FakeMachine::new().run_ok("iii", &ps(OTHER));
        let refused = start(&machine).unwrap_err();
        assert_eq!(
            refused,
            StartRefused::Taken {
                pid: 9,
                compose_file: Some("/elsewhere/worker-compose.yaml".into())
            }
        );
        assert!(!machine.called("spawn"));
    }

    #[test]
    fn a_failed_doctor_stops_the_start() {
        // `iii ps` answers, then `iii --version` reports the wrong iii.
        let machine = FakeMachine::new()
            .run_ok("iii", &ps(""))
            .run_ok("iii", "0.23.0")
            .run_ok("gh", "ok")
            .file("/repo/worker-compose.yaml", COMPOSE);
        assert!(matches!(start(&machine), Err(StartRefused::Doctor { .. })));
        assert!(!machine.called("spawn"));
    }

    #[test]
    fn a_stopped_stack_starts_in_the_checkout() {
        // `iii ps` (stopped), `iii --version`, then `iii ps` (running) from then on.
        let machine = FakeMachine::new()
            .run_ok("iii", &ps(""))
            .run_ok("iii", "0.25.0")
            .run_ok("iii", &ps(OURS))
            .run_ok("gh", "ok")
            .file("/repo/worker-compose.yaml", COMPOSE);
        assert_eq!(start(&machine).unwrap().state, StackState::Running);
        assert!(machine.called("spawn iii compose --up -f /repo/worker-compose.yaml"));
    }

    #[test]
    fn stop_kills_this_compose_project_only() {
        let machine = ready_machine();
        let _ = stack().stop(&machine);
        assert!(machine.called("iii kill --compose /repo/worker-compose.yaml"));
    }
}

/// Attach to this machine's running stack: `cargo test -p bastion -- --ignored --nocapture live_attach`.
#[cfg(test)]
mod live {
    use super::*;
    use crate::engine::IiiEngine;
    use crate::machine::ThisMachine;

    #[test]
    #[ignore = "needs this machine's stack"]
    fn live_attach() {
        let stack = Stack {
            checkout: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .canonicalize()
                .unwrap(),
            port: 49160,
            log: std::env::temp_dir().join("bastion-live-compose.log"),
        };
        println!("status: {:?}", stack.status(&ThisMachine));
        let engine = IiiEngine::connect("ws://127.0.0.1:49160");
        let started = tauri::async_runtime::block_on(stack.start(&ThisMachine, &engine));
        println!("start: {started:?}");
        assert_eq!(started.unwrap().state, StackState::Running);
    }
}
