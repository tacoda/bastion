//! A step's reviewers (D51): each agent named by a reviewer constraint at
//! the step reads its change in a turn with no tools, and the first fail
//! fails the step.

use std::sync::Arc;

use bastion_core::Event;
use bastion_core::review::{self, Verdict};

use super::{Factory, Record, is_diff, now, short_id};
use crate::evals;
use crate::store::Artifact;

impl Factory {
    /// Run the step's reviewers off the effect's path, then send their verdict.
    pub(super) fn review_step(
        self: &Arc<Self>,
        record: &Record,
        step: String,
        reviewers: Vec<String>,
    ) {
        let (factory, record) = (self.clone(), record.clone());
        crate::rt::spawn(async move {
            let verdict = factory
                .verdict(&record, &step, &reviewers)
                .await
                .unwrap_or_else(|error| Verdict::Broken { error });
            let id = &record.job.id;
            if let Err(e) = factory.apply(id, Event::Reviewed { verdict }).await {
                eprintln!("job {id}: the review was not applied: {e}");
            }
        });
    }

    async fn verdict(
        &self,
        record: &Record,
        step: &str,
        reviewers: &[String],
    ) -> Result<Verdict, String> {
        let job = &record.job;
        let change = self.change_at(&job.id, step).await?;
        // The agent's file as the job's base has it, so the turn under
        // review cannot rewrite its reviewer.
        let file = self
            .at_base(record)
            .await
            .ok_or("the job has no worktree")?;
        for name in reviewers {
            let path = format!(".agents/agents/{name}.md");
            let agent = file(&path).ok_or(format!("the reviewer {name} has no {path}"))?;
            let session = format!("{}-{step}-{}", job.id, short_id());
            let brief = review::reviewer_brief(&agent, job, &change);
            let reply = evals::one_turn(&*self.engine, &*self.store, &session, &brief).await?;
            evals::meter(&*self.engine, &*self.store, job, &session).await;
            let kept = Artifact {
                stage: job.stage,
                step: step.into(),
                body: format!("Review by {name}:\n{reply}"),
                created_at: now(),
            };
            self.store.add_artifact(&job.id, &kept).await?;
            match review::verdict(&reply) {
                Some(Verdict::Pass) => continue,
                Some(Verdict::Fail { detail }) => {
                    let detail = format!("The reviewer {name} failed it:\n{detail}");
                    return Ok(Verdict::Fail { detail });
                }
                _ => return Err(format!("The reviewer {name} gave no verdict.")),
            }
        }
        Ok(Verdict::Pass)
    }

    /// The step's change: its diff, or its reply when it only reads.
    async fn change_at(&self, job: &str, step: &str) -> Result<String, String> {
        let artifacts = self.store.artifacts(job).await?;
        let at: Vec<&Artifact> = artifacts.iter().filter(|a| a.step == step).collect();
        let diff = at.iter().rev().find(|a| is_diff(&a.body));
        let last = diff
            .or(at.last())
            .ok_or(format!("the step {step} kept nothing to review"))?;
        Ok(last.body.clone())
    }
}
