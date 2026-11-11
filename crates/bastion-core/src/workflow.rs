//! Workflows (DECISIONS.md D35): a graph of steps on a canvas. A step is a
//! prompt, one of the repository's agents, skills or commands, a shell
//! script, or a Bastion stage. A link sends one step's results to another, and carries no
//! condition: a step's entry criteria are the results linked into it, and
//! its exit criteria are the results it declares.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Dial, Kind, Level, Stage};

/// One step on the canvas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// Unique in its workflow; a built-in step's is its stage's name.
    pub id: String,
    /// What runs: `prompt`, `agent`, `skill`, `command`, `shell`, or a
    /// Bastion stage.
    pub stage: Stage,
    /// The agent's, skill's or command's name, or the shell script. Empty
    /// for anything else.
    pub run: String,
    pub dial: Dial,
    /// The step's instructions. A prompt's whole brief; for a Bastion
    /// stage's turn, its instructions in place of the built-in ones; for an
    /// agent, skill or command, what to add to it. Empty keeps the built-in.
    pub note: String,
    /// The results it declares, by name, such as `design-notes`.
    pub results: Vec<String>,
    pub x: i32,
    pub y: i32,
}

/// The later node gets the earlier node's results.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
}

/// A step as a job runs it: what runs, its dial, its note, the results it
/// needs (entry criteria) and the results it gives (exit criteria).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub stage: Stage,
    pub run: String,
    pub dial: Dial,
    pub note: String,
    pub needs: Vec<String>,
    pub gives: Vec<String>,
}

/// The id a chained stage's node has.
pub fn chain_id(stage: Stage) -> String {
    format!("{stage:?}").to_lowercase()
}

/// The level of steps in this order with these dials (D33): lights-on when
/// every step is, dim when at least one up to the merge is, and dark when
/// none up to the merge is. Land is the short pipeline's merge.
pub fn level_of(steps: &[(Stage, Dial)]) -> Level {
    let on = |s: &(Stage, Dial)| s.1 == Dial::LightsOn;
    let merge = steps
        .iter()
        .position(|s| matches!(s.0, Stage::Merge | Stage::Land))
        .map_or(steps.len(), |i| i + 1);
    if steps.iter().all(on) {
        Level::LightsOn
    } else if steps[..merge].iter().any(on) {
        Level::Dim
    } else {
        Level::Dark
    }
}

/// A workflow's own step, which may repeat: a prompt, an agent, a skill, a
/// command or a shell script.
pub fn custom(stage: Stage) -> bool {
    use Stage::*;
    matches!(stage, Prompt | Agent | Skill | Command | Shell)
}

/// An agent's, skill's or command's name is a file or folder under
/// `.claude`: letters, digits, `-` and `_` only, so it cannot leave it.
pub fn skill_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// What a node lacks to run: an agent, skill or command its name, a shell
/// step its script, a prompt its instructions.
fn missing_run(n: &Node) -> Option<&'static str> {
    match n.stage {
        Stage::Agent if !skill_name_ok(&n.run) => Some("an agent needs its name in .claude/agents"),
        Stage::Skill if !skill_name_ok(&n.run) => Some("a skill needs its folder's name"),
        Stage::Command if !skill_name_ok(&n.run) => {
            Some("a command needs its name in .claude/commands")
        }
        Stage::Shell if n.run.trim().is_empty() => Some("a shell step needs its script"),
        Stage::Prompt if n.note.trim().is_empty() => Some("a prompt needs its instructions"),
        _ => None,
    }
}

/// A node has what it runs, and gives results only if an agent or a
/// command runs it.
fn what_runs(n: &Node) -> Result<(), String> {
    if let Some(why) = missing_run(n) {
        return Err(format!("{}: {why}", n.id));
    }
    let gives = n.stage.kind() == Kind::Turn || n.stage == Stage::Shell;
    if !gives && !n.results.is_empty() {
        return Err(format!("{}: {:?} gives no results", n.id, n.stage));
    }
    Ok(())
}

/// The results a report gives: the text under each `RESULT <name>:` line,
/// up to the next one.
pub fn results(report: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in report.lines() {
        let header = line
            .trim()
            .strip_prefix("RESULT ")
            .and_then(|r| r.strip_suffix(':'));
        if let Some(name) = header {
            if let Some((n, text)) = current.take() {
                found.insert(n, text.join("\n").trim().to_string());
            }
            current = Some((name.trim().to_string(), Vec::new()));
        } else if let Some((_, text)) = &mut current {
            text.push(line);
        }
    }
    if let Some((n, text)) = current {
        found.insert(n, text.join("\n").trim().to_string());
    }
    found.retain(|_, text| !text.is_empty());
    found
}

