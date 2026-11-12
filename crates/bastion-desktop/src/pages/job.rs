//! The job page: one job as a run. The request and its badges,
//! the owner's actions when it waits, what governs it, its workflow, then
//! its steps, each opening to its artifacts and turn.

use std::collections::{BTreeMap, BTreeSet};

use bastion_protocol::api::{self, Request};
use bastion_protocol::{
    ArtifactView, CharterView, HeldCallRef, HeldCallView, JobDetail, JobSummary, StepView,
    TranscriptLine,
};
use iced::widget::markdown;
use iced::widget::{Column, column, container, row, scrollable, text, text_editor, text_input};
use iced::{Element, Fill, Task};

use super::{Ctx, ago, diff, mark, section};
use crate::app::Message;
use crate::route::Route;
use crate::theme;

pub struct State {
    id: String,
    detail: Option<JobDetail>,
    note: text_editor::Content,
    error: String,
    /// The field note being written, and what it is about.
    field: String,
    field_about: &'static str,
    /// A lesson's shape, and its hook's command or permission's rule.
    field_shape: &'static str,
    field_detail: String,
    /// What the owner typed to refuse each held call, by call id.
    feedback: BTreeMap<String, String>,
    /// The steps the owner opened or closed, against their default.
    toggled: BTreeSet<String>,
    turn_open: bool,
    /// Each step's state by its node id, for the drawing.
    marks: BTreeMap<String, String>,
    /// The landmarks of the job's trail.
    trail: Vec<StepView>,
    /// The landmark pressed, whose card is open over the trail.
    pressed: Option<super::trail::Pressed>,
    /// Each artifact's body, parsed as markdown, by its text.
    docs: BTreeMap<String, markdown::Content>,
    /// The diff lines the last click named in the note.
    pick: Option<diff::Pick>,
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
            pick: None,
            marks: BTreeMap::new(),
            trail: Vec::new(),
            pressed: None,
            field: String::new(),
            field_about: "lesson",
            field_shape: "rule",
            field_detail: String::new(),
            docs: BTreeMap::new(),
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
    /// A click on a diff line: its file and its line.
    CommentAt(String, u32),
    Act(Action),
    Acted(Result<(), String>),
    Feedback(String, String),
    Allow(String),
    Refuse(String),
    ToggleStep(String),
    /// A landmark pressed on the trail; its card opens over the trail.
    Pressed(super::trail::Pressed),
    /// Close the trail's card.
    CloseCard,
    /// The field note's words, what it is about, and signing it (D59).
    Field(String),
    FieldAbout(&'static str),
    FieldShape(&'static str),
    FieldDetail(String),
    Sign,
    ToggleTurn,
    /// Open a kept screenshot or recording with the system's viewer.
    OpenMedia(String),
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

/// A click on a diff line names it in the note. A shift-click on a line of
/// the same file widens what the last click named into a range instead.
fn comment_at(state: &mut State, path: String, n: u32, shift: bool) {
    let before = state.pick.take();
    let widened = before
        .as_ref()
        .filter(|_| shift)
        .and_then(|p| p.to(&path, n));
    let pick = match (widened, before) {
        (Some(wide), Some(before)) => {
            rename(&mut state.note, &before.reference(), &wide.reference());
            wide
        }
        _ => {
            let one = diff::Pick::line(path, n);
            append(&mut state.note, &one.reference());
            one
        }
    };
    state.pick = Some(pick);
}

/// Put `new` where the note last says `old`; append it if the note no
/// longer says `old`.
fn rename(note: &mut text_editor::Content, old: &str, new: &str) {
    let t = note.text();
    let Some(at) = t.rfind(old) else {
        return append(note, new);
    };
    let renamed = format!("{}{new}{}", &t[..at], &t[at + old.len()..]);
    *note = text_editor::Content::with_text(&renamed);
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
            state.docs = parsed(&d.artifacts);
            state.trail = trail_steps(&d);
            state.detail = Some(*d);
        }
        Msg::Read(Err(e)) | Msg::Acted(Err(e)) => state.error = e,
        Msg::Edit(action) => state.note.perform(action),
        Msg::CommentAt(path, n) => comment_at(state, path, n, ctx.shift),
        Msg::Act(action) => return act(state, ctx, action),
        Msg::Acted(Ok(_)) => {
            state.note = text_editor::Content::new();
            state.pick = None;
            state.error.clear();
            // A field note pushes no event, so read the job again.
            return load(state, ctx);
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
        Msg::Pressed(p) => state.pressed = Some(p),
        Msg::CloseCard => state.pressed = None,
        Msg::Field(t) => state.field = t,
        Msg::FieldAbout(a) => state.field_about = a,
        Msg::FieldShape(a) => state.field_shape = a,
        Msg::FieldDetail(t) => state.field_detail = t,
        Msg::Sign => {
            let note = api::FieldNote {
                id: state.id.clone(),
                about: state.field_about.into(),
                shape: state.field_shape.into(),
                detail: std::mem::take(&mut state.field_detail),
                text: std::mem::take(&mut state.field),
            };
            return ask(ctx, note);
        }
        Msg::OpenMedia(name) => return open_media(ctx, &state.id, name),
    }
    Task::none()
}

/// Whether an artifact is a change: a diff, or the note that there is none.
fn is_change(a: &ArtifactView) -> bool {
    a.body.starts_with("diff --git") || a.body.starts_with("No commits:")
}

/// What the wait judges: the waiting step's latest report or output, and
/// the latest change any step made.
fn judged(d: &JobDetail) -> (Option<String>, Option<String>) {
    let at = d.job.node.as_deref().unwrap_or_default();
    let latest = |keep: &dyn Fn(&ArtifactView) -> bool| {
        d.artifacts
            .iter()
            .rev()
            .find(|a| keep(a))
            .map(|a| a.body.clone())
    };
    let report = latest(&|a| !at.is_empty() && a.step == at && !is_change(a));
    (report, latest(&is_change))
}

/// What Approve does where the job waits (D41).
fn what_approve_does(d: &JobDetail) -> &'static str {
    let failed = d.job.error.is_some();
    match (d.job.node.is_some(), failed) {
        (false, true) => "Approve tries the finish again.",
        (false, false) => {
            "Approve finishes the job: its branch is ready, and the workspace merges it or leaves it."
        }
        (true, true) => "Approve goes on without this step, or send it back with a note.",
        (true, false) => "Approve this step's result to move on, or send it back with a note.",
    }
}

/// A step's name, which the workflow gives it.
fn step_name(step: &StepView) -> String {
    step.id.replace('_', " ")
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

/// Each artifact's body as markdown, by its text.
fn parsed(artifacts: &[ArtifactView]) -> BTreeMap<String, markdown::Content> {
    artifacts
        .iter()
        .map(|a| (a.body.clone(), markdown::Content::parse(&a.body)))
        .collect()
}

/// An agent's text, rendered as markdown, in a block like [`block`]'s.
/// Text not parsed yet shows as it is.
fn doc<'a>(state: &'a State, body: String) -> Element<'a, Message> {
    let Some(content) = state.docs.get(&body) else {
        return block(body);
    };
    // iced's default here is the system face, not the app's, and a near-black
    // highlight that vanishes on this block's background.
    let style = markdown::Style {
        font: theme::FONT,
        inline_code_font: theme::MONO,
        inline_code_color: theme::link_color(),
        inline_code_padding: iced::Padding::from([0, 2]),
        inline_code_highlight: markdown::Highlight {
            background: theme::panel_color().into(),
            border: iced::border::rounded(3),
        },
        code_block_font: theme::MONO,
        ..markdown::Style::from(&theme::bastion())
    };
    // A report's headings sit just above its text, not at twice its size.
    let settings = markdown::Settings {
        h1_size: 18.into(),
        h2_size: 16.into(),
        h3_size: 15.into(),
        h4_size: 14.into(),
        ..markdown::Settings::with_text_size(14, style)
    };
    let shown = markdown::view(content.items(), settings).map(Message::OpenUrl);
    let body = container(shown).padding(12).width(Fill);
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
    crate::lift::button(text(label).size(12).color(theme::link_color()))
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
    let state = super::journey_word(&d.job.state);
    let state = match super::IN_PROGRESS.contains(&d.job.state.as_str()) {
        true => row![super::mark_view(&d.job.state, 16.0), chip(state, color)]
            .spacing(6)
            .align_y(iced::Center)
            .into(),
        false => chip(format!("{symbol} {state}"), color),
    };
    let mut badges = vec![
        state,
        chip(server.into(), theme::muted()),
        chip(d.job.workspace.clone(), theme::muted()),
        chip(d.job.id.clone(), theme::muted()),
    ];
    badges.push(chip(ago(d.job.created_at), theme::muted()));
    badges.extend(
        d.job
            .issue
            .iter()
            .map(|u| link_chip(format!("issue {}", last_segment(u)), u.clone())),
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    badges.extend(still_working(&d.job, now).map(|t| chip(t, theme::warn())));
    // The title is the request's first line; a longer request reads in full under it.
    let title = super::jobs::title(&d.job.request);
    let mut head = column![text(title.clone()).size(20).font(theme::BOLD)].spacing(8);
    if title != d.job.request.trim() {
        head = head.push(text(d.job.request.clone()).size(14).color(theme::muted()));
    }
    head.push(row(badges).spacing(8).wrap()).into()
}

/// Why the job failed, or why its land did.
fn failure<'a>(state: &State, d: &JobDetail) -> Option<Element<'a, Message>> {
    let e = d.job.error.clone()?;
    let title = match (d.job.state.as_str(), d.job.node.is_some()) {
        ("failed", _) => "This job failed",
        (_, false) => "The finish failed; the VM and its commits are kept",
        _ => "This step is stuck",
    };
    let mut body = column![
        text(title).size(13).font(theme::BOLD).color(theme::bad()),
        block(e)
    ]
    .spacing(8);
    if state.allows("retry") {
        let retry = crate::lift::button(text("Retry").size(13))
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

/// Approving a stuck step goes on without it; at the finish, it tries again.
fn approve_label(state: &State) -> &'static str {
    let at_step = state.detail.as_ref().is_some_and(|d| d.job.node.is_some());
    match (state.allows("retry"), at_step) {
        (true, true) => "Ship anyway",
        (true, false) => "Finish again",
        _ => "Approve",
    }
}

fn actions(state: &State) -> Element<'_, Message> {
    let answer = state.allows("answer");
    let mut buttons = row![].spacing(8);
    if state.allows("approve") {
        buttons = buttons.push(
            crate::lift::button(approve_label(state))
                .padding([8, 12])
                .style(theme::primary)
                .on_press(Message::Job(Msg::Act(Action::Approve))),
        );
    }
    if state.allows("comment") || answer {
        let label = if answer { "Answer" } else { "Send back" };
        let ready = !state.note.text().trim().is_empty();
        let send = crate::lift::button(label)
            .padding([8, 12])
            .style(theme::secondary);
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
        column![text(label).size(13).color(theme::muted()), editor]
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
            .color(theme::warn()),
    );
    if let Some(q) = &d.job.question {
        body = body.push(row![text("The agent asks: ").font(theme::BOLD), text(q.clone())].wrap());
    }
    if state.allows("approve") {
        body = body.push(text(what_approve_does(d)).size(13).color(theme::muted()));
        let (report, change) = judged(d);
        body = body.extend(report.map(|r| doc(state, r)));
        if let Some(change) = change {
            body = body.push(
                text(
                    "The change (click a line to comment on it; shift-click to take in more lines)",
                )
                .size(13)
                .font(theme::BOLD)
                .color(theme::muted()),
            );
            let comment = |path, n| Message::Job(Msg::CommentAt(path, n));
            body = body.push(diff::view(&change, comment, state.pick.as_ref()));
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
    let allow = crate::lift::button("Allow once")
        .padding([8, 12])
        .style(theme::primary)
        .on_press(Message::Job(Msg::Allow(call.id.clone())));
    let refuse = crate::lift::button("Refuse with this note")
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
            .color(theme::muted())
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
        .color(theme::warn());
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
        body = body.push(text(title).size(12).font(theme::BOLD).color(theme::muted()));
        body = body.extend(items.iter().map(|i| text(format!("• {i}")).size(12).into()));
    }
    container(body)
        .padding(16)
        .width(Fill)
        .style(theme::panel)
        .into()
}

/// The steps the trail draws: the job's own, or for a job from before
/// steps were kept, its workflow's steps in canvas order, passed when the
/// job is at no step.
// ponytail: an old job's steps are guessed from its node; the server could keep them.
fn trail_steps(d: &JobDetail) -> Vec<StepView> {
    if !d.steps.is_empty() {
        return d.steps.clone();
    }
    let mut nodes: Vec<&bastion_protocol::NodeView> = d.workflow.nodes.iter().collect();
    nodes.sort_by_key(|n| (n.x, n.y));
    let passed = d.job.node.is_none();
    nodes
        .into_iter()
        .map(|n| StepView {
            id: n.id.clone(),
            stage: n.stage.clone(),
            state: if passed { "done" } else { "pending" }.into(),
        })
        .collect()
}

/// The job as a trail (L4): its steps as landmarks from the request to
/// the final peak, and each result its steps gave, as a document.
fn flow<'a>(state: &'a State, d: &'a JobDetail) -> Element<'a, Message> {
    let trail = super::trail::view(super::trail::Trail {
        steps: &state.trail,
        job: &d.job.state,
        pick: |p| Message::Job(Msg::Pressed(p)),
    });
    let mut map = iced::widget::stack![trail];
    if let Some(p) = &state.pressed {
        let (x, y) = card_at(p);
        let shown = match p.id == super::trail::ATLAS {
            true => atlas_card(state, d),
            false => step_card(state, d, &p.id),
        };
        let card = iced::widget::pin(shown).x(x).y(y);
        map = map.push(card.width(Fill).height(super::trail::TALL));
    }
    let mut body = column![map].spacing(8);
    for (name, result) in &d.results {
        let head = text(format!("Result · {name}"))
            .size(12)
            .font(theme::BOLD)
            .color(theme::muted());
        body = body.push(
            container(column![head, block(result.clone())].spacing(6))
                .padding(12)
                .width(Fill)
                .style(theme::panel),
        );
    }
    body.into()
}

/// The trail card's width, and its tallest before it scrolls.
const CARD: f32 = 460.0;
const CARD_MAX: f32 = 230.0;

/// Where the card goes over the trail: beside the landmark, flipped left
/// near the right edge.
fn card_at(p: &super::trail::Pressed) -> (f32, f32) {
    let right = p.at.x + 34.0;
    let x = if right + CARD > p.width - 16.0 {
        p.at.x - 34.0 - CARD
    } else {
        right
    };
    (x.max(16.0), 16.0)
}

/// How a lesson is codified (D61): a rule file, or a hook or permission
/// in the region's settings, with the command or rule it needs.
fn lesson_shape(state: &State) -> Element<'_, Message> {
    let pick = |label: &'static str, shape: &'static str| {
        let on = state.field_shape == shape;
        super::step_rules::choice(label, on, Some(Message::Job(Msg::FieldShape(shape))))
    };
    let shapes = row![
        pick("As a rule", "rule"),
        pick("As a hook", "hook"),
        pick("Allow", "allow"),
        pick("Ask", "ask"),
        pick("Deny", "deny")
    ]
    .spacing(6);
    let hint = match state.field_shape {
        "rule" => return shapes.into(),
        "hook" => "The command to run before each shell call, such as ./lint.sh",
        _ => "The rule, such as Bash(git push:*)",
    };
    let detail = text_input(hint, &state.field_detail)
        .on_input(|t| Message::Job(Msg::FieldDetail(t)))
        .style(theme::field);
    column![shapes, detail].spacing(6).into()
}

/// What to write for each kind of note (D61).
fn field_hint(about: &str) -> &'static str {
    match about {
        "lesson" => "A lesson: what to do next time, as a rule for the region",
        "fact" => "A fact: why the code is this way, a surprise, a scenario",
        _ => "About Bastion: what its trail or guide should change",
    }
}

