//! The Workspaces page: the repositories jobs run against, each with its
//! settings and its charter (D58), and the accounts they use. On All
//! servers each server has its section, and a change goes to that server.

mod charter;

use std::collections::BTreeMap;

use bastion_protocol::api::{self, Request};
use bastion_protocol::{AccountView, Workspace, WorkspaceCharterView, branch_prefix};
use iced::widget::{Column, button, column, row, text, text_editor, text_input};
use iced::{Element, Task};

use crate::app::Message;
use crate::pages::settings::{card, small};
use crate::pages::{Ctx, labeled, scrolled};
use crate::theme;

pub struct State {
    workspace: Workspace,
    /// The form holds a workspace already added, to change it.
    editing: bool,
    /// The onboarding turn is drafting the form's settings.
    drafting: bool,
    /// The form's cap on running jobs, as typed; empty is none.
    max_jobs: String,
    /// The form's revision budget, as typed; empty is none.
    budget: String,
    /// The form's quality gates, one `name: command` per line.
    gates: text_editor::Content,
    workspaces: Vec<Workspace>,
    accounts: Vec<AccountView>,
    charters: Vec<WorkspaceCharterView>,
    charter: charter::State,
    note: String,
}

impl Default for State {
    fn default() -> Self {
        let workspace = Workspace {
            name: String::new(),
            path: String::new(),
            branch: "main".into(),
            checks: String::new(),
            max_jobs: None,
            revision_budget: None,
            branch_prefix: branch_prefix(),
            setup: String::new(),
            cleanup: String::new(),
            env: String::new(),
            gates: String::new(),
            autofix: String::new(),
            image: "python".into(),
            finish: "merge".into(),
        };
        State {
            workspace,
            editing: false,
            drafting: false,
            max_jobs: String::new(),
            budget: String::new(),
            gates: text_editor::Content::new(),
            workspaces: vec![],
            accounts: vec![],
            charters: vec![],
            charter: charter::State::default(),
            note: String::new(),
        }
    }
}

/// A field of the workspace being added.
#[derive(Clone, Copy, Debug)]
pub enum Field {
    Name,
    Path,
    Branch,
    Checks,
    MaxJobs,
    Budget,
    BranchPrefix,
    Setup,
    Cleanup,
    Env,
    Autofix,
    Image,
    Finish,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Workspaces(Result<Vec<Workspace>, String>),
    Accounts(Result<Vec<AccountView>, String>),
    Charters(Result<Vec<WorkspaceCharterView>, String>),
    Edit(Field, String),
    EditWorkspace(String),
    SaveWorkspace,
    DraftWorkspace,
    Drafted(Box<Result<Workspace, String>>),
    Gates(text_editor::Action),
    Charter(charter::Msg),
    Done(Result<(), String>),
}

fn wrap(msg: Msg) -> Message {
    // The page names the server as the message leaves its section.
    Message::Workspaces(String::new(), msg)
}

fn task<R>(ctx: &Ctx, request: R, then: fn(Result<R::Answer, String>) -> Msg) -> Task<Message>
where
    R: Request + Send + Sync + 'static,
    R::Answer: Send + 'static,
{
    ctx.task(&ctx.current, request, then).map(wrap)
}

fn load(ctx: &Ctx) -> Task<Message> {
    Task::batch([
        task(ctx, api::Workspaces, Msg::Workspaces),
        task(ctx, api::Accounts, Msg::Accounts),
        task(ctx, api::Charters, Msg::Charters),
    ])
}

fn edit(state: &mut State, field: Field, value: String) {
    let w = &mut state.workspace;
    match field {
        Field::Name => w.name = value,
        Field::Path => w.path = value,
        Field::Branch => w.branch = value,
        Field::Checks => w.checks = value,
        Field::MaxJobs => state.max_jobs = value,
        Field::Budget => state.budget = value,
        Field::BranchPrefix => state.workspace.branch_prefix = value,
        Field::Setup => state.workspace.setup = value,
        Field::Cleanup => state.workspace.cleanup = value,
        Field::Env => state.workspace.env = value,
        Field::Autofix => state.workspace.autofix = value,
        Field::Image => state.workspace.image = value,
        Field::Finish => state.workspace.finish = value,
    }
}

