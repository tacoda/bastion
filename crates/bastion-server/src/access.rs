//! Who is asking, and whether they may (PLAN.md D31). The HTTP layer asks
//! these; the role rules themselves are `bastion_core::people`.

use axum::http::Method;
use bastion_core::people::{Act, Person, Role};

use crate::factory::now;
use crate::people_store::hash;
use crate::server::Server;

/// Why a request was turned away.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No token, or one this server does not know.
    Unknown,
    /// The people are in the database, which is not open yet.
    NotReady,
    /// Known, but the role does not allow it.
    Forbidden(String),
}

/// The person a bearer token belongs to. The owner's token, kept in
/// `data/token`, works before the database opens; every other token is
/// looked up by its hash.
pub async fn person(
    server: &Server,
    owner: &Person,
    owner_token: &str,
    given: &str,
) -> Result<Person, Refusal> {
    if given.is_empty() {
        return Err(Refusal::Unknown);
    }
    if given == owner_token {
        return Ok(owner.clone());
    }
    let factory = server.factory.get().ok_or(Refusal::NotReady)?;
    match factory.store().person_for(&hash(given), now()).await {
        Ok(Some(p)) => Ok(p),
        Ok(None) => Err(Refusal::Unknown),
        Err(_) => Err(Refusal::NotReady),
    }
}

/// What a route does, by its method and matched path. Acting on a job is
/// counted as on one's own here; the handler checks whose job it is.
pub fn act_for(method: &Method, path: &str) -> Act {
    // Token ids are an admin's business, even to read.
    if path == "/tokens" {
        return Act::Manage;
    }
    if method == Method::GET {
        return Act::Read;
    }
    match path {
        "/jobs" | "/issues/{workspace}/{number}/job" => Act::Submit,
        "/jobs/{id}/approve" | "/jobs/{id}/comment" | "/jobs/{id}/stop" | "/held/allow"
        | "/held/refuse" => Act::OnJob { own: true },
        _ => Act::Manage,
    }
}

/// Whether `person` may do what the route does.
pub fn check(person: &Person, act: Act) -> Result<(), Refusal> {
    if person.role.may(act) {
        return Ok(());
    }
    let what = match act {
        Act::Read => "read",
        Act::Submit => "submit a request",
        Act::OnJob { .. } => "act on a job",
        Act::Manage => "change this server",
    };
    Err(Refusal::Forbidden(format!(
        "{} is a {:?}, which may not {what}",
        person.name, person.role
    )))
}

/// Whether `person` may act on a job `requester` asked for.
pub fn may_act_on(person: &Person, requester: Option<&str>) -> bool {
    let own = requester == Some(person.name.as_str());
    person.role.may(Act::OnJob { own })
}

/// A job as `person` should see it: no actions they may not take.
pub fn as_seen_by(
    person: &Person,
    mut job: bastion_protocol::JobSummary,
) -> bastion_protocol::JobSummary {
    if !may_act_on(person, job.requester.as_deref()) {
        job.actions.clear();
    }
    job
}

/// Whether `person` may act on job `id`: an admin on any, a member only on
/// a job they asked for.
pub async fn on_job(server: &Server, person: &Person, id: &str) -> Result<(), Refusal> {
    if person.role == Role::Admin {
        return Ok(());
    }
    let factory = server.factory.get().ok_or(Refusal::NotReady)?;
    let record = factory
        .store()
        .load(id)
        .await
        .map_err(|_| Refusal::Unknown)?;
    let requester = record.runtime.requester.unwrap_or_default();
    check(
        person,
        Act::OnJob {
            own: requester == person.name,
        },
    )
    .map_err(|_| Refusal::Forbidden(format!("only {requester} or an admin may act on job {id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_map_to_what_they_do() {
        assert_eq!(act_for(&Method::GET, "/settings"), Act::Read);
        assert_eq!(act_for(&Method::POST, "/jobs"), Act::Submit);
        assert_eq!(
            act_for(&Method::POST, "/jobs/{id}/approve"),
            Act::OnJob { own: true }
        );
        assert_eq!(
            act_for(&Method::POST, "/held/refuse"),
            Act::OnJob { own: true }
        );
        assert_eq!(act_for(&Method::PUT, "/settings"), Act::Manage);
        assert_eq!(act_for(&Method::GET, "/tokens"), Act::Manage);
        assert_eq!(act_for(&Method::GET, "/people"), Act::Read);
        assert_eq!(act_for(&Method::POST, "/stack/stop"), Act::Manage);
    }

    #[test]
    fn only_those_who_may_act_on_a_job_see_its_actions() {
        let mia = Person {
            name: "mia".into(),
            role: Role::Member,
        };
        let vic = Person {
            name: "vic".into(),
            role: Role::Viewer,
        };
        let boss = Person {
            name: "ana".into(),
            role: Role::Admin,
        };
        assert!(may_act_on(&mia, Some("mia")));
        assert!(!may_act_on(&mia, Some("ana")));
        assert!(!may_act_on(&vic, Some("vic")));
        assert!(may_act_on(&boss, Some("mia")));
        assert!(may_act_on(&boss, None));
    }

    #[test]
    fn a_refusal_says_who_and_why() {
        let viewer = Person {
            name: "vic".into(),
            role: Role::Viewer,
        };
        assert_eq!(check(&viewer, Act::Read), Ok(()));
        let Err(Refusal::Forbidden(why)) = check(&viewer, Act::Submit) else {
            panic!("a viewer may not submit");
        };
        assert_eq!(why, "vic is a Viewer, which may not submit a request");
    }
}
