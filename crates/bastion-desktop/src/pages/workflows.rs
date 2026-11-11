//! The Workflows page (DECISIONS.md D35): each workflow on a canvas. An admin
//! copies a workflow, adds steps, drags them, links them from a step's
//! right edge, edits the picked step, and saves a new version; everyone
//! else reads it.

use std::collections::BTreeMap;

use bastion_protocol::api::{self, Request};
use bastion_protocol::{PersonView, WorkflowView};
use iced::widget::{button, column, container, pick_list, row, text, text_editor, text_input};
use iced::{Element, Fill, Task};

use super::drawing::{self, Drawing, Pointer};
use super::edit::{self, Drag, Pick};
use super::{Choice, Ctx, labeled, scrolled};
use crate::app::Message;
use crate::theme;

/// The ways a workflow's own step runs, each with its one text.
const CUSTOM: [(&str, &str); 5] = [
    ("prompt", "Prompt"),
    ("agent", "Agent, in .claude/agents"),
    ("skill", "Skill, in .claude/skills"),
    ("command", "Command, in .claude/commands"),
    ("shell", "Shell script"),
];
const DIALS: [(&str, &str); 2] = [
    ("lights_on", "lights-on: a person decides"),
    ("dark", "dark: runs on its own"),
];

#[derive(Default)]
pub struct State {
    all: Vec<WorkflowView>,
    chosen: String,
    draft: Option<WorkflowView>,
    picked: Pick,
    drag: Option<Drag>,
    admin: bool,
    /// The picked step's text (its instructions, or a shell step's script)
    /// and its results, as typed.
    text: text_editor::Content,
    results: String,
    /// A shell step's secrets, as typed.
    secrets: String,
    /// The picked step's name, as typed: it renames the step once no
    /// other step has it.
    name: String,
    copy_name: String,
    said: String,
    no_marks: BTreeMap<String, String>,
}

/// A field of the picked step.
#[derive(Clone, Copy, Debug)]
pub enum StepField {
    Name,
    Run,
    Results,
    Secrets,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Workflows(Result<Vec<WorkflowView>, String>),
    Me(Result<PersonView, String>),
    Choose(Choice),
    Pointer(Pointer),
    AddStep(String),
    /// How the picked step runs: one of `CUSTOM`.
    Kind(Choice),
    Field(StepField, String),
    Text(text_editor::Action),
    /// Whether a shell step runs with the network (D40).
    Network(bool),
    /// Whether an agent's step may only read (D42).
    ReadsOnly(bool),
    Dial(Choice),
    RemoveStep,
    RemoveLink,
    CopyName(String),
    Copy,
    New,
    Delete,
    Save,
    Done(Result<Done, String>),
}

/// What a copy, delete or save answered.
#[derive(Clone, Debug)]
pub enum Done {
    /// The copy's id.
    Copied(String),
    Deleted,
    /// The saved workflow's new version.
    Saved(u64),
}

fn wrap(msg: Msg) -> Message {
    Message::Workflows(msg)
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let read = ctx.task(&ctx.current, api::Workflows, Msg::Workflows);
    let me = ctx.task(&ctx.current, api::Me, Msg::Me);
    Task::batch([read, me]).map(wrap)
}

fn act<R>(ctx: &Ctx, request: R, done: fn(R::Answer) -> Done) -> Task<Message>
where
    R: Request + Send + Sync + 'static,
    R::Answer: Send + 'static,
{
    let read = ctx.clone().get(ctx.current.clone(), request);
    Task::perform(read, move |r| wrap(Msg::Done(r.map(done))))
}

impl State {
    /// An admin changes any workflow; a built-in one changed is kept as theirs.
    fn editable(&self) -> bool {
        self.admin && self.draft.is_some()
    }

    /// Show the chosen workflow, or the first, as the server has it. A draft
    /// stays until another is chosen or the server keeps a new version.
    fn show_latest(&mut self) {
        let fresh = if self.chosen.is_empty() {
            self.all.first()
        } else {
            self.all.iter().find(|w| w.id == self.chosen)
        };
        let Some(fresh) = fresh.cloned() else { return };
        let same = self
            .draft
            .as_ref()
            .is_some_and(|d| (&d.id, d.version) == (&fresh.id, fresh.version));
        if !same {
            self.chosen = fresh.id.clone();
            self.draft = Some(fresh);
            self.pick(Pick::Nothing);
        }
    }

