//! The Commands seam (PLAN.md §6.2): every command the view calls and every
//! event the app emits, with their types. The view and the app both depend on
//! this crate, and on nothing of each other's.

use serde::{Deserialize, Serialize};

/// Command names, so the view and the app cannot drift apart.
pub mod command {
    pub const ENGINE_STATUS: &str = "engine_status";
    pub const DOCTOR: &str = "doctor";
    pub const STACK_STATUS: &str = "stack_status";
    pub const STACK_START: &str = "stack_start";
    pub const STACK_STOP: &str = "stack_stop";
    pub const PROFILE: &str = "profile";
    /// Takes `{ stopStack: bool }`.
    pub const QUIT: &str = "quit";
    pub const CONSOLE: &str = "console";
    /// Takes `{ request: LogRequest }`.
    pub const CONTAINER_LOGS: &str = "container_logs";
    /// Take `{ name: String }`.
    pub const CONTAINER_STOP: &str = "container_stop";
    pub const CONTAINER_START: &str = "container_start";
    pub const WORKSPACES: &str = "workspaces";
    /// Takes `{ workspace: Workspace }`.
    pub const WORKSPACE_ADD: &str = "workspace_add";
    pub const JOBS: &str = "jobs";
    /// Takes `{ id: String }`, as do the job actions below.
    pub const JOB: &str = "job";
    /// Takes `{ workspace: String, request: String }`.
    pub const JOB_SUBMIT: &str = "job_submit";
    pub const JOB_APPROVE: &str = "job_approve";
    /// Takes `{ id: String, text: String }`: a comment, or the answer to a question.
    pub const JOB_COMMENT: &str = "job_comment";
    pub const JOB_STOP: &str = "job_stop";
    pub const DIALS: &str = "dials";
    /// Takes `{ dials: DialsView }`.
    pub const DIALS_SET: &str = "dials_set";
    /// Takes `{ held: HeldCallRef }`: run that call once.
    pub const HELD_ALLOW: &str = "held_allow";
    /// Takes `{ held: HeldCallRef, feedback: String }`.
    pub const HELD_REFUSE: &str = "held_refuse";
    pub const CONSTRAINTS: &str = "constraints";
    /// Takes `{ constraint: ConstraintView }`. The source is always the owner.
    pub const CONSTRAINT_ADD: &str = "constraint_add";
    /// Takes `{ id: String, up: bool }`: one rung up or down.
    pub const CONSTRAINT_MOVE: &str = "constraint_move";
    pub const CAPABILITIES: &str = "capabilities";
    /// Each workspace's charter, read from its repository.
    pub const CHARTERS: &str = "charters";
    /// What the factory itself may do, from the settings.
    pub const FACTORY_CAPABILITIES: &str = "factory_capabilities";
    /// Takes `{ stage: String, function: String, up: bool }`.
    pub const CAPABILITY_MOVE: &str = "capability_move";
    pub const AUDIT: &str = "audit";
    pub const EVALS: &str = "evals";
    pub const SETTINGS: &str = "settings";
    /// Takes `{ settings: SettingsView }`; saves a version if anything moved.
    pub const SETTINGS_SET: &str = "settings_set";
    /// Takes `{ preset: String }`: `high`, `medium`, `low` or `off`.
    pub const SETTINGS_PRESET: &str = "settings_preset";
    pub const SETTINGS_HISTORY: &str = "settings_history";
    /// Takes `{ version: u64 }`.
    pub const SETTINGS_RESTORE: &str = "settings_restore";
    pub const MODELS: &str = "models";
    pub const ACCOUNTS: &str = "accounts";
    /// Takes `{ workspace: String }`: the open issues of its GitHub repository.
    pub const ISSUES: &str = "issues";
    /// Takes `{ workspace: String, number: u64 }`; returns the job's id.
    pub const JOB_FROM_ISSUE: &str = "job_from_issue";
    /// Takes `{ id: String }`.
    pub const EVAL: &str = "eval";
    /// Takes `{ eval: NewEval }`; returns the eval's id.
    pub const EVAL_CREATE: &str = "eval_create";
    /// Takes `{ id: String, arm: String }`: set the live policy to that arm's rungs.
    pub const EVAL_APPLY: &str = "eval_apply";
    /// Returns the path of the JSON Lines file it wrote.
    pub const AUDIT_EXPORT: &str = "audit_export";
}

/// Event names the app emits to the page (D10).
pub mod event {
    /// Carries an [`EngineStatus`](super::EngineStatus) each time the link changes.
    pub const ENGINE_CHANGED: &str = "engine-changed";
    /// The owner asked to quit while the stack runs: the page asks whether to stop it.
    pub const QUIT_REQUESTED: &str = "quit-requested";
    /// A worker joined or left the engine: the page reads the console again.
    pub const CONSOLE_CHANGED: &str = "console-changed";
    /// Carries a job's id each time it changes: the page reads it again. An
    /// empty id means the factory just started, so every page reads again.
    pub const JOB_CHANGED: &str = "job-changed";
}

