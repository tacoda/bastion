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

/// Run a submitted job through its plan, the approval and its build, to review.
async fn to_review(rig: &Rig, id: &str) {
    until(rig, id, Stage::Plan, State::Running).await;
    finish_turn(rig, id).await;
    until(rig, id, Stage::Approve, State::NeedsYou).await;
    rig.factory.apply(id, Event::Approved).await.unwrap();
    until(rig, id, Stage::Build, State::Running).await;
    finish_turn(rig, id).await;
    until(rig, id, Stage::Review, State::NeedsYou).await;
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
        let review: Vec<_> = rig
            .store
            .artifacts(&id)
            .await
            .unwrap()
            .into_iter()
            .filter(|a| a.stage == Stage::Review)
            .collect();
        assert_eq!(
            review[0].body,
            "No commits: the build left nothing to land."
        );

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
        let land = rig
            .engine
            .calls
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.1 == "worktree::land")
            .map(|c| c.2.clone())
            .unwrap();
        assert_eq!(land["target_branch"], "main");
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

#[test]
fn an_auto_approve_dial_skips_the_owner_at_approve() {
    let rig = rig();
    *rig.store.dials.lock().unwrap() = bastion_core::Dials {
        approve: bastion_core::Dial::Auto,
        review: bastion_core::Dial::Ask,
        ..bastion_core::Dials::default()
    };
    tauri::async_runtime::block_on(async {
        let id = rig.factory.submit("w", "x").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Build, State::Running).await;
    });
}

#[test]
fn a_trial_runs_from_its_eval_commit_under_its_arm_and_stops_at_review() {
    use crate::eval_store::{Eval, EvalStore};
    use bastion_core::eval::Override;
    use bastion_core::govern::Enforcement;

    let rig = rig();
    let e = Eval {
        id: "e1".into(),
        name: "keep tests".into(),
        workspace: "w".into(),
        base: "abc1234".into(),
        created_at: 1,
    };
    let b = [Override::Constraint {
        id: "keep-tests".into(),
        rung: Enforcement::Enforced,
    }];
    tauri::async_runtime::block_on(async {
        rig.store.add_eval(&e, &[], &b).await.unwrap();
        let run = crate::eval_store::EvalRun {
            eval: "e1".into(),
            arm: "b".into(),
            idx: 0,
            request: "x".into(),
            job: String::new(),
            grade: None,
        };
        let id = rig.factory.submit_trial(&e, run).await.unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        let create = rig
            .engine
            .calls
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.1 == "worktree::create")
            .map(|c| c.2.clone())
            .unwrap();
        assert_eq!(
            create["base_ref"], "abc1234",
            "the trial starts from the eval's commit"
        );
        let rules = rig.factory.gate().constraints(&id);
        assert_eq!(
            rules.iter().find(|c| c.id == "keep-tests").map(|c| c.rung),
            None,
            "the fake store has no owner rules"
        );

        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Build, State::Running).await;
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Review, State::Stopped).await;
        assert!(
            rig.store
                .artifacts(&id)
                .await
                .unwrap()
                .iter()
                .any(|a| a.stage == Stage::Review)
        );
        assert!(!rig.engine.called("worktree::land"), "a trial never lands");
        assert!(
            rig.engine.called("harness::metrics"),
            "each turn is metered"
        );
    });
}

/// A rig whose workspace's origin is on GitHub, where every gh call
/// answers with the pull request's URL.
fn github_rig(pr: &str) -> Rig {
    let engine = Arc::new(
        engine()
            .answer(
                "sandbox::exec",
                Ok(json!({ "exit_code": 0, "stdout": "+x" })),
            )
            .answer(
                "github::exec",
                Ok(json!({ "exit_code": 0, "stdout": format!("{pr}\n") })),
            ),
    );
    let store = Arc::new(FakeStore::default());
    store.workspaces.lock().unwrap().push(Workspace {
        name: "w".into(),
        path: "/repo".into(),
        branch: "main".into(),
    });
    let machine = Arc::new(FakeMachine::new().run_ok("git", "git@github.com:o/r.git\n"));
    let factory = Factory::new(
        engine.clone() as Arc<dyn Engine>,
        store.clone() as Arc<dyn Store>,
        machine,
        Arc::new(|_| {}),
        "/s".into(),
    );
    Rig {
        factory,
        engine,
        store,
    }
}

#[test]
fn with_github_as_the_forge_a_reviewed_change_goes_up_as_a_pull_request_and_merges() {
    use crate::settings_store::SettingsStore;
    use bastion_core::settings::{Forge, Settings};

    let pr = "https://github.com/o/r/pull/1";
    let rig = github_rig(pr);
    tauri::async_runtime::block_on(async {
        let github = Settings {
            forge: Forge::Github,
            ..Settings::default()
        };
        rig.store
            .add_version(&github, "forge: local -> github", 1)
            .await
            .unwrap();
        let id = rig.factory.submit("w", "Add modulo").await.unwrap();
        to_review(&rig, &id).await;
        rig.factory.apply(&id, Event::Approved).await.unwrap();
        for _ in 0..200 {
            if rig.store.load(&id).await.unwrap().job.pr.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let job = rig.store.load(&id).await.unwrap().job;
        assert_eq!(
            (job.pr.as_deref(), job.state),
            (Some(pr), State::Running),
            "the pull request is up and CI runs"
        );
        assert!(!rig.engine.called("worktree::land"), "nothing merges here");
        let created = rig
            .engine
            .calls
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.1 == "github::exec")
            .map(|c| c.2.clone())
            .unwrap();
        assert_eq!(
            (&created["args"][0], &created["args"][1]),
            (&json!("pr"), &json!("create"))
        );

        rig.factory.apply(&id, Event::CiPassed).await.unwrap();
        until(&rig, &id, Stage::Land, State::NeedsYou).await;
        rig.factory.apply(&id, Event::Approved).await.unwrap();
        until(&rig, &id, Stage::Land, State::Landed).await;
        let merged = rig
            .engine
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.1 == "github::exec" && c.2["args"][1] == "merge");
        assert!(merged, "approve at land merges the pull request");
    });
}