/// The job's field notes, and a short form to sign one (D59): what the
/// job taught, about its workspace or about Bastion.
fn atlas_card<'a>(state: &'a State, d: &'a JobDetail) -> Element<'a, Message> {
    let title = text("Field notes").size(15).font(theme::BOLD);
    let close = crate::lift::button(text("Close").size(13))
        .style(theme::link)
        .on_press(Message::Job(Msg::CloseCard));
    let head = row![title, iced::widget::space::horizontal(), close].align_y(iced::Center);
    let notes = d.notes.iter().map(|n| {
        let whom = match (n.about.as_str(), n.branch.is_empty()) {
            ("bastion", _) => "about Bastion".to_string(),
            (kind, false) => format!("a {kind} on {}, to merge", n.branch),
            (kind, true) => format!("a {kind}"),
        };
        let by = text(format!("{} · {whom} · {}", n.by, ago(n.at))).size(12);
        column![text(n.text.clone()).size(13), by.color(theme::muted())]
            .spacing(2)
            .into()
    });
    let mut body = Column::new().spacing(10).extend(notes);
    if d.notes.is_empty() {
        let none = "No field notes yet. What did this journey teach?";
        body = body.push(text(none).size(13).color(theme::muted()));
    }
    let pick = |label: &'static str, about: &'static str| {
        let on = state.field_about == about;
        super::step_rules::choice(label, on, Some(Message::Job(Msg::FieldAbout(about))))
    };
    let about = row![
        pick("A lesson", "lesson"),
        pick("A fact", "fact"),
        pick("About Bastion", "bastion")
    ]
    .spacing(6);
    let words = text_input(field_hint(state.field_about), &state.field)
        .on_input(|t| Message::Job(Msg::Field(t)))
        .on_submit(Message::Job(Msg::Sign))
        .style(theme::field);
    let sign = crate::lift::button(text("Sign").size(13))
        .style(theme::primary)
        .on_press_maybe((!state.field.trim().is_empty()).then_some(Message::Job(Msg::Sign)));
    let mut form = column![about].spacing(6);
    if state.field_about == "lesson" {
        form = form.push(lesson_shape(state));
    }
    let form = form.push(row![words, sign].spacing(8));
    let body = scrollable(body).height(iced::Length::Shrink);
    container(column![head, container(body).max_height(CARD_MAX - 70.0), form].spacing(10))
        .padding(12)
        .width(CARD)
        .style(theme::panel)
        .into()
}

