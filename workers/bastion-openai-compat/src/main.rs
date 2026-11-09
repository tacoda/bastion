//! Bastion's worker for OpenAI-compatible hosts: Together,
//! Fireworks, Qwen through DashScope, a local Ollama. Each host the admin
//! lists is one llm-router provider; its key, if it needs one, is set like
//! any provider's (D34).

mod host;
mod wire;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use host::{Ctx, Live, WORKER};
use iii_sdk::iii::WorkerMetadata;
use iii_sdk::{Error, InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};
use tokio::signal::unix::{SignalKind, signal};

const HOSTS: &str = "hosts";
/// Ids of removed hosts. The router still lists them, so the server lets
/// them be added again.
const RETIRED: &str = "retired";

fn valid_id(id: &str) -> bool {
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_';
    (1..=64).contains(&id.len()) && id.chars().all(allowed)
}

fn checked_host(h: &Value, seen: &mut HashSet<String>) -> Result<Value, String> {
    let id = h["id"].as_str().unwrap_or_default().trim();
    let name = h["name"].as_str().unwrap_or_default().trim();
    let url = h["api_url"].as_str().unwrap_or_default().trim();
    if !valid_id(id) {
        return Err(format!(
            "a host id is 1 to 64 of a-z, 0-9, - and _, not {id:?}"
        ));
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("{id}: the URL must start with http:// or https://"));
    }
    if !seen.insert(id.to_string()) {
        return Err(format!("{id} is listed twice"));
    }
    let name = if name.is_empty() { id } else { name };
    Ok(json!({ "id": id, "name": name, "api_url": url }))
}

/// The hosts as given, trimmed, or why they cannot be kept.
fn checked(hosts: &Value) -> Result<Vec<Value>, String> {
    let mut seen = HashSet::new();
    let given = hosts.as_array().ok_or("hosts must be a list")?;
    given.iter().map(|h| checked_host(h, &mut seen)).collect()
}

type Registry = Mutex<BTreeMap<String, Live>>;

/// Make the registered hosts match `hosts`: drop every host, retire the
/// ones no longer listed, register each.
fn apply(ctx: &Arc<Ctx>, live: &Registry, hosts: &[Value]) {
    let mut live = live.lock().unwrap();
    for (id, was) in std::mem::take(&mut *live) {
        was.unregister();
        if !hosts.iter().any(|h| h["id"] == id.as_str()) {
            tokio::spawn(host::retire(ctx.clone(), was.host));
        }
    }
    for h in hosts {
        let id = h["id"].as_str().unwrap_or_default().to_string();
        live.insert(id, host::register(ctx, h));
    }
}

fn ids(list: &Value, field: Option<&str>) -> BTreeSet<String> {
    let items = list.as_array().into_iter().flatten();
    let id = |v: &Value| field.map_or(v, |f| &v[f]).as_str().map(String::from);
    items.filter_map(id).collect()
}

/// Every id that was a host and is not one now.
fn retired(was_retired: &Value, was: &Value, hosts: &[Value]) -> Vec<String> {
    let now = ids(&json!(hosts), Some("id"));
    let mut all = ids(was_retired, None);
    all.extend(ids(was, Some("id")));
    all.retain(|id| !now.contains(id));
    all.into_iter().collect()
}

async fn set(ctx: Arc<Ctx>, live: Arc<Registry>, input: Value) -> Result<Value, Error> {
    let hosts = checked(&input[HOSTS]).map_err(Error::Handler)?;
    let gone = retired(&ctx.kept(RETIRED).await, &ctx.kept(HOSTS).await, &hosts);
    ctx.keep(RETIRED, json!(gone))
        .await
        .map_err(Error::Handler)?;
    ctx.keep(HOSTS, json!(hosts))
        .await
        .map_err(Error::Handler)?;
    apply(&ctx, &live, &hosts);
    Ok(json!({ HOSTS: hosts }))
}

fn connect() -> iii_sdk::IIIClient {
    let url = std::env::var("III_URL").unwrap_or_else(|_| "ws://127.0.0.1:49160".into());
    let metadata = WorkerMetadata {
        name: WORKER.into(),
        ..WorkerMetadata::default()
    };
    let options = InitOptions {
        metadata: Some(metadata),
        namespace: Some("bastion".into()),
        ..InitOptions::default()
    };
    register_worker(&url, options)
}

/// The kept hosts, read again until the state worker answers, as it may
/// register a little after this worker starts.
async fn hosts_at_start(ctx: &Ctx) -> Value {
    let mut wait = Duration::from_millis(500);
    for _ in 0..30 {
        match ctx.read(HOSTS).await {
            Ok(hosts) => return hosts,
            Err(e) => eprintln!("could not read the hosts ({e}); trying again in {wait:?}"),
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(10));
    }
    Value::Null
}

#[tokio::main]
async fn main() {
    let ctx = Ctx::new(connect());
    let live: Arc<Registry> = Arc::default();
    let (c, l) = (ctx.clone(), live.clone());
    ctx.iii.register_function(
        "bastion::openai_compat::hosts::set",
        RegisterFunction::new_async(move |input: Value| set(c.clone(), l.clone(), input)),
    );
    let c = ctx.clone();
    ctx.iii.register_function(
        "bastion::openai_compat::hosts::list",
        RegisterFunction::new_async(move |_: Value| {
            let c = c.clone();
            async move {
                let (hosts, retired) = (c.kept(HOSTS).await, c.kept(RETIRED).await);
                Ok::<_, Error>(json!({ HOSTS: hosts, RETIRED: retired }))
            }
        }),
    );
    let kept = checked(&hosts_at_start(&ctx).await).unwrap_or_default();
    apply(&ctx, &live, &kept);
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
    fn hosts_are_trimmed_named_and_refused_when_they_cannot_be_providers() {
        let given =
            json!([{ "id": " ollama ", "api_url": "http://localhost:11434/v1/chat/completions" }]);
        let hosts = checked(&given).unwrap();
        assert_eq!(hosts[0]["id"], "ollama");
        assert_eq!(hosts[0]["name"], "ollama");
        let bad_id = json!([{ "id": "Ollama::x", "api_url": "http://h" }]);
        assert!(checked(&bad_id).is_err());
        let bad_url = json!([{ "id": "h", "api_url": "ftp://h" }]);
        assert!(checked(&bad_url).is_err());
        let twice =
            json!([{ "id": "h", "api_url": "http://a" }, { "id": "h", "api_url": "http://b" }]);
        assert!(checked(&twice).is_err());
        assert!(checked(&Value::Null).is_err());
    }

    #[test]
    fn a_removed_host_is_retired_until_it_is_added_again() {
        let was = json!([{ "id": "a" }, { "id": "b" }]);
        assert_eq!(
            retired(&json!(["c"]), &was, &[json!({ "id": "a" })]),
            ["b", "c"]
        );
        assert_eq!(
            retired(&json!(["b"]), &json!(null), &[json!({ "id": "b" })]),
            Vec::<String>::new()
        );
    }
}
