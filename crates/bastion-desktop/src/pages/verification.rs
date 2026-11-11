//! The Verification page: each landed job with what checked it, the
//! commits it landed and the escapes found in them, and a form to report
//! an escape by its commit.

use std::collections::BTreeMap;

use bastion_protocol::api;
use bastion_protocol::{EscapeReport, EscapeView, LandedView};
use iced::widget::{Column, button, column, row, text, text_input};
use iced::{Element, Fill, Task};

use super::{Ctx, ago, scrolled, section};
use crate::app::Message;
use crate::theme;

#[derive(Default)]
pub struct State {
    landed: Vec<LandedView>,
    commit: String,
    what: String,
    error: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Result<Vec<LandedView>, String>),
    Commit(String),
    What(String),
    Report,
    Reported(Result<EscapeView, String>),
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    ctx.task(&ctx.current, api::Verification, Msg::Read)
        .map(Message::Verification)
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Read(Ok(landed)) => (state.landed, state.error) = (landed, String::new()),
        Msg::Read(Err(e)) | Msg::Reported(Err(e)) => state.error = e,
        Msg::Commit(c) => state.commit = c,
        Msg::What(w) => state.what = w,
        Msg::Report => {
            let report = EscapeReport {
                commit: state.commit.clone(),
                what: state.what.clone(),
            };
            let send = ctx.task(&ctx.current, api::EscapeAdd(report), Msg::Reported);
            return send.map(Message::Verification);
        }
        // The audit keeps the escape a moment later, so add it here.
        Msg::Reported(Ok(escape)) => {
            if let Some(l) = state.landed.iter_mut().find(|l| l.job == escape.job) {
                l.escapes.push(escape);
            }
            (state.commit, state.what, state.error) = Default::default();
        }
    }
    Task::none()
}

/// Per workspace: how many jobs landed, and how many of them escaped.
fn rates(landed: &[LandedView]) -> BTreeMap<&str, (u32, u32)> {
    let mut all: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    for l in landed {
        let rate = all.entry(&l.workspace).or_default();
        rate.0 += 1;
        rate.1 += u32::from(!l.escapes.is_empty());
    }
    all
}

fn rates_view(landed: &[LandedView]) -> Element<'static, Message> {
    let all = rates(landed);
    let (n, escaped) = all
        .iter()
        .fold((0, 0), |(n, e), (_, (dn, de))| (n + dn, e + de));
    let rate = match n {
        0 => "none landed".to_string(),
        n => format!("{:.0}%", escaped as f64 * 100.0 / n as f64),
    };
    let tiles = row![
        super::chart::tile(n.to_string(), "jobs landed", theme::OK),
        super::chart::tile(escaped.to_string(), "with an escape", theme::BAD),
        super::chart::tile(rate, "escape rate", theme::WARN),
    ]
    .spacing(12);
    let rows = all.into_iter().map(|(ws, (n, escaped))| {
        row![
            text(ws.to_string()).size(13).width(Fill),
            text(format!("{n} landed"))
                .size(13)
                .font(theme::MONO)
                .width(110),
            text(format!("{escaped} with an escape"))
                .size(13)
                .font(theme::MONO)
                .width(150),
        ]
        .into()
    });
    column![tiles, Column::with_children(rows).spacing(4)]
        .spacing(12)
        .padding([4, 12])
        .into()
}

fn report_form(state: &State) -> Element<'_, Message> {
    let msg = |m| Message::Verification(m);
    row![
        text_input("commit id", &state.commit)
            .on_input(move |c| msg(Msg::Commit(c)))
            .font(theme::MONO)
            .width(160),
        text_input("what went wrong", &state.what)
            .on_input(move |w| msg(Msg::What(w)))
            .on_submit(msg(Msg::Report)),
        button(text("Report").size(13))
            .style(theme::primary)
            .on_press(msg(Msg::Report)),
    ]
    .spacing(8)
    .padding([4, 12])
    .into()
}

fn landed_view(l: &LandedView, ctx: &Ctx) -> Element<'static, Message> {
    let open = Message::OpenJob {
        server: ctx.current.clone(),
        id: l.job.clone(),
    };
    let request = l.request.lines().next().unwrap_or_default().to_string();
    let head = row![
        button(text(l.job.clone()).size(13).font(theme::MONO))
            .style(theme::link)
            .on_press(open),
        text(format!("{} · {request}", l.workspace)).width(Fill),
        text(ago(l.landed_at)).size(13).color(theme::MUTED),
    ]
    .spacing(12)
    .align_y(iced::Center);
    let short: Vec<&str> = l.commits.iter().map(|c| &c[..c.len().min(7)]).collect();
    let mut body = column![head].spacing(2);
    for check in &l.checks {
        body = body.push(text(check.clone()).size(12).color(theme::MUTED));
    }
    if !short.is_empty() {
        body = body.push(text(short.join(" ")).size(12).font(theme::MONO));
    }
    for e in &l.escapes {
        let line = format!("escaped in {}: {}", e.commit, e.what);
        body = body.push(text(line).size(13).color(theme::BAD));
    }
    body.padding([6, 12]).into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let lede = format!(
        "Each job {} landed, with the decisions that let it through and its commits. Report a defect by its commit to trace it to its job.",
        ctx.current
    );
    let landed = state.landed.iter().map(|l| landed_view(l, ctx));
    scrolled(
        column![
            column![super::head("Verification", lede, vec![])]
                .extend(super::problem(&state.error))
                .spacing(8),
            section("Escape rate".into(), rates_view(&state.landed), false),
            section("Report an escape".into(), report_form(state), false),
            section(
                "Landed, newest first".into(),
                Column::with_children(landed).into(),
                false
            ),
        ]
        .spacing(12),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_counts_its_landed_jobs_and_those_that_escaped() {
        let landed = |ws: &str, escapes: usize| LandedView {
            workspace: ws.into(),
            escapes: vec![EscapeView::default(); escapes],
            ..LandedView::default()
        };
        let all = [landed("a", 2), landed("a", 0), landed("b", 0)];
        let got = rates(&all);
        assert_eq!(got["a"], (2, 1));
        assert_eq!(got["b"], (1, 0));
    }
}
