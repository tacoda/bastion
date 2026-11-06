//! The factory's rules (PLAN.md D18, §6.1). Everything here is a pure
//! function: an event and the current job go in, the next job and the effects
//! to run come out. No I/O, no clock, no engine.

pub mod acceptance;
pub mod audit;
mod brief;
pub mod charter;
pub mod eval;
pub mod govern;
mod pipeline;
pub mod review;
pub mod settings;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use brief::{Discovery, QUESTION_MARKER, brief, discovery, outcome};
pub use pipeline::{Kind, Pipeline};

/// How many failed turns in a row a stage gets before the job fails (D16).
pub const MAX_TURN_FAILURES: u32 = 2;
/// The longest a turn may run before it counts as failed (PLAN.md §9).
// ponytail: one cap for every stage; give each stage its own when a stage needs longer.
pub const TURN_CAP_SECS: u64 = 60 * 60;

/// Every stage a pipeline has (`Pipeline::stages` gives the order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// An agent reads the code and writes a plan.
    Plan,
    /// The owner approves the plan, or comments on it.
    Approve,
    /// An agent carries the plan out.
    Build,
    /// The owner approves the change, or comments on it.
    Review,
    /// The change lands on the workspace's branch.
    Land,
    // The full pipeline's own stages (A8), after wipp's.
    /// The workspace's setup steps run in the VM.
    Prepare,
    /// An agent judges whether the request is one change or several.
    Scope,
    /// An agent reads the code and lists the tasks, and the questions it has.
    Discover,
    /// The owner answers discovery's questions.
    Clarify,
    /// An agent writes the acceptance criteria as scenarios.
    Define,
    /// The owner approves the criteria, or comments on them.
    YourChecks,
    /// An agent writes the acceptance tests and the commands that run them.
    Specify,
    /// The acceptance tests run before the build, and must fail.
    Baseline,
    /// An agent carries out one task, tests first.
    Implement,
    /// The workspace's test suites run.
    Checks,
    /// The approved acceptance tests run, and must pass.
    Verify,
    /// An agent proposes refactorings.
    Refactor,
    /// The owner picks the refactorings to make, or none.
    PickRefactors,
    /// The acceptance tests run again, after the refactoring.
    Acceptance,
    /// An agent reviews the change.
    CodeReview,
    /// The owner picks the review findings to fix, or none.
    PickFindings,
    /// The factory commits, pushes and opens the pull request.
    Ship,
    /// CI runs on the pull request.
    Ci,
    /// An agent fixes what CI found.
    CiFix,
    /// An agent proves the change meets its criteria, with evidence.
    Prove,
    /// An agent writes the briefing on the change.
    Comprehend,
    /// The owner tries the change.
    Uat,
    /// The owner decides the merge.
    Merge,
    /// The factory watches the deploy of the merged change.
    Deploy,
    /// An agent proposes what the factory should learn from the job.
    Learn,
    /// An agent breaks the product code on purpose, so the baseline can see
    /// each check that passed already fail.
    Falsify,
}

impl Stage {
    /// Every stage, for the store's checks. A new stage goes here too.
    pub const ALL: [Stage; 31] = {
        use Stage::*;
        [
            Plan,
            Approve,
            Build,
            Review,
            Land,
            Prepare,
            Scope,
            Discover,
            Clarify,
            Define,
            YourChecks,
            Specify,
            Baseline,
            Implement,
            Checks,
            Verify,
            Refactor,
            PickRefactors,
            Acceptance,
            CodeReview,
            PickFindings,
            Ship,
            Ci,
            CiFix,
            Prove,
            Comprehend,
            Uat,
            Merge,
            Deploy,
            Learn,
            Falsify,
        ]
    };

    pub fn kind(self) -> Kind {
        use Stage::*;
        match self {
            Approve | Review | Clarify | YourChecks | PickRefactors | PickFindings | Uat
            | Merge => Kind::Checkpoint,
            Land | Prepare | Baseline | Checks | Verify | Acceptance | Ship | Ci | Deploy => {
                Kind::Factory
            }
            Plan | Build | Scope | Discover | Define | Specify | Implement | Refactor
            | CodeReview | CiFix | Prove | Comprehend | Learn | Falsify => Kind::Turn,
        }
    }

