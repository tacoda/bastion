//! Workflows (DECISIONS.md D35, D39): graphs of steps, kept as versions.
//! Bastion seeds the common ones (`seed_flows`); each is the owner's to
//! change, and deleting a shipped one saves its original again.
//! `Store` extends this trait.

use std::collections::BTreeMap;

use bastion_core::workflow::{Graph, Link, Node, Reach, Step};
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

/// Whether workflow `id` comes with Bastion, changed or not.
pub fn ships(id: &str) -> bool {
    crate::seed_flows::ships(id)
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

/// Every kept workflow, at its newest version.
pub async fn all(store: &dyn Store) -> Result<Vec<Workflow>, String> {
    let mut all = Vec::new();
    for (id, name, version) in store.workflows().await? {
        let graph = store.graph_at(&id, version).await?.unwrap_or_default();
        all.push(Workflow {
            id,
            name,
            version,
            graph,
        });
    }
    Ok(all)
}

pub async fn find(store: &dyn Store, id: &str) -> Result<Workflow, String> {
    all(store)
        .await?
        .into_iter()
        .find(|w| w.id == id)
        .ok_or(format!("no workflow {id}"))
}

/// Keep `w` as its workflow's next version.
pub async fn save(store: &dyn Store, w: &Workflow, at: u64) -> Result<u64, String> {
    if w.name.trim().is_empty() {
        return Err("a workflow needs a name".into());
    }
    w.graph.check()?;
    store.save_workflow(w, at).await
}

/// `job` on workflow `id`, at its first step. Returns the job and the
/// version it runs.
pub async fn start_on(
    store: &dyn Store,
    id: &str,
    job: bastion_core::Job,
) -> Result<(bastion_core::Job, u64), String> {
    let w = find(store, id).await?;
    let steps = w.graph.steps()?;
    if steps.is_empty() {
        return Err(format!("the workflow {id} has no steps"));
    }
    Ok((job.running(steps), w.version))
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
            reads_only: r["reads_only"].as_i64() == Some(1),
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
            "INSERT INTO workflow_nodes (workflow, version, id, stage, run, dial, note, x, y, network, secrets, reads_only) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            json!([id, version, n.id, name(n.stage), n.run.trim(), name(n.dial), n.note, n.x, n.y, n.reach.network as i64, n.reach.secrets.join(" "), n.reach.reads_only as i64]),
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
                        "SELECT id, stage, run, dial, note, x, y, network, secrets, reads_only FROM workflow_nodes {where_}"
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
    use bastion_core::Dial;

    fn copy() -> Workflow {
        let mut graph = crate::seed_flows::graph_of("short").unwrap();
        graph.nodes[0].results = vec!["design-notes".into()];
        Workflow {
            id: "w1".into(),
            name: "Plan notes".into(),
            version: 0,
            graph,
        }
    }

    #[test]
    fn a_shipped_workflow_is_the_owners_to_change_and_comes_back_when_deleted() {
        let store = FakeStore::default();
        crate::rt::block_on(async {
            crate::seed_flows::seed(&store).await.unwrap();
            let mut short = find(&store, "short").await.unwrap();
            assert!(ships("short") && !ships("w1"));
            short.name = "Short, mine".into();
            let changed = save(&store, &short, 1).await.unwrap();
            let mine = find(&store, "short").await.unwrap();
            assert_eq!((mine.name.as_str(), mine.version), ("Short, mine", changed));
            let listed = all(&store).await.unwrap();
            assert_eq!(listed.iter().filter(|w| w.id == "short").count(), 1);
            store.delete_workflow("short").await.unwrap();
            crate::seed_flows::seed(&store).await.unwrap();
            assert_eq!(find(&store, "short").await.unwrap().name, "Short");
        });
    }

    #[test]
    fn a_saved_workflow_gets_its_next_version_and_a_bad_one_is_refused() {
        let store = FakeStore::default();
        crate::rt::block_on(async {
            assert_eq!(save(&store, &copy(), 1).await, Ok(1));
            assert_eq!(save(&store, &copy(), 2).await, Ok(2));
            let mut bad = copy();
            bad.graph.links[0].to = "nowhere".into();
            assert!(
                save(&store, &bad, 3)
                    .await
                    .unwrap_err()
                    .contains("not a step here")
            );
            assert_eq!(find(&store, "w1").await.unwrap().version, 2);
            store.delete_workflow("w1").await.unwrap();
            assert!(find(&store, "w1").await.is_err());
            assert!(
                store.graph_at("w1", 1).await.unwrap().is_some(),
                "its versions stay"
            );
        });
    }

    #[test]
    fn a_job_runs_its_workflows_version_with_each_steps_own_dial() {
        let store = FakeStore::default();
        let job = bastion_core::Job::new("j1".into(), "w".into(), "r".into(), 1);
        crate::rt::block_on(async {
            assert!(start_on(&store, "w1", job.clone()).await.is_err());
            let mut w = copy();
            w.graph.nodes[1].dial = Dial::Dark;
            save(&store, &w, 1).await.unwrap();
            let (j, version) = start_on(&store, "w1", job).await.unwrap();
            let ids: Vec<&str> = j.flow.iter().map(|s| s.id.as_str()).collect();
            assert_eq!((version, ids), (1, vec!["plan", "build", "checks"]));
            assert_eq!(j.node.as_deref(), Some("plan"));
            assert!(j.flow[1].needs.contains(&"design-notes".to_string()));
            assert_eq!(j.flow[1].dial, Dial::Dark, "each step keeps its own dial");
        });
    }
}
