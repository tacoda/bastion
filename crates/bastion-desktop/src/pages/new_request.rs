//! New request (PLAN.md D35): a request for a workspace on one
//! Bastion server, run by the workflow picked, or started from a GitHub
//! issue.

use bastion_protocol::api::{self, Request};
use bastion_protocol::{IssueView, LOCAL, ModelOption, WorkflowView, Workspace};
use iced::widget::{Column, button, column, container, pick_list, row, text, text_editor};
use iced::{Element, Fill, Task};

use super::{Choice, Ctx, labeled};
use crate::app::Message;
use crate::route::Route;
use crate::theme;

fn default_workflow() -> Choice {
    Choice::new("", "The server's default")
}

fn default_model() -> Choice {
    Choice::new("", "The server's plan and build models")
}

pub struct State {
    /// A request runs on `local` unless the owner picks another server (D30).
    server: String,
    workspaces: Vec<Workspace>,
    workspace: Option<String>,
    workflows: Vec<Choice>,
    workflow: Choice,
    /// The model every turn runs on, as `provider/model` (F8).
    models: Vec<Choice>,
    model: Choice,
    request: text_editor::Content,
    issues: Vec<IssueView>,
    issue_note: String,
    error: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            server: LOCAL.into(),
            workspaces: vec![],
            workspace: None,
            workflows: vec![default_workflow()],
            workflow: default_workflow(),
            models: vec![default_model()],
            model: default_model(),
            request: text_editor::Content::new(),
            issues: vec![],
            issue_note: String::new(),
            error: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Server(String),
    Workspaces(Result<Vec<Workspace>, String>),
    Workflows(Result<Vec<WorkflowView>, String>),
    Workspace(String),
    Workflow(Choice),
    Models(Result<Vec<ModelOption>, String>),
    Model(Choice),
    Edit(text_editor::Action),
    Submit,
    LoadIssues,
    Issues(Result<Vec<IssueView>, String>),
    FromIssue(u64),
    Started(Result<String, String>),
}

fn wrap(task: Task<Msg>) -> Task<Message> {
    task.map(Message::NewRequest)
}

/// Read the picked server's workspaces and workflows.
pub fn load(state: &State, ctx: &Ctx) -> Task<Message> {
    let on = state.server.clone();
    let spaces = ctx.clone().get(on.clone(), api::Workspaces);
    let flows = ctx.clone().get(on.clone(), api::Workflows);
    let models = ctx.clone().get(on, api::Models);
    wrap(Task::batch([
        Task::perform(spaces, Msg::Workspaces),
        Task::perform(flows, Msg::Workflows),
        Task::perform(models, Msg::Models),
    ]))
}

impl State {
    /// The workspace picked, else the server's first.
    fn chosen(&self) -> Option<String> {
        self.workspace
            .clone()
            .or_else(|| self.workspaces.first().map(|w| w.name.clone()))
    }

    fn ready(&self) -> bool {
        self.chosen().is_some() && !self.request.text().trim().is_empty()
    }

    /// Send `request` to the picked server.
    fn ask<R>(
        &self,
        ctx: &Ctx,
        request: R,
        then: fn(Result<R::Answer, String>) -> Msg,
    ) -> Task<Message>
    where
        R: Request + Send + Sync + 'static,
        R::Answer: Send + 'static,
    {
        wrap(ctx.task(&self.server, request, then))
    }

    fn submit(&self, ctx: &Ctx) -> Task<Message> {
        let submit = api::JobSubmit {
            workspace: self.chosen().unwrap_or_default(),
            request: self.request.text().trim().into(),
            pipeline: None,
            workflow: (!self.workflow.id.is_empty()).then(|| self.workflow.id.clone()),
            model: (!self.model.id.is_empty()).then(|| self.model.id.clone()),
        };
        self.ask(ctx, submit, Msg::Started)
    }

