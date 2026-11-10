//! The job page: one job as a run. The request and its badges,
//! the owner's actions when it waits, what governs it, its workflow, then
//! its steps, each opening to its artifacts and turn.

use std::collections::{BTreeMap, BTreeSet};

use bastion_protocol::api::{self, Request};
use bastion_protocol::{
    ArtifactView, CharterView, HeldCallRef, HeldCallView, JobDetail, JobSummary, StepView,
    TranscriptLine,
};
use iced::widget::{
    Column, button, column, container, row, scrollable, text, text_editor, text_input,
};
use iced::{Element, Fill, Task};

use super::drawing::{self, Drawing};
use super::{Ctx, ago, diff, mark, section};
use crate::app::Message;
use crate::route::Route;
use crate::theme;

pub struct State {
    id: String,
    detail: Option<JobDetail>,
    note: text_editor::Content,
    error: String,
    /// What the owner typed to refuse each held call, by call id.
    feedback: BTreeMap<String, String>,
    /// The steps the owner opened or closed, against their default.
    toggled: BTreeSet<String>,
    turn_open: bool,
    /// Each step's state by its node id, for the drawing.
    marks: BTreeMap<String, String>,
}

impl State {
    pub fn new(id: String) -> Self {
        State {
            id,
            detail: None,
            note: text_editor::Content::new(),
            error: String::new(),
            feedback: BTreeMap::new(),
            toggled: BTreeSet::new(),
            turn_open: false,
            marks: BTreeMap::new(),
        }
    }

    fn allows(&self, action: &str) -> bool {
        self.detail
            .as_ref()
            .is_some_and(|d| d.job.actions.iter().any(|a| a == action))
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Result<Box<JobDetail>, String>),
    Edit(text_editor::Action),
    CommentAt(String),
    Act(Action),
    Acted(Result<(), String>),
    Feedback(String, String),
    Allow(String),
    Refuse(String),
    ToggleStep(String),
    ToggleTurn,
}

/// What the owner does to a job from its page.
#[derive(Clone, Copy, Debug)]
pub enum Action {
    Approve,
    Comment,
    Retry,
    Extend,
    Stop,
}

/// Send a write to the server shown, its answer to `Msg::Acted`.
fn ask<R>(ctx: &Ctx, request: R) -> Task<Message>
where
    R: Request<Answer = ()> + Send + Sync + 'static,
{
    ctx.task(&ctx.current, request, Msg::Acted)
        .map(Message::Job)
}

/// Read the job again from the server the page shows.
impl State {
    /// Whether the job is in progress.
    pub fn moving(&self) -> bool {
        let busy = |d: &JobDetail| super::IN_PROGRESS.contains(&d.job.state.as_str());
        self.detail.as_ref().is_some_and(busy)
    }
}

pub fn load(state: &State, ctx: &Ctx) -> Task<Message> {
    let read = api::Job {
        id: state.id.clone(),
    };
    ctx.task(&ctx.current, read, |r| Msg::Read(r.map(Box::new)))
        .map(Message::Job)
}

/// Put `at` on its own line at the end of the note.
fn append(note: &mut text_editor::Content, at: &str) {
    let mut t = note.text();
    if !t.is_empty() && !t.ends_with('\n') {
        t.push('\n');
    }
    t.push_str(at);
    *note = text_editor::Content::with_text(&t);
    note.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
}

/// Approve, send back or answer with the note, or stop.
fn act(state: &State, ctx: &Ctx, action: Action) -> Task<Message> {
    let id = state.id.clone();
    match action {
        Action::Approve => ask(ctx, api::JobApprove { id }),
        Action::Retry => ask(ctx, api::JobRetry { id }),
        Action::Extend => ask(ctx, api::JobExtend { id }),
        Action::Stop => ask(ctx, api::JobStop { id }),
        Action::Comment => {
            let text = state.note.text().trim().to_string();
            ask(ctx, api::JobComment { id, text })
        }
    }
}

