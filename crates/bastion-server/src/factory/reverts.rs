//! Escapes traced to the job that landed them (F6), and the revert watch
//! (F7): a commit on a workspace's branch that reverts a job's commit is
//! an escape of that job.

use bastion_core::audit::Record;
use bastion_core::{Stage, escape};

use super::Factory;

/// The audit kind an escape is kept under.
pub const ESCAPE: &str = "escape";

impl Factory {
    /// Every land's commits as (job, workspace, artifact body): what a land
    /// or a ship brought back, and the commit a merge made.
    pub async fn lands(&self) -> Result<Vec<(String, String, String)>, String> {
        let mut all = Vec::new();
        for stage in [Stage::Land, Stage::Ship, Stage::Merge] {
            all.extend(self.store.artifacts_at(stage).await?);
        }
        Ok(all)
    }

    /// The landed job whose commits hold `commit`.
    pub async fn landed_in(&self, commit: &str) -> Result<Option<String>, String> {
        let lands = self.lands().await?.into_iter();
        let mut holding = lands.filter(|(_, _, body)| escape::holds(body, commit));
        Ok(holding.next().map(|(job, ..)| job))
    }

    /// Keep an escape of `job` in the audit.
    pub fn note_escape(&self, job: &str, commit: &str, what: &str) -> Result<(), String> {
        self.gate.audit()?.note(Record {
            kind: ESCAPE.into(),
            subject: job.into(),
            detail: format!("{commit} {what}"),
        });
        Ok(())
    }

    /// Keep an escape for each revert on a workspace's branch of a commit a
    /// job landed, once each.
    // ponytail: reads each workspace's local branch, which a local land and a
    // merge bring up to date; fetch first if reverts made only on GitHub must
    // show before the next merge.
    pub async fn sweep_reverts(&self) -> Result<(), String> {
        let lands = self.lands().await?;
        let kept = self.store.entries_of(ESCAPE).await?;
        let seen = |revert: &str| kept.iter().any(|e| e.record.detail.contains(revert));
        let grep = format!("--grep={}", escape::REVERTS);
        for ws in self.store.workspaces().await? {
            let args = ["log", &ws.branch, &grep, "--format=%H%x1f%B%x1e"];
            let Ok(log) = self.git(std::path::Path::new(&ws.path), &args) else {
                continue;
            };
            for (revert, undone) in escape::reverts(&log) {
                let landed = lands
                    .iter()
                    .find(|(_, _, body)| escape::holds(body, undone));
                if let Some((job, ..)) = landed.filter(|_| !seen(revert)) {
                    self.note_escape(job, undone, &format!("reverted by {revert}"))?;
                }
            }
        }
        Ok(())
    }
}
