//! Constraints, capabilities and the gate (DECISIONS.md D20). Every call a turn
//! makes passes the gate, and each rule's rung on its ladder is data, so a
//! change in behavior traces to a change in that data.

use serde::{Deserialize, Serialize};

use crate::Stage;

/// How hard a constraint binds: the agent is told it, a call that breaks it
/// is recorded, or that call is refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Enforcement {
    #[default]
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

/// What a grant lets a call do: held for the owner, or let through. A call
/// no grant covers is refused, so refusing needs no grant (D48).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Ask,
    Allowed,
}

impl Access {
    pub fn promote(self) -> Self {
        Access::Allowed
    }

    /// One rung down; none below ask, where the grant goes and the default,
    /// refusal, applies.
    pub fn demote(self) -> Option<Self> {
        match self {
            Access::Allowed => Some(Access::Ask),
            Access::Ask => None,
        }
    }
}

/// Where a constraint comes from: the workspace's charter, or the owner's
/// defaults when the workspace has none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Charter,
    #[default]
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    /// A command that runs in the job's VM before each call it binds, and
    /// refuses the call by exiting 2 (D50); empty for a rule.
    #[serde(default)]
    pub hook: String,
    /// An agent in `.agents/agents/` that must pass the change of each step
    /// it applies at (D51); empty for a rule.
    #[serde(default)]
    pub reviewer: String,
}

/// The scope that turns everything on or off at every workflow (D54).
pub const EVERY: &str = "*";

/// What a scope list sets at `key`: on, off with `!`, or nothing.
pub fn setting(scopes: &[String], key: &str) -> Option<bool> {
    let on = scopes.iter().any(|s| s == key);
    let off = scopes.iter().any(|s| s.strip_prefix('!') == Some(key));
    match (on, off) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

/// Whether a scope list turns its item on at `step`, named `workflow/step`
/// (D54): the step's own setting wins, then its workflow's, then every
/// workflow's. With no setting for every workflow, a list that turns no
/// step on means every step, and one that does means only those.
pub fn on_at(scopes: &[String], step: &str) -> bool {
    let workflow = step.split('/').next().unwrap_or_default();
    let named = [step, workflow].into_iter().filter(|k| !k.is_empty());
    let set = named.into_iter().find_map(|k| setting(scopes, k));
    set.or_else(|| setting(scopes, EVERY))
        .unwrap_or_else(|| scopes.iter().all(|s| s.starts_with('!')))
}

/// Whether constraint `c` applies at `step`, named `workflow/step`.
pub fn applies(c: &Constraint, step: &str) -> bool {
    on_at(&c.steps, step)
}

/// `scopes` with `key` set on or off, or cleared so it inherits. A narrower
/// setting first writes down what every workflow had, so turning one step
/// on never turns the others off.
pub fn set_scope(scopes: &mut Vec<String>, key: &str, on: Option<bool>) {
    if key != EVERY && setting(scopes, EVERY).is_none() {
        let every = scopes.iter().all(|s| s.starts_with('!'));
        set_scope(scopes, EVERY, Some(every));
    }
    scopes.retain(|s| s != key && s.strip_prefix('!') != Some(key));
    match on {
        Some(true) => scopes.push(key.into()),
        Some(false) => scopes.push(format!("!{key}")),
        None => {}
    }
}

/// The hooks to run before `call` at `step`: those of the constraints that
/// apply there, bind its function, and are more than told (D50).
pub fn hooks_at<'a>(constraints: &'a [Constraint], step: &str, call: &Call) -> Vec<&'a str> {
    let runs =
        |c: &&Constraint| !c.hook.is_empty() && c.rung != Enforcement::Told && applies(c, step);
    let bound = constraints.iter().filter(runs);
    bound
        .filter(|c| matches(&c.function, &call.function))
        .map(|c| c.hook.as_str())
        .collect()
}

/// A grant: what calls to a function pattern may do at one kind of step,
/// or at one workflow step, which wins over its kind (D44).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub stage: Stage,
    /// Empty for a grant, which is one item for its kind (D54). A step's own
    /// grant from before D54 names its step until the gate folds it in.
    #[serde(default)]
    pub step: String,
    pub function: String,
    pub rung: Access,
    /// Where the grant is on or off, as a constraint's steps are (D54).
    #[serde(default)]
    pub scopes: Vec<String>,
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

/// The grants that decide at `step` of kind `stage`: its kind's items that
/// are on there (D54).
pub fn grants_at<'a>(at: At, grants: &'a [Capability]) -> Vec<&'a Capability> {
    let on =
        |g: &&Capability| g.step.is_empty() && g.stage == at.stage && on_at(&g.scopes, at.step);
    grants.iter().filter(on).collect()
}

