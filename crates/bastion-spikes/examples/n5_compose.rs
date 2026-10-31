//! N5: does compose run and restart a Rust worker binary?
//!
//! Every Bastion worker becomes a Rust binary that compose starts through
//! `scripts.run` (PLAN.md D6). This adds one with `compose::add`, calls it,
//! restarts it, and removes it again. Free. Run against `make up`:
//! `cargo build -p bastion-spikes --example n5_worker && cargo run -p bastion-spikes --example n5_compose`.

use std::time::Duration;

use anyhow::{Result, anyhow};
use bastion_spikes::{Checks, call, connect, ensure};
use iii_sdk::IIIClient;
use iii_sdk::protocol::TriggerRequest;
use serde_json::{Value, json};

const PATH: &str = "path://./crates/bastion-spikes";

/// The worker's pid, once it answers. Compose starts it in the background.
async fn ping(iii: &IIIClient, not: Option<u64>) -> Result<u64> {
    for _ in 0..60 {
        if let Ok(out) = call(iii, "bastion::spike::n5::ping", json!({})).await {
            let pid = out["pid"].as_u64().unwrap_or_default();
            if Some(pid) != not {
                return Ok(pid);
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(anyhow!(
        "bastion::spike::n5::ping never answered with a new pid"
    ))
}

/// compose registers in this worker's namespace, with a long timeout.
async fn compose(iii: &IIIClient, function_id: &str, payload: Value) -> Result<Value> {
    let mut request = TriggerRequest::new(function_id, payload);
    request.timeout_ms = Some(120_000);
    Ok(iii.trigger(request).await?)
}

async fn container(iii: &IIIClient) -> Result<String> {
    let status = compose(iii, "compose::status", json!({})).await?;
    let found = status["containers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c: &&Value| c.to_string().contains("bastion-spikes"))
        .ok_or_else(|| anyhow!("no container for {PATH}"))?;
    Ok(found["container"].as_str().unwrap_or_default().to_string())
}

#[tokio::main]
async fn main() {
    let iii = connect("spike-n5");
    let mut checks = Checks::default();
    let mut name = String::new();
    checks
        .run(
            "compose starts a Rust binary through scripts.run, and restarts it",
            async {
                compose(
                    &iii,
                    "compose::add",
                    json!({ "workers": [{
                "worker": PATH, "working_dir": ".", "start_after": [],
                "scripts": { "run": "target/debug/examples/n5_worker" },
            }] }),
                )
                .await?;
                let first = ping(&iii, None).await?;
                name = container(&iii).await?;
                compose(&iii, "compose::restart", json!({ "container": name })).await?;
                let second = ping(&iii, Some(first)).await?;
                ensure(first != second, "the same process answered after a restart")?;
                Ok(format!(
                    "container {name}: pid {first}, then {second} after a restart"
                ))
            },
        )
        .await;
    if !name.is_empty() {
        let _ = compose(&iii, "compose::remove", json!({ "worker": name })).await;
    }
    checks.finish(iii)
}
