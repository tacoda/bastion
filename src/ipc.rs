//! The view's side of the API (PLAN.md D28, D29). A server command goes to
//! the window's server over HTTP, by the route table below; the window's own
//! commands (the profile, quit, the target) still go to Tauri. Server events
//! arrive on one `GET /events` stream and reach each page by the same event
//! names as before.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bastion_protocol::routes::{self, Body};
use bastion_protocol::{Change, EngineStatus, LOCAL, Link, Server, command, event};
use js_sys::{Promise, Reflect, Uint8Array};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, ReadableStreamDefaultReader, Request, RequestInit, Response};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn invoke_js(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen)]
    async fn listen_js(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> JsValue;
}

/// Call a command on the current Bastion server. `E` is the command's typed
/// error; a string error reads as one too.
pub async fn call<T: DeserializeOwned, E: DeserializeOwned>(
    cmd: &str,
    args: impl Serialize,
) -> Result<T, Failure<E>> {
    call_on(&current(), cmd, args).await
}

/// Call a command on the Bastion server named `on`.
pub async fn call_on<T: DeserializeOwned, E: DeserializeOwned>(
    on: &str,
    cmd: &str,
    args: impl Serialize,
) -> Result<T, Failure<E>> {
    let Some((method, path, body)) = routes::route(cmd) else {
        return invoke(cmd, args).await;
    };
    let mut args = match serde_json::to_value(&args) {
        Ok(Value::Object(map)) => map,
        Ok(_) => Map::new(),
        Err(e) => return Err(Failure::Broken(e.to_string())),
    };
    let path = fill(path, &mut args);
    let body = match body {
        Body::Empty => None,
        Body::Field(name) => args.remove(name),
        Body::Rest => Some(Value::Object(args)),
    };
    let target = server(on).await.map_err(Failure::Broken)?;
    let to = To {
        method,
        path: &path,
        body: body.as_ref(),
        last_event: None,
    };
    let (ok, text) = fetch(&target, &to).await.map_err(|_| {
        Failure::Broken(format!(
            "The Bastion server {} at {} cannot be reached.",
            target.name, target.url
        ))
    })?;
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if ok {
        return serde_json::from_value(value).map_err(|e| Failure::Broken(e.to_string()));
    }
    // A refusal is `{ "error": text }`, or a typed error as it is.
    let refusal = value.get("error").cloned().unwrap_or(value);
    Err(serde_json::from_value::<E>(refusal.clone())
        .map(Failure::Refused)
        .unwrap_or_else(|_| Failure::Broken(refusal.to_string())))
}

/// Put each `{name}` argument into the path, and take it out of the args.
fn fill(template: &str, args: &mut Map<String, Value>) -> String {
    let mut path = template.to_string();
    while let Some(start) = path.find('{') {
        let end = start + path[start..].find('}').unwrap_or(0);
        let name = path[start + 1..end].to_string();
        let value = match args.remove(&name) {
            Some(Value::String(s)) => s,
            Some(other) => other.to_string(),
            None => String::new(),
        };
        path.replace_range(start..=end, &encode(&value));
    }
    path
}

