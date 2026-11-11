//! The shell: the sidebar, the page it shows, the server
//! picker, each server's pushes, and the question on quit while the stack
//! runs.

use std::collections::BTreeSet;

use bastion_protocol::{Change, LOCAL, Server, StackState, api};
use iced::widget::{
    Column, button, center, column, container, opaque, pick_list, row, stack, text,
};
use iced::{Element, Fill, Subscription, Task, window};

use crate::client;
use crate::events::{self, Feed};
use crate::pages::{self, Ctx, Page, Push, engine, evals, govern, job, jobs, new_request};
use crate::route::{PAGES, Route};
use crate::{icons, start, theme};

#[derive(Clone, Debug)]
pub enum Message {
    Started(Result<Server, String>),
    Servers(Result<Vec<Server>, String>),
    Go(Route),
    Choose(String),
    Feed((String, Feed)),
    CloseRequested,
    StackRunning(bool),
    Quit,
    Stay,
    /// A minute passed: draw the page again, for its times.
    Tick,
    /// How many jobs on a server wait on a person (F9).
    Waiting(String, u32),
    /// A job's page, on the server it runs on.
    OpenJob {
        server: String,
        id: String,
    },
    Jobs(jobs::Msg),
    NewRequest(new_request::Msg),
    Job(job::Msg),
    Govern(govern::Msg),
    Engine(engine::Msg),
    Evals(evals::Msg),
    Costs(pages::costs::Msg),
    Learn(pages::learn::Msg),
    Verification(pages::verification::Msg),
    Session(pages::session::Msg),
    Settings(pages::settings::Msg),
    Workflows(pages::workflows::Msg),
    /// Read the servers again, after Settings added or removed one.
    ReadServers,
    /// Open a link, such as a pull request, in the browser.
    OpenUrl(String),
}