fn held(state: &State, ctx: &Ctx, call: String, allow: bool) -> Task<Message> {
    let feedback = state.feedback.get(&call).cloned().unwrap_or_default();
    let held = HeldCallRef {
        job: state.id.clone(),
        call,
    };
    if allow {
        return ask(ctx, api::HeldAllow(held));
    }
    ask(ctx, api::HeldRefuse { held, feedback })
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Read(Ok(d)) => {
            state.marks = d
                .steps
                .iter()
                .map(|s| (s.id.clone(), s.state.clone()))
                .collect();
            state.detail = Some(*d);
        }
        Msg::Read(Err(e)) | Msg::Acted(Err(e)) => state.error = e,
        Msg::Edit(action) => state.note.perform(action),
        Msg::CommentAt(at) => append(&mut state.note, &at),
        Msg::Act(action) => return act(state, ctx, action),
        Msg::Acted(Ok(_)) => {
            state.note = text_editor::Content::new();
            state.error.clear();
        }
        Msg::Feedback(call, text) => {
            state.feedback.insert(call, text);
        }
        Msg::Allow(call) => return held(state, ctx, call, true),
        Msg::Refuse(call) => return held(state, ctx, call, false),
        Msg::ToggleStep(id) => {
            if !state.toggled.remove(&id) {
                state.toggled.insert(id);
            }
        }
        Msg::ToggleTurn => state.turn_open = !state.turn_open,
    }
    Task::none()
}

/// What the wait judges: the step's own artifact, with the diff after the
/// build and the code review, and before a step that lands the change.
fn judged(d: &JobDetail) -> (Option<String>, Option<String>) {
    let latest = |stage: &str| {
        d.artifacts
            .iter()
            .rev()
            .find(|a| a.stage == stage)
            .map(|a| a.body.clone())
    };
    match d.job.stage.as_str() {
        "build" | "code_review" => (latest(&d.job.stage), latest("review")),
        "land" => (latest("build"), latest("review")),
        "ship" | "merge" => (latest("comprehend"), latest("review")),
        stage => (latest(stage), None),
    }
}

/// What Approve does at a step that lands, ships, merges or deploys (D33).
fn what_a_release_approve_does(stage: &str, failed: bool, has_pr: bool) -> Option<&'static str> {
    Some(match stage {
        "land" if has_pr && !failed => {
            "CI passed on the pull request. Approve merges it, or stop the job."
        }
        "land" | "ship" | "merge" | "deploy" if failed => {
            "Approve tries the step again, or stop the job."
        }
        "land" => "Approve lands the change on the workspace's branch.",
        "ship" => "Approve pushes the change and opens its pull request.",
        "merge" => "Approve merges the pull request.",
        "deploy" => "Approve watches the deploy of the merged change.",
        _ => return None,
    })
}

/// What Approve does at the step the job waits on (D33).
fn what_approve_does(d: &JobDetail) -> &'static str {
    let failed = d.job.error.is_some();
    if let Some(release) = what_a_release_approve_does(&d.job.stage, failed, d.job.pr.is_some()) {
        return release;
    }
    match d.job.stage.as_str() {
        _ if failed => "Approve goes on without this step, or send it back with a note.",
        "discover" => {
            "Answer discovery's questions in a note, or approve to plan on its assumptions."
        }
        "define" => "Approve these criteria to write their tests, or send them back with a note.",
        "refactor" => "Name the refactorings to make in a note, or approve to make none.",
        "code_review" => "Name the findings to fix in a note, or approve to fix none.",
        "prove" => "The proof is partial. Approve to go on, or send it back with a note.",
        "comprehend" => {
            "Try the change as the briefing says, then approve, or say what is wrong in a note."
        }
        _ => "Approve this step's result to move on, or send it back with a note.",
    }
}

/// A step's name: a skill's, prompt's or command's node id, else its stage.
fn step_name(step: &StepView) -> String {
    match step.stage.as_str() {
        "skill" | "prompt" | "command" => format!("{} · {}", step.stage, step.id),
        _ => step.stage.replace('_', " "),
    }
}

