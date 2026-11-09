//! The Workflows page (PLAN.md D35): each workflow on a canvas. An admin
//! copies a workflow, adds steps, drags them, links them from a step's
//! right edge, edits the picked step, and saves a new version; everyone
//! else reads it.

use std::collections::BTreeMap;

use bastion_protocol::{PersonView, WorkflowView, command};
use iced::widget::{button, column, container, pick_list, row, text, text_editor, text_input};
use iced::{Element, Fill, Task};
use serde_json::{Value, json};

use super::drawing::{self, Drawing, Pointer, title};
use super::edit::{self, Drag, Pick};
use super::{Choice, Ctx, labeled, scrolled};
use crate::app::Message;
use crate::client::Ask;
use crate::theme;

const CUSTOM: [&str; 3] = ["skill", "prompt", "command"];
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
    /// The picked step's note and results as typed.
    note: text_editor::Content,
    results: String,
    copy_name: String,
    said: String,
    no_marks: BTreeMap<String, String>,
}

/// A field of the picked step.
#[derive(Clone, Copy, Debug)]
pub enum StepField {
    Run,
    Results,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Workflows(Result<Vec<WorkflowView>, String>),
    Me(Result<PersonView, String>),
    Choose(Choice),
    Pointer(Pointer),
    AddStep(String),
    Field(StepField, String),
    Note(text_editor::Action),
    Dial(Choice),
    RemoveStep,
    RemoveLink,
    CopyName(String),
    Copy,
    Delete,
    Save,
    Done(&'static str, Result<Value, String>),
}

fn wrap(msg: Msg) -> Message {
    Message::Workflows(msg)
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let read = ctx.task(&ctx.current, Ask::bare(command::WORKFLOWS), Msg::Workflows);
    let me = ctx.task(&ctx.current, Ask::bare(command::ME), Msg::Me);
    Task::batch([read, me]).map(wrap)
}

fn act(ctx: &Ctx, what: &'static str, ask: Ask) -> Task<Message> {
    let read = ctx.clone().get::<Value>(ctx.current.clone(), ask);
    Task::perform(read, move |r| wrap(Msg::Done(what, r)))
}

impl State {
    fn editable(&self) -> bool {
        self.admin && self.draft.as_ref().is_some_and(|w| !w.built_in)
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
        self.note = text_editor::Content::with_text(step.as_ref().map_or("", |n| n.note.as_str()));
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

fn on_field(state: &mut State, field: StepField, value: String) {
    match field {
        StepField::Run => state.step(|n| n.run = value),
        StepField::Results => {
            let results = value
                .split(',')
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .map(String::from)
                .collect();
            state.step(|n| n.results = results);
            state.results = value;
        }
    }
}

fn on_done(
    state: &mut State,
    ctx: &Ctx,
    what: &'static str,
    answer: Result<Value, String>,
) -> Task<Message> {
    match (what, answer) {
        (_, Err(e)) => {
            state.said = e;
            return Task::none();
        }
        ("copy", Ok(id)) => {
            (state.chosen, state.copy_name) =
                (id.as_str().unwrap_or_default().into(), String::new());
            state.said = "Copied. Change it, then save.".into();
        }
        ("delete", Ok(_)) => {
            (state.chosen, state.said) = (
                String::new(),
                "Deleted. Its versions stay in the history.".into(),
            )
        }
        (_, Ok(version)) => state.said = format!("Saved as version {version}."),
    }
    load(ctx)
}

fn manage(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::CopyName(n) => state.copy_name = n,
        Msg::Copy => {
            return act(
                ctx,
                "copy",
                Ask {
                    cmd: command::WORKFLOW_COPY,
                    args: json!({ "from": state.chosen, "name": state.copy_name.trim() }),
                },
            );
        }
        Msg::Delete => {
            return act(
                ctx,
                "delete",
                Ask {
                    cmd: command::WORKFLOW_DELETE,
                    args: json!({ "id": state.chosen }),
                },
            );
        }
        Msg::Save => {
            let Some(w) = state.draft.clone() else {
                return Task::none();
            };
            return act(
                ctx,
                "save",
                Ask {
                    cmd: command::WORKFLOW_SAVE,
                    args: json!({ "id": w.id, "workflow": w }),
                },
            );
        }
        Msg::Done(what, answer) => return on_done(state, ctx, what, answer),
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
        Msg::Note(a) => {
            state.note.perform(a);
            let note = state.note.text();
            state.step(|n| n.note = note);
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
        m @ (Msg::CopyName(_) | Msg::Copy | Msg::Delete | Msg::Save | Msg::Done(..)) => {
            return manage(state, m, ctx);
        }
        other => change(state, other),
    }
    Task::none()
}

/// The shown workflow's facts: built in or not, its version and its level.
fn facts(w: &WorkflowView) -> String {
    let kind = if w.built_in {
        "built in, copy it to change it"
    } else {
        "yours to change"
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

/// Copy the shown workflow under a new name, and delete one not built in.
fn manage_view(state: &State) -> Element<'_, Message> {
    let name = text_input("Name of the copy", &state.copy_name)
        .on_input(|n| wrap(Msg::CopyName(n)))
        .style(theme::field)
        .width(240);
    let copy = small(
        "Copy",
        (!state.copy_name.trim().is_empty()).then_some(wrap(Msg::Copy)),
    );
    let built_in = state.draft.as_ref().is_none_or(|w| w.built_in);
    let delete = (!built_in).then(|| small("Delete", Some(wrap(Msg::Delete))));
    row![name, copy]
        .extend(delete)
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// Add a skill, a prompt, a command, or a Bastion stage not used yet.
fn palette(state: &State) -> Element<'_, Message> {
    let used: Vec<&str> = state
        .draft
        .iter()
        .flat_map(|w| &w.nodes)
        .map(|n| n.stage.as_str())
        .collect();
    let unused: Vec<Choice> = edit::stages(&state.all)
        .iter()
        .filter(|s| !used.contains(&s.as_str()))
        .map(|s| Choice::new(s, &s.replace('_', " ")))
        .collect();
    let stage = pick_list(unused, None::<Choice>, |c| wrap(Msg::AddStep(c.id)))
        .placeholder("a Bastion stage…")
        .style(theme::select);
    let custom = CUSTOM
        .iter()
        .map(|s| small(s, Some(wrap(Msg::AddStep(s.to_string())))));
    row![text("Add a step").size(13).color(theme::MUTED)]
        .extend(custom)
        .push(stage)
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn step_detail<'a>(state: &'a State, id: &str, editable: bool) -> Option<Element<'a, Message>> {
    let n = state
        .draft
        .as_ref()
        .and_then(|w| edit::node(w, id))?
        .clone();
    let on = |m: Msg| editable.then_some(wrap(m));
    let mut body = column![
        row![
            text(title(&n)).font(theme::BOLD),
            text(n.id.clone()).size(13).color(theme::MUTED)
        ]
        .spacing(8)
    ];
    let run_label = match n.stage.as_str() {
        "skill" => Some("Skill: the folder under .claude/skills"),
        "command" => Some("Command line, run in the repository"),
        _ => None,
    };
    if let Some(label) = run_label {
        let input = text_input("", &n.run)
            .on_input_maybe(editable.then_some(|v| wrap(Msg::Field(StepField::Run, v))))
            .style(theme::field);
        body = body.push(labeled(label, input));
    }
    let dials: Vec<Choice> = DIALS
        .iter()
        .map(|(id, name)| Choice::new(id, name))
        .collect();
    let dial = dials
        .iter()
        .find(|c| c.id == n.dial)
        .cloned()
        .or_else(|| dials.first().cloned());
    let dial = pick_list(dials, dial, |c| wrap(Msg::Dial(c))).style(theme::select);
    let note_label = if n.stage == "prompt" {
        "Instructions"
    } else {
        "Note to the agent"
    };
    let mut note = text_editor(&state.note).height(96);
    if editable {
        note = note.on_action(|a| wrap(Msg::Note(a)));
    }
    let results = text_input("", &state.results)
        .on_input_maybe(editable.then_some(|v| wrap(Msg::Field(StepField::Results, v))))
        .style(theme::field);
    body = body
        .push(labeled("Dial", dial))
        .push(labeled(note_label, note))
        .push(labeled("Results it gives, comma-separated", results))
        .push(small("Remove step", on(Msg::RemoveStep)));
    Some(
        container(body.spacing(10))
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

    fn workflow(id: &str, version: u64, built_in: bool) -> WorkflowView {
        WorkflowView {
            id: id.into(),
            name: id.into(),
            version,
            built_in,
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
    fn only_an_admin_changes_a_workflow_that_is_not_built_in() {
        let mut state = State {
            draft: Some(workflow("mine", 1, false)),
            ..State::default()
        };
        assert!(!state.editable());
        state.admin = true;
        assert!(state.editable());
        state.draft = Some(workflow("short", 1, true));
        assert!(!state.editable());
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
}
