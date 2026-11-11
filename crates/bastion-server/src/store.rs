//! The Store seam: workspaces, jobs and artifacts as rows in
//! the `database` worker, each job with a `rev` so a stale write is refused
//! (D8), and the governance rows: constraints, capabilities, held
//! calls and the audit chain. `SqlStore` is the only code that writes SQL.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bastion_core::audit::{self, Entry};
use bastion_core::govern::{Call, Capability, Constraint};
use bastion_core::{Dial, Dials, Job, Stage, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::Engine;
use crate::eval_store::{self, EvalStore};
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

pub trait Store: EvalStore + PeopleStore + SettingsStore + WorkflowStore + Send + Sync {
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
    /// The checkpoint dials. A checkpoint with no row asks.
    fn dials(&self) -> Answer<'_, Dials>;
    fn constraints(&self) -> Answer<'_, Vec<Constraint>>;
    /// Add a constraint, or replace the one with its id.
    fn set_constraint<'a>(&'a self, constraint: &'a Constraint) -> Answer<'a, ()>;
    fn capabilities(&self) -> Answer<'_, Vec<Capability>>;
    /// Add a grant, or replace the one for its stage and function.
    fn set_capability<'a>(&'a self, grant: &'a Capability) -> Answer<'a, ()>;
    fn hold<'a>(&'a self, call: &'a HeldCall) -> Answer<'a, ()>;
    /// The calls held for a job, oldest first.
    fn held<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<HeldCall>>;
    /// Take a held call off the list. True only for the one caller that
    /// removed it, so two people settling the same call cannot both run it.
    fn claim<'a>(&'a self, id: &'a str) -> Answer<'a, bool>;
    /// The whole audit chain, oldest first.
    fn audit(&self) -> Answer<'_, Vec<Entry>>;
    /// The gate's entries for one job, oldest first.
    fn gate_entries<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Entry>>;
    /// Every entry of one kind, oldest first.
    fn entries_of<'a>(&'a self, kind: &'a str) -> Answer<'a, Vec<Entry>>;
    /// The newest entry, the one the next appends after.
    fn audit_tail(&self) -> Answer<'_, Option<Entry>>;
    /// Add the next entry. A second entry with the same `seq` is refused, so
    /// two writers cannot fork the chain.
    fn append_audit<'a>(&'a self, entry: &'a Entry) -> Answer<'a, ()>;
}

const NAMESPACE: &str = "bastion";

/// `CHECK (column IN (...))` over every stage the core has.
fn stage_check(column: &str) -> String {
    let names: Vec<String> = Stage::ALL
        .iter()
        .map(|s| format!("'{}'", name(s)))
        .collect();
    format!("CHECK ({column} IN ({}))", names.join(","))
}

/// The tables whose rows name a stage, with the columns a rebuild keeps.
/// SQLite cannot change a `CHECK` in place, so a new stage in the core means
/// a new migration that rebuilds these (`migrations`).
fn staged_tables() -> [(&'static str, &'static str, String); 4] {
    let stage = stage_check("stage");
    [
        jobs_table(&stage),
        (
            "artifacts",
            "id, job, stage, body, created_at",
            format!(
                "CREATE TABLE IF NOT EXISTS artifacts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        job TEXT NOT NULL REFERENCES jobs(id),
        stage TEXT NOT NULL {stage},
        body TEXT NOT NULL,
        created_at INTEGER NOT NULL)"
            ),
        ),
        (
            "capabilities",
            "stage, function, rung",
            format!(
                "CREATE TABLE IF NOT EXISTS capabilities (
        stage TEXT NOT NULL {stage},
        function TEXT NOT NULL CHECK (length(function) > 0),
        rung TEXT NOT NULL CHECK (rung IN ('denied','ask','allowed')),
        PRIMARY KEY (stage, function))"
            ),
        ),
        turns_table(&stage),
    ]
}

/// `jobs`, under the stage check `stage`.
fn jobs_table(stage: &str) -> (&'static str, &'static str, String) {
    let back_to = stage_check("back_to");
    (
        "jobs",
        LEGACY_JOB_COLUMNS,
        format!(
                "CREATE TABLE IF NOT EXISTS jobs (
        id TEXT PRIMARY KEY,
        workspace TEXT NOT NULL REFERENCES workspaces(name),
        request TEXT NOT NULL CHECK (length(request) > 0),
        stage TEXT NOT NULL {stage},
        state TEXT NOT NULL CHECK (state IN ('queued','provisioning','running','needs_you','landed','failed','stopped')),
        question TEXT,
        feedback TEXT,
        plan TEXT,
        failures INTEGER NOT NULL CHECK (failures >= 0),
        error TEXT,
        worktree TEXT,
        sandbox TEXT,
        session TEXT,
        rev INTEGER NOT NULL CHECK (rev >= 1),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        pr TEXT,
        ci_fixes INTEGER NOT NULL DEFAULT 0 CHECK (ci_fixes >= 0),
        issue TEXT,
        pipeline TEXT NOT NULL DEFAULT 'short' CHECK (pipeline IN ('short','full','full_no_acceptance')),
        back_to TEXT {back_to},
        last_refusal TEXT,
        done_tasks INTEGER NOT NULL DEFAULT 0 CHECK (done_tasks >= 0),
        criteria TEXT,
        falsify_tried INTEGER NOT NULL DEFAULT 0 CHECK (falsify_tried IN (0, 1)),
        requester TEXT,
        workflow TEXT,
        workflow_version INTEGER,
        node TEXT,
        parent TEXT,
        after_job TEXT,
        part TEXT,
        approved TEXT)"
            ),
    )
}

/// `turns`, under the stage check `stage`.
fn turns_table(stage: &str) -> (&'static str, &'static str, String) {
    (
        "turns",
        TURN_COLUMNS,
        format!(
            "CREATE TABLE IF NOT EXISTS turns (
    session TEXT PRIMARY KEY,
    job TEXT NOT NULL REFERENCES jobs(id),
    stage TEXT NOT NULL {stage},
    turns INTEGER NOT NULL CHECK (turns >= 0),
    calls INTEGER NOT NULL CHECK (calls >= 0),
    call_errors INTEGER NOT NULL CHECK (call_errors >= 0),
    cost_micros INTEGER NOT NULL CHECK (cost_micros >= 0),
    ended_at INTEGER NOT NULL)"
        ),
    )
}

/// The columns of `turns`, which named only the short pipeline's
/// stages until migration 2.
const TURN_COLUMNS: &str = "session, job, stage, turns, calls, call_errors, cost_micros, ended_at";

/// The schema (D8): typed columns with constraints, no JSON in a text column.
fn schema() -> Vec<String> {
    let staged = staged_tables().map(|(_, _, sql)| sql);
    SCHEMA.map(String::from).into_iter().chain(staged).collect()
}

