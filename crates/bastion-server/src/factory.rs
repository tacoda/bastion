//! The factory loop: load a job, step it with `bastion-core`,
//! save it with its `rev`, run the effects on iii, and tell the clients. Every
//! factory rule lives in the core; this file only does what an effect says.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use bastion_core::govern::Enforcement;
use bastion_core::review;
use bastion_core::{
    BASE_TAG, Dials, Effect, Event, Job, Outcome, Pipeline, Stage, admit, outcome, step,
};
use serde_json::{Value, json};

use crate::audit_log::{self, AuditLog};
use crate::charter;
use crate::engine::{DEFAULT_NAMESPACE, Engine};
use crate::eval_store::{Eval, EvalRun};
use crate::evals;
use crate::gate::Gate;
use crate::machine::Machine;
use crate::settings_store;
use crate::store::{Artifact, Record, Runtime, STALE, Store};
use crate::workflow_store;

const NS: &str = "bastion";
/// At most this many jobs hold a VM (iii-sandbox's `max_concurrent_sandboxes`).
const MAX_VMS: usize = 8;
const MAX_TURN_STEPS: u64 = 40;
/// The tools a turn may call. A pre-trigger hook pins each to the job's VM.
const TOOLS: [&str; 4] = [
    "sandbox::exec",
    "sandbox::fs::read",
    "sandbox::fs::write",
    "sandbox::fs::ls",
];
const REPO: &str = "/work/repo";

