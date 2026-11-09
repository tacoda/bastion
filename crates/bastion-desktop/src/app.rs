//! The shell (PLAN.md G1): the sidebar, the page it shows, the server
//! picker, each server's pushes, and the question on quit while the stack
//! runs.

use std::collections::BTreeSet;

use bastion_protocol::{Change, LOCAL, Server, StackState, StackStatus, command};
use iced::widget::{
    Column, button, center, column, container, opaque, pick_list, row, stack, text,
};
use iced::{Element, Fill, Subscription, Task, window};
use serde_json::Value;

use crate::client::{self, Ask};
use crate::events::{self, Feed};
use crate::route::{PAGES, Route};
use crate::{icons, start, theme};

#[derive(Clone, Debug)]
pub enum Message {
    Started(Result<Vec<Server>, String>),
    Go(Route),
    Choose(String),
    Feed((String, Feed)),
    CloseRequested,
    StackRunning(bool),
    Quit { stop_stack: bool },
    Stay,
}

pub struct App {
    http: reqwest::Client,
    servers: Vec<Server>,
    current: String,
    route: Route,
    /// Servers whose stream is down, so what was read from them may be stale.
    down: BTreeSet<String>,
    /// The last push, shown until the pages that use it are built (G2).
    last_push: Option<String>,
    asking: bool,
    problem: Option<String>,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let app = App {
            http: client::http(),
            servers: vec![],
            current: LOCAL.into(),
            route: Route::Jobs,
            down: BTreeSet::new(),
            last_push: None,
            asking: false,
            problem: None,
        };
        let started = async {
            tokio::task::spawn_blocking(start::start)
                .await
                .map_err(|e| e.to_string())?
        };
        (app, Task::perform(started, Message::Started))
    }

    fn server(&self, name: &str) -> Option<Server> {
        self.servers.iter().find(|s| s.name == name).cloned()
    }

    /// Send `ask` to the server named `on`, and map its answer.
    fn call(
        &self,
        on: &str,
        ask: Ask,
        then: fn(Result<Value, String>) -> Message,
    ) -> Task<Message> {
        let Some(server) = self.server(on) else {
            return Task::none();
        };
        Task::perform(client::call(self.http.clone(), server, ask), then)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Started(Ok(all)) => self.servers = all,
            Message::Started(Err(why)) => self.problem = Some(why),
            Message::Go(route) => self.route = route,
            Message::Choose(name) => self.current = name,
            Message::Feed((server, feed)) => self.on_feed(server, feed),
            Message::CloseRequested => return self.on_close(),
            Message::StackRunning(true) => self.asking = true,
            Message::StackRunning(false) => return iced::exit(),
            Message::Quit { stop_stack } => return self.quit(stop_stack),
            Message::Stay => self.asking = false,
        }
        Task::none()
    }

    fn on_feed(&mut self, server: String, feed: Feed) {
        match feed {
            Feed::Up => {
                self.down.remove(&server);
            }
            Feed::Down => {
                self.down.insert(server);
            }
            Feed::Resync => self.last_push = Some(format!("{server}: read everything again")),
            Feed::Change(change) => {
                self.last_push = Some(format!("{server}: {}", describe(&change)))
            }
        }
    }

    /// The stack belongs to the server (D27), so a quit asks only while it runs.
    fn on_close(&self) -> Task<Message> {
        if self.server(LOCAL).is_none() {
            return iced::exit();
        }
        self.call(LOCAL, Ask::bare(command::STACK_STATUS), |answer| {
            let status = answer
                .ok()
                .and_then(|v| serde_json::from_value::<StackStatus>(v).ok());
            Message::StackRunning(status.is_some_and(|s| s.state == StackState::Running))
        })
    }

    fn quit(&self, stop_stack: bool) -> Task<Message> {
        if !stop_stack {
            return iced::exit();
        }
        self.call(LOCAL, Ask::bare(command::STACK_STOP), |_| {
            Message::StackRunning(false)
        })
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let pushes = events::follow(&self.servers).map(Message::Feed);
        let closes = window::close_requests().map(|_| Message::CloseRequested);
        Subscription::batch([pushes, closes])
    }

    pub fn view(&self) -> Element<'_, Message> {
        let main = column![self.picker(), self.page()]
            .spacing(16)
            .padding(32)
            .width(Fill);
        let shell = container(row![self.sidebar(), main])
            .width(Fill)
            .height(Fill)
            .style(theme::page);
        if !self.asking {
            return shell.into();
        }
        stack![
            shell,
            opaque(container(center(quit_dialog())).style(theme::veil))
        ]
        .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let here = self.route.section();
        let mut items = vec![text("Bastion").size(18).font(theme::BOLD).into()];
        for (route, label, glyph) in PAGES {
            let on = route == here;
            let color = if on { iced::Color::WHITE } else { theme::INK };
            let entry = row![icons::icon(glyph, color), text(label)]
                .spacing(8)
                .align_y(iced::Center);
            items.push(
                button(entry)
                    .width(Fill)
                    .padding([6, 8])
                    .style(theme::nav(on))
                    .on_press(Message::Go(route))
                    .into(),
            );
        }
        let list = Column::with_children(items).spacing(4);
        let body = column![list, iced::widget::space::vertical(), self.reach()];
        container(body)
            .width(200)
            .height(Fill)
            .padding(12)
            .style(theme::sidebar)
            .into()
    }

    /// Each server and whether its stream is up.
    fn reach(&self) -> Element<'_, Message> {
        let rows = self.servers.iter().map(|s| {
            let up = !self.down.contains(&s.name);
            let mark =
                text(if up { "●" } else { "○" }).color(if up { theme::OK } else { theme::BAD });
            row![mark, text(&s.name).size(12).color(theme::MUTED)]
                .spacing(6)
                .into()
        });
        Column::with_children(rows).spacing(2).into()
    }

    /// Which Bastion server a page about one server shows (D30).
    fn picker(&self) -> Element<'_, Message> {
        if !self.route.about_one_server() {
            return column![].into();
        }
        let names: Vec<String> = self.servers.iter().map(|s| s.name.clone()).collect();
        let pick =
            pick_list(names, Some(self.current.clone()), Message::Choose).style(theme::select);
        row![text("Bastion server").color(theme::MUTED), pick]
            .spacing(8)
            .align_y(iced::Center)
            .into()
    }

    fn page(&self) -> Element<'_, Message> {
        let title = text(self.route.title()).size(24).font(theme::BOLD);
        let note = match (&self.problem, &self.last_push) {
            (Some(why), _) => text(why.clone()).color(theme::BAD),
            (None, Some(push)) => text(format!("Last push: {push}")).color(theme::MUTED),
            (None, None) => text("This page is built in G2.").color(theme::MUTED),
        };
        column![title, note].spacing(12).into()
    }
}

fn describe(change: &Change) -> String {
    match change {
        Change::Job { id } if id.is_empty() => "every job".into(),
        Change::Job { id } => format!("job {id}"),
        Change::Engine { status } => format!("engine {:?}", status.link),
        Change::Console => "the console".into(),
    }
}

fn quit_dialog<'a>() -> Element<'a, Message> {
    let buttons = row![
        button("Keep it running")
            .padding([8, 12])
            .style(theme::primary)
            .on_press(Message::Quit { stop_stack: false }),
        button("Stop the stack")
            .padding([8, 12])
            .style(theme::secondary)
            .on_press(Message::Quit { stop_stack: true }),
        button("Cancel")
            .padding([8, 12])
            .style(theme::quiet)
            .on_press(Message::Stay),
    ]
    .spacing(8);
    let body = column![
        text("The stack is running").size(18).font(theme::BOLD),
        text("Stop it with the app, or keep it running for the next start to attach to?")
            .color(theme::MUTED),
        buttons,
    ]
    .spacing(12)
    .max_width(400);
    container(body).padding(20).style(theme::panel).into()
}