/// A path segment, percent-encoded.
fn encode(segment: &str) -> String {
    segment
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A window command, through Tauri.
async fn invoke<T: DeserializeOwned, E: DeserializeOwned>(
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

thread_local! {
    /// The Bastion servers the app knows, `local` first, read once.
    static SERVERS: RefCell<Option<Vec<Server>>> = const { RefCell::new(None) };
    /// The server a page about one server shows, and a job's page uses (D30).
    static CURRENT: RefCell<String> = RefCell::new(LOCAL.to_string());
    /// The servers whose events are followed.
    static FOLLOWED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Every Bastion server the app knows, `local` first.
pub async fn servers() -> Vec<Server> {
    if let Some(all) = SERVERS.with(|s| s.borrow().clone()) {
        return all;
    }
    refresh_servers().await
}

/// Read the server list again, after one was added or removed, and follow
/// any new one's events.
pub async fn refresh_servers() -> Vec<Server> {
    let all = invoke::<Vec<Server>, String>(command::SERVERS, None {})
        .await
        .unwrap_or_default();
    SERVERS.with(|s| *s.borrow_mut() = Some(all.clone()));
    for s in &all {
        if !FOLLOWED.with(|f| f.borrow().contains(&s.name)) {
            FOLLOWED.with(|f| f.borrow_mut().push(s.name.clone()));
            leptos::task::spawn_local(follow(s.name.clone()));
        }
    }
    all
}

async fn server(name: &str) -> Result<Server, String> {
    servers()
        .await
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("no Bastion server named {name}"))
}

/// The server pages about one server show.
pub fn current() -> String {
    CURRENT.with(|c| c.borrow().clone())
}

/// Show the server named `name`: every page reads again from it.
pub fn choose(name: &str) {
    if CURRENT.with(|c| c.replace(name.to_string())) != name {
        read_everything_again();
    }
}

/// One request: its method, its path, its JSON body, and the last event
/// seen when it is the event stream.
struct To<'a> {
    method: &'a str,
    path: &'a str,
    body: Option<&'a Value>,
    last_event: Option<&'a str>,
}

/// One request to the server. Returns whether it succeeded, and its body.
async fn fetch(target: &Server, to: &To<'_>) -> Result<(bool, String), JsValue> {
    let response = request(target, to).await?;
    let text = JsFuture::from(response.text()?).await?;
    Ok((response.ok(), text.as_string().unwrap_or_default()))
}

async fn request(target: &Server, to: &To<'_>) -> Result<Response, JsValue> {
    let headers = Headers::new()?;
    headers.set("Authorization", &format!("Bearer {}", target.token))?;
    if let Some(id) = to.last_event {
        headers.set("Last-Event-ID", id)?;
    }
    let init = RequestInit::new();
    init.set_method(to.method);
    if let Some(body) = to.body {
        headers.set("Content-Type", "application/json")?;
        init.set_body(&JsValue::from_str(&body.to_string()));
    }
    init.set_headers(&headers);
    let url = format!("{}{}", target.url.trim_end_matches('/'), to.path);
    let request = Request::new_with_str_and_init(&url, &init)?;
    let window = web_sys::window().ok_or("no window")?;
    Ok(JsFuture::from(window.fetch_with_request(&request))
        .await?
        .unchecked_into())
}

/// Why a call did not return its value.
pub enum Failure<E> {
    /// The command answered with its typed error.
    Refused(E),
    /// The call itself failed: no server, unknown command, or an answer that did not parse.
    Broken(String),
}

impl Failure<String> {
    pub fn text(self) -> String {
        match self {
            Failure::Refused(text) | Failure::Broken(text) => text,
        }
    }
}

type Handler = Rc<dyn Fn(&Value)>;

thread_local! {
    /// Each page's handlers, with a flag that drops when the page closes.
    static LISTENERS: RefCell<Vec<(&'static str, Handler, Arc<AtomicBool>)>> =
        const { RefCell::new(Vec::new()) };
    static FOLLOWING: Cell<bool> = const { Cell::new(false) };
}

/// Run `on_event` with each payload of `event`, until the page that called
/// this is closed. A closed page's signals are disposed, and reading one
/// panics, so its handler must stop.
pub fn listen<T: DeserializeOwned + 'static>(event: &'static str, on_event: impl Fn(T) + 'static) {
    if event == event::QUIT_REQUESTED {
        return listen_window(event, on_event);
    }
    let open = Arc::new(AtomicBool::new(true));
    let closed = open.clone();
    leptos::prelude::on_cleanup(move || closed.store(false, Ordering::Relaxed));
    let handler: Handler = Rc::new(move |payload: &Value| {
        if let Ok(value) = serde_json::from_value(payload.clone()) {
            on_event(value);
        }
    });
    LISTENERS.with(|l| l.borrow_mut().push((event, handler, open)));
    if !FOLLOWING.with(|f| f.replace(true)) {
        // A hidden webview can miss pushes, so every page reads again on focus.
        let on_focus = Closure::<dyn Fn()>::new(read_everything_again);
        if let Some(w) = web_sys::window() {
            let _ = w.add_event_listener_with_callback("focus", on_focus.as_ref().unchecked_ref());
        }
        on_focus.forget(); // the window lives as long as the app
        leptos::task::spawn_local(async {
            refresh_servers().await;
        });
    }
}

/// A window event, through Tauri.
fn listen_window<T: DeserializeOwned + 'static>(
    event: &'static str,
    on_event: impl Fn(T) + 'static,
) {
    leptos::task::spawn_local(async move {
        let handler = Closure::new(move |e: JsValue| {
            let payload = Reflect::get(&e, &"payload".into()).unwrap_or(JsValue::NULL);
            if let Ok(value) = serde_wasm_bindgen::from_value(payload) {
                on_event(value);
            }
        });
        listen_js(event, &handler).await;
        handler.forget(); // the window lives as long as the app
    });
}

/// Hand `payload` to every open page that listens for `event`.
fn dispatch(event: &str, payload: Value) {
    let handlers: Vec<Handler> = LISTENERS.with(|l| {
        let mut l = l.borrow_mut();
        l.retain(|(_, _, open)| open.load(Ordering::Relaxed));
        l.iter()
            .filter(|(name, _, _)| *name == event)
            .map(|(_, h, _)| h.clone())
            .collect()
    });
    for handler in handlers {
        handler(&payload);
    }
}