    /// Pick `pick`, and load the picked step's note and results to type in.
    fn pick(&mut self, pick: Pick) {
        let step = match &pick {
            Pick::Step(id) => self.draft.as_ref().and_then(|w| edit::node(w, id)).cloned(),
            _ => None,
        };
        self.text = text_editor::Content::with_text(&step.as_ref().map(shown).unwrap_or_default());
        self.name = step.as_ref().map(|n| n.id.clone()).unwrap_or_default();
        self.secrets = step
            .as_ref()
            .map(|n| n.reach.secrets.join(", "))
            .unwrap_or_default();
        self.results = step.map(|n| n.results.join(", ")).unwrap_or_default();
        self.picked = pick;
    }

    /// Change the picked step.
    fn step(&mut self, f: impl FnOnce(&mut bastion_protocol::NodeView)) {
        let Pick::Step(id) = &self.picked else { return };
        let node = self
            .draft
            .as_mut()
            .and_then(|w| w.nodes.iter_mut().find(|n| &n.id == id));
        if let Some(n) = node {
            f(n);
        }
    }
}

/// Whether a step's one text box holds a script, not instructions.
fn is_script(n: &bastion_protocol::NodeView) -> bool {
    n.stage == "shell"
}

/// The text a step's box shows: its script or its instructions.
fn shown(n: &bastion_protocol::NodeView) -> String {
    match is_script(n) {
        true => n.run.clone(),
        false => n.note.clone(),
    }
}

/// Keep what was typed: a script as the step's run, instructions as its note.
fn keep_text(n: &mut bastion_protocol::NodeView, typed: String) {
    match is_script(n) {
        true => n.run = typed,
        false => n.note = typed,
    }
}

fn on_pointer(state: &mut State, p: Pointer) {
    let editable = state.editable();
    let Some(w) = state.draft.as_mut() else {
        return;
    };
    match p {
        Pointer::Press(at) => {
            let (pick, drag) = edit::press(w, at, editable);
            state.drag = drag;
            state.pick(pick);
        }
        Pointer::Move(at) => {
            if let Some(d) = state.drag.as_mut() {
                edit::follow(w, d, at);
            }
        }
        Pointer::Release(at) => {
            if let Some(d) = state.drag.take() {
                edit::release(w, &d, at);
            }
        }
    }
}

/// Rename the picked step to what was typed, once that name is free.
fn rename(state: &mut State, typed: String) {
    if let (Pick::Step(id), Some(w)) = (&state.picked, state.draft.as_mut())
        && edit::rename_step(w, id, &typed).is_ok()
    {
        state.picked = Pick::Step(typed.trim().into());
    }
    state.name = typed;
}

/// A comma-separated list as typed, as its names.
fn listed(typed: &str) -> Vec<String> {
    let names = typed.split(',').map(str::trim).filter(|r| !r.is_empty());
    names.map(String::from).collect()
}

fn on_field(state: &mut State, field: StepField, value: String) {
    match field {
        StepField::Name => rename(state, value),
        StepField::Run => state.step(|n| n.run = value),
        StepField::Results => {
            let results = listed(&value);
            state.step(|n| n.results = results);
            state.results = value;
        }
        StepField::Secrets => {
            let secrets = listed(&value);
            state.step(|n| n.reach.secrets = secrets);
            state.secrets = value;
        }
    }
}

fn on_done(state: &mut State, ctx: &Ctx, answer: Result<Done, String>) -> Task<Message> {
    match answer {
        Err(e) => {
            state.said = e;
            return Task::none();
        }
        Ok(Done::Copied(id)) => {
            (state.chosen, state.copy_name) = (id, String::new());
            state.said = "Copied. Change it, then save.".into();
        }
        Ok(Done::Deleted) => {
            (state.chosen, state.said) = (
                String::new(),
                "Deleted. Its versions stay in the history.".into(),
            )
        }
        Ok(Done::Saved(version)) => state.said = format!("Saved as version {version}."),
    }
    load(ctx)
}

