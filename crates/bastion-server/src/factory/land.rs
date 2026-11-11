//! Landing a job: its commits come back from the VM, then merge here or
//! go up as a pull request whose CI the factory watches.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bastion_core::settings::Forge;
use bastion_core::{Effect, Event, Stage, escape};
use serde_json::{Value, json};

use super::{CI_POLLS, Factory, NS, REPO, SLOW_MS};
use crate::engine::DEFAULT_NAMESPACE;
use crate::forge;
use crate::settings_store;
use crate::store::{Artifact, Record, Workspace};

impl Factory {
    /// The workspace's gates pass, opengantry's verdict allows the change,
    /// the VM's commits come back to the worktree, then `worktree::land` merges them, and its trigger reports
    /// the outcome.
    pub(super) async fn land(self: &Arc<Self>, record: &Record) -> Result<(), String> {
        self.gate_change(record).await?;
        self.verdict(record).await?;
        let (path, target) = self.bring_back(record).await?;
        self.keep_commits(record, &path, &target).await;
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        match self.forge_for(&path).await? {
            Forge::Github => {
                let url = self.pull_request(record, &path, &target).await?;
                self.apply(&record.job.id, Event::PrOpened { url }).await
            }
            Forge::Local => self.land_here(record, &worktree, &target).await,
        }
    }

    /// Ship: bring the commits back, then push them and open the
    /// pull request, or, with the local forge, go on with them in the worktree.
    pub(super) async fn ship(&self, record: &Record) -> Result<Event, String> {
        self.gate_change(record).await?;
        self.verdict(record).await?;
        let (path, target) = self.bring_back(record).await?;
        self.keep_commits(record, &path, &target).await;
        Ok(match self.forge_for(&path).await? {
            Forge::Github => Event::PrOpened {
                url: self.pull_request(record, &path, &target).await?,
            },
            Forge::Local => Event::StepPassed,
        })
    }

