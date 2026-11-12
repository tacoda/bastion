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
pub const H: f32 = 74.0;
pub const PAD: f32 = 24.0;

fn words(word: &str) -> String {
    word.replace('_', " ")
}

/// The most characters a step's name shows, clear of the dial at the
/// box's top right.
const NAME: usize = 20;

/// A step's first line: its name, which the workflow gives it.
pub fn title(n: &NodeView) -> String {
    let name = words(&n.id);
    match name.chars().count() > NAME {
        true => name.chars().take(NAME - 1).chain(['…']).collect(),
        false => name,
    }
}

/// The most characters a line of a step's box holds.
const LINE: usize = 24;

/// `s`, cut to a box's line with an ellipsis.
fn clip(s: String) -> String {
    match s.chars().count() > LINE {
        true => s.chars().take(LINE - 1).chain(['…']).collect(),
        false => s,
    }
}

/// A step's last line, beside the artifact icon: the results it gives.
pub fn subtitle(n: &NodeView) -> String {
    clip(n.results.join(", "))
}

/// Under the name: the step's kind and what it runs, how far it reaches,
/// and its budget when it has one (D43).
pub fn kind_line(n: &NodeView) -> String {
    let kind = match n.stage.as_str() {
        "agent" => format!("Agent · {}", n.run),
        "skill" => format!("Skill · {}", n.run),
        "shell" => "Shell".into(),
        _ => "Prompt".into(),
    };
    let reach = match (n.stage.as_str(), n.reach.network, n.reach.reads_only) {
        ("shell", true, _) => Some("network".to_string()),
        (_, _, true) => Some("reads only".to_string()),
        _ => None,
    };
    let spend = n
        .options
        .spend_micros
        .map(|m| format!("${:.2}", m as f64 / 1e6));
    let revisions = n.options.revisions.map(|r| format!("{r}×"));
    let all: Vec<String> = [Some(kind), reach, spend, revisions]
        .into_iter()
        .flatten()
        .collect();
    clip(all.join(" · "))
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
/// One link as a curve with its head: a failure link red and dashed (D47).
fn link(frame: &mut Frame, from: &NodeView, to: &NodeView, look: (bool, bool)) {
    let (picked, failure) = look;
    let [start, c1, c2, end] = curve(from, to);
    let color = match (picked, failure) {
        (true, _) => theme::primary_color(),
        (false, true) => theme::bad(),
        (false, false) => theme::muted(),
    };
    let line = Path::new(|p| {
        p.move_to(start);
        p.bezier_curve_to(c1, c2, end);
    });
    let mut stroke = Stroke::default().with_width(2.0).with_color(color);
    if failure {
        stroke.line_dash = LineDash {
            segments: &[6.0, 4.0],
            offset: 0,
        };
    }
    frame.stroke(&line, stroke);
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
    frame.fill(&body, theme::bg());
    let edge = if look.picked {
        theme::primary_color()
    } else {
        theme::line()
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
        theme::ink(),
        14.0,
    ));
    let line = |content: String, x: f32, y: f32| {
        caption(
            content,
            Point::new(at.x + x, at.y + y),
            theme::muted(),
            12.0,
        )
    };
    let icon = |frame: &mut Frame, svg: &str, x: f32, y: f32| {
        let at = Point::new(at.x + x, at.y + y);
        frame.draw_svg(
            iced::Rectangle::new(at, Size::new(13.0, 13.0)),
            &crate::icons::colored(svg, theme::muted()),
        );
    };
    // The dial at the top right; the kind under the name; the artifacts below.
    icon(frame, dial_icon(n), W - 23.0, 11.0);
    frame.fill_text(line(kind_line(n), 10.0, 32.0));
    if !n.results.is_empty() {
        icon(frame, crate::icons::ARTIFACT, 10.0, 51.0);
        frame.fill_text(line(subtitle(n), 29.0, 50.0));
    }
    if look.port {
        frame.fill(
            &Path::circle(Point::new(at.x + W, at.y + H / 2.0), 7.0),
            theme::primary_color(),
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
                let picked = *self.picked == Pick::Link(l.from.clone(), l.to.clone());
                link(&mut frame, from, to, (picked, l.failure));
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
                dash.with_width(2.0).with_color(theme::primary_color()),
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
        .width(iced::Fill)
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
            options: Default::default(),
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
        review.id = "a_very_long_step_name_indeed".into();
        assert_eq!(title(&review), "a very long step na…", "clear of the dial");
        assert_eq!(
            subtitle(&node("plan", "", &["design-notes", "risks"])),
            "design-notes, risks"
        );
        let mut dark = node("plan", "", &[]);
        assert_eq!(dial_icon(&dark), crate::icons::SUN);
        dark.dial = "dark".into();
        assert_eq!(dial_icon(&dark), crate::icons::MOON);
    }

    #[test]
    fn a_step_shows_its_kind_reach_and_budget_beside_its_dial() {
        let mut agent = node("agent", "security", &["review"]);
        assert_eq!(kind_line(&agent), "Agent · security");
        agent.reach.reads_only = true;
        agent.options.spend_micros = Some(1_500_000);
        agent.options.revisions = Some(3);
        assert_eq!(
            kind_line(&agent),
            "Agent · security · read…",
            "clipped to the box"
        );
        let mut prompt = node("prompt", "", &[]);
        prompt.options.spend_micros = Some(1_500_000);
        prompt.options.revisions = Some(3);
        assert_eq!(kind_line(&prompt), "Prompt · $1.50 · 3×");
        let mut shell = node("shell", "", &[]);
        shell.reach.network = true;
        assert_eq!(kind_line(&shell), "Shell · network");
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
        let tall = (100.0 + H + 2.0 * PAD).max(240.0);
        assert_eq!(extent(&w), Size::new(900.0 + W + 2.0 * PAD, tall));
    }
}
