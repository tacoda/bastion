//! The spec a specify turn writes (A8 step 3): per criterion, the command
//! that runs only its acceptance tests, and the files that hold them. The
//! app reads the reply's JSON; `bastion_core::acceptance` judges the runs.

use serde::{Deserialize, Serialize};

/// The checks a specify turn wrote: per criterion, the command that runs
/// only its tests, and the files that hold them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec {
    pub checks: Vec<Check>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub criterion: String,
    pub run: String,
    pub files: Vec<String>,
}

impl Spec {
    /// Every file the checks name, once each, sorted: the approved files.
    pub fn files(&self) -> Vec<String> {
        let mut all: Vec<String> = self.checks.iter().flat_map(|c| c.files.clone()).collect();
        all.sort();
        all.dedup();
        all
    }
}

/// Read a specify turn's reply: one JSON object, fenced or bare.
pub fn read(report: &str) -> Result<Spec, String> {
    let body = fenced(report).unwrap_or(report);
    let spec: Spec = serde_json::from_str(body.trim())
        .map_err(|e| format!("The reply was not the JSON asked for: {e}."))?;
    if spec.checks.is_empty() {
        return Err("The spec has no checks. Every criterion needs at least one.".into());
    }
    for check in &spec.checks {
        if check.run.trim().is_empty() || check.files.is_empty() {
            return Err(format!(
                "Check {} needs a command and its files.",
                check.criterion
            ));
        }
        if let Some(bad) = check.files.iter().find(|f| !safe_path(f)) {
            return Err(format!(
                "{bad} is not a path inside the repository. Name files relative to it."
            ));
        }
    }
    Ok(spec)
}

/// The text of the first fenced block, if the reply has one.
fn fenced(report: &str) -> Option<&str> {
    let start = report.find("```")?;
    let after = &report[start + 3..];
    let body = &after[after.find('\n')? + 1..];
    Some(&body[..body.find("```")?])
}

/// A path relative to the repository, with no way out of it.
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.split('/').any(|part| part == "..")
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || "_./@+-".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = "Here it is.\n```json\n{\"checks\": [\n {\"criterion\": \"C1\", \"run\": \"pytest tests/acceptance -k c1\", \"files\": [\"tests/acceptance/test_c1.py\"]},\n {\"criterion\": \"C2\", \"run\": \"pytest -k c2\", \"files\": [\"tests/acceptance/test_c2.py\", \"tests/acceptance/test_c1.py\"]}\n]}\n```\n";

    #[test]
    fn a_fenced_spec_reads_with_its_files_once_each() {
        let s = read(REPLY).unwrap();
        assert_eq!(s.checks.len(), 2);
        assert_eq!(
            s.files(),
            ["tests/acceptance/test_c1.py", "tests/acceptance/test_c2.py"]
        );
    }

    #[test]
    fn a_spec_without_a_command_files_or_a_safe_path_is_refused() {
        assert!(read("no json").is_err());
        assert!(read(r#"{"checks": []}"#).is_err());
        assert!(read(r#"{"checks": [{"criterion": "C1", "run": "", "files": ["a"]}]}"#).is_err());
        for bad in ["../etc/passwd", "/tmp/x", "a b"] {
            let reply =
                format!(r#"{{"checks": [{{"criterion": "C1", "run": "x", "files": ["{bad}"]}}]}}"#);
            assert!(read(&reply).is_err(), "{bad}");
        }
    }
}
