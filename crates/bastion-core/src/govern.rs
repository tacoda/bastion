//! Constraints, capabilities and the gate (DECISIONS.md D20). Every call a turn
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

/// Where a rule acts (D25): the repository's charter, the harness that runs
/// an agent's turn, or the factory that moves a job through its pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Charter,
    Harness,
    Factory,
}

/// A constraint from the charter belongs to it; one that binds the review
/// diff belongs to the factory; the rest bind a turn, so the harness.
pub fn layer(c: &Constraint) -> Layer {
    match (c.source, c.function == crate::review::DIFF) {
        (Source::Charter, _) => Layer::Charter,
        (_, true) => Layer::Factory,
        _ => Layer::Harness,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraint {
    pub id: String,
    /// What the agent is told.
    pub rule: String,
    /// The calls it binds: a function pattern (see [`matches`]) and words
    /// the call holds in this order (see [`mentions`]). An empty pattern
    /// binds no call, so the rule can only be told.
    pub function: String,
    pub contains: String,
    pub rung: Enforcement,
    pub source: Source,
    /// The workflow steps it applies at, each as `workflow/step`; empty for
    /// every step (D44).
    #[serde(default)]
    pub steps: Vec<String>,
}

/// Whether constraint `c` applies at `step`, named `workflow/step`.
pub fn applies(c: &Constraint, step: &str) -> bool {
    c.steps.is_empty() || c.steps.iter().any(|s| s == step)
}

/// A grant: what calls to a function pattern may do at one kind of step,
/// or at one workflow step, which wins over its kind (D44).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub stage: Stage,
    /// The workflow step it grants at, as `workflow/step`; empty for every
    /// step of its kind.
    #[serde(default)]
    pub step: String,
    pub function: String,
    pub rung: Access,
}

/// One call a turn makes. `text` is its arguments as the rules read them,
/// such as a command line or a path; `content` is what a write writes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub function: String,
    pub text: String,
    #[serde(default)]
    pub content: String,
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

/// `text` holds every word of `words` in order, with any words between, so
/// `git rm` binds `git -C /work/repo rm a.py`. No words binds every text.
pub fn mentions(text: &str, words: &str) -> bool {
    let mut said = text.split_whitespace();
    words.split_whitespace().all(|w| said.any(|t| t == w))
}

/// Whether a constraint's words are in the call: its arguments, or what it
/// writes. A constraint with no words binds every call of its function.
fn binds(c: &Constraint, call: &Call) -> bool {
    let written = !c.contains.is_empty() && mentions(&call.content, &c.contains);
    mentions(&call.text, &c.contains) || written
}

/// The sandbox function that writes a file.
const WRITE: &str = "sandbox::fs::write";

/// Where a call is made: the kind of step, and the workflow step by name,
/// `workflow/step`, empty when no workflow names it (D44).
#[derive(Clone, Copy, Debug)]
pub struct At<'a> {
    pub stage: Stage,
    pub step: &'a str,
}

/// Judge one call at a step no workflow names: only the grants for its
/// kind and the constraints for every step apply.
pub fn gate(
    call: &Call,
    stage: Stage,
    constraints: &[Constraint],
    grants: &[Capability],
) -> Verdict {
    gate_at(call, At { stage, step: "" }, constraints, grants)
}

/// The most specific grant for a call at `step` of kind `stage`: a grant for
/// the step wins over one for its kind, then an exact function over a
/// pattern, then the longer pattern.
fn grant_for<'a>(call: &Call, at: At, grants: &'a [Capability]) -> Option<&'a Capability> {
    let At { stage, step } = at;
    let fits = |g: &&Capability| {
        let here = (g.step.is_empty() && g.stage == stage) || (!step.is_empty() && g.step == step);
        here && matches(&g.function, &call.function)
    };
    grants.iter().filter(fits).max_by_key(|g| {
        (
            !g.step.is_empty(),
            !g.function.ends_with('*'),
            g.function.len(),
        )
    })
}