const SCHEMA: [&str; 8] = [
    "CREATE TABLE IF NOT EXISTS workspaces (
        name TEXT PRIMARY KEY CHECK (length(name) > 0),
        path TEXT NOT NULL UNIQUE,
        branch TEXT NOT NULL CHECK (length(branch) > 0),
        created_at INTEGER NOT NULL,
        checks TEXT NOT NULL DEFAULT '',
        max_jobs INTEGER CHECK (max_jobs > 0),
        revision_budget INTEGER CHECK (revision_budget > 0),
        branch_prefix TEXT NOT NULL DEFAULT 'bastion/',
        setup TEXT NOT NULL DEFAULT '',
        cleanup TEXT NOT NULL DEFAULT '',
        env TEXT NOT NULL DEFAULT '',
        gates TEXT NOT NULL DEFAULT '',
        autofix TEXT NOT NULL DEFAULT '')",
    // Discovery's tasks for implement, in order.
    "CREATE TABLE IF NOT EXISTS tasks (
        job TEXT NOT NULL REFERENCES jobs(id),
        n INTEGER NOT NULL CHECK (n >= 0),
        text TEXT NOT NULL CHECK (length(text) > 0),
        PRIMARY KEY (job, n))",
    // The results a job's steps gave (D35), each kept as its document.
    "CREATE TABLE IF NOT EXISTS job_results (
        job TEXT NOT NULL REFERENCES jobs(id),
        name TEXT NOT NULL CHECK (length(name) > 0),
        body TEXT NOT NULL,
        PRIMARY KEY (job, name))",
    // How many times each step sent a job's work back.
    "CREATE TABLE IF NOT EXISTS revisions (
        job TEXT NOT NULL REFERENCES jobs(id),
        stage TEXT NOT NULL CHECK (length(stage) > 0),
        count INTEGER NOT NULL CHECK (count >= 1),
        PRIMARY KEY (job, stage))",
    "CREATE TABLE IF NOT EXISTS dials (
        checkpoint TEXT PRIMARY KEY CHECK (checkpoint IN ('approve','review')),
        dial TEXT NOT NULL CHECK (dial IN ('ask','hold','auto')))",
    "CREATE TABLE IF NOT EXISTS constraints (
        id TEXT PRIMARY KEY CHECK (length(id) > 0),
        rule TEXT NOT NULL CHECK (length(rule) > 0),
        function TEXT NOT NULL,
        contains TEXT NOT NULL,
        rung TEXT NOT NULL CHECK (rung IN ('told','checked','enforced')),
        source TEXT NOT NULL CHECK (source IN ('charter','owner')))",
    // `arguments` is the engine's JSON, kept opaque to run the call later.
    "CREATE TABLE IF NOT EXISTS held_calls (
        id TEXT PRIMARY KEY,
        job TEXT NOT NULL REFERENCES jobs(id),
        session TEXT NOT NULL,
        turn TEXT NOT NULL,
        function TEXT NOT NULL,
        text TEXT NOT NULL,
        arguments TEXT NOT NULL,
        at INTEGER NOT NULL)",
    // The keyed chain (D21). An older unkeyed `audit` table, if any, is left as it was.
    "CREATE TABLE IF NOT EXISTS audit_log (
        seq INTEGER PRIMARY KEY CHECK (seq >= 1),
        at INTEGER NOT NULL,
        kind TEXT NOT NULL,
        subject TEXT NOT NULL,
        detail TEXT NOT NULL,
        prev TEXT NOT NULL CHECK (length(prev) = 64),
        hash TEXT NOT NULL UNIQUE CHECK (length(hash) = 64))",
];

const UPSERT_CONSTRAINT: &str = "INSERT INTO constraints (id, rule, function, contains, rung, source) VALUES (?, ?, ?, ?, ?, ?)
    ON CONFLICT (id) DO UPDATE SET rule = excluded.rule, function = excluded.function, contains = excluded.contains, rung = excluded.rung, source = excluded.source";
const UPSERT_CAPABILITY: &str = "INSERT INTO capabilities (stage, function, rung) VALUES (?, ?, ?)
    ON CONFLICT (stage, function) DO UPDATE SET rung = excluded.rung";

fn constraint_params(c: &Constraint) -> Value {
    json!([
        c.id,
        c.rule,
        c.function,
        c.contains,
        name(c.rung),
        name(c.source)
    ])
}

fn capability_params(g: &Capability) -> Value {
    json!([name(g.stage), g.function, name(g.rung)])
}

/// The statements in a seed file (D26): `--` comment lines dropped, split on `;`.
// ponytail: a plain split; a seed value that holds a `;` needs a real SQL tokenizer.
pub fn seed_statements(sql: &str) -> Vec<Value> {
    let kept: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    kept.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| json!({ "sql": s, "params": [] }))
        .collect()
}

const JOB_COLUMNS: &str = "id, workspace, request, stage, state, question, feedback, plan, failures, error, worktree, sandbox, session, rev, created_at, updated_at, pr, ci_fixes, issue, pipeline, back_to, last_refusal, done_tasks, criteria, falsify_tried, requester, workflow, workflow_version, node, parent, after_job, part, approved";
/// The jobs the admission pump and resume need: every unfinished one, and
/// every job a queued part waits on, so a part whose earlier part failed
/// is seen as abandoned rather than admitted.
const ACTIVE: &str = "state NOT IN ('landed','failed','stopped') OR id IN (SELECT after_job FROM jobs WHERE state = 'queued')";
/// The job columns before migration 1, which its rebuild copies.
const LEGACY_JOB_COLUMNS: &str = "id, workspace, request, stage, state, question, feedback, plan, failures, error, worktree, sandbox, session, rev, created_at, updated_at, pr, ci_fixes, issue";

/// Columns added after a table was first made. Each runs once; on a table
/// that already has the column it fails, and that failure is expected.
// They run before the numbered migrations, which need these columns to exist.
const ALTERS: [&str; 20] = [
    "ALTER TABLE jobs ADD COLUMN pr TEXT",
    "ALTER TABLE jobs ADD COLUMN ci_fixes INTEGER NOT NULL DEFAULT 0 CHECK (ci_fixes >= 0)",
    "ALTER TABLE jobs ADD COLUMN issue TEXT",
    "ALTER TABLE workspaces ADD COLUMN checks TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE jobs ADD COLUMN requester TEXT",
    "ALTER TABLE jobs ADD COLUMN workflow TEXT",
    "ALTER TABLE jobs ADD COLUMN workflow_version INTEGER",
    "ALTER TABLE jobs ADD COLUMN node TEXT",
    "ALTER TABLE jobs ADD COLUMN parent TEXT",
    "ALTER TABLE jobs ADD COLUMN after_job TEXT",
    "ALTER TABLE jobs ADD COLUMN part TEXT",
    "ALTER TABLE jobs ADD COLUMN approved TEXT",
    "ALTER TABLE workspaces ADD COLUMN max_jobs INTEGER CHECK (max_jobs > 0)",
    "ALTER TABLE workspaces ADD COLUMN branch_prefix TEXT NOT NULL DEFAULT 'bastion/'",
    "ALTER TABLE workspaces ADD COLUMN setup TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workspaces ADD COLUMN cleanup TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workspaces ADD COLUMN env TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workspaces ADD COLUMN gates TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workspaces ADD COLUMN autofix TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workspaces ADD COLUMN revision_budget INTEGER CHECK (revision_budget > 0)",
];

/// Numbered migrations, each run once in order; `PRAGMA user_version` is the
/// last one applied. Migration 1 rebuilds the tables that name a stage, for
/// the full pipeline; migration 2 rebuilds `turns`, which it missed;
/// migration 3 rebuilds them all for a workflow's own steps (D35).
fn migrations() -> Vec<Vec<String>> {
    let staged = staged_tables();
    vec![
        staged.iter().flat_map(rebuild).collect(),
        staged
            .iter()
            .filter(|t| t.0 == "turns")
            .flat_map(rebuild)
            .collect(),
        // Every column now, as the alters ran before the migrations.
        staged
            .iter()
            .map(|(table, columns, create)| {
                let columns = if *table == "jobs" {
                    JOB_COLUMNS
                } else {
                    columns
                };
                (*table, columns, create.clone())
            })
            .collect::<Vec<_>>()
            .iter()
            .flat_map(rebuild)
            .collect(),
    ]
}

