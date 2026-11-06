//! Workflows (PLAN.md D35, W1): a graph of steps on a canvas. A link sends
//! one step's results to another, and carries no condition: a step's entry
//! criteria are the results linked into it, and its exit criteria are the
//! results it declares.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Dial, Dials, Kind, Stage};

/// One step on the canvas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub stage: Stage,
    pub dial: Dial,
    /// What the step should know, shown when the node is expanded and given
    /// to its agent.
    pub note: String,
    /// The results it declares, by name, such as `design-notes`.
    pub results: Vec<String>,
    pub x: i32,
    pub y: i32,
}

/// The later step gets the earlier step's results.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub from: Stage,
    pub to: Stage,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
}

/// A step as a job runs it: its note, the results it needs (entry
/// criteria) and the results it gives (exit criteria).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub stage: Stage,
    pub note: String,
    pub needs: Vec<String>,
    pub gives: Vec<String>,
}

impl Graph {
    /// `stages` linked one after another, with `dials`: a built-in workflow.
    pub fn chain(stages: &[Stage], dials: &Dials) -> Graph {
        let nodes = stages.iter().enumerate().map(|(i, s)| Node {
            stage: *s,
            dial: dials.at(*s),
            note: String::new(),
            results: Vec::new(),
            x: 220 * i as i32,
            y: 0,
        });
        let links = stages.windows(2).map(|w| Link {
            from: w[0],
            to: w[1],
        });
        Graph {
            nodes: nodes.collect(),
            links: links.collect(),
        }
    }

    pub fn dials(&self) -> Dials {
        let mut d = Dials::default();
        for n in &self.nodes {
            d.set(n.stage, n.dial);
        }
        d
    }

    fn node(&self, stage: Stage) -> Option<&Node> {
        self.nodes.iter().find(|n| n.stage == stage)
    }

    /// The nodes in link order: a node runs after every node linked into
    /// it, and nodes free at once run left to right on the canvas.
    pub fn order(&self) -> Result<Vec<Stage>, String> {
        let mut by_place: Vec<&Node> = self.nodes.iter().collect();
        by_place.sort_by_key(|n| (n.x, n.y));
        let mut waiting: BTreeMap<Stage, usize> = self.nodes.iter().map(|n| (n.stage, 0)).collect();
        for l in &self.links {
            *waiting.get_mut(&l.to).ok_or(format!(
                "a link goes to {:?}, which is not a step here",
                l.to
            ))? += 1;
        }
        let mut order = Vec::new();
        while let Some(next) = by_place
            .iter()
            .find(|n| waiting[&n.stage] == 0 && !order.contains(&n.stage))
        {
            order.push(next.stage);
            for l in self.links.iter().filter(|l| l.from == next.stage) {
                *waiting.get_mut(&l.to).expect("checked above") -= 1;
            }
        }
        if order.len() < self.nodes.len() {
            return Err("the links go round in a circle".into());
        }
        Ok(order)
    }

    /// Whether the graph makes a workflow: each node a step a person can
    /// oversee, each stage once, no circle, links between its own nodes,
    /// results only on agent steps and each named once, and one node that
    /// lands the change with only deploy and learn after it.
    pub fn check(&self) -> Result<(), String> {
        for (i, n) in self.nodes.iter().enumerate() {
            self.check_node(i, n)?;
        }
        let names: Vec<&String> = self.nodes.iter().flat_map(|n| &n.results).collect();
        for (i, r) in names.iter().enumerate() {
            if r.trim().is_empty() || names[..i].contains(r) {
                return Err(format!("the result name {r:?} is empty or taken"));
            }
        }
        if let Some(l) = self.links.iter().find(|l| self.node(l.from).is_none()) {
            return Err(format!(
                "a link comes from {:?}, which is not a step here",
                l.from
            ));
        }
        let order = self.order()?;
        crate::pipeline::check_landing(&order)
    }

