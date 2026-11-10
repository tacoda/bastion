//! Where jobs run: the Bastion servers this window knows (D30), the
//! workspaces on the server shown, and the accounts it uses.

use bastion_protocol::api::{self, Request};
use bastion_protocol::{AccountView, LOCAL, Server, Workspace};
use iced::widget::{Column, button, column, row, text, text_input};
use iced::{Element, Task};

use super::{card, small};
use crate::app::Message;
use crate::pages::{Ctx, labeled};
use crate::{start, theme};

pub struct State {
    server: Server,
    workspace: Workspace,
    workspaces: Vec<Workspace>,
    accounts: Vec<AccountView>,
    note: String,
}

impl Default for State {
    fn default() -> Self {
        let server = Server {
            name: String::new(),
            url: String::new(),
            token: String::new(),
        };
        let workspace = Workspace {
            name: String::new(),
            path: String::new(),
            branch: "main".into(),
            checks: String::new(),
        };
        State {
            server,
            workspace,
            workspaces: vec![],
            accounts: vec![],
            note: String::new(),
        }
    }
}

/// A field of the server or workspace being added.
#[derive(Clone, Copy, Debug)]
pub enum Field {
    ServerName,
    ServerUrl,
    ServerToken,
    Name,
    Path,
    Branch,
    Checks,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Workspaces(Result<Vec<Workspace>, String>),
    Accounts(Result<Vec<AccountView>, String>),
    Edit(Field, String),
    AddServer,
    RemoveServer(String),
    ServersChanged(Result<(), String>),
    AddWorkspace,
    Done(Result<(), String>),
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Places(msg))
}

fn task<R>(ctx: &Ctx, request: R, then: fn(Result<R::Answer, String>) -> Msg) -> Task<Message>
where
    R: Request + Send + Sync + 'static,
    R::Answer: Send + 'static,
{
    ctx.task(&ctx.current, request, then).map(wrap)
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    Task::batch([
        task(ctx, api::Workspaces, Msg::Workspaces),
        task(ctx, api::Accounts, Msg::Accounts),
    ])
}

/// Add or remove a server in the window's own list and the Keychain, off
/// the window's thread, as the Keychain can take seconds.
fn change_servers(
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
    Task::perform(work, |r| wrap(Msg::ServersChanged(r)))
}

fn edit(state: &mut State, field: Field, value: String) {
    let (s, w) = (&mut state.server, &mut state.workspace);
    match field {
        Field::ServerName => s.name = value,
        Field::ServerUrl => s.url = value,
        Field::ServerToken => s.token = value,
        Field::Name => w.name = value,
        Field::Path => w.path = value,
        Field::Branch => w.branch = value,
        Field::Checks => w.checks = value,
    }
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Workspaces(Ok(all)) => state.workspaces = all,
        Msg::Accounts(Ok(all)) => state.accounts = all,
        Msg::Workspaces(Err(e))
        | Msg::Accounts(Err(e))
        | Msg::Done(Err(e))
        | Msg::ServersChanged(Err(e)) => state.note = e,
        Msg::Edit(f, v) => edit(state, f, v),
        Msg::AddServer => {
            let server = std::mem::replace(&mut state.server, State::default().server);
            return change_servers(ctx, move |all| all.add(server));
        }
        Msg::RemoveServer(name) => return change_servers(ctx, move |all| all.remove(&name)),
        Msg::ServersChanged(Ok(())) => {
            state.note.clear();
            return Task::done(Message::ReadServers);
        }
        Msg::AddWorkspace => {
            let workspace = std::mem::replace(&mut state.workspace, State::default().workspace);
            return task(ctx, api::WorkspaceAdd(workspace), Msg::Done);
        }
        Msg::Done(Ok(_)) => {
            state.note.clear();
            return load(ctx);
        }
    }
    Task::none()
}

fn field<'a>(label: &'a str, hint: &'a str, value: &str, f: Field) -> Element<'a, Message> {
    let input = text_input(hint, value)
        .on_input(move |v| wrap(Msg::Edit(f, v)))
        .style(theme::field)
        .width(200);
    labeled(
        label,
        if matches!(f, Field::ServerToken) {
            input.secure(true)
        } else {
            input
        },
    )
}

fn servers<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let rows = ctx.servers.iter().map(|s| {
        let remove = (s.name != LOCAL).then(|| {
            button(text("Remove").size(13).color(theme::BAD))
                .style(theme::link)
                .on_press(wrap(Msg::RemoveServer(s.name.clone())))
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
        field("Name", "", &s.name, Field::ServerName),
        field("URL", "http://127.0.0.1:49191", &s.url, Field::ServerUrl),
        field("Token", "", &s.token, Field::ServerToken),
        small("Add", Some(wrap(Msg::AddServer))),
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

fn workspaces(state: &State) -> Element<'_, Message> {
    let lede = "The repositories jobs run against on this Bastion server. Each needs its branch; its checks command runs its tests at the checks step.";
    let rows = state.workspaces.iter().map(|w| {
        let checks = (!w.checks.is_empty())
            .then(|| text(w.checks.clone()).font(theme::MONO).color(theme::MUTED));
        row![
            text(w.name.clone()),
            text(format!("{} @ {}", w.path, w.branch)).color(theme::MUTED)
        ]
        .extend(checks.map(Element::from))
        .spacing(10)
        .into()
    });
    let w = &state.workspace;
    let form = row![
        field("Name", "", &w.name, Field::Name),
        field("Repository path", "", &w.path, Field::Path),
        field("Branch", "", &w.branch, Field::Branch),
        field("Checks", "pytest -q", &w.checks, Field::Checks),
        small("Add", Some(wrap(Msg::AddWorkspace))),
    ];
    card(
        "Workspaces",
        column![
            text(lede).size(13).color(theme::MUTED),
            Column::with_children(rows).spacing(4),
            form.spacing(8).align_y(iced::Bottom)
        ],
    )
}

fn accounts(state: &State) -> Element<'_, Message> {
    let rows = state.accounts.iter().map(|a| {
        let mark =
            text(if a.ok { "✓ " } else { "! " }).color(if a.ok { theme::OK } else { theme::WARN });
        row![
            mark,
            text(a.name.clone()).font(theme::BOLD),
            text(format!(" · {}", a.status)).color(theme::MUTED)
        ]
        .into()
    });
    card("Accounts", Column::with_children(rows).spacing(4))
}

/// The servers first, then the workspaces and accounts, as the page orders them.
pub fn view<'a>(state: &'a State, ctx: &Ctx) -> (Element<'a, Message>, [Element<'a, Message>; 2]) {
    (servers(state, ctx), [workspaces(state), accounts(state)])
}
