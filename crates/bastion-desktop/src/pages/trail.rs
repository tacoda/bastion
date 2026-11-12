//! A job drawn as a journey (L4) along its workflow's trail, on a chart
//! after the software expedition:
//! the request at the outpost, base camp, each step a landmark the route
//! climbs past (a peak for an agent's step, a checkpoint for a script), a
//! marker where the job is now, and the summit where it lands. A job that
//! stops or fails ends in the marsh. When the job comes home, a dotted way
//! leads back to the atlas, where its field notes are kept (D59). Pressing
//! a landmark opens its step; pressing the atlas opens the notes.

use bastion_protocol::StepView;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineDash, Path, Stroke, Text};
use iced::{Element, Fill, Point, Rectangle, Renderer, Size, Theme, mouse};

use super::carto;
use crate::theme;

const HEIGHT: f32 = 340.0;
/// Where the land starts, past the sea of requests.
const SHORE: f32 = 70.0;
/// Room on the right for the summit and the compass.
const EAST: f32 = 190.0;
/// How near a press must land to a landmark to pick it.
const REACH: f32 = 22.0;

/// One step's place on the trail: `at` is the foot of its landmark.
#[derive(Clone, Debug, PartialEq)]
pub struct Landmark {
    pub id: String,
    pub stage: String,
    pub state: String,
    pub at: Point,
}

/// Where the route climbs: from base camp low on the left toward the
/// summit high on the right, with a wander so it reads as ground.
fn rise(i: usize, n: usize, width: f32) -> Point {
    let west = SHORE + 270.0;
    let span = (width - west - EAST).max(1.0);
    let t = (i as f32 + 0.5) / n.max(1) as f32;
    let wander = (i as f32 * 1.9).sin() * 14.0;
    Point::new(west + t * span, HEIGHT * 0.80 - t * HEIGHT * 0.36 + wander)
}

/// Each step's landmark on a trail `width` wide, in the order the job runs.
pub fn landmarks(steps: &[StepView], width: f32) -> Vec<Landmark> {
    let n = steps.len();
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| Landmark {
            id: s.id.clone(),
            stage: s.stage.clone(),
            state: s.state.clone(),
            at: rise(i, n, width),
        })
        .collect()
}

fn outpost() -> Point {
    Point::new(SHORE + 70.0, HEIGHT * 0.60)
}

fn camp() -> Point {
    Point::new(SHORE + 150.0, HEIGHT * 0.68)
}

/// The atlas, low by the outpost, where the way home ends.
fn atlas_at() -> Point {
    Point::new(SHORE + 70.0, HEIGHT * 0.93)
}

/// The id a press on the atlas publishes; no step has it.
pub const ATLAS: &str = "atlas";

/// Whether the job has come home: landed, failed or stopped.
fn home(job: &str) -> bool {
    matches!(job, "landed" | "failed" | "stopped")
}

fn summit(width: f32) -> Point {
    Point::new(width - EAST * 0.62, HEIGHT * 0.40)
}

/// The landmark whose drawing is under `p`, if one is near enough.
fn under(marks: &[Landmark], p: Point) -> Option<&Landmark> {
    let middle = |m: &Landmark| Point::new(m.at.x, m.at.y - 16.0);
    marks.iter().find(|m| middle(m).distance(p) <= REACH)
}

/// Whether the route up to this landmark is walked: the steps done.
fn walked(state: &str) -> bool {
    state == "done"
}

/// Where the job is now: the step running or waiting on a person.
fn here(state: &str) -> bool {
    matches!(state, "running" | "waiting" | "queued")
}

/// A landmark pressed: its step, where it stands, and how wide the trail
/// was, so a card can sit beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pressed {
    pub id: String,
    pub at: Point,
    pub width: f32,
}

pub struct Trail<'a, M> {
    pub steps: &'a [StepView],
    /// The job's own state: `landed`, `failed`, `stopped` or on its way.
    pub job: &'a str,
    pub pick: fn(Pressed) -> M,
}

/// The trail's height, for a card laid over it.
pub const TALL: f32 = HEIGHT;

/// The route out, walked in red dashes, the rest of the way dotted.
fn route(frame: &mut Frame, points: &[(Point, bool)]) {
    for pair in points.windows(2) {
        let ((a, _), (b, done)) = (pair[0], pair[1]);
        let mid = Point::new((a.x + b.x) / 2.0, a.y.min(b.y) - 10.0);
        let path = Path::new(|p| {
            p.move_to(a);
            p.quadratic_curve_to(mid, b);
        });
        let (dash, width, color): (&[f32], f32, _) = match done {
            true => (&[9.0, 6.0], 3.0, theme::link_color()),
            false => (&[2.0, 5.0], 2.0, theme::ink()),
        };
        let stroke = Stroke {
            line_dash: LineDash {
                segments: dash,
                offset: 0,
            },
            ..Stroke::default()
        };
        frame.stroke(&path, stroke.with_width(width).with_color(color));
    }
}

