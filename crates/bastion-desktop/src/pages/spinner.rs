//! The mark of a job or step in progress: a dashed circle that turns.

use std::f32::consts::TAU;
use std::time::{SystemTime, UNIX_EPOCH};

use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::svg;
use iced::{Color, Element, Point, Radians, Rotation, Vector};

/// How long one turn takes.
const TURN_MS: u128 = 1200;
/// How many dashes the circle has.
const DASHES: f32 = 8.0;

/// The circle's angle at `ms` since the epoch, in radians.
fn angle(ms: u128) -> f32 {
    (ms % TURN_MS) as f32 / TURN_MS as f32 * TAU
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Draw the turning circle at `center` on `frame`.
pub fn ring(frame: &mut Frame, center: Point, radius: f32, color: Color) {
    frame.with_save(|f| {
        f.translate(Vector::new(center.x, center.y));
        f.rotate(angle(now_ms()));
        let dash = TAU * radius / DASHES / 2.0;
        let stroke = Stroke {
            line_dash: canvas::LineDash {
                segments: &[dash, dash],
                offset: 0,
            },
            ..Stroke::default().with_width(1.75).with_color(color)
        };
        f.stroke(&Path::circle(Point::ORIGIN, radius), stroke);
    });
}

/// The dashed circle, drawn once; the widget turns it.
const RING: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="none" stroke="black" stroke-width="2.4" stroke-dasharray="3.93 3.93"/></svg>"#;

/// The turning circle as a widget, `size` across.
pub fn spinner<'a, M: 'a>(size: f32, color: Color) -> Element<'a, M> {
    let turned = Rotation::Floating(Radians(angle(now_ms())));
    iced::widget::svg(svg::Handle::from_memory(RING))
        .width(size)
        .height(size)
        .rotation(turned)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_circle_turns_once_each_turn() {
        assert_eq!(angle(0), 0.0);
        assert_eq!(angle(TURN_MS), 0.0);
        assert!((angle(TURN_MS / 2) - TAU / 2.0).abs() < 1e-5);
    }
}