/// Judge one call at `step`, named `workflow/step`, of kind `stage`. A
/// write to the charter is refused first, then any call an enforced
/// constraint that applies at the step binds. Then the most specific grant
/// decides, and a call no grant covers is refused.
pub fn gate_at(call: &Call, at: At, constraints: &[Constraint], grants: &[Capability]) -> Verdict {
    let At { stage, step } = at;
    let broken = || {
        constraints
            .iter()
            .filter(|c| applies(c, step) && matches(&c.function, &call.function) && binds(c, call))
    };
    let flagged = broken()
        .filter(|c| c.rung == Enforcement::Checked)
        .map(|c| c.id.clone())
        .collect();
    let charter = (call.function == WRITE).then(|| crate::charter::guards(&call.text));
    // A refusal names the step when a workflow names it, else its kind.
    let place = match step.is_empty() {
        true => format!("{stage:?}"),
        false => step.to_string(),
    };
    let decision = if let Some(reason) = charter.flatten() {
        Decision::Deny { reason }
    } else if let Some(c) = broken().find(|c| c.rung == Enforcement::Enforced) {
        Decision::Deny {
            reason: format!("Refused by constraint {}: {}", c.id, c.rule),
        }
    } else {
        match grant_for(call, at, grants).map(|g| g.rung) {
            Some(Access::Allowed) => Decision::Continue,
            Some(Access::Ask) => Decision::Hold {
                reason: format!("{} at {place} waits for the owner.", call.function),
            },
            Some(Access::Denied) => Decision::Deny {
                reason: format!("{} is not granted at {place}.", call.function),
            },
            None => Decision::Deny {
                reason: format!("No grant covers {} at {place}.", call.function),
            },
        }
    };
    Verdict { decision, flagged }
}

