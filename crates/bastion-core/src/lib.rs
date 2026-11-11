//! The factory's rules (PLAN.md D18). Everything here is a pure
//! function: an event and the current job go in, the next job and the effects
//! to run come out. No I/O, no clock, no engine.

pub mod acceptance;
pub mod audit;
mod brief;
pub mod charter;
pub mod escape;
pub mod eval;
pub mod gates;
pub mod govern;
pub mod people;
mod pipeline;
pub mod publish;
pub mod review;
pub mod settings;
pub mod workflow;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use brief::{BASE_TAG, Discovery, QUESTION_MARKER, brief, discovery, listed, outcome};
pub use pipeline::{Kind, Pipeline, folded};
pub use workflow::results;

/// How many failed turns in a row a stage gets before the job fails (D16).
pub const MAX_TURN_FAILURES: u32 = 2;
/// The longest a turn may run before it counts as failed.
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
    // The full pipeline's own stages, after wipp's.
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
    /// The factory merges the pull request.
    Merge,
    /// The factory watches the deploy of the merged change.
    Deploy,
    /// An agent proposes what the factory should learn from the job.
    Learn,
    /// An agent breaks the product code on purpose, so the baseline can see
    /// each check that passed already fail.
    Falsify,
    // A workflow's own steps (D35).
    /// An agent follows a skill in the repository.
    Skill,
    /// An agent follows the workflow's instructions for the step.
    Prompt,
    /// A shell line runs in the VM: a bash command, a script or a task runner.
    Command,
}

impl Stage {
    /// Every stage, for the store's checks. A new stage goes here too.
    pub const ALL: [Stage; 34] = {
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
            Skill,
            Prompt,
            Command,
        ]
    };

    pub fn kind(self) -> Kind {
        use Stage::*;
        match self {
            Approve | Review | Clarify | YourChecks | PickRefactors | PickFindings | Uat => {
                Kind::Checkpoint
            }
            Land | Prepare | Baseline | Checks | Verify | Acceptance | Ship | Ci | Merge
            | Deploy | Command => Kind::Factory,
            Plan | Build | Scope | Discover | Define | Specify | Implement | Refactor
            | CodeReview | CiFix | Prove | Comprehend | Learn | Falsify | Skill | Prompt => {
                Kind::Turn
            }
        }
    }

    /// A step that acts outside the job: it waits under its dial before it
    /// acts, not after (D33).
    pub fn acts(self) -> bool {
        use Stage::*;
        matches!(self, Ship | Land | Merge | Deploy)
    }

    /// A step a person oversees: every step but the old checkpoints and the
    /// side steps, which hand the work back and never wait.
    pub fn dialed(self) -> bool {
        self.kind() != Kind::Checkpoint && !matches!(self, Stage::CiFix | Stage::Falsify)
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
    /// A step's result, a step that acts, or a question waits on a person.
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
    /// The steps of the workflow the job runs (D35), in order. Empty: the
    /// steps of its pipeline.
    pub flow: Vec<workflow::Step>,
    /// The workflow step the job is at, by id.
    pub node: Option<String>,
    /// The results its steps gave, by name.
    pub results: BTreeMap<String, String>,
    /// The job this one is a part of, when scope split it.
    pub parent: Option<String>,
    /// The part that must land before this one is admitted.
    pub after: Option<String>,
    /// The part of the request this job does.
    pub part: Option<String>,
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
    /// A queued part will never run: the part before it ended unlanded.
    Abandoned {
        error: String,
    },
    /// The change went up as a pull request; CI runs on it now.
    PrOpened {
        url: String,
    },
    CiPassed,
    /// CI failed on the pull request, with the log of what failed.
    CiFailed {
        log: String,
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
    /// A command step passed, with what it printed.
    Ran {
        output: String,
    },
    /// The job's VM is gone, as after its stack restarted. Its commits are
    /// in the worktree, so a new VM picks the job up where it was (revive).
    VmLost,
    /// A person asked to run a failed job, or a stuck step, again.
    Retried,
    /// A person gave the running turn its full cap again, from now.
    Extended,
}

/// Whether a person oversees a step (D33). Stored `ask` and `hold` read as
/// lights-on, and `auto` as dark.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dial {
    /// The step waits for a person to approve its result or send it back.
    #[default]
    #[serde(alias = "ask", alias = "hold")]
    LightsOn,
    /// The step goes on at once.
    #[serde(alias = "auto")]
    Dark,
}

