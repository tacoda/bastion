//! One llm-router provider per host: its four functions, its declaration
//! to the router, its catalog, and its turns.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iii_sdk::channel::{ChannelWriter, StreamChannelRef};
use iii_sdk::protocol::{RegisterTriggerInput, TriggerRequest};
use iii_sdk::runtime::FunctionRef;
use iii_sdk::trigger::Trigger;
use iii_sdk::{Error, IIIClient, RegisterFunction};
use serde_json::{Value, json};
use tokio::task::AbortHandle;

use crate::wire;

pub const WORKER: &str = "bastion-openai-compat";
const NAMESPACE: &str = "bastion";
/// The router's idle timeout is 120 s; a ping keeps a slow host's turn alive.
const PING: Duration = Duration::from_secs(25);

/// What every host's functions share.
pub struct Ctx {
    pub iii: IIIClient,
    pub http: reqwest::Client,
    aborts: Mutex<HashMap<String, AbortHandle>>,
}

/// One host's live registration, dropped when the admin removes it.
pub struct Live {
    pub host: Value,
    functions: Vec<FunctionRef>,
    ready: Option<Trigger>,
}

impl Live {
    pub fn unregister(&self) {
        self.functions.iter().for_each(FunctionRef::unregister);
        if let Some(t) = &self.ready {
            t.unregister();
        }
    }
}

impl Ctx {
    pub fn new(iii: IIIClient) -> Arc<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            // A host silent this long is gone, as the router's idle timeout says.
            .read_timeout(Duration::from_secs(120))
            .build()
            .expect("an HTTP client");
        Arc::new(Self {
            iii,
            http,
            aborts: Mutex::new(HashMap::new()),
        })
    }

    pub async fn call(&self, function: &str, payload: Value) -> Result<Value, String> {
        let request = TriggerRequest::new(function, payload).namespace(NAMESPACE);
        self.iii.trigger(request).await.map_err(|e| e.to_string())
    }

    pub async fn kept(&self, key: &str) -> Value {
        self.read(key).await.unwrap_or(Value::Null)
    }

    pub async fn read(&self, key: &str) -> Result<Value, String> {
        let get = json!({ "scope": WORKER, "key": key });
        self.call("state::get", get).await
    }

    pub async fn keep(&self, key: &str, value: Value) -> Result<(), String> {
        let set = json!({ "scope": WORKER, "key": key, "value": value });
        self.call("state::set", set).await.map(|_| ())
    }
}

fn token_key(id: &str) -> String {
    format!("registration_token:{id}")
}

fn with_token(mut payload: Value, token: &Value) -> Value {
    if token.is_string() {
        payload["token"] = token.clone();
    }
    payload
}

/// Tell the router this host is a provider, keeping the token it answers
/// with: the router refuses the id to anyone without it. An `open` host
/// needs no key.
async fn declare(ctx: &Ctx, host: &Value, open: bool) -> Result<(), String> {
    let id = host["id"].as_str().unwrap_or_default();
    let token = ctx.kept(&token_key(id)).await;
    let declaration = json!({
        "id": id,
        "display_name": host["name"],
        "credential_optional": open,
        "defaults": { "api_url": host["api_url"], "max_tokens": wire::MAX_TOKENS },
        "supports_model_listing": true,
        "worker_id": WORKER,
    });
    let answer = ctx
        .call(
            "router::provider::register",
            with_token(declaration, &token),
        )
        .await?;
    match &answer["registration_token"] {
        t @ Value::String(_) if *t != token => ctx.keep(&token_key(id), t.clone()).await,
        _ => Ok(()),
    }
}

/// The host's key, URL and output limit, as the router resolves them.
async fn resolve(ctx: &Ctx, id: &str) -> Result<Value, String> {
    let token = ctx.kept(&token_key(id)).await;
    let ask = with_token(json!({ "id": id }), &token);
    let r = ctx.call("router::provider::resolve", ask).await?;
    match r["credential_error"].as_str() {
        Some(why) => Err(why.into()),
        None => Ok(r),
    }
}