/// A report, an artifact or an error: monospace on the page color, scrolled
/// past 384 px.
fn block<'a>(body: String) -> Element<'a, Message> {
    let body = container(text(body).font(theme::MONO).size(13))
        .padding(12)
        .width(Fill);
    container(scrollable(body).height(iced::Length::Shrink))
        .max_height(384)
        .style(theme::code)
        .into()
}

fn chip<'a>(label: String, color: iced::Color) -> Element<'a, Message> {
    container(text(label).size(12).color(color))
        .padding([2, 8])
        .style(theme::chip)
        .into()
}

fn link_chip<'a>(label: String, url: String) -> Element<'a, Message> {
    button(text(label).size(12).color(theme::LINK))
        .padding([2, 8])
        .style(theme::secondary)
        .on_press(Message::OpenUrl(url))
        .into()
}

fn last_segment(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or_default()
}

/// The request and its badges: state, Bastion server, workspace, id and age.
/// How long a running turn has gone, once it is long. The turn counts
/// from `updated_at`, its start or its last extend.
fn still_working(job: &JobSummary, now: u64) -> Option<String> {
    let minutes = now.saturating_sub(job.updated_at) / 60;
    let turn = job.actions.iter().any(|a| a == "extend");
    (turn && minutes >= 10).then(|| format!("still working, {minutes} min into this turn"))
}

fn header<'a>(d: &JobDetail, server: &str) -> Element<'a, Message> {
    let (symbol, color) = mark(&d.job.state);
    let mut badges = vec![
        chip(format!("{symbol} {}", d.job.state.replace('_', " ")), color),
        chip(server.into(), theme::MUTED),
        chip(d.job.workspace.clone(), theme::MUTED),
        chip(d.job.id.clone(), theme::MUTED),
    ];
    badges.extend(
        d.job
            .parent
            .iter()
            .map(|p| chip(format!("part of {p}"), theme::MUTED)),
    );
    badges.push(chip(ago(d.job.created_at), theme::MUTED));
    badges.extend(
        d.job
            .issue
            .iter()
            .map(|u| link_chip(format!("issue {}", last_segment(u)), u.clone())),
    );
    badges.extend(
        d.job
            .pr
            .iter()
            .map(|u| link_chip(format!("pull request {}", last_segment(u)), u.clone())),
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    badges.extend(still_working(&d.job, now).map(|t| chip(t, theme::WARN)));
    let mut head = column![text(d.job.request.clone()).size(24).font(theme::BOLD)].spacing(8);
    if let Some(p) = &d.job.part {
        head = head.push(text(format!("Part: {p}")));
    }
    head.push(row(badges).spacing(8).wrap()).into()
}

/// Why the job failed, or why its land did.
fn failure<'a>(state: &State, d: &JobDetail) -> Option<Element<'a, Message>> {
    let e = d.job.error.clone()?;
    let title = match (d.job.state.as_str(), d.job.stage.as_str()) {
        ("failed", _) => "This job failed",
        (_, "land" | "merge") => "The land failed; the VM and its commits are kept",
        _ => "This step is stuck",
    };
    let mut body = column![
        text(title).size(13).font(theme::BOLD).color(theme::BAD),
        block(e)
    ]
    .spacing(8);
    if state.allows("retry") {
        let retry = button(text("Retry").size(13))
            .padding([8, 12])
            .style(theme::secondary)
            .on_press(Message::Job(Msg::Act(Action::Retry)));
        body = body.push(retry);
    }
    Some(
        container(body)
            .padding(16)
            .width(Fill)
            .style(theme::alarmed)
            .into(),
    )
}

/// Approving a stuck step goes on without it.
fn approve_label(state: &State) -> &'static str {
    if state.allows("retry") {
        "Ship anyway"
    } else {
        "Approve"
    }
}

