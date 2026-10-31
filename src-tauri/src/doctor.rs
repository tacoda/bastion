//! Doctor (PLAN.md A1): what must be true before the stack starts, each failed
//! check with its fix. The machine checks run with the engine down; the
//! provider check needs it up.

use std::path::Path;

use bastion_protocol::{Check, CheckState, DoctorReport, Link};
use serde_json::{Value, json};

use crate::engine::Engine;
use crate::machine::Machine;

/// The `iii` this stack is pinned to (PLAN.md A0, `docs/levels.md`).
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

fn check(
    name: &str,
    required: bool,
    state: CheckState,
    detail: impl Into<String>,
    fix: Option<String>,
) -> Check {
    Check {
        name: name.into(),
        required,
        state,
        detail: detail.into(),
        fix,
    }
}

fn failed(required: bool) -> CheckState {
    if required {
        CheckState::Fail
    } else {
        CheckState::Warn
    }
}

fn iii(machine: &dyn Machine) -> Check {
    let fix = Some(format!(
        "Install iii {III_VERSION} and put `iii` on the PATH Bastion starts with."
    ));
    match machine.run("iii", &["--version"], None) {
        Ok(out) if out.success && out.text.trim() == III_VERSION => check(
            "iii",
            true,
            CheckState::Pass,
            format!("iii {III_VERSION}"),
            None,
        ),
        Ok(out) => check(
            "iii",
            true,
            CheckState::Fail,
            format!("found iii {}", out.text.trim()),
            fix,
        ),
        Err(e) => check("iii", true, CheckState::Fail, e, fix),
    }
}

/// Every registry package in the compose file is pinned to one exact version.
fn pins(machine: &dyn Machine, checkout: &Path) -> Check {
    let path = checkout.join(COMPOSE_FILE);
    let text = match machine.read(&path) {
        Ok(text) => text,
        Err(e) => {
            let fix = Some("Point the profile's `checkout` at a Bastion checkout.".into());
            return check("Pins", true, CheckState::Fail, e, fix);
        }
    };
    let loose = unpinned(&text);
    if loose.is_empty() {
        check(
            "Pins",
            true,
            CheckState::Pass,
            format!("every package in {COMPOSE_FILE} is pinned"),
            None,
        )
    } else {
        let fix = Some(format!(
            "Give each one `version: \"x.y.z\"` in {}.",
            path.display()
        ));
        check(
            "Pins",
            true,
            CheckState::Fail,
            format!("not pinned: {}", loose.join(", ")),
            fix,
        )
    }
}