fn authorized(request: reqwest::RequestBuilder, resolved: &Value) -> reqwest::RequestBuilder {
    match resolved["credential"]["key"].as_str() {
        Some(key) => request.bearer_auth(key),
        None => request,
    }
}

/// Read the host's models and give them to the router's catalog.
async fn refresh(ctx: &Ctx, id: &str) -> Result<usize, String> {
    let resolved = resolve(ctx, id).await?;
    let url = wire::models_url(resolved["api_url"].as_str().unwrap_or_default());
    let response = authorized(ctx.http.get(url), &resolved)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let listing: Value = response.json().await.map_err(|e| e.to_string())?;
    let max = resolved["max_tokens"].as_u64().unwrap_or(wire::MAX_TOKENS);
    let models = wire::catalog(id, &listing, max);
    let count = models.len();
    let token = ctx.kept(&token_key(id)).await;
    let reconcile = with_token(json!({ "provider": id, "models": models }), &token);
    ctx.call("router::models::reconcile", reconcile).await?;
    Ok(count)
}

async fn declare_and_refresh(ctx: Arc<Ctx>, host: Value) {
    let id = host["id"].as_str().unwrap_or_default().to_string();
    let mut delay = Duration::from_millis(500);
    while let Err(e) = declare(&ctx, &host, true).await {
        eprintln!("[{WORKER}] {id}: register failed ({e}); retrying in {delay:?}");
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(10));
    }
    match refresh(&ctx, &id).await {
        Ok(n) => println!("[{WORKER}] {id}: {n} models"),
        Err(e) => eprintln!("[{WORKER}] {id}: no models ({e})"),
    }
}

/// A removed host. The router has no call to drop a provider, so the host
/// stays listed, but it offers no models and is not ready without a key.
pub async fn retire(ctx: Arc<Ctx>, host: Value) {
    let id = host["id"].as_str().unwrap_or_default();
    let token = ctx.kept(&token_key(id)).await;
    let empty = with_token(json!({ "provider": id, "models": [] }), &token);
    let done = match declare(&ctx, &host, false).await {
        Ok(()) => ctx
            .call("router::models::reconcile", empty)
            .await
            .map(|_| ()),
        Err(e) => Err(e),
    };
    if let Err(e) = done {
        eprintln!("[{WORKER}] {id}: not retired ({e})");
    }
}

