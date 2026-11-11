//! Every server route as a typed request (DECISIONS.md D38): its method, its
//! path, its body and the type of its answer. A client sends one with
//! `bastion_client::send`, and the server's tests check each against its
//! router, so a route and its request cannot drift apart.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::*;

/// One call to a server. Fields marked `#[serde(skip)]` go in the path;
/// the rest is the JSON body, sent only when `BODY` is true.
pub trait Request: Serialize {
    const METHOD: &'static str;
    /// The route as the server's router names it, `{name}` for a part of
    /// the path.
    const ROUTE: &'static str;
    const BODY: bool = false;
    /// How long the answer may take, in seconds, when longer than a call's
    /// usual limit.
    const WAIT_SECS: Option<u64> = None;
    type Answer: DeserializeOwned;

    /// The path this request goes to.
    fn path(&self) -> String {
        Self::ROUTE.into()
    }
}

/// `/a/{x}/b` with `{x}` replaced by `value`, percent-encoded.
fn fill(route: &str, name: &str, value: &str) -> String {
    route.replace(&format!("{{{name}}}"), &encode(value))
}

/// The route a job's kept screenshot or recording is read from. It
/// answers with the file's bytes, not JSON, so it is no `Request`.
pub const MEDIA_ROUTE: &str = "/jobs/{id}/media/{name}";

/// The path of `job`'s kept file `name`.
pub fn media_path(job: &str, name: &str) -> String {
    fill(&fill(MEDIA_ROUTE, "id", job), "name", name)
}

/// A path segment, percent-encoded.
fn encode(segment: &str) -> String {
    let kept = |b: u8| b.is_ascii_alphanumeric() || b"-._~".contains(&b);
    segment
        .bytes()
        .map(|b| match kept(b) {
            true => (b as char).to_string(),
            false => format!("%{b:02X}"),
        })
        .collect()
}

