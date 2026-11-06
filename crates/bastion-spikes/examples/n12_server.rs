//! N12 (PLAN.md S0): can the Tauri page call a Bastion server over HTTP and
//! follow its events? `/ping` and `/events` both need the bearer token, and a
//! client that reconnects with `Last-Event-ID` resumes after the last id it saw.
//! `cargo run -p bastion-spikes --example n12_server -- [port]` (default 49180).

use std::convert::Infallible;
use std::time::Duration;

use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::Stream;
use futures_util::stream;
use serde_json::{Value, json};
use tower_http::cors::CorsLayer;

const TOKEN: &str = "Bearer spike-n12";

fn authorized(headers: &HeaderMap) -> Result<(), StatusCode> {
    match headers.get(header::AUTHORIZATION) {
        Some(v) if v == TOKEN => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

async fn ping(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    authorized(&headers)?;
    Ok(Json(json!({ "ok": true, "pid": std::process::id() })))
}

/// One event a second, numbered. A reconnect starts after `Last-Event-ID`.
async fn events(
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    authorized(&headers)?;
    let last = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    println!("events: connected, last id {last:?}");
    let first = last.map_or(1, |n| n + 1);
    let numbered = stream::unfold(first, |n| async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let data = json!({ "kind": "job", "id": "j1", "revision": n });
        let event = Event::default().id(n.to_string()).data(data.to_string());
        Some((Ok(event), n + 1))
    });
    Ok(Sse::new(numbered).keep_alive(KeepAlive::default()))
}

#[tokio::main]
async fn main() {
    let port = std::env::args().nth(1).unwrap_or_else(|| "49180".into());
    // The dev window loads from Trunk; a built app loads from tauri://localhost.
    let origins = ["http://localhost:1420", "tauri://localhost"].map(HeaderValue::from_static);
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET])
        .allow_headers([header::AUTHORIZATION, "last-event-id".parse().unwrap()]);
    let app = Router::new()
        .route("/ping", get(ping))
        .route("/events", get(events))
        .layer(cors);
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    println!("n12: serving on 127.0.0.1:{port}");
    axum::serve(listener, app).await.unwrap();
}
