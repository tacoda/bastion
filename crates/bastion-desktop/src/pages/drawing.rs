//! A workflow drawn on a canvas (DECISIONS.md D35): each step a box, each link a
//! curve from one box's right edge to the next box's left. The job page
//! draws it read-only, each step marked with its state.

use std::collections::BTreeMap;

use bastion_protocol::{NodeView, WorkflowView};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineDash, Path, Stroke, Text};
use iced::{Element, Point, Rectangle, Renderer, Size, Theme, mouse};

use super::edit::{Drag, Pick, curve, node};
use super::mark;
use crate::theme;

pub const W: f32 = 168.0;
pub const H: f32 = 56.0;
pub const PAD: f32 = 24.0;

fn words(word: &str) -> String {
    word.replace('_', " ")
}

/// A step's first line: its name, which the workflow gives it.
pub fn title(n: &NodeView) -> String {
    words(&n.id)
}

/// A step's second line: its dial and the results it gives.
pub fn subtitle(n: &NodeView) -> String {
    match n.results.is_empty() {
        true => String::new(),
        false => format!("→ {}", n.results.join(", ")),
    }
}

/// A step's dial as an icon: a moon when it runs on its own, a sun when a
/// person decides.
fn dial_icon(n: &NodeView) -> &'static str {
    match n.dial.as_str() {
        "dark" => crate::icons::MOON,
        _ => crate::icons::SUN,
    }
}

/// The canvas size that holds every step.
pub fn extent(w: &WorkflowView) -> Size {
    let fit = fit(w);
    Size::new(fit.width.max(800.0), fit.height.max(240.0))
}

/// The room the workflow's steps take, and no more: a drawing nobody edits
/// needs no space to drag a step into.
fn fit(w: &WorkflowView) -> Size {
    let x = w.nodes.iter().map(|n| n.x).max().unwrap_or(0) as f32;
    let y = w.nodes.iter().map(|n| n.y).max().unwrap_or(0) as f32;
    Size::new(x + W + 2.0 * PAD, y + H + 2.0 * PAD)
}

fn corner(n: &NodeView) -> Point {
    Point::new(n.x as f32 + PAD, n.y as f32 + PAD)
}

/// The curve from `from`'s right edge to `to`'s left, with its arrowhead.
fn link(frame: &mut Frame, from: &NodeView, to: &NodeView, picked: bool) {
    let [start, c1, c2, end] = curve(from, to);
    let color = if picked { theme::PRIMARY } else { theme::MUTED };
    let line = Path::new(|p| {
        p.move_to(start);
        p.bezier_curve_to(c1, c2, end);
    });
    frame.stroke(&line, Stroke::default().with_width(2.0).with_color(color));
    let head = Path::new(|p| {
        p.move_to(end);
        p.line_to(Point::new(end.x - 8.0, end.y - 4.0));
        p.line_to(Point::new(end.x - 8.0, end.y + 4.0));
        p.close();
    });
    frame.fill(&head, color);
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

/// How a step's box is drawn: its run state, whether it is picked, and
/// whether it shows the port a link starts from.
struct Look<'a> {
    state: Option<&'a String>,
    picked: bool,
    port: bool,
}

/// One step's box: its state mark and title, then its dial and results.
fn step(frame: &mut Frame, n: &NodeView, look: Look) {
    let at = corner(n);
    let body = Path::rounded_rectangle(at, Size::new(W, H), 8.0.into());
    frame.fill(&body, theme::BG);
    let edge = if look.picked {
        theme::PRIMARY
    } else {
        theme::LINE
    };
    frame.stroke(&body, Stroke::default().with_width(2.0).with_color(edge));
    let mut x = at.x + 10.0;
    if let Some(s) = look
        .state
        .filter(|s| super::IN_PROGRESS.contains(&s.as_str()))
    {
        let center = Point::new(x + 6.0, at.y + 18.0);
        super::spinner::ring(frame, center, 6.0, mark(s).1);
        x += 16.0;
    } else if let Some((symbol, color)) = look.state.map(|s| mark(s)) {
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
    let icon = crate::icons::colored(dial_icon(n), theme::MUTED);
    frame.draw_svg(
        iced::Rectangle::new(Point::new(at.x + 10.0, at.y + 33.0), Size::new(13.0, 13.0)),
        &icon,
    );
    frame.fill_text(caption(
        subtitle(n),
        Point::new(at.x + 29.0, at.y + 32.0),
        theme::MUTED,
        12.0,
    ));
    if look.port {
        frame.fill(
            &Path::circle(Point::new(at.x + W, at.y + H / 2.0), 7.0),
            theme::PRIMARY,
        );
    }
}

/// The canvas's pointer, as the page hears it, in canvas coordinates.
#[derive(Clone, Copy, Debug)]
pub enum Pointer {
    Press(Point),
    Move(Point),
    Release(Point),
}

/// A workflow on the canvas, each step marked with its state by node id.
/// With `on`, the pointer picks, moves and links; without it, it is drawn
/// read-only.
pub struct Drawing<'a, M> {
    pub workflow: &'a WorkflowView,
    pub marks: &'a BTreeMap<String, String>,
    pub picked: &'a Pick,
    pub editable: bool,
    pub drag: Option<&'a Drag>,
    pub on: Option<fn(Pointer) -> M>,
}

