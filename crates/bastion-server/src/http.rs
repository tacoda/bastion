//! The HTTP API (D28) and its event stream (D29). Each route calls one
//! function in `api`, and every route needs the server's bearer token.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bastion_core::Pipeline;
use bastion_protocol::{
    Change, ConstraintView, DialsView, HeldCallRef, LogRequest, NewEval, SettingsView, Workspace,
};
use futures_util::{Stream, stream};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tower_http::cors::CorsLayer;

use crate::api::{self, factory::NOT_STARTED};
use crate::server::Server;

/// The origins of the app's window: Trunk's in development, Tauri's built.
const ORIGINS: [&str; 2] = ["http://localhost:1420", "tauri://localhost"];

#[derive(Clone)]
struct App {
    server: Arc<Server>,
    token: Arc<str>,
}

/// The API for `server`, open to a request that carries `token`.
pub fn router(server: Arc<Server>, token: &str) -> Router {
    let app = App {
        server,
        token: token.into(),
    };
    let cors = CorsLayer::new()
        .allow_origin(ORIGINS.map(HeaderValue::from_static))
        .allow_methods([Method::GET, Method::POST, Method::PUT])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            "last-event-id".parse().unwrap(),
        ]);
    routes()
        .route_layer(middleware::from_fn_with_state(app.clone(), authorized))
        .with_state(app)
        .layer(cors)
}

fn routes() -> Router<App> {
    Router::new()
        .route("/events", get(events))
        .route("/engine", get(engine))
        .route("/doctor", get(doctor))
        .route("/stack", get(stack_status))
        .route("/stack/start", post(stack_start))
        .route("/stack/stop", post(stack_stop))
        .route("/console", get(console))
        .route("/console/logs", post(container_logs))
        .route("/console/{name}/stop", post(container_stop))
        .route("/console/{name}/start", post(container_start))
        .merge(factory_routes())
        .merge(govern_routes())
        .merge(settings_routes())
}

fn factory_routes() -> Router<App> {
    Router::new()
        .route("/workspaces", get(workspaces).post(workspace_add))
        .route("/jobs", get(jobs).post(job_submit))
        .route("/jobs/{id}", get(job))
        .route("/jobs/{id}/approve", post(job_approve))
        .route("/jobs/{id}/comment", post(job_comment))
        .route("/jobs/{id}/stop", post(job_stop))
        .route("/dials", get(dials).put(dials_set))
        .route("/evals", get(evals).post(eval_create))
        .route("/evals/{id}", get(eval))
        .route("/evals/{id}/apply", post(eval_apply))
}

fn govern_routes() -> Router<App> {
    Router::new()
        .route("/held/allow", post(held_allow))
        .route("/held/refuse", post(held_refuse))
        .route("/constraints", get(constraints).post(constraint_add))
        .route("/constraints/{id}/move", post(constraint_move))
        .route("/capabilities", get(capabilities))
        .route("/capabilities/move", post(capability_move))
        .route("/capabilities/factory", get(factory_capabilities))
        .route("/charters", get(charters))
        .route("/audit", get(audit))
        .route("/audit/export", post(audit_export))
}

fn settings_routes() -> Router<App> {
    Router::new()
        .route("/settings", get(settings).put(settings_set))
        .route("/settings/preset", post(settings_preset))
        .route("/settings/history", get(settings_history))
        .route("/settings/restore", post(settings_restore))
        .route("/models", get(models))
        .route("/accounts", get(accounts))
        .route("/issues/{workspace}", get(issues))
        .route("/issues/{workspace}/{number}/job", post(job_from_issue))
}

async fn authorized(State(app): State<App>, request: Request, next: Next) -> Response {
    let expected = format!("Bearer {}", app.token);
    match request.headers().get(header::AUTHORIZATION) {
        Some(given) if given.as_bytes() == expected.as_bytes() => next.run(request).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "no valid token" })),
        )
            .into_response(),
    }
}

