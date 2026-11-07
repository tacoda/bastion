//! Scope split a request (A8 step 6): keep each later part as a queued job,
//! asked for by the same person on the same workflow. Admission holds each
//! part until the one before it lands.

use std::sync::Arc;

use bastion_core::Job;

use super::{Factory, now};
use crate::store::{Record, Runtime};

impl Factory {
    pub(super) async fn add_parts(
        self: &Arc<Self>,
        of: &Record,
        parts: Vec<Job>,
    ) -> Result<(), String> {
        for job in parts {
            let id = job.id.clone();
            let record = Record {
                job,
                runtime: Runtime {
                    requester: of.runtime.requester.clone(),
                    workflow: of.runtime.workflow.clone(),
                    workflow_version: of.runtime.workflow_version,
                    ..Runtime::default()
                },
                rev: 1,
                created_at: now(),
            };
            self.store.insert(&record).await?;
            (self.on_change)(&id);
        }
        Ok(())
    }
}
