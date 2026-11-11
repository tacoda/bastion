//! The shell: the top bar with its pages and the server switcher, the
//! page it shows, each server's pushes, and the question on quit while the
//! stack runs.

use std::collections::BTreeSet;

use bastion_protocol::{Change, LOCAL, Server, StackState, api};
use iced::widget::{Row, button, center, column, container, opaque, row, stack, svg, text};
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
    /// Show one server, by name.
    Choose(String),
    /// Show every server on the records pages.
    ChooseAll,
    Feed((String, Feed)),
    CloseRequested,
    StackRunning(bool),
    Quit,
    Stay,
    /// A minute passed: draw the page again, for its times.
    Tick,
    /// How many jobs on a server wait on a person.
    Waiting(String, u32),
    /// A job's page, on the server it runs on.
    OpenJob {
        server: String,
        id: String,
    },
    /// One record's page, such as an eval, on the server that holds it.
    OpenOn {
        server: String,
        route: Route,
    },
    Jobs(jobs::Msg),
    NewRequest(new_request::Msg),
    Job(job::Msg),
    Govern(govern::Msg),
    Audit(pages::audit::Msg),
    Engine(engine::Msg),
    Evals(evals::Msg),
    Costs(pages::costs::Msg),
    Verification(pages::verification::Msg),
    Session(pages::session::Msg),
    Settings(pages::settings::Msg),
    Workflows(pages::workflows::Msg),
    /// Read the servers again, after Settings added or removed one.
    ReadServers,
    /// Open a link, such as a pull request, in the browser.
    OpenUrl(String),
    /// Whether shift is held, so a shift-click on a diff line picks a range.
    Shift(bool),
}

pub struct App {
    http: reqwest::Client,
    servers: Vec<Server>,
    /// The server the pages show, or the last one picked while `all` is on.
    current: String,
    /// Whether the records pages show every server.
    all: bool,
    route: Route,
    /// Servers whose stream is down, so what was read from them may be stale.
    down: BTreeSet<String>,
    page: Page,
    asking: bool,
    problem: Option<String>,
    /// How many jobs wait on a person, by server.
    waiting: std::collections::BTreeMap<String, u32>,
    shift: bool,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        crate::dock::show_icon();
        let app = App {
            http: client::http(),
            servers: vec![],
            current: LOCAL.into(),
            all: true,
            route: Route::Jobs,
            down: BTreeSet::new(),
            page: Page::Pending,
            asking: false,
            problem: None,
            waiting: Default::default(),
            shift: false,
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
            all: self.all && self.route.records(),
            down: self.down.clone(),
            shift: self.shift,
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

    /// The page to show after the server changes: the same one, or for one
    /// record's page, the list it sits under, as the record is the old
    /// server's.
    fn again(&self) -> Route {
        match self.route.one_record() {
            true => self.route.section(),
            false => self.route.clone(),
        }
    }

    /// Show `route`'s page and read it.
    fn go(&mut self, route: Route) -> Task<Message> {
        // The page reads under its own route: a records page may read every server.
        self.route = route;
        let (page, first) = Page::open(&self.route, &self.ctx());
        self.page = page;
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
                (self.current, self.all) = (name, false);
                return self.go(self.again());
            }
            Message::ChooseAll => {
                self.all = true;
                return self.go(self.again());
            }
            Message::OpenJob { server, id } => {
                // The job's page talks to the server it runs on.
                self.current = server;
                return self.go(Route::Job(id));
            }
            Message::OpenOn { server, route } => {
                self.current = server;
                return self.go(route);
            }
            page @ (Message::Jobs(_)
            | Message::NewRequest(_)
            | Message::Job(_)
            | Message::Govern(_)
            | Message::Audit(_)
            | Message::Engine(_)
            | Message::Evals(_)
            | Message::Costs(_)
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
            Message::Shift(held) => self.shift = held,
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
            // The window counts the notice.
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
        let keys = iced::event::listen_with(|event, _, _| match event {
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(m)) => {
                Some(Message::Shift(m.shift()))
            }
            _ => None,
        });
        Subscription::batch([pushes, closes, ticks, frames, keys])
    }

    pub fn view(&self) -> Element<'_, Message> {
        let main = container(self.page())
            .padding([24, 32])
            .width(Fill)
            .height(Fill);
        let shell = container(column![self.bar(), main])
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

    /// The castle, the pages, and the server switcher.
    fn bar(&self) -> Element<'_, Message> {
        let castle = svg(svg::Handle::from_memory(CASTLE)).width(28).height(28);
        let brand = row![castle, text("Bastion").size(17).font(theme::BOLD)]
            .spacing(8)
            .align_y(iced::Center);
        let waiting: u32 = self.waiting.values().sum();
        let here = self.route.section();
        let pages = PAGES.into_iter().map(|(route, label, glyph)| {
            let label = match (route == Route::Jobs, waiting) {
                (true, n) if n > 0 => format!("{label} ({n})"),
                _ => label.to_string(),
            };
            tab(label, glyph, route == here, Message::Go(route))
        });
        let top = row![brand, iced::widget::space::horizontal(), self.switcher()]
            .spacing(16)
            .align_y(iced::Center);
        let nav = Row::with_children(pages).spacing(2).wrap();
        container(column![top, nav].spacing(10))
            .padding([12, 24])
            .width(Fill)
            .style(theme::bar)
            .into()
    }

    /// One button per server, each with a dot for its stream, and All
    /// servers on a records page (VISION.md).
    fn switcher(&self) -> Element<'_, Message> {
        let all_on = self.all && self.route.records();
        let mut choices: Vec<Element<'_, Message>> = Vec::new();
        if self.route.records() {
            choices.push(choice(
                "All servers".into(),
                None,
                all_on,
                Message::ChooseAll,
            ));
        }
        for s in &self.servers {
            let up = !self.down.contains(&s.name);
            let on = !all_on && s.name == self.current;
            choices.push(choice(
                s.name.clone(),
                Some(up),
                on,
                Message::Choose(s.name.clone()),
            ));
        }
        container(Row::with_children(choices).spacing(2).wrap())
            .padding(3)
            .style(theme::panel)
            .into()
    }

    fn page(&self) -> Element<'_, Message> {
        if let Some(why) = &self.problem {
            return text(why.clone()).color(theme::BAD).into();
        }
        self.page.view(&self.route, &self.ctx())
    }
}

/// The castle, from the app's icon.
const CASTLE: &[u8] = include_bytes!("../../../assets/icon/bastion.svg");

/// One page in the top bar.
fn tab<'a>(label: String, glyph: &str, on: bool, go: Message) -> Element<'a, Message> {
    let color = if on { iced::Color::WHITE } else { theme::INK };
    let body = row![icons::icon(glyph, color), text(label).size(13)]
        .spacing(6)
        .align_y(iced::Center);
    button(body)
        .padding([6, 10])
        .style(theme::nav(on))
        .on_press(go)
        .into()
}

/// One choice in the server switcher, with its stream's dot when it is a
/// server.
fn choice<'a>(label: String, up: Option<bool>, on: bool, pick: Message) -> Element<'a, Message> {
    let mut body = row![].spacing(6).align_y(iced::Center);
    if let Some(up) = up {
        let (mark, color) = if up {
            ("●", theme::OK)
        } else {
            ("○", theme::BAD)
        };
        body = body.push(text(mark).size(10).color(color));
    }
    button(body.push(text(label).size(13)))
        .padding([4, 10])
        .style(theme::nav(on))
        .on_press(pick)
        .into()
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
