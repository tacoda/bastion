//! A workspace's charter (D58): what its repository brings in `AGENTS.md`
//! and `.agents/`, for that workspace's jobs. A person edits a file here,
//! and the server commits the edit on a new branch, never on the base, so
//! it applies once merged.

use bastion_protocol::api;
use bastion_protocol::{CharterChange, WorkspaceCharterView};
use iced::widget::{Column, button, column, row, text, text_editor};
use iced::{Element, Task};

use super::wrap;
use crate::app::Message;
use crate::pages::Ctx;
use crate::pages::settings::small;
use crate::theme;

/// The file being edited, if any, and what the last save said.
#[derive(Default)]
pub struct State {
    open: Option<Open>,
    said: String,
    /// The workspace the last save was for.
    said_for: String,
}

struct Open {
    workspace: String,
    path: String,
    text: text_editor::Content,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Edit(String, String, String),
    Text(text_editor::Action),
    Save,
    Cancel,
    Saved(Result<String, String>),
}

fn send(msg: Msg) -> Message {
    wrap(super::Msg::Charter(msg))
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Edit(workspace, path, text) => {
            let text = text_editor::Content::with_text(&text);
            state.open = Some(Open {
                workspace,
                path,
                text,
            });
            state.said.clear();
        }
        Msg::Text(action) => {
            if let Some(open) = &mut state.open {
                open.text.perform(action);
            }
        }
        Msg::Cancel => state.open = None,
        Msg::Save => {
            let Some(open) = state.open.take() else {
                return Task::none();
            };
            state.said_for = open.workspace.clone();
            let change = CharterChange {
                workspace: open.workspace,
                path: open.path,
                text: open.text.text(),
            };
            return ctx
                .task(&ctx.current, api::CharterEdit(change), Msg::Saved)
                .map(send);
        }
        Msg::Saved(Ok(branch)) => {
            state.said = format!("Committed on the branch {branch}. Merge it for jobs to use it.")
        }
        Msg::Saved(Err(e)) => state.said = e,
    }
    Task::none()
}

/// One kind of item: a muted label and its lines, or nothing when the
/// charter has none.
fn part<'a>(label: &'a str, lines: &[String]) -> Option<Element<'a, Message>> {
    if lines.is_empty() {
        return None;
    }
    let rows = lines.iter().map(|l| text(l.clone()).size(13).into());
    let lines = Column::with_children(rows).spacing(2);
    let label = text(label).size(12).color(theme::muted()).width(90);
    Some(row![label, lines].spacing(10).into())
}

/// The files a person may edit, each with its Edit link.
fn files<'a>(c: &WorkspaceCharterView) -> Element<'a, Message> {
    let links = c.texts.iter().map(|(path, body)| {
        let open = Msg::Edit(c.workspace.clone(), path.clone(), body.clone());
        button(text(path.clone()).size(13))
            .style(theme::link)
            .on_press(send(open))
            .into()
    });
    let label = text("Edit").size(12).color(theme::muted()).width(90);
    row![label, row(links).spacing(4).wrap()].spacing(10).into()
}

/// The open file's editor, with Save and Cancel.
fn editor(open: &Open) -> Element<'_, Message> {
    let body = text_editor(&open.text)
        .on_action(|a| send(Msg::Text(a)))
        .height(240);
    let actions = row![
        small("Save on a new branch", Some(send(Msg::Save))),
        small("Cancel", Some(send(Msg::Cancel))),
    ]
    .spacing(8);
    column![text(open.path.clone()).font(theme::MONO), body, actions]
        .spacing(6)
        .into()
}

/// The workspace's charter, or a line that says it has none.
pub fn view<'a>(
    charter: Option<&'a WorkspaceCharterView>,
    state: &'a State,
) -> Element<'a, Message> {
    let Some(c) = charter.filter(|c| !c.files.is_empty() || !c.texts.is_empty()) else {
        return text("No charter.").size(13).color(theme::muted()).into();
    };
    let parts = [
        part("Charter", &c.files),
        part("Allows", &c.allow),
        part("Rules", &c.rules),
        part("Hooks", &c.hooks),
        part("Skills", &c.skills),
        part("Agents", &c.agents),
    ];
    let open = state.open.as_ref().filter(|o| o.workspace == c.workspace);
    let mine = state.said_for == c.workspace && !state.said.is_empty();
    let said = mine.then(|| text(&state.said).size(13).color(theme::muted()));
    column(parts.into_iter().flatten())
        .push(files(c))
        .extend(open.map(editor))
        .extend(said.map(Element::from))
        .spacing(6)
        .into()
}
