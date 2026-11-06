//! Commands and events about the engine link.

use std::sync::Arc;
use std::time::Duration;

use bastion_protocol::{Change, EngineStatus, Link};
use serde_json::json;

use crate::engine::{DEFAULT_NAMESPACE, Engine};
use crate::events::Bus;

/// How often the watcher reads the link. The SDK offers no listener for it.
// ponytail: a local read every half second; switch to a listener if iii-sdk adds one.
const LINK_POLL: Duration = Duration::from_millis(500);

pub async fn status(engine: &dyn Engine) -> EngineStatus {
    let link = engine.link();
    let functions = match link {
        Link::Up => engine
            .call(DEFAULT_NAMESPACE, "engine::functions::list", json!({}))
            .await
            .ok()
            .and_then(|out| out["functions"].as_array().map(Vec::len)),
        _ => None,
    };
    EngineStatus {
        url: engine.url().into(),
        link,
        functions,
    }
}

pub async fn engine_status(engine: &Arc<dyn Engine>) -> Result<EngineStatus, ()> {
    Ok(status(engine.as_ref()).await)
}

/// Publish the engine status each time the link changes.
pub fn watch(events: Arc<Bus>, engine: Arc<dyn Engine>) {
    crate::rt::spawn(async move {
        let mut last = None;
        loop {
            let link = engine.link();
            if last != Some(link) {
                last = Some(link);
                let status = status(engine.as_ref()).await;
                events.publish(Change::Engine { status });
            }
            tokio::time::sleep(LINK_POLL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeEngine;

    #[test]
    fn up_link_counts_the_engine_functions() {
        let engine = FakeEngine::new(Link::Up).answer(
            "engine::functions::list",
            Ok(json!({ "functions": [{}, {}, {}] })),
        );
        let status = crate::rt::block_on(status(&engine));
        assert_eq!(
            status,
            EngineStatus {
                url: "ws://fake".into(),
                link: Link::Up,
                functions: Some(3)
            }
        );
        assert_eq!(engine.calls.lock().unwrap()[0].0, DEFAULT_NAMESPACE);
    }

    #[test]
    fn down_link_makes_no_call() {
        let engine = FakeEngine::new(Link::Down);
        let status = crate::rt::block_on(status(&engine));
        assert_eq!(status.functions, None);
        assert!(engine.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failed_call_still_reports_the_link() {
        let engine =
            FakeEngine::new(Link::Up).answer("engine::functions::list", Err("timeout".into()));
        let status = crate::rt::block_on(status(&engine));
        assert_eq!((status.link, status.functions), (Link::Up, None));
    }
}