/// The app's link to the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    Connecting,
    Up,
    Reconnecting,
    Down,
}

/// What the page shows about the engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStatus {
    pub url: String,
    pub link: Link,
    /// The count of registered functions, when the link is up and the engine answered.
    pub functions: Option<usize>,
}

/// The outcome of one Doctor check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Pass,
    /// Failed, but the stack can start without it.
    Warn,
    Fail,
    /// Not run, because what it needs is not there yet (the engine, say).
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub required: bool,
    pub state: CheckState,
    pub detail: String,
    /// What to do about a `Warn` or a `Fail`.
    pub fix: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}

impl DoctorReport {
    /// The stack may start: no required check failed.
    pub fn ready(&self) -> bool {
        !self
            .checks
            .iter()
            .any(|c| c.required && c.state == CheckState::Fail)
    }
}

/// The stack this profile runs, as `iii ps` sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum StackState {
    Stopped,
    Running,
    /// Another engine holds the profile's port.
    Taken {
        pid: u32,
        compose_file: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackStatus {
    pub state: StackState,
    pub compose_file: String,
    /// Where the compose daemon's output goes when the app started it.
    pub log: String,
}

/// Why the stack did not start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum StartRefused {
    /// A required Doctor check failed.
    Doctor {
        report: DoctorReport,
    },
    Taken {
        pid: u32,
        compose_file: Option<String>,
    },
    Failed {
        message: String,
    },
}

/// The profile the app runs with (PLAN.md D12). It holds no secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveProfile {
    pub name: String,
    pub kind: String,
    pub engine_url: String,
    pub checkout: String,
    /// The file the profiles live in.
    pub file: String,
}

/// One container of the compose project (PLAN.md A2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    pub name: String,
    pub version: Option<String>,
    /// `starting`, `ready`, `restarting`, `failed` or `stopped`, as compose says it.
    pub state: String,
    /// The compose daemon started it and can stop it.
    pub owned: bool,
    pub last_error: Option<String>,
}

/// A function or a trigger type the engine has registered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registered {
    pub id: String,
    pub namespace: String,
    pub worker: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queue {
    pub name: String,
    pub depth: u64,
    pub delivered: u64,
    pub failed: u64,
    pub dead_letters: u64,
}

/// Everything the engine console shows, in one read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Console {
    pub containers: Vec<Container>,
    pub functions: Vec<Registered>,
    pub trigger_types: Vec<Registered>,
    pub queues: Vec<Queue>,
}

/// Where the last log read stopped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogCursor {
    pub generation: String,
    pub offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogRequest {
    pub container: String,
    /// `None` reads the most recent lines; a cursor waits for lines after it.
    pub cursor: Option<LogCursor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    /// `stdout` or `stderr`.
    pub stream: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogPage {
    pub lines: Vec<LogLine>,
    pub cursor: Option<LogCursor>,
    /// The cursor was older than the logs kept, so some lines were skipped.
    pub truncated: bool,
}

/// A repository the owner registered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub path: String,
    pub branch: String,
}

/// One job as the floor and the inbox show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSummary {
    pub id: String,
    pub workspace: String,
    pub request: String,
    /// `plan`, `approve`, `build`, `review` or `land`.
    pub stage: String,
    /// `queued`, `provisioning`, `running`, `needs_you`, `landed`, `failed` or `stopped`.
    pub state: String,
    pub question: Option<String>,
    pub error: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    /// When a held checkpoint approves itself, if the job waits at one.
    pub hold_until: Option<u64>,
    /// What the owner may do now: `approve`, `comment`, `answer`, `stop`.
    pub actions: Vec<String>,
    /// The job's pull request, when the forge is GitHub.
    pub pr: Option<String>,
    /// The tracker issue the request came from.
    pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactView {
    pub stage: String,
    pub body: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptLine {
    pub role: String,
    pub text: String,
}

/// One step of a job's pipeline, as a run of steps shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepView {
    pub stage: String,
    /// `done`, `queued`, `running`, `waiting`, `failed`, `stopped` or `pending`.
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobDetail {
    pub job: JobSummary,
    pub steps: Vec<StepView>,
    pub artifacts: Vec<ArtifactView>,
    /// The latest turn's messages.
    pub transcript: Vec<TranscriptLine>,
    /// Calls the gate holds for the owner (PLAN.md A5).
    pub held: Vec<HeldCallView>,
    /// The repository's charter as the job's last turn read it.
    pub charter: Option<CharterView>,
}

/// A workspace's charter for one job (PLAN.md A5). No files means the job
/// runs on the owner's defaults.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharterView {
    pub files: Vec<String>,
    /// Constraints from the repository's `permissions`.
    pub rules: Vec<String>,
    /// The repository's hooks as they ran, latest last.
    pub hook_runs: Vec<String>,
    /// What the latest review diff broke, by constraint.
    pub findings: Vec<String>,
}

