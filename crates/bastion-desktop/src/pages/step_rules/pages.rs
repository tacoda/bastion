//! The rules pages (D57): Constraints, Capabilities and Permissions. Each
//! has an All workflows view, where an item is on or off for every
//! workflow, and a Workflows view, where an item's list picks the
//! workflows and steps it applies to. Nothing is global (D58).

use bastion_protocol::{CapabilityView, ConstraintView};
use iced::widget::{Column, column, container, row, text, text_input};
use iced::{Element, Fill};

use super::applies::{steps_on, tree};
use super::capabilities::{granting, line as grant_line};
use super::constraints::{
    badge, check_form, hook_form, kind, mode, note_form, reviewer_form, rows, rule_form,
};
use super::layers::{KINDS, factory_line, heading, section};
use super::{Adding, At, EVERY, Item, Msg, Rules, choice, help, small};
use crate::theme;

/// Which rules a page holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Constraints,
    Capabilities,
    Permissions,
}

impl Which {
    /// The page's route.
    pub fn route(self) -> crate::route::Route {
        use crate::route::Route;
        match self {
            Which::Constraints => Route::Constraints,
            Which::Capabilities => Route::Capabilities,
            Which::Permissions => Route::Permissions,
        }
    }
}

/// Where a new item goes: on for all workflows in the All workflows view; off
/// everywhere in the Workflows view, until its list picks steps.
const OFF: &str = "!*";

/// What a page shows: its rules, in the view picked, and whether you may
/// change them.
#[derive(Clone, Copy)]
struct Shown<'a> {
    rules: &'a Rules,
    global: bool,
    editable: bool,
}

/// The level a page acts at.
fn at(global: bool) -> At<'static> {
    let scope = if global { EVERY } else { OFF };
    At { scope, stage: "" }
}

/// An item's row in the Workflows view: what it is, how many steps it is
/// on at, and its list when open.
fn placed<'a>(
    v: Shown<'a>,
    item: Item,
    head: Element<'a, Msg>,
    scopes: &[String],
) -> Element<'a, Msg> {
    let (rules, editable) = (v.rules, v.editable);
    let (on, all) = steps_on(rules, scopes);
    let open = rules.open.as_ref() == Some(&item);
    let label = if open { "Close" } else { "Where" };
    let summary = help(format!("on at {on} of {all} steps"));
    let line = row![head, summary, small(label, Some(Msg::Open(item.clone())))]
        .spacing(8)
        .align_y(iced::Center);
    match open {
        true => column![line, tree(rules, &item, scopes, editable)]
            .spacing(6)
            .into(),
        false => line.into(),
    }
}

fn constraint_head<'a>(c: &ConstraintView) -> Element<'a, Msg> {
    row![
        badge(kind(c), theme::MUTED),
        badge(mode(c), theme::MUTED),
        text(c.rule.clone()).size(13).width(Fill)
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn grant_head<'a>(g: &CapabilityView) -> Element<'a, Msg> {
    let name = text(g.function.clone())
        .size(13)
        .font(theme::MONO)
        .width(Fill);
    row![
        badge(&g.stage, theme::MUTED),
        badge(&g.rung, theme::MUTED),
        name
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// Constraints as rows: at all workflows in the All workflows view, or with
/// their lists in the Workflows view.
fn constraint_rows<'a>(v: Shown<'a>, list: Vec<&ConstraintView>) -> Vec<Element<'a, Msg>> {
    if v.global {
        return rows(v.rules, list, &at(true), v.editable);
    }
    list.into_iter()
        .map(|c| {
            let item = Item::Constraint(c.id.clone());
            placed(v, item, constraint_head(c), &c.steps)
        })
        .collect()
}

fn constraints<'a>(v: Shown<'a>) -> Element<'a, Msg> {
    let kept = v
        .rules
        .constraints
        .iter()
        .filter(|c| !c.capability && c.layer != "charter");
    let (feedback, feedforward): (Vec<&ConstraintView>, Vec<&ConstraintView>) =
        kept.partition(|c| mode(c) == "Feedback");
    let mut body = vec![heading(
        "Feedforward",
        "told to the agent before it acts; the agent judges it",
    )];
    body.extend(constraint_rows(v, feedforward));
    body.push(heading(
        "Feedback",
        "checks what the agent did; code or a reviewer judges it",
    ));
    body.extend(constraint_rows(v, feedback));
    section("Constraints", "What the agent must keep.", body)
}

