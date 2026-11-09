//! People on a Bastion server and their tokens (D31). Only an admin on a
//! remote server manages them; a token is shown once, when it is issued.

use bastion_protocol::{LOCAL, PersonView, TokenView, command};
use iced::widget::{Column, button, column, pick_list, row, text, text_input};
use iced::{Element, Task};
use serde_json::{Value, json};

use super::{card, small};
use crate::app::Message;
use crate::client::Ask;
use crate::pages::{Ctx, labeled};
use crate::theme;

const ROLES: [&str; 3] = ["admin", "member", "viewer"];

pub struct State {
    me: Option<PersonView>,
    people: Vec<PersonView>,
    tokens: Vec<TokenView>,
    name: String,
    role: String,
    issued: String,
    note: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            me: None,
            people: vec![],
            tokens: vec![],
            name: String::new(),
            role: "member".into(),
            issued: String::new(),
            note: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Me(Result<PersonView, String>),
    People(Result<Vec<PersonView>, String>),
    Tokens(Result<Vec<TokenView>, String>),
    Name(String),
    Role(String),
    Add,
    ChangeRole(String, String),
    Issue(String),
    Issued(String, Result<String, String>),
    Revoke(String),
    Done(Result<Value, String>),
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::People(msg))
}

fn task<T>(ctx: &Ctx, ask: Ask, then: fn(Result<T, String>) -> Msg) -> Task<Message>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    ctx.task(&ctx.current, ask, then).map(wrap)
}

/// Whether `me` manages people here: an admin, on a remote Bastion server.
/// `local` listens only on this machine, so nobody else can sign in to it.
fn manages(server: &str, me: Option<&PersonView>) -> bool {
    server != LOCAL && me.is_some_and(|p| p.role == "admin")
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    task(ctx, Ask::bare(command::ME), Msg::Me)
}

fn on_me(state: &mut State, ctx: &Ctx, me: PersonView) -> Task<Message> {
    let manager = manages(&ctx.current, Some(&me));
    state.me = Some(me);
    if !manager {
        return Task::none();
    }
    Task::batch([
        task(ctx, Ask::bare(command::PEOPLE), Msg::People),
        task(ctx, Ask::bare(command::TOKENS), Msg::Tokens),
    ])
}

fn issue(ctx: &Ctx, name: String) -> Task<Message> {
    let ask = Ask {
        cmd: command::TOKEN_ISSUE,
        args: json!({ "name": name, "label": "issued in Settings" }),
    };
    let read = ctx.clone().get::<String>(ctx.current.clone(), ask);
    Task::perform(read, move |token| wrap(Msg::Issued(name.clone(), token)))
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Me(Ok(me)) => return on_me(state, ctx, me),
        Msg::People(Ok(all)) => state.people = all,
        Msg::Tokens(Ok(all)) => state.tokens = all,
        Msg::Me(Err(e)) | Msg::People(Err(e)) | Msg::Tokens(Err(e)) | Msg::Done(Err(e)) => {
            state.note = e
        }
        Msg::Name(n) => state.name = n,
        Msg::Role(r) => state.role = r,
        Msg::Add => {
            let person = json!({ "person": { "name": state.name.trim(), "role": state.role } });
            state.name.clear();
            return task(
                ctx,
                Ask {
                    cmd: command::PERSON_ADD,
                    args: person,
                },
                Msg::Done,
            );
        }
        Msg::ChangeRole(name, role) => {
            return task(
                ctx,
                Ask {
                    cmd: command::PERSON_ROLE,
                    args: json!({ "name": name, "role": role }),
                },
                Msg::Done,
            );
        }
        Msg::Issue(name) => return issue(ctx, name),
        Msg::Issued(name, Ok(token)) => {
            state.issued = format!("{name}'s new token, shown only now: {token}");
            return load(ctx);
        }
        Msg::Issued(_, Err(e)) => state.note = e,
        Msg::Revoke(id) => {
            return task(
                ctx,
                Ask {
                    cmd: command::TOKEN_REVOKE,
                    args: json!({ "id": id }),
                },
                Msg::Done,
            );
        }
        Msg::Done(Ok(_)) => {
            state.note.clear();
            return load(ctx);
        }
    }
    Task::none()
}

fn roles() -> Vec<String> {
    ROLES.iter().map(|r| r.to_string()).collect()
}

fn person_row<'a>(p: &PersonView) -> Element<'a, Message> {
    let name = p.name.clone();
    let role = pick_list(roles(), Some(p.role.clone()), move |r| {
        wrap(Msg::ChangeRole(name.clone(), r))
    })
    .style(theme::select);
    let token = button(text("Issue a token").size(13).color(theme::LINK))
        .style(theme::link)
        .on_press(wrap(Msg::Issue(p.name.clone())));
    row![text(p.name.clone()).font(theme::BOLD), role, token]
        .spacing(10)
        .align_y(iced::Center)
        .into()
}

fn token_row<'a>(t: &TokenView) -> Element<'a, Message> {
    let revoke = button(text("Revoke").size(13).color(theme::BAD))
        .style(theme::link)
        .on_press(wrap(Msg::Revoke(t.id.clone())));
    row![
        text(t.id.clone()).font(theme::MONO),
        text(t.person.clone()),
        text(t.label.clone()).color(theme::MUTED),
        revoke
    ]
    .spacing(10)
    .align_y(iced::Center)
    .into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let who = state
        .me
        .as_ref()
        .map(|p| format!("You are {}, a {} on this Bastion server.", p.name, p.role))
        .unwrap_or_default();
    let mut body = column![text(who).size(13).color(theme::MUTED)];
    if manages(&ctx.current, state.me.as_ref()) {
        let name = text_input("", &state.name)
            .on_input(|n| wrap(Msg::Name(n)))
            .style(theme::field)
            .width(200);
        let role = pick_list(roles(), Some(state.role.clone()), |r| wrap(Msg::Role(r)))
            .style(theme::select);
        body = body
            .push(Column::with_children(state.people.iter().map(person_row)).spacing(6))
            .push(
                row![
                    labeled("Name", name),
                    labeled("Role", role),
                    small("Add", Some(wrap(Msg::Add)))
                ]
                .spacing(8)
                .align_y(iced::Bottom),
            )
            .push(text(&state.issued).size(13).color(theme::OK))
            .push(
                text("Tokens")
                    .size(13)
                    .font(theme::BOLD)
                    .color(theme::MUTED),
            )
            .push(Column::with_children(state.tokens.iter().map(token_row)).spacing(6));
    }
    card(
        "People",
        body.push(text(&state.note).size(13).color(theme::BAD)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_admin_on_a_remote_server_manages_people() {
        let admin = PersonView {
            name: "ana".into(),
            role: "admin".into(),
        };
        let member = PersonView {
            role: "member".into(),
            ..admin.clone()
        };
        assert!(manages("box", Some(&admin)));
        assert!(!manages(LOCAL, Some(&admin)));
        assert!(!manages("box", Some(&member)));
        assert!(!manages("box", None));
    }
}
