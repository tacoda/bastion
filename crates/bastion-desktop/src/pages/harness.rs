//! The Harness: what governs an agent, by kind, each item on for all
//! workflows or at the workflows and steps you pick. On All servers each
//! server has its section, and a change goes to that server (D58).

use std::collections::BTreeMap;

use bastion_protocol::api;
use bastion_protocol::{ConstraintView, FieldNoteView, PersonView};
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
    /// What journeys taught about Bastion, newest first (D61).
    learned: Vec<FieldNoteView>,
    /// Why the last note could not become guidance.
    adopt_error: String,
}

pub struct State {
    /// The kind shown, or every kind.
    kind: Option<Which>,
    /// The All workflows view, or the Workflows view.
    global: bool,
    /// Each server shown, by its name.
    servers: BTreeMap<String, Server>,
}

impl Default for State {
    fn default() -> State {
        State {
            kind: None,
            global: true,
            servers: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Kind(Option<Which>),
    Global(bool),
    Me(String, Result<PersonView, String>),
    Learned(String, Result<Vec<FieldNoteView>, String>),
    /// Make a note about Bastion guidance on its journey's trail.
    Adopt(String, FieldNoteView),
    Adopted(String, Result<(), String>),
    Rules(String, step_rules::Msg),
}

/// A server's rules message, as the window's.
fn on(server: String) -> impl Fn(step_rules::Msg) -> Message + Clone {
    move |m| Message::Rules(Msg::Rules(server.clone(), m))
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let each = ctx.shown().into_iter().map(|name| {
        let here = ctx.on(&name);
        let (who, whose) = (name.clone(), name.clone());
        let me = Task::perform(here.clone().get(name.clone(), api::Me), move |me| {
            Message::Rules(Msg::Me(who.clone(), me))
        });
        let learned = Task::perform(here.clone().get(name.clone(), api::Atlas), move |r| {
            Message::Rules(Msg::Learned(whose.clone(), r))
        });
        Task::batch([me, learned, step_rules::load(&here).map(on(name))])
    });
    Task::batch(each)
}

/// A note about Bastion as guidance: a rule the agent is told, on at the
/// trail the journey took, or every trail when it took none on record.
fn guidance(n: &FieldNoteView) -> ConstraintView {
    let scope = if n.workflow.is_empty() {
        "*"
    } else {
        n.workflow.as_str()
    };
    ConstraintView {
        id: format!("learned-{}-{}", n.job, n.at),
        rule: n.text.clone(),
        rung: "told".into(),
        source: "owner".into(),
        steps: vec![scope.into()],
        ..ConstraintView::default()
    }
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Kind(kind) => state.kind = kind,
        Msg::Global(global) => state.global = global,
        Msg::Me(name, me) => {
            state.servers.entry(name).or_default().admin = me.is_ok_and(|me| me.role == "admin")
        }
        Msg::Learned(name, notes) => {
            let about = notes.unwrap_or_default().into_iter();
            let mine = about.filter(|n| n.about == "bastion").collect();
            state.servers.entry(name).or_default().learned = mine;
        }
        Msg::Adopt(name, note) => {
            let add = api::ConstraintAdd(guidance(&note));
            let who = name.clone();
            let here = ctx.on(&name);
            return Task::perform(here.get(name, add), move |r| {
                Message::Rules(Msg::Adopted(who.clone(), r))
            });
        }
        Msg::Adopted(name, done) => {
            let s = state.servers.entry(name.clone()).or_default();
            if let Err(e) = done {
                s.adopt_error = e;
                return Task::none();
            }
            s.adopt_error.clear();
            return load(&ctx.on(&name));
        }
        Msg::Rules(name, m) => {
            let at = step_rules::level(state.global);
            let rules = &mut state.servers.entry(name.clone()).or_default().rules;
            return step_rules::update(rules, m, &at, &ctx.on(&name)).map(on(name));
        }
    }
    Task::none()
}

/// The learning loop (D61): what journeys taught about Bastion, each one a
/// press away from being guidance on the trail it came from.
fn learned<'a>(s: &'a Server, server: &str) -> Element<'a, Message> {
    let in_rules = |n: &FieldNoteView| s.rules.has_rule(&n.text);
    let rows = s.learned.iter().map(|n| {
        let trail = if n.workflow.is_empty() {
            "every trail".to_string()
        } else {
            n.workflow.clone()
        };
        let whence = format!("{} · {} · {}", n.by, n.request, super::ago(n.at));
        let act: Element<'a, Message> = match (in_rules(n), s.admin) {
            (true, _) => text("Guidance now").size(12).color(theme::ok()).into(),
            (false, true) => {
                crate::lift::button(text(format!("Make it guidance on {trail}")).size(13))
                    .style(theme::secondary)
                    .on_press(Message::Rules(Msg::Adopt(server.into(), n.clone())))
                    .into()
            }
            (false, false) => text("").into(),
        };
        let words = column![
            text(n.text.clone()).size(13),
            text(whence).size(12).color(theme::muted())
        ]
        .spacing(2)
        .width(iced::Fill);
        row![words, act].spacing(10).align_y(iced::Center).into()
    });
    let mut body = Column::new().spacing(10).extend(rows);
    if s.learned.is_empty() {
        let none =
            "Nothing yet. When a journey comes home, sign a note about Bastion on its charter.";
        body = body.push(text(none).size(13).color(theme::muted()));
    }
    if !s.adopt_error.is_empty() {
        body = body.push(text(&s.adopt_error).size(13).color(theme::bad()));
    }
    let head = row![
        text("Learned on journeys").size(15).font(theme::BOLD),
        text("What journeys taught about Bastion. Make one guidance on its trail.")
            .size(12)
            .color(theme::muted())
    ]
    .spacing(10)
    .align_y(iced::Center);
    iced::widget::container(column![head, body].spacing(10))
        .padding(12)
        .width(iced::Fill)
        .style(theme::panel)
        .into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let pick = |label: &str, global: bool| {
        choice(
            label,
            state.global == global,
            Some(Message::Rules(Msg::Global(global))),
        )
    };
    let views = row![pick("All trails", true), pick("Trails", false)].spacing(6);
    let kind = |label: &str, k: Option<Which>| {
        choice(label, state.kind == k, Some(Message::Rules(Msg::Kind(k))))
    };
    let kinds = Which::ALL.iter().map(|k| kind(k.label(), Some(*k)));
    let kinds = row![kind("Everything", None)].extend(kinds).spacing(6);
    let shown_kinds: Vec<Which> = state.kind.map_or(Which::ALL.to_vec(), |k| vec![k]);
    let shown = ctx.shown();
    let named = shown.len() > 1;
    let sections = shown.into_iter().filter_map(|name| {
        let s = state.servers.get(&name)?;
        let page = step_rules::page(&s.rules, &shown_kinds, state.global, s.admin);
        let page = page.map(on(name.clone()));
        let page = column![learned(s, &name), page].spacing(10);
        let head = named.then(|| text(name).size(18).font(theme::BOLD));
        let head = head.map(Element::from);
        Some(Column::new().spacing(10).extend(head).push(page).into())
    });
    let about = "The guide that takes each journey up the trail: what the agent is told, what checks its work, and what its calls may do.";
    let body = column![super::head("The guide", about, vec![]), kinds, views]
        .extend(sections)
        .spacing(12);
    scrolled(body)
}