/// A shell step is a checkpoint; an agent's step is a peak to climb.
fn landmark(frame: &mut Frame, m: &Landmark, i: usize, hovered: bool) {
    let faint = m.state == "pending";
    if m.stage == "shell" {
        carto::castle(frame, m.at, faint);
    } else {
        let height = 38.0 + (i % 3) as f32 * 7.0 + if hovered { 6.0 } else { 0.0 };
        carto::mountain(frame, m.at, height, faint);
    }
    match m.state.as_str() {
        "failed" => carto::cross(frame, Point::new(m.at.x, m.at.y + 4.0), theme::bad()),
        "done" => carto::flag(frame, Point::new(m.at.x + 14.0, m.at.y), theme::ok()),
        state if here(state) => pin(frame, Point::new(m.at.x, m.at.y - 40.0)),
        _ => {}
    }
    let note = if hovered { m.state.as_str() } else { "" };
    carto::name(
        frame,
        Point::new(m.at.x, m.at.y + 8.0),
        &m.id.replace('_', " "),
        note,
    );
}

/// The marker over the step the job is at now.
fn pin(frame: &mut Frame, tip: Point) {
    let head = Point::new(tip.x, tip.y - 14.0);
    let body = Path::new(|p| {
        p.move_to(tip);
        p.line_to(Point::new(head.x - 7.0, head.y));
        p.line_to(Point::new(head.x + 7.0, head.y));
        p.close();
    });
    frame.fill(&body, theme::bad());
    frame.fill(&Path::circle(head, 7.0), theme::bad());
    frame.fill(&Path::circle(head, 2.5), theme::panel_color());
    let mut you = Text {
        content: "you are here".into(),
        position: Point::new(head.x, head.y - 24.0),
        size: 12.0.into(),
        font: carto::SERIF,
        align_x: iced::alignment::Horizontal::Center.into(),
        ..Text::default()
    };
    you.color = theme::ink();
    frame.fill_text(you);
}

/// The summit when the job lands or is on its way; the marsh when it
/// stopped or failed.
/// Where the marsh lies: low, under the summit.
fn marsh_at(width: f32) -> Point {
    Point::new(summit(width).x - 40.0, HEIGHT * 0.84)
}

/// Whether the job ended in the marsh: failed or stopped.
fn lost(job: &str) -> bool {
    matches!(job, "failed" | "stopped")
}

/// The summit, flagged when the job landed, and the marsh below it,
/// crossed out when the job ended there.
fn finish(frame: &mut Frame, width: f32, job: &str) {
    let top = summit(width);
    let low = marsh_at(width);
    carto::marsh(frame, low, lost(job));
    let why = if lost(job) {
        "the trail went nowhere, said out loud"
    } else {
        ""
    };
    carto::name(
        frame,
        Point::new(low.x - 70.0, low.y - 6.0),
        "the marsh",
        why,
    );
    let base = Point::new(top.x, top.y + 70.0);
    carto::mountain(frame, base, 90.0, job != "landed");
    if job == "landed" {
        carto::flag(frame, Point::new(top.x, top.y - 20.0), theme::bad());
    }
    let note = match job {
        "landed" => "it works: the job landed",
        _ => "where it lands",
    };
    carto::name(frame, Point::new(top.x, top.y - 92.0), "the summit", note);
}

/// The ground between: hills and trees, the same each time it is drawn.
fn scenery(frame: &mut Frame, width: f32) {
    for k in 0..4 {
        let x = SHORE + 220.0 + (k as f32 * 271.0) % (width - SHORE - EAST - 260.0).max(1.0);
        carto::hill(
            frame,
            Point::new(x, HEIGHT * 0.30 + (k % 2) as f32 * 18.0),
            46.0,
        );
    }
    for k in 0..9 {
        let x = SHORE + 330.0 + (k as f32 * 157.0) % (width - SHORE - EAST - 380.0).max(1.0);
        let y = HEIGHT * 0.90 - (k % 3) as f32 * 6.0;
        carto::tree(frame, Point::new(x, y));
    }
}

/// The way home, from the summit or the marsh, to the atlas (D59).
fn homeward(frame: &mut Frame, width: f32, job: &str, lit: bool) {
    let from = match job {
        "landed" => Point::new(summit(width).x - 30.0, summit(width).y + 70.0),
        _ => marsh_at(width),
    };
    carto::way_home(
        frame,
        from,
        Point::new(atlas_at().x + 24.0, atlas_at().y - 10.0),
    );
    carto::atlas(frame, atlas_at(), lit);
    let note = if lit {
        "press to read and write field notes"
    } else {
        "the way home, with field notes"
    };
    carto::name(
        frame,
        Point::new(atlas_at().x + 120.0, atlas_at().y - 30.0),
        "the atlas",
        note,
    );
}

