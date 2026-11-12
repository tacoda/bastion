//! Bastion's MCP bridge (D52). iii ships none: its docs say to write a
//! worker that bridges MCP into `trigger()` calls, and this is that worker.
//! Each MCP server the admin lists is reached over HTTP, and each of its
//! tools is the function `mcp::<server>::<tool>`. A step's grant on
//! `mcp::<server>::*` lets its agent call them; the gate decides each call.

mod mcp;

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iii_sdk::iii::WorkerMetadata;
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::runtime::FunctionRef;
use iii_sdk::{Error, IIIClient, InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};
use tokio::signal::unix::{SignalKind, signal};

const WORKER: &str = "bastion-mcp";
const NAMESPACE: &str = "bastion";
const SERVERS: &str = "servers";

struct Ctx {
    iii: IIIClient,
    /// Each server's tool functions, by server id, dropped when it goes.
    live: Mutex<BTreeMap<String, Vec<FunctionRef>>>,
    /// How each server's connection went, for Settings.
    status: Mutex<BTreeMap<String, String>>,
}

impl Ctx {
    async fn call(&self, function: &str, payload: Value) -> Result<Value, String> {
        let request = TriggerRequest::new(function, payload).namespace(NAMESPACE);
        self.iii.trigger(request).await.map_err(|e| e.to_string())
    }

    async fn read(&self, key: &str) -> Result<Value, String> {
        self.call("state::get", json!({ "scope": WORKER, "key": key }))
            .await
    }

    async fn keep(&self, key: &str, value: Value) -> Result<(), String> {
        let set = json!({ "scope": WORKER, "key": key, "value": value });
        self.call("state::set", set).await.map(|_| ())
    }

    fn said(&self, id: &str, status: String) {
        eprintln!("[{WORKER}] {id}: {status}");
        self.status.lock().unwrap().insert(id.into(), status);
    }
}

fn valid_id(id: &str) -> bool {
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_';
    (1..=64).contains(&id.len()) && id.chars().all(allowed)
}

fn checked_server(s: &Value, seen: &mut HashSet<String>) -> Result<Value, String> {
    let text = |k: &str| s[k].as_str().unwrap_or_default().trim().to_string();
    let (id, url, secret) = (text("id"), text("url"), text("secret"));
    if !valid_id(&id) {
        return Err(format!(
            "a server id is 1 to 64 of a-z, 0-9, - and _, not {id:?}"
        ));
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("{id}: the URL must start with http:// or https://"));
    }
    if !seen.insert(id.clone()) {
        return Err(format!("{id} is listed twice"));
    }
    Ok(json!({ "id": id, "url": url, "secret": secret }))
}

/// The servers as given, trimmed, or why they cannot be kept.
fn checked(servers: &Value) -> Result<Vec<Value>, String> {
    let mut seen = HashSet::new();
    let given = servers.as_array().ok_or("servers must be a list")?;
    given.iter().map(|s| checked_server(s, &mut seen)).collect()
}

/// What a tool's call sends on: its arguments without the VM the gate pins
/// every call to and the caller iii adds, which an MCP server knows nothing of.
fn arguments(mut input: Value) -> Value {
    if let Some(map) = input.as_object_mut() {
        map.remove("sandbox_id");
        map.remove("_caller_worker_id");
    }
    input
}

/// Connect to server `server` and register each of its tools.
async fn connect(ctx: Arc<Ctx>, server: Value) {
    let id = server["id"].as_str().unwrap_or_default().to_string();
    match register(&ctx, &server).await {
        Ok(functions) => {
            ctx.said(&id, format!("connected, {} tools", functions.len()));
            ctx.live.lock().unwrap().insert(id, functions);
        }
        Err(e) => ctx.said(&id, format!("not connected: {e}")),
    }
}

async fn register(ctx: &Arc<Ctx>, server: &Value) -> Result<Vec<FunctionRef>, String> {
    let (id, url) = (
        server["id"].as_str().unwrap_or_default(),
        server["url"].as_str().unwrap_or_default(),
    );
    let secret = server["secret"].as_str().unwrap_or_default();
    // The vault gives the token only to this worker, named its consumer.
    let token = match secret.is_empty() {
        true => None,
        false => {
            let resolved = ctx.call(
                "secrets::resolve",
                json!({ "ref": format!("secret://{secret}") }),
            );
            Some(
                resolved.await?["value"]
                    .as_str()
                    .ok_or("the vault gave no token")?
                    .to_string(),
            )
        }
    };
    let client = Arc::new(mcp::Client::connect(url, token).await?);
    let tools = client.tools().await?;
    Ok(tools.iter().map(|t| tool(ctx, id, &client, t)).collect())
}

