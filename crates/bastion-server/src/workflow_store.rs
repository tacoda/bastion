//! Workflows (DECISIONS.md D35): graphs of steps, kept as versions. The
//! built-in workflows are the pipelines as chains, under the settings'
//! dials. A person may change one: their version is kept under its id and
//! takes its place, and deleting it brings the original back.
//! `Store` extends this trait.

use std::collections::BTreeMap;

use bastion_core::workflow::{Graph, Link, Node, Reach, Step};
use bastion_core::{Dials, Pipeline};
use serde_json::{Value, json};

use crate::store::{Answer, Record, SqlStore, Store, name, parse};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    /// 0 for a built-in workflow.
    pub version: u64,
    pub graph: Graph,
}

impl Workflow {
    /// A built-in workflow as Bastion ships it, not changed.
    pub fn built_in(&self) -> bool {
        self.version == 0
    }
}

/// Whether workflow `id` comes with Bastion, changed or not.
pub fn ships(id: &str) -> bool {
    pipeline_of(id).is_some()
}

pub trait WorkflowStore {
    /// Each kept workflow's id, name and newest version.
    fn workflows(&self) -> Answer<'_, Vec<(String, String, u64)>>;
    fn graph_at<'a>(&'a self, id: &'a str, version: u64) -> Answer<'a, Option<Graph>>;
    /// Keep `w`'s graph as its next version. Returns the version.
    fn save_workflow<'a>(&'a self, w: &'a Workflow, at: u64) -> Answer<'a, u64>;
    /// Forget the workflow. Its versions stay for the jobs that ran them.
    fn delete_workflow<'a>(&'a self, id: &'a str) -> Answer<'a, ()>;
}

/// The built-in workflows: id, name and pipeline.
const BUILT_IN: [(&str, &str, Pipeline); 3] = [
    ("short", "Short", Pipeline::Short),
    ("full", "Full", Pipeline::Full),
    (
        "full_no_acceptance",
        "Full, without acceptance tests",
        Pipeline::FullNoAcceptance,
    ),
];

/// The built-in workflow that runs `pipeline`.
pub fn id_of(pipeline: Pipeline) -> Option<&'static str> {
    BUILT_IN.iter().find(|b| b.2 == pipeline).map(|b| b.0)
}

/// The pipeline a built-in workflow runs, if `id` is one.
pub fn pipeline_of(id: &str) -> Option<Pipeline> {
    BUILT_IN.iter().find(|b| b.0 == id).map(|b| b.2)
}

/// Every workflow: the built-ins under the settings' `dials`, then the kept ones.
pub async fn all(store: &dyn Store, dials: &Dials) -> Result<Vec<Workflow>, String> {
    let mut all: Vec<Workflow> = BUILT_IN
        .iter()
        .map(|(id, name, pipeline)| Workflow {
            id: id.to_string(),
            name: name.to_string(),
            version: 0,
            graph: Graph::chain(&pipeline.stages(), |s| dials.at(s)),
        })
        .collect();
    for (id, name, version) in store.workflows().await? {
        let graph = store.graph_at(&id, version).await?.unwrap_or_default();
        let kept = Workflow {
            id,
            name,
            version,
            graph,
        };
        // A changed built-in takes the original's place.
        match all.iter_mut().find(|w| w.id == kept.id) {
            Some(original) => *original = kept,
            None => all.push(kept),
        }
    }
    Ok(all)
}

pub async fn find(store: &dyn Store, dials: &Dials, id: &str) -> Result<Workflow, String> {
    all(store, dials)
        .await?
        .into_iter()
        .find(|w| w.id == id)
        .ok_or(format!("no workflow {id}"))
}

/// Keep `w` as its workflow's next version; a built-in's is the first
/// that takes its place.
pub async fn save(store: &dyn Store, w: &Workflow, at: u64) -> Result<u64, String> {
    if w.name.trim().is_empty() {
        return Err("a workflow needs a name".into());
    }
    w.graph.check()?;
    store.save_workflow(w, at).await
}

/// `job` on workflow `id`: a built-in's pipeline, or a kept workflow's
/// steps. Returns the job and the version it runs, none for a built-in.
pub async fn start_on(
    store: &dyn Store,
    dials: &Dials,
    id: &str,
    job: bastion_core::Job,
) -> Result<(bastion_core::Job, Option<u64>), String> {
    let w = find(store, dials, id).await?;
    if let Some(pipeline) = pipeline_of(id).filter(|_| w.built_in()) {
        return Ok((job.on(pipeline), None));
    }
    Ok((job.running(w.graph.steps()?), Some(w.version)))
}