    /// The checkpoint a person decides, if this stage is one.
    pub fn is_checkpoint(self) -> bool {
        self.kind() == Kind::Checkpoint
    }

    /// An agent stage may not change the tree.
    // ponytail: learn runs read-only, where wipp gives it no tools at all.
    pub fn read_only(self) -> bool {
        use Stage::*;
        matches!(
            self,
            Plan | Scope | Discover | Define | Refactor | CodeReview | Prove | Comprehend | Learn
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Waiting for a free sandbox.
    Queued,
    /// Getting its worktree and VM.
    Provisioning,
    /// An agent turn is running.
    Running,
    /// A checkpoint or a question waits on the owner.
    NeedsYou,
    Landed,
    Failed,
    Stopped,
}

impl State {
    pub fn finished(self) -> bool {
        matches!(self, State::Landed | State::Failed | State::Stopped)
    }

    /// Holds a sandbox VM.
    pub fn holds_vm(self) -> bool {
        !self.finished() && self != State::Queued
    }
}

/// One request against one workspace, and where it is in its pipeline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub workspace: String,
    pub request: String,
    pub stage: Stage,
    pub state: State,
    /// What the owner must answer while `NeedsYou` holds outside a checkpoint.
    pub question: Option<String>,
    /// The note that becomes the next turn's brief (D16).
    pub feedback: Option<String>,
    /// The latest plan the agent wrote, carried into the build brief.
    pub plan: Option<String>,
    pub failures: u32,
    /// Why the job failed, when it did.
    pub error: Option<String>,
    /// The pull request the job's change went up as, when the forge is one.
    pub pr: Option<String>,
    /// Fix turns a failing CI run has started (D16).
    pub ci_fixes: u32,
    pub updated_at: u64,
    pub pipeline: Pipeline,
    /// Where the work goes after the turn that reworks it (`pipeline::revise`).
    pub back_to: Option<Stage>,
    /// How many times each step has sent the work back.
    pub revisions: BTreeMap<Stage, u32>,
    /// The last failure that sent the work back. The same one again, word
    /// for word, means the fix did not help, so the job waits on the owner.
    pub last_refusal: Option<String>,
    /// Discovery's tasks. Implement runs one turn per task.
    pub tasks: Vec<String>,
    pub done_tasks: u32,
    /// The acceptance criteria define wrote, carried into later briefs.
    pub criteria: Option<String>,
    /// A falsify turn ran for the current spec; the next baseline judges it.
    pub falsify_tried: bool,
}

/// Something that happened to a job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Event {
    /// A sandbox is free for this queued job.
    Admitted,
    Provisioned,
    ProvisionFailed {
        error: String,
    },
    TurnFinished {
        outcome: Outcome,
    },
    /// The owner approved the checkpoint the job waits at.
    Approved,
    /// The owner commented on the artifact at the checkpoint, or answered a question.
    Commented {
        text: String,
    },
    Landed,
    LandFailed {
        error: String,
    },
    Stopped,
    /// The change went up as a pull request; CI runs on it now.
    PrOpened {
        url: String,
    },
    CiPassed,
    /// CI failed on the pull request, with the log of what failed.
    CiFailed {
        log: String,
    },
    /// A held checkpoint's time ran out (D15).
    HoldExpired {
        stage: Stage,
    },
    /// A step the factory runs itself passed.
    StepPassed,
    /// A step the factory runs itself failed, with what it printed.
    StepFailed {
        detail: String,
    },
    /// A step found what only the owner can settle; no fix turn is sent.
    StepStuck {
        detail: String,
    },
    /// The baseline found checks that pass before anything was built.
    ChecksGreen,
}

/// How a checkpoint is decided (D15): the owner decides, the default lands
/// after a hold unless the owner acts first, or the default lands at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dial {
    #[default]
    Ask,
    Hold,
    Auto,
}

