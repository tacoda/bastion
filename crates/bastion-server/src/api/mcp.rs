//! MCP servers (D52). The `bastion-mcp` worker keeps the list and makes
//! each server's tools functions. A server's token stays in the vault: this
//! names the worker a consumer of the secret, so only the worker reads it.

use std::collections::BTreeSet;

use bastion_protocol::McpServerView;
use serde_json::{Value, json};

use crate::engine::Engine;
use crate::engine::NS;

const LIST: &str = "bastion::mcp::servers::list";
const SET: &str = "bastion::mcp::servers::set";
const BRIDGE: &str = "bastion-mcp";

/// The MCP servers the admin listed, with how each one's connection went.
pub async fn mcp_servers(engine: &dyn Engine) -> Result<Vec<McpServerView>, String> {
    let out = engine.call(NS, LIST, json!({})).await?;
    let listed = out["servers"].as_array().into_iter().flatten();
    let text = |s: &Value, k: &str| s[k].as_str().unwrap_or_default().to_string();
    Ok(listed
        .map(|s| McpServerView {
            id: text(s, "id"),
            url: text(s, "url"),
            secret: text(s, "secret"),
            status: text(&out["status"], &text(s, "id")),
        })
        .collect())
}

fn secrets(servers: &[McpServerView]) -> BTreeSet<String> {
    let named = servers.iter().map(|s| s.secret.trim().to_string());
    named.filter(|s| !s.is_empty()).collect()
}

/// Name the bridge a consumer of secret `name`, or take it off.
async fn consume(engine: &dyn Engine, name: &str, bridge: bool) -> Result<(), String> {
    let missing = |_| format!("the vault has no secret {name}");
    let held = engine
        .call(NS, "secrets::get", json!({ "name": name }))
        .await
        .map_err(missing)?;
    let mut consumers: Vec<Value> = held["consumers"].as_array().cloned().unwrap_or_default();
    consumers.retain(|c| c != BRIDGE);
    if bridge {
        consumers.push(json!(BRIDGE));
    }
    let access = json!({ "name": name, "consumers": consumers });
    engine.call(NS, "secrets::access", access).await.map(|_| ())
}

/// Keep `wanted` as the MCP servers. The bridge may read the secrets they
/// name, and no longer the ones only a removed server named.
pub async fn mcp_servers_set(
    engine: &dyn Engine,
    wanted: Vec<McpServerView>,
) -> Result<(), String> {
    let was = secrets(&mcp_servers(engine).await?);
    let now = secrets(&wanted);
    for name in &now {
        consume(engine, name, true).await?;
    }
    for name in was.difference(&now) {
        // Best effort: a secret already deleted has no consumers to change.
        let _ = consume(engine, name, false).await;
    }
    let servers = wanted
        .iter()
        .map(|s| json!({ "id": s.id, "url": s.url, "secret": s.secret }));
    let set = json!({ "servers": servers.collect::<Vec<_>>() });
    engine.call(NS, SET, set).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeEngine;
    use bastion_protocol::Link;

    #[test]
    fn a_listed_server_s_secret_goes_to_the_bridge_and_a_removed_one_s_does_not() {
        let engine = FakeEngine::new(Link::Up)
            .answer(
                LIST,
                Ok(json!({ "servers": [{ "id": "old", "url": "http://o", "secret": "OLD" }], "status": { "old": "connected, 2 tools" } })),
            )
            .answer("secrets::get", Ok(json!({ "consumers": ["bastion-server"] })))
            .answer("secrets::access", Ok(json!({})))
            .answer(SET, Ok(json!({})));
        let listed = crate::rt::block_on(mcp_servers(&engine)).unwrap();
        assert_eq!(listed[0].status, "connected, 2 tools");
        let docs = McpServerView {
            id: "docs".into(),
            url: "https://mcp.example/mcp".into(),
            secret: "DOCS".into(),
            ..McpServerView::default()
        };
        crate::rt::block_on(mcp_servers_set(&engine, vec![docs])).unwrap();
        let calls = engine.calls.lock().unwrap().clone();
        let access: Vec<&Value> = calls
            .iter()
            .filter(|c| c.1 == "secrets::access")
            .map(|c| &c.2)
            .collect();
        assert_eq!(
            access[0]["consumers"],
            json!(["bastion-server", "bastion-mcp"])
        );
        assert_eq!(access[1]["name"], "OLD");
        assert_eq!(access[1]["consumers"], json!(["bastion-server"]));
    }
}
