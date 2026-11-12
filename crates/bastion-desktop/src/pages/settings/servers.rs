//! The Bastion servers this window knows (D30): the local one, and each
//! remote one with its URL and token.

use bastion_protocol::{LOCAL, Server};
use iced::widget::{Column, button, column, row, text, text_input};
use iced::{Element, Task};

use super::{card, small};
use crate::app::Message;
use crate::pages::{Ctx, labeled};
use crate::{start, theme};

pub struct State {
    server: Server,
    note: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            server: blank(),
            note: String::new(),
        }
    }
}

fn blank() -> Server {
    Server {
        name: String::new(),
        url: String::new(),
        token: String::new(),
    }
}

/// A field of the server being added.
#[derive(Clone, Copy, Debug)]
pub enum Field {
    Name,
    Url,
    Token,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Edit(Field, String),
    Add,
    Remove(String),
    Changed(Result<(), String>),
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Servers(msg))
}

/// Add or remove a server in the window's own list and the Keychain, off
/// the window's thread, as the Keychain can take seconds.
fn change(
    ctx: &Ctx,
    change: impl FnOnce(crate::servers::Servers) -> Result<(), String> + Send + 'static,
) -> Task<Message> {
    let local = ctx.servers.iter().find(|s| s.name == LOCAL).cloned();
    let work = async move {
        let local = local.ok_or("the local Bastion server has not started")?;
        tokio::task::spawn_blocking(move || change(start::servers(local)))
            .await
            .map_err(|e| e.to_string())?
    };
    Task::perform(work, |r| wrap(Msg::Changed(r)))
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    let s = &mut state.server;
    match msg {
        Msg::Edit(Field::Name, v) => s.name = v,
        Msg::Edit(Field::Url, v) => s.url = v,
        Msg::Edit(Field::Token, v) => s.token = v,
        Msg::Add => {
            let server = std::mem::replace(s, blank());
            return change(ctx, move |all| all.add(server));
        }
        Msg::Remove(name) => return change(ctx, move |all| all.remove(&name)),
        Msg::Changed(Ok(())) => {
            state.note.clear();
            return Task::done(Message::ReadServers);
        }
        Msg::Changed(Err(e)) => state.note = e,
    }
    Task::none()
}

fn field<'a>(label: &'a str, hint: &'a str, value: &str, f: Field) -> Element<'a, Message> {
    let input = text_input(hint, value)
        .on_input(move |v| wrap(Msg::Edit(f, v)))
        .style(theme::field)
        .width(200)
        .secure(matches!(f, Field::Token));
    labeled(label, input)
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let rows = ctx.servers.iter().map(|s| {
        let remove = (s.name != LOCAL).then(|| {
            button(text("Remove").size(13).color(theme::BAD))
                .style(theme::link)
                .on_press(wrap(Msg::Remove(s.name.clone())))
        });
        row![
            text(s.name.clone()).font(theme::BOLD),
            text(s.url.clone()).color(theme::MUTED)
        ]
        .extend(remove.map(Element::from))
        .spacing(10)
        .align_y(iced::Center)
        .into()
    });
    let s = &state.server;
    let form = row![
        field("Name", "", &s.name, Field::Name),
        field("URL", "https://bastion.example.com", &s.url, Field::Url),
        field("Token", "", &s.token, Field::Token),
        small("Add", Some(wrap(Msg::Add))),
    ];
    card(
        "Bastion servers",
        column![
            Column::with_children(rows).spacing(6),
            form.spacing(8).align_y(iced::Bottom),
            text(&state.note).size(13).color(theme::BAD)
        ],
    )
}
