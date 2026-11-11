//! A job's VM made ready: booted, and the job's branch cloned into it.
//! A workspace's setup command runs first in a VM of its own, the only
//! one with the network, on the commit the job forked from, so it never
//! runs code the agent wrote. What it made reaches the job's offline VM as
//! a tarball of the repository.

use serde_json::json;

use super::{Factory, REPO, SLOW_MS};
use crate::engine::DEFAULT_NAMESPACE;
use crate::store::{Record, Workspace};

/// How long a setup command may run.
const SETUP_MS: u64 = 20 * 60_000;
/// How long any other command may run.
const EXEC_MS: u64 = 120_000;
/// The repository as a tarball, in a VM.
pub(super) const TREE: &str = "/work/repo.tgz";

impl Workspace {
    /// The environment as a VM takes it, one `K=V` per entry.
    // ponytail: a value cannot hold a space; quoting comes when one must.
    pub fn env_vars(&self) -> Result<Vec<String>, String> {
        self.env
            .split_whitespace()
            .map(|pair| match pair.split_once('=') {
                Some((key, _)) if !key.is_empty() => Ok(pair.to_string()),
                _ => Err(format!("{pair} is not KEY=VALUE")),
            })
            .collect()
    }
}

impl Factory {
    /// A VM with the workspace's environment, and the network only when
    /// `network`. Returns its id.
    pub(super) async fn create_vm(&self, network: bool, ws: &Workspace) -> Result<String, String> {
        self.create_vm_from("python", network, ws).await
    }

    /// A VM from `image`, as [`Factory::create_vm`] makes one.
    pub(super) async fn create_vm_from(
        &self,
        image: &str,
        network: bool,
        ws: &Workspace,
    ) -> Result<String, String> {
        let env = ws.env_vars()?;
        let vm =
            json!({ "image": image, "network": network, "env": env, "idle_timeout_secs": 3600 });
        let made = self
            .engine
            .call_for(DEFAULT_NAMESPACE, "sandbox::create", vm, SLOW_MS)
            .await?;
        made["sandbox_id"]
            .as_str()
            .map(String::from)
            .ok_or("sandbox::create gave no sandbox_id".into())
    }

    /// Fill the job's VM with the job's branch, set up by the workspace's
    /// setup command when it has one.
    pub(super) async fn fill(
        &self,
        ws: &Workspace,
        vm: &str,
        repo: Repo<'_>,
    ) -> Result<(), String> {
        if ws.setup.is_empty() {
            return self.clone_into(vm, &repo).await;
        }
        let tree = self.set_up(&repo, ws).await?;
        self.put(vm, TREE, tree).await?;
        self.vm_exec(vm, &["tar", "xzf", TREE, "-C", "/work"])
            .await?;
        let checkout = ["git", "-C", REPO, "checkout", "-q", "-f", repo.branch];
        self.vm_exec(vm, &checkout).await.map(|_| ())
    }

    /// Clone the job's branch into the VM's repository, committing as the
    /// agent.
    pub(super) async fn clone_into(&self, vm: &str, repo: &Repo<'_>) -> Result<(), String> {
        self.put(vm, "/work/in.bundle", repo.bundle.clone()).await?;
        let clone = [
            "git",
            "clone",
            "-q",
            "-b",
            repo.branch,
            "/work/in.bundle",
            REPO,
        ];
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
    pub(super) async fn put(&self, vm: &str, path: &str, bytes: Vec<u8>) -> Result<(), String> {
        let content = self.engine.upload(bytes).await?;
        let file = json!({ "sandbox_id": vm, "path": path, "content": content, "parents": true });
        let wrote = self
            .engine
            .call_for(DEFAULT_NAMESPACE, "sandbox::fs::write", file, SLOW_MS);
        wrote.await.map(|_| ())
    }

    /// Run `command` by a shell in the VM's repository, for up to `ms`.
    async fn sh(&self, vm: &str, command: &str, ms: u64) -> Result<(), String> {
        let line = format!("cd {REPO} && {command}");
        self.vm_exec_for(vm, &["sh", "-c", &line], ms)
            .await
            .map(|_| ())
    }

    /// The repository after the setup command ran in it at the fork point,
    /// in a VM with the network that stops after, as a tarball.
    async fn set_up(&self, repo: &Repo<'_>, ws: &Workspace) -> Result<Vec<u8>, String> {
        let vm = self.create_vm(true, ws).await?;
        let tree = self.set_up_in(&vm, repo, ws).await;
        self.stop_vm(&vm).await;
        tree.map_err(|e| format!("the setup command failed: {e}"))
    }

    async fn set_up_in(
        &self,
        vm: &str,
        repo: &Repo<'_>,
        ws: &Workspace,
    ) -> Result<Vec<u8>, String> {
        self.clone_into(vm, repo).await?;
        let at_base = ["git", "-C", REPO, "checkout", "-q", repo.base];
        self.vm_exec(vm, &at_base).await?;
        self.sh(vm, &ws.setup, SETUP_MS).await?;
        // What it made stays out of the agent's commits.
        let made = "git ls-files --others --directory --exclude-standard";
        self.sh(vm, &format!("{made} >> .git/info/exclude"), EXEC_MS)
            .await?;
        self.tree(vm).await
    }

    /// The VM's repository as a tarball, streamed back.
    pub(super) async fn tree(&self, vm: &str) -> Result<Vec<u8>, String> {
        self.vm_exec(vm, &["tar", "czf", TREE, "-C", "/work", "repo"])
            .await?;
        let read = json!({ "sandbox_id": vm, "path": TREE });
        let out = self
            .engine
            .call_for(DEFAULT_NAMESPACE, "sandbox::fs::read", read, SLOW_MS)
            .await?;
        self.engine.read_channel(out["content"].clone()).await
    }

    /// Run the workspace's cleanup command in the job's VM, if it has both.
    /// A failure is logged; the VM stops anyway.
    pub(super) async fn clean_up(&self, record: &Record) {
        let (Some(vm), Ok(ws)) = (&record.runtime.sandbox, self.workspace(record).await) else {
            return;
        };
        if ws.cleanup.is_empty() {
            return;
        }
        if let Err(e) = self.sh(vm, &ws.cleanup, EXEC_MS).await {
            eprintln!("job {}: the cleanup command failed: {e}", record.job.id);
        }
    }

    /// Stop a VM, without waiting for it to go.
    pub(super) async fn stop_vm(&self, vm: &str) {
        let stop = json!({ "sandbox_id": vm, "wait": false });
        let stopped = self
            .engine
            .call_for(DEFAULT_NAMESPACE, "sandbox::stop", stop, 60_000);
        if let Err(e) = stopped.await {
            eprintln!("sandbox {vm} did not stop: {e}");
        }
    }
}

/// A job's repository on its way into a VM.
pub(super) struct Repo<'a> {
    pub bundle: Vec<u8>,
    pub branch: &'a str,
    /// The commit the job forked from.
    pub base: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_environment_is_key_value_pairs() {
        let ws = |env: &str| Workspace {
            env: env.into(),
            ..Workspace::default()
        };
        assert_eq!(
            ws(" CI=1  A=b=c ").env_vars(),
            Ok(vec!["CI=1".into(), "A=b=c".into()])
        );
        assert_eq!(ws("").env_vars(), Ok(vec![]));
        assert!(ws("CI").env_vars().is_err());
        assert!(ws("=1").env_vars().is_err());
    }
}