/// How long a held checkpoint waits before it approves itself.
pub const HOLD_SECS: u64 = 30 * 60;

/// The dial of each checkpoint, as the owner set them. `review` is both the
/// short pipeline's review and the full pipeline's pick of review findings.
// ponytail: no dial for wipp's `refused` checkpoint; held calls (A5) wait on the owner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dials {
    pub approve: Dial,
    pub review: Dial,
    pub questions: Dial,
    pub checks: Dial,
    pub refactor: Dial,
    pub uat: Dial,
    pub merge: Dial,
    /// An eval's run (A6): it approves its own plan and stops at review
    /// with its diff, so nothing lands.
    pub trial: bool,
}

impl Dials {
    /// The dial a stage runs under. Review and merge decide what lands, so
    /// they never go below hold (D15). A stage that is not a checkpoint asks.
    pub fn at(&self, stage: Stage) -> Dial {
        if self.trial {
            return if stage == Stage::Approve {
                Dial::Auto
            } else {
                Dial::Ask
            };
        }
        let at_least_hold = |d| if d == Dial::Auto { Dial::Hold } else { d };
        match stage {
            Stage::Approve => self.approve,
            Stage::Review => at_least_hold(self.review),
            Stage::Merge => at_least_hold(self.merge),
            Stage::Clarify => self.questions,
            Stage::YourChecks => self.checks,
            Stage::PickRefactors => self.refactor,
            Stage::PickFindings => self.review,
            Stage::Uat => self.uat,
            _ => Dial::Ask,
        }
    }
}

/// How an agent turn ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Outcome {
    /// The turn finished its stage, with the text it reported.
    Done { report: String },
    /// The agent stopped with a question for the owner.
    Asked { question: String },
    /// The turn failed, or ran past the cap.
    Failed { error: String },
    /// The turn replied, but the adapter could not read what it asked for,
    /// such as a spec that is not the JSON the brief gave.
    Unusable { report: String, problem: String },
}

/// What the adapter must do after a step. Effects are data; the core never
/// waits on one (§6.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Effect {
    /// Make the job's worktree and VM, and put its code in the VM.
    Provision,
    StartTurn {
        stage: Stage,
        brief: String,
        read_only: bool,
        cap_secs: u64,
    },
    /// Keep the turn's report as the stage's artifact.
    RecordArtifact { stage: Stage, body: String },
    /// Keep the build's diff against its base as the review's artifact.
    CaptureDiff,
    /// Bring the VM's commits back to the worktree, then land them.
    Land,
    /// Stop the VM and release the worktree.
    Teardown,
    /// Send `HoldExpired` for this checkpoint after `secs`.
    Hold { stage: Stage, secs: u64 },
    /// Wait for CI on the job's pull request, then send `CiPassed` or `CiFailed`.
    WatchCi,
    /// Merge the job's pull request, then send `Landed` or `LandFailed`.
    Merge,
    /// Run a step the factory runs itself, then send `StepPassed` or
    /// `StepFailed`. Ship sends `PrOpened` when it passes.
    RunStep { stage: Stage },
}

impl Job {
    pub fn new(id: String, workspace: String, request: String, at: u64) -> Self {
        Job {
            id,
            workspace,
            request,
            stage: Stage::Plan,
            state: State::Queued,
            question: None,
            feedback: None,
            plan: None,
            failures: 0,
            error: None,
            pr: None,
            ci_fixes: 0,
            updated_at: at,
            pipeline: Pipeline::Short,
            back_to: None,
            revisions: BTreeMap::new(),
            last_refusal: None,
            tasks: Vec::new(),
            done_tasks: 0,
            criteria: None,
            falsify_tried: false,
        }
    }

    /// The job on another pipeline, at that pipeline's first stage.
    pub fn on(mut self, pipeline: Pipeline) -> Self {
        self.stage = pipeline.stages()[0];
        self.pipeline = pipeline;
        self
    }
}

