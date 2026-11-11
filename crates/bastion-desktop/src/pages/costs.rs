//! The Costs page: what the turns on the server shown, or on every server,
//! cost: in all, by server, by workspace, by step and by day, and the
//! newest turns one by one.

use std::collections::BTreeMap;

use bastion_protocol::api;
use bastion_protocol::{CostsView, SessionView, Tally};
use iced::widget::{Column, button, column, container, row, text};
use iced::{Element, Fill, Task};

use super::chart::{self, Bars};
use super::{Ctx, Each, ago, scrolled, section};
use crate::app::Message;
use crate::route::Route;
use crate::theme;

#[derive(Default)]
pub struct State {
    /// What the servers read, added up; none before the first read.
    view: Option<Combined>,
    /// Why each server that did not answer did not.
    failed: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Box<Each<CostsView>>),
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let read = ctx.clone().each(api::Costs);
    Task::perform(read, |r| Message::Costs(Msg::Read(Box::new(r))))
}

pub fn update(state: &mut State, msg: Msg) {
    match msg {
        Msg::Read(r) => {
            let (read, failed) = *r;
            (state.view, state.failed) = (Some(combine(&read)), failed);
        }
    }
}

/// Every server's costs added up, each turn with its server.
#[derive(Debug, Default, PartialEq)]
struct Combined {
    total: Tally,
    by_server: Vec<Tally>,
    by_workspace: Vec<Tally>,
    by_stage: Vec<Tally>,
    /// Newest day first.
    by_day: Vec<Tally>,
    /// Newest turn first.
    sessions: Vec<(String, SessionView)>,
    /// Each server's cost by day, for the chart.
    daily: Vec<(String, BTreeMap<String, u64>)>,
}

fn add(into: &mut Tally, t: &Tally) {
    into.sessions += t.sessions;
    into.turns += t.turns;
    into.calls += t.calls;
    into.call_errors += t.call_errors;
    into.cost_micros += t.cost_micros;
}

/// The tallies with the same key added into one, the costliest first.
fn merge(all: impl Iterator<Item = Tally>) -> Vec<Tally> {
    let mut by: BTreeMap<String, Tally> = BTreeMap::new();
    for t in all {
        let kept = by.entry(t.key.clone()).or_insert_with(|| Tally {
            key: t.key.clone(),
            ..Tally::default()
        });
        add(kept, &t);
    }
    let mut merged: Vec<Tally> = by.into_values().collect();
    merged.sort_by_key(|t| std::cmp::Reverse(t.cost_micros));
    merged
}

