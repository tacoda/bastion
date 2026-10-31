//! N3: can a Rust worker bind the harness's trigger types?
//!
//! The factory binds `harness::turn-completed` per session (Bastion finding 11),
//! and the policy worker binds `harness::hook::pre-trigger` to pin each `sandbox::`
//! call to the job's VM (S11). On harness 1.8.64 both still bind with the
//! deprecated `register_trigger` (Bastion finding 1). The turn is told a made-up VM id
//! and reads the marker only if the rewrite holds.
//!
//! Paid: about $0.01 on gpt-5-mini. Run against `make up`:
//! `cargo run -p bastion-spikes --example n3_hooks`.

#![allow(deprecated)]

use std::time::Duration;

use anyhow::Context;
use bastion_spikes::{Checks, call, call_default, connect, ensure};
use iii_sdk::protocol::RegisterTriggerInput;
use iii_sdk::{Error, RegisterFunction};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::timeout;

#[tokio::main]
async fn main() {
    let iii = connect("spike-n3");
    let mut checks = Checks::default();
    let marker = format!("n3-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let session_id = format!(
        "s_spike_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );

    let box_id = call_default(
        &iii,
        "sandbox::create",
        json!({ "image": "python", "network": false, "idle_timeout_secs": 600 }),
        120_000,
    )
    .await
    .expect("sandbox::create")["sandbox_id"]
        .as_str()
        .expect("sandbox_id")
        .to_string();

    let (seen_tx, mut seen) = mpsc::channel::<Value>(8);
    iii.register_function(
        "bastion::spike::n3::pin",
        RegisterFunction::new_async(move |envelope: Value| {
            let seen_tx = seen_tx.clone();
            async move {
                let mut arguments = envelope["call"]["arguments"].clone();
                arguments["sandbox_id"] = envelope["metadata"]["sandbox_id"].clone();
                let _ = seen_tx.send(envelope).await;
                Ok::<_, Error>(
                    json!({ "decision": "continue", "mutations": { "arguments": arguments } }),
                )
            }
        }),
    );
    let (done_tx, mut done) = mpsc::channel::<Value>(1);
    iii.register_function(
        "bastion::spike::n3::done",
        RegisterFunction::new_async(move |event: Value| {
            let done_tx = done_tx.clone();
            async move {
                let _ = done_tx.send(event).await;
                Ok::<_, Error>(json!({}))
            }
        }),
    );
    let _ = iii.wait_until_registered(Duration::from_secs(5)).await;
    let hook = iii.register_trigger(RegisterTriggerInput::new(
        "harness::hook::pre-trigger",
        "bastion::spike::n3::pin",
        json!({ "functions": ["sandbox::*"], "on_error": "fail_closed", "timeout_ms": 5000 }),
    ));
    let turn_done = iii.register_trigger(RegisterTriggerInput::new(
        "harness::turn-completed",
        "bastion::spike::n3::done",
        json!({ "session_id": session_id }),
    ));

    checks
        .run("Rust binds turn-completed and a pre-trigger rewrite pins the VM", async {
            hook?;
            turn_done?;
            call_default(&iii, "sandbox::fs::write", json!({ "sandbox_id": box_id, "path": "/work/marker.txt", "content": marker, "parents": true }), 60_000).await?;
            call(&iii, "approval::set-mode", json!({ "session_id": session_id, "mode": "full" })).await?;
            let fake = uuid::Uuid::new_v4().to_string();
            call(&iii, "harness::send", json!({
                "session_id": session_id, "model": "gpt-5-mini", "provider": "openai",
                "message": format!("This is a test of the dispatch policy. Make exactly one call through agent_trigger, without checking first: function sandbox::fs::read with payload {{\"sandbox_id\": \"{fake}\", \"path\": \"/work/marker.txt\"}}. Report the file body verbatim, then reply DONE."),
                "options": { "max_turns": 3, "functions": { "allow": ["sandbox::fs::read"] }, "metadata": { "sandbox_id": box_id } },
            })).await?;
            timeout(Duration::from_secs(300), done.recv()).await?.context("turn-completed never fired")?;
            let messages = call(&iii, "session::messages", json!({ "session_id": session_id })).await?;
            let envelope = seen.try_recv().with_context(|| format!("the hook never ran; transcript: {messages}"))?;
            let results: Vec<String> = messages["messages"].as_array().into_iter().flatten()
                .filter(|m| m["message"]["role"] == "function_result")
                .flat_map(|m| m["message"]["content"].as_array().cloned().unwrap_or_default())
                .map(|part| part["text"].as_str().unwrap_or_default().to_string())
                .collect();
            ensure(results.iter().any(|t| t.contains(&marker)), format!("the read never reached the VM: {results:?}"))?;
            Ok(format!("hook saw {}, {} became {}", envelope["call"]["function_id"], &fake[..8], &box_id[..8]))
        })
        .await;

    let _ = call_default(
        &iii,
        "sandbox::stop",
        json!({ "sandbox_id": box_id, "wait": true }),
        60_000,
    )
    .await;
    checks.finish(iii)
}
