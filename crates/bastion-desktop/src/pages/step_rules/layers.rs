//! The rules at a level, by layer (D25, D54): the charter each repository
//! brings, the harness on the agent's calls, and the factory around the
//! work. The same view serves every workflow, one workflow, and one step;
//! each item says whether it is on there and where that comes from.

use bastion_protocol::{ConstraintView, FactoryCapabilityView, WorkspaceCharterView};
use iced::widget::{Column, column, container, row, text, text_input};
use iced::{Element, Fill};

use super::capabilities::{granting, line};
use super::constraints::{hook_form, layer, reviewer_form, rows, rule_form};
use super::{Adding, At, Msg, Rules, choice, help};
use crate::theme;

/// The kinds of step an agent runs, each with grants of its own.
const KINDS: [(&str, &str); 3] = [("prompt", "Prompt"), ("agent", "Agent"), ("skill", "Skill")];

/// The list narrows to a filter once a level has more constraints than this.
const FILTER_FROM: usize = 5;

/// A layer's section: its title, what it covers, and its rows.
fn section<'a>(title: &'a str, about: &'a str, body: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    let head = row![
        text(title).size(12).font(theme::BOLD).color(theme::INK),
        text(about).size(12).color(theme::MUTED),
    ]
    .spacing(10)
    .align_y(iced::Center);
    let body = Column::with_children(body).spacing(8);
    container(column![head, body].spacing(10))
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}

/// One workspace's charter: what its repository brings, read-only here.
fn charter_line<'a>(c: &WorkspaceCharterView) -> Element<'a, Msg> {
    let counts = format!("{} rules, {} hooks", c.rules.len(), c.hooks.len());
    let head = row![text(c.workspace.clone()).size(13), help(counts)].spacing(8);
    let items = c
        .rules
        .iter()
        .chain(&c.hooks)
        .map(|r| help(format!("  {r}")));
    Column::new().spacing(2).push(head).extend(items).into()
}

/// The charter: each workspace's, and any charter constraint the server keeps.
fn charter<'a>(rules: &'a Rules, kept: Vec<&ConstraintView>, at: &At) -> Element<'a, Msg> {
    let charters = rules.charters.iter().filter(|c| !c.files.is_empty());
    let mut body: Vec<Element<'a, Msg>> = charters.map(charter_line).collect();
    body.extend(rows(rules, kept, at, false));
    if body.is_empty() {
        body.push(help(
            "No workspace's repository has AGENTS.md or .agents/settings.json.",
        ));
    }
    section(
        "Charter",
        "From each workspace's repository. Change it there.",
        body,
    )
}

/// The grants at the level: the step's kind's at a step, else each kind's.
fn grants<'a>(rules: &'a Rules, at: &At, editable: bool) -> Vec<Element<'a, Msg>> {
    let kinds = KINDS
        .iter()
        .filter(|(k, _)| !at.is_step() || *k == at.stage);
    kinds
        .map(|(kind, name)| {
            let head = help(format!("{name} steps' grants"));
            let rows = rules
                .kind(kind)
                .into_iter()
                .map(|g| line(g, rules, at, editable));
            Column::new().spacing(6).push(head).extend(rows).into()
        })
        .collect()
}

/// What the factory itself may do, as its workspace and steps set it.
fn factory_line<'a>(f: &FactoryCapabilityView) -> Element<'a, Msg> {
    let what = column![text(f.name.clone()).size(13), help(f.how.clone())]
        .spacing(2)
        .width(Fill);
    row![what, super::chip(f.rung.clone())]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// One form at a time to add an item on at the level, behind a switch.
fn adding<'a>(rules: &'a Rules, at: &At) -> Element<'a, Msg> {
    let pick =
        |label: &str, a: Adding| choice(label.to_string(), rules.adding == a, Some(Msg::Adding(a)));
    let switch = row![
        text("Add").size(13).color(theme::MUTED),
        pick("A grant", Adding::Grant),
        pick("A rule", Adding::Rule),
        pick("A hook", Adding::Hook),
        pick("A reviewer", Adding::Reviewer),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let form = match rules.adding {
        Adding::Grant if at.is_step() => granting(rules),
        Adding::Grant => {
            let kind = rules.kind_of(at);
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
        Adding::Rule => rule_form(rules),
        Adding::Hook => hook_form(rules),
        Adding::Reviewer => reviewer_form(rules),
    };
    container(
        column![
            switch,
            help("What you add is on here, and inherited below."),
            form
        ]
        .spacing(10),
    )
    .padding(12)
    .width(Fill)
    .style(theme::panel)
    .into()
}

/// The rules at the level `at`, by layer, and the Add bar.
pub fn view<'a>(rules: &'a Rules, at: &At, editable: bool) -> Element<'a, Msg> {
    let all: Vec<&ConstraintView> = rules.constraints.iter().collect();
    let of = |name: &str| {
        all.iter()
            .copied()
            .filter(|c| layer(c) == name)
            .collect::<Vec<_>>()
    };
    let mut body = Column::new().spacing(10);
    if all.len() > FILTER_FROM {
        let filter = text_input(
            "Filter the constraints by words, kind or rung",
            &rules.filter,
        )
        .on_input(Msg::Filter)
        .style(theme::field);
        body = body.push(filter);
    }
    let mut harness = grants(rules, at, editable);
    harness.extend(rows(rules, of("harness"), at, editable));
    let mut factory = rows(rules, of("factory"), at, editable);
    if at.is_every() {
        factory.extend(rules.factory.iter().map(factory_line));
    }
    body = body
        .push(charter(rules, of("charter"), at))
        .push(section(
            "Harness",
            "The agent's calls: what it may call, and the rules and hooks on them.",
            harness,
        ))
        .push(section(
            "Factory",
            "The work around the calls: checks on the change, and reviewers that must pass.",
            factory,
        ));
    if editable {
        body = body.push(adding(rules, at));
    }
    body.extend(super::said(rules)).into()
}
