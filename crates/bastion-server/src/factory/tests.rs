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
    machine: Arc<FakeMachine>,
}

fn rig() -> Rig {
    rig_on(engine())
}

fn rig_on(engine: FakeEngine) -> Rig {
    let engine = Arc::new(engine);
    let store = Arc::new(FakeStore::default());
    store.workspaces.lock().unwrap().push(Workspace {
        name: "w".into(),
        path: "/repo".into(),
        branch: "main".into(),
        checks: String::new(),
        max_jobs: None,
        revision_budget: None,
        branch_prefix: "bastion/".into(),
        setup: String::new(),
        cleanup: String::new(),
        env: String::new(),
    });
    let machine = Arc::new(FakeMachine::new().run_ok("git", ""));
    let factory = Factory::new(
        engine.clone() as Arc<dyn Engine>,
        store.clone() as Arc<dyn Store>,
        machine.clone(),
        Arc::new(|_| {}),
        "/scratch".into(),
    );
    Rig {
        factory,
        engine,
        store,
        machine,
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

/// Run a submitted job through its plan, the approval and its build, to
/// the build's wait for review.
async fn to_review(rig: &Rig, id: &str) {
    until(rig, id, Stage::Plan, State::Running).await;
    finish_turn(rig, id).await;
    until(rig, id, Stage::Plan, State::NeedsYou).await;
    rig.factory.apply(id, Event::Approved).await.unwrap();
    until(rig, id, Stage::Build, State::Running).await;
    finish_turn(rig, id).await;
    until(rig, id, Stage::Build, State::NeedsYou).await;
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
    *rig.store.dials.lock().unwrap() = bastion_core::Dials::all(bastion_core::Dial::Dark);
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
        assert!(commands.contains(&"git -C /work/repo tag -f bastion-base HEAD".to_string()));
        assert!(
            commands.contains(&"git -C /work/repo reset -q --hard bastion-before-turn".to_string())
        );
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
        assert_eq!(stages, [Stage::Prepare, Stage::Scope, Stage::Discover]);
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
        assert_eq!((job.stage, job.state), (Stage::Checks, State::NeedsYou));
        assert!(vm_commands(&rig).contains(&"sh -c cd /work/repo && pytest -q".to_string()));
    });
}

#[test]
fn with_the_local_forge_ship_brings_the_commits_back_and_prove_starts() {
    let rig = rig();
    crate::rt::block_on(async {
        let mut job = Job::new("j1".into(), "w".into(), "r".into(), 1).on(Pipeline::Full);
        (job.stage, job.state) = (Stage::Ship, State::Running);
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
        rig.factory.run_step(&record, Stage::Ship).await;
        until(&rig, "j1", Stage::Prove, State::Running).await;
        assert!(
            vm_commands(&rig)
                .iter()
                .any(|c| c.contains("bundle create /work/out.bundle"))
        );
        assert_eq!(rig.store.load("j1").await.unwrap().job.pr, None);
    });
}

/// A rig whose every VM dies at the first turn, with one job that keeps
/// its worktree.
fn dying_vms() -> Rig {
    let engine =
        Arc::new(engine().answer("approval::set-mode", Err("sandbox not found: vm1".into())));
    let store = Arc::new(FakeStore::default());
    store.workspaces.lock().unwrap().push(Workspace {
        name: "w".into(),
        path: "/repo".into(),
        branch: "main".into(),
        checks: String::new(),
        max_jobs: None,
        revision_budget: None,
        branch_prefix: "bastion/".into(),
        setup: String::new(),
        cleanup: String::new(),
        env: String::new(),
    });
    let machine = Arc::new(FakeMachine::new().run_ok("git", ""));
    let factory = Factory::new(
        engine.clone() as Arc<dyn Engine>,
        store.clone() as Arc<dyn Store>,
        machine.clone(),
        Arc::new(|_| {}),
        "/scratch".into(),
    );
    Rig {
        factory,
        engine,
        store,
        machine,
    }
}

