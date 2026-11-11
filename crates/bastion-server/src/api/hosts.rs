//! OpenAI-compatible hosts. The `bastion-openai-compat` worker
//! keeps the list and makes each host a router provider; a host's key is
//! set like any provider's, in `providers`.

use std::collections::BTreeSet;

use bastion_protocol::HostView;
use serde_json::{Value, json};

use super::providers;
use crate::engine::Engine;
use crate::engine::NS;

const LIST: &str = "bastion::openai_compat::hosts::list";
const SET: &str = "bastion::openai_compat::hosts::set";

fn view(h: &Value) -> HostView {
    let text = |k: &str| h[k].as_str().unwrap_or_default().to_string();
    HostView {
        id: text("id"),
        name: text("name"),
        api_url: text("api_url"),
    }
}

/// The hosts the admin listed.
pub async fn hosts(engine: &dyn Engine) -> Result<Vec<HostView>, String> {
    let out = engine.call(NS, LIST, json!({})).await?;
    let listed = out["hosts"].as_array().into_iter().flatten();
    Ok(listed.map(view).collect())
}

/// The ids in `listed[field]`, as objects with an id or as plain ids.
fn ids(listed: &Value, field: &str) -> BTreeSet<String> {
    let all = listed[field].as_array().into_iter().flatten();
    all.filter_map(|v| v["id"].as_str().or(v.as_str()).map(String::from))
        .collect()
}

/// The ids of removed hosts, or none when the worker does not answer.
pub(super) async fn retired(engine: &dyn Engine) -> BTreeSet<String> {
    let listed = engine.call(NS, LIST, json!({})).await;
    listed.map(|l| ids(&l, "retired")).unwrap_or_default()
}

/// Keep `wanted` as the hosts. A new host may not take the id of a
/// provider the router already has, unless it was a host here, and a
/// removed host's key goes with it.
pub async fn hosts_set(engine: &dyn Engine, wanted: Vec<HostView>) -> Result<(), String> {
    let listed = engine.call(NS, LIST, json!({})).await?;
    let current = ids(&listed, "hosts");
    let ours: BTreeSet<String> = current.union(&ids(&listed, "retired")).cloned().collect();
    let taken = providers::providers(engine).await?;
    if let Some(h) = wanted
        .iter()
        .find(|h| !ours.contains(&h.id) && taken.iter().any(|p| p.id == h.id))
    {
        return Err(format!("{} is already a model provider", h.id));
    }
    let gone: Vec<&String> = current
        .iter()
        .filter(|id| !wanted.iter().any(|h| &h.id == *id))
        .collect();
    for id in gone {
        // Best effort: a host that never had a key has none to remove.
        let _ = providers::key_remove(engine, id.clone()).await;
    }
    let set = json!({ "hosts": wanted.iter().map(|h| json!({
        "id": h.id, "name": h.name, "api_url": h.api_url,
    })).collect::<Vec<_>>() });
    engine.call(NS, SET, set).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeEngine;
    use bastion_protocol::Link;

    fn host(id: &str) -> HostView {
        HostView {
            id: id.into(),
            name: id.into(),
            api_url: "http://localhost:11434/v1/chat/completions".into(),
        }
    }

    fn engine() -> FakeEngine {
        FakeEngine::new(Link::Up)
            .answer(LIST, Ok(json!({ "hosts": [{ "id": "ollama", "name": "Ollama", "api_url": "http://h" }], "retired": ["fake2"] })))
            .answer(
                "router::provider::list",
                Ok(json!({ "providers": [
                    { "id": "deepseek", "display_name": "DeepSeek", "credential_source": "none" },
                    { "id": "ollama", "display_name": "Ollama", "credential_source": "none", "configured": true },
                    { "id": "fake2", "display_name": "Fake", "credential_source": "none" },
                ]})),
            )
            .answer(SET, Ok(json!({})))
            .answer("configuration::get", Ok(json!({ "value": {} })))
            .answer("configuration::set", Ok(json!({})))
            .answer("secrets::delete", Ok(json!({})))
    }

    #[test]
    fn a_host_cannot_take_a_providers_id_but_keeps_its_own_or_a_retired_one() {
        let engine = engine();
        let taken = crate::rt::block_on(hosts_set(&engine, vec![host("ollama"), host("deepseek")]));
        assert_eq!(taken, Err("deepseek is already a model provider".into()));
        assert!(!engine.called(SET));
        crate::rt::block_on(hosts_set(
            &engine,
            vec![host("ollama"), host("together"), host("fake2")],
        ))
        .unwrap();
        assert!(engine.called(SET));
        assert!(!engine.called("secrets::delete"));
    }

    #[test]
    fn a_removed_host_loses_its_key() {
        let engine = engine();
        crate::rt::block_on(hosts_set(&engine, vec![])).unwrap();
        assert!(engine.called("secrets::delete"));
        let listed = crate::rt::block_on(hosts(&engine)).unwrap();
        assert_eq!(listed[0].name, "Ollama");
    }

    #[test]
    fn a_host_that_needs_no_key_is_ready_and_a_provider_without_one_is_not() {
        let all = crate::rt::block_on(providers::providers(&engine())).unwrap();
        let ready = |id: &str| all.iter().find(|p| p.id == id).unwrap().ready;
        assert!(ready("ollama"));
        assert!(!ready("deepseek"));
        assert!(
            all.iter().all(|p| p.id != "fake2"),
            "a retired host is not listed"
        );
    }
}