/// Put a workspace in the form, to change it.
fn open(state: &mut State, name: &str) {
    let Some(w) = state.workspaces.iter().find(|w| w.name == name) else {
        return;
    };
    let typed = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
    (state.max_jobs, state.budget) = (typed(w.max_jobs), typed(w.revision_budget));
    state.gates = text_editor::Content::with_text(&w.gates);
    (state.workspace, state.editing) = (w.clone(), true);
}

/// A typed count: empty is none, otherwise a whole number above 0.
fn count(typed: &str) -> Result<Option<u32>, String> {
    match typed.trim() {
        "" => Ok(None),
        n => n
            .parse()
            .ok()
            .filter(|n| *n > 0)
            .map(Some)
            .ok_or_else(|| format!("{n} is not a whole number above 0")),
    }
}

/// Add the form's workspace, or save the one it holds, then empty it.
fn save(state: &mut State, ctx: &Ctx) -> Task<Message> {
    let (max_jobs, revision_budget) = match (count(&state.max_jobs), count(&state.budget)) {
        (Ok(jobs), Ok(budget)) => (jobs, budget),
        (Err(e), _) | (_, Err(e)) => {
            state.note = e;
            return Task::none();
        }
    };
    let blank = State::default();
    let editing = std::mem::replace(&mut state.editing, false);
    (state.max_jobs, state.budget) = (blank.max_jobs, blank.budget);
    let gates = std::mem::replace(&mut state.gates, blank.gates).text();
    let w = Workspace {
        max_jobs,
        revision_budget,
        gates,
        ..std::mem::replace(&mut state.workspace, blank.workspace)
    };
    if !editing {
        return task(ctx, api::WorkspaceAdd(w), Msg::Done);
    }
    let edit = api::WorkspaceEdit {
        name: w.name,
        branch: w.branch,
        checks: w.checks,
        max_jobs,
        revision_budget,
        branch_prefix: w.branch_prefix,
        setup: w.setup,
        cleanup: w.cleanup,
        env: w.env,
        gates: w.gates,
        autofix: w.autofix,
        image: w.image,
        finish: w.finish,
    };
    task(ctx, edit, Msg::Done)
}

/// Put the onboarding turn's draft in the form; nothing is saved until a
/// person adds the workspace.
fn drafted_into(state: &mut State, drafted: Result<Workspace, String>) {
    state.drafting = false;
    let d = match drafted {
        Ok(d) => d,
        Err(e) => return state.note = e,
    };
    let w = &mut state.workspace;
    (w.checks, w.setup, w.cleanup, w.env) = (d.checks, d.setup, d.cleanup, d.env);
    state.note.clear();
}

fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Workspaces(Ok(all)) => state.workspaces = all,
        Msg::Accounts(Ok(all)) => state.accounts = all,
        Msg::Charters(Ok(all)) => state.charters = all,
        Msg::Workspaces(Err(e))
        | Msg::Accounts(Err(e))
        | Msg::Charters(Err(e))
        | Msg::Done(Err(e)) => state.note = e,
        Msg::Edit(f, v) => edit(state, f, v),
        Msg::EditWorkspace(name) => open(state, &name),
        Msg::SaveWorkspace => return save(state, ctx),
        Msg::DraftWorkspace => {
            state.drafting = true;
            let w = &state.workspace;
            let draft = api::WorkspaceDraft {
                path: w.path.clone(),
                branch: w.branch.clone(),
            };
            return task(ctx, draft, |r| Msg::Drafted(Box::new(r)));
        }
        Msg::Drafted(drafted) => drafted_into(state, *drafted),
        Msg::Gates(action) => state.gates.perform(action),
        Msg::Charter(charter::Msg::Saved(Ok(branch))) => {
            let saved = charter::Msg::Saved(Ok(branch));
            return Task::batch([charter::update(&mut state.charter, saved, ctx), load(ctx)]);
        }
        Msg::Charter(m) => return charter::update(&mut state.charter, m, ctx),
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
    labeled(label, input)
}