/// Rebuild a table under its new schema, keeping its rows. The rows go back
/// under the same name, so with deferred foreign keys the rows that reference
/// it still resolve at commit. Renaming a new table into place does not:
/// both were tried on a copy of `data/bastion.db` with foreign keys on.
fn rebuild((table, columns, create): &(&str, &str, String)) -> Vec<String> {
    vec![
        format!("CREATE TEMP TABLE {table}_copy AS SELECT * FROM {table}"),
        format!("DROP TABLE {table}"),
        create.clone(),
        format!("INSERT INTO {table} ({columns}) SELECT {columns} FROM {table}_copy"),
        format!("DROP TABLE {table}_copy"),
    ]
}

pub struct SqlStore {
    engine: Arc<dyn Engine>,
}

impl SqlStore {
    /// Make the tables if they are missing, then run the seed file's statements.
    pub async fn open(engine: Arc<dyn Engine>, seed: &str) -> Result<Self, String> {
        let schema = schema();
        let statements: Vec<Value> = schema
            .iter()
            .map(String::as_str)
            .chain(eval_store::SCHEMA)
            .chain(people_store::SCHEMA)
            .chain(settings_store::SCHEMA)
            .chain(workflow_store::SCHEMA)
            .map(|sql| json!({ "sql": sql, "params": [] }))
            .chain(seed_statements(seed))
            .collect();
        engine
            .call(
                NAMESPACE,
                "database::transaction",
                json!({ "statements": statements }),
            )
            .await?;
        for sql in ALTERS {
            let applied = engine
                .call(
                    NAMESPACE,
                    "database::execute",
                    json!({ "sql": sql, "params": [] }),
                )
                .await;
            if let Some(e) = applied
                .as_ref()
                .err()
                .filter(|e| !e.contains("duplicate column"))
            {
                return Err(format!("{sql}: {e}"));
            }
        }
        let store = Self { engine };
        store.migrate().await?;
        Ok(store)
    }

    /// Run each migration past the database's `user_version`, each in one
    /// transaction that also records its number.
    async fn migrate(&self) -> Result<(), String> {
        let rows = self.query("PRAGMA user_version", json!([])).await?;
        let applied = rows
            .first()
            .and_then(|r| r["user_version"].as_u64())
            .unwrap_or(0) as usize;
        for (i, migration) in migrations().into_iter().enumerate().skip(applied) {
            let statements = std::iter::once("PRAGMA defer_foreign_keys = ON".to_string())
                .chain(migration)
                .chain([format!("PRAGMA user_version = {}", i + 1)])
                .map(|sql| json!({ "sql": sql, "params": [] }))
                .collect();
            self.transaction(statements)
                .await
                .map_err(|e| format!("migration {}: {e}", i + 1))?;
        }
        Ok(())
    }

    /// Run statements in one transaction.
    pub(crate) async fn transaction(&self, statements: Vec<Value>) -> Result<(), String> {
        self.engine
            .call(
                NAMESPACE,
                "database::transaction",
                json!({ "statements": statements }),
            )
            .await
            .map(|_| ())
    }

    async fn query(&self, sql: &str, params: Value) -> Result<Vec<Value>, String> {
        let out = self
            .engine
            .call(
                NAMESPACE,
                "database::query",
                json!({ "sql": sql, "params": params }),
            )
            .await?;
        Ok(out["rows"].as_array().cloned().unwrap_or_default())
    }

    /// Each row of a query, read by `row`.
    pub(crate) async fn rows<T>(
        &self,
        sql: &str,
        params: Value,
        row: fn(&Value) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        self.query(sql, params).await?.iter().map(row).collect()
    }

    /// Like `rows`, for a list a person browses: a row that does not parse
    /// is logged and left out, so one bad row does not hide the rest. Never
    /// for the audit chain or the rules the gate judges by, where a missing
    /// row would change the answer.
    pub(crate) async fn listed<T>(
        &self,
        sql: &str,
        params: Value,
        row: fn(&Value) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        let all = self.query(sql, params).await?;
        let parsed = all.iter().map(row).filter_map(|r| {
            r.map_err(|e| eprintln!("a row left out of a list: {e}"))
                .ok()
        });
        Ok(parsed.collect())
    }

    /// Fill each job's tasks, results and revision counts from their tables. `filter`
    /// narrows both reads, with `params` for its marks.
    async fn with_lists(
        &self,
        mut records: Vec<Record>,
        filter: &str,
        params: Value,
    ) -> Result<Vec<Record>, String> {
        let tasks = self
            .query(
                &format!("SELECT job, text FROM tasks {filter} ORDER BY job, n"),
                params.clone(),
            )
            .await?;
        let results = self
            .query(
                &format!("SELECT job, name, body FROM job_results {filter}"),
                params.clone(),
            )
            .await?;
        let revisions = self
            .query(
                &format!("SELECT job, stage, count FROM revisions {filter}"),
                params,
            )
            .await?;
        for r in &mut records {
            let id = r.job.id.as_str();
            let mine = |row: &&Value| row["job"].as_str() == Some(id);
            for row in tasks.iter().filter(mine) {
                r.job.tasks.push(parse(row, "text")?);
            }
            for row in results.iter().filter(mine) {
                r.job
                    .results
                    .insert(parse(row, "name")?, parse(row, "body")?);
            }
            for row in revisions.iter().filter(mine) {
                r.job
                    .revisions
                    .insert(parse(row, "stage")?, parse(row, "count")?);
            }
        }
        workflow_store::with_flows(self, records).await
    }

    pub(crate) async fn execute(&self, sql: &str, params: Value) -> Result<Vec<Value>, String> {
        let out = self
            .engine
            .call(
                NAMESPACE,
                "database::execute",
                json!({ "sql": sql, "params": params }),
            )
            .await?;
        Ok(out["returned_rows"].as_array().cloned().unwrap_or_default())
    }
}

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

fn text(row: &Value, column: &str) -> Option<String> {
    row[column].as_str().map(String::from)
}

/// A live job kept at a checkpoint before D33 waits on the step it judged.
fn record(row: &Value) -> Result<Record, String> {
    let state: State = parse(row, "state")?;
    let stage: Stage = parse(row, "stage")?;
    Ok(Record {
        job: Job {
            id: parse(row, "id")?,
            workspace: parse(row, "workspace")?,
            request: parse(row, "request")?,
            stage: if state.finished() {
                stage
            } else {
                bastion_core::folded(stage)
            },
            state,
            question: text(row, "question"),
            feedback: text(row, "feedback"),
            plan: text(row, "plan"),
            failures: parse(row, "failures")?,
            error: text(row, "error"),
            pr: text(row, "pr"),
            ci_fixes: row["ci_fixes"].as_u64().unwrap_or(0) as u32,
            updated_at: parse(row, "updated_at")?,
            pipeline: parse(row, "pipeline")?,
            back_to: parse(row, "back_to")?,
            // Filled from their own tables (`with_lists`).
            revisions: Default::default(),
            last_refusal: text(row, "last_refusal"),
            tasks: Vec::new(),
            flow: Vec::new(),
            node: text(row, "node"),
            results: Default::default(),
            parent: text(row, "parent"),
            after: text(row, "after_job"),
            part: text(row, "part"),
            done_tasks: parse(row, "done_tasks")?,
            criteria: text(row, "criteria"),
            falsify_tried: row["falsify_tried"].as_i64() == Some(1)
                || row["falsify_tried"].as_bool() == Some(true),
        },
        runtime: Runtime {
            worktree: text(row, "worktree"),
            sandbox: text(row, "sandbox"),
            session: text(row, "session"),
            issue: text(row, "issue"),
            requester: text(row, "requester"),
            workflow: text(row, "workflow"),
            workflow_version: row["workflow_version"].as_u64(),
            approved: text(row, "approved")
                .map(|files| files.lines().map(String::from).collect())
                .unwrap_or_default(),
        },
        rev: parse(row, "rev")?,
        created_at: parse(row, "created_at")?,
    })
}