/// Apply one event to one job. An event that does not fit the job's state
/// changes nothing and returns no effects.
pub fn step(job: &Job, event: &Event, at: u64, dials: &Dials) -> (Job, Vec<Effect>) {
    let mut next = job.clone();
    next.updated_at = at;
    let effects = match (job.state, event) {
        (state, _) if state.finished() => return (job.clone(), Vec::new()),
        (_, Event::Stopped) => {
            next.state = State::Stopped;
            vec![Effect::Teardown]
        }
        (State::Queued, Event::Admitted) => {
            next.state = State::Provisioning;
            vec![Effect::Provision]
        }
        (State::Provisioning, Event::Provisioned) => enter(&mut next, dials),
        (State::Provisioning, Event::ProvisionFailed { error }) => fail(&mut next, error),
        (State::Running, Event::TurnFinished { outcome }) => {
            turn_finished(&mut next, outcome, dials)
        }
        (State::NeedsYou, event) if decides(&next, event, dials) => approved(&mut next, dials),
        (State::NeedsYou, Event::Commented { text }) => commented(&mut next, text, dials),
        (_, event) if next.stage == Stage::Land => match land(&mut next, event, dials) {
            Some(effects) => effects,
            None => return (job.clone(), Vec::new()),
        },
        (State::Running, event) => match factory(&mut next, event, dials) {
            Some(effects) => effects,
            None => return (job.clone(), Vec::new()),
        },
        _ => return (job.clone(), Vec::new()),
    };
    (next, effects)
}

/// Whether the event decides the checkpoint the job waits at: the owner
/// approved it, or its hold ran out. A question waits for its answer, and
/// approve at land is the land's own (`land`).
fn decides(job: &Job, event: &Event, dials: &Dials) -> bool {
    let open = job.question.is_none() && job.stage != Stage::Land;
    match event {
        Event::Approved => open,
        Event::HoldExpired { stage } => {
            open && *stage == job.stage && dials.at(*stage) == Dial::Hold
        }
        _ => false,
    }
}

/// Start the current stage.
fn enter(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    job.question = None;
    if dials.trial && job.stage == Stage::Review {
        job.state = State::Stopped;
        return vec![Effect::Teardown];
    }
    if job.stage.is_checkpoint() {
        return wait(job, dials);
    }
    job.state = State::Running;
    match job.stage {
        Stage::Land => return vec![Effect::Land],
        Stage::Ci => return vec![Effect::WatchCi],
        stage if stage.kind() == Kind::Factory => return vec![Effect::RunStep { stage }],
        _ => {}
    }
    vec![Effect::StartTurn {
        stage: job.stage,
        brief: brief(job),
        read_only: job.stage.read_only(),
        cap_secs: TURN_CAP_SECS,
    }]
}

/// Wait at a checkpoint, under its dial.
fn wait(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    job.state = State::NeedsYou;
    match dials.at(job.stage) {
        Dial::Ask => Vec::new(),
        Dial::Hold => vec![Effect::Hold {
            stage: job.stage,
            secs: HOLD_SECS,
        }],
        Dial::Auto => advance(job, dials),
    }
}

/// An event at the land stage: a local land, or a pull request's CI and
/// merge. `None` means the event does not fit, and nothing changes.
fn land(job: &mut Job, event: &Event, dials: &Dials) -> Option<Vec<Effect>> {
    Some(match (job.state, event) {
        (State::Running, Event::Landed) => {
            job.state = State::Landed;
            vec![Effect::Teardown]
        }
        // A failed land keeps the VM and its commits, and waits on the owner:
        // approve retries the land, stop ends the job.
        (State::Running, Event::LandFailed { error }) => {
            job.state = State::NeedsYou;
            job.error = Some(error.clone());
            Vec::new()
        }
        // With a pull request open, approve at land is the merge decision.
        (State::NeedsYou, Event::Approved) if job.pr.is_some() => {
            job.error = None;
            job.state = State::Running;
            vec![Effect::Merge]
        }
        (State::NeedsYou, Event::Approved) => {
            job.error = None;
            enter(job, dials)
        }
        (State::Running, Event::PrOpened { url }) => {
            job.pr = Some(url.clone());
            vec![Effect::WatchCi]
        }
        // CI passed: the owner decides the merge.
        (State::Running, Event::CiPassed) => {
            job.state = State::NeedsYou;
            Vec::new()
        }
        (State::Running, Event::CiFailed { log }) => ci_failed(job, log, dials),
        _ => return None,
    })
}

