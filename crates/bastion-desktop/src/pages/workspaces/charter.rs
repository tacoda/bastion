//! A workspace's charter, read-only (D58): what its repository brings in
//! `AGENTS.md` and `.agents/`, for that workspace's jobs. Bastion applies
//! it; the repository changes it.

use bastion_protocol::WorkspaceCharterView;
use iced::Element;
use iced::widget::{Column, column, row, text};

use crate::app::Message;
use crate::theme;

/// One kind of item: a muted label and its lines, or nothing when the
/// charter has none.
fn part<'a>(label: &'a str, lines: &[String]) -> Option<Element<'a, Message>> {
    if lines.is_empty() {
        return None;
    }
    let rows = lines.iter().map(|l| text(l.clone()).size(13).into());
    let lines = Column::with_children(rows).spacing(2);
    let label = text(label).size(12).color(theme::MUTED).width(90);
    Some(row![label, lines].spacing(10).into())
}

/// The workspace's charter, or a line that says it has none.
pub fn view(charter: Option<&WorkspaceCharterView>) -> Element<'_, Message> {
    let Some(c) = charter.filter(|c| !c.files.is_empty()) else {
        return text("No charter.").size(13).color(theme::MUTED).into();
    };
    let parts = [
        part("Charter", &c.files),
        part("Allows", &c.allow),
        part("Rules", &c.rules),
        part("Hooks", &c.hooks),
        part("Skills", &c.skills),
        part("Agents", &c.agents),
    ];
    column(parts.into_iter().flatten()).spacing(6).into()
}
