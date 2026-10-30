//! Bastion's first page (PLAN.md A0, N6): the engine's state through a command,
//! and each push from the core as it arrives.

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"])]
    async fn listen(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> JsValue;
}

#[derive(Serialize)]
struct Seen {
    event: String,
}

#[component]
pub fn App() -> impl IntoView {
    let (engine, set_engine) = signal(String::from("…"));
    let (last, set_last) = signal(String::from("nothing yet"));
    spawn_local(async move {
        let answer = invoke("engine_status", JsValue::NULL).await;
        set_engine.set(answer.as_string().unwrap_or_else(|| format!("{answer:?}")));
    });
    spawn_local(async move {
        let handler = Closure::new(move |event: JsValue| {
            let payload = js_sys::Reflect::get(&event, &"payload".into()).unwrap_or(JsValue::NULL);
            let text = js_sys::JSON::stringify(&payload).map(String::from).unwrap_or_default();
            set_last.set(text.clone());
            spawn_local(async move {
                let args = serde_wasm_bindgen::to_value(&Seen { event: text }).unwrap();
                invoke("seen", args).await;
            });
        });
        listen("changed", &handler).await;
        handler.forget(); // the page lives as long as the window
    });

    view! {
        <main class="min-h-screen p-8 grid gap-4 content-start">
            <h1 class="m-0 text-2xl font-extrabold">"Bastion"</h1>
            <section class="rounded-xl border border-line bg-panel p-4">
                <h2 class="m-0 text-sm font-semibold text-muted">"Engine"</h2>
                <p data-testid="engine">{engine}</p>
            </section>
            <section class="rounded-xl border border-line bg-panel p-4">
                <h2 class="m-0 text-sm font-semibold text-muted">"Last push from the engine"</h2>
                <p data-testid="last"><code class="text-link">{last}</code></p>
            </section>
            <button class="w-fit rounded-lg bg-primary px-4 py-2 font-semibold text-white hover:bg-primary-hover">
                "Purple"
            </button>
        </main>
    }
}
