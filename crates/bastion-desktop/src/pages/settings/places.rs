//! Where jobs run: the Bastion servers this window knows (D30), the
//! workspaces on the server shown, and the accounts it uses.

use bastion_protocol::api::{self, Request};
use bastion_protocol::{AccountView, LOCAL, Server, Workspace, branch_prefix};
use iced::widget::{Column, button, column, row, text, text_input};
use iced::{Element, Task};

use super::{card, small};
use crate::app::Message;
use crate::pages::{Ctx, labeled};
use crate::{start, theme};

pub struct State {
    server: Server,
    workspace: Workspace,
    /// The form holds a workspace already added, to change it.
    editing: bool,
    /// The form's cap on running jobs, as typed; empty is none.
    max_jobs: String,
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
            max_jobs: None,
            branch_prefix: branch_prefix(),
        };
        State {
            server,
            workspace,
            editing: false,
            max_jobs: String::new(),
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
    MaxJobs,
    BranchPrefix,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Workspaces(Result<Vec<Workspace>, String>),
    Accounts(Result<Vec<AccountView>, String>),
    Edit(Field, String),
    AddServer,
    RemoveServer(String),
    ServersChanged(Result<(), String>),
    EditWorkspace(String),
    SaveWorkspace,
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
        Field::MaxJobs => state.max_jobs = value,
        Field::BranchPrefix => state.workspace.branch_prefix = value,
    }
}

/// Put a workspace in the form, to change it.
fn open(state: &mut State, name: &str) {
    let Some(w) = state.workspaces.iter().find(|w| w.name == name) else {
        return;
    };
    state.max_jobs = w.max_jobs.map(|n| n.to_string()).unwrap_or_default();
    (state.workspace, state.editing) = (w.clone(), true);
}

/// A typed cap: empty is none, otherwise a whole number above 0.
fn cap(typed: &str) -> Result<Option<u32>, String> {
    match typed.trim() {
        "" => Ok(None),
        n => n
            .parse()
            .ok()
            .filter(|n| *n > 0)
            .map(Some)
            .ok_or_else(|| format!("{n} is not a number of jobs above 0")),
    }
}

/// Add the form's workspace, or save the one it holds, then empty it.
fn save(state: &mut State, ctx: &Ctx) -> Task<Message> {
    let max_jobs = match cap(&state.max_jobs) {
        Ok(n) => n,
        Err(e) => {
            state.note = e;
            return Task::none();
        }
    };
    let blank = State::default();
    let editing = std::mem::replace(&mut state.editing, false);
    state.max_jobs.clear();
    let w = std::mem::replace(&mut state.workspace, blank.workspace);
    if !editing {
        return task(
            ctx,
            api::WorkspaceAdd(Workspace { max_jobs, ..w }),
            Msg::Done,
        );
    }
    let edit = api::WorkspaceEdit {
        name: w.name,
        branch: w.branch,
        checks: w.checks,
        max_jobs,
        branch_prefix: w.branch_prefix,
    };
    task(ctx, edit, Msg::Done)
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
        Msg::EditWorkspace(name) => open(state, &name),
        Msg::SaveWorkspace => return save(state, ctx),
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

/// A field that cannot change: a saved workspace keeps its name and path.
fn fixed<'a>(label: &'a str, value: &str) -> Element<'a, Message> {
    labeled(label, text_input("", value).style(theme::field).width(200))
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
    let lede = "The repositories jobs run against on this Bastion server. Each needs its branch; its checks command runs its tests at the checks step. A job pushes to its branch prefix and its id, and the job limit caps how many of its jobs run at once.";
    let rows = state.workspaces.iter().map(|w| {
        let checks = (!w.checks.is_empty())
            .then(|| text(w.checks.clone()).font(theme::MONO).color(theme::MUTED));
        let cap = w
            .max_jobs
            .map(|n| text(format!("at most {n} at once")).color(theme::MUTED));
        let edit = button(text("Edit").size(13))
            .style(theme::link)
            .on_press(wrap(Msg::EditWorkspace(w.name.clone())));
        row![
            text(w.name.clone()),
            text(format!("{} @ {}", w.path, w.branch)).color(theme::MUTED)
        ]
        .extend(checks.map(Element::from))
        .extend(cap.map(Element::from))
        .push(edit)
        .spacing(10)
        .into()
    });
    card(
        "Workspaces",
        column![
            text(lede).size(13).color(theme::MUTED),
            Column::with_children(rows).spacing(4),
            workspace_form(state)
        ],
    )
}

/// The form that adds a workspace, or changes the one it holds.
fn workspace_form(state: &State) -> Column<'_, Message> {
    let w = &state.workspace;
    let (name, path, label) = match state.editing {
        true => (
            fixed("Name", &w.name),
            fixed("Repository path", &w.path),
            "Save",
        ),
        false => (
            field("Name", "", &w.name, Field::Name),
            field("Repository path", "", &w.path, Field::Path),
            "Add",
        ),
    };
    let place = row![name, path, field("Branch", "", &w.branch, Field::Branch)];
    let settings = row![
        field("Checks", "pytest -q", &w.checks, Field::Checks),
        field("Branch prefix", "", &w.branch_prefix, Field::BranchPrefix),
        field("Job limit", "none", &state.max_jobs, Field::MaxJobs),
        small(label, Some(wrap(Msg::SaveWorkspace))),
    ];
    column![
        place.spacing(8).align_y(iced::Bottom),
        settings.spacing(8).align_y(iced::Bottom)
    ]
    .spacing(8)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_job_limit_is_none_or_a_whole_number_above_0() {
        assert_eq!(cap(" "), Ok(None));
        assert_eq!(cap("3"), Ok(Some(3)));
        assert!(cap("0").is_err());
        assert!(cap("two").is_err());
    }

    #[test]
    fn editing_a_workspace_fills_the_form_with_it() {
        let mut state = State {
            workspaces: vec![Workspace {
                name: "demo".into(),
                path: "/r".into(),
                branch: "main".into(),
                checks: "make test".into(),
                max_jobs: Some(2),
                branch_prefix: "ian/".into(),
            }],
            ..State::default()
        };
        open(&mut state, "demo");
        assert!(state.editing);
        assert_eq!(state.workspace, state.workspaces[0]);
        assert_eq!(state.max_jobs, "2");
    }
}
