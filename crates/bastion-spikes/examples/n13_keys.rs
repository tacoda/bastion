//! N13 (PLAN.md D34, K1): can a provider key live in the `secrets` vault and
//! reach `llm-router` as `secret://NAME`? Calls one function with a JSON
//! payload and prints the answer, so each step of the spike is one run:
//! `cargo run -p bastion-spikes --example n13_keys -- router::provider::list '{}'`.

use bastion_spikes::{call, connect};
use serde_json::Value;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let function = args.next().expect("a function id");
    let payload: Value =
        serde_json::from_str(&args.next().unwrap_or_else(|| "{}".into())).expect("a JSON payload");
    let iii = connect("spike-n13");
    match call(&iii, &function, payload).await {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => println!("error: {e}"),
    }
    std::process::exit(0);
}
