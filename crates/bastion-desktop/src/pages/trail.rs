//! A job drawn as a trail (L4), after the software expedition: the request
//! at the outpost, each step a landmark the route climbs past, a marker
//! where the job is now, and the final peak where it lands. A job that
//! stops or fails ends in the marsh. Pressing a landmark opens its step.

use bastion_protocol::StepView;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineDash, Path, Stroke, Text};
use iced::{Element, Fill, Point, Rectangle, Renderer, Theme, mouse};

use crate::theme;

const HEIGHT: f32 = 190.0;
/// Room at each end for the outpost and the peak.
const END: f32 = 70.0;
const DOT: f32 = 10.0;
/// How near a press must land to a landmark to pick it.
const REACH: f32 = 16.0;

/// One step's place on the trail.
#[derive(Clone, Debug, PartialEq)]
pub struct Landmark {
    pub id: String,
    pub state: String,
    pub at: Point,
}

/// Where the route climbs: the outpost low on the left, each step a little
/// higher, the peak high on the right, with a gentle wander so it reads as
/// ground rather than a chart.
fn rise(i: usize, n: usize, width: f32) -> Point {
    let span = (width - 2.0 * END).max(1.0);
    let t = (i as f32 + 1.0) / (n as f32 + 1.0);
    let wander = (i as f32 * 1.7).sin() * 10.0;
    Point::new(END + t * span, HEIGHT * 0.70 - t * HEIGHT * 0.38 + wander)
}

/// Each step's landmark on a trail `width` wide, in the order the job runs.
pub fn landmarks(steps: &[StepView], width: f32) -> Vec<Landmark> {
    let n = steps.len();
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| Landmark {
            id: s.id.clone(),
            state: s.state.clone(),
            at: rise(i, n, width),
        })
        .collect()
}

fn outpost() -> Point {
    Point::new(END * 0.5, HEIGHT * 0.74)
}

fn peak(width: f32) -> Point {
    Point::new(width - END * 0.5, HEIGHT * 0.26)
}

/// The landmark under `p`, if one is near enough to press.
fn under(marks: &[Landmark], p: Point) -> Option<&Landmark> {
    marks.iter().find(|m| m.at.distance(p) <= REACH)
}

/// Whether the route up to this landmark is walked: the steps done.
fn walked(state: &str) -> bool {
    state == "done"
}

/// Where the job is now: the step running or waiting on a person.
fn here(state: &str) -> bool {
    matches!(state, "running" | "waiting" | "queued")
}

pub struct Trail<'a, M> {
    pub steps: &'a [StepView],
    /// The job's own state: `landed`, `failed`, `stopped` or on its way.
    pub job: &'a str,
    pub pick: fn(String) -> M,
}

fn label(content: String, at: Point, size: f32) -> Text {
    Text {
        content,
        position: at,
        color: theme::muted(),
        size: size.into(),
        font: theme::FONT,
        align_x: iced::alignment::Horizontal::Center.into(),
        align_y: iced::alignment::Vertical::Top,
        ..Text::default()
    }
}

/// Faint contour lines, so the trail sits on ground.
fn contours(frame: &mut Frame, width: f32) {
    let line = Stroke::default().with_width(1.0).with_color(theme::line());
    for k in 0..3 {
        let base = HEIGHT * (0.45 + 0.17 * k as f32);
        let path = Path::new(|b| {
            b.move_to(Point::new(0.0, base));
            let mut x = 0.0;
            while x < width {
                let y = base + ((x / 90.0) + k as f32).sin() * 6.0;
                b.line_to(Point::new(x, y));
                x += 12.0;
            }
        });
        frame.stroke(&path, line);
    }
}

/// The route through every landmark, walked solid, the rest dashed.
fn route(frame: &mut Frame, points: &[(Point, bool)]) {
    for pair in points.windows(2) {
        let ((a, _), (b, done)) = (pair[0], pair[1]);
        let mid = Point::new((a.x + b.x) / 2.0, a.y.min(b.y) - 8.0);
        let path = Path::new(|p| {
            p.move_to(a);
            p.quadratic_curve_to(mid, b);
        });
        let stroke = match done {
            true => Stroke::default().with_width(3.0).with_color(theme::ink()),
            false => Stroke {
                line_dash: LineDash {
                    segments: &[6.0, 5.0],
                    offset: 0,
                },
                ..Stroke::default()
            }
            .with_width(2.0)
            .with_color(theme::muted()),
        };
        frame.stroke(&path, stroke);
    }
}

