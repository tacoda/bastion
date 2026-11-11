//! The Settings page: the Bastion servers, people, model providers and
//! hosts, the factory, history, workspaces and accounts, each a
//! section in its own file.

mod factory;
mod notify;
mod people;
mod places;
mod providers;

use iced::widget::{Column, button, column, container, text};
use iced::{Element, Fill, Task};

use super::{Ctx, scrolled};
use crate::app::Message;
use crate::theme;

#[derive(Default)]
pub struct State {
    places: places::State,
    people: people::State,
    providers: providers::State,
    factory: factory::State,
    notify: notify::State,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Places(places::Msg),
    People(people::Msg),
    Providers(providers::Msg),
    Factory(factory::Msg),
    Notify(notify::Msg),
}

/// A titled section of the page.
fn card<'a>(title: &'a str, body: Column<'a, Message>) -> Element<'a, Message> {
    let head = text(title).size(13).font(theme::BOLD).color(theme::MUTED);
    container(column![head, body.spacing(8)].spacing(10))
        .padding(16)
        .width(Fill)
        .style(theme::panel)
        .into()
}

/// A small lined button, faded when it cannot be pressed.
fn small(label: &str, msg: Option<Message>) -> Element<'_, Message> {
    button(text(label).size(13))
        .padding([4, 12])
        .style(theme::secondary)
        .on_press_maybe(msg)
        .into()
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    Task::batch([
        places::load(ctx),
        people::load(ctx),
        providers::load(ctx),
        factory::load(ctx),
        notify::load(ctx),
    ])
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Places(m) => places::update(&mut state.places, m, ctx),
        Msg::People(m) => people::update(&mut state.people, m, ctx),
        Msg::Providers(m) => providers::update(&mut state.providers, m, ctx),
        Msg::Factory(m) => factory::update(&mut state.factory, m, ctx),
        Msg::Notify(m) => notify::update(&mut state.notify, m, ctx),
    }
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let (servers, [workspaces, accounts]) = places::view(&state.places, ctx);
    let [providers, hosts] = providers::view(&state.providers);
    let [factory, history] = factory::view(&state.factory);
    let sections = [
        servers,
        people::view(&state.people, ctx),
        providers,
        hosts,
        factory,
        notify::view(&state.notify),
        history,
        workspaces,
        accounts,
    ];
    let body = column![text("Settings").size(24).font(theme::BOLD)]
        .extend(sections)
        .spacing(16);
    scrolled(body)
}
