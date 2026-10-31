//! Commands for A/B evals (PLAN.md A6).

use std::sync::Arc;

use bastion_core::eval::{self, Override, Run};
use bastion_protocol::{EvalRow, EvalSummary, EvalView, NewEval, RunView};
use serde_json::json;
use tauri::State;

use super::factory::{FactoryCell, factory};
use crate::eval_store::Eval;
use crate::evals::{self, Plan, RunRead};
use crate::factory::Factory;
use crate::machine::Machine;

/// `constraint:<id>` or `capability:<stage>:<function>`, at one rung.
fn override_of(target: &str, rung: &str) -> Result<Override, String> {
    let value = |text: &str| json!(text);
    let bad = || format!("{target:?} at {rung:?} is not a constraint or a grant at a rung");
    match target.split_once(':') {
        Some(("constraint", id)) => Ok(Override::Constraint {
            id: id.into(),
            rung: serde_json::from_value(value(rung)).map_err(|_| bad())?,
        }),
        Some(("capability", rest)) => {
            let (stage, function) = rest.split_once(':').ok_or_else(bad)?;
            Ok(Override::Capability {
                stage: serde_json::from_value(value(stage)).map_err(|_| bad())?,
                function: function.into(),
                rung: serde_json::from_value(value(rung)).map_err(|_| bad())?,
            })
        }
        _ => Err(bad()),
    }
}

fn describe(o: &Override) -> String {
    match o {
        Override::Constraint { id, rung } => format!("constraint {id} at {rung:?}"),
        Override::Capability {
            stage,
            function,
            rung,
        } => format!("{stage:?} {function} at {rung:?}"),
    }
}

fn view_of(t: &Run, job: &str, state: &str, refused_calls: Vec<String>) -> RunView {
    RunView {
        job: job.into(),
        state: state.into(),
        reviewed: t.reviewed,
        passed: t.passed,
        refused: t.refused,
        held: t.held,
        flagged: t.flagged,
        hook_refused: t.hook_refused,
        findings: t.findings,
        turns: t.usage.turns,
        cost_micros: t.usage.cost_micros,
        seconds: t.seconds,
        grade: t.grade,
        refused_calls,
    }
}

fn finished(r: &RunRead) -> bool {
    let over = matches!(r.state.as_str(), "stopped" | "failed" | "landed");
    over && (!r.tally.reviewed || r.run.grade.is_some())
}

async fn reads(factory: &Factory, e: &Eval) -> Result<Vec<RunRead>, String> {
    let chain = factory.gate().audit()?.read().await?.chain;
    let mut out = Vec::new();
    for run in factory.store().eval_runs(&e.id).await? {
        out.push(evals::read_run(&**factory.store(), &chain, run).await?);
    }
    Ok(out)
}

fn summary(e: Eval, runs: &[RunRead]) -> EvalSummary {
    EvalSummary {
        id: e.id,
        name: e.name,
        workspace: e.workspace,
        base: e.base,
        created_at: e.created_at,
        runs: runs.len() as u32,
        done: runs.iter().filter(|r| finished(r)).count() as u32,
    }
}

#[tauri::command]
pub async fn evals(cell: State<'_, FactoryCell>) -> Result<Vec<EvalSummary>, String> {
    let factory = factory(&cell)?;
    let mut out = Vec::new();
    for e in factory.store().evals().await? {
        let runs = reads(&factory, &e).await?;
        out.push(summary(e, &runs));
    }
    Ok(out)
}

#[tauri::command]
pub async fn eval_create(
    cell: State<'_, FactoryCell>,
    machine: State<'_, Arc<dyn Machine>>,
    eval: NewEval,
) -> Result<String, String> {
    let plan = Plan {
        name: eval.name,
        workspace: eval.workspace,
        requests: eval.requests,
        a: vec![override_of(eval.target.trim(), &eval.rung_a)?],
        b: vec![override_of(eval.target.trim(), &eval.rung_b)?],
    };
    evals::create(&factory(&cell)?, machine.inner().as_ref(), plan).await
}

