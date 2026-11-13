//! A server's plugins on the Guide (D62): each installed at a pinned
//! commit, with its parts listed, off. Installing one adds nothing to a
//! journey.

use bastion_protocol::{PluginView, api};
use iced::widget::{Column, column, container, row, text, text_input};
use iced::{Element, Fill, Task};

use super::Ctx;
use crate::theme;

/// One server's plugins and its install form.
#[derive(Default)]
pub struct Shelf {
    plugins: Vec<PluginView>,
    url: String,
    commit: String,
    busy: bool,
    error: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Result<Vec<PluginView>, String>),
    Url(String),
    Commit(String),
    Install,
    Remove(String),
    /// An install or a remove ended: read the shelf again, or say why not.
    Done(Result<(), String>),
}

/// Read the plugins on the server `ctx` is on.
pub fn load(ctx: &Ctx) -> Task<Msg> {
    ctx.task(&ctx.current, api::Plugins, Msg::Read)
}

pub fn update(shelf: &mut Shelf, msg: Msg, ctx: &Ctx) -> Task<Msg> {
    match msg {
        Msg::Read(read) => match read {
            Ok(all) => shelf.plugins = all,
            Err(e) => shelf.error = e,
        },
        Msg::Url(v) => shelf.url = v,
        Msg::Commit(v) => shelf.commit = v,
        Msg::Install => {
            shelf.busy = true;
            let ask = api::PluginInstall {
                url: shelf.url.trim().into(),
                commit: shelf.commit.trim().into(),
            };
            return ctx.task(&ctx.current, ask, |r| Msg::Done(r.map(|_| ())));
        }
        Msg::Remove(name) => {
            shelf.busy = true;
            let remove = api::PluginRemove { name };
            return ctx.task(&ctx.current, remove, Msg::Done);
        }
        Msg::Done(done) => {
            shelf.busy = false;
            match done {
                Ok(()) => {
                    (shelf.url, shelf.commit, shelf.error) = Default::default();
                    return load(ctx);
                }
                Err(e) => shelf.error = e,
            }
        }
    }
    Task::none()
}

/// One plugin: its name, commit and source, then its parts, each off.
fn plugin<'a>(p: &'a PluginView, admin: bool) -> Element<'a, Msg> {
    let short = &p.commit[..p.commit.len().min(7)];
    let head = column![
        text(format!("{} @ {short}", p.name))
            .size(14)
            .font(theme::BOLD),
        text(format!("{} · {}", p.description, p.url))
            .size(12)
            .color(theme::muted())
    ]
    .spacing(2)
    .width(Fill);
    let remove = admin.then(|| {
        crate::lift::button(text("Remove").size(13))
            .style(theme::link)
            .on_press(Msg::Remove(p.name.clone()))
    });
    let parts = p.parts.iter().map(|part| {
        let what = match part.detail.is_empty() {
            true => part.name.clone(),
            false => format!("{}: {}", part.name, part.detail),
        };
        row![
            container(text(part.kind.clone()).size(11).color(theme::muted()))
                .padding([2, 8])
                .style(theme::chip),
            text(what).size(13).width(Fill),
            text("off").size(12).color(theme::muted())
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    });
    column![row![head].extend(remove.map(Element::from))]
        .extend(parts)
        .spacing(6)
        .into()
}

/// The install form: a repository's https URL and the commit to pin.
fn form(shelf: &Shelf) -> Element<'_, Msg> {
    let url = text_input("https://github.com/owner/plugin", &shelf.url)
        .on_input(Msg::Url)
        .size(13);
    let commit = text_input("Commit to pin", &shelf.commit)
        .on_input(Msg::Commit)
        .size(13)
        .width(220);
    let ready = !shelf.busy && !shelf.url.trim().is_empty() && !shelf.commit.trim().is_empty();
    let label = if shelf.busy {
        "Installing…"
    } else {
        "Install"
    };
    let install = crate::lift::button(text(label).size(13))
        .style(theme::primary)
        .on_press_maybe(ready.then_some(Msg::Install));
    row![url, commit, install]
        .spacing(6)
        .align_y(iced::Center)
        .into()
}

pub fn view(shelf: &Shelf, admin: bool) -> Element<'_, Msg> {
    let head = row![
        text("Plugins").size(15).font(theme::BOLD),
        text("Installed on this server at a pinned commit. Each part is off until applied.")
            .size(12)
            .color(theme::muted())
    ]
    .spacing(10)
    .align_y(iced::Center);
    let mut body = Column::new()
        .spacing(14)
        .extend(shelf.plugins.iter().map(|p| plugin(p, admin)));
    if shelf.plugins.is_empty() {
        body = body.push(text("No plugins yet.").size(13).color(theme::muted()));
    }
    if admin {
        body = body.push(form(shelf));
    }
    if !shelf.error.is_empty() {
        body = body.push(text(&shelf.error).size(13).color(theme::bad()));
    }
    container(column![head, body].spacing(10))
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}
