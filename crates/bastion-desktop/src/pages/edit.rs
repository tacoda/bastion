//! Editing a workflow on the canvas (DECISIONS.md D35), as plain functions: what
//! a point hits, and what a press, a move and a release do to the graph.

use bastion_protocol::{LinkView, NodeView, WorkflowView};
use iced::Point;

use super::drawing::{H, PAD, W};

/// What the canvas has picked: nothing, a step by id, or a link.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Pick {
    #[default]
    Nothing,
    Step(String),
    Link(String, String),
}

/// A drag under way: moving step `node`, or drawing a link out of it.
/// `start` is where the pointer went down, `origin` the point it moved.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub node: String,
    pub linking: bool,
    pub start: Point,
    pub origin: Point,
    pub now: Point,
}

impl Drag {
    fn new(node: &str, linking: bool, start: Point, origin: Point) -> Drag {
        Drag {
            node: node.into(),
            linking,
            start,
            origin,
            now: start,
        }
    }

    /// Where the dragged point is now, never left of or above the canvas.
    pub fn moved(&self) -> Point {
        let x = self.origin.x + self.now.x - self.start.x;
        let y = self.origin.y + self.now.y - self.start.y;
        Point::new(x.max(0.0), y.max(0.0))
    }
}

/// What a point on the canvas is over.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    /// A step's right edge, where a link starts.
    Port(String),
    Step(String),
    Link(String, String),
    Nothing,
}

pub fn node<'a>(w: &'a WorkflowView, id: &str) -> Option<&'a NodeView> {
    w.nodes.iter().find(|n| n.id == id)
}

/// A step's box in canvas coordinates.
fn corner(n: &NodeView) -> Point {
    Point::new(n.x as f32 + PAD, n.y as f32 + PAD)
}

fn port(n: &NodeView) -> Point {
    let c = corner(n);
    Point::new(c.x + W, c.y + H / 2.0)
}

fn inside(n: &NodeView, p: Point) -> bool {
    let c = corner(n);
    (c.x..=c.x + W).contains(&p.x) && (c.y..=c.y + H).contains(&p.y)
}

/// The points of a link's curve, as the drawing draws it.
pub fn curve(from: &NodeView, to: &NodeView) -> [Point; 4] {
    let (a, b) = (port(from), corner(to));
    let end = Point::new(b.x, b.y + H / 2.0);
    [
        a,
        Point::new(a.x + 60.0, a.y),
        Point::new(end.x - 60.0, end.y),
        end,
    ]
}

/// Whether `p` lies within 6 px of a link's curve.
fn near_link(w: &WorkflowView, l: &LinkView, p: Point) -> bool {
    let (Some(f), Some(t)) = (node(w, &l.from), node(w, &l.to)) else {
        return false;
    };
    let [a, b, c, d] = curve(f, t);
    (0..=24).any(|i| {
        let t = i as f32 / 24.0;
        let u = 1.0 - t;
        let x = u * u * u * a.x + 3.0 * u * u * t * b.x + 3.0 * u * t * t * c.x + t * t * t * d.x;
        let y = u * u * u * a.y + 3.0 * u * u * t * b.y + 3.0 * u * t * t * c.y + t * t * t * d.y;
        p.distance(Point::new(x, y)) < 6.0
    })
}

/// What `p` is over. A port counts only where the steps can be changed.
pub fn hit(w: &WorkflowView, p: Point, editable: bool) -> Hit {
    let on_port = |n: &&NodeView| editable && p.distance(port(n)) < 9.0;
    if let Some(n) = w.nodes.iter().rev().find(on_port) {
        return Hit::Port(n.id.clone());
    }
    if let Some(n) = w.nodes.iter().rev().find(|n| inside(n, p)) {
        return Hit::Step(n.id.clone());
    }
    match w.links.iter().find(|l| near_link(w, l, p)) {
        Some(l) => Hit::Link(l.from.clone(), l.to.clone()),
        None => Hit::Nothing,
    }
}

/// A press: pick what is under it, and start moving a step or drawing a
/// link when the workflow can be changed.
pub fn press(w: &WorkflowView, p: Point, editable: bool) -> (Pick, Option<Drag>) {
    match hit(w, p, editable) {
        Hit::Port(id) => {
            let from = port(node(w, &id).expect("hit a step"));
            (Pick::Step(id.clone()), Some(Drag::new(&id, true, p, from)))
        }
        Hit::Step(id) => {
            let n = node(w, &id).expect("hit a step");
            let drag =
                editable.then(|| Drag::new(&id, false, p, Point::new(n.x as f32, n.y as f32)));
            (Pick::Step(id), drag)
        }
        Hit::Link(from, to) => (Pick::Link(from, to), None),
        Hit::Nothing => (Pick::Nothing, None),
    }
}

/// A move: a moving step follows the pointer.
pub fn follow(w: &mut WorkflowView, drag: &mut Drag, p: Point) {
    drag.now = p;
    if drag.linking {
        return;
    }
    let to = drag.moved();
    if let Some(n) = w.nodes.iter_mut().find(|n| n.id == drag.node) {
        (n.x, n.y) = (to.x as i32, to.y as i32);
    }
}

