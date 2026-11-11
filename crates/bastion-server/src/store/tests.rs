use super::fake::FakeStore;
use super::schema::*;
use super::sql::{self, *};
use super::*;
use crate::engine::fake::FakeEngine;
use crate::govern_store::held_call;
use bastion_core::Kind;
use bastion_core::audit;
use bastion_core::govern::Call;
use bastion_core::{Job, Stage, State};
use bastion_protocol::Link;
use serde_json::{Value, json};
use std::sync::Arc;

fn record() -> Record {
    Record {
        job: Job::new("j1".into(), "w".into(), "r".into(), 5),
        runtime: Runtime::default(),
        rev: 1,
        created_at: 5,
    }
}

#[test]
fn the_seed_grants_every_turn_stage_its_sandbox() {
    let turns = Stage::ALL.into_iter().filter(|s| s.kind() == Kind::Turn);
    for stage in turns {
        let row = format!("('{}', 'sandbox::*', 'allowed')", name(stage));
        assert!(SEED.contains(&row), "the seed grants nothing at {stage:?}");
    }
}

#[test]
fn a_stale_save_is_refused_and_a_current_one_bumps_the_rev() {
    let store = FakeStore::default();
    let r = record();
    crate::rt::block_on(async {
        store.insert(&r).await.unwrap();
        assert_eq!(store.save(&r).await, Ok(2));
        assert_eq!(
            store.save(&r).await,
            Err(STALE.into()),
            "rev 1 is stale now"
        );
    });
}

#[test]
fn an_update_that_matches_no_row_is_stale() {
    let engine = Arc::new(
        FakeEngine::new(Link::Up)
            .answer("database::transaction", Ok(json!({ "committed": true })))
            .answer(
                "database::query",
                Ok(json!({ "rows": [{ "user_version": 1 }] })),
            )
            .answer("database::execute", Ok(json!({ "returned_rows": [] }))),
    );
    crate::rt::block_on(async {
        let store = SqlStore::open(engine.clone(), SEED).await.unwrap();
        assert_eq!(store.save(&record()).await, Err(STALE.into()));
        let calls = engine.calls.lock().unwrap();
        let update = &calls.last().unwrap().2;
        assert!(
            update["sql"]
                .as_str()
                .unwrap()
                .contains("WHERE id = ? AND rev = ?")
        );
        let params = update["params"].as_array().unwrap();
        assert_eq!(params[params.len() - 2..], [json!("j1"), json!(1)]);
    });
}

#[test]
fn a_second_entry_with_the_same_seq_is_refused() {
    let store = FakeStore::default();
    let what = audit::Record {
        kind: "gate".into(),
        subject: "j1".into(),
        detail: "continue".into(),
    };
    let first = audit::append(None, 1, what, b"k");
    crate::rt::block_on(async {
        store.append_audit(&first).await.unwrap();
        assert!(store.append_audit(&first).await.is_err());
    });
}

#[test]
fn a_held_call_row_reads_back_with_its_arguments() {
    let h = HeldCall {
        id: "fc1".into(),
        job: "j1".into(),
        session: "s".into(),
        turn: "t".into(),
        call: Call {
            content: String::new(),
            function: "sandbox::fs::write".into(),
            text: "/work/repo/a.py".into(),
        },
        arguments: json!({ "path": "/work/repo/a.py", "content": "x" }),
        at: 9,
    };
    let row = json!({ "id": "fc1", "job": "j1", "session": "s", "turn": "t", "function": "sandbox::fs::write",
        "text": "/work/repo/a.py", "arguments": h.arguments.to_string(), "at": 9 });
    assert_eq!(held_call(&row).unwrap(), h);
}

/// The transactions `open` sends on a database at `version`.
fn transactions_at(version: u64) -> Vec<Vec<String>> {
    let engine = Arc::new(
        FakeEngine::new(Link::Up)
            .answer("database::transaction", Ok(json!({ "committed": true })))
            .answer("database::execute", Ok(json!({ "returned_rows": [] })))
            .answer(
                "database::query",
                Ok(json!({ "rows": [{ "user_version": version }] })),
            ),
    );
    crate::rt::block_on(async {
        SqlStore::open(engine.clone(), SEED).await.unwrap();
    });
    let calls = engine.calls.lock().unwrap();
    calls
        .iter()
        .filter(|c| c.1 == "database::transaction")
        .map(|c| {
            c.2["statements"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["sql"].as_str().unwrap().to_string())
                .collect()
        })
        .collect()
}

