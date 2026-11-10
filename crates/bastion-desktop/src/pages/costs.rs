//! The Costs page (F5): what the turns on the server shown cost, in all, by
//! workspace, by step and by day, and the newest turns one by one.

use bastion_protocol::api;
use bastion_protocol::{CostsView, SessionView, Tally};
use iced::widget::{Column, button, column, container, row, text};
use iced::{Element, Fill, Task};

use super::{Ctx, ago, scrolled, section};
use crate::app::Message;
use crate::route::Route;
use crate::theme;

#[derive(Default)]
pub struct State {
    view: Option<CostsView>,
    error: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Result<Box<CostsView>, String>),
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let read = ctx.task(&ctx.current, api::Costs, |r| Msg::Read(r.map(Box::new)));
    read.map(Message::Costs)
}

pub fn update(state: &mut State, msg: Msg) {
    match msg {
        Msg::Read(Ok(view)) => (state.view, state.error) = (Some(*view), String::new()),
        Msg::Read(Err(e)) => state.error = e,
    }
}

/// Millionths of a dollar, as dollars.
fn dollars(micros: u64) -> String {
    format!("${:.4}", micros as f64 / 1e6)
}

/// One tally as a row: its key, then what it used.
fn tally_row(t: &Tally) -> Element<'static, Message> {
    let cell = |s: String| text(s).size(13).font(theme::MONO).width(110);
    row![
        text(t.key.clone()).width(Fill),
        cell(dollars(t.cost_micros)),
        cell(format!("{} sessions", t.sessions)),
        cell(format!("{} turns", t.turns)),
        cell(format!("{} calls", t.calls)),
        cell(format!("{} failed", t.call_errors)),
    ]
    .spacing(8)
    .padding([4, 12])
    .into()
}

fn tallies(title: &str, all: &[Tally]) -> Element<'static, Message> {
    let rows = Column::with_children(all.iter().map(tally_row)).into();
    section(title.into(), rows, false)
}

/// One turn: its job, a link to it, and what it used.
fn session_row<'a>(s: &SessionView, ctx: &Ctx) -> Element<'a, Message> {
    let open = Message::OpenJob {
        server: ctx.current.clone(),
        id: s.job.clone(),
    };
    let job = button(text(s.job.clone()).size(13).font(theme::MONO))
        .style(theme::link)
        .on_press(open);
    let steps = button(text("steps").size(13))
        .style(theme::link)
        .on_press(Message::Go(Route::Session(s.session.clone())));
    row![
        job,
        steps,
        text(format!("{} · {}", s.workspace, s.stage.replace('_', " "))).width(Fill),
        text(dollars(s.cost_micros)).size(13).font(theme::MONO),
        text(format!("{} calls, {} failed", s.calls, s.call_errors)).size(13),
        text(ago(s.ended_at)).size(13).color(theme::MUTED),
    ]
    .spacing(12)
    .padding([4, 12])
    .align_y(iced::Center)
    .into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let lede = "What the turns on this Bastion server cost, as each turn's end metered it: in all, by workspace, by step and by day, then the newest turns one by one.";
    let mut body = column![
        text("Costs").size(24).font(theme::BOLD),
        text(lede).size(13).color(theme::MUTED),
        text(&state.error).color(theme::BAD),
    ];
    let Some(v) = &state.view else {
        return scrolled(body.spacing(12));
    };
    let total = format!(
        "{} in all, over {} turns in {} sessions",
        dollars(v.total.cost_micros),
        v.total.turns,
        v.total.sessions
    );
    let sessions = v.sessions.iter().map(|s| session_row(s, ctx));
    body = body.extend([
        container(text(total).size(18).font(theme::BOLD)).into(),
        tallies("By workspace", &v.by_workspace),
        tallies("By step", &v.by_stage),
        tallies("By day", &v.by_day),
        section(
            "Sessions, newest first".into(),
            Column::with_children(sessions).into(),
            false,
        ),
    ]);
    scrolled(body.spacing(12))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cost_reads_in_dollars() {
        assert_eq!(dollars(11_175), "$0.0112");
        assert_eq!(dollars(0), "$0.0000");
    }
}
