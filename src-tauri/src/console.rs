//! The engine console (PLAN.md A2): containers from compose, functions and
//! trigger types from the engine, queues with their depth, and one container's
//! logs. Each read goes through the Engine seam.

use bastion_protocol::{
    Console, Container, LogCursor, LogLine, LogPage, LogRequest, Queue, Registered,
};
use serde_json::{Value, json};

use crate::engine::{DEFAULT_NAMESPACE, Engine};

/// The namespace the compose daemon and the queue worker answer in.
const STACK_NAMESPACE: &str = "bastion";
/// How long a log read waits for new lines. compose allows 5000.
const LOG_WAIT_MS: u64 = 5_000;
const LOG_TAIL: u64 = 200;

pub async fn read(engine: &dyn Engine) -> Result<Console, String> {
    let status = engine
        .call(STACK_NAMESPACE, "compose::status", json!({}))
        .await?;
    let functions = engine
        .call(DEFAULT_NAMESPACE, "engine::functions::list", json!({}))
        .await?;
    let triggers = engine
        .call(DEFAULT_NAMESPACE, "engine::triggers::list", json!({}))
        .await?;
    let topics = engine
        .call(STACK_NAMESPACE, "engine::queue::list_topics", json!({}))
        .await?;
    let mut queues = Vec::new();
    for name in topics
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str())
    {
        let stats = engine
            .call(
                STACK_NAMESPACE,
                "engine::queue::topic_stats",
                json!({ "topic": name }),
            )
            .await?;
        queues.push(queue(name, &stats));
    }
    Ok(Console {
        containers: containers(&status),
        functions: registered(&functions["functions"], "function_id"),
        trigger_types: registered(&triggers["triggers"], "id"),
        queues,
    })
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

fn containers(status: &Value) -> Vec<Container> {
    let rows = status["containers"].as_array().cloned().unwrap_or_default();
    rows.iter()
        .map(|c| Container {
            name: text(&c["container"]),
            version: c["version"].as_str().map(String::from),
            state: text(&c["state"]),
            owned: c["owned"] == true,
            last_error: c["last_error"].as_str().map(String::from),
        })
        .collect()
}