/// Every page reads again: after missed changes, or a reconnect. The
/// engine's status rides on its event, so it is read here for the pages.
fn read_everything_again() {
    dispatch(event::JOB_CHANGED, Value::String(String::new()));
    dispatch(event::CONSOLE_CHANGED, Value::Null);
    leptos::task::spawn_local(async {
        if let Ok(status) = call::<EngineStatus, ()>(command::ENGINE_STATUS, None {}).await {
            dispatch(
                event::ENGINE_CHANGED,
                serde_json::to_value(status).unwrap_or_default(),
            );
        }
    });
}

/// One Bastion server's event feed: its name and the last event id seen.
struct Feed {
    server: String,
    last: Option<String>,
}

impl Feed {
    /// A change from this server. Job changes reach the pages from every
    /// server, because the Jobs page lists them all; the rest only from the
    /// current server, so a page never shows another server's state.
    fn publish(&self, event: &str, payload: Value) {
        if event == event::JOB_CHANGED || self.server == current() {
            dispatch(event, payload);
        }
    }

    fn deliver(&self, change: Change) {
        let (event, payload) = match change {
            Change::Job { id } => (event::JOB_CHANGED, Value::String(id)),
            Change::Engine { status } => (
                event::ENGINE_CHANGED,
                serde_json::to_value(status).unwrap_or_default(),
            ),
            Change::Console => (event::CONSOLE_CHANGED, Value::Null),
        };
        self.publish(event, payload);
    }

    /// Changes were missed: the jobs read again, and everything else too if
    /// this is the server the pages show.
    fn resync(&self) {
        if self.server == current() {
            read_everything_again();
        } else {
            dispatch(event::JOB_CHANGED, Value::String(String::new()));
        }
    }

    /// One server-sent event: an id and a change, or a request to read again.
    fn block(&mut self, block: &str) {
        for line in block.lines() {
            if let Some(id) = line.strip_prefix("id: ") {
                self.last = Some(id.to_string());
            } else if line == "event: resync" {
                self.resync();
            } else if let Some(change) = line
                .strip_prefix("data: ")
                .and_then(|data| serde_json::from_str::<Change>(data).ok())
            {
                self.deliver(change);
            }
        }
    }

    /// Read one connection's events until it ends.
    async fn stream(&mut self, server: &Server) -> Result<(), JsValue> {
        let to = To {
            method: "GET",
            path: "/events",
            body: None,
            last_event: self.last.as_deref(),
        };
        let response = request(server, &to).await?;
        if !response.ok() {
            return Err(JsValue::from_str(&format!(
                "events: status {}",
                response.status()
            )));
        }
        // A reconnect may have missed changes the server did not replay.
        if self.last.is_some() {
            self.resync();
        }
        let reader: ReadableStreamDefaultReader = response
            .body()
            .ok_or("events: no body")?
            .get_reader()
            .unchecked_into();
        let mut buffer = String::new();
        loop {
            let chunk = JsFuture::from(reader.read()).await?;
            if Reflect::get(&chunk, &"done".into())?.as_bool() == Some(true) {
                return Ok(());
            }
            let bytes = Uint8Array::new(&Reflect::get(&chunk, &"value".into())?).to_vec();
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(end) = buffer.find("\n\n") {
                let block: String = buffer.drain(..end + 2).collect();
                self.block(&block);
            }
        }
    }
}

/// Follow one server's events while it is in the list, reconnecting with
/// the last id seen. While it cannot be reached, its engine shows as down.
async fn follow(name: String) {
    let mut feed = Feed {
        server: name,
        last: None,
    };
    while let Ok(s) = server(&feed.server).await {
        if let Err(e) = feed.stream(&s).await {
            web_sys::console::warn_1(&e);
        }
        let down = EngineStatus {
            url: s.url.clone(),
            link: Link::Down,
            functions: None,
        };
        feed.publish(
            event::ENGINE_CHANGED,
            serde_json::to_value(down).unwrap_or_default(),
        );
        sleep(1000).await;
    }
    FOLLOWED.with(|f| f.borrow_mut().retain(|n| *n != feed.server));
}

async fn sleep(ms: i32) {
    let wait = Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(wait).await;
}

/// No arguments.
#[derive(Serialize)]
pub struct None {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn path_arguments_fill_the_path_and_leave_the_body() {
        let mut args = json!({ "id": "j1", "text": "a b" })
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(fill("/jobs/{id}/comment", &mut args), "/jobs/j1/comment");
        assert_eq!(Value::Object(args), json!({ "text": "a b" }));
        let mut args = json!({ "workspace": "my repo", "number": 7 })
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            fill("/issues/{workspace}/{number}/job", &mut args),
            "/issues/my%20repo/7/job"
        );
    }
}
