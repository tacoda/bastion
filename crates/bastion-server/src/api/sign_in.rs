//! Model providers that sign in with a subscription instead of a key (D34).
//! OpenAI Codex signs in with a ChatGPT plan by a device code, through its
//! worker. Claude takes a key only (D36).

use bastion_protocol::{ProviderView, SignIn};
use serde_json::json;

use crate::engine::Engine;
use crate::engine::NS;

/// Whether OpenAI Codex is signed in. `llm-router` reports it as having no
/// credential, so its worker is asked. A provider that takes a key is left
/// as the router reported it.
pub async fn with_sign_in(engine: &dyn Engine, mut p: ProviderView) -> ProviderView {
    if p.id != "openai-codex" {
        return p;
    }
    let status = engine.call(NS, "provider::openai-codex::auth::status", json!({}));
    let signed_in = status.await.is_ok_and(|v| v["status"] == "authenticated");
    p.source = if signed_in { "signed_in" } else { "none" }.into();
    p.ready = signed_in;
    p
}

/// Start a ChatGPT sign-in for OpenAI Codex.
pub async fn codex_sign_in(engine: &dyn Engine) -> Result<SignIn, String> {
    let started = engine
        .call(NS, "provider::openai-codex::login::start", json!({}))
        .await?;
    serde_json::from_value(started).map_err(|e| format!("the Codex sign-in answered oddly: {e}"))
}

/// How sign-in `login_id` stands. Once it is `ok`, the worker reads its
/// models again.
pub async fn codex_sign_in_poll(engine: &dyn Engine, login_id: String) -> Result<String, String> {
    let polled = engine
        .call(
            NS,
            "provider::openai-codex::login::poll",
            json!({ "login_id": login_id }),
        )
        .await?;
    let status = polled["status"].as_str().unwrap_or("error").to_string();
    if status == "ok" {
        engine
            .call(NS, "provider::openai-codex::refresh_models", json!({}))
            .await?;
    }
    Ok(status)
}

pub async fn codex_sign_out(engine: &dyn Engine) -> Result<(), String> {
    engine
        .call(NS, "provider::openai-codex::auth::logout", json!({}))
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeEngine;
    use bastion_protocol::Link;

    fn view(id: &str) -> ProviderView {
        ProviderView {
            id: id.into(),
            name: id.into(),
            source: "none".into(),
            ready: false,
        }
    }

    #[test]
    fn codex_is_asked_and_a_key_provider_is_left_alone() {
        let engine = FakeEngine::new(Link::Up).answer(
            "provider::openai-codex::auth::status",
            Ok(json!({ "status": "authenticated" })),
        );
        let codex = crate::rt::block_on(with_sign_in(&engine, view("openai-codex")));
        assert_eq!(codex.source, "signed_in");
        let mut deepseek = view("deepseek");
        deepseek.source = "secret".into();
        let kept = crate::rt::block_on(with_sign_in(&engine, deepseek.clone()));
        assert_eq!(kept, deepseek);
    }

    #[test]
    fn a_finished_sign_in_reads_the_models_again() {
        let engine = FakeEngine::new(Link::Up)
            .answer(
                "provider::openai-codex::login::poll",
                Ok(json!({ "status": "ok" })),
            )
            .answer(
                "provider::openai-codex::refresh_models",
                Ok(json!({ "count": 3 })),
            );
        let status = crate::rt::block_on(codex_sign_in_poll(&engine, "l1".into()));
        assert_eq!(status, Ok("ok".into()));
        assert!(engine.called("provider::openai-codex::refresh_models"));
    }
}
