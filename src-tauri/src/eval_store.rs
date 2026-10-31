//! The rows A/B evals keep (PLAN.md A6): each eval, the rungs its two arms
//! set, the job that ran each request under each arm, and the metered usage
//! of every turn of every job. `Store` extends this trait, so the factory
//! keeps one store.

use bastion_core::Stage;
use bastion_core::eval::{Override, Usage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::store::{Answer, SqlStore, name, parse};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Eval {
    pub id: String,
    pub name: String,
    pub workspace: String,
    /// The commit every run starts from, so both arms see the same code.
    pub base: String,
    pub created_at: u64,
}

/// One request under one arm, and the job that runs it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalRun {
    pub eval: String,
    /// `a` or `b`.
    pub arm: String,
    pub idx: u32,
    pub request: String,
    pub job: String,
    pub grade: Option<u8>,
}

/// One turn's usage, as `harness::metrics` metered it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub session: String,
    pub job: String,
    pub stage: Stage,
    pub usage: Usage,
    pub ended_at: u64,
}

pub trait EvalStore {
    fn add_turn<'a>(&'a self, turn: &'a Turn) -> Answer<'a, ()>;
    fn turns<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Turn>>;
    /// The eval and its arms' rungs, in one write.
    fn add_eval<'a>(
        &'a self,
        eval: &'a Eval,
        a: &'a [Override],
        b: &'a [Override],
    ) -> Answer<'a, ()>;
    /// Every eval, newest first.
    fn evals(&self) -> Answer<'_, Vec<Eval>>;
    fn eval<'a>(&'a self, id: &'a str) -> Answer<'a, Eval>;
    fn arm<'a>(&'a self, eval: &'a str, arm: &'a str) -> Answer<'a, Vec<Override>>;
    fn add_eval_run<'a>(&'a self, run: &'a EvalRun) -> Answer<'a, ()>;
    fn eval_runs<'a>(&'a self, eval: &'a str) -> Answer<'a, Vec<EvalRun>>;
    /// The eval run a job is, if it is one.
    fn eval_run_of<'a>(&'a self, job: &'a str) -> Answer<'a, Option<EvalRun>>;
    fn set_grade<'a>(&'a self, job: &'a str, grade: u8) -> Answer<'a, ()>;
}

pub const SCHEMA: [&str; 4] = [
    "CREATE TABLE IF NOT EXISTS turns (
        session TEXT PRIMARY KEY,
        job TEXT NOT NULL REFERENCES jobs(id),
        stage TEXT NOT NULL CHECK (stage IN ('plan','approve','build','review','land')),
        turns INTEGER NOT NULL CHECK (turns >= 0),
        calls INTEGER NOT NULL CHECK (calls >= 0),
        call_errors INTEGER NOT NULL CHECK (call_errors >= 0),
        cost_micros INTEGER NOT NULL CHECK (cost_micros >= 0),
        ended_at INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS evals (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL CHECK (length(name) > 0),
        workspace TEXT NOT NULL REFERENCES workspaces(name),
        base TEXT NOT NULL CHECK (length(base) >= 7),
        created_at INTEGER NOT NULL)",
    // One row per rung an arm sets. `stage` is set only for a capability.
    "CREATE TABLE IF NOT EXISTS eval_arms (
        eval TEXT NOT NULL REFERENCES evals(id),
        arm TEXT NOT NULL CHECK (arm IN ('a','b')),
        kind TEXT NOT NULL CHECK (kind IN ('constraint','capability')),
        target TEXT NOT NULL CHECK (length(target) > 0),
        stage TEXT CHECK (stage IS NULL OR stage IN ('plan','approve','build','review','land')),
        rung TEXT NOT NULL CHECK (rung IN ('told','checked','enforced','denied','ask','allowed')))",
    "CREATE TABLE IF NOT EXISTS eval_runs (
        eval TEXT NOT NULL REFERENCES evals(id),
        arm TEXT NOT NULL CHECK (arm IN ('a','b')),
        idx INTEGER NOT NULL CHECK (idx >= 0),
        request TEXT NOT NULL CHECK (length(request) > 0),
        job TEXT NOT NULL UNIQUE REFERENCES jobs(id),
        grade INTEGER CHECK (grade IS NULL OR grade BETWEEN 0 AND 10),
        PRIMARY KEY (eval, arm, idx))",
];

