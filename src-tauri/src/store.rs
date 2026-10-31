//! The Store seam (PLAN.md §6.2): workspaces, jobs and artifacts as rows in
//! the `database` worker, each job with a `rev` so a stale write is refused
//! (D8), and the governance rows of A5: constraints, capabilities, held
//! calls and the audit chain. `SqlStore` is the only code that writes SQL.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bastion_core::audit::{self, Entry};
use bastion_core::govern::{self, Call, Capability, Constraint};
use bastion_core::{Dial, Dials, Job, Stage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::Engine;
use crate::eval_store::{self, EvalStore};
use crate::settings_store::{self, SettingsStore};

pub type Answer<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// A registered repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub path: String,
    pub branch: String,
}

/// What the adapter holds for a job besides the core's state: where it runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runtime {
    pub worktree: Option<String>,
    pub sandbox: Option<String>,
    pub session: Option<String>,
    /// The tracker issue the request came from, as its URL.
    pub issue: Option<String>,
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

/// A save lost to another writer (D8). The caller reloads and steps again.
pub const STALE: &str = "stale: the job changed since it was read";

pub trait Store: EvalStore + SettingsStore + Send + Sync {
    fn add_workspace<'a>(&'a self, workspace: &'a Workspace) -> Answer<'a, ()>;
    fn workspaces(&self) -> Answer<'_, Vec<Workspace>>;
    fn insert<'a>(&'a self, record: &'a Record) -> Answer<'a, ()>;
    fn load<'a>(&'a self, id: &'a str) -> Answer<'a, Record>;
    /// Every job, newest first.
    fn jobs(&self) -> Answer<'_, Vec<Record>>;
    /// Write `record` if its `rev` is still current. Returns the new `rev`.
    fn save<'a>(&'a self, record: &'a Record) -> Answer<'a, i64>;
    fn add_artifact<'a>(&'a self, job: &'a str, artifact: &'a Artifact) -> Answer<'a, ()>;
    fn artifacts<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Artifact>>;
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
    fn release<'a>(&'a self, id: &'a str) -> Answer<'a, ()>;
    /// The whole audit chain, oldest first.
    fn audit(&self) -> Answer<'_, Vec<Entry>>;
    /// Add the next entry. A second entry with the same `seq` is refused, so
    /// two writers cannot fork the chain.
    fn append_audit<'a>(&'a self, entry: &'a Entry) -> Answer<'a, ()>;
}

const NAMESPACE: &str = "bastion";

/// The schema (D8): typed columns with constraints, no JSON in a text column.
// ponytail: idempotent CREATEs; add numbered migrations when a column changes.
const SCHEMA: [&str; 8] = [
    "CREATE TABLE IF NOT EXISTS workspaces (
        name TEXT PRIMARY KEY CHECK (length(name) > 0),
        path TEXT NOT NULL UNIQUE,
        branch TEXT NOT NULL CHECK (length(branch) > 0),
        created_at INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS jobs (
        id TEXT PRIMARY KEY,
        workspace TEXT NOT NULL REFERENCES workspaces(name),
        request TEXT NOT NULL CHECK (length(request) > 0),
        stage TEXT NOT NULL CHECK (stage IN ('plan','approve','build','review','land')),
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
        issue TEXT)",
    "CREATE TABLE IF NOT EXISTS artifacts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        job TEXT NOT NULL REFERENCES jobs(id),
        stage TEXT NOT NULL CHECK (stage IN ('plan','approve','build','review','land')),
        body TEXT NOT NULL,
        created_at INTEGER NOT NULL)",
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
    "CREATE TABLE IF NOT EXISTS capabilities (
        stage TEXT NOT NULL CHECK (stage IN ('plan','approve','build','review','land')),
        function TEXT NOT NULL CHECK (length(function) > 0),
        rung TEXT NOT NULL CHECK (rung IN ('denied','ask','allowed')),
        PRIMARY KEY (stage, function))",
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

/// The owner's defaults, written once. A row the owner changed is kept.
fn seed() -> Vec<Value> {
    let constraints = govern::default_constraints().into_iter().map(|c| {
        json!({ "sql": "INSERT OR IGNORE INTO constraints (id, rule, function, contains, rung, source) VALUES (?, ?, ?, ?, ?, ?)", "params": constraint_params(&c) })
    });
    let grants = govern::default_capabilities().into_iter().map(|g| {
        json!({ "sql": "INSERT OR IGNORE INTO capabilities (stage, function, rung) VALUES (?, ?, ?)", "params": capability_params(&g) })
    });
    constraints.chain(grants).collect()
}

const JOB_COLUMNS: &str = "id, workspace, request, stage, state, question, feedback, plan, failures, error, worktree, sandbox, session, rev, created_at, updated_at, pr, ci_fixes, issue";

/// Columns added after a table was first made. Each runs once; on a table
/// that already has the column it fails, and that failure is expected.
// ponytail: try-and-ignore ALTERs; a numbered user_version replaces this once a migration is more than a new column.
const MIGRATIONS: [&str; 3] = [
    "ALTER TABLE jobs ADD COLUMN pr TEXT",
    "ALTER TABLE jobs ADD COLUMN ci_fixes INTEGER NOT NULL DEFAULT 0 CHECK (ci_fixes >= 0)",
    "ALTER TABLE jobs ADD COLUMN issue TEXT",
];

