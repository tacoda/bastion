//! A workspace's charter as read from its worktree (PLAN.md A5): the files
//! the brief carries, and the repository's own `.claude/settings.json`, whose
//! permissions become constraints and whose hooks run in the job's VM.

use std::path::Path;

use bastion_core::charter;
use bastion_core::govern::Constraint;
use serde_json::{Value, json};

use crate::engine::{DEFAULT_NAMESPACE, Engine};
use crate::machine::Machine;

const HOOK_MS: u64 = 60_000;

/// The charter files put before every brief, when present.
pub const BRIEF_FILES: [&str; 2] = ["CLAUDE.md", "AGENTS.md"];
pub const SETTINGS: &str = ".claude/settings.json";

/// A repository hook: a command, and the sandbox functions its matcher covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hook {
    pub functions: Vec<&'static str>,
    pub command: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Charter {
    /// The charter files found, with a note when one could not be read.
    pub files: Vec<String>,
    pub constraints: Vec<Constraint>,
    /// `PreToolUse` hooks: they run before a matching call, and exit 2 refuses it.
    pub before: Vec<Hook>,
    /// `Stop` hooks: they run when a build turn ends, and their results are kept.
    pub stop: Vec<String>,
}

pub fn read(machine: &dyn Machine, worktree: &Path) -> Charter {
    let mut found = Charter::default();
    for name in BRIEF_FILES {
        if machine.exists(&worktree.join(name)) {
            found.files.push(name.into());
        }
    }
    let Ok(text) = machine.read(&worktree.join(SETTINGS)) else {
        return found;
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(settings) => {
            found.files.push(SETTINGS.into());
            let list = |key: &str| -> Vec<String> {
                settings["permissions"][key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r.as_str().map(String::from))
                    .collect()
            };
            found.constraints = charter::constraints(&list("deny"), &list("ask"));
            found.before = hooks(&settings, "PreToolUse");
            found.stop = hooks(&settings, "Stop")
                .into_iter()
                .map(|h| h.command)
                .collect();
        }
        Err(e) => found
            .files
            .push(format!("{SETTINGS} (not valid JSON: {e})")),
    }
    found
}

/// Every command hook under one event, with the functions its matcher covers.
fn hooks(settings: &Value, event: &str) -> Vec<Hook> {
    let groups = settings["hooks"][event].as_array().into_iter().flatten();
    groups
        .flat_map(|group| {
            let functions = charter::hook_functions(group["matcher"].as_str().unwrap_or(""));
            group["hooks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|h| h["type"] == "command")
                .filter_map(|h| h["command"].as_str())
                .map(move |command| Hook {
                    functions: functions.clone(),
                    command: command.into(),
                })
        })
        .collect()
}

/// How one hook run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ran {
    pub command: String,
    /// The exit code, or None when the hook could not run at all.
    pub code: Option<i64>,
    pub output: String,
}

impl Ran {
    /// Exit 2 refuses, as in Claude Code. Any other failure is recorded and
    /// stood aside from, never fatal.
    pub fn refuses(&self) -> bool {
        self.code == Some(2)
    }

    pub fn line(&self) -> String {
        let code = self
            .code
            .map_or("did not run".to_string(), |c| format!("exit {c}"));
        format!("{} ({code}) {}", self.command, self.output.trim())
            .trim_end()
            .to_string()
    }
}

/// Run one hook in the job's VM from the repository's root, with the hook's
/// JSON input on stdin.
pub async fn run(engine: &dyn Engine, sandbox: &str, command: &str, input: &Value) -> Ran {
    let script = format!("cd /work/repo && printf '%s' \"$BASTION_HOOK_INPUT\" | {command}");
    let payload = json!({
        "sandbox_id": sandbox, "argv": ["sh", "-c", script], "timeout_ms": HOOK_MS,
        "env": { "BASTION_HOOK_INPUT": input.to_string(), "CLAUDE_PROJECT_DIR": "/work/repo" },
    });
    let ran = engine
        .call_for(DEFAULT_NAMESPACE, "sandbox::exec", payload, HOOK_MS + 5_000)
        .await;
    match ran {
        Ok(out) => Ran {
            command: command.into(),
            code: out["exit_code"].as_i64(),
            output: format!(
                "{}{}",
                out["stdout"].as_str().unwrap_or(""),
                out["stderr"].as_str().unwrap_or("")
            ),
        },
        Err(e) => Ran {
            command: command.into(),
            code: None,
            output: e,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::fake::FakeMachine;

    const SETTINGS_JSON: &str = r#"{
        "permissions": { "deny": ["Bash(git push:*)"], "ask": ["Edit(./secrets/**)"] },
        "hooks": {
            "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "./check.sh" }] }],
            "Stop": [{ "hooks": [{ "type": "command", "command": "python3 -m pytest -q" }] }]
        }
    }"#;

    #[test]
    fn settings_give_constraints_and_hooks() {
        let machine = FakeMachine::new()
            .file("/wt/CLAUDE.md", "# rules")
            .file("/wt/.claude/settings.json", SETTINGS_JSON);
        let charter = read(&machine, Path::new("/wt"));
        assert_eq!(charter.files, ["CLAUDE.md", ".claude/settings.json"]);
        assert_eq!(charter.constraints.len(), 2);
        assert_eq!(charter.constraints[0].contains, "git push");
        assert_eq!(
            charter.before,
            [Hook {
                functions: vec!["sandbox::exec"],
                command: "./check.sh".into()
            }]
        );
        assert_eq!(charter.stop, ["python3 -m pytest -q"]);
    }

    #[test]
    fn no_charter_reads_as_empty_and_bad_json_says_so() {
        assert_eq!(
            read(&FakeMachine::new(), Path::new("/wt")),
            Charter::default()
        );
        let bad = FakeMachine::new().file("/wt/.claude/settings.json", "{ nope");
        assert!(read(&bad, Path::new("/wt")).files[0].contains("not valid JSON"));
    }
}