/// How many fix turns a failing CI run may start before the job fails (D16).
pub const MAX_CI_FIXES: u32 = 2;

/// A failing CI run sends the work back to build with its log, at most
/// `MAX_CI_FIXES` times. The fixed build goes through review again, and its
/// land pushes to the same pull request.
fn ci_failed(job: &mut Job, log: &str, dials: &Dials) -> Vec<Effect> {
    if job.ci_fixes >= MAX_CI_FIXES {
        return fail(
            job,
            &format!("CI still failed after {MAX_CI_FIXES} fixes:\n{log}"),
        );
    }
    job.ci_fixes += 1;
    job.stage = Stage::Build;
    job.failures = 0;
    job.feedback = Some(format!(
        "CI failed on the pull request. Read the log, fix the cause, and commit.\n<ci-log>\n{log}\n</ci-log>"
    ));
    enter(job, dials)
}

/// An event at a step the factory runs, or at the merge. `None` means the
/// event does not fit, and nothing changes.
fn factory(job: &mut Job, event: &Event, dials: &Dials) -> Option<Vec<Effect>> {
    let runs = job.stage.kind() == Kind::Factory;
    Some(match (job.stage, event) {
        (Stage::Ship, Event::PrOpened { url }) => {
            job.pr = Some(url.clone());
            advance(job, dials)
        }
        (Stage::Ci, Event::CiPassed) | (Stage::Merge, Event::Landed) => advance(job, dials),
        (Stage::Ci, Event::CiFailed { log }) => step_failed(job, log, dials),
        (Stage::Merge, Event::LandFailed { error }) => stuck(job, error),
        (_, Event::StepPassed) if runs => advance(job, dials),
        (_, Event::StepFailed { detail }) if runs => step_failed(job, detail, dials),
        (_, Event::StepStuck { detail }) if runs => stuck(job, detail),
        (Stage::Baseline, Event::ChecksGreen) => checks_green(job, dials),
        _ => return None,
    })
}

/// Checks that pass before the build go to falsify, once per spec. After
/// it, the baseline reports how falsify went (`acceptance::falsified`).
fn checks_green(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    if job.falsify_tried {
        let detail = "A check passed before anything was built, after falsify too.";
        return step_failed(job, detail, dials);
    }
    job.falsify_tried = true;
    rework(job, Stage::Falsify, Some(Stage::Baseline), dials)
}

/// A failing step sends the work back to be fixed, with what failed as the
/// fixing turn's brief, up to its limit (`pipeline::revise`). Past the limit,
/// or when the same failure comes back word for word, the job waits on the
/// owner.
fn step_failed(job: &mut Job, detail: &str, dials: &Dials) -> Vec<Effect> {
    let Some((fix, back_to, limit)) = pipeline::revise(job.stage) else {
        return stuck(job, detail);
    };
    let tries = job.revisions.entry(job.stage).or_default();
    if *tries >= limit || job.last_refusal.as_deref() == Some(detail) {
        return stuck(job, detail);
    }
    *tries += 1;
    job.last_refusal = Some(detail.into());
    job.feedback = Some(format!(
        "A step failed. Read its output, fix the cause, and commit.\n<output>\n{detail}\n</output>"
    ));
    rework(job, fix, back_to, dials)
}

/// The job waits on the owner at a step that failed: approve goes on
/// without it, comment asks for one more fix.
fn stuck(job: &mut Job, error: &str) -> Vec<Effect> {
    job.state = State::NeedsYou;
    job.error = Some(error.into());
    Vec::new()
}

