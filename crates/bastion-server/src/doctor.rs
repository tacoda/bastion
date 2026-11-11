//! Doctor: what must be true before the stack starts, each failed
//! check with its fix. The machine checks run with the engine down; the
//! provider check needs it up.

use std::path::Path;

use bastion_protocol::{Check, CheckState, DoctorReport, Link};
use serde_json::{Value, json};

use crate::engine::Engine;
use crate::engine::NS;
use crate::machine::Machine;

/// The `iii` this stack is pinned to.
pub const III_VERSION: &str = "0.25.0";
pub const COMPOSE_FILE: &str = "worker-compose.yaml";

pub async fn run(machine: &dyn Machine, engine: &dyn Engine, checkout: &Path) -> DoctorReport {
    let checks = vec![
        iii(machine),
        pins(machine, checkout),
        sandbox(machine),
        github(machine),
        providers(engine).await,
    ];
    DoctorReport { checks }
}

/// One check by name, ready to say how it went.
struct Probe {
    name: &'static str,
    required: bool,
}

impl Probe {
    fn required(name: &'static str) -> Self {
        Probe {
            name,
            required: true,
        }
    }

    /// A check whose failure only warns.
    fn optional(name: &'static str) -> Self {
        Probe {
            name,
            required: false,
        }
    }

    fn report(&self, state: CheckState, detail: impl Into<String>, fix: Option<String>) -> Check {
        Check {
            name: self.name.into(),
            required: self.required,
            state,
            detail: detail.into(),
            fix,
        }
    }

    fn pass(&self, detail: impl Into<String>) -> Check {
        self.report(CheckState::Pass, detail, None)
    }

    /// A failure, or only a warning when the check is optional, with its fix.
    fn fail(&self, detail: impl Into<String>, fix: impl Into<String>) -> Check {
        let state = match self.required {
            true => CheckState::Fail,
            false => CheckState::Warn,
        };
        self.report(state, detail, Some(fix.into()))
    }

    fn skipped(&self, detail: impl Into<String>) -> Check {
        self.report(CheckState::Skipped, detail, None)
    }
}

fn iii(machine: &dyn Machine) -> Check {
    let iii = Probe::required("iii");
    let fix = format!("Install iii {III_VERSION} and put `iii` on the PATH Bastion starts with.");
    match machine.run("iii", &["--version"], None) {
        Ok(out) if out.success && out.text.trim() == III_VERSION => {
            iii.pass(format!("iii {III_VERSION}"))
        }
        Ok(out) => iii.fail(format!("found iii {}", out.text.trim()), fix),
        Err(e) => iii.fail(e, fix),
    }
}

/// Every registry package in the compose file is pinned to one exact version.
fn pins(machine: &dyn Machine, checkout: &Path) -> Check {
    let pins = Probe::required("Pins");
    let path = checkout.join(COMPOSE_FILE);
    let text = match machine.read(&path) {
        Ok(text) => text,
        Err(e) => {
            return pins.fail(
                e,
                "Start the server with `--checkout` at a Bastion checkout.",
            );
        }
    };
    let loose = unpinned(&text);
    if loose.is_empty() {
        return pins.pass(format!("every package in {COMPOSE_FILE} is pinned"));
    }
    let fix = format!("Give each one `version: \"x.y.z\"` in {}.", path.display());
    pins.fail(format!("not pinned: {}", loose.join(", ")), fix)
}

/// The packages with no exact `x.y.z` version after their `worker:` line.
// ponytail: a line scan of our own compose file, not a YAML parser; use `compose::validate` once the engine is up.
fn unpinned(compose: &str) -> Vec<String> {
    let mut loose = Vec::new();
    let mut open: Option<String> = None;
    for line in compose.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("worker: package://") {
            loose.extend(open.replace(name.into()));
            continue;
        }
        let Some(version) = line.strip_prefix("version:") else {
            continue;
        };
        let version = version.trim().trim_matches('"');
        let named = open.take();
        loose.extend(
            named
                .filter(|_| !exact(version))
                .map(|n| format!("{n} ({version})")),
        );
    }
    loose.extend(open);
    loose
}