fn constraint(r: &Value) -> Result<Constraint, String> {
    Ok(Constraint {
        id: parse(r, "id")?,
        rule: parse(r, "rule")?,
        function: parse(r, "function")?,
        contains: parse(r, "contains")?,
        rung: parse(r, "rung")?,
        source: parse(r, "source")?,
    })
}

fn capability(r: &Value) -> Result<Capability, String> {
    Ok(Capability {
        stage: parse(r, "stage")?,
        function: parse(r, "function")?,
        rung: parse(r, "rung")?,
    })
}

fn held_call(row: &Value) -> Result<HeldCall, String> {
    let arguments: String = parse(row, "arguments")?;
    Ok(HeldCall {
        id: parse(row, "id")?,
        job: parse(row, "job")?,
        session: parse(row, "session")?,
        turn: parse(row, "turn")?,
        call: Call {
            content: String::new(),
            function: parse(row, "function")?,
            text: parse(row, "text")?,
        },
        arguments: serde_json::from_str(&arguments)
            .map_err(|e| format!("column arguments: {e}"))?,
        at: parse(row, "at")?,
    })
}

fn entry(row: &Value) -> Result<Entry, String> {
    Ok(Entry {
        seq: parse(row, "seq")?,
        at: parse(row, "at")?,
        record: audit::Record {
            kind: parse(row, "kind")?,
            subject: parse(row, "subject")?,
            detail: parse(row, "detail")?,
        },
        prev: parse(row, "prev")?,
        hash: parse(row, "hash")?,
    })
}

/// The approved files as one column, one per line, or NULL when none.
fn approved(r: &Record) -> Option<String> {
    let files = &r.runtime.approved;
    (!files.is_empty()).then(|| files.join("\n"))
}

fn job_params(r: &Record) -> Value {
    let j = &r.job;
    json!([
        j.id,
        j.workspace,
        j.request,
        name(j.stage),
        name(j.state),
        j.question,
        j.feedback,
        j.plan,
        j.failures,
        j.error,
        r.runtime.worktree,
        r.runtime.sandbox,
        r.runtime.session,
        r.rev,
        r.created_at,
        j.updated_at,
        j.pr,
        j.ci_fixes,
        r.runtime.issue,
        name(j.pipeline),
        j.back_to.map(name),
        j.last_refusal,
        j.done_tasks,
        j.criteria,
        j.falsify_tried,
        r.runtime.requester,
        r.runtime.workflow,
        r.runtime.workflow_version,
        j.node,
        j.parent,
        j.after,
        j.part,
        approved(r)
    ])
}

/// A job's tasks, results and revision counts, written whole over what was there.
fn list_statements(j: &Job) -> Vec<Value> {
    let statement = |sql: &str, params: Value| json!({ "sql": sql, "params": params });
    let tasks = j.tasks.iter().enumerate().map(|(n, text)| {
        statement(
            "INSERT INTO tasks (job, n, text) VALUES (?, ?, ?)",
            json!([j.id, n, text]),
        )
    });
    let results = j.results.iter().map(|(name, body)| {
        statement(
            "INSERT INTO job_results (job, name, body) VALUES (?, ?, ?)",
            json!([j.id, name, body]),
        )
    });
    let revisions = j.revisions.iter().map(|(stage, count)| {
        statement(
            "INSERT INTO revisions (job, stage, count) VALUES (?, ?, ?)",
            json!([j.id, name(stage), count]),
        )
    });
    [
        statement("DELETE FROM tasks WHERE job = ?", json!([j.id])),
        statement("DELETE FROM job_results WHERE job = ?", json!([j.id])),
        statement("DELETE FROM revisions WHERE job = ?", json!([j.id])),
    ]
    .into_iter()
    .chain(tasks)
    .chain(results)
    .chain(revisions)
    .collect()
}

