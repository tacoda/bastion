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
    rig_with(engine, FakeMachine::new().run_ok("git", ""))
}

/// A rig on `engine` and `machine`, with one workspace `w` on `/repo`.
fn rig_with(engine: FakeEngine, machine: FakeMachine) -> Rig {
    let (engine, machine) = (Arc::new(engine), Arc::new(machine));
    let store = Arc::new(FakeStore::default());
    store.workspaces.lock().unwrap().push(workspace());
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

/// The workspace every rig has: `w` on `/repo`, branch `main`.
fn workspace() -> Workspace {
    Workspace {
        name: "w".into(),
        path: "/repo".into(),
        branch: "main".into(),
        branch_prefix: "bastion/".into(),
        ..Workspace::default()
    }
}

/// Ask `ready` every 10 ms for up to two seconds; whether it said yes.
async fn eventually(mut ready: impl AsyncFnMut() -> bool) -> bool {
    for _ in 0..200 {
        if ready().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// The job `id` as the store holds it now.
async fn job_of(rig: &Rig, id: &str) -> Job {
    rig.store.load(id).await.unwrap().job
}

/// Job `j1` on `pipeline` at `stage` and `state`, on worktree `wt1` and VM `vm1`.
fn record_at(pipeline: Pipeline, stage: Stage, state: State) -> Record {
    let mut job = Job::new("j1".into(), "w".into(), "r".into(), 1).on(pipeline);
    (job.stage, job.state) = (stage, state);
    let runtime = Runtime {
        worktree: Some("wt1".into()),
        sandbox: Some("vm1".into()),
        ..Runtime::default()
    };
    Record {
        job,
        runtime,
        rev: 1,
        created_at: 1,
    }
}

/// Wait until the job reaches `stage` and `state`, or fail after two seconds.
async fn until(rig: &Rig, id: &str, stage: Stage, state: State) {
    let at = async || {
        let j = job_of(rig, id).await;
        (j.stage, j.state) == (stage, state)
    };
    if eventually(at).await {
        return;
    }
    let job = job_of(rig, id).await;
    panic!(
        "job is at {:?} {:?}, not {stage:?} {state:?}: {:?}",
        job.stage, job.state, job.error
    );
}

/// Fire the turn-completed binding for the job's current session.
async fn finish_turn(rig: &Rig, id: &str) {
    // The turn saves its session before it binds, so read it again each try.
    let fired = async || {
        let r = rig.store.load(id).await.unwrap();
        let session = r.runtime.session.unwrap_or_default();
        let done = json!({ "session_id": session });
        let fired = rig
            .engine
            .fire("harness::turn-completed", "session_id", done);
        !fired.is_empty()
    };
    assert!(
        eventually(fired).await,
        "no turn-completed binding for job {id}"
    );
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
        let mut record = record_at(Pipeline::Full, Stage::Baseline, State::Running);
        record.job.request = "Add a health check".into();
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
        let job = job_of(&rig, "j1").await;
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
        let record = record_at(Pipeline::Full, Stage::Checks, State::Running);
        rig.store.insert(&record).await.unwrap();
        rig.factory.run_step(&record, Stage::Checks).await;
        let job = job_of(&rig, "j1").await;
        assert_eq!((job.stage, job.state), (Stage::Checks, State::NeedsYou));
        assert!(vm_commands(&rig).contains(&"sh -c cd /work/repo && pytest -q".to_string()));
    });
}

#[test]
fn with_the_local_forge_ship_brings_the_commits_back_and_prove_starts() {
    let rig = rig();
    crate::rt::block_on(async {
        let record = record_at(Pipeline::Full, Stage::Ship, State::Running);
        rig.store.insert(&record).await.unwrap();
        rig.factory.run_step(&record, Stage::Ship).await;
        until(&rig, "j1", Stage::Prove, State::Running).await;
        assert!(
            vm_commands(&rig)
                .iter()
                .any(|c| c.contains("bundle create /work/out.bundle"))
        );
        assert_eq!(job_of(&rig, "j1").await.pr, None);
    });
}

/// A rig whose every VM dies at the first turn, with one job that keeps
/// its worktree.
fn dying_vms() -> Rig {
    rig_on(engine().answer("approval::set-mode", Err("sandbox not found: vm1".into())))
}

/// How many VMs the rig has made.
fn vms_made(rig: &Rig) -> usize {
    let calls = rig.engine.calls.lock().unwrap();
    calls.iter().filter(|c| c.1 == "sandbox::create").count()
}

/// Wait until the job fails, after the rig has made at least `vms` VMs.
async fn failed_after(rig: &Rig, vms: usize) -> Record {
    let failed = async || vms_made(rig) >= vms && job_of(rig, "j1").await.state == State::Failed;
    eventually(failed).await;
    rig.store.load("j1").await.unwrap()
}

/// Start the job on a VM that dies, and wait until it fails.
async fn lose_vms(rig: &Rig) -> Record {
    let mut record = record_at(Pipeline::Full, Stage::Implement, State::Provisioning);
    record.runtime.worktree = Some("wt-kept".into());
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
        let landed = json!({ "worktree_id": "wt1" });
        let fire = async || {
            !rig.engine
                .fire("worktree::landed", "worktree_id", landed.clone())
                .is_empty()
        };
        eventually(fire).await;
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
        let stopped = async || rig.engine.called("sandbox::stop");
        assert!(eventually(stopped).await, "the VM was never stopped");
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
        let cleaned = async || {
            vm_commands(&rig)
                .iter()
                .any(|r| r.ends_with("&& make clean"))
        };
        let ran = eventually(cleaned).await;
        assert!(ran, "cleanup did not run: {:#?}", vm_commands(&rig));
    });
}

