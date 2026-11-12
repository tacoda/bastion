//! The Constraints, Capabilities and Permissions tabs of Workflows (D57):
//! Bastion's rules, each on for all workflows, or on at the workflows and
//! steps you pick. On All servers each server has its section, and a
//! change goes to that server (D58).

use std::collections::BTreeMap;

use bastion_protocol::PersonView;
use bastion_protocol::api;
use iced::widget::{Column, column, row, text};
use iced::{Element, Task};

use super::step_rules::{self, Which, choice};
use super::{Ctx, scrolled};
use crate::app::Message;
use crate::theme;

/// One server's rules, and whether its person may change them.
#[derive(Default)]
struct Server {
    admin: bool,
    rules: step_rules::Rules,
}

pub struct State {
    pub which: Which,
    /// The All workflows view, or the Workflows view.
    global: bool,
    /// Each server shown, by its name.
    servers: BTreeMap<String, Server>,
}

impl State {
    pub fn new(which: Which) -> State {
        State {
            which,
            global: true,
            servers: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Global(bool),
    Me(String, Result<PersonView, String>),
    Rules(String, step_rules::Msg),
}

/// A server's rules message, as the window's.
fn on(server: String) -> impl Fn(step_rules::Msg) -> Message + Clone {
    move |m| Message::Rules(Msg::Rules(server.clone(), m))
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let each = ctx.shown().into_iter().map(|name| {
        let here = ctx.on(&name);
        let who = name.clone();
        let me = Task::perform(here.clone().get(name.clone(), api::Me), move |me| {
            Message::Rules(Msg::Me(who.clone(), me))
        });
        Task::batch([me, step_rules::load(&here).map(on(name))])
    });
    Task::batch(each)
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Global(global) => state.global = global,
        Msg::Me(name, me) => {
            state.servers.entry(name).or_default().admin = me.is_ok_and(|me| me.role == "admin")
        }
        Msg::Rules(name, m) => {
            let at = step_rules::level(state.global);
            let rules = &mut state.servers.entry(name.clone()).or_default().rules;
            return step_rules::update(rules, m, &at, &ctx.on(&name)).map(on(name));
        }
    }
    Task::none()
}

pub fn view<'a>(state: &'a State, title: &'a str, ctx: &Ctx) -> Element<'a, Message> {
    let pick = |label: &str, global: bool| {
        choice(
            label,
            state.global == global,
            Some(Message::Rules(Msg::Global(global))),
        )
    };
    let views = row![pick("All workflows", true), pick("Workflows", false)].spacing(6);
    let tabs = super::workflow_tabs(&state.which.route());
    let shown = ctx.shown();
    let named = shown.len() > 1;
    let sections = shown.into_iter().filter_map(|name| {
        let s = state.servers.get(&name)?;
        let page = step_rules::page(&s.rules, state.which, state.global, s.admin);
        let page = page.map(on(name.clone()));
        let head = named.then(|| text(name).size(18).font(theme::BOLD));
        let head = head.map(Element::from);
        Some(Column::new().spacing(10).extend(head).push(page).into())
    });
    let body = column![text(title).size(24).font(theme::BOLD), tabs, views]
        .extend(sections)
        .spacing(12);
    scrolled(body)
}
