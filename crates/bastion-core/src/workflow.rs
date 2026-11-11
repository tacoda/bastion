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
    /// What a shell step may reach beyond its VM (D40).
    #[serde(default)]
    pub reach: Reach,
    pub x: i32,
    pub y: i32,
}

/// What a step may reach and change. A shell step may reach the network,
/// with the vault's secrets it gets as environment variables, by name
/// (D40); an agent's step may be narrowed to reading only (D42).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reach {
    pub network: bool,
    pub secrets: Vec<String>,
    /// The step's agent may not write a file.
    #[serde(default)]
    pub reads_only: bool,
}

impl Reach {
    /// A secret's name: an environment variable's, as `GH_TOKEN`.
    fn bad_secret(&self) -> Option<&String> {
        let ok = |s: &String| {
            !s.is_empty()
                && !s.starts_with(|c: char| c.is_ascii_digit())
                && s.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        };
        self.secrets.iter().find(|s| !ok(s))
    }
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
    #[serde(default)]
    pub reach: Reach,
}

/// The level of a workflow's steps by their dials (D33): lights-on when
/// every step is, dark when none is, and dim between.
pub fn level_of(dials: &[Dial]) -> Level {
    let on = dials.iter().filter(|d| **d == Dial::LightsOn).count();
    match on {
        0 => Level::Dark,
        n if n == dials.len() => Level::LightsOn,
        _ => Level::Dim,
    }
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
    reach_fits(n).map_err(|why| format!("{}: {why}", n.id))
}

/// A step reaches only what its kind may: the network and secrets for a
/// shell step (D40), reading only for an agent's step (D42).
fn reach_fits(n: &Node) -> Result<(), String> {
    let past_vm = n.reach.network || !n.reach.secrets.is_empty();
    if n.stage != Stage::Shell && past_vm {
        return Err("only a shell step reaches past its VM".into());
    }
    if n.stage.kind() != Kind::Turn && n.reach.reads_only {
        return Err("only an agent's step reads only".into());
    }
    match n.reach.bad_secret() {
        Some(s) => Err(format!("{s:?} is not a secret's name, such as GH_TOKEN")),
        None => Ok(()),
    }
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
            keep(&mut found, current.take());
            current = Some((name.trim().to_string(), Vec::new()));
        } else if let Some((_, text)) = &mut current {
            text.push(line);
        }
    }
    keep(&mut found, current);
    found
}

/// Keep a result's text, unless it is empty: a header said again with
/// nothing under it, as a model may end its reply, keeps what came before.
fn keep(found: &mut BTreeMap<String, String>, result: Option<(String, Vec<&str>)>) {
    if let Some((name, text)) = result {
        let text = text.join("\n").trim().to_string();
        if !text.is_empty() {
            found.insert(name, text);
        }
    }
}