fn exact(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Sandbox VMs need Apple Silicon, or Linux with `/dev/kvm`.
fn sandbox(machine: &dyn Machine) -> Check {
    let vms = Probe::required("Sandbox VMs");
    match machine.platform() {
        ("macos", "aarch64") => vms.pass("Apple Silicon"),
        ("linux", _) if machine.exists(Path::new("/dev/kvm")) => vms.pass("Linux with /dev/kvm"),
        ("linux", _) => vms.fail(
            "no /dev/kvm",
            "Enable KVM and give your user access to /dev/kvm.",
        ),
        (os, arch) => vms.fail(
            format!("{os} on {arch}"),
            "Run Bastion on Apple Silicon, or on Linux with /dev/kvm.",
        ),
    }
}

/// `gh` signs pushes and PRs on the host (D14). Landing locally works without it.
fn github(machine: &dyn Machine) -> Check {
    let gh = Probe::optional("GitHub CLI");
    match machine.run("gh", &["auth", "status"], None) {
        Ok(out) if out.success => gh.pass("signed in"),
        Ok(_) => gh.fail("not signed in", "Run `gh auth login`."),
        Err(e) => gh.fail(e, "Install `gh`, then run `gh auth login`."),
    }
}

/// At least one model provider has a credential, as `llm-router` sees it.
async fn providers(engine: &dyn Engine) -> Check {
    if engine.link() != Link::Up {
        return Probe::required("Model providers").skipped("needs the engine; start it");
    }
    match engine.call(NS, "router::provider::list", json!({})).await {
        Ok(out) => provider_check(&out),
        Err(e) => Probe::required("Model providers").fail(e, "Check that `llm-router` is running."),
    }
}

/// Ready when any provider has a key, from Settings or the environment
/// (D34). The others are named, but none is required.
fn provider_check(out: &Value) -> Check {
    let providers = out["providers"].as_array().cloned().unwrap_or_default();
    let names = |configured: bool| -> Vec<String> {
        providers
            .iter()
            .filter(|p| (p["configured"] == true) == configured)
            .map(|p| p["display_name"].as_str().unwrap_or("?").to_string())
            .collect()
    };
    let (ready, missing) = (names(true), names(false));
    let detail = format!("ready: {}; no key: {}", list(&ready), list(&missing));
    let check = Probe::required("Model providers");
    match ready.is_empty() {
        true => check.fail(
            detail,
            "Add a key for a model provider in Settings, under Model providers.",
        ),
        false => check.pass(detail),
    }
}

fn list(names: &[String]) -> String {
    if names.is_empty() {
        "none".into()
    } else {
        names.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeEngine;
    use crate::machine::fake::FakeMachine;

    const COMPOSE: &str = "containers:\n  a:\n    worker: package://a\n    version: \"1.2.3\"\n  b:\n    worker: package://b\n    version: \"^1.0\"\n  c:\n    worker: package://c\n";

    fn by_name<'a>(report: &'a DoctorReport, name: &str) -> &'a Check {
        report.checks.iter().find(|c| c.name == name).unwrap()
    }

    fn healthy() -> FakeMachine {
        machine("0.25.0\n", true)
    }

    /// A machine whose `iii --version` prints `iii` and whose `gh` is signed in or not.
    fn machine(iii: &str, gh_signed_in: bool) -> FakeMachine {
        let machine = FakeMachine::new().run_ok("iii", iii);
        let machine = if gh_signed_in {
            machine.run_ok("gh", "Logged in")
        } else {
            machine.run_fails("gh", "not logged in")
        };
        machine.file(
            "/repo/worker-compose.yaml",
            "containers:\n  a:\n    worker: package://a\n    version: \"1.2.3\"\n",
        )
    }

    fn report(machine: &FakeMachine, engine: &FakeEngine) -> DoctorReport {
        crate::rt::block_on(run(machine, engine, Path::new("/repo")))
    }

    #[test]
    fn loose_and_missing_versions_are_named() {
        assert_eq!(
            unpinned(COMPOSE),
            vec!["b (^1.0)".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn a_healthy_machine_with_the_engine_down_is_ready() {
        let report = report(&healthy(), &FakeEngine::new(Link::Down));
        assert!(report.ready());
        assert_eq!(
            by_name(&report, "Model providers").state,
            CheckState::Skipped
        );
    }

    #[test]
    fn a_wrong_iii_blocks_the_start_with_its_fix() {
        let machine = machine("0.23.0", true);
        let report = report(&machine, &FakeEngine::new(Link::Down));
        let iii = by_name(&report, "iii");
        assert_eq!(
            (iii.state, iii.detail.as_str()),
            (CheckState::Fail, "found iii 0.23.0")
        );
        assert!(iii.fix.as_deref().unwrap().contains(III_VERSION));
        assert!(!report.ready());
    }

    #[test]
    fn gh_signed_out_only_warns() {
        let machine = machine("0.25.0", false);
        let report = report(&machine, &FakeEngine::new(Link::Down));
        assert_eq!(by_name(&report, "GitHub CLI").state, CheckState::Warn);
        assert!(report.ready());
    }

    #[test]
    fn an_intel_mac_cannot_run_sandboxes() {
        let mut machine = healthy();
        machine.platform = ("macos", "x86_64");
        assert_eq!(
            by_name(
                &report(&machine, &FakeEngine::new(Link::Down)),
                "Sandbox VMs"
            )
            .state,
            CheckState::Fail
        );
    }

    #[test]
    fn one_provider_with_a_key_is_enough() {
        let engine = FakeEngine::new(Link::Up).answer(
            "router::provider::list",
            Ok(json!({ "providers": [
                { "display_name": "Anthropic", "configured": false },
                { "display_name": "OpenAI", "configured": true },
            ]})),
        );
        let check = by_name(&report(&healthy(), &engine), "Model providers").clone();
        assert_eq!(check.state, CheckState::Pass);
        assert_eq!(check.detail, "ready: OpenAI; no key: Anthropic");
        assert_eq!(check.fix, None);
    }

    #[test]
    fn no_provider_at_all_blocks_the_start() {
        let engine = FakeEngine::new(Link::Up).answer(
            "router::provider::list",
            Ok(json!({ "providers": [{ "display_name": "Anthropic", "configured": false, "credential_env_var": "ANTHROPIC_API_KEY" }] })),
        );
        assert!(!report(&healthy(), &engine).ready());
    }
}

/// Doctor against this machine and the engine on 49160: `cargo test -p bastion-server -- --ignored --nocapture live_doctor`.
#[cfg(test)]
mod live {
    use super::*;
    use crate::engine::IiiEngine;
    use crate::machine::ThisMachine;

    #[test]
    #[ignore = "needs this machine's stack"]
    fn live_doctor() {
        crate::rt::block_on(async {
            let engine = IiiEngine::connect("ws://127.0.0.1:49160");
            for _ in 0..50 {
                if engine.link() == Link::Up {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let report = run(&ThisMachine, &engine, &checkout).await;
            for c in &report.checks {
                println!(
                    "{:?}  {}: {}  {}",
                    c.state,
                    c.name,
                    c.detail,
                    c.fix.as_deref().unwrap_or("")
                );
            }
            println!("ready: {}", report.ready());
        });
    }
}