/// How much of a workflow a person oversees, as its steps' dials make it:
/// a gate at every step, at least one gate before the merge, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    LightsOn,
    Dim,
    Dark,
}

/// The dial of each step, as the owner set them. A step not set is lights-on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dials {
    pub steps: BTreeMap<Stage, Dial>,
    /// An eval's run: its plan goes on at once, and it stops after the
    /// build with its diff, so nothing lands.
    pub trial: bool,
    /// How many times in all the job's steps may send its work back by
    /// themselves; past it, a failing step waits on the owner. It comes from
    /// the job's workspace, so the owner's saved dials never hold it.
    #[serde(skip)]
    pub revision_budget: Option<u32>,
}

impl Dials {
    /// Every dialed step at `dial`.
    pub fn all(dial: Dial) -> Dials {
        let mut d = Dials::default();
        for stage in Stage::ALL.into_iter().filter(|s| s.dialed()) {
            d.set(stage, dial);
        }
        d
    }

    pub fn at(&self, stage: Stage) -> Dial {
        if self.trial {
            return if stage == Stage::Plan {
                Dial::Dark
            } else {
                Dial::LightsOn
            };
        }
        self.steps.get(&stage).copied().unwrap_or_default()
    }

    /// The level of a workflow of `stages` (D33): lights-on when every
    /// step is, dim when at least one step up to the merge is, and dark when
    /// no step up to the merge is. The short pipeline's land is its merge.
    pub fn level(&self, stages: &[Stage]) -> Level {
        let steps: Vec<(Stage, Dial)> = stages.iter().map(|s| (*s, self.at(*s))).collect();
        workflow::level_of(&steps)
    }

    /// Keeps only the steps that are not lights-on, so equal dials compare
    /// equal however they were set.
    pub fn set(&mut self, stage: Stage, dial: Dial) {
        if dial == Dial::LightsOn {
            self.steps.remove(&stage);
        } else {
            self.steps.insert(stage, dial);
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
    },
    /// Keep the turn's report as the stage's artifact.
    RecordArtifact {
        stage: Stage,
        body: String,
    },
    /// Keep the build's diff against its base as the review's artifact.
    CaptureDiff,
    /// Bring the VM's commits back to the worktree, then land them.
    Land,
    /// Stop the VM and release the worktree.
    Teardown,
    /// Wait for CI on the job's pull request, then send `CiPassed` or `CiFailed`.
    WatchCi,
    /// Merge the job's pull request, then send `Landed` or `LandFailed`.
    Merge,
    /// Run a step the factory runs itself, then send `StepPassed` or
    /// `StepFailed`. Ship sends `PrOpened` when it passes.
    RunStep {
        stage: Stage,
    },
    /// A dark step went on with no person (D33): note it in the audit log.
    DarkDecided {
        stage: Stage,
    },
    /// Scope split the request: keep these jobs, the later parts, queued.
    Split {
        parts: Vec<Job>,
    },
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
            flow: Vec::new(),
            node: None,
            results: BTreeMap::new(),
            parent: None,
            after: None,
            part: None,
        }
    }

    /// The stages this job runs, in order.
    pub fn stages(&self) -> Vec<Stage> {
        if self.flow.is_empty() {
            self.pipeline.stages()
        } else {
            self.flow.iter().map(|s| s.stage).collect()
        }
    }

    /// Each step's node id, in the order of `stages()`.
    pub fn step_ids(&self) -> Vec<String> {
        if self.flow.is_empty() {
            self.stages().into_iter().map(workflow::chain_id).collect()
        } else {
            self.flow.iter().map(|s| s.id.clone()).collect()
        }
    }

    /// The workflow step the job is at. None while a fix loop runs another
    /// stage for it, or on a pipeline.
    pub fn step(&self) -> Option<&workflow::Step> {
        let at = self.node.as_deref()?;
        self.flow
            .iter()
            .find(|s| s.id == at && s.stage == self.stage)
    }

    /// The job on a workflow's steps, at its first step.
    pub fn running(mut self, flow: Vec<workflow::Step>) -> Self {
        if let Some(first) = flow.first() {
            self.stage = first.stage;
            self.node = Some(first.id.clone());
        }
        self.flow = flow;
        self
    }

    /// The step after this one: the next node of its workflow, or the next
    /// stage of its pipeline.
    fn next(&self) -> Option<(Stage, Option<String>)> {
        if self.flow.is_empty() {
            return pipeline::next_listed(&self.stages(), self.stage).map(|s| (s, None));
        }
        let at = self
            .flow
            .iter()
            .position(|s| Some(&s.id) == self.node.as_ref());
        let next = self.flow.get(at.map_or(0, |i| i + 1))?;
        Some((next.stage, Some(next.id.clone())))
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
        (_, Event::Retried | Event::Extended) => match by_person(&mut next, event, dials) {
            Some(effects) => effects,
            None => return (job.clone(), Vec::new()),
        },
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
        // A turn's result counts only at a step that runs a turn.
        (State::Running, Event::TurnFinished { outcome }) if job.stage.kind() == Kind::Turn => {
            turn_finished(&mut next, outcome, dials)
        }
        (State::Queued, Event::Abandoned { error }) => fail(&mut next, error),
        (State::NeedsYou, event) if decides(&next, event) => approved(&mut next, dials),
        (State::NeedsYou, Event::Commented { text }) => commented(&mut next, text, dials),
        (State::Running, Event::VmLost) => revive(&mut next),
        (State::Running, event) if kept(job, event).is_some() => {
            let Some(effects) = after_step(&mut next, event, dials) else {
                return (job.clone(), Vec::new());
            };
            kept(job, event).into_iter().chain(effects).collect()
        }
        _ => return (job.clone(), Vec::new()),
    };
    (next, effects)
}