/// How many VMs the rig has made.
fn vms_made(rig: &Rig) -> usize {
    let calls = rig.engine.calls.lock().unwrap();
    calls.iter().filter(|c| c.1 == "sandbox::create").count()
}

/// Wait until the job fails, after the rig has made at least `vms` VMs.
async fn failed_after(rig: &Rig, vms: usize) -> Record {
    for _ in 0..200 {
        let r = rig.store.load("j1").await.unwrap();
        if vms_made(rig) >= vms && r.job.state == State::Failed {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    rig.store.load("j1").await.unwrap()
}

/// Start the job on a VM that dies, and wait until it fails.
async fn lose_vms(rig: &Rig) -> Record {
    let mut job = Job::new("j1".into(), "w".into(), "r".into(), 1).on(Pipeline::Full);
    (job.stage, job.state) = (Stage::Implement, State::Provisioning);
    let record = Record {
        job,
        runtime: Runtime {
            worktree: Some("wt-kept".into()),
            sandbox: Some("vm1".into()),
            ..Runtime::default()
        },
        rev: 1,
        created_at: 1,
    };
    rig.store.insert(&record).await.unwrap();
    rig.factory.apply("j1", Event::Provisioned).await.unwrap();
    failed_after(rig, 0).await
}

#[test]
fn a_lost_vm_gets_a_new_one_on_the_same_worktree_and_one_that_keeps_dying_fails_the_job() {
    let rig = dying_vms();
    crate::rt::block_on(async {
        let r = lose_vms(&rig).await;
        assert_eq!(r.job.state, State::Failed, "{:?}", r.job.error);
        assert_eq!(r.runtime.worktree.as_deref(), Some("wt-kept"));
        let calls = rig.engine.calls.lock().unwrap();
        assert!(
            !calls.iter().any(|c| c.1 == "worktree::create"),
            "the worktree is kept"
        );
        drop(calls);
        assert_eq!(
            vms_made(&rig),
            bastion_core::MAX_REVIVES as usize,
            "a new VM per revive"
        );
    });
}

#[test]
fn a_failed_job_retried_gets_a_new_vm_on_the_same_worktree() {
    let rig = dying_vms();
    crate::rt::block_on(async {
        lose_vms(&rig).await;
        let before = vms_made(&rig);
        rig.factory
            .decide("j1", Event::Retried, "ada")
            .await
            .unwrap();
        let r = failed_after(&rig, before + 1).await;
        assert_eq!(vms_made(&rig), 2 * before + 1, "a new VM, then its revives");
        assert_eq!(r.runtime.worktree.as_deref(), Some("wt-kept"));
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
        until(&rig, &id, Stage::Plan, State::NeedsYou).await;
        let plan = rig.store.artifacts(&id).await.unwrap();
        assert_eq!(
            (plan[0].stage, plan[0].body.as_str()),
            (Stage::Plan, "1. Add /health")
        );

        rig.factory.apply(&id, Event::Approved).await.unwrap();
        until(&rig, &id, Stage::Build, State::Running).await;
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Build, State::NeedsYou).await;
        let artifacts = rig.store.artifacts(&id).await.unwrap();
        let diff = artifacts.iter().find(|a| a.stage == Stage::Review).unwrap();
        assert_eq!(diff.body, "No commits: the build left nothing to land.");

        rig.factory.apply(&id, Event::Approved).await.unwrap();
        until(&rig, &id, Stage::Land, State::NeedsYou).await;
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

/// The payload of each call to `function`, in order.
fn payloads(rig: &Rig, function: &str) -> Vec<serde_json::Value> {
    let calls = rig.engine.calls.lock().unwrap();
    calls
        .iter()
        .filter(|c| c.1 == function)
        .map(|c| c.2.clone())
        .collect()
}

#[test]
fn setup_runs_with_the_network_in_a_vm_of_its_own_and_cleanup_runs_at_the_end() {
    let rig = rig();
    {
        let mut all = rig.store.workspaces.lock().unwrap();
        all[0].setup = "pip install -e .".into();
        (all[0].cleanup, all[0].env) = ("make clean".into(), "CI=1".into());
    }
    crate::rt::block_on(async {
        let id = rig.factory.submit("w", "x").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        let vms = payloads(&rig, "sandbox::create");
        let nets: Vec<_> = vms.iter().map(|v| v["network"].clone()).collect();
        assert_eq!(
            nets,
            [json!(false), json!(true)],
            "the job's VM, then setup's"
        );
        assert!(vms.iter().all(|v| v["env"] == json!(["CI=1"])));
        assert_eq!(payloads(&rig, "sandbox::stop").len(), 1, "setup's VM stops");
        let ran = vm_commands(&rig);
        let at = |c: &str| ran.iter().position(|r| r.starts_with(c));
        let order = [
            at("git -C /work/repo checkout -q HEAD"),
            at("sh -c cd /work/repo && pip install -e ."),
            at("tar czf /work/repo.tgz"),
            at("tar xzf /work/repo.tgz -C /work"),
            at("git -C /work/repo checkout -q -f iii/wt1"),
        ];
        assert!(order.iter().all(Option::is_some), "{ran:#?}");
        assert!(order.is_sorted(), "{ran:#?}");
        rig.factory.apply(&id, Event::Stopped).await.unwrap();
        for _ in 0..200 {
            if vm_commands(&rig)
                .iter()
                .any(|r| r.ends_with("&& make clean"))
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("cleanup did not run: {:#?}", vm_commands(&rig));
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
        max_jobs: None,
        revision_budget: None,
        branch_prefix: "bastion/".into(),
        setup: String::new(),
        cleanup: String::new(),
        env: String::new(),
    });
    let machine = Arc::new(FakeMachine::new());
    let factory = Factory::new(
        engine.clone() as Arc<dyn Engine>,
        store.clone() as Arc<dyn Store>,
        machine.clone(),
        Arc::new(|_| {}),
        "/s".into(),
    );
    let rig = Rig {
        factory,
        engine,
        store,
        machine,
    };
    crate::rt::block_on(async {
        let id = rig.factory.submit("w", "x").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Failed).await;
        let job = rig.store.load(&id).await.unwrap().job;
        assert!(job.error.unwrap().contains("W503"));
    });
}

#[test]
fn a_dark_plan_goes_on_without_the_owner() {
    let rig = rig();
    rig.store
        .dials
        .lock()
        .unwrap()
        .set(Stage::Plan, bastion_core::Dial::Dark);
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
        max_jobs: None,
        revision_budget: None,
        branch_prefix: "bastion/".into(),
        setup: String::new(),
        cleanup: String::new(),
        env: String::new(),
    });
    let machine = Arc::new(FakeMachine::new().run_ok("git", "git@github.com:o/r.git\n"));
    let factory = Factory::new(
        engine.clone() as Arc<dyn Engine>,
        store.clone() as Arc<dyn Store>,
        machine.clone(),
        Arc::new(|_| {}),
        "/s".into(),
    );
    Rig {
        factory,
        engine,
        store,
        machine,
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
        until(&rig, &id, Stage::Land, State::NeedsYou).await;
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
        super::turn::usable(Stage::Specify, done("no json")),
        Outcome::Unusable { .. }
    ));
    let spec = r#"{"checks": [{"criterion": "C1", "run": "x", "files": ["t.py"]}]}"#;
    assert_eq!(super::turn::usable(Stage::Specify, done(spec)), done(spec));
    assert_eq!(
        super::turn::usable(Stage::Plan, done("no json")),
        done("no json")
    );
}

#[test]
fn a_scope_that_names_parts_keeps_each_later_part_as_a_job_that_waits() {
    let parts = "PARTS:\n1. Add the model\n2. Add the endpoint";
    let reply = json!({ "messages": [
        { "message": { "role": "assistant", "content": [{ "type": "text", "text": parts }] } },
    ]});
    let rig = rig_on(engine().answer("session::messages", Ok(reply)));
    *rig.store.dials.lock().unwrap() = bastion_core::Dials::all(bastion_core::Dial::Dark);
    crate::rt::block_on(async {
        let how = super::Submission {
            pipeline: Pipeline::Full,
            requester: Some("ana".into()),
            ..Default::default()
        };
        let id = rig
            .factory
            .submit_with("w", "Add a health check", how)
            .await
            .unwrap();
        until(&rig, &id, Stage::Scope, State::Running).await;
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Discover, State::Running).await;
        let first = rig.store.load(&id).await.unwrap();
        assert_eq!(first.job.part.as_deref(), Some("Add the model"));
        let second = rig.store.load(&format!("{id}-2")).await.unwrap();
        assert_eq!(
            (
                second.job.state,
                second.job.after.as_deref(),
                second.job.part.as_deref()
            ),
            (State::Queued, Some(id.as_str()), Some("Add the endpoint"))
        );
        assert_eq!(second.runtime.requester.as_deref(), Some("ana"));
    });
}