/// Grants from before D54 folded into items: a step's own grant turns its
/// kind's item for the same function on at the step, made from it when the
/// kind has none, and turns the kind's other items off there, as its own
/// grants once took their place. The second list is what to drop.
// ponytail: a step grant whose rung differs from its kind's takes the kind's rung; none did when D54 landed.
pub fn fold(grants: &[Capability]) -> (Vec<Capability>, Vec<Capability>) {
    let (old, mut items): (Vec<Capability>, Vec<Capability>) =
        grants.iter().cloned().partition(|g| !g.step.is_empty());
    for g in &old {
        let mine = |i: &Capability| i.stage == g.stage && i.function == g.function;
        if !items.iter().any(mine) {
            items.push(Capability {
                step: String::new(),
                scopes: Vec::new(),
                ..g.clone()
            });
            set_scope(&mut items.last_mut().unwrap().scopes, EVERY, Some(false));
        }
        let own: Vec<&str> = old
            .iter()
            .filter(|o| o.step == g.step && o.stage == g.stage)
            .map(|o| o.function.as_str())
            .collect();
        for i in items.iter_mut().filter(|i| i.stage == g.stage) {
            set_scope(
                &mut i.scopes,
                &g.step,
                Some(own.contains(&i.function.as_str())),
            );
        }
    }
    (items, old)
}

/// A grant's function and the command it names, if any (D49): the grant
/// `sandbox::exec cargo test` is for `sandbox::exec`, naming `cargo test`.
pub fn named(function: &str) -> (&str, &str) {
    match function.split_once(' ') {
        Some((function, command)) => (function, command.trim()),
        None => (function, ""),
    }
}

/// What lets one command line run a second command.
const SHELL_OPERATORS: [char; 7] = [';', '&', '|', '`', '$', '>', '<'];

/// The grant covers the call: its function matches, and a command it names
/// starts the call's line, which runs nothing else.
fn covers(g: &Capability, call: &Call) -> bool {
    let (function, command) = named(&g.function);
    let line = &call.text;
    let runs_only = || {
        let alone = !line.contains(SHELL_OPERATORS) && !line.contains('\n');
        alone
            && line
                .split_whitespace()
                .take(command.split_whitespace().count())
                .eq(command.split_whitespace())
    };
    matches(function, &call.function) && (command.is_empty() || runs_only())
}

/// The most specific grant for a call at a step: one naming a command over
/// one that names none, an exact function over a pattern, then the longer.
fn grant_for<'a>(call: &Call, at: At, grants: &'a [Capability]) -> Option<&'a Capability> {
    let fits = grants_at(at, grants)
        .into_iter()
        .filter(|g| covers(g, call));
    fits.max_by_key(|g| {
        let (function, command) = named(&g.function);
        (
            !command.is_empty(),
            !function.ends_with('*'),
            g.function.len(),
        )
    })
}

/// The functions a step's grants name, without the commands they bind, so
/// a turn there may call them (D49).
pub fn granted_functions<'a>(at: At, grants: &'a [Capability]) -> Vec<&'a str> {
    let mut all: Vec<&str> = grants_at(at, grants)
        .into_iter()
        .map(|g| named(&g.function).0)
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// Judge one call at `step`, named `workflow/step`, of kind `stage`. A
/// write to the charter is refused first, then any call an enforced
/// constraint that applies at the step binds. Then the most specific grant
/// decides, and a call no grant covers is refused.
/// The call breaks rule `c` at `step`. A hook decides its own calls, when
/// it runs (D50), and a reviewer reads the change, not a call (D51).
fn breaks(c: &Constraint, step: &str, call: &Call) -> bool {
    let rule = c.hook.is_empty() && c.reviewer.is_empty();
    rule && applies(c, step) && matches(&c.function, &call.function) && binds(c, call)
}

