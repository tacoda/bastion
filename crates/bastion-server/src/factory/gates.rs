//! A workspace's quality gates, run in the job's VM over the files its
//! change touched, before the change is pushed (F3). A failing gate gets
//! the workspace's autofix once; a gate that still fails stops the push.

use bastion_core::BASE_TAG;
use bastion_core::gates::{Gate, fill, parse};
use serde_json::json;

use super::acceptance::tail;
use super::{Factory, REPO};
use crate::engine::DEFAULT_NAMESPACE;
use crate::store::Record;

/// How long one gate may run.
const GATE_MS: u64 = 10 * 60_000;

impl Factory {
    /// Run the workspace's gates over the change; an error names each gate
    /// that still fails, with the end of what it printed.
    pub(super) async fn gate_change(&self, record: &Record) -> Result<(), String> {
        let ws = self.workspace(record).await?;
        let gates = parse(&ws.gates);
        let vm = record.runtime.sandbox.as_deref().ok_or("no VM")?;
        if gates.is_empty() {
            return Ok(());
        }
        let files = self.changed_files(vm).await?;
        if files.is_empty() {
            return Ok(());
        }
        let mut failed = self.failing(vm, &gates, &files).await;
        if !failed.is_empty() && !ws.autofix.is_empty() {
            self.autofix(vm, &fill(&ws.autofix, &files)).await;
            let again: Vec<Gate> = failed.into_iter().map(|(g, _)| g).collect();
            failed = self.failing(vm, &again, &files).await;
        }
        match failed.is_empty() {
            true => Ok(()),
            false => Err(report(&failed)),
        }
    }

    /// The files the change adds or modifies since the fork point.
    async fn changed_files(&self, vm: &str) -> Result<Vec<String>, String> {
        let diff = [
            "git",
            "-C",
            REPO,
            "diff",
            "--name-only",
            "--diff-filter=d",
            BASE_TAG,
        ];
        let out = self.vm_exec(vm, &diff).await?;
        let listed = out["stdout"].as_str().unwrap_or_default();
        Ok(listed.lines().map(String::from).collect())
    }

    /// The gates that fail, each with what it printed.
    async fn failing(&self, vm: &str, gates: &[Gate], files: &[String]) -> Vec<(Gate, String)> {
        let mut failed = Vec::new();
        for gate in gates {
            if let Err(out) = self.run_in_repo(vm, &fill(&gate.command, files)).await {
                failed.push((gate.clone(), out));
            }
        }
        failed
    }

    /// Run the autofix, then commit what it changed. A fix that fails or
    /// changes nothing leaves the gates to say so.
    async fn autofix(&self, vm: &str, command: &str) {
        let _ = self.run_in_repo(vm, command).await;
        let commit = "git add -A && git commit -q -m 'Apply the workspace autofix'";
        let _ = self.run_in_repo(vm, commit).await;
    }

    /// Run `command` by a shell in the VM's repository. An error is what it
    /// printed, stdout and stderr, when it exits other than 0.
    async fn run_in_repo(&self, vm: &str, command: &str) -> Result<(), String> {
        let line = format!("cd {REPO} && {command}");
        let exec = json!({ "sandbox_id": vm, "argv": ["sh", "-c", line], "timeout_ms": GATE_MS });
        let out = self
            .engine
            .call_for(DEFAULT_NAMESPACE, "sandbox::exec", exec, GATE_MS + 60_000)
            .await?;
        match out["exit_code"].as_i64() {
            Some(0) => Ok(()),
            code => Err(tail(&format!(
                "exit {}\n{}{}",
                code.unwrap_or(-1),
                out["stdout"].as_str().unwrap_or_default(),
                out["stderr"].as_str().unwrap_or_default()
            ))),
        }
    }
}

/// Why the push stopped: each failing gate and what it printed.
fn report(failed: &[(Gate, String)]) -> String {
    let each: Vec<String> = failed
        .iter()
        .map(|(g, out)| format!("{} ({}):\n{}", g.name, g.command, out.trim()))
        .collect();
    format!("The gates failed before the push.\n\n{}", each.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_push_names_each_gate_and_what_it_printed() {
        let lint = Gate {
            name: "lint".into(),
            command: "ruff check {files}".into(),
        };
        let said = report(&[(lint, "exit 1\ncalc.py:3: E501 line too long\n".into())]);
        assert!(said.starts_with("The gates failed before the push."));
        assert!(said.contains("lint (ruff check {files}):\nexit 1\ncalc.py:3: E501"));
    }
}