fn manage(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::CopyName(n) => state.copy_name = n,
        Msg::Copy => {
            let copy = api::WorkflowCopy {
                from: state.chosen.clone(),
                name: state.copy_name.trim().into(),
            };
            return act(ctx, copy, Done::Copied);
        }
        // A new workflow starts as plan, build and land, under the name typed.
        Msg::New => {
            let new = api::WorkflowCopy {
                from: "short".into(),
                name: state.copy_name.trim().into(),
            };
            return act(ctx, new, Done::Copied);
        }
        Msg::Delete => {
            let id = state.chosen.clone();
            return act(ctx, api::WorkflowDelete { id }, |()| Done::Deleted);
        }
        Msg::Save => {
            let Some(w) = state.draft.clone() else {
                return Task::none();
            };
            let save = api::WorkflowSave {
                id: w.id.clone(),
                workflow: w,
            };
            return act(ctx, save, Done::Saved);
        }
        Msg::Done(answer) => return on_done(state, ctx, answer),
        _ => {}
    }
    Task::none()
}

fn change(state: &mut State, msg: Msg) {
    match msg {
        Msg::Pointer(p) => on_pointer(state, p),
        Msg::AddStep(stage) => state
            .draft
            .iter_mut()
            .for_each(|w| edit::add_step(w, &stage)),
        Msg::Field(f, v) => on_field(state, f, v),
        Msg::Kind(_) if !state.editable() => {}
        Msg::Kind(c) => {
            state.step(|n| n.stage = c.id);
            state.pick(state.picked.clone());
        }
        Msg::Network(on) => state.step(|n| n.reach.network = on),
        Msg::ReadsOnly(on) => state.step(|n| n.reach.reads_only = on),
        Msg::Text(a) => {
            state.text.perform(a);
            let typed = state.text.text();
            state.step(|n| keep_text(n, typed));
        }
        Msg::Dial(c) => state.step(|n| n.dial = c.id),
        Msg::RemoveStep => {
            if let (Pick::Step(id), Some(w)) = (&state.picked, state.draft.as_mut()) {
                edit::remove_step(w, id);
            }
            state.pick(Pick::Nothing);
        }
        Msg::RemoveLink => {
            if let (Pick::Link(from, to), Some(w)) = (&state.picked, state.draft.as_mut()) {
                w.links.retain(|l| !(&l.from == from && &l.to == to));
            }
            state.pick(Pick::Nothing);
        }
        _ => {}
    }
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Workflows(Ok(all)) => {
            state.all = all;
            state.show_latest();
        }
        Msg::Me(Ok(me)) => state.admin = me.role == "admin",
        Msg::Workflows(Err(e)) | Msg::Me(Err(e)) => state.said = e,
        Msg::Choose(c) => {
            (state.chosen, state.draft) = (c.id, None);
            state.show_latest();
        }
        m @ (Msg::CopyName(_) | Msg::Copy | Msg::New | Msg::Delete | Msg::Save | Msg::Done(..)) => {
            return manage(state, m, ctx);
        }
        other => change(state, other),
    }
    Task::none()
}

/// The shown workflow's facts: shipped or not, its version and its level.
fn facts(w: &WorkflowView) -> String {
    let kind = if w.ships {
        "ships with Bastion"
    } else {
        "yours"
    };
    let level = w.level.replace('_', " ").replace("lights on", "lights-on");
    format!("{kind} · version {} · runs {level}", w.version)
}

fn small<'a>(label: &'a str, msg: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(13))
        .padding([4, 12])
        .style(theme::secondary)
        .on_press_maybe(msg)
        .into()
}

fn chooser(state: &State) -> Element<'_, Message> {
    let all: Vec<Choice> = state
        .all
        .iter()
        .map(|w| Choice::new(&w.id, &w.name))
        .collect();
    let shown = all.iter().find(|c| c.id == state.chosen).cloned();
    let pick = pick_list(all, shown, |c| wrap(Msg::Choose(c))).style(theme::select);
    let facts = text(state.draft.as_ref().map(facts).unwrap_or_default())
        .size(13)
        .color(theme::MUTED);
    row![pick, facts].spacing(12).align_y(iced::Center).into()
}

