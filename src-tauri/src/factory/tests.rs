//! The short pipeline end to end, on the fake engine, store and machine.

use std::sync::Arc;
use std::time::Duration;

use bastion_core::{Event, Stage, State};
use bastion_protocol::Link;
use serde_json::json;

use super::Factory;
use crate::engine::Engine;
use crate::engine::fake::FakeEngine;
use crate::machine::fake::FakeMachine;
use crate::store::fake::FakeStore;
use crate::store::{Store, Workspace};

fn engine() -> FakeEngine {
    FakeEngine::new(Link::Up)
        .answer("worktree::create", Ok(json!({ "worktree_id": "wt1" })))
        .answer(
            "worktree::get",
            Ok(json!({ "path": "/wt", "branch": "iii/wt1" })),
        )
        .answer("sandbox::create", Ok(json!({ "sandbox_id": "vm1" })))
        .answer("sandbox::fs::write", Ok(json!({})))
        .answer("sandbox::exec", Ok(json!({ "exit_code": 0 })))
        .answer("sandbox::fs::read", Ok(json!({ "content": {} })))
        .answer("sandbox::stop", Ok(json!({})))
        .answer("approval::set-mode", Ok(json!({})))
        .answer("harness::send", Ok(json!({})))
        .answer(
            "harness::status",
            Ok(json!({ "status": "completed", "result": "\"1. Add /h …(truncated)" })),
        )
        .answer(
            "session::messages",
            Ok(json!({ "messages": [
                { "message": { "role": "user", "content": [{ "type": "text", "text": "plan it" }] } },
                { "message": { "role": "assistant", "content": [{ "type": "text", "text": "1. Add /health" }] } },
            ]})),
        )
        .answer("worktree::land", Ok(json!({ "queued": true })))
}

struct Rig {
    factory: Arc<Factory>,
    engine: Arc<FakeEngine>,
    store: Arc<FakeStore>,
}

fn rig() -> Rig {
    let engine = Arc::new(engine());
    let store = Arc::new(FakeStore::default());
    store.workspaces.lock().unwrap().push(Workspace {
        name: "w".into(),
        path: "/repo".into(),
        branch: "main".into(),
    });
    let machine = Arc::new(FakeMachine::new().run_ok("git", ""));
    let factory = Factory::new(
        engine.clone() as Arc<dyn Engine>,
        store.clone() as Arc<dyn Store>,
        machine,
        Arc::new(|_| {}),
        "/scratch".into(),
    );
    Rig {
        factory,
        engine,
        store,
    }
}

/// Wait until the job reaches `stage` and `state`, or fail after two seconds.
async fn until(rig: &Rig, id: &str, stage: Stage, state: State) {
    for _ in 0..200 {
        let job = rig.store.load(id).await.unwrap().job;
        if (job.stage, job.state) == (stage, state) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let job = rig.store.load(id).await.unwrap().job;
    panic!(
        "job is at {:?} {:?}, not {stage:?} {state:?}: {:?}",
        job.stage, job.state, job.error
    );
}

/// Fire the turn-completed binding for the job's current session.
async fn finish_turn(rig: &Rig, id: &str) {
    let mut session = String::new();
    for _ in 0..200 {
        // The turn saves its session before it binds, so read it again each try.
        session = rig
            .store
            .load(id)
            .await
            .unwrap()
            .runtime
            .session
            .unwrap_or_default();
        if !rig
            .engine
            .fire(
                "harness::turn-completed",
                "session_id",
                json!({ "session_id": session }),
            )
            .is_empty()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no turn-completed binding for {session}");
}

#[test]
fn a_request_runs_the_short_pipeline_to_landed() {
    let rig = rig();
    tauri::async_runtime::block_on(async {
        let id = rig.factory.submit("w", "Add a health check").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        let record = rig.store.load(&id).await.unwrap();
        assert_eq!(
            (
                record.runtime.worktree.as_deref(),
                record.runtime.sandbox.as_deref()
            ),
            (Some("wt1"), Some("vm1"))
        );
        assert!(rig.engine.called("harness::send"));

        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Approve, State::NeedsYou).await;
        let plan = rig.store.artifacts(&id).await.unwrap();
        assert_eq!(
            (plan[0].stage, plan[0].body.as_str()),
            (Stage::Plan, "1. Add /health")
        );

        rig.factory.apply(&id, Event::Approved).await.unwrap();
        until(&rig, &id, Stage::Build, State::Running).await;
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Review, State::NeedsYou).await;

        rig.factory.apply(&id, Event::Approved).await.unwrap();
        until(&rig, &id, Stage::Land, State::Running).await;
        for _ in 0..200 {
            if !rig
                .engine
                .fire(
                    "worktree::landed",
                    "worktree_id",
                    json!({ "worktree_id": "wt1" }),
                )
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        until(&rig, &id, Stage::Land, State::Landed).await;
        assert!(rig.engine.called("worktree::land"));
        for _ in 0..200 {
            if rig.engine.called("sandbox::stop") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the VM was never stopped");
    });
}

#[test]
fn a_failed_provision_fails_the_job_with_its_reason() {
    let engine = Arc::new(engine().answer(
        "worktree::create",
        Err("W503 repository not in gates.repos".into()),
    ));
    let store = Arc::new(FakeStore::default());
    store.workspaces.lock().unwrap().push(Workspace {
        name: "w".into(),
        path: "/repo".into(),
        branch: "main".into(),
    });
    let factory = Factory::new(
        engine.clone() as Arc<dyn Engine>,
        store.clone() as Arc<dyn Store>,
        Arc::new(FakeMachine::new()),
        Arc::new(|_| {}),
        "/s".into(),
    );
    let rig = Rig {
        factory,
        engine,
        store,
    };
    tauri::async_runtime::block_on(async {
        let id = rig.factory.submit("w", "x").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Failed).await;
        let job = rig.store.load(&id).await.unwrap().job;
        assert!(job.error.unwrap().contains("W503"));
    });
}
