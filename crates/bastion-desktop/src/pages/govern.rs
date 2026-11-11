//! The governance pages: Constraints and Capabilities, each on its ladder
//! with promote and demote. A constraint's history comes from the audit.

use bastion_protocol::api::{self, Request};
use bastion_protocol::{
    AuditView, CapabilityView, ConstraintView, FactoryCapabilityView, WorkspaceCharterView,
};
use iced::widget::{Column, button, column, container, pick_list, row, text, text_input};
use iced::{Element, Fill, Task};

use super::{Ctx, ago, labeled, scrolled, section};
use crate::app::Message;
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Which {
    Constraints,
    Capabilities,
}

/// The fields of a new constraint.
#[derive(Clone, Copy, Debug)]
pub enum Field {
    Id,
    Rule,
    Function,
    Contains,
}

#[derive(Default)]
struct Draft {
    id: String,
    rule: String,
    function: String,
    contains: String,
    rung: String,
    open: bool,
}

pub struct State {
    which: Which,
    constraints: Vec<ConstraintView>,
    grants: Vec<CapabilityView>,
    audit: Option<AuditView>,
    charters: Vec<WorkspaceCharterView>,
    factory: Vec<FactoryCapabilityView>,
    error: String,
    /// The constraint whose history is open.
    history: Option<String>,
    draft: Draft,
}

impl State {
    pub fn new(which: Which) -> Self {
        State {
            which,
            constraints: vec![],
            grants: vec![],
            audit: None,
            charters: vec![],
            factory: vec![],
            error: String::new(),
            history: None,
            draft: Draft {
                rung: "told".into(),
                ..Draft::default()
            },
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Constraints(Result<Vec<ConstraintView>, String>),
    Grants(Result<Vec<CapabilityView>, String>),
    Audit(Result<AuditView, String>),
    Charters(Result<Vec<WorkspaceCharterView>, String>),
    Factory(Result<Vec<FactoryCapabilityView>, String>),
    MoveConstraint(String, bool),
    MoveGrant(CapabilityView, bool),
    Done(Result<(), String>),
    History(String),
    Edit(Field, String),
    Rung(String),
    ToggleForm,
    Add,
}

/// Read what the page shows from the server it shows.
pub fn load(state: &State, ctx: &Ctx) -> Task<Message> {
    let on = &ctx.current;
    let audit = ctx.task(on, api::Audit, Msg::Audit);
    let tasks = match state.which {
        Which::Constraints => vec![
            ctx.task(on, api::Constraints, Msg::Constraints),
            ctx.task(on, api::Charters, Msg::Charters),
            audit,
        ],
        Which::Capabilities => vec![
            ctx.task(on, api::Capabilities, Msg::Grants),
            ctx.task(on, api::Charters, Msg::Charters),
            ctx.task(on, api::FactoryCapabilities, Msg::Factory),
        ],
    };
    Task::batch(tasks).map(Message::Govern)
}

/// Send a write to the server shown, and read again when it lands.
fn send<R>(ctx: &Ctx, request: R) -> Task<Message>
where
    R: Request<Answer = ()> + Send + Sync + 'static,
{
    ctx.task(&ctx.current, request, Msg::Done)
        .map(Message::Govern)
}

/// The owner's new constraint, as the server takes it.
fn new_constraint(d: &Draft) -> api::ConstraintAdd {
    api::ConstraintAdd(ConstraintView {
        id: d.id.trim().into(),
        rule: d.rule.trim().into(),
        function: d.function.trim().into(),
        contains: d.contains.trim().into(),
        rung: d.rung.clone(),
        source: "owner".into(),
        layer: String::new(),
    })
}

fn edit(d: &mut Draft, field: Field, value: String) {
    match field {
        Field::Id => d.id = value,
        Field::Rule => d.rule = value,
        Field::Function => d.function = value,
        Field::Contains => d.contains = value,
    }
}

/// Note what a read answered, or why it failed.
fn keep<T>(slot: &mut T, error: &mut String, answer: Result<T, String>) {
    match answer {
        Ok(v) => *slot = v,
        Err(e) => *error = e,
    }
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    let e = &mut state.error;
    match msg {
        Msg::Constraints(a) => keep(&mut state.constraints, e, a),
        Msg::Grants(a) => keep(&mut state.grants, e, a),
        Msg::Audit(a) => keep(&mut state.audit, e, a.map(Some)),
        Msg::Charters(a) => keep(&mut state.charters, e, a),
        Msg::Factory(a) => keep(&mut state.factory, e, a),
        Msg::MoveConstraint(id, up) => {
            return send(ctx, api::ConstraintMove { id, up });
        }
        Msg::MoveGrant(g, up) => {
            let moved = api::CapabilityMove {
                stage: g.stage,
                function: g.function,
                up,
            };
            return send(ctx, moved);
        }
        Msg::Done(Ok(_)) => {
            state.error.clear();
            return load(state, ctx);
        }
        Msg::Done(Err(why)) => state.error = why,
        Msg::History(id) => state.history = (state.history.as_ref() != Some(&id)).then_some(id),
        Msg::Edit(field, value) => edit(&mut state.draft, field, value),
        Msg::Rung(r) => state.draft.rung = r,
        Msg::ToggleForm => state.draft.open = !state.draft.open,
        Msg::Add => {
            let added = send(ctx, new_constraint(&state.draft));
            state.draft = Draft {
                rung: state.draft.rung.clone(),
                open: true,
                ..Draft::default()
            };
            return added;
        }
    }
    Task::none()
}

fn rung<'a>(name: &str) -> Element<'a, Message> {
    container(text(name.to_string()).size(12))
        .width(72)
        .center_x(72)
        .padding([2, 8])
        .style(theme::chip)
        .into()
}

fn small<'a>(label: &'a str, msg: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(13))
        .padding([2, 8])
        .style(theme::secondary)
        .on_press_maybe(msg)
        .into()
}

