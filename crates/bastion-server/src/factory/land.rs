//! The finish of a job (D41): its commits come back from the VM to its
//! branch, then merge into the workspace's base here, or stay on the branch.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bastion_core::{Event, escape};
use serde_json::{Value, json};

use super::{Factory, NS, REPO, SLOW_MS};
use crate::engine::DEFAULT_NAMESPACE;
use crate::lessons;
use crate::store::{Artifact, Record, Workspace};

impl Factory {
    /// The workflow's last step passed (D41): its commits come back to the
    /// job's branch, then merge into the workspace's base, or stay on the
    /// branch when the workspace leaves it.
    pub(super) async fn finish_job(self: &Arc<Self>, record: &Record) -> Result<(), String> {
        let (path, target) = self.bring_back(record).await?;
        if let Err(e) = self.write_lessons(record, &path).await {
            eprintln!(
                "job {}: its lessons and facts were not written: {e}",
                record.job.id
            );
        }
        self.keep_commits(record, &path, &target).await;
        let workspaces = self.store.workspaces().await?;
        let ws = workspaces
            .iter()
            .find(|w| w.name == record.job.workspace)
            .ok_or("no such workspace")?;
        if !ws.merges() {
            return self.apply(&record.job.id, Event::Landed).await;
        }
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        self.land_here(record, &worktree, &target).await
    }

    /// Write the lessons and facts signed while the journey was under way
    /// onto its own branch, so they land with its change (D61). Paths are
    /// inferred from the journey's change, leaving out the charter itself.
    /// It runs before a step that reaches past its VM, such as one opening
    /// a pull request, and again as the journey lands; a file already
    /// written as it would be is left alone. Answers whether it committed.
    pub(super) async fn write_lessons(&self, record: &Record, path: &Path) -> Result<bool, String> {
        let id = &record.job.id;
        let entries = self.store.entries_of(lessons::NOTE).await?;
        let queued: Vec<lessons::Written> = entries
            .iter()
            .map(lessons::signed)
            .filter(|s| &s.job == id && s.branch == lessons::ON_LANDING)
            .map(|s| s.written)
            .collect();
        if queued.is_empty() {
            return Ok(false);
        }
        let base = self.fork_point(record, path).await;
        let code = [&base, "HEAD", "--", ".", ":(exclude).agents"];
        let diff = self.git(path, &[&["diff"][..], &code].concat())?;
        let settings = self.machine.read(&path.join(crate::charter::SETTINGS)).ok();
        let landing = lessons::Landing {
            job: id,
            diff: &diff,
            settings,
        };
        let files = lessons::files(&queued, landing)?;
        let fresh = files
            .iter()
            .filter(|(name, text)| self.machine.read(&path.join(name)).ok().as_ref() != Some(text));
        let fresh: Vec<_> = fresh.collect();
        for (name, text) in &fresh {
            let file = path.join(name);
            if let Some(dir) = file.parent() {
                self.machine.make_dir(dir)?;
            }
            self.machine.write_bytes(&file, text.as_bytes())?;
            self.git(path, &["add", "--", name])?;
        }
        if fresh.is_empty() {
            return Ok(false);
        }
        let message = format!("What journey {id} taught: lessons and facts");
        self.git(path, &["commit", "-q", "-m", &message])?;
        Ok(true)
    }

