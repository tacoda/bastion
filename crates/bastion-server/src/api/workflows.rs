//! Commands for workflows (PLAN.md D35, W1): read them, copy one, save a
//! new version, delete one. Only an admin reaches the last three (D31).

use bastion_core::workflow::Graph;
use bastion_protocol::{LinkView, NodeView, WorkflowView};
use serde_json::json;

use super::factory::{FactoryCell, factory, name};
use crate::factory::now;
use crate::settings_store;
use crate::store::{Record, Store};
use crate::workflow_store::{self, Workflow};

fn view_of(w: &Workflow) -> WorkflowView {
    WorkflowView {
        id: w.id.clone(),
        name: w.name.clone(),
        built_in: w.built_in(),
        version: w.version,
        level: w.graph.level().map_or_else(|_| "dim".into(), name),
        nodes: w
            .graph
            .nodes
            .iter()
            .map(|n| NodeView {
                id: n.id.clone(),
                stage: name(n.stage),
                run: n.run.clone(),
                dial: name(n.dial),
                note: n.note.clone(),
                results: n.results.clone(),
                x: n.x,
                y: n.y,
            })
            .collect(),
        links: w
            .graph
            .links
            .iter()
            .map(|l| LinkView {
                from: l.from.clone(),
                to: l.to.clone(),
            })
            .collect(),
    }
}

/// The graph a view draws. A word that is not a stage or a dial is refused.
fn graph_of(v: &WorkflowView) -> Result<Graph, String> {
    let value = json!({ "nodes": v.nodes, "links": v.links });
    serde_json::from_value(value).map_err(|e| format!("the workflow does not read: {e}"))
}

/// The workflow job `r` runs: the version it started on, or its
/// pipeline's stages as a chain under the settings' dials.
pub(super) async fn of_job(store: &dyn Store, r: &Record) -> Result<WorkflowView, String> {
    let id = r.runtime.workflow.clone().unwrap_or_default();
    if let Some(version) = r.runtime.workflow_version
        && let Some(graph) = store.graph_at(&id, version).await?
    {
        let name = id.clone();
        return Ok(view_of(&Workflow {
            id,
            name,
            version,
            graph,
        }));
    }
    let dials = settings_store::current(store).await?.dials;
    let graph = Graph::chain(&r.job.stages(), |s| dials.at(s));
    let name = id.clone();
    Ok(view_of(&Workflow {
        id,
        name,
        version: 0,
        graph,
    }))
}

async fn every(store: &dyn Store) -> Result<Vec<Workflow>, String> {
    let dials = settings_store::current(store).await?.dials;
    workflow_store::all(store, &dials).await
}

pub async fn workflows(cell: &FactoryCell) -> Result<Vec<WorkflowView>, String> {
    let all = every(&**factory(cell)?.store()).await?;
    Ok(all.iter().map(view_of).collect())
}

/// Copy workflow `from` as a new workflow called `name`. Returns its id.
pub async fn workflow_copy(
    cell: &FactoryCell,
    from: String,
    name: String,
) -> Result<String, String> {
    let store = factory(cell)?.store().clone();
    let source = every(&*store)
        .await?
        .into_iter()
        .find(|w| w.id == from)
        .ok_or(format!("no workflow {from}"))?;
    let copy = Workflow {
        id: format!("w{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
        name,
        version: 0,
        graph: source.graph,
    };
    workflow_store::save(&*store, &copy, now()).await?;
    Ok(copy.id)
}

/// Keep `v` as the next version of workflow `id`.
pub async fn workflow_save(cell: &FactoryCell, id: String, v: WorkflowView) -> Result<u64, String> {
    let w = Workflow {
        id,
        name: v.name.clone(),
        version: 0,
        graph: graph_of(&v)?,
    };
    workflow_store::save(&**factory(cell)?.store(), &w, now()).await
}

pub async fn workflow_delete(cell: &FactoryCell, id: String) -> Result<(), String> {
    if workflow_store::pipeline_of(&id).is_some() {
        return Err(format!("{id} is built in and cannot be deleted"));
    }
    factory(cell)?.store().delete_workflow(&id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastion_core::{Dial, Stage};

    #[test]
    fn a_view_reads_back_as_its_graph_and_a_bad_word_is_refused() {
        let w = Workflow {
            id: "short".into(),
            name: "Short".into(),
            version: 0,
            graph: Graph::chain(&[Stage::Plan, Stage::Build, Stage::Land], |_| {
                Dial::LightsOn
            }),
        };
        let v = view_of(&w);
        assert_eq!((v.built_in, v.level.as_str()), (true, "lights_on"));
        assert_eq!(graph_of(&v), Ok(w.graph));
        let mut bad = v;
        bad.nodes[0].stage = "nap".into();
        assert!(graph_of(&bad).unwrap_err().contains("does not read"));
    }
}