/// Fill each job's steps from the workflow version it runs.
pub async fn with_flows(
    store: &(dyn WorkflowStore + Sync),
    mut records: Vec<Record>,
) -> Result<Vec<Record>, String> {
    let mut graphs: BTreeMap<(String, u64), Vec<Step>> = BTreeMap::new();
    for r in &mut records {
        let (Some(id), Some(version)) = (&r.runtime.workflow, r.runtime.workflow_version) else {
            continue;
        };
        let key = (id.clone(), version);
        if !graphs.contains_key(&key) {
            let graph = store.graph_at(id, version).await?.unwrap_or_default();
            graphs.insert(key.clone(), graph.steps()?);
        }
        r.job.flow = graphs[&key].clone();
    }
    Ok(records)
}

pub const SCHEMA: [&str; 4] = [
    "CREATE TABLE IF NOT EXISTS workflows (
        id TEXT PRIMARY KEY CHECK (length(id) > 0),
        name TEXT NOT NULL CHECK (length(name) > 0),
        version INTEGER NOT NULL CHECK (version >= 1),
        updated_at INTEGER NOT NULL)",
    // Each version's steps, where they sit on the canvas. `run` is a
    // skill's name or a command line.
    "CREATE TABLE IF NOT EXISTS workflow_nodes (
        workflow TEXT NOT NULL,
        version INTEGER NOT NULL CHECK (version >= 1),
        id TEXT NOT NULL CHECK (length(id) > 0),
        stage TEXT NOT NULL CHECK (length(stage) > 0),
        run TEXT NOT NULL,
        dial TEXT NOT NULL CHECK (dial IN ('lights_on','dark')),
        note TEXT NOT NULL,
        x INTEGER NOT NULL,
        y INTEGER NOT NULL,
        PRIMARY KEY (workflow, version, id))",
    // The results each step declares, in order.
    "CREATE TABLE IF NOT EXISTS workflow_results (
        workflow TEXT NOT NULL,
        version INTEGER NOT NULL,
        node TEXT NOT NULL,
        n INTEGER NOT NULL CHECK (n >= 0),
        name TEXT NOT NULL CHECK (length(name) > 0),
        PRIMARY KEY (workflow, version, name))",
    "CREATE TABLE IF NOT EXISTS workflow_links (
        workflow TEXT NOT NULL,
        version INTEGER NOT NULL,
        source TEXT NOT NULL,
        target TEXT NOT NULL,
        PRIMARY KEY (workflow, version, source, target))",
];

fn node_of(r: &Value) -> Result<Node, String> {
    Ok(Node {
        id: parse(r, "id")?,
        stage: parse(r, "stage")?,
        run: parse(r, "run")?,
        dial: parse(r, "dial")?,
        note: parse(r, "note")?,
        results: Vec::new(),
        reach: Reach {
            network: r["network"].as_i64() == Some(1),
            secrets: parse::<String>(r, "secrets")?
                .split_whitespace()
                .map(String::from)
                .collect(),
        },
        x: parse(r, "x")?,
        y: parse(r, "y")?,
    })
}

/// A version's rows, as the statements that keep them.
fn graph_statements(id: &str, version: u64, g: &Graph) -> Vec<Value> {
    let statement = |sql: &str, params: Value| json!({ "sql": sql, "params": params });
    let nodes = g.nodes.iter().map(|n| {
        statement(
            "INSERT INTO workflow_nodes (workflow, version, id, stage, run, dial, note, x, y, network, secrets) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            json!([id, version, n.id, name(n.stage), n.run.trim(), name(n.dial), n.note, n.x, n.y, n.reach.network as i64, n.reach.secrets.join(" ")]),
        )
    });
    let results = g.nodes.iter().flat_map(|n| {
        n.results.iter().enumerate().map(move |(i, r)| {
            statement(
                "INSERT INTO workflow_results (workflow, version, node, n, name) VALUES (?, ?, ?, ?, ?)",
                json!([id, version, n.id, i, r.trim()]),
            )
        })
    });
    let links = g.links.iter().map(|l| {
        statement(
            "INSERT INTO workflow_links (workflow, version, source, target) VALUES (?, ?, ?, ?)",
            json!([id, version, l.from, l.to]),
        )
    });
    nodes.chain(results).chain(links).collect()
}

