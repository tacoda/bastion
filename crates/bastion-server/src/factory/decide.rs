//! A decision on the step a job waits on goes in the audit log (D33, O2): a
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
        _ => format!("approved by {by}"),
    };
    format!("{}: {verdict}", name(job.stage))
}

impl Factory {
    /// Apply a person's decision on the step a job waits on, and note it in
    /// the audit log with their name when it moved the job (D33).
    pub async fn decide(self: &Arc<Self>, id: &str, event: Event, by: &str) -> Result<(), String> {
        let before = self.store.load(id).await?;
        self.apply(id, event.clone()).await?;
        if self.store.load(id).await?.rev == before.rev {
            return Ok(());
        }
        self.note_decision(id, decision(&before.job, &event, by));
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

    #[test]
    fn a_persons_decision_names_the_step_and_the_person() {
        let mut job = Job::new("j1".into(), "w".into(), "r".into(), 1);
        job.stage = Stage::CodeReview;
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
        job.question = Some("Which port?".into());
        assert_eq!(
            decision(&job, &note, "ada"),
            "code_review: answered by ada: fix 1"
        );
    }
}
