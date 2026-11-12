//! Cartography for the maps (L4, L5): the pieces of an old-world chart,
//! after the software expedition's map. Each draws in the theme's colors,
//! so the chart reads in every theme.

use iced::widget::canvas::{Frame, LineDash, Path, Stroke, Text};
use iced::{Color, Font, Point, Size, font};

use crate::theme;

/// The serif the chart's names are lettered in.
pub const SERIF: Font = Font::with_name("Baskerville");
const ITALIC: Font = Font {
    style: font::Style::Italic,
    ..SERIF
};

fn pen(width: f32, color: Color) -> Stroke<'static> {
    Stroke::default().with_width(width).with_color(color)
}

fn faded(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// A place's name in spaced capitals, and an italic note under it.
pub fn name(frame: &mut Frame, at: Point, title: &str, note: &str) {
    let spaced: String = title
        .to_uppercase()
        .chars()
        .flat_map(|c| [c, '\u{2009}'])
        .collect();
    frame.fill_text(lettering(spaced, at, &TITLE));
    if !note.is_empty() {
        let below = Point::new(at.x, at.y + 17.0);
        frame.fill_text(lettering(note.into(), below, &NOTE));
    }
}

/// How a piece of lettering is set: its size, face and ink.
struct Face {
    size: f32,
    font: Font,
    ink: fn() -> Color,
}

const TITLE: Face = Face {
    size: 13.0,
    font: SERIF,
    ink: theme::ink,
};
const NOTE: Face = Face {
    size: 12.0,
    font: ITALIC,
    ink: theme::muted,
};
const MARK: Face = Face {
    size: 12.0,
    font: SERIF,
    ink: theme::ink,
};

fn lettering(content: String, at: Point, face: &Face) -> Text {
    Text {
        content,
        position: at,
        color: (face.ink)(),
        size: face.size.into(),
        font: face.font,
        align_x: iced::alignment::Horizontal::Center.into(),
        align_y: iced::alignment::Vertical::Top,
        ..Text::default()
    }
}

/// A legend's words: small italic, starting at `at`.
pub fn caption(frame: &mut Frame, at: Point, words: &str) {
    let mut text = lettering(words.into(), at, &NOTE);
    text.align_x = iced::alignment::Horizontal::Left.into();
    text.color = theme::ink();
    frame.fill_text(text);
}

/// The chart's frame: a double rule with a band of ticks between.
pub fn border(frame: &mut Frame, size: Size) {
    let outer = Path::rectangle(
        Point::new(3.0, 3.0),
        Size::new(size.width - 6.0, size.height - 6.0),
    );
    let inner = Path::rectangle(
        Point::new(11.0, 11.0),
        Size::new(size.width - 22.0, size.height - 22.0),
    );
    frame.stroke(&outer, pen(2.0, theme::ink()));
    frame.stroke(&inner, pen(1.0, theme::ink()));
    let ticks = Stroke {
        line_dash: LineDash {
            segments: &[8.0, 8.0],
            offset: 0,
        },
        ..pen(4.0, theme::ink())
    };
    let band = Path::rectangle(
        Point::new(7.0, 7.0),
        Size::new(size.width - 14.0, size.height - 14.0),
    );
    frame.stroke(&band, ticks);
}

/// Rhumb lines from a point, faint, as a chart has them.
pub fn rhumbs(frame: &mut Frame, from: Point, size: Size) {
    let line = pen(1.0, faded(theme::line(), 0.6));
    for k in 0..12 {
        let angle = k as f32 * std::f32::consts::PI / 6.0;
        let far = size.width.max(size.height);
        let to = Point::new(from.x + angle.cos() * far, from.y + angle.sin() * far);
        frame.stroke(&Path::line(from, to), line);
    }
}

/// The coast along the left edge, with the sea beyond it.
pub fn coast(frame: &mut Frame, x: f32, size: Size) {
    let shore = Path::new(|p| {
        p.move_to(Point::new(14.0, 14.0));
        p.line_to(Point::new(x, 14.0));
        p.bezier_curve_to(
            Point::new(x - 18.0, size.height * 0.35),
            Point::new(x + 14.0, size.height * 0.65),
            Point::new(x - 6.0, size.height - 14.0),
        );
        p.line_to(Point::new(14.0, size.height - 14.0));
        p.close();
    });
    frame.fill(&shore, faded(theme::raised(), 0.9));
    frame.stroke(&shore, pen(1.5, theme::muted()));
    let wave = pen(1.0, theme::muted());
    for row in 0..((size.height / 26.0) as usize) {
        let y = 30.0 + row as f32 * 26.0;
        let start = 20.0 + (row % 2) as f32 * 9.0;
        let mut px = start;
        while px < x - 22.0 {
            let w = Path::new(|p| {
                p.move_to(Point::new(px, y));
                p.quadratic_curve_to(Point::new(px + 4.0, y - 3.0), Point::new(px + 8.0, y));
            });
            frame.stroke(&w, wave);
            px += 18.0;
        }
    }
}

/// A mountain: shaded on its right flank, a snowcap at its top.
pub fn mountain(frame: &mut Frame, base: Point, height: f32, faint: bool) {
    let half = height * 0.62;
    let top = Point::new(base.x, base.y - height);
    let body = Path::new(|p| {
        p.move_to(Point::new(base.x - half, base.y));
        p.line_to(top);
        p.line_to(Point::new(base.x + half, base.y));
        p.close();
    });
    let alpha = if faint { 0.45 } else { 1.0 };
    frame.fill(&body, faded(theme::raised(), alpha));
    let hatch = pen(1.0, faded(theme::muted(), 0.6 * alpha));
    let mut t = 0.15;
    while t < 0.95 {
        let from = Point::new(top.x + 2.0, top.y + height * t);
        let to = Point::new(top.x + half * t, top.y + height * t * 0.98 + 6.0);
        frame.stroke(&Path::line(from, to), hatch);
        t += 0.09;
    }
    let cap = Path::new(|p| {
        p.move_to(top);
        p.line_to(Point::new(top.x - half * 0.22, top.y + height * 0.22));
        p.line_to(Point::new(top.x, top.y + height * 0.16));
        p.line_to(Point::new(top.x + half * 0.22, top.y + height * 0.22));
        p.close();
    });
    frame.fill(&cap, theme::panel_color());
    frame.stroke(&body, pen(1.5, faded(theme::ink(), alpha)));
}

/// A walled checkpoint: two towers and a gate.
pub fn castle(frame: &mut Frame, base: Point, faint: bool) {
    let alpha = if faint { 0.45 } else { 1.0 };
    let ink = pen(1.5, faded(theme::ink(), alpha));
    for dx in [-14.0, 6.0] {
        let tower = Path::rectangle(Point::new(base.x + dx, base.y - 30.0), Size::new(9.0, 30.0));
        frame.fill(&tower, faded(theme::raised(), alpha));
        frame.stroke(&tower, ink);
        for k in 0..2 {
            let merlon = Path::rectangle(
                Point::new(base.x + dx + k as f32 * 5.0, base.y - 34.0),
                Size::new(3.0, 4.0),
            );
            frame.fill(&merlon, faded(theme::ink(), alpha));
        }
    }
    let wall = Path::rectangle(
        Point::new(base.x - 6.0, base.y - 20.0),
        Size::new(12.0, 20.0),
    );
    frame.fill(&wall, faded(theme::raised(), alpha));
    frame.stroke(&wall, ink);
    let gate = Path::rectangle(Point::new(base.x - 3.0, base.y - 9.0), Size::new(6.0, 9.0));
    frame.fill(&gate, faded(theme::ink(), alpha));
}

/// The outpost: two houses with red roofs and a flag on a tower.
pub fn outpost(frame: &mut Frame, base: Point) {
    for (dx, w) in [(-26.0, 16.0), (-8.0, 14.0)] {
        let house = Path::rectangle(Point::new(base.x + dx, base.y - 12.0), Size::new(w, 12.0));
        frame.fill(&house, theme::panel_color());
        frame.stroke(&house, pen(1.2, theme::ink()));
        let roof = Path::new(|p| {
            p.move_to(Point::new(base.x + dx - 2.0, base.y - 12.0));
            p.line_to(Point::new(base.x + dx + w / 2.0, base.y - 21.0));
            p.line_to(Point::new(base.x + dx + w + 2.0, base.y - 12.0));
            p.close();
        });
        frame.fill(&roof, theme::bad());
    }
    let tower = Path::rectangle(
        Point::new(base.x + 10.0, base.y - 24.0),
        Size::new(7.0, 24.0),
    );
    frame.fill(&tower, theme::panel_color());
    frame.stroke(&tower, pen(1.2, theme::ink()));
    flag(
        frame,
        Point::new(base.x + 13.5, base.y - 24.0),
        theme::bad(),
    );
}

/// Two tents, for base camp.
pub fn tents(frame: &mut Frame, base: Point) {
    for dx in [-10.0, 6.0] {
        let tent = Path::new(|p| {
            p.move_to(Point::new(base.x + dx - 9.0, base.y));
            p.line_to(Point::new(base.x + dx, base.y - 14.0));
            p.line_to(Point::new(base.x + dx + 9.0, base.y));
            p.close();
        });
        frame.fill(&tent, theme::panel_color());
        frame.stroke(&tent, pen(1.2, theme::ink()));
    }
}

/// A pennant on a pole whose foot is at `foot`.
pub fn flag(frame: &mut Frame, foot: Point, color: Color) {
    let top = Point::new(foot.x, foot.y - 16.0);
    frame.stroke(&Path::line(foot, top), pen(1.5, theme::ink()));
    let cloth = Path::new(|p| {
        p.move_to(top);
        p.line_to(Point::new(top.x + 13.0, top.y + 3.0));
        p.line_to(Point::new(top.x + 8.0, top.y + 5.0));
        p.line_to(Point::new(top.x + 13.0, top.y + 8.0));
        p.line_to(Point::new(top.x, top.y + 8.0));
        p.close();
    });
    frame.fill(&cloth, color);
}

/// A small tree.
pub fn tree(frame: &mut Frame, at: Point) {
    frame.stroke(
        &Path::line(at, Point::new(at.x, at.y - 6.0)),
        pen(1.2, theme::ink()),
    );
    let crown = Path::circle(Point::new(at.x, at.y - 9.0), 4.0);
    frame.fill(&crown, faded(theme::muted(), 0.5));
    frame.stroke(&crown, pen(1.0, theme::ink()));
}

/// A low hill.
pub fn hill(frame: &mut Frame, at: Point, w: f32) {
    let arc = Path::new(|p| {
        p.move_to(Point::new(at.x - w / 2.0, at.y));
        p.quadratic_curve_to(
            Point::new(at.x, at.y - w * 0.45),
            Point::new(at.x + w / 2.0, at.y),
        );
    });
    frame.fill(&arc, faded(theme::raised(), 0.8));
    frame.stroke(&arc, pen(1.2, theme::muted()));
}

/// The marsh: a low pool with reeds, crossed out when a trail ended there.
pub fn marsh(frame: &mut Frame, at: Point, reached: bool) {
    let oval = Path::new(|p| {
        p.move_to(Point::new(at.x - 34.0, at.y));
        p.bezier_curve_to(
            Point::new(at.x - 30.0, at.y - 14.0),
            Point::new(at.x + 30.0, at.y - 14.0),
            Point::new(at.x + 34.0, at.y),
        );
        p.bezier_curve_to(
            Point::new(at.x + 30.0, at.y + 14.0),
            Point::new(at.x - 30.0, at.y + 14.0),
            Point::new(at.x - 34.0, at.y),
        );
    });
    frame.fill(&oval, faded(theme::muted(), 0.25));
    let dots = Stroke {
        line_dash: LineDash {
            segments: &[2.0, 3.0],
            offset: 0,
        },
        ..pen(1.0, theme::muted())
    };
    frame.stroke(&oval, dots);
    for dx in [-18.0, -6.0, 8.0, 20.0] {
        let reed = Path::line(
            Point::new(at.x + dx, at.y + 4.0),
            Point::new(at.x + dx + 2.0, at.y - 7.0),
        );
        frame.stroke(&reed, pen(1.0, theme::ink()));
    }
    if reached {
        cross(frame, at, theme::bad());
    }
}

/// An X where a trail ended.
pub fn cross(frame: &mut Frame, at: Point, color: Color) {
    let red = pen(3.0, color);
    frame.stroke(
        &Path::line(
            Point::new(at.x - 8.0, at.y - 8.0),
            Point::new(at.x + 8.0, at.y + 8.0),
        ),
        red,
    );
    frame.stroke(
        &Path::line(
            Point::new(at.x + 8.0, at.y - 8.0),
            Point::new(at.x - 8.0, at.y + 8.0),
        ),
        red,
    );
}

/// A compass rose: eight points around a ring.
pub fn compass(frame: &mut Frame, at: Point, r: f32) {
    frame.stroke(&Path::circle(at, r * 0.55), pen(1.0, theme::ink()));
    for k in 0..8 {
        let angle = k as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
        let len = if k % 2 == 0 { r } else { r * 0.6 };
        let tip = Point::new(at.x + angle.cos() * len, at.y + angle.sin() * len);
        let side = angle + std::f32::consts::FRAC_PI_2;
        let w = r * 0.12;
        let a = Point::new(at.x + side.cos() * w, at.y + side.sin() * w);
        let b = Point::new(at.x - side.cos() * w, at.y - side.sin() * w);
        let point = Path::new(|p| {
            p.move_to(a);
            p.line_to(tip);
            p.line_to(b);
            p.close();
        });
        let fill = if k % 2 == 0 {
            theme::ink()
        } else {
            theme::muted()
        };
        frame.fill(&point, fill);
    }
    frame.fill(&Path::circle(at, 2.5), theme::bad());
    let north = Point::new(at.x, at.y - r - 16.0);
    frame.fill_text(lettering("N".into(), north, &MARK));
}

/// The atlas: a domed hall with columns, where the field notes are kept.
pub fn atlas(frame: &mut Frame, base: Point, lit: bool) {
    let w = if lit { 40.0 } else { 34.0 };
    let ink = pen(1.3, theme::ink());
    let step = Path::rectangle(
        Point::new(base.x - w / 2.0 - 4.0, base.y - 4.0),
        Size::new(w + 8.0, 4.0),
    );
    frame.fill(&step, theme::raised());
    frame.stroke(&step, ink);
    let body = Path::rectangle(
        Point::new(base.x - w / 2.0, base.y - 22.0),
        Size::new(w, 18.0),
    );
    frame.fill(&body, theme::panel_color());
    frame.stroke(&body, ink);
    for k in 0..5 {
        let x = base.x - w / 2.0 + 4.0 + k as f32 * (w - 8.0) / 4.0;
        frame.stroke(
            &Path::line(Point::new(x, base.y - 21.0), Point::new(x, base.y - 5.0)),
            ink,
        );
    }
    let roof = Path::new(|p| {
        p.move_to(Point::new(base.x - w / 2.0 - 3.0, base.y - 22.0));
        p.line_to(Point::new(base.x, base.y - 30.0));
        p.line_to(Point::new(base.x + w / 2.0 + 3.0, base.y - 22.0));
        p.close();
    });
    frame.fill(&roof, theme::raised());
    frame.stroke(&roof, ink);
    let dome = Path::new(|p| {
        p.move_to(Point::new(base.x - 9.0, base.y - 30.0));
        p.quadratic_curve_to(
            Point::new(base.x, base.y - 46.0),
            Point::new(base.x + 9.0, base.y - 30.0),
        );
    });
    frame.fill(&dome, theme::raised());
    frame.stroke(&dome, ink);
}

/// The way home: a dotted line, as the chart draws the return.
pub fn way_home(frame: &mut Frame, from: Point, to: Point) {
    let dots = Stroke {
        line_dash: LineDash {
            segments: &[2.0, 6.0],
            offset: 0,
        },
        ..pen(2.5, theme::ink())
    };
    let path = Path::new(|p| {
        p.move_to(from);
        p.bezier_curve_to(
            Point::new(from.x - 80.0, from.y + 90.0),
            Point::new(to.x + 160.0, to.y + 40.0),
            to,
        );
    });
    frame.stroke(&path, dots);
}