/// A release: a link drawn out of a step lands on the step let go over.
pub fn release(w: &mut WorkflowView, drag: &Drag, p: Point) {
    if let (true, Hit::Step(to)) = (drag.linking, hit(w, p, true)) {
        add_link(w, &drag.node, &to);
    }
}

/// A step id from `base` that `w` does not use yet.
fn fresh_id(w: &WorkflowView, base: &str) -> String {
    let taken = |id: &str| w.nodes.iter().any(|n| n.id == id);
    let mut id = base.to_string();
    let mut n = 1;
    while taken(&id) {
        n += 1;
        id = format!("{base}{n}");
    }
    id
}

/// Add a lights-on step of `stage` right of the others, named `step`, or
/// for a Bastion stage, the stage.
pub fn add_step(w: &mut WorkflowView, stage: &str) {
    let x = w.nodes.iter().map(|n| n.x + 220).max().unwrap_or(0);
    let custom = ["prompt", "agent", "skill", "shell"].contains(&stage);
    let id = fresh_id(w, if custom { "step" } else { stage });
    w.nodes.push(NodeView {
        id,
        stage: stage.into(),
        dial: "lights_on".into(),
        x,
        ..NodeView::default()
    });
}

/// Give step `id` the name `to`, and its links with it. Refused when the
/// name is empty or another step has it.
pub fn rename_step(w: &mut WorkflowView, id: &str, to: &str) -> Result<(), String> {
    let to = to.trim();
    let taken = w.nodes.iter().any(|n| n.id == to && n.id != id);
    if to.is_empty() || taken {
        return Err(format!("{to:?} is empty or another step's name"));
    }
    for n in w.nodes.iter_mut().filter(|n| n.id == id) {
        n.id = to.into();
    }
    for l in &mut w.links {
        for end in [&mut l.from, &mut l.to] {
            if end == id {
                *end = to.into();
            }
        }
    }
    Ok(())
}

pub fn add_link(w: &mut WorkflowView, from: &str, to: &str) {
    let exists = w.links.iter().any(|l| l.from == from && l.to == to);
    if from != to && !exists {
        w.links.push(LinkView {
            from: from.into(),
            to: to.into(),
            failure: false,
        });
    }
}

/// Remove step `id` and every link to or from it.
pub fn remove_step(w: &mut WorkflowView, id: &str) {
    w.nodes.retain(|n| n.id != id);
    w.links.retain(|l| l.from != id && l.to != id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> WorkflowView {
        let mut w = WorkflowView::default();
        add_step(&mut w, "plan");
        add_step(&mut w, "skill");
        add_step(&mut w, "skill");
        w
    }

    #[test]
    fn a_new_step_gets_a_free_id_right_of_the_others() {
        let ids: Vec<(String, i32)> = flow().nodes.into_iter().map(|n| (n.id, n.x)).collect();
        assert_eq!(
            ids,
            [
                ("plan".to_string(), 0),
                ("step".into(), 220),
                ("step2".into(), 440)
            ]
        );
    }

    #[test]
    fn a_link_is_kept_once_and_goes_with_its_step() {
        let mut w = flow();
        add_link(&mut w, "plan", "skill");
        add_link(&mut w, "plan", "skill");
        add_link(&mut w, "skill", "skill");
        add_link(&mut w, "skill", "skill2");
        assert_eq!(w.links.len(), 2);
        remove_step(&mut w, "skill");
        assert!(w.links.is_empty());
    }

    #[test]
    fn dragging_a_step_moves_it_and_stays_on_the_canvas() {
        let mut w = flow();
        let inside_plan = Point::new(PAD + 10.0, PAD + 10.0);
        let (pick, drag) = press(&w, inside_plan, true);
        assert_eq!(pick, Pick::Step("plan".into()));
        let mut drag = drag.unwrap();
        follow(
            &mut w,
            &mut drag,
            Point::new(inside_plan.x + 30.0, inside_plan.y + 40.0),
        );
        assert_eq!((w.nodes[0].x, w.nodes[0].y), (30, 40));
        follow(&mut w, &mut drag, Point::new(0.0, 0.0));
        assert_eq!((w.nodes[0].x, w.nodes[0].y), (0, 0));
        assert_eq!(
            press(&w, inside_plan, false).1,
            None,
            "a read-only step does not move"
        );
    }

    #[test]
    fn a_link_drawn_from_a_port_lands_on_the_step_let_go_over() {
        let mut w = flow();
        let (_, drag) = press(&w, Point::new(PAD + W, PAD + H / 2.0), true);
        let drag = drag.unwrap();
        assert!(drag.linking);
        release(&mut w, &drag, Point::new(PAD + 220.0 + 20.0, PAD + 20.0));
        assert_eq!(
            w.links,
            [LinkView {
                from: "plan".into(),
                to: "step".into(),
                failure: false,
            }]
        );
        let mid = Point::new(PAD + W + 26.0, PAD + H / 2.0);
        assert_eq!(hit(&w, mid, true), Hit::Link("plan".into(), "step".into()));
        assert_eq!(
            hit(&w, Point::new(PAD + W, PAD + H / 2.0), false),
            Hit::Step("plan".into())
        );
    }
}
