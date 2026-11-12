//! The Harness: what governs an agent, by kind, each item on for all
//! workflows or at the workflows and steps you pick. On All servers each
//! server has its section, and a change goes to that server (D58).

use std::collections::BTreeMap;

use bastion_protocol::api;
use bastion_protocol::{ConstraintView, FieldNoteView, PersonView, WorkflowView};
use iced::widget::{Column, column, row, text, text_input};
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
    /// Why the last note could not become guidance or a check.
    adopt_error: String,
    /// The check being written from a note, when one is.
    draft: Option<Draft>,
}

/// A check being written from a note: the calls it binds, by a function
/// pattern and the words the call holds.
#[derive(Clone, Debug, Default)]
struct Draft {
    note: FieldNoteView,
    function: String,
    contains: String,
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
    /// Make a note about Bastion guidance at its journey's waypoint.
    Adopt(String, FieldNoteView),
    /// Start writing a check from a note, or stop.
    Draft(String, Option<FieldNoteView>),
    /// The check's function pattern, or the words its call holds.
    DraftCall(String, String),
    DraftWords(String, String),
    /// Add the check being written.
    AddCheck(String),
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

/// Where what a note taught applies: the waypoint its journey was at, its
/// trail when the step is not on record, or every trail.
fn waypoint(n: &FieldNoteView) -> String {
    match (n.workflow.as_str(), n.step.as_str()) {
        ("", _) => "*".into(),
        (w, "") => w.into(),
        (w, step) => format!("{w}/{step}"),
    }
}

/// The waypoint in words, for a button: the trail by its name in `trails`.
fn waypoint_words(n: &FieldNoteView, trails: &[WorkflowView]) -> String {
    let trail = trails.iter().find(|w| w.id == n.workflow);
    let name = trail.map_or(n.workflow.as_str(), |w| w.name.as_str());
    match waypoint(n).as_str() {
        "*" => "every trail".into(),
        _ if n.step.is_empty() => name.into(),
        _ => format!("{name} › {}", n.step),
    }
}

/// A note about Bastion as guidance: a rule the agent is told, at the
/// note's waypoint.
fn guidance(n: &FieldNoteView) -> ConstraintView {
    ConstraintView {
        id: format!("learned-{}-{}", n.job, n.at),
        rule: n.text.clone(),
        rung: "told".into(),
        source: "owner".into(),
        steps: vec![waypoint(n)],
        ..ConstraintView::default()
    }
}

/// A note about Bastion as a check: enforced on the calls the draft names,
/// at the note's waypoint.
fn check(d: &Draft) -> ConstraintView {
    ConstraintView {
        id: format!("learned-check-{}-{}", d.note.job, d.note.at),
        function: d.function.trim().into(),
        contains: d.contains.trim().into(),
        rung: "enforced".into(),
        ..guidance(&d.note)
    }
}

/// Add `c`, made from a note, on server `name`.
fn adopt(ctx: &Ctx, name: String, c: ConstraintView) -> Task<Message> {
    let who = name.clone();
    let here = ctx.on(&name);
    Task::perform(here.get(name, api::ConstraintAdd(c)), move |r| {
        Message::Rules(Msg::Adopted(who.clone(), r))
    })
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
        Msg::Draft(name, note) => {
            let s = state.servers.entry(name).or_default();
            s.draft = note.map(|note| Draft {
                note,
                ..Draft::default()
            });
        }
        Msg::DraftCall(name, v) => {
            if let Some(d) = &mut state.servers.entry(name).or_default().draft {
                d.function = v;
            }
        }
        Msg::DraftWords(name, v) => {
            if let Some(d) = &mut state.servers.entry(name).or_default().draft {
                d.contains = v;
            }
        }
        Msg::AddCheck(name) => {
            let s = state.servers.entry(name.clone()).or_default();
            let Some(d) = s.draft.take() else {
                return Task::none();
            };
            return adopt(ctx, name, check(&d));
        }
        Msg::Adopt(name, note) => return adopt(ctx, name, guidance(&note)),
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

/// What a person may do with a note: make it guidance or a check at its
/// waypoint, or nothing when it is already in the Guide.
fn note_actions<'a>(s: &'a Server, server: &str, n: &FieldNoteView) -> Element<'a, Message> {
    if s.rules.has_rule(&n.text) {
        return text("In the Guide now").size(12).color(theme::ok()).into();
    }
    if !s.admin {
        return text("").into();
    }
    let at = waypoint_words(n, s.rules.workflows());
    let press = |label: String, msg: Msg| {
        crate::lift::button(text(label).size(13))
            .style(theme::secondary)
            .on_press(Message::Rules(msg))
    };
    row![
        press(
            format!("Guidance at {at}"),
            Msg::Adopt(server.into(), n.clone())
        ),
        press(
            format!("A check at {at}"),
            Msg::Draft(server.into(), Some(n.clone()))
        ),
    ]
    .spacing(6)
    .into()
}

