//! Where an item applies (D57): each workflow, then each of its steps that
//! runs an agent, with an Inherit | On | Off switch, so a rule lands on the
//! steps you pick. A shell step runs no agent, so no rule binds it.

use bastion_protocol::WorkflowView;
use iced::widget::{Column, container, row, text};
use iced::{Element, Fill};

use super::{At, Item, Msg, Rules, help, scope, scope_switch};
use crate::theme;

/// How many of the workflows' agent steps the item is on at.
pub(super) fn steps_on(rules: &Rules, scopes: &[String]) -> (usize, usize) {
    let steps = rules.workflows.iter().flat_map(|w| {
        let agent = w.nodes.iter().filter(|n| n.stage != "shell");
        agent.map(move |n| format!("{}/{}", w.id, n.id))
    });
    let all: Vec<String> = steps.collect();
    let on = all.iter().filter(|k| scope::on(scopes, k)).count();
    (on, all.len())
}

/// One workflow's row and its steps' rows.
fn workflow<'a>(
    item: &Item,
    scopes: &[String],
    w: &WorkflowView,
    editable: bool,
) -> Element<'a, Msg> {
    let at = At {
        scope: &w.id,
        stage: "",
    };
    let name = text(format!("{} · every step", w.name))
        .size(13)
        .width(Fill);
    let head = row![name, scope_switch(item.clone(), scopes, &at, editable)]
        .spacing(8)
        .align_y(iced::Center);
    let agent = w.nodes.iter().filter(|n| n.stage != "shell");
    let steps = agent.map(|n| {
        let key = format!("{}/{}", w.id, n.id);
        let at = At {
            scope: &key,
            stage: &n.stage,
        };
        let name = text(n.id.replace('_', " "))
            .size(13)
            .color(theme::MUTED)
            .width(Fill);
        row![name, scope_switch(item.clone(), scopes, &at, editable)]
            .spacing(8)
            .padding(iced::Padding::from([0, 0]).left(24))
            .align_y(iced::Center)
            .into()
    });
    Column::new().spacing(4).push(head).extend(steps).into()
}

/// The item's list: every workflow and its agent steps.
pub(super) fn tree<'a>(
    rules: &Rules,
    item: &Item,
    scopes: &[String],
    editable: bool,
) -> Element<'a, Msg> {
    let workflows = rules
        .workflows
        .iter()
        .map(|w| workflow(item, scopes, w, editable));
    let why = "A step's own setting wins, then its workflow's, then every workflow's.";
    let body = Column::new().spacing(10).push(help(why)).extend(workflows);
    container(body)
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}
