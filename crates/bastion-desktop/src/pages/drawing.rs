//! A workflow drawn on a canvas (PLAN.md D35): each step a box, each link a
//! curve from one box's right edge to the next box's left. The job page
//! draws it read-only, each step marked with its state.

use std::collections::BTreeMap;

use bastion_protocol::{NodeView, WorkflowView};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::{Element, Point, Rectangle, Renderer, Size, Theme, mouse};

use super::mark;
use crate::theme;

pub const W: f32 = 168.0;
pub const H: f32 = 56.0;
pub const PAD: f32 = 24.0;

fn words(word: &str) -> String {
    word.replace('_', " ")
}

/// A step's first line: a skill's or command's run, else its stage.
pub fn title(n: &NodeView) -> String {
    match n.stage.as_str() {
        "skill" | "command" if !n.run.is_empty() => format!("{} · {}", n.stage, n.run),
        _ => words(&n.stage),
    }
}

/// A step's second line: its dial and the results it gives.
pub fn subtitle(n: &NodeView) -> String {
    let dial = if n.dial == "dark" {
        "dark"
    } else {
        "lights-on"
    };
    match n.results.is_empty() {
        true => dial.into(),
        false => format!("{dial} → {}", n.results.join(", ")),
    }
}

/// The canvas size that holds every step.
pub fn extent(w: &WorkflowView) -> Size {
    let x = w.nodes.iter().map(|n| n.x).max().unwrap_or(0) as f32;
    let y = w.nodes.iter().map(|n| n.y).max().unwrap_or(0) as f32;
    Size::new(
        (x + W + 2.0 * PAD).max(800.0),
        (y + H + 2.0 * PAD).max(240.0),
    )
}

fn corner(n: &NodeView) -> Point {
    Point::new(n.x as f32 + PAD, n.y as f32 + PAD)
}

/// The curve from `from`'s right edge to `to`'s left, with its arrowhead.
fn link(frame: &mut Frame, from: &NodeView, to: &NodeView) {
    let (a, b) = (corner(from), corner(to));
    let (start, end) = (
        Point::new(a.x + W, a.y + H / 2.0),
        Point::new(b.x, b.y + H / 2.0),
    );
    let curve = Path::new(|p| {
        p.move_to(start);
        p.bezier_curve_to(
            Point::new(start.x + 60.0, start.y),
            Point::new(end.x - 60.0, end.y),
            end,
        );
    });
    frame.stroke(
        &curve,
        Stroke::default().with_width(2.0).with_color(theme::MUTED),
    );
    let head = Path::new(|p| {
        p.move_to(end);
        p.line_to(Point::new(end.x - 8.0, end.y - 4.0));
        p.line_to(Point::new(end.x - 8.0, end.y + 4.0));
        p.close();
    });
    frame.fill(&head, theme::MUTED);
}

fn caption(content: String, position: Point, color: iced::Color, size: f32) -> Text {
    Text {
        content,
        position,
        color,
        size: size.into(),
        font: theme::FONT,
        ..Text::default()
    }
}

/// One step's box: its state mark and title, then its dial and results.
fn step(frame: &mut Frame, n: &NodeView, state: Option<&String>) {
    let at = corner(n);
    let body = Path::rounded_rectangle(at, Size::new(W, H), 8.0.into());
    frame.fill(&body, theme::BG);
    frame.stroke(
        &body,
        Stroke::default().with_width(2.0).with_color(theme::LINE),
    );
    let mut x = at.x + 10.0;
    if let Some((symbol, color)) = state.map(|s| mark(s)) {
        frame.fill_text(caption(
            symbol.into(),
            Point::new(x, at.y + 10.0),
            color,
            14.0,
        ));
        x += 16.0;
    }
    frame.fill_text(caption(
        title(n),
        Point::new(x, at.y + 10.0),
        theme::INK,
        14.0,
    ));
    frame.fill_text(caption(
        subtitle(n),
        Point::new(at.x + 10.0, at.y + 32.0),
        theme::MUTED,
        12.0,
    ));
}

/// A workflow drawn read-only, each step marked with its state by node id.
pub struct Drawing<'a> {
    pub workflow: &'a WorkflowView,
    pub marks: &'a BTreeMap<String, String>,
}

impl<M> canvas::Program<M> for Drawing<'_> {
    type State = ();

    // iced's trait fixes this signature.
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let node = |id: &str| self.workflow.nodes.iter().find(|n| n.id == id);
        for l in &self.workflow.links {
            if let (Some(from), Some(to)) = (node(&l.from), node(&l.to)) {
                link(&mut frame, from, to);
            }
        }
        for n in &self.workflow.nodes {
            step(&mut frame, n, self.marks.get(&n.id));
        }
        vec![frame.into_geometry()]
    }
}

pub fn view<'a, M: 'a>(drawing: Drawing<'a>) -> Element<'a, M> {
    let size = extent(drawing.workflow);
    let canvas = Canvas::new(drawing).width(size.width).height(size.height);
    let scroll =
        iced::widget::scrollable(canvas).direction(iced::widget::scrollable::Direction::Both {
            vertical: Default::default(),
            horizontal: Default::default(),
        });
    iced::widget::container(scroll).style(theme::panel).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(stage: &str, run: &str, results: &[&str]) -> NodeView {
        NodeView {
            id: "n".into(),
            stage: stage.into(),
            run: run.into(),
            dial: "lights_on".into(),
            note: String::new(),
            results: results.iter().map(|r| r.to_string()).collect(),
            x: 200,
            y: 100,
        }
    }

    #[test]
    fn a_step_names_its_run_and_its_results() {
        assert_eq!(title(&node("code_review", "", &[])), "code review");
        assert_eq!(title(&node("skill", "research", &[])), "skill · research");
        assert_eq!(
            subtitle(&node("plan", "", &["design-notes"])),
            "lights-on → design-notes"
        );
    }

    #[test]
    fn the_canvas_holds_every_step_and_never_shrinks_below_its_minimum() {
        let mut w = WorkflowView::default();
        assert_eq!(extent(&w), Size::new(800.0, 240.0));
        w.nodes = vec![
            node("plan", "", &[]),
            NodeView {
                x: 900,
                ..node("build", "", &[])
            },
        ];
        assert_eq!(extent(&w), Size::new(900.0 + W + 2.0 * PAD, 240.0));
    }
}
