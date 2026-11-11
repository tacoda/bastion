//! What a shell step reaches (D40): every script gets the job's branch,
//! its base and its GitHub repository as environment variables, and a step
//! that asks for the network runs in a VM of its own, with the network, the
//! job's commits and the vault's secrets it names.

use bastion_core::workflow::Reach;
use serde_json::{Value, json};

use super::{Factory, setup};
use crate::engine::NS;
use crate::forge;
use crate::store::Record;

/// `text` as one shell word.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

impl Factory {
    /// The line a script starts with: `BASTION_BRANCH`, the job's branch;
    /// `BASTION_BASE`, the workspace's; `BASTION_REPO`, its GitHub
    /// `owner/name`, empty without an `origin`.
    pub(super) async fn shell_env(&self, record: &Record) -> Result<String, String> {
        let ws = self.workspace(record).await?;
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        let (path, branch) = self.worktree_path(&worktree).await?;
        let repo = forge::slug(&*self.machine, &path).unwrap_or_default();
        Ok(format!(
            "export BASTION_BRANCH={} BASTION_BASE={} BASTION_REPO={}",
            quoted(&branch),
            quoted(&ws.branch),
            quoted(&repo)
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