/// The rules `seed/governance.sql` writes, as values for tests. The running
/// factory reads its rules from the database (D26), never from here.
#[cfg(any(test, feature = "fixtures"))]
pub fn default_constraints() -> Vec<Constraint> {
    let rule = |id: &str, rule: &str, function: &str, contains: &str, rung| Constraint {
        id: id.into(),
        rule: rule.into(),
        function: function.into(),
        contains: contains.into(),
        rung,
        source: Source::Owner,
        steps: Vec::new(),
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
            "tests-kept",
            "Every existing test still exists at review.",
            crate::review::DIFF,
            crate::review::DELETED_TESTS,
            Enforcement::Checked,
        ),
        rule(
            "no-secrets",
            "Commit no secret: no key, token or private key.",
            crate::review::DIFF,
            crate::review::SECRETS,
            Enforcement::Enforced,
        ),
        rule(
            "assertions-kept",
            "Keep every assertion a test makes.",
            crate::review::DIFF,
            crate::review::REMOVED_ASSERTIONS,
            Enforcement::Checked,
        ),
        rule(
            "no-skipped-tests",
            "Do not skip or ignore a test.",
            crate::review::DIFF,
            crate::review::SKIP_MARKERS,
            Enforcement::Checked,
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

/// The grants `seed/governance.sql` writes, as values for tests.
#[cfg(any(test, feature = "fixtures"))]
pub fn default_capabilities() -> Vec<Capability> {
    let grant = |stage, function: &str, rung| Capability {
        stage,
        step: String::new(),
        function: function.into(),
        rung,
    };
    vec![
        grant(Stage::Agent, "sandbox::*", Access::Allowed),
        grant(Stage::Agent, "sandbox::fs::write", Access::Ask),
        grant(Stage::Prompt, "sandbox::*", Access::Allowed),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(text: &str) -> Call {
        Call {
            content: String::new(),
            function: "sandbox::exec".into(),
            text: text.into(),
        }
    }

    fn write() -> Call {
        Call {
            content: String::new(),
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
    fn rule_words_bind_in_order_with_words_between() {
        assert!(mentions("git -C /work/repo rm PLAN.txt", "git rm"));
        assert!(mentions("bash -lc pytest", "bash -lc"));
        assert!(!mentions("rm git", "git rm"));
        assert!(!mentions("git rmdir x", "git rm"));
        assert!(mentions("anything", ""));
    }

    #[test]
    fn plan_asks_before_a_write_and_build_allows_it() {
        assert!(matches!(
            decide(&write(), Stage::Agent, &[]),
            Decision::Hold { .. }
        ));
        assert_eq!(decide(&write(), Stage::Prompt, &[]), Decision::Continue);
        assert_eq!(
            decide(&exec("pytest -q"), Stage::Agent, &[]),
            Decision::Continue
        );
    }

    #[test]
    fn a_rule_reads_what_a_write_writes() {
        let rule = Constraint {
            id: "no-pickle".into(),
            rule: "Do not load pickles.".into(),
            function: "sandbox::fs::write".into(),
            contains: "import pickle".into(),
            rung: Enforcement::Enforced,
            source: Source::Owner,
            steps: Vec::new(),
        };
        let pickled = Call {
            content: "import os\nimport pickle\n".into(),
            ..write()
        };
        assert!(matches!(
            decide(&pickled, Stage::Prompt, std::slice::from_ref(&rule)),
            Decision::Deny { .. }
        ));
        assert_eq!(decide(&write(), Stage::Prompt, &[rule]), Decision::Continue);
    }

    #[test]
    fn a_call_no_grant_covers_is_refused() {
        let call = Call {
            content: String::new(),
            function: "github::pr::create".into(),
            text: String::new(),
        };
        assert!(matches!(
            decide(&call, Stage::Prompt, &[]),
            Decision::Deny { .. }
        ));
        assert!(matches!(
            decide(&write(), Stage::Shell, &[]),
            Decision::Deny { .. }
        ));
    }

    #[test]
    fn a_denied_grant_refuses() {
        let grants = [Capability {
            stage: Stage::Prompt,
            step: String::new(),
            function: "sandbox::exec".into(),
            rung: Access::Denied,
        }];
        let verdict = gate(&exec("ls"), Stage::Prompt, &[], &grants);
        assert!(matches!(verdict.decision, Decision::Deny { .. }));
    }

    #[test]
    fn promoting_a_constraint_turns_a_passing_call_into_a_refused_one() {
        let mut rules = default_constraints();
        let call = exec("git -C /work/repo rm test_calc.py");
        assert_eq!(
            gate(&call, Stage::Prompt, &rules, &default_capabilities()),
            Verdict {
                decision: Decision::Continue,
                flagged: vec![],
            }
        );

        let keep = rules.iter_mut().find(|c| c.id == "keep-tests").unwrap();
        keep.rung = keep.rung.promote();
        let checked = gate(&call, Stage::Prompt, &rules, &default_capabilities());
        assert_eq!(checked.decision, Decision::Continue);
        assert_eq!(checked.flagged, vec!["keep-tests".to_string()]);

        let keep = rules.iter_mut().find(|c| c.id == "keep-tests").unwrap();
        keep.rung = keep.rung.promote();
        let Decision::Deny { reason } = decide(&call, Stage::Prompt, &rules) else {
            panic!("an enforced constraint must refuse the call");
        };
        assert!(reason.contains("keep-tests"), "{reason}");
    }

    #[test]
    fn an_enforced_constraint_wins_over_an_allowed_grant() {
        let rules = default_constraints();
        assert!(matches!(
            decide(&exec("bash -lc pytest"), Stage::Prompt, &rules),
            Decision::Deny { .. }
        ));
        assert_eq!(
            decide(&exec("pytest -q"), Stage::Prompt, &rules),
            Decision::Continue
        );
    }

    #[test]
    fn each_constraint_has_its_layer() {
        let rules = default_constraints();
        let of = |id: &str| layer(rules.iter().find(|c| c.id == id).unwrap());
        assert_eq!(
            (of("no-login-shell"), of("tests-kept"), of("commit-work")),
            (Layer::Harness, Layer::Factory, Layer::Harness)
        );
        let charter = crate::charter::constraints(&["Bash(git push:*)".into()], &[]);
        assert_eq!(layer(&charter[0]), Layer::Charter);
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
                decide(&exec(""), Stage::Prompt, &[promoted]),
                Decision::Continue
            );
        }
    }

    #[test]
    fn a_grant_for_one_workflow_step_wins_over_its_kinds() {
        let mut grants = default_capabilities();
        grants.push(Capability {
            stage: Stage::Prompt,
            step: "short/review".into(),
            function: "sandbox::fs::write".into(),
            rung: Access::Denied,
        });
        let at = |step| {
            gate_at(
                &write(),
                At {
                    stage: Stage::Prompt,
                    step,
                },
                &[],
                &grants,
            )
            .decision
        };
        let named = Decision::Deny {
            reason: "sandbox::fs::write is not granted at short/review.".into(),
        };
        assert!(
            at("short/review") == named,
            "the step's own grant, and a refusal names the step"
        );
        assert_eq!(
            at("short/build"),
            Decision::Continue,
            "the kind's at another step"
        );
        assert_eq!(at(""), Decision::Continue, "no step named");
    }

    #[test]
    fn a_constraint_for_some_steps_binds_only_there() {
        let rule = Constraint {
            id: "no-rm".into(),
            rule: "Do not delete.".into(),
            function: "sandbox::exec".into(),
            contains: "rm".into(),
            rung: Enforcement::Enforced,
            source: Source::Owner,
            steps: vec!["short/build".into()],
        };
        let at = |step| {
            gate_at(
                &exec("rm x"),
                At {
                    stage: Stage::Prompt,
                    step,
                },
                std::slice::from_ref(&rule),
                &default_capabilities(),
            )
            .decision
        };
        assert!(matches!(at("short/build"), Decision::Deny { .. }));
        assert_eq!(at("short/plan"), Decision::Continue);
        assert!(applies(
            &Constraint {
                steps: vec![],
                ..rule.clone()
            },
            "any/step"
        ));
    }
}
