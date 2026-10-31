//! Commands and events about the engine link.

use std::sync::Arc;
use std::time::Duration;

use bastion_protocol::{EngineStatus, Link, event};
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use crate::engine::{DEFAULT_NAMESPACE, Engine};

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

#[tauri::command]
pub async fn engine_status(engine: State<'_, Arc<dyn Engine>>) -> Result<EngineStatus, ()> {
    Ok(status(engine.inner().as_ref()).await)
}

/// Emit the engine status to the page each time the link changes.
pub fn watch(app: AppHandle, engine: Arc<dyn Engine>) {
    tauri::async_runtime::spawn(async move {
        let mut last = None;
        loop {
            let link = engine.link();
            if last != Some(link) {
                last = Some(link);
                let _ = app.emit(event::ENGINE_CHANGED, status(engine.as_ref()).await);
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
        let status = tauri::async_runtime::block_on(status(&engine));
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
        let status = tauri::async_runtime::block_on(status(&engine));
        assert_eq!(status.functions, None);
        assert!(engine.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failed_call_still_reports_the_link() {
        let engine =
            FakeEngine::new(Link::Up).answer("engine::functions::list", Err("timeout".into()));
        let status = tauri::async_runtime::block_on(status(&engine));
        assert_eq!((status.link, status.functions), (Link::Up, None));
    }
}