#[test]
fn a_failed_provision_fails_the_job_with_its_reason() {
    let failing = engine().answer(
        "worktree::create",
        Err("W503 repository not in gates.repos".into()),
    );
    let rig = rig_with(failing, FakeMachine::new());
    crate::rt::block_on(async {
        let id = rig.factory.submit("w", "x").await.unwrap();
        until(&rig, &id, Stage::Plan, State::Failed).await;
        let job = job_of(&rig, &id).await;
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
            contract: None,
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

        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Build, State::Running).await;
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Review, State::Stopped).await;
        let artifacts = rig.store.artifacts(&id).await.unwrap();
        assert!(artifacts.iter().any(|a| a.stage == Stage::Review));
        assert!(!rig.engine.called("worktree::land"), "a trial never lands");
        assert!(
            rig.engine.called("harness::metrics"),
            "each turn is metered"
        );
        let checked = contract_before_stop(&rig);
        assert!(
            checked,
            "the contract is checked in the trial's VM before it stops"
        );
    });
}

/// Whether the contract's `verify.yaml` was read in the VM before it stopped.
fn contract_before_stop(rig: &Rig) -> bool {
    let calls = rig.engine.calls.lock().unwrap();
    let at = |name: &str, holds: &str| {
        (calls.iter()).position(|c| c.1 == name && c.2.to_string().contains(holds))
    };
    matches!((at("sandbox::exec", "verify.yaml"), at("sandbox::stop", "")), (Some(c), Some(s)) if c < s)
}