/// What deleting the shown workflow does: a shipped one resets to the
/// original, and one of the owner's goes.
fn delete_label(w: &WorkflowView) -> &'static str {
    match w.ships {
        true => "Reset to the original",
        false => "Delete",
    }
}

/// Start a new workflow, or copy the shown one, under the name typed; and
/// delete or reset the shown one.
fn manage_view(state: &State) -> Element<'_, Message> {
    let name = text_input("Name of a new workflow", &state.copy_name)
        .on_input(|n| wrap(Msg::CopyName(n)))
        .style(theme::field)
        .width(240);
    let named = !state.copy_name.trim().is_empty();
    let new = small("New", named.then_some(wrap(Msg::New)));
    let copy = small("Copy this", named.then_some(wrap(Msg::Copy)));
    let label = state.draft.as_ref().map(delete_label);
    let delete = label.map(|l| small(l, Some(wrap(Msg::Delete))));
    row![name, new, copy]
        .extend(delete)
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// Add a step: a prompt, which the step's panel turns into any other kind.
fn palette(state: &State) -> Element<'_, Message> {
    let add = state
        .editable()
        .then(|| wrap(Msg::AddStep("prompt".into())));
    row![small("Add a step", add)].into()
}

/// The kinds a step may be (D39).
fn kinds() -> Vec<Choice> {
    CUSTOM
        .iter()
        .map(|(id, name)| Choice::new(id, name))
        .collect()
}

/// The name an agent, skill or command step runs, and where it lives.
fn name_label(stage: &str) -> Option<&'static str> {
    match stage {
        "agent" => Some("Agent: a file in .claude/agents, without .md"),
        "skill" => Some("Skill: a folder in .claude/skills"),
        "command" => Some("Command: a file in .claude/commands, without .md"),
        _ => None,
    }
}

/// What the step's one text box is for.
fn text_label(n: &bastion_protocol::NodeView) -> &'static str {
    match n.stage.as_str() {
        "shell" => "Script, run by sh in the repository in the job's VM",
        "prompt" => "Instructions",
        _ => "Added instructions (optional)",
    }
}

/// How a step runs: its kind, its dial, and an agent's, skill's or
/// command's name.
fn how_row<'a>(n: &bastion_protocol::NodeView, editable: bool) -> Element<'a, Message> {
    let all = kinds();
    let kind = all.iter().find(|c| c.id == n.stage).cloned();
    let kind = pick_list(all, kind, |c| wrap(Msg::Kind(c)))
        .width(Fill)
        .style(theme::select);
    let dials: Vec<Choice> = DIALS
        .iter()
        .map(|(id, name)| Choice::new(id, name))
        .collect();
    let dial = dials
        .iter()
        .find(|c| c.id == n.dial)
        .cloned()
        .or_else(|| dials.first().cloned());
    let dial = pick_list(dials, dial, |c| wrap(Msg::Dial(c)))
        .width(Fill)
        .style(theme::select);
    let mut how = row![
        container(labeled("Runs as", kind)).width(Fill),
        container(labeled("Dial", dial)).width(Fill),
    ]
    .spacing(12);
    if let Some(label) = name_label(&n.stage) {
        let input = text_input("", &n.run)
            .on_input_maybe(editable.then_some(|v| wrap(Msg::Field(StepField::Run, v))))
            .style(theme::field);
        how = how.push(container(labeled(label, input)).width(Fill));
    }
    how.into()
}

