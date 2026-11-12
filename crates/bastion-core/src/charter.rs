//! What a repository's own `.agents/settings.json` asks of a turn: its
//! `permissions` become constraints from the charter, and its hook matchers
//! map onto the sandbox functions a turn calls here.

use crate::govern::{Constraint, Enforcement, Source};

/// The sandbox function each tool name in a charter stands for.
const TOOLS: [(&str, &str); 7] = [
    ("Bash", "sandbox::exec"),
    ("Write", "sandbox::fs::write"),
    ("Edit", "sandbox::fs::write"),
    ("MultiEdit", "sandbox::fs::write"),
    ("Read", "sandbox::fs::read"),
    ("LS", "sandbox::fs::ls"),
    ("Glob", "sandbox::fs::ls"),
];

/// The charter files put before every brief, when present.
pub const BRIEF_FILES: [&str; 1] = ["AGENTS.md"];
/// The directory that holds the repository's settings, hooks, agents and
/// skills (D46).
pub const SETTINGS_DIR: &str = ".agents";

/// Why a write to `path` is refused, when it would change the charter: a
/// brief file at any depth, or anything under an `.agents` directory. The
/// factory reads the charter from the base commit, so this only stops the
/// agent from proposing its own rules in the change it hands back.
pub fn guards(path: &str) -> Option<String> {
    let mut parts = path.split('/').filter(|p| !p.is_empty() && *p != ".");
    let last = path.rsplit('/').next().unwrap_or_default();
    let charter = parts.any(|p| p == SETTINGS_DIR) || BRIEF_FILES.contains(&last);
    charter.then(|| {
        format!("{path} is part of the repository's charter, which a turn may not change.")
    })
}

/// Whether a person may edit `path` from the app (D58): `AGENTS.md`, the
/// settings, an agent's file, or a skill's `SKILL.md`. Nothing else, and
/// nothing that leaves the repository.
pub fn editable(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let plain = |p: &&str| !p.is_empty() && *p != "." && *p != "..";
    if !parts.iter().all(plain) {
        return false;
    }
    match parts.as_slice() {
        ["AGENTS.md"] | [".agents", "settings.json"] => true,
        [".agents", "agents", name] => name.ends_with(".md"),
        [".agents", "skills", _, "SKILL.md"] => true,
        _ => false,
    }
}

/// The tool name for a sandbox function, as a hook expects it.
pub fn tool_name(function: &str) -> &'static str {
    TOOLS
        .iter()
        .find(|(_, f)| *f == function)
        .map_or("Bash", |(t, _)| t)
}

/// The sandbox functions a hook matcher such as `Bash|Write` or `*` covers.
pub fn hook_functions(matcher: &str) -> Vec<&'static str> {
    let names: Vec<&str> = matcher.split('|').map(str::trim).collect();
    let all = matcher.trim().is_empty() || names.contains(&"*");
    let mut functions: Vec<&'static str> = TOOLS
        .iter()
        .filter(|(tool, _)| all || names.contains(tool))
        .map(|(_, f)| *f)
        .collect();
    functions.dedup();
    functions
}

/// The repository's `deny` and `ask` rules as constraints. A `Bash(...)` rule
/// binds the command's words; any other rule can only be told, for now. An
/// unattended turn has no one to ask, so `ask` refuses like `deny`.
pub fn constraints(deny: &[String], ask: &[String]) -> Vec<Constraint> {
    let rules = deny
        .iter()
        .map(|r| (r, "denies"))
        .chain(ask.iter().map(|r| (r, "asks first for")));
    rules
        .enumerate()
        .map(|(i, (rule, verb))| match bash_words(rule) {
            Some(words) => Constraint {
                id: format!("charter-{}", i + 1),
                rule: format!("The repository {verb} {rule}."),
                function: "sandbox::exec".into(),
                contains: words,
                rung: Enforcement::Enforced,
                source: Source::Charter,
                // A charter's rules hold at each of its workspace's steps (D58).
                steps: vec![crate::govern::EVERY.into()],
                hook: String::new(),
                reviewer: String::new(),
                capability: false,
            },
            None => Constraint {
                id: format!("charter-{}", i + 1),
                rule: format!(
                    "The repository {verb} {rule}. Bastion tells the agent but cannot bind it yet."
                ),
                function: String::new(),
                contains: String::new(),
                rung: Enforcement::Told,
                source: Source::Charter,
                steps: vec![crate::govern::EVERY.into()],
                hook: String::new(),
                reviewer: String::new(),
                capability: false,
            },
        })
        .collect()
}

