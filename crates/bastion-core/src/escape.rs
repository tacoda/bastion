//! Escapes (F6): a defect found after a change landed, traced to the job
//! whose commits hold it. A land keeps its commits as an artifact; a
//! reported commit names the job whose list holds it.

/// The first line of the artifact a land keeps.
pub const COMMITS: &str = "Commits landed:";

/// The fewest characters of a commit's id that name it.
pub const SHORTEST: usize = 7;

/// The artifact for commits given as `git log --format='%H %s'` lines, one
/// list item each, so the job's page reads it as a list.
pub fn commits_artifact(log: &str) -> Option<String> {
    let lines: Vec<String> = log
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| format!("- {l}"))
        .collect();
    (!lines.is_empty()).then(|| format!("{COMMITS}\n\n{}", lines.join("\n")))
}

/// The commit ids in a land's artifact; none for any other artifact.
pub fn commits(body: &str) -> Vec<&str> {
    match body.strip_prefix(COMMITS) {
        Some(rest) => rest
            .lines()
            .filter_map(|l| l.strip_prefix("- ")?.split_whitespace().next())
            .collect(),
        None => Vec::new(),
    }
}

/// Whether a land's artifact holds `commit`, given in full or by a prefix.
pub fn holds(body: &str, commit: &str) -> bool {
    let commit = commit.trim().to_ascii_lowercase();
    commit.len() >= SHORTEST && commits(body).iter().any(|c| c.starts_with(&commit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_land_keeps_its_commits_and_a_prefix_traces_to_it() {
        let log = "a1b2c3d4e5f6 Add the parser\n\n0f0f0f0f0f0f Test it\n";
        let body = commits_artifact(log).unwrap();
        assert_eq!(commits(&body), ["a1b2c3d4e5f6", "0f0f0f0f0f0f"]);
        assert!(holds(&body, "A1B2C3D"));
        assert!(!holds(&body, "a1b2c3"), "too short to name one commit");
        assert!(
            !holds("Plan: a1b2c3d4e5f6", "a1b2c3d4"),
            "not a land's artifact"
        );
        assert_eq!(commits_artifact("  \n"), None);
    }
}