impl WorkflowStore for SqlStore {
    fn workflows(&self) -> Answer<'_, Vec<(String, String, u64)>> {
        Box::pin(async move {
            let sql = "SELECT id, name, version FROM workflows ORDER BY name";
            self.listed(sql, json!([]), |r| {
                Ok((parse(r, "id")?, parse(r, "name")?, parse(r, "version")?))
            })
            .await
        })
    }

    fn graph_at<'a>(&'a self, id: &'a str, version: u64) -> Answer<'a, Option<Graph>> {
        Box::pin(async move {
            let at = json!([id, version]);
            let where_ = "WHERE workflow = ? AND version = ?";
            let mut nodes = self
                .rows(
                    &format!(
                        "SELECT id, stage, run, dial, note, x, y, network, secrets FROM workflow_nodes {where_}"
                    ),
                    at.clone(),
                    node_of,
                )
                .await?;
            if nodes.is_empty() {
                return Ok(None);
            }
            let results = self
                .rows(
                    &format!("SELECT node, name FROM workflow_results {where_} ORDER BY node, n"),
                    at.clone(),
                    |r| Ok((parse::<String>(r, "node")?, parse::<String>(r, "name")?)),
                )
                .await?;
            for (node, result) in results {
                if let Some(n) = nodes.iter_mut().find(|n| n.id == node) {
                    n.results.push(result);
                }
            }
            let links = self
                .rows(
                    &format!("SELECT source, target FROM workflow_links {where_}"),
                    at,
                    |r| {
                        Ok(Link {
                            from: parse(r, "source")?,
                            to: parse(r, "target")?,
                        })
                    },
                )
                .await?;
            Ok(Some(Graph { nodes, links }))
        })
    }

    fn save_workflow<'a>(&'a self, w: &'a Workflow, at: u64) -> Answer<'a, u64> {
        Box::pin(async move {
            // A deleted workflow's versions stay for the jobs that ran them,
            // so the next version comes after every one kept.
            let kept = self
                .query(
                    "SELECT max(version) AS v FROM workflow_nodes WHERE workflow = ?",
                    json!([w.id]),
                )
                .await?;
            let newest = kept.first().and_then(|r| r["v"].as_u64()).unwrap_or(0);
            let listed = self.workflows().await?;
            let listed = listed.iter().find(|k| k.0 == w.id).map_or(0, |k| k.2);
            let version = newest.max(listed) + 1;
            let head = json!({
                "sql": "INSERT INTO workflows (id, name, version, updated_at) VALUES (?, ?, ?, ?)
                        ON CONFLICT (id) DO UPDATE SET name = excluded.name, version = excluded.version, updated_at = excluded.updated_at",
                "params": [w.id, w.name.trim(), version, at],
            });
            let statements = std::iter::once(head)
                .chain(graph_statements(&w.id, version, &w.graph))
                .collect();
            self.transaction(statements).await?;
            Ok(version)
        })
    }

    fn delete_workflow<'a>(&'a self, id: &'a str) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute("DELETE FROM workflows WHERE id = ?", json!([id]))
                .await
                .map(|_| ())
        })
    }
}

#[cfg(test)]
pub mod fake {
    use std::sync::Mutex;

    use super::*;
    use crate::store::fake::FakeStore;

    /// Every version of every workflow, and which are still kept.
    #[derive(Default)]
    pub struct Workflows {
        pub versions: Mutex<Vec<Workflow>>,
        pub deleted: Mutex<Vec<String>>,
    }

    impl WorkflowStore for FakeStore {
        fn workflows(&self) -> Answer<'_, Vec<(String, String, u64)>> {
            let deleted = self.workflows.deleted.lock().unwrap().clone();
            let mut heads: Vec<(String, String, u64)> = Vec::new();
            for w in self.workflows.versions.lock().unwrap().iter() {
                if deleted.contains(&w.id) {
                    continue;
                }
                heads.retain(|h| h.0 != w.id);
                heads.push((w.id.clone(), w.name.clone(), w.version));
            }
            Box::pin(async move { Ok(heads) })
        }