const RUN_COLUMNS: &str = "eval, arm, idx, request, job, grade";

/// An override as its row: kind, target, stage, rung.
fn override_row(o: &Override) -> (&'static str, &str, Option<String>, String) {
    match o {
        Override::Constraint { id, rung } => ("constraint", id, None, name(rung)),
        Override::Capability {
            stage,
            function,
            rung,
        } => ("capability", function, Some(name(stage)), name(rung)),
    }
}

fn override_of(r: &Value) -> Result<Override, String> {
    let target: String = parse(r, "target")?;
    let rung = |column| json!(r[column].as_str().unwrap_or_default());
    match r["kind"].as_str() {
        Some("constraint") => Ok(Override::Constraint {
            id: target,
            rung: serde_json::from_value(rung("rung")).map_err(|e| e.to_string())?,
        }),
        _ => Ok(Override::Capability {
            stage: parse(r, "stage")?,
            function: target,
            rung: serde_json::from_value(rung("rung")).map_err(|e| e.to_string())?,
        }),
    }
}

fn run_of(r: &Value) -> Result<EvalRun, String> {
    Ok(EvalRun {
        eval: parse(r, "eval")?,
        arm: parse(r, "arm")?,
        idx: parse(r, "idx")?,
        request: parse(r, "request")?,
        job: parse(r, "job")?,
        grade: r["grade"].as_u64().map(|g| g as u8),
    })
}

fn turn_of(r: &Value) -> Result<Turn, String> {
    Ok(Turn {
        session: parse(r, "session")?,
        job: parse(r, "job")?,
        stage: parse(r, "stage")?,
        usage: Usage {
            turns: parse(r, "turns")?,
            calls: parse(r, "calls")?,
            call_errors: parse(r, "call_errors")?,
            cost_micros: parse(r, "cost_micros")?,
        },
        ended_at: parse(r, "ended_at")?,
    })
}

fn eval_of(r: &Value) -> Result<Eval, String> {
    Ok(Eval {
        id: parse(r, "id")?,
        name: parse(r, "name")?,
        workspace: parse(r, "workspace")?,
        base: parse(r, "base")?,
        created_at: parse(r, "created_at")?,
    })
}

impl EvalStore for SqlStore {
    fn add_turn<'a>(&'a self, t: &'a Turn) -> Answer<'a, ()> {
        Box::pin(async move {
            let u = t.usage;
            self.execute(
                "INSERT OR REPLACE INTO turns (session, job, stage, turns, calls, call_errors, cost_micros, ended_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                json!([t.session, t.job, name(t.stage), u.turns, u.calls, u.call_errors, u.cost_micros, t.ended_at]),
            )
            .await
            .map(|_| ())
        })
    }

    fn turns<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Turn>> {
        Box::pin(async move {
            let sql = "SELECT session, job, stage, turns, calls, call_errors, cost_micros, ended_at FROM turns WHERE job = ? ORDER BY ended_at";
            self.rows(sql, json!([job]), turn_of).await
        })
    }

    fn add_eval<'a>(&'a self, e: &'a Eval, a: &'a [Override], b: &'a [Override]) -> Answer<'a, ()> {
        Box::pin(async move {
            let mut statements = vec![json!({
                "sql": "INSERT INTO evals (id, name, workspace, base, created_at) VALUES (?, ?, ?, ?, ?)",
                "params": [e.id, e.name, e.workspace, e.base, e.created_at],
            })];
            for (arm, overrides) in [("a", a), ("b", b)] {
                for o in overrides {
                    let (kind, target, stage, rung) = override_row(o);
                    statements.push(json!({
                        "sql": "INSERT INTO eval_arms (eval, arm, kind, target, stage, rung) VALUES (?, ?, ?, ?, ?, ?)",
                        "params": [e.id, arm, kind, target, stage, rung],
                    }));
                }
            }
            self.transaction(statements).await
        })
    }