/// Send the work to `stage`, to come back to `back_to` once it is done.
fn rework(job: &mut Job, stage: Stage, back_to: Option<Stage>, dials: &Dials) -> Vec<Effect> {
    job.stage = stage;
    job.back_to = back_to;
    job.failures = 0;
    enter(job, dials)
}

/// The owner approved, or a hold ran out. At the merge, that merges.
fn approved(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    job.feedback = None;
    job.error = None;
    if job.stage == Stage::Merge {
        job.state = State::Running;
        return vec![Effect::Merge];
    }
    advance(job, dials)
}

/// Move to the pipeline's next stage. Past the last one, the job has landed.
fn advance(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    let stages = job.pipeline.stages();
    let at = stages.iter().position(|s| *s == job.stage);
    job.last_refusal = None;
    match at.and_then(|i| stages.get(i + 1)) {
        Some(stage) => {
            job.stage = *stage;
            job.failures = 0;
            enter(job, dials)
        }
        None => {
            job.state = State::Landed;
            vec![Effect::Teardown]
        }
    }
}

/// Where a finished turn goes: the next task, back to the step that sent
/// the work, or on.
fn after_turn(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    if job.stage == Stage::Implement && job.back_to.is_none() {
        job.done_tasks += 1;
        if (job.done_tasks as usize) < job.tasks.len() {
            return enter(job, dials);
        }
    }
    match job.back_to.take() {
        Some(stage) => rework(job, stage, None, dials),
        None => advance(job, dials),
    }
}

fn fail(job: &mut Job, error: &str) -> Vec<Effect> {
    job.state = State::Failed;
    job.error = Some(error.into());
    vec![Effect::Teardown]
}

fn turn_finished(job: &mut Job, outcome: &Outcome, dials: &Dials) -> Vec<Effect> {
    match outcome {
        Outcome::Done { report } => {
            job.failures = 0;
            if job.stage == Stage::Plan {
                job.plan = Some(report.clone());
            }
            let mut effects = vec![Effect::RecordArtifact {
                stage: job.stage,
                body: report.clone(),
            }];
            if job.stage == Stage::Build {
                effects.push(Effect::CaptureDiff);
            }
            job.feedback = None;
            match job.stage {
                Stage::Discover => discovered(job, report),
                Stage::Define => job.criteria = Some(report.clone()),
                Stage::Specify => job.falsify_tried = false,
                _ => {}
            }
            effects.extend(after_turn(job, dials));
            effects
        }
        Outcome::Unusable { report, problem } => {
            job.failures = 0;
            let mut effects = vec![Effect::RecordArtifact {
                stage: job.stage,
                body: report.clone(),
            }];
            effects.extend(bad_reply(job, problem, dials));
            effects
        }
        Outcome::Asked { question } => {
            job.state = State::NeedsYou;
            job.question = Some(question.clone());
            Vec::new()
        }
        Outcome::Failed { error } => {
            job.failures += 1;
            if job.failures >= MAX_TURN_FAILURES {
                return fail(job, error);
            }
            enter(job, dials)
        }
    }
}

/// Keep discovery's tasks for implement. With no questions there is nothing
/// to clarify, so the job moves past clarify as it leaves discover.
// ponytail: scope's verdict is not read; every request is one change until split (A8 step 6).
fn discovered(job: &mut Job, report: &str) {
    let found = discovery(report);
    job.tasks = found.tasks;
    job.done_tasks = 0;
    if found.questions.is_empty() {
        job.stage = Stage::Clarify;
    }
}

/// A reply the factory cannot read gets one more try with the reason, then
/// the job waits on the owner, as in wipp.
fn bad_reply(job: &mut Job, problem: &str, dials: &Dials) -> Vec<Effect> {
    let tries = job.revisions.entry(job.stage).or_default();
    if *tries >= 1 {
        return stuck(job, problem);
    }
    *tries += 1;
    job.feedback = Some(format!("Your last reply could not be used. {problem}"));
    enter(job, dials)
}

/// How many times the owner may send the criteria back to define (wipp's limit).
pub const MAX_REDEFINES: u32 = 3;

