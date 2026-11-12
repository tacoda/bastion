//! The rules at a level (D56): constraints, capabilities and permissions.
//! Constraints and capabilities sort by mode: feedforward guides the agent
//! before it acts, and feedback checks what it did. Each row says where it
//! comes from: the workspace's repository, read-only here, or Bastion. The
//! same view serves a workflow and one of its steps.

use bastion_protocol::{ConstraintView, FactoryCapabilityView, WorkspaceCharterView};
use iced::widget::{Column, column, container, row, text, text_input};
use iced::{Element, Fill};

use super::capabilities::{granting, line};
use super::constraints::{check_form, hook_form, mode, note_form, reviewer_form, rows, rule_form};
use super::{Adding, At, Msg, Rules, chip, choice, help};
use crate::theme;

/// The kinds of step an agent runs, each with grants of its own.
const KINDS: [(&str, &str); 3] = [("prompt", "Prompt"), ("agent", "Agent"), ("skill", "Skill")];

/// The list narrows to a filter once a level has more constraints than this.
const FILTER_FROM: usize = 5;

/// A section: its title, what it covers, and its rows.
fn section<'a>(title: &'a str, about: &'a str, body: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    let head = row![
        text(title).size(13).font(theme::BOLD).color(theme::INK),
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

/// A mode's heading inside a section.
fn heading<'a>(name: &'a str, about: &'a str) -> Element<'a, Msg> {
    row![
        text(name).size(12).font(theme::BOLD).color(theme::MUTED),
        help(about)
    ]
    .spacing(8)
    .into()
}

/// A line the workspace's repository brings: read-only, named for its
/// workspace.
fn from_workspace<'a>(workspace: &str, what: String) -> Element<'a, Msg> {
    row![
        chip(format!("Workspace · {workspace}")),
        text(what).size(13).width(Fill)
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// The workspaces whose repository has a charter.
fn charters(rules: &Rules) -> impl Iterator<Item = &WorkspaceCharterView> {
    rules.charters.iter().filter(|c| !c.files.is_empty())
}

/// What each workspace brings, as `pick` reads it from its charter.
fn workspace_lines<'a>(
    rules: &Rules,
    pick: impl Fn(&WorkspaceCharterView) -> Vec<String>,
) -> Vec<Element<'a, Msg>> {
    let lines = charters(rules).flat_map(|c| {
        pick(c)
            .into_iter()
            .map(|w| (c.workspace.clone(), w))
            .collect::<Vec<_>>()
    });
    lines.map(|(ws, what)| from_workspace(&ws, what)).collect()
}

/// Constraints: what the agent must keep, told before it acts or checked
/// after.
fn constraints<'a>(rules: &'a Rules, at: &At, editable: bool) -> Element<'a, Msg> {
    let kept = rules.constraints.iter().filter(|c| !c.capability);
    let (feedback, feedforward): (Vec<&ConstraintView>, Vec<&ConstraintView>) =
        kept.partition(|c| mode(c) == "Feedback");
    let agents_md = |c: &WorkspaceCharterView| {
        let told = c.files.iter().any(|f| f == "AGENTS.md");
        told.then(|| "AGENTS.md is told to the agent.".to_string())
            .into_iter()
            .collect()
    };
    let mut body = vec![heading(
        "Feedforward",
        "told to the agent before it acts; the agent judges it",
    )];
    body.extend(workspace_lines(rules, agents_md));
    body.extend(rows(rules, feedforward, at, editable));
    body.push(heading(
        "Feedback",
        "checks what the agent did; code or a reviewer judges it",
    ));
    body.extend(workspace_lines(rules, |c| c.hooks.clone()));
    body.extend(rows(rules, feedback, at, editable));
    section("Constraints", "What the agent must keep.", body)
}

/// Capabilities: what the agent may use, told to it before it acts.
fn capabilities<'a>(rules: &'a Rules, at: &At, editable: bool) -> Element<'a, Msg> {
    let notes: Vec<&ConstraintView> = rules.constraints.iter().filter(|c| c.capability).collect();
    let skills = |c: &WorkspaceCharterView| {
        let skills = c
            .skills
            .iter()
            .map(|s| format!("The skill {s}, in .agents/skills"));
        skills
            .chain(
                c.agents
                    .iter()
                    .map(|a| format!("The agent {a}, in .agents/agents")),
            )
            .collect()
    };
    let mut body = vec![heading(
        "Feedforward",
        "told to the agent as what it may use",
    )];
    body.extend(workspace_lines(rules, skills));
    body.extend(rows(rules, notes, at, editable));
    section("Capabilities", "What the agent may use.", body)
}

/// The grants at the level: the step's kind's at a step, else each kind's.
fn grants<'a>(rules: &'a Rules, at: &At, editable: bool) -> Vec<Element<'a, Msg>> {
    let kinds = KINDS
        .iter()
        .filter(|(k, _)| !at.is_step() || *k == at.stage);
    kinds
        .map(|(kind, name)| {
            let head = help(format!("{name} steps' grants, from Bastion"));
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
    row![chip("Bastion · factory"), what, chip(f.rung.clone())]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// Permissions: what the gate lets a call do. Bastion's grants, ask or
/// allowed; the repository's allow, ask and deny rules; and what the
/// factory may do.
fn permissions<'a>(rules: &'a Rules, at: &At, editable: bool) -> Element<'a, Msg> {
    let mut body = grants(rules, at, editable);
    body.extend(workspace_lines(rules, |c| {
        c.allow.iter().map(|a| format!("allows {a}")).collect()
    }));
    body.extend(workspace_lines(rules, |c| c.rules.clone()));
    if !at.is_step() {
        body.extend(rules.factory.iter().map(factory_line));
    }
    section(
        "Permissions",
        "What the gate lets a call do. A call no grant covers is refused.",
        body,
    )
}

/// One form at a time to add something on at the level, behind a switch.
fn adding<'a>(rules: &'a Rules, at: &At) -> Element<'a, Msg> {
    let pick =
        |label: &str, a: Adding| choice(label.to_string(), rules.adding == a, Some(Msg::Adding(a)));
    let label = |words: &'a str| text(words).size(12).color(theme::MUTED);
    let switch = row![
        text("Add").size(13).color(theme::MUTED),
        label("constraint:"),
        pick("A rule", Adding::Rule),
        pick("A check", Adding::Check),
        pick("A hook", Adding::Hook),
        pick("A reviewer", Adding::Reviewer),
        label("capability:"),
        pick("A note", Adding::Note),
        label("permission:"),
        pick("A grant", Adding::Grant),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let form = match rules.adding {
        Adding::Rule => rule_form(rules),
        Adding::Note => note_form(rules),
        Adding::Check => check_form(rules),
        Adding::Hook => hook_form(rules),
        Adding::Reviewer => reviewer_form(rules),
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

/// The rules at the level `at`, and the Add bar.
pub fn view<'a>(rules: &'a Rules, at: &At, editable: bool) -> Element<'a, Msg> {
    let mut body = Column::new().spacing(10);
    if rules.constraints.len() > FILTER_FROM {
        let filter = text_input("Filter by words or kind", &rules.filter)
            .on_input(Msg::Filter)
            .style(theme::field);
        body = body.push(filter);
    }
    body = body
        .push(constraints(rules, at, editable))
        .push(capabilities(rules, at, editable))
        .push(permissions(rules, at, editable));
    if editable {
        body = body.push(adding(rules, at));
    }
    body.extend(super::said(rules)).into()
}
