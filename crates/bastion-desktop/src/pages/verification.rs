//! The Verification page: each job the servers shown landed, with what
//! checked it, the commits it landed and the escapes found in them, and a
//! form to report an escape by its commit to the server that landed it.

use std::collections::BTreeMap;

use bastion_protocol::api;
use bastion_protocol::{EscapeReport, EscapeView, LandedView};
use iced::widget::{Column, column, row, text, text_input};
use iced::{Element, Fill, Task};

use super::{Ctx, Each, ago, scrolled, section};
use crate::app::Message;
use crate::theme;

#[derive(Default)]
pub struct State {
    /// Each landed job with the server that landed it, newest first.
    landed: Vec<(String, LandedView)>,
    commit: String,
    what: String,
    error: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Box<Each<Vec<LandedView>>>),
    Commit(String),
    What(String),
    Report,
    Reported(Result<EscapeView, String>),
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let read = ctx.clone().each(api::Verification);
    Task::perform(read, |r| Message::Verification(Msg::Read(Box::new(r))))
}

/// The server that landed `commit`, by a landed job's commit it starts.
fn landed_on<'a>(landed: &'a [(String, LandedView)], commit: &str) -> Option<&'a str> {
    let commit = commit.trim();
    let has =
        |l: &LandedView| !commit.is_empty() && l.commits.iter().any(|c| c.starts_with(commit));
    landed
        .iter()
        .find(|(_, l)| has(l))
        .map(|(server, _)| server.as_str())
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Read(read) => {
            let (read, failed) = *read;
            state.landed = read
                .into_iter()
                .flat_map(|(server, all)| all.into_iter().map(move |l| (server.clone(), l)))
                .collect();
            state
                .landed
                .sort_by_key(|(_, l)| std::cmp::Reverse(l.landed_at));
            state.error = failed.join(" ");
        }
        Msg::Reported(Err(e)) => state.error = e,
        Msg::Commit(c) => state.commit = c,
        Msg::What(w) => state.what = w,
        Msg::Report => {
            let report = EscapeReport {
                commit: state.commit.clone(),
                what: state.what.clone(),
            };
            // An escape belongs to the server that landed its commit.
            let Some(server) = landed_on(&state.landed, &state.commit) else {
                state.error = format!(
                    "No job landed a commit that starts {}.",
                    state.commit.trim()
                );
                return Task::none();
            };
            let send = ctx.task(server, api::EscapeAdd(report), Msg::Reported);
            return send.map(Message::Verification);
        }
        // The audit keeps the escape a moment later, so add it here.
        Msg::Reported(Ok(escape)) => {
            if let Some((_, l)) = state.landed.iter_mut().find(|(_, l)| l.job == escape.job) {
                l.escapes.push(escape);
            }
            (state.commit, state.what, state.error) = Default::default();
        }
    }
    Task::none()
}

/// Per workspace, named with its server when several show: how many jobs
/// landed, and how many of them escaped.
fn rates(landed: &[(String, LandedView)], many: bool) -> BTreeMap<String, (u32, u32)> {
    let mut all: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for (server, l) in landed {
        let key = match many {
            true => format!("{server} · {}", l.workspace),
            false => l.workspace.clone(),
        };
        let rate = all.entry(key).or_default();
        rate.0 += 1;
        rate.1 += u32::from(!l.escapes.is_empty());
    }
    all
}

fn rates_view(landed: &[(String, LandedView)], many: bool) -> Element<'static, Message> {
    let all = rates(landed, many);
    let (n, escaped) = all
        .iter()
        .fold((0, 0), |(n, e), (_, (dn, de))| (n + dn, e + de));
    let rate = match n {
        0 => "none landed".to_string(),
        n => format!("{:.0}%", escaped as f64 * 100.0 / n as f64),
    };
    let tiles = row![
        super::chart::tile(n.to_string(), "jobs landed", theme::ok()),
        super::chart::tile(escaped.to_string(), "with an escape", theme::bad()),
        super::chart::tile(rate, "escape rate", theme::warn()),
    ]
    .spacing(12);
    let rows = all.into_iter().map(|(ws, (n, escaped))| {
        row![
            text(ws).size(13).width(Fill),
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
        crate::lift::button(text("Report").size(13))
            .style(theme::primary)
            .on_press(msg(Msg::Report)),
    ]
    .spacing(8)
    .padding([4, 12])
    .into()
}

fn landed_view(server: &str, l: &LandedView, many: bool) -> Element<'static, Message> {
    let open = Message::OpenJob {
        server: server.to_string(),
        id: l.job.clone(),
    };
    let place = match many {
        true => format!("{server} · {}", l.workspace),
        false => l.workspace.clone(),
    };
    let request = l.request.lines().next().unwrap_or_default().to_string();
    let head = row![
        crate::lift::button(text(l.job.clone()).size(13).font(theme::MONO))
            .style(theme::link)
            .on_press(open),
        text(format!("{place} · {request}")).width(Fill),
        text(ago(l.landed_at)).size(13).color(theme::muted()),
    ]
    .spacing(12)
    .align_y(iced::Center);
    let short: Vec<&str> = l.commits.iter().map(|c| &c[..c.len().min(7)]).collect();
    let mut body = column![head].spacing(2);
    for check in &l.checks {
        body = body.push(text(check.clone()).size(12).color(theme::muted()));
    }
    if !short.is_empty() {
        body = body.push(text(short.join(" ")).size(12).font(theme::MONO));
    }
    for e in &l.escapes {
        let line = format!("escaped in {}: {}", e.commit, e.what);
        body = body.push(text(line).size(13).color(theme::bad()));
    }
    body.padding([6, 12]).into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let lede = format!(
        "Specimens: each journey {} brought home, with the decisions that let it through and its commits. Report a defect by its commit to trace it to its journey.",
        super::on(ctx)
    );
    let many = ctx.all;
    let landed = state.landed.iter().map(|(s, l)| landed_view(s, l, many));
    scrolled(
        column![
            column![super::head("Specimens", lede, vec![])]
                .extend(super::problem(&state.error))
                .spacing(8),
            section("Escape rate".into(), rates_view(&state.landed, many), false),
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
        let on = |server: &str, l| (server.to_string(), l);
        let all = [
            on("local", landed("a", 2)),
            on("local", landed("a", 0)),
            on("two", landed("b", 0)),
        ];
        let got = rates(&all, false);
        assert_eq!(got["a"], (2, 1));
        assert_eq!(got["b"], (1, 0));
        let many = rates(&all, true);
        assert_eq!(many["two · b"], (1, 0), "named with its server");
    }

    #[test]
    fn an_escape_goes_to_the_server_that_landed_its_commit() {
        let landed = |commit: &str| LandedView {
            commits: vec![commit.into()],
            ..LandedView::default()
        };
        let all = [
            ("local".to_string(), landed("6a6e6d5360a2")),
            ("two".to_string(), landed("1c15ab9b0000")),
        ];
        assert_eq!(landed_on(&all, " 1c15ab9 "), Some("two"));
        assert_eq!(landed_on(&all, "6a6e6d5"), Some("local"));
        assert_eq!(landed_on(&all, "ffff"), None);
        assert_eq!(landed_on(&all, ""), None, "an empty commit matches nothing");
    }
}
