//! Constraints that bind a build's diff at review, not a call.
//! A rule on a call cannot see a test the agent deleted by rewriting its
//! file; the diff can.

use serde::{Deserialize, Serialize};

use crate::govern::{Constraint, Enforcement};

/// The function a constraint names to bind a build's diff at review.
pub const DIFF: &str = "review::diff";
/// The check a `DIFF` constraint runs: tests the diff removes and does not add back.
pub const DELETED_TESTS: &str = "deleted tests";
/// The check a `DIFF` constraint runs: a key, token or private key the diff adds.
pub const SECRETS: &str = "secrets";
/// The check a `DIFF` constraint runs: fewer assertions than before.
pub const REMOVED_ASSERTIONS: &str = "removed assertions";
/// The check a `DIFF` constraint runs: a test the diff marks to skip.
pub const SKIP_MARKERS: &str = "skip markers";

/// Lines that assert, as a test writes them.
const ASSERTS: [&str; 3] = ["assert", "expect(", "self.assert"];
/// Marks that skip or ignore a test.
const SKIPS: [&str; 7] = [
    "@pytest.mark.skip",
    "@unittest.skip",
    "#[ignore",
    "it.skip(",
    "test.skip(",
    "describe.skip(",
    "xit(",
];

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
    constraints
        .iter()
        .filter(|c| c.function == DIFF && c.rung != Enforcement::Told)
        .filter_map(|c| {
            check(diff, &c.contains).map(|detail| Finding {
                constraint: c.id.clone(),
                rung: c.rung,
                rule: c.rule.clone(),
                detail,
            })
        })
        .collect()
}

/// What the check a constraint names finds in `diff`, if anything.
fn check(diff: &str, name: &str) -> Option<String> {
    let diff = Diff(diff);
    let found = match name {
        DELETED_TESTS => diff.deleted_tests(),
        SECRETS => crate::publish::secrets(&diff.added()),
        REMOVED_ASSERTIONS => diff.removed_assertions(),
        SKIP_MARKERS => diff.skip_markers(),
        _ => Vec::new(),
    };
    (!found.is_empty()).then(|| format!("{name}: {}", found.join(", ")))
}

/// A unified diff.
#[derive(Clone, Copy)]
pub struct Diff<'a>(pub &'a str);

impl<'a> Diff<'a> {
    /// The added lines, as one text.
    fn added(self) -> String {
        self.changed('+').collect::<Vec<_>>().join("\n")
    }

    /// How many assertions the diff takes away, net of those it adds.
    pub fn removed_assertions(self) -> Vec<String> {
        let count = |sign| {
            self.changed(sign)
                .filter(|code| ASSERTS.iter().any(|a| code.starts_with(a)))
                .count()
        };
        let (gone, new) = (count('-'), count('+'));
        match gone > new {
            true => vec![format!("{} fewer", gone - new)],
            false => Vec::new(),
        }
    }

    /// The skip marks the diff adds, as written.
    pub fn skip_markers(self) -> Vec<String> {
        self.changed('+')
            .filter(|code| SKIPS.iter().any(|s| code.contains(s)))
            .map(String::from)
            .collect()
    }

    /// The test functions the diff removes and does not add back, by name:
    /// `def test_x`, `fn test_x`, and `#[test]` functions.
    pub fn deleted_tests(self) -> Vec<String> {
        let added = self.tests('+');
        self.tests('-')
            .into_iter()
            .filter(|n| !added.contains(n))
            .collect()
    }

    /// The tests named on the lines that start with `sign`.
    fn tests(self, sign: char) -> Vec<String> {
        let mut found = Vec::new();
        let mut marked = false;
        for code in self.changed(sign) {
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
    fn changed(self, sign: char) -> impl Iterator<Item = &'a str> {
        let header = if sign == '+' { "+++" } else { "---" };
        self.0
            .lines()
            .filter(move |l| l.starts_with(sign) && !l.starts_with(header))
            .map(|l| l[1..].trim())
    }
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
        assert_eq!(Diff(DIFF_TEXT).deleted_tests(), ["test_add"]);
        assert_eq!(
            Diff("-    #[test]\n-    fn adds() {\n").deleted_tests(),
            ["adds"]
        );
        assert!(Diff("+def test_new():\n").deleted_tests().is_empty());
    }

    #[test]
    fn a_diff_that_adds_a_secret_a_skip_or_drops_an_assertion_is_found() {
        let rules = default_constraints();
        let key = format!("ghp_{}", "x9".repeat(10));
        let diff = format!(
            "+++ b/t.py\n+TOKEN = \"{key}\"\n+@pytest.mark.skip\n-    assert add(2, 3) == 5\n-    assert add(1, 1) == 2\n+    assert add(2, 3) == 5\n"
        );
        let found = review(&diff, &rules);
        let by = |id: &str| found.iter().find(|f| f.constraint == id).unwrap();
        assert_eq!(by("no-secrets").detail, "secrets: it holds a GitHub token");
        assert_eq!(by("no-secrets").rung, Enforcement::Enforced);
        assert_eq!(by("assertions-kept").detail, "removed assertions: 1 fewer");
        assert_eq!(
            by("no-skipped-tests").detail,
            "skip markers: @pytest.mark.skip"
        );
        assert!(found.iter().all(|f| !f.detail.contains(&key)));
    }

    #[test]
    fn review_findings_follow_the_rung() {
        let mut rules = default_constraints();
        let kept = |found: Vec<Finding>| found.into_iter().find(|f| f.constraint == "tests-kept");
        let found = kept(review(DIFF_TEXT, &rules)).unwrap();
        assert_eq!(found.rung, Enforcement::Checked);
        assert!(found.detail.contains("test_add"));
        let rule = rules.iter_mut().find(|c| c.id == "tests-kept").unwrap();
        rule.rung = Enforcement::Told;
        assert!(kept(review(DIFF_TEXT, &rules)).is_none());
        assert!(review("+def test_new():\n", &default_constraints()).is_empty());
    }

    #[test]
    fn diff_rules_bind_no_call() {
        let rules = default_constraints();
        let call = Call {
            content: String::new(),
            function: "sandbox::exec".into(),
            text: "deleted tests".into(),
        };
        assert_eq!(
            gate(&call, Stage::Build, &rules, &default_capabilities()).flagged,
            Vec::<String>::new()
        );
    }
}
