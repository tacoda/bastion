//! Charts drawn on iced's canvas, with no charting dependency: stacked bars
//! over days, a legend, and number tiles. Hovering a bar names its day and
//! each part of it.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Text};
use iced::widget::{Column, Row, column, container, row, text};
use iced::{Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::theme;

/// The colors the series take, in order, read from the theme in use.
pub const PALETTE: [theme::Tint; 6] = [
    theme::primary_color,
    theme::ok,
    theme::warn,
    theme::bad,
    theme::link_color,
    theme::muted,
];

const HEIGHT: f32 = 180.0;
/// Room under the bars for the day labels, and over them for the hover line.
const FOOT: f32 = 20.0;
const HEAD: f32 = 22.0;

/// Stacked bars: one per column, each split into its series' parts.
pub struct Bars {
    /// Each column's label, left to right.
    pub columns: Vec<String>,
    /// Each series' name and color.
    pub series: Vec<(String, Color)>,
    /// Each column's value per series, in the series' order.
    pub values: Vec<Vec<f64>>,
    /// How a value reads, such as in dollars.
    pub unit: fn(f64) -> String,
}

impl Bars {
    fn top(&self) -> f64 {
        let sums = self.values.iter().map(|v| v.iter().sum::<f64>());
        sums.fold(0.0, f64::max)
    }

    /// The column under `x`, in a chart `width` wide.
    fn column_at(&self, x: f32, width: f32) -> Option<usize> {
        let n = self.columns.len();
        let i = (x / (width / n.max(1) as f32)) as usize;
        (n > 0 && i < n).then_some(i)
    }

    /// What the hover line says about column `i`.
    fn about(&self, i: usize) -> String {
        let parts: Vec<String> = self
            .series
            .iter()
            .zip(&self.values[i])
            .filter(|(_, v)| **v > 0.0)
            .map(|((name, _), v)| format!("{name} {}", (self.unit)(*v)))
            .collect();
        let total = (self.unit)(self.values[i].iter().sum());
        let mut line = format!("{}: {total}", self.columns[i]);
        if !parts.is_empty() {
            line = format!("{line} · {}", parts.join(" · "));
        }
        line
    }
}

fn label(content: String, position: Point, color: Color) -> Text {
    Text {
        content,
        position,
        color,
        size: 11.0.into(),
        font: theme::FONT,
        ..Text::default()
    }
}

impl<M> canvas::Program<M> for Bars {
    type State = ();

    // iced's trait fixes this signature.
    fn update(
        &self,
        _: &mut (),
        event: &canvas::Event,
        _: Rectangle,
        _: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        matches!(
            event,
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. })
        )
        .then(canvas::Action::request_redraw)
    }

    // iced's trait fixes this signature.
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (width, room) = (bounds.width, bounds.height - FOOT - HEAD);
        let n = self.columns.len().max(1) as f32;
        let slot = width / n;
        let top = self.top().max(f64::MIN_POSITIVE);
        let hovered = cursor
            .position_in(bounds)
            .and_then(|p| self.column_at(p.x, width));
        for (i, parts) in self.values.iter().enumerate() {
            let x = i as f32 * slot + slot * 0.15;
            let mut y = HEAD + room;
            for ((_, color), v) in self.series.iter().zip(parts) {
                let h = (v / top) as f32 * room;
                let lit = hovered.is_none_or(|h| h == i);
                let color = Color {
                    a: if lit { 1.0 } else { 0.45 },
                    ..*color
                };
                y -= h;
                let bar = Path::rectangle(Point::new(x, y), Size::new(slot * 0.7, h));
                frame.fill(&bar, color);
            }
        }
        // A day label under every bar that has room for one.
        let every = (56.0 / slot).ceil().max(1.0) as usize;
        for (i, day) in self.columns.iter().enumerate().step_by(every) {
            let at = Point::new(i as f32 * slot + slot * 0.15, HEAD + room + 4.0);
            frame.fill_text(label(short(day), at, theme::muted()));
        }
        let line = match hovered {
            Some(i) => label(self.about(i), Point::new(0.0, 2.0), theme::ink()),
            None => label(
                format!("most in a day: {}", (self.unit)(self.top())),
                Point::new(0.0, 2.0),
                theme::muted(),
            ),
        };
        frame.fill_text(line);
        vec![frame.into_geometry()]
    }
}