/// A rig whose workspace's origin is on GitHub, where every gh call
/// answers with the pull request's URL.
fn github_rig(pr: &str) -> Rig {
    let engine = engine()
        .answer(
            "sandbox::exec",
            Ok(json!({ "exit_code": 0, "stdout": "+x" })),
        )
        .answer(
            "github::exec",
            Ok(json!({ "exit_code": 0, "stdout": format!("{pr}\n") })),
        );
    // The repository has a verify.yaml, so the push waits on a verdict;
    // the verify package is already on the host.
    let machine = FakeMachine::new()
        .run_ok("git", "git@github.com:o/r.git\n")
        .file("/s/opengantry-verify-4.0.2/kit.tgz", "kit");
    rig_with(engine, machine)
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
        eventually(async || job_of(&rig, &id).await.pr.is_some()).await;
        let job = job_of(&rig, &id).await;
        assert_eq!(
            (job.pr.as_deref(), job.state),
            (Some(pr), State::Running),
            "the pull request is up and CI runs"
        );
        assert!(!rig.engine.called("worktree::land"), "nothing merges here");
        let calls = rig.engine.calls.lock().unwrap().clone();
        let judge = calls
            .iter()
            .find(|c| c.1 == "sandbox::create" && c.2["image"] == "node")
            .expect("the verdict ran in a VM of its own");
        assert_eq!(judge.2["network"], false, "the verdict's VM is offline");
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
        let kept = rig.store.artifacts(&id).await.unwrap();
        let merge = |a: &&Artifact| a.stage == Stage::Merge && a.body.contains("pull/1");
        assert!(kept.iter().any(|a| merge(&a)), "the merge commit is kept");
        let calls = rig.engine.calls.lock().unwrap();
        let gh: Vec<_> = calls.iter().filter(|c| c.1 == "github::exec").collect();
        let at = |verb: &str| gh.iter().position(|c| c.2["args"][1] == verb);
        assert!(
            at("merge").is_some(),
            "approve at land merges the pull request"
        );
        let edited = at("edit").is_some_and(|e| Some(e) < at("merge"));
        assert!(edited, "the body is said again before the merge");
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
        let job = job_of(&rig, &id).await;
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
        let building = async || {
            let session = rig.store.load(&id).await.unwrap().runtime.session;
            session.is_some_and(|s| s.contains("-build-"))
        };
        eventually(building).await;
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
        let stop = |c: &(String, String, serde_json::Value)| {
            c.1 == "harness::stop" && c.2["session_id"] == session
        };
        let stopped = async || rig.engine.calls.lock().unwrap().iter().any(stop);
        assert!(eventually(stopped).await, "no harness::stop for {session}");
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
        let mut record = record_at(Pipeline::Short, Stage::Build, State::NeedsYou);
        record.runtime.sandbox = Some("vm-gone".into());
        rig.store.insert(&record).await.unwrap();
        let sent_back = Event::Commented {
            text: "again".into(),
        };
        rig.factory.apply("j1", sent_back).await.unwrap();
        eventually(async || rig.engine.called("sandbox::create")).await;
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
        eventually(async || job_of(&rig, "j2").await.failures > 0).await;
        let timed_out = job_of(&rig, "j2").await;
        assert!(timed_out.failures > 0, "the late turn ran past its cap");
        let job = job_of(&rig, "j1").await;
        assert_eq!((job.state, job.failures), (State::Running, 0));
    });
}

#[test]
fn a_revived_vm_with_no_new_commits_lands_what_the_worktree_holds() {
    let empty = Err("exited 128: \"fatal: Refusing to create empty bundle.\\n\"".into());
    let rig = rig_on(engine().answer("sandbox::exec", empty));
    let record = Record {
        job: Job::new("j1".into(), "w".into(), "r".into(), 1),
        runtime: Runtime {
            worktree: Some("wt1".into()),
            sandbox: Some("vm1".into()),
            ..Runtime::default()
        },
        rev: 1,
        created_at: 1,
    };
    crate::rt::block_on(async {
        let (path, target) = rig.factory.bring_back(&record).await.unwrap();
        assert_eq!((path.to_str(), target.as_str()), (Some("/wt"), "main"));
        assert!(!rig.engine.called("sandbox::fs::read"));
    });
}

#[test]
fn a_turn_the_harness_lost_starts_again_on_resume() {
    let lost = Err("handler error: session/not_found: session j1-build-old does not exist".into());
    let rig = rig_on(engine().answer("harness::status", lost));
    let mut job = Job::new("j1".into(), "w".into(), "r".into(), super::now());
    (job.stage, job.state) = (Stage::Build, State::Running);
    let record = Record {
        job,
        runtime: Runtime {
            worktree: Some("wt1".into()),
            sandbox: Some("vm1".into()),
            session: Some("j1-build-old".into()),
            ..Runtime::default()
        },
        rev: 1,
        created_at: 1,
    };
    crate::rt::block_on(async {
        rig.store.insert(&record).await.unwrap();
        rig.factory.clone().resume().await;
        let again = async || {
            let session = rig.store.load("j1").await.unwrap().runtime.session;
            session.as_deref() != Some("j1-build-old") && rig.engine.called("harness::send")
        };
        assert!(eventually(again).await, "the lost turn did not start again");
    });
}

