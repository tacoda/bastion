//! Model providers (D34) and OpenAI-compatible hosts (K3). A key is sent
//! once, to the stack's vault, and never shown again. OpenAI Codex signs in
//! with a ChatGPT plan by a device code the page polls until it ends.

use std::collections::BTreeMap;
use std::time::Duration;

use bastion_protocol::{HostView, ProviderView, SignIn, command};
use iced::widget::{Column, button, column, row, text, text_input};
use iced::{Element, Fill, Task};
use serde_json::{Value, json};

use super::{card, small};
use crate::app::Message;
use crate::client::Ask;
use crate::pages::Ctx;
use crate::theme;

#[derive(Default)]
pub struct State {
    providers: Vec<ProviderView>,
    /// What the owner typed as each provider's key, until Set sends it.
    keys: BTreeMap<String, String>,
    signing_in: Option<SignIn>,
    hosts: Vec<HostView>,
    new_host: HostView,
    note: String,
}

#[derive(Clone, Copy, Debug)]
pub enum HostField {
    Id,
    Name,
    Url,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Providers(Result<Vec<ProviderView>, String>),
    Hosts(Result<Vec<HostView>, String>),
    Key(String, String),
    SetKey(String),
    RemoveKey(String),
    SignIn,
    Started(Result<SignIn, String>),
    Polled(Result<String, String>),
    SignOut,
    HostField(HostField, String),
    AddHost,
    RemoveHost(String),
    Done(Result<Value, String>),
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Providers(msg))
}

fn task<T>(ctx: &Ctx, ask: Ask, then: fn(Result<T, String>) -> Msg) -> Task<Message>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    ctx.task(&ctx.current, ask, then).map(wrap)
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    Task::batch([
        task(ctx, Ask::bare(command::PROVIDERS), Msg::Providers),
        task(ctx, Ask::bare(command::HOSTS), Msg::Hosts),
    ])
}

/// The host list with `host` added, or with the host of that id replaced.
fn with_host(mut hosts: Vec<HostView>, host: HostView) -> Vec<HostView> {
    hosts.retain(|h| h.id != host.id);
    hosts.push(host);
    hosts
}

fn save_hosts(ctx: &Ctx, hosts: Vec<HostView>) -> Task<Message> {
    task(
        ctx,
        Ask {
            cmd: command::HOSTS_SET,
            args: json!({ "hosts": hosts }),
        },
        Msg::Done,
    )
}

/// Wait the sign-in's interval, then ask how it went.
fn poll(ctx: &Ctx, s: &SignIn) -> Task<Message> {
    let (wait, ask) = (
        Duration::from_secs(s.interval.max(1)),
        Ask {
            cmd: command::CODEX_SIGN_IN_POLL,
            args: json!({ "login_id": s.login_id }),
        },
    );
    let read = ctx.clone().get::<String>(ctx.current.clone(), ask);
    Task::perform(
        async move {
            tokio::time::sleep(wait).await;
            read.await
        },
        |r| wrap(Msg::Polled(r)),
    )
}

fn on_polled(state: &mut State, ctx: &Ctx, answer: Result<String, String>) -> Task<Message> {
    match answer.as_deref() {
        Ok("pending") => {
            return state
                .signing_in
                .as_ref()
                .map_or_else(Task::none, |s| poll(ctx, s));
        }
        Ok("ok") => state.note.clear(),
        Ok(other) => state.note = format!("The ChatGPT sign-in ended: {other}. Try again."),
        Err(e) => state.note = e.clone(),
    }
    state.signing_in = None;
    load(ctx)
}

fn on_host(state: &mut State, field: HostField, value: String) {
    let h = &mut state.new_host;
    match field {
        HostField::Id => h.id = value,
        HostField::Name => h.name = value,
        HostField::Url => h.api_url = value,
    }
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Providers(Ok(all)) => state.providers = all,
        Msg::Hosts(Ok(all)) => state.hosts = all,
        Msg::Providers(Err(e)) | Msg::Hosts(Err(e)) | Msg::Done(Err(e)) | Msg::Started(Err(e)) => {
            state.note = e
        }
        Msg::Key(id, key) => {
            state.keys.insert(id, key);
        }
        Msg::SetKey(id) => {
            let key = state.keys.remove(&id).unwrap_or_default();
            return task(
                ctx,
                Ask {
                    cmd: command::PROVIDER_KEY_SET,
                    args: json!({ "id": id, "key": key }),
                },
                Msg::Done,
            );
        }
        Msg::RemoveKey(id) => {
            return task(
                ctx,
                Ask {
                    cmd: command::PROVIDER_KEY_REMOVE,
                    args: json!({ "id": id }),
                },
                Msg::Done,
            );
        }
        Msg::SignIn => return task(ctx, Ask::bare(command::CODEX_SIGN_IN), Msg::Started),
        Msg::Started(Ok(s)) => {
            let next = poll(ctx, &s);
            state.signing_in = Some(s);
            return next;
        }
        Msg::Polled(answer) => return on_polled(state, ctx, answer),
        Msg::SignOut => return task(ctx, Ask::bare(command::CODEX_SIGN_OUT), Msg::Done),
        Msg::HostField(f, v) => on_host(state, f, v),
        Msg::AddHost => {
            let host = std::mem::take(&mut state.new_host);
            return save_hosts(ctx, with_host(state.hosts.clone(), host));
        }
        Msg::RemoveHost(id) => {
            return save_hosts(
                ctx,
                state.hosts.iter().filter(|h| h.id != id).cloned().collect(),
            );
        }
        Msg::Done(Ok(_)) => {
            state.note.clear();
            return load(ctx);
        }
    }
    Task::none()
}

