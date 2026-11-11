//! The API's contract (DECISIONS.md D28): every request a client sends and every
//! change the server pushes, with their types. The server and each client
//! depend on this crate, and on nothing of each other's.

pub mod api;

use serde::{Deserialize, Serialize};

/// A change the server pushes (D10, D29). A client reads the named
/// resource again; only the engine's status rides along whole.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// A job changed. An empty id means every job, as after a settings change.
    Job {
        id: String,
    },
    Engine {
        status: EngineStatus,
    },
    /// A worker joined or left the engine.
    Console,
    /// A job came to wait on a person, failed or landed, with how many
    /// jobs on the server wait on a person now.
    Notice {
        job: String,
        workspace: String,
        /// `needs_you`, `failed` or `landed`.
        state: String,
        text: String,
        waiting: u32,
    },
}

/// A Bastion server a client talks to (D28, D30): its name, its base URL and
/// its bearer token. `local` is the one the client starts on this machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    pub name: String,
    pub url: String,
    pub token: String,
}

/// A person on a Bastion server and their role: `admin`, `member` or `viewer` (D31).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonView {
    pub name: String,
    pub role: String,
}

/// A token as the server lists it: never the token itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenView {
    pub id: String,
    pub person: String,
    pub label: String,
    pub created_at: u64,
    pub last_used: Option<u64>,
}

/// A model provider and where its credential comes from: `secret` (a key
/// A workflow (DECISIONS.md D35): its steps on the canvas and the links between
/// them. Each node's `stage` and `dial` are their words, and `level` is
/// `lights_on`, `dim` or `dark`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowView {
    pub id: String,
    pub name: String,
    /// Comes with Bastion, changed or not: deleting a changed one resets it.
    #[serde(default)]
    pub ships: bool,
    pub version: u64,
    pub level: String,
    pub nodes: Vec<NodeView>,
    pub links: Vec<LinkView>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeView {
    /// Unique in its workflow; links name it.
    pub id: String,
    /// `prompt`, `agent`, `skill`, `command`, `shell`, or a Bastion stage.
    pub stage: String,
    /// An agent's, skill's or command's name, or a shell script.
    pub run: String,
    pub dial: String,
    /// The step's instructions; empty keeps the stage's built-in ones.
    pub note: String,
    pub results: Vec<String>,
    /// A shell step's network and the vault's secrets it gets (D40).
    #[serde(default)]
    pub reach: ReachView,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachView {
    pub network: bool,
    /// Environment variable names, such as `GH_TOKEN`.
    pub secrets: Vec<String>,
    /// An agent's step that may not write a file (D42).
    #[serde(default)]
    pub reads_only: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkView {
    pub from: String,
    pub to: String,
}

/// set in Settings), `env`, `signed_in` (a subscription), or `none`. Never
/// the credential (D34).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub source: String,
    /// Whether turns can run on it: a credential, or none needed (a local
    /// host).
    #[serde(default)]
    pub ready: bool,
}

/// An OpenAI-compatible host that serves models as its own provider.
/// `api_url` is its full `/chat/completions` URL; its key, if
/// it needs one, is set like any provider's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostView {
    pub id: String,
    pub name: String,
    pub api_url: String,
}

/// A device-code sign-in under way: open `verification_uri`, enter
/// `user_code` before `expires_at` (Unix seconds), and poll every
/// `interval` seconds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignIn {
    pub login_id: String,
    pub verification_uri: String,
    pub user_code: String,
    pub expires_at: u64,
    pub interval: u64,
}

/// The name of the Bastion server a client starts on this machine.
pub const LOCAL: &str = "local";

/// The server's link to the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    Connecting,
    Up,
    Reconnecting,
    Down,
}

/// What a client shows about the engine.
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

/// The server's stack, as `iii ps` sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum StackState {
    Stopped,
    Running,
    /// Another engine holds the stack's port.
    Taken {
        pid: u32,
        compose_file: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackStatus {
    pub state: StackState,
    pub compose_file: String,
    /// Where the compose daemon's output goes when the server started it.
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

/// One container of the compose project.
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
    /// The command that runs this workspace's tests, such as `pytest -q`.
    /// Empty runs nothing.
    #[serde(default)]
    pub checks: String,
    /// At most this many of its jobs hold a VM at once; none leaves only
    /// the server's own limit.
    #[serde(default)]
    pub max_jobs: Option<u32>,
    /// How many times in all a job's steps may send its work back by
    /// themselves before the job waits on the owner; none leaves each
    /// step's own limit.
    #[serde(default)]
    pub revision_budget: Option<u32>,
    /// What a job's branch starts with, before the job's id.
    #[serde(default = "branch_prefix")]
    pub branch_prefix: String,
    /// A shell command run in the repository, with the network, before a
    /// job's turns; what it makes there goes to the job's offline VM.
    #[serde(default)]
    pub setup: String,
    /// A shell command run in the repository as the job's VM stops.
    #[serde(default)]
    pub cleanup: String,
    /// The VMs' environment, as `K=V` pairs split by spaces. The agent can
    /// read it, so it holds no secrets.
    #[serde(default)]
    pub env: String,
    /// Quality gates run in the VM over a change's files before a push,
    /// one `name: command` per line; `{files}` stands for the files.
    #[serde(default)]
    pub gates: String,
    /// A command run once when a gate fails, before the gates run again.
    #[serde(default)]
    pub autofix: String,
    /// The VM image jobs boot: `python`, `node` or `browser`.
    #[serde(default)]
    pub image: String,
    /// What a job's branch does when its workflow's last step passes
    /// (D41): `merge` into the base branch, or `leave` it.
    #[serde(default)]
    pub finish: String,
}

/// The branch prefix a workspace starts with.
pub fn branch_prefix() -> String {
    "bastion/".into()
}

/// One turn as the Costs page lists it: its job, its step and what it used.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionView {
    pub session: String,
    pub job: String,
    pub workspace: String,
    pub stage: String,
    pub turns: u32,
    pub calls: u32,
    pub call_errors: u32,
    /// Millionths of a US dollar.
    pub cost_micros: u64,
    pub ended_at: u64,
}

