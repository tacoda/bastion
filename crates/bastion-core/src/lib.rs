//! The factory's rules (DECISIONS.md D18). Everything here is a pure
//! function: an event and the current job go in, the next job and the effects
//! to run come out. No I/O, no clock, no engine.
//!
//! A job runs its workflow's steps in order (D39): an agent's step runs a
//! turn, a shell step runs a script, and past the last step the job finishes
//! as its workspace says (D41).

pub mod audit;
mod brief;
pub mod charter;
pub mod corpus;
pub mod escape;
pub mod eval;
pub mod govern;
pub mod media;
pub mod notice;
pub mod people;
pub mod request;
pub mod review;
pub mod secrets;
pub mod settings;
pub mod workflow;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use brief::{BASE_TAG, QUESTION_MARKER, brief, outcome};
pub use workflow::results;

/// How many failed turns in a row a step gets before the job fails (D16).
pub const MAX_TURN_FAILURES: u32 = 2;
/// The longest a turn may run before it counts as failed.
pub const TURN_CAP_SECS: u64 = 60 * 60;
/// How many times one step may send the work back by itself, past which
/// it waits on a person.
pub const MAX_REVISIONS: u32 = 2;

/// What a workflow's step is (D39).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// An agent follows the workflow's instructions for the step.
    Prompt,
    /// An agent acts as one of the repository's agents, in `.agents/agents`.
    Agent,
    /// An agent follows a skill in the repository.
    Skill,
    /// A shell script runs in the VM: a one-line CLI command, a script or a
    /// task runner.
    Shell,
}

/// Who runs a step: an agent's turn, or the factory itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Turn,
    Factory,
}

impl Stage {
    /// Every kind, for the store's checks.
    pub const ALL: [Stage; 4] = [Stage::Prompt, Stage::Agent, Stage::Skill, Stage::Shell];

