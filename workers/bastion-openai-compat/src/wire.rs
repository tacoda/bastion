//! Chat Completions on one side, llm-router's provider protocol on the
//! other. Pure functions: no engine, no network.

use serde_json::{Value, json};

/// A catalog entry gets this when the host does not say.
// ponytail: one context window for every model; read it from the host's /models when one reports it.
const CONTEXT_WINDOW: u64 = 128_000;
pub const MAX_TOKENS: u64 = 8192;

/// `/models` beside the `/chat/completions` URL the host is given as.
pub fn models_url(api_url: &str) -> String {
    let base = api_url.trim_end_matches('/');
    let base = base.strip_suffix("/chat/completions").unwrap_or(base);
    format!("{base}/models")
}

/// The router's catalog from a host's `/models` answer. A model's id is
/// `<host>/<model>`, so two hosts serving the same model never make the
/// router's choice ambiguous.
pub fn catalog(host: &str, listing: &Value, max_tokens: u64) -> Vec<Value> {
    let ids = listing["data"].as_array().into_iter().flatten();
    ids.filter_map(|m| m["id"].as_str())
        .map(|id| {
            json!({
                "id": format!("{host}/{id}"),
                "provider": host,
                "display_name": id,
                "context_window": CONTEXT_WINDOW,
                "max_output_tokens": max_tokens,
                "supports_tools": true,
            })
        })
        .collect()
}

/// A tool name the host accepts: `agent::trigger` goes as `agent__trigger`.
fn encode(name: &Value) -> String {
    name.as_str().unwrap_or_default().replace("::", "__")
}

fn decode(name: &Value) -> String {
    name.as_str().unwrap_or_default().replace("__", "::")
}

fn texts(content: &Value) -> String {
    let blocks = content.as_array().into_iter().flatten();
    let text: Vec<&str> = blocks
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    text.join("\n")
}

fn tool_call(block: &Value) -> Value {
    json!({
        "id": block["id"],
        "type": "function",
        "function": {
            "name": encode(&block["function_id"]),
            "arguments": block["arguments"].to_string(),
        },
    })
}

fn assistant(m: &Value) -> Option<Value> {
    let blocks = m["content"].as_array().into_iter().flatten();
    let calls: Vec<Value> = blocks
        .filter(|b| b["type"] == "function_call")
        .map(tool_call)
        .collect();
    let text = texts(&m["content"]);
    if text.is_empty() && calls.is_empty() {
        return None;
    }
    let mut out = json!({ "role": "assistant", "content": text });
    if !calls.is_empty() {
        out["tool_calls"] = Value::Array(calls);
    }
    Some(out)
}

/// One router message as one Chat Completions message, or none.
// ponytail: images and orphaned tool calls are dropped; add them when a host needs them.
fn message(m: &Value) -> Option<Value> {
    match m["role"].as_str()? {
        "user" => Some(json!({ "role": "user", "content": texts(&m["content"]) })),
        "assistant" => assistant(m),
        "function_result" => Some(json!({
            "role": "tool",
            "tool_call_id": m["function_call_id"],
            "content": texts(&m["content"]),
        })),
        _ => None,
    }
}

/// The Chat Completions body for one `provider::<id>::stream` input.
pub fn request(turn: &Turn, input: &Value, max_tokens: u64) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = input["system_prompt"].as_str().filter(|s| !s.is_empty()) {
        messages.push(json!({ "role": "system", "content": system }));
    }
    let given = input["messages"].as_array().into_iter().flatten();
    messages.extend(given.filter_map(message));
    let model = turn.upstream();
    let max = input["max_output_tokens"].as_u64().unwrap_or(max_tokens);
    let mut body =
        json!({ "model": model, "messages": messages, "max_tokens": max, "stream": false });
    let tools: Vec<Value> = input["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            json!({ "type": "function", "function": {
                "name": encode(&t["name"]),
                "description": t["description"],
                "parameters": t["parameters"],
            }})
        })
        .collect();
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    body
}