pub struct SqlStore {
    engine: Arc<dyn Engine>,
}

impl SqlStore {
    /// Make the tables if they are missing.
    pub async fn open(engine: Arc<dyn Engine>) -> Result<Self, String> {
        let statements: Vec<Value> = SCHEMA
            .iter()
            .chain(eval_store::SCHEMA.iter())
            .chain(settings_store::SCHEMA.iter())
            .map(|sql| json!({ "sql": sql, "params": [] }))
            .chain(seed())
            .collect();
        engine
            .call(
                NAMESPACE,
                "database::transaction",
                json!({ "statements": statements }),
            )
            .await?;
        for sql in MIGRATIONS {
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
        Ok(Self { engine })
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

fn record(row: &Value) -> Result<Record, String> {
    Ok(Record {
        job: Job {
            id: parse(row, "id")?,
            workspace: parse(row, "workspace")?,
            request: parse(row, "request")?,
            stage: parse(row, "stage")?,
            state: parse(row, "state")?,
            question: text(row, "question"),
            feedback: text(row, "feedback"),
            plan: text(row, "plan"),
            failures: parse(row, "failures")?,
            error: text(row, "error"),
            pr: text(row, "pr"),
            ci_fixes: row["ci_fixes"].as_u64().unwrap_or(0) as u32,
            updated_at: parse(row, "updated_at")?,
        },
        runtime: Runtime {
            worktree: text(row, "worktree"),
            sandbox: text(row, "sandbox"),
            session: text(row, "session"),
            issue: text(row, "issue"),
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
        r.runtime.issue
    ])
}

impl Store for SqlStore {
    fn add_workspace<'a>(&'a self, w: &'a Workspace) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(
                "INSERT INTO workspaces (name, path, branch, created_at) VALUES (?, ?, ?, unixepoch())",
                json!([w.name, w.path, w.branch]),
            )
            .await
            .map(|_| ())
        })
    }