impl Graph {
    /// `stages` linked one after another, with each stage's dial: a
    /// built-in workflow.
    pub fn chain(stages: &[Stage], dial: impl Fn(Stage) -> Dial) -> Graph {
        let id = |s: &Stage| chain_id(*s);
        let nodes = stages.iter().enumerate().map(|(i, s)| Node {
            id: id(s),
            stage: *s,
            run: String::new(),
            dial: dial(*s),
            note: String::new(),
            results: Vec::new(),
            x: 220 * i as i32,
            y: 0,
        });
        let links = stages.windows(2).map(|w| Link {
            from: id(&w[0]),
            to: id(&w[1]),
        });
        Graph {
            nodes: nodes.collect(),
            links: links.collect(),
        }
    }

    fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// The nodes in link order: a node runs after every node linked into
    /// it, and nodes free at once run left to right on the canvas.
    pub fn order(&self) -> Result<Vec<&Node>, String> {
        let mut by_place: Vec<&Node> = self.nodes.iter().collect();
        by_place.sort_by_key(|n| (n.x, n.y));
        let mut waiting: BTreeMap<&str, usize> =
            self.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();
        for l in &self.links {
            let to = waiting.get_mut(l.to.as_str());
            *to.ok_or(format!("a link goes to {}, which is not a step here", l.to))? += 1;
        }
        let mut order: Vec<&Node> = Vec::new();
        while let Some(next) = by_place
            .iter()
            .find(|n| waiting[n.id.as_str()] == 0 && !order.iter().any(|o| o.id == n.id))
        {
            order.push(next);
            for l in self.links.iter().filter(|l| l.from == next.id) {
                *waiting.get_mut(l.to.as_str()).expect("checked above") -= 1;
            }
        }
        if order.len() < self.nodes.len() {
            return Err("the links go round in a circle".into());
        }
        Ok(order)
    }

    /// The workflow's level, its nodes in link order.
    pub fn level(&self) -> Result<Level, String> {
        let steps: Vec<(Stage, Dial)> = self.order()?.iter().map(|n| (n.stage, n.dial)).collect();
        Ok(level_of(&steps))
    }