fn registered(rows: &Value, id_field: &str) -> Vec<Registered> {
    let mut out: Vec<Registered> = rows
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| Registered {
            id: text(&r[id_field]),
            namespace: text(&r["namespace"]),
            worker: text(&r["worker_name"]),
            description: text(&r["description"]),
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn queue(name: &str, stats: &Value) -> Queue {
    let n = |field: &str| stats[field].as_u64().unwrap_or(0);
    Queue {
        name: name.into(),
        depth: n("depth"),
        delivered: n("delivered"),
        failed: n("failed"),
        dead_letters: n("dlq_depth"),
    }
}

/// Read one container's logs. With a cursor, wait up to 5 s for new lines.
pub async fn logs(engine: &dyn Engine, request: &LogRequest) -> Result<LogPage, String> {
    let mut payload = json!({ "container": request.container });
    match &request.cursor {
        Some(cursor) => {
            payload["cursors"] = json!({ request.container.as_str(): cursor });
            payload["wait_ms"] = json!(LOG_WAIT_MS);
        }
        None => payload["tail"] = json!(LOG_TAIL),
    }
    let out = engine
        .call(STACK_NAMESPACE, "compose::logs", payload)
        .await?;
    let ours = out["containers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["container"] == request.container.as_str())
        .cloned()
        .unwrap_or(Value::Null);
    Ok(LogPage {
        lines: ours["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| LogLine {
                stream: text(&e["stream"]),
                message: text(&e["message"]),
            })
            .collect(),
        cursor: serde_json::from_value::<LogCursor>(ours["cursor"].clone())
            .ok()
            .or_else(|| request.cursor.clone()),
        truncated: ours["truncated"] == true,
    })
}

/// Stop or start one container. compose keeps its dependents consistent.
pub async fn set_running(engine: &dyn Engine, name: &str, running: bool) -> Result<(), String> {
    let function = if running {
        "compose::up"
    } else {
        "compose::down"
    };
    engine
        .call(STACK_NAMESPACE, function, json!({ "container": name }))
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeEngine;
    use bastion_protocol::Link;

    fn engine() -> FakeEngine {
        FakeEngine::new(Link::Up)
            .answer(
                "compose::status",
                Ok(json!({ "containers": [
                    { "container": "queue", "version": "0.21.26", "state": "ready", "owned": true },
                    { "container": "ade", "state": "failed", "owned": true, "last_error": "port 3114 in use" },
                ]})),
            )
            .answer(
                "engine::functions::list",
                Ok(json!({ "functions": [
                    { "function_id": "worktree::land", "namespace": "bastion", "worker_name": "worktree", "description": "Land" },
                    { "function_id": "approval::list", "namespace": "bastion", "worker_name": "approval-gate" },
                ]})),
            )
            .answer(
                "engine::triggers::list",
                Ok(json!({ "triggers": [{ "id": "state::on-changed", "namespace": "bastion", "worker_name": "state" }] })),
            )
            .answer(
                "engine::queue::list_topics",
                Ok(json!([{ "name": "bastion-steps" }])),
            )
            .answer(
                "engine::queue::topic_stats",
                Ok(json!({ "depth": 3, "delivered": 10, "failed": 1, "dlq_depth": 1 })),
            )
    }

    #[test]
    fn the_console_reads_every_part_in_one_call() {
        let console = tauri::async_runtime::block_on(read(&engine())).unwrap();
        assert_eq!(
            console.containers[1].last_error.as_deref(),
            Some("port 3114 in use")
        );
        assert_eq!(console.containers[0].version.as_deref(), Some("0.21.26"));
        assert_eq!(
            console
                .functions
                .iter()
                .map(|f| f.id.as_str())
                .collect::<Vec<_>>(),
            ["approval::list", "worktree::land"],
            "sorted by id"
        );
        assert_eq!(console.trigger_types[0].worker, "state");
        assert_eq!(
            console.queues,
            vec![Queue {
                name: "bastion-steps".into(),
                depth: 3,
                delivered: 10,
                failed: 1,
                dead_letters: 1
            }]
        );
    }

    #[test]
    fn a_failed_read_names_the_function() {
        let engine = FakeEngine::new(Link::Up);
        let error = tauri::async_runtime::block_on(read(&engine)).unwrap_err();
        assert!(error.contains("compose::status"), "{error}");
    }

    #[test]
    fn the_first_log_read_tails_and_the_next_one_waits() {
        let engine = FakeEngine::new(Link::Up).answer(
            "compose::logs",
            Ok(json!({ "containers": [{
                "container": "queue",
                "entries": [{ "stream": "stdout", "message": "ready" }],
                "cursor": { "generation": "g1", "offset": 42 },
                "truncated": false,
            }]})),
        );
        let first = LogRequest {
            container: "queue".into(),
            cursor: None,
        };
        let page = tauri::async_runtime::block_on(logs(&engine, &first)).unwrap();
        assert_eq!(page.lines[0].message, "ready");
        let next = LogRequest {
            container: "queue".into(),
            cursor: page.cursor,
        };
        tauri::async_runtime::block_on(logs(&engine, &next)).unwrap();
        let calls = engine.calls.lock().unwrap();
        assert_eq!(calls[0].2["tail"], 200);
        assert_eq!(calls[1].2["wait_ms"], 5000);
        assert_eq!(calls[1].2["cursors"]["queue"]["offset"], 42);
    }

    #[test]
    fn stop_and_start_name_one_container() {
        let engine = FakeEngine::new(Link::Up)
            .answer("compose::down", Ok(json!({})))
            .answer("compose::up", Ok(json!({})));
        tauri::async_runtime::block_on(set_running(&engine, "ade", false)).unwrap();
        tauri::async_runtime::block_on(set_running(&engine, "ade", true)).unwrap();
        let calls = engine.calls.lock().unwrap();
        assert_eq!(
            (calls[0].1.as_str(), &calls[0].2),
            ("compose::down", &json!({ "container": "ade" }))
        );
        assert_eq!(calls[1].1, "compose::up");
    }
}
