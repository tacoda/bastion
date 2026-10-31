//! N10 (PLAN.md A7): what `github::exec` returns, from read-only gh calls.
//! `cargo run -p bastion-spikes --example n10_gh -- --version`.

use bastion_spikes::{call, connect};
use serde_json::json;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let iii = connect("spike-n10-gh");
    let out = call(&iii, "github::exec", json!({ "args": args })).await;
    println!(
        "{}",
        match out {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap(),
            Err(e) => e.to_string(),
        }
    );
    std::process::exit(0);
}