/// A step the factory runs reported: its land, CI or own result. `None`
/// means the event does not fit, and nothing changes.
fn after_step(job: &mut Job, event: &Event, dials: &Dials) -> Option<Vec<Effect>> {
    if job.stage == Stage::Land {
        land(job, event, dials)
    } else {
        factory(job, event, dials)
    }
}

/// The result a step the factory runs reported, kept as its artifact
/// whatever the dial (D33).
fn kept(job: &Job, event: &Event) -> Option<Effect> {
    let body = match event {
        Event::StepPassed | Event::CiPassed => "Passed.".into(),
        Event::Landed => "Landed.".into(),
        Event::ChecksGreen => "Every check passed before anything was built.".into(),
        Event::PrOpened { url } => format!("Opened {url}"),
        Event::StepFailed { detail } | Event::StepStuck { detail } => detail.clone(),
        Event::Ran { output } => output.clone(),
        Event::CiFailed { log } => log.clone(),
        Event::LandFailed { error } => error.clone(),
        _ => return None,
    };
    Some(Effect::RecordArtifact {
        stage: job.stage,
        body,
    })
}

/// Whether the event decides the step the job waits on: a person approved
/// it. A question waits for its answer instead.
fn decides(job: &Job, event: &Event) -> bool {
    *event == Event::Approved && job.question.is_none()
}

/// How many times in a row a job may lose its VM before it fails.
pub const MAX_REVIVES: u32 = 2;

/// Give the job a new VM, which re-enters its stage once provisioned. A VM
/// that keeps dying fails the job rather than looping.
fn revive(job: &mut Job) -> Vec<Effect> {
    job.failures += 1;
    if job.failures > MAX_REVIVES {
        return fail(job, "the job's VM was lost again after a revive");
    }
    job.state = State::Provisioning;
    vec![Effect::Provision]
}

/// A person's retry or extend. `None` means it does not fit, and nothing
/// changes.
fn by_person(job: &mut Job, event: &Event, dials: &Dials) -> Option<Vec<Effect>> {
    match event {
        Event::Retried if job.state == State::Failed => Some(requeue(job)),
        Event::Retried if retries(job) => {
            job.error = None;
            Some(start(job, dials))
        }
        // The cap counts from `updated_at`, which `step` moved to now.
        Event::Extended if extends(job) => Some(Vec::new()),
        _ => None,
    }
}

