//! A small MCP client over streamable HTTP: initialize, list the tools,
//! call one. A reply comes as JSON or as an event stream; either way the
//! message with the request's id is the answer.
// ponytail: no server-to-client requests and no resumed streams; add them when a server needs them.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

const VERSION: &str = "2025-06-18";
const SESSION: &str = "mcp-session-id";

/// One MCP server, connected.
pub struct Client {
    http: reqwest::Client,
    url: String,
    token: Option<String>,
    session: Mutex<Option<String>>,
    next: AtomicU64,
}

/// The JSON-RPC message with `id` in a reply body, as JSON or as an event
/// stream of `data:` lines.
fn answer_in(body: &str, id: u64) -> Option<Value> {
    let parsed: Vec<Value> = match serde_json::from_str::<Value>(body) {
        Ok(v) => vec![v],
        Err(_) => body
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str(d.trim()).ok())
            .collect(),
    };
    parsed.into_iter().find(|m| m["id"] == id)
}

/// The result of a JSON-RPC answer, or its error's message.
fn result_of(answer: Value) -> Result<Value, String> {
    match answer.get("error") {
        Some(e) => Err(e["message"].as_str().unwrap_or("the server failed").into()),
        None => Ok(answer["result"].clone()),
    }
}

impl Client {
    /// Connect to the server at `url` and say hello, as MCP asks.
    pub async fn connect(url: &str, token: Option<String>) -> Result<Self, String> {
        let client = Client {
            http: reqwest::Client::new(),
            url: url.into(),
            token,
            session: Mutex::new(None),
            next: AtomicU64::new(1),
        };
        let hello = json!({
            "protocolVersion": VERSION,
            "capabilities": {},
            "clientInfo": { "name": "bastion-mcp", "version": env!("CARGO_PKG_VERSION") },
        });
        client.request("initialize", hello).await?;
        client
            .post(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await?;
        Ok(client)
    }

    async fn post(&self, message: Value) -> Result<reqwest::Response, String> {
        let mut post = self
            .http
            .post(&self.url)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", VERSION)
            .json(&message);
        if let Some(token) = &self.token {
            post = post.bearer_auth(token);
        }
        if let Some(session) = self.session.lock().unwrap().clone() {
            post = post.header(SESSION, session);
        }
        let reply = post.send().await.map_err(|e| e.to_string())?;
        if let Some(session) = reply.headers().get(SESSION).and_then(|v| v.to_str().ok()) {
            *self.session.lock().unwrap() = Some(session.into());
        }
        match reply.status().is_success() {
            true => Ok(reply),
            false => Err(format!("the server answered {}", reply.status())),
        }
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let body = self
            .post(message)
            .await?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let answer = answer_in(&body, id).ok_or(format!("no answer to {method}"))?;
        result_of(answer)
    }

    /// Every tool the server lists, page by page.
    pub async fn tools(&self) -> Result<Vec<Value>, String> {
        let mut all = Vec::new();
        let mut cursor = Value::Null;
        loop {
            let params = match &cursor {
                Value::Null => json!({}),
                c => json!({ "cursor": c }),
            };
            let page = self.request("tools/list", params).await?;
            all.extend(page["tools"].as_array().cloned().unwrap_or_default());
            cursor = page["nextCursor"].clone();
            if !cursor.is_string() {
                return Ok(all);
            }
        }
    }

    /// Call tool `name` with `arguments`, and its result as MCP gives it.
    pub async fn call(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let params = json!({ "name": name, "arguments": arguments });
        self.request("tools/call", params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_is_the_message_with_the_request_s_id() {
        let plain = r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#;
        assert_eq!(answer_in(plain, 2).unwrap()["result"]["tools"], json!([]));
        let stream = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{}}\n";
        assert!(answer_in(stream, 3).is_some());
        assert!(answer_in(stream, 4).is_none());
        let failed = json!({ "id": 1, "error": { "message": "no such tool" } });
        assert_eq!(result_of(failed), Err("no such tool".into()));
    }
}