fn actions(state: &State) -> Element<'_, Message> {
    let answer = state.allows("answer");
    let mut buttons = row![].spacing(8);
    if state.allows("approve") {
        buttons = buttons.push(
            button(approve_label(state))
                .padding([8, 12])
                .style(theme::primary)
                .on_press(Message::Job(Msg::Act(Action::Approve))),
        );
    }
    if state.allows("comment") || answer {
        let label = if answer { "Answer" } else { "Send back" };
        let ready = !state.note.text().trim().is_empty();
        let send = button(label).padding([8, 12]).style(theme::secondary);
        buttons = buttons
            .push(send.on_press_maybe(ready.then_some(Message::Job(Msg::Act(Action::Comment)))));
    }
    buttons.into()
}

fn note(state: &State) -> Option<Element<'_, Message>> {
    let answer = state.allows("answer");
    if !(answer || state.allows("comment")) {
        return None;
    }
    let label = if answer {
        "Your answer"
    } else {
        "A note for the agent (sends the work back)"
    };
    let editor = text_editor(&state.note)
        .on_action(|a| Message::Job(Msg::Edit(a)))
        .height(96);
    Some(
        column![text(label).size(13).color(theme::MUTED), editor]
            .spacing(4)
            .into(),
    )
}

/// The wait: the agent's question, what Approve does, what it judges, the
/// note, and the buttons.
fn waiting<'a>(state: &'a State, d: &'a JobDetail) -> Element<'a, Message> {
    let mut body = Column::new().spacing(12).push(
        text("Waiting for you")
            .size(13)
            .font(theme::BOLD)
            .color(theme::WARN),
    );
    if let Some(q) = &d.job.question {
        body = body.push(row![text("The agent asks: ").font(theme::BOLD), text(q.clone())].wrap());
    }
    if state.allows("approve") {
        body = body.push(text(what_approve_does(d)).size(13).color(theme::MUTED));
        let (report, change) = judged(d);
        body = body.extend(report.map(block));
        if let Some(change) = change {
            body = body.push(
                text("The change (click a line to comment on it)")
                    .size(13)
                    .font(theme::BOLD)
                    .color(theme::MUTED),
            );
            body = body.push(diff::view(&change, |at| Message::Job(Msg::CommentAt(at))));
        }
    }
    body = body.extend(note(state));
    container(body.push(actions(state)))
        .padding(16)
        .width(Fill)
        .style(theme::accented)
        .into()
}

/// One call the gate holds: allow exactly it once, or refuse it
/// with a note the agent reads as the call's result.
fn held_call<'a>(state: &'a State, call: &'a HeldCallView) -> Element<'a, Message> {
    let typed = state.feedback.get(&call.id).cloned().unwrap_or_default();
    let id = call.id.clone();
    let field = text_input("What the agent should do instead", &typed)
        .on_input(move |t| Message::Job(Msg::Feedback(id.clone(), t)))
        .style(theme::field);
    let allow = button("Allow once")
        .padding([8, 12])
        .style(theme::primary)
        .on_press(Message::Job(Msg::Allow(call.id.clone())));
    let refuse = button("Refuse with this note")
        .padding([8, 12])
        .style(theme::secondary);
    let refuse = refuse.on_press_maybe(
        (!typed.trim().is_empty()).then(|| Message::Job(Msg::Refuse(call.id.clone()))),
    );
    let what = row![
        text(call.function.clone()).font(theme::BOLD),
        text(call.text.clone()).font(theme::MONO),
        text(format!(" · held {}", ago(call.at)))
            .size(12)
            .color(theme::MUTED)
    ];
    column![
        what.spacing(6).wrap(),
        field,
        row![allow, refuse].spacing(8)
    ]
    .spacing(8)
    .into()
}