/// A landmark: done is a filled stone, failed a red peak, the rest open.
fn landmark(frame: &mut Frame, m: &Landmark, hovered: bool) {
    let r = if hovered { DOT + 3.0 } else { DOT };
    match m.state.as_str() {
        "failed" => {
            let peak = Path::new(|p| {
                p.move_to(Point::new(m.at.x - r, m.at.y + r));
                p.line_to(Point::new(m.at.x, m.at.y - r * 1.2));
                p.line_to(Point::new(m.at.x + r, m.at.y + r));
                p.close();
            });
            frame.fill(&peak, theme::bad());
        }
        "done" => frame.fill(&Path::circle(m.at, r), theme::ok()),
        state if here(state) => {
            frame.fill(&Path::circle(m.at, r), theme::warn());
            pin(frame, m.at);
        }
        _ => {
            frame.fill(&Path::circle(m.at, r), theme::panel_color());
            let ring = Stroke::default().with_width(2.0).with_color(theme::muted());
            frame.stroke(&Path::circle(m.at, r), ring);
        }
    }
    let name = m.id.replace('_', " ");
    frame.fill_text(label(name, Point::new(m.at.x, m.at.y + r + 6.0), 13.0));
    if hovered {
        let state = label(m.state.clone(), Point::new(m.at.x, m.at.y + r + 22.0), 11.0);
        frame.fill_text(state);
    }
}

/// The marker over the step the job is at now.
fn pin(frame: &mut Frame, at: Point) {
    let top = Point::new(at.x, at.y - 34.0);
    let stem = Stroke::default().with_width(2.0).with_color(theme::ink());
    frame.stroke(&Path::line(Point::new(at.x, at.y - DOT), top), stem);
    let flag = Path::new(|p| {
        p.move_to(top);
        p.line_to(Point::new(top.x + 16.0, top.y + 5.0));
        p.line_to(Point::new(top.x, top.y + 10.0));
        p.close();
    });
    frame.fill(&flag, theme::primary_color());
    let mut you = label("you are here".into(), Point::new(at.x, top.y - 16.0), 11.0);
    you.color = theme::ink();
    frame.fill_text(you);
}

/// The final peak, reached when the job lands; the marsh when it does not.
fn finish(frame: &mut Frame, at: Point, job: &str) {
    if matches!(job, "failed" | "stopped") {
        let reeds = Stroke::default().with_width(2.0).with_color(theme::muted());
        for k in 0..3 {
            let y = at.y + 40.0 + k as f32 * 6.0;
            let wave = Path::new(|p| {
                p.move_to(Point::new(at.x - 22.0, y));
                p.quadratic_curve_to(Point::new(at.x, y - 6.0), Point::new(at.x + 22.0, y));
            });
            frame.stroke(&wave, reeds);
        }
        frame.fill_text(label(
            "the marsh".into(),
            Point::new(at.x, at.y + 66.0),
            12.0,
        ));
        return;
    }
    let reached = job == "landed";
    let summit = Path::new(|p| {
        p.move_to(Point::new(at.x - 26.0, at.y + 22.0));
        p.line_to(Point::new(at.x, at.y - 18.0));
        p.line_to(Point::new(at.x + 26.0, at.y + 22.0));
        p.close();
    });
    let color = if reached {
        theme::ok()
    } else {
        theme::raised()
    };
    frame.fill(&summit, color);
    let edge = Stroke::default().with_width(2.0).with_color(theme::muted());
    frame.stroke(&summit, edge);
    let words = if reached { "landed" } else { "final peak" };
    frame.fill_text(label(words.into(), Point::new(at.x, at.y + 28.0), 12.0));
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
        let over = at.and_then(|p| under(&marks, p)).map(|m| m.id.clone());
        let press = matches!(
            event,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        );
        if let (true, Some(id)) = (press, &over) {
            return Some(canvas::Action::publish((self.pick)(id.clone())).and_capture());
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
        let width = bounds.width;
        contours(&mut frame, width);
        let marks = landmarks(self.steps, width);
        let start = (outpost(), true);
        let middle = marks.iter().map(|m| (m.at, walked(&m.state)));
        let end = (peak(width), self.job == "landed");
        let points: Vec<(Point, bool)> = std::iter::once(start)
            .chain(middle)
            .chain(std::iter::once(end))
            .collect();
        route(&mut frame, &points);
        let camp = Path::new(|p| {
            let o = outpost();
            p.move_to(Point::new(o.x - 12.0, o.y + 8.0));
            p.line_to(Point::new(o.x, o.y - 10.0));
            p.line_to(Point::new(o.x + 12.0, o.y + 8.0));
            p.close();
        });
        frame.fill(&camp, theme::muted());
        let o = outpost();
        frame.fill_text(label("request".into(), Point::new(o.x, o.y + 14.0), 12.0));
        finish(&mut frame, peak(width), self.job);
        for m in &marks {
            landmark(&mut frame, m, hovered.as_deref() == Some(m.id.as_str()));
        }
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
        assert!(marks[0].at.y > peak(900.0).y, "the peak is highest");
        let near = Point::new(marks[1].at.x + 5.0, marks[1].at.y);
        assert_eq!(under(&marks, near).map(|m| m.id.as_str()), Some("build"));
        assert_eq!(under(&marks, Point::new(0.0, 0.0)), None);
        assert!(here("waiting") && !here("done") && walked("done"));
    }
}
