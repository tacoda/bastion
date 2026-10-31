//! Constraints that bind a build's diff at review, not a call (PLAN.md A5).
//! A rule on a call cannot see a test the agent deleted by rewriting its
//! file; the diff can.

use serde::{Deserialize, Serialize};

use crate::govern::{Constraint, Enforcement};

/// The function a constraint names to bind a build's diff at review.
pub const DIFF: &str = "review::diff";
/// The check a `DIFF` constraint runs: tests the diff removes and does not add back.
pub const DELETED_TESTS: &str = "deleted tests";

/// What one diff constraint found at review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub constraint: String,
    pub rung: Enforcement,
    pub rule: String,
    /// What broke the rule, such as the tests that went missing.
    pub detail: String,
}

/// Judge a build's diff by every constraint that binds it. A told rule finds
/// nothing, because it is only told.
pub fn review(diff: &str, constraints: &[Constraint]) -> Vec<Finding> {
    let gone = deleted_tests(diff);
    if gone.is_empty() {
        return Vec::new();
    }
    constraints
        .iter()
        .filter(|c| binds_diff(c))
        .map(|c| Finding {
            constraint: c.id.clone(),
            rung: c.rung,
            rule: c.rule.clone(),
            detail: format!("Deleted tests: {}", gone.join(", ")),
        })
        .collect()
}

fn binds_diff(c: &Constraint) -> bool {
    let checks_tests = c.function == DIFF && c.contains == DELETED_TESTS;
    checks_tests && c.rung != Enforcement::Told
}

/// The test functions a unified diff removes and does not add back, by name:
/// `def test_x`, `fn test_x`, and `#[test]` functions.
pub fn deleted_tests(diff: &str) -> Vec<String> {
    let added = tests(diff, '+');
    tests(diff, '-')
        .into_iter()
        .filter(|n| !added.contains(n))
        .collect()
}

/// The tests named on the diff's lines that start with `sign`.
fn tests(diff: &str, sign: char) -> Vec<String> {
    let mut found = Vec::new();
    let mut marked = false;
    for code in changed(diff, sign) {
        if code == "#[test]" {
            marked = true;
            continue;
        }
        if let Some(name) = function_name(code).filter(|n| marked || n.starts_with("test_")) {
            found.push(name.to_string());
        }
        marked = false;
    }
    found
}

/// The code of each added or removed line, without the file header lines.
fn changed(diff: &str, sign: char) -> impl Iterator<Item = &str> {
    let header = if sign == '+' { "+++" } else { "---" };
    diff.lines()
        .filter(move |l| l.starts_with(sign) && !l.starts_with(header))
        .map(|l| l[1..].trim())
}

/// The name a Python or Rust function definition gives, if the line is one.
fn function_name(code: &str) -> Option<&str> {
    ["def ", "async def ", "fn ", "pub fn "]
        .iter()
        .find_map(|p| code.strip_prefix(p))
        .and_then(|rest| rest.split(['(', '<']).next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Stage;
    use crate::govern::{Call, default_capabilities, default_constraints, gate};

    const DIFF_TEXT: &str = "diff --git a/test_calc.py b/test_calc.py\n--- a/test_calc.py\n+++ b/test_calc.py\n@@ -1,8 +1,4 @@\n-def test_add():\n-    assert add(2, 3) == 5\n-def test_sub():\n+def test_sub():\n+def test_modulo():\n";

    #[test]
    fn a_diff_that_drops_a_test_is_found_and_a_renamed_one_is_not_kept() {
        assert_eq!(deleted_tests(DIFF_TEXT), ["test_add"]);
        assert_eq!(deleted_tests("-    #[test]\n-    fn adds() {\n"), ["adds"]);
        assert!(deleted_tests("+def test_new():\n").is_empty());
    }

    #[test]
    fn review_findings_follow_the_rung() {
        let mut rules = default_constraints();
        let found = review(DIFF_TEXT, &rules);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].constraint.as_str(), found[0].rung),
            ("tests-kept", Enforcement::Checked)
        );
        assert!(found[0].detail.contains("test_add"));
        let kept = rules.iter_mut().find(|c| c.id == "tests-kept").unwrap();
        kept.rung = Enforcement::Told;
        assert!(review(DIFF_TEXT, &rules).is_empty());
        assert!(review("+def test_new():\n", &default_constraints()).is_empty());
    }

    #[test]
    fn diff_rules_bind_no_call() {
        let rules = default_constraints();
        let call = Call {
            function: "sandbox::exec".into(),
            text: "deleted tests".into(),
        };
        assert_eq!(
            gate(&call, Stage::Build, &rules, &default_capabilities()).flagged,
            Vec::<String>::new()
        );
    }
}
