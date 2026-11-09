//! The governance pages: Constraints and Capabilities, each on
//! its ladder with promote and demote, and the Audit Log with its check.

use bastion_protocol::{
    AuditView, CapabilityView, ConstraintView, FactoryCapabilityView, WorkspaceCharterView, command,
};
use iced::widget::{Column, button, column, container, pick_list, row, text, text_input};
use iced::{Color, Element, Fill, Task};
use serde_json::{Value, json};

use super::{Ctx, ago, labeled, scrolled, section};
use crate::app::Message;
use crate::client::Ask;
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Which {
    Constraints,
    Capabilities,
    Audit,
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
    exported: String,
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
            exported: String::new(),
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
    Done(Result<Value, String>),
    History(String),
    Edit(Field, String),
    Rung(String),
    ToggleForm,
    Add,
    Export,
    Exported(Result<String, String>),
}

/// Read what the page shows from the server it shows.
pub fn load(state: &State, ctx: &Ctx) -> Task<Message> {
    let on = &ctx.current;
    let audit = ctx.task(on, Ask::bare(command::AUDIT), Msg::Audit);
    let tasks = match state.which {
        Which::Constraints => vec![
            ctx.task(on, Ask::bare(command::CONSTRAINTS), Msg::Constraints),
            ctx.task(on, Ask::bare(command::CHARTERS), Msg::Charters),
            audit,
        ],
        Which::Capabilities => vec![
            ctx.task(on, Ask::bare(command::CAPABILITIES), Msg::Grants),
            ctx.task(on, Ask::bare(command::CHARTERS), Msg::Charters),
            ctx.task(on, Ask::bare(command::FACTORY_CAPABILITIES), Msg::Factory),
        ],
        Which::Audit => vec![audit],
    };
    Task::batch(tasks).map(Message::Govern)
}

fn send(ctx: &Ctx, cmd: &'static str, args: Value) -> Task<Message> {
    ctx.task(&ctx.current, Ask { cmd, args }, Msg::Done)
        .map(Message::Govern)
}

/// The owner's new constraint, as the server takes it.
fn new_constraint(d: &Draft) -> Value {
    json!({ "constraint": {
        "id": d.id.trim(), "rule": d.rule.trim(), "function": d.function.trim(),
        "contains": d.contains.trim(), "rung": d.rung, "source": "owner", "layer": "",
    }})
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
            return send(ctx, command::CONSTRAINT_MOVE, json!({ "id": id, "up": up }));
        }
        Msg::MoveGrant(g, up) => {
            return send(
                ctx,
                command::CAPABILITY_MOVE,
                json!({ "stage": g.stage, "function": g.function, "up": up }),
            );
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
            let added = send(ctx, command::CONSTRAINT_ADD, new_constraint(&state.draft));
            state.draft = Draft {
                rung: state.draft.rung.clone(),
                open: true,
                ..Draft::default()
            };
            return added;
        }
        Msg::Export => {
            return ctx
                .task(
                    &ctx.current,
                    Ask::bare(command::AUDIT_EXPORT),
                    Msg::Exported,
                )
                .map(Message::Govern);
        }
        Msg::Exported(answer) => {
            state.exported = answer.map_or_else(|e| e, |path| format!("Written to {path}"))
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
    let lede = "Each rule climbs told, then checked (recorded), then enforced (refused). The charter's rules come from each repository; the harness binds an agent's turn; the factory binds the pipeline.";
    column![
        text(lede).size(13).color(theme::MUTED),
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
        section("Harness: what an agent's calls may do per stage: denied, then ask (held for you), then allowed".into(), Column::with_children(grants).into(), false),
        section("Factory: what the factory may do, as Settings set it".into(), Column::with_children(factory).into(), false),
    ]
    .spacing(16)
    .into()
}

/// Whether the chain verifies, and whether its writer keeps up.
fn audit_status(a: &AuditView) -> (String, Color, Option<(String, Color)>) {
    let (status, color) = match a.broken_at {
        None => (
            format!(
                "Verified: all {} entries chain from the first.",
                a.entries.len()
            ),
            theme::OK,
        ),
        Some(seq) => (
            format!("Broken: entry {seq} does not follow from the one before it."),
            theme::BAD,
        ),
    };
    let writer = match (a.pending, &a.last_error) {
        (_, Some(e)) => Some((format!("The audit writer is failing: {e}"), theme::BAD)),
        (0, None) => None,
        (n, None) => Some((format!("{n} records waiting to be written."), theme::WARN)),
    };
    (status, color, writer)
}

fn audit(state: &State) -> Element<'_, Message> {
    let head = row![
        text("Audit")
            .size(13)
            .font(theme::BOLD)
            .color(theme::MUTED)
            .width(Fill),
        small("Export", Some(Message::Govern(Msg::Export)))
    ];
    let mut body = column![
        container(head.align_y(iced::Center)).padding([8, 12]),
        theme::rule()
    ];
    body =
        body.push(container(text(&state.exported).size(12).color(theme::MUTED)).padding([0, 12]));
    if let Some(a) = &state.audit {
        let (status, color, writer) = audit_status(a);
        body = body.push(container(text(status).font(theme::BOLD).color(color)).padding([8, 12]));
        body =
            body.extend(writer.map(|(t, c)| container(text(t).color(c)).padding([0, 12]).into()));
        let rows = a.entries.iter().rev().map(|e| {
            let what = column![
                text(format!("{} · {}", e.subject, e.detail)),
                text(e.hash.chars().take(12).collect::<String>())
                    .size(12)
                    .color(theme::MUTED)
            ];
            let line = row![
                text(format!("#{}", e.seq)).color(theme::MUTED).width(64),
                text(ago(e.at)).color(theme::MUTED).width(96),
                text(e.kind.clone()).width(112),
                what.width(Fill)
            ];
            container(line.spacing(8)).padding([6, 12]).into()
        });
        body = body.push(Column::with_children(rows));
    }
    container(body).width(Fill).style(theme::panel).into()
}

pub fn view(state: &State) -> Element<'_, Message> {
    let title = match state.which {
        Which::Constraints => "Constraints",
        Which::Capabilities => "Capabilities",
        Which::Audit => "Audit Log",
    };
    let body = match state.which {
        Which::Constraints => constraints(state),
        Which::Capabilities => capabilities(state),
        Which::Audit => audit(state),
    };
    scrolled(
        column![
            text(title).size(24).font(theme::BOLD),
            text(&state.error).color(theme::BAD),
            body
        ]
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
        let c = &new_constraint(&d)["constraint"];
        assert_eq!(
            (c["id"].as_str(), c["source"].as_str(), c["rung"].as_str()),
            (Some("no-curl"), Some("owner"), Some("checked"))
        );
    }

    #[test]
    fn the_audit_says_whether_it_verifies_and_whether_the_writer_keeps_up() {
        let a = AuditView {
            entries: vec![],
            broken_at: None,
            pending: 2,
            last_error: None,
        };
        let (status, _, writer) = audit_status(&a);
        assert!(status.starts_with("Verified: all 0"));
        assert_eq!(writer.unwrap().0, "2 records waiting to be written.");
        let broken = AuditView {
            broken_at: Some(7),
            pending: 0,
            last_error: Some("disk".into()),
            ..a
        };
        let (status, _, writer) = audit_status(&broken);
        assert!(status.starts_with("Broken: entry 7"));
        assert!(writer.unwrap().0.ends_with("disk"));
    }
}
