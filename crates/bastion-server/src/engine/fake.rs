//! An in-memory engine for tests: canned answers by function id, and a record
//! of every call.

use std::collections::HashMap;
use std::sync::Mutex;

use bastion_protocol::Link;
use serde_json::Value;

use super::{Bytes, Call, Engine, OnEvent, OnPush};

pub struct FakeEngine {
    pub link: Mutex<Link>,
    answers: HashMap<String, Result<Value, String>>,
    pub calls: Mutex<Vec<(String, String, Value)>>,
    pub on_functions_changed: Mutex<Option<OnPush>>,
    /// Bindings by id: trigger type, config, handler.
    pub bindings: Mutex<HashMap<String, (String, Value, OnEvent)>>,
    pub channel_bytes: Vec<u8>,
}

impl FakeEngine {
    pub fn new(link: Link) -> Self {
        Self {
            link: Mutex::new(link),
            answers: HashMap::new(),
            calls: Mutex::new(Vec::new()),
            on_functions_changed: Mutex::new(None),
            bindings: Mutex::new(HashMap::new()),
            channel_bytes: Vec::new(),
        }
    }

    /// Fire every binding of `trigger_type` whose config shares `key`'s value
    /// with `event`, as the engine would. Returns the handlers' answers.
    pub fn fire(&self, trigger_type: &str, key: &str, event: Value) -> Vec<Value> {
        let handlers: Vec<OnEvent> = self
            .bindings
            .lock()
            .unwrap()
            .values()
            .filter(|(t, config, _)| {
                t == trigger_type && (config[key].is_null() || config[key] == event[key])
            })
            .map(|(_, _, h)| h.clone())
            .collect();
        handlers.into_iter().map(|h| h(event.clone())).collect()
    }

    /// Whether `function_id` was called.
    pub fn called(&self, function_id: &str) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.1 == function_id)
    }

    pub fn answer(mut self, function_id: &str, answer: Result<Value, String>) -> Self {
        self.answers.insert(function_id.into(), answer);
        self
    }
}

impl Engine for FakeEngine {
    fn url(&self) -> &str {
        "ws://fake"
    }

    fn link(&self) -> Link {
        *self.link.lock().unwrap()
    }

    fn call_for<'a>(
        &'a self,
        namespace: &'a str,
        function_id: &'a str,
        payload: Value,
        _timeout_ms: u64,
    ) -> Call<'a> {
        self.calls
            .lock()
            .unwrap()
            .push((namespace.into(), function_id.into(), payload));
        let answer = self
            .answers
            .get(function_id)
            .cloned()
            .unwrap_or_else(|| Err(format!("function_not_found: {function_id}")));
        Box::pin(async move { answer })
    }

    fn on_functions_changed(&self, on_push: OnPush) -> Result<(), String> {
        *self.on_functions_changed.lock().unwrap() = Some(on_push);
        Ok(())
    }

    fn bind(&self, trigger_type: &str, config: Value, on_event: OnEvent) -> Result<String, String> {
        let mut bindings = self.bindings.lock().unwrap();
        let id = format!("binding-{}", bindings.len());
        bindings.insert(id.clone(), (trigger_type.into(), config, on_event));
        Ok(id)
    }

    fn unbind(&self, id: &str) {
        self.bindings.lock().unwrap().remove(id);
    }

    fn read_channel(&self, _content: Value) -> Bytes<'_> {
        let bytes = self.channel_bytes.clone();
        Box::pin(async move { Ok(bytes) })
    }
}