/// Whether the job runs a turn a person may give more time.
fn extends(job: &Job) -> bool {
    job.state == State::Running && job.stage.kind() == Kind::Turn
}

/// A failed job goes back to the queue at the step it failed at. Its
/// worktree kept its commits, so a new VM picks the work up there.
fn requeue(job: &mut Job) -> Vec<Effect> {
    job.state = State::Queued;
    job.error = None;
    job.failures = 0;
    Vec::new()
}

/// Whether a person may run the step the job waits on again: it failed or
/// is stuck. A land or merge retries on approve instead.
fn retries(job: &Job) -> bool {
    job.state == State::Failed
        || (job.state == State::NeedsYou
            && job.error.is_some()
            && job.question.is_none()
            && !matches!(job.stage, Stage::Land | Stage::Merge))
}

/// Enter the current stage. A step that acts outside the job waits first.
fn enter(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    job.question = None;
    if job.stage.acts() {
        return wait(job, dials);
    }
    start(job, dials)
}

/// Run the current stage, once its entry criteria hold: every result
/// linked into it exists (D35).
fn start(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    let needs = job.step().map(|s| s.needs.clone()).unwrap_or_default();
    if let Some(missing) = needs.iter().find(|n| !job.results.contains_key(*n)) {
        return stuck(
            job,
            &format!("This step needs the result {missing}, which no step gave."),
        );
    }
    job.state = State::Running;
    match job.stage {
        Stage::Land => return vec![Effect::Land],
        Stage::Merge => return vec![Effect::Merge],
        // With no pull request (a local forge) there is no CI to watch.
        Stage::Ci if job.pr.is_none() => return advance(job, dials),
        Stage::Ci => return vec![Effect::WatchCi],
        stage if stage.kind() == Kind::Factory => return vec![Effect::RunStep { stage }],
        _ => {}
    }
    vec![Effect::StartTurn {
        stage: job.stage,
        brief: brief(job),
    }]
}

/// Wait on a person under the step's dial: after a step, on its result;
/// before a step that acts, on whether it should. A trial stops after its
/// build, at review, with its diff.
fn wait(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    if dials.trial && job.stage == Stage::Build {
        job.stage = Stage::Review;
        job.state = State::Stopped;
        return vec![Effect::Teardown];
    }
    let dial = job.step().map_or_else(|| dials.at(job.stage), |s| s.dial);
    match dial {
        Dial::LightsOn => {
            job.state = State::NeedsYou;
            Vec::new()
        }
        Dial::Dark => {
            let decided = Effect::DarkDecided { stage: job.stage };
            std::iter::once(decided)
                .chain(approved(job, dials))
                .collect()
        }
    }
}

