//! Bastion's first page: the engine link, read through a command and kept
//! current by the `engine-changed` event (PLAN.md D9, D10).

use bastion_protocol::{EngineStatus, Link, command, event};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"])]
    async fn listen(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> JsValue;
}

fn describe(status: &EngineStatus) -> String {
    match (status.link, status.functions) {
        (Link::Up, Some(n)) => format!("up at {}, {n} functions", status.url),
        (Link::Up, None) => format!("up at {}", status.url),
        (Link::Connecting, _) => format!("connecting to {}", status.url),
        (Link::Reconnecting, _) => format!("reconnecting to {}", status.url),
        (Link::Down, _) => format!("down: {} does not answer", status.url),
    }
}

#[component]
pub fn App() -> impl IntoView {
    let (engine, set_engine) = signal(String::from("…"));
    spawn_local(async move {
        let text = match invoke(command::ENGINE_STATUS, JsValue::NULL).await {
            Ok(value) => serde_wasm_bindgen::from_value::<EngineStatus>(value)
                .map(|status| describe(&status))
                .unwrap_or_else(|e| format!("unreadable status: {e}")),
            Err(e) => format!("{e:?}"),
        };
        set_engine.set(text);
    });
    spawn_local(async move {
        let handler = Closure::new(move |event: JsValue| {
            let payload = js_sys::Reflect::get(&event, &"payload".into()).unwrap_or(JsValue::NULL);
            if let Ok(status) = serde_wasm_bindgen::from_value::<EngineStatus>(payload) {
                set_engine.set(describe(&status));
            }
        });
        listen(event::ENGINE_CHANGED, &handler).await;
        handler.forget(); // the page lives as long as the window
    });

    view! {
        <main class="min-h-screen p-8 grid gap-4 content-start">
            <h1 class="m-0 text-2xl font-extrabold">"Bastion"</h1>
            <section class="rounded-xl border border-line bg-panel p-4">
                <h2 class="m-0 text-sm font-semibold text-muted">"Engine"</h2>
                <p data-testid="engine">{engine}</p>
            </section>
        </main>
    }
}
