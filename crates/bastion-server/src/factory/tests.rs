//! The short pipeline end to end, on the fake engine, store and machine.

use std::sync::Arc;
use std::time::Duration;

use bastion_core::{Event, Job, Pipeline, Stage, State};
use bastion_protocol::Link;
use serde_json::json;

use super::Factory;
use crate::engine::Engine;
use crate::engine::fake::FakeEngine;
use crate::machine::fake::FakeMachine;
use crate::store::fake::FakeStore;
use crate::store::{Artifact, Record, Runtime, Store, Workspace};

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
        checks: String::new(),
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

/// The `argv` of each command the factory ran in the VM.
fn vm_commands(rig: &Rig) -> Vec<String> {
    let calls = rig.engine.calls.lock().unwrap();
    calls
        .iter()
        .filter(|c| c.1 == "sandbox::exec")
        .map(|c| {
            let argv: Vec<&str> = c.2["argv"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap())
                .collect();
            argv.join(" ")
        })
        .collect()
}

#[test]
fn a_full_request_prepares_and_its_read_only_turns_put_the_tree_back_to_a_tag() {
    let rig = rig();
    crate::rt::block_on(async {
        let id = rig
            .factory
            .submit_with(
                "w",
                "Add a health check",
                super::Submission {
                    pipeline: Pipeline::Full,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        until(&rig, &id, Stage::Scope, State::Running).await;
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Discover, State::Running).await;
        let commands = vm_commands(&rig);
        assert!(commands.contains(&"git -C /work/repo tag -f bastion-before-turn".to_string()));
        assert!(commands.contains(&"git -C /work/repo tag -f bastion-base".to_string()));
        assert!(
            commands.contains(&"git -C /work/repo reset -q --hard bastion-before-turn".to_string())
        );
        // The fake's reply has no TASKS: or QUESTIONS:, so there is nothing to clarify.
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Plan, State::Running).await;
        let stages: Vec<Stage> = rig
            .store
            .artifacts(&id)
            .await
            .unwrap()
            .iter()
            .map(|a| a.stage)
            .collect();
        assert_eq!(stages, [Stage::Scope, Stage::Discover]);
    });
}

#[test]
fn checks_that_pass_before_the_build_go_to_falsify_and_one_that_stays_green_goes_back_to_specify() {
    let rig = rig();
    crate::rt::block_on(async {
        let mut job =
            Job::new("j1".into(), "w".into(), "Add a health check".into(), 1).on(Pipeline::Full);
        (job.stage, job.state) = (Stage::Baseline, State::Running);
        let record = Record {
            job,
            runtime: Runtime {
                worktree: Some("wt1".into()),
                sandbox: Some("vm1".into()),
                ..Runtime::default()
            },
            rev: 1,
            created_at: 1,
        };
        rig.store.insert(&record).await.unwrap();
        let spec = r#"{"checks": [{"criterion": "C1", "run": "pytest -k c1", "files": ["tests/test_c1.py"]}]}"#;
        let artifact = Artifact {
            stage: Stage::Specify,
            body: spec.into(),
            created_at: 1,
        };
        rig.store.add_artifact("j1", &artifact).await.unwrap();

        // The fake VM exits 0 for every command, so C1 passes before the build.
        rig.factory.run_step(&record, Stage::Baseline).await;
        until(&rig, "j1", Stage::Falsify, State::Running).await;
        finish_turn(&rig, "j1").await;
        // And it passes against the broken code too, so it checks nothing.
        until(&rig, "j1", Stage::Specify, State::Running).await;
        let job = rig.store.load("j1").await.unwrap().job;
        assert!(job.feedback.unwrap().contains("C1 stayed green"));
        let commands = vm_commands(&rig);
        for expected in [
            "sh -c cd /work/repo && pytest -k c1",
            "git -C /work/repo tag -f bastion-before-turn",
            "sh -c cd /work/repo && git reset -q --hard bastion-before-turn && git clean -qfd",
        ] {
            assert!(commands.iter().any(|c| c == expected), "{expected}");
        }
    });
}

#[test]
fn the_checks_step_runs_the_workspaces_command_and_passes_on_exit_0() {
    let rig = rig();
    rig.store.workspaces.lock().unwrap()[0].checks = "pytest -q".into();
    crate::rt::block_on(async {
        let mut job = Job::new("j1".into(), "w".into(), "r".into(), 1).on(Pipeline::Full);
        (job.stage, job.state) = (Stage::Checks, State::Running);
        let record = Record {
            job,
            runtime: Runtime {
                sandbox: Some("vm1".into()),
                ..Runtime::default()
            },
            rev: 1,
            created_at: 1,
        };
        rig.store.insert(&record).await.unwrap();
        rig.factory.run_step(&record, Stage::Checks).await;
        let job = rig.store.load("j1").await.unwrap().job;
        assert_eq!(job.stage, Stage::Verify);
        assert!(vm_commands(&rig).contains(&"sh -c cd /work/repo && pytest -q".to_string()));
    });
}

#[test]
fn a_request_runs_the_short_pipeline_to_landed() {
    let rig = rig();
    crate::rt::block_on(async {
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
        checks: String::new(),
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
    crate::rt::block_on(async {
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
    crate::rt::block_on(async {
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
    crate::rt::block_on(async {
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
        checks: String::new(),
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
    crate::rt::block_on(async {
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

#[test]
fn a_specify_reply_that_is_not_a_spec_is_unusable_and_other_replies_pass_through() {
    use bastion_core::Outcome;
    let done = |r: &str| Outcome::Done { report: r.into() };
    assert!(matches!(
        super::usable(Stage::Specify, done("no json")),
        Outcome::Unusable { .. }
    ));
    let spec = r#"{"checks": [{"criterion": "C1", "run": "x", "files": ["t.py"]}]}"#;
    assert_eq!(super::usable(Stage::Specify, done(spec)), done(spec));
    assert_eq!(super::usable(Stage::Plan, done("no json")), done("no json"));
}