/// A call the gate holds: allow it once, or refuse it with feedback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldCallView {
    pub id: String,
    pub function: String,
    /// Its command line or path.
    pub text: String,
    pub at: u64,
}

/// Which held call an owner's decision is for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldCallRef {
    pub job: String,
    pub call: String,
}

/// A rule on its ladder (PLAN.md D20): `told`, `checked` or `enforced`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstraintView {
    pub id: String,
    pub rule: String,
    /// The calls it binds: a function pattern and words the call holds in order.
    /// An empty pattern binds no call.
    pub function: String,
    pub contains: String,
    pub rung: String,
    /// `charter` or `owner`.
    pub source: String,
    /// `charter`, `harness` or `factory`.
    #[serde(default)]
    pub layer: String,
}

/// A grant on its ladder (PLAN.md D20): `denied`, `ask` or `allowed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityView {
    pub stage: String,
    pub function: String,
    pub rung: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntryView {
    pub seq: u64,
    pub at: u64,
    pub kind: String,
    pub subject: String,
    pub detail: String,
    pub hash: String,
}

/// The audit chain and whether it verifies (PLAN.md D21).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditView {
    pub entries: Vec<AuditEntryView>,
    /// The `seq` of the first entry that breaks the chain, if one does.
    pub broken_at: Option<u64>,
    /// Records queued and not yet written.
    pub pending: u64,
    /// Why the last write failed or stalled, if it did.
    pub last_error: Option<String>,
}

/// How each checkpoint is decided (PLAN.md D15): `ask`, `hold` or `auto`.
/// Review never runs below `hold`, whatever it is set to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialsView {
    pub approve: String,
    pub review: String,
}

/// An eval to run (PLAN.md A6): its requests, and the one rung its two arms
/// set differently. `target` is `constraint:<id>` or
/// `capability:<stage>:<function>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewEval {
    pub name: String,
    pub workspace: String,
    pub requests: Vec<String>,
    pub target: String,
    pub rung_a: String,
    pub rung_b: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalSummary {
    pub id: String,
    pub name: String,
    pub workspace: String,
    /// The commit every run started from.
    pub base: String,
    pub created_at: u64,
    pub runs: u32,
    /// Runs that finished and, if they reached review, were graded.
    pub done: u32,
}

/// One run, or an arm's total, as the comparison shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunView {
    /// Empty for a total.
    pub job: String,
    pub state: String,
    pub reviewed: bool,
    pub passed: u32,
    pub refused: u32,
    pub held: u32,
    pub flagged: u32,
    pub hook_refused: u32,
    pub findings: u32,
    pub turns: u32,
    pub cost_micros: u64,
    pub seconds: u64,
    pub grade: Option<u8>,
    /// The audit lines of the calls refused, each with its entry number.
    pub refused_calls: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalRow {
    pub idx: u32,
    pub request: String,
    pub a: Option<RunView>,
    pub b: Option<RunView>,
    /// Each measure that differs, as "name: a -> b".
    pub differences: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalView {
    pub summary: EvalSummary,
    /// The rungs each arm sets.
    pub arm_a: Vec<String>,
    pub arm_b: Vec<String>,
    pub rows: Vec<EvalRow>,
    pub total_a: RunView,
    pub total_b: RunView,
    pub differences: Vec<String>,
}

/// The owner's settings (PLAN.md A7), each value as its word.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsView {
    pub approve: String,
    pub review: String,
    pub plan_provider: String,
    pub plan_model: String,
    pub build_provider: String,
    pub build_model: String,
    /// `local` or `github`.
    pub forge: String,
    /// `none` or `github_issues`.
    pub tracker: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionView {
    pub version: u64,
    pub at: u64,
    pub note: String,
}

/// A model `llm-router` offers on a provider with a key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelOption {
    pub provider: String,
    pub id: String,
    pub name: String,
}

/// One of the owner's accounts with another service, and whether it works.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    pub name: String,
    pub ok: bool,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueView {
    pub number: u64,
    pub title: String,
    pub url: String,
}

/// One workspace's charter, as its repository has it (PLAN.md D25).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceCharterView {
    pub workspace: String,
    pub files: Vec<String>,
    /// Constraints from its `permissions` deny and ask rules.
    pub rules: Vec<String>,
    /// Its hooks: before a call (exit 2 refuses it) and when a build ends.
    pub hooks: Vec<String>,
    /// Its `permissions.allow` rules.
    pub allow: Vec<String>,
}

/// One thing the factory may do, and who decides it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactoryCapabilityView {
    pub name: String,
    /// `allowed`, `ask`, `hold` or `denied`.
    pub rung: String,
    pub how: String,
}