impl<M> canvas::Program<M> for Trail<'_, M> {
    /// The landmark the pointer is over.
    type State = Option<String>;

    // iced's trait fixes this signature.
    fn update(
        &self,
        hovered: &mut Option<String>,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let at = cursor.position_in(bounds);
        let marks = landmarks(self.steps, bounds.width);
        let atlas = Landmark {
            id: ATLAS.into(),
            stage: String::new(),
            state: String::new(),
            at: atlas_at(),
        };
        let marks = match home(self.job) {
            true => marks.into_iter().chain(std::iter::once(atlas)).collect(),
            false => marks,
        };
        let mark = at.and_then(|p| under(&marks, p));
        let over = mark.map(|m| m.id.clone());
        let press = matches!(
            event,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        );
        if let (true, Some(m)) = (press, mark) {
            let pressed = Pressed {
                id: m.id.clone(),
                at: m.at,
                width: bounds.width,
            };
            return Some(canvas::Action::publish((self.pick)(pressed)).and_capture());
        }
        if over != *hovered {
            *hovered = over;
            return Some(canvas::Action::request_redraw());
        }
        None
    }

    // iced's trait fixes this signature.
    fn draw(
        &self,
        hovered: &Option<String>,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (width, size) = (bounds.width, Size::new(bounds.width, HEIGHT));
        carto::rhumbs(&mut frame, Point::new(width * 0.55, HEIGHT * 0.55), size);
        carto::coast(&mut frame, SHORE, size);
        scenery(&mut frame, width);
        let marks = landmarks(self.steps, width);
        let start = (camp(), true);
        let middle = marks.iter().map(|m| (m.at, walked(&m.state)));
        let end = match lost(self.job) {
            true => (marsh_at(width), true),
            false => (
                Point::new(summit(width).x, summit(width).y + 70.0),
                self.job == "landed",
            ),
        };
        let points: Vec<(Point, bool)> = std::iter::once((outpost(), true))
            .chain(std::iter::once(start))
            .chain(middle)
            .chain(std::iter::once(end))
            .collect();
        route(&mut frame, &points);
        carto::outpost(&mut frame, outpost());
        carto::name(
            &mut frame,
            Point::new(outpost().x, outpost().y + 8.0),
            "the outpost",
            "the request",
        );
        carto::tents(&mut frame, camp());
        finish(&mut frame, width, self.job);
        if home(self.job) {
            homeward(
                &mut frame,
                width,
                self.job,
                hovered.as_deref() == Some(ATLAS),
            );
        }
        for (i, m) in marks.iter().enumerate() {
            landmark(&mut frame, m, i, hovered.as_deref() == Some(m.id.as_str()));
        }
        carto::compass(&mut frame, Point::new(width - 52.0, HEIGHT - 66.0), 26.0);
        carto::name(
            &mut frame,
            Point::new(width / 2.0, 22.0),
            "the journey",
            "this job, along its workflow's trail",
        );
        carto::border(&mut frame, size);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        hovered: &Option<String>,
        _: Rectangle,
        _: mouse::Cursor,
    ) -> mouse::Interaction {
        match hovered {
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }
}

pub fn view<'a, M: 'a>(trail: Trail<'a, M>) -> Element<'a, M> {
    let canvas = Canvas::new(trail).width(Fill).height(HEIGHT);
    iced::widget::container(canvas)
        .width(Fill)
        .style(theme::panel)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, state: &str) -> StepView {
        StepView {
            id: id.into(),
            stage: "prompt".into(),
            state: state.into(),
        }
    }

    #[test]
    fn the_trail_climbs_left_to_right_and_a_press_picks_the_nearest_landmark() {
        let steps = [
            step("plan", "done"),
            step("build", "running"),
            step("review", "pending"),
        ];
        let marks = landmarks(&steps, 900.0);
        assert!(marks.windows(2).all(|w| w[0].at.x < w[1].at.x), "in order");
        assert!(marks[0].at.y > summit(900.0).y, "the summit is highest");
        let near = Point::new(marks[1].at.x + 5.0, marks[1].at.y - 16.0);
        assert_eq!(under(&marks, near).map(|m| m.id.as_str()), Some("build"));
        assert_eq!(under(&marks, Point::new(0.0, 0.0)), None);
        assert!(here("waiting") && !here("done") && walked("done"));
    }
}
