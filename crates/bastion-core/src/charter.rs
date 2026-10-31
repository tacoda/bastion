//! What a repository's own `.claude/settings.json` asks of a turn (PLAN.md
//! A5): its `permissions` become constraints from the charter, and its hook
//! matchers map onto the sandbox functions a turn calls here.

use crate::govern::{Constraint, Enforcement, Source};

/// The sandbox function each Claude Code tool name stands for.
const TOOLS: [(&str, &str); 7] = [
    ("Bash", "sandbox::exec"),
    ("Write", "sandbox::fs::write"),
    ("Edit", "sandbox::fs::write"),
    ("MultiEdit", "sandbox::fs::write"),
    ("Read", "sandbox::fs::read"),
    ("LS", "sandbox::fs::ls"),
    ("Glob", "sandbox::fs::ls"),
];

/// The Claude Code tool name for a sandbox function, as a hook expects it.
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
    fn a_denied_bash_rule_refuses_the_command_and_others_are_told() {
        let rules = constraints(
            &["Bash(git push:*)".into(), "Read(./.env)".into()],
            &["Bash(rm -rf)".into()],
        );
        assert_eq!(rules[0].contains, "git push");
        assert_eq!(rules[1].rung, Enforcement::Told);
        assert_eq!(rules[2].contains, "rm -rf");
        let push = Call {
            function: "sandbox::exec".into(),
            text: "git -C /work/repo push origin".into(),
        };
        let Decision::Deny { reason } =
            gate(&push, Stage::Build, &rules, &default_capabilities()).decision
        else {
            panic!("a denied command must be refused");
        };
        assert!(reason.contains("Bash(git push:*)"), "{reason}");
    }
}