#[test]
fn migration_1_rebuilds_the_staged_tables_once_with_deferred_keys() {
    let fresh = transactions_at(0);
    assert_eq!(fresh.len(), 6, "the schema, then migrations 1 to 5");
    let m1 = &fresh[1];
    assert_eq!(m1.first().unwrap(), "PRAGMA defer_foreign_keys = ON");
    assert_eq!(m1.last().unwrap(), "PRAGMA user_version = 1");
    for table in ["jobs", "artifacts", "capabilities", "turns"] {
        assert!(m1.contains(&format!("DROP TABLE {table}")));
    }
    assert!(m1.iter().any(|s| s.contains("'code_review'")));
    let m2 = &transactions_at(1)[1];
    assert!(m2.contains(&"DROP TABLE turns".to_string()));
    assert!(!m2.contains(&"DROP TABLE jobs".to_string()));
    let m3 = &transactions_at(2)[1];
    assert!(m3.iter().any(|s| s.contains("'command'")));
    assert!(
        m3.iter()
            .any(|s| s.starts_with("INSERT INTO jobs") && s.contains("workflow_version, node"))
    );
    let m4 = &transactions_at(3)[1];
    assert!(
        m4.iter()
            .any(|s| s.contains("'shell'") && s.starts_with("CREATE TABLE"))
    );
    let jobs_copied =
        format!("INSERT INTO jobs ({JOB_COLUMNS}) SELECT {JOB_COLUMNS} FROM jobs_copy");
    assert!(
        m4.contains(&jobs_copied),
        "every column the alters added is kept"
    );
    for table in ["jobs", "artifacts", "turns", "workflow_nodes"] {
        let renamed = format!("UPDATE {table} SET stage = 'shell' WHERE stage = 'command'");
        assert!(m4.contains(&renamed), "{table}");
    }
    let m5 = &fresh[5];
    assert_eq!(m5.last().unwrap(), "PRAGMA user_version = 5");
    assert!(
        m5.iter()
            .any(|s| s.starts_with("DELETE FROM workflow_nodes WHERE stage IN ('land'")),
        "{m5:#?}"
    );
    assert_eq!(transactions_at(4).len(), 2, "only migration 5 is left");
    assert_eq!(transactions_at(5).len(), 1, "nothing left to migrate");
}

#[test]
fn open_seeds_the_owner_defaults_without_overwriting() {
    let engine = Arc::new(
        FakeEngine::new(Link::Up)
            .answer("database::transaction", Ok(json!({ "committed": true })))
            // An older database already has the column; that error is expected.
            .answer("database::execute", Err("duplicate column name: pr".into()))
            .answer(
                "database::query",
                Ok(json!({ "rows": [{ "user_version": 1 }] })),
            ),
    );
    crate::rt::block_on(async {
        SqlStore::open(engine.clone(), SEED).await.unwrap();
    });
    let calls = engine.calls.lock().unwrap();
    let statements = calls[0].2["statements"].as_array().unwrap();
    let seeds: Vec<_> = statements
        .iter()
        .filter(|s| s["sql"].as_str().unwrap().starts_with("INSERT OR IGNORE"))
        .collect();
    let tables: Vec<_> = seeds
        .iter()
        .map(|s| {
            s["sql"]
                .as_str()
                .unwrap()
                .split_whitespace()
                .nth(4)
                .unwrap()
        })
        .collect();
    assert_eq!(
        tables,
        [
            "constraints",
            "capabilities",
            "workflows",
            "workflow_nodes",
            "workflow_results",
            "workflow_links"
        ]
    );
}

#[test]
fn a_row_reads_back_as_the_record_it_was_written_from() {
    let mut r = record();
    r.job.state = State::NeedsYou;
    r.job.stage = Stage::Shell;
    r.job.node = Some("lint".into());
    r.runtime.sandbox = Some("vm-1".into());
    r.runtime.workflow = Some("w1".into());
    r.runtime.workflow_version = Some(3);
    r.runtime.approved = vec!["tests/a.py".into(), "tests/b.py".into()];
    let columns: Vec<&str> = JOB_COLUMNS.split(", ").collect();
    let values = job_params(&r);
    let row: serde_json::Map<String, Value> = columns
        .iter()
        .zip(values.as_array().unwrap())
        .map(|(c, v)| (c.to_string(), v.clone()))
        .collect();
    assert_eq!(sql::record(&Value::Object(row)).unwrap(), r);
}

#[test]
fn a_job_row_that_does_not_parse_is_left_out_of_the_list() {
    let columns = JOB_COLUMNS.split(", ");
    let mut good: serde_json::Map<String, Value> = columns
        .zip(job_params(&record()).as_array().unwrap().clone())
        .map(|(c, v)| (c.to_string(), v))
        .collect();
    // `open` reads the schema version from the same answer.
    good.insert("user_version".into(), json!(99));
    let bad = json!({ "id": "j2", "state": "nonsense", "user_version": 99 });
    let engine = Arc::new(
        FakeEngine::new(Link::Up)
            .answer("database::transaction", Ok(json!({ "committed": true })))
            .answer("database::query", Ok(json!({ "rows": [good, bad] })))
            .answer("database::execute", Ok(json!({ "returned_rows": [] }))),
    );
    crate::rt::block_on(async {
        let store = SqlStore::open(engine, SEED).await.unwrap();
        let listed = store.jobs(10, 0).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].job.id, "j1");
    });
}

#[test]
fn a_transaction_that_did_not_commit_is_an_error_with_its_reason() {
    let refused = json!({
        "committed": false,
        "error": { "message": "NOT NULL constraint failed: workflows.name" },
    });
    assert_eq!(
        sql::committed(&refused),
        Err("the transaction did not commit: NOT NULL constraint failed: workflows.name".into())
    );
    assert_eq!(sql::committed(&json!({ "committed": true })), Ok(()));
}