/// A field that cannot change: a saved workspace keeps its name and path.
fn fixed<'a>(label: &'a str, value: &str) -> Element<'a, Message> {
    labeled(label, text_input("", value).style(theme::field).width(200))
}

/// One workspace: its name, where it is, how it checks a change, and its
/// charter.
fn workspace<'a>(
    w: &Workspace,
    own: Option<&'a WorkspaceCharterView>,
    editing: &'a charter::State,
) -> Element<'a, Message> {
    let edit = button(text("Edit").size(13))
        .style(theme::link)
        .on_press(wrap(Msg::EditWorkspace(w.name.clone())));
    let mut about = vec![format!("{} @ {}", w.path, w.branch)];
    about.extend((!w.checks.is_empty()).then(|| format!("checks: {}", w.checks)));
    about.extend(w.max_jobs.map(|n| format!("at most {n} at once")));
    column![
        row![text(w.name.clone()).font(theme::BOLD), edit]
            .spacing(10)
            .align_y(iced::Center),
        text(about.join(" · ")).size(13).color(theme::muted()),
        charter::view(own, editing),
    ]
    .spacing(6)
    .into()
}

fn workspaces(state: &State) -> Element<'_, Message> {
    let lede = "The repositories jobs run against on this Bastion server. Checks run the tests at the checks step. A job pushes to its branch prefix and its id, the job limit caps how many run at once, and the revision budget caps how often its steps send work back before you decide. Setup runs with the network in a VM of its own, before the agent starts; the agent's VM stays offline. The environment is visible to the agent, so keep secrets out of it. Before a push, each gate runs over the change's files, where {files} names them; a failing gate gets the autofix once, then stops the push.";
    let rows = state.workspaces.iter().map(|w| {
        let own = state.charters.iter().find(|c| c.workspace == w.name);
        workspace(w, own, &state.charter)
    });
    card(
        "Workspaces",
        column![
            text(lede).size(13).color(theme::muted()),
            Column::with_children(rows).spacing(18),
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
    let draft = match state.drafting {
        true => small("Drafting…", None),
        false => small("Draft settings", Some(wrap(Msg::DraftWorkspace))),
    };
    let place = row![name, path, field("Branch", "", &w.branch, Field::Branch)]
        .extend((!state.editing).then_some(draft));
    let settings = row![
        field("Checks", "pytest -q", &w.checks, Field::Checks),
        field("Branch prefix", "", &w.branch_prefix, Field::BranchPrefix),
        field("Job limit", "none", &state.max_jobs, Field::MaxJobs),
        field("Revision budget", "none", &state.budget, Field::Budget),
    ];
    let vm = row![
        field("Setup", "pip install -e .", &w.setup, Field::Setup),
        field("Cleanup", "", &w.cleanup, Field::Cleanup),
        field("Environment", "CI=1 LANG=C", &w.env, Field::Env),
        small(label, Some(wrap(Msg::SaveWorkspace))),
    ];
    column![
        place.spacing(8).align_y(iced::Bottom),
        settings.spacing(8).align_y(iced::Bottom),
        vm.spacing(8).align_y(iced::Bottom),
        gates_row(state)
    ]
    .spacing(8)
}

/// The quality gates, run over a change's files before a push, and the
/// autofix run once when one fails.
fn gates_row(state: &State) -> Element<'_, Message> {
    let gates = text_editor(&state.gates)
        .placeholder("lint: ruff check {files}")
        .on_action(|a| wrap(Msg::Gates(a)))
        .height(80)
        .width(416);
    let autofix = field(
        "Autofix",
        "ruff check --fix {files}",
        &state.workspace.autofix,
        Field::Autofix,
    );
    let image = field(
        "VM image",
        "python, node or browser",
        &state.workspace.image,
        Field::Image,
    );
    let finish = field(
        "When the last step passes",
        "merge or leave the branch",
        &state.workspace.finish,
        Field::Finish,
    );
    row![
        labeled("Gates, one name: command per line", gates),
        autofix,
        image,
        finish
    ]
    .spacing(8)
    .align_y(iced::Top)
    .into()
}