/// A result as a response: the value, or the reason it was refused. A
/// factory that has not started yet is unavailable rather than refused.
fn answer<T: Serialize>(result: Result<T, String>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) if e == NOT_STARTED => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": e }))).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

/// A result whose error is typed, such as the stack's `StartRefused`.
fn typed<T: Serialize, E: Serialize>(result: Result<T, E>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => (StatusCode::CONFLICT, Json(e)).into_response(),
    }
}

/// A result that cannot fail.
fn always<T: Serialize>(result: Result<T, ()>) -> Response {
    typed(result)
}

async fn engine(State(app): State<App>) -> Response {
    always(api::engine::engine_status(&app.server.engine).await)
}

async fn doctor(State(app): State<App>) -> Response {
    let s = &app.server;
    always(api::doctor::doctor(&s.machine, &s.engine, &s.stack.checkout).await)
}

async fn stack_status(State(app): State<App>) -> Response {
    answer(api::stack::stack_status(
        &app.server.stack,
        &app.server.machine,
    ))
}

async fn stack_start(State(app): State<App>) -> Response {
    let s = &app.server;
    typed(api::stack::stack_start(&s.stack, &s.machine, &s.engine).await)
}

async fn stack_stop(State(app): State<App>) -> Response {
    answer(api::stack::stack_stop(
        &app.server.stack,
        &app.server.machine,
    ))
}

async fn console(State(app): State<App>) -> Response {
    answer(api::console::console(&app.server.engine).await)
}

async fn container_logs(State(app): State<App>, Json(r): Json<LogRequest>) -> Response {
    answer(api::console::container_logs(&app.server.engine, r).await)
}

async fn container_stop(State(app): State<App>, Path(name): Path<String>) -> Response {
    answer(api::console::container_stop(&app.server.engine, name).await)
}

async fn container_start(State(app): State<App>, Path(name): Path<String>) -> Response {
    answer(api::console::container_start(&app.server.engine, name).await)
}

async fn workspaces(State(app): State<App>) -> Response {
    answer(api::factory::workspaces(&app.server.factory).await)
}

async fn workspace_add(State(app): State<App>, Json(w): Json<Workspace>) -> Response {
    let s = &app.server;
    answer(api::factory::workspace_add(&s.factory, &s.machine, w).await)
}

async fn jobs(State(app): State<App>) -> Response {
    answer(api::factory::jobs(&app.server.factory).await)
}

#[derive(Deserialize)]
struct Submit {
    workspace: String,
    request: String,
    pipeline: Option<Pipeline>,
}

async fn job_submit(State(app): State<App>, Json(b): Json<Submit>) -> Response {
    let f = &app.server.factory;
    answer(api::factory::job_submit(f, b.workspace, b.request, b.pipeline).await)
}

async fn job(State(app): State<App>, Path(id): Path<String>) -> Response {
    answer(api::factory::job(&app.server.factory, id).await)
}

async fn job_approve(State(app): State<App>, Path(id): Path<String>) -> Response {
    answer(api::factory::job_approve(&app.server.factory, id).await)
}

#[derive(Deserialize)]
struct Text {
    text: String,
}

async fn job_comment(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(b): Json<Text>,
) -> Response {
    answer(api::factory::job_comment(&app.server.factory, id, b.text).await)
}

async fn job_stop(State(app): State<App>, Path(id): Path<String>) -> Response {
    answer(api::factory::job_stop(&app.server.factory, id).await)
}

async fn dials(State(app): State<App>) -> Response {
    answer(api::factory::dials(&app.server.factory).await)
}

async fn dials_set(State(app): State<App>, Json(d): Json<DialsView>) -> Response {
    answer(api::factory::dials_set(&app.server.factory, d).await)
}

async fn evals(State(app): State<App>) -> Response {
    answer(api::evals::evals(&app.server.factory).await)
}