    /// Whether the graph makes a workflow: each node sound, results named
    /// once, links between its own nodes, no circle, and one node that lands
    /// the change with only deploy and learn after it.
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
        if let Some(l) = self.links.iter().find(|l| self.node(&l.from).is_none()) {
            return Err(format!(
                "a link comes from {}, which is not a step here",
                l.from
            ));
        }
        let stages: Vec<Stage> = self.order()?.iter().map(|n| n.stage).collect();
        crate::pipeline::check_landing(&stages)
    }

    /// The `i`th node: its id new, a step a person can oversee, a Bastion
    /// stage at most once, a skill or command with what it runs, a prompt
    /// with its instructions, and results only where an agent or a command
    /// gives them.
    fn check_node(&self, i: usize, n: &Node) -> Result<(), String> {
        let before = &self.nodes[..i];
        if n.id.trim().is_empty() || before.iter().any(|m| m.id == n.id) {
            return Err(format!("the step id {:?} is empty or taken", n.id));
        }
        if !n.stage.dialed() {
            return Err(format!("{:?} is not a step", n.stage));
        }
        let custom = custom(n.stage);
        if !custom && before.iter().any(|m| m.stage == n.stage) {
            return Err(format!("{:?} is in the workflow twice", n.stage));
        }
        what_runs(n)
    }

    /// The steps a job runs, in link order.
    pub fn steps(&self) -> Result<Vec<Step>, String> {
        let gives = |id: &str| self.node(id).map(|n| n.results.clone()).unwrap_or_default();
        Ok(self
            .order()?
            .into_iter()
            .map(|n| Step {
                id: n.id.clone(),
                stage: n.stage,
                run: n.run.clone(),
                dial: n.dial,
                note: n.note.clone(),
                needs: self
                    .links
                    .iter()
                    .filter(|l| l.to == n.id)
                    .flat_map(|l| gives(&l.from))
                    .collect(),
                gives: n.results.clone(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Stage::*;

    fn node(id: &str, stage: Stage, x: i32, results: &[&str]) -> Node {
        Node {
            id: id.into(),
            stage,
            run: String::new(),
            dial: Dial::LightsOn,
            note: String::new(),
            results: results.iter().map(|r| r.to_string()).collect(),
            x,
            y: 0,
        }
    }

    fn link(from: &str, to: &str) -> Link {
        Link {
            from: from.into(),
            to: to.into(),
        }
    }

    fn ids(g: &Graph) -> Vec<String> {
        g.order().unwrap().iter().map(|n| n.id.clone()).collect()
    }

    #[test]
    fn nodes_run_in_link_order_and_free_ones_left_to_right() {
        let g = Graph {
            nodes: vec![
                node("land", Land, 0, &[]),
                node("build", Build, 50, &[]),
                node("plan", Plan, 100, &[]),
            ],
            links: vec![link("plan", "build"), link("build", "land")],
        };
        assert_eq!(ids(&g), ["plan", "build", "land"]);
        let free = Graph {
            nodes: vec![
                node("plan", Plan, 100, &[]),
                node("scope", Scope, 0, &[]),
                node("land", Land, 200, &[]),
            ],
            links: vec![],
        };
        assert_eq!(ids(&free), ["scope", "plan", "land"]);
    }

    #[test]
    fn a_step_needs_the_results_linked_into_it() {
        let g = Graph {
            nodes: vec![
                node("plan", Plan, 0, &["design-notes"]),
                node("build", Build, 100, &[]),
                node("land", Land, 200, &[]),
            ],
            links: vec![link("plan", "build"), link("build", "land")],
        };
        assert_eq!(g.check(), Ok(()));
        let steps = g.steps().unwrap();
        assert_eq!(steps[0].gives, ["design-notes"]);
        assert_eq!(steps[1].needs, ["design-notes"]);
        assert!(steps[2].needs.is_empty());
    }

    #[test]
    fn skills_prompts_and_commands_may_repeat_and_each_needs_what_it_runs() {
        let mut lint = node("lint", Shell, 0, &["lint-report"]);
        lint.run = "make lint".into();
        let mut review = node("review", Skill, 100, &[]);
        review.run = "code-review".into();
        let mut tests = node("tests", Shell, 200, &[]);
        tests.run = "npm test".into();
        let g = Graph {
            nodes: vec![lint, review, tests, node("land", Land, 300, &[])],
            links: vec![link("lint", "review")],
        };
        assert_eq!(g.check(), Ok(()));
        let mut bad = g.clone();
        bad.nodes[1].run = "../etc".into();
        assert!(bad.check().unwrap_err().contains("skill"));
        let mut bad = g.clone();
        bad.nodes[2].run = " ".into();
        assert!(bad.check().unwrap_err().contains("script"));
        let mut bad = g.clone();
        bad.nodes.push(node("ask", Prompt, 250, &[]));
        assert!(bad.check().unwrap_err().contains("instructions"));
        let mut bad = g;
        let mut agent = node("security", Agent, 250, &[]);
        agent.run = "reviewer".into();
        let mut command = node("ship-notes", Command, 260, &[]);
        command.run = "release-notes".into();
        bad.nodes.extend([agent.clone(), agent]);
        assert!(
            bad.check().unwrap_err().contains("taken"),
            "ids stay unique"
        );
        bad.nodes.pop();
        bad.nodes.push(command);
        assert_eq!(bad.check(), Ok(()));
        bad.nodes[5].run = "../x".into();
        assert!(bad.check().unwrap_err().contains(".claude/commands"));
    }

    #[test]
    fn a_bad_graph_says_what_is_wrong() {
        let circle = Graph {
            nodes: vec![
                node("plan", Plan, 0, &[]),
                node("build", Build, 100, &[]),
                node("land", Land, 200, &[]),
            ],
            links: vec![link("plan", "build"), link("build", "plan")],
        };
        assert!(circle.check().unwrap_err().contains("circle"));
        let results_on_land = Graph {
            nodes: vec![
                node("plan", Plan, 0, &[]),
                node("land", Land, 100, &["notes"]),
            ],
            links: vec![],
        };
        assert!(results_on_land.check().unwrap_err().contains("no results"));
        let no_land = Graph {
            nodes: vec![node("plan", Plan, 0, &[])],
            links: vec![],
        };
        assert!(no_land.check().unwrap_err().contains("nothing lands"));
        let twice = Graph {
            nodes: vec![
                node("a", Plan, 0, &[]),
                node("b", Plan, 50, &[]),
                node("land", Land, 100, &[]),
            ],
            links: vec![],
        };
        assert!(twice.check().unwrap_err().contains("twice"));
        let stray = Graph {
            nodes: vec![node("plan", Plan, 0, &[]), node("land", Land, 100, &[])],
            links: vec![link("build", "land")],
        };
        assert!(stray.check().unwrap_err().contains("not a step here"));
    }

    #[test]
    fn a_chain_is_a_valid_workflow_with_the_dials_given() {
        let dark_build = |s| {
            if s == Build {
                Dial::Dark
            } else {
                Dial::LightsOn
            }
        };
        let g = Graph::chain(&[Plan, Build, Land], dark_build);
        assert_eq!(g.check(), Ok(()));
        assert_eq!(g.nodes[1].dial, Dial::Dark);
        assert_eq!(ids(&g), ["plan", "build", "land"]);
        assert_eq!(g.level(), Ok(Level::Dim));
    }
}
