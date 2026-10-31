//! Constraints, capabilities and the gate (PLAN.md D20). Every call a turn
//! makes passes the gate, and each rule's rung on its ladder is data, so a
//! change in behavior traces to a change in that data.

use serde::{Deserialize, Serialize};

use crate::Stage;

/// How hard a constraint binds: the agent is told it, a call that breaks it
/// is recorded, or that call is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Enforcement {
    Told,
    Checked,
    Enforced,
}

impl Enforcement {
    pub fn promote(self) -> Self {
        match self {
            Enforcement::Told => Enforcement::Checked,
            _ => Enforcement::Enforced,
        }
    }

    pub fn demote(self) -> Self {
        match self {
            Enforcement::Enforced => Enforcement::Checked,
            _ => Enforcement::Told,
        }
    }
}

/// What a grant lets a call do: refused, held for the owner, or let through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Denied,
    Ask,
    Allowed,
}

impl Access {
    pub fn promote(self) -> Self {
        match self {
            Access::Denied => Access::Ask,
            _ => Access::Allowed,
        }
    }

    pub fn demote(self) -> Self {
        match self {
            Access::Allowed => Access::Ask,
            _ => Access::Denied,
        }
    }
}

/// Where a constraint comes from: the workspace's charter, or the owner's
/// defaults when the workspace has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Charter,
    Owner,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraint {
    pub id: String,
    /// What the agent is told.
    pub rule: String,
    /// The calls it binds: a function pattern (see [`matches`]) and text the
    /// call contains. An empty pattern binds no call, so the rule can only be
    /// told.
    pub function: String,
    pub contains: String,
    pub rung: Enforcement,
    pub source: Source,
}

/// A grant: what calls to a function pattern may do at one stage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub stage: Stage,
    pub function: String,
    pub rung: Access,
}

/// One call a turn makes. `text` is its arguments as the rules read them,
/// such as a command line or a path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub function: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum Decision {
    Continue,
    Deny {
        reason: String,
    },
    /// Wait for the owner: retry with feedback, allow this call once, or stop.
    Hold {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub decision: Decision,
    /// The constraints at `checked` that the call breaks, for the record.
    pub flagged: Vec<String>,
}

/// `name` matches a pattern that equals it, or a pattern ending in `*` that
/// it starts with. The empty pattern matches nothing.
pub fn matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => !pattern.is_empty() && name.starts_with(prefix),
        None => !pattern.is_empty() && pattern == name,
    }
}

/// Judge one call. An enforced constraint refuses it first. Then the most
/// specific grant for the stage decides, and a call no grant covers is
/// refused.
pub fn gate(
    call: &Call,
    stage: Stage,
    constraints: &[Constraint],
    grants: &[Capability],
) -> Verdict {
    let broken = || {
        constraints
            .iter()
            .filter(|c| matches(&c.function, &call.function) && call.text.contains(&c.contains))
    };
    let flagged = broken()
        .filter(|c| c.rung == Enforcement::Checked)
        .map(|c| c.id.clone())
        .collect();
    let decision = if let Some(c) = broken().find(|c| c.rung == Enforcement::Enforced) {
        Decision::Deny {
            reason: format!("Refused by constraint {}: {}", c.id, c.rule),
        }
    } else {
        let grant = grants
            .iter()
            .filter(|g| g.stage == stage && matches(&g.function, &call.function))
            .max_by_key(|g| (!g.function.ends_with('*'), g.function.len()));
        match grant.map(|g| g.rung) {
            Some(Access::Allowed) => Decision::Continue,
            Some(Access::Ask) => Decision::Hold {
                reason: format!("{} at {stage:?} waits for the owner.", call.function),
            },
            Some(Access::Denied) => Decision::Deny {
                reason: format!("{} is not granted at {stage:?}.", call.function),
            },
            None => Decision::Deny {
                reason: format!("No grant covers {} at {stage:?}.", call.function),
            },
        }
    };
    Verdict { decision, flagged }
}

/// The owner's default constraints, from what agents did in A4 (PLAN.md).
pub fn default_constraints() -> Vec<Constraint> {
    let rule = |id: &str, rule: &str, function: &str, contains: &str, rung| Constraint {
        id: id.into(),
        rule: rule.into(),
        function: function.into(),
        contains: contains.into(),
        rung,
        source: Source::Owner,
    };
    vec![
        rule(
            "no-login-shell",
            "Run commands directly, not through bash -lc.",
            "sandbox::exec",
            "bash -lc",
            Enforcement::Enforced,
        ),
        rule(
            "keep-tests",
            "Do not delete existing tests.",
            "sandbox::exec",
            "git rm",
            Enforcement::Told,
        ),
        rule(
            "commit-work",
            "Commit every change before you reply.",
            "",
            "",
            Enforcement::Told,
        ),
        rule(
            "clean-tree",
            "Leave a clean tree: no untracked files.",
            "",
            "",
            Enforcement::Told,
        ),
    ]
}