    fn evals(&self) -> Answer<'_, Vec<Eval>> {
        Box::pin(async move {
            let sql = "SELECT id, name, workspace, base, created_at FROM evals ORDER BY created_at DESC, id";
            self.rows(sql, json!([]), eval_of).await
        })
    }

    fn eval<'a>(&'a self, id: &'a str) -> Answer<'a, Eval> {
        Box::pin(async move {
            let sql = "SELECT id, name, workspace, base, created_at FROM evals WHERE id = ?";
            let found = self.rows(sql, json!([id]), eval_of).await?;
            found
                .into_iter()
                .next()
                .ok_or_else(|| format!("no eval {id}"))
        })
    }

    fn arm<'a>(&'a self, eval: &'a str, arm: &'a str) -> Answer<'a, Vec<Override>> {
        Box::pin(async move {
            let sql = "SELECT kind, target, stage, rung FROM eval_arms WHERE eval = ? AND arm = ? ORDER BY rowid";
            self.rows(sql, json!([eval, arm]), override_of).await
        })
    }

    fn add_eval_run<'a>(&'a self, r: &'a EvalRun) -> Answer<'a, ()> {
        Box::pin(async move {
            let sql = format!("INSERT INTO eval_runs ({RUN_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?)");
            self.execute(
                &sql,
                json!([r.eval, r.arm, r.idx, r.request, r.job, r.grade]),
            )
            .await
            .map(|_| ())
        })
    }

    fn eval_runs<'a>(&'a self, eval: &'a str) -> Answer<'a, Vec<EvalRun>> {
        Box::pin(async move {
            let sql =
                format!("SELECT {RUN_COLUMNS} FROM eval_runs WHERE eval = ? ORDER BY idx, arm");
            self.rows(&sql, json!([eval]), run_of).await
        })
    }

    fn eval_run_of<'a>(&'a self, job: &'a str) -> Answer<'a, Option<EvalRun>> {
        Box::pin(async move {
            let sql = format!("SELECT {RUN_COLUMNS} FROM eval_runs WHERE job = ?");
            Ok(self
                .rows(&sql, json!([job]), run_of)
                .await?
                .into_iter()
                .next())
        })
    }

    fn set_grade<'a>(&'a self, job: &'a str, grade: u8) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(
                "UPDATE eval_runs SET grade = ? WHERE job = ?",
                json!([grade, job]),
            )
            .await
            .map(|_| ())
        })
    }
}

#[cfg(test)]
pub mod fake {
    use std::sync::Mutex;

    use super::*;
    use crate::store::fake::FakeStore;

    /// An eval with its arms' rungs, A then B.
    pub type WithArms = (Eval, Vec<Override>, Vec<Override>);

    /// The eval rows the fake store keeps.
    #[derive(Default)]
    pub struct EvalRows {
        pub turns: Mutex<Vec<Turn>>,
        pub evals: Mutex<Vec<WithArms>>,
        pub runs: Mutex<Vec<EvalRun>>,
    }

    fn ready<T: Send + 'static>(value: Result<T, String>) -> Answer<'static, T> {
        Box::pin(async move { value })
    }

    /// The rows `keep` picks, in order.
    fn picked<T: Clone>(rows: &Mutex<Vec<T>>, keep: impl Fn(&T) -> bool) -> Vec<T> {
        rows.lock()
            .unwrap()
            .iter()
            .filter(|r| keep(r))
            .cloned()
            .collect()
    }

