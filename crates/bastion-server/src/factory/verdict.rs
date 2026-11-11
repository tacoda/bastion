//! A machine verdict on promotion (F4). A repository that commits a
//! `verify.yaml` contract has each change judged by opengantry's
//! `gantry-verify` before it is pushed or landed. The verify runs the
//! contract's checks, which run the agent's code, so it runs in a VM of its
//! own with no network, never on the host. It reads the contract from the
//! fork point, so a change cannot weaken the contract it is judged by.

use std::path::{Path, PathBuf};

use bastion_core::gates::parse;
use bastion_core::{BASE_TAG, eval};

use super::setup::TREE;
use super::{Factory, REPO};
use crate::store::Record;

/// The opengantry verify package, pinned.
const KIT: &str = "@jeger-ai/opengantry-verify@4.0.2";
/// Where the package's bin sits once its tarball is unpacked in a VM.
const VERIFY: &str = "/work/kit/node_modules/@jeger-ai/opengantry-verify/dist/cli.js";
/// The contract a repository commits to ask for a verdict.
const CONTRACT: &str = "verify.yaml";

impl Factory {
    /// The verdict on the job's change, when its repository has a contract
    /// at the fork point; an error says why the change may not go on.
    pub(super) async fn verdict(&self, record: &Record) -> Result<bool, String> {
        let vm = record.runtime.sandbox.as_deref().ok_or("no VM")?;
        let at_base = format!("{BASE_TAG}:{CONTRACT}");
        let contract = self
            .vm_exec(vm, &["git", "-C", REPO, "show", &at_base])
            .await;
        let empty = |out: &serde_json::Value| out["stdout"].as_str().is_none_or(str::is_empty);
        if contract.as_ref().is_ok_and(|c| !empty(c)) {
            return self.judge(record, vm).await.map(|()| true);
        }
        Ok(false)
    }

    /// A trial's contract as a grader (F6): the workspace's gates over the
    /// change, with no autofix, then opengantry's verdict. None when the
    /// workspace has no gates and the repository no contract.
    pub(super) async fn contract_holds(&self, record: &Record) -> Result<Option<bool>, String> {
        let vm = record.runtime.sandbox.as_deref().ok_or("no VM")?;
        let gates = parse(&self.workspace(record).await?.gates);
        let files = self.changed_files(vm).await?;
        let failed = match files.is_empty() {
            true => Vec::new(),
            false => self.failing(vm, &gates, &files).await,
        };
        let judged = self.verdict(record).await;
        Ok(eval::contract(!gates.is_empty(), failed.is_empty(), judged))
    }

    /// Judge the change in a VM of its own, from the node image, offline.
    async fn judge(&self, record: &Record, vm: &str) -> Result<(), String> {
        let tree = self.tree(vm).await?;
        let kit = self.verify_kit()?;
        let ws = self.workspace(record).await?;
        let judge = self.create_vm_from("node", false, &ws).await?;
        let judged = self.judge_in(&judge, tree, kit).await;
        self.stop_vm(&judge).await;
        judged.map_err(|e| format!("opengantry refused the change ({CONTRACT}):\n{e}"))
    }

    /// Unpack the tree and the verify package in `vm`, and run the verify.
    async fn judge_in(&self, vm: &str, tree: Vec<u8>, kit: Vec<u8>) -> Result<(), String> {
        self.put(vm, TREE, tree).await?;
        self.vm_exec(vm, &["tar", "xzf", TREE, "-C", "/work"])
            .await?;
        self.put(vm, "/work/kit.tgz", kit).await?;
        self.vm_exec(vm, &["mkdir", "-p", "/work/kit"]).await?;
        self.vm_exec(vm, &["tar", "xzf", "/work/kit.tgz", "-C", "/work/kit"])
            .await?;
        self.run_in_repo(vm, &format!("node {VERIFY} --base {BASE_TAG}"))
            .await
    }

    /// The verify package and what it needs, as a tarball, installed on the
    /// host once with no install scripts.
    fn verify_kit(&self) -> Result<Vec<u8>, String> {
        let dir = kit_dir(&self.scratch);
        let tarball = dir.join("kit.tgz");
        if self.machine.exists(&tarball) {
            return self.machine.read_bytes(&tarball);
        }
        self.machine.make_dir(&dir)?;
        let at = dir.display().to_string();
        let install = [
            "install",
            "--prefix",
            &at,
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            KIT,
        ];
        self.host("npm", &install, &dir)?;
        let file = tarball.display().to_string();
        self.host("tar", &["czf", &file, "-C", &at, "node_modules"], &dir)?;
        self.machine.read_bytes(&tarball)
    }

    fn host(&self, program: &str, args: &[&str], dir: &Path) -> Result<(), String> {
        let out = self.machine.run(program, args, Some(dir))?;
        match out.success {
            true => Ok(()),
            false => Err(format!("{program} {}: {}", args[0], out.text.trim())),
        }
    }
}

/// Where the verify package is kept, by its pinned version.
fn kit_dir(scratch: &Path) -> PathBuf {
    let version = KIT.rsplit('@').next().unwrap_or_default();
    scratch.join(format!("opengantry-verify-{version}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kit_is_kept_by_its_pinned_version() {
        let dir = kit_dir(Path::new("/scratch"));
        assert_eq!(dir, Path::new("/scratch/opengantry-verify-4.0.2"));
    }
}
