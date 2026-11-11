//! Model provider keys in configuration, not the environment (PLAN.md D34).
//! A key goes to the stack's `secrets` vault, readable only by
//! `llm-router`, and the router's entry names it as `secret://NAME`. No
//! function here returns a key.

use bastion_protocol::ProviderView;
use serde_json::{Map, Value, json};

use super::sign_in::with_sign_in;
use crate::engine::Engine;
use crate::engine::NS;

/// The router's configuration entry, named for the stack's project.
// ponytail: the project is always `bastion`, like the namespace; read it from the stack if a second project needs keys.
const ROUTER_ENTRY: &str = "bastion-llm-router";
/// The worker allowed to read a provider key.
const READER: &str = "llm-router";

/// Every provider the router knows, and where its key comes from. A
/// removed OpenAI-compatible host is left out: the router cannot drop it.
pub async fn providers(engine: &dyn Engine) -> Result<Vec<ProviderView>, String> {
    let out = engine.call(NS, "router::provider::list", json!({})).await?;
    let retired = super::hosts::retired(engine).await;
    let listed = out["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| !retired.contains(p["id"].as_str().unwrap_or_default()))
        .map(|p| ProviderView {
            id: text(&p["id"]),
            name: p["display_name"]
                .as_str()
                .map_or_else(|| text(&p["id"]), Into::into),
            source: p["credential_source"].as_str().unwrap_or("none").into(),
            ready: p["configured"] == true,
        });
    let mut all = Vec::new();
    for p in listed {
        all.push(with_sign_in(engine, p).await);
    }
    Ok(all)
}

/// Keep `key` for provider `id` in the vault, and point the router at it.
pub async fn key_set(engine: &dyn Engine, id: String, key: String) -> Result<(), String> {
    let name = known(engine, &id).await?;
    let key = key.trim();
    if key.is_empty() {
        return Err("a key cannot be empty".into());
    }
    let stored = json!({ "name": name, "value": key, "store": "vault", "consumers": [READER] });
    engine.call(NS, "secrets::set", stored).await?;
    point(engine, &id, Some(format!("secret://{name}"))).await?;
    refresh(engine, &id).await;
    Ok(())
}

/// Forget provider `id`'s key: the router stops naming it, then the vault
/// drops it.
pub async fn key_remove(engine: &dyn Engine, id: String) -> Result<(), String> {
    let name = known(engine, &id).await?;
    point(engine, &id, None).await?;
    engine
        .call(NS, "secrets::delete", json!({ "name": name }))
        .await?;
    refresh(engine, &id).await;
    Ok(())
}

/// Ask the provider to read its models again, as it reads them only at
/// start. Best effort: the key is already in place either way.
async fn refresh(engine: &dyn Engine, id: &str) {
    let function = format!("provider::{id}::refresh_models");
    if let Err(e) = engine.call(NS, &function, json!({})).await {
        eprintln!("{function}: {e}");
    }
}

/// The router's entry with provider `id`'s key set to `reference`, or
/// removed. Everything else in it is kept.
pub fn with_key(entry: Value, id: &str, reference: Option<String>) -> Value {
    let mut entry = match entry {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let providers = object(entry.entry("providers"));
    let provider = object(providers.entry(id));
    match reference {
        Some(r) => {
            provider.insert("api_key".into(), Value::String(r));
        }
        None => {
            provider.remove("api_key");
        }
    }
    Value::Object(entry)
}

fn object(slot: serde_json::map::Entry<'_>) -> &mut Map<String, Value> {
    let value = slot.or_insert_with(|| json!({}));
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("just made an object")
}

/// One write to the router's entry at a time, as each replaces it whole.
// ponytail: one lock per server process; two servers on one stack can still race.
static ENTRY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Read the router's entry, set or clear one provider's key, write it back.
/// `configuration::set` replaces the whole value.
async fn point(engine: &dyn Engine, id: &str, reference: Option<String>) -> Result<(), String> {
    let _entry = ENTRY.lock().await;
    let get = json!({ "id": ROUTER_ENTRY, "raw": true });
    let current = engine.call("default", "configuration::get", get).await?;
    let value = with_key(current["value"].clone(), id, reference);
    let set = json!({ "id": ROUTER_ENTRY, "value": value });
    engine
        .call("default", "configuration::set", set)
        .await
        .map(|_| ())
}

/// The vault name of provider `id`'s key (`deepseek` keeps
/// `DEEPSEEK_API_KEY`), or a refusal for a provider the router does not
/// know, so a typo makes no secret.
async fn known(engine: &dyn Engine, id: &str) -> Result<String, String> {
    let all = providers(engine).await?;
    let Some(p) = all.iter().find(|p| p.id == id) else {
        return Err(format!("no model provider {id}"));
    };
    if p.source == "signed_in" || id == "openai-codex" {
        return Err(format!(
            "{} signs in with a subscription, not a key",
            p.name
        ));
    }
    Ok(format!("{}_API_KEY", id.to_uppercase().replace('-', "_")))
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeEngine;
    use bastion_protocol::Link;

    fn engine() -> FakeEngine {
        FakeEngine::new(Link::Up)
            .answer(
                "router::provider::list",
                Ok(json!({ "providers": [
                    { "id": "deepseek", "display_name": "DeepSeek", "credential_source": "none" },
                ]})),
            )
            .answer(
                "configuration::get",
                Ok(json!({ "value": { "providers": { "anthropic": { "api_key": "secret://ANTHROPIC_API_KEY" } } } })),
            )
            .answer("configuration::set", Ok(json!({})))
            .answer("secrets::set", Ok(json!({})))
            .answer("secrets::delete", Ok(json!({})))
    }

    /// The payload of each call the engine got, by function id.
    fn sent(engine: &FakeEngine) -> std::collections::HashMap<String, Value> {
        let calls = engine.calls.lock().unwrap();
        calls.iter().map(|c| (c.1.clone(), c.2.clone())).collect()
    }

    #[test]
    fn a_key_goes_to_the_vault_for_the_router_only_and_the_entry_keeps_the_rest() {
        let engine = engine();
        crate::rt::block_on(key_set(&engine, "deepseek".into(), " sk-1 \n".into())).unwrap();
        let stored = sent(&engine)["secrets::set"].clone();
        assert_eq!(stored["name"], "DEEPSEEK_API_KEY");
        assert_eq!(stored["value"], "sk-1");
        assert_eq!(stored["consumers"], json!(["llm-router"]));
        let providers = &sent(&engine)["configuration::set"]["value"]["providers"];
        assert_eq!(
            providers["deepseek"]["api_key"],
            "secret://DEEPSEEK_API_KEY"
        );
        assert_eq!(
            providers["anthropic"]["api_key"],
            "secret://ANTHROPIC_API_KEY"
        );
        assert!(engine.called("provider::deepseek::refresh_models"));
    }

    #[test]
    fn an_unknown_provider_or_an_empty_key_stores_nothing() {
        let engine = engine();
        let typo = crate::rt::block_on(key_set(&engine, "deepsek".into(), "sk".into()));
        assert_eq!(typo, Err("no model provider deepsek".into()));
        let empty = crate::rt::block_on(key_set(&engine, "deepseek".into(), "  ".into()));
        assert!(empty.is_err());
        assert!(!engine.called("secrets::set"));
    }

    #[test]
    fn removing_a_key_unpoints_the_router_then_deletes_the_secret() {
        let engine = engine();
        crate::rt::block_on(key_remove(&engine, "deepseek".into())).unwrap();
        assert_eq!(sent(&engine)["secrets::delete"]["name"], "DEEPSEEK_API_KEY");
        let entry = sent(&engine)["configuration::set"]["value"].clone();
        assert_eq!(entry["providers"]["deepseek"], json!({}));
        let order: Vec<String> = engine
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.1.clone())
            .collect();
        let at = |f: &str| order.iter().position(|c| c == f).unwrap();
        assert!(
            at("configuration::set") < at("secrets::delete"),
            "{order:?}"
        );
    }

    #[test]
    fn an_empty_entry_gets_its_providers() {
        let entry = with_key(
            Value::Null,
            "openai",
            Some("secret://OPENAI_API_KEY".into()),
        );
        assert_eq!(
            entry,
            json!({ "providers": { "openai": { "api_key": "secret://OPENAI_API_KEY" } } })
        );
    }
}
