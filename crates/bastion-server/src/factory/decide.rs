//! A decision on the step a job waits on goes in the audit log (D33): a
//! person's with their name, and a dark step's as its dial's.

use std::sync::Arc;

use bastion_core::{Event, Job, Stage};

use super::Factory;
use crate::store::name;

/// How the audit log words a person's decision on the step `job` waited on.
fn decision(job: &Job, event: &Event, by: &str) -> String {
    let verdict = match (event, &job.question) {
        (Event::Commented { text }, Some(_)) => format!("answered by {by}: {text}"),
        (Event::Commented { text }, None) => format!("sent back by {by}: {text}"),
        (Event::Retried, _) => format!("retried by {by}"),
        (Event::Extended, _) => format!("extended by {by}"),
        _ if stuck(job) => format!("shipped anyway by {by}, waiving: {}", waived(job)),
        _ => format!("approved by {by}"),
    };
    format!("{}: {verdict}", job.node.as_deref().unwrap_or("the finish"))
}

/// The job waits at a step that failed, which approve goes on without.
/// At a failed finish, approve finishes again and waives nothing.
fn stuck(job: &Job) -> bool {
    job.node.is_some() && bastion_core::actions(job).contains(&bastion_core::Action::Retry)
}

/// The first line of what failed, short enough for one audit line.
fn waived(job: &Job) -> String {
    let error = job.error.as_deref().unwrap_or_default();
    error
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect()
}

impl Factory {
    /// Apply a person's decision on the step a job waits on, and note it in
    /// the audit log with their name when it moved the job (D33).
    pub async fn decide(self: &Arc<Self>, id: &str, event: Event, by: &str) -> Result<(), String> {
        let always = Arc::new(|_: &crate::store::Record| true);
        if let Some(before) = self.apply_when(id, event.clone(), always).await? {
            self.note_decision(id, decision(&before, &event, by));
        }
        Ok(())
    }

    /// Note that `stage`'s dark dial let job `id` go on.
    pub(super) fn dark_decided(&self, id: &str, stage: Stage) {
        self.note_decision(id, format!("{}: approved by its dark dial", name(stage)));
    }

    /// Note a decision on job `id` in the audit log.
    fn note_decision(&self, id: &str, detail: String) {
        if let Ok(log) = self.gate.audit() {
            log.note(bastion_core::audit::Record {
                kind: "decide".into(),
                subject: id.into(),
                detail,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastion_core::State;

    #[test]
    fn a_persons_decision_names_the_step_and_the_person() {
        let mut job = Job::new("j1".into(), "w".into(), "r".into(), 1);
        job.node = Some("code_review".into());
        let note = Event::Commented {
            text: "fix 1".into(),
        };
        assert_eq!(
            decision(&job, &note, "ada"),
            "code_review: sent back by ada: fix 1"
        );
        assert_eq!(
            decision(&job, &Event::Approved, "ada"),
            "code_review: approved by ada"
        );
        assert_eq!(
            decision(&job, &Event::Retried, "ada"),
            "code_review: retried by ada"
        );
        job.state = State::NeedsYou;
        job.error = Some("2 tests failed\nlong output".into());
        assert_eq!(
            decision(&job, &Event::Approved, "ada"),
            "code_review: shipped anyway by ada, waiving: 2 tests failed"
        );
        job.node = None;
        assert_eq!(
            decision(&job, &Event::Approved, "ada"),
            "the finish: approved by ada"
        );
        (job.node, job.error) = (Some("code_review".into()), None);
        job.question = Some("Which port?".into());
        assert_eq!(
            decision(&job, &note, "ada"),
            "code_review: answered by ada: fix 1"
        );
    }
}
