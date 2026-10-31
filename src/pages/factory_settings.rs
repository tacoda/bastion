//! The factory's settings on the Settings page (PLAN.md A7): an oversight
//! preset, the model each agent stage runs, the forge and tracker, every
//! version with Restore, and the owner's accounts.

use bastion_protocol::{AccountView, ModelOption, SettingsView, VersionView, command, event};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc::{self, None};
use crate::route::ago;

#[derive(Serialize)]
struct SetSettings {
    settings: SettingsView,
}

#[derive(Serialize)]
struct Preset {
    preset: String,
}

#[derive(Serialize)]
struct Restore {
    version: u64,
}

const INPUT: &str = "rounded border border-line bg-bg px-2 py-1 text-sm text-ink";
const BUTTON: &str = "rounded-lg border border-line px-3 py-1 text-sm hover:bg-line";
const SECTION: &str = "mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4";

/// Read `cmd` now and on every push.
fn read<T: serde::de::DeserializeOwned + Send + Sync + 'static>(
    cmd: &'static str,
) -> (ReadSignal<Option<T>>, Callback<()>) {
    let (value, set_value) = signal(None::<T>);
    let reload = Callback::new(move |()| {
        spawn_local(async move {
            if let Ok(v) = ipc::call::<T, String>(cmd, None {}).await {
                set_value.set(Some(v));
            }
        });
    });
    ipc::listen(event::JOB_CHANGED, move |_: String| reload.run(()));
    reload.run(());
    (value, reload)
}

/// Send a change, then read everything again or say why it failed.
fn send<A: Serialize + 'static>(
    cmd: &'static str,
    args: A,
    note: WriteSignal<String>,
    reload: Callback<()>,
) {
    spawn_local(async move {
        match ipc::call::<(), String>(cmd, args).await {
            Ok(()) => {
                note.set("Saved as a new version.".into());
                reload.run(());
            }
            Err(e) => note.set(e.text()),
        }
    });
}

/// A select over `options` as (value, label), showing `current`, that calls
/// `pick` with the chosen value.
fn choose(
    options: impl Fn() -> Vec<(String, String)> + Send + Sync + 'static,
    current: impl Fn() -> String + Send + Sync + Copy + 'static,
    pick: impl Fn(String) + 'static,
) -> impl IntoView {
    view! {
        <select class=INPUT on:change=move |e| pick(event_target_value(&e))>
            {move || options().into_iter().map(|(v, label)| {
                let here = v.clone();
                view! { <option value=v selected=move || current() == here>{label}</option> }
            }).collect_view()}
        </select>
    }
}

fn words(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(v, l)| (v.to_string(), l.to_string()))
        .collect()
}

/// Each model as a choice: `provider/id`, labeled with its name.
fn model_choices(models: Vec<ModelOption>) -> Vec<(String, String)> {
    models
        .into_iter()
        .map(|m| {
            (
                format!("{}/{}", m.provider, m.id),
                format!("{} ({})", m.name, m.provider),
            )
        })
        .collect()
}

/// Set a stage's model from a `provider/id` choice.
fn set_model(s: &mut SettingsView, plan: bool, choice: &str) {
    let (provider, model) = choice.split_once('/').unwrap_or_default();
    let (p, m) = if plan {
        (&mut s.plan_provider, &mut s.plan_model)
    } else {
        (&mut s.build_provider, &mut s.build_model)
    };
    (*p, *m) = (provider.to_string(), model.to_string());
}

