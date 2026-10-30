//! What every spike shares: one engine connection, calls, and named checks.
//!
//! A spike answers a question in PLAN.md A0. Each check prints PASS or FAIL
//! with its reason, and the example exits non-zero when any check fails. Run
//! one with `cargo run -p bastion-spikes --example n1_engine` against `make up`.

use std::future::Future;
use std::time::Instant;

use anyhow::{anyhow, Result};
use iii_sdk::iii::WorkerMetadata;
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::{register_worker, IIIClient, InitOptions};
use serde_json::Value;

pub const NAMESPACE: &str = "bastion";

pub fn engine_url() -> String {
    std::env::var("III_URL").unwrap_or_else(|_| "ws://127.0.0.1:49160".into())
}

pub fn connect(name: &str) -> IIIClient {
    let metadata = WorkerMetadata { name: name.into(), ..WorkerMetadata::default() };
    let options = InitOptions {
        metadata: Some(metadata),
        namespace: Some(NAMESPACE.into()),
        ..InitOptions::default()
    };
    register_worker(&engine_url(), options)
}

/// Call a function in this worker's namespace and wait for its answer.
pub async fn call(iii: &IIIClient, function_id: &str, payload: Value) -> Result<Value> {
    Ok(iii.trigger(TriggerRequest::new(function_id, payload)).await?)
}

/// Call a function the engine registers in `default`, such as `sandbox::*` or `engine::*`.
pub async fn call_default(iii: &IIIClient, function_id: &str, payload: Value, timeout_ms: u64) -> Result<Value> {
    let mut request = TriggerRequest::new(function_id, payload);
    request.timeout_ms = Some(timeout_ms);
    Ok(iii.trigger(request.namespace("default")).await?)
}

pub fn ensure(ok: bool, why: impl Into<String>) -> Result<()> {
    if ok { Ok(()) } else { Err(anyhow!(why.into())) }
}

#[derive(Default)]
pub struct Checks {
    failed: Vec<String>,
}

impl Checks {
    /// Run one check and report it. A failure never stops the run.
    pub async fn run<F: Future<Output = Result<String>>>(&mut self, name: &str, check: F) {
        let started = Instant::now();
        match check.await {
            Ok(detail) => println!("PASS  {name}  ({} ms)  {detail}", started.elapsed().as_millis()),
            Err(error) => {
                println!("FAIL  {name}: {error:#}");
                self.failed.push(name.into());
            }
        }
    }

    /// Shut the connection down and exit non-zero on any failure (L7).
    pub fn finish(self, iii: IIIClient) -> ! {
        iii.shutdown();
        if self.failed.is_empty() {
            std::process::exit(0);
        }
        println!("\n{} check(s) failed: {}", self.failed.len(), self.failed.join(", "));
        std::process::exit(1);
    }
}
