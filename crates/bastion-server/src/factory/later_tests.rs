//! The factory's later features end to end on the fake engine, store and
//! machine (F7 to F11), on the rig in `tests.rs`.

use std::time::Duration;

use bastion_core::{Pipeline, Stage, State};
use serde_json::json;

use super::tests::*;
use crate::machine::fake::FakeMachine;
use crate::store::{Artifact, Store};

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

#[test]
fn a_requests_screenshots_are_kept_and_put_in_its_vm() {
    use super::Submission;
    let rig = rig();
    crate::rt::block_on(async {
        let how = Submission {
            screenshots: vec![("broken.png".into(), vec![1, 2, 3])],
            criteria: Some("2 + 2 is 4".into()),
            ..Submission::default()
        };
        let id = rig
            .factory
            .submit_with("w", "Fix the total", how)
            .await
            .unwrap();
        until(&rig, &id, Stage::Plan, State::Running).await;
        let written = rig.machine.calls.lock().unwrap().clone();
        assert!(
            written.contains(&format!("write /s/requests/{id}/broken.png")),
            "{written:?}"
        );
        let job = job_of(&rig, &id).await;
        assert_eq!(
            (job.results["screenshots"].as_str(), job.criteria.as_deref()),
            ("broken.png", Some("2 + 2 is 4"))
        );
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
        until(&rig, &id, Stage::Plan, State::Running).await;
        finish_turn(&rig, &id).await;
        until(&rig, &id, Stage::Plan, State::NeedsYou).await;
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
        let record = record_at(Pipeline::Short, Stage::Build, State::Running);
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