/// Where a request came from, and the pipeline it runs.
#[derive(Default)]
pub struct Submission {
    /// The tracker issue it came from, as its URL.
    pub issue: Option<String>,
    pub pipeline: Pipeline,
    /// The person asking (D31).
    pub requester: Option<String>,
    /// The workflow to run, by id (D35); none runs `pipeline`.
    pub workflow: Option<String>,
}
/// The VM tag a read-only turn's tree is put back to.
const BEFORE_TURN: &str = "bastion-before-turn";
const SLOW_MS: u64 = 180_000;
/// How a turn reaches its VM. The pre-trigger hook replaces `sandbox_id`, so
/// the turn passes any value; without one some calls fail their schema.
const TOOL_GUIDE: &str = "\
Your tools run in this job's VM. The repository is at /work/repo.
Call sandbox::exec with {\"sandbox_id\": \"job\", \"argv\": [\"git\", \"-C\", \"/work/repo\", \"status\"]},
and sandbox::fs::read, sandbox::fs::write and sandbox::fs::ls with {\"sandbox_id\": \"job\", \"path\": ...}.
Bastion points \"job\" at your VM. Only committed work leaves the VM.
Run each program directly as an argv, never through bash -lc, and pass
\"timeout_ms\": 60000 so a hung command fails instead of stalling the turn.
Run Python tests with [\"python3\", \"-m\", \"pytest\", \"-q\", \"/work/repo\"].
Your reply ends your turn. Keep calling tools until the work is done and
committed, and reply only then. Do not describe a next step; take it.";
const SAVE_TRIES: usize = 3;
/// How many times CI is polled, 20 s apart, before the land gives up.
const CI_POLLS: u32 = 90;

pub type OnChange = Arc<dyn Fn(&str) + Send + Sync>;
/// A condition on a job, checked in the same save as a change.
pub type Holds = Arc<dyn Fn(&Record) -> bool + Send + Sync>;
type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

pub struct Factory {
    engine: Arc<dyn Engine>,
    store: Arc<dyn Store>,
    machine: Arc<dyn Machine>,
    on_change: OnChange,
    /// Where bundles pass through on their way between worktree and VM.
    scratch: PathBuf,
    /// The trigger bindings each running turn or land holds, by job.
    bindings: Mutex<HashMap<String, Vec<String>>>,
    /// The turns being finished now, so a turn that ends twice (its event
    /// and its cap, or its event and a resume) is finished once.
    finishing: Mutex<std::collections::HashSet<String>>,
    gate: Arc<Gate>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

/// Where the factory keeps files: `scratch` for what it can lose, such as
/// bundles, and `data` for what it must keep, such as audit records not yet
/// written. A test gives no `data`.
pub struct Dirs {
    pub scratch: PathBuf,
    pub data: Option<PathBuf>,
}

impl From<&str> for Dirs {
    fn from(scratch: &str) -> Self {
        Dirs {
            scratch: scratch.into(),
            data: None,
        }
    }
}

impl Factory {
    pub fn new(
        engine: Arc<dyn Engine>,
        store: Arc<dyn Store>,
        machine: Arc<dyn Machine>,
        on_change: OnChange,
        dirs: Dirs,
    ) -> Arc<Self> {
        let audit = audit_log::key(&*machine).map(|key| {
            let (store, limit, dir) = (store.clone(), audit_log::WRITE_LIMIT, dirs.data);
            AuditLog::start(audit_log::Writer {
                store,
                key,
                limit,
                dir,
            })
        });
        if let Err(e) = &audit {
            eprintln!("the audit has no key, so the gate refuses every call: {e}");
        }
        let gate = Gate::new(store.clone(), engine.clone(), on_change.clone(), audit);
        Arc::new(Self {
            engine,
            store,
            machine,
            on_change,
            scratch: dirs.scratch,
            bindings: Mutex::new(HashMap::new()),
            finishing: Mutex::default(),
            gate,
        })
    }

    pub fn store(&self) -> &Arc<dyn Store> {
        &self.store
    }

    pub fn engine(&self) -> &Arc<dyn Engine> {
        &self.engine
    }

    pub fn gate(&self) -> &Arc<Gate> {
        &self.gate
    }

    /// Put every `sandbox::*` call a turn makes through the gate, which pins
    /// it to the job's VM, then resume.
    pub fn start(self: &Arc<Self>) -> Result<(), String> {
        let gate = self.gate.clone();
        self.engine.bind(
            "harness::hook::pre-trigger",
            json!({ "functions": ["sandbox::*"], "on_error": "fail_closed", "timeout_ms": 5000 }),
            Arc::new(move |envelope: Value| gate.judge(&envelope)),
        )?;
        let factory = self.clone();
        crate::rt::spawn(async move {
            if let Err(e) = factory.gate.reload().await {
                eprintln!("the gate has no policy, so it refuses every call: {e}");
            }
            factory.keep_resuming();
        });
        Ok(())
    }

    #[cfg(test)]
    pub async fn submit(
        self: &Arc<Self>,
        workspace: &str,
        request: &str,
    ) -> Result<String, String> {
        self.submit_with(workspace, request, Submission::default())
            .await
    }

    /// Submit a request, with the tracker issue it came from if any.
    pub async fn submit_with(
        self: &Arc<Self>,
        workspace: &str,
        request: &str,
        how: Submission,
    ) -> Result<String, String> {
        let id = format!("j{}", short_id());
        let at = now();
        let mut job = Job::new(id.clone(), workspace.into(), request.into(), at).on(how.pipeline);
        let mut workflow_version = None;
        if let Some(w) = &how.workflow {
            let dials = settings_store::current(&*self.store).await?.dials;
            (job, workflow_version) =
                workflow_store::start_on(&*self.store, &dials, w, job).await?;
        }
        let record = Record {
            job,
            runtime: Runtime {
                issue: how.issue,
                requester: how.requester,
                workflow: how.workflow,
                workflow_version,
                ..Runtime::default()
            },
            rev: 1,
            created_at: at,
        };
        self.store.insert(&record).await?;
        (self.on_change)(&id);
        self.pump();
        Ok(id)
    }

    /// Submit one request of an eval under one arm: a trial job, kept with
    /// its eval run before it is admitted.
    pub async fn submit_trial(
        self: &Arc<Self>,
        e: &Eval,
        mut run: EvalRun,
    ) -> Result<String, String> {
        let id = format!("j{}", short_id());
        let at = now();
        let record = Record {
            job: Job::new(id.clone(), e.workspace.clone(), run.request.clone(), at),
            runtime: Runtime::default(),
            rev: 1,
            created_at: at,
        };
        self.store.insert(&record).await?;
        run.job = id.clone();
        self.store.add_eval_run(&run).await?;
        (self.on_change)(&id);
        self.pump();
        Ok(id)
    }

    /// The dials a job runs under: a trial's own, or the owner's. A job on
    /// a kept workflow takes each step's dial from the step.
    async fn dials_for(&self, id: &str) -> Result<Dials, String> {
        Ok(match self.store.eval_run_of(id).await? {
            Some(_) => Dials {
                trial: true,
                ..Dials::default()
            },
            None => settings_store::current(&*self.store).await?.dials,
        })
    }

    /// Step one job with one event, save it, and run its effects.
    pub async fn apply(self: &Arc<Self>, id: &str, event: Event) -> Result<(), String> {
        self.apply_when(id, event, Arc::new(|_| true)).await
    }

    /// Apply `event` only if `holds` is true of the job as it is saved, so a
    /// check and the change it allows cannot be split by another change.
    pub async fn apply_when(
        self: &Arc<Self>,
        id: &str,
        event: Event,
        holds: Holds,
    ) -> Result<(), String> {
        self.clone().apply_owned(id.to_string(), event, holds).await
    }

    // Boxed, because an effect it spawns can call it again.
    fn apply_owned(
        self: Arc<Self>,
        id: String,
        event: Event,
        holds: Holds,
    ) -> BoxFuture<Result<(), String>> {
        Box::pin(async move {
            for _ in 0..SAVE_TRIES {
                let mut record = self.store.load(&id).await?;
                if !holds(&record) {
                    return Ok(());
                }
                let dials = self.dials_for(&id).await?;
                let (job, effects) = step(&record.job, &event, now(), &dials);
                if job == record.job && effects.is_empty() {
                    return Ok(());
                }
                record.job = job;
                match self.store.save(&record).await {
                    Ok(rev) => {
                        record.rev = rev;
                        self.after_save(record, effects).await;
                        return Ok(());
                    }
                    Err(e) if e == STALE => continue,
                    Err(e) => return Err(e),
                }
            }
            Err(STALE.into())
        })
    }

    /// Run what a saved change asks for. Artifacts are written before a
    /// client hears of the change, so a page that reads at once sees them.
    /// Dark decisions go in the audit log in the order they fell, and a
    /// split's parts are kept before a client hears.
    async fn after_save(self: &Arc<Self>, record: Record, effects: Vec<Effect>) {
        let in_order = |e: &Effect| {
            matches!(
                e,
                Effect::RecordArtifact { .. }
                    | Effect::CaptureDiff
                    | Effect::DarkDecided { .. }
                    | Effect::Split { .. }
            )
        };
        let (first, rest): (Vec<Effect>, Vec<Effect>) = effects.into_iter().partition(in_order);
        for effect in first {
            self.clone().run(record.clone(), effect).await;
        }
        (self.on_change)(&record.job.id);
        for effect in rest {
            let (factory, record) = (self.clone(), record.clone());
            crate::rt::spawn(factory.run(record, effect));
        }
        self.pump();
    }

    /// Admit queued jobs while VMs are free.
    // ponytail: two pumps at once can each see a free slot; a per-app lock fixes it if the cap is ever overrun.
    fn pump(self: &Arc<Self>) {
        let factory = self.clone();
        crate::rt::spawn(async move {
            let Ok(records) = factory.store.jobs().await else {
                return;
            };
            let jobs: Vec<Job> = records.into_iter().map(|r| r.job).collect();
            for id in bastion_core::orphans(&jobs) {
                let error = "the part before it ended without landing".into();
                let _ = factory.apply(&id, Event::Abandoned { error }).await;
            }
            for id in admit(&jobs, MAX_VMS) {
                let _ = factory.apply(&id, Event::Admitted).await;
            }
        });
    }

    /// Change where a job runs, without stepping it.
    async fn set_runtime(&self, id: &str, change: impl Fn(&mut Runtime)) -> Result<Record, String> {
        for _ in 0..SAVE_TRIES {
            let mut record = self.store.load(id).await?;
            change(&mut record.runtime);
            match self.store.save(&record).await {
                Ok(rev) => {
                    record.rev = rev;
                    return Ok(record);
                }
                Err(e) if e == STALE => continue,
                Err(e) => return Err(e),
            }
        }
        Err(STALE.into())
    }

    fn run(self: Arc<Self>, record: Record, effect: Effect) -> BoxFuture<()> {
        Box::pin(self.run_effect(record, effect))
    }

    async fn run_effect(self: Arc<Self>, record: Record, effect: Effect) {
        let id = record.job.id.clone();
        let failed = match effect {
            Effect::DarkDecided { stage } => {
                self.dark_decided(&id, stage);
                None
            }
            Effect::Provision => match self.provision(&record).await {
                Ok(()) => self.apply(&id, Event::Provisioned).await.err(),
                Err(error) => self
                    .apply(&id, Event::ProvisionFailed { error })
                    .await
                    .err(),
            },
            Effect::StartTurn {
                stage,
                brief,
                cap_secs,
                ..
            } => {
                let started = self.start_turn(&record, stage, &brief, cap_secs).await;
                let failed = |error| Event::TurnFinished {
                    outcome: Outcome::Failed { error },
                };
                self.report_failure(&id, started, failed).await
            }
            Effect::RunStep { stage } => self.run_step(&record, stage).await,
            Effect::Split { parts } => self.add_parts(&record, parts).await.err(),
            Effect::RecordArtifact { stage, body } => {
                let artifact = Artifact {
                    stage,
                    body,
                    created_at: now(),
                };
                self.store.add_artifact(&id, &artifact).await.err()
            }
            Effect::CaptureDiff => {
                if let Some(sandbox) = &record.runtime.sandbox {
                    self.gate.stop_hooks(&id, sandbox).await;
                }
                let body = self
                    .diff(&record)
                    .await
                    .unwrap_or_else(|e| format!("The diff could not be read: {e}"));
                self.review(&id, &body);
                let artifact = Artifact {
                    stage: Stage::Review,
                    body,
                    created_at: now(),
                };
                self.store.add_artifact(&id, &artifact).await.err()
            }
            Effect::Land => {
                let landed = self.land(&record).await;
                let failed = |error| Event::LandFailed { error };
                self.report_failure(&id, landed, failed).await
            }
            Effect::WatchCi | Effect::Merge => self.forge_effect(&record, effect).await,
            Effect::Teardown => {
                self.grade_trial(&record);
                self.teardown(&record).await.err()
            }
        };
        if let Some(error) = failed {
            eprintln!("job {id}: {error}");
        }
    }

    async fn vm_exec(&self, sandbox: &str, argv: &[&str]) -> Result<Value, String> {
        let out = self
            .engine
            .call_for(
                DEFAULT_NAMESPACE,
                "sandbox::exec",
                json!({ "sandbox_id": sandbox, "argv": argv, "timeout_ms": 120_000 }),
                SLOW_MS,
            )
            .await?;
        match out["exit_code"].as_i64() {
            Some(0) | None => Ok(out),
            Some(code) => Err(format!(
                "{} exited {code}: {}",
                argv.join(" "),
                out["stderr"]
            )),
        }
    }

    async fn worktree_path(&self, worktree: &str) -> Result<(PathBuf, String), String> {
        let wt = self
            .engine
            .call(NS, "worktree::get", json!({ "worktree_id": worktree }))
            .await?;
        let wt = if wt["worktree"].is_object() {
            &wt["worktree"]
        } else {
            &wt
        };
        let path = wt["path"]
            .as_str()
            .ok_or(format!("worktree {worktree} has no path"))?;
        let branch = wt["branch"].as_str().unwrap_or_default();
        Ok((PathBuf::from(path), branch.to_string()))
    }

    fn git(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        let out = self.machine.run("git", args, Some(dir))?;
        if out.success {
            Ok(out.text)
        } else {
            Err(format!("git {}: {}", args.join(" "), out.text.trim()))
        }
    }

    /// A worktree on the host, a VM, and the worktree's branch cloned into it.
    /// On a revive the job keeps its worktree, which holds its commits.
    async fn provision(&self, record: &Record) -> Result<(), String> {
        let job = &record.job;
        let worktree = match &record.runtime.worktree {
            Some(kept) => kept.clone(),
            None => {
                let made = self.create_worktree(job).await?;
                self.set_runtime(&job.id, |r| r.worktree = Some(made.clone()))
                    .await?;
                made
            }
        };
        let sandbox = self
            .engine
            .call_for(
                DEFAULT_NAMESPACE,
                "sandbox::create",
                json!({ "image": "python", "network": false, "idle_timeout_secs": 3600 }),
                SLOW_MS,
            )
            .await?["sandbox_id"]
            .as_str()
            .ok_or("sandbox::create gave no sandbox_id")?
            .to_string();
        self.set_runtime(&job.id, |r| r.sandbox = Some(sandbox.clone()))
            .await?;

        let (path, branch) = self.worktree_path(&worktree).await?;
        self.machine.make_dir(&self.scratch)?;
        let bundle = self.scratch.join(format!("{}.in.bundle", job.id));
        self.git(
            &path,
            &[
                "bundle",
                "create",
                &bundle.display().to_string(),
                "HEAD",
                &branch,
            ],
        )?;
        let bytes = self.machine.read_bytes(&bundle)?;
        // ponytail: content_b64 suits small repos; a channel upload lifts the limit (docs/engine-findings.md).
        self.engine
            .call_for(DEFAULT_NAMESPACE, "sandbox::fs::write", json!({ "sandbox_id": sandbox, "path": "/work/in.bundle", "content_b64": base64::engine::general_purpose::STANDARD.encode(&bytes), "parents": true }), SLOW_MS)
            .await?;
        self.vm_exec(
            &sandbox,
            &["git", "clone", "-q", "-b", &branch, "/work/in.bundle", REPO],
        )
        .await?;
        self.vm_exec(
            &sandbox,
            &["git", "-C", REPO, "config", "user.name", "Bastion agent"],
        )
        .await?;
        self.vm_exec(
            &sandbox,
            &[
                "git",
                "-C",
                REPO,
                "config",
                "user.email",
                "agent@bastion.local",
            ],
        )
        .await?;
        // What a review or refactor turn diffs against: where the job forked
        // from its workspace's branch, so a revived VM still sees the change.
        let base = self.fork_point(record, &path).await;
        self.vm_exec(&sandbox, &["git", "-C", REPO, "tag", "-f", BASE_TAG, &base])
            .await?;
        Ok(())
    }

    fn hold(&self, job: &str, binding: String) {
        let mut bindings = self.bindings.lock().unwrap();
        bindings.entry(job.into()).or_default().push(binding);
    }

    fn release(&self, job: &str) {
        let held = self.bindings.lock().unwrap().remove(job);
        for binding in held.unwrap_or_default() {
            self.engine.unbind(&binding);
        }
    }

    async fn start_turn(
        self: &Arc<Self>,
        record: &Record,
        stage: Stage,
        brief: &str,
        cap_secs: u64,
    ) -> Result<(), String> {
        let job = &record.job;
        let sandbox = record.runtime.sandbox.clone().ok_or("no VM")?;
        let turn = turn::Turn::start(&job.id, stage);
        let session = turn.session.clone();
        self.set_runtime(&job.id, |r| r.session = Some(session.clone()))
            .await?;
        self.listen_for_turn(&turn)?;
        // The owner may have edited the rows directly (D26).
        self.gate.reload().await?;
        if !record.runtime.approved.is_empty() {
            self.gate.protect(&job.id, record.runtime.approved.clone());
        }

        let mut message = String::new();
        if let Some(file) = self.at_base(record).await {
            self.gate.set_charter(&job.id, charter::read(&file));
            self.load_arm(&job.id).await?;
            for name in charter::BRIEF_FILES {
                if let Some(text) = file(name) {
                    message.push_str(&format!(
                        "The repository's charter ({name}):\n<charter>\n{text}\n</charter>\n\n"
                    ));
                }
            }
        }
        if stage.read_only() || stage == Stage::Falsify {
            // Where the tree goes back to after the turn, commits included:
            // by `put_back` for a read-only turn, by the baseline after falsify.
            self.vm_exec(&sandbox, &["git", "-C", REPO, "tag", "-f", BEFORE_TURN])
                .await?;
        }
        message.push_str(TOOL_GUIDE);
        message.push_str("\n\n");
        message.push_str(&self.gate.rules(&job.id));
        message.push_str(brief);
        let settings = settings_store::current(&*self.store).await?;
        let model = settings.model(stage);
        self.engine
            .call(
                NS,
                "approval::set-mode",
                json!({ "session_id": session, "mode": "full" }),
            )
            .await?;
        self.engine
            .call_for(NS, "harness::send", json!({
                "session_id": session, "provider": model.provider, "model": model.model, "message": message,
                "options": { "max_turns": MAX_TURN_STEPS, "functions": { "allow": TOOLS }, "metadata": { "sandbox_id": sandbox, "job": job.id, "stage": stage } },
            }), 60_000)
            .await?;
        self.arm_cap(&turn, cap_secs);
        Ok(())
    }

    /// When an effect failed, tell the core: the VM is gone, or `failed`.
    async fn report_failure(
        self: &Arc<Self>,
        id: &str,
        result: Result<(), String>,
        failed: impl FnOnce(String) -> Event,
    ) -> Option<String> {
        let error = result.err()?;
        let event = if vm_lost(&error) {
            Event::VmLost
        } else {
            failed(error)
        };
        self.apply(id, event).await.err()
    }

    /// After a turn: a read-only stage's tree goes back, and a turn that
    /// writes code has its commits kept in the worktree, so a lost VM loses
    /// none. A bring-back error means no new commits, which is not a failure.
    async fn tidy_after_turn(&self, record: &Record) {
        let stage = record.job.stage;
        match &record.runtime.sandbox {
            Some(sandbox) if stage.read_only() => self.put_back(sandbox).await,
            Some(_) if writes_code(stage) => {
                let _ = self.bring_back(record).await;
            }
            _ => {}
        }
    }

    /// A read-only stage leaves no change behind, not even a commit: the tree
    /// goes back to the tag made before the turn, or to `HEAD` without one.
    async fn put_back(&self, sandbox: &str) {
        let to_tag = ["git", "-C", REPO, "reset", "-q", "--hard", BEFORE_TURN];
        if self.vm_exec(sandbox, &to_tag).await.is_err() {
            let _ = self.vm_exec(sandbox, &to_tag[..6]).await;
        }
        let _ = self
            .vm_exec(sandbox, &["git", "-C", REPO, "clean", "-q", "-fd"])
            .await;
    }

    /// Run a step the factory runs itself, and report how it went.
    // ponytail: a workspace has no setup steps until F2, so prepare passes at
    // once; any other step fails until it has a runner.
    async fn run_step(self: &Arc<Self>, record: &Record, stage: Stage) -> Option<String> {
        let event = match stage {
            Stage::Prepare => Event::StepPassed,
            Stage::Baseline | Stage::Verify | Stage::Acceptance => {
                self.acceptance_step(record, stage).await
            }
            Stage::Checks => self.checks_step(record).await,
            Stage::Command => self.command_step(record).await,
            Stage::Ship => self
                .ship(record)
                .await
                .unwrap_or_else(|detail| Event::StepStuck { detail }),
            // ponytail: nothing watches CD yet, so a deploy passes; watching the merge commit's checks comes with a workspace that deploys.
            Stage::Deploy => Event::StepPassed,
            _ => Event::StepFailed {
                detail: format!("Bastion does not run the {stage:?} step yet."),
            },
        };
        let event = match event {
            Event::StepStuck { detail } if vm_lost(&detail) => Event::VmLost,
            other => other,
        };
        self.apply(&record.job.id, event).await.err()
    }

    /// The build's commits against the branch it started from, from the VM.
    async fn diff(&self, record: &Record) -> Result<String, String> {
        let sandbox = record.runtime.sandbox.clone().ok_or("no VM")?;
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        let (_, branch) = self.worktree_path(&worktree).await?;
        let out = self
            .vm_exec(
                &sandbox,
                &["git", "-C", REPO, "diff", &format!("origin/{branch}..HEAD")],
            )
            .await?;
        let text = out["stdout"].as_str().unwrap_or_default();
        let mut body = if text.trim().is_empty() {
            "No commits: the build left nothing to land.".to_string()
        } else {
            text.to_string()
        };
        // Work the turn left uncommitted does not land, so the review says so.
        let status = self
            .vm_exec(&sandbox, &["git", "-C", REPO, "status", "--porcelain"])
            .await?;
        let dirty: Vec<&str> = status["stdout"]
            .as_str()
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.contains("__pycache__"))
            .collect();
        if !dirty.is_empty() {
            body.push_str("\n\nNot committed, so it will not land:\n");
            body.push_str(&dirty.join("\n"));
        }
        Ok(body)
    }

    /// Stop the VM. The worktree stays for the owner to look at.
    async fn teardown(&self, record: &Record) -> Result<(), String> {
        self.release(&record.job.id);
        if let Some(session) = &record.runtime.session {
            turn::stop(&*self.engine, session).await;
        }
        self.gate.clear(&record.job.id).await;
        if let Some(sandbox) = &record.runtime.sandbox {
            self.engine
                .call_for(
                    DEFAULT_NAMESPACE,
                    "sandbox::stop",
                    json!({ "sandbox_id": sandbox, "wait": false }),
                    60_000,
                )
                .await?;
        }
        Ok(())
    }

    /// Judge a build's diff by the constraints that bind it. An enforced
    /// rule it breaks sends the work back once with the rule and what broke
    /// it; if the next build breaks it the same way, the owner decides.
    fn review(self: &Arc<Self>, id: &str, diff: &str) {
        let findings = review::review(diff, &self.gate.constraints(id));
        let repeated = self.gate.reviewed(id, &findings);
        let Some(f) = findings.iter().find(|f| f.rung == Enforcement::Enforced) else {
            return;
        };
        if repeated {
            eprintln!(
                "job {id}: the build did not fix {}; it waits for the owner",
                f.constraint
            );
            return;
        }
        let text = format!(
            "Bastion sent this back: {} {}. Put them back as they were, with the same names.",
            f.rule, f.detail
        );
        let (factory, id) = (self.clone(), id.to_string());
        crate::rt::spawn(async move {
            if let Err(e) = factory.apply(&id, Event::Commented { text }).await {
                eprintln!("job {id}: the review could not send the work back: {e}");
            }
        });
    }

    /// Make the job's worktree from its workspace, at the base it starts from.
    async fn create_worktree(&self, job: &Job) -> Result<String, String> {
        let workspaces = self.store.workspaces().await?;
        let ws = workspaces
            .iter()
            .find(|w| w.name == job.workspace)
            .ok_or(format!("no workspace {}", job.workspace))?;
        let base = self.base_of(&job.id, &ws.branch).await?;
        let created = self
            .engine
            .call_for(
                NS,
                "worktree::create",
                json!({ "repo_path": ws.path, "base_ref": base, "session_id": job.id }),
                60_000,
            )
            .await?;
        created["worktree_id"]
            .as_str()
            .or(created["id"].as_str())
            .map(String::from)
            .ok_or(format!("worktree::create: {created}"))
    }

    /// Where a job's worktree starts: a trial from its eval's commit, so both
    /// arms see the same code; any other job from the workspace's branch.
    async fn base_of(&self, id: &str, branch: &str) -> Result<String, String> {
        Ok(match self.store.eval_run_of(id).await? {
            Some(run) => self.store.eval(&run.eval).await?.base,
            None => branch.into(),
        })
    }

    /// Grade a trial whose diff reached review, off the effect's path.
    fn grade_trial(self: &Arc<Self>, record: &Record) {
        if record.job.stage != Stage::Review {
            return;
        }
        let (factory, id) = (self.clone(), record.job.id.clone());
        crate::rt::spawn(async move {
            let Ok(Some(run)) = factory.store.eval_run_of(&id).await else {
                return;
            };
            match evals::grade(&*factory.engine, &*factory.store, &run).await {
                Ok(_) => (factory.on_change)(&id),
                Err(e) => eprintln!("job {id}: the run was not graded: {e}"),
            }
        });
    }

    /// A trial's calls are judged under its arm's rungs.
    async fn load_arm(&self, id: &str) -> Result<(), String> {
        if let Some(run) = self.store.eval_run_of(id).await? {
            self.gate
                .set_overrides(id, self.store.arm(&run.eval, &run.arm).await?);
        }
        Ok(())
    }

    /// Judge the job's calls by its charter, read again from its worktree.
    async fn read_charter(&self, record: &Record) {
        let _ = self.load_arm(&record.job.id).await;
        if !record.runtime.approved.is_empty() {
            self.gate
                .protect(&record.job.id, record.runtime.approved.clone());
        }
        if let Some(file) = self.at_base(record).await {
            self.gate.set_charter(&record.job.id, charter::read(&file));
        }
    }

    /// A file as the job's base commit has it, so nothing the agent writes
    /// to its worktree changes the charter that binds it. An empty file says
    /// nothing, so it reads as missing.
    async fn at_base(&self, record: &Record) -> Option<impl Fn(&str) -> Option<String> + '_> {
        let worktree = record.runtime.worktree.as_ref()?;
        let (path, _) = self.worktree_path(worktree).await.ok()?;
        let base = self.fork_point(record, &path).await;
        Some(move |name: &str| {
            let text = self.git(&path, &["show", &format!("{base}:{name}")]).ok()?;
            (!text.trim().is_empty()).then_some(text)
        })
    }
}

mod acceptance;
mod decide;
mod land;
mod resume;
mod split;
mod turn;

pub use turn::final_reply;
/// The sandbox worker's answer for a VM that no longer exists (S002).
fn vm_lost(error: &str) -> bool {
    error.contains("sandbox not found")
}

/// The turns whose commits are the job's work.
fn writes_code(stage: Stage) -> bool {
    matches!(
        stage,
        Stage::Build | Stage::Implement | Stage::CiFix | Stage::Specify
    )
}

#[cfg(test)]
mod tests;