/// `Bash(git push:*)` gives `git push`; `Bash` alone gives no words.
fn bash_words(rule: &str) -> Option<String> {
    let inner = rule.strip_prefix("Bash(")?.strip_suffix(')')?;
    let words = inner.trim_end_matches('*').trim_end_matches(':').trim();
    (!words.is_empty()).then(|| words.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_person_edits_only_the_charter_files() {
        for ok in [
            "AGENTS.md",
            ".agents/settings.json",
            ".agents/agents/strict.md",
            ".agents/skills/root-cause/SKILL.md",
        ] {
            assert!(editable(ok), "{ok}");
        }
        for no in [
            "src/main.rs",
            "/etc/passwd",
            "../AGENTS.md",
            ".agents/../AGENTS.md",
            "sub/AGENTS.md",
            ".agents/agents/a/b.md",
            ".agents/skills/x/run.sh",
            ".agents//settings.json",
        ] {
            assert!(!editable(no), "{no}");
        }
    }
    use crate::Stage;
    use crate::govern::{Call, Decision, default_capabilities, gate};

    #[test]
    fn hook_matchers_cover_the_sandbox_functions() {
        assert_eq!(hook_functions("Bash"), ["sandbox::exec"]);
        assert_eq!(hook_functions("Write|Edit"), ["sandbox::fs::write"]);
        assert_eq!(hook_functions("*").len(), 4);
        assert_eq!(hook_functions("").len(), 4);
        assert!(hook_functions("WebFetch").is_empty());
        assert_eq!(tool_name("sandbox::fs::write"), "Write");
    }

    #[test]
    fn a_turn_may_not_write_the_charter() {
        let write = |path: &str| Call {
            content: String::new(),
            function: "sandbox::fs::write".into(),
            text: path.into(),
        };
        for path in [
            "/work/repo/AGENTS.md",
            "AGENTS.md",
            "/work/repo/sub/AGENTS.md",
            "/work/repo/.agents/settings.json",
            "./.agents/hooks/check.sh",
        ] {
            let verdict = gate(&write(path), Stage::Prompt, &[], &default_capabilities());
            assert!(
                matches!(verdict.decision, Decision::Deny { .. }),
                "{path} must be refused"
            );
        }
        for path in [
            "/work/repo/src/agents.rs",
            "/work/repo/docs/AGENTS.md.txt",
            "/work/repo/CLAUDE.md",
        ] {
            let verdict = gate(&write(path), Stage::Prompt, &[], &default_capabilities());
            assert_eq!(verdict.decision, Decision::Continue, "{path}");
        }
    }

    #[test]
    fn a_denied_bash_rule_refuses_the_command_and_others_are_told() {
        let rules = constraints(
            &["Bash(git push:*)".into(), "Read(./.env)".into()],
            &["Bash(rm -rf)".into()],
        );
        assert_eq!(rules[0].contains, "git push");
        assert_eq!(rules[1].rung, Enforcement::Told);
        assert_eq!(rules[2].contains, "rm -rf");
        let push = Call {
            content: String::new(),
            function: "sandbox::exec".into(),
            text: "git -C /work/repo push origin".into(),
        };
        let Decision::Deny { reason } =
            gate(&push, Stage::Prompt, &rules, &default_capabilities()).decision
        else {
            panic!("a denied command must be refused");
        };
        assert!(reason.contains("Bash(git push:*)"), "{reason}");
    }
}
