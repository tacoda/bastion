//! The factory loop (PLAN.md §6.1): load a job, step it with `bastion-core`,
//! save it with its `rev`, run the effects on iii, and tell the page. Every
//! factory rule lives in the core; this file only does what an effect says.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use bastion_core::govern::Enforcement;
use bastion_core::review;
use bastion_core::settings::Forge;
use bastion_core::{
    Dial, Dials, Effect, Event, HOLD_SECS, Job, Outcome, Stage, State, admit, outcome, step,
};
use serde_json::{Value, json};

use crate::audit_log::{self, AuditLog};
use crate::charter;
use crate::engine::{DEFAULT_NAMESPACE, Engine};
use crate::eval_store::{Eval, EvalRun};
use crate::evals;
use crate::forge;
use crate::gate::Gate;
use crate::machine::Machine;
use crate::settings_store;
use crate::store::{Artifact, Record, Runtime, STALE, Store};

const NS: &str = "bastion";
/// At most this many jobs hold a VM (iii-sandbox's `max_concurrent_sandboxes`).
const MAX_VMS: usize = 8;
/// Every turn's model until A7 lets the owner pick one per stage.
// ponytail: one fixed model; A7 moves it into settings.
pub const PROVIDER: &str = "openai";
pub const MODEL: &str = "gpt-5-mini";
const MAX_TURN_STEPS: u64 = 40;
/// The tools a turn may call. A pre-trigger hook pins each to the job's VM.
const TOOLS: [&str; 4] = [
    "sandbox::exec",
    "sandbox::fs::read",
    "sandbox::fs::write",
    "sandbox::fs::ls",
];
const REPO: &str = "/work/repo";
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
    gate: Arc<Gate>,
}