fn holds<'a>(state: &'a State, d: &'a JobDetail) -> Option<Element<'a, Message>> {
    if d.held.is_empty() {
        return None;
    }
    let title = text("The gate holds a call for you")
        .size(13)
        .font(theme::BOLD)
        .color(theme::WARN);
    let calls = d.held.iter().map(|c| held_call(state, c));
    let body = column![title, Column::with_children(calls).spacing(16)].spacing(12);
    Some(
        container(body)
            .padding(16)
            .width(Fill)
            .style(theme::accented)
            .into(),
    )
}

/// What the repository asks of this job, or that it asks nothing.
fn charter<'a>(c: &CharterView) -> Element<'a, Message> {
    let files = if c.files.is_empty() {
        "No charter: this job runs on the owner's defaults.".to_string()
    } else {
        format!("Charter: {}", c.files.join(", "))
    };
    let mut body = column![text(files).size(13)].spacing(6);
    for (title, items) in [
        ("The repository's rules", &c.rules),
        ("The repository's hooks, as they ran", &c.hook_runs),
        ("What review found", &c.findings),
    ] {
        if items.is_empty() {
            continue;
        }
        body = body.push(text(title).size(12).font(theme::BOLD).color(theme::MUTED));
        body = body.extend(items.iter().map(|i| text(format!("• {i}")).size(12).into()));
    }
    container(body)
        .padding(16)
        .width(Fill)
        .style(theme::panel)
        .into()
}

/// The job's workflow, each step marked with its state (D35), and each
/// result its steps gave, as a document.
fn flow<'a>(d: &'a JobDetail, marks: &'a BTreeMap<String, String>) -> Element<'a, Message> {
    let title = text("Workflow")
        .size(13)
        .font(theme::BOLD)
        .color(theme::MUTED);
    let mut body = column![title, drawing::view(Drawing::read_only(&d.workflow, marks))].spacing(8);
    for (name, result) in &d.results {
        let head = text(format!("Result · {name}"))
            .size(12)
            .font(theme::BOLD)
            .color(theme::MUTED);
        body = body.push(
            container(column![head, block(result.clone())].spacing(6))
                .padding(12)
                .width(Fill)
                .style(theme::panel),
        );
    }
    body.into()
}

fn artifact<'a>(a: &ArtifactView) -> Element<'a, Message> {
    let head = text(format!("{} artifact · {}", a.stage, ago(a.created_at)))
        .size(12)
        .font(theme::BOLD)
        .color(theme::MUTED);
    column![head, block(a.body.clone())].spacing(4).into()
}

fn turn<'a>(lines: &'a [TranscriptLine], open: bool) -> Element<'a, Message> {
    let arrow = if open { "▾" } else { "▸" };
    let toggle = button(text(format!("{arrow} Latest turn ({} messages)", lines.len())).size(12))
        .style(theme::link)
        .on_press(Message::Job(Msg::ToggleTurn));
    if !open {
        return toggle.into();
    }
    let each = lines.iter().map(|l| {
        column![
            text(l.role.clone()).size(12).font(theme::BOLD),
            text(l.text.clone()).size(12).font(theme::MONO)
        ]
        .into()
    });
    column![
        toggle,
        Column::with_children(each)
            .spacing(8)
            .padding(iced::Padding::ZERO.left(20))
    ]
    .into()
}

fn step_row<'a>(
    state: &'a State,
    d: &'a JobDetail,
    step: &'a StepView,
    turn_step: Option<&str>,
) -> Element<'a, Message> {
    let (symbol, color) = mark(&step.state);
    let open = matches!(step.state.as_str(), "running" | "waiting" | "failed")
        != state.toggled.contains(&step.id);
    let summary = row![
        text(symbol).color(color).width(16),
        text(step_name(step)).font(theme::BOLD),
        text(step.state.clone()).size(12).color(theme::MUTED)
    ]
    .spacing(12)
    .align_y(iced::Center);
    let head = button(summary)
        .width(Fill)
        .padding([10, 12])
        .style(theme::row_button)
        .on_press(Message::Job(Msg::ToggleStep(step.id.clone())));
    let mut body = Column::new()
        .spacing(12)
        .padding(iced::Padding::from([0, 12]).left(40).bottom(12));
    body = body.extend(
        d.artifacts
            .iter()
            .rev()
            .filter(|a| a.stage == step.stage)
            .map(artifact),
    );
    if turn_step == Some(step.stage.as_str()) && !d.transcript.is_empty() {
        body = body.push(turn(&d.transcript, state.turn_open));
    }
    if open {
        column![head, body].into()
    } else {
        head.into()
    }
}