    /// The server's forge, except that a repository with no `origin`
    /// remote has nowhere to push, so its change lands here.
    async fn forge_for(&self, path: &Path) -> Result<Forge, String> {
        let forge = settings_store::current(&*self.store).await?.forge;
        match forge::has_origin(&*self.machine, path) {
            true => Ok(forge),
            false => Ok(Forge::Local),
        }
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

    /// Push the job's branch and open its pull request, or reuse the one a
    /// fix turn pushes to. Returns the pull request's URL.
    pub(super) async fn pull_request(
        &self,
        record: &Record,
        path: &Path,
        base: &str,
    ) -> Result<String, String> {
        let job = &record.job;
        let slug = forge::slug(&*self.machine, path)?;
        // ponytail: a prefix changed while the PR is open sends a fix to a new branch; keep the pushed branch on the job if that bites.
        // The branch a job's change goes up on: its workspace's prefix, then its id.
        let pr = self.pr_text(record, base).await?;
        forge::push(&*self.machine, path, &pr.head)?;
        let url = match &job.pr {
            Some(url) => url.clone(),
            None => {
                forge::GitHub {
                    engine: &*self.engine,
                    slug,
                }
                .open(&pr)
                .await?
            }
        };
        Ok(url)
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

    /// The repository a job's pull request lives in, from its worktree's origin.
    pub(super) async fn slug(&self, record: &Record) -> Result<String, String> {
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        let (path, _) = self.worktree_path(&worktree).await?;
        forge::slug(&*self.machine, &path)
    }

    /// Poll the pull request's checks until they pass or fail.
    // ponytail: polls every 20 s for 30 min; the github worker's PR webhooks replace this once a tunnel is set up.
    pub(super) fn watch_ci(self: &Arc<Self>, record: &Record) {
        let (factory, record) = (self.clone(), record.clone());
        crate::rt::spawn(async move {
            let id = record.job.id.clone();
            let (Some(pr), Ok(slug)) = (record.job.pr.clone(), factory.slug(&record).await) else {
                let error = "the pull request or its repository is unknown".into();
                let _ = factory.apply(&id, Event::LandFailed { error }).await;
                return;
            };
            for _ in 0..CI_POLLS {
                tokio::time::sleep(Duration::from_secs(20)).await;
                let hub = forge::GitHub {
                    engine: &*factory.engine,
                    slug: slug.clone(),
                };
                let event = match hub.checks(&pr).await {
                    Ok(forge::Checks::Pending) => continue,
                    Ok(forge::Checks::Passed) => {
                        factory.keep_preview(&record, &hub).await;
                        Event::CiPassed
                    }
                    Ok(forge::Checks::Failed(log)) => Event::CiFailed { log },
                    Err(error) => Event::LandFailed { error },
                };
                let _ = factory.apply(&id, event).await;
                return;
            }
            let error = "CI did not finish within 30 minutes".into();
            let _ = factory.apply(&id, Event::LandFailed { error }).await;
        });
    }

    /// Keep the preview a host deployed for the pull request's branch, so a
    /// reviewer can try the change (F7). A branch nothing deploys has none.
    pub(super) async fn keep_preview(&self, record: &Record, hub: &forge::GitHub<'_>) {
        let Ok(ws) = self.workspace(record).await else {
            return;
        };
        let head = format!("ref={}{}", ws.branch_prefix, record.job.id);
        let Ok(Some(forge::Deployment { url: Some(url), .. })) = hub.deployment(&head).await else {
            return;
        };
        let artifact = Artifact {
            stage: record.job.stage,
            body: format!("Preview: <{url}>"),
            created_at: super::now(),
        };
        let _ = self.store.add_artifact(&record.job.id, &artifact).await;
    }

    /// Watch CI on the job's pull request, or merge it.
    pub(super) async fn forge_effect(
        self: &Arc<Self>,
        record: &Record,
        effect: Effect,
    ) -> Option<String> {
        if effect == Effect::WatchCi {
            self.watch_ci(record);
            return None;
        }
        // With the local forge there is no pull request: the merge lands here.
        if record.job.pr.is_none() {
            return self.merge_here(record).await.err();
        }
        let event = match self.merge(record).await {
            Ok(()) => Event::Landed,
            Err(error) => Event::LandFailed { error },
        };
        self.apply(&record.job.id, event).await.err()
    }

    /// Land the worktree into the workspace's branch, as the short pipeline does.
    pub(super) async fn merge_here(self: &Arc<Self>, record: &Record) -> Result<(), String> {
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        let target = self.workspace(record).await?.branch;
        self.land_here(record, &worktree, &target).await
    }

    /// Keep the commits the job lands as an artifact, so an escape found in
    /// one later traces to the job.
    // ponytail: a pull request merged by squash or rebase lands new ids; trace
    // by the merge commit when that matters.
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

    /// The pull request's text as the job stands now, once it passes the
    /// publishing gate: nothing leaves before it does.
    async fn pr_text(&self, record: &Record, base: &str) -> Result<forge::PullRequest, String> {
        let head = self.workspace(record).await?.branch_prefix + &record.job.id;
        let pr = forge::describe(&record.job, base, &head, &self.notes(record).await?);
        forge::publishable(&pr)?;
        Ok(pr)
    }

    /// What the job's later steps wrote and what a person waived (F7).
    async fn notes(&self, record: &Record) -> Result<forge::Notes, String> {
        let id = &record.job.id;
        let artifacts = self.store.artifacts(id).await?;
        let last = |stage| {
            let found = artifacts.iter().rev().find(|a| a.stage == stage);
            found.map(|a| a.body.clone())
        };
        let decided = self.store.entries_of("decide").await?.into_iter();
        let waived = decided
            .filter(|e| e.record.subject == *id && e.record.detail.contains(", waiving: "))
            .map(|e| e.record.detail)
            .collect();
        Ok(forge::Notes {
            briefing: last(Stage::Comprehend),
            proof: last(Stage::Prove),
            waived,
            issue: record.runtime.issue.clone(),
        })
    }

    /// Say the pull request's text again with the proof and the briefing
    /// that came after it opened, merge it, then bring the workspace's branch
    /// up to date.
    pub(super) async fn merge(&self, record: &Record) -> Result<(), String> {
        let url = record.job.pr.clone().ok_or("the job has no pull request")?;
        let ws = self.workspace(record).await?;
        let pr = self.pr_text(record, &ws.branch).await?;
        let hub = forge::GitHub {
            engine: &*self.engine,
            slug: self.slug(record).await?,
        };
        hub.edit(&url, &pr.body).await?;
        hub.merge(&url).await?;
        // Best effort: a workspace with local changes keeps them and stays behind.
        let pull = ["pull", "-q", "--ff-only", "origin", &ws.branch];
        let _ = self.machine.run("git", &pull, Some(Path::new(&ws.path)));
        Ok(())
    }
}