/// The oversight presets, each one button that sets both dials.
fn presets(preset: impl Fn(&'static str) + Copy + 'static) -> impl IntoView {
    let all = [
        ("high", "High: you approve plans and changes"),
        ("medium", "Medium: both hold"),
        ("low", "Low: plans auto, changes hold"),
        ("off", "Off"),
    ];
    view! {
        <div class="flex flex-wrap items-center gap-2 text-sm">
            <span class="text-muted">"Oversight:"</span>
            {all.map(|(p, title)| view! { <button class=BUTTON title=title on:click=move |_| preset(p)>{p}</button> })}
        </div>
    }
}

#[component]
pub fn FactorySettings() -> impl IntoView {
    let (settings, reload) = read::<SettingsView>(command::SETTINGS);
    let (models, _) = read::<Vec<ModelOption>>(command::MODELS);
    let (note, set_note) = signal(String::new());
    let save = move |change: Box<dyn Fn(&mut SettingsView)>| {
        if let Some(mut s) = settings.get_untracked() {
            change(&mut s);
            send(
                command::SETTINGS_SET,
                SetSettings { settings: s },
                set_note,
                reload,
            );
        }
    };
    let preset = move |p: &'static str| {
        send(
            command::SETTINGS_PRESET,
            Preset { preset: p.into() },
            set_note,
            reload,
        )
    };
    let model_options = move || model_choices(models.get().unwrap_or_default());
    let current = move |f: fn(&SettingsView) -> String| {
        move || settings.get().map(|s| f(&s)).unwrap_or_default()
    };
    let set_model =
        move |plan: bool| move |v: String| save(Box::new(move |s| set_model(s, plan, &v)));
    let label = "grid gap-1 text-xs text-muted";
    view! {
        <section class=SECTION>
            <h2 class="m-0 text-sm font-semibold text-muted">"Factory"</h2>
            {presets(preset)}
            <div class="grid gap-3 sm:grid-cols-2">
                <label class=label>"Plan model"
                    {choose(model_options, current(|s| format!("{}/{}", s.plan_provider, s.plan_model)), set_model(true))}
                </label>
                <label class=label>"Build model"
                    {choose(model_options, current(|s| format!("{}/{}", s.build_provider, s.build_model)), set_model(false))}
                </label>
                <label class=label>"Where a reviewed change goes"
                    {choose(|| words(&[("local", "Merged here, into the workspace's branch"), ("github", "Pushed and opened as a GitHub pull request")]),
                        current(|s| s.forge.clone()), move |v| save(Box::new(move |s| s.forge = v.clone())))}
                </label>
                <label class=label>"Where requests can come from"
                    {choose(|| words(&[("none", "Only what you type"), ("github_issues", "Also GitHub issues")]),
                        current(|s| s.tracker.clone()), move |v| save(Box::new(move |s| s.tracker = v.clone())))}
                </label>
            </div>
            <p class="m-0 text-sm text-muted" role="status">{note}</p>
        </section>
    }
}

/// Every version of the settings, newest first, each with Restore.
#[component]
pub fn SettingsHistory() -> impl IntoView {
    let (versions, reload) = read::<Vec<VersionView>>(command::SETTINGS_HISTORY);
    let (note, set_note) = signal(String::new());
    view! {
        <section class=SECTION>
            <h2 class="m-0 text-sm font-semibold text-muted">"History"</h2>
            <ol class="m-0 grid list-none gap-1 p-0 text-sm">
                {move || {
                    let all = versions.get().unwrap_or_default();
                    let newest = all.first().map(|v| v.version);
                    all.into_iter().map(|v| {
                        let version = v.version;
                        view! {
                            <li class="flex flex-wrap items-center gap-2">
                                <span class="text-muted">{format!("v{} · {}", v.version, ago(v.at))}</span>
                                <span class="flex-1 break-all">{v.note}</span>
                                {(newest != Some(version)).then(|| view! {
                                    <button class=BUTTON on:click=move |_| send(command::SETTINGS_RESTORE, Restore { version }, set_note, reload)>"Restore"</button>
                                })}
                            </li>
                        }
                    }).collect_view()
                }}
            </ol>
            <p class="m-0 text-sm text-muted" role="status">{note}</p>
        </section>
    }
}

/// The owner's accounts with other services, and whether each answers.
#[component]
pub fn Accounts() -> impl IntoView {
    let (accounts, _) = read::<Vec<AccountView>>(command::ACCOUNTS);
    view! {
        <section class=SECTION>
            <h2 class="m-0 text-sm font-semibold text-muted">"Accounts"</h2>
            <ul class="m-0 grid list-none gap-1 p-0 text-sm">
                {move || accounts.get().unwrap_or_default().into_iter().map(|a| view! {
                    <li><span class={if a.ok { "text-ok" } else { "text-warn" }}>{if a.ok { "✓ " } else { "! " }}</span>
                        <strong>{a.name}</strong><span class="text-muted">{format!(" · {}", a.status)}</span></li>
                }).collect_view()}
            </ul>
        </section>
    }
}