fn steps<'a>(state: &'a State, d: &'a JobDetail) -> Element<'a, Message> {
    // The latest turn belongs to the last agent step that ran.
    let agent =
        |s: &&StepView| matches!(s.stage.as_str(), "plan" | "build") && s.state != "pending";
    let turn_step = d.steps.iter().rev().find(agent).map(|s| s.stage.as_str());
    let rows = d.steps.iter().map(|s| step_row(state, d, s, turn_step));
    section("Steps".into(), Column::with_children(rows).into(), false)
}

fn top(state: &State) -> Element<'_, Message> {
    let back = button(text("← All jobs").size(13))
        .style(theme::link)
        .on_press(Message::Go(Route::Jobs));
    let mut bar = row![back, iced::widget::space::horizontal()]
        .spacing(8)
        .align_y(iced::Center);
    if state.allows("extend") {
        let extend = button(text("Extend turn").size(13))
            .padding([8, 12])
            .style(theme::secondary)
            .on_press(Message::Job(Msg::Act(Action::Extend)));
        bar = bar.push(extend);
    }
    if state.allows("stop") {
        let stop = button(text("Stop").color(theme::BAD))
            .padding([8, 12])
            .style(theme::secondary);
        bar = bar.push(stop.on_press(Message::Job(Msg::Act(Action::Stop))));
    }
    bar.into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let mut page = column![top(state)].spacing(16);
    if let Some(d) = &state.detail {
        page = page.push(header(d, &ctx.current)).extend(failure(state, d));
        if state.allows("approve") || state.allows("answer") {
            page = page.push(waiting(state, d));
        }
        page = page
            .extend(holds(state, d))
            .extend(d.charter.as_ref().map(charter));
    }
    page = page.push(text(&state.error).color(theme::BAD));
    if let Some(d) = &state.detail {
        page = page.push(flow(d, &state.marks)).push(steps(state, d));
    }
    super::scrolled(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_running_turn_is_marked_still_working() {
        let job = JobSummary {
            updated_at: 1_000,
            actions: vec!["extend".into(), "stop".into()],
            ..JobSummary::default()
        };
        assert_eq!(still_working(&job, 1_000 + 9 * 60), None);
        assert_eq!(
            still_working(&job, 1_000 + 25 * 60).as_deref(),
            Some("still working, 25 min into this turn")
        );
        let waiting = JobSummary {
            actions: vec!["approve".into()],
            ..job
        };
        assert_eq!(still_working(&waiting, 1_000 + 25 * 60), None);
    }

    #[test]
    fn a_click_on_a_line_starts_a_comment_on_its_own_line() {
        let mut note = text_editor::Content::with_text("Fix this");
        append(&mut note, "calc.py:4: ");
        append(&mut note, "calc.py:9: ");
        assert_eq!(note.text(), "Fix this\ncalc.py:4: \ncalc.py:9: ");
    }

    #[test]
    fn approve_says_what_it_does_at_a_release_step() {
        assert_eq!(what_a_release_approve_does("plan", false, false), None);
        assert!(
            what_a_release_approve_does("land", false, true)
                .unwrap()
                .starts_with("CI passed")
        );
        assert!(
            what_a_release_approve_does("merge", true, false)
                .unwrap()
                .starts_with("Approve tries")
        );
        let custom = StepView {
            id: "research".into(),
            stage: "skill".into(),
            state: "done".into(),
        };
        assert_eq!(step_name(&custom), "skill · research");
    }
}
