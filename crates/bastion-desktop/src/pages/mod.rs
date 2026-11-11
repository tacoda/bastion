//! One file per page. A page keeps its state here, reads from
//! the servers through `Ctx`, and draws with the shared pieces below.

pub mod audit;
pub mod costs;
pub mod diff;
pub mod drawing;
pub mod edit;
pub mod engine;
pub mod evals;
pub mod govern;
pub mod job;
pub mod jobs;
pub mod new_request;
pub mod session;
pub mod settings;
pub mod spinner;
pub mod verification;
pub mod workflows;

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use bastion_protocol::Server;
use bastion_protocol::api::Request;
use iced::widget::{column, container, text};
use iced::{Color, Element, Fill, Task};

use crate::app::Message;
use crate::client;
use crate::route::Route;
use crate::theme;

/// What a page may read: the servers, which one is shown, and which cannot
/// be reached now. Cheap to clone into a task.
#[derive(Clone)]
pub struct Ctx {
    pub http: reqwest::Client,
    pub servers: Vec<Server>,
    pub current: String,
    /// Whether a records page shows every server, not only `current`.
    pub all: bool,
    pub down: BTreeSet<String>,
    /// Whether shift is held now.
    pub shift: bool,
}

impl Ctx {
    pub fn server(&self, name: &str) -> Result<Server, String> {
        let found = self.servers.iter().find(|s| s.name == name);
        found
            .cloned()
            .ok_or_else(|| format!("no Bastion server named {name}"))
    }

    /// Send `request` to the server named `on`, and read its answer.
    pub async fn get<R: Request>(self, on: String, request: R) -> Result<R::Answer, String> {
        let server = self.server(&on)?;
        client::send(&self.http, &server, &request).await
    }

    /// A task that sends `request` to `on` and hands its answer to `then`.
    pub fn task<R, M>(
        &self,
        on: &str,
        request: R,
        then: fn(Result<R::Answer, String>) -> M,
    ) -> Task<M>
    where
        R: Request + Send + Sync + 'static,
        R::Answer: Send + 'static,
        M: Send + 'static,
    {
        Task::perform(self.clone().get(on.to_string(), request), then)
    }
}

impl Ctx {
    /// The servers a records page reads: every one while All servers is
    /// on, else the one shown.
    pub fn shown(&self) -> Vec<String> {
        match self.all {
            true => self.servers.iter().map(|s| s.name.clone()).collect(),
            false => vec![self.current.clone()],
        }
    }

    /// Send `request` to each server [`Ctx::shown`] names. Returns each
    /// answer with its server, and why each server that failed did.
    pub async fn each<R: Request + Clone>(self, request: R) -> Each<R::Answer> {
        let (mut read, mut failed) = (Vec::new(), Vec::new());
        for name in self.shown() {
            match self.clone().get(name.clone(), request.clone()).await {
                Ok(answer) => read.push((name, answer)),
                Err(e) => failed.push(format!("{name}: {e}")),
            }
        }
        (read, failed)
    }
}

/// Each server's answer by its name, and why each server that failed did.
pub type Each<A> = (Vec<(String, A)>, Vec<String>);

/// The states of a job or step still on its way; their marks turn.
pub const IN_PROGRESS: [&str; 3] = ["queued", "provisioning", "running"];
/// How often a turning mark is drawn again.
pub const SPIN_MS: u64 = 30;

/// A status mark and its color, as a run of steps shows them. A mark in
/// progress is drawn turning by [`mark_view`]; its glyph stands still.
pub fn mark(state: &str) -> (&'static str, Color) {
    match state {
        "done" | "landed" => ("✓", theme::OK),
        "running" | "provisioning" => ("◌", theme::LINK),
        "waiting" | "needs_you" => ("!", theme::WARN),
        "failed" => ("✕", theme::BAD),
        "stopped" => ("■", theme::MUTED),
        "queued" => ("◌", theme::MUTED),
        _ => ("○", theme::MUTED),
    }
}