        fn graph_at<'a>(&'a self, id: &'a str, version: u64) -> Answer<'a, Option<Graph>> {
            let found = self
                .workflows
                .versions
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.id == id && w.version == version)
                .map(|w| w.graph.clone());
            Box::pin(async move { Ok(found) })
        }

        fn save_workflow<'a>(&'a self, w: &'a Workflow, _at: u64) -> Answer<'a, u64> {
            let mut all = self.workflows.versions.lock().unwrap();
            let version = all
                .iter()
                .filter(|k| k.id == w.id)
                .map(|k| k.version)
                .max()
                .unwrap_or(0)
                + 1;
            all.push(Workflow {
                version,
                ..w.clone()
            });
            self.workflows
                .deleted
                .lock()
                .unwrap()
                .retain(|d| *d != w.id);
            Box::pin(async move { Ok(version) })
        }

        fn delete_workflow<'a>(&'a self, id: &'a str) -> Answer<'a, ()> {
            self.workflows.deleted.lock().unwrap().push(id.to_string());
            Box::pin(async { Ok(()) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::fake::FakeStore;
    use bastion_core::{Dial, Stage};

    fn copy() -> Workflow {
        let mut graph = Graph::chain(&[Stage::Plan, Stage::Build, Stage::Land], |_| {
            Dial::LightsOn
        });
        graph.nodes[0].results = vec!["design-notes".into()];
        Workflow {
            id: "w1".into(),
            name: "Plan notes".into(),
            version: 0,
            graph,
        }
    }

    #[test]
    fn a_built_in_follows_the_dials_until_changed_and_comes_back_when_deleted() {
        let store = FakeStore::default();
        let mut dials = Dials::default();
        dials.set(Stage::Build, Dial::Dark);
        crate::rt::block_on(async {
            let mut short = find(&store, &dials, "short").await.unwrap();
            assert_eq!(short.graph.nodes[1].dial, Dial::Dark);
            assert!(short.built_in() && ships("short"));
            short.name = "Short, mine".into();
            assert_eq!(save(&store, &short, 1).await, Ok(1));
            let mine = find(&store, &dials, "short").await.unwrap();
            assert_eq!(
                (mine.name.as_str(), mine.built_in()),
                ("Short, mine", false)
            );
            let listed = all(&store, &dials).await.unwrap();
            assert_eq!(listed.iter().filter(|w| w.id == "short").count(), 1);
            let job = bastion_core::Job::new("j1".into(), "w".into(), "r".into(), 1);
            let (job, version) = start_on(&store, &dials, "short", job).await.unwrap();
            assert_eq!(
                (job.flow.len(), version),
                (3, Some(1)),
                "a changed built-in runs as kept"
            );
            store.delete_workflow("short").await.unwrap();
            assert!(find(&store, &dials, "short").await.unwrap().built_in());
        });
    }

    #[test]
    fn a_saved_workflow_gets_its_next_version_and_a_bad_one_is_refused() {
        let store = FakeStore::default();
        crate::rt::block_on(async {
            assert_eq!(save(&store, &copy(), 1).await, Ok(1));
            assert_eq!(save(&store, &copy(), 2).await, Ok(2));
            let mut bad = copy();
            bad.graph.nodes.pop();
            bad.graph.links.pop();
            assert!(
                save(&store, &bad, 3)
                    .await
                    .unwrap_err()
                    .contains("nothing lands")
            );
            let kept = find(&store, &Dials::default(), "w1").await.unwrap();
            assert_eq!(kept.version, 2);
            store.delete_workflow("w1").await.unwrap();
            assert!(find(&store, &Dials::default(), "w1").await.is_err());
            assert!(
                store.graph_at("w1", 1).await.unwrap().is_some(),
                "its versions stay"
            );
        });
    }

    #[test]
    fn a_job_on_a_kept_workflow_runs_that_versions_steps_with_their_dials() {
        let store = FakeStore::default();
        let job = bastion_core::Job::new("j1".into(), "w".into(), "r".into(), 1);
        crate::rt::block_on(async {
            let (short, version) = start_on(&store, &Dials::default(), "short", job.clone())
                .await
                .unwrap();
            assert_eq!(
                (short.flow.len(), version),
                (0, None),
                "a built-in runs its pipeline"
            );
            let mut w = copy();
            w.graph.nodes[1].dial = Dial::Dark;
            save(&store, &w, 1).await.unwrap();
            let (j, version) = start_on(&store, &Dials::default(), "w1", job)
                .await
                .unwrap();
            assert_eq!(version, Some(1));
            assert_eq!(j.stages(), [Stage::Plan, Stage::Build, Stage::Land]);
            assert_eq!(j.flow[1].needs, ["design-notes"]);
            assert_eq!(j.flow[1].dial, Dial::Dark, "each step keeps its own dial");
        });
    }
}
