//! What checked each landed change, and what escaped it (F6). A landed job
//! shows each step's decision from the audit, the commits its land kept,
//! and the escapes a person traced to those commits.

use std::collections::HashMap;

use bastion_core::State;
use bastion_core::audit::Entry;
use bastion_core::escape;
use bastion_protocol::{EscapeReport, EscapeView, LandedView};

use super::factory::{FactoryCell, factory};
use crate::factory::ESCAPE;

/// How many of the newest jobs the page looks through for landed ones.
const LOOKED: usize = 200;

// ponytail: the newest LOOKED jobs only, and every decide and escape entry per
// call; a landed-jobs query and an index on audit kind when a server keeps more.
pub async fn verification(cell: &FactoryCell) -> Result<Vec<LandedView>, String> {
    let factory = factory(cell)?;
    // A revert made since the last look is an escape before the page reads them.
    if let Err(e) = factory.sweep_reverts().await {
        eprintln!("the revert watch did not run: {e}");
    }
    let store = factory.store();
    let mut commits: HashMap<String, Vec<String>> = HashMap::new();
    for (job, _, body) in factory.lands().await? {
        let kept = commits.entry(job).or_default();
        kept.extend(escape::commits(&body).into_iter().map(String::from));
    }
    let mut checks = by_job(store.entries_of("decide").await?, |e| e.record.detail);
    let mut escapes = by_job(store.entries_of(ESCAPE).await?, escape_view);
    let landed = store.jobs(LOOKED, 0).await?.into_iter();
    Ok(landed
        .filter(|r| r.job.state == State::Landed)
        .map(|r| LandedView {
            commits: commits.remove(&r.job.id).unwrap_or_default(),
            checks: checks.remove(&r.job.id).unwrap_or_default(),
            escapes: escapes.remove(&r.job.id).unwrap_or_default(),
            landed_at: r.job.updated_at,
            job: r.job.id,
            workspace: r.job.workspace,
            request: r.job.request,
        })
        .collect())
}

/// Entries grouped by the job they name, each as `f` reads it, oldest first.
fn by_job<T>(entries: Vec<Entry>, f: impl Fn(Entry) -> T) -> HashMap<String, Vec<T>> {
    let mut all: HashMap<String, Vec<T>> = HashMap::new();
    for e in entries {
        all.entry(e.record.subject.clone()).or_default().push(f(e));
    }
    all
}

/// An escape entry's detail is `<commit> <what>`.
fn escape_view(e: Entry) -> EscapeView {
    let (commit, what) = e
        .record
        .detail
        .split_once(' ')
        .unwrap_or((&e.record.detail, ""));
    EscapeView {
        commit: commit.into(),
        what: what.into(),
        at: e.at,
        job: e.record.subject.clone(),
    }
}

/// Trace an escape to the landed job whose commits hold it, and keep it.
pub async fn escape_add(cell: &FactoryCell, report: EscapeReport) -> Result<EscapeView, String> {
    let factory = factory(cell)?;
    let commit = report.commit.trim().to_ascii_lowercase();
    let what = report.what.trim();
    if what.is_empty() {
        return Err("say what went wrong".into());
    }
    if commit.len() < escape::SHORTEST || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "{commit:?} is not a commit id of {} or more hex digits",
            escape::SHORTEST
        ));
    }
    let job = factory
        .landed_in(&commit)
        .await?
        .ok_or_else(|| format!("no job on this server landed commit {commit}"))?;
    factory.note_escape(&job, &commit, what)?;
    Ok(EscapeView {
        job,
        commit,
        what: what.into(),
        at: crate::factory::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastion_core::audit::Record;

    fn entry(subject: &str, detail: &str) -> Entry {
        Entry {
            seq: 1,
            at: 9,
            record: Record {
                kind: ESCAPE.into(),
                subject: subject.into(),
                detail: detail.into(),
            },
            prev: String::new(),
            hash: String::new(),
        }
    }

    #[test]
    fn escapes_group_by_job_with_their_commit_and_what() {
        let all = vec![
            entry("j1", "a1b2c3d off by one"),
            entry("j2", "0f0f0f0 crash"),
            entry("j1", "a1b2c3e nil"),
        ];
        let got = by_job(all, escape_view);
        assert_eq!(got["j1"].len(), 2);
        assert_eq!(
            (got["j1"][0].commit.as_str(), got["j1"][0].what.as_str()),
            ("a1b2c3d", "off by one")
        );
        assert_eq!(got["j2"][0].job, "j2");
    }
}
