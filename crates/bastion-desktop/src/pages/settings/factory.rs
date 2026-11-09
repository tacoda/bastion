//! The factory's settings (A7, D33): the oversight presets, the model per
//! stage, where a change goes and where requests come from; each step's
//! dial; and every version of the settings with Restore.

use bastion_protocol::{DialsView, ModelOption, PipelineDials, SettingsView, VersionView, command};
use iced::widget::{Column, button, column, pick_list, row, text};
use iced::{Element, Fill, Task};
use serde_json::{Value, json};

use super::{card, small};
use crate::app::Message;
use crate::client::Ask;
use crate::pages::{Choice, Ctx, ago, labeled};
use crate::theme;

const PRESETS: [(&str, &str); 3] = [("lights_on", "lights-on"), ("dim", "dim"), ("dark", "dark")];
const FORGES: [(&str, &str); 2] = [
    ("local", "Merged here, into the workspace's branch"),
    ("github", "Pushed and opened as a GitHub pull request"),
];
const TRACKERS: [(&str, &str); 2] = [
    ("none", "Only what you type"),
    ("github_issues", "Also GitHub issues"),
];

#[derive(Default)]
pub struct State {
    settings: Option<SettingsView>,
    models: Vec<Choice>,
    dials: Option<DialsView>,
    versions: Vec<VersionView>,
    note: String,
}

/// Which setting a pick changes.
#[derive(Clone, Copy, Debug)]
pub enum Pick {
    PlanModel,
    BuildModel,
    Forge,
    Tracker,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Settings(Result<SettingsView, String>),
    Models(Result<Vec<ModelOption>, String>),
    Dials(Result<DialsView, String>),
    Versions(Result<Vec<VersionView>, String>),
    Set(Pick, Choice),
    Preset(&'static str),
    Dial(String, String),
    Restore(u64),
    Done(Result<Value, String>),
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Factory(msg))
}

fn task<T>(ctx: &Ctx, ask: Ask, then: fn(Result<T, String>) -> Msg) -> Task<Message>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    ctx.task(&ctx.current, ask, then).map(wrap)
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    Task::batch([
        task(ctx, Ask::bare(command::SETTINGS), Msg::Settings),
        task(ctx, Ask::bare(command::MODELS), Msg::Models),
        task(ctx, Ask::bare(command::DIALS), Msg::Dials),
        task(ctx, Ask::bare(command::SETTINGS_HISTORY), Msg::Versions),
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
        Pick::Forge => s.forge = choice.into(),
        Pick::Tracker => s.tracker = choice.into(),
    }
    s
}

fn keep<T>(slot: &mut T, note: &mut String, answer: Result<T, String>) {
    match answer {
        Ok(v) => *slot = v,
        Err(e) => *note = e,
    }
}

fn send(ctx: &Ctx, cmd: &'static str, args: Value) -> Task<Message> {
    task(ctx, Ask { cmd, args }, Msg::Done)
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    let note = &mut state.note;
    match msg {
        Msg::Settings(a) => keep(&mut state.settings, note, a.map(Some)),
        Msg::Models(a) => keep(&mut state.models, note, a.map(model_choices)),
        Msg::Dials(a) => keep(&mut state.dials, note, a.map(Some)),
        Msg::Versions(a) => keep(&mut state.versions, note, a),
        Msg::Set(pick, choice) => {
            let Some(s) = state.settings.clone() else {
                return Task::none();
            };
            return send(
                ctx,
                command::SETTINGS_SET,
                json!({ "settings": apply(s, pick, &choice.id) }),
            );
        }
        Msg::Preset(p) => return send(ctx, command::SETTINGS_PRESET, json!({ "preset": p })),
        Msg::Dial(stage, dial) => {
            return send(
                ctx,
                command::DIALS_SET,
                json!({ "stage": stage, "dial": dial }),
            );
        }
        Msg::Restore(version) => {
            return send(
                ctx,
                command::SETTINGS_RESTORE,
                json!({ "version": version }),
            );
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
    let presets = PRESETS
        .iter()
        .map(|(id, label)| small(label, Some(wrap(Msg::Preset(id)))));
    let mut body = column![
        row![text("Oversight:").size(13).color(theme::MUTED)]
            .extend(presets)
            .spacing(8)
            .align_y(iced::Center)
    ];
    if let Some(s) = &state.settings {
        let models = &state.models;
        body = body
            .push(
                row![
                    picker(
                        "Plan model",
                        models.clone(),
                        format!("{}/{}", s.plan_provider, s.plan_model),
                        Pick::PlanModel
                    ),
                    picker(
                        "Build model",
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
                        "Where a reviewed change goes",
                        choices(&FORGES),
                        s.forge.clone(),
                        Pick::Forge
                    ),
                    picker(
                        "Where requests can come from",
                        choices(&TRACKERS),
                        s.tracker.clone(),
                        Pick::Tracker
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

fn pipeline<'a>(title: &str, p: &PipelineDials) -> Element<'a, Message> {
    let steps = p.steps.iter().map(|s| {
        let on = s.dial == "lights_on";
        let label = format!(
            "{} · {}",
            s.stage.replace('_', " "),
            if on { "lights-on" } else { "dark" }
        );
        let flip = wrap(Msg::Dial(
            s.stage.clone(),
            if on { "dark" } else { "lights_on" }.into(),
        ));
        let look = if on { theme::secondary } else { theme::quiet };
        button(text(label).size(13))
            .padding([4, 10])
            .style(look)
            .on_press(flip)
            .into()
    });
    column![
        text(format!("{title}: {}", p.level.replace('_', "-"))).size(13),
        row(steps).spacing(8).wrap()
    ]
    .spacing(8)
    .into()
}

fn oversight(state: &State) -> Element<'_, Message> {
    let lede = "A lights-on step waits for you to approve its result or send it back. A dark step goes on at once. A pipeline is lights-on when every step is, dim when a step up to the merge is, and dark when none is.";
    let mut body = column![text(lede).size(13).color(theme::MUTED)];
    if let Some(d) = &state.dials {
        body = body
            .push(pipeline("Short pipeline", &d.short))
            .push(pipeline("Full pipeline", &d.full));
    }
    card("Oversight", body)
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

pub fn view(state: &State) -> [Element<'_, Message>; 3] {
    [factory(state), oversight(state), history(state)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_sets_one_setting_and_keeps_the_rest() {
        let s = SettingsView {
            plan_provider: "openai".into(),
            plan_model: "gpt-5-mini".into(),
            forge: "github".into(),
            ..SettingsView::default()
        };
        let picked = apply(s.clone(), Pick::PlanModel, "localfake/echo-1");
        assert_eq!(
            (
                picked.plan_provider.as_str(),
                picked.plan_model.as_str(),
                picked.forge.as_str()
            ),
            ("localfake", "echo-1", "github")
        );
        assert_eq!(
            apply(s, Pick::Tracker, "github_issues").tracker,
            "github_issues"
        );
    }
}
