//! The Audit Log: each server's chain, whether it verifies, and its entries,
//! for the server the switcher shows or for every server, newest first.

use std::collections::BTreeMap;

use bastion_protocol::api;
use bastion_protocol::{AuditEntryView, AuditView};
use iced::widget::{Column, button, column, container, row, text};
use iced::{Color, Element, Fill, Task};

use super::{Ctx, Each, ago, scrolled};
use crate::app::Message;
use crate::theme;

#[derive(Default)]
pub struct State {
    /// Each server's chain, by its name.
    read: Vec<(String, AuditView)>,
    /// Why each server that did not answer did not.
    failed: Vec<String>,
    exported: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Box<Each<AuditView>>),
    Export,
    Exported(Result<String, String>),
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let read = ctx.clone().each(api::Audit);
    Task::perform(read, |r| Message::Audit(Msg::Read(Box::new(r))))
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Read(r) => (state.read, state.failed) = *r,
        Msg::Export => {
            return ctx
                .task(&ctx.current, api::AuditExport, Msg::Exported)
                .map(Message::Audit);
        }
        Msg::Exported(answer) => {
            state.exported = answer.map_or_else(|e| e, |path| format!("Written to {path}"))
        }
    }
    Task::none()
}

/// Whether the chain verifies, and whether its writer keeps up.
fn audit_status(a: &AuditView) -> (String, Color, Option<(String, Color)>) {
    let (status, color) = match a.broken_at {
        None => (
            format!(
                "Verified: all {} entries chain from the first.",
                a.entries.len()
            ),
            theme::OK,
        ),
        Some(seq) => (
            format!("Broken: entry {seq} does not follow from the one before it."),
            theme::BAD,
        ),
    };
    let writer = match (a.pending, &a.last_error) {
        (_, Some(e)) => Some((format!("The audit writer is failing: {e}"), theme::BAD)),
        (0, None) => None,
        (n, None) => Some((format!("{n} records waiting to be written."), theme::WARN)),
    };
    (status, color, writer)
}

/// How many entries of each kind the chains hold, the gate's split by its
/// decision, most first.
fn tallies<'a>(entries: impl Iterator<Item = &'a AuditEntryView>) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in entries {
        let key = match e.kind.as_str() {
            "gate" => format!("gate {}", e.detail.split(' ').next().unwrap_or_default()),
            kind => kind.to_string(),
        };
        *counts.entry(key).or_default() += 1;
    }
    let mut all: Vec<_> = counts.into_iter().collect();
    all.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    all
}

/// Every entry with its server, newest first.
fn merged(read: &[(String, AuditView)]) -> Vec<(&str, &AuditEntryView)> {
    let mut all: Vec<(&str, &AuditEntryView)> = read
        .iter()
        .flat_map(|(server, a)| a.entries.iter().map(move |e| (server.as_str(), e)))
        .collect();
    all.sort_by_key(|(_, e)| std::cmp::Reverse(e.at));
    all
}

/// Each server's check: its name when several show, then its status.
fn checks<'a>(state: &State, many: bool) -> Element<'a, Message> {
    let lines = state.read.iter().map(|(server, a)| {
        let (status, color, writer) = audit_status(a);
        let status = match many {
            true => format!("{server}: {status}"),
            false => status,
        };
        let mut one = column![text(status).font(theme::BOLD).color(color)];
        one = one.extend(writer.map(|(t, c)| text(t).color(c).into()));
        one.into()
    });
    let failed = state
        .failed
        .iter()
        .map(|f| text(f.clone()).color(theme::BAD).into());
    Column::with_children(lines.chain(failed)).spacing(6).into()
}

fn entry_row<'a>(server: &str, e: &AuditEntryView, many: bool) -> Element<'a, Message> {
    let what = column![
        text(format!("{} · {}", e.subject, e.detail)),
        text(e.hash.chars().take(12).collect::<String>())
            .size(12)
            .color(theme::MUTED)
    ];
    let mut line = row![].spacing(8);
    if many {
        line = line.push(text(server.to_string()).color(theme::LINK).width(96));
    }
    line = line.extend([
        text(format!("#{}", e.seq))
            .color(theme::MUTED)
            .width(64)
            .into(),
        text(ago(e.at)).color(theme::MUTED).width(96).into(),
        text(e.kind.clone()).width(112).into(),
        what.width(Fill).into(),
    ]);
    container(line).padding([6, 12]).into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let many = ctx.all;
    // An export writes the file on the server, so it takes one server.
    let export = button(text("Export").size(13))
        .padding([4, 12])
        .style(theme::secondary)
        .on_press_maybe((!many).then_some(Message::Audit(Msg::Export)));
    let head = row![
        text("Audit Log").size(24).font(theme::BOLD).width(Fill),
        export
    ]
    .align_y(iced::Center);
    let entries = merged(&state.read);
    let counts: Vec<String> = tallies(entries.iter().map(|(_, e)| *e))
        .into_iter()
        .map(|(key, n)| format!("{key}: {n}"))
        .collect();
    let rows = entries.iter().map(|(s, e)| entry_row(s, e, many));
    let list = container(Column::with_children(rows))
        .width(Fill)
        .style(theme::panel);
    scrolled(
        column![
            head,
            text(&state.exported).size(12).color(theme::MUTED),
            checks(state, many),
            text(counts.join(" · ")).size(13).color(theme::MUTED),
            list
        ]
        .spacing(12),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, detail: &str, at: u64) -> AuditEntryView {
        AuditEntryView {
            seq: 1,
            at,
            kind: kind.into(),
            subject: "j1".into(),
            detail: detail.into(),
            hash: String::new(),
        }
    }

    #[test]
    fn the_audit_is_tallied_by_kind_and_the_gate_by_decision() {
        let all = [
            entry("gate", "continue sandbox::exec ls", 0),
            entry("gate", "continue sandbox::fs::read a", 0),
            entry("gate", "deny (no) sandbox::exec rm", 0),
            entry("decide", "plan: approved by Ian", 0),
        ];
        assert_eq!(
            tallies(all.iter()),
            [
                ("gate continue".to_string(), 2),
                ("decide".to_string(), 1),
                ("gate deny".to_string(), 1)
            ]
        );
    }

    #[test]
    fn the_audit_says_whether_it_verifies_and_whether_the_writer_keeps_up() {
        let a = AuditView {
            entries: vec![],
            broken_at: None,
            pending: 2,
            last_error: None,
        };
        let (status, _, writer) = audit_status(&a);
        assert!(status.starts_with("Verified: all 0"));
        assert_eq!(writer.unwrap().0, "2 records waiting to be written.");
        let broken = AuditView {
            broken_at: Some(7),
            pending: 0,
            last_error: Some("disk".into()),
            ..a
        };
        let (status, _, writer) = audit_status(&broken);
        assert!(status.starts_with("Broken: entry 7"));
        assert!(writer.unwrap().0.ends_with("disk"));
    }

    #[test]
    fn every_servers_entries_show_together_newest_first() {
        let chain = |entries| AuditView {
            entries,
            broken_at: None,
            pending: 0,
            last_error: None,
        };
        let read = vec![
            (
                "local".to_string(),
                chain(vec![entry("gate", "a", 1), entry("gate", "b", 5)]),
            ),
            ("team".to_string(), chain(vec![entry("decide", "c", 3)])),
        ];
        let order: Vec<(&str, u64)> = merged(&read).iter().map(|(s, e)| (*s, e.at)).collect();
        assert_eq!(order, [("local", 5), ("team", 3), ("local", 1)]);
    }
}