async fn eval_create(State(app): State<App>, Json(e): Json<NewEval>) -> Response {
    let s = &app.server;
    answer(api::evals::eval_create(&s.factory, &s.machine, e).await)
}

async fn eval(State(app): State<App>, Path(id): Path<String>) -> Response {
    answer(api::evals::eval(&app.server.factory, id).await)
}

#[derive(Deserialize)]
struct Arm {
    arm: String,
}

async fn eval_apply(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(b): Json<Arm>,
) -> Response {
    answer(api::evals::eval_apply(&app.server.factory, id, b.arm).await)
}

async fn held_allow(State(app): State<App>, Json(h): Json<HeldCallRef>) -> Response {
    answer(api::govern::held_allow(&app.server.factory, h).await)
}

#[derive(Deserialize)]
struct Refusal {
    held: HeldCallRef,
    feedback: String,
}

async fn held_refuse(State(app): State<App>, Json(b): Json<Refusal>) -> Response {
    answer(api::govern::held_refuse(&app.server.factory, b.held, b.feedback).await)
}

async fn constraints(State(app): State<App>) -> Response {
    answer(api::govern::constraints(&app.server.factory).await)
}

async fn constraint_add(State(app): State<App>, Json(c): Json<ConstraintView>) -> Response {
    answer(api::govern::constraint_add(&app.server.factory, c).await)
}

#[derive(Deserialize)]
struct Up {
    up: bool,
}

async fn constraint_move(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(b): Json<Up>,
) -> Response {
    answer(api::govern::constraint_move(&app.server.factory, id, b.up).await)
}

async fn capabilities(State(app): State<App>) -> Response {
    answer(api::govern::capabilities(&app.server.factory).await)
}

#[derive(Deserialize)]
struct Grant {
    stage: String,
    function: String,
    up: bool,
}

async fn capability_move(State(app): State<App>, Json(b): Json<Grant>) -> Response {
    let f = &app.server.factory;
    answer(api::govern::capability_move(f, b.stage, b.function, b.up).await)
}

async fn factory_capabilities(State(app): State<App>) -> Response {
    answer(api::govern::factory_capabilities(&app.server.factory).await)
}

async fn charters(State(app): State<App>) -> Response {
    answer(api::govern::charters(&app.server.factory, &app.server.machine).await)
}

async fn audit(State(app): State<App>) -> Response {
    answer(api::govern::audit(&app.server.factory).await)
}

async fn audit_export(State(app): State<App>) -> Response {
    let s = &app.server;
    answer(api::govern::audit_export(&s.data, &s.factory, &s.machine).await)
}

async fn settings(State(app): State<App>) -> Response {
    answer(api::settings::settings(&app.server.factory).await)
}

async fn settings_set(State(app): State<App>, Json(v): Json<SettingsView>) -> Response {
    let s = &app.server;
    answer(api::settings::settings_set(&s.events, &s.factory, v).await)
}

#[derive(Deserialize)]
struct Preset {
    preset: String,
}

async fn settings_preset(State(app): State<App>, Json(b): Json<Preset>) -> Response {
    let s = &app.server;
    answer(api::settings::settings_preset(&s.events, &s.factory, b.preset).await)
}

async fn settings_history(State(app): State<App>) -> Response {
    answer(api::settings::settings_history(&app.server.factory).await)
}

#[derive(Deserialize)]
struct Version {
    version: u64,
}

async fn settings_restore(State(app): State<App>, Json(b): Json<Version>) -> Response {
    let s = &app.server;
    answer(api::settings::settings_restore(&s.events, &s.factory, b.version).await)
}

async fn models(State(app): State<App>) -> Response {
    answer(api::settings::models(&app.server.factory).await)
}

async fn accounts(State(app): State<App>) -> Response {
    answer(api::settings::accounts(&app.server.factory).await)
}

