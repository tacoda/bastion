//! The Engine page (PLAN.md A1, A2): the link, the stack with Start and Stop,
//! Doctor, then the console: containers with stop, start and logs,
//! functions and trigger types, and queues.

use bastion_protocol::{
    Check, CheckState, Console, Container, DoctorReport, EngineStatus, Link, LogCursor, LogLine,
    LogPage, Queue, Registered, StackState, StackStatus, StartRefused, command,
};
use iced::widget::{Column, button, column, container, row, scrollable, text, text_input};
use iced::{Color, Element, Fill, Task};
use serde_json::{Value, json};

use super::{Ctx, scrolled, section};
use crate::app::Message;
use crate::client::Ask;
use crate::theme;

/// The most log lines the panel keeps.
const LOG_LINES: usize = 500;
/// The most functions or trigger types a filter shows.
const SHOWN: usize = 100;

#[derive(Default)]
pub struct State {
    link: Option<EngineStatus>,
    stack: Option<StackStatus>,
    note: String,
    busy: bool,
    report: Option<DoctorReport>,
    console: Option<Console>,
    error: String,
    toggling: Option<String>,
    watching: Option<String>,
    lines: Vec<LogLine>,
    filter: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Link(Result<EngineStatus, String>),
    Stack(Result<StackStatus, String>),
    Doctor(Result<DoctorReport, String>),
    Console(Result<Console, String>),
    Start,
    Stop,
    Started(Result<StackStatus, String>),
    RunDoctor,
    Toggle(String, bool),
    Toggled(Result<Value, String>),
    Watch(String),
    Logs(String, Result<LogPage, String>),
    CloseLogs,
    Filter(String),
}

fn task<T>(ctx: &Ctx, ask: Ask, then: fn(Result<T, String>) -> Msg) -> Task<Message>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    ctx.task(&ctx.current, ask, then).map(Message::Engine)
}

/// Read the link, the stack, Doctor and the console.
pub fn load(ctx: &Ctx) -> Task<Message> {
    Task::batch([
        task(ctx, Ask::bare(command::ENGINE_STATUS), Msg::Link),
        task(ctx, Ask::bare(command::STACK_STATUS), Msg::Stack),
        task(ctx, Ask::bare(command::DOCTOR), Msg::Doctor),
        task(ctx, Ask::bare(command::CONSOLE), Msg::Console),
    ])
}

fn describe_link(status: &EngineStatus) -> String {
    match (status.link, status.functions) {
        (Link::Up, Some(n)) => format!("Up at {}, {n} functions.", status.url),
        (Link::Up, None) => format!("Up at {}.", status.url),
        (Link::Connecting, _) => format!("Connecting to {}.", status.url),
        (Link::Reconnecting, _) => format!("Reconnecting to {}.", status.url),
        (Link::Down, _) => format!("Down: {} does not answer.", status.url),
    }
}

fn describe_stack(status: &StackStatus) -> String {
    match &status.state {
        StackState::Stopped => "Stopped.".into(),
        StackState::Running => format!("Running from {}.", status.compose_file),
        StackState::Taken { pid, compose_file } => {
            format!(
                "Port taken by engine pid {pid} ({}).",
                compose_file.as_deref().unwrap_or("not a compose project")
            )
        }
    }
}

/// Why Start was refused, read from the server's typed refusal.
fn describe_refusal(why: &str) -> (String, Option<DoctorReport>) {
    match serde_json::from_str::<StartRefused>(why) {
        Ok(StartRefused::Doctor { report }) => (
            "Doctor found a required check that failed. See below.".into(),
            Some(report),
        ),
        Ok(StartRefused::Taken { pid, compose_file }) => (
            format!(
                "Not started: engine pid {pid} already holds the port ({}).",
                compose_file.as_deref().unwrap_or("not a compose project")
            ),
            None,
        ),
        Ok(StartRefused::Failed { message }) => (format!("Not started: {message}"), None),
        Err(_) => (why.to_string(), None),
    }
}

/// Read the next page of the watched container's logs, waiting for lines
/// after `cursor`.
fn read_logs(ctx: &Ctx, name: String, cursor: Option<LogCursor>) -> Task<Message> {
    let ask = Ask {
        cmd: command::CONTAINER_LOGS,
        args: json!({ "request": { "container": name, "cursor": cursor } }),
    };
    let read = ctx.clone().get::<LogPage>(ctx.current.clone(), ask);
    Task::perform(read, move |page| {
        Message::Engine(Msg::Logs(name.clone(), page))
    })
}