    /// List the workspace's open issues, or start a job from issue `number`.
    fn issues(&self, ctx: &Ctx, number: Option<u64>) -> Task<Message> {
        let workspace = self.chosen().unwrap_or_default();
        let Some(number) = number else {
            return self.ask(ctx, api::Issues { workspace }, Msg::Issues);
        };
        self.ask(ctx, api::JobFromIssue { workspace, number }, Msg::Started)
    }
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Server(name) => {
            *state = State {
                server: name,
                ..State::default()
            };
            return load(state, ctx);
        }
        Msg::Workspaces(Ok(all)) => state.workspaces = all,
        Msg::Workflows(Ok(all)) => {
            let named = all.into_iter().map(|w| Choice {
                id: w.id,
                name: w.name,
            });
            state.workflows = std::iter::once(default_workflow()).chain(named).collect();
        }
        Msg::Models(Ok(all)) => {
            let named = all.into_iter().map(|m| {
                let id = format!("{}/{}", m.provider, m.id);
                Choice::new(&id, &format!("{} ({})", m.name, m.provider))
            });
            state.models = std::iter::once(default_model()).chain(named).collect();
        }
        Msg::Model(choice) => state.model = choice,
        Msg::Workspaces(Err(e)) | Msg::Workflows(Err(e)) | Msg::Models(Err(e)) => state.error = e,
        Msg::Workspace(name) => state.workspace = Some(name),
        Msg::Workflow(choice) => state.workflow = choice,
        Msg::Edit(action) => state.request.perform(action),
        Msg::Submit => return state.submit(ctx),
        Msg::LoadIssues => return state.issues(ctx, None),
        Msg::Issues(Ok(all)) => {
            state.issue_note = if all.is_empty() {
                "No open issues.".into()
            } else {
                String::new()
            };
            state.issues = all;
        }
        Msg::Issues(Err(e)) => state.issue_note = e,
        Msg::FromIssue(number) => return state.issues(ctx, Some(number)),
        Msg::Started(Ok(id)) => {
            return Task::done(Message::OpenJob {
                server: state.server.clone(),
                id,
            });
        }
        Msg::Started(Err(e)) => state.error = e,
    }
    Task::none()
}

fn form<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let msg = Message::NewRequest;
    let names: Vec<String> = ctx.servers.iter().map(|s| s.name.clone()).collect();
    let server = pick_list(names, Some(state.server.clone()), move |n| {
        msg(Msg::Server(n))
    })
    .style(theme::select);
    let spaces: Vec<String> = state.workspaces.iter().map(|w| w.name.clone()).collect();
    let space =
        pick_list(spaces, state.chosen(), move |n| msg(Msg::Workspace(n))).style(theme::select);
    let flow = pick_list(
        state.workflows.clone(),
        Some(state.workflow.clone()),
        move |c| msg(Msg::Workflow(c)),
    )
    .style(theme::select);
    let model = pick_list(state.models.clone(), Some(state.model.clone()), move |c| {
        msg(Msg::Model(c))
    })
    .style(theme::select);
    let editor = text_editor(&state.request)
        .on_action(move |a| msg(Msg::Edit(a)))
        .height(128);
    let empty = if state.workspaces.is_empty() {
        "This Bastion server has no workspace yet. Add one in Settings."
    } else {
        ""
    };
    let submit = button("Submit").padding([8, 12]).style(theme::primary);
    let submit = if state.ready() {
        submit.on_press(msg(Msg::Submit))
    } else {
        submit
    };
    let body = column![
        labeled("Bastion server", server),
        labeled("Workspace", space),
        labeled("Workflow", flow),
        labeled("Model", model),
        labeled("What should change?", editor),
        text(empty).size(13).color(theme::MUTED),
        text(&state.error).color(theme::BAD),
        submit,
    ]
    .spacing(12);
    container(body)
        .padding(16)
        .max_width(672)
        .style(theme::panel)
        .into()
}

fn issues(state: &State) -> Element<'_, Message> {
    let load = button(text("Load issues").size(13))
        .padding([4, 12])
        .style(theme::secondary);
    let head = row![
        text("Or start from a GitHub issue")
            .size(13)
            .font(theme::BOLD)
            .color(theme::MUTED)
            .width(Fill),
        load.on_press(Message::NewRequest(Msg::LoadIssues)),
    ]
    .align_y(iced::Center);
    let rows = state.issues.iter().map(|i| {
        let label = row![
            text(format!("#{} ", i.number)).color(theme::MUTED),
            text(i.title.clone())
        ];
        let open = Message::NewRequest(Msg::FromIssue(i.number));
        button(label)
            .width(Fill)
            .padding([4, 8])
            .style(theme::row_button)
            .on_press(open)
            .into()
    });
    let body = column![
        head,
        Column::with_children(rows),
        text(&state.issue_note).size(13).color(theme::MUTED)
    ];
    container(body.spacing(8))
        .padding(16)
        .max_width(672)
        .style(theme::panel)
        .into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let back = button(text("← All jobs").size(13))
        .style(theme::link)
        .on_press(Message::Go(Route::Jobs));
    let title = text("New request").size(24).font(theme::BOLD);
    super::scrolled(column![back, title, form(state, ctx), issues(state)].spacing(12))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_needs_a_workspace_and_words_and_picks_the_first_workspace() {
        let mut state = State::default();
        assert!(!state.ready());
        state.workspaces = vec![Workspace {
            name: "calc".into(),
            path: "/c".into(),
            branch: "main".into(),
            checks: String::new(),
            max_jobs: None,
            revision_budget: None,
            branch_prefix: "bastion/".into(),
            setup: String::new(),
            cleanup: String::new(),
            env: String::new(),
            gates: String::new(),
            autofix: String::new(),
        }];
        assert_eq!(state.chosen().as_deref(), Some("calc"));
        state.request = text_editor::Content::with_text("  ");
        assert!(!state.ready());
        state.request = text_editor::Content::with_text("Add subtract.");
        assert!(state.ready());
    }
}
