//! Plugins (D62): a git repository laid out as a Claude Code plugin,
//! installed on one server at a pinned commit. Each lives in its own
//! checkout under `<data>/plugins/<name>`, detached at its commit, so the
//! checkout is the record. Installing one adds nothing to a journey: its
//! parts are listed, off, for a person to apply.

use std::path::{Path, PathBuf};

use bastion_protocol::{PluginPart, PluginView};
use serde_json::Value;

use crate::machine::Machine;

/// Where a plugin says what it is.
const MANIFEST: &str = ".claude-plugin/plugin.json";

/// A plugin's name: lower case, digits and hyphens, as Claude Code's are.
fn name_ok(name: &str) -> bool {
    let fine = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    !name.is_empty() && name.len() <= 64 && !name.starts_with('-') && name.chars().all(fine)
}

/// A commit: 7 to 40 hex digits, so it can never read as an option.
fn commit_ok(commit: &str) -> bool {
    (7..=40).contains(&commit.len()) && commit.chars().all(|c| c.is_ascii_hexdigit())
}

/// Only https: git's other transports can run a command on this host.
fn url_ok(url: &str) -> bool {
    url.starts_with("https://") && !url.contains(char::is_whitespace)
}

fn git(machine: &dyn Machine, dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = machine.run("git", args, Some(dir))?;
    match out.success {
        true => Ok(out.text.trim().to_string()),
        false => Err(format!("git {}: {}", args.join(" "), out.text.trim())),
    }
}

/// The names under `dir` of `root`, each with `suffix` cut off, for the
/// files that have it.
fn names(machine: &dyn Machine, root: &Path, dir: &str, suffix: &str) -> Vec<String> {
    let all = machine.list(&root.join(dir)).into_iter();
    all.filter_map(|n| n.strip_suffix(suffix).map(String::from))
        .collect()
}

fn part(kind: &str, name: String, detail: String) -> PluginPart {
    PluginPart {
        kind: kind.into(),
        name,
        detail,
    }
}

/// Each command a hooks file runs, as `<event>: <command>`.
fn hooks(file: &Value) -> Vec<PluginPart> {
    let Some(events) = file["hooks"].as_object() else {
        return Vec::new();
    };
    let each = events.iter().flat_map(|(event, groups)| {
        let groups = groups.as_array().cloned().unwrap_or_default();
        let commands = groups.into_iter().flat_map(|g| {
            let matcher = g["matcher"].as_str().unwrap_or_default().to_string();
            let hooks = g["hooks"].as_array().cloned().unwrap_or_default();
            let run = hooks
                .into_iter()
                .filter_map(|h| h["command"].as_str().map(String::from));
            run.map(move |c| (matcher.clone(), c)).collect::<Vec<_>>()
        });
        let event = event.clone();
        commands.map(move |(matcher, command)| {
            let on = match matcher.is_empty() {
                true => event.clone(),
                false => format!("{event} ({matcher})"),
            };
            part("hook", on, command)
        })
    });
    each.collect()
}

/// The parts of the plugin checked out at `root`, whose manifest is
/// `manifest`: its skills, agents, commands, hooks and MCP servers, from
/// where Claude Code looks for them. Hooks and MCP servers may sit in the
/// manifest itself or in their own files.
pub fn parts(machine: &dyn Machine, root: &Path, manifest: &Value) -> Vec<PluginPart> {
    let mut all: Vec<PluginPart> = Vec::new();
    let skills = machine.list(&root.join("skills")).into_iter();
    let skills = skills.filter(|s| machine.exists(&root.join("skills").join(s).join("SKILL.md")));
    all.extend(skills.map(|s| part("skill", s, String::new())));
    for (kind, dir) in [("agent", "agents"), ("command", "commands")] {
        let each = names(machine, root, dir, ".md").into_iter();
        all.extend(each.map(|n| part(kind, n, String::new())));
    }
    let json = |name: &str| -> Value {
        let text = machine.read(&root.join(name)).unwrap_or_default();
        serde_json::from_str(&text).unwrap_or(Value::Null)
    };
    let inline = |key: &str, file: &str| match manifest[key].is_object() {
        true => serde_json::json!({ key: manifest[key].clone() }),
        false => json(file),
    };
    all.extend(hooks(&inline("hooks", "hooks/hooks.json")));
    if let Some(servers) = inline("mcpServers", ".mcp.json")["mcpServers"].as_object() {
        all.extend(
            servers
                .keys()
                .map(|n| part("mcp", n.clone(), String::new())),
        );
    }
    all
}

/// The plugin checked out at `root`, as the app shows it.
fn view(machine: &dyn Machine, root: &Path) -> Result<PluginView, String> {
    let text = machine.read(&root.join(MANIFEST))?;
    let manifest: Value = serde_json::from_str(&text).map_err(|e| format!("{MANIFEST}: {e}"))?;
    let name = manifest["name"].as_str().unwrap_or_default().to_string();
    Ok(PluginView {
        name,
        description: manifest["description"].as_str().unwrap_or_default().into(),
        url: git(machine, root, &["remote", "get-url", "origin"]).unwrap_or_default(),
        commit: git(machine, root, &["rev-parse", "HEAD"])?,
        parts: parts(machine, root, &manifest),
    })
}

/// Where plugins live on this server.
pub fn home(data: &Path) -> PathBuf {
    data.join("plugins")
}

/// Every plugin installed in `home`.
pub fn installed(machine: &dyn Machine, home: &Path) -> Vec<PluginView> {
    let dirs = machine.list(home).into_iter().filter(|d| name_ok(d));
    dirs.filter_map(|d| view(machine, &home.join(d)).ok())
        .collect()
}

