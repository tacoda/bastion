//! The pieces the rules pages share, and the read-only list of what
//! applies at a workflow step (D57). Each row says where it comes from:
//! the workspace's charter or Bastion.

use bastion_protocol::{ConstraintView, FactoryCapabilityView};
use iced::widget::{Column, column, container, row, text};
use iced::{Element, Fill};

use super::constraints::{badge, kind, mode, source};
use super::{At, Msg, Rules, Which, chip, help, scope, small};
use crate::theme;

/// The kinds of step an agent runs, each with grants of its own.
pub(super) const KINDS: [(&str, &str); 3] =
    [("prompt", "Prompt"), ("agent", "Agent"), ("skill", "Skill")];

/// A section: its title, what it covers, and its rows.
pub(super) fn section<'a>(
    title: &'a str,
    about: &'a str,
    body: Vec<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let head = row![
        text(title).size(13).font(theme::BOLD).color(theme::ink()),
        text(about).size(12).color(theme::muted()),
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

/// What the factory itself may do, as its workspace and steps set it.
pub(super) fn factory_line<'a>(f: &FactoryCapabilityView) -> Element<'a, Msg> {
    let what = column![text(f.name.clone()).size(13), help(f.how.clone())]
        .spacing(2)
        .width(Fill);
    row![chip("Bastion · factory"), what, chip(f.rung.clone())]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// One rule's read-only line: its kind, its words, where it comes from.
fn shown<'a>(kind: &str, words: String, from: String) -> Element<'a, Msg> {
    row![
        badge(kind, theme::muted()),
        text(words).size(13).width(Fill),
        chip(from)
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// A section of the list, with a link to the Harness, which changes it.
fn part<'a>(which: Which, about: &'a str, body: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    let mut body = body;
    if body.is_empty() {
        body.push(help("None here."));
    }
    body.push(small("Change on Harness", Some(Msg::Go)));
    section(which.label(), about, body)
}

/// What applies at the step `at`, read-only (D57): the Harness changes it.
pub fn view<'a>(rules: &'a Rules, at: &At) -> Element<'a, Msg> {
    let on = |scopes: &[String]| scope::on(scopes, at.scope);
    let rule = |c: &ConstraintView| shown(kind(c), c.rule.clone(), source(c));
    let (checks, told): (Vec<&ConstraintView>, Vec<&ConstraintView>) = rules
        .constraints
        .iter()
        .filter(|c| on(&c.steps))
        .partition(|c| !c.capability && mode(c) == "Check");
    let grants = rules.kind(at.stage).into_iter().filter(|g| on(&g.scopes));
    let grants = grants.map(|g| shown(&g.rung, g.function.clone(), "Bastion".into()));
    column![
        part(
            Which::Guidance,
            "What the agent is told before it acts.",
            told.into_iter().map(rule).collect()
        ),
        part(
            Which::Checks,
            "What judges its work. A failing check blocks.",
            checks.into_iter().map(rule).collect()
        ),
        part(
            Which::Permissions,
            "What the gate lets a call do.",
            grants.collect()
        ),
    ]
    .spacing(10)
    .into()
}