impl Store for SqlStore {
    fn add_workspace<'a>(&'a self, w: &'a Workspace) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(
                "INSERT INTO workspaces (name, path, branch, checks, max_jobs, revision_budget, branch_prefix, setup, cleanup, env, gates, autofix, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch())",
                json!([w.name, w.path, w.branch, w.checks, w.max_jobs, w.revision_budget, w.branch_prefix, w.setup, w.cleanup, w.env, w.gates, w.autofix]),
            )
            .await
            .map(|_| ())
        })
    }

    fn workspaces(&self) -> Answer<'_, Vec<Workspace>> {
        Box::pin(async move {
            self.query(
                "SELECT name, path, branch, checks, max_jobs, revision_budget, branch_prefix, setup, cleanup, env, gates, autofix FROM workspaces ORDER BY name",
                json!([]),
            )
            .await?
            .iter()
            .map(|r| {
                Ok(Workspace {
                    name: parse(r, "name")?,
                    path: parse(r, "path")?,
                    branch: parse(r, "branch")?,
                    checks: parse(r, "checks")?,
                    max_jobs: parse(r, "max_jobs")?,
                    revision_budget: parse(r, "revision_budget")?,
                    branch_prefix: parse(r, "branch_prefix")?,
                    setup: parse(r, "setup")?,
                    cleanup: parse(r, "cleanup")?,
                    env: parse(r, "env")?,
                    gates: parse(r, "gates")?,
                    autofix: parse(r, "autofix")?,
                })
            })
            .collect()
        })
    }

    fn edit_workspace<'a>(&'a self, w: &'a Workspace) -> Answer<'a, ()> {
        Box::pin(async move {
            let found = self
                .execute(
                    "UPDATE workspaces SET branch = ?, checks = ?, max_jobs = ?, revision_budget = ?, branch_prefix = ?, setup = ?, cleanup = ?, env = ?, gates = ?, autofix = ? WHERE name = ? RETURNING name",
                    json!([w.branch, w.checks, w.max_jobs, w.revision_budget, w.branch_prefix, w.setup, w.cleanup, w.env, w.gates, w.autofix, w.name]),
                )
                .await?;
            match found.is_empty() {
                true => Err(format!("no workspace named {}", w.name)),
                false => Ok(()),
            }
        })
    }

    fn insert<'a>(&'a self, r: &'a Record) -> Answer<'a, ()> {
        Box::pin(async move {
            let marks = vec!["?"; JOB_COLUMNS.split(", ").count()].join(", ");
            let sql = format!("INSERT INTO jobs ({JOB_COLUMNS}) VALUES ({marks})");
            self.execute(&sql, job_params(r)).await?;
            self.transaction(list_statements(&r.job)).await
        })
    }

    fn load<'a>(&'a self, id: &'a str) -> Answer<'a, Record> {
        Box::pin(async move {
            let rows = self
                .query(
                    &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?"),
                    json!([id]),
                )
                .await?;
            let found = record(rows.first().ok_or_else(|| no_job(id))?)?;
            let mut one = self
                .with_lists(vec![found], "WHERE job = ?", json!([id]))
                .await?;
            Ok(one.remove(0))
        })
    }

    fn jobs(&self, limit: usize, offset: usize) -> Answer<'_, Vec<Record>> {
        Box::pin(async move {
            let page = "ORDER BY created_at DESC, id LIMIT ? OFFSET ?";
            let sql = format!("SELECT {JOB_COLUMNS} FROM jobs {page}");
            let some = self.listed(&sql, json!([limit, offset]), record).await?;
            let filter = format!("WHERE job IN (SELECT id FROM jobs {page})");
            self.with_lists(some, &filter, json!([limit, offset])).await
        })
    }

    fn active(&self) -> Answer<'_, Vec<Record>> {
        Box::pin(async move {
            let sql = format!(
                "SELECT {JOB_COLUMNS} FROM jobs WHERE {ACTIVE} ORDER BY created_at DESC, id"
            );
            let some = self.listed(&sql, json!([]), record).await?;
            let filter = format!("WHERE job IN (SELECT id FROM jobs WHERE {ACTIVE})");
            self.with_lists(some, &filter, json!([])).await
        })
    }

    fn save<'a>(&'a self, r: &'a Record) -> Answer<'a, i64> {
        Box::pin(async move {
            let j = &r.job;
            let rows = self
                .execute(
                    "UPDATE jobs SET stage = ?, state = ?, question = ?, feedback = ?, plan = ?, failures = ?, error = ?, worktree = ?, sandbox = ?, session = ?, updated_at = ?, pr = ?, ci_fixes = ?, issue = ?,
                     pipeline = ?, back_to = ?, last_refusal = ?, done_tasks = ?, criteria = ?, falsify_tried = ?, node = ?, part = ?, approved = ?, rev = rev + 1
                     WHERE id = ? AND rev = ? RETURNING rev",
                    json!([
                        name(j.stage), name(j.state), j.question, j.feedback, j.plan, j.failures, j.error,
                        r.runtime.worktree, r.runtime.sandbox, r.runtime.session, j.updated_at, j.pr, j.ci_fixes, r.runtime.issue,
                        name(j.pipeline), j.back_to.map(name), j.last_refusal, j.done_tasks, j.criteria, j.falsify_tried, j.node, j.part, approved(r), j.id, r.rev
                    ]),
                )
                .await?;
            let rev = rows
                .first()
                .map(|row| parse(row, "rev"))
                .unwrap_or_else(|| Err(STALE.into()))?;
            // ponytail: the lists are written after the row, not in its transaction; a crash between them loses a count.
            self.transaction(list_statements(j)).await?;
            Ok(rev)
        })
    }

    fn add_artifact<'a>(&'a self, job: &'a str, a: &'a Artifact) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(
                "INSERT INTO artifacts (job, stage, body, created_at) VALUES (?, ?, ?, ?)",
                json!([job, name(a.stage), a.body, a.created_at]),
            )
            .await
            .map(|_| ())
        })
    }

    fn artifacts_at(&self, stage: Stage) -> Answer<'_, Vec<(String, String, String)>> {
        Box::pin(async move {
            let sql = "SELECT a.job, j.workspace, a.body FROM artifacts a JOIN jobs j ON j.id = a.job WHERE a.stage = ? ORDER BY a.id DESC";
            let row = |r: &Value| Ok((parse(r, "job")?, parse(r, "workspace")?, parse(r, "body")?));
            self.rows(sql, json!([name(stage)]), row).await
        })
    }

    fn artifacts<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Artifact>> {
        Box::pin(async move {
            self.query(
                "SELECT stage, body, created_at FROM artifacts WHERE job = ? ORDER BY id",
                json!([job]),
            )
            .await?
            .iter()
            .map(|r| {
                Ok(Artifact {
                    stage: parse(r, "stage")?,
                    body: parse(r, "body")?,
                    created_at: parse(r, "created_at")?,
                })
            })
            .collect()
        })
    }

    fn dials(&self) -> Answer<'_, Dials> {
        Box::pin(async move {
            let mut dials = Dials::default();
            for row in self
                .query("SELECT checkpoint, dial FROM dials", json!([]))
                .await?
            {
                let dial: Dial = parse(&row, "dial")?;
                let judged: &[Stage] = match row["checkpoint"].as_str() {
                    Some("approve") => &[Stage::Plan],
                    Some("review") => &[Stage::Build, Stage::CodeReview],
                    _ => &[],
                };
                for stage in judged {
                    dials.set(*stage, dial);
                }
            }
            Ok(dials)
        })
    }

    fn constraints(&self) -> Answer<'_, Vec<Constraint>> {
        Box::pin(async move {
            let sql =
                "SELECT id, rule, function, contains, rung, source FROM constraints ORDER BY id";
            self.rows(sql, json!([]), constraint).await
        })
    }

    fn set_constraint<'a>(&'a self, c: &'a Constraint) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(UPSERT_CONSTRAINT, constraint_params(c))
                .await
                .map(|_| ())
        })
    }

    fn capabilities(&self) -> Answer<'_, Vec<Capability>> {
        Box::pin(async move {
            let sql = "SELECT stage, function, rung FROM capabilities ORDER BY stage, function";
            self.rows(sql, json!([]), capability).await
        })
    }

    fn set_capability<'a>(&'a self, g: &'a Capability) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(UPSERT_CAPABILITY, capability_params(g))
                .await
                .map(|_| ())
        })
    }

    fn hold<'a>(&'a self, h: &'a HeldCall) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(
                "INSERT INTO held_calls (id, job, session, turn, function, text, arguments, at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                json!([h.id, h.job, h.session, h.turn, h.call.function, h.call.text, h.arguments.to_string(), h.at]),
            )
            .await
            .map(|_| ())
        })
    }

    fn held<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<HeldCall>> {
        Box::pin(async move {
            let sql = "SELECT id, job, session, turn, function, text, arguments, at FROM held_calls WHERE job = ? ORDER BY at, id";
            self.rows(sql, json!([job]), held_call).await
        })
    }

    fn claim<'a>(&'a self, id: &'a str) -> Answer<'a, bool> {
        Box::pin(async move {
            let sql = "DELETE FROM held_calls WHERE id = ? RETURNING id";
            Ok(!self.execute(sql, json!([id])).await?.is_empty())
        })
    }

    fn audit(&self) -> Answer<'_, Vec<Entry>> {
        Box::pin(async move {
            let sql =
                "SELECT seq, at, kind, subject, detail, prev, hash FROM audit_log ORDER BY seq";
            self.rows(sql, json!([]), entry).await
        })
    }

    fn gate_entries<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Entry>> {
        Box::pin(async move {
            let sql = "SELECT seq, at, kind, subject, detail, prev, hash FROM audit_log WHERE kind = 'gate' AND subject = ? ORDER BY seq";
            self.rows(sql, json!([job]), entry).await
        })
    }

    fn entries_of<'a>(&'a self, kind: &'a str) -> Answer<'a, Vec<Entry>> {
        Box::pin(async move {
            let sql = "SELECT seq, at, kind, subject, detail, prev, hash FROM audit_log WHERE kind = ? ORDER BY seq";
            self.rows(sql, json!([kind]), entry).await
        })
    }

    fn audit_tail(&self) -> Answer<'_, Option<Entry>> {
        Box::pin(async move {
            let sql = "SELECT seq, at, kind, subject, detail, prev, hash FROM audit_log ORDER BY seq DESC LIMIT 1";
            Ok(self.rows(sql, json!([]), entry).await?.pop())
        })
    }

    fn append_audit<'a>(&'a self, e: &'a Entry) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(
                "INSERT INTO audit_log (seq, at, kind, subject, detail, prev, hash) VALUES (?, ?, ?, ?, ?, ?, ?)",
                json!([e.seq, e.at, e.record.kind, e.record.subject, e.record.detail, e.prev, e.hash]),
            )
            .await
            .map(|_| ())
        })
    }
}

#[cfg(test)]
pub mod fake {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;

    /// The rows `keep` picks, in order.
    fn matching<T: Clone>(rows: &Mutex<Vec<T>>, keep: impl Fn(&T) -> bool) -> Vec<T> {
        rows.lock()
            .unwrap()
            .iter()
            .filter(|r| keep(r))
            .cloned()
            .collect()
    }

