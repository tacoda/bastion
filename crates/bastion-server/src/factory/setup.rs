//! A job's VM made ready: booted, and the job's branch cloned into it.

use std::path::Path;

use serde_json::json;

use super::{Factory, REPO, SLOW_MS};
use crate::engine::DEFAULT_NAMESPACE;

impl Factory {
    /// A VM with no network, for a job's turns. Returns its id.
    pub(super) async fn create_vm(&self) -> Result<String, String> {
        let made = self
            .engine
            .call_for(
                DEFAULT_NAMESPACE,
                "sandbox::create",
                json!({ "image": "python", "network": false, "idle_timeout_secs": 3600 }),
                SLOW_MS,
            )
            .await?;
        made["sandbox_id"]
            .as_str()
            .map(String::from)
            .ok_or("sandbox::create gave no sandbox_id".into())
    }

    /// The worktree at `path` as a git bundle of `branch`.
    pub(super) fn bundle(&self, job: &str, path: &Path, branch: &str) -> Result<Vec<u8>, String> {
        self.machine.make_dir(&self.scratch)?;
        let bundle = self.scratch.join(format!("{job}.in.bundle"));
        let file = bundle.display().to_string();
        self.git(path, &["bundle", "create", &file, "HEAD", branch])?;
        self.machine.read_bytes(&bundle)
    }

    /// Clone `branch` from `bundle` into the VM's repository, committing as
    /// the agent.
    pub(super) async fn clone_into(
        &self,
        vm: &str,
        bundle: Vec<u8>,
        branch: &str,
    ) -> Result<(), String> {
        self.put(vm, "/work/in.bundle", bundle).await?;
        let clone = ["git", "clone", "-q", "-b", branch, "/work/in.bundle", REPO];
        self.vm_exec(vm, &clone).await?;
        let name = ["git", "-C", REPO, "config", "user.name", "Bastion agent"];
        self.vm_exec(vm, &name).await?;
        let email = [
            "git",
            "-C",
            REPO,
            "config",
            "user.email",
            "agent@bastion.local",
        ];
        self.vm_exec(vm, &email).await.map(|_| ())
    }

    /// Write `bytes` to `path` in the VM, streamed, so size has no limit.
    async fn put(&self, vm: &str, path: &str, bytes: Vec<u8>) -> Result<(), String> {
        let content = self.engine.upload(bytes).await?;
        let file = json!({ "sandbox_id": vm, "path": path, "content": content, "parents": true });
        let wrote = self
            .engine
            .call_for(DEFAULT_NAMESPACE, "sandbox::fs::write", file, SLOW_MS);
        wrote.await.map(|_| ())
    }
}
