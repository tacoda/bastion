//! The Engine seam on a real engine, through `iii-sdk`.

use bastion_protocol::Link;
use iii_sdk::iii::WorkerMetadata;
use iii_sdk::protocol::{RegisterTriggerInput, TriggerRequest};
use iii_sdk::runtime::IIIConnectionState;
use iii_sdk::{Error, IIIClient, InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};

use super::{Call, Engine, OnPush};

const FUNCTIONS_CHANGED: &str = "bastion::app::functions-changed";

const CALL_TIMEOUT_MS: u64 = 5_000;

pub struct IiiEngine {
    url: String,
    client: IIIClient,
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

    fn call<'a>(&'a self, namespace: &'a str, function_id: &'a str, payload: Value) -> Call<'a> {
        Box::pin(async move {
            let mut request = TriggerRequest::new(function_id, payload);
            request.timeout_ms = Some(CALL_TIMEOUT_MS);
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
}