/// The packages with no exact `x.y.z` version after their `worker:` line.
// ponytail: a line scan of our own compose file, not a YAML parser; use `compose::validate` once the engine is up.
fn unpinned(compose: &str) -> Vec<String> {
    let mut loose = Vec::new();
    let mut open: Option<String> = None;
    for line in compose.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("worker: package://") {
            loose.extend(open.replace(name.into()));
        } else if let Some(version) = line.strip_prefix("version:") {
            let version = version.trim().trim_matches('"');
            if let Some(name) = open.take()
                && !exact(version)
            {
                loose.push(format!("{name} ({version})"));
            }
        }
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

/// Sandbox VMs need Apple Silicon, or Linux with `/dev/kvm` (PLAN.md §2).
fn sandbox(machine: &dyn Machine) -> Check {
    match machine.platform() {
        ("macos", "aarch64") => check("Sandbox VMs", true, CheckState::Pass, "Apple Silicon", None),
        ("linux", _) if machine.exists(Path::new("/dev/kvm")) => check(
            "Sandbox VMs",
            true,
            CheckState::Pass,
            "Linux with /dev/kvm",
            None,
        ),
        ("linux", _) => check(
            "Sandbox VMs",
            true,
            CheckState::Fail,
            "no /dev/kvm",
            Some("Enable KVM and give your user access to /dev/kvm.".into()),
        ),
        (os, arch) => check(
            "Sandbox VMs",
            true,
            CheckState::Fail,
            format!("{os} on {arch}"),
            Some("Run Bastion on Apple Silicon, or on Linux with /dev/kvm.".into()),
        ),
    }
}

/// `gh` signs pushes and PRs on the host (D14). Landing locally works without it.
fn github(machine: &dyn Machine) -> Check {
    let fix = Some("Run `gh auth login`.".into());
    match machine.run("gh", &["auth", "status"], None) {
        Ok(out) if out.success => check("GitHub CLI", false, CheckState::Pass, "signed in", None),
        Ok(_) => check("GitHub CLI", false, failed(false), "not signed in", fix),
        Err(e) => check(
            "GitHub CLI",
            false,
            failed(false),
            e,
            Some("Install `gh`, then run `gh auth login`.".into()),
        ),
    }
}

/// At least one model provider has a credential, as `llm-router` sees it.
async fn providers(engine: &dyn Engine) -> Check {
    if engine.link() != Link::Up {
        return check(
            "Model providers",
            true,
            CheckState::Skipped,
            "needs the engine; start the stack",
            None,
        );
    }
    match engine
        .call("bastion", "router::provider::list", json!({}))
        .await
    {
        Ok(out) => provider_check(&out),
        Err(e) => check(
            "Model providers",
            true,
            CheckState::Fail,
            e,
            Some("Check that `llm-router` is running.".into()),
        ),
    }
}

fn provider_check(out: &Value) -> Check {
    let providers = out["providers"].as_array().cloned().unwrap_or_default();
    let name = |p: &Value| p["display_name"].as_str().unwrap_or("?").to_string();
    let ready: Vec<String> = providers
        .iter()
        .filter(|p| p["configured"] == true)
        .map(name)
        .collect();
    let missing: Vec<&Value> = providers
        .iter()
        .filter(|p| p["configured"] != true)
        .collect();
    let fix = (!missing.is_empty()).then(|| {
        let vars: Vec<String> = missing
            .iter()
            .map(|p| {
                format!(
                    "{} for {}",
                    p["credential_env_var"].as_str().unwrap_or("?"),
                    name(p)
                )
            })
            .collect();
        format!(
            "Set {} in the environment you start Bastion from, then restart the stack.",
            vars.join(", ")
        )
    });
    let missing_names: Vec<String> = missing.into_iter().map(name).collect();
    let detail = format!(
        "ready: {}; no credential: {}",
        list(&ready),
        list(&missing_names)
    );
    let state = match (ready.is_empty(), fix.is_some()) {
        (true, _) => CheckState::Fail,
        (false, true) => CheckState::Warn,
        (false, false) => CheckState::Pass,
    };
    check("Model providers", true, state, detail, fix)
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
        FakeMachine::new()
            .run_ok("iii", "0.25.0\n")
            .run_ok("gh", "Logged in")
            .file(
                "/repo/worker-compose.yaml",
                "containers:\n  a:\n    worker: package://a\n    version: \"1.2.3\"\n",
            )
    }

    fn report(machine: &FakeMachine, engine: &FakeEngine) -> DoctorReport {
        tauri::async_runtime::block_on(run(machine, engine, Path::new("/repo")))
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
        let machine = healthy().run_ok("iii", "0.23.0");
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
        let machine = healthy().run_fails("gh", "not logged in");
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
    fn a_provider_with_no_credential_is_named_with_its_variable() {
        let engine = FakeEngine::new(Link::Up).answer(
            "router::provider::list",
            Ok(json!({ "providers": [
                { "display_name": "Anthropic", "configured": false, "credential_env_var": "ANTHROPIC_API_KEY" },
                { "display_name": "OpenAI", "configured": true, "credential_env_var": "OPENAI_API_KEY" },
            ]})),
        );
        let check = by_name(&report(&healthy(), &engine), "Model providers").clone();
        assert_eq!(check.state, CheckState::Warn);
        assert_eq!(check.detail, "ready: OpenAI; no credential: Anthropic");
        assert!(
            check
                .fix
                .unwrap()
                .contains("ANTHROPIC_API_KEY for Anthropic")
        );
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

/// Doctor against this machine and the engine on 49160: `cargo test -p bastion -- --ignored --nocapture live_doctor`.
#[cfg(test)]
mod live {
    use super::*;
    use crate::engine::IiiEngine;
    use crate::machine::ThisMachine;

    #[test]
    #[ignore = "needs this machine's stack"]
    fn live_doctor() {
        tauri::async_runtime::block_on(async {
            let engine = IiiEngine::connect("ws://127.0.0.1:49160");
            for _ in 0..50 {
                if engine.link() == Link::Up {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
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
