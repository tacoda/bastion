//! N10 (PLAN.md A7): the models and providers `llm-router` lists.
//! `cargo run -p bastion-spikes --example n10_router`.

use bastion_spikes::{call, connect};
use serde_json::json;

#[tokio::main]
async fn main() {
    let iii = connect("spike-n10-router");
    for f in ["router::provider::list", "router::models::list"] {
        let out = call(&iii, f, json!({}))
            .await
            .map(|v| serde_json::to_string(&v).unwrap())
            .unwrap_or_else(|e| e.to_string());
        println!("{f}: {}", &out[..out.len().min(1500)]);
    }
    std::process::exit(0);
}