/// The owner's default grants. A plan reads and runs commands, and asks
/// before it writes a file. A build may use the whole sandbox.
pub fn default_capabilities() -> Vec<Capability> {
    let grant = |stage, function: &str, rung| Capability {
        stage,
        function: function.into(),
        rung,
    };
    vec![
        grant(Stage::Plan, "sandbox::*", Access::Allowed),
        grant(Stage::Plan, "sandbox::fs::write", Access::Ask),
        grant(Stage::Build, "sandbox::*", Access::Allowed),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(text: &str) -> Call {
        Call {
            function: "sandbox::exec".into(),
            text: text.into(),
        }
    }

    fn write() -> Call {
        Call {
            function: "sandbox::fs::write".into(),
            text: "/work/repo/calc.py".into(),
        }
    }

    fn decide(call: &Call, stage: Stage, constraints: &[Constraint]) -> Decision {
        gate(call, stage, constraints, &default_capabilities()).decision
    }

    #[test]
    fn ladders_saturate_at_both_ends() {
        assert_eq!(
            Enforcement::Told.promote().promote().promote(),
            Enforcement::Enforced
        );
        assert_eq!(
            Enforcement::Enforced.demote().demote().demote(),
            Enforcement::Told
        );
        assert_eq!(
            Access::Denied.promote().promote().promote(),
            Access::Allowed
        );
        assert_eq!(Access::Allowed.demote().demote().demote(), Access::Denied);
    }

    #[test]
    fn patterns_match_exactly_or_by_prefix() {
        assert!(matches("sandbox::*", "sandbox::fs::write"));
        assert!(matches("sandbox::exec", "sandbox::exec"));
        assert!(!matches("sandbox::exec", "sandbox::execute"));
        assert!(!matches("", "sandbox::exec"));
        assert!(!matches("", ""));
    }

    #[test]
    fn plan_asks_before_a_write_and_build_allows_it() {
        assert!(matches!(
            decide(&write(), Stage::Plan, &[]),
            Decision::Hold { .. }
        ));
        assert_eq!(decide(&write(), Stage::Build, &[]), Decision::Continue);
        assert_eq!(
            decide(&exec("pytest -q"), Stage::Plan, &[]),
            Decision::Continue
        );
    }

    #[test]
    fn a_call_no_grant_covers_is_refused() {
        let call = Call {
            function: "github::pr::create".into(),
            text: String::new(),
        };
        assert!(matches!(
            decide(&call, Stage::Build, &[]),
            Decision::Deny { .. }
        ));
        assert!(matches!(
            decide(&write(), Stage::Review, &[]),
            Decision::Deny { .. }
        ));
    }

    #[test]
    fn a_denied_grant_refuses() {
        let grants = [Capability {
            stage: Stage::Build,
            function: "sandbox::exec".into(),
            rung: Access::Denied,
        }];
        let verdict = gate(&exec("ls"), Stage::Build, &[], &grants);
        assert!(matches!(verdict.decision, Decision::Deny { .. }));
    }

    #[test]
    fn promoting_a_constraint_turns_a_passing_call_into_a_refused_one() {
        let mut rules = default_constraints();
        let call = exec("git rm test_calc.py");
        assert_eq!(
            gate(&call, Stage::Build, &rules, &default_capabilities()),
            Verdict {
                decision: Decision::Continue,
                flagged: vec![],
            }
        );

        let keep = rules.iter_mut().find(|c| c.id == "keep-tests").unwrap();
        keep.rung = keep.rung.promote();
        let checked = gate(&call, Stage::Build, &rules, &default_capabilities());
        assert_eq!(checked.decision, Decision::Continue);
        assert_eq!(checked.flagged, vec!["keep-tests".to_string()]);

        let keep = rules.iter_mut().find(|c| c.id == "keep-tests").unwrap();
        keep.rung = keep.rung.promote();
        let Decision::Deny { reason } = decide(&call, Stage::Build, &rules) else {
            panic!("an enforced constraint must refuse the call");
        };
        assert!(reason.contains("keep-tests"), "{reason}");
    }

    #[test]
    fn an_enforced_constraint_wins_over_an_allowed_grant() {
        let rules = default_constraints();
        assert!(matches!(
            decide(&exec("bash -lc pytest"), Stage::Build, &rules),
            Decision::Deny { .. }
        ));
        assert_eq!(
            decide(&exec("pytest -q"), Stage::Build, &rules),
            Decision::Continue
        );
    }

    #[test]
    fn told_rules_bind_no_call() {
        let rules = default_constraints();
        let told: Vec<_> = rules.iter().filter(|c| c.function.is_empty()).collect();
        assert!(!told.is_empty());
        for rule in told {
            let mut promoted = rule.clone();
            promoted.rung = Enforcement::Enforced;
            assert_eq!(
                decide(&exec(""), Stage::Build, &[promoted]),
                Decision::Continue
            );
        }
    }
}
