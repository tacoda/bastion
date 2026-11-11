//! The SQL against this machine's `database` worker: `cargo test -p bastion-server -- --ignored --nocapture live_store`.

use super::*;
use crate::engine::IiiEngine;
use bastion_core::govern::Call;
use bastion_core::{Job, Stage};
use bastion_protocol::Link;
use serde_json::json;
use std::sync::Arc;

/// The store on this machine's stack, with one workspace and one job
/// named `tag`.
async fn open_with_job(tag: &str) -> (SqlStore, Record) {
    let engine: Arc<dyn Engine> = Arc::new(IiiEngine::connect("ws://127.0.0.1:49160"));
    for _ in 0..150 {
        if engine.link() == Link::Up {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(engine.link(), Link::Up, "no engine link in 15 s");
    let store = SqlStore::open(engine, SEED).await.unwrap();
    let w = Workspace {
        name: tag.into(),
        path: format!("/tmp/{tag}"),
        branch: "main".into(),
        checks: String::new(),
        max_jobs: None,
        revision_budget: None,
        branch_prefix: "bastion/".into(),
        setup: String::new(),
        cleanup: String::new(),
        env: String::new(),
        gates: String::new(),
        autofix: String::new(),
        image: "python".into(),
        finish: "merge".into(),
    };
    store.add_workspace(&w).await.unwrap();
    let r = Record {
        job: Job::new(tag.into(), tag.into(), "check the store".into(), 7),
        runtime: Runtime::default(),
        rev: 1,
        created_at: 7,
    };
    store.insert(&r).await.unwrap();
    (store, r)
}

#[test]
#[ignore = "needs this machine's stack"]
fn live_store() {
    crate::rt::block_on(async {
        let tag = format!("live-{}", std::process::id());
        let (store, mut r) = open_with_job(&tag).await;
        r.job.state = bastion_core::State::Running;
        r.runtime.sandbox = Some("vm".into());
        let rev = store.save(&r).await.unwrap();
        let stale = store.save(&r).await;
        store
            .add_artifact(
                &tag,
                &Artifact {
                    stage: Stage::Agent,
                    step: "build".into(),
                    body: "1. x".into(),
                    created_at: 8,
                },
            )
            .await
            .unwrap();
        let back = store.load(&tag).await.unwrap();
        let artifacts = store.artifacts(&tag).await.unwrap();
        let bad = store
            .execute(
                "UPDATE jobs SET state = 'nonsense' WHERE id = ?",
                json!([tag]),
            )
            .await;
        for sql in [
            "DELETE FROM artifacts WHERE job = ?",
            "DELETE FROM jobs WHERE id = ?",
            "DELETE FROM workspaces WHERE name = ?",
        ] {
            store.execute(sql, json!([tag])).await.unwrap();
        }
        println!(
            "rev {rev}, stale {stale:?}, state {:?}, sandbox {:?}, artifacts {}, bad state {bad:?}",
            back.job.state,
            back.runtime.sandbox,
            artifacts.len()
        );
        assert_eq!(rev, 2);
        assert_eq!(stale, Err(STALE.into()));
        assert_eq!(
            (back.rev, back.job.state, back.runtime.sandbox.as_deref()),
            (2, bastion_core::State::Running, Some("vm"))
        );
        assert_eq!(artifacts[0].body, "1. x");
        assert!(
            bad.is_err(),
            "the CHECK constraint refuses an unknown state"
        );
    });
}

#[test]
#[ignore = "needs this machine's stack"]
fn live_workspace_edit() {
    crate::rt::block_on(async {
        let tag = format!("live-ws-{}", std::process::id());
        let (store, _) = open_with_job(&tag).await;
        let mut w = store.workspaces().await.unwrap();
        w.retain(|w| w.name == tag);
        let mut edited = w.remove(0);
        (edited.checks, edited.max_jobs) = ("make test".into(), Some(2));
        edited.revision_budget = Some(4);
        edited.branch_prefix = "ian/".into();
        (edited.setup, edited.env) = ("make deps".into(), "CI=1".into());
        store.edit_workspace(&edited).await.unwrap();
        let back = store.workspaces().await.unwrap();
        let unknown = Workspace {
            name: format!("{tag}-none"),
            ..edited.clone()
        };
        let missing = store.edit_workspace(&unknown).await;
        let zero = Workspace {
            max_jobs: Some(0),
            ..edited.clone()
        };
        let refused = store.edit_workspace(&zero).await;
        for sql in [
            "DELETE FROM jobs WHERE id = ?",
            "DELETE FROM workspaces WHERE name = ?",
        ] {
            store.execute(sql, json!([tag])).await.unwrap();
        }
        assert!(back.contains(&edited));
        assert!(missing.is_err(), "no workspace by that name");
        assert!(refused.is_err(), "the CHECK refuses a cap of 0");
    });
}

#[test]
#[ignore = "needs this machine's stack"]
fn live_governance() {
    crate::rt::block_on(async {
        let tag = format!("live-gov-{}", std::process::id());
        let (store, _) = open_with_job(&tag).await;
        let rules = store.constraints().await.unwrap();
        let grants = store.capabilities().await.unwrap();
        let mut keep = rules.iter().find(|c| c.id == "keep-tests").unwrap().clone();
        let was = keep.rung;
        keep.rung = keep.rung.promote();
        store.set_constraint(&keep).await.unwrap();
        let promoted = store
            .constraints()
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.id == "keep-tests")
            .unwrap()
            .rung;
        keep.rung = was;
        store.set_constraint(&keep).await.unwrap();
        let held = HeldCall {
            id: format!("fc-{tag}"),
            job: tag.clone(),
            session: "s".into(),
            turn: "t".into(),
            call: Call {
                content: String::new(),
                function: "sandbox::fs::write".into(),
                text: "/work/repo/a.py".into(),
            },
            arguments: json!({ "path": "/work/repo/a.py" }),
            at: 9,
        };
        store.hold(&held).await.unwrap();
        let held_back = store.held(&tag).await.unwrap();
        assert!(
            store.claim(&held.id).await.unwrap(),
            "the first claim takes it"
        );
        assert!(
            !store.claim(&held.id).await.unwrap(),
            "a second claim finds it gone"
        );
        let released = store.held(&tag).await.unwrap();
        println!(
            "constraints {}, grants {}, promoted {promoted:?}, held {}",
            rules.len(),
            grants.len(),
            held_back.len()
        );
        assert!(rules.len() >= 5, "the seed file's five rules");
        assert!(grants.len() >= 3, "the seed file's three grants");
        assert_eq!(promoted, was.promote());
        assert_eq!(held_back, vec![held]);
        assert!(released.is_empty());
        for sql in [
            "DELETE FROM jobs WHERE id = ?",
            "DELETE FROM workspaces WHERE name = ?",
        ] {
            store.execute(sql, json!([tag])).await.unwrap();
        }
    });
}
