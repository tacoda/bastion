//! Spike N14 on iced. `N14_MODE=jobs|canvas|diff` picks the screen; each
//! measures itself, prints `RESULT` lines and quits. `N14_STAY=1` keeps the
//! window open for a screenshot.

#[path = "../../n14-data.rs"]
mod data;

use std::time::{Duration, Instant};

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{column, container, row, scrollable, text, Column};
use iced::{mouse, window, Background, Border, Color, Element, Fill, Point, Rectangle, Renderer, Size, Subscription, Task, Theme};

const FRAMES: usize = 300;
const PUSHES: usize = 30;

fn hex(h: &str) -> Color {
    let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
    Color::from_rgb8(v(1), v(3), v(5))
}

fn state_color(state: &str) -> Color {
    hex(match state {
        "needs_you" => data::WARN,
        "landed" => data::OK,
        "failed" => data::BAD,
        _ => data::LINK,
    })
}

#[derive(Debug, Clone)]
enum Message {
    Frame(Instant),
    Push,
}

struct App {
    mode: String,
    jobs: Vec<data::Job>,
    steps: Vec<data::Step>,
    links: Vec<(usize, usize)>,
    diff: Vec<(char, String)>,
    offset: f32,
    frame: usize,
    painted: bool,
    last: Option<Instant>,
    intervals: Vec<f64>,
    pushed: Option<Instant>,
    repaints: Vec<f64>,
    pushes: usize,
    done: bool,
    builds: std::cell::RefCell<Vec<f64>>,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let (steps, links) = data::workflow();
        let app = Self {
            mode: std::env::var("N14_MODE").unwrap_or_else(|_| "jobs".into()),
            jobs: data::jobs(),
            steps,
            links,
            diff: data::diff(),
            offset: 0.0,
            frame: 0,
            painted: false,
            last: None,
            intervals: vec![],
            pushed: None,
            repaints: vec![],
            pushes: 0,
            done: false,
            builds: Default::default(),
        };
        (app, Task::none())
    }

    fn finish(&mut self) -> Task<Message> {
        if self.done {
            return Task::none();
        }
        self.done = true;
        if !self.intervals.is_empty() {
            println!("{}", data::summary("frame_ms", self.intervals.clone()));
        }
        if !self.repaints.is_empty() {
            println!("{}", data::summary("push_to_paint_ms", self.repaints.clone()));
        }
        println!("{}", data::summary("view_build_ms", self.builds.borrow().clone()));
        if std::env::var("N14_STAY").is_ok() { Task::none() } else { iced::exit() }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Frame(now) => self.on_frame(now),
            Message::Push => self.on_push(),
        }
    }

    /// Note the first paint and a push's repaint. A frame stamped before
    /// the push was already under way; the push shows on the first frame
    /// that starts after it.
    fn note_paint(&mut self, now: Instant) {
        if !self.painted {
            self.painted = true;
            println!("FIRST_PAINT {}", data::now_ms());
        }
        if let Some(t) = self.pushed.filter(|t| now > *t) {
            self.repaints.push(t.elapsed().as_secs_f64() * 1000.0);
            self.pushed = None;
        }
    }

    /// Move the canvas or the diff one step, and note the frame's interval.
    fn step(&mut self, now: Instant) {
        let last = self.last.replace(now);
        if let Some(last) = last {
            self.intervals.push((now - last).as_secs_f64() * 1000.0);
        }
        self.frame += 1;
        self.offset = 300.0 * (self.frame as f32 / 30.0).sin().abs();
    }

    fn on_frame(&mut self, now: Instant) -> Task<Message> {
        self.note_paint(now);
        let animated = self.mode != "jobs" && !self.done;
        if !animated {
            return Task::none();
        }
        self.step(now);
        if self.frame > FRAMES {
            return self.finish();
        }
        if self.mode != "diff" {
            return Task::none();
        }
        let y = (self.frame as f32 * 40.0) % 40_000.0;
        iced::widget::operation::scroll_to("diff", scrollable::AbsoluteOffset { x: 0.0, y })
    }

    fn on_push(&mut self) -> Task<Message> {
        if !self.painted {
            return Task::none();
        }
        if self.pushes == PUSHES {
            return self.finish();
        }
        let job = &mut self.jobs[(self.pushes * 37) % 200];
        job.state = data::next_state(job.state);
        self.pushed = Some(Instant::now());
        self.pushes += 1;
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let frames = window::frames().map(Message::Frame);
        if self.mode == "jobs" {
            let pushes = iced::time::every(Duration::from_millis(200)).map(|_| Message::Push);
            Subscription::batch([frames, pushes])
        } else {
            frames
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let started = Instant::now();
        let built = self.build();
        self.builds.borrow_mut().push(started.elapsed().as_secs_f64() * 1000.0);
        built
    }

    fn build(&self) -> Element<'_, Message> {
        let (title, body): (&str, Element<'_, Message>) = match self.mode.as_str() {
            "canvas" => ("Workflows", Canvas::new(self).width(1100).height(520).into()),
            "diff" => ("calc.py", self.diff_view()),
            _ => ("Jobs", self.jobs_view()),
        };
        let page = column![text(title).size(24).color(hex(data::INK)), body]
            .spacing(12)
            .padding([16, 24]);
        let root = row![sidebar(if self.mode == "canvas" { "Workflows" } else if self.mode == "diff" { "Diff" } else { "Jobs" }), page];
        container(root)
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(hex(data::BG))),
                text_color: Some(hex(data::INK)),
                ..Default::default()
            })
            .into()
    }

    fn jobs_view(&self) -> Element<'_, Message> {
        let rows = self.jobs.iter().map(|j| {
            container(
                row![
                    text(&j.id).font(iced::Font::MONOSPACE).color(hex(data::LINK)),
                    text(&j.request).width(Fill),
                    text(j.stage).color(hex(data::MUTED)),
                    text(j.state).color(state_color(j.state)).width(90),
                ]
                .spacing(12),
            )
            .padding([12, 16])
            .width(Fill)
            .style(panel)
            .into()
        });
        scrollable(Column::with_children(rows).spacing(8)).height(Fill).into()
    }

    fn diff_view(&self) -> Element<'_, Message> {
        let lines = self.diff.iter().map(|(mark, line)| {
            let color = match mark {
                '+' => hex(data::OK),
                '-' => hex(data::BAD),
                _ => hex(data::INK),
            };
            text(format!("{mark} {line}")).font(iced::Font::MONOSPACE).size(13).color(color).into()
        });
        let body = container(Column::with_children(lines)).padding(8).width(Fill).style(panel);
        scrollable(body).id("diff").height(Fill).into()
    }
}

