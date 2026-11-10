//! The Engine seam on a real engine, through `iii-sdk`.

use bastion_protocol::Link;
use iii_sdk::iii::WorkerMetadata;
use iii_sdk::protocol::{RegisterTriggerInput, TriggerRequest};
use iii_sdk::runtime::IIIConnectionState;
use std::collections::HashMap;
use std::sync::Mutex;

use iii_sdk::channel::{ChannelReader, StreamChannelRef};
use iii_sdk::trigger::Trigger;
use iii_sdk::{Error, IIIClient, InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};

use super::{Bytes, Call, Engine, OnEvent, OnPush};

const FUNCTIONS_CHANGED: &str = "bastion::app::functions-changed";

/// The waits before each new try of a call that never reached the engine.
const BACKOFF_MS: [u64; 4] = [500, 1_000, 2_000, 4_000];

/// Whether a call failed before the engine got it, as while the link is
/// down, so trying it again cannot run it twice. A timed-out call may have
/// run, so it is not one.
fn never_sent(error: &str) -> bool {
    error == "iii is not connected" || error.starts_with("websocket error")
}

/// Make the call, and again after each wait in `BACKOFF_MS` while it
/// failed before the engine got it.
async fn retried<F, Fut>(mut call: F) -> Result<Value, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    for wait in BACKOFF_MS {
        match call().await {
            Err(e) if never_sent(&e) => {
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            }
            done => return done,
        }
    }
    call().await
}

pub struct IiiEngine {
    url: String,
    client: IIIClient,
    bindings: Mutex<HashMap<String, Trigger>>,
}

impl IiiEngine {
    pub fn connect(url: &str) -> Self {
        let options = InitOptions {
            metadata: Some(WorkerMetadata {
                name: format!("bastion-app-{}", std::process::id()),
                ..WorkerMetadata::default()
            }),
            namespace: Some("bastion".into()),
            ..InitOptions::default()
        };
        Self {
            url: url.into(),
            client: register_worker(url, options),
            bindings: Mutex::new(HashMap::new()),
        }
    }
}

impl Engine for IiiEngine {
    fn url(&self) -> &str {
        &self.url
    }

    fn link(&self) -> Link {
        match self.client.get_connection_state() {
            IIIConnectionState::Connecting => Link::Connecting,
            IIIConnectionState::Connected => Link::Up,
            IIIConnectionState::Reconnecting => Link::Reconnecting,
            IIIConnectionState::Disconnected | IIIConnectionState::Failed => Link::Down,
        }
    }

    fn call_for<'a>(
        &'a self,
        namespace: &'a str,
        function_id: &'a str,
        payload: Value,
        timeout_ms: u64,
    ) -> Call<'a> {
        Box::pin(retried(move || {
            let mut request = TriggerRequest::new(function_id, payload.clone());
            request.timeout_ms = Some(timeout_ms);
            async move {
                self.client
                    .trigger(request.namespace(namespace))
                    .await
                    .map_err(|e| e.to_string())
            }
        }))
    }

    // `engine::functions-available` is a trigger type, and `observe` only
    // watches functions, so this binds it the deprecated way.
    #[allow(deprecated)]
    fn on_functions_changed(&self, on_push: OnPush) -> Result<(), String> {
        self.client.register_function(
            FUNCTIONS_CHANGED,
            RegisterFunction::new_async(move |_: Value| {
                on_push();
                async { Ok::<_, Error>(json!({})) }
            }),
        );
        self.client
            .register_trigger(RegisterTriggerInput::new(
                "engine::functions-available",
                FUNCTIONS_CHANGED,
                json!({}),
            ))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    // Trigger types such as `harness::turn-completed` and the `pre-trigger`
    // hook have no `observe` form yet, so this binds them the deprecated way.
    #[allow(deprecated)]
    fn bind(&self, trigger_type: &str, config: Value, on_event: OnEvent) -> Result<String, String> {
        let id = format!("bastion::app::on::{}", uuid::Uuid::new_v4().simple());
        self.client.register_function(
            &id,
            RegisterFunction::new_async(move |event: Value| {
                let answer = on_event(event);
                async move { Ok::<_, Error>(answer) }
            }),
        );
        let trigger = self
            .client
            .register_trigger(RegisterTriggerInput::new(trigger_type, &id, config))
            .map_err(|e| e.to_string())?;
        self.bindings.lock().unwrap().insert(id.clone(), trigger);
        Ok(id)
    }

    fn unbind(&self, id: &str) {
        if let Some(trigger) = self.bindings.lock().unwrap().remove(id) {
            trigger.unregister();
        }
    }

    fn read_channel(&self, content: Value) -> Bytes<'_> {
        Box::pin(async move {
            let channel: StreamChannelRef =
                serde_json::from_value(content).map_err(|e| format!("not a channel: {e}"))?;
            ChannelReader::new(&self.url, &channel)
                .read_all()
                .await
                .map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A call that fails with `error` the first `fails` times.
    async fn failing(tries: &AtomicU32, fails: u32, error: &str) -> Result<Value, String> {
        if tries.fetch_add(1, Ordering::SeqCst) < fails {
            Err(error.into())
        } else {
            Ok(json!({ "ok": true }))
        }
    }

    #[test]
    fn a_call_that_never_reached_the_engine_is_tried_again() {
        let tries = AtomicU32::new(0);
        let out = crate::rt::block_on(retried(|| failing(&tries, 1, "iii is not connected")));
        assert_eq!(out, Ok(json!({ "ok": true })));
        assert_eq!(tries.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_timed_out_call_is_not_tried_again() {
        let tries = AtomicU32::new(0);
        let out = crate::rt::block_on(retried(|| failing(&tries, 1, "invocation timed out")));
        assert_eq!(out, Err("invocation timed out".into()));
        assert_eq!(tries.load(Ordering::SeqCst), 1);
    }
}