/// The text of a session's last assistant message.
pub async fn final_reply(engine: &dyn Engine, session: &str) -> Result<String, String> {
    let out = engine
        .call(NS, "session::messages", json!({ "session_id": session }))
        .await?;
    out["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .find(|m| m["message"]["role"] == "assistant")
        .map(|m| {
            m["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .ok_or_else(|| format!("session {session} has no reply"))
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

impl Factory {
    pub fn new(
        engine: Arc<dyn Engine>,
        store: Arc<dyn Store>,
        machine: Arc<dyn Machine>,
        on_change: OnChange,
        scratch: PathBuf,
    ) -> Arc<Self> {
        let audit = audit_log::key(&*machine)
            .map(|key| AuditLog::start(store.clone(), key, audit_log::WRITE_LIMIT));
        if let Err(e) = &audit {
            eprintln!("the audit has no key, so the gate refuses every call: {e}");
        }
        let gate = Gate::new(store.clone(), engine.clone(), on_change.clone(), audit);
        Arc::new(Self {
            engine,
            store,
            machine,
            on_change,
            scratch,
            bindings: Mutex::new(HashMap::new()),
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
        tauri::async_runtime::spawn(async move {
            if let Err(e) = factory.gate.reload().await {
                eprintln!("the gate has no policy, so it refuses every call: {e}");
            }
            factory.resume().await
        });
        Ok(())
    }

    pub async fn submit(
        self: &Arc<Self>,
        workspace: &str,
        request: &str,
    ) -> Result<String, String> {
        self.submit_with(workspace, request, None).await
    }

    /// Submit a request, with the tracker issue it came from if any.
    pub async fn submit_with(
        self: &Arc<Self>,
        workspace: &str,
        request: &str,
        issue: Option<String>,
    ) -> Result<String, String> {
        let id = format!("j{}", short_id());
        let at = now();
        let record = Record {
            job: Job::new(id.clone(), workspace.into(), request.into(), at),
            runtime: Runtime {
                issue,
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

    /// The dials a job runs under: a trial's own, or the owner's.
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
        self.clone().apply_owned(id.to_string(), event).await
    }

    // Boxed, because an effect it spawns can call it again.
    fn apply_owned(self: Arc<Self>, id: String, event: Event) -> BoxFuture<Result<(), String>> {
        Box::pin(async move {
            for _ in 0..SAVE_TRIES {
                let mut record = self.store.load(&id).await?;
                let dials = self.dials_for(&id).await?;
                let (job, effects) = step(&record.job, &event, now(), &dials);
                if job == record.job && effects.is_empty() {
                    return Ok(());
                }
                record.job = job;
                match self.store.save(&record).await {
                    Ok(rev) => {
                        record.rev = rev;
                        // Artifacts are written before the page hears of the
                        // change, so a page that reads at once sees them.
                        let (artifacts, rest): (Vec<Effect>, Vec<Effect>) =
                            effects.into_iter().partition(|e| {
                                matches!(e, Effect::RecordArtifact { .. } | Effect::CaptureDiff)
                            });
                        for effect in artifacts {
                            self.clone().run(record.clone(), effect).await;
                        }
                        (self.on_change)(&id);
                        for effect in rest {
                            let (factory, record) = (self.clone(), record.clone());
                            tauri::async_runtime::spawn(factory.run(record, effect));
                        }
                        self.pump();
                        return Ok(());
                    }
                    Err(e) if e == STALE => continue,
                    Err(e) => return Err(e),
                }
            }
            Err(STALE.into())
        })
    }

    /// Admit queued jobs while VMs are free.
    // ponytail: two pumps at once can each see a free slot; a per-app lock fixes it if the cap is ever overrun.
    fn pump(self: &Arc<Self>) {
        let factory = self.clone();
        tauri::async_runtime::spawn(async move {
            let Ok(records) = factory.store.jobs().await else {
                return;
            };
            let jobs: Vec<Job> = records.into_iter().map(|r| r.job).collect();
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
            } => match self.start_turn(&record, stage, &brief, cap_secs).await {
                Ok(()) => None,
                Err(error) => {
                    let outcome = Outcome::Failed { error };
                    self.apply(&id, Event::TurnFinished { outcome }).await.err()
                }
            },
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
            Effect::Land => match self.land(&record).await {
                Ok(()) => None,
                Err(error) => self.apply(&id, Event::LandFailed { error }).await.err(),
            },
            Effect::WatchCi | Effect::Merge => self.forge_effect(&record, effect).await,
            Effect::Teardown => {
                self.grade_trial(&record);
                self.teardown(&record).await.err()
            }
            Effect::Hold { stage, secs } => {
                self.hold_checkpoint(id.clone(), stage, secs);
                None
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

    /// A worktree on the host, a VM, and the worktree's branch cloned into it (N7).
    async fn provision(&self, record: &Record) -> Result<(), String> {
        let job = &record.job;
        let worktree = self.create_worktree(job).await?;
        self.set_runtime(&job.id, |r| r.worktree = Some(worktree.clone()))
            .await?;
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
        // ponytail: content_b64 suits small repos; a channel upload lifts the limit (docs/levels.md N7).
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
        Ok(())
    }

    fn hold(&self, job: &str, binding: String) {
        self.bindings
            .lock()
            .unwrap()
            .entry(job.into())
            .or_default()
            .push(binding);
    }

    fn release(&self, job: &str) {
        for binding in self
            .bindings
            .lock()
            .unwrap()
            .remove(job)
            .unwrap_or_default()
        {
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
        let session = format!("{}-{}", job.id, short_id());
        self.set_runtime(&job.id, |r| r.session = Some(session.clone()))
            .await?;
        self.listen_for_turn(&job.id, &session)?;

        let mut message = String::new();
        if let Some(worktree) = &record.runtime.worktree {
            let (path, _) = self.worktree_path(worktree).await?;
            self.gate
                .set_charter(&job.id, charter::read(&*self.machine, &path));
            self.load_arm(&job.id).await?;
            for name in charter::BRIEF_FILES {
                if let Ok(text) = self.machine.read(&path.join(name)) {
                    message.push_str(&format!(
                        "The repository's charter ({name}):\n<charter>\n{text}\n</charter>\n\n"
                    ));
                }
            }
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
        let factory = self.clone();
        let (id, turn) = (job.id.clone(), session.clone());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(cap_secs)).await;
            factory.turn_done(&id, &turn, true).await;
        });
        Ok(())
    }

    fn listen_for_turn(self: &Arc<Self>, job: &str, session: &str) -> Result<(), String> {
        let factory = self.clone();
        let (id, turn) = (job.to_string(), session.to_string());
        let binding = self.engine.bind(
            "harness::turn-completed",
            json!({ "session_id": session }),
            Arc::new(move |_| {
                let (factory, id, turn) = (factory.clone(), id.clone(), turn.clone());
                tauri::async_runtime::spawn(
                    async move { factory.turn_done(&id, &turn, false).await },
                );
                json!({})
            }),
        )?;
        self.hold(job, binding);
        Ok(())
    }

    /// A turn ended, or ran past its cap. Ignored when the job moved on.
    async fn turn_done(self: &Arc<Self>, id: &str, session: &str, timed_out: bool) {
        let Ok(record) = self.store.load(id).await else {
            return;
        };
        if record.runtime.session.as_deref() != Some(session) || record.job.state != State::Running
        {
            return;
        }
        let result = if timed_out {
            Outcome::Failed {
                error: format!(
                    "the turn ran past {} minutes",
                    bastion_core::TURN_CAP_SECS / 60
                ),
            }
        } else {
            match self
                .engine
                .call(NS, "harness::status", json!({ "session_id": session }))
                .await
            {
                Ok(status) if status["status"] != "completed" => return,
                Ok(status) => match status["result_error"].as_str() {
                    Some(error) => Outcome::Failed {
                        error: error.into(),
                    },
                    None => match self.final_reply(session).await {
                        Ok(reply) => outcome(&reply),
                        Err(error) => Outcome::Failed { error },
                    },
                },
                Err(error) => Outcome::Failed { error },
            }
        };
        self.release(id);
        evals::meter(&*self.engine, &*self.store, &record.job, session).await;
        if record.job.stage.read_only()
            && let Some(sandbox) = &record.runtime.sandbox
        {
            // A read-only stage leaves no change behind.
            let _ = self
                .vm_exec(sandbox, &["git", "-C", REPO, "reset", "-q", "--hard"])
                .await;
            let _ = self
                .vm_exec(sandbox, &["git", "-C", REPO, "clean", "-q", "-fd"])
                .await;
        }
        let _ = self
            .apply(id, Event::TurnFinished { outcome: result })
            .await;
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

    /// The turn's last assistant message, whole. `harness::status` truncates
    /// its `result` and encodes it as a JSON string.
    async fn final_reply(&self, session: &str) -> Result<String, String> {
        final_reply(&*self.engine, session).await
    }

    /// The VM's commits come back to the worktree (N7), then `worktree::land`
    /// merges them, and its trigger reports the outcome.
    async fn land(self: &Arc<Self>, record: &Record) -> Result<(), String> {
        let id = &record.job.id;
        let sandbox = record.runtime.sandbox.clone().ok_or("no VM")?;
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        let (path, branch) = self.worktree_path(&worktree).await?;
        self.vm_exec(
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
        .await
        .map_err(|e| format!("no new commits to land: {e}"))?;
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
        let target = self
            .store
            .workspaces()
            .await?
            .into_iter()
            .find(|w| w.name == record.job.workspace)
            .map(|w| w.branch)
            .ok_or(format!("no workspace {}", record.job.workspace))?;
        match settings_store::current(&*self.store).await?.forge {
            Forge::Github => self.open_pr(record, &path, &target).await,
            Forge::Local => self.land_here(record, &worktree, &target).await,
        }
    }

    /// Push the job's branch and open its pull request, or reuse the one a
    /// fix turn pushes to.
    async fn open_pr(
        self: &Arc<Self>,
        record: &Record,
        path: &Path,
        base: &str,
    ) -> Result<(), String> {
        let job = &record.job;
        let slug = forge::slug(&*self.machine, path)?;
        forge::push(&*self.machine, path, &job.id)?;
        let url = match &job.pr {
            Some(url) => url.clone(),
            None => {
                let pr = forge::describe(job, base, record.runtime.issue.as_deref());
                forge::GitHub {
                    engine: &*self.engine,
                    slug,
                }
                .open(&pr)
                .await?
            }
        };
        self.apply(&job.id, Event::PrOpened { url }).await
    }

    /// Merge the worktree into the workspace's branch here, with `worktree::land`.
    async fn land_here(
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
                    tauri::async_runtime::spawn(async move {
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

    /// Stop the VM. The worktree stays for the owner to look at.
    async fn teardown(&self, record: &Record) -> Result<(), String> {
        self.release(&record.job.id);
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

    /// Approve a held checkpoint when its time runs out. The core ignores it if
    /// the owner acted first.
    fn hold_checkpoint(self: &Arc<Self>, id: String, stage: Stage, secs: u64) {
        let factory = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            let _ = factory.apply(&id, Event::HoldExpired { stage }).await;
        });
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
        tauri::async_runtime::spawn(async move {
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

    /// The repository a job's pull request lives in, from its worktree's origin.
    async fn slug(&self, record: &Record) -> Result<String, String> {
        let worktree = record.runtime.worktree.clone().ok_or("no worktree")?;
        let (path, _) = self.worktree_path(&worktree).await?;
        forge::slug(&*self.machine, &path)
    }

    /// Poll the pull request's checks until they pass or fail.
    // ponytail: polls every 20 s for 30 min; the github worker's PR webhooks replace this once a tunnel is set up.
    fn watch_ci(self: &Arc<Self>, record: &Record) {
        let (factory, record) = (self.clone(), record.clone());
        tauri::async_runtime::spawn(async move {
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
                    Ok(forge::Checks::Passed) => Event::CiPassed,
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

    /// Watch CI on the job's pull request, or merge it.
    async fn forge_effect(self: &Arc<Self>, record: &Record, effect: Effect) -> Option<String> {
        if effect == Effect::WatchCi {
            self.watch_ci(record);
            return None;
        }
        let event = match self.merge(record).await {
            Ok(()) => Event::Landed,
            Err(error) => Event::LandFailed { error },
        };
        self.apply(&record.job.id, event).await.err()
    }

    /// Merge the pull request, then bring the workspace's branch up to date.
    async fn merge(&self, record: &Record) -> Result<(), String> {
        let pr = record.job.pr.clone().ok_or("the job has no pull request")?;
        forge::GitHub {
            engine: &*self.engine,
            slug: self.slug(record).await?,
        }
        .merge(&pr)
        .await?;
        let ws = self
            .store
            .workspaces()
            .await?
            .into_iter()
            .find(|w| w.name == record.job.workspace);
        if let Some(ws) = ws {
            // Best effort: a workspace with local changes keeps them and stays behind.
            let _ = self.machine.run(
                "git",
                &["pull", "-q", "--ff-only", "origin", &ws.branch],
                Some(Path::new(&ws.path)),
            );
        }
        Ok(())
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
        tauri::async_runtime::spawn(async move {
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
        if let Some(worktree) = &record.runtime.worktree
            && let Ok((path, _)) = self.worktree_path(worktree).await
        {
            self.gate
                .set_charter(&record.job.id, charter::read(&*self.machine, &path));
        }
    }

    /// After a restart: pick every active job back up where it was.
    async fn resume(self: Arc<Self>) {
        let Ok(records) = self.store.jobs().await else {
            return;
        };
        for record in records.into_iter().filter(|r| !r.job.state.finished()) {
            let id = record.job.id.clone();
            match (
                record.job.state,
                record.job.stage,
                record.runtime.session.clone(),
            ) {
                (State::Provisioning, ..) => {
                    let error = "the app quit while the job was provisioning".into();
                    let _ = self.apply(&id, Event::ProvisionFailed { error }).await;
                }
                // A pull request already up waits on its CI again.
                (State::Running, Stage::Land, _) => {
                    let effect = if record.job.pr.is_some() {
                        Effect::WatchCi
                    } else {
                        Effect::Land
                    };
                    let factory = self.clone();
                    tauri::async_runtime::spawn(async move { factory.run(record, effect).await });
                }
                (State::NeedsYou, stage, _) => {
                    // A hold that was counting down when the app quit.
                    if let Ok(dials) = self.store.dials().await
                        && dials.at(stage) == Dial::Hold
                    {
                        let left = (record.job.updated_at + HOLD_SECS).saturating_sub(now());
                        self.hold_checkpoint(id, stage, left);
                    }
                }
                // The turn may have ended while the app was closed.
                (State::Running, _, Some(session))
                    if self.listen_for_turn(&id, &session).is_ok() =>
                {
                    self.read_charter(&record).await;
                    self.turn_done(&id, &session, false).await;
                }
                _ => {}
            }
        }
        self.pump();
    }
}

#[cfg(test)]
mod tests;