#[test]
fn a_turn_that_ends_twice_counts_once_and_a_stale_one_changes_nothing() {
    let rig = rig();
    crate::rt::block_on(async {
        let id = rig.factory.submit("w", "x").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        // The turn's event and its cap (or a resume) end the same turn.
        let plan = loop {
            let record = rig.store.load(&id).await.unwrap();
            if let Some(s) = record.runtime.session.filter(|s| s.contains("-plan-")) {
                break s;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let plan = super::turn::Turn {
            job: id.clone(),
            session: plan,
        };
        let (a, b) = (rig.factory.clone(), rig.factory.clone());
        let (t1, t2) = (plan.clone(), plan.clone());
        let first = tokio::spawn(async move { a.turn_done(&t1, false).await });
        let second = tokio::spawn(async move { b.turn_done(&t2, false).await });
        let _ = (first.await, second.await);
        until(&rig, &id, Stage::Plan, State::NeedsYou).await;
        let plans = |all: &[(String, crate::store::Artifact)]| {
            all.iter()
                .filter(|(j, a)| *j == id && a.stage == Stage::Plan)
                .count()
        };
        assert_eq!(
            plans(&rig.store.artifacts.lock().unwrap()),
            1,
            "one plan artifact"
        );
        rig.factory.apply(&id, Event::Approved).await.unwrap();
        until(&rig, &id, Stage::Build, State::Running).await;
        // The plan's session, found again later, does not finish the build.
        rig.factory.turn_done(&plan, false).await;
        let job = rig.store.load(&id).await.unwrap().job;
        assert_eq!((job.stage, job.state), (Stage::Build, State::Running));
    });
}

#[test]
fn a_turn_reads_the_charter_from_the_base_commit_not_the_worktree() {
    let rig = rig();
    crate::rt::block_on(async {
        let id = rig.factory.submit("w", "x").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        // The merge base is empty in the fake, so the base is HEAD.
        for name in ["CLAUDE.md", "AGENTS.md", ".claude/settings.json"] {
            assert!(
                rig.machine.called(&format!("git show HEAD:{name}")),
                "{name}"
            );
        }
    });
}

#[test]
fn approved_tests_stay_protected_after_a_restart() {
    let rig = rig();
    crate::rt::block_on(async {
        let id = rig.factory.submit("w", "x").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        // As a baseline before a restart left it: stored, not in the gate.
        let files = vec!["tests/test_c1.py".to_string()];
        rig.factory
            .set_runtime(&id, |r| r.approved = files.clone())
            .await
            .unwrap();
        assert!(rig.factory.gate.protected(&id).is_empty());
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Plan, State::NeedsYou).await;
        rig.factory.apply(&id, Event::Approved).await.unwrap();
        until(&rig, &id, Stage::Build, State::Running).await;
        for _ in 0..200 {
            let session = rig.store.load(&id).await.unwrap().runtime.session;
            if session.is_some_and(|s| s.contains("-build-")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(rig.factory.gate.protected(&id), files);
    });
}

#[test]
fn stopping_a_job_cancels_its_turn() {
    let rig = rig();
    crate::rt::block_on(async {
        let id = rig.factory.submit("w", "x").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        let session = loop {
            if let Some(s) = rig.store.load(&id).await.unwrap().runtime.session {
                break s;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        rig.factory.apply(&id, Event::Stopped).await.unwrap();
        until(&rig, &id, Stage::Plan, State::Stopped).await;
        for _ in 0..200 {
            let stop = |c: &(String, String, serde_json::Value)| {
                c.1 == "harness::stop" && c.2["session_id"] == session
            };
            if rig.engine.calls.lock().unwrap().iter().any(stop) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no harness::stop for {session}");
    });
}

#[test]
fn an_event_reports_the_job_it_moved_and_none_when_it_moved_nothing() {
    let rig = rig();
    crate::rt::block_on(async {
        let id = rig.factory.submit("w", "x").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        let always = || Arc::new(|_: &Record| true);
        let early = rig.factory.apply_when(&id, Event::Approved, always());
        assert_eq!(early.await.unwrap(), None, "an approval before the plan");
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Plan, State::NeedsYou).await;
        let moved = rig.factory.apply_when(&id, Event::Approved, always());
        let before = moved.await.unwrap().expect("the approval moved the job");
        assert_eq!((before.stage, before.state), (Stage::Plan, State::NeedsYou));
    });
}

#[test]
fn a_turn_on_a_job_whose_vm_went_while_it_waited_gets_a_new_vm_first() {
    let lost = Err("sandbox not found: vm-gone".into());
    let rig = rig_on(engine().answer("sandbox::exec", lost));
    crate::rt::block_on(async {
        let mut job = Job::new("j1".into(), "w".into(), "r".into(), 1);
        (job.stage, job.state) = (Stage::Build, State::NeedsYou);
        let record = Record {
            job,
            runtime: Runtime {
                worktree: Some("wt1".into()),
                sandbox: Some("vm-gone".into()),
                ..Runtime::default()
            },
            rev: 1,
            created_at: 1,
        };
        rig.store.insert(&record).await.unwrap();
        let sent_back = Event::Commented {
            text: "again".into(),
        };
        rig.factory.apply("j1", sent_back).await.unwrap();
        for _ in 0..200 {
            if rig
                .engine
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.1 == "sandbox::create")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let calls = rig.engine.calls.lock().unwrap();
        assert!(calls.iter().any(|c| c.1 == "sandbox::create"), "a new VM");
        assert!(
            !calls.iter().any(|c| c.1 == "harness::send"),
            "no turn on the VM that went"
        );
    });
}

/// A job at a running plan turn whose step began at `at`.
async fn planning(rig: &Rig, id: &str, at: u64) -> super::turn::Turn {
    let mut job = Job::new(id.into(), "w".into(), "r".into(), at);
    job.state = State::Running;
    let session = format!("{id}-plan-abc");
    let record = Record {
        job,
        runtime: Runtime {
            session: Some(session.clone()),
            ..Runtime::default()
        },
        rev: 1,
        created_at: at,
    };
    rig.store.insert(&record).await.unwrap();
    super::turn::Turn {
        job: id.into(),
        session,
    }
}

#[test]
fn a_cap_that_runs_out_after_an_extend_waits_out_the_new_one() {
    let rig = rig();
    crate::rt::block_on(async {
        let extended = planning(&rig, "j1", super::now()).await;
        let late = planning(&rig, "j2", 1).await;
        rig.factory.arm_cap(&extended, 0);
        rig.factory.arm_cap(&late, 0);
        for _ in 0..200 {
            if rig.store.load("j2").await.unwrap().job.failures > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let timed_out = rig.store.load("j2").await.unwrap().job;
        assert!(timed_out.failures > 0, "the late turn ran past its cap");
        let job = rig.store.load("j1").await.unwrap().job;
        assert_eq!((job.state, job.failures), (State::Running, 0));
    });
}