    pub fn kind(self) -> Kind {
        match self {
            Stage::Shell => Kind::Factory,
            _ => Kind::Turn,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Waiting for a free sandbox.
    Queued,
    /// Getting its worktree and VM.
    Provisioning,
    /// A step runs, or the job finishes.
    Running,
    /// A step's result, or a question, waits on a person.
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

/// One request against one workspace, and where it is in its workflow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub workspace: String,
    pub request: String,
    /// The current step's kind.
    pub stage: Stage,
    pub state: State,
    /// What the owner must answer while `NeedsYou` holds for a question.
    pub question: Option<String>,
    /// The note that becomes the next turn's brief (D16).
    pub feedback: Option<String>,
    pub failures: u32,
    /// Why the job failed, or why a step waits, when it did.
    pub error: Option<String>,
    pub updated_at: u64,
    /// How many times each step has sent the work back, by step id.
    pub revisions: BTreeMap<String, u32>,
    /// The last failure that sent the work back. The same one again, word
    /// for word, means the fix did not help, so the job waits on the owner.
    pub last_refusal: Option<String>,
    /// The steps of the workflow the job runs (D39), in order.
    pub flow: Vec<workflow::Step>,
    /// The step the job is at, by id. None past its last step, while its
    /// branch merges or is left (D41).
    pub node: Option<String>,
    /// The results its steps gave, by name.
    pub results: BTreeMap<String, String>,
    /// The steps whose last run failed; one that passes later comes off.
    #[serde(default)]
    pub failed: std::collections::BTreeSet<String>,
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
    /// A person approved the step the job waits on.
    Approved,
    /// A person sent the step's result back with a note, or answered a question.
    Commented {
        text: String,
    },
    /// The job's branch merged, or was left, as its workspace says (D41).
    Landed,
    LandFailed {
        error: String,
    },
    Stopped,
    /// A shell step passed, with what it printed.
    Ran {
        output: String,
    },
    /// A shell step failed, with what it printed.
    StepFailed {
        detail: String,
    },
    /// A step found what only a person can settle; no fix is sent.
    StepStuck {
        detail: String,
    },
    /// The job's VM is gone, as after its stack restarted. Its commits are
    /// in the worktree, so a new VM picks the job up where it was (revive).
    VmLost,
    /// A person asked to run a failed job, or a stuck step, again.
    Retried,
    /// A person gave the running turn its full cap again, from now.
    Extended,
    /// The step's reviewers read its change (D51).
    Reviewed {
        verdict: review::Verdict,
    },
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
/// a gate at every step, at some, or at none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    LightsOn,
    Dim,
    Dark,
}

/// What bounds one job's run beyond its workflow.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    /// An eval's run: every step goes on at once, and it stops before its
    /// finish, so nothing lands.
    pub trial: bool,
    /// How many times in all the job's steps may send its work back by
    /// themselves; past it, a failing step waits on the owner. It comes from
    /// the job's workspace.
    pub revision_budget: Option<u32>,
    /// What each step's turns have cost so far, by step id, in millionths of
    /// a US dollar, for the steps' own budgets (D43).
    pub spent: BTreeMap<String, u64>,
    /// The agents that must pass the change of the step the job is at (D51).
    pub reviewers: Vec<String>,
}

/// How an agent turn ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Outcome {
    /// The turn finished its step, with the text it reported.
    Done { report: String },
    /// The agent stopped with a question for the owner.
    Asked { question: String },
    /// The turn failed, or ran past the cap.
    Failed { error: String },
    /// The turn replied, but not with what its step asked for, such as a
    /// result it must give.
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
    /// Keep a step's report or output as its artifact, under the step's id.
    RecordArtifact {
        stage: Stage,
        step: String,
        body: String,
    },
    /// Keep the change against its base as step `step`'s artifact, and review it.
    CaptureDiff {
        stage: Stage,
        step: String,
    },
    /// Run a shell step, then send `Ran`, `StepFailed` or `StepStuck`.
    RunStep {
        stage: Stage,
    },
    /// The workflow's last step passed: bring the VM's commits back, then
    /// merge them or leave the branch, as the workspace says (D41).
    Finish,
    /// Stop the VM and release the worktree.
    Teardown,
    /// A dark step went on with no person (D33): note it in the audit log.
    DarkDecided {
        stage: Stage,
    },
    /// Have each reviewer read step `step`'s change, then send `Reviewed`
    /// (D51). The job waits at the step until then.
    Review {
        step: String,
        reviewers: Vec<String>,
    },
}

impl Job {
    pub fn new(id: String, workspace: String, request: String, at: u64) -> Self {
        Job {
            id,
            workspace,
            request,
            stage: Stage::Prompt,
            state: State::Queued,
            question: None,
            feedback: None,
            failures: 0,
            error: None,
            updated_at: at,
            revisions: BTreeMap::new(),
            last_refusal: None,
            flow: Vec::new(),
            node: None,
            results: BTreeMap::new(),
            failed: Default::default(),
        }
    }