    /// Rows in memory, with the same `rev` rule as `SqlStore`.
    #[derive(Default)]
    pub struct FakeStore {
        pub workspaces: Mutex<Vec<Workspace>>,
        pub jobs: Mutex<BTreeMap<String, Record>>,
        pub artifacts: Mutex<Vec<(String, Artifact)>>,
        pub dials: Mutex<Dials>,
        pub constraints: Mutex<Vec<Constraint>>,
        pub capabilities: Mutex<Vec<Capability>>,
        pub held: Mutex<Vec<HeldCall>>,
        pub audit: Mutex<Vec<Entry>>,
        /// Make every audit append wait forever, like a stalled database.
        pub stall_audit: Mutex<bool>,
        pub eval_rows: crate::eval_store::fake::EvalRows,
        pub versions: crate::settings_store::fake::Versions,
        pub people: crate::people_store::fake::People,
        pub workflows: crate::workflow_store::fake::Workflows,
    }

    impl Store for FakeStore {
        fn add_workspace<'a>(&'a self, w: &'a Workspace) -> Answer<'a, ()> {
            self.workspaces.lock().unwrap().push(w.clone());
            Box::pin(async { Ok(()) })
        }

        fn workspaces(&self) -> Answer<'_, Vec<Workspace>> {
            let all = self.workspaces.lock().unwrap().clone();
            Box::pin(async move { Ok(all) })
        }

        fn edit_workspace<'a>(&'a self, w: &'a Workspace) -> Answer<'a, ()> {
            let mut all = self.workspaces.lock().unwrap();
            let found = all.iter_mut().find(|o| o.name == w.name);
            let done = found
                .map(|o| *o = w.clone())
                .ok_or_else(|| format!("no workspace named {}", w.name));
            Box::pin(async move { done })
        }

        fn insert<'a>(&'a self, r: &'a Record) -> Answer<'a, ()> {
            self.jobs
                .lock()
                .unwrap()
                .insert(r.job.id.clone(), r.clone());
            Box::pin(async { Ok(()) })
        }

        fn load<'a>(&'a self, id: &'a str) -> Answer<'a, Record> {
            let found = self.jobs.lock().unwrap().get(id).cloned().ok_or(no_job(id));
            Box::pin(async move { found })
        }

        fn jobs(&self, limit: usize, offset: usize) -> Answer<'_, Vec<Record>> {
            let mut all: Vec<Record> = self.jobs.lock().unwrap().values().cloned().collect();
            all.sort_by_key(|r| std::cmp::Reverse(r.created_at));
            let some = all.into_iter().skip(offset).take(limit).collect();
            Box::pin(async move { Ok(some) })
        }

        fn active(&self) -> Answer<'_, Vec<Record>> {
            let all = self.jobs.lock().unwrap();
            let waited_on: Vec<String> = all
                .values()
                .filter(|r| r.job.state == State::Queued)
                .filter_map(|r| r.job.after.clone())
                .collect();
            let mut some: Vec<Record> = all
                .values()
                .filter(|r| !r.job.state.finished() || waited_on.contains(&r.job.id))
                .cloned()
                .collect();
            some.sort_by_key(|r| std::cmp::Reverse(r.created_at));
            Box::pin(async move { Ok(some) })
        }

        fn save<'a>(&'a self, r: &'a Record) -> Answer<'a, i64> {
            let mut jobs = self.jobs.lock().unwrap();
            let result = match jobs.get(&r.job.id) {
                Some(current) if current.rev == r.rev => {
                    let mut saved = r.clone();
                    saved.rev += 1;
                    jobs.insert(r.job.id.clone(), saved);
                    Ok(r.rev + 1)
                }
                _ => Err(STALE.into()),
            };
            Box::pin(async move { result })
        }

        fn add_artifact<'a>(&'a self, job: &'a str, a: &'a Artifact) -> Answer<'a, ()> {
            self.artifacts.lock().unwrap().push((job.into(), a.clone()));
            Box::pin(async { Ok(()) })
        }

        fn artifacts<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Artifact>> {
            let found = matching(&self.artifacts, |(j, _)| j == job)
                .into_iter()
                .map(|(_, a)| a)
                .collect();
            Box::pin(async move { Ok(found) })
        }

        fn artifacts_at(&self, stage: Stage) -> Answer<'_, Vec<(String, String, String)>> {
            let jobs = self.jobs.lock().unwrap();
            let mut all: Vec<_> = matching(&self.artifacts, |(_, a)| a.stage == stage)
                .into_iter()
                .filter_map(|(j, a)| Some((j.clone(), jobs.get(&j)?.job.workspace.clone(), a.body)))
                .collect();
            all.reverse();
            Box::pin(async move { Ok(all) })
        }

        fn dials(&self) -> Answer<'_, Dials> {
            let dials = self.dials.lock().unwrap().clone();
            Box::pin(async move { Ok(dials) })
        }

        fn constraints(&self) -> Answer<'_, Vec<Constraint>> {
            let all = self.constraints.lock().unwrap().clone();
            Box::pin(async move { Ok(all) })
        }

        fn set_constraint<'a>(&'a self, c: &'a Constraint) -> Answer<'a, ()> {
            let mut all = self.constraints.lock().unwrap();
            all.retain(|x| x.id != c.id);
            all.push(c.clone());
            Box::pin(async { Ok(()) })
        }

        fn capabilities(&self) -> Answer<'_, Vec<Capability>> {
            let all = self.capabilities.lock().unwrap().clone();
            Box::pin(async move { Ok(all) })
        }

        fn set_capability<'a>(&'a self, g: &'a Capability) -> Answer<'a, ()> {
            let mut all = self.capabilities.lock().unwrap();
            all.retain(|x| (x.stage, &x.function) != (g.stage, &g.function));
            all.push(g.clone());
            Box::pin(async { Ok(()) })
        }

        fn hold<'a>(&'a self, h: &'a HeldCall) -> Answer<'a, ()> {
            self.held.lock().unwrap().push(h.clone());
            Box::pin(async { Ok(()) })
        }

        fn held<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<HeldCall>> {
            let found = matching(&self.held, |h| h.job == job);
            Box::pin(async move { Ok(found) })
        }

        fn claim<'a>(&'a self, id: &'a str) -> Answer<'a, bool> {
            let mut held = self.held.lock().unwrap();
            let before = held.len();
            held.retain(|h| h.id != id);
            let claimed = held.len() < before;
            Box::pin(async move { Ok(claimed) })
        }

        fn audit(&self) -> Answer<'_, Vec<Entry>> {
            let all = self.audit.lock().unwrap().clone();
            Box::pin(async move { Ok(all) })
        }

        fn gate_entries<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Entry>> {
            let all = self.audit.lock().unwrap().clone();
            let of_job = all
                .into_iter()
                .filter(|e| e.record.kind == "gate" && e.record.subject == job)
                .collect();
            Box::pin(async move { Ok(of_job) })
        }

        fn entries_of<'a>(&'a self, kind: &'a str) -> Answer<'a, Vec<Entry>> {
            let all = self.audit.lock().unwrap().clone();
            let of_kind = all.into_iter().filter(|e| e.record.kind == kind).collect();
            Box::pin(async move { Ok(of_kind) })
        }

        fn audit_tail(&self) -> Answer<'_, Option<Entry>> {
            let last = self.audit.lock().unwrap().last().cloned();
            Box::pin(async move { Ok(last) })
        }

        fn append_audit<'a>(&'a self, e: &'a Entry) -> Answer<'a, ()> {
            if *self.stall_audit.lock().unwrap() {
                return Box::pin(std::future::pending());
            }
            let mut all = self.audit.lock().unwrap();
            let result = if all.iter().any(|x| x.seq == e.seq) {
                Err(format!("audit entry {} exists", e.seq))
            } else {
                all.push(e.clone());
                Ok(())
            };
            Box::pin(async move { result })
        }
    }
}

/// The seed file the stack carries, for the tests.
#[cfg(test)]
pub(crate) const SEED: &str = include_str!("../../../seed/governance.sql");

#[cfg(test)]
mod tests {
    use super::fake::FakeStore;
    use super::*;
    use crate::engine::fake::FakeEngine;
    use bastion_core::{Kind, State};
    use bastion_protocol::Link;