/// A comment at a checkpoint sends the work back to the agent stage before
/// it. An answer to a question goes to the stage that asked.
fn commented(job: &mut Job, text: &str, dials: &Dials) -> Vec<Effect> {
    if let Some(question) = job.question.take() {
        job.feedback = Some(format!("You asked: {question}\nThe answer: {text}"));
    } else {
        job.feedback = Some(text.into());
        job.error = None;
        match job.stage {
            Stage::Uat => job.revisions.clear(),
            Stage::YourChecks => *job.revisions.entry(Stage::YourChecks).or_default() += 1,
            _ => {}
        }
        let (stage, back_to) = pipeline::sent_back(job.stage);
        return rework(job, stage, back_to, dials);
    }
    enter(job, dials)
}

/// Where one step of a job's pipeline stands, as a run of steps shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Done,
    /// Waiting for a free VM.
    Queued,
    Running,
    /// Waiting on the owner.
    Waiting,
    Failed,
    Stopped,
    /// Not reached yet.
    Pending,
}

/// Every stage of the job's pipeline, in order, with its state.
pub fn steps(job: &Job) -> Vec<(Stage, StepState)> {
    let stages = job.pipeline.stages();
    let current = stages.iter().position(|s| *s == job.stage).unwrap_or(0);
    stages
        .iter()
        .enumerate()
        .map(|(i, stage)| {
            let state = match i.cmp(&current) {
                std::cmp::Ordering::Less => StepState::Done,
                std::cmp::Ordering::Greater => StepState::Pending,
                std::cmp::Ordering::Equal => match job.state {
                    State::Queued => StepState::Queued,
                    State::Provisioning | State::Running => StepState::Running,
                    State::NeedsYou => StepState::Waiting,
                    State::Landed => StepState::Done,
                    State::Failed => StepState::Failed,
                    State::Stopped => StepState::Stopped,
                },
            };
            (*stage, state)
        })
        .collect()
}

/// When a held checkpoint approves itself, if the job waits at one.
pub fn hold_until(job: &Job, dials: &Dials) -> Option<u64> {
    (job.state == State::NeedsYou && job.question.is_none() && dials.at(job.stage) == Dial::Hold)
        .then_some(job.updated_at + HOLD_SECS)
}

/// What the owner may do with a job now. The view shows only these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Approve the checkpoint the job waits at.
    Approve,
    /// Comment on the checkpoint's artifact, which sends the work back.
    Comment,
    /// Answer the agent's question.
    Answer,
    Stop,
}

pub fn actions(job: &Job) -> Vec<Action> {
    let mut allowed = Vec::new();
    if job.state == State::NeedsYou && matches!(job.stage, Stage::Land | Stage::Merge) {
        allowed.push(Action::Approve);
    } else if job.state == State::NeedsYou {
        if job.question.is_some() {
            allowed.push(Action::Answer);
        } else {
            allowed.push(Action::Approve);
            if !redefined_enough(job) {
                allowed.push(Action::Comment);
            }
        }
    }
    if !job.state.finished() {
        allowed.push(Action::Stop);
    }
    allowed
}

fn redefined_enough(job: &Job) -> bool {
    job.stage == Stage::YourChecks
        && job.revisions.get(&Stage::YourChecks).copied().unwrap_or(0) >= MAX_REDEFINES
}

/// The queued jobs to admit, oldest first, so that at most `limit` jobs hold
/// a VM at once.
pub fn admit(jobs: &[Job], limit: usize) -> Vec<String> {
    let busy = jobs.iter().filter(|j| j.state.holds_vm()).count();
    let mut queued: Vec<&Job> = jobs.iter().filter(|j| j.state == State::Queued).collect();
    queued.sort_by_key(|j| j.updated_at);
    queued
        .into_iter()
        .take(limit.saturating_sub(busy))
        .map(|j| j.id.clone())
        .collect()
}

#[cfg(test)]
mod full_tests;
#[cfg(test)]
mod tests;