/// One eval: each request under A and B, the totals, and what differs.
#[tauri::command]
pub async fn eval(cell: State<'_, FactoryCell>, id: String) -> Result<EvalView, String> {
    let factory = factory(&cell)?;
    let e = factory.store().eval(&id).await?;
    let (arm_a, arm_b) = (
        factory.store().arm(&id, "a").await?,
        factory.store().arm(&id, "b").await?,
    );
    let runs = reads(&factory, &e).await?;
    let pick = |arm: &str, idx: u32| runs.iter().find(|r| r.run.arm == arm && r.run.idx == idx);
    let mut idxs: Vec<u32> = runs.iter().map(|r| r.run.idx).collect();
    idxs.dedup();
    let rows = idxs
        .into_iter()
        .map(|idx| {
            let (a, b) = (pick("a", idx), pick("b", idx));
            let differences = match (a, b) {
                (Some(a), Some(b)) => eval::differences(&a.tally, &b.tally),
                _ => Vec::new(),
            };
            let view = |r: Option<&RunRead>| {
                r.map(|r| view_of(&r.tally, &r.run.job, &r.state, r.refused.clone()))
            };
            EvalRow {
                idx,
                request: a.or(b).map(|r| r.run.request.clone()).unwrap_or_default(),
                a: view(a),
                b: view(b),
                differences,
            }
        })
        .collect();
    let total = |arm: &str| {
        eval::total(
            &runs
                .iter()
                .filter(|r| r.run.arm == arm)
                .map(|r| r.tally.clone())
                .collect::<Vec<_>>(),
        )
    };
    let (total_a, total_b) = (total("a"), total("b"));
    Ok(EvalView {
        summary: summary(e, &runs),
        arm_a: arm_a.iter().map(describe).collect(),
        arm_b: arm_b.iter().map(describe).collect(),
        rows,
        differences: eval::differences(&total_a, &total_b),
        total_a: view_of(&total_a, "", "", Vec::new()),
        total_b: view_of(&total_b, "", "", Vec::new()),
    })
}

/// Set the live policy to one arm's rungs, and record which eval it came from.
#[tauri::command]
pub async fn eval_apply(
    cell: State<'_, FactoryCell>,
    id: String,
    arm: String,
) -> Result<(), String> {
    let factory = factory(&cell)?;
    let overrides = factory.store().arm(&id, &arm).await?;
    if overrides.is_empty() {
        return Err(format!("eval {id} has no arm {arm:?}"));
    }
    factory.gate().apply_arm(&id, &arm, &overrides).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval_store::EvalRun;
    use bastion_core::Stage;
    use bastion_core::govern::{Access, Enforcement};

    #[test]
    fn a_target_reads_as_an_override() {
        assert_eq!(
            override_of("constraint:keep-tests", "enforced"),
            Ok(Override::Constraint {
                id: "keep-tests".into(),
                rung: Enforcement::Enforced
            })
        );
        assert_eq!(
            override_of("capability:plan:sandbox::fs::write", "allowed"),
            Ok(Override::Capability {
                stage: Stage::Plan,
                function: "sandbox::fs::write".into(),
                rung: Access::Allowed
            })
        );
        assert!(override_of("constraint:keep-tests", "allowed").is_err());
        assert!(override_of("keep-tests", "told").is_err());
    }

    #[test]
    fn a_run_counts_as_done_once_over_and_graded() {
        let run = |state: &str, reviewed: bool, grade: Option<u8>| RunRead {
            run: EvalRun {
                eval: "e".into(),
                arm: "a".into(),
                idx: 0,
                request: "r".into(),
                job: "j".into(),
                grade,
            },
            state: state.into(),
            tally: Run {
                reviewed,
                ..Run::default()
            },
            refused: vec![],
        };
        assert!(!finished(&run("running", false, None)));
        assert!(
            !finished(&run("stopped", true, None)),
            "waits for its grade"
        );
        assert!(finished(&run("stopped", true, Some(7))));
        assert!(finished(&run("failed", false, None)));
    }
}
