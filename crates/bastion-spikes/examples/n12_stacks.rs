//! N12 (PLAN.md S0): do two stacks on this machine keep apart? Counts the
//! jobs in the database of the stack at `III_URL`.
//! `III_URL=ws://127.0.0.1:49170 cargo run -p bastion-spikes --example n12_stacks`.

use bastion_spikes::{call, connect, engine_url};
use serde_json::json;

#[tokio::main]
async fn main() {
    let iii = connect("spike-n12");
    let sql = json!({ "sql": "SELECT count(*) AS n FROM sqlite_master WHERE name = 'jobs'", "params": [] });
    let tables = call(&iii, "database::query", sql).await;
    let jobs = call(
        &iii,
        "database::query",
        json!({ "sql": "SELECT count(*) AS n FROM jobs", "params": [] }),
    )
    .await;
    println!("{}: jobs table {tables:?}; jobs {jobs:?}", engine_url());
    std::process::exit(0);
}