#[test]
fn a_preview_a_host_deployed_for_the_branch_is_kept_for_reviewers() {
    let deployed =
        json!({ "id": 1, "state": "success", "environment_url": "https://pr-1.example.dev" });
    let gh = json!({ "exit_code": 0, "stdout": json!([deployed]).to_string() });
    let rig = rig_on(engine().answer("github::exec", Ok(gh)));
    crate::rt::block_on(async {
        let record = record_at(Pipeline::Full, Stage::Ci, State::Running);
        rig.store.insert(&record).await.unwrap();
        let hub = crate::forge::GitHub {
            engine: &*rig.engine,
            slug: "o/r".into(),
        };
        rig.factory.keep_preview(&record, &hub).await;
        let kept = rig.store.artifacts("j1").await.unwrap();
        assert_eq!(kept[0].body, "Preview: <https://pr-1.example.dev>");
        let calls = rig.engine.calls.lock().unwrap();
        let listed = &calls.iter().find(|c| c.1 == "github::exec").unwrap().2["args"][1];
        assert_eq!(listed, "repos/o/r/deployments?ref=bastion/j1&per_page=1");
    });
}

#[test]
fn a_revert_of_a_landed_commit_is_an_escape_of_its_job_once() {
    use crate::govern_store::GovernStore;
    let log = "r9\u{1f}Revert it\n\nThis reverts commit a1b2c3d4e5f6.\n\u{1e}";
    let key = "ab".repeat(32);
    let machine = FakeMachine::new()
        .run_ok("git", log)
        .run_ok("security", &key);
    let rig = rig_with(engine(), machine);
    crate::rt::block_on(async {
        rig.store
            .insert(&record_at(Pipeline::Short, Stage::Land, State::Landed))
            .await
            .unwrap();
        let commits = bastion_core::escape::commits_artifact("a1b2c3d4e5f6 Add cube").unwrap();
        let kept = Artifact {
            stage: Stage::Land,
            body: commits,
            created_at: 1,
        };
        rig.store.add_artifact("j1", &kept).await.unwrap();
        let escapes = async || rig.store.entries_of(crate::factory::ESCAPE).await.unwrap();
        rig.factory.sweep_reverts().await.unwrap();
        assert!(
            eventually(async || !escapes().await.is_empty()).await,
            "no escape kept"
        );
        rig.factory.sweep_reverts().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let all = escapes().await;
        assert_eq!(all.len(), 1, "a revert is kept once");
        let (job, detail) = (&all[0].record.subject, &all[0].record.detail);
        assert_eq!(
            (job.as_str(), detail.as_str()),
            ("j1", "a1b2c3d4e5f6 reverted by r9")
        );
    });
}

#[test]
fn a_paused_intake_admits_nothing_until_it_opens() {
    use crate::settings_store::SettingsStore;
    let rig = rig();
    crate::rt::block_on(async {
        let paused = bastion_core::settings::Settings {
            intake_paused: true,
            ..Default::default()
        };
        rig.store.add_version(&paused, "paused", 1).await.unwrap();
        let id = rig.factory.submit("w", "x").await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            job_of(&rig, &id).await.state,
            State::Queued,
            "held while paused"
        );
        rig.store
            .add_version(&Default::default(), "open", 2)
            .await
            .unwrap();
        rig.factory.pump();
        until(&rig, &id, Stage::Plan, State::Running).await;
    });
}

#[test]
fn a_job_that_names_a_model_runs_its_turns_on_it() {
    use super::Submission;
    let rig = rig();
    crate::rt::block_on(async {
        let how = |model: &str| Submission {
            model: Some(model.into()),
            ..Submission::default()
        };
        let refused = rig.factory.submit_with("w", "x", how("no-slash")).await;
        assert!(refused.unwrap_err().contains("provider/model"));
        let id = rig
            .factory
            .submit_with("w", "x", how("deepseek/deepseek-flash"))
            .await
            .unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        let sent = async || rig.engine.called("harness::send");
        assert!(eventually(sent).await, "no turn started");
        let calls = rig.engine.calls.lock().unwrap();
        let send = calls.iter().find(|c| c.1 == "harness::send").unwrap();
        assert_eq!(
            (&send.2["provider"], &send.2["model"]),
            (&json!("deepseek"), &json!("deepseek-flash"))
        );
    });
}
