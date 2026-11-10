//! Calls to a Bastion server (PLAN.md D28): each typed request in
//! `bastion-protocol` goes over HTTP, with the server's bearer token.

use std::time::Duration;

use bastion_protocol::Server;
use bastion_protocol::api::Request;
use serde_json::Value;

/// How long a server has to accept a connection.
const CONNECT: Duration = Duration::from_secs(5);
/// How long a call may take, start of a stack included.
const CALL: Duration = Duration::from_secs(120);
/// How long the event stream may stay silent. The server sends a keep-alive
/// every 15 s, so silence past this means a half-open connection.
const SILENCE: Duration = Duration::from_secs(45);

/// One HTTP client for every call, so connections are kept.
pub fn http() -> reqwest::Client {
    let built = reqwest::Client::builder()
        .connect_timeout(CONNECT)
        .timeout(CALL)
        .build();
    built.expect("the TLS backend loads")
}

/// The client for a server's event stream: no limit on the whole stream,
/// only on each silence.
pub fn stream_http() -> reqwest::Client {
    let built = reqwest::Client::builder()
        .connect_timeout(CONNECT)
        .read_timeout(SILENCE)
        .build();
    built.expect("the TLS backend loads")
}

/// Send `request` to `server`, and read its answer, or the server's
/// refusal as text.
pub async fn send<R: Request>(
    http: &reqwest::Client,
    server: &Server,
    request: &R,
) -> Result<R::Answer, String> {
    if server.token.is_empty() {
        return Err(format!(
            "There is no token for the Bastion server {}. Remove it and add it again with a token.",
            server.name
        ));
    }
    let url = format!("{}{}", server.url.trim_end_matches('/'), request.path());
    let method = reqwest::Method::from_bytes(R::METHOD.as_bytes()).map_err(|e| e.to_string())?;
    let mut sent = http.request(method, url).bearer_auth(&server.token);
    if R::BODY {
        sent = sent.json(request);
    }
    if let Some(secs) = R::WAIT_SECS {
        sent = sent.timeout(Duration::from_secs(secs));
    }
    let unreachable = |_| {
        format!(
            "The Bastion server {} at {} cannot be reached.",
            server.name, server.url
        )
    };
    let response = sent.send().await.map_err(unreachable)?;
    let ok = response.status().is_success();
    let text = response.text().await.unwrap_or_default();
    serde_json::from_value(answer(ok, &text)?).map_err(|e| e.to_string())
}

/// A success's value, or a refusal's text: `{ "error": text }`, or the
/// typed error as it is.
fn answer(ok: bool, text: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    if ok {
        return Ok(value);
    }
    match value.get("error").cloned().unwrap_or(value) {
        Value::String(why) => Err(why),
        other => Err(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_refusal_reads_as_its_text() {
        assert_eq!(answer(true, r#"{"a":1}"#), Ok(json!({ "a": 1 })));
        assert_eq!(answer(false, r#"{"error":"no"}"#), Err("no".into()));
        assert_eq!(answer(false, r#"{"code":7}"#), Err(r#"{"code":7}"#.into()));
    }
}