/// A state's mark as a widget, `size` across: a turning dashed circle while
/// in progress.
pub fn mark_view<'a, M: 'a>(state: &str, size: f32) -> Element<'a, M> {
    let (symbol, color) = mark(state);
    match IN_PROGRESS.contains(&state) {
        true => spinner::spinner(size * 0.8, color),
        false => text(symbol).size(size).color(color).into(),
    }
}

/// "3 min ago", from seconds since the epoch.
pub fn ago(at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    match now.saturating_sub(at) {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

/// A titled section: a rounded panel with its title above a line. An
/// accented one has the warning color's edge.
pub fn section<'a, M: 'a>(title: String, body: Element<'a, M>, accent: bool) -> Element<'a, M> {
    let head =
        container(text(title).size(13).color(theme::MUTED).font(theme::BOLD)).padding([8, 12]);
    let style = if accent {
        theme::accented
    } else {
        theme::panel
    };
    container(column![head, theme::rule(), body])
        .width(Fill)
        .style(style)
        .into()
}

/// One option of a picker: the value it stands for and the words it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub id: String,
    pub name: String,
}

impl Choice {
    pub fn new(id: &str, name: &str) -> Self {
        Choice {
            id: id.into(),
            name: name.into(),
        }
    }
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// The id of every page's scroll area, so a new page starts at its top.
pub const SCROLL: &str = "page";

/// A page's body in its scroll area, clear of the scroll bar.
pub fn scrolled<'a, M: 'a>(body: impl Into<Element<'a, M>>) -> Element<'a, M> {
    let body = container(body).padding(iced::Padding::ZERO.right(16));
    iced::widget::scrollable(body)
        .id(SCROLL)
        .height(Fill)
        .into()
}

/// A label above a field, as the forms show them.
pub fn labeled<'a, M: 'a>(label: &'a str, field: impl Into<Element<'a, M>>) -> Element<'a, M> {
    column![text(label).size(13).color(theme::MUTED), field.into()]
        .spacing(4)
        .into()
}

/// What a push says changed, so a page reads again only what it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Push {
    /// Read everything: a page just opened, changes were missed, or a
    /// change reached every job (a setting, say).
    All,
    /// One job changed.
    Job,
    /// The engine or its console changed.
    Engine,
}

/// The page the window shows, with its state.
pub enum Page {
    Jobs(jobs::State),
    NewRequest(new_request::State),
    Job(Box<job::State>),
    Govern(govern::State),
    Audit(Box<audit::State>),
    Engine(Box<engine::State>),
    Evals(Box<evals::List>),
    Costs(Box<costs::State>),
    Verification(Box<verification::State>),
    Session(Box<session::State>),
    Eval(Box<evals::One>),
    Settings(Box<settings::State>),
    Workflows(Box<workflows::State>),
    /// Nothing yet: the window has not reached its first server.
    Pending,
}

impl Page {
    /// Whether the page shows a job or step in progress, whose mark turns.
    pub fn moving(&self) -> bool {
        match self {
            Page::Jobs(s) => s.moving(),
            Page::Job(s) => s.moving(),
            _ => false,
        }
    }

    /// The page for `route`, and its first read.
    pub fn open(route: &Route, ctx: &Ctx) -> (Page, Task<Message>) {
        let page = match route {
            Route::Jobs => Page::Jobs(jobs::State::default()),
            Route::NewRequest => Page::NewRequest(new_request::State::default()),
            Route::Job(id) => Page::Job(Box::new(job::State::new(id.clone()))),
            Route::Constraints => Page::Govern(govern::State::new(govern::Which::Constraints)),
            Route::Capabilities => Page::Govern(govern::State::new(govern::Which::Capabilities)),
            Route::Audit => Page::Audit(Box::default()),
            Route::Engine => Page::Engine(Box::default()),
            Route::Evals => Page::Evals(Box::default()),
            Route::Costs => Page::Costs(Box::default()),
            Route::Verification => Page::Verification(Box::default()),
            Route::Session(id) => Page::Session(Box::new(session::State::new(id.clone()))),
            Route::Eval(id) => Page::Eval(Box::new(evals::One::new(id.clone()))),
            Route::Settings => Page::Settings(Box::default()),
            Route::Workflows => Page::Workflows(Box::default()),
        };
        let first = page.reload(ctx, Push::All);
        (page, first)
    }

    /// Whether `push` changed what the page shows.
    fn wants(&self, push: Push) -> bool {
        match self {
            Page::Pending => false,
            Page::NewRequest(_) | Page::Settings(_) => push == Push::All,
            Page::Engine(_) => push != Push::Job,
            // ponytail: any job's change reads a job or an eval again; pass the id when that costs.
            _ => push != Push::Engine,
        }
    }

    /// Read what the page shows.
    fn load(&self, ctx: &Ctx) -> Task<Message> {
        match self {
            Page::Jobs(s) => jobs::load(s, ctx).map(Message::Jobs),
            Page::NewRequest(s) => new_request::load(s, ctx),
            Page::Job(s) => job::load(s, ctx),
            Page::Govern(s) => govern::load(s, ctx),
            Page::Audit(_) => audit::load(ctx),
            Page::Engine(_) => engine::load(ctx),
            Page::Evals(_) => evals::load_list(ctx),
            Page::Costs(_) => costs::load(ctx),
            Page::Verification(_) => verification::load(ctx),
            Page::Session(s) => session::load(s, ctx),
            Page::Eval(s) => evals::load_one(s, ctx),
            Page::Settings(_) => settings::load(ctx),
            Page::Workflows(_) => workflows::load(ctx),
            Page::Pending => Task::none(),
        }
    }

    /// Read again after a push, when it changed what the page shows.
    pub fn reload(&self, ctx: &Ctx, push: Push) -> Task<Message> {
        if self.wants(push) {
            self.load(ctx)
        } else {
            Task::none()
        }
    }

    /// Hand a page's message to the page it belongs to; one for a page no
    /// longer shown is dropped.
    pub fn update(&mut self, message: Message, ctx: &Ctx) -> Task<Message> {
        match (self, message) {
            (Page::Jobs(s), Message::Jobs(m)) => jobs::update(s, m),
            (Page::NewRequest(s), Message::NewRequest(m)) => return new_request::update(s, m, ctx),
            (Page::Job(s), Message::Job(m)) => return job::update(s, m, ctx),
            (Page::Govern(s), Message::Govern(m)) => return govern::update(s, m, ctx),
            (Page::Audit(s), Message::Audit(m)) => return audit::update(s, m, ctx),
            (Page::Engine(s), Message::Engine(m)) => return engine::update(s, m, ctx),
            (Page::Evals(s), Message::Evals(m)) => return evals::update_list(s, m, ctx),
            (Page::Costs(s), Message::Costs(m)) => costs::update(s, m),
            (Page::Verification(s), Message::Verification(m)) => {
                return verification::update(s, m, ctx);
            }
            (Page::Session(s), Message::Session(m)) => session::update(s, m),
            (Page::Eval(s), Message::Evals(m)) => return evals::update_one(s, m, ctx),
            (Page::Settings(s), Message::Settings(m)) => return settings::update(s, m, ctx),
            (Page::Workflows(s), Message::Workflows(m)) => return workflows::update(s, m, ctx),
            _ => {}
        }
        Task::none()
    }

    pub fn view<'a>(&'a self, route: &Route, ctx: &Ctx) -> Element<'a, Message> {
        match self {
            Page::Jobs(s) => jobs::view(s, ctx),
            Page::NewRequest(s) => new_request::view(s, ctx),
            Page::Job(s) => job::view(s, ctx),
            Page::Govern(s) => govern::view(s),
            Page::Audit(s) => audit::view(s, ctx),
            Page::Engine(s) => engine::view(s),
            Page::Evals(s) => evals::view_list(s),
            Page::Costs(s) => costs::view(s, ctx),
            Page::Verification(s) => verification::view(s, ctx),
            Page::Session(s) => session::view(s, ctx),
            Page::Eval(s) => evals::view_one(s, ctx),
            Page::Settings(s) => settings::view(s, ctx),
            Page::Workflows(s) => workflows::view(s),
            Page::Pending => {
                let title = text(route.title()).size(24).font(theme::BOLD);
                let note = text("Reaching the Bastion server…").color(theme::MUTED);
                column![title, note].spacing(12).into()
            }
        }
    }
}