pub fn gate_at(call: &Call, at: At, constraints: &[Constraint], grants: &[Capability]) -> Verdict {
    let At { stage, step } = at;
    let broken = || constraints.iter().filter(|c| breaks(c, step, call));
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
        ..Constraint::default()
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
        scopes: Vec::new(),
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

    const BUILD: At = At {
        stage: Stage::Prompt,
        step: "short/build",
    };

    fn at_build(function: &str, rung: Access) -> Capability {
        Capability {
            stage: Stage::Prompt,
            step: String::new(),
            function: function.into(),
            rung,
            scopes: vec!["short/build".into()],
        }
    }

    #[test]
    fn a_grant_naming_a_command_covers_only_that_command() {
        let grants = [
            at_build("sandbox::exec cargo test", Access::Allowed),
            at_build("sandbox::exec", Access::Ask),
        ];
        // The line, and whether the named grant (not plain exec) decides it.
        let cases = [
            ("cargo test -p core", true),
            ("cargo build", false),
            ("cargo test; rm -rf /", false),
            ("cargo test $(curl x)", false),
            ("cargo test && make", false),
        ];
        for (line, named) in cases {
            let decided = gate_at(&exec(line), BUILD, &[], &grants).decision;
            assert_eq!(decided == Decision::Continue, named, "{line}");
        }
    }

    #[test]
    fn a_turn_may_call_the_functions_its_step_grants() {
        let grants = [
            at_build("sandbox::exec cargo", Access::Allowed),
            at_build("mcp::github::*", Access::Allowed),
        ];
        let functions = granted_functions(BUILD, &grants);
        assert_eq!(functions, ["mcp::github::*", "sandbox::exec"]);
    }

    #[test]
    fn a_hook_runs_at_its_steps_unless_only_told() {
        let hook = |id: &str, rung| Constraint {
            id: id.into(),
            rule: "Lint first.".into(),
            function: "sandbox::exec".into(),
            contains: "never".into(),
            rung,
            source: Source::Owner,
            steps: vec!["short/build".into()],
            hook: format!("./{id}.sh"),
            reviewer: String::new(),
        };
        let all = [
            hook("lint", Enforcement::Enforced),
            hook("note", Enforcement::Told),
        ];
        assert_eq!(hooks_at(&all, "short/build", &exec("never")), ["./lint.sh"]);
        assert!(hooks_at(&all, "short/plan", &exec("never")).is_empty());
        let words = gate_at(&exec("never"), BUILD, &all, &default_capabilities());
        assert_eq!(
            words.decision,
            Decision::Continue,
            "the hook decides, not its words"
        );
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
        assert_eq!(Access::Ask.promote().promote(), Access::Allowed);
        assert_eq!(Access::Allowed.demote(), Some(Access::Ask));
        assert_eq!(Access::Ask.demote(), None, "below ask, the grant goes");
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
            hook: String::new(),
            reviewer: String::new(),
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
    fn a_steps_grants_from_before_scopes_fold_in_and_decide_as_they_did() {
        let mut old = default_capabilities();
        old.push(Capability {
            stage: Stage::Prompt,
            step: "short/review".into(),
            function: "sandbox::fs::read".into(),
            rung: Access::Allowed,
            scopes: Vec::new(),
        });
        let (grants, dropped) = fold(&old);
        assert_eq!(dropped.len(), 1, "the step's grant");
        assert!(grants.iter().all(|g| g.step.is_empty()));
        let at = |call: &Call, step| {
            let at = At {
                stage: Stage::Prompt,
                step,
            };
            gate_at(call, at, &[], &grants).decision
        };
        let named = Decision::Deny {
            reason: "No grant covers sandbox::fs::write at short/review.".into(),
        };
        assert_eq!(
            at(&write(), "short/review"),
            named,
            "only what the step lists"
        );
        let read = Call {
            function: "sandbox::fs::read".into(),
            text: "a.py".into(),
            content: String::new(),
        };
        assert_eq!(at(&read, "short/review"), Decision::Continue);
        assert_eq!(
            at(&write(), "short/build"),
            Decision::Continue,
            "its kind's elsewhere"
        );
        assert_eq!(at(&write(), ""), Decision::Continue, "no step named");
    }

    #[test]
    fn the_most_specific_scope_wins() {
        let scopes = |all: &[&str]| all.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let on = |list: &[&str], step| on_at(&scopes(list), step);
        assert!(on(&[], "bug-fix/build"), "no setting: every step");
        assert!(
            !on(&["short/plan"], "bug-fix/build"),
            "only the steps it names"
        );
        assert!(on(&["!bug-fix"], "short/plan"), "off for one workflow only");
        let mixed = ["*", "!bug-fix", "bug-fix/build"];
        assert_eq!(
            [
                on(&mixed, "short/plan"),
                on(&mixed, "bug-fix/plan"),
                on(&mixed, "bug-fix/build")
            ],
            [true, false, true]
        );
        let mut list = scopes(&mixed);
        set_scope(&mut list, "bug-fix", None);
        set_scope(&mut list, EVERY, Some(false));
        assert_eq!(list, ["bug-fix/build", "!*"]);
        let mut everywhere = Vec::new();
        set_scope(&mut everywhere, "bug-fix/build", Some(false));
        assert_eq!(everywhere, ["*", "!bug-fix/build"], "the rest stays on");
        assert!(on_at(&everywhere, "short/plan"));
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
            hook: String::new(),
            reviewer: String::new(),
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
                hook: String::new(),
                reviewer: String::new(),
                ..rule.clone()
            },
            "any/step"
        ));
    }
}