/// A step's output: the results it gave, its artifacts, and its latest
/// turn, or a line that says it has none yet.
fn step_output<'a>(state: &'a State, d: &'a JobDetail, id: &'a str) -> Column<'a, Message> {
    let node = d.workflow.nodes.iter().find(|n| n.id == id);
    let gives = |name: &String| name == id || node.is_some_and(|n| n.results.contains(name));
    let results = d
        .results
        .iter()
        .filter(|(name, _)| gives(name))
        .map(|(name, out)| {
            let head = text(format!("Result · {name}")).size(12).font(theme::BOLD);
            column![head.color(theme::muted()), block(out.clone())]
                .spacing(4)
                .into()
        });
    let mine: Vec<&ArtifactView> = d.artifacts.iter().rev().filter(|a| a.step == id).collect();
    let none = !d.results.keys().any(gives) && mine.is_empty();
    let out = mine.into_iter().map(|a| artifact(state, a));
    let mut body = Column::new().spacing(10).extend(results).extend(out);
    let agent = |s: &&StepView| s.stage != "shell" && s.state != "pending";
    let turned = d.steps.iter().rev().find(agent).is_some_and(|s| s.id == id);
    if turned && !d.transcript.is_empty() {
        body = body.push(turn(&d.transcript, state.turn_open));
    }
    if none && !turned {
        let line = text("No output from this step yet.").size(13);
        body = body.push(line.color(theme::muted()));
    }
    body
}

