//! Commands for workflows (DECISIONS.md D35): read them, copy one, save a
//! new version, delete one. Only an admin reaches the last three (D31).

use bastion_core::workflow::Graph;
use bastion_protocol::{LinkView, NodeView, WorkflowView};
use serde_json::json;

use super::factory::{FactoryCell, factory, name};
use crate::factory::now;
use crate::store::{Record, Store};
use crate::workflow_store::{self, Workflow};

fn view_of(w: &Workflow) -> WorkflowView {
    WorkflowView {
        id: w.id.clone(),
        name: w.name.clone(),
        ships: workflow_store::ships(&w.id),
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
                reach: bastion_protocol::ReachView {
                    network: n.reach.network,
                    secrets: n.reach.secrets.clone(),
                    reads_only: n.reach.reads_only,
                },
                options: bastion_protocol::OptionsView {
                    model: n.options.model.clone(),
                    revisions: n.options.revisions,
                    spend_micros: n.options.spend_micros,
                },
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
                failure: l.failure,
            })
            .collect(),
    }
}

/// The graph a view draws. A word that is not a stage or a dial is refused.
fn graph_of(v: &WorkflowView) -> Result<Graph, String> {
    let value = json!({ "nodes": v.nodes, "links": v.links });
    serde_json::from_value(value).map_err(|e| format!("the workflow does not read: {e}"))
}

/// The workflow job `r` runs, at the version it started on. A job from
/// before workflows were its steps has none to draw.
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
    let name = id.clone();
    Ok(view_of(&Workflow {
        id,
        name,
        version: 0,
        graph: Graph::default(),
    }))
}

async fn every(store: &dyn Store) -> Result<Vec<Workflow>, String> {
    workflow_store::all(store).await
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

/// Delete workflow `id`. A shipped one comes back at once as Bastion ships it.
pub async fn workflow_delete(cell: &FactoryCell, id: String) -> Result<(), String> {
    let factory = factory(cell)?;
    let store = factory.store();
    store.delete_workflow(&id).await?;
    if workflow_store::ships(&id) {
        crate::seed_flows::seed(&**store).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_reads_back_as_its_graph_and_a_bad_word_is_refused() {
        let w = Workflow {
            id: "short".into(),
            name: "Short".into(),
            version: 0,
            graph: crate::seed_flows::graph_of("short").unwrap(),
        };
        let v = view_of(&w);
        assert_eq!((v.ships, v.level.as_str()), (true, "dim"));
        assert_eq!(graph_of(&v), Ok(w.graph));
        let mut bad = v;
        bad.nodes[0].stage = "nap".into();
        assert!(graph_of(&bad).unwrap_err().contains("does not read"));
    }
}