fn capabilities<'a>(v: Shown<'a>) -> Element<'a, Msg> {
    let notes: Vec<&ConstraintView> = v
        .rules
        .constraints
        .iter()
        .filter(|c| c.capability)
        .collect();
    let mut body = vec![heading(
        "Feedforward",
        "told to the agent as what it may use",
    )];
    body.extend(constraint_rows(v, notes));
    section("Capabilities", "What the agent may use.", body)
}

fn permissions<'a>(v: Shown<'a>) -> Element<'a, Msg> {
    let rules = v.rules;
    let grants = KINDS.iter().map(|(k, name)| {
        let rows = rules.kind(k).into_iter().map(|g| match v.global {
            true => grant_line(g, rules, &at(true), v.editable),
            false => {
                let item = Item::Grant(g.stage.clone(), g.function.clone());
                placed(v, item, grant_head(g), &g.scopes)
            }
        });
        Column::new()
            .spacing(6)
            .push(help(format!("{name} steps' grants")))
            .extend(rows)
            .into()
    });
    let mut body: Vec<Element<'a, Msg>> = grants.collect();
    body.extend(rules.factory.iter().map(factory_line));
    section(
        "Permissions",
        "What the gate lets a call do. A call no grant covers is refused.",
        body,
    )
}

/// The Add bar for the page's kind of item.
fn adding<'a>(rules: &'a Rules, which: Which, global: bool) -> Element<'a, Msg> {
    let pick =
        |label: &str, a: Adding| choice(label.to_string(), rules.adding == a, Some(Msg::Adding(a)));
    let choices: Vec<Element<'a, Msg>> = match which {
        Which::Constraints => vec![
            pick("A rule", Adding::Rule),
            pick("A check", Adding::Check),
            pick("A hook", Adding::Hook),
            pick("A reviewer", Adding::Reviewer),
        ],
        Which::Capabilities => vec![pick("A note", Adding::Note)],
        Which::Permissions => vec![pick("A grant", Adding::Grant)],
    };
    let switch = row![text("Add").size(13).color(theme::MUTED)]
        .extend(choices)
        .spacing(6)
        .align_y(iced::Center);
    let form = match (which, rules.adding) {
        (Which::Capabilities, _) => note_form(rules),
        (Which::Permissions, _) => grant_form(rules),
        (_, Adding::Check) => check_form(rules),
        (_, Adding::Hook) => hook_form(rules),
        (_, Adding::Reviewer) => reviewer_form(rules),
        _ => rule_form(rules),
    };
    let lands = match global {
        true => "What you add is on for every workflow.",
        false => "What you add is off until you pick where it applies.",
    };
    container(column![switch, help(lands), form].spacing(10))
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}

/// A grant for one kind, found by search, as a command, or an MCP server's.
fn grant_form<'a>(rules: &'a Rules) -> Element<'a, Msg> {
    let kind = rules.kind_of(&at(true));
    let kinds = KINDS.iter().map(|(k, name)| {
        choice(
            format!("{name} steps"),
            kind == *k,
            Some(Msg::ForKind(k.to_string())),
        )
    });
    let kinds = row![text("For").size(12).color(theme::MUTED)]
        .extend(kinds)
        .spacing(6)
        .align_y(iced::Center);
    column![kinds, granting(rules)].spacing(10).into()
}

/// One rules page: its items in the view picked, and its Add bar.
pub fn page<'a>(rules: &'a Rules, which: Which, global: bool, editable: bool) -> Element<'a, Msg> {
    let mut body = Column::new().spacing(10);
    if which != Which::Permissions {
        let filter = text_input("Filter by words or kind", &rules.filter)
            .on_input(Msg::Filter)
            .style(theme::field);
        body = body.push(filter);
    }
    let v = Shown {
        rules,
        global,
        editable,
    };
    let shown = match which {
        Which::Constraints => constraints(v),
        Which::Capabilities => capabilities(v),
        Which::Permissions => permissions(v),
    };
    body = body.push(shown);
    if editable {
        body = body.push(adding(rules, which, global));
    }
    body.extend(super::said(rules)).into()
}

/// The level a page's Add acts at, for its update.
pub fn level(global: bool) -> At<'static> {
    at(global)
}