/// A step's card on the trail: its state, its output, its latest turn,
/// and what it does.
fn step_card<'a>(state: &'a State, d: &'a JobDetail, id: &'a str) -> Element<'a, Message> {
    let step = d.steps.iter().find(|s| s.id == id);
    let st = step.map_or("not run".to_string(), |s| s.state.clone());
    let title = text(id.replace('_', " ")).size(15).font(theme::BOLD);
    let close = crate::lift::button(text("Close").size(13))
        .style(theme::link)
        .on_press(Message::Job(Msg::CloseCard));
    let head = row![
        title,
        text(st).size(12).color(theme::muted()),
        iced::widget::space::horizontal(),
        close
    ]
    .spacing(8)
    .align_y(iced::Center);
    let mut body = step_output(state, d, id);
    if let Some(n) = d.workflow.nodes.iter().find(|n| n.id == id) {
        let what = text("What it does").size(12).font(theme::BOLD);
        body = body
            .push(what.color(theme::muted()))
            .push(super::map::does(n));
    }
    let body = scrollable(body).height(iced::Length::Shrink);
    container(column![head, container(body).max_height(CARD_MAX)].spacing(8))
        .padding(12)
        .width(CARD)
        .style(theme::panel)
        .into()
}

fn artifact<'a>(state: &'a State, a: &ArtifactView) -> Element<'a, Message> {
    let head = text(format!("{} artifact · {}", a.stage, ago(a.created_at)))
        .size(12)
        .font(theme::BOLD)
        .color(theme::muted());
    column![head, doc(state, a.body.clone())].spacing(4).into()
}

