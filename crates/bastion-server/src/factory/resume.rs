//! After a restart, the factory picks every active job back up where it was.

use std::sync::Arc;

use bastion_core::{Effect, Event, Stage, State};

use super::Factory;

impl Factory {
    /// After a restart: pick every active job back up where it was.
    pub(super) async fn resume(self: Arc<Self>) {
        let Ok(records) = self.store.jobs().await else {
            return;
        };
        for record in records.into_iter().filter(|r| !r.job.state.finished()) {
            let id = record.job.id.clone();
            match (
                record.job.state,
                record.job.stage,
                record.runtime.session.clone(),
            ) {
                (State::Provisioning, ..) => {
                    let error = "the server stopped while the job was provisioning".into();
                    let _ = self.apply(&id, Event::ProvisionFailed { error }).await;
                }
                // A pull request already up waits on its CI again.
                (State::Running, Stage::Land, _) => {
                    let effect = if record.job.pr.is_some() {
                        Effect::WatchCi
                    } else {
                        Effect::Land
                    };
                    let factory = self.clone();
                    crate::rt::spawn(async move { factory.run(record, effect).await });
                }
                // The turn may have ended while the server was down.
                (State::Running, _, Some(session))
                    if self.listen_for_turn(&id, &session).is_ok() =>
                {
                    self.read_charter(&record).await;
                    self.turn_done(&id, &session, false).await;
                }
                _ => {}
            }
        }
        self.pump();
    }
}
