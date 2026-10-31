//! N8 (PLAN.md A5): can a `pre-trigger` hook deny one call and hold another,
//! and can the app release a held call later?
//!
//! The hook denies `sandbox::fs::ls` with a reason, and holds `sandbox::exec`.
//! The spike then releases the held call with `harness::function::resolve`
//! (mode `execute`) and checks that the command ran and the turn finished.
//!
//! Paid: one short turn, about $0.01 on gpt-5-mini. Run against the stack:
//! `cargo run -p bastion-spikes --example n8_gate`.

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
    let iii = connect("spike-n8");
    let mut checks = Checks::default();
    let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let session_id = format!("s_spike_n8_{tag}");

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

    let (held_tx, mut held) = mpsc::channel::<Value>(4);
    let pinned = box_id.clone();
    iii.register_function(
        "bastion::spike::n8::gate",
        RegisterFunction::new_async(move |envelope: Value| {
            let (held_tx, pinned) = (held_tx.clone(), pinned.clone());
            async move {
                let function = envelope["call"]["function_id"].as_str().unwrap_or_default().to_string();
                let mut arguments = envelope["call"]["arguments"].clone();
                arguments["sandbox_id"] = json!(pinned);
                let answer = match function.as_str() {
                    "sandbox::fs::ls" => json!({ "decision": "deny", "reason": "n8: listing is not allowed in this stage" }),
                    "sandbox::exec" => {
                        let _ = held_tx.send(envelope.clone()).await;
                        json!({ "decision": "hold", "reason": "n8: waiting for the owner", "mutations": { "arguments": arguments } })
                    }
                    _ => json!({ "decision": "continue", "mutations": { "arguments": arguments } }),
                };
                Ok::<_, Error>(answer)
            }
        }),
    );
    let (done_tx, mut done) = mpsc::channel::<Value>(1);
    iii.register_function(
        "bastion::spike::n8::done",
        RegisterFunction::new_async(move |event: Value| {
            let done_tx = done_tx.clone();
            async move {
                let _ = done_tx.send(event).await;
                Ok::<_, Error>(json!({}))
            }
        }),
    );
    let _ = iii.wait_until_registered(Duration::from_secs(5)).await;

    checks
        .run("a hook denies one call, holds another, and the app releases it", async {
            let hook = iii.register_trigger(RegisterTriggerInput::new(
                "harness::hook::pre-trigger",
                "bastion::spike::n8::gate",
                json!({ "functions": ["sandbox::*"], "on_error": "fail_closed", "timeout_ms": 5000 }),
            ))?;
            let turn_done = iii.register_trigger(RegisterTriggerInput::new(
                "harness::turn-completed",
                "bastion::spike::n8::done",
                json!({ "session_id": session_id }),
            ))?;
            call(&iii, "approval::set-mode", json!({ "session_id": session_id, "mode": "full" })).await?;
            call(&iii, "harness::send", json!({
                "session_id": session_id, "model": "gpt-5-mini", "provider": "openai",
                "message": "Make exactly these two calls through agent_trigger, in order, without checking first. 1) sandbox::fs::ls with {\"sandbox_id\": \"job\", \"path\": \"/\"}. 2) sandbox::exec with {\"sandbox_id\": \"job\", \"argv\": [\"echo\", \"n8-ran\"]}. Make call 2 even if call 1 fails. Then report exactly what each call returned, and reply DONE.",
                "options": { "max_turns": 6, "functions": { "allow": ["sandbox::fs::ls", "sandbox::exec"] } },
            })).await?;

            let envelope = timeout(Duration::from_secs(180), held.recv()).await?.context("the exec was never held")?;
            println!("held envelope keys: {:?}", envelope.as_object().map(|o| o.keys().collect::<Vec<_>>()));
            println!("held call: {}", serde_json::to_string(&envelope["call"]).unwrap_or_default());
            let status = call(&iii, "harness::status", json!({ "session_id": session_id, "verbose": true })).await?;
            println!("status while held: {}", status["status"]);
            let call_id = envelope["call"]["call_id"].as_str()
                .or(envelope["call"]["function_call_id"].as_str())
                .or(envelope["call"]["id"].as_str())
                .unwrap_or_default().to_string();
            // The owner approved: the app runs the call itself, pinned to the VM,
            // and delivers its result to the parked turn.
            let mut arguments = envelope["call"]["arguments"].clone();
            arguments["sandbox_id"] = json!(box_id);
            let ran = call_default(&iii, "sandbox::exec", arguments, 60_000).await?;
            let released = call(&iii, "harness::function::resolve", json!({
                "session_id": session_id, "turn_id": envelope["turn_id"], "function_call_id": call_id, "action": "deliver",
                "content": [{ "type": "text", "text": ran.to_string() }], "details": ran, "is_error": false,
            })).await;
            println!("resolve: {released:?}");
            released?;

            timeout(Duration::from_secs(180), done.recv()).await?.context("turn-completed never fired")?;
            hook.unregister();
            turn_done.unregister();
            let messages = call(&iii, "session::messages", json!({ "session_id": session_id })).await?;
            let results: Vec<String> = messages["messages"].as_array().into_iter().flatten()
                .filter(|m| m["message"]["role"] == "function_result")
                .map(|m| m["message"]["content"].to_string())
                .collect();
            for r in &results {
                println!("result: {}", &r[..r.len().min(300)]);
            }
            ensure(results.iter().any(|r| r.contains("not allowed in this stage")), "the deny reason never reached the turn")?;
            ensure(results.iter().any(|r| r.contains("n8-ran")), "the released exec never ran")?;
            Ok(format!("{} results; denied ls, held then released exec", results.len()))
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
