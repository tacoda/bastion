//! The people on a Bastion server and what each may do (DECISIONS.md D31).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Reads everything and changes nothing.
    Viewer,
    /// Submits requests and acts on their own jobs.
    Member,
    /// Manages the server: settings, workspaces, rules, the stack and people.
    Admin,
}

/// What a request asks to do, as far as roles are concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Read anything.
    Read,
    /// Submit a request.
    Submit,
    /// Act on a job: approve, comment, stop, allow or refuse a held call.
    OnJob { own: bool },
    /// Change the server: settings, workspaces, rules, the stack, evals, people.
    Manage,
}

impl Role {
    /// The role's word, as the API and the records carry it.
    pub fn word(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Member => "member",
            Role::Admin => "admin",
        }
    }

    pub fn may(self, act: Act) -> bool {
        match (self, act) {
            (_, Act::Read) => true,
            (Role::Viewer, _) => false,
            (Role::Member, Act::Submit) => true,
            (Role::Member, Act::OnJob { own }) => own,
            (Role::Member, Act::Manage) => false,
            (Role::Admin, _) => true,
        }
    }
}

/// A person signed in to a Bastion server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub name: String,
    pub role: Role,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_viewer_reads_a_member_works_its_own_jobs_and_an_admin_does_everything() {
        let acts = [
            Act::Read,
            Act::Submit,
            Act::OnJob { own: true },
            Act::OnJob { own: false },
            Act::Manage,
        ];
        let allowed = |role: Role| acts.map(|a| role.may(a));
        assert_eq!(allowed(Role::Viewer), [true, false, false, false, false]);
        assert_eq!(allowed(Role::Member), [true, true, true, false, false]);
        assert_eq!(allowed(Role::Admin), [true, true, true, true, true]);
    }
}
