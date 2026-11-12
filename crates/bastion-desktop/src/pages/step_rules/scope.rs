//! Where a rule or grant is on (D54), as the server reads it: a step's own
//! setting wins, then its workflow's, then every workflow's. The desktop
//! reaches no `bastion-core`, so it keeps this small copy.

/// Every workflow's scope.
pub const EVERY: &str = "*";

/// What `scopes` sets at `key`: on, off with `!`, or nothing.
pub fn setting(scopes: &[String], key: &str) -> Option<bool> {
    let on = scopes.iter().any(|s| s == key);
    let off = scopes.iter().any(|s| s.strip_prefix('!') == Some(key));
    match (on, off) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

/// Every workflow's setting, or what a list without one means: every step
/// when it turns no step on.
fn every(scopes: &[String]) -> bool {
    setting(scopes, EVERY).unwrap_or_else(|| scopes.iter().all(|s| s.starts_with('!')))
}

/// What the level `key` sets itself, and what it would inherit without
/// that: from the workflow and every workflow above a step, and from every
/// workflow above a workflow. Every workflow inherits the list's default.
pub fn at(scopes: &[String], key: &str) -> (Option<bool>, bool) {
    let own = setting(scopes, key);
    let inherited = match key.split_once('/') {
        _ if key == EVERY => scopes
            .iter()
            .filter(|s| s.trim_start_matches('!') != EVERY)
            .all(|s| s.starts_with('!')),
        Some((workflow, _)) => setting(scopes, workflow).unwrap_or_else(|| every(scopes)),
        None => every(scopes),
    };
    (own, inherited)
}

/// Whether every setting in `scopes` is about `workflow` or its steps, so
/// the item was made for it and removing it touches no other workflow.
pub fn only_in(scopes: &[String], workflow: &str) -> bool {
    let mine = |s: &String| {
        let key = s.trim_start_matches('!');
        key == workflow
            || key
                .strip_prefix(workflow)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    !scopes.is_empty() && scopes.iter().all(mine)
}

/// Whether the item is on at `key`, its own setting or what it inherits.
pub fn on(scopes: &[String], key: &str) -> bool {
    let (own, inherited) = at(scopes, key);
    own.unwrap_or(inherited)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_shows_its_own_setting_and_what_it_inherits() {
        let list = |all: &[&str]| all.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mixed = list(&["*", "!bug-fix", "bug-fix/build"]);
        assert_eq!(
            at(&mixed, EVERY),
            (Some(true), false),
            "it turns one step on"
        );
        assert_eq!(at(&mixed, "bug-fix"), (Some(false), true));
        assert_eq!(at(&mixed, "bug-fix/build"), (Some(true), false));
        assert_eq!(at(&mixed, "bug-fix/plan"), (None, false));
        assert!(!on(&mixed, "bug-fix/plan") && on(&mixed, "short/plan"));
        assert!(on(&[], "short/plan"), "no setting: every step");
        assert!(
            !on(&list(&["short/plan"]), "bug-fix/build"),
            "only the steps it names"
        );
        assert!(only_in(&list(&["bug-fix", "!bug-fix/plan"]), "bug-fix"));
        assert!(
            !only_in(&list(&["*", "!bug-fix"]), "bug-fix"),
            "it is everywhere else"
        );
        assert!(!only_in(&list(&["bug-fixer"]), "bug-fix"));
        assert!(!only_in(&[], "bug-fix"), "no setting is every workflow");
    }
}