    fn workspaces(&self) -> Answer<'_, Vec<Workspace>> {
        Box::pin(async move {
            self.query(
                "SELECT name, path, branch FROM workspaces ORDER BY name",
                json!([]),
            )
            .await?
            .iter()
            .map(|r| {
                Ok(Workspace {
                    name: parse(r, "name")?,
                    path: parse(r, "path")?,
                    branch: parse(r, "branch")?,
                })
            })
            .collect()
        })
    }

    fn insert<'a>(&'a self, r: &'a Record) -> Answer<'a, ()> {
        Box::pin(async move {
            let sql = format!(
                "INSERT INTO jobs ({JOB_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            );
            self.execute(&sql, job_params(r)).await.map(|_| ())
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
            record(rows.first().ok_or_else(|| format!("no job {id}"))?)
        })
    }

    fn jobs(&self) -> Answer<'_, Vec<Record>> {
        Box::pin(async move {
            let sql = format!("SELECT {JOB_COLUMNS} FROM jobs ORDER BY created_at DESC, id");
            self.rows(&sql, json!([]), record).await
        })
    }

    fn save<'a>(&'a self, r: &'a Record) -> Answer<'a, i64> {
        Box::pin(async move {
            let j = &r.job;
            let rows = self
                .execute(
                    "UPDATE jobs SET stage = ?, state = ?, question = ?, feedback = ?, plan = ?, failures = ?, error = ?, worktree = ?, sandbox = ?, session = ?, updated_at = ?, pr = ?, ci_fixes = ?, issue = ?, rev = rev + 1
                     WHERE id = ? AND rev = ? RETURNING rev",
                    json!([
                        name(j.stage), name(j.state), j.question, j.feedback, j.plan, j.failures, j.error,
                        r.runtime.worktree, r.runtime.sandbox, r.runtime.session, j.updated_at, j.pr, j.ci_fixes, r.runtime.issue, j.id, r.rev
                    ]),
                )
                .await?;
            rows.first()
                .map(|row| parse(row, "rev"))
                .unwrap_or_else(|| Err(STALE.into()))
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
                match row["checkpoint"].as_str() {
                    Some("approve") => dials.approve = dial,
                    Some("review") => dials.review = dial,
                    _ => {}
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

    fn release<'a>(&'a self, id: &'a str) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute("DELETE FROM held_calls WHERE id = ?", json!([id]))
                .await
                .map(|_| ())
        })
    }

    fn audit(&self) -> Answer<'_, Vec<Entry>> {
        Box::pin(async move {
            let sql =
                "SELECT seq, at, kind, subject, detail, prev, hash FROM audit_log ORDER BY seq";
            self.rows(sql, json!([]), entry).await
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

        fn insert<'a>(&'a self, r: &'a Record) -> Answer<'a, ()> {
            self.jobs
                .lock()
                .unwrap()
                .insert(r.job.id.clone(), r.clone());
            Box::pin(async { Ok(()) })
        }

        fn load<'a>(&'a self, id: &'a str) -> Answer<'a, Record> {
            let found = self
                .jobs
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .ok_or(format!("no job {id}"));
            Box::pin(async move { found })
        }

        fn jobs(&self) -> Answer<'_, Vec<Record>> {
            let mut all: Vec<Record> = self.jobs.lock().unwrap().values().cloned().collect();
            all.sort_by_key(|r| std::cmp::Reverse(r.created_at));
            Box::pin(async move { Ok(all) })
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

        fn dials(&self) -> Answer<'_, Dials> {
            let dials = *self.dials.lock().unwrap();
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

        fn release<'a>(&'a self, id: &'a str) -> Answer<'a, ()> {
            self.held.lock().unwrap().retain(|h| h.id != id);
            Box::pin(async { Ok(()) })
        }

        fn audit(&self) -> Answer<'_, Vec<Entry>> {
            let all = self.audit.lock().unwrap().clone();
            Box::pin(async move { Ok(all) })
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

#[cfg(test)]
mod tests {
    use super::fake::FakeStore;
    use super::*;
    use crate::engine::fake::FakeEngine;
    use bastion_core::State;
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
    fn a_stale_save_is_refused_and_a_current_one_bumps_the_rev() {
        let store = FakeStore::default();
        let r = record();
        tauri::async_runtime::block_on(async {
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
                .answer("database::execute", Ok(json!({ "returned_rows": [] }))),
        );
        tauri::async_runtime::block_on(async {
            let store = SqlStore::open(engine.clone()).await.unwrap();
            assert_eq!(store.save(&record()).await, Err(STALE.into()));
            let calls = engine.calls.lock().unwrap();
            let update = &calls.last().unwrap().2;
            assert!(
                update["sql"]
                    .as_str()
                    .unwrap()
                    .contains("WHERE id = ? AND rev = ?")
            );
            assert_eq!(update["params"][14], "j1");
            assert_eq!(update["params"][15], 1);
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
        tauri::async_runtime::block_on(async {
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

    #[test]
    fn open_seeds_the_owner_defaults_without_overwriting() {
        let engine = Arc::new(
            FakeEngine::new(Link::Up)
                .answer("database::transaction", Ok(json!({ "committed": true })))
                // An older database already has the column; that error is expected.
                .answer("database::execute", Err("duplicate column name: pr".into())),
        );
        tauri::async_runtime::block_on(async {
            SqlStore::open(engine.clone()).await.unwrap();
        });
        let calls = engine.calls.lock().unwrap();
        let statements = calls[0].2["statements"].as_array().unwrap();
        let seeds: Vec<_> = statements
            .iter()
            .filter(|s| s["sql"].as_str().unwrap().starts_with("INSERT OR IGNORE"))
            .collect();
        assert_eq!(
            seeds.len(),
            govern::default_constraints().len() + govern::default_capabilities().len()
        );
    }

    #[test]
    fn a_row_reads_back_as_the_record_it_was_written_from() {
        let mut r = record();
        r.job.state = State::NeedsYou;
        r.job.stage = Stage::Approve;
        r.job.plan = Some("1. x".into());
        r.runtime.sandbox = Some("vm-1".into());
        let columns: Vec<&str> = JOB_COLUMNS.split(", ").collect();
        let values = job_params(&r);
        let row: serde_json::Map<String, Value> = columns
            .iter()
            .zip(values.as_array().unwrap())
            .map(|(c, v)| (c.to_string(), v.clone()))
            .collect();
        assert_eq!(super::record(&Value::Object(row)).unwrap(), r);
    }
}

/// The SQL against this machine's `database` worker: `cargo test -p bastion -- --ignored --nocapture live_store`.
#[cfg(test)]
mod live {
    use super::*;
    use crate::engine::IiiEngine;
    use bastion_protocol::Link;

    /// The store on this machine's stack, with one workspace and one job
    /// named `tag`.
    async fn open_with_job(tag: &str) -> (SqlStore, Record) {
        let engine: Arc<dyn Engine> = Arc::new(IiiEngine::connect("ws://127.0.0.1:49160"));
        while engine.link() != Link::Up {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let store = SqlStore::open(engine).await.unwrap();
        let w = Workspace {
            name: tag.into(),
            path: format!("/tmp/{tag}"),
            branch: "main".into(),
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
        tauri::async_runtime::block_on(async {
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
    fn live_governance() {
        tauri::async_runtime::block_on(async {
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
                    function: "sandbox::fs::write".into(),
                    text: "/work/repo/a.py".into(),
                },
                arguments: json!({ "path": "/work/repo/a.py" }),
                at: 9,
            };
            store.hold(&held).await.unwrap();
            let held_back = store.held(&tag).await.unwrap();
            store.release(&held.id).await.unwrap();
            let released = store.held(&tag).await.unwrap();
            println!(
                "constraints {}, grants {}, promoted {promoted:?}, held {}",
                rules.len(),
                grants.len(),
                held_back.len()
            );
            assert!(rules.len() >= govern::default_constraints().len());
            assert!(grants.len() >= govern::default_capabilities().len());
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
