//! A workspace's quality gates (F3): lint, static analysis, coverage or
//! mutation commands run over a change's files before it is pushed.

/// One gate: its name and its command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gate {
    pub name: String,
    pub command: String,
}

/// The gates a workspace's setting names, one `name: command` per line.
/// A line with no name is named by its command's first word.
pub fn parse(setting: &str) -> Vec<Gate> {
    setting
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|line| match line.split_once(": ") {
            Some((name, command)) if !name.contains(' ') => Gate {
                name: name.into(),
                command: command.trim().into(),
            },
            _ => Gate {
                name: line.split_whitespace().next().unwrap_or_default().into(),
                command: line.into(),
            },
        })
        .collect()
}

/// `command` with `{files}` replaced by `files`, each quoted for `sh`.
pub fn fill(command: &str, files: &[String]) -> String {
    let quoted: Vec<String> = files
        .iter()
        .map(|f| format!("'{}'", f.replace('\'', r"'\''")))
        .collect();
    command.replace("{files}", &quoted.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_names_its_gates() {
        let gates = parse("lint: ruff check {files}\n\nmypy --strict {files}\n");
        assert_eq!(gates.len(), 2);
        assert_eq!(
            (gates[0].name.as_str(), gates[0].command.as_str()),
            ("lint", "ruff check {files}")
        );
        assert_eq!(gates[1].name, "mypy");
        assert!(parse(" \n").is_empty());
    }

    #[test]
    fn files_are_quoted_for_the_shell() {
        let files = vec!["a.py".to_string(), "it's.py".to_string()];
        assert_eq!(
            fill("ruff check {files}", &files),
            r"ruff check 'a.py' 'it'\''s.py'"
        );
        assert_eq!(fill("pytest --cov", &files), "pytest --cov");
    }
}