    /// Bring the VM's commits back to the job's worktree. Returns the
    /// worktree's path and the workspace branch the change goes into.
    pub(super) async fn bring_back(&self, record: &Record) -> Result<(PathBuf, String), String> {
        let id = &record.job.id;
        let sandbox = record.runtime.sandbox.clone().ok_or("no VM")?;
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        let (path, branch) = self.worktree_path(&worktree).await?;
        let bundled = self
            .vm_exec(
                &sandbox,
                &[
                    "git",
                    "-C",
                    REPO,
                    "bundle",
                    "create",
                    "/work/out.bundle",
                    &format!("origin/{branch}..{branch}"),
                ],
            )
            .await;
        match bundled {
            // A VM revived from the worktree has no commits the worktree
            // lacks, so there is nothing to bring back.
            Err(e) if e.contains("Refusing to create empty bundle") => {
                return Ok((path, self.workspace(record).await?.branch));
            }
            Err(e) => return Err(format!("no new commits to land: {e}")),
            Ok(_) => {}
        }
        let read = self
            .engine
            .call_for(
                DEFAULT_NAMESPACE,
                "sandbox::fs::read",
                json!({ "sandbox_id": sandbox, "path": "/work/out.bundle" }),
                SLOW_MS,
            )
            .await?;
        let bytes = self.engine.read_channel(read["content"].clone()).await?;
        let bundle = self.scratch.join(format!("{id}.out.bundle"));
        self.machine.write_bytes(&bundle, &bytes)?;
        self.git(
            &path,
            &[
                "pull",
                "-q",
                "--ff-only",
                &bundle.display().to_string(),
                &branch,
            ],
        )?;
        Ok((path, self.workspace(record).await?.branch))
    }

    /// Merge the worktree into the workspace's branch here, with `worktree::land`.
    pub(super) async fn land_here(
        self: &Arc<Self>,
        record: &Record,
        worktree: &str,
        target: &str,
    ) -> Result<(), String> {
        let id = &record.job.id;
        for (trigger, landed) in [
            ("worktree::landed", true),
            ("worktree::land-blocked", false),
        ] {
            let (factory, job) = (self.clone(), id.clone());
            let binding = self.engine.bind(
                trigger,
                json!({ "worktree_id": worktree }),
                Arc::new(move |event: Value| {
                    let (factory, job) = (factory.clone(), job.clone());
                    crate::rt::spawn(async move {
                        factory.release(&job);
                        if landed {
                            let _ = factory.sweep_reverts().await;
                        }
                        let event = if landed {
                            Event::Landed
                        } else {
                            Event::LandFailed {
                                error: format!(
                                    "land blocked: {}",
                                    event["reason"].as_str().unwrap_or("no reason given")
                                ),
                            }
                        };
                        let _ = factory.apply(&job, event).await;
                    });
                    json!({})
                }),
            )?;
            self.hold(id, binding);
        }
        self.engine
            .call(
                NS,
                "worktree::land",
                json!({ "worktree_id": worktree, "target_branch": target }),
            )
            .await?;
        Ok(())
    }

    /// Keep the commits the job lands as an artifact, so an escape found in
    /// one later traces to the job.
    async fn keep_commits(&self, record: &Record, path: &Path, target: &str) {
        let range = format!("{target}..HEAD");
        let Ok(log) = self.git(path, &["log", "--format=%H %s", &range]) else {
            return;
        };
        let Some(body) = escape::commits_artifact(&log) else {
            return;
        };
        let artifact = Artifact {
            stage: record.job.stage,
            step: record.job.node.clone().unwrap_or_default(),
            body,
            created_at: super::now(),
        };
        let _ = self.store.add_artifact(&record.job.id, &artifact).await;
    }

    /// The commit the job's branch forked from its workspace's branch, or the
    /// branch's head when that cannot be read.
    pub(super) async fn fork_point(&self, record: &Record, path: &Path) -> String {
        let Ok(target) = self.workspace(record).await.map(|w| w.branch) else {
            return "HEAD".into();
        };
        self.git(path, &["merge-base", "HEAD", &target])
            .map(|sha| sha.trim().to_string())
            .ok()
            .filter(|sha| !sha.is_empty())
            .unwrap_or_else(|| "HEAD".into())
    }

    /// The job's workspace, whose branch its change goes into.
    pub(super) async fn workspace(&self, record: &Record) -> Result<Workspace, String> {
        self.store
            .workspaces()
            .await?
            .into_iter()
            .find(|w| w.name == record.job.workspace)
            .ok_or(format!("no workspace {}", record.job.workspace))
    }
}