    impl EvalStore for FakeStore {
        fn add_turn<'a>(&'a self, t: &'a Turn) -> Answer<'a, ()> {
            self.eval_rows.turns.lock().unwrap().push(t.clone());
            ready(Ok(()))
        }

        fn turns<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Turn>> {
            ready(Ok(picked(&self.eval_rows.turns, |t| t.job == job)))
        }

        fn add_eval<'a>(
            &'a self,
            e: &'a Eval,
            a: &'a [Override],
            b: &'a [Override],
        ) -> Answer<'a, ()> {
            self.eval_rows
                .evals
                .lock()
                .unwrap()
                .push((e.clone(), a.to_vec(), b.to_vec()));
            ready(Ok(()))
        }

        fn evals(&self) -> Answer<'_, Vec<Eval>> {
            let all = self
                .eval_rows
                .evals
                .lock()
                .unwrap()
                .iter()
                .rev()
                .map(|(e, ..)| e.clone())
                .collect();
            ready(Ok(all))
        }

        fn eval<'a>(&'a self, id: &'a str) -> Answer<'a, Eval> {
            let found = self
                .eval_rows
                .evals
                .lock()
                .unwrap()
                .iter()
                .find(|(e, ..)| e.id == id)
                .map(|(e, ..)| e.clone());
            ready(found.ok_or_else(|| format!("no eval {id}")))
        }

        fn arm<'a>(&'a self, eval: &'a str, arm: &'a str) -> Answer<'a, Vec<Override>> {
            let evals = self.eval_rows.evals.lock().unwrap();
            let found = evals
                .iter()
                .find(|(e, ..)| e.id == eval)
                .map(|(_, a, b)| if arm == "a" { a.clone() } else { b.clone() });
            ready(Ok(found.unwrap_or_default()))
        }

        fn add_eval_run<'a>(&'a self, r: &'a EvalRun) -> Answer<'a, ()> {
            self.eval_rows.runs.lock().unwrap().push(r.clone());
            ready(Ok(()))
        }

        fn eval_runs<'a>(&'a self, eval: &'a str) -> Answer<'a, Vec<EvalRun>> {
            ready(Ok(picked(&self.eval_rows.runs, |r| r.eval == eval)))
        }

        fn eval_run_of<'a>(&'a self, job: &'a str) -> Answer<'a, Option<EvalRun>> {
            ready(Ok(picked(&self.eval_rows.runs, |r| r.job == job)
                .into_iter()
                .next()))
        }

        fn set_grade<'a>(&'a self, job: &'a str, grade: u8) -> Answer<'a, ()> {
            for r in self
                .eval_rows
                .runs
                .lock()
                .unwrap()
                .iter_mut()
                .filter(|r| r.job == job)
            {
                r.grade = Some(grade);
            }
            ready(Ok(()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastion_core::govern::{Access, Enforcement};

    #[test]
    fn overrides_read_back_from_their_rows() {
        let overrides = [
            Override::Constraint {
                id: "keep-tests".into(),
                rung: Enforcement::Enforced,
            },
            Override::Capability {
                stage: Stage::Plan,
                function: "sandbox::fs::write".into(),
                rung: Access::Allowed,
            },
        ];
        for o in overrides {
            let (kind, target, stage, rung) = override_row(&o);
            let row = json!({ "kind": kind, "target": target, "stage": stage, "rung": rung });
            assert_eq!(override_of(&row).unwrap(), o);
        }
    }

    #[test]
    fn a_run_row_reads_back_with_or_without_a_grade() {
        let row =
            json!({ "eval": "e1", "arm": "b", "idx": 2, "request": "r", "job": "j1", "grade": 7 });
        assert_eq!(run_of(&row).unwrap().grade, Some(7));
        let ungraded = json!({ "eval": "e1", "arm": "b", "idx": 2, "request": "r", "job": "j1", "grade": null });
        assert_eq!(run_of(&ungraded).unwrap().grade, None);
    }
}
