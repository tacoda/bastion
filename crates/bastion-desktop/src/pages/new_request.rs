//! New request (DECISIONS.md D35): a request for a workspace on one
//! Bastion server, run by the workflow picked, or started from a GitHub
//! issue.

use bastion_protocol::api::{self, Request, Screenshot};
use bastion_protocol::{IssueView, LOCAL, ModelOption, WorkflowView, Workspace};
use iced::widget::{
    Column, button, column, container, pick_list, row, text, text_editor, text_input,
};
use iced::{Element, Fill, Task};

use super::{Choice, Ctx, labeled};
use crate::app::Message;
use crate::route::Route;
use crate::theme;

fn default_workflow() -> Choice {
    Choice::new("", "The server's default")
}

fn default_model() -> Choice {
    Choice::new("", "The server's models")
}

pub struct State {
    /// A request runs on `local` unless the owner picks another server (D30).
    server: String,
    workspaces: Vec<Workspace>,
    workspace: Option<String>,
    workflows: Vec<Choice>,
    workflow: Choice,
    /// The model every turn runs on, as `provider/model`.
    models: Vec<Choice>,
    model: Choice,
    request: text_editor::Content,
    criteria: text_editor::Content,
    /// One web address per line.
    links: text_editor::Content,
    /// The path typed for the next screenshot, and those added.
    shot_path: String,
    shots: Vec<Screenshot>,
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
            criteria: text_editor::Content::new(),
            links: text_editor::Content::new(),
            shot_path: String::new(),
            shots: vec![],
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
    EditCriteria(text_editor::Action),
    EditLinks(text_editor::Action),
    ShotPath(String),
    AddShot,
    DropShot(usize),
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
            workflow: (!self.workflow.id.is_empty()).then(|| self.workflow.id.clone()),
            model: (!self.model.id.is_empty()).then(|| self.model.id.clone()),
            criteria: Some(self.criteria.text().trim().to_string()).filter(|c| !c.is_empty()),
            links: lines(&self.links.text()),
            screenshots: self.shots.clone(),
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
        Msg::EditCriteria(action) => state.criteria.perform(action),
        Msg::EditLinks(action) => state.links.perform(action),
        Msg::ShotPath(path) => state.shot_path = path,
        Msg::AddShot => match screenshot(&state.shot_path) {
            Ok(shot) => {
                state.shots.push(shot);
                (state.shot_path, state.error) = Default::default();
            }
            Err(e) => state.error = e,
        },
        Msg::DropShot(i) => drop(state.shots.remove(i)),
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

/// The non-blank lines of `text`, each trimmed.
fn lines(text: &str) -> Vec<String> {
    let kept = text.lines().map(str::trim).filter(|l| !l.is_empty());
    kept.map(String::from).collect()
}

/// The image at `path` on this machine, sent by its file name: the server
/// may be another machine.
fn screenshot(path: &str) -> Result<Screenshot, String> {
    let path = std::path::Path::new(path.trim());
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Screenshot {
        name: name.into(),
        bytes,
    })
}

/// The widest the form grows, so a line of the request stays readable.
const FORM_WIDTH: f32 = 1200.0;

/// The criteria, links and screenshots a request may carry.
fn details(state: &State) -> Element<'_, Message> {
    let msg = Message::NewRequest;
    let criteria = text_editor(&state.criteria)
        .on_action(move |a| msg(Msg::EditCriteria(a)))
        .height(96);
    let links = text_editor(&state.links)
        .on_action(move |a| msg(Msg::EditLinks(a)))
        .height(64);
    let path = text_input("/path/to/screenshot.png", &state.shot_path)
        .on_input(move |p| msg(Msg::ShotPath(p)))
        .on_submit(msg(Msg::AddShot));
    let add = button(text("Add").size(13))
        .padding([4, 12])
        .style(theme::secondary)
        .on_press(msg(Msg::AddShot));
    let added = state.shots.iter().enumerate().map(|(i, s)| {
        let drop = button(text("Remove").size(12)).style(theme::link);
        row![
            text(s.name.clone()).size(13).width(Fill),
            drop.on_press(msg(Msg::DropShot(i)))
        ]
        .into()
    });
    column![
        labeled("Acceptance criteria (optional)", criteria),
        labeled("Links, one per line (optional)", links),
        labeled(
            "Screenshots (optional)",
            row![path, add].spacing(8).align_y(iced::Center)
        ),
        Column::with_children(added).spacing(2),
    ]
    .spacing(12)
    .into()
}

