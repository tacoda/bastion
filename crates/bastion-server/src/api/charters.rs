//! A workspace's charter files, read for the app and edited from it (D58).
//! An edit is a commit on a new branch, made in a worktree of its own, so
//! the workspace's checkout and its base branch never change. The factory
//! reads the charter from the base commit, so an edit applies once merged.

use std::path::Path;

use bastion_core::charter::editable;
use bastion_protocol::{CharterBranch, CharterChange};

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
    let in_dir = |dir: &str| -> Vec<String> {
        let names = machine.list(&root.join(dir));
        names.into_iter().map(|n| format!("{dir}/{n}")).collect()
    };
    let paths = ["AGENTS.md".to_string(), ".agents/settings.json".to_string()]
        .into_iter()
        .chain(in_dir(".agents/agents"))
        .chain(one_down(machine, root, ".agents/skills", "SKILL.md"))
        .chain(in_dir(".agents/rules"))
        .chain(in_dir(bastion_core::corpus::DIR));
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

/// The charter branches under `prefix` in the repository at `root` not
/// yet merged into `base`: each one's name, last subject and time.
pub fn unmerged(
    machine: &dyn Machine,
    root: &Path,
    prefix: &str,
    base: &str,
) -> Vec<CharterBranch> {
    let pattern = format!("refs/heads/{prefix}charter-*");
    let format = "--format=%(refname:short)%09%(subject)%09%(committerdate:unix)";
    let args = ["for-each-ref", "--no-merged", base, format, &pattern];
    let Ok(out) = machine.run("git", &args, Some(root)) else {
        return Vec::new();
    };
    if !out.success {
        return Vec::new();
    }
    let rows = out.text.lines().filter_map(|l| {
        let mut parts = l.splitn(3, '\t');
        let (branch, subject, at) = (parts.next()?, parts.next()?, parts.next()?);
        Some(CharterBranch {
            branch: branch.into(),
            subject: subject.into(),
            at: at.trim().parse().unwrap_or_default(),
            ..CharterBranch::default()
        })
    });
    rows.collect()
}

/// Every region's charter branches waiting to be merged, newest first.
pub async fn branches(
    cell: &FactoryCell,
    machine: &dyn Machine,
) -> Result<Vec<CharterBranch>, String> {
    let workspaces = factory(cell)?.store().workspaces().await?;
    let mut all: Vec<CharterBranch> = workspaces
        .iter()
        .flat_map(|w| {
            let found = unmerged(machine, Path::new(&w.path), &w.branch_prefix, &w.branch);
            found.into_iter().map(|b| CharterBranch {
                workspace: w.name.clone(),
                ..b
            })
        })
        .collect();
    all.sort_by_key(|b| std::cmp::Reverse(b.at));
    Ok(all)
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
    fn the_texts_hold_the_regions_rules_and_corpus_but_only_markdown() {
        let machine = FakeMachine::new()
            .file("/r/AGENTS.md", "Be brief.")
            .file("/r/.agents/rules/small.md", "Keep calc small.")
            .file("/r/.agents/rules/notes.txt", "not a rule")
            .file("/r/.agents/corpus/why.md", "---\npaths: **\n---\nWhy.");
        let paths: Vec<String> = texts(&machine, Path::new("/r"))
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        assert_eq!(
            paths,
            [
                "AGENTS.md",
                ".agents/rules/small.md",
                ".agents/corpus/why.md"
            ]
        );
    }

    #[test]
    fn unmerged_charter_branches_read_with_their_subject_and_time() {
        let listed = "bastion/charter-2\tWhat j1 taught\t1700000002\nbastion/charter-1\tEdit AGENTS.md\t1700000001\n";
        let machine = FakeMachine::new().run_ok("git", listed);
        let found = unmerged(&machine, Path::new("/r"), "bastion/", "main");
        assert!(machine.called(
            "git for-each-ref --no-merged main --format=%(refname:short)%09%(subject)%09%(committerdate:unix) refs/heads/bastion/charter-*"
        ));
        assert_eq!(found.len(), 2);
        assert_eq!(
            (
                found[0].branch.as_str(),
                found[0].subject.as_str(),
                found[0].at
            ),
            ("bastion/charter-2", "What j1 taught", 1700000002)
        );
        let none = FakeMachine::new().run_fails("git", "not a repo");
        assert!(unmerged(&none, Path::new("/r"), "bastion/", "main").is_empty());
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
