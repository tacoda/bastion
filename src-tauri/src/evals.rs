//! A/B evals in the factory (PLAN.md A6, D22). An eval submits each request
//! once per arm as a trial job from the same commit; the gate judges each
//! trial under its arm's rungs. Every turn's usage is metered, and each run
//! that reaches review is graded by a turn with no tools. The comparison
//! itself is `bastion_core::eval`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bastion_core::audit::Record;
use bastion_core::eval::{self, Override, Run, Usage};
use bastion_core::{Job, Stage};
use serde_json::json;

use crate::engine::Engine;
use crate::eval_store::{Eval, EvalRun, Turn};
use crate::factory::{Factory, final_reply, now, short_id};
use crate::machine::Machine;
use crate::store::Store;

const NS: &str = "bastion";
/// How long a grading turn may take.
const GRADE_SECS: u64 = 180;

/// Record one turn's usage, as the harness metered its session.
pub async fn meter(engine: &dyn Engine, store: &dyn Store, job: &Job, session: &str) {
    let metered = engine
        .call(
            NS,
            "harness::metrics",
            json!({ "root_session_id": session }),
        )
        .await;
    let Ok(out) = metered else { return };
    let t = &out["totals"];
    let count = |key: &str| t[key].as_u64().unwrap_or(0) as u32;
    let turn = Turn {
        session: session.into(),
        job: job.id.clone(),
        stage: job.stage,
        usage: Usage {
            turns: count("turns"),
            calls: count("function_calls"),
            call_errors: count("function_call_errors"),
            cost_micros: (t["cost_usd"].as_f64().unwrap_or(0.0) * 1e6).round() as u64,
        },
        ended_at: now(),
    };
    if let Err(e) = store.add_turn(&turn).await {
        eprintln!("job {}: the turn's usage was not kept: {e}", job.id);
    }
}

/// Grade a run that reached review: a turn with no tools reads the request
/// and the diff, and its last `GRADE: n` line is the grade.
// ponytail: polls harness::status every 3 s; bind turn-completed if grading ever runs in bulk.
pub async fn grade(engine: &dyn Engine, store: &dyn Store, run: &EvalRun) -> Result<u8, String> {
    let artifacts = store.artifacts(&run.job).await?;
    let diff = artifacts
        .iter()
        .rev()
        .find(|a| a.stage == Stage::Review)
        .map(|a| a.body.clone())
        .ok_or("the run has no diff to grade")?;
    let session = format!("{}-grade-{}", run.job, short_id());
    let brief = eval::grading_brief(&run.request, &diff);
    engine
        .call_for(NS, "harness::send", json!({
            "session_id": session, "provider": crate::factory::PROVIDER, "model": crate::factory::MODEL,
            "message": brief, "options": { "max_turns": 1, "functions": { "allow": [] } },
        }), 60_000)
        .await?;
    for _ in 0..GRADE_SECS / 3 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let status = engine
            .call(NS, "harness::status", json!({ "session_id": session }))
            .await?;
        if status["status"] == "completed" {
            let reply = final_reply(engine, &session).await?;
            let g =
                eval::grade(&reply).ok_or_else(|| format!("the judge gave no grade: {reply}"))?;
            store.set_grade(&run.job, g).await?;
            return Ok(g);
        }
    }
    Err(format!("the grading turn ran past {GRADE_SECS} s"))
}

/// What an owner asks for: requests, and the rungs each arm sets.
pub struct Plan {
    pub name: String,
    pub workspace: String,
    pub requests: Vec<String>,
    pub a: Vec<Override>,
    pub b: Vec<Override>,
}

/// Pin the workspace's branch to its current commit, keep the eval, and
/// submit each request under each arm.
pub async fn create(
    factory: &Arc<Factory>,
    machine: &dyn Machine,
    plan: Plan,
) -> Result<String, String> {
    let requests: Vec<String> = plan
        .requests
        .iter()
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .collect();
    if plan.name.trim().is_empty() || requests.is_empty() {
        return Err("an eval needs a name and at least one request".into());
    }
    let workspaces = factory.store().workspaces().await?;
    let ws = workspaces
        .iter()
        .find(|w| w.name == plan.workspace)
        .ok_or(format!("no workspace {}", plan.workspace))?;
    let head = machine.run("git", &["rev-parse", &ws.branch], Some(Path::new(&ws.path)))?;
    if !head.success {
        return Err(format!(
            "{} has no branch {}: {}",
            ws.path,
            ws.branch,
            head.text.trim()
        ));
    }
    let e = Eval {
        id: format!("e{}", short_id()),
        name: plan.name.trim().into(),
        workspace: ws.name.clone(),
        base: head.text.trim().into(),
        created_at: now(),
    };
    factory.store().add_eval(&e, &plan.a, &plan.b).await?;
    for (idx, request) in requests.iter().enumerate() {
        for arm in ["a", "b"] {
            let run = EvalRun {
                eval: e.id.clone(),
                arm: arm.into(),
                idx: idx as u32,
                request: request.clone(),
                job: String::new(),
                grade: None,
            };
            factory.submit_trial(&e, run).await?;
        }
    }
    factory.gate().note_eval(&e, requests.len());
    Ok(e.id)
}

