//! What a shell step reaches (D40): every script gets the job's branch,
//! its base and its GitHub repository as environment variables, and a step
//! that asks for the network runs in a VM of its own, with the network, the
//! job's commits and the vault's secrets it names.

use bastion_core::Event;
use bastion_core::workflow::Reach;
use serde_json::{Value, json};

use super::{Factory, REPO, setup};
use crate::engine::{DEFAULT_NAMESPACE, NS};
use crate::forge;
use crate::store::Record;

/// The longest a script may run: the sandbox stops a command at five
/// minutes, whatever it is asked for.
const SCRIPT_SECS: u64 = 300;
/// How much of a script's output a step keeps.
const TAIL: usize = 3000;

/// The end of `text`, at most `TAIL` bytes, cut on a character.
pub(super) fn tail(text: &str) -> String {
    let start = text.len().saturating_sub(TAIL);
    let start = (start..=text.len())
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(text.len());
    text[start..].to_string()
}

/// A script's exit as an event: 0 passes with its output; anything else
/// fails with it.
fn exited(code: i64, output: String) -> Event {
    match code {
        0 => Event::Ran { output },
        code => Event::StepFailed {
            detail: format!("The command failed (exit {code}):\n{output}"),
        },
    }
}

/// `text` as one shell word.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

impl Factory {
    /// A shell step (D40): its script runs in the job's VM, or in a VM of
    /// its own with the network and its secrets, stopped after.
    pub(super) async fn shell_step(&self, record: &Record) -> Event {
        let ran = async {
            let step = record.job.step().ok_or("no script to run")?;
            let script = format!("{}\n{}", self.shell_env(record).await?, step.run);
            let own = match step.reach.network {
                true => Some(self.networked_vm(record, &step.reach).await?),
                false => None,
            };
            let sandbox = own.as_deref().or(record.runtime.sandbox.as_deref());
            let ran = self.script(sandbox.ok_or("no VM")?, &script).await;
            if let Some(own) = &own {
                self.stop_vm(own).await;
            }
            ran
        };
        ran.await
            .unwrap_or_else(|detail: String| Event::StepStuck { detail })
    }

    /// Run `script` by sh in the VM's repository.
    async fn script(&self, sandbox: &str, script: &str) -> Result<Event, String> {
        let argv = ["sh", "-c", &format!("cd {REPO} && {script}")];
        let exec = json!({ "sandbox_id": sandbox, "argv": argv, "timeout_ms": SCRIPT_SECS * 1000 });
        let wait = SCRIPT_SECS * 1000 + 60_000;
        let out = self
            .engine
            .call_for(DEFAULT_NAMESPACE, "sandbox::exec", exec, wait)
            .await?;
        let printed = format!(
            "{}{}",
            out["stdout"].as_str().unwrap_or_default(),
            out["stderr"].as_str().unwrap_or_default()
        );
        Ok(exited(
            out["exit_code"].as_i64().unwrap_or(-1),
            tail(&printed),
        ))
    }

    /// The line a script starts with: `BASTION_BRANCH`, the job's branch;
    /// `BASTION_BASE`, the workspace's; `BASTION_REPO`, its GitHub
    /// `owner/name`, empty without an `origin`; `BASTION_CHECKS`, the
    /// workspace's checks.
    pub(super) async fn shell_env(&self, record: &Record) -> Result<String, String> {
        let ws = self.workspace(record).await?;
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        let (path, branch) = self.worktree_path(&worktree).await?;
        let repo = forge::slug(&*self.machine, &path).unwrap_or_default();
        Ok(format!(
            "export BASTION_BRANCH={} BASTION_BASE={} BASTION_REPO={} BASTION_CHECKS={}",
            quoted(&branch),
            quoted(&ws.branch),
            quoted(&repo),
            quoted(&ws.checks)
        ))
    }

    /// A VM with the network and the step's secrets, holding the job's
    /// commits as they are now. The caller stops it.
    pub(super) async fn networked_vm(
        &self,
        record: &Record,
        reach: &Reach,
    ) -> Result<String, String> {
        let mut env = Vec::new();
        for name in &reach.secrets {
            env.push(format!("{name}={}", self.vault_value(name).await?));
        }
        let ws = self.workspace(record).await?;
        let (path, _) = self.bring_back(record).await?;
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        let (_, branch) = self.worktree_path(&worktree).await?;
        let bundle = self.bundle(&record.job.id, &path, &branch)?;
        let base = self.fork_point(record, &path).await;
        let access = setup::Access {
            network: true,
            env: &env,
        };
        let vm = self.create_vm_from(ws.vm_image(), &ws, access).await?;
        let repo = setup::Repo {
            bundle,
            branch: &branch,
            base: &base,
        };
        if let Err(e) = self.clone_into(&vm, &repo).await {
            self.stop_vm(&vm).await;
            return Err(e);
        }
        Ok(vm)
    }

    /// A secret's value from the vault. The vault gives a value only to a
    /// worker it names as a consumer, so this process is named for the read
    /// and the consumers are put back as they were.
    pub(super) async fn vault_value(&self, name: &str) -> Result<String, String> {
        let missing = |_| format!("the vault has no secret {name}");
        let held = self
            .engine
            .call(NS, "secrets::get", json!({ "name": name }))
            .await
            .map_err(missing)?;
        let before: Vec<Value> = held["consumers"].as_array().cloned().unwrap_or_default();
        let mut with = before.clone();
        with.push(json!(crate::engine::worker_name()));
        let access = |consumers: Vec<Value>| json!({ "name": name, "consumers": consumers });
        self.engine
            .call(NS, "secrets::access", access(with))
            .await?;
        let resolved = self
            .engine
            .call(
                NS,
                "secrets::resolve",
                json!({ "ref": format!("secret://{name}") }),
            )
            .await;
        let restored = self
            .engine
            .call(NS, "secrets::access", access(before))
            .await;
        let value = resolved?["value"].as_str().map(String::from);
        restored?;
        value.ok_or_else(|| format!("the vault gave no value for {name}"))
    }
}