fn turn<'a>(lines: &'a [TranscriptLine], open: bool) -> Element<'a, Message> {
    let arrow = if open { "▾" } else { "▸" };
    let toggle = crate::lift::button(
        text(format!("{arrow} Latest turn ({} messages)", lines.len())).size(12),
    )
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
    let open = matches!(step.state.as_str(), "running" | "waiting" | "failed")
        != state.toggled.contains(&step.id);
    let summary = row![
        container(super::mark_view(&step.state, 16.0)).width(16),
        text(step_name(step)).font(theme::BOLD),
        text(step.state.clone()).size(12).color(theme::muted())
    ]
    .spacing(12)
    .align_y(iced::Center);
    let head = crate::lift::button(summary)
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
            .filter(|a| a.step == step.id)
            .map(|a| artifact(state, a)),
    );
    if turn_step == Some(step.id.as_str()) && !d.transcript.is_empty() {
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
    let agent = |s: &&StepView| s.stage != "shell" && s.state != "pending";
    let turn_step = d.steps.iter().rev().find(agent).map(|s| s.id.as_str());
    let rows = d.steps.iter().map(|s| step_row(state, d, s, turn_step));
    section("Steps".into(), Column::with_children(rows).into(), false)
}

fn top(state: &State) -> Element<'_, Message> {
    let back = crate::lift::button(text("← All journeys").size(13))
        .style(theme::link)
        .on_press(Message::Go(Route::Jobs));
    let mut bar = row![back, iced::widget::space::horizontal()]
        .spacing(8)
        .align_y(iced::Center);
    if state.allows("extend") {
        let extend = crate::lift::button(text("Extend turn").size(13))
            .padding([8, 12])
            .style(theme::secondary)
            .on_press(Message::Job(Msg::Act(Action::Extend)));
        bar = bar.push(extend);
    }
    if state.allows("stop") {
        let stop = crate::lift::button(text("Stop").color(theme::bad()))
            .padding([8, 12])
            .style(theme::secondary);
        bar = bar.push(stop.on_press(Message::Job(Msg::Act(Action::Stop))));
    }
    bar.into()
}

