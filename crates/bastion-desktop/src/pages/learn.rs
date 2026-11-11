//! The Learning page: what learn turns proposed, each with why and the
//! evidence from its job, open ones first. A person accepts one, which adds
//! the constraint or the gate, or declines it.

use bastion_protocol::api;
use bastion_protocol::{ProposalDecision, ProposalView};
use iced::widget::{Column, button, column, container, row, text};
use iced::{Element, Fill, Task};

use super::{Ctx, scrolled, section};
use crate::app::Message;
use crate::theme;

#[derive(Default)]
pub struct State {
    all: Vec<ProposalView>,
    error: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Result<Vec<ProposalView>, String>),
    Decide { job: String, n: u32, accept: bool },
    Done(Result<(), String>),
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    ctx.task(&ctx.current, api::Proposals, Msg::Read)
        .map(Message::Learn)
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Read(Ok(all)) => (state.all, state.error) = (all, String::new()),
        Msg::Read(Err(e)) | Msg::Done(Err(e)) => state.error = e,
        Msg::Done(Ok(())) => state.error.clear(),
        Msg::Decide { job, n, accept } => {
            // The audit keeps the decision a moment later, so mark it here.
            if let Some(p) = state.all.iter_mut().find(|p| p.job == job && p.n == n) {
                p.decision = if accept { "accepted" } else { "declined" }.into();
            }
            let decide = api::ProposalDecide(ProposalDecision { job, n, accept });
            return ctx
                .task(&ctx.current, decide, Msg::Done)
                .map(Message::Learn);
        }
    }
    Task::none()
}

fn proposal_view(p: &ProposalView, ctx: &Ctx) -> Element<'static, Message> {
    let open = Message::OpenJob {
        server: ctx.current.clone(),
        id: p.job.clone(),
    };
    let head = row![
        text(format!("{} · {}", p.kind, p.workspace))
            .size(12)
            .color(theme::MUTED),
        button(text(p.job.clone()).size(12).font(theme::MONO))
            .style(theme::link)
            .on_press(open),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let evidence = container(text(p.evidence.clone()).font(theme::MONO).size(12))
        .padding(8)
        .width(Fill)
        .style(theme::code);
    let mut body = column![
        head,
        text(p.change.clone()).size(14).font(theme::BOLD),
        text(p.binds.clone()).size(12).font(theme::MONO),
        text(p.why.clone()).size(13),
        evidence,
    ]
    .spacing(4);
    body = body.push(decision(p));
    container(body).padding([8, 12]).width(Fill).into()
}

/// Accept and decline while open; the decision once made.
fn decision(p: &ProposalView) -> Element<'static, Message> {
    if !p.decision.is_empty() {
        return text(p.decision.clone()).size(12).color(theme::MUTED).into();
    }
    let decide = |accept| {
        Message::Learn(Msg::Decide {
            job: p.job.clone(),
            n: p.n,
            accept,
        })
    };
    row![
        button(text(format!("Accept as {}", p.kind)).size(13))
            .style(theme::primary)
            .on_press(decide(true)),
        button(text("Decline").size(13))
            .style(theme::secondary)
            .on_press(decide(false)),
    ]
    .spacing(8)
    .into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let lede = "What learn turns proposed, each with the evidence from its job. Accept a constraint to tell every turn its rule; accept a gate to check each change in its workspace.";
    let (open, decided): (Vec<_>, Vec<_>) = state.all.iter().partition(|p| p.decision.is_empty());
    let list = |ps: Vec<&ProposalView>| {
        Column::with_children(ps.into_iter().map(|p| proposal_view(p, ctx))).into()
    };
    scrolled(
        column![
            text("Learning").size(24).font(theme::BOLD),
            text(lede).size(13).color(theme::MUTED),
            text(&state.error).color(theme::BAD),
            section(format!("Open ({})", open.len()), list(open), true),
            section(format!("Decided ({})", decided.len()), list(decided), false),
        ]
        .spacing(12),
    )
}