/// `2026-11-10` as `11-10`; other labels as they are.
fn short(day: &str) -> String {
    match day.len() == 10 {
        true => day[5..].to_string(),
        false => day.to_string(),
    }
}

/// The bars with their legend, in a card titled `title`.
pub fn view<'a, M: 'a>(title: &str, bars: Bars) -> Element<'a, M> {
    let legend = Row::with_children(bars.series.iter().map(|(name, color)| {
        row![
            container(text(""))
                .width(10)
                .height(10)
                .style(swatch(*color)),
            text(name.clone()).size(12).color(theme::muted())
        ]
        .spacing(6)
        .align_y(iced::Center)
        .into()
    }))
    .spacing(16)
    .wrap();
    let head = row![
        text(title.to_string())
            .size(13)
            .font(theme::BOLD)
            .color(theme::muted())
            .width(Fill),
        legend
    ]
    .align_y(iced::Center);
    let canvas = Canvas::new(bars).width(Fill).height(HEIGHT);
    container(column![head, canvas].spacing(8))
        .padding([12, 16])
        .width(Fill)
        .style(theme::panel)
        .into()
}

fn swatch(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(color.into()),
        border: iced::border::rounded(2),
        ..container::Style::default()
    }
}

/// A number with its name under it, in a small card.
pub fn tile<'a, M: 'a>(value: String, name: &str, color: Color) -> Element<'a, M> {
    container(
        column![
            text(value).size(22).font(theme::BOLD).color(color),
            text(name.to_string()).size(12).color(theme::muted())
        ]
        .spacing(2),
    )
    .padding([12, 16])
    .width(Fill)
    .style(theme::panel)
    .into()
}

/// One measure compared between two arms: its name, each arm's value and
/// how it reads, and whether less is better; none for a measure that is
/// neither, such as refusals, which an enforced rung is meant to raise.
pub struct Pair {
    pub name: String,
    pub a: f64,
    pub b: f64,
    pub shown: (String, String),
    pub less_is_better: Option<bool>,
}

/// How B differs from A, and whether that is better, when the measure says:
/// as a percentage, or as a count when A is zero; none when they are the same.
fn change(p: &Pair) -> Option<(String, Option<bool>)> {
    if p.a == p.b {
        return None;
    }
    let better = p.less_is_better.map(|less| (p.b < p.a) == less);
    let words = match p.a == 0.0 {
        true => format!("B {:+}", p.b - p.a),
        false => format!("B {:+.0}%", (p.b - p.a) / p.a * 100.0),
    };
    Some((words, better))
}