/// Fetch a kept file, put it in a temporary folder, and open it there.
fn open_media(ctx: &Ctx, job: &str, name: String) -> Task<Message> {
    let (ctx, job) = (ctx.clone(), job.to_string());
    let saved = async move {
        let server = ctx.server(&ctx.current)?;
        let bytes = bastion_client::client::media(&ctx.http, &server, &job, &name).await?;
        let dir = std::env::temp_dir().join("bastion-media").join(&job);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(&name);
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok::<_, String>(path.display().to_string())
    };
    Task::perform(saved, |done| match done {
        Ok(path) => Message::OpenUrl(path),
        Err(e) => Message::Job(Msg::Acted(Err(e))),
    })
}

/// The screenshots and recordings the job's turns kept, each to open.
fn kept_files(d: &JobDetail) -> Option<Element<'_, Message>> {
    if d.media.is_empty() {
        return None;
    }
    let rows = d.media.iter().map(|name| {
        let open = crate::lift::button(text("Open").size(12))
            .style(theme::link)
            .on_press(Message::Job(Msg::OpenMedia(name.clone())));
        row![
            text(name.clone()).size(13).font(theme::MONO).width(Fill),
            open
        ]
        .spacing(8)
        .into()
    });
    let body = iced::widget::Column::with_children(rows)
        .spacing(4)
        .padding([8, 12]);
    Some(section("Kept files".into(), body.into(), false))
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let mut page = column![top(state)].spacing(16);
    if let Some(d) = &state.detail {
        page = page
            .push(header(d, &ctx.current))
            .push(flow(state, d))
            .extend(failure(state, d));
        if state.allows("approve") || state.allows("answer") {
            page = page.push(waiting(state, d));
        }
        page = page
            .extend(holds(state, d))
            .extend(kept_files(d))
            .extend(d.charter.as_ref().map(charter));
    }
    if !state.error.is_empty() {
        page = page.push(text(&state.error).color(theme::bad()));
    }
    if let Some(d) = &state.detail {
        page = page.push(steps(state, d));
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
    fn an_artifact_reads_as_markdown() {
        let a = ArtifactView {
            stage: "prompt".into(),
            step: "build".into(),
            body: "**What I changed**\n- `calc.py`: a docstring\n".into(),
            created_at: 0,
        };
        let docs = parsed(&[a]);
        let items = docs.values().next().unwrap().items();
        assert!(matches!(items[0], markdown::Item::Paragraph(_)));
        assert!(matches!(items[1], markdown::Item::List { .. }));
    }

    #[test]
    fn a_shift_click_widens_the_last_line_named_to_a_range() {
        let mut state = State::new("j1".into());
        state.note = text_editor::Content::with_text("Fix this");
        comment_at(&mut state, "calc.py".into(), 12, false);
        comment_at(&mut state, "calc.py".into(), 15, true);
        assert_eq!(state.note.text(), "Fix this\ncalc.py:12-15: ");
        comment_at(&mut state, "calc.py".into(), 9, true);
        assert_eq!(state.note.text(), "Fix this\ncalc.py:9-15: ");
        // Another file, or no shift, starts a new line of the note.
        comment_at(&mut state, "main.py".into(), 3, true);
        comment_at(&mut state, "main.py".into(), 4, false);
        assert_eq!(
            state.note.text(),
            "Fix this\ncalc.py:9-15: \nmain.py:3: \nmain.py:4: "
        );
    }

    #[test]
    fn a_click_on_a_line_starts_a_comment_on_its_own_line() {
        let mut note = text_editor::Content::with_text("Fix this");
        append(&mut note, "calc.py:4: ");
        append(&mut note, "calc.py:9: ");
        assert_eq!(note.text(), "Fix this\ncalc.py:4: \ncalc.py:9: ");
    }

    #[test]
    fn a_wait_judges_its_steps_report_and_the_latest_change() {
        let art = |step: &str, body: &str| ArtifactView {
            stage: "prompt".into(),
            step: step.into(),
            body: body.into(),
            created_at: 0,
        };
        let mut d = JobDetail::default();
        d.job.node = Some("review".into());
        d.artifacts = vec![
            art("build", "Built."),
            art("build", "diff --git a/x b/x"),
            art("review", "Looks right."),
        ];
        assert_eq!(
            judged(&d),
            (
                Some("Looks right.".into()),
                Some("diff --git a/x b/x".into())
            )
        );
        assert!(what_approve_does(&d).starts_with("Approve this step's"));
        (d.job.node, d.job.error) = (None, Some("conflict".into()));
        assert_eq!(what_approve_does(&d), "Approve tries the finish again.");
        assert_eq!(judged(&d).0, None, "the finish has no report of its own");
        let custom = StepView {
            id: "research".into(),
            stage: "skill".into(),
            state: "done".into(),
        };
        assert_eq!(step_name(&custom), "research", "a step goes by its name");
    }
}