fn keep<T>(slot: &mut Option<T>, error: &mut String, answer: Result<T, String>) {
    match answer {
        Ok(v) => *slot = Some(v),
        Err(e) => *error = e,
    }
}

fn on_started(state: &mut State, answer: Result<StackStatus, String>) {
    state.busy = false;
    match answer {
        Ok(s) => {
            state.note.clear();
            state.stack = Some(s);
        }
        Err(why) => {
            let (note, report) = describe_refusal(&why);
            state.note = note;
            state.report = report.or(state.report.take());
        }
    }
}

fn on_logs(
    state: &mut State,
    ctx: &Ctx,
    name: String,
    page: Result<LogPage, String>,
) -> Task<Message> {
    if state.watching.as_deref() != Some(name.as_str()) {
        return Task::none();
    }
    match page {
        Ok(page) => {
            state.lines.extend(page.lines);
            let over = state.lines.len().saturating_sub(LOG_LINES);
            state.lines.drain(..over);
            read_logs(ctx, name, page.cursor)
        }
        Err(e) => {
            state.error = e;
            Task::none()
        }
    }
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    let e = &mut state.error;
    match msg {
        Msg::Link(a) => keep(&mut state.link, e, a),
        Msg::Stack(a) => keep(&mut state.stack, e, a),
        Msg::Doctor(a) => keep(&mut state.report, e, a),
        Msg::Console(a) => keep(&mut state.console, e, a),
        Msg::Start => {
            (state.busy, state.note) = (true, "Starting: Doctor first, then the stack.".into());
            return task(ctx, Ask::bare(command::STACK_START), Msg::Started);
        }
        Msg::Stop => {
            state.busy = true;
            return task(ctx, Ask::bare(command::STACK_STOP), Msg::Started);
        }
        Msg::Started(answer) => on_started(state, answer),
        Msg::RunDoctor => return task(ctx, Ask::bare(command::DOCTOR), Msg::Doctor),
        Msg::Toggle(name, start) => {
            let cmd = if start {
                command::CONTAINER_START
            } else {
                command::CONTAINER_STOP
            };
            let ask = Ask {
                cmd,
                args: json!({ "name": name }),
            };
            state.toggling = Some(name);
            return task(ctx, ask, Msg::Toggled);
        }
        Msg::Toggled(answer) => {
            state.toggling = None;
            if let Err(why) = answer {
                state.error = why;
            }
            return task(ctx, Ask::bare(command::CONSOLE), Msg::Console);
        }
        Msg::Watch(name) => {
            (state.watching, state.lines) = (Some(name.clone()), vec![]);
            return read_logs(ctx, name, None);
        }
        Msg::Logs(name, page) => return on_logs(state, ctx, name, page),
        Msg::CloseLogs => state.watching = None,
        Msg::Filter(f) => state.filter = f,
    }
    Task::none()
}

fn heading<'a>(title: &'a str) -> Element<'a, Message> {
    text(title)
        .size(13)
        .font(theme::BOLD)
        .color(theme::MUTED)
        .into()
}

fn panel<'a>(body: Column<'a, Message>) -> Element<'a, Message> {
    container(body.spacing(8))
        .padding(16)
        .width(Fill)
        .style(theme::panel)
        .into()
}

fn stack_panel(state: &State) -> Element<'_, Message> {
    let running = matches!(
        state.stack.as_ref().map(|s| &s.state),
        Some(StackState::Running)
    );
    let described = state
        .stack
        .as_ref()
        .map_or_else(|| "…".to_string(), describe_stack);
    let start = button("Start").padding([8, 12]).style(theme::primary);
    let stop = button("Stop").padding([8, 12]).style(theme::secondary);
    let start =
        start.on_press_maybe((!state.busy && !running).then_some(Message::Engine(Msg::Start)));
    let stop = stop.on_press_maybe((!state.busy && running).then_some(Message::Engine(Msg::Stop)));
    panel(column![
        heading("Stack"),
        text(described),
        text(&state.note).color(theme::MUTED),
        row![start, stop].spacing(8)
    ])
}

