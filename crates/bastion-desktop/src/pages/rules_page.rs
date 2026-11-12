//! The Constraints, Capabilities and Permissions pages (D57): Bastion's
//! rules on the server, each on for every workflow, or on at the workflows
//! and steps you pick.

use bastion_protocol::PersonView;
use bastion_protocol::api;
use iced::widget::{column, row, text};
use iced::{Element, Task};

use super::step_rules::{self, Which, choice};
use super::{Ctx, scrolled};
use crate::app::Message;
use crate::theme;

pub struct State {
    pub which: Which,
    /// The Global view, or the Workflows view.
    global: bool,
    admin: bool,
    rules: step_rules::Rules,
}

impl State {
    pub fn new(which: Which) -> State {
        State {
            which,
            global: true,
            admin: false,
            rules: step_rules::Rules::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Global(bool),
    Me(Result<PersonView, String>),
    Rules(step_rules::Msg),
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let me = ctx.task(&ctx.current, api::Me, Msg::Me);
    let rules = step_rules::load(ctx).map(Msg::Rules);
    Task::batch([me, rules]).map(Message::Rules)
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Global(global) => state.global = global,
        Msg::Me(me) => state.admin = me.is_ok_and(|me| me.role == "admin"),
        Msg::Rules(m) => {
            let at = step_rules::level(state.global);
            return step_rules::update(&mut state.rules, m, &at, ctx)
                .map(|m| Message::Rules(Msg::Rules(m)));
        }
    }
    Task::none()
}

pub fn view<'a>(state: &'a State, title: &'a str) -> Element<'a, Message> {
    let pick = |label: &str, global: bool| {
        choice(
            label,
            state.global == global,
            Some(Message::Rules(Msg::Global(global))),
        )
    };
    let views = row![pick("Global", true), pick("Workflows", false)].spacing(6);
    let body = step_rules::page(&state.rules, state.which, state.global, state.admin)
        .map(|m| Message::Rules(Msg::Rules(m)));
    scrolled(column![text(title).size(24).font(theme::BOLD), views, body].spacing(12))
}
