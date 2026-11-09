//! One file per page (PLAN.md G2). A page keeps its state here, reads from
//! the servers through `Ctx`, and draws with the shared pieces below.

pub mod diff;
pub mod drawing;
pub mod engine;
pub mod govern;
pub mod job;
pub mod jobs;
pub mod new_request;

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use bastion_protocol::Server;
use iced::widget::{column, container, text};
use iced::{Color, Element, Fill, Task};
use serde::de::DeserializeOwned;

use crate::app::Message;
use crate::client::{self, Ask};
use crate::route::Route;
use crate::theme;

/// What a page may read: the servers, which one is shown, and which cannot
/// be reached now. Cheap to clone into a task.
#[derive(Clone)]
pub struct Ctx {
    pub http: reqwest::Client,
    pub servers: Vec<Server>,
    pub current: String,
    pub down: BTreeSet<String>,
}

impl Ctx {
    fn server(&self, name: &str) -> Result<Server, String> {
        let found = self.servers.iter().find(|s| s.name == name);
        found
            .cloned()
            .ok_or_else(|| format!("no Bastion server named {name}"))
    }

    /// Send `ask` to the server named `on`, and read its answer as `T`.
    pub async fn get<T: DeserializeOwned>(self, on: String, ask: Ask) -> Result<T, String> {
        let server = self.server(&on)?;
        let value = client::call(self.http, server, ask).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    /// A task that sends `ask` to `on` and hands its answer to `then`.
    pub fn task<T, M>(&self, on: &str, ask: Ask, then: fn(Result<T, String>) -> M) -> Task<M>
    where
        T: DeserializeOwned + Send + 'static,
        M: Send + 'static,
    {
        Task::perform(self.clone().get(on.to_string(), ask), then)
    }
}

/// A status mark and its color, as a run of steps shows them.
pub fn mark(state: &str) -> (&'static str, Color) {
    match state {
        "done" | "landed" => ("✓", theme::OK),
        "running" | "provisioning" => ("●", theme::LINK),
        "waiting" | "needs_you" => ("!", theme::WARN),
        "failed" => ("✕", theme::BAD),
        "stopped" => ("■", theme::MUTED),
        "queued" => ("◌", theme::MUTED),
        _ => ("○", theme::MUTED),
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
    Engine(Box<engine::State>),
    /// A page G2 has not built yet.
    Pending,
}

impl Page {
    /// The page for `route`, and its first read.
    pub fn open(route: &Route, ctx: &Ctx) -> (Page, Task<Message>) {
        let page = match route {
            Route::Jobs => Page::Jobs(jobs::State::default()),
            Route::NewRequest => Page::NewRequest(new_request::State::default()),
            Route::Job(id) => Page::Job(Box::new(job::State::new(id.clone()))),
            Route::Constraints => Page::Govern(govern::State::new(govern::Which::Constraints)),
            Route::Capabilities => Page::Govern(govern::State::new(govern::Which::Capabilities)),
            Route::Audit => Page::Govern(govern::State::new(govern::Which::Audit)),
            Route::Engine => Page::Engine(Box::default()),
            _ => Page::Pending,
        };
        let first = page.reload(ctx, Push::All);
        (page, first)
    }

    /// Read again after a push, when it changed what the page shows.
    pub fn reload(&self, ctx: &Ctx, push: Push) -> Task<Message> {
        let jobs = push != Push::Engine;
        match self {
            Page::Jobs(s) if jobs => jobs::load(s, ctx).map(Message::Jobs),
            Page::NewRequest(s) if push == Push::All => new_request::load(s, ctx),
            // ponytail: any job's change reads this one again; pass the id when that costs.
            Page::Job(s) if jobs => job::load(s, ctx),
            Page::Govern(s) if jobs => govern::load(s, ctx),
            Page::Engine(_) if push != Push::Job => engine::load(ctx),
            _ => Task::none(),
        }
    }

    pub fn view<'a>(&'a self, route: &Route, ctx: &Ctx) -> Element<'a, Message> {
        match self {
            Page::Jobs(s) => jobs::view(s, ctx),
            Page::NewRequest(s) => new_request::view(s, ctx),
            Page::Job(s) => job::view(s, ctx),
            Page::Govern(s) => govern::view(s),
            Page::Engine(s) => engine::view(s),
            Page::Pending => {
                let title = text(route.title()).size(24).font(theme::BOLD);
                column![title, text("This page is built in G2.").color(theme::MUTED)]
                    .spacing(12)
                    .into()
            }
        }
    }
}