async fn issues(State(app): State<App>, Path(workspace): Path<String>) -> Response {
    let s = &app.server;
    answer(api::settings::issues(&s.factory, &s.machine, workspace).await)
}

async fn job_from_issue(
    State(app): State<App>,
    Path((workspace, number)): Path<(String, u64)>,
) -> Response {
    let s = &app.server;
    answer(api::settings::job_from_issue(&s.factory, &s.machine, workspace, number).await)
}

/// What the stream sends: a change with its number, or a request to read
/// everything again because changes were missed.
enum Out {
    Change(u64, Change),
    Resync,
}

/// The server's changes as server-sent events. A client that names the last
/// number it saw gets what it missed first, or a `resync` when the server no
/// longer holds it.
async fn events(
    State(app): State<App>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let bus = &app.server.events;
    // Subscribe before reading the backlog, so nothing falls between them.
    let live = bus.subscribe();
    let last = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let backlog: Vec<Out> = match last.map(|n| bus.since(n)) {
        None => Vec::new(),
        Some(Some(missed)) => missed.into_iter().map(|(n, c)| Out::Change(n, c)).collect(),
        Some(None) => vec![Out::Resync],
    };
    let sent = backlog
        .iter()
        .filter_map(|o| match o {
            Out::Change(n, _) => Some(*n),
            Out::Resync => None,
        })
        .max()
        .or(last);
    let live = stream::unfold(live, move |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok((n, _)) if sent.is_some_and(|s| n <= s) => continue,
                Ok((n, change)) => return Some((Out::Change(n, change), rx)),
                Err(RecvError::Lagged(_)) => return Some((Out::Resync, rx)),
                Err(RecvError::Closed) => return None,
            }
        }
    });
    let all = futures_util::StreamExt::map(
        futures_util::StreamExt::chain(stream::iter(backlog), live),
        |out| Ok(sse(out)),
    );
    Sse::new(all).keep_alive(KeepAlive::default())
}