/// What a person asks to install: a repository and the commit to pin.
pub struct Install<'a> {
    pub url: &'a str,
    pub commit: &'a str,
    /// A word that keeps two installs at once apart.
    pub stamp: &'a str,
}

/// Install the plugin at `ask.url`, pinned at `ask.commit`, into `home`.
/// A plugin already there by that name is replaced, which is how one is
/// moved to a new commit.
pub fn install(machine: &dyn Machine, home: &Path, ask: Install) -> Result<PluginView, String> {
    if !url_ok(ask.url) {
        return Err("a plugin's URL starts with https://".into());
    }
    if !commit_ok(ask.commit) {
        return Err("pin a commit: 7 to 40 hex digits".into());
    }
    machine.make_dir(home)?;
    let staged = home.join(format!(".install-{}", ask.stamp));
    let at = staged.display().to_string();
    let fetched = git(
        machine,
        home,
        &["clone", "-q", "--no-checkout", "--", ask.url, &at],
    )
    .and_then(|_| {
        git(
            machine,
            &staged,
            &["checkout", "-q", "--detach", ask.commit],
        )
    })
    .and_then(|_| view(machine, &staged));
    let placed = fetched.and_then(|v| match name_ok(&v.name) {
        true => place(machine, home, &staged, &v.name).map(|_| v),
        false => Err(format!("{:?} is not a plugin name", v.name)),
    });
    if placed.is_err() {
        let _ = machine.run("rm", &["-rf", &at], None);
    }
    placed
}

/// Move the checkout at `staged` to `home/<name>`, over any old one.
fn place(machine: &dyn Machine, home: &Path, staged: &Path, name: &str) -> Result<(), String> {
    let to = home.join(name).display().to_string();
    remove(machine, home, name)?;
    let out = machine.run("mv", &[&staged.display().to_string(), &to], None)?;
    match out.success {
        true => Ok(()),
        false => Err(format!("mv: {}", out.text.trim())),
    }
}

/// Remove plugin `name` from `home`; one already gone is not an error.
pub fn remove(machine: &dyn Machine, home: &Path, name: &str) -> Result<(), String> {
    if !name_ok(name) {
        return Err(format!("{name:?} is not a plugin name"));
    }
    let out = machine.run("rm", &["-rf", &home.join(name).display().to_string()], None)?;
    match out.success {
        true => Ok(()),
        false => Err(format!("rm: {}", out.text.trim())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::fake::FakeMachine;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn caveman() -> FakeMachine {
        let at = "/d/plugins/.install-1";
        FakeMachine::new()
            .run_ok("git", SHA)
            .run_ok("mv", "")
            .run_ok("rm", "")
            .file(
                &format!("{at}/{MANIFEST}"),
                r#"{"name": "caveman", "description": "Talk less."}"#,
            )
            .file(&format!("{at}/skills/caveman/SKILL.md"), "Be terse.")
            .file(&format!("{at}/commands/cave.md"), "Go terse.")
            .file(
                &format!("{at}/hooks/hooks.json"),
                r#"{"hooks": {"UserPromptSubmit": [{"hooks": [{"type": "command", "command": "./say.sh"}]}],
                   "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "./guard.sh"}]}]}}"#,
            )
            .file(&format!("{at}/.mcp.json"), r#"{"mcpServers": {"notes": {}}}"#)
    }

    #[test]
    fn a_plugin_installs_at_its_commit_and_lists_its_parts() {
        let machine = caveman();
        let ask = Install {
            url: "https://github.com/x/caveman",
            commit: SHA,
            stamp: "1",
        };
        let v = install(&machine, Path::new("/d/plugins"), ask).unwrap();
        assert_eq!((v.name.as_str(), v.commit.as_str()), ("caveman", SHA));
        assert!(machine.called(
            "git clone -q --no-checkout -- https://github.com/x/caveman /d/plugins/.install-1"
        ));
        assert!(machine.called(&format!("git checkout -q --detach {SHA}")));
        assert!(machine.called("mv /d/plugins/.install-1 /d/plugins/caveman"));
        let kinds: Vec<(&str, &str)> = v
            .parts
            .iter()
            .map(|p| (p.kind.as_str(), p.name.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("skill", "caveman"),
                ("command", "cave"),
                ("hook", "PreToolUse (Bash)"),
                ("hook", "UserPromptSubmit"),
                ("mcp", "notes"),
            ]
        );
        assert_eq!(v.parts[2].detail, "./guard.sh");
    }

    #[test]
    fn hooks_and_mcp_servers_may_sit_in_the_manifest() {
        let manifest = serde_json::json!({
            "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "node a.js"}]}]},
            "mcpServers": {"docs": {}}
        });
        let found = parts(&FakeMachine::new(), Path::new("/p"), &manifest);
        let kinds: Vec<(&str, &str)> = found
            .iter()
            .map(|p| (p.kind.as_str(), p.name.as_str()))
            .collect();
        assert_eq!(kinds, [("hook", "SessionStart"), ("mcp", "docs")]);
    }

    #[test]
    fn only_https_a_hex_commit_and_a_plain_name_are_taken() {
        let machine = FakeMachine::new();
        let home = Path::new("/d/plugins");
        let ask = |url, commit| Install {
            url,
            commit,
            stamp: "1",
        };
        let ext = install(&machine, home, ask("ext::sh -c touch% /tmp/x", SHA));
        assert!(ext.unwrap_err().contains("https://"));
        let option = install(&machine, home, ask("https://x/y", "--upload-pack=x"));
        assert!(option.unwrap_err().contains("hex"));
        assert!(remove(&machine, home, "../etc").is_err());
        assert!(machine.calls.lock().unwrap().is_empty(), "nothing ran");
    }
}