/// A rule or a grant on its ladder: its rung, what it is, and its buttons.
fn ladder_row<'a>(
    rung_name: &str,
    about: Element<'a, Message>,
    buttons: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let line = row![rung(rung_name), container(about).width(Fill)].push(row(buttons).spacing(6));
    container(line.spacing(12).align_y(iced::Center))
        .padding([8, 12])
        .into()
}

fn history<'a>(state: &State, id: &str) -> Element<'a, Message> {
    let entries = state
        .audit
        .iter()
        .flat_map(|a| &a.entries)
        .filter(|e| e.subject == id);
    let lines: Vec<Element<'a, Message>> = entries
        .rev()
        .map(|e| {
            text(format!(
                "#{} · {} · {} {}",
                e.seq,
                ago(e.at),
                e.kind,
                e.detail
            ))
            .size(12)
            .color(theme::MUTED)
            .into()
        })
        .collect();
    let lines = if lines.is_empty() {
        vec![
            text("No changes recorded yet.")
                .size(12)
                .color(theme::MUTED)
                .into(),
        ]
    } else {
        lines
    };
    Column::with_children(lines)
        .padding(iced::Padding::from([0, 12]).left(24).bottom(8))
        .into()
}

fn constraint_row<'a>(state: &State, c: &ConstraintView) -> Element<'a, Message> {
    let binds = if c.function.is_empty() {
        "binds no call".to_string()
    } else {
        format!("binds {} with the words {:?}", c.function, c.contains)
    };
    let about = column![
        text(c.rule.clone()),
        text(format!("{} · {} · {binds}", c.id, c.source))
            .size(12)
            .color(theme::MUTED)
    ];
    let go = |up: bool| Message::Govern(Msg::MoveConstraint(c.id.clone(), up));
    let buttons = vec![
        small("History", Some(Message::Govern(Msg::History(c.id.clone())))),
        small("Demote", (c.rung != "told").then(|| go(false))),
        small("Promote", (c.rung != "enforced").then(|| go(true))),
    ];
    let line = ladder_row(&c.rung, about.into(), buttons);
    if state.history.as_deref() == Some(&c.id) {
        column![line, history(state, &c.id)].into()
    } else {
        line
    }
}

fn layer_title(layer: &str) -> &'static str {
    if layer == "factory" {
        "Factory: rules on the pipeline, checked at review"
    } else {
        "Harness: rules on an agent's calls"
    }
}

fn layer<'a>(state: &State, name: &str) -> Element<'a, Message> {
    let rows = state
        .constraints
        .iter()
        .filter(|c| c.layer == name)
        .map(|c| constraint_row(state, c));
    section(
        layer_title(name).into(),
        Column::with_children(rows).into(),
        false,
    )
}

/// Lines under a label, or nothing when there are none.
fn lines<'a>(label: &'a str, items: &[String]) -> Option<Element<'a, Message>> {
    if items.is_empty() {
        return None;
    }
    let each = items.iter().map(|i| text(format!("• {i}")).size(13).into());
    Some(
        column![
            text(label).size(12).color(theme::MUTED),
            Column::with_children(each)
        ]
        .spacing(2)
        .into(),
    )
}

fn charter_rules(state: &State) -> Element<'_, Message> {
    let rows = state.charters.iter().map(|c| {
        let mut head = row![text(c.workspace.clone()).font(theme::BOLD)];
        if c.files.is_empty() {
            head = head.push(
                text(" · no charter: the owner's rules apply")
                    .size(13)
                    .color(theme::MUTED),
            );
        }
        let body = column![head]
            .extend(lines("Rules (enforced)", &c.rules))
            .extend(lines("Hooks (exit 2 refuses the call)", &c.hooks));
        container(body.spacing(4)).padding([8, 12]).into()
    });
    section(
        "Charter: each repository's own rules, from .claude/settings.json".into(),
        Column::with_children(rows).into(),
        false,
    )
}

