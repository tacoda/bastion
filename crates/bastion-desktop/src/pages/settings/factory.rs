//! The factory's settings (D33): the models agents run on, where requests
//! come from and whether intake is open; and every version of the settings
//! with Restore. A step's dial is its workflow's (D39).

use bastion_protocol::api::{self, Request};
use bastion_protocol::{ModelOption, SettingsView, VersionView};
use iced::widget::{Column, pick_list, row, text};
use iced::{Element, Fill, Task};

use super::{card, small};
use crate::app::Message;
use crate::pages::{Choice, Ctx, ago, labeled};
use crate::theme;

const INTAKE: [(&str, &str); 2] = [
    ("open", "Open: queued requests start as room frees"),
    ("paused", "Paused: requests queue, and nothing new starts"),
];
const TRACKERS: [(&str, &str); 2] = [
    ("none", "Only what you type"),
    ("github_issues", "Also GitHub issues"),
];

#[derive(Default)]
pub struct State {
    settings: Option<SettingsView>,
    models: Vec<Choice>,
    versions: Vec<VersionView>,
    note: String,
}

/// Which setting a pick changes.
#[derive(Clone, Copy, Debug)]
pub enum Pick {
    PlanModel,
    BuildModel,
    Tracker,
    Intake,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Settings(Result<SettingsView, String>),
    Models(Result<Vec<ModelOption>, String>),
    Versions(Result<Vec<VersionView>, String>),
    Set(Pick, Choice),
    Restore(u64),
    Done(Result<(), String>),
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Factory(msg))
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
        task(ctx, api::Settings, Msg::Settings),
        task(ctx, api::Models, Msg::Models),
        task(ctx, api::SettingsHistory, Msg::Versions),
    ])
}

/// Each model as a choice: `provider/id`, labeled with its name.
fn model_choices(models: Vec<ModelOption>) -> Vec<Choice> {
    models
        .into_iter()
        .map(|m| {
            Choice::new(
                &format!("{}/{}", m.provider, m.id),
                &format!("{} ({})", m.name, m.provider),
            )
        })
        .collect()
}

/// The settings with one pick applied.
fn apply(mut s: SettingsView, pick: Pick, choice: &str) -> SettingsView {
    let model = || {
        let (p, m) = choice.split_once('/').unwrap_or_default();
        (p.to_string(), m.to_string())
    };
    match pick {
        Pick::PlanModel => (s.plan_provider, s.plan_model) = model(),
        Pick::BuildModel => (s.build_provider, s.build_model) = model(),
        Pick::Tracker => s.tracker = choice.into(),
        Pick::Intake => s.intake_paused = choice == "paused",
    }
    s
}

fn keep<T>(slot: &mut T, note: &mut String, answer: Result<T, String>) {
    match answer {
        Ok(v) => *slot = v,
        Err(e) => *note = e,
    }
}

/// Send a change, and read again when it lands.
fn send<R>(ctx: &Ctx, request: R) -> Task<Message>
where
    R: Request<Answer = ()> + Send + Sync + 'static,
{
    task(ctx, request, Msg::Done)
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    let note = &mut state.note;
    match msg {
        Msg::Settings(a) => keep(&mut state.settings, note, a.map(Some)),
        Msg::Models(a) => keep(&mut state.models, note, a.map(model_choices)),
        Msg::Versions(a) => keep(&mut state.versions, note, a),
        Msg::Set(pick, choice) => {
            let Some(s) = state.settings.clone() else {
                return Task::none();
            };
            return send(ctx, api::SettingsSet(apply(s, pick, &choice.id)));
        }
        Msg::Restore(version) => {
            return send(ctx, api::SettingsRestore { version });
        }
        Msg::Done(Ok(_)) => {
            state.note = "Saved as a new version.".into();
            return load(ctx);
        }
        Msg::Done(Err(e)) => state.note = e,
    }
    Task::none()
}

fn choices(all: &[(&str, &str)]) -> Vec<Choice> {
    all.iter().map(|(id, name)| Choice::new(id, name)).collect()
}

fn picker<'a>(
    label: &'a str,
    options: Vec<Choice>,
    current: String,
    pick: Pick,
) -> Element<'a, Message> {
    let shown = options.iter().find(|c| c.id == current).cloned();
    let list = pick_list(options, shown, move |c| wrap(Msg::Set(pick, c)))
        .style(theme::select)
        .width(Fill);
    labeled(label, list)
}

fn factory(state: &State) -> Element<'_, Message> {
    let mut body = Column::new().spacing(12);
    if let Some(s) = &state.settings {
        let models = &state.models;
        body = body
            .push(
                row![
                    picker(
                        "Model for steps that only read",
                        models.clone(),
                        format!("{}/{}", s.plan_provider, s.plan_model),
                        Pick::PlanModel
                    ),
                    picker(
                        "Model for every other step",
                        models.clone(),
                        format!("{}/{}", s.build_provider, s.build_model),
                        Pick::BuildModel
                    ),
                ]
                .spacing(16),
            )
            .push(
                row![
                    picker(
                        "Where requests can come from",
                        choices(&TRACKERS),
                        s.tracker.clone(),
                        Pick::Tracker
                    ),
                    picker(
                        "Intake",
                        choices(&INTAKE),
                        INTAKE[usize::from(s.intake_paused)].0.into(),
                        Pick::Intake,
                    ),
                ]
                .spacing(16),
            );
    }
    card(
        "Factory",
        body.push(text(&state.note).size(13).color(theme::MUTED)),
    )
}

fn history(state: &State) -> Element<'_, Message> {
    let newest = state.versions.first().map(|v| v.version);
    let rows = state.versions.iter().map(|v| {
        let restore = (newest != Some(v.version))
            .then(|| small("Restore", Some(wrap(Msg::Restore(v.version)))));
        row![
            text(format!("v{} · {}", v.version, ago(v.at)))
                .size(13)
                .color(theme::MUTED)
                .width(140),
            text(v.note.clone()).size(13).width(Fill)
        ]
        .extend(restore)
        .spacing(8)
        .align_y(iced::Center)
        .into()
    });
    card("History", Column::with_children(rows))
}

pub fn view(state: &State) -> [Element<'_, Message>; 2] {
    [factory(state), history(state)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_sets_one_setting_and_keeps_the_rest() {
        let s = SettingsView {
            plan_provider: "openai".into(),
            plan_model: "gpt-5-mini".into(),
            tracker: "github_issues".into(),
            ..SettingsView::default()
        };
        let picked = apply(s.clone(), Pick::PlanModel, "localfake/echo-1");
        assert_eq!(
            (
                picked.plan_provider.as_str(),
                picked.plan_model.as_str(),
                picked.tracker.as_str()
            ),
            ("localfake", "echo-1", "github_issues")
        );
        assert_eq!(apply(s.clone(), Pick::Tracker, "none").tracker, "none");
        assert!(apply(s, Pick::Intake, "paused").intake_paused);
    }
}
