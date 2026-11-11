//! One turn, step by step (F5): what the agent was asked, said and thought,
//! each call it made with the gate's verdict on it, and what each returned.

use bastion_protocol::{SessionDetail, TurnStep};
use serde_json::{Value, json};

use super::factory::{FactoryCell, factory};
use crate::engine::NS;
use crate::gate::call_of;

/// How much of a call's result a step keeps.
const RESULT_CHARS: usize = 2000;

pub async fn session(cell: &FactoryCell, id: String) -> Result<SessionDetail, String> {
    let factory = factory(cell)?;
    // A session is named `<job>-<step>-<random>`.
    let job = id.split('-').next().unwrap_or_default().to_string();
    let asked = json!({ "session_id": id });
    let out = factory
        .engine()
        .call(NS, "session::messages", asked)
        .await?;
    let gate = factory.store().gate_entries(&job).await?;
    let verdicts = gate.into_iter().map(|e| e.record.detail).collect();
    Ok(SessionDetail {
        steps: steps(&out["messages"], verdicts),
        session: id,
        job,
    })
}

/// The turn's messages as steps, each call with the first unused gate entry
/// that names it. The gate writes `<decision> <function> <text>`.
// ponytail: entries are the whole job's, so a later turn's call can take an
// earlier identical call's verdict; bound them by the turn's start if the
// two ever differ.
fn steps(messages: &Value, mut verdicts: Vec<String>) -> Vec<TurnStep> {
    let mut all = Vec::new();
    for m in messages.as_array().into_iter().flatten() {
        let role = m["message"]["role"].as_str().unwrap_or_default();
        for part in m["message"]["content"].as_array().into_iter().flatten() {
            all.extend(step(role, part, &mut verdicts));
        }
    }
    all
}

fn step(role: &str, part: &Value, verdicts: &mut Vec<String>) -> Option<TurnStep> {
    let text = part["text"].as_str().unwrap_or_default();
    let (kind, text, verdict) = match (role, part["type"].as_str()?) {
        (_, "function_call") => return Some(called(&part["arguments"], verdicts)),
        ("function_result", _) => ("result", clip(text), String::new()),
        ("user", _) => ("asked", text.into(), String::new()),
        (_, "thinking") => ("thought", text.into(), String::new()),
        _ => ("said", text.into(), String::new()),
    };
    Some(TurnStep {
        kind: kind.into(),
        text,
        verdict,
    })
}

/// A call as the gate read it, and the verdict it wrote for it.
fn called(arguments: &Value, verdicts: &mut Vec<String>) -> TurnStep {
    let function = arguments["function"].as_str().unwrap_or_default();
    let call = call_of(&json!({ "function_id": function, "arguments": arguments["payload"] }));
    let named = format!(" {} {}", call.function, call.text);
    let found = verdicts.iter().position(|v| v.contains(&named));
    let verdict = found.map(|i| verdicts.remove(i)).map(|v| {
        let at = v.find(&named).unwrap_or(v.len());
        v[..at].to_string()
    });
    TurnStep {
        kind: "call".into(),
        text: named.trim_start().to_string(),
        verdict: verdict.unwrap_or_default(),
    }
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(RESULT_CHARS) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_reads_as_steps_each_call_with_its_verdict() {
        let call = |cmd: &str| {
            json!({ "type": "function_call", "function_id": "agent_trigger",
            "arguments": { "function": "sandbox::exec", "payload": { "argv": cmd.split(' ').collect::<Vec<_>>() } } })
        };
        let messages = json!([
            { "message": { "role": "user", "content": [{ "type": "text", "text": "plan it" }] } },
            { "message": { "role": "assistant", "content": [
                { "type": "thinking", "text": "look first" },
                call("git status"), call("rm -rf /work") ] } },
            { "message": { "role": "function_result", "content": [{ "type": "text", "text": "clean" }] } },
            { "message": { "role": "assistant", "content": [{ "type": "text", "text": "done" }] } },
        ]);
        let gate = vec![
            "continue sandbox::exec git status".to_string(),
            "deny (no deleting) sandbox::exec rm -rf /work".to_string(),
        ];
        let got = steps(&messages, gate);
        let kinds: Vec<_> = got.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(
            kinds,
            ["asked", "thought", "call", "call", "result", "said"]
        );
        assert_eq!(
            (got[2].text.as_str(), got[2].verdict.as_str()),
            ("sandbox::exec git status", "continue")
        );
        assert_eq!(got[3].verdict, "deny (no deleting)");
        assert_eq!(got[4].text, "clean");
    }

    #[test]
    fn a_long_result_is_clipped() {
        let long = "x".repeat(RESULT_CHARS + 5);
        assert_eq!(clip(&long).chars().count(), RESULT_CHARS + 1);
        assert_eq!(clip("short"), "short");
    }
}