fn add_form(state: &State) -> Element<'_, Message> {
    let arrow = if state.draft.open { "▾" } else { "▸" };
    let toggle = button(
        text(format!("{arrow} Add a constraint"))
            .size(13)
            .font(theme::BOLD),
    )
    .style(theme::link)
    .on_press(Message::Govern(Msg::ToggleForm));
    if !state.draft.open {
        return container(toggle)
            .padding(8)
            .width(Fill)
            .style(theme::panel)
            .into();
    }
    let d = &state.draft;
    let field =
        |label: &'static str, hint: &'static str, value: &str, f: Field| -> Element<'_, Message> {
            let input = text_input(hint, value)
                .on_input(move |v| Message::Govern(Msg::Edit(f, v)))
                .style(theme::field);
            labeled(label, input)
        };
    let rungs = vec!["told".to_string(), "checked".into(), "enforced".into()];
    let pick = pick_list(rungs, Some(d.rung.clone()), |r| {
        Message::Govern(Msg::Rung(r))
    })
    .style(theme::select);
    let body = column![
        toggle,
        row![
            field("Id", "no-curl", &d.id, Field::Id),
            field(
                "Rule the agent is told",
                "Do not fetch from the network.",
                &d.rule,
                Field::Rule
            )
        ]
        .spacing(12),
        row![
            field(
                "Calls it binds (empty binds none)",
                "sandbox::exec",
                &d.function,
                Field::Function
            ),
            field(
                "Words the call holds, in order",
                "curl",
                &d.contains,
                Field::Contains
            )
        ]
        .spacing(12),
        row![
            labeled("Rung", pick),
            small("Add", Some(Message::Govern(Msg::Add)))
        ]
        .spacing(12)
        .align_y(iced::Bottom),
    ];
    container(body.spacing(10))
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}

fn constraints(state: &State) -> Element<'_, Message> {
    column![
        charter_rules(state),
        layer(state, "harness"),
        layer(state, "factory"),
        add_form(state)
    ]
    .spacing(16)
    .into()
}

fn grant_row<'a>(g: &CapabilityView) -> Element<'a, Message> {
    let about = row![
        text(g.stage.replace('_', " ")),
        text(format!(" · {}", g.function))
    ];
    let go = |up: bool| Message::Govern(Msg::MoveGrant(g.clone(), up));
    let buttons = vec![
        small("Demote", (g.rung != "denied").then(|| go(false))),
        small("Promote", (g.rung != "allowed").then(|| go(true))),
    ];
    ladder_row(&g.rung, about.into(), buttons)
}

fn capabilities(state: &State) -> Element<'_, Message> {
    let allows = state.charters.iter().map(|c| {
        let mut head = row![text(c.workspace.clone()).font(theme::BOLD)];
        if c.allow.is_empty() {
            head = head.push(
                text(" · allows nothing beyond the harness")
                    .size(13)
                    .color(theme::MUTED),
            );
        }
        let body = column![head].extend(lines(
            "Allowed by the repository; the harness's grants still decide",
            &c.allow,
        ));
        container(body.spacing(4)).padding([8, 12]).into()
    });
    let grants = state.grants.iter().map(grant_row);
    let factory = state.factory.iter().map(|f| {
        let about = column![
            text(f.name.clone()),
            text(f.how.clone()).size(12).color(theme::MUTED)
        ];
        ladder_row(&f.rung, about.into(), vec![])
    });
    column![
        section("Charter: what each repository allows".into(), Column::with_children(allows).into(), false),
        section("Harness: what an agent's calls may do in each kind of step: denied, then ask (held for you), then allowed".into(), Column::with_children(grants).into(), false),
        section("Factory: what the factory may do, as Settings set it".into(), Column::with_children(factory).into(), false),
    ]
    .spacing(16)
    .into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let title = match state.which {
        Which::Constraints => "Constraints",
        Which::Capabilities => "Capabilities",
    };
    let body = match state.which {
        Which::Constraints => constraints(state),
        Which::Capabilities => capabilities(state),
    };
    let about = match state.which {
        Which::Constraints => format!(
            "The rules an agent's work must keep on {}. Each climbs told, then checked (recorded), then enforced (refused).",
            ctx.current
        ),
        Which::Capabilities => format!(
            "What an agent may call on {}, by the kind of step it runs in.",
            ctx.current
        ),
    };
    scrolled(
        column![super::head(title, about, vec![])]
            .extend(super::problem(&state.error))
            .push(body)
            .spacing(12),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_constraint_is_the_owners_and_trimmed() {
        let mut d = Draft {
            rung: "checked".into(),
            ..Draft::default()
        };
        edit(&mut d, Field::Id, " no-curl ".into());
        edit(&mut d, Field::Contains, "curl".into());
        let c = new_constraint(&d).0;
        assert_eq!(
            (c.id.as_str(), c.source.as_str(), c.rung.as_str()),
            ("no-curl", "owner", "checked")
        );
    }
}