    fn record() -> Record {
        Record {
            job: Job::new("j1".into(), "w".into(), "r".into(), 5),
            runtime: Runtime::default(),
            rev: 1,
            created_at: 5,
        }
    }

    #[test]
    fn the_seed_grants_every_turn_stage_its_sandbox() {
        let turns = Stage::ALL.into_iter().filter(|s| s.kind() == Kind::Turn);
        for stage in turns {
            let row = format!("('{}', 'sandbox::*', 'allowed')", name(stage));
            assert!(SEED.contains(&row), "the seed grants nothing at {stage:?}");
        }
    }

    #[test]
    fn a_stale_save_is_refused_and_a_current_one_bumps_the_rev() {
        let store = FakeStore::default();
        let r = record();
        crate::rt::block_on(async {
            store.insert(&r).await.unwrap();
            assert_eq!(store.save(&r).await, Ok(2));
            assert_eq!(
                store.save(&r).await,
                Err(STALE.into()),
                "rev 1 is stale now"
            );
        });
    }

    #[test]
    fn an_update_that_matches_no_row_is_stale() {
        let engine = Arc::new(
            FakeEngine::new(Link::Up)
                .answer("database::transaction", Ok(json!({ "committed": true })))
                .answer(
                    "database::query",
                    Ok(json!({ "rows": [{ "user_version": 1 }] })),
                )
                .answer("database::execute", Ok(json!({ "returned_rows": [] }))),
        );
        crate::rt::block_on(async {
            let store = SqlStore::open(engine.clone(), SEED).await.unwrap();
            assert_eq!(store.save(&record()).await, Err(STALE.into()));
            let calls = engine.calls.lock().unwrap();
            let update = &calls.last().unwrap().2;
            assert!(
                update["sql"]
                    .as_str()
                    .unwrap()
                    .contains("WHERE id = ? AND rev = ?")
            );
            assert_eq!(update["params"][23], "j1");
            assert_eq!(update["params"][24], 1);
        });
    }

    #[test]
    fn a_second_entry_with_the_same_seq_is_refused() {
        let store = FakeStore::default();
        let what = audit::Record {
            kind: "gate".into(),
            subject: "j1".into(),
            detail: "continue".into(),
        };
        let first = audit::append(None, 1, what, b"k");
        crate::rt::block_on(async {
            store.append_audit(&first).await.unwrap();
            assert!(store.append_audit(&first).await.is_err());
        });
    }

    #[test]
    fn a_held_call_row_reads_back_with_its_arguments() {
        let h = HeldCall {
            id: "fc1".into(),
            job: "j1".into(),
            session: "s".into(),
            turn: "t".into(),
            call: Call {
                content: String::new(),
                function: "sandbox::fs::write".into(),
                text: "/work/repo/a.py".into(),
            },
            arguments: json!({ "path": "/work/repo/a.py", "content": "x" }),
            at: 9,
        };
        let row = json!({ "id": "fc1", "job": "j1", "session": "s", "turn": "t", "function": "sandbox::fs::write",
            "text": "/work/repo/a.py", "arguments": h.arguments.to_string(), "at": 9 });
        assert_eq!(held_call(&row).unwrap(), h);
    }

    /// The transactions `open` sends on a database at `version`.
    fn transactions_at(version: u64) -> Vec<Vec<String>> {
        let engine = Arc::new(
            FakeEngine::new(Link::Up)
                .answer("database::transaction", Ok(json!({ "committed": true })))
                .answer("database::execute", Ok(json!({ "returned_rows": [] })))
                .answer(
                    "database::query",
                    Ok(json!({ "rows": [{ "user_version": version }] })),
                ),
        );
        crate::rt::block_on(async {
            SqlStore::open(engine.clone(), SEED).await.unwrap();
        });
        let calls = engine.calls.lock().unwrap();
        calls
            .iter()
            .filter(|c| c.1 == "database::transaction")
            .map(|c| {
                c.2["statements"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s["sql"].as_str().unwrap().to_string())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn migration_1_rebuilds_the_staged_tables_once_with_deferred_keys() {
        let fresh = transactions_at(0);
        assert_eq!(fresh.len(), 4, "the schema, then migrations 1 to 3");
        let m1 = &fresh[1];
        assert_eq!(m1.first().unwrap(), "PRAGMA defer_foreign_keys = ON");
        assert_eq!(m1.last().unwrap(), "PRAGMA user_version = 1");
        for table in ["jobs", "artifacts", "capabilities", "turns"] {
            assert!(m1.contains(&format!("DROP TABLE {table}")));
        }
        assert!(m1.iter().any(|s| s.contains("'code_review'")));
        let m2 = &transactions_at(1)[1];
        assert!(m2.contains(&"DROP TABLE turns".to_string()));
        assert!(!m2.contains(&"DROP TABLE jobs".to_string()));
        let m3 = &transactions_at(2)[1];
        assert!(m3.iter().any(|s| s.contains("'command'")));
        assert!(
            m3.iter()
                .any(|s| s.starts_with("INSERT INTO jobs") && s.contains("workflow_version, node"))
        );
        assert_eq!(transactions_at(3).len(), 1, "nothing left to migrate");
    }

    #[test]
    fn open_seeds_the_owner_defaults_without_overwriting() {
        let engine = Arc::new(
            FakeEngine::new(Link::Up)
                .answer("database::transaction", Ok(json!({ "committed": true })))
                // An older database already has the column; that error is expected.
                .answer("database::execute", Err("duplicate column name: pr".into()))
                .answer(
                    "database::query",
                    Ok(json!({ "rows": [{ "user_version": 1 }] })),
                ),
        );
        crate::rt::block_on(async {
            SqlStore::open(engine.clone(), SEED).await.unwrap();
        });
        let calls = engine.calls.lock().unwrap();
        let statements = calls[0].2["statements"].as_array().unwrap();
        let seeds: Vec<_> = statements
            .iter()
            .filter(|s| s["sql"].as_str().unwrap().starts_with("INSERT OR IGNORE"))
            .collect();
        let tables: Vec<_> = seeds
            .iter()
            .map(|s| {
                s["sql"]
                    .as_str()
                    .unwrap()
                    .split_whitespace()
                    .nth(4)
                    .unwrap()
            })
            .collect();
        assert_eq!(
            tables,
            [
                "constraints",
                "capabilities",
                "workflows",
                "workflow_nodes",
                "workflow_results",
                "workflow_links"
            ]
        );
    }

    #[test]
    fn a_row_reads_back_as_the_record_it_was_written_from() {
        let mut r = record();
        r.job.state = State::NeedsYou;
        // Kept at a checkpoint before D33.
        r.job.stage = Stage::Approve;
        r.job.plan = Some("1. x".into());
        r.job.pipeline = bastion_core::Pipeline::Full;
        r.job.back_to = Some(Stage::Checks);
        r.job.done_tasks = 2;
        r.job.node = Some("lint".into());
        r.runtime.sandbox = Some("vm-1".into());
        r.runtime.workflow = Some("w1".into());
        r.runtime.workflow_version = Some(3);
        r.runtime.approved = vec!["tests/a.py".into(), "tests/b.py".into()];
        let columns: Vec<&str> = JOB_COLUMNS.split(", ").collect();
        let values = job_params(&r);
        let row: serde_json::Map<String, Value> = columns
            .iter()
            .zip(values.as_array().unwrap())
            .map(|(c, v)| (c.to_string(), v.clone()))
            .collect();
        let mut folded = r;
        folded.job.stage = Stage::Plan;
        assert_eq!(super::record(&Value::Object(row)).unwrap(), folded);
    }

    #[test]
    fn a_job_row_that_does_not_parse_is_left_out_of_the_list() {
        let columns = JOB_COLUMNS.split(", ");
        let mut good: serde_json::Map<String, Value> = columns
            .zip(job_params(&record()).as_array().unwrap().clone())
            .map(|(c, v)| (c.to_string(), v))
            .collect();
        // `open` reads the schema version from the same answer.
        good.insert("user_version".into(), json!(99));
        let bad = json!({ "id": "j2", "state": "nonsense", "user_version": 99 });
        let engine = Arc::new(
            FakeEngine::new(Link::Up)
                .answer("database::transaction", Ok(json!({ "committed": true })))
                .answer("database::query", Ok(json!({ "rows": [good, bad] })))
                .answer("database::execute", Ok(json!({ "returned_rows": [] }))),
        );
        crate::rt::block_on(async {
            let store = SqlStore::open(engine, SEED).await.unwrap();
            let listed = store.jobs(10, 0).await.unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].job.id, "j1");
        });
    }
}

/// The SQL against this machine's `database` worker: `cargo test -p bastion-server -- --ignored --nocapture live_store`.
#[cfg(test)]
mod live {
    use super::*;
    use crate::engine::IiiEngine;
    use bastion_protocol::Link;

