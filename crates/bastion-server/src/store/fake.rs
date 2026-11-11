//! The store in memory, for the tests.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::*;
use bastion_core::Stage;
use bastion_core::audit::Entry;
use bastion_core::govern::{Capability, Constraint};

/// The rows `keep` picks, in order.
pub(crate) fn matching<T: Clone>(rows: &Mutex<Vec<T>>, keep: impl Fn(&T) -> bool) -> Vec<T> {
    rows.lock()
        .unwrap()
        .iter()
        .filter(|r| keep(r))
        .cloned()
        .collect()
}

/// Rows in memory, with the same `rev` rule as `SqlStore`.
#[derive(Default)]
pub struct FakeStore {
    pub workspaces: Mutex<Vec<Workspace>>,
    pub jobs: Mutex<BTreeMap<String, Record>>,
    pub artifacts: Mutex<Vec<(String, Artifact)>>,
    pub constraints: Mutex<Vec<Constraint>>,
    pub capabilities: Mutex<Vec<Capability>>,
    pub held: Mutex<Vec<HeldCall>>,
    pub audit: Mutex<Vec<Entry>>,
    /// Make every audit append wait forever, like a stalled database.
    pub stall_audit: Mutex<bool>,
    pub eval_rows: crate::eval_store::fake::EvalRows,
    pub versions: crate::settings_store::fake::Versions,
    pub people: crate::people_store::fake::People,
    pub workflows: crate::workflow_store::fake::Workflows,
}

impl Store for FakeStore {
    fn add_workspace<'a>(&'a self, w: &'a Workspace) -> Answer<'a, ()> {
        self.workspaces.lock().unwrap().push(w.clone());
        Box::pin(async { Ok(()) })
    }

    fn workspaces(&self) -> Answer<'_, Vec<Workspace>> {
        let all = self.workspaces.lock().unwrap().clone();
        Box::pin(async move { Ok(all) })
    }

    fn edit_workspace<'a>(&'a self, w: &'a Workspace) -> Answer<'a, ()> {
        let mut all = self.workspaces.lock().unwrap();
        let found = all.iter_mut().find(|o| o.name == w.name);
        let done = found
            .map(|o| *o = w.clone())
            .ok_or_else(|| format!("no workspace named {}", w.name));
        Box::pin(async move { done })
    }

    fn insert<'a>(&'a self, r: &'a Record) -> Answer<'a, ()> {
        self.jobs
            .lock()
            .unwrap()
            .insert(r.job.id.clone(), r.clone());
        Box::pin(async { Ok(()) })
    }

    fn load<'a>(&'a self, id: &'a str) -> Answer<'a, Record> {
        let found = self.jobs.lock().unwrap().get(id).cloned().ok_or(no_job(id));
        Box::pin(async move { found })
    }

    fn jobs(&self, limit: usize, offset: usize) -> Answer<'_, Vec<Record>> {
        let mut all: Vec<Record> = self.jobs.lock().unwrap().values().cloned().collect();
        all.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        let some = all.into_iter().skip(offset).take(limit).collect();
        Box::pin(async move { Ok(some) })
    }

    fn active(&self) -> Answer<'_, Vec<Record>> {
        let all = self.jobs.lock().unwrap();
        let mut some: Vec<Record> = all
            .values()
            .filter(|r| !r.job.state.finished())
            .cloned()
            .collect();
        some.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        Box::pin(async move { Ok(some) })
    }

    fn save<'a>(&'a self, r: &'a Record) -> Answer<'a, i64> {
        let mut jobs = self.jobs.lock().unwrap();
        let result = match jobs.get(&r.job.id) {
            Some(current) if current.rev == r.rev => {
                let mut saved = r.clone();
                saved.rev += 1;
                jobs.insert(r.job.id.clone(), saved);
                Ok(r.rev + 1)
            }
            _ => Err(STALE.into()),
        };
        Box::pin(async move { result })
    }

    fn add_artifact<'a>(&'a self, job: &'a str, a: &'a Artifact) -> Answer<'a, ()> {
        self.artifacts.lock().unwrap().push((job.into(), a.clone()));
        Box::pin(async { Ok(()) })
    }

    fn artifacts<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Artifact>> {
        let found = matching(&self.artifacts, |(j, _)| j == job)
            .into_iter()
            .map(|(_, a)| a)
            .collect();
        Box::pin(async move { Ok(found) })
    }

    fn artifacts_at(&self, stage: Stage) -> Answer<'_, Vec<(String, String, String)>> {
        let jobs = self.jobs.lock().unwrap();
        let mut all: Vec<_> = matching(&self.artifacts, |(_, a)| a.stage == stage)
            .into_iter()
            .filter_map(|(j, a)| Some((j.clone(), jobs.get(&j)?.job.workspace.clone(), a.body)))
            .collect();
        all.reverse();
        Box::pin(async move { Ok(all) })
    }
}
