//! Spike N12 (PLAN.md S0), temporary: the page calls a Bastion server over
//! HTTP and follows its events with `fetch`, so the token goes in a header,
//! and reconnects with `Last-Event-ID`. Removed when S3 builds the real client.

use js_sys::{Promise, Reflect, Uint8Array};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, ReadableStreamDefaultReader, Request, RequestInit, Response};

const BASE: &str = "http://127.0.0.1:49180";
const TOKEN: &str = "Bearer spike-n12";

#[component]
pub fn SpikeN12() -> impl IntoView {
    let (log, set_log) = signal(Vec::<String>::new());
    let say = move |line: String| {
        set_log.update(|l| {
            l.push(line);
            let excess = l.len().saturating_sub(8);
            l.drain(..excess);
        })
    };
    spawn_local(async move {
        match get(&format!("{BASE}/ping"), None).await {
            Ok(r) => say(format!("ping: {}", text(r).await)),
            Err(e) => say(format!("ping failed: {e:?}")),
        }
        let mut last: Option<String> = None;
        let mut connects = 0;
        loop {
            connects += 1;
            say(format!("connect {connects}, last id {last:?}"));
            if let Err(e) = follow(&mut last, say).await {
                say(format!("stream ended: {e:?}"));
            }
            sleep(1000).await;
        }
    });
    view! {
        <pre data-spike="n12" class="m-0 max-h-40 overflow-auto bg-panel p-2 text-xs">
            {move || log.get().join("\n")}
        </pre>
    }
}

/// Read `/events` until it ends, keeping the last id seen.
async fn follow(last: &mut Option<String>, say: impl Fn(String)) -> Result<(), JsValue> {
    let resp = get(&format!("{BASE}/events"), last.as_deref()).await?;
    if !resp.ok() {
        return Err(JsValue::from_str(&format!("status {}", resp.status())));
    }
    let body = resp.body().ok_or("no body")?;
    let reader: ReadableStreamDefaultReader = body.get_reader().unchecked_into();
    let mut buffer = String::new();
    loop {
        let chunk = JsFuture::from(reader.read()).await?;
        if Reflect::get(&chunk, &"done".into())?.as_bool() == Some(true) {
            return Ok(());
        }
        let bytes = Uint8Array::new(&Reflect::get(&chunk, &"value".into())?).to_vec();
        buffer.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(end) = buffer.find("\n\n") {
            let event: String = buffer.drain(..end + 2).collect();
            for line in event.lines() {
                if let Some(id) = line.strip_prefix("id: ") {
                    *last = Some(id.to_string());
                }
                if let Some(data) = line.strip_prefix("data: ") {
                    say(format!("event {}: {data}", last.as_deref().unwrap_or("?")));
                }
            }
        }
    }
}

async fn get(url: &str, last_id: Option<&str>) -> Result<Response, JsValue> {
    let headers = Headers::new()?;
    headers.set("Authorization", TOKEN)?;
    if let Some(id) = last_id {
        headers.set("Last-Event-ID", id)?;
    }
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_headers(&headers);
    let request = Request::new_with_str_and_init(url, &init)?;
    let window = web_sys::window().ok_or("no window")?;
    Ok(JsFuture::from(window.fetch_with_request(&request))
        .await?
        .unchecked_into())
}

async fn text(resp: Response) -> String {
    match resp.text() {
        Ok(p) => JsFuture::from(p)
            .await
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default(),
        Err(e) => format!("{e:?}"),
    }
}

async fn sleep(ms: i32) {
    let wait = Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(wait).await;
}
