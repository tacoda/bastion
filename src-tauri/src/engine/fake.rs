//! An in-memory engine for tests: canned answers by function id, and a record
//! of every call.

use std::collections::HashMap;
use std::sync::Mutex;

use bastion_protocol::Link;
use serde_json::Value;

use super::{Call, Engine, OnPush};

pub struct FakeEngine {
    pub link: Mutex<Link>,
    answers: HashMap<String, Result<Value, String>>,
    pub calls: Mutex<Vec<(String, String, Value)>>,
    pub on_functions_changed: Mutex<Option<OnPush>>,
}

impl FakeEngine {
    pub fn new(link: Link) -> Self {
        Self {
            link: Mutex::new(link),
            answers: HashMap::new(),
            calls: Mutex::new(Vec::new()),
            on_functions_changed: Mutex::new(None),
        }
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

    fn call<'a>(&'a self, namespace: &'a str, function_id: &'a str, payload: Value) -> Call<'a> {
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
}