/// An event at the land stage: a local land, or a pull request's CI and
/// merge. `None` means the event does not fit, and nothing changes.
fn land(job: &mut Job, event: &Event, dials: &Dials) -> Option<Vec<Effect>> {
    Some(match (job.state, event) {
        // A step after the land, such as learn, runs before the job is landed.
        (State::Running, Event::Landed) => advance(job, dials),
        // A failed land keeps the VM and its commits, and waits on the owner:
        // approve retries the land, stop ends the job.
        (State::Running, Event::LandFailed { error }) => {
            job.state = State::NeedsYou;
            job.error = Some(error.clone());
            Vec::new()
        }
        (State::Running, Event::PrOpened { url }) => {
            job.pr = Some(url.clone());
            vec![Effect::WatchCi]
        }
        // CI passed: the merge waits under land's dial.
        (State::Running, Event::CiPassed) => wait(job, dials),
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
        (Stage::Ci, Event::CiPassed) => wait(job, dials),
        (Stage::Merge, Event::Landed) => advance(job, dials),
        (Stage::Ci, Event::CiFailed { log }) => step_failed(job, log, dials),
        (Stage::Merge, Event::LandFailed { error }) => stuck(job, error),
        // A step that acts was judged before it ran.
        (stage, Event::StepPassed) if runs && stage.acts() => advance(job, dials),
        (_, Event::StepPassed) if runs => wait(job, dials),
        (_, Event::StepFailed { detail }) if runs => step_failed(job, detail, dials),
        (_, Event::StepStuck { detail }) if runs => stuck(job, detail),
        (Stage::Baseline, Event::ChecksGreen) => checks_green(job, dials),
        // A command's output is each result it declares.
        (Stage::Command, Event::Ran { output }) => {
            for name in job.step().map(|s| s.gives.clone()).unwrap_or_default() {
                job.results.insert(name, output.clone());
            }
            wait(job, dials)
        }
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
    if budget_spent(job, dials) {
        let budget = dials.revision_budget.unwrap_or_default();
        return stuck(
            job,
            &format!("The revision budget of {budget} is spent.\n{detail}"),
        );
    }
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

/// Whether the job's steps have sent its work back as often as its
/// workspace allows. A person's send-backs (`YourChecks`) do not count.
fn budget_spent(job: &Job, dials: &Dials) -> bool {
    let by_steps = job
        .revisions
        .iter()
        .filter(|(stage, _)| **stage != Stage::YourChecks)
        .map(|(_, n)| n)
        .sum::<u32>();
    dials
        .revision_budget
        .is_some_and(|budget| by_steps >= budget)
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

/// A person approved, a dim wait ran out, or the dial is dark. A step that
/// acts runs now (again, if it failed); at land with a pull request up, that
/// merges it. Any other step goes on.
fn approved(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    job.feedback = None;
    job.error = None;
    match job.stage {
        Stage::Land if job.pr.is_some() => {
            job.state = State::Running;
            vec![Effect::Merge]
        }
        stage if stage.acts() => start(job, dials),
        _ => advance(job, dials),
    }
}

/// Move to the job's next step. Past the last one, the job has landed.
fn advance(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    job.last_refusal = None;
    match job.next() {
        Some((stage, node)) => {
            job.stage = stage;
            job.node = node;
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
/// the work, or to wait on its result.
fn after_turn(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    if job.stage == Stage::Implement && job.back_to.is_none() {
        job.done_tasks += 1;
        if (job.done_tasks as usize) < job.tasks.len() {
            return enter(job, dials);
        }
    }
    match job.back_to.take() {
        Some(stage) => rework(job, stage, None, dials),
        None => wait(job, dials),
    }
}

fn fail(job: &mut Job, error: &str) -> Vec<Effect> {
    job.state = State::Failed;
    job.error = Some(error.into());
    vec![Effect::Teardown]
}

/// The results a step must give that `report` lacks (its exit criteria).
fn missing_results(job: &Job, report: &str) -> Option<String> {
    let given = results(report);
    let gives = job.step().map(|s| s.gives.clone()).unwrap_or_default();
    let missing: Vec<&String> = gives.iter().filter(|n| !given.contains_key(*n)).collect();
    (!missing.is_empty()).then(|| {
        let names: Vec<String> = missing.iter().map(|n| format!("RESULT {n}:")).collect();
        format!("End the reply with these results: {}", names.join(", "))
    })
}

/// Keep each result the step gives on the job; the store keeps it as its
/// document.
fn keep_results(job: &mut Job, report: &str) {
    let gives = job.step().map(|s| s.gives.clone()).unwrap_or_default();
    let mut given = results(report);
    for name in gives {
        if let Some(body) = given.remove(&name) {
            job.results.insert(name, body);
        }
    }
}

fn turn_finished(job: &mut Job, outcome: &Outcome, dials: &Dials) -> Vec<Effect> {
    match outcome {
        Outcome::Done { report } if missing_results(job, report).is_some() => {
            let problem = missing_results(job, report).unwrap_or_default();
            let unusable = Outcome::Unusable {
                report: report.clone(),
                problem,
            };
            turn_finished(job, &unusable, dials)
        }
        Outcome::Done { report } => {
            job.failures = 0;
            if job.stage == Stage::Plan {
                job.plan = Some(report.clone());
            }
            let mut effects = vec![Effect::RecordArtifact {
                stage: job.stage,
                body: report.clone(),
            }];
            if matches!(job.stage, Stage::Build | Stage::CodeReview) {
                effects.push(Effect::CaptureDiff);
            }
            keep_results(job, report);
            job.feedback = None;
            effects.extend(after_report(job, report, dials));
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

/// What a finished turn's report decides, then where the job goes.
fn after_report(job: &mut Job, report: &str, dials: &Dials) -> Vec<Effect> {
    match job.stage {
        Stage::Discover => discovered(job, report),
        Stage::Scope => {
            let mut effects = split(job, &parts(report));
            effects.extend(after_turn(job, dials));
            return effects;
        }
        Stage::Define => job.criteria = Some(report.into()),
        Stage::Specify => job.falsify_tried = false,
        Stage::Prove => return proved(job, report, dials),
        _ => {}
    }
    after_turn(job, dials)
}

/// Prove's verdict, its report's last `VERDICT:` line. Yes goes on; no sends
/// the work back to implement; anything else waits on the owner, who reads
/// the evidence and approves or sends it back.
fn proved(job: &mut Job, report: &str, dials: &Dials) -> Vec<Effect> {
    let verdict = report
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("VERDICT:"))
        .map(|v| v.trim().to_ascii_lowercase());
    match verdict.as_deref() {
        Some("yes") if cites_evidence(report) => after_turn(job, dials),
        Some("yes") => stuck(
            job,
            "The proof says yes but quotes no output. Read it, then approve to go on, or send it back with a note.",
        ),
        Some("no") => step_failed(job, report, dials),
        _ => stuck(
            job,
            "The proof is partial. Read it, then approve to go on, or send it back with a note.",
        ),
    }
}

/// Whether a proof quotes output: a fenced code block or a quoted line.
fn cites_evidence(report: &str) -> bool {
    let fences = report.lines().filter(|l| l.trim_start().starts_with("```"));
    let quoted = report.lines().any(|l| l.trim_start().starts_with("> "));
    fences.count() >= 2 || quoted
}

/// Keep discovery's tasks for implement. Its questions wait with it, and
/// the answers go to plan (`pipeline::sent_back`).
fn discovered(job: &mut Job, report: &str) {
    job.tasks = discovery(report).tasks;
    job.done_tasks = 0;
}

/// The parts scope's report names under `PARTS:`, in the order they land.
fn parts(report: &str) -> Vec<String> {
    report
        .lines()
        .map(str::trim)
        .skip_while(|l| !l.eq_ignore_ascii_case("PARTS:"))
        .filter_map(brief::item)
        .map(String::from)
        .collect()
}

/// Scope named several parts: this job does the first, and each later part
/// is a new job that waits for the one before it to land. A part is not
/// split again.
fn split(job: &mut Job, parts: &[String]) -> Vec<Effect> {
    let is_part = job.parent.is_some() || job.part.is_some();
    if is_part || parts.len() < 2 {
        return Vec::new();
    }
    job.part = Some(parts[0].clone());
    let mut before = job.id.clone();
    let later = parts[1..].iter().enumerate().map(|(k, part)| {
        let mut next = Job::new(
            format!("{}-{}", job.id, k + 2),
            job.workspace.clone(),
            job.request.clone(),
            job.updated_at,
        )
        .on(job.pipeline)
        .running(job.flow.clone());
        next.parent = Some(job.id.clone());
        next.after = Some(std::mem::replace(&mut before, next.id.clone()));
        next.part = Some(part.clone());
        next
    });
    vec![Effect::Split {
        parts: later.collect(),
    }]
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

/// A note on a step's result sends the work back with it
/// (`pipeline::sent_back`). An answer to a question goes to the stage that
/// asked.
fn commented(job: &mut Job, text: &str, dials: &Dials) -> Vec<Effect> {
    if let Some(question) = job.question.take() {
        job.feedback = Some(format!("You asked: {question}\nThe answer: {text}"));
        return enter(job, dials);
    }
    let judged = job.error.take().is_none();
    job.feedback = Some(text.into());
    match job.stage {
        Stage::Comprehend if judged => job.revisions.clear(),
        // Counted under your checks, the checkpoint it was before D33.
        Stage::Define => *job.revisions.entry(Stage::YourChecks).or_default() += 1,
        _ => {}
    }
    let (stage, back_to) = pipeline::sent_back(job.stage, judged);
    rework(job, stage, back_to, dials)
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

/// Every stage of the job's pipeline, in order, with its state. On a
/// workflow the job is at its node, so a stage it holds twice is told apart.
pub fn steps(job: &Job) -> Vec<(Stage, StepState)> {
    let stages = job.stages();
    let at_node = job
        .node
        .as_ref()
        .and_then(|n| job.flow.iter().position(|s| &s.id == n));
    let current = at_node
        .or_else(|| stages.iter().position(|s| *s == job.stage))
        .unwrap_or(0);
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

/// What a person may do with a job now. A client offers only these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Approve the step the job waits on.
    Approve,
    /// A note on the step's result, which sends the work back.
    Comment,
    /// Answer the agent's question.
    Answer,
    /// Run the failed job, or the stuck step, again.
    Retry,
    /// Give the running turn its full cap again.
    Extend,
    Stop,
}

pub fn actions(job: &Job) -> Vec<Action> {
    let mut allowed = if job.state == State::NeedsYou {
        decisions(job)
    } else {
        Vec::new()
    };
    if retries(job) {
        allowed.push(Action::Retry);
    }
    if extends(job) {
        allowed.push(Action::Extend);
    }
    if !job.state.finished() {
        allowed.push(Action::Stop);
    }
    allowed
}

/// What a person may do with a job that waits on them.
fn decisions(job: &Job) -> Vec<Action> {
    if matches!(job.stage, Stage::Land | Stage::Merge) {
        return vec![Action::Approve];
    }
    if job.question.is_some() {
        return vec![Action::Answer];
    }
    if redefined_enough(job) {
        return vec![Action::Approve];
    }
    vec![Action::Approve, Action::Comment]
}

fn redefined_enough(job: &Job) -> bool {
    job.stage == Stage::Define
        && job.revisions.get(&Stage::YourChecks).copied().unwrap_or(0) >= MAX_REDEFINES
}

/// The queued jobs to admit, oldest first, so that at most `limit` jobs hold
/// a VM at once, and at most `cap(workspace)` of them in one workspace when
/// it has a cap.
pub fn admit(jobs: &[Job], limit: usize, cap: impl Fn(&str) -> Option<usize>) -> Vec<String> {
    let holding: Vec<&Job> = jobs.iter().filter(|j| j.state.holds_vm()).collect();
    let busy = |w: &str| holding.iter().filter(|j| j.workspace == w).count();
    let mut free = limit.saturating_sub(holding.len());
    let mut taken: BTreeMap<&str, usize> = BTreeMap::new();
    let mut admitted = Vec::new();
    for j in queued(jobs) {
        let w = j.workspace.as_str();
        let held = taken.entry(w).or_insert_with(|| busy(w));
        if free == 0 || cap(w).is_some_and(|c| *held >= c) {
            continue;
        }
        (free, *held) = (free - 1, *held + 1);
        admitted.push(j.id.clone());
    }
    admitted
}

/// The queued jobs that may start, oldest first. A part waits for the part
/// before it to land.
fn queued(jobs: &[Job]) -> Vec<&Job> {
    let waiting = |j: &Job| {
        j.after
            .as_ref()
            .is_some_and(|a| jobs.iter().any(|o| &o.id == a && o.state != State::Landed))
    };
    let mut queued: Vec<&Job> = jobs
        .iter()
        .filter(|j| j.state == State::Queued && !waiting(j))
        .collect();
    queued.sort_by_key(|j| j.updated_at);
    queued
}

/// The effects that start a running job's current step again, as after a
/// restart lost them. None when the job is not running, or when starting
/// it would move it to another step.
pub fn restart(job: &Job, dials: &Dials) -> Vec<Effect> {
    if job.state != State::Running {
        return Vec::new();
    }
    let mut again = job.clone();
    let effects = start(&mut again, dials);
    if (again.stage, again.state) == (job.stage, job.state) {
        effects
    } else {
        Vec::new()
    }
}

/// Queued parts that will never run, because the part before them ended
/// without landing.
pub fn orphans(jobs: &[Job]) -> Vec<String> {
    let unlanded = |id: &String| {
        jobs.iter()
            .any(|o| &o.id == id && o.state.finished() && o.state != State::Landed)
    };
    let orphaned = |j: &&Job| j.state == State::Queued && j.after.as_ref().is_some_and(unlanded);
    jobs.iter().filter(orphaned).map(|j| j.id.clone()).collect()
}

#[cfg(test)]
mod full_tests;
#[cfg(test)]
mod tests;
