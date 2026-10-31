//! N11 (PLAN.md D26): can the stack's `configuration` worker carry Bastion's
//! harness and factory defaults? `ensure` seeds an id once and `get` reads it.
//! `cargo run -p bastion-spikes --example n11_config -- ensure|get`.

use bastion_spikes::{call_default, connect};
use serde_json::json;

const ID: &str = "spike-d26";

#[tokio::main]
async fn main() {
    let iii = connect("spike-n11");
    let step = std::env::args().nth(1).unwrap_or_else(|| "get".into());
    let answer = match step.as_str() {
        "ensure" => {
            let payload = json!({
                "id": ID,
                "name": "Spike D26",
                "description": "A seeded default the owner may edit.",
                "schema": { "type": "object" },
                "initial_value": { "rules": ["seeded"] },
            });
            call_default(&iii, "configuration::ensure", payload, 10_000).await
        }
        _ => call_default(&iii, "configuration::get", json!({ "id": ID }), 10_000).await,
    };
    println!("{step}: {answer:?}");
    std::process::exit(0);
}
