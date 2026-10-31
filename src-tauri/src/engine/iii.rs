//! The Engine seam on a real engine, through `iii-sdk`.

use bastion_protocol::Link;
use iii_sdk::iii::WorkerMetadata;
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::runtime::IIIConnectionState;
use iii_sdk::{IIIClient, InitOptions, register_worker};
use serde_json::Value;

use super::{Call, Engine};

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
}
