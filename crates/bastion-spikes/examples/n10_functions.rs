//! N10 (PLAN.md A7): what the stock `github`, `secrets` and `router` workers
//! offer an adapter. Lists each function under a prefix with its description.
//! `cargo run -p bastion-spikes --example n10_functions -- github::`.

use bastion_spikes::{call_default, connect};
use serde_json::json;

#[tokio::main]
async fn main() {
    let prefix = std::env::args().nth(1).unwrap_or_default();
    let iii = connect("spike-n10");
    let all = call_default(&iii, "engine::functions::list", json!({}), 10_000)
        .await
        .expect("list");
    for f in all["functions"].as_array().into_iter().flatten() {
        let id = f["function_id"].as_str().unwrap_or("");
        if id.starts_with(&prefix) {
            println!(
                "{id} [{}] :: {}",
                f["namespace"].as_str().unwrap_or(""),
                f["description"].as_str().unwrap_or("")
            );
        }
    }
    std::process::exit(0);
}
