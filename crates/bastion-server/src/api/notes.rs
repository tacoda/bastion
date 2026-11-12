//! Field notes (D61): what a journey taught, one sentence signed by the
//! person who wrote it. A lesson or a fact about the region becomes a
//! charter change under `.agents/`: signed while the journey is under way,
//! it is written onto the journey's own branch when it lands; signed after
//! it came home, it goes on a branch of its own. A note about Bastion
//! stays here, for the learning loop on the Guide. Every note is also an
//! audit entry, so the chain signs it.

use std::sync::Arc;

use bastion_core::State;
use bastion_core::audit::Record;
use bastion_protocol::{CharterChange, FieldNoteView};

use super::factory::{FactoryCell, factory};
use crate::charter::SETTINGS;
use crate::lessons::{self, Landing, NOTE, ON_LANDING, Written};
use crate::machine::Machine;

fn view(e: &bastion_core::audit::Entry) -> FieldNoteView {
    let s = lessons::signed(e);
    FieldNoteView {
        job: s.job,
        by: s.written.by,
        about: s.written.about,
        text: s.written.text,
        branch: s.branch,
        step: s.written.step,
        at: s.at,
        ..FieldNoteView::default()
    }
}

/// The waypoint a journey is at, or the last one it reached once home.
fn waypoint(job: &bastion_core::Job) -> String {
    let reached = bastion_core::steps(job)
        .into_iter()
        .rev()
        .find(|(_, _, s)| {
            !matches!(
                s,
                bastion_core::StepState::Pending | bastion_core::StepState::Skipped
            )
        });
    let last = reached.map(|(id, _, _)| id);
    job.node.clone().or(last).unwrap_or_default()
}

/// Whether a journey has come home: at the summit or in the marsh.
fn home(state: State) -> bool {
    matches!(state, State::Landed | State::Failed | State::Stopped)
}

/// A note on a journey already home, committed on a branch of its own:
/// its file, read against the region's base branch.
async fn on_its_own(
    cell: &FactoryCell,
    machine: &dyn Machine,
    id: &str,
    w: &Written,
) -> Result<String, String> {
    let factory = factory(cell)?;
    let record = factory.store().load(id).await?;
    let all = factory.store().workspaces().await?;
    let ws = all.iter().find(|ws| ws.name == record.job.workspace);
    let ws = ws.ok_or_else(|| format!("no region named {}", record.job.workspace))?;
    let artifacts = factory.store().artifacts(id).await?;
    let change = artifacts
        .iter()
        .rev()
        .find(|a| a.body.starts_with("diff --git"));
    let at = format!("{}:{SETTINGS}", ws.branch);
    let shown = machine.run("git", &["show", &at], Some(std::path::Path::new(&ws.path)))?;
    let landing = Landing {
        job: id,
        diff: change.map_or("", |a| a.body.as_str()),
        settings: shown.success.then_some(shown.text),
    };
    let files = lessons::files(std::slice::from_ref(w), landing)?;
    let (path, text) = files.into_iter().next().ok_or("the note makes no file")?;
    let change = CharterChange {
        workspace: record.job.workspace,
        path,
        text,
    };
    super::charters::edit(cell, machine, change).await
}

/// Sign a field note on journey `id`.
pub async fn note(
    cell: &FactoryCell,
    machine: Arc<dyn Machine>,
    id: String,
    w: Written,
) -> Result<(), String> {
    let factory = factory(cell)?;
    let record = factory.store().load(&id).await?;
    let w = Written {
        step: waypoint(&record.job),
        ..w.kept()?
    };
    let branch = match (w.about.as_str(), home(record.job.state)) {
        ("bastion", _) => String::new(),
        (_, false) => ON_LANDING.into(),
        (_, true) => on_its_own(cell, machine.as_ref(), &id, &w).await?,
    };
    factory.gate().audit()?.note(Record {
        kind: NOTE.into(),
        subject: id,
        detail: w.detail_on(&branch),
    });
    Ok(())
}

/// The notes on job `id`, oldest first.
pub async fn notes_of(cell: &FactoryCell, id: &str) -> Result<Vec<FieldNoteView>, String> {
    let entries = factory(cell)?.store().entries_of(NOTE).await?;
    Ok(entries
        .iter()
        .filter(|e| e.record.subject == id)
        .map(view)
        .collect())
}

/// Every note on the server, newest first, each with its job's request.
pub async fn atlas(cell: &FactoryCell) -> Result<Vec<FieldNoteView>, String> {
    let factory = factory(cell)?;
    let mut notes: Vec<FieldNoteView> = factory
        .store()
        .entries_of(NOTE)
        .await?
        .iter()
        .map(view)
        .collect();
    for n in &mut notes {
        if let Ok(r) = factory.store().load(&n.job).await {
            n.request = r.job.request;
            n.workspace = r.job.workspace;
            n.landed = r.job.state == State::Landed;
            n.workflow = r.runtime.workflow.unwrap_or_default();
        }
    }
    notes.sort_by_key(|n| std::cmp::Reverse(n.at));
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastion_core::Job;

    #[test]
    fn a_note_is_at_the_journeys_waypoint_or_the_last_it_reached() {
        let graph = crate::seed_flows::graph_of("short").unwrap();
        let job = Job::new("j1".into(), "w".into(), "r".into(), 1);
        let mut job = job.running(graph.steps().unwrap());
        assert_eq!(waypoint(&job), "plan");
        let last = job.flow.last().unwrap().id.clone();
        (job.node, job.state) = (None, State::Landed);
        assert_eq!(waypoint(&job), last, "home: the last step it reached");
        let old = Job::new("j0".into(), "w".into(), "r".into(), 1);
        assert_eq!(waypoint(&old), "", "a journey with no trail on record");
    }
}