impl<'a, M> Drawing<'a, M> {
    pub fn read_only(workflow: &'a WorkflowView, marks: &'a BTreeMap<String, String>) -> Self {
        Drawing {
            workflow,
            marks,
            picked: &Pick::Nothing,
            editable: false,
            drag: None,
            on: None,
        }
    }
}

/// The pointer event a canvas event is, at `at`.
fn pointer(event: &canvas::Event, at: Option<Point>, dragging: bool) -> Option<Pointer> {
    use iced::mouse::{Button, Event as Mouse};
    match event {
        canvas::Event::Mouse(Mouse::ButtonPressed(Button::Left)) => at.map(Pointer::Press),
        canvas::Event::Mouse(Mouse::CursorMoved { .. }) if dragging => at.map(Pointer::Move),
        canvas::Event::Mouse(Mouse::ButtonReleased(Button::Left)) if dragging => {
            Some(Pointer::Release(at.unwrap_or_default()))
        }
        _ => None,
    }
}

impl<M> canvas::Program<M> for Drawing<'_, M> {
    type State = ();

    // iced's trait fixes this signature.
    fn update(
        &self,
        _: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let on = self.on?;
        let p = pointer(event, cursor.position_in(bounds), self.drag.is_some())?;
        Some(canvas::Action::publish(on(p)).and_capture())
    }

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
        let w = self.workflow;
        for l in &w.links {
            if let (Some(from), Some(to)) = (node(w, &l.from), node(w, &l.to)) {
                link(
                    &mut frame,
                    from,
                    to,
                    *self.picked == Pick::Link(l.from.clone(), l.to.clone()),
                );
            }
        }
        for n in &w.nodes {
            let look = Look {
                state: self.marks.get(&n.id),
                picked: *self.picked == Pick::Step(n.id.clone()),
                port: self.editable,
            };
            step(&mut frame, n, look);
        }
        if let Some(d) = self.drag.filter(|d| d.linking) {
            let dash = Stroke {
                line_dash: LineDash {
                    segments: &[4.0, 4.0],
                    offset: 0,
                },
                ..Stroke::default()
            };
            frame.stroke(
                &Path::line(d.origin, d.now),
                dash.with_width(2.0).with_color(theme::PRIMARY),
            );
        }
        vec![frame.into_geometry()]
    }
}

pub fn view<'a, M: 'a>(drawing: Drawing<'a, M>) -> Element<'a, M> {
    let size = match drawing.on {
        Some(_) => extent(drawing.workflow),
        None => fit(drawing.workflow),
    };
    let canvas = Canvas::new(drawing).width(size.width).height(size.height);
    let both = iced::widget::scrollable::Direction::Both {
        vertical: Default::default(),
        horizontal: Default::default(),
    };
    iced::widget::container(iced::widget::scrollable(canvas).direction(both))
        .style(theme::panel)
        .into()
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
            reach: Default::default(),
            results: results.iter().map(|r| r.to_string()).collect(),
            x: 200,
            y: 100,
        }
    }

    #[test]
    fn a_step_names_its_run_and_its_results() {
        let mut review = node("skill", "research", &[]);
        review.id = "code_review".into();
        assert_eq!(title(&review), "code review", "a step goes by its name");
        assert_eq!(
            subtitle(&node("plan", "", &["design-notes"])),
            "→ design-notes"
        );
        let mut dark = node("plan", "", &[]);
        assert_eq!(dial_icon(&dark), crate::icons::SUN);
        dark.dial = "dark".into();
        assert_eq!(dial_icon(&dark), crate::icons::MOON);
    }

    #[test]
    fn the_canvas_holds_every_step_and_never_shrinks_below_its_minimum() {
        let mut w = WorkflowView::default();
        assert_eq!(extent(&w), Size::new(800.0, 240.0));
        assert_eq!(fit(&w), Size::new(W + 2.0 * PAD, H + 2.0 * PAD));
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