/// A bar `value` long out of `top`, in `color`, then its reading.
fn bar<'a, M: 'a>(value: f64, top: f64, color: Color, shown: String) -> Element<'a, M> {
    let filled = match top > 0.0 {
        true => ((value / top) * 1000.0).round() as u16,
        false => 0,
    };
    // A zero draws no bar at all, so it does not read as a small one.
    let mut track = row![].height(10);
    if filled > 0 {
        let part = iced::Length::FillPortion(filled);
        track = track.push(
            container(text(""))
                .height(10)
                .width(part)
                .style(swatch(color)),
        );
    }
    if filled < 1000 {
        let rest = iced::Length::FillPortion(1000 - filled);
        track = track.push(iced::widget::space::horizontal().width(rest));
    }
    row![
        container(track).width(Fill),
        text(shown).size(12).font(theme::MONO).width(90)
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// Each measure's A and B as two bars, each pair scaled to its larger
/// value, with how B differs from A.
pub fn compare<'a, M: 'a>(title: &str, pairs: Vec<Pair>) -> Element<'a, M> {
    let (a, b) = (PALETTE[0](), PALETTE[1]());
    let rows = pairs.into_iter().map(|p| {
        let top = p.a.max(p.b);
        let note = match change(&p) {
            Some((words, Some(true))) => text(words).size(12).color(theme::ok()),
            Some((words, Some(false))) => text(words).size(12).color(theme::bad()),
            Some((words, None)) => text(words).size(12).color(theme::ink()),
            None => text("same").size(12).color(theme::muted()),
        };
        row![
            text(p.name).size(13).width(110),
            column![bar(p.a, top, a, p.shown.0), bar(p.b, top, b, p.shown.1)]
                .spacing(3)
                .width(Fill),
            container(note).width(70)
        ]
        .spacing(12)
        .align_y(iced::Center)
        .into()
    });
    let legend = row![
        container(text("")).width(10).height(10).style(swatch(a)),
        text("A").size(12).color(theme::muted()),
        container(text("")).width(10).height(10).style(swatch(b)),
        text("B").size(12).color(theme::muted())
    ]
    .spacing(6)
    .align_y(iced::Center);
    let head = row![
        text(title.to_string())
            .size(13)
            .font(theme::BOLD)
            .color(theme::muted())
            .width(Fill),
        legend
    ];
    container(column![head, Column::with_children(rows).spacing(10)].spacing(10))
        .padding([12, 16])
        .width(Fill)
        .style(theme::panel)
        .into()
}

/// A day, `YYYY-MM-DD` in UTC, from seconds since the epoch.
pub fn day(secs: u64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The last `n` days up to the day of `now`, oldest first.
pub fn last_days(now: u64, n: u64) -> Vec<String> {
    (0..n).rev().map(|back| day(now - back * 86_400)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_reads_in_utc_and_the_last_days_end_today() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(951_782_400), "2000-02-29");
        assert_eq!(
            last_days(951_782_400, 3),
            ["2000-02-27", "2000-02-28", "2000-02-29"]
        );
    }

    #[test]
    fn b_is_compared_to_a_and_marked_better_by_the_measure() {
        let pair = |a, b, less_is_better| Pair {
            name: "x".into(),
            a,
            b,
            shown: (String::new(), String::new()),
            less_is_better,
        };
        let (less, more) = (Some(true), Some(false));
        assert_eq!(
            change(&pair(10.0, 8.0, less)),
            Some(("B -20%".into(), Some(true)))
        );
        assert_eq!(
            change(&pair(9.0, 10.0, more)),
            Some(("B +11%".into(), Some(true)))
        );
        assert_eq!(
            change(&pair(9.0, 10.0, less)),
            Some(("B +11%".into(), Some(false)))
        );
        assert_eq!(change(&pair(3.0, 3.0, less)), None);
        assert_eq!(
            change(&pair(0.0, 5.0, None)),
            Some(("B +5".into(), None)),
            "from zero, a count, and neither better nor worse"
        );
    }

    #[test]
    fn a_hovered_bar_names_its_day_and_each_part() {
        let bars = Bars {
            columns: vec!["2026-11-10".into(), "2026-11-11".into()],
            series: vec![("local".into(), PALETTE[0]()), ("two".into(), PALETTE[1]())],
            values: vec![vec![2.0, 0.0], vec![1.0, 3.0]],
            unit: |v| format!("{v}"),
        };
        assert_eq!(bars.top(), 4.0);
        assert_eq!(bars.column_at(150.0, 200.0), Some(1));
        assert_eq!(bars.column_at(250.0, 200.0), None);
        assert_eq!(bars.about(0), "2026-11-10: 2 · local 2");
        assert_eq!(bars.about(1), "2026-11-11: 4 · local 1 · two 3");
        let empty = Bars {
            values: vec![vec![0.0, 0.0], vec![0.0, 0.0]],
            ..bars
        };
        assert_eq!(
            empty.about(0),
            "2026-11-10: 0",
            "an empty day names no parts"
        );
        assert_eq!(short("2026-11-10"), "11-10");
    }
}
