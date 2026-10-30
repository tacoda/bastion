//! The worker N5 asks compose to run: one function that answers with its pid.

use bastion_spikes::connect;
use iii_sdk::{Error, RegisterFunction};
use serde_json::{json, Value};
use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() {
    let iii = connect("spike-n5-worker");
    iii.register_function(
        "bastion::spike::n5::ping",
        RegisterFunction::new_async(|_: Value| async move { Ok::<_, Error>(json!({ "pid": std::process::id() })) }),
    );
    signal(SignalKind::terminate()).expect("SIGTERM handler").recv().await;
    iii.shutdown();
}
