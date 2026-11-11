//! `SqlStore`: the only code that writes SQL, on the `database` worker.

use std::sync::Arc;

use bastion_core::{Job, Stage, State};
use serde_json::{Value, json};

use super::schema::*;
use super::*;
use crate::engine::NS;

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
                NS,
                "database::transaction",
                json!({ "statements": statements }),
            )
            .await?;
        for sql in ALTERS {
            let applied = engine
                .call(NS, "database::execute", json!({ "sql": sql, "params": [] }))
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
                NS,
                "database::transaction",
                json!({ "statements": statements }),
            )
            .await
            .map(|_| ())
    }

    pub(crate) async fn query(&self, sql: &str, params: Value) -> Result<Vec<Value>, String> {
        let out = self
            .engine
            .call(
                NS,
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
                NS,
                "database::execute",
                json!({ "sql": sql, "params": params }),
            )
            .await?;
        Ok(out["returned_rows"].as_array().cloned().unwrap_or_default())
    }
}

pub(super) fn text(row: &Value, column: &str) -> Option<String> {
    row[column].as_str().map(String::from)
}

/// A live job kept at a checkpoint before D33 waits on the step it judged.
pub(super) fn record(row: &Value) -> Result<Record, String> {
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
            model: text(row, "model"),
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

/// The approved files as one column, one per line, or NULL when none.
pub(super) fn approved(r: &Record) -> Option<String> {
    let files = &r.runtime.approved;
    (!files.is_empty()).then(|| files.join("\n"))
}

pub(super) fn job_params(r: &Record) -> Value {
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
        approved(r),
        r.runtime.model
    ])
}

/// A job's tasks, results and revision counts, written whole over what was there.
pub(super) fn list_statements(j: &Job) -> Vec<Value> {
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
                "INSERT INTO workspaces (name, path, branch, checks, max_jobs, revision_budget, branch_prefix, setup, cleanup, env, gates, autofix, image, finish, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch())",
                json!([w.name, w.path, w.branch, w.checks, w.max_jobs, w.revision_budget, w.branch_prefix, w.setup, w.cleanup, w.env, w.gates, w.autofix, w.image, w.finish]),
            )
            .await
            .map(|_| ())
        })
    }

    fn workspaces(&self) -> Answer<'_, Vec<Workspace>> {
        Box::pin(async move {
            self.query(
                "SELECT name, path, branch, checks, max_jobs, revision_budget, branch_prefix, setup, cleanup, env, gates, autofix, image, finish FROM workspaces ORDER BY name",
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
                    image: parse(r, "image")?,
                    finish: parse(r, "finish")?,
                })
            })
            .collect()
        })
    }

    fn edit_workspace<'a>(&'a self, w: &'a Workspace) -> Answer<'a, ()> {
        Box::pin(async move {
            let found = self
                .execute(
                    "UPDATE workspaces SET branch = ?, checks = ?, max_jobs = ?, revision_budget = ?, branch_prefix = ?, setup = ?, cleanup = ?, env = ?, gates = ?, autofix = ?, image = ?, finish = ? WHERE name = ? RETURNING name",
                    json!([w.branch, w.checks, w.max_jobs, w.revision_budget, w.branch_prefix, w.setup, w.cleanup, w.env, w.gates, w.autofix, w.image, w.finish, w.name]),
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
}