/// On this machine's stack: `cargo test -p bastion-server -- --ignored --nocapture live_setup`.
#[cfg(test)]
mod live {
    use std::process::Command;
    use std::sync::Arc;

    use bastion_protocol::Link;

    use super::*;
    use crate::engine::{Engine, IiiEngine};
    use crate::machine::ThisMachine;
    use crate::store::fake::FakeStore;
    use crate::store::{Store, Workspace};

    const ONLINE: &str =
        "python -c \"import urllib.request as u; u.urlopen('https://pypi.org', timeout=10)\"";

    /// A repository with one commit on `main`, in a new directory.
    fn repo() -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("bastion-live-setup-{}", std::process::id()));
        let path = dir.join("repo");
        std::fs::create_dir_all(&path).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(&path)
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(path.join("README"), "hi\n").unwrap();
        git(&["add", "README"]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "one",
        ]);
        let base = git(&["rev-parse", "HEAD"]);
        (dir, base)
    }

    #[test]
    #[ignore = "needs this machine's stack"]
    fn live_setup() {
        let (dir, base) = repo();
        let store = Arc::new(FakeStore::default());
        store.workspaces.lock().unwrap().push(Workspace {
            name: "w".into(),
            setup: format!("{ONLINE} && echo made > made.txt && mkdir .venv && touch .venv/x"),
            env: "CI=1".into(),
            ..Workspace::default()
        });
        crate::rt::block_on(async {
            let engine = Arc::new(IiiEngine::connect("ws://127.0.0.1:49160"));
            for _ in 0..150 {
                if engine.link() == Link::Up {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let scratch = dir.join("scratch");
            let factory = Factory::new(
                engine as Arc<dyn Engine>,
                store as Arc<dyn Store>,
                Arc::new(ThisMachine),
                Arc::new(|_: bastion_protocol::Change| {}),
                scratch.to_str().unwrap().into(),
            );
            let bundle = factory.bundle("live", &dir.join("repo"), "main").unwrap();
            let ws = factory.store.workspaces().await.unwrap().remove(0);
            let vm = factory.create_vm(false, &ws).await.unwrap();
            let repo = Repo {
                bundle,
                branch: "main",
                base: &base,
            };
            let filled = factory.fill(&ws, &vm, repo).await;
            let made = factory
                .vm_exec(&vm, &["test", "-f", "/work/repo/made.txt"])
                .await;
            let status = factory
                .vm_exec(&vm, &["git", "-C", REPO, "status", "--porcelain"])
                .await;
            let env = factory
                .vm_exec(&vm, &["sh", "-c", "test \"$CI\" = 1"])
                .await;
            let online = factory.vm_exec(&vm, &["sh", "-c", ONLINE]).await;
            factory.stop_vm(&vm).await;
            let _ = std::fs::remove_dir_all(&dir);
            filled.unwrap();
            made.expect("setup's file reached the job's VM");
            assert_eq!(
                status.unwrap()["stdout"],
                "",
                "what setup made is not a change"
            );
            env.expect("the job's VM has the environment");
            assert!(online.is_err(), "the job's VM has no network");
        });
    }
}