impl Graph {
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
        let dials: Vec<Dial> = self.order()?.iter().map(|n| n.dial).collect();
        Ok(level_of(&dials))
    }

    /// Whether the graph makes a workflow: each node sound, results named
    /// once, links between its own nodes, and no circle.
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
        self.order().map(|_| ())
    }

    /// The `i`th node: its id new, with what it runs and what it reaches.
    fn check_node(&self, i: usize, n: &Node) -> Result<(), String> {
        let before = &self.nodes[..i];
        if n.id.trim().is_empty() || before.iter().any(|m| m.id == n.id) {
            return Err(format!("the step id {:?} is empty or taken", n.id));
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
                reach: n.reach.clone(),
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
            reach: Reach::default(),
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

    /// A prompt with its instructions, so it is sound.
    fn prompt(id: &str, x: i32, results: &[&str]) -> Node {
        Node {
            note: "Do it.".into(),
            ..node(id, Prompt, x, results)
        }
    }

    #[test]
    fn nodes_run_in_link_order_and_free_ones_left_to_right() {
        let g = Graph {
            nodes: vec![
                prompt("land", 0, &[]),
                prompt("build", 50, &[]),
                prompt("plan", 100, &[]),
            ],
            links: vec![link("plan", "build"), link("build", "land")],
        };
        assert_eq!(ids(&g), ["plan", "build", "land"]);
        let free = Graph {
            nodes: vec![
                prompt("plan", 100, &[]),
                prompt("scope", 0, &[]),
                prompt("land", 200, &[]),
            ],
            links: vec![],
        };
        assert_eq!(ids(&free), ["scope", "plan", "land"]);
    }

    #[test]
    fn a_step_needs_the_results_linked_into_it() {
        let g = Graph {
            nodes: vec![
                prompt("plan", 0, &["design-notes"]),
                prompt("build", 100, &[]),
            ],
            links: vec![link("plan", "build")],
        };
        assert_eq!(g.check(), Ok(()));
        let steps = g.steps().unwrap();
        assert_eq!(steps[0].gives, ["design-notes"]);
        assert_eq!(steps[1].needs, ["design-notes"]);
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
            nodes: vec![lint, review, tests, prompt("build", 300, &[])],
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
            nodes: vec![prompt("plan", 0, &[]), prompt("build", 100, &[])],
            links: vec![link("plan", "build"), link("build", "plan")],
        };
        assert!(circle.check().unwrap_err().contains("circle"));
        let stray = Graph {
            nodes: vec![prompt("plan", 0, &[])],
            links: vec![link("build", "plan")],
        };
        assert!(stray.check().unwrap_err().contains("not a step here"));
        let named_twice = Graph {
            nodes: vec![prompt("a", 0, &["notes"]), prompt("b", 50, &["notes"])],
            links: vec![],
        };
        assert!(named_twice.check().unwrap_err().contains("taken"));
    }

    #[test]
    fn a_workflows_level_follows_its_dials() {
        let mut g = Graph {
            nodes: vec![prompt("plan", 0, &[]), prompt("build", 100, &[])],
            links: vec![link("plan", "build")],
        };
        assert_eq!(g.level(), Ok(Level::LightsOn));
        g.nodes[1].dial = Dial::Dark;
        assert_eq!(g.level(), Ok(Level::Dim));
        g.nodes[0].dial = Dial::Dark;
        assert_eq!(g.level(), Ok(Level::Dark));
    }

    #[test]
    fn only_a_shell_step_reaches_past_its_vm_and_a_secret_is_named_as_a_variable() {
        let mut build = node("build", Prompt, 0, &[]);
        build.note = "Do it.".into();
        let mut pr = node("pr", Shell, 100, &[]);
        pr.run = "gh pr create".into();
        let g = Graph {
            nodes: vec![build, pr],
            links: Vec::new(),
        };
        assert_eq!(g.check(), Ok(()), "a workflow of steps needs no land");
        let mut bad = g.clone();
        bad.nodes[0].reach.network = true;
        assert!(bad.check().unwrap_err().contains("only a shell step"));
        let mut bad = g.clone();
        bad.nodes[1].reach.secrets = vec!["gh token".into()];
        assert!(bad.check().unwrap_err().contains("not a secret's name"));
        let mut reads = g.clone();
        reads.nodes[0].reach.reads_only = true;
        assert_eq!(reads.check(), Ok(()), "a prompt may only read");
        let mut bad = g;
        bad.nodes[1].reach.reads_only = true;
        assert!(
            bad.check()
                .unwrap_err()
                .contains("only an agent's step reads only")
        );
    }

    #[test]
    fn a_result_said_again_with_nothing_under_it_keeps_its_text() {
        let reply = "Thinking first.\nRESULT plan:\n1. Add triple.\n2. Test it.\n\nRESULT plan:\n";
        assert_eq!(results(reply)["plan"], "1. Add triple.\n2. Test it.");
        assert!(results("RESULT plan:\n").is_empty());
    }
}
