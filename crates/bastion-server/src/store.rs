//! The Store seam: workspaces, jobs and artifacts as rows in
//! the `database` worker, each job with a `rev` so a stale write is refused
//! (D8). The governance rows are `GovernStore`'s, and `Store` extends it and
//! the other stores. `SqlStore` is the only code that writes SQL.

use std::future::Future;
use std::pin::Pin;

use bastion_core::govern::Call;
use bastion_core::{Job, Stage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::Engine;
use crate::eval_store::{self, EvalStore};
use crate::govern_store::GovernStore;
use crate::people_store::{self, PeopleStore};
use crate::settings_store::{self, SettingsStore};
use crate::workflow_store::{self, WorkflowStore};

pub type Answer<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// A registered repository.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub path: String,
    pub branch: String,
    /// The command that runs its tests at the checks step; empty runs nothing.
    pub checks: String,
    /// At most this many of its jobs hold a VM at once, when set.
    pub max_jobs: Option<u32>,
    /// How many times in all a job's steps may send its work back, when set.
    pub revision_budget: Option<u32>,
    /// What a job's branch starts with, before the job's id.
    pub branch_prefix: String,
    /// A shell command run, with the network, before a job's turns.
    pub setup: String,
    /// A shell command run as a job's VM stops.
    pub cleanup: String,
    /// The VMs' environment, as `K=V` pairs split by spaces.
    pub env: String,
    /// Quality gates run over a change's files before a push, one
    /// `name: command` per line.
    pub gates: String,
    /// A command run once when a gate fails, before the gates run again.
    pub autofix: String,
}

/// What the adapter holds for a job besides the core's state: where it runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runtime {
    pub worktree: Option<String>,
    pub sandbox: Option<String>,
    pub session: Option<String>,
    /// The tracker issue the request came from, as its URL.
    pub issue: Option<String>,
    /// The person who asked for the job (D31).
    pub requester: Option<String>,
    /// The model the job's turns run on, as `provider/model`, when its
    /// request named one (F8); else the settings' plan and build models.
    pub model: Option<String>,
    /// The workflow the job runs (D35), and the version it started on.
    pub workflow: Option<String>,
    pub workflow_version: Option<u64>,
    /// The approved acceptance test files, which no turn may write.
    pub approved: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub job: Job,
    pub runtime: Runtime,
    pub rev: i64,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub stage: Stage,
    pub body: String,
    pub created_at: u64,
}

/// A call the gate held for the owner, with what is needed to run it later.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldCall {
    /// The harness's `function_call_id`.
    pub id: String,
    pub job: String,
    pub session: String,
    pub turn: String,
    pub call: Call,
    /// The call's arguments as the engine sent them, to run on approval.
    pub arguments: Value,
    pub at: u64,
}

/// Why a job could not be loaded: it does not exist. The HTTP API answers
/// this with 404.
pub fn no_job(id: &str) -> String {
    format!("{NO_JOB}{id}")
}

pub fn is_no_job(error: &str) -> bool {
    error.starts_with(NO_JOB)
}

const NO_JOB: &str = "no job ";

/// A save lost to another writer (D8). The caller reloads and steps again.
pub const STALE: &str = "stale: the job changed since it was read";

pub trait Store:
    EvalStore + GovernStore + PeopleStore + SettingsStore + WorkflowStore + Send + Sync
{
    fn add_workspace<'a>(&'a self, workspace: &'a Workspace) -> Answer<'a, ()>;
    fn workspaces(&self) -> Answer<'_, Vec<Workspace>>;
    /// Save a workspace's settings; its name finds it, and its path stays.
    fn edit_workspace<'a>(&'a self, workspace: &'a Workspace) -> Answer<'a, ()>;
    fn insert<'a>(&'a self, record: &'a Record) -> Answer<'a, ()>;
    fn load<'a>(&'a self, id: &'a str) -> Answer<'a, Record>;
    /// Up to `limit` jobs, newest first, after skipping `offset`.
    fn jobs(&self, limit: usize, offset: usize) -> Answer<'_, Vec<Record>>;
    /// The unfinished jobs, and each job a queued part waits on, newest first.
    fn active(&self) -> Answer<'_, Vec<Record>>;
    /// Write `record` if its `rev` is still current. Returns the new `rev`.
    fn save<'a>(&'a self, record: &'a Record) -> Answer<'a, i64>;
    fn add_artifact<'a>(&'a self, job: &'a str, artifact: &'a Artifact) -> Answer<'a, ()>;
    fn artifacts<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Artifact>>;
    /// Every artifact kept at `stage` as (job, workspace, body), newest first.
    fn artifacts_at(&self, stage: Stage) -> Answer<'_, Vec<(String, String, String)>>;
}

mod schema;
mod sql;

pub use sql::SqlStore;

/// An enum as the snake_case text its column holds.
pub(crate) fn name<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

pub(crate) fn parse<T: for<'de> Deserialize<'de>>(row: &Value, column: &str) -> Result<T, String> {
    serde_json::from_value(row[column].clone()).map_err(|e| format!("column {column}: {e}"))
}

#[cfg(test)]
pub mod fake;

/// The seed file the stack carries, for the tests.
#[cfg(test)]
pub(crate) const SEED: &str = include_str!("../../../seed/governance.sql");

#[cfg(test)]
mod tests;

#[cfg(test)]
mod live;