/// One run as the eval page reads it: tallied from its audit record and its
/// turns, with the calls the gate refused.
pub struct RunRead {
    pub run: EvalRun,
    pub state: String,
    pub tally: Run,
    pub refused: Vec<String>,
}

pub async fn read_run(
    store: &dyn Store,
    chain: &[bastion_core::audit::Entry],
    run: EvalRun,
) -> Result<RunRead, String> {
    let record = store.load(&run.job).await?;
    let records: Vec<Record> = chain
        .iter()
        .filter(|e| e.record.subject == run.job)
        .map(|e| e.record.clone())
        .collect();
    let mut tally = eval::tally(&records);
    tally.usage = store
        .turns(&run.job)
        .await?
        .iter()
        .fold(Usage::default(), |u, t| u + t.usage);
    tally.reviewed = record.job.stage == Stage::Review;
    tally.seconds = record.job.updated_at.saturating_sub(record.created_at);
    tally.grade = run.grade;
    let refused = chain
        .iter()
        .filter(|e| {
            e.record.subject == run.job
                && (e.record.detail.starts_with("deny") || e.record.kind == "hook-refuse")
        })
        .map(|e| format!("#{} {}", e.seq, e.record.detail))
        .collect();
    let state = serde_json::to_value(record.job.state)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    Ok(RunRead {
        run,
        state,
        tally,
        refused,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeEngine;
    use crate::store::Artifact;
    use crate::store::fake::FakeStore;
    use bastion_protocol::Link;

    #[test]
    fn a_turn_is_metered_from_its_session_totals() {
        let engine = FakeEngine::new(Link::Up).answer(
            "harness::metrics",
            Ok(json!({ "totals": { "turns": 17, "function_calls": 16, "function_call_errors": 3, "cost_usd": 0.0111751 } })),
        );
        let store = FakeStore::default();
        let mut job = Job::new("j1".into(), "w".into(), "r".into(), 1);
        job.stage = Stage::Build;
        tauri::async_runtime::block_on(meter(&engine, &store, &job, "s1"));
        let turns = store.eval_rows.turns.lock().unwrap().clone();
        assert_eq!(
            turns[0].usage,
            Usage {
                turns: 17,
                calls: 16,
                call_errors: 3,
                cost_micros: 11175
            }
        );
    }

    #[test]
    fn a_run_is_graded_from_the_judge_reply() {
        let engine = FakeEngine::new(Link::Up)
            .answer("harness::send", Ok(json!({})))
            .answer("harness::status", Ok(json!({ "status": "completed" })))
            .answer("session::messages", Ok(json!({ "messages": [
                { "message": { "role": "assistant", "content": [{ "type": "text", "text": "Does it.\nGRADE: 9" }] } },
            ] })));
        let store = FakeStore::default();
        let run = EvalRun {
            eval: "e1".into(),
            arm: "a".into(),
            idx: 0,
            request: "r".into(),
            job: "j1".into(),
            grade: None,
        };
        store.eval_rows.runs.lock().unwrap().push(run.clone());
        tauri::async_runtime::block_on(async {
            assert!(grade(&engine, &store, &run).await.is_err(), "no diff yet");
            let diff = Artifact {
                stage: Stage::Review,
                body: "+x".into(),
                created_at: 1,
            };
            store.add_artifact("j1", &diff).await.unwrap();
            assert_eq!(grade(&engine, &store, &run).await, Ok(9));
        });
        assert_eq!(store.eval_rows.runs.lock().unwrap()[0].grade, Some(9));
        let calls = engine.calls.lock().unwrap();
        let sent = calls.iter().find(|c| c.1 == "harness::send").unwrap();
        assert_eq!(sent.2["options"]["functions"]["allow"], json!([]));
    }
}