fn check_row<'a>(check: &Check) -> Element<'a, Message> {
    let (mark, color) = match check.state {
        CheckState::Pass => ("Pass", theme::OK),
        CheckState::Warn => ("Warn", theme::WARN),
        CheckState::Fail => ("Fail", theme::BAD),
        CheckState::Skipped => ("Skipped", theme::MUTED),
    };
    let mut about = column![
        row![
            text(format!("{}: ", check.name)).font(theme::BOLD),
            text(check.detail.clone())
        ]
        .wrap()
    ];
    if let Some(fix) = &check.fix {
        about = about.push(text(fix.clone()).color(theme::MUTED));
    }
    row![
        text(mark).font(theme::BOLD).color(color).width(72),
        about.width(Fill)
    ]
    .spacing(12)
    .into()
}

fn doctor(state: &State) -> Element<'_, Message> {
    let again = button(text("Run again").size(13))
        .padding([4, 12])
        .style(theme::secondary)
        .on_press(Message::Engine(Msg::RunDoctor));
    let checks = state.report.iter().flat_map(|r| &r.checks).map(check_row);
    panel(column![
        row![container(heading("Doctor")).width(Fill), again].align_y(iced::Center),
        Column::with_children(checks).spacing(8)
    ])
}

fn state_color(state: &str) -> Color {
    match state {
        "ready" => theme::OK,
        "failed" => theme::BAD,
        _ => theme::WARN,
    }
}

fn container_row<'a>(state: &State, c: &Container) -> Element<'a, Message> {
    let ready = c.state == "ready";
    let mut status = column![text(c.state.clone()).color(state_color(&c.state))];
    if let Some(e) = &c.last_error {
        status = status.push(text(e.clone()).size(12).color(theme::MUTED));
    }
    let small = |label: &'static str, msg: Option<Message>| {
        button(text(label).size(13))
            .padding([2, 8])
            .style(theme::secondary)
            .on_press_maybe(msg)
    };
    let mut actions = row![].spacing(6);
    // compose owns only a running process, so any stopped container may be started.
    if !ready || c.owned {
        let free = state.toggling.as_deref() != Some(c.name.as_str());
        let label = if ready { "Stop" } else { "Start" };
        actions = actions.push(small(
            label,
            free.then(|| Message::Engine(Msg::Toggle(c.name.clone(), !ready))),
        ));
    }
    actions = actions.push(small(
        "Logs",
        Some(Message::Engine(Msg::Watch(c.name.clone()))),
    ));
    let version = text(c.version.clone().unwrap_or_default())
        .color(theme::MUTED)
        .width(120);
    row![
        text(c.name.clone()).width(Fill),
        version,
        status.width(Fill),
        actions
    ]
    .spacing(12)
    .padding([4, 0])
    .into()
}

fn containers(state: &State) -> Element<'_, Message> {
    let head = row![
        text("Name").width(Fill),
        text("Version").width(120),
        text("State").width(Fill),
        text("").width(120)
    ];
    let rows = state
        .console
        .iter()
        .flat_map(|c| &c.containers)
        .map(|c| container_row(state, c));
    let table = column![head.spacing(12), theme::rule()].extend(rows);
    panel(column![
        heading("Containers"),
        text(&state.error).color(theme::BAD),
        table
    ])
}

fn logs(state: &State) -> Option<Element<'_, Message>> {
    let name = state.watching.as_ref()?;
    let close = button(text("Close").size(13))
        .padding([2, 8])
        .style(theme::secondary)
        .on_press(Message::Engine(Msg::CloseLogs));
    let lines = state.lines.iter().map(|l| {
        let color = if l.stream == "stderr" {
            theme::WARN
        } else {
            theme::INK
        };
        text(l.message.clone())
            .size(12)
            .font(theme::MONO)
            .color(color)
            .into()
    });
    let body = container(scrollable(Column::with_children(lines).padding(12)).anchor_bottom())
        .max_height(320)
        .width(Fill)
        .style(theme::code);
    Some(panel(column![
        row![
            container(heading("Logs")).width(Fill),
            text(name.clone()).size(13),
            close
        ]
        .spacing(8),
        body
    ]))
}