pub struct App {
    http: reqwest::Client,
    servers: Vec<Server>,
    current: String,
    route: Route,
    /// Servers whose stream is down, so what was read from them may be stale.
    down: BTreeSet<String>,
    page: Page,
    asking: bool,
    problem: Option<String>,
    /// How many jobs wait on a person, by server (F9).
    waiting: std::collections::BTreeMap<String, u32>,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let app = App {
            http: client::http(),
            servers: vec![],
            current: LOCAL.into(),
            route: Route::Jobs,
            down: BTreeSet::new(),
            page: Page::Pending,
            asking: false,
            problem: None,
            waiting: Default::default(),
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

    fn ctx(&self) -> Ctx {
        Ctx {
            http: self.http.clone(),
            servers: self.servers.clone(),
            current: self.current.clone(),
            down: self.down.clone(),
        }
    }

    /// Read every server again, `local` first. The added servers' tokens
    /// come from the Keychain, which can take seconds.
    fn read_servers(&self) -> Task<Message> {
        let Some(local) = self.server(LOCAL) else {
            return Task::none();
        };
        let every = async move {
            tokio::task::spawn_blocking(move || start::every(local))
                .await
                .map_err(|e| e.to_string())?
        };
        Task::perform(every, Message::Servers)
    }

    /// Show `route`'s page and read it.
    fn go(&mut self, route: Route) -> Task<Message> {
        let (page, first) = Page::open(&route, &self.ctx());
        (self.route, self.page) = (route, page);
        let top = iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 };
        Task::batch([
            first,
            iced::widget::operation::scroll_to(pages::SCROLL, top),
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Started(Ok(local)) => {
                self.servers = vec![local];
                return Task::batch([self.go(self.route.clone()), self.read_servers()]);
            }
            Message::Servers(Ok(all)) => {
                self.servers = all;
                let counts = self.servers.iter().map(|s| self.count_waiting(&s.name));
                let reload = self.page.reload(&self.ctx(), Push::All);
                return Task::batch(counts.chain([reload]));
            }
            Message::Servers(Err(why)) => self.problem = Some(why),
            Message::ReadServers => return self.read_servers(),
            Message::Started(Err(why)) => self.problem = Some(why),
            Message::Go(route) => return self.go(route),
            Message::Choose(name) => {
                self.current = name;
                return self.go(self.route.clone());
            }
            Message::OpenJob { server, id } => {
                // The job's page talks to the server it runs on.
                self.current = server;
                return self.go(Route::Job(id));
            }
            page @ (Message::Jobs(_)
            | Message::NewRequest(_)
            | Message::Job(_)
            | Message::Govern(_)
            | Message::Engine(_)
            | Message::Evals(_)
            | Message::Costs(_)
            | Message::Learn(_)
            | Message::Verification(_)
            | Message::Session(_)
            | Message::Settings(_)
            | Message::Workflows(_)) => {
                let ctx = self.ctx();
                return self.page.update(page, &ctx);
            }
            Message::OpenUrl(url) => open(&url),
            Message::Feed((server, feed)) => return self.on_feed(server, feed),
            Message::CloseRequested => return self.on_close(),
            Message::StackRunning(true) => self.asking = true,
            Message::StackRunning(false) => return iced::exit(),
            Message::Quit => {
                println!("bastion: the local Bastion server keeps running, and its stack with it");
                return iced::exit();
            }
            Message::Stay => self.asking = false,
            Message::Tick => {}
            Message::Waiting(server, n) => drop(self.waiting.insert(server, n)),
        }
        Task::none()
    }

    /// A push: note whether the server answers, and read the page again
    /// when what it shows may have changed. A job change reaches the page
    /// from every server, because the Jobs page lists them all; the rest
    /// only from the server the page shows.
    fn on_feed(&mut self, server: String, feed: Feed) -> Task<Message> {
        let push = match feed {
            // A server back from down may have missed changes.
            Feed::Up if self.down.remove(&server) => {
                let page = self.page.reload(&self.ctx(), Push::All);
                return Task::batch([self.count_waiting(&server), page]);
            }
            Feed::Up => return Task::none(),
            Feed::Down => {
                self.down.insert(server);
                return Task::none();
            }
            Feed::Resync => {
                let page = self.page.reload(&self.ctx(), Push::All);
                return Task::batch([self.count_waiting(&server), page]);
            }
            // The tray shows the notice itself; the window counts it.
            Feed::Change(Change::Notice { waiting, .. }) => {
                self.waiting.insert(server, waiting);
                Push::Job
            }
            Feed::Change(Change::Job { id }) if id.is_empty() => Push::All,
            Feed::Change(Change::Job { .. }) => Push::Job,
            Feed::Change(_) if server != self.current => return Task::none(),
            Feed::Change(_) => Push::Engine,
        };
        self.page.reload(&self.ctx(), push)
    }

    /// Read how many of `server`'s jobs wait on a person.
    fn count_waiting(&self, server: &str) -> Task<Message> {
        let name = server.to_string();
        let jobs = self.ctx().get(name.clone(), api::Jobs);
        Task::perform(jobs, move |listed| {
            let all = listed.unwrap_or_default();
            let n = all.iter().filter(|j| j.state == "needs_you").count() as u32;
            Message::Waiting(name, n)
        })
    }

    /// The window's title, with how many jobs wait on a person.
    pub fn title(&self) -> String {
        titled(self.waiting.values().sum())
    }

    /// The server and its stack outlive the client (D13), so a quit says
    /// so while they run.
    fn on_close(&self) -> Task<Message> {
        if self.server(LOCAL).is_none() {
            return iced::exit();
        }
        self.ctx().task(LOCAL, api::StackStatus, |answer| {
            Message::StackRunning(answer.is_ok_and(|s| s.state == StackState::Running))
        })
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let pushes = events::follow(&self.servers).map(Message::Feed);
        let closes = window::close_requests().map(|_| Message::CloseRequested);
        let ticks = iced::time::every(std::time::Duration::from_secs(60)).map(|_| Message::Tick);
        // A job in progress has a turning mark, which needs frames.
        let frames = match self.page.moving() {
            true => iced::time::every(std::time::Duration::from_millis(pages::SPIN_MS))
                .map(|_| Message::Tick),
            false => Subscription::none(),
        };
        Subscription::batch([pushes, closes, ticks, frames])
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
            let waiting: u32 = self.waiting.values().sum();
            let label = match (route == Route::Jobs, waiting) {
                (true, n) if n > 0 => format!("{label} ({n})"),
                _ => label.to_string(),
            };
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
        if let Some(why) = &self.problem {
            return text(why.clone()).color(theme::BAD).into();
        }
        self.page.view(&self.route, &self.ctx())
    }
}

/// Open `url` in the browser.
fn open(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(opener).arg(url).spawn() {
        eprintln!("{opener} {url}: {e}");
    }
}

fn quit_dialog<'a>() -> Element<'a, Message> {
    let buttons = row![
        button("Quit")
            .padding([8, 12])
            .style(theme::primary)
            .on_press(Message::Quit),
        button("Cancel")
            .padding([8, 12])
            .style(theme::quiet)
            .on_press(Message::Stay),
    ]
    .spacing(8);
    let body = column![
        text("The Bastion server keeps running").size(18).font(theme::BOLD),
        text("It and its stack go on after you quit, so jobs keep running. The next start attaches to it. Stopping the server stops the stack too.")
            .color(theme::MUTED),
        buttons,
    ]
    .spacing(12)
    .max_width(400);
    container(body).padding(20).style(theme::panel).into()
}

/// "Bastion", and how many jobs wait on a person when any do.
fn titled(waiting: u32) -> String {
    match waiting {
        0 => "Bastion".into(),
        n => format!("Bastion ({n} waiting)"),
    }
}

#[cfg(test)]
mod title_tests {
    use super::titled;

    #[test]
    fn the_title_counts_the_jobs_waiting_on_a_person() {
        assert_eq!(titled(0), "Bastion");
        assert_eq!(titled(3), "Bastion (3 waiting)");
    }
}
