//! A workspace's charter files, read for the app and edited from it (D58).
//! An edit is a commit on a new branch, made in a worktree of its own, so
//! the workspace's checkout and its base branch never change. The factory
//! reads the charter from the base commit, so an edit applies once merged.

use std::path::Path;

use bastion_core::charter::editable;
use bastion_protocol::CharterChange;

use super::factory::{FactoryCell, factory};
use crate::factory::now;
use crate::machine::Machine;

/// The files under `dir` named `file`, one level down, such as each
/// skill's `SKILL.md`.
fn one_down(machine: &dyn Machine, root: &Path, dir: &str, file: &str) -> Vec<String> {
    let names = machine.list(&root.join(dir));
    names
        .into_iter()
        .map(|n| format!("{dir}/{n}/{file}"))
        .collect()
}

/// Each charter file a person may edit, with its text, as the checkout at
/// `root` has it.
pub fn texts(machine: &dyn Machine, root: &Path) -> Vec<(String, String)> {
    let agents = machine.list(&root.join(".agents/agents"));
    let paths = ["AGENTS.md".to_string(), ".agents/settings.json".to_string()]
        .into_iter()
        .chain(agents.into_iter().map(|a| format!(".agents/agents/{a}")))
        .chain(one_down(machine, root, ".agents/skills", "SKILL.md"));
    paths
        .filter(|p| editable(p))
        .filter_map(|p| machine.read(&root.join(&p)).ok().map(|text| (p, text)))
        .collect()
}

fn git(machine: &dyn Machine, dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = machine.run("git", args, Some(dir))?;
    match out.success {
        true => Ok(()),
        false => Err(format!("git {}: {}", args.join(" "), out.text.trim())),
    }
}

/// Where an edit is made, and on which branch from which base.
pub struct Edit<'a> {
    pub root: &'a Path,
    pub base: &'a str,
    pub branch: &'a str,
    pub worktree: &'a Path,
}

/// Commit `text` to `path` on a new branch from the base, in a worktree
/// removed after. A branch with no commit is deleted again.
pub fn commit(machine: &dyn Machine, at: &Edit, path: &str, text: &str) -> Result<(), String> {
    let tree = at.worktree.display().to_string();
    let add = ["worktree", "add", "-b", at.branch, &tree, at.base];
    git(machine, at.root, &add)?;
    let made = write_and_commit(machine, at.worktree, path, text);
    let removed = git(machine, at.root, &["worktree", "remove", "--force", &tree]);
    if made.is_err() {
        git(machine, at.root, &["branch", "-D", at.branch])?;
    }
    made.and(removed)
}

fn write_and_commit(
    machine: &dyn Machine,
    tree: &Path,
    path: &str,
    text: &str,
) -> Result<(), String> {
    let file = tree.join(path);
    if let Some(dir) = file.parent() {
        machine.make_dir(dir)?;
    }
    machine.write_bytes(&file, text.as_bytes())?;
    git(machine, tree, &["add", "--", path])?;
    let message = format!("Edit {path} from Bastion");
    git(machine, tree, &["commit", "-m", &message])
}

/// Commit a person's change to a charter file on a new branch of its
/// workspace's repository. Answers with the branch.
pub async fn edit(
    cell: &FactoryCell,
    machine: &dyn Machine,
    change: CharterChange,
) -> Result<String, String> {
    if !editable(&change.path) {
        return Err(format!("{} is not a charter file", change.path));
    }
    let workspaces = factory(cell)?.store().workspaces().await?;
    let w = workspaces
        .into_iter()
        .find(|w| w.name == change.workspace)
        .ok_or_else(|| format!("no workspace named {}", change.workspace))?;
    let stamp = now();
    let branch = format!("{}charter-{stamp}", w.branch_prefix);
    let worktree = std::env::temp_dir().join(format!("bastion-charter-{stamp}"));
    let at = Edit {
        root: Path::new(&w.path),
        base: &w.branch,
        branch: &branch,
        worktree: &worktree,
    };
    commit(machine, &at, &change.path, &change.text)?;
    Ok(branch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::fake::FakeMachine;

    fn at<'a>(root: &'a Path, tree: &'a Path) -> Edit<'a> {
        Edit {
            root,
            base: "main",
            branch: "bastion/charter-1",
            worktree: tree,
        }
    }

    #[test]
    fn an_edit_is_a_commit_on_a_new_branch_in_a_worktree_removed_after() {
        let machine = FakeMachine::new().run_ok("git", "");
        let (root, tree) = (Path::new("/r"), Path::new("/tmp/w"));
        commit(&machine, &at(root, tree), "AGENTS.md", "Be brief.").unwrap();
        let calls = machine.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            [
                "git worktree add -b bastion/charter-1 /tmp/w main",
                "mkdir /tmp/w",
                "write /tmp/w/AGENTS.md",
                "git add -- AGENTS.md",
                "git commit -m Edit AGENTS.md from Bastion",
                "git worktree remove --force /tmp/w",
            ]
        );
    }

    #[test]
    fn a_failed_commit_leaves_no_branch_behind() {
        let machine = FakeMachine::new()
            .run_ok("git", "")
            .run_ok("git", "")
            .run_fails("git", "nothing to commit")
            .run_ok("git", "");
        let (root, tree) = (Path::new("/r"), Path::new("/tmp/w"));
        let made = commit(&machine, &at(root, tree), "AGENTS.md", "same");
        assert!(made.unwrap_err().contains("nothing to commit"));
        assert!(machine.called("git branch -D bastion/charter-1"));
    }
}