fn panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(hex(data::PANEL))),
        border: Border { radius: 12.0.into(), width: 1.0, color: hex(data::LINE) },
        ..Default::default()
    }
}

fn sidebar(active: &str) -> Element<'static, Message> {
    let mut items = vec![text("Bastion").size(18).into()];
    for page in data::PAGES.iter().filter(|p| !p.is_empty()) {
        let on = *page == active;
        items.push(
            container(text(*page))
                .padding([8, 12])
                .width(Fill)
                .style(move |_| container::Style {
                    background: on.then(|| Background::Color(hex(data::PRIMARY))),
                    border: Border { radius: 8.0.into(), ..Default::default() },
                    ..Default::default()
                })
                .into(),
        );
    }
    container(Column::with_children(items).spacing(2))
        .width(200)
        .height(Fill)
        .padding([16, 12])
        .style(|_| container::Style {
            border: Border { width: 1.0, color: hex(data::LINE), ..Default::default() },
            ..Default::default()
        })
        .into()
}

impl canvas::Program<Message> for App {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let at = |i: usize| {
            let s = &self.steps[i];
            Point::new(s.x as f32 + if i == 0 { self.offset } else { 0.0 }, s.y as f32)
        };
        let (w, h) = (data::STEP_W as f32, data::STEP_H as f32);
        for &(a, b) in &self.links {
            let (pa, pb) = (at(a), at(b));
            let (from, to) = (Point::new(pa.x + w, pa.y + h / 2.0), Point::new(pb.x, pb.y + h / 2.0));
            let curve = Path::new(|p| {
                p.move_to(from);
                p.bezier_curve_to(Point::new(from.x + 40.0, from.y), Point::new(to.x - 40.0, to.y), to);
            });
            frame.stroke(&curve, Stroke::default().with_width(2.0).with_color(hex(data::LINK)));
        }
        for (i, s) in self.steps.iter().enumerate() {
            let p = at(i);
            let rect = Path::rounded_rectangle(p, Size::new(w, h), 10.0.into());
            frame.fill(&rect, hex(data::PANEL));
            let edge = if i == 0 { data::PRIMARY } else { data::LINE };
            frame.stroke(&rect, Stroke::default().with_width(2.0).with_color(hex(edge)));
            frame.fill_text(canvas::Text {
                content: s.name.clone(),
                position: Point::new(p.x + 12.0, p.y + 20.0),
                color: hex(data::INK),
                size: 13.0.into(),
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .subscription(App::subscription)
        .title("Bastion")
        .window_size(Size::new(1280.0, 900.0))
        .run()
}
