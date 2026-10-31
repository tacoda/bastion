//! The view's side of the Commands seam: typed calls and events over Tauri's
//! global API (PLAN.md D9, D10).

use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn invoke_js(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen)]
    async fn listen_js(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> JsValue;
}

/// Call a command. `E` is the command's typed error; a string error reads as one too.
pub async fn call<T: DeserializeOwned, E: DeserializeOwned>(
    cmd: &str,
    args: impl Serialize,
) -> Result<T, Failure<E>> {
    let args = serde_wasm_bindgen::to_value(&args).map_err(|e| Failure::Broken(e.to_string()))?;
    match invoke_js(cmd, args).await {
        Ok(value) => {
            serde_wasm_bindgen::from_value(value).map_err(|e| Failure::Broken(e.to_string()))
        }
        Err(value) => Err(serde_wasm_bindgen::from_value::<E>(value.clone())
            .map(Failure::Refused)
            .unwrap_or_else(|_| Failure::Broken(format!("{value:?}")))),
    }
}

/// Why a call did not return its value.
pub enum Failure<E> {
    /// The command answered with its typed error.
    Refused(E),
    /// The call itself failed: unknown command, or an answer that did not parse.
    Broken(String),
}

impl Failure<String> {
    pub fn text(self) -> String {
        match self {
            Failure::Refused(text) | Failure::Broken(text) => text,
        }
    }
}

/// Run `on_event` with each payload of `event`, for the life of the page.
pub fn listen<T: DeserializeOwned + 'static>(event: &'static str, on_event: impl Fn(T) + 'static) {
    leptos::task::spawn_local(async move {
        let handler = Closure::new(move |e: JsValue| {
            let payload = js_sys::Reflect::get(&e, &"payload".into()).unwrap_or(JsValue::NULL);
            if let Ok(value) = serde_wasm_bindgen::from_value(payload) {
                on_event(value);
            }
        });
        listen_js(event, &handler).await;
        handler.forget(); // the page lives as long as the window
    });
}

/// No arguments.
#[derive(Serialize)]
pub struct None {}
