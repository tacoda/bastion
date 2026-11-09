//! Calls to a Bastion server (PLAN.md D28): each command goes over HTTP by
//! the route table in `bastion-protocol`, with the server's bearer token.

use std::time::Duration;

use bastion_protocol::Server;
use bastion_protocol::routes::{self, Body};
use serde_json::{Map, Value};

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

/// One command for a server and its arguments.
pub struct Ask {
    pub cmd: &'static str,
    pub args: Value,
}

impl Ask {
    /// A command with no arguments.
    pub fn bare(cmd: &'static str) -> Self {
        Ask {
            cmd,
            args: Value::Null,
        }
    }
}

/// Send `ask` to `server`, and return its answer, or the server's refusal
/// as text.
pub async fn call(http: reqwest::Client, server: Server, ask: Ask) -> Result<Value, String> {
    let Ask { cmd, args } = ask;
    let (method, path, body) =
        routes::route(cmd).ok_or_else(|| format!("{cmd} is not a server command"))?;
    let mut args = match args {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let path = fill(path, &mut args);
    let body = match body {
        Body::Empty => None,
        Body::Field(name) => args.remove(name),
        Body::Rest => Some(Value::Object(args)),
    };
    let url = format!("{}{path}", server.url.trim_end_matches('/'));
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    let mut request = http.request(method, url).bearer_auth(&server.token);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let unreachable = |_| {
        format!(
            "The Bastion server {} at {} cannot be reached.",
            server.name, server.url
        )
    };
    let response = request.send().await.map_err(unreachable)?;
    let ok = response.status().is_success();
    let text = response.text().await.unwrap_or_default();
    answer(ok, &text)
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
    let kept = |b: u8| b.is_ascii_alphanumeric() || b"-._~".contains(&b);
    segment
        .bytes()
        .map(|b| {
            if kept(b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

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

    #[test]
    fn a_refusal_reads_as_its_text() {
        assert_eq!(answer(true, r#"{"a":1}"#), Ok(json!({ "a": 1 })));
        assert_eq!(answer(false, r#"{"error":"no"}"#), Err("no".into()));
        assert_eq!(answer(false, r#"{"code":7}"#), Err(r#"{"code":7}"#.into()));
    }
}