    /// The `i`th node: a step a person can oversee, not already in the
    /// graph, and giving results only if an agent runs it.
    fn check_node(&self, i: usize, n: &Node) -> Result<(), String> {
        if !n.stage.dialed() {
            return Err(format!("{:?} is not a step", n.stage));
        }
        if self.nodes[..i].iter().any(|m| m.stage == n.stage) {
            return Err(format!("{:?} is in the workflow twice", n.stage));
        }
        if !n.results.is_empty() && n.stage.kind() != Kind::Turn {
            return Err(format!(
                "{:?} runs no agent, so it gives no results",
                n.stage
            ));
        }
        Ok(())
    }

    /// The steps a job runs, in link order.
    pub fn steps(&self) -> Result<Vec<Step>, String> {
        let gives = |stage: Stage| {
            self.node(stage)
                .map(|n| n.results.clone())
                .unwrap_or_default()
        };
        Ok(self
            .order()?
            .into_iter()
            .map(|stage| Step {
                stage,
                note: self.node(stage).map(|n| n.note.clone()).unwrap_or_default(),
                needs: self
                    .links
                    .iter()
                    .filter(|l| l.to == stage)
                    .flat_map(|l| gives(l.from))
                    .collect(),
                gives: gives(stage),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Stage::*;

    fn node(stage: Stage, x: i32, results: &[&str]) -> Node {
        Node {
            stage,
            dial: Dial::LightsOn,
            note: String::new(),
            results: results.iter().map(|r| r.to_string()).collect(),
            x,
            y: 0,
        }
    }

    fn link(from: Stage, to: Stage) -> Link {
        Link { from, to }
    }

    #[test]
    fn nodes_run_in_link_order_and_free_ones_left_to_right() {
        let g = Graph {
            nodes: vec![
                node(Land, 0, &[]),
                node(Build, 50, &[]),
                node(Plan, 100, &[]),
            ],
            links: vec![link(Plan, Build), link(Build, Land)],
        };
        assert_eq!(g.order(), Ok(vec![Plan, Build, Land]));
        let free = Graph {
            nodes: vec![
                node(Plan, 100, &[]),
                node(Scope, 0, &[]),
                node(Land, 200, &[]),
            ],
            links: vec![],
        };
        assert_eq!(free.order(), Ok(vec![Scope, Plan, Land]));
    }

    #[test]
    fn a_step_needs_the_results_linked_into_it() {
        let g = Graph {
            nodes: vec![
                node(Plan, 0, &["design-notes"]),
                node(Build, 100, &[]),
                node(Land, 200, &[]),
            ],
            links: vec![link(Plan, Build), link(Build, Land)],
        };
        assert_eq!(g.check(), Ok(()));
        let steps = g.steps().unwrap();
        assert_eq!(steps[0].gives, ["design-notes"]);
        assert_eq!(steps[1].needs, ["design-notes"]);
        assert!(steps[2].needs.is_empty());
    }

    #[test]
    fn a_bad_graph_says_what_is_wrong() {
        let circle = Graph {
            nodes: vec![
                node(Plan, 0, &[]),
                node(Build, 100, &[]),
                node(Land, 200, &[]),
            ],
            links: vec![link(Plan, Build), link(Build, Plan)],
        };
        assert!(circle.check().unwrap_err().contains("circle"));
        let results_on_land = Graph {
            nodes: vec![node(Plan, 0, &[]), node(Land, 100, &["notes"])],
            links: vec![],
        };
        assert!(results_on_land.check().unwrap_err().contains("no results"));
        let no_land = Graph {
            nodes: vec![node(Plan, 0, &[])],
            links: vec![],
        };
        assert!(no_land.check().unwrap_err().contains("nothing lands"));
        let stray = Graph {
            nodes: vec![node(Plan, 0, &[]), node(Land, 100, &[])],
            links: vec![link(Build, Land)],
        };
        assert!(stray.check().unwrap_err().contains("not a step here"));
    }

    #[test]
    fn a_chain_is_a_valid_workflow_with_the_dials_given() {
        let mut dials = Dials::default();
        dials.set(Build, Dial::Dark);
        let g = Graph::chain(&[Plan, Build, Land], &dials);
        assert_eq!(g.check(), Ok(()));
        assert_eq!(g.dials().at(Build), Dial::Dark);
        assert_eq!(g.order(), Ok(vec![Plan, Build, Land]));
    }
}
