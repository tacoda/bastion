//! N9 (PLAN.md A6): what `judge::evaluate` takes and gives back, for a
//! grade of one eval run. Paid: one short judge call on gpt-5-mini.
//! `cargo run -p bastion-spikes --example n9_judge`.

use bastion_spikes::{call, connect};
use serde_json::json;

#[tokio::main]
async fn main() {
    let iii = connect("spike-n9");
    for payload in [
        json!({}),
        json!({
            "provider": "openai", "model": "gpt-5-mini",
            "state": { "request": "Add modulo(a, b) with a test", "diff": "+def modulo(a, b):\n+    return a % b\n+def test_modulo():\n+    assert modulo(5, 3) == 2\n" },
            "questions": [{ "id": "done", "type": "score", "question": "From 0 to 10, how fully does the diff do the request?", "min": 0, "max": 10 }],
        }),
    ] {
        match call(&iii, "judge::evaluate", payload).await {
            Ok(out) => println!("ok: {}", serde_json::to_string_pretty(&out).unwrap()),
            Err(e) => println!("err: {e}"),
        }
    }
    std::process::exit(0);
}