/// The form a check from a note is written in: the calls it binds.
fn check_form<'a>(server: &str, d: &'a Draft) -> Element<'a, Message> {
    let (on_call, on_words) = (server.to_string(), server.to_string());
    let call = text_input("The call it binds, such as sandbox::exec", &d.function)
        .on_input(move |v| Message::Rules(Msg::DraftCall(on_call.clone(), v)))
        .size(13);
    let words = text_input("Words the call holds, such as git push", &d.contains)
        .on_input(move |v| Message::Rules(Msg::DraftWords(on_words.clone(), v)))
        .size(13);
    let ready = !d.function.trim().is_empty();
    let add = crate::lift::button(text("Add the check").size(13))
        .style(theme::primary)
        .on_press_maybe(ready.then_some(Message::Rules(Msg::AddCheck(server.into()))));
    let cancel = crate::lift::button(text("Cancel").size(13))
        .style(theme::link)
        .on_press(Message::Rules(Msg::Draft(server.into(), None)));
    column![call, words, row![add, cancel].spacing(6)]
        .spacing(6)
        .into()
}

/// The learning loop (D61): what journeys taught about Bastion, each one a
/// press away from being guidance or a check at the waypoint it came from.
fn learned<'a>(s: &'a Server, server: &str) -> Element<'a, Message> {
    let rows = s.learned.iter().map(|n| {
        let whence = format!("{} · {} · {}", n.by, n.request, super::ago(n.at));
        let words = column![
            text(n.text.clone()).size(13),
            text(whence).size(12).color(theme::muted())
        ]
        .spacing(2)
        .width(iced::Fill);
        let line = row![words, note_actions(s, server, n)]
            .spacing(10)
            .align_y(iced::Center);
        let drafting = s
            .draft
            .as_ref()
            .filter(|d| d.note.job == n.job && d.note.at == n.at);
        let form = drafting.map(|d| check_form(server, d));
        column![line].extend(form).spacing(8).into()
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
        text(
            "What journeys taught about Bastion. Make one guidance or a check where it was learned."
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_becomes_guidance_or_a_check_at_its_waypoint() {
        let note = FieldNoteView {
            job: "j1".into(),
            at: 7,
            text: "Never push from a plan step.".into(),
            workflow: "bug-fix".into(),
            step: "plan".into(),
            ..FieldNoteView::default()
        };
        let told = guidance(&note);
        assert_eq!(
            (told.rung.as_str(), told.steps.clone()),
            ("told", vec!["bug-fix/plan".to_string()])
        );
        assert_eq!(waypoint_words(&note, &[]), "bug-fix › plan");
        let named = WorkflowView {
            id: "bug-fix".into(),
            name: "Bug fix".into(),
            ..WorkflowView::default()
        };
        assert_eq!(waypoint_words(&note, &[named]), "Bug fix › plan");
        let d = Draft {
            note: note.clone(),
            function: " sandbox::exec ".into(),
            contains: "git push".into(),
        };
        let c = check(&d);
        assert_eq!(c.id, "learned-check-j1-7");
        assert_eq!(
            (c.function.as_str(), c.contains.as_str(), c.rung.as_str()),
            ("sandbox::exec", "git push", "enforced")
        );
        assert_eq!(c.steps, ["bug-fix/plan"]);
        let old = FieldNoteView {
            step: String::new(),
            ..note.clone()
        };
        assert_eq!(
            waypoint(&old),
            "bug-fix",
            "a note from before waypoints: its trail"
        );
        let none = FieldNoteView {
            workflow: String::new(),
            ..note
        };
        assert_eq!(waypoint_words(&none, &[]), "every trail");
    }
}