/// A shell step's network and the vault's secrets it gets (D40).
fn reach_fields<'a>(
    state: &'a State,
    n: &bastion_protocol::NodeView,
    editable: bool,
) -> Element<'a, Message> {
    let network = iced::widget::checkbox(n.reach.network)
        .label("Needs the network: runs in a VM of its own with these secrets")
        .on_toggle_maybe(editable.then_some(|on| wrap(Msg::Network(on))));
    let secrets = text_input("GH_TOKEN", &state.secrets)
        .on_input_maybe(editable.then_some(|v| wrap(Msg::Field(StepField::Secrets, v))))
        .style(theme::field);
    let secrets = labeled(
        "Secrets from the vault, as environment variables, comma-separated",
        secrets,
    );
    column![network, secrets].spacing(12).into()
}

fn step_detail<'a>(state: &'a State, id: &str, editable: bool) -> Option<Element<'a, Message>> {
    let n = state
        .draft
        .as_ref()
        .and_then(|w| edit::node(w, id))?
        .clone();
    let on = |m: Msg| editable.then_some(wrap(m));
    let name = text_input("A name for this step", &state.name)
        .on_input_maybe(editable.then_some(|v| wrap(Msg::Field(StepField::Name, v))))
        .style(theme::field);
    let head = labeled("Name", name);
    let how = how_row(&n, editable);
    let mut body = column![head, how];
    let mut words = text_editor(&state.text).height(240);
    if is_script(&n) {
        words = words.font(theme::MONO);
    }
    if editable {
        words = words.on_action(|a| wrap(Msg::Text(a)));
    }
    body = body.push(labeled(text_label(&n), words));
    if is_script(&n) {
        body = body.push(reach_fields(state, &n, editable));
    } else {
        let reads = iced::widget::checkbox(n.reach.reads_only)
            .label("Only reads: the agent may not write a file")
            .on_toggle_maybe(editable.then_some(|on| wrap(Msg::ReadsOnly(on))));
        body = body.push(reads);
    }
    let results = text_input("", &state.results)
        .on_input_maybe(editable.then_some(|v| wrap(Msg::Field(StepField::Results, v))))
        .style(theme::field);
    body = body
        .push(labeled("Results it gives, comma-separated", results))
        .push(small("Remove step", on(Msg::RemoveStep)));
    Some(
        container(body.spacing(12))
            .padding(16)
            .width(Fill)
            .style(theme::panel)
            .into(),
    )
}

/// What the canvas has picked, opened below it.
fn picked(state: &State) -> Option<Element<'_, Message>> {
    let editable = state.editable();
    match &state.picked {
        Pick::Step(id) => step_detail(state, id, editable),
        Pick::Link(from, to) if editable => {
            let body = column![
                text(format!("The link from {from} to {to}")),
                small("Remove link", Some(wrap(Msg::RemoveLink)))
            ]
            .spacing(8);
            Some(
                container(body)
                    .padding(16)
                    .width(Fill)
                    .style(theme::panel)
                    .into(),
            )
        }
        _ => None,
    }
}

