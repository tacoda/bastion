//! The Settings page: the Bastion servers, people, model providers and
//! hosts, MCP servers, the factory and history, each a section in its own
//! file. Workspaces have a page of their own (S2).

mod factory;
mod mcp;
mod notify;
mod people;
mod providers;
mod servers;

use iced::widget::{Column, column, container, text};
use iced::{Element, Fill, Task};

use super::{Ctx, scrolled};
use crate::app::Message;
use crate::theme;

#[derive(Default)]
pub struct State {
    servers: servers::State,
    people: people::State,
    providers: providers::State,
    mcp: mcp::State,
    factory: factory::State,
    notify: notify::State,
    /// Why the theme could not be kept, when it could not.
    theme_note: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Servers(servers::Msg),
    People(people::Msg),
    Providers(providers::Msg),
    Mcp(mcp::Msg),
    Factory(factory::Msg),
    Notify(notify::Msg),
    /// Use the theme named, and keep it.
    Theme(String),
}

/// A titled section of the page.
pub(in crate::pages) fn card<'a>(
    title: &'a str,
    body: Column<'a, Message>,
) -> Element<'a, Message> {
    let head = text(title).size(13).font(theme::BOLD).color(theme::muted());
    container(column![head, body.spacing(8)].spacing(10))
        .padding(16)
        .width(Fill)
        .style(theme::panel)
        .into()
}

/// A small lined button, faded when it cannot be pressed.
pub(in crate::pages) fn small(label: &str, msg: Option<Message>) -> Element<'_, Message> {
    crate::lift::button(text(label).size(13))
        .padding([4, 12])
        .style(theme::secondary)
        .on_press_maybe(msg)
        .into()
}

/// This app's theme: one of the named themes, kept for the next start.
fn appearance(state: &State) -> Element<'_, Message> {
    let picks = theme::THEMES.iter().map(|(name, _)| {
        let on = *name == theme::name();
        crate::lift::button(text(*name).size(13))
            .padding([5, 12])
            .style(theme::nav(on))
            .on_press(Message::Settings(Msg::Theme(name.to_string())))
            .into()
    });
    let note = text(&state.theme_note).size(13).color(theme::bad());
    card(
        "Theme",
        column![iced::widget::Row::with_children(picks).spacing(6), note],
    )
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    Task::batch([
        people::load(ctx),
        providers::load(ctx),
        mcp::load(ctx),
        factory::load(ctx),
        notify::load(ctx),
    ])
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Servers(m) => servers::update(&mut state.servers, m, ctx),
        Msg::People(m) => people::update(&mut state.people, m, ctx),
        Msg::Providers(m) => providers::update(&mut state.providers, m, ctx),
        Msg::Mcp(m) => mcp::update(&mut state.mcp, m, ctx),
        Msg::Factory(m) => factory::update(&mut state.factory, m, ctx),
        Msg::Notify(m) => notify::update(&mut state.notify, m, ctx),
        Msg::Theme(name) => {
            state.theme_note = crate::theme::save(&name).err().unwrap_or_default();
            Task::none()
        }
    }
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let servers = servers::view(&state.servers, ctx);
    let appearance = appearance(state);
    let [providers, hosts] = providers::view(&state.providers);
    let [factory, history] = factory::view(&state.factory);
    let sections = [
        appearance,
        servers,
        people::view(&state.people, ctx),
        providers,
        hosts,
        mcp::view(&state.mcp),
        factory,
        notify::view(&state.notify),
        history,
    ];
    let about = format!(
        "{}'s settings, and the Bastion servers this app knows.",
        ctx.current
    );
    let body = column![crate::pages::head("Settings", about, vec![])]
        .extend(sections)
        .spacing(16);
    scrolled(body)
}