/// A request with nothing in its path or body.
macro_rules! bare {
    ($(#[$doc:meta])* $name:ident, $method:literal, $route:literal, $answer:ty) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Default, Serialize)]
        pub struct $name;

        impl Request for $name {
            const METHOD: &'static str = $method;
            const ROUTE: &'static str = $route;
            type Answer = $answer;
        }
    };
}

/// A request with one part of its path, `{$part}`, and no body.
macro_rules! at {
    ($(#[$doc:meta])* $name:ident, $method:literal, $route:literal, $part:ident, $answer:ty) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Serialize)]
        pub struct $name {
            #[serde(skip)]
            pub $part: String,
        }

        impl Request for $name {
            const METHOD: &'static str = $method;
            const ROUTE: &'static str = $route;
            type Answer = $answer;
            fn path(&self) -> String {
                fill(Self::ROUTE, stringify!($part), &self.$part)
            }
        }
    };
}

/// A request whose body is one value.
macro_rules! with {
    ($(#[$doc:meta])* $name:ident, $method:literal, $route:literal, $body:ty, $answer:ty) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Serialize)]
        pub struct $name(pub $body);

        impl Request for $name {
            const METHOD: &'static str = $method;
            const ROUTE: &'static str = $route;
            const BODY: bool = true;
            type Answer = $answer;
        }
    };
}

// The engine and the stack.
bare!(EngineStatus, "GET", "/engine", crate::EngineStatus);
bare!(Doctor, "GET", "/doctor", DoctorReport);
bare!(StackStatus, "GET", "/stack", crate::StackStatus);
bare!(StackStart, "POST", "/stack/start", crate::StackStatus);
bare!(StackStop, "POST", "/stack/stop", crate::StackStatus);
bare!(Console, "GET", "/console", crate::Console);
with!(ContainerLogs, "POST", "/console/logs", LogRequest, LogPage);
at!(ContainerStop, "POST", "/console/{name}/stop", name, ());
at!(ContainerStart, "POST", "/console/{name}/start", name, ());

// Workspaces and jobs.
bare!(Workspaces, "GET", "/workspaces", Vec<Workspace>);
with!(WorkspaceAdd, "POST", "/workspaces", Workspace, ());

/// Change a workspace's settings. Its name and its path stay; a repository
/// somewhere else is a new workspace.
#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceEdit {
    #[serde(skip)]
    pub name: String,
    pub branch: String,
    pub checks: String,
    pub max_jobs: Option<u32>,
    pub revision_budget: Option<u32>,
    pub branch_prefix: String,
    pub setup: String,
    pub cleanup: String,
    pub env: String,
    pub gates: String,
    pub autofix: String,
    #[serde(default)]
    pub image: String,
}

impl Request for WorkspaceEdit {
    const METHOD: &'static str = "PUT";
    const ROUTE: &'static str = "/workspaces/{name}";
    const BODY: bool = true;
    type Answer = ();
    fn path(&self) -> String {
        fill(Self::ROUTE, "name", &self.name)
    }
}
/// Draft a new workspace's settings: an agent reads the repository at
/// `path` on `branch`. Answers with the workspace as drafted; nothing is
/// saved.
#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceDraft {
    pub path: String,
    pub branch: String,
}

impl Request for WorkspaceDraft {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/workspaces/draft";
    const BODY: bool = true;
    /// The onboarding turn's limit, and a minute more.
    const WAIT_SECS: Option<u64> = Some(360);
    type Answer = Workspace;
}
bare!(Jobs, "GET", "/jobs", Vec<JobSummary>);
bare!(Costs, "GET", "/costs", CostsView);
at!(Session, "GET", "/sessions/{id}", id, SessionDetail);
bare!(Proposals, "GET", "/proposals", Vec<ProposalView>);
bare!(Verification, "GET", "/verification", Vec<LandedView>);
with!(EscapeAdd, "POST", "/escapes", EscapeReport, EscapeView);
with!(
    ProposalDecide,
    "POST",
    "/proposals/decide",
    ProposalDecision,
    ()
);
at!(Job, "GET", "/jobs/{id}", id, JobDetail);
at!(JobApprove, "POST", "/jobs/{id}/approve", id, ());
at!(JobStop, "POST", "/jobs/{id}/stop", id, ());
at!(JobRetry, "POST", "/jobs/{id}/retry", id, ());
at!(JobExtend, "POST", "/jobs/{id}/extend", id, ());

/// Submit a request: a new job. Answers with its id.
#[derive(Default, Clone, Debug, Serialize)]
pub struct JobSubmit {
    pub workspace: String,
    pub request: String,
    /// `short`, `full` or `full_no_acceptance`; the server's default when none.
    pub pipeline: Option<String>,
    pub workflow: Option<String>,
    /// The model every turn runs on, as `provider/model`; the server's
    /// plan and build models when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// What the change must do, in the requester's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<Screenshot>,
}

/// An image that shows what the request means, by its file name.
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct Screenshot {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl Request for JobSubmit {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/jobs";
    const BODY: bool = true;
    type Answer = String;
}

/// Send a job back with a comment, or answer its question.
#[derive(Clone, Debug, Serialize)]
pub struct JobComment {
    #[serde(skip)]
    pub id: String,
    pub text: String,
}

impl Request for JobComment {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/jobs/{id}/comment";
    const BODY: bool = true;
    type Answer = ();
    fn path(&self) -> String {
        fill(Self::ROUTE, "id", &self.id)
    }
}

/// Set the parts scope proposed, while it waits. One part, or none,
/// keeps the request as one job.
#[derive(Clone, Debug, Serialize)]
pub struct JobParts {
    #[serde(skip)]
    pub id: String,
    pub parts: Vec<String>,
}

impl Request for JobParts {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/jobs/{id}/parts";
    const BODY: bool = true;
    type Answer = ();
    fn path(&self) -> String {
        fill(Self::ROUTE, "id", &self.id)
    }
}

// Governance.
bare!(Dials, "GET", "/dials", DialsView);

/// Set a checkpoint's dial.
#[derive(Clone, Debug, Serialize)]
pub struct DialsSet {
    /// The stage's snake_case name.
    #[serde(skip)]
    pub stage: String,
    /// `lights_on`, `dim` or `dark`.
    pub dial: String,
}

impl Request for DialsSet {
    const METHOD: &'static str = "PUT";
    const ROUTE: &'static str = "/dials/{stage}";
    const BODY: bool = true;
    type Answer = ();
    fn path(&self) -> String {
        fill(Self::ROUTE, "stage", &self.stage)
    }
}

with!(HeldAllow, "POST", "/held/allow", HeldCallRef, ());

/// Refuse a held call, telling the agent why.
#[derive(Clone, Debug, Serialize)]
pub struct HeldRefuse {
    pub held: HeldCallRef,
    pub feedback: String,
}

impl Request for HeldRefuse {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/held/refuse";
    const BODY: bool = true;
    type Answer = ();
}

bare!(Constraints, "GET", "/constraints", Vec<ConstraintView>);
with!(ConstraintAdd, "POST", "/constraints", ConstraintView, ());

/// Move a constraint up or down its ladder.
#[derive(Clone, Debug, Serialize)]
pub struct ConstraintMove {
    #[serde(skip)]
    pub id: String,
    pub up: bool,
}

impl Request for ConstraintMove {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/constraints/{id}/move";
    const BODY: bool = true;
    type Answer = ();
    fn path(&self) -> String {
        fill(Self::ROUTE, "id", &self.id)
    }
}

bare!(Capabilities, "GET", "/capabilities", Vec<CapabilityView>);

/// Move a step's grant of a function up or down its ladder.
#[derive(Clone, Debug, Serialize)]
pub struct CapabilityMove {
    pub stage: String,
    pub function: String,
    pub up: bool,
}

impl Request for CapabilityMove {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/capabilities/move";
    const BODY: bool = true;
    type Answer = ();
}

bare!(
    FactoryCapabilities,
    "GET",
    "/capabilities/factory",
    Vec<FactoryCapabilityView>
);
bare!(Charters, "GET", "/charters", Vec<WorkspaceCharterView>);
bare!(Audit, "GET", "/audit", AuditView);
bare!(
    /// Answers with the exported file's path.
    AuditExport,
    "POST",
    "/audit/export",
    String
);

// Evals.
bare!(Evals, "GET", "/evals", Vec<EvalSummary>);
with!(
    /// Answers with the new eval's id.
    EvalCreate,
    "POST",
    "/evals",
    NewEval,
    String
);
at!(Eval, "GET", "/evals/{id}", id, EvalView);

/// Apply an eval's arm to the factory.
#[derive(Clone, Debug, Serialize)]
pub struct EvalApply {
    #[serde(skip)]
    pub id: String,
    pub arm: String,
}

impl Request for EvalApply {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/evals/{id}/apply";
    const BODY: bool = true;
    type Answer = ();
    fn path(&self) -> String {
        fill(Self::ROUTE, "id", &self.id)
    }
}

// Settings.
bare!(Settings, "GET", "/settings", SettingsView);
with!(SettingsSet, "PUT", "/settings", SettingsView, ());
bare!(Notify, "GET", "/notify", NotifyView);
with!(NotifySetWebhook, "PUT", "/notify", NotifySet, ());
bare!(NotifyTest, "POST", "/notify/test", ());

/// Apply a named preset.
#[derive(Clone, Debug, Serialize)]
pub struct SettingsPreset {
    pub preset: String,
}

impl Request for SettingsPreset {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/settings/preset";
    const BODY: bool = true;
    type Answer = ();
}

bare!(
    SettingsHistory,
    "GET",
    "/settings/history",
    Vec<VersionView>
);

/// Restore a settings version.
#[derive(Clone, Debug, Serialize)]
pub struct SettingsRestore {
    pub version: u64,
}

impl Request for SettingsRestore {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/settings/restore";
    const BODY: bool = true;
    type Answer = ();
}

bare!(Models, "GET", "/models", Vec<ModelOption>);
bare!(Accounts, "GET", "/accounts", Vec<AccountView>);
at!(
    Issues,
    "GET",
    "/issues/{workspace}",
    workspace,
    Vec<IssueView>
);

/// Make a job from a tracker issue. Answers with its id.
#[derive(Clone, Debug, Serialize)]
pub struct JobFromIssue {
    #[serde(skip)]
    pub workspace: String,
    #[serde(skip)]
    pub number: u64,
}

impl Request for JobFromIssue {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/issues/{workspace}/{number}/job";
    type Answer = String;
    fn path(&self) -> String {
        let at = fill(Self::ROUTE, "workspace", &self.workspace);
        fill(&at, "number", &self.number.to_string())
    }
}

// People and tokens.
bare!(Me, "GET", "/me", PersonView);
bare!(People, "GET", "/people", Vec<PersonView>);
with!(PersonAdd, "POST", "/people", PersonView, ());

/// Change a person's role.
#[derive(Clone, Debug, Serialize)]
pub struct PersonRole {
    #[serde(skip)]
    pub name: String,
    /// `admin`, `member` or `viewer`.
    pub role: String,
}

impl Request for PersonRole {
    const METHOD: &'static str = "PUT";
    const ROUTE: &'static str = "/people/{name}/role";
    const BODY: bool = true;
    type Answer = ();
    fn path(&self) -> String {
        fill(Self::ROUTE, "name", &self.name)
    }
}

/// Issue a token for a person. Answers with the token, shown once.
#[derive(Clone, Debug, Serialize)]
pub struct TokenIssue {
    #[serde(skip)]
    pub name: String,
    pub label: String,
}

impl Request for TokenIssue {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/people/{name}/tokens";
    const BODY: bool = true;
    type Answer = String;
    fn path(&self) -> String {
        fill(Self::ROUTE, "name", &self.name)
    }
}

bare!(Tokens, "GET", "/tokens", Vec<TokenView>);
at!(TokenRevoke, "DELETE", "/tokens/{id}", id, ());

// Model providers.
bare!(Providers, "GET", "/providers", Vec<ProviderView>);

/// Keep a provider's key in the stack's vault.
#[derive(Clone, Debug, Serialize)]
pub struct ProviderKeySet {
    #[serde(skip)]
    pub id: String,
    pub key: String,
}

impl Request for ProviderKeySet {
    const METHOD: &'static str = "PUT";
    const ROUTE: &'static str = "/providers/{id}/key";
    const BODY: bool = true;
    type Answer = ();
    fn path(&self) -> String {
        fill(Self::ROUTE, "id", &self.id)
    }
}

at!(ProviderKeyRemove, "DELETE", "/providers/{id}/key", id, ());
bare!(
    CodexSignIn,
    "POST",
    "/providers/openai-codex/sign-in",
    SignIn
);

/// Ask whether a Codex sign-in finished. Answers with its state.
#[derive(Clone, Debug, Serialize)]
pub struct CodexSignInPoll {
    pub login_id: String,
}

impl Request for CodexSignInPoll {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/providers/openai-codex/sign-in/poll";
    const BODY: bool = true;
    type Answer = String;
}

bare!(CodexSignOut, "POST", "/providers/openai-codex/sign-out", ());
bare!(Hosts, "GET", "/providers/hosts", Vec<HostView>);
with!(HostsSet, "PUT", "/providers/hosts", Vec<HostView>, ());

// Workflows.
bare!(Workflows, "GET", "/workflows", Vec<WorkflowView>);

/// Copy a workflow under a new name. Answers with the copy's id.
#[derive(Clone, Debug, Serialize)]
pub struct WorkflowCopy {
    pub from: String,
    pub name: String,
}

impl Request for WorkflowCopy {
    const METHOD: &'static str = "POST";
    const ROUTE: &'static str = "/workflows";
    const BODY: bool = true;
    type Answer = String;
}

/// Save a workflow. Answers with its new version.
#[derive(Clone, Debug, Serialize)]
pub struct WorkflowSave {
    #[serde(skip)]
    pub id: String,
    #[serde(flatten)]
    pub workflow: WorkflowView,
}

impl Request for WorkflowSave {
    const METHOD: &'static str = "PUT";
    const ROUTE: &'static str = "/workflows/{id}";
    const BODY: bool = true;
    type Answer = u64;
    fn path(&self) -> String {
        fill(Self::ROUTE, "id", &self.id)
    }
}

at!(WorkflowDelete, "DELETE", "/workflows/{id}", id, ());

macro_rules! all {
    ($($t:ty),* $(,)?) => { vec![$((<$t>::METHOD, <$t>::ROUTE)),*] };
}

/// The method and route of every request, for the server's router test.
pub fn every() -> Vec<(&'static str, &'static str)> {
    let mut every = factory_requests();
    every.extend(admin_requests());
    every
}

/// The requests on the stack, workspaces, jobs, and how they are governed.
fn factory_requests() -> Vec<(&'static str, &'static str)> {
    all![
        EngineStatus,
        Doctor,
        StackStatus,
        StackStart,
        StackStop,
        Console,
        ContainerLogs,
        ContainerStop,
        ContainerStart,
        Workspaces,
        WorkspaceAdd,
        WorkspaceEdit,
        WorkspaceDraft,
        Jobs,
        Costs,
        Session,
        Proposals,
        ProposalDecide,
        Verification,
        EscapeAdd,
        Job,
        JobSubmit,
        JobApprove,
        JobComment,
        JobParts,
        JobStop,
        JobRetry,
        JobExtend,
        Dials,
        DialsSet,
        HeldAllow,
        HeldRefuse,
        Constraints,
        ConstraintAdd,
        ConstraintMove,
        Capabilities,
        CapabilityMove,
        FactoryCapabilities,
        Charters,
        Audit,
        AuditExport,
    ]
}

/// The requests on evals, settings, people, providers, hosts and workflows.
fn admin_requests() -> Vec<(&'static str, &'static str)> {
    all![
        Evals,
        EvalCreate,
        Eval,
        EvalApply,
        Settings,
        SettingsSet,
        Notify,
        NotifySetWebhook,
        NotifyTest,
        SettingsPreset,
        SettingsHistory,
        SettingsRestore,
        Models,
        Accounts,
        Issues,
        JobFromIssue,
        Me,
        People,
        PersonAdd,
        PersonRole,
        TokenIssue,
        Tokens,
        TokenRevoke,
        Providers,
        ProviderKeySet,
        ProviderKeyRemove,
        CodexSignIn,
        CodexSignInPoll,
        CodexSignOut,
        Hosts,
        HostsSet,
        Workflows,
        WorkflowCopy,
        WorkflowSave,
        WorkflowDelete,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_takes_its_parts_and_the_body_leaves_them_out() {
        let issue = JobFromIssue {
            workspace: "w".into(),
            number: 7,
        };
        assert_eq!(issue.path(), "/issues/w/7/job");
        let comment = JobComment {
            id: "j1".into(),
            text: "x".into(),
        };
        assert_eq!(comment.path(), "/jobs/j1/comment");
        let spaced = WorkflowDelete { id: "a b/c".into() };
        assert_eq!(spaced.path(), "/workflows/a%20b%2Fc");
        assert_eq!(every().len(), 76);
    }
}
