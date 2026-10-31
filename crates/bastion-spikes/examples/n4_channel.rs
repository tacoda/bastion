//! N4: can Rust read a file out of a sandbox through its content channel?
//!
//! `sandbox::fs::read` answers inline only for UTF-8 text under 1 MiB. The P6
//! factory base64-encodes each bundle to fit, so its commits stop at 1 MiB. The
//! content channel carries any size. Free. Run against `make up`:
//! `cargo run -p bastion-spikes --example n4_channel`.

use anyhow::Context;
use bastion_spikes::{Checks, call_default, connect, engine_url, ensure};
use iii_sdk::channel::{ChannelReader, StreamChannelRef};
use serde_json::json;
use sha2::{Digest, Sha256};

const SIZE: usize = 2_000_000; // past the 1 MiB inline limit

#[tokio::main]
async fn main() {
    let iii = connect("spike-n4");
    let mut checks = Checks::default();
    let created = call_default(
        &iii,
        "sandbox::create",
        json!({ "image": "python", "network": false }),
        120_000,
    )
    .await;
    let box_id = created
        .as_ref()
        .map(|c| c["sandbox_id"].as_str().unwrap_or_default().to_string())
        .unwrap_or_default();

    checks
        .run(
            "a 2 MB binary file leaves the VM whole through its channel",
            async {
                created?;
                let exec = |argv: Vec<&str>| {
                    call_default(
                        &iii,
                        "sandbox::exec",
                        json!({ "sandbox_id": box_id, "argv": argv, "timeout_ms": 60_000 }),
                        90_000,
                    )
                };
                exec(vec![
                    "sh",
                    "-c",
                    &format!("mkdir -p /work && head -c {SIZE} /dev/urandom > /work/big.bin"),
                ])
                .await?;
                let inside = exec(vec!["sha256sum", "/work/big.bin"]).await?;
                let inside = inside["stdout"]
                    .as_str()
                    .context("no stdout")?
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string();
                let read = call_default(
                    &iii,
                    "sandbox::fs::read",
                    json!({ "sandbox_id": box_id, "path": "/work/big.bin" }),
                    60_000,
                )
                .await?;
                ensure(
                    read.get("body").is_none_or(|b| b.is_null()),
                    "an inline body came back for binary",
                )?;
                let channel: StreamChannelRef = serde_json::from_value(read["content"].clone())?;
                let bytes = ChannelReader::new(&engine_url(), &channel)
                    .read_all()
                    .await?;
                let outside = format!("{:x}", Sha256::digest(&bytes));
                ensure(
                    bytes.len() == SIZE && outside == inside,
                    format!("{} bytes, sha {outside} vs {inside}", bytes.len()),
                )?;
                Ok(format!("{} bytes, sha256 {}", bytes.len(), &outside[..12]))
            },
        )
        .await;

    let _ = call_default(
        &iii,
        "sandbox::stop",
        json!({ "sandbox_id": box_id, "wait": true }),
        60_000,
    )
    .await;
    checks.finish(iii)
}