/// The rows the filter lets through, at most `SHOWN`, and how many matched.
fn matching<'a>(rows: &'a [Registered], filter: &str) -> (Vec<&'a Registered>, usize) {
    let needle = filter.to_lowercase();
    let all: Vec<&Registered> = rows
        .iter()
        .filter(|r| needle.is_empty() || r.id.to_lowercase().contains(&needle))
        .collect();
    let total = all.len();
    (all.into_iter().take(SHOWN).collect(), total)
}

fn registered_row<'a>(r: &Registered) -> Element<'a, Message> {
    let line = row![
        text(r.id.clone()).font(theme::MONO).color(theme::LINK),
        text(format!("  {} · {}", r.worker, r.namespace)).color(theme::MUTED)
    ];
    column![
        line,
        text(r.description.clone()).size(12).color(theme::MUTED)
    ]
    .into()
}

fn registered(state: &State) -> Element<'_, Message> {
    let filter = text_input("Filter", &state.filter)
        .on_input(|f| Message::Engine(Msg::Filter(f)))
        .style(theme::field)
        .width(240);
    let mut body = column![
        row![
            container(heading("Functions and trigger types")).width(Fill),
            filter
        ]
        .align_y(iced::Center)
    ];
    if let Some(c) = &state.console {
        for (title, rows) in [
            ("Functions", &c.functions),
            ("Trigger types", &c.trigger_types),
        ] {
            let (shown, total) = matching(rows, &state.filter);
            body = body.push(text(format!("{title}: {total}")).size(13).font(theme::BOLD));
            body =
                body.push(Column::with_children(shown.into_iter().map(registered_row)).spacing(4));
        }
    }
    panel(body)
}

fn queue_row<'a>(q: &Queue) -> Element<'a, Message> {
    let alarm = |n: u64| {
        text(n.to_string())
            .color(if n > 0 { theme::BAD } else { theme::INK })
            .width(Fill)
    };
    row![
        text(q.name.clone()).width(Fill),
        text(q.depth.to_string()).width(Fill),
        text(q.delivered.to_string()).width(Fill),
        alarm(q.failed),
        alarm(q.dead_letters)
    ]
    .into()
}

fn queues(state: &State) -> Element<'_, Message> {
    let head = row![
        text("Topic").width(Fill),
        text("Depth").width(Fill),
        text("Delivered").width(Fill),
        text("Failed").width(Fill),
        text("Dead letters").width(Fill)
    ];
    let rows = state.console.iter().flat_map(|c| &c.queues).map(queue_row);
    section(
        "Queues".into(),
        container(column![head, theme::rule()].extend(rows).spacing(4))
            .padding(12)
            .into(),
        false,
    )
}

pub fn view(state: &State) -> Element<'_, Message> {
    let link = state
        .link
        .as_ref()
        .map_or_else(|| "…".to_string(), describe_link);
    let body = column![
        text("Engine").size(24).font(theme::BOLD),
        panel(column![heading("Link"), text(link)]),
        stack_panel(state),
        doctor(state),
        containers(state),
    ]
    .extend(logs(state))
    .push(registered(state))
    .push(queues(state))
    .spacing(16);
    scrolled(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_start_says_why_and_brings_doctors_report() {
        let refused = serde_json::to_string(&StartRefused::Taken {
            pid: 7,
            compose_file: None,
        })
        .unwrap();
        assert_eq!(
            describe_refusal(&refused).0,
            "Not started: engine pid 7 already holds the port (not a compose project)."
        );
        let doctor = serde_json::to_string(&StartRefused::Doctor {
            report: DoctorReport { checks: vec![] },
        })
        .unwrap();
        assert!(describe_refusal(&doctor).1.is_some());
        assert_eq!(describe_refusal("no route").0, "no route");
    }

    #[test]
    fn the_filter_shows_at_most_a_hundred_and_counts_them_all() {
        let r = |id: &str| Registered {
            id: id.into(),
            namespace: "n".into(),
            worker: "w".into(),
            description: String::new(),
        };
        let rows: Vec<Registered> = (0..150)
            .map(|i| r(&format!("router::f{i}")))
            .chain([r("state::get")])
            .collect();
        let (shown, total) = matching(&rows, "ROUTER");
        assert_eq!((shown.len(), total), (SHOWN, 150));
        assert_eq!(matching(&rows, "state").1, 1);
    }
}