/// What `read` cost, added up across its servers. A workspace is named
/// with its server when several servers show, as two may share a name.
fn combine(read: &[(String, CostsView)]) -> Combined {
    let many = read.len() > 1;
    let mut total = Tally::default();
    read.iter().for_each(|(_, v)| add(&mut total, &v.total));
    let by_server = read.iter().map(|(server, v)| Tally {
        key: server.clone(),
        ..v.total.clone()
    });
    let workspaces = read.iter().flat_map(|(server, v)| {
        v.by_workspace.iter().map(move |t| Tally {
            key: match many {
                true => format!("{server} · {}", t.key),
                false => t.key.clone(),
            },
            ..t.clone()
        })
    });
    let mut by_day = merge(read.iter().flat_map(|(_, v)| v.by_day.clone()));
    by_day.sort_by(|a, b| b.key.cmp(&a.key));
    let mut sessions: Vec<(String, SessionView)> = read
        .iter()
        .flat_map(|(server, v)| v.sessions.iter().map(move |s| (server.clone(), s.clone())))
        .collect();
    sessions.sort_by_key(|(_, s)| std::cmp::Reverse(s.ended_at));
    let daily = read
        .iter()
        .map(|(server, v)| {
            let days = v.by_day.iter().map(|t| (t.key.clone(), t.cost_micros));
            (server.clone(), days.collect())
        })
        .collect();
    Combined {
        daily,
        total,
        by_server: merge(by_server),
        by_workspace: merge(workspaces),
        by_stage: merge(read.iter().flat_map(|(_, v)| v.by_stage.clone())),
        by_day,
        sessions,
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

/// One turn: its server when several show, its job, a link to it, and
/// what it used.
fn session_row<'a>(server: &str, s: &SessionView, many: bool) -> Element<'a, Message> {
    let open = Message::OpenJob {
        server: server.to_string(),
        id: s.job.clone(),
    };
    let job = button(text(s.job.clone()).size(13).font(theme::MONO))
        .style(theme::link)
        .on_press(open);
    let steps = button(text("steps").size(13))
        .style(theme::link)
        .on_press(Message::Go(Route::Session(s.session.clone())));
    let mut line = row![].spacing(12).padding([4, 12]).align_y(iced::Center);
    if many {
        line = line.push(text(server.to_string()).size(13).color(theme::LINK));
    }
    line.extend([job.into(), steps.into()])
        .extend([
            text(format!("{} · {}", s.workspace, s.stage.replace('_', " ")))
                .width(Fill)
                .into(),
            text(dollars(s.cost_micros))
                .size(13)
                .font(theme::MONO)
                .into(),
            text(format!("{} calls, {} failed", s.calls, s.call_errors))
                .size(13)
                .into(),
            text(ago(s.ended_at)).size(13).color(theme::MUTED).into(),
        ])
        .into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let many = ctx.all;
    let about = format!(
        "What the turns on {} cost, as each turn's end metered it.",
        super::on(ctx)
    );
    let failed = state.failed.iter().filter_map(|f| super::problem(f));
    let mut body = column![super::head("Costs", about, vec![])].extend(failed);
    let Some(v) = &state.view else {
        return scrolled(body.spacing(12));
    };
    let total = format!(
        "{} in all, over {} turns in {} sessions",
        dollars(v.total.cost_micros),
        v.total.turns,
        v.total.sessions
    );
    body = body.push(container(text(total).size(18).font(theme::BOLD)));
    body = body.push(chart::view("Spend by day, last 30 days", spend(v)));
    if many {
        body = body.push(tallies("By server", &v.by_server));
    }
    let sessions = v
        .sessions
        .iter()
        .map(|(server, s)| session_row(server, s, many));
    body = body.extend([
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

/// Each day's spend over the last 30 days, split by server.
fn spend(v: &Combined) -> Bars {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let columns = chart::last_days(now, 30);
    let values = columns
        .iter()
        .map(|day| {
            let on = |(_, days): &(String, BTreeMap<String, u64>)| days.get(day).copied();
            v.daily.iter().map(|d| on(d).unwrap_or(0) as f64).collect()
        })
        .collect();
    let series = v.daily.iter().zip(chart::PALETTE.iter().cycle());
    Bars {
        columns,
        series: series.map(|((s, _), c)| (s.clone(), *c)).collect(),
        values,
        unit: |micros| format!("${:.4}", micros / 1e6),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cost_reads_in_dollars() {
        assert_eq!(dollars(11_175), "$0.0112");
        assert_eq!(dollars(0), "$0.0000");
    }

    #[test]
    fn every_servers_costs_add_up_by_key_and_name_their_server() {
        let t = |key: &str, cost| Tally {
            key: key.into(),
            sessions: 1,
            cost_micros: cost,
            ..Tally::default()
        };
        let costs = |ws: &str, cost| CostsView {
            total: t("", cost),
            by_workspace: vec![t(ws, cost)],
            by_stage: vec![t("prompt", cost)],
            by_day: vec![t("2026-11-10", cost)],
            sessions: Vec::new(),
        };
        let read = vec![
            ("local".to_string(), costs("demo", 100)),
            ("team".to_string(), costs("demo", 300)),
        ];
        let all = combine(&read);
        assert_eq!((all.total.cost_micros, all.total.sessions), (400, 2));
        let keys = |ts: &[Tally]| ts.iter().map(|t| t.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&all.by_server), ["team", "local"]);
        assert_eq!(keys(&all.by_workspace), ["team · demo", "local · demo"]);
        assert_eq!((all.by_stage.len(), all.by_stage[0].cost_micros), (1, 400));
        assert_eq!(all.by_day[0].sessions, 2);
        let one = combine(&read[..1]);
        assert_eq!(
            keys(&one.by_workspace),
            ["demo"],
            "one server names no server"
        );
    }
}