fn sse(out: Out) -> Event {
    match out {
        Out::Change(n, change) => Event::default()
            .id(n.to_string())
            .data(serde_json::to_string(&change).unwrap_or_default()),
        Out::Resync => Event::default().event("resync").data("{}"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::Body;
    use axum::http::Request;
    use bastion_protocol::Link;
    use futures_util::StreamExt;
    use tower::ServiceExt;

    use super::*;
    use crate::engine::fake::FakeEngine;
    use crate::machine::fake::FakeMachine;
    use crate::stack::Stack;

    const TOKEN: &str = "t0ken";

    fn server() -> Arc<Server> {
        let stack = Stack {
            checkout: PathBuf::from("/checkout"),
            port: 49160,
            log: PathBuf::from("/log"),
        };
        let engine = Arc::new(FakeEngine::new(Link::Down));
        Server::new(engine, Arc::new(FakeMachine::new()), stack, "/data".into()).unwrap()
    }

    fn get(path: &str, token: Option<&str>) -> Request<Body> {
        let mut r = Request::get(path);
        if let Some(t) = token {
            r = r.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        r.body(Body::empty()).unwrap()
    }

    async fn call(server: Arc<Server>, request: Request<Body>) -> (StatusCode, String) {
        let response = router(server, TOKEN).oneshot(request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// Every command the page routes reaches a handler here: no 404 for a
    /// path that does not exist, no 405 for the wrong method.
    #[test]
    fn every_route_the_page_uses_exists() {
        crate::rt::block_on(async {
            for cmd in bastion_protocol::routes::ROUTED {
                let (method, path, body) = bastion_protocol::routes::route(cmd).unwrap();
                let path = path.replace("{number}", "7").replace(['{', '}'], "");
                let body = match body {
                    bastion_protocol::routes::Body::Empty => Body::empty(),
                    _ => Body::from("{}"),
                };
                let request = Request::builder()
                    .method(method)
                    .uri(&path)
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(body)
                    .unwrap();
                let status = router(server(), TOKEN)
                    .oneshot(request)
                    .await
                    .unwrap()
                    .status();
                assert!(
                    status != StatusCode::NOT_FOUND && status != StatusCode::METHOD_NOT_ALLOWED,
                    "{cmd}: {method} {path} answered {status}"
                );
            }
        });
    }

    #[test]
    fn a_request_without_the_token_is_refused() {
        crate::rt::block_on(async {
            let (status, _) = call(server(), get("/jobs", None)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            let (status, _) = call(server(), get("/jobs", Some("wrong"))).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        });
    }

    #[test]
    fn routes_answer_with_json_and_a_factory_not_started_is_unavailable() {
        crate::rt::block_on(async {
            let (status, body) = call(server(), get("/engine", Some(TOKEN))).await;
            assert_eq!(status, StatusCode::OK);
            assert!(body.contains("\"link\""), "{body}");
            let (status, body) = call(server(), get("/jobs", Some(TOKEN))).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert!(body.contains(NOT_STARTED), "{body}");
        });
    }

    #[test]
    fn the_windows_origin_may_call_the_api() {
        crate::rt::block_on(async {
            let preflight = Request::options("/jobs")
                .header(header::ORIGIN, "tauri://localhost")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
                .body(Body::empty())
                .unwrap();
            let response = router(server(), TOKEN).oneshot(preflight).await.unwrap();
            assert_eq!(
                response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
                "tauri://localhost"
            );
        });
    }

    /// The first `count` chunks of an event stream, as text.
    async fn first_events(server: Arc<Server>, last: Option<&str>, count: usize) -> String {
        let mut request = get("/events", Some(TOKEN));
        if let Some(id) = last {
            request
                .headers_mut()
                .insert("last-event-id", id.parse().unwrap());
        }
        let response = router(server, TOKEN).oneshot(request).await.unwrap();
        let mut body = response.into_body().into_data_stream();
        let mut text = String::new();
        for _ in 0..count {
            let chunk = body.next().await.unwrap().unwrap();
            text.push_str(&String::from_utf8_lossy(&chunk));
        }
        text
    }

    #[test]
    fn events_replay_what_a_reconnect_missed_or_ask_for_a_resync() {
        crate::rt::block_on(async {
            let s = server();
            let mut seen = s.events.subscribe();
            s.events.publish(Change::Job { id: "j1".into() });
            s.events.publish(Change::Job { id: "j2".into() });
            // The engine watcher may publish first; find j1's number.
            let first = loop {
                if let (n, Change::Job { id }) = seen.recv().await.unwrap()
                    && id == "j1"
                {
                    break n;
                }
            };

            let replay = first_events(s.clone(), Some(&first.to_string()), 1).await;
            assert!(replay.contains(&format!("id: {}", first + 1)), "{replay}");
            assert!(replay.contains("\"id\":\"j2\""), "{replay}");
            assert!(!replay.contains("j1"), "{replay}");

            let stale = first_events(s.clone(), Some("1"), 1).await;
            assert!(stale.contains("event: resync"), "{stale}");
        });
    }

    #[test]
    fn events_stream_each_change_as_it_happens() {
        crate::rt::block_on(async {
            let s = server();
            let publisher = s.clone();
            crate::rt::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                publisher.events.publish(Change::Console);
            });
            // The engine watcher may publish its status first, so read on.
            let response = router(s, TOKEN)
                .oneshot(get("/events", Some(TOKEN)))
                .await
                .unwrap();
            let mut body = response.into_body().into_data_stream();
            let found = async {
                let mut text = String::new();
                while !text.contains("\"kind\":\"console\"") {
                    let chunk = body.next().await.unwrap().unwrap();
                    text.push_str(&String::from_utf8_lossy(&chunk));
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(2), found)
                .await
                .expect("the console change arrives");
        });
    }
}