    /// The step the job is at.
    pub fn step(&self) -> Option<&workflow::Step> {
        let at = self.node.as_deref()?;
        self.flow.iter().find(|s| s.id == at)
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

    /// Past its last step: the job finishes (D41).
    pub fn finishing(&self) -> bool {
        self.node.is_none() && !self.flow.is_empty()
    }

    fn position(&self) -> Option<usize> {
        let at = self.node.as_ref()?;
        self.flow.iter().position(|s| &s.id == at)
    }

    /// Move to step `i` of the workflow.
    fn go_to(&mut self, i: usize) {
        let step = &self.flow[i];
        (self.stage, self.node) = (step.stage, Some(step.id.clone()));
        self.failures = 0;
    }

    /// The nearest agent's step at or before this one, which a failing or
    /// sent-back step's work goes back to.
    fn fixer(&self) -> Option<usize> {
        let at = self.position()?;
        (0..=at)
            .rev()
            .find(|&i| self.flow[i].stage.kind() == Kind::Turn && !self.flow[i].reach.reads_only)
    }
}

/// Apply one event to one job. An event that does not fit the job's state
/// changes nothing and returns no effects.
pub fn step(job: &Job, event: &Event, at: u64, bounds: &Bounds) -> (Job, Vec<Effect>) {
    let mut next = job.clone();
    next.updated_at = at;
    let effects = match (job.state, event) {
        (_, Event::Retried | Event::Extended) => match by_person(&mut next, event) {
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
        (State::Provisioning, Event::Provisioned) => start_within(&mut next, bounds),
        (State::Provisioning, Event::ProvisionFailed { error }) => fail(&mut next, error),
        (State::Running, Event::TurnFinished { outcome }) if job.stage.kind() == Kind::Turn => {
            turn_finished(&mut next, outcome, bounds)
        }
        (State::NeedsYou, Event::Approved) if job.question.is_none() => approved(&mut next, bounds),
        (State::NeedsYou, Event::Commented { text }) => commented(&mut next, text),
        (State::Running, Event::VmLost) => revive(&mut next),
        (State::Running, Event::Landed | Event::LandFailed { .. }) if job.finishing() => {
            finished(&mut next, event)
        }
        (
            State::Running,
            Event::Ran { .. } | Event::StepFailed { .. } | Event::StepStuck { .. },
        ) if job.stage == Stage::Shell => shell_ran(&mut next, event, bounds),
        // Only a turn's step waits for reviewers.
        (State::Running, Event::Reviewed { verdict }) => reviewed(&mut next, verdict, bounds),
        _ => return (job.clone(), Vec::new()),
    };
    (next, effects)
}

/// How many times in a row a job may lose its VM before it fails.
pub const MAX_REVIVES: u32 = 2;

/// Give the job a new VM, which starts its step again once provisioned. A
/// VM that keeps dying fails the job rather than looping.
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
fn by_person(job: &mut Job, event: &Event) -> Option<Vec<Effect>> {
    match event {
        Event::Retried if job.state == State::Failed => {
            job.state = State::Queued;
            job.error = None;
            job.failures = 0;
            Some(Vec::new())
        }
        Event::Retried if retries(job) => {
            job.error = None;
            Some(start(job))
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

/// Whether a person may run the step the job waits on again: it failed, or
/// it is stuck.
fn retries(job: &Job) -> bool {
    job.state == State::Failed
        || (job.state == State::NeedsYou && job.error.is_some() && job.question.is_none())
}

/// Start the current step as [`start`] does, unless its turns have spent
/// its budget (D43); then it waits on a person. Only the factory's own
/// starts are bounded: a person's retry, note or answer always runs.
fn start_within(job: &mut Job, bounds: &Bounds) -> Vec<Effect> {
    match over_budget(job, bounds) {
        Some(spent) => stuck(job, &spent),
        None => start(job),
    }
}

/// Why the current step may not start another turn, when its turns have
/// cost its budget or more.
fn over_budget(job: &Job, bounds: &Bounds) -> Option<String> {
    let step = job.step().filter(|s| s.stage.kind() == Kind::Turn)?;
    let budget = step.options.spend_micros?;
    let spent = bounds.spent.get(&step.id).copied().unwrap_or_default();
    let dollars = |m: u64| format!("${:.4}", m as f64 / 1e6);
    (spent >= budget).then(|| {
        format!(
            "This step's budget of {} is spent: its turns cost {}.",
            dollars(budget),
            dollars(spent)
        )
    })
}

/// Run the current step, once its entry criteria hold: every result
/// linked into it exists (D35).
fn start(job: &mut Job) -> Vec<Effect> {
    job.question = None;
    if job.finishing() {
        job.state = State::Running;
        return vec![Effect::Finish];
    }
    let needs = job.step().map(|s| s.needs.clone()).unwrap_or_default();
    if let Some(missing) = needs.iter().find(|n| !job.results.contains_key(*n)) {
        return stuck(
            job,
            &format!("This step needs the result {missing}, which no step gave."),
        );
    }
    job.state = State::Running;
    match job.stage {
        Stage::Shell => vec![Effect::RunStep {
            stage: Stage::Shell,
        }],
        stage => vec![Effect::StartTurn {
            stage,
            brief: brief(job),
        }],
    }
}

/// Wait on a person under the step's dial, or go on at once when it is
/// dark, or in a trial.
fn wait(job: &mut Job, bounds: &Bounds) -> Vec<Effect> {
    let dial = job.step().map_or(Dial::LightsOn, |s| s.dial);
    if dial == Dial::LightsOn && !bounds.trial {
        job.state = State::NeedsYou;
        return Vec::new();
    }
    let decided = Effect::DarkDecided { stage: job.stage };
    std::iter::once(decided)
        .chain(approved(job, bounds))
        .collect()
}

/// The id of the step the job is at; empty at the finish.
fn at_step(job: &Job) -> String {
    job.node.clone().unwrap_or_default()
}

/// A shell step's result, kept as its artifact. Its output is each result
/// it declares; a failure goes back to be fixed.
fn shell_ran(job: &mut Job, event: &Event, bounds: &Bounds) -> Vec<Effect> {
    let step = at_step(job);
    let (body, effects) = match event {
        Event::Ran { output } => {
            job.failed.remove(&step);
            for name in job.step().map(|s| s.gives.clone()).unwrap_or_default() {
                job.results.insert(name, output.clone());
            }
            (output.clone(), wait(job, bounds))
        }
        Event::StepFailed { detail } => (detail.clone(), step_failed(job, detail, bounds)),
        Event::StepStuck { detail } => (detail.clone(), stuck(job, detail)),
        _ => return Vec::new(),
    };
    let kept = Effect::RecordArtifact {
        stage: Stage::Shell,
        step,
        body,
    };
    std::iter::once(kept).chain(effects).collect()
}

/// A failing shell step sends its output back to the nearest agent's step
/// before it, to fix, up to its limit and the workspace's budget. Past
/// them, or when the same failure comes back word for word, the job waits
/// on a person.
fn step_failed(job: &mut Job, detail: &str, bounds: &Bounds) -> Vec<Effect> {
    job.failed.extend(job.node.clone());
    // A failure link wins over sending the work back to be fixed (D47).
    let branch = job.step().and_then(|s| s.on_failure.clone());
    let to = match &branch {
        Some(target) => job.flow.iter().position(|s| &s.id == target),
        None => job.fixer(),
    };
    let (Some(to), Some(at)) = (to, job.node.clone()) else {
        return stuck(job, detail);
    };
    if budget_spent(job, bounds) {
        let budget = bounds.revision_budget.unwrap_or_default();
        let spent = format!("The revision budget of {budget} is spent.\n{detail}");
        return stuck(job, &spent);
    }
    let limit = job
        .step()
        .and_then(|s| s.options.revisions)
        .unwrap_or(MAX_REVISIONS);
    let tries = job.revisions.entry(at.clone()).or_default();
    if *tries >= limit || job.last_refusal.as_deref() == Some(detail) {
        return stuck(job, detail);
    }
    *tries += 1;
    job.last_refusal = Some(detail.into());
    let itself = job.position() == Some(to);
    job.feedback = Some(match branch {
        None if itself => format!(
            "Your change failed its review. Read why, fix it, and commit.\n<review>\n{detail}\n</review>"
        ),
        Some(_) => format!(
            "The step {at} failed, and its workflow sends the failure here. Its output:\n<output>\n{detail}\n</output>"
        ),
        None => format!(
            "A later step failed. Read its output, fix the cause, and commit.\n<output>\n{detail}\n</output>"
        ),
    });
    job.go_to(to);
    start_within(job, bounds)
}

/// The reviewers' verdict on a step's change (D51): a pass goes on as the
/// step would have, a fail fails the step, and a reviewer that could not
/// run leaves the job with a person.
fn reviewed(job: &mut Job, verdict: &review::Verdict, bounds: &Bounds) -> Vec<Effect> {
    match verdict {
        review::Verdict::Pass => {
            if let Some(node) = &job.node {
                job.failed.remove(node);
            }
            wait(job, bounds)
        }
        review::Verdict::Fail { detail } => step_failed(job, detail, bounds),
        review::Verdict::Broken { error } => stuck(job, error),
    }
}

/// Whether the job's steps have sent its work back as often as its
/// workspace allows.
fn budget_spent(job: &Job, bounds: &Bounds) -> bool {
    let spent: u32 = job.revisions.values().sum();
    bounds.revision_budget.is_some_and(|budget| spent >= budget)
}

/// The job waits on a person at a step that failed: approve goes on
/// without it, a note sends it back to be fixed.
fn stuck(job: &mut Job, error: &str) -> Vec<Effect> {
    job.state = State::NeedsYou;
    job.error = Some(error.into());
    Vec::new()
}

/// A person approved, or the dial is dark: the job goes on.
fn approved(job: &mut Job, bounds: &Bounds) -> Vec<Effect> {
    job.feedback = None;
    job.error = None;
    advance(job, bounds)
}

/// The end of a workflow (D41): landed once its branch is merged or left,
/// or waiting on a person when that failed; approving finishes again.
fn finished(job: &mut Job, event: &Event) -> Vec<Effect> {
    match event {
        Event::LandFailed { error } => stuck(job, error),
        _ => {
            job.state = State::Landed;
            vec![Effect::Teardown]
        }
    }
}

/// Move to the job's next step. Past the last one the job finishes, and a
/// trial stops instead, so nothing lands.
fn advance(job: &mut Job, bounds: &Bounds) -> Vec<Effect> {
    job.last_refusal = None;
    // A step only a failure leads to is skipped on the way (D47).
    let mut next = job.position().map_or(job.flow.len(), |i| i + 1);
    while next < job.flow.len() && job.flow[next].failure_only {
        next += 1;
    }
    if next < job.flow.len() {
        job.go_to(next);
        return start_within(job, bounds);
    }
    if bounds.trial {
        job.state = State::Stopped;
        return vec![Effect::Teardown];
    }
    job.node = None;
    start(job)
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

/// Keep each result the step gives on the job.
fn keep_results(job: &mut Job, report: &str) {
    let gives = job.step().map(|s| s.gives.clone()).unwrap_or_default();
    let mut given = results(report);
    for name in gives {
        if let Some(body) = given.remove(&name) {
            job.results.insert(name, body);
        }
    }
}

fn turn_finished(job: &mut Job, outcome: &Outcome, bounds: &Bounds) -> Vec<Effect> {
    match outcome {
        Outcome::Done { report } => match missing_results(job, report) {
            Some(problem) => {
                let unusable = Outcome::Unusable {
                    report: report.clone(),
                    problem,
                };
                turn_finished(job, &unusable, bounds)
            }
            None => done(job, report, bounds),
        },
        Outcome::Unusable { report, problem } => {
            job.failures = 0;
            let kept = Effect::RecordArtifact {
                stage: job.stage,
                step: at_step(job),
                body: report.clone(),
            };
            std::iter::once(kept)
                .chain(bad_reply(job, problem, bounds))
                .collect()
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
            start_within(job, bounds)
        }
    }
}

/// A turn that did its step: its report kept, its change captured unless
/// it only reads, its results kept, then the dial decides.
fn done(job: &mut Job, report: &str, bounds: &Bounds) -> Vec<Effect> {
    job.failures = 0;
    let step = at_step(job);
    let mut effects = vec![Effect::RecordArtifact {
        stage: job.stage,
        step: step.clone(),
        body: report.into(),
    }];
    if !job.step().is_some_and(|s| s.reach.reads_only) {
        effects.push(Effect::CaptureDiff {
            stage: job.stage,
            step,
        });
    }
    keep_results(job, report);
    job.feedback = None;
    // A step with reviewers waits at itself for their verdict (D51).
    match bounds.reviewers.is_empty() {
        true => effects.extend(wait(job, bounds)),
        false => effects.push(Effect::Review {
            step: at_step(job),
            reviewers: bounds.reviewers.clone(),
        }),
    }
    effects
}

/// A reply the factory cannot use gets one more try with the reason, then
/// the job waits on a person, as in wipp.
fn bad_reply(job: &mut Job, problem: &str, bounds: &Bounds) -> Vec<Effect> {
    let at = job.node.clone().unwrap_or_default();
    let tries = job.revisions.entry(at).or_default();
    if *tries >= 1 {
        return stuck(job, problem);
    }
    *tries += 1;
    job.feedback = Some(format!("Your last reply could not be used. {problem}"));
    start_within(job, bounds)
}

/// A note on a step's result sends the work back with it to the nearest
/// agent's step, to run again from there. An answer goes to the step that
/// asked.
fn commented(job: &mut Job, text: &str) -> Vec<Effect> {
    if let Some(question) = job.question.take() {
        job.feedback = Some(format!("You asked: {question}\nThe answer: {text}"));
        return start(job);
    }
    job.error = None;
    job.feedback = Some(text.into());
    if let Some(fixer) = job.fixer() {
        job.go_to(fixer);
    }
    start(job)
}

/// Where one step of a job stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Done,
    /// Waiting for a free VM.
    Queued,
    Running,
    /// Waiting on a person.
    Waiting,
    Failed,
    Stopped,
    /// Not reached yet.
    Pending,
    /// Passed by: a step only a failure leads to, when none did (D47).
    Skipped,
}

/// Every step of the job's workflow, in order: its id, its kind and its
/// state.
pub fn steps(job: &Job) -> Vec<(String, Stage, StepState)> {
    // A step only failures lead to ran when a failed step leads to it.
    let reached = |id: &str| {
        let failed = job.flow.iter().filter(|s| job.failed.contains(&s.id));
        failed
            .into_iter()
            .any(|s| s.on_failure.as_deref() == Some(id))
    };
    // Finishing, the last step that ran stands for the finish.
    let ran = |s: &workflow::Step| !s.failure_only || reached(&s.id);
    let last_ran = job.flow.iter().rposition(ran).unwrap_or_default();
    let current = job.position().unwrap_or(last_ran);
    let behind = |s: &workflow::Step| match (job.failed.contains(&s.id), s.failure_only) {
        (true, _) => StepState::Failed,
        (false, true) if !reached(&s.id) => StepState::Skipped,
        _ => StepState::Done,
    };
    let state = |i: usize| match i.cmp(&current) {
        std::cmp::Ordering::Less => behind(&job.flow[i]),
        // Past the finish, a later step was passed by, not still to come.
        std::cmp::Ordering::Greater if job.finishing() => StepState::Skipped,
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
    let all = job.flow.iter().enumerate();
    all.map(|(i, s)| (s.id.clone(), s.stage, state(i)))
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
    let mut allowed = match (job.state, job.question.is_some()) {
        (State::NeedsYou, true) => vec![Action::Answer],
        (State::NeedsYou, false) => vec![Action::Approve, Action::Comment],
        _ => Vec::new(),
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

/// The queued jobs to admit, oldest first, so that at most `limit` jobs hold
/// a VM at once, and at most `cap(workspace)` of them in one workspace when
/// it has a cap.
pub fn admit(jobs: &[Job], limit: usize, cap: impl Fn(&str) -> Option<usize>) -> Vec<String> {
    let holding: Vec<&Job> = jobs.iter().filter(|j| j.state.holds_vm()).collect();
    let busy = |w: &str| holding.iter().filter(|j| j.workspace == w).count();
    let mut free = limit.saturating_sub(holding.len());
    let mut taken: BTreeMap<&str, usize> = BTreeMap::new();
    let mut admitted = Vec::new();
    let mut queued: Vec<&Job> = jobs.iter().filter(|j| j.state == State::Queued).collect();
    queued.sort_by_key(|j| j.updated_at);
    for j in queued {
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

/// The effects that start a running job's current step again, as after a
/// restart lost them. None when the job is not running, or when starting
/// it would move it to another step.
pub fn restart(job: &Job) -> Vec<Effect> {
    if job.state != State::Running {
        return Vec::new();
    }
    let mut again = job.clone();
    let effects = start(&mut again);
    match (again.node == job.node, again.state) {
        (true, State::Running) => effects,
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
