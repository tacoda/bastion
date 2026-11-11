//! After a restart, or when the engine comes back, the factory picks every
//! active job back up where it was: a turn that may have ended is checked
//! and its cap armed again, and a step whose work was lost starts again.

use std::sync::Arc;
use std::time::Duration;

use bastion_core::{Effect, Event, Job, Kind, Stage, State, TURN_CAP_SECS, restart};
use bastion_protocol::Link;
use serde_json::json;

use super::{Factory, NS, now, turn};
use crate::store::Record;

/// How often the factory looks at the engine link.
const LINK_POLL: Duration = Duration::from_secs(2);
/// How many times resume reads the jobs before giving up, as the store
/// registers a little after the engine comes up.
const READ_TRIES: u32 = 30;

impl Factory {
    /// Resume once now, and again each time the engine link comes back up.
    pub(super) fn keep_resuming(self: Arc<Self>) {
        crate::rt::spawn(async move {
            let mut was = self.engine.link();
            self.clone().resume().await;
            loop {
                tokio::time::sleep(LINK_POLL).await;
                let link = self.engine.link();
                if link == Link::Up && was != Link::Up {
                    self.clone().resume().await;
                }
                was = link;
            }
        });
    }

    /// Every unfinished job, read again until the store answers.
    async fn active(&self) -> Vec<Record> {
        let mut wait = Duration::from_millis(500);
        for _ in 0..READ_TRIES {
            match self.store.active().await {
                Ok(all) => {
                    return all
                        .into_iter()
                        .filter(|r| !r.job.state.finished())
                        .collect();
                }
                Err(e) => {
                    eprintln!("resume could not read the jobs ({e}); trying again in {wait:?}")
                }
            }
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(Duration::from_secs(10));
        }
        Vec::new()
    }

    /// Pick every active job back up where it was.
    pub(super) async fn resume(self: Arc<Self>) {
        for record in self.active().await {
            match record.job.state {
                State::Provisioning => {
                    let error = "the server stopped while the job was provisioning".into();
                    let _ = self
                        .apply(&record.job.id, Event::ProvisionFailed { error })
                        .await;
                }
                State::Running => self.clone().resume_step(record).await,
                _ => {}
            }
        }
        self.pump();
    }

    /// A running job's step: its turn, if one is out, else its work again.
    async fn resume_step(self: Arc<Self>, record: Record) {
        let out = record.runtime.session.clone().map(|session| turn::Turn {
            job: record.job.id.clone(),
            session,
        });
        let out = out.filter(|t| t.owns(&record));
        if let (Kind::Turn, Some(out)) = (record.job.stage.kind(), out) {
            return self.resume_turn(record, &out).await;
        }
        self.start_again(record).await;
    }

    /// Start a running job's step again, as if it had just begun.
    async fn start_again(self: Arc<Self>, record: Record) {
        let dials = match self.dials_for(&record).await {
            Ok(dials) => dials,
            Err(e) => return eprintln!("resume {}: {e}", record.job.id),
        };
        for effect in again(&record.job, &dials) {
            let (factory, record) = (self.clone(), record.clone());
            crate::rt::spawn(async move { factory.run(record, effect).await });
        }
    }

    /// A turn that was out: listen for its end again, arm what is left of
    /// its cap, and check whether it ended while nobody listened.
    /// A turn the harness no longer has, as after the stack restarted, can
    /// never end, so its step starts again.
    async fn resume_turn(self: Arc<Self>, record: Record, out: &turn::Turn) {
        let asked = json!({ "session_id": out.session });
        let status = self.engine.call(NS, "harness::status", asked).await;
        if status.is_err_and(|e| e.contains("session/not_found")) {
            return self.start_again(record).await;
        }
        if self.listen_for_turn(out).is_err() {
            return;
        }
        self.arm_cap(out, cap_left(&record.job, now()));
        let _ = self.read_charter(&record).await;
        self.turn_done(out, false).await;
    }
}

/// The effects that start a running job's step again. A land whose pull
/// request is up waits on its CI instead of landing again.
fn again(job: &Job, dials: &bastion_core::Dials) -> Vec<Effect> {
    if job.stage == Stage::Land && job.pr.is_some() {
        return vec![Effect::WatchCi];
    }
    restart(job, dials)
}

/// The seconds a turn has left of its cap, counted from when the job last
/// changed, which is when the turn started.
pub(super) fn cap_left(job: &Job, at: u64) -> u64 {
    TURN_CAP_SECS.saturating_sub(at.saturating_sub(job.updated_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resumed_turn_keeps_only_what_is_left_of_its_cap() {
        let mut job = Job::new("j1".into(), "w".into(), "r".into(), 1_000);
        job.updated_at = 1_000;
        assert_eq!(cap_left(&job, 1_000 + 600), TURN_CAP_SECS - 600);
        assert_eq!(cap_left(&job, 1_000 + TURN_CAP_SECS + 5), 0);
    }

    #[test]
    fn a_land_with_its_pull_request_up_watches_ci_again() {
        let mut job = Job::new("j1".into(), "w".into(), "r".into(), 0);
        (job.stage, job.state, job.pr) =
            (Stage::Land, State::Running, Some("https://x/pull/1".into()));
        assert_eq!(
            again(&job, &bastion_core::Dials::default()),
            vec![Effect::WatchCi]
        );
        job.pr = None;
        assert_eq!(
            again(&job, &bastion_core::Dials::default()),
            vec![Effect::Land]
        );
    }
}
