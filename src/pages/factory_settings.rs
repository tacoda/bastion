//! The factory's settings on the Settings page (PLAN.md A7): an oversight
//! preset, the model each agent stage runs, the forge and tracker, every
//! version with Restore, the model providers' keys, and the owner's accounts.

use bastion_protocol::{
    AccountView, ModelOption, ProviderView, SettingsView, SignIn, VersionView, command, event,
};
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
pub(crate) const BUTTON: &str = "rounded-lg border border-line px-3 py-1 text-sm hover:bg-line";
const SECTION: &str = "mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4";

/// Read `cmd` now and on every push.
pub(crate) fn read<T: serde::de::DeserializeOwned + Send + Sync + 'static>(
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
pub(crate) fn send<A: Serialize + 'static>(
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

/// The oversight presets, each one button that sets every step's dial (D33).
fn presets(preset: impl Fn(&'static str) + Copy + 'static) -> impl IntoView {
    let all = [
        ("lights_on", "Lights-on: every step waits for you"),
        ("dim", "Dim: only ship, land, merge and deploy wait for you"),
        ("dark", "Dark: no step waits for you"),
    ];
    view! {
        <div class="flex flex-wrap items-center gap-2 text-sm">
            <span class="text-muted">"Oversight:"</span>
            {all.map(|(p, title)| view! { <button class=BUTTON title=title on:click=move |_| preset(p)>{p.replace('_', "-")}</button> })}
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
            <h2 class="m-0 text-sm font-semibold text-muted">"Orchestrator"</h2>
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

#[derive(Serialize)]
struct KeySet {
    id: String,
    key: String,
}

#[derive(Serialize)]
struct ProviderId {
    id: String,
}

/// The model providers and where each one's key comes from (D34). A key is
/// sent once, to the stack's vault, and never shown again.
#[component]
pub fn ModelProviders() -> impl IntoView {
    let (providers, reload) = read::<Vec<ProviderView>>(command::PROVIDERS);
    let (note, set_note) = signal(String::new());
    view! {
        <section class=SECTION>
            <h2 class="m-0 text-sm font-semibold text-muted">"Model providers"</h2>
            <ul class="m-0 grid list-none gap-2 p-0 text-sm">
                {move || providers.get().unwrap_or_default().into_iter().map(|p| provider_row(p, set_note, reload)).collect_view()}
            </ul>
            <p class="m-0 text-bad" role="alert">{note}</p>
        </section>
    }
}

/// One provider and how it gets its credential: a sign-in for a
/// subscription, else a key.
fn provider_row(p: ProviderView, note: WriteSignal<String>, reload: Callback<()>) -> AnyView {
    match p.id.as_str() {
        "openai-codex" => codex_row(p, note, reload).into_any(),
        _ => key_row(p, note, reload).into_any(),
    }
}

/// A provider's name with a mark for whether it has a credential.
fn provider_name(p: &ProviderView) -> impl IntoView + use<> {
    let ok = p.source != "none";
    view! {
        <span class={if ok { "text-ok" } else { "text-warn" }}>{if ok { "✓" } else { "!" }}</span>
        <strong>{p.name.clone()}</strong>
    }
}

/// OpenAI Codex signs in with a ChatGPT plan: a link and a code, then the
/// page polls until the sign-in ends.
fn codex_row(p: ProviderView, note: WriteSignal<String>, reload: Callback<()>) -> impl IntoView {
    let (pending, set_pending) = signal(None::<SignIn>);
    let sign_in = move |_| {
        spawn_local(async move {
            match ipc::call::<SignIn, String>(command::CODEX_SIGN_IN, None {}).await {
                Ok(started) => {
                    set_pending.set(Some(started.clone()));
                    let ended = poll_codex(started).await;
                    set_pending.set(Option::None);
                    note.set(ended);
                    reload.run(());
                }
                Err(e) => note.set(e.text()),
            }
        });
    };
    let sign_out = move |_| send(command::CODEX_SIGN_OUT, None {}, note, reload);
    let signed_in = p.source == "signed_in";
    view! {
        <li class="grid gap-1">
            <div class="flex flex-wrap items-center gap-2">
                {provider_name(&p)}
                <span class="text-muted">{if signed_in { "signed in with ChatGPT" } else { "uses a ChatGPT plan, no key" }}</span>
                {if signed_in {
                    view! { <button class="text-bad hover:underline" on:click=sign_out>"Sign out"</button> }.into_any()
                } else {
                    view! { <button class="rounded-lg border border-line px-3 py-1 hover:bg-line" on:click=sign_in>"Sign in with ChatGPT"</button> }.into_any()
                }}
            </div>
            {move || pending.get().map(|s| view! {
                <p class="m-0" role="status">
                    "Open "<a class="text-link hover:underline" href=s.verification_uri.clone() target="_blank" rel="noopener noreferrer">{s.verification_uri.clone()}</a>
                    " and enter "<code>{s.user_code}</code>". Waiting for approval..."
                </p>
            })}
        </li>
    }
}

/// Poll a Codex sign-in until it ends, and say how it ended.
async fn poll_codex(started: SignIn) -> String {
    #[derive(Serialize)]
    struct Poll {
        login_id: String,
    }
    loop {
        crate::ipc::sleep(started.interval.max(1) as i32 * 1000).await;
        let args = Poll {
            login_id: started.login_id.clone(),
        };
        match ipc::call::<String, String>(command::CODEX_SIGN_IN_POLL, args).await {
            Ok(s) if s == "pending" => continue,
            Ok(s) if s == "ok" => return String::new(),
            Ok(s) => return format!("The ChatGPT sign-in ended: {s}. Try again."),
            Err(e) => return e.text(),
        }
    }
}

/// A provider that takes a key: where it comes from, a field to set one,
/// and Remove for a key set here.
fn key_row(p: ProviderView, note: WriteSignal<String>, reload: Callback<()>) -> impl IntoView {
    let (key, set_key) = signal(String::new());
    let id = p.id.clone();
    let save = move |_| {
        let args = KeySet {
            id: id.clone(),
            key: key.get(),
        };
        send(command::PROVIDER_KEY_SET, args, note, reload);
        set_key.set(String::new());
    };
    let status = match p.source.as_str() {
        "secret" => "key set here",
        "env" => "key from the stack's environment",
        _ => "no key",
    };
    let remove = (p.source == "secret").then(|| {
        let id = p.id.clone();
        let remove = move |_| {
            send(
                command::PROVIDER_KEY_REMOVE,
                ProviderId { id: id.clone() },
                note,
                reload,
            )
        };
        view! { <button class="text-bad hover:underline" on:click=remove>"Remove"</button> }
    });
    view! {
        <li class="flex flex-wrap items-center gap-2">
            {provider_name(&p)}
            <span class="text-muted">{status}</span>
            <input type="password" autocomplete="off" aria-label="API key" placeholder="API key" class=INPUT
                prop:value=key on:input=move |e| set_key.set(event_target_value(&e)) />
            <button class="rounded-lg border border-line px-3 py-1 hover:bg-line" on:click=save>"Set"</button>
            {remove}
        </li>
    }
}