fn tool(ctx: &Arc<Ctx>, server: &str, client: &Arc<mcp::Client>, t: &Value) -> FunctionRef {
    let name = t["name"].as_str().unwrap_or_default().to_string();
    let (c, n) = (client.clone(), name.clone());
    let call = RegisterFunction::new_async(move |input: Value| {
        let (c, n) = (c.clone(), n.clone());
        async move { c.call(&n, arguments(input)).await.map_err(Error::Handler) }
    });
    let about = t["description"].as_str().unwrap_or_default();
    let call = call
        .description(format!("{about} (MCP server {server})"))
        .request_format(t["inputSchema"].clone());
    ctx.iii
        .register_function(format!("mcp::{server}::{name}"), call)
}

/// Make the registered servers match `servers`.
fn apply(ctx: &Arc<Ctx>, servers: &[Value]) {
    for (_, functions) in std::mem::take(&mut *ctx.live.lock().unwrap()) {
        functions.iter().for_each(FunctionRef::unregister);
    }
    ctx.status.lock().unwrap().clear();
    for s in servers {
        tokio::spawn(connect(ctx.clone(), s.clone()));
    }
}

async fn set(ctx: Arc<Ctx>, input: Value) -> Result<Value, Error> {
    let servers = checked(&input[SERVERS]).map_err(Error::Handler)?;
    ctx.keep(SERVERS, json!(servers))
        .await
        .map_err(Error::Handler)?;
    apply(&ctx, &servers);
    Ok(json!({ SERVERS: servers }))
}

async fn list(ctx: Arc<Ctx>) -> Result<Value, Error> {
    let servers = ctx.read(SERVERS).await.unwrap_or(Value::Null);
    let status = ctx.status.lock().unwrap().clone();
    Ok(json!({ SERVERS: servers, "status": status }))
}

fn worker() -> IIIClient {
    let url = std::env::var("III_URL").unwrap_or_else(|_| "ws://127.0.0.1:49160".into());
    let metadata = WorkerMetadata {
        name: WORKER.into(),
        ..WorkerMetadata::default()
    };
    let options = InitOptions {
        metadata: Some(metadata),
        namespace: Some(NAMESPACE.into()),
        ..InitOptions::default()
    };
    register_worker(&url, options)
}

/// The kept servers, read again until the state worker answers.
async fn servers_at_start(ctx: &Ctx) -> Value {
    let mut wait = Duration::from_millis(500);
    for _ in 0..30 {
        match ctx.read(SERVERS).await {
            Ok(servers) => return servers,
            Err(e) => eprintln!("could not read the servers ({e}); trying again in {wait:?}"),
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(10));
    }
    Value::Null
}

#[tokio::main]
async fn main() {
    let ctx = Arc::new(Ctx {
        iii: worker(),
        live: Mutex::default(),
        status: Mutex::default(),
    });
    let c = ctx.clone();
    ctx.iii.register_function(
        "bastion::mcp::servers::set",
        RegisterFunction::new_async(move |input: Value| set(c.clone(), input)),
    );
    let c = ctx.clone();
    ctx.iii.register_function(
        "bastion::mcp::servers::list",
        RegisterFunction::new_async(move |_: Value| list(c.clone())),
    );
    let kept = checked(&servers_at_start(&ctx).await).unwrap_or_default();
    apply(&ctx, &kept);
    signal(SignalKind::terminate())
        .expect("a SIGTERM handler")
        .recv()
        .await;
    ctx.iii.shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_are_trimmed_and_refused_when_they_cannot_be_reached() {
        let given =
            json!([{ "id": " docs ", "url": "https://mcp.example/mcp", "secret": " DOCS_TOKEN " }]);
        let servers = checked(&given).unwrap();
        assert_eq!(servers[0]["id"], "docs");
        assert_eq!(servers[0]["secret"], "DOCS_TOKEN");
        assert!(checked(&json!([{ "id": "Docs::x", "url": "http://h" }])).is_err());
        assert!(checked(&json!([{ "id": "d", "url": "stdio:./server" }])).is_err());
    }

    #[test]
    fn a_tool_call_drops_the_vm_the_gate_pinned() {
        let sent =
            arguments(json!({ "query": "x", "sandbox_id": "vm1", "_caller_worker_id": "w" }));
        assert_eq!(sent, json!({ "query": "x" }));
    }
}
