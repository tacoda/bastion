//! A step's Rules tab, by layer (D25): the charter the repository brings,
//! the harness on the agent's calls, and the factory around the work. One
//! form at a time adds a grant, a rule, a hook, a reviewer, or a constraint
//! from another step.

use bastion_protocol::{ConstraintView, FactoryCapabilityView, WorkspaceCharterView};
use iced::widget::{Column, column, container, row, text, text_input};
use iced::{Element, Fill};

use super::capabilities::{granting, grants, line};
use super::constraints::{elsewhere_form, hook_form, layer, reviewer_form, rows, rule_form};
use super::{Adding, At, Msg, Rules, choice, help, small};
use crate::theme;

/// The list narrows to a filter once a step has more constraints than this.
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
    let mut body: Vec<Element<'a, Msg>> = rules
        .charters
        .iter()
        .filter(|c| !c.files.is_empty())
        .map(charter_line)
        .collect();
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

/// One form at a time to add something at the step, behind a switch.
fn adding<'a>(rules: &'a Rules, at: &At) -> Element<'a, Msg> {
    let pick =
        |label: &str, a: Adding| choice(label.to_string(), rules.adding == a, Some(Msg::Adding(a)));
    let switch = row![
        text("Add").size(13).color(theme::MUTED),
        pick("A grant", Adding::Grant),
        pick("A rule", Adding::Rule),
        pick("A hook", Adding::Hook),
        pick("A reviewer", Adding::Reviewer),
        pick("One from another step", Adding::Elsewhere),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let own = !rules.own(at.step).is_empty();
    let form = match rules.adding {
        Adding::Grant if own => granting(rules),
        Adding::Grant => row![
            container(help(
                "This step uses its kind's grants. Give it its own to add one."
            ))
            .width(Fill),
            small("Give it its own", Some(Msg::Own)),
        ]
        .align_y(iced::Center)
        .into(),
        Adding::Rule => rule_form(rules),
        Adding::Hook => hook_form(rules),
        Adding::Reviewer => reviewer_form(rules),
        Adding::Elsewhere => elsewhere_form(rules, at),
    };
    container(column![switch, form].spacing(10))
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}

pub fn view<'a>(rules: &'a Rules, at: &At, editable: bool) -> Element<'a, Msg> {
    let here = rules.here(at.step);
    let of = |name: &str| {
        here.iter()
            .copied()
            .filter(|c| layer(c) == name)
            .collect::<Vec<_>>()
    };
    let mut body = Column::new().spacing(10);
    if here.len() > FILTER_FROM {
        let filter = text_input(
            "Filter the constraints by words, kind or rung",
            &rules.filter,
        )
        .on_input(Msg::Filter)
        .style(theme::field);
        body = body.push(filter);
    }
    let mut harness = vec![grants(rules, at, editable)];
    harness.extend(rows(rules, of("harness"), at, editable));
    let factory = rows(rules, of("factory"), at, editable);
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
    body.into()
}

/// The kinds of step an agent runs, each with grants of its own.
const KINDS: [(&str, &str); 3] = [("prompt", "Prompt"), ("agent", "Agent"), ("skill", "Skill")];

/// Each kind's grants: what an agent at a step of that kind may call,
/// unless the step has its own.
fn kind_grants<'a>(rules: &'a Rules, editable: bool) -> Vec<Element<'a, Msg>> {
    KINDS
        .iter()
        .map(|(kind, name)| {
            let head = text(format!("{name} steps")).size(12).color(theme::MUTED);
            let rows = rules
                .kind(kind)
                .into_iter()
                .map(|g| line(g, &rules.catalog, editable));
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

/// The add bar for every step: a grant for one kind, a rule, a hook or a
/// reviewer, each for every step.
fn adding_every<'a>(rules: &'a Rules) -> Element<'a, Msg> {
    let pick =
        |label: &str, a: Adding| choice(label.to_string(), rules.adding == a, Some(Msg::Adding(a)));
    let switch = row![
        text("Add for every step").size(13).color(theme::MUTED),
        pick("A grant", Adding::Grant),
        pick("A rule", Adding::Rule),
        pick("A hook", Adding::Hook),
        pick("A reviewer", Adding::Reviewer),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let kind = rules.kind_of(&At {
        step: "",
        stage: "",
    });
    let form = match rules.adding {
        Adding::Hook => hook_form(rules),
        Adding::Reviewer => reviewer_form(rules),
        Adding::Grant | Adding::Elsewhere => {
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
    };
    container(column![switch, form].spacing(10))
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}

/// What applies at every step of every workflow: each kind's grants, the
/// rules and checks for every step, and the charters, by layer.
pub fn every_step<'a>(rules: &'a Rules, server: &str, editable: bool) -> Element<'a, Msg> {
    let at = At {
        step: "",
        stage: "",
    };
    let here = rules.here("");
    let of = |name: &str| {
        here.iter()
            .copied()
            .filter(|c| layer(c) == name && c.steps.is_empty())
            .collect::<Vec<_>>()
    };
    let mut harness = kind_grants(rules, editable);
    harness.extend(rows(rules, of("harness"), &at, editable));
    let mut factory = rows(rules, of("factory"), &at, editable);
    factory.extend(rules.factory.iter().map(factory_line));
    let head = column![
        text("Every step").size(18).font(theme::BOLD),
        help(format!("What applies at every step of every workflow on {server}, unless a step has its own grants. Pick a step on the canvas for its own rules.")),
    ]
    .spacing(2);
    let mut body = Column::new()
        .spacing(10)
        .push(head)
        .push(charter(rules, of("charter"), &at))
        .push(section("Harness", "The agent's calls: each kind's grants, and the rules and hooks on calls.", harness))
        .push(section("Factory", "The work around the calls: checks on the change, reviewers, and what the factory may do.", factory));
    if editable {
        body = body.push(adding_every(rules));
    }
    container(body.extend(super::said(rules)))
        .padding(16)
        .width(Fill)
        .style(theme::panel)
        .into()
}