/// A provider's name with a mark for whether turns can run on it.
fn name<'a>(p: &ProviderView) -> Element<'a, Message> {
    let ok = p.ready || p.source != "none";
    let mark = text(if ok { "✓" } else { "!" }).color(if ok { theme::OK } else { theme::WARN });
    row![mark, text(p.name.clone()).font(theme::BOLD)]
        .spacing(6)
        .into()
}

fn key_status(p: &ProviderView) -> &'static str {
    match p.source.as_str() {
        "secret" => "key set here",
        "env" => "key from the stack's environment",
        _ if p.ready => "no key needed",
        _ => "no key",
    }
}

fn key_row<'a>(state: &State, p: &ProviderView) -> Element<'a, Message> {
    let typed = state.keys.get(&p.id).cloned().unwrap_or_default();
    let id = p.id.clone();
    let field = text_input("API key", &typed)
        .secure(true)
        .on_input(move |k| wrap(Msg::Key(id.clone(), k)))
        .style(theme::field)
        .width(260);
    let set = small(
        "Set",
        (!typed.trim().is_empty()).then(|| wrap(Msg::SetKey(p.id.clone()))),
    );
    let remove = (p.source == "secret").then(|| {
        button(text("Remove").size(13).color(theme::BAD))
            .style(theme::link)
            .on_press(wrap(Msg::RemoveKey(p.id.clone())))
    });
    row![
        name(p),
        text(key_status(p)).size(13).color(theme::MUTED),
        field,
        set
    ]
    .extend(remove.map(Element::from))
    .spacing(10)
    .align_y(iced::Center)
    .into()
}

fn codex_row<'a>(state: &State, p: &ProviderView) -> Element<'a, Message> {
    let signed_in = p.source == "signed_in";
    let status = if signed_in {
        "signed in with ChatGPT"
    } else {
        "uses a ChatGPT plan, no key"
    };
    let action = if signed_in {
        button(text("Sign out").size(13).color(theme::BAD))
            .style(theme::link)
            .on_press(wrap(Msg::SignOut))
    } else {
        button(text("Sign in with ChatGPT").size(13))
            .padding([4, 12])
            .style(theme::secondary)
            .on_press(wrap(Msg::SignIn))
    };
    let line = row![name(p), text(status).size(13).color(theme::MUTED), action]
        .spacing(10)
        .align_y(iced::Center);
    let Some(s) = &state.signing_in else {
        return line.into();
    };
    let open = button(text(s.verification_uri.clone()).size(13).color(theme::LINK))
        .padding(0)
        .style(theme::link)
        .on_press(Message::OpenUrl(s.verification_uri.clone()));
    let waiting = row![
        text("Open ").size(13),
        open,
        text(format!(
            " and enter {}. Waiting for approval...",
            s.user_code
        ))
        .size(13)
    ];
    column![line, waiting].spacing(4).into()
}

fn providers(state: &State) -> Element<'_, Message> {
    let rows = state.providers.iter().map(|p| {
        if p.id == "openai-codex" {
            codex_row(state, p)
        } else {
            key_row(state, p)
        }
    });
    card(
        "Model providers",
        Column::with_children(rows)
            .spacing(8)
            .push(text(&state.note).size(13).color(theme::BAD)),
    )
}

fn hosts(state: &State) -> Element<'_, Message> {
    let lede = "Each host serves its models as a provider of its own. A host that needs a key gets it under Model providers.";
    let rows = state.hosts.iter().map(|h| {
        let remove = button(text("Remove").size(13).color(theme::BAD))
            .style(theme::link)
            .on_press(wrap(Msg::RemoveHost(h.id.clone())));
        row![
            text(h.name.clone()).font(theme::BOLD),
            text(format!("{} · {}", h.id, h.api_url))
                .size(13)
                .color(theme::MUTED),
            remove
        ]
        .spacing(10)
        .align_y(iced::Center)
        .into()
    });
    let h = &state.new_host;
    let field = |hint: &'static str, value: &str, f: HostField| {
        text_input(hint, value)
            .on_input(move |v| wrap(Msg::HostField(f, v)))
            .style(theme::field)
    };
    let form = row![
        field("Id, such as ollama", &h.id, HostField::Id).width(180),
        field("Name", &h.name, HostField::Name).width(180),
        field(
            "URL, such as http://localhost:11434/v1/chat/completions",
            &h.api_url,
            HostField::Url
        )
        .width(Fill),
        small("Add", Some(wrap(Msg::AddHost))),
    ]
    .spacing(8);
    card(
        "OpenAI-compatible hosts",
        column![
            text(lede).size(13).color(theme::MUTED),
            Column::with_children(rows).spacing(4),
            form
        ],
    )
}

pub fn view(state: &State) -> [Element<'_, Message>; 2] {
    [providers(state), hosts(state)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(id: &str, url: &str) -> HostView {
        HostView {
            id: id.into(),
            name: id.into(),
            api_url: url.into(),
        }
    }

    #[test]
    fn adding_a_host_with_a_listed_id_replaces_it() {
        let hosts = with_host(
            vec![host("a", "http://1"), host("b", "http://2")],
            host("a", "http://3"),
        );
        assert_eq!(
            hosts.iter().map(|h| h.api_url.as_str()).collect::<Vec<_>>(),
            ["http://2", "http://3"]
        );
    }

    #[test]
    fn a_provider_says_where_its_key_comes_from() {
        let p = |source: &str, ready: bool| ProviderView {
            id: "x".into(),
            name: "X".into(),
            source: source.into(),
            ready,
        };
        assert_eq!(key_status(&p("secret", true)), "key set here");
        assert_eq!(key_status(&p("none", true)), "no key needed");
        assert_eq!(key_status(&p("none", false)), "no key");
    }
}
