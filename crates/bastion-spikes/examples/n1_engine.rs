//! N1: can a Rust worker do the engine work the Python workers do?
//!
//! It registers a function and calls it, calls an engine function, observes a
//! function the way the app will observe job changes, and reads and writes
//! through `database`. Free. Run
//! against `make up`: `cargo run -p bastion-spikes --example n1_engine`.

use std::time::Duration;

use anyhow::Context;
use bastion_spikes::{Checks, call, call_default, connect, ensure};
use iii_sdk::{Capture, Error, RegisterFunction, Source};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::timeout;

#[tokio::main]
async fn main() {
    let iii = connect("spike-n1");
    let mut checks = Checks::default();

    iii.register_function(
        "bastion::spike::n1::echo",
        RegisterFunction::new_async(|input: Value| async move {
            Ok::<_, Error>(json!({ "echo": input }))
        }),
    );
    checks
        .run("a Rust function answers a call", async {
            iii.wait_until_registered(Duration::from_secs(5)).await?;
            let out = call(&iii, "bastion::spike::n1::echo", json!({ "word": "hi" })).await?;
            ensure(
                out["echo"]["word"] == "hi",
                format!("unexpected answer: {out}"),
            )?;
            Ok(format!("{out}"))
        })
        .await;

    checks
        .run("an engine function answers a Rust caller", async {
            let out = call_default(&iii, "engine::functions::list", json!({}), 10_000).await?;
            let functions = out["functions"].as_array().context("no functions array")?;
            Ok(format!("{} functions registered", functions.len()))
        })
        .await;

    // The source stands in for the factory's job-changed function.
    iii.register_function(
        "bastion::spike::n1::source",
        RegisterFunction::new_async(|_: Value| async move { Ok::<_, Error>(json!({})) }),
    );
    let (tx, mut rx) = mpsc::channel::<Value>(8);
    iii.register_function(
        "bastion::spike::n1::changed",
        RegisterFunction::new_async(move |event: Value| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(event).await;
                Ok::<_, Error>(json!({}))
            }
        }),
    );
    let observer = iii.observe(
        Source::function("bastion::spike::n1::source").capture([Capture::Input]),
        "bastion::spike::n1::changed",
    );
    checks
        .run("an observer receives each call by push", async {
            observer.ready().await?;
            call(
                &iii,
                "bastion::spike::n1::source",
                json!({ "id": "spike-n1", "revision": 7 }),
            )
            .await?;
            loop {
                let event = timeout(Duration::from_secs(5), rx.recv())
                    .await?
                    .context("observer closed")?;
                if event["input"]["id"] == "spike-n1" {
                    return Ok(format!("{}", event["input"]));
                }
            }
        })
        .await;

    checks
        .run("database::query and database::transaction work from Rust", async {
            let done = call(&iii, "database::transaction", json!({ "statements": [
                { "sql": "CREATE TABLE IF NOT EXISTS bastion_spike_n1 (word TEXT)", "params": [] },
                { "sql": "INSERT INTO bastion_spike_n1 VALUES (?)", "params": ["rust"] },
            ] }))
            .await?;
            ensure(done["committed"] == true, format!("not committed: {done}"))?;
            let rows = call(&iii, "database::query", json!({ "sql": "SELECT word FROM bastion_spike_n1", "params": [] })).await?;
            call(&iii, "database::transaction", json!({ "statements": [{ "sql": "DROP TABLE bastion_spike_n1", "params": [] }] })).await?;
            ensure(rows["rows"][0]["word"] == "rust", format!("rows: {rows}"))?;
            Ok("rows keyed by column name (L3)".into())
        })
        .await;

    checks.finish(iii)
}