/// One step of a turn, in order: what the agent was asked, said or thought,
/// a call it made with the gate's verdict on it, or what a call returned.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnStep {
    /// `asked`, `said`, `thought`, `call` or `result`.
    pub kind: String,
    pub text: String,
    /// The gate's decision on a call, such as `continue` or `deny (...)`;
    /// empty for any other step, or a call the gate has no entry for.
    pub verdict: String,
}

/// One turn, step by step.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDetail {
    pub session: String,
    pub job: String,
    pub steps: Vec<TurnStep>,
}

/// A defect found after a change landed: the commit it is in and what
/// went wrong.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscapeReport {
    pub commit: String,
    pub what: String,
}

/// An escape, traced to the job whose commits hold it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscapeView {
    pub job: String,
    pub commit: String,
    pub what: String,
    pub at: u64,
}

/// A landed job and what checked it: each step's decision and who made it,
/// the commits it landed, and the escapes found in them since.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandedView {
    pub job: String,
    pub workspace: String,
    pub request: String,
    pub landed_at: u64,
    pub commits: Vec<String>,
    /// Such as `code_review: approved by ada`, oldest first.
    pub checks: Vec<String>,
    pub escapes: Vec<EscapeView>,
}

/// What some turns used in all, under one key such as a workspace or a day.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub key: String,
    pub sessions: u32,
    pub turns: u32,
    pub calls: u32,
    pub call_errors: u32,
    pub cost_micros: u64,
}

/// What the turns on a server cost: in all, by workspace, by step and by
/// day, and the newest turns themselves.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostsView {
    pub total: Tally,
    pub by_workspace: Vec<Tally>,
    pub by_stage: Vec<Tally>,
    /// Newest day first, as `YYYY-MM-DD` in UTC.
    pub by_day: Vec<Tally>,
    pub sessions: Vec<SessionView>,
}

/// One job as the floor and the inbox show it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSummary {
    pub id: String,
    pub workspace: String,
    pub request: String,
    /// The kind of step the job is at: `prompt`, `agent`, `skill`,
    /// `command` or `shell`.
    pub stage: String,
    /// The id of the step the job is at; none at the finish.
    #[serde(default)]
    pub node: Option<String>,
    /// `queued`, `provisioning`, `running`, `needs_you`, `landed`, `failed` or `stopped`.
    pub state: String,
    pub question: Option<String>,
    pub error: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    /// What the owner may do now: `approve`, `comment`, `answer`, `stop`.
    pub actions: Vec<String>,
    /// The tracker issue the request came from.
    pub issue: Option<String>,
    /// Who asked for the job (D31).
    #[serde(default)]
    pub requester: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactView {
    pub stage: String,
    /// The id of the step it came from; empty from before steps had ids.
    #[serde(default)]
    pub step: String,
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
    /// The workflow node the step is (D35).
    pub id: String,
    pub stage: String,
    /// `done`, `queued`, `running`, `waiting`, `failed`, `stopped` or `pending`.
    pub state: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobDetail {
    pub job: JobSummary,
    pub steps: Vec<StepView>,
    pub artifacts: Vec<ArtifactView>,
    /// The latest turn's messages.
    pub transcript: Vec<TranscriptLine>,
    /// Calls the gate holds for the owner.
    pub held: Vec<HeldCallView>,
    /// The repository's charter as the job's last turn read it.
    pub charter: Option<CharterView>,
    /// The workflow the job runs, as the version it started on (D35).
    pub workflow: WorkflowView,
    /// Each result its steps gave, by name.
    pub results: std::collections::BTreeMap<String, String>,
    /// The screenshots and recordings its turns kept, by file name.
    #[serde(default)]
    pub media: Vec<String>,
}

/// A workspace's charter for one job. No files means the job
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

/// A rule on its ladder (DECISIONS.md D20): `told`, `checked` or `enforced`.
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

/// A grant on its ladder (DECISIONS.md D20): `denied`, `ask` or `allowed`.
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

/// The audit chain and whether it verifies (DECISIONS.md D21).
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

/// An eval to run: its requests, and the one rung its two arms
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
    /// Whether the run's contract held: its workspace's gates and its
    /// repository's verify contract. None without one, or not yet checked.
    #[serde(default)]
    pub contract: Option<bool>,
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

/// The owner's settings, each value as its word.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsView {
    pub plan_provider: String,
    pub plan_model: String,
    pub build_provider: String,
    pub build_model: String,
    /// `none` or `github_issues`.
    pub tracker: String,
    /// No queued job is admitted while true.
    #[serde(default)]
    pub intake_paused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionView {
    pub version: u64,
    pub at: u64,
    pub note: String,
}

/// Where notices go besides the clients: a chat webhook, shown only
/// as a hint of its address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyView {
    pub webhook: Option<String>,
}

/// A webhook to set, or an empty one to forget it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifySet {
    pub webhook: String,
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

/// One workspace's charter, as its repository has it (DECISIONS.md D25).
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
