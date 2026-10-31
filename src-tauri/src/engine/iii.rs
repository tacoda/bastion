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

pub struct IiiEngine {
    url: String,
    client: IIIClient,
    bindings: Mutex<HashMap<String, Trigger>>,
}

impl IiiEngine {
    pub fn connect(url: &str) -> Self {
        let options = InitOptions {
            metadata: Some(WorkerMetadata {
                name: "bastion-app".into(),
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
        Box::pin(async move {
            let mut request = TriggerRequest::new(function_id, payload);
            request.timeout_ms = Some(timeout_ms);
            self.client
                .trigger(request.namespace(namespace))
                .await
                .map_err(|e| e.to_string())
        })
    }

    // `engine::functions-available` is a trigger type, and `observe` only
    // watches functions, so this binds it the deprecated way (as N3 did).
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
