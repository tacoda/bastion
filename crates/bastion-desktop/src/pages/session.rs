//! One turn, step by step: each step the agent took, each call with
//! the gate's verdict on it, and what each call returned.

use bastion_protocol::api;
use bastion_protocol::{SessionDetail, TurnStep};
use iced::widget::{Column, column, container, row, text};
use iced::{Color, Element, Fill, Task};

use super::{Ctx, scrolled};
use crate::app::Message;
use crate::theme;

pub struct State {
    id: String,
    detail: Option<SessionDetail>,
    error: String,
}

impl State {
    pub fn new(id: String) -> Self {
        State {
            id,
            detail: None,
            error: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Result<Box<SessionDetail>, String>),
}

pub fn load(state: &State, ctx: &Ctx) -> Task<Message> {
    let read = api::Session {
        id: state.id.clone(),
    };
    let task = ctx.task(&ctx.current, read, |r| Msg::Read(r.map(Box::new)));
    task.map(Message::Session)
}

pub fn update(state: &mut State, msg: Msg) {
    match msg {
        Msg::Read(Ok(d)) => (state.detail, state.error) = (Some(*d), String::new()),
        Msg::Read(Err(e)) => state.error = e,
    }
}

/// The color a verdict reads in: allowed, held or refused.
fn verdict_color(verdict: &str) -> Color {
    match verdict.split(' ').next() {
        Some("continue") => theme::ok(),
        Some("hold") => theme::warn(),
        Some("deny") => theme::bad(),
        _ => theme::muted(),
    }
}

fn step_view(s: &TurnStep) -> Element<'static, Message> {
    let label = text(s.kind.clone())
        .size(12)
        .color(theme::muted())
        .width(64);
    let body: Element<'static, Message> = match s.kind.as_str() {
        "call" => column![
            text(s.text.clone()).font(theme::MONO).size(13),
            text(s.verdict.clone())
                .size(12)
                .color(verdict_color(&s.verdict))
        ]
        .spacing(2)
        .into(),
        "result" => container(text(s.text.clone()).font(theme::MONO).size(12))
            .padding(8)
            .width(Fill)
            .style(theme::code)
            .into(),
        "thought" => text(s.text.clone()).size(13).color(theme::muted()).into(),
        _ => text(s.text.clone()).size(13).into(),
    };
    row![label, body].spacing(12).padding([4, 12]).into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let mut body = column![
        text(format!("Session {}", state.id))
            .size(24)
            .font(theme::BOLD),
        text(&state.error).color(theme::bad()),
    ];
    if let Some(d) = &state.detail {
        let open = Message::OpenJob {
            server: ctx.current.clone(),
            id: d.job.clone(),
        };
        let job = crate::lift::button(text(format!("Job {}", d.job)).size(13))
            .style(theme::link)
            .on_press(open);
        let steps = Column::with_children(d.steps.iter().map(step_view));
        body = body.push(job).push(steps);
    }
    scrolled(body.spacing(12))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_reads_in_the_color_of_its_decision() {
        assert_eq!(verdict_color("continue"), theme::ok());
        assert_eq!(verdict_color("deny (no deleting)"), theme::bad());
        assert_eq!(verdict_color("hold (ask)"), theme::warn());
        assert_eq!(verdict_color(""), theme::muted());
    }
}