fn accounts(state: &State) -> Element<'_, Message> {
    let rows = state.accounts.iter().map(|a| {
        let mark = text(if a.ok { "✓ " } else { "! " }).color(if a.ok {
            theme::ok()
        } else {
            theme::warn()
        });
        row![
            mark,
            text(a.name.clone()).font(theme::BOLD),
            text(format!(" · {}", a.status)).color(theme::muted())
        ]
        .into()
    });
    card("Accounts", Column::with_children(rows).spacing(4))
}

/// Each server shown, by its name.
#[derive(Default)]
pub struct Servers {
    each: BTreeMap<String, State>,
}

/// A message from `server`'s section, named for it.
fn tag(server: String) -> impl Fn(Message) -> Message + Clone {
    move |m| match m {
        Message::Workspaces(_, m) => Message::Workspaces(server.clone(), m),
        other => other,
    }
}

pub fn load_all(ctx: &Ctx) -> Task<Message> {
    let each = ctx
        .shown()
        .into_iter()
        .map(|name| load(&ctx.on(&name)).map(tag(name)));
    Task::batch(each)
}

pub fn update_all(all: &mut Servers, server: String, msg: Msg, ctx: &Ctx) -> Task<Message> {
    let state = all.each.entry(server.clone()).or_default();
    update(state, msg, &ctx.on(&server)).map(tag(server))
}

/// One server's section: its workspaces, its accounts and why a change failed.
fn section(state: &State) -> Column<'_, Message> {
    column![
        workspaces(state),
        accounts(state),
        text(&state.note).size(13).color(theme::bad())
    ]
    .spacing(16)
}

pub fn view<'a>(all: &'a Servers, ctx: &Ctx) -> Element<'a, Message> {
    let shown = ctx.shown();
    let named = shown.len() > 1;
    let about = match named {
        true => "The repositories jobs on each server run against.".to_string(),
        false => format!("The repositories jobs on {} run against.", ctx.current),
    };
    let sections = shown.into_iter().filter_map(|name| {
        let state = all.each.get(&name)?;
        let head = named.then(|| text(name.clone()).size(18).font(theme::BOLD));
        let body = Element::from(section(state)).map(tag(name));
        Some(
            Column::new()
                .spacing(10)
                .extend(head.map(Element::from))
                .push(body)
                .into(),
        )
    });
    let body = column![crate::pages::head("Workspaces", about, vec![])]
        .extend(sections)
        .spacing(16);
    scrolled(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_count_is_none_or_a_whole_number_above_0() {
        assert_eq!(count(" "), Ok(None));
        assert_eq!(count("3"), Ok(Some(3)));
        assert!(count("0").is_err());
        assert!(count("two").is_err());
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
                revision_budget: Some(4),
                branch_prefix: "ian/".into(),
                setup: "pip install -e .".into(),
                cleanup: String::new(),
                env: "CI=1".into(),
                gates: "lint: ruff check {files}".into(),
                autofix: "ruff check --fix {files}".into(),
                image: "python".into(),
                finish: "merge".into(),
            }],
            ..State::default()
        };
        open(&mut state, "demo");
        assert!(state.editing);
        assert_eq!(state.workspace, state.workspaces[0]);
        assert_eq!((state.max_jobs.as_str(), state.budget.as_str()), ("2", "4"));
        assert_eq!(state.gates.text().trim(), "lint: ruff check {files}");
    }

    #[test]
    fn a_draft_fills_the_settings_and_keeps_what_was_typed() {
        let mut state = State {
            drafting: true,
            ..State::default()
        };
        state.workspace.name = "demo".into();
        let draft = Workspace {
            name: String::new(),
            checks: "cargo test".into(),
            setup: "cargo fetch".into(),
            ..state.workspace.clone()
        };
        drafted_into(&mut state, Ok(draft));
        assert!(!state.drafting);
        assert_eq!(state.workspace.name, "demo");
        assert_eq!(state.workspace.checks, "cargo test");
        assert_eq!(state.workspace.setup, "cargo fetch");
    }
}