/// Send the host's whole answer, pinging the router while it waits.
async fn ask(
    ctx: &Ctx,
    turn: &wire::Turn<'_>,
    input: &Value,
    writer: &ChannelWriter,
) -> Result<Value, (String, String)> {
    let permanent = |e: String| ("permanent".to_string(), e);
    let resolved = resolve(ctx, turn.host)
        .await
        .map_err(|e| ("auth_expired".into(), e))?;
    let url = resolved["api_url"].as_str().unwrap_or_default();
    let max = resolved["max_tokens"].as_u64().unwrap_or(wire::MAX_TOKENS);
    let body = wire::request(turn, input, max);
    let send = authorized(ctx.http.post(url), &resolved).json(&body).send();
    tokio::pin!(send);
    let mut ping = tokio::time::interval(PING);
    ping.tick().await;
    let response = loop {
        tokio::select! {
            r = &mut send => break r.map_err(|e| ("transient".to_string(), e.to_string()))?,
            _ = ping.tick() => { let _ = writer.send_message(r#"{"type":"ping"}"#).await; }
        }
    };
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| permanent(e.to_string()))?;
    if !status.is_success() {
        return Err((
            wire::error_kind(status.as_u16()).into(),
            format!("{status}: {text}"),
        ));
    }
    serde_json::from_str(&text).map_err(|e| permanent(e.to_string()))
}

async fn turn(ctx: Arc<Ctx>, id: String, input: Value) -> Result<(), String> {
    let channel: StreamChannelRef =
        serde_json::from_value(input["writer_ref"].clone()).map_err(|e| e.to_string())?;
    let writer = ChannelWriter::new(ctx.iii.address(), &channel);
    let model = input["model"].as_str().unwrap_or_default();
    let turn = wire::Turn { host: &id, model };
    let frames = match ask(&ctx, &turn, &input, &writer).await {
        Ok(answer) => turn.frames(&answer),
        Err((kind, why)) => turn.failure(&kind, &why),
    };
    for frame in frames {
        writer
            .send_message(&frame.to_string())
            .await
            .map_err(|e| e.to_string())?;
    }
    writer.close().await.map_err(|e| e.to_string())
}

/// `provider::<id>::stream`: the turn runs as a task `abort` can stop.
async fn stream(ctx: Arc<Ctx>, id: String, input: Value) -> Result<Value, Error> {
    let key = input["resolution_key"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let task = tokio::spawn(turn(ctx.clone(), id, input));
    ctx.aborts
        .lock()
        .unwrap()
        .insert(key.clone(), task.abort_handle());
    let ended = task.await;
    ctx.aborts.lock().unwrap().remove(&key);
    match ended {
        Ok(Err(e)) => Err(Error::Handler(e)),
        _ => Ok(json!({ "ok": true })),
    }
}

fn abort(ctx: &Ctx, input: &Value) -> Value {
    let key = input["request_id"].as_str().unwrap_or_default();
    let handle = ctx.aborts.lock().unwrap().remove(key);
    handle.iter().for_each(AbortHandle::abort);
    json!({ "aborted": handle.is_some() })
}

/// Register host `host`'s four functions and declare it to the router.
// `router::ready` is a trigger type, and `observe` only watches functions, so
// this binds it the deprecated way, as the server's engine does.
#[allow(deprecated)]
pub fn register(ctx: &Arc<Ctx>, host: &Value) -> Live {
    let id = host["id"].as_str().unwrap_or_default().to_string();
    let function = |name: &str| format!("provider::{id}::{name}");
    let internal = json!({ "internal": true });
    let (c, i) = (ctx.clone(), id.clone());
    let on_stream =
        RegisterFunction::new_async(move |input: Value| stream(c.clone(), i.clone(), input));
    let c = ctx.clone();
    let on_abort = RegisterFunction::new_async(move |input: Value| {
        let answer = abort(&c, &input);
        async move { Ok::<_, Error>(answer) }
    });
    let (c, i) = (ctx.clone(), id.clone());
    let on_refresh = RegisterFunction::new_async(move |_: Value| {
        let (c, i) = (c.clone(), i.clone());
        async move {
            let count = refresh(&c, &i).await.map_err(Error::Handler)?;
            Ok::<_, Error>(json!({ "ok": true, "count": count }))
        }
    });
    let (c, h) = (ctx.clone(), host.clone());
    let on_ready = RegisterFunction::new_async(move |_: Value| {
        tokio::spawn(declare_and_refresh(c.clone(), h.clone()));
        async { Ok::<_, Error>(json!({ "ok": true })) }
    });
    let functions = vec![
        ctx.iii.register_function(function("stream"), on_stream),
        ctx.iii
            .register_function(function("abort"), on_abort.metadata(internal.clone())),
        ctx.iii
            .register_function(function("refresh_models"), on_refresh),
        ctx.iii
            .register_function(function("on_router_ready"), on_ready.metadata(internal)),
    ];
    let bind = RegisterTriggerInput::new("router::ready", function("on_router_ready"), json!({}));
    let ready = ctx.iii.register_trigger(bind).ok();
    tokio::spawn(declare_and_refresh(ctx.clone(), host.clone()));
    Live {
        host: host.clone(),
        functions,
        ready,
    }
}