fn stop_reason(finish: &Value) -> &'static str {
    match finish.as_str() {
        Some("length") => "length",
        Some("tool_calls" | "function_call") => "function_call",
        _ => "end",
    }
}

/// The router's kind for a failed HTTP status: it retries only
/// `rate_limited` and `transient`.
pub fn error_kind(status: u16) -> &'static str {
    match status {
        401 | 403 => "auth_expired",
        429 => "rate_limited",
        408 | 500..=599 => "transient",
        _ => "permanent",
    }
}

fn now() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |d| d.as_millis() as i64)
}

fn called(call: &Value) -> Value {
    let raw = call["function"]["arguments"].as_str().unwrap_or("{}");
    let arguments = serde_json::from_str(raw).unwrap_or_else(|_| json!({}));
    json!({
        "type": "function_call",
        "id": call["id"],
        "function_id": decode(&call["function"]["name"]),
        "arguments": arguments,
    })
}

/// One turn: the host that answers it and the catalog model asked for.
pub struct Turn<'a> {
    pub host: &'a str,
    pub model: &'a str,
}

impl Turn<'_> {
    /// The host's own name for the catalog model.
    pub fn upstream(&self) -> &str {
        let own = self.model.strip_prefix(self.host);
        own.and_then(|m| m.strip_prefix('/')).unwrap_or(self.model)
    }

    /// An assistant message as the router expects one.
    pub fn reply(&self, content: Vec<Value>, stop: &str) -> Value {
        json!({
            "role": "assistant",
            "content": content,
            "stop_reason": stop,
            "model": self.model,
            "provider": self.host,
            "timestamp": now(),
        })
    }

    /// The `error` frames for a call that failed: a stop, then the terminal.
    pub fn failure(&self, kind: &str, why: &str) -> Vec<Value> {
        let mut error = self.reply(vec![], "error");
        error["error_kind"] = json!(kind);
        error["error_message"] = json!(why);
        vec![
            json!({ "type": "stop", "stop_reason": "error", "error_kind": kind, "error_message": why }),
            json!({ "type": "error", "error": error }),
        ]
    }

    /// The frames for one whole Chat Completions answer, ending with `done`.
    // ponytail: the host answers whole (stream: false), so text comes as one delta; parse SSE when token-by-token display matters.
    pub fn frames(&self, answer: &Value) -> Vec<Value> {
        let choice = &answer["choices"][0];
        let text = choice["message"]["content"].as_str().unwrap_or_default();
        let calls = choice["message"]["tool_calls"]
            .as_array()
            .into_iter()
            .flatten();
        let stop = stop_reason(&choice["finish_reason"]);
        let mut content = Vec::new();
        let mut out = vec![json!({ "type": "start", "partial": self.reply(vec![], stop) })];
        if !text.is_empty() {
            content.push(json!({ "type": "text", "text": text }));
            let partial = self.reply(content.clone(), stop);
            out.push(json!({ "type": "text_start", "partial": partial }));
            out.push(json!({ "type": "text_delta", "delta": text }));
            out.push(json!({ "type": "text_end", "partial": partial }));
        }
        for call in calls {
            content.push(called(call));
            let partial = self.reply(content.clone(), stop);
            out.push(json!({ "type": "functioncall_start", "partial": partial }));
            out.push(json!({ "type": "functioncall_end", "partial": partial }));
        }
        let usage = json!({
            "input": answer["usage"]["prompt_tokens"],
            "output": answer["usage"]["completion_tokens"],
        });
        let mut message = self.reply(content, stop);
        message["usage"] = usage.clone();
        out.push(json!({ "type": "usage", "usage": usage }));
        out.push(json!({ "type": "stop", "stop_reason": stop }));
        out.push(json!({ "type": "done", "message": message }));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catalog_model_names_its_host_and_goes_upstream_without_it() {
        let listing = json!({ "data": [{ "id": "qwen-max" }, { "id": "llama3:8b" }] });
        let models = catalog("dashscope", &listing, 4096);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["id"], "dashscope/qwen-max");
        assert_eq!(models[0]["provider"], "dashscope");
        assert_eq!(models[1]["max_output_tokens"], 4096);
        let turn = |model| Turn {
            host: "dashscope",
            model,
        };
        assert_eq!(turn("dashscope/qwen-max").upstream(), "qwen-max");
        assert_eq!(turn("llama3:8b").upstream(), "llama3:8b");
    }

    #[test]
    fn models_sit_beside_chat_completions() {
        let url = "http://localhost:11434/v1/chat/completions";
        assert_eq!(models_url(url), "http://localhost:11434/v1/models");
        assert_eq!(models_url("https://h/v1/"), "https://h/v1/models");
    }

    #[test]
    fn a_turn_with_a_tool_call_and_its_result_maps_to_chat_completions() {
        let input = json!({
            "model": "ollama/llama3",
            "system_prompt": "Be brief.",
            "messages": [
                { "role": "user", "content": [{ "type": "text", "text": "hi" }], "timestamp": 0 },
                { "role": "assistant", "content": [{ "type": "function_call", "id": "c1",
                  "function_id": "sandbox::exec", "arguments": { "cmd": "ls" } }] },
                { "role": "function_result", "function_call_id": "c1", "function_id": "sandbox::exec",
                  "content": [{ "type": "text", "text": "a.py" }], "details": {}, "is_error": false },
            ],
            "tools": [{ "name": "sandbox::exec", "description": "Run", "parameters": { "type": "object" } }],
        });
        let turn = Turn {
            host: "ollama",
            model: "ollama/llama3",
        };
        let body = request(&turn, &input, MAX_TOKENS);
        assert_eq!(body["model"], "llama3");
        assert_eq!(body["max_tokens"], 8192);
        let m = &body["messages"];
        assert_eq!(m[0]["role"], "system");
        assert_eq!(m[1]["content"], "hi");
        assert_eq!(m[2]["tool_calls"][0]["function"]["name"], "sandbox__exec");
        assert_eq!(
            m[2]["tool_calls"][0]["function"]["arguments"],
            r#"{"cmd":"ls"}"#
        );
        assert_eq!(m[3]["role"], "tool");
        assert_eq!(m[3]["tool_call_id"], "c1");
        assert_eq!(body["tools"][0]["function"]["name"], "sandbox__exec");
    }

    #[test]
    fn an_answer_with_a_tool_call_ends_in_done_with_the_call_decoded() {
        let answer = json!({
            "choices": [{ "finish_reason": "tool_calls", "message": {
                "content": "Looking.",
                "tool_calls": [{ "id": "c2", "type": "function",
                  "function": { "name": "sandbox__exec", "arguments": "{\"cmd\":\"ls\"}" } }],
            }}],
            "usage": { "prompt_tokens": 10, "completion_tokens": 3 },
        });
        let out = Turn {
            host: "ollama",
            model: "ollama/llama3",
        }
        .frames(&answer);
        let kinds: Vec<&str> = out.iter().map(|f| f["type"].as_str().unwrap()).collect();
        assert_eq!(kinds.first(), Some(&"start"));
        assert_eq!(kinds.last(), Some(&"done"));
        assert!(kinds.contains(&"text_delta") && kinds.contains(&"functioncall_end"));
        let done = &out.last().unwrap()["message"];
        assert_eq!(done["stop_reason"], "function_call");
        assert_eq!(done["provider"], "ollama");
        assert_eq!(done["content"][1]["function_id"], "sandbox::exec");
        assert_eq!(done["content"][1]["arguments"]["cmd"], "ls");
        assert_eq!(done["usage"]["input"], 10);
    }

    #[test]
    fn a_failed_call_ends_in_one_error_frame_the_router_can_retry() {
        assert_eq!(error_kind(429), "rate_limited");
        assert_eq!(error_kind(503), "transient");
        assert_eq!(error_kind(401), "auth_expired");
        assert_eq!(error_kind(400), "permanent");
        let out = Turn {
            host: "h",
            model: "h/m",
        }
        .failure("transient", "502 Bad Gateway");
        assert_eq!(out.last().unwrap()["type"], "error");
        assert_eq!(out.last().unwrap()["error"]["error_kind"], "transient");
    }
}
