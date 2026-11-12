//! MCP servers (D52): each one's tools are functions an agent may call
//! once a step grants them. A server's token stays in the vault; the page
//! names its secret, never the token.

use bastion_protocol::McpServerView;
use bastion_protocol::api::{self, Request};
use iced::widget::{Column, row, text, text_input};
use iced::{Element, Fill, Task};

use super::{card, small};
use crate::app::Message;
use crate::pages::Ctx;
use crate::theme;

#[derive(Default)]
pub struct State {
    servers: Vec<McpServerView>,
    new: McpServerView,
    note: String,
}

#[derive(Clone, Copy, Debug)]
pub enum Field {
    Id,
    Url,
    Secret,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Result<Vec<McpServerView>, String>),
    Type(Field, String),
    Add,
    Remove(String),
    Done(Result<(), String>),
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Mcp(msg))
}

fn task<R>(ctx: &Ctx, request: R, then: fn(Result<R::Answer, String>) -> Msg) -> Task<Message>
where
    R: Request + Send + Sync + 'static,
    R::Answer: Send + 'static,
{
    ctx.task(&ctx.current, request, then).map(wrap)
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    task(ctx, api::McpServers, Msg::Read)
}

/// The list with `server` added, or with the server of that id replaced.
fn with(mut servers: Vec<McpServerView>, server: McpServerView) -> Vec<McpServerView> {
    servers.retain(|s| s.id != server.id);
    servers.push(server);
    servers
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    let save = |servers| task(ctx, api::McpServersSet(servers), Msg::Done);
    match msg {
        Msg::Read(Ok(all)) => state.servers = all,
        Msg::Type(Field::Id, v) => state.new.id = v,
        Msg::Type(Field::Url, v) => state.new.url = v,
        Msg::Type(Field::Secret, v) => state.new.secret = v,
        Msg::Add => return save(with(state.servers.clone(), std::mem::take(&mut state.new))),
        Msg::Remove(id) => {
            let kept = state.servers.iter().filter(|s| s.id != id).cloned();
            return save(kept.collect());
        }
        Msg::Done(Ok(())) => {
            state.note.clear();
            return load(ctx);
        }
        Msg::Read(Err(e)) | Msg::Done(Err(e)) => state.note = e,
    }
    Task::none()
}

fn line(s: &McpServerView) -> Element<'_, Message> {
    let secret = match s.secret.is_empty() {
        true => "no token".to_string(),
        false => format!("token in {}", s.secret),
    };
    let about = format!("{} · {secret} · {}", s.url, s.status);
    row![
        text(&s.id).size(13).width(140),
        text(about).size(12).color(theme::MUTED).width(Fill),
        small("Remove", Some(wrap(Msg::Remove(s.id.clone())))),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

pub fn view(state: &State) -> Element<'_, Message> {
    let lede = "Each tool of a server is the function mcp::<id>::<tool>. A step's grant on mcp::<id>::* lets its agent call them. Only servers over HTTP.";
    let n = &state.new;
    let field = |hint: &'static str, value: &str, f: Field| {
        text_input(hint, value)
            .on_input(move |v| wrap(Msg::Type(f, v)))
            .style(theme::field)
    };
    let ready = !n.id.trim().is_empty() && !n.url.trim().is_empty();
    let form = row![
        field("Id, such as docs", &n.id, Field::Id).width(160),
        field(
            "URL, such as https://mcp.example.com/mcp",
            &n.url,
            Field::Url
        ),
        field("Secret with its token, if any", &n.secret, Field::Secret).width(220),
        small("Add", ready.then(|| wrap(Msg::Add))),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let mut body = Column::new().push(text(lede).size(12).color(theme::MUTED));
    body = body.extend(state.servers.iter().map(line)).push(form);
    if !state.note.is_empty() {
        body = body.push(text(&state.note).size(13).color(theme::BAD));
    }
    card("MCP servers", body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_a_server_with_a_listed_id_replaces_it() {
        let server = |id: &str, url: &str| McpServerView {
            id: id.into(),
            url: url.into(),
            ..McpServerView::default()
        };
        let all = with(
            vec![server("a", "http://1"), server("b", "http://2")],
            server("a", "http://3"),
        );
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].url, "http://3");
    }
}