pub fn view(state: &State) -> Element<'_, Message> {
    let editable = state.editable();
    let mut body =
        column![text("Workflows").size(24).font(theme::BOLD), chooser(state)].spacing(12);
    if state.admin {
        body = body.push(manage_view(state));
    }
    if editable {
        body = body.push(palette(state));
    }
    if let Some(w) = &state.draft {
        let on: fn(Pointer) -> Message = |p| wrap(Msg::Pointer(p));
        body = body.push(drawing::view(Drawing {
            workflow: w,
            marks: &state.no_marks,
            picked: &state.picked,
            editable,
            drag: state.drag.as_ref(),
            on: Some(on),
        }));
    }
    body = body.extend(picked(state));
    if editable {
        body = body.push(
            button("Save a new version")
                .padding([8, 12])
                .style(theme::primary)
                .on_press(wrap(Msg::Save)),
        );
    }
    scrolled(body.push(text(&state.said).size(13).color(theme::MUTED)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow(id: &str, version: u64, ships: bool) -> WorkflowView {
        WorkflowView {
            id: id.into(),
            name: id.into(),
            version,
            ships,
            ..WorkflowView::default()
        }
    }

    #[test]
    fn a_draft_stays_until_the_server_keeps_a_new_version() {
        let mut state = State {
            all: vec![workflow("short", 1, true), workflow("mine", 1, false)],
            ..State::default()
        };
        state.show_latest();
        assert_eq!(state.chosen, "short");
        state.chosen = "mine".into();
        state.draft = None;
        state.show_latest();
        state.draft.as_mut().unwrap().name = "edited".into();
        state.show_latest();
        assert_eq!(
            state.draft.as_ref().unwrap().name,
            "edited",
            "the same version keeps the draft"
        );
        state.all[1].version = 2;
        state.show_latest();
        assert_eq!(state.draft.as_ref().unwrap().version, 2);
        assert_eq!(state.draft.as_ref().unwrap().name, "mine");
    }

    #[test]
    fn an_admin_changes_any_workflow_and_resets_a_shipped_one() {
        let mut state = State {
            draft: Some(workflow("mine", 1, false)),
            ..State::default()
        };
        assert!(!state.editable());
        state.admin = true;
        assert!(state.editable());
        let short = workflow("short", 0, true);
        state.draft = Some(short.clone());
        assert!(state.editable(), "a shipped one is changed as the admin's");
        assert_eq!(delete_label(&short), "Reset to the original");
        assert_eq!(delete_label(&workflow("mine", 1, false)), "Delete");
    }

    #[test]
    fn results_are_kept_as_typed_and_stored_as_a_list() {
        let mut w = workflow("mine", 1, false);
        edit::add_step(&mut w, "plan");
        let mut state = State {
            draft: Some(w),
            admin: true,
            ..State::default()
        };
        state.pick(Pick::Step("plan".into()));
        on_field(&mut state, StepField::Results, "design-notes, ".into());
        assert_eq!(state.results, "design-notes, ");
        assert_eq!(state.draft.unwrap().nodes[0].results, ["design-notes"]);
    }

    #[test]
    fn a_step_is_renamed_with_its_links_once_the_name_is_free() {
        let mut w = workflow("mine", 1, false);
        edit::add_step(&mut w, "prompt");
        edit::add_step(&mut w, "shell");
        assert_eq!(
            (w.nodes[0].id.as_str(), w.nodes[1].id.as_str()),
            ("step", "step2")
        );
        edit::add_link(&mut w, "step", "step2");
        let mut state = State {
            draft: Some(w),
            admin: true,
            ..State::default()
        };
        state.pick(Pick::Step("step".into()));
        on_field(&mut state, StepField::Name, "step2".into());
        assert_eq!(
            state.picked,
            Pick::Step("step".into()),
            "a taken name waits"
        );
        on_field(&mut state, StepField::Name, "plan".into());
        let w = state.draft.as_ref().unwrap();
        assert_eq!(
            (w.nodes[0].id.as_str(), w.links[0].from.as_str()),
            ("plan", "plan")
        );
        assert_eq!(state.picked, Pick::Step("plan".into()));
    }

    #[test]
    fn a_step_shows_its_text_and_keeps_what_is_typed_by_its_kind() {
        let mut w = workflow("mine", 1, false);
        edit::add_step(&mut w, "prompt");
        w.nodes[0].note = "Plan the change.".into();
        let mut state = State {
            draft: Some(w),
            admin: true,
            ..State::default()
        };
        let node = |state: &State| state.draft.as_ref().unwrap().nodes[0].clone();
        let id = node(&state).id;
        state.pick(Pick::Step(id));
        assert_eq!(state.text.text(), "Plan the change.");
        let typed = |state: &mut State, words: &str| {
            state.text = text_editor::Content::with_text(words);
            let words = state.text.text();
            state.step(|n| keep_text(n, words));
        };
        typed(&mut state, "Plan in three lines.");
        assert_eq!(node(&state).note, "Plan in three lines.");
        change(&mut state, Msg::Kind(Choice::new("shell", "Shell script")));
        typed(&mut state, "make lint\nmake test");
        assert_eq!(
            (node(&state).run.as_str(), node(&state).note.as_str()),
            ("make lint\nmake test", "Plan in three lines."),
            "a script is the step's run"
        );
        state.admin = false;
        change(&mut state, Msg::Kind(Choice::new("prompt", "Prompt")));
        assert_eq!(node(&state).stage, "shell", "a reader cannot change a step");
    }
}
