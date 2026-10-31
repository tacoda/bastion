//! The factory's rules (PLAN.md D18, §6.1). Everything here is a pure
//! function: an event and the current job go in, the next job and the effects
//! to run come out. No I/O, no clock, no engine.

pub mod audit;
mod brief;
pub mod charter;
pub mod govern;
pub mod review;

use serde::{Deserialize, Serialize};

pub use brief::{QUESTION_MARKER, brief, outcome};

/// How many failed turns in a row a stage gets before the job fails (D16).
pub const MAX_TURN_FAILURES: u32 = 2;
/// The longest a turn may run before it counts as failed (PLAN.md §9).
// ponytail: one cap for every stage; give each stage its own when a stage needs longer.
pub const TURN_CAP_SECS: u64 = 60 * 60;

/// The stages of the short pipeline (D17), in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
}

pub const SHORT_PIPELINE: [Stage; 5] = [
    Stage::Plan,
    Stage::Approve,
    Stage::Build,
    Stage::Review,
    Stage::Land,
];

impl Stage {
    /// The checkpoint a person decides, if this stage is one.
    pub fn is_checkpoint(self) -> bool {
        matches!(self, Stage::Approve | Stage::Review)
    }

    /// An agent stage may not change the tree.
    pub fn read_only(self) -> bool {
        self == Stage::Plan
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
    pub updated_at: u64,
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
    /// A held checkpoint's time ran out (D15).
    HoldExpired {
        stage: Stage,
    },
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

/// The dial of each checkpoint, as the owner set them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dials {
    pub approve: Dial,
    pub review: Dial,
}

impl Dials {
    /// The dial a stage runs under. Review is the merge decision, so it never
    /// goes below hold (D15). A stage that is not a checkpoint asks.
    pub fn at(&self, stage: Stage) -> Dial {
        match stage {
            Stage::Approve => self.approve,
            Stage::Review if self.review == Dial::Auto => Dial::Hold,
            Stage::Review => self.review,
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
}

impl Job {
    pub fn new(id: String, workspace: String, request: String, at: u64) -> Self {
        Job {
            id,
            workspace,
            request,
            stage: SHORT_PIPELINE[0],
            state: State::Queued,
            question: None,
            feedback: None,
            plan: None,
            failures: 0,
            error: None,
            updated_at: at,
        }
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
        (State::NeedsYou, Event::Approved)
            if next.question.is_none() && next.stage != Stage::Land =>
        {
            next.feedback = None;
            advance(&mut next, dials)
        }
        (State::NeedsYou, Event::HoldExpired { stage })
            if *stage == next.stage
                && next.question.is_none()
                && dials.at(*stage) == Dial::Hold =>
        {
            next.feedback = None;
            advance(&mut next, dials)
        }
        (State::NeedsYou, Event::Commented { text }) => commented(&mut next, text, dials),
        (State::Running, Event::Landed) if next.stage == Stage::Land => {
            next.state = State::Landed;
            vec![Effect::Teardown]
        }
        // A failed land keeps the VM and its commits, and waits on the owner:
        // approve retries the land, stop ends the job.
        (State::Running, Event::LandFailed { error }) if next.stage == Stage::Land => {
            next.state = State::NeedsYou;
            next.error = Some(error.clone());
            Vec::new()
        }
        (State::NeedsYou, Event::Approved) if next.stage == Stage::Land => {
            next.error = None;
            enter(&mut next, dials)
        }
        _ => return (job.clone(), Vec::new()),
    };
    (next, effects)
}

/// Start the current stage.
fn enter(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    job.question = None;
    if job.stage.is_checkpoint() {
        job.state = State::NeedsYou;
        return match dials.at(job.stage) {
            Dial::Ask => Vec::new(),
            Dial::Hold => vec![Effect::Hold {
                stage: job.stage,
                secs: HOLD_SECS,
            }],
            Dial::Auto => advance(job, dials),
        };
    }
    job.state = State::Running;
    if job.stage == Stage::Land {
        return vec![Effect::Land];
    }
    vec![Effect::StartTurn {
        stage: job.stage,
        brief: brief(job),
        read_only: job.stage.read_only(),
        cap_secs: TURN_CAP_SECS,
    }]
}

fn advance(job: &mut Job, dials: &Dials) -> Vec<Effect> {
    let at = SHORT_PIPELINE.iter().position(|s| *s == job.stage);
    match at.and_then(|i| SHORT_PIPELINE.get(i + 1)) {
        Some(stage) => {
            job.stage = *stage;
            job.failures = 0;
            enter(job, dials)
        }
        None => Vec::new(),
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
            effects.extend(advance(job, dials));
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

/// A comment at a checkpoint sends the work back to the agent stage before
/// it. An answer to a question goes to the stage that asked.
fn commented(job: &mut Job, text: &str, dials: &Dials) -> Vec<Effect> {
    if let Some(question) = job.question.take() {
        job.feedback = Some(format!("You asked: {question}\nThe answer: {text}"));
    } else {
        job.feedback = Some(text.into());
        job.stage = match job.stage {
            Stage::Approve => Stage::Plan,
            Stage::Review => Stage::Build,
            other => other,
        };
        job.failures = 0;
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
    let current = SHORT_PIPELINE
        .iter()
        .position(|s| *s == job.stage)
        .unwrap_or(0);
    SHORT_PIPELINE
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
    if job.state == State::NeedsYou && job.stage == Stage::Land {
        allowed.push(Action::Approve);
    } else if job.state == State::NeedsYou {
        if job.question.is_some() {
            allowed.push(Action::Answer);
        } else {
            allowed.extend([Action::Approve, Action::Comment]);
        }
    }
    if !job.state.finished() {
        allowed.push(Action::Stop);
    }
    allowed
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
mod tests;
