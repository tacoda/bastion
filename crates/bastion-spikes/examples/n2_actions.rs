//! N2: do the factory's two routing actions work from Rust?
//!
//! The factory enqueues each step on `bastion-steps` and sends job changes as
//! fire-and-forget (Bastion finding 12). Free. Run against `make up`:
//! `cargo run -p bastion-spikes --example n2_actions`.

use std::time::Duration;

use anyhow::Context;
use bastion_spikes::{connect, ensure, Checks};
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::{Error, RegisterFunction, TriggerAction};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::timeout;

#[tokio::main]
async fn main() {
    let iii = connect("spike-n2");
    let mut checks = Checks::default();
    let (tx, mut rx) = mpsc::channel::<Value>(8);
    iii.register_function(
        "bastion::spike::n2::step",
        RegisterFunction::new_async(move |input: Value| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(input).await;
                Ok::<_, Error>(json!({}))
            }
        }),
    );
    let _ = iii.wait_until_registered(Duration::from_secs(5)).await;

    for (name, action) in [
        ("Enqueue on bastion-steps delivers the step", TriggerAction::Enqueue { queue: "bastion-steps".into() }),
        ("Void delivers and returns at once", TriggerAction::Void),
    ] {
        let mark = format!("{action:?}");
        let mut request = TriggerRequest::new("bastion::spike::n2::step", json!({ "mark": mark }));
        request.action = Some(action);
        let sent = iii.trigger(request).await;
        checks
            .run(name, async {
                let answer = sent?;
                let got = timeout(Duration::from_secs(10), rx.recv()).await?.context("handler closed")?;
                ensure(got["mark"] == mark.as_str(), format!("delivered {got}"))?;
                Ok(format!("answered {answer}, delivered {}", got["mark"]))
            })
            .await;
    }
    checks.finish(iii)
}
