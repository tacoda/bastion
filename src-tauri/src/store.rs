//! The Store seam (PLAN.md §6.2): workspaces, jobs and artifacts as rows in
//! the `database` worker, each job with a `rev` so a stale write is refused
//! (D8). `SqlStore` is the only code that writes SQL.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bastion_core::{Dial, Dials, Job, Stage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::Engine;

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

/// A save lost to another writer (D8). The caller reloads and steps again.
pub const STALE: &str = "stale: the job changed since it was read";

pub trait Store: Send + Sync {
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
    fn set_dials<'a>(&'a self, dials: &'a Dials) -> Answer<'a, ()>;
}

const NAMESPACE: &str = "bastion";

/// The schema (D8): typed columns with constraints, no JSON in a text column.
// ponytail: idempotent CREATEs; add numbered migrations when a column changes.
const SCHEMA: [&str; 4] = [
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
        updated_at INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS artifacts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        job TEXT NOT NULL REFERENCES jobs(id),
        stage TEXT NOT NULL CHECK (stage IN ('plan','approve','build','review','land')),
        body TEXT NOT NULL,
        created_at INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS dials (
        checkpoint TEXT PRIMARY KEY CHECK (checkpoint IN ('approve','review')),
        dial TEXT NOT NULL CHECK (dial IN ('ask','hold','auto')))",
];

const JOB_COLUMNS: &str = "id, workspace, request, stage, state, question, feedback, plan, failures, error, worktree, sandbox, session, rev, created_at, updated_at";

pub struct SqlStore {
    engine: Arc<dyn Engine>,
}

impl SqlStore {
    /// Make the tables if they are missing.
    pub async fn open(engine: Arc<dyn Engine>) -> Result<Self, String> {
        let statements: Vec<Value> = SCHEMA
            .iter()
            .map(|sql| json!({ "sql": sql, "params": [] }))
            .collect();
        engine
            .call(
                NAMESPACE,
                "database::transaction",
                json!({ "statements": statements }),
            )
            .await?;
        Ok(Self { engine })
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

    async fn execute(&self, sql: &str, params: Value) -> Result<Vec<Value>, String> {
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
fn name<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

fn parse<T: for<'de> Deserialize<'de>>(row: &Value, column: &str) -> Result<T, String> {
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
            updated_at: parse(row, "updated_at")?,
        },
        runtime: Runtime {
            worktree: text(row, "worktree"),
            sandbox: text(row, "sandbox"),
            session: text(row, "session"),
        },
        rev: parse(row, "rev")?,
        created_at: parse(row, "created_at")?,
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
        j.updated_at
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
                "INSERT INTO jobs ({JOB_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
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
            self.query(
                &format!("SELECT {JOB_COLUMNS} FROM jobs ORDER BY created_at DESC, id"),
                json!([]),
            )
            .await?
            .iter()
            .map(record)
            .collect()
        })
    }

    fn save<'a>(&'a self, r: &'a Record) -> Answer<'a, i64> {
        Box::pin(async move {
            let j = &r.job;
            let rows = self
                .execute(
                    "UPDATE jobs SET stage = ?, state = ?, question = ?, feedback = ?, plan = ?, failures = ?, error = ?, worktree = ?, sandbox = ?, session = ?, updated_at = ?, rev = rev + 1
                     WHERE id = ? AND rev = ? RETURNING rev",
                    json!([
                        name(j.stage), name(j.state), j.question, j.feedback, j.plan, j.failures, j.error,
                        r.runtime.worktree, r.runtime.sandbox, r.runtime.session, j.updated_at, j.id, r.rev
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

    fn set_dials<'a>(&'a self, dials: &'a Dials) -> Answer<'a, ()> {
        Box::pin(async move {
            for (checkpoint, dial) in [("approve", dials.approve), ("review", dials.review)] {
                self.execute(
                    "INSERT INTO dials (checkpoint, dial) VALUES (?, ?) ON CONFLICT (checkpoint) DO UPDATE SET dial = excluded.dial",
                    json!([checkpoint, name(dial)]),
                )
                .await?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
pub mod fake {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;

    /// Rows in memory, with the same `rev` rule as `SqlStore`.
    #[derive(Default)]
    pub struct FakeStore {
        pub workspaces: Mutex<Vec<Workspace>>,
        pub jobs: Mutex<BTreeMap<String, Record>>,
        pub artifacts: Mutex<Vec<(String, Artifact)>>,
        pub dials: Mutex<Dials>,
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
            let found = self
                .artifacts
                .lock()
                .unwrap()
                .iter()
                .filter(|(j, _)| j == job)
                .map(|(_, a)| a.clone())
                .collect();
            Box::pin(async move { Ok(found) })
        }

        fn dials(&self) -> Answer<'_, Dials> {
            let dials = *self.dials.lock().unwrap();
            Box::pin(async move { Ok(dials) })
        }

        fn set_dials<'a>(&'a self, dials: &'a Dials) -> Answer<'a, ()> {
            *self.dials.lock().unwrap() = *dials;
            Box::pin(async { Ok(()) })
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
            assert_eq!(update["params"][11], "j1");
            assert_eq!(update["params"][12], 1);
        });
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

    #[test]
    #[ignore = "needs this machine's stack"]
    fn live_store() {
        tauri::async_runtime::block_on(async {
            let engine: Arc<dyn Engine> = Arc::new(IiiEngine::connect("ws://127.0.0.1:49160"));
            while engine.link() != Link::Up {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let store = SqlStore::open(engine.clone()).await.unwrap();
            let tag = format!("live-{}", std::process::id());
            let w = Workspace {
                name: tag.clone(),
                path: format!("/tmp/{tag}"),
                branch: "main".into(),
            };
            store.add_workspace(&w).await.unwrap();
            let mut r = Record {
                job: Job::new(tag.clone(), tag.clone(), "check the store".into(), 7),
                runtime: Runtime::default(),
                rev: 1,
                created_at: 7,
            };
            store.insert(&r).await.unwrap();
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
}
