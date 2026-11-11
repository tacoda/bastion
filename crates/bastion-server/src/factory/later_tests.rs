//! The factory's later features end to end on the fake engine, store and
//! machine (F7 to F11), on the rig in `tests.rs`.

use std::time::Duration;

use bastion_core::{Stage, State};
use serde_json::json;

use super::tests::*;
use crate::machine::fake::FakeMachine;
use crate::store::{Artifact, Store};

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
        let mut landed = record_at("checks", State::Landed);
        landed.job.node = None;
        rig.store.insert(&landed).await.unwrap();
        let commits = bastion_core::escape::commits_artifact("a1b2c3d4e5f6 Add cube").unwrap();
        let kept = Artifact {
            stage: Stage::Shell,
            step: "build".into(),
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
        until(&rig, &id, Some("plan"), State::Running).await;
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
        until(&rig, &id, Some("plan"), State::Running).await;
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

#[test]
fn a_steps_own_model_wins_over_the_jobs() {
    use super::Submission;
    let rig = rig();
    crate::rt::block_on(async {
        let mut short = crate::workflow_store::find(&*rig.store, "short")
            .await
            .unwrap();
        short.graph.nodes[0].options.model = "anthropic/claude-sonnet-5-5".into();
        crate::workflow_store::save(&*rig.store, &short, 1)
            .await
            .unwrap();
        let how = Submission {
            model: Some("deepseek/deepseek-flash".into()),
            ..Submission::default()
        };
        let id = rig.factory.submit_with("w", "x", how).await.unwrap();
        until(&rig, &id, Some("plan"), State::Running).await;
        let sent = async || rig.engine.called("harness::send");
        assert!(eventually(sent).await, "no turn started");
        let calls = rig.engine.calls.lock().unwrap();
        let send = calls.iter().find(|c| c.1 == "harness::send").unwrap();
        assert_eq!(
            (&send.2["provider"], &send.2["model"]),
            (&json!("anthropic"), &json!("claude-sonnet-5-5"))
        );
        assert_eq!(
            send.2["options"]["metadata"]["step"], "short/plan",
            "the gate knows the step as workflow/step"
        );
    });
}

#[test]
fn a_requests_screenshots_are_kept_and_put_in_its_vm() {
    use super::Submission;
    let rig = rig();
    crate::rt::block_on(async {
        let how = Submission {
            screenshots: vec![("broken.png".into(), vec![1, 2, 3])],
            ..Submission::default()
        };
        let id = rig
            .factory
            .submit_with("w", "Fix the total", how)
            .await
            .unwrap();
        until(&rig, &id, Some("plan"), State::Running).await;
        let written = rig.machine.calls.lock().unwrap().clone();
        assert!(
            written.contains(&format!("write /s/requests/{id}/broken.png")),
            "{written:?}"
        );
        let job = job_of(&rig, &id).await;
        assert_eq!(job.results["screenshots"], "broken.png");
        let calls = rig.engine.calls.lock().unwrap();
        let put = |c: &&(String, String, serde_json::Value)| {
            c.1 == "sandbox::fs::write" && c.2["path"] == "/work/request/broken.png"
        };
        assert!(calls.iter().any(|c| put(&c)), "the screenshot is in the VM");
    });
}

#[test]
fn a_job_that_comes_to_wait_is_a_notice_with_the_count_waiting() {
    use bastion_protocol::Change;
    let rig = rig();
    crate::rt::block_on(async {
        let id = rig.factory.submit("w", "Add /health").await.unwrap();
        until(&rig, &id, Some("plan"), State::Running).await;
        finish_turn(&rig, &id).await;
        until(&rig, &id, Some("plan"), State::NeedsYou).await;
        let told = rig.told.lock().unwrap().clone();
        let notices: Vec<_> = told
            .iter()
            .filter(|c| matches!(c, Change::Notice { .. }))
            .collect();
        let Some(Change::Notice {
            job,
            state,
            text,
            waiting,
            ..
        }) = notices.first()
        else {
            panic!("no notice in {told:?}");
        };
        assert_eq!((job, state.as_str(), *waiting), (&id, "needs_you", 1));
        assert!(text.contains("waits for you at plan"), "{text}");
        assert_eq!(notices.len(), 1, "the job's other changes are not notices");
    });
}

#[test]
fn a_turns_screenshot_is_kept_once_with_the_job() {
    let listing = json!({ "entries": [{ "name": "home.png", "is_dir": false, "size": 3 }] });
    let mut engine = engine()
        .answer("sandbox::fs::ls", Ok(listing))
        .answer("sandbox::fs::read", Ok(json!({ "content": {} })));
    engine.channel_bytes = vec![1, 2, 3];
    let rig = rig_on(engine);
    crate::rt::block_on(async {
        let record = record_at("build", State::Running);
        rig.store.insert(&record).await.unwrap();
        rig.factory.keep_media(&record).await;
        rig.factory.keep_media(&record).await;
        let kept = rig.store.artifacts("j1").await.unwrap();
        let named: Vec<_> = kept
            .iter()
            .filter_map(|a| bastion_core::media::named(&a.body))
            .collect();
        assert_eq!(named, ["home.png"], "kept once");
        let written = rig.machine.calls.lock().unwrap().clone();
        assert!(
            written.contains(&"write /s/media/j1/home.png".to_string()),
            "{written:?}"
        );
    });
}

#[test]
fn a_new_vm_brings_loopback_up_and_a_browser_vm_finds_its_browsers() {
    let rig = rig();
    crate::rt::block_on(async {
        let ws = workspace();
        let vm = rig
            .factory
            .create_vm_from("browser", &ws, super::setup::Access::network(false))
            .await;
        assert_eq!(vm.as_deref(), Ok("vm1"));
        let calls = rig.engine.calls.lock().unwrap().clone();
        let made = calls.iter().find(|c| c.1 == "sandbox::create").unwrap();
        let env = made.2["env"].as_array().unwrap();
        assert!(env.contains(&json!("PLAYWRIGHT_BROWSERS_PATH=/ms-playwright")));
        let exec = calls.iter().find(|c| c.1 == "sandbox::exec").unwrap();
        assert_eq!(exec.2["argv"][0], "perl", "loopback comes up first");
        assert!(exec.2["argv"][2].as_str().unwrap().contains("0x8914"));
    });
}

#[test]
fn a_shell_step_gets_the_jobs_branch_and_the_vault_names_this_server_only_for_a_read() {
    use bastion_core::workflow::{Reach, Step};
    let rig = rig_on(
        engine()
            .answer(
                "secrets::get",
                Ok(json!({ "name": "GH_TOKEN", "consumers": ["llm-router"] })),
            )
            .answer("secrets::access", Ok(json!({})))
            .answer("secrets::resolve", Ok(json!({ "value": "t0k" }))),
    );
    crate::rt::block_on(async {
        let mut record = record_at("checks", State::Running);
        record.job.flow = vec![Step {
            id: "tests".into(),
            stage: Stage::Shell,
            run: "make test".into(),
            dial: bastion_core::Dial::Dark,
            note: String::new(),
            needs: Vec::new(),
            gives: Vec::new(),
            reach: Reach::default(),
            options: Default::default(),
        }];
        record.job.node = Some("tests".into());
        rig.factory.shell_step(&record).await;
        let calls = rig.engine.calls.lock().unwrap().clone();
        let ran = calls.iter().find(|c| c.1 == "sandbox::exec").unwrap();
        let line = ran.2["argv"][2].as_str().unwrap();
        assert!(
            line.contains("BASTION_BRANCH='iii/wt1' BASTION_BASE='main'"),
            "{line}"
        );
        assert!(line.ends_with("make test"));
        assert!(
            !calls.iter().any(|c| c.1 == "sandbox::create"),
            "the job's own VM"
        );

        let value = rig.factory.vault_value("GH_TOKEN").await;
        assert_eq!(value.as_deref(), Ok("t0k"));
        let calls = rig.engine.calls.lock().unwrap().clone();
        let access: Vec<&serde_json::Value> = calls
            .iter()
            .filter(|c| c.1 == "secrets::access")
            .map(|c| &c.2["consumers"])
            .collect();
        let me = crate::engine::worker_name();
        assert_eq!(access[0], &json!(["llm-router", me]));
        assert_eq!(access[1], &json!(["llm-router"]), "put back as it was");
    });
}