    /// The store on this machine's stack, with one workspace and one job
    /// named `tag`.
    async fn open_with_job(tag: &str) -> (SqlStore, Record) {
        let engine: Arc<dyn Engine> = Arc::new(IiiEngine::connect("ws://127.0.0.1:49160"));
        for _ in 0..150 {
            if engine.link() == Link::Up {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert_eq!(engine.link(), Link::Up, "no engine link in 15 s");
        let store = SqlStore::open(engine, SEED).await.unwrap();
        let w = Workspace {
            name: tag.into(),
            path: format!("/tmp/{tag}"),
            branch: "main".into(),
            checks: String::new(),
            max_jobs: None,
            revision_budget: None,
            branch_prefix: "bastion/".into(),
            setup: String::new(),
            cleanup: String::new(),
            env: String::new(),
            gates: String::new(),
            autofix: String::new(),
        };
        store.add_workspace(&w).await.unwrap();
        let r = Record {
            job: Job::new(tag.into(), tag.into(), "check the store".into(), 7),
            runtime: Runtime::default(),
            rev: 1,
            created_at: 7,
        };
        store.insert(&r).await.unwrap();
        (store, r)
    }

    #[test]
    #[ignore = "needs this machine's stack"]
    fn live_store() {
        crate::rt::block_on(async {
            let tag = format!("live-{}", std::process::id());
            let (store, mut r) = open_with_job(&tag).await;
            r.job.state = bastion_core::State::Running;
            r.runtime.sandbox = Some("vm".into());
            let rev = store.save(&r).await.unwrap();
            let stale = store.save(&r).await;
            store
                .add_artifact(
                    &tag,
                    &Artifact {
                        stage: Stage::Plan,
                        body: "1. x".into(),
                        created_at: 8,
                    },
                )
                .await
                .unwrap();
            let back = store.load(&tag).await.unwrap();
            let artifacts = store.artifacts(&tag).await.unwrap();
            let bad = store
                .execute(
                    "UPDATE jobs SET state = 'nonsense' WHERE id = ?",
                    json!([tag]),
                )
                .await;
            for sql in [
                "DELETE FROM artifacts WHERE job = ?",
                "DELETE FROM jobs WHERE id = ?",
                "DELETE FROM workspaces WHERE name = ?",
            ] {
                store.execute(sql, json!([tag])).await.unwrap();
            }
            println!(
                "rev {rev}, stale {stale:?}, state {:?}, sandbox {:?}, artifacts {}, bad state {bad:?}",
                back.job.state,
                back.runtime.sandbox,
                artifacts.len()
            );
            assert_eq!(rev, 2);
            assert_eq!(stale, Err(STALE.into()));
            assert_eq!(
                (back.rev, back.job.state, back.runtime.sandbox.as_deref()),
                (2, bastion_core::State::Running, Some("vm"))
            );
            assert_eq!(artifacts[0].body, "1. x");
            assert!(
                bad.is_err(),
                "the CHECK constraint refuses an unknown state"
            );
        });
    }

    #[test]
    #[ignore = "needs this machine's stack"]
    fn live_workspace_edit() {
        crate::rt::block_on(async {
            let tag = format!("live-ws-{}", std::process::id());
            let (store, _) = open_with_job(&tag).await;
            let mut w = store.workspaces().await.unwrap();
            w.retain(|w| w.name == tag);
            let mut edited = w.remove(0);
            (edited.checks, edited.max_jobs) = ("make test".into(), Some(2));
            edited.revision_budget = Some(4);
            edited.branch_prefix = "ian/".into();
            (edited.setup, edited.env) = ("make deps".into(), "CI=1".into());
            store.edit_workspace(&edited).await.unwrap();
            let back = store.workspaces().await.unwrap();
            let unknown = Workspace {
                name: format!("{tag}-none"),
                ..edited.clone()
            };
            let missing = store.edit_workspace(&unknown).await;
            let zero = Workspace {
                max_jobs: Some(0),
                ..edited.clone()
            };
            let refused = store.edit_workspace(&zero).await;
            for sql in [
                "DELETE FROM jobs WHERE id = ?",
                "DELETE FROM workspaces WHERE name = ?",
            ] {
                store.execute(sql, json!([tag])).await.unwrap();
            }
            assert!(back.contains(&edited));
            assert!(missing.is_err(), "no workspace by that name");
            assert!(refused.is_err(), "the CHECK refuses a cap of 0");
        });
    }

    #[test]
    #[ignore = "needs this machine's stack"]
    fn live_governance() {
        crate::rt::block_on(async {
            let tag = format!("live-gov-{}", std::process::id());
            let (store, _) = open_with_job(&tag).await;
            let rules = store.constraints().await.unwrap();
            let grants = store.capabilities().await.unwrap();
            let mut keep = rules.iter().find(|c| c.id == "keep-tests").unwrap().clone();
            let was = keep.rung;
            keep.rung = keep.rung.promote();
            store.set_constraint(&keep).await.unwrap();
            let promoted = store
                .constraints()
                .await
                .unwrap()
                .into_iter()
                .find(|c| c.id == "keep-tests")
                .unwrap()
                .rung;
            keep.rung = was;
            store.set_constraint(&keep).await.unwrap();
            let held = HeldCall {
                id: format!("fc-{tag}"),
                job: tag.clone(),
                session: "s".into(),
                turn: "t".into(),
                call: Call {
                    content: String::new(),
                    function: "sandbox::fs::write".into(),
                    text: "/work/repo/a.py".into(),
                },
                arguments: json!({ "path": "/work/repo/a.py" }),
                at: 9,
            };
            store.hold(&held).await.unwrap();
            let held_back = store.held(&tag).await.unwrap();
            assert!(
                store.claim(&held.id).await.unwrap(),
                "the first claim takes it"
            );
            assert!(
                !store.claim(&held.id).await.unwrap(),
                "a second claim finds it gone"
            );
            let released = store.held(&tag).await.unwrap();
            println!(
                "constraints {}, grants {}, promoted {promoted:?}, held {}",
                rules.len(),
                grants.len(),
                held_back.len()
            );
            assert!(rules.len() >= 5, "the seed file's five rules");
            assert!(grants.len() >= 3, "the seed file's three grants");
            assert_eq!(promoted, was.promote());
            assert_eq!(held_back, vec![held]);
            assert!(released.is_empty());
            for sql in [
                "DELETE FROM jobs WHERE id = ?",
                "DELETE FROM workspaces WHERE name = ?",
            ] {
                store.execute(sql, json!([tag])).await.unwrap();
            }
        });
    }
}