fn form<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let msg = Message::NewRequest;
    let names: Vec<String> = ctx.servers.iter().map(|s| s.name.clone()).collect();
    let server = pick_list(names, Some(state.server.clone()), move |n| {
        msg(Msg::Server(n))
    })
    .width(Fill)
    .style(theme::select);
    let spaces: Vec<String> = state.workspaces.iter().map(|w| w.name.clone()).collect();
    let space = pick_list(spaces, state.chosen(), move |n| msg(Msg::Workspace(n)))
        .width(Fill)
        .style(theme::select);
    let flow = pick_list(
        state.workflows.clone(),
        Some(state.workflow.clone()),
        move |c| msg(Msg::Workflow(c)),
    )
    .width(Fill)
    .style(theme::select);
    let model = pick_list(state.models.clone(), Some(state.model.clone()), move |c| {
        msg(Msg::Model(c))
    })
    .width(Fill)
    .style(theme::select);
    let editor = text_editor(&state.request)
        .on_action(move |a| msg(Msg::Edit(a)))
        .height(276);
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
    // Where it runs, in one row; then what to do beside what it must meet.
    let place = row![
        container(labeled("Bastion server", server)).width(Fill),
        container(labeled("Workspace", space)).width(Fill),
        container(labeled("Workflow", flow)).width(Fill),
        container(labeled("Model", model)).width(Fill),
    ]
    .spacing(12);
    let work = row![
        container(labeled("What should change?", editor)).width(iced::FillPortion(3)),
        container(details(state)).width(iced::FillPortion(2)),
    ]
    .spacing(16);
    let mut body = column![place, work].spacing(16);
    if !empty.is_empty() {
        body = body.push(text(empty).size(13).color(theme::muted()));
    }
    if !state.error.is_empty() {
        body = body.push(text(&state.error).color(theme::bad()));
    }
    let body = body.push(submit);
    container(body)
        .padding(20)
        .max_width(FORM_WIDTH)
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
            .color(theme::muted())
            .width(Fill),
        load.on_press(Message::NewRequest(Msg::LoadIssues)),
    ]
    .align_y(iced::Center);
    let rows = state.issues.iter().map(|i| {
        let label = row![
            text(format!("#{} ", i.number)).color(theme::muted()),
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
        text(&state.issue_note).size(13).color(theme::muted())
    ];
    container(body.spacing(8))
        .padding(16)
        .max_width(FORM_WIDTH)
        .style(theme::panel)
        .into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let back = button(text("← All jobs").size(13))
        .style(theme::link)
        .on_press(Message::Go(Route::Jobs));
    let head = super::head(
        "New request",
        "A change for one workspace, in your words.",
        vec![],
    );
    super::scrolled(column![back, head, form(state, ctx), issues(state)].spacing(12))
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
            image: "python".into(),
            finish: "merge".into(),
        }];
        assert_eq!(state.chosen().as_deref(), Some("calc"));
        state.request = text_editor::Content::with_text("  ");
        assert!(!state.ready());
        state.request = text_editor::Content::with_text("Add subtract.");
        assert!(state.ready());
    }

    #[test]
    fn a_screenshot_is_read_from_its_path_and_sent_by_its_name() {
        let dir = std::env::temp_dir().join(format!("bastion-shot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.png");
        std::fs::write(&path, [1, 2, 3]).unwrap();
        let shot = screenshot(&format!(" {} ", path.display())).unwrap();
        assert_eq!(
            (shot.name.as_str(), shot.bytes.as_slice()),
            ("broken.png", &[1, 2, 3][..])
        );
        assert!(screenshot("/no/such/file.png").is_err());
        assert_eq!(
            lines("https://a\n\n https://b "),
            ["https://a", "https://b"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
