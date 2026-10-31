//! The Settings page: the active profile (A1) and the workspaces (A3). A7
//! adds the rest.

use bastion_protocol::{ActiveProfile, DialsView, Workspace, command, event};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc::{self, None};
use crate::pages::factory_settings::{Accounts, FactorySettings, SettingsHistory};

#[derive(Serialize)]
struct AddWorkspace {
    workspace: Workspace,
}

#[derive(Serialize)]
struct SetDials {
    dials: DialsView,
}

/// How each checkpoint is decided (PLAN.md D15).
#[component]
fn Checkpoints() -> impl IntoView {
    let (dials, set_dials) = signal(None::<DialsView>);
    let (note, set_note) = signal(String::new());
    spawn_local(async move {
        match ipc::call::<DialsView, String>(command::DIALS, None {}).await {
            Ok(d) => set_dials.set(Some(d)),
            Err(e) => set_note.set(e.text()),
        }
    });
    let save = move |change: DialsView| {
        spawn_local(async move {
            match ipc::call::<(), String>(
                command::DIALS_SET,
                SetDials {
                    dials: change.clone(),
                },
            )
            .await
            {
                Ok(()) => {
                    set_note
                        .set("Saved. It applies the next time a job reaches a checkpoint.".into());
                    set_dials.set(Some(change));
                }
                Err(e) => set_note.set(e.text()),
            }
        });
    };
    let choice = move |label: &'static str, is_review: bool| {
        view! {
            <label class="grid gap-1 text-sm text-muted">{label}
                <select class="rounded border border-line bg-bg px-2 py-1 text-ink"
                    on:change=move |e| {
                        if let Some(mut d) = dials.get_untracked() {
                            let value = event_target_value(&e);
                            if is_review { d.review = value } else { d.approve = value }
                            save(d);
                        }
                    }>
                    {["ask", "hold", "auto"].map(|option| {
                        let selected = move || dials.get().is_some_and(|d| (if is_review { d.review } else { d.approve }) == option);
                        view! { <option value=option selected=selected>{option}</option> }
                    })}
                </select>
            </label>
        }
    };
    view! {
        <section class="mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Checkpoints"</h2>
            <p class="m-0 text-sm text-muted">
                "Ask waits for you. Hold approves by itself after 30 minutes unless you act. Auto approves at once. Review is the merge, so it never goes below hold."
            </p>
            <div class="flex flex-wrap gap-4">
                {choice("Approve the plan", false)}
                {choice("Review the change", true)}
            </div>
            <p class="m-0 text-sm text-muted" role="status">{note}</p>
        </section>
    }
}

#[component]
fn Workspaces() -> impl IntoView {
    let (workspaces, set_workspaces) = signal(Vec::<Workspace>::new());
    let (note, set_note) = signal(String::new());
    let (name, set_name) = signal(String::new());
    let (path, set_path) = signal(String::new());
    let (branch, set_branch) = signal(String::from("main"));
    let load = move || {
        spawn_local(async move {
            match ipc::call::<Vec<Workspace>, String>(command::WORKSPACES, None {}).await {
                Ok(all) => {
                    set_note.set(String::new());
                    set_workspaces.set(all);
                }
                Err(e) => set_note.set(e.text()),
            }
        });
    };
    ipc::listen(event::JOB_CHANGED, move |id: String| {
        if id.is_empty() {
            load();
        }
    });
    load();
    let add = move |_| {
        let workspace = Workspace {
            name: name.get(),
            path: path.get(),
            branch: branch.get(),
        };
        spawn_local(async move {
            match ipc::call::<(), String>(command::WORKSPACE_ADD, AddWorkspace { workspace }).await
            {
                Ok(()) => {
                    set_name.set(String::new());
                    set_path.set(String::new());
                    load();
                }
                Err(e) => set_note.set(e.text()),
            }
        });
    };
    let input = "rounded border border-line bg-bg px-2 py-1 text-ink";
    view! {
        <section class="mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Workspaces"</h2>
            <p class="m-0 text-sm text-muted">"The repositories jobs run against. Each needs its branch."</p>
            <ul class="m-0 grid list-none gap-1 p-0 text-sm">
                {move || workspaces.get().into_iter().map(|w| view! {
                    <li>{w.name}<span class="text-muted">{format!("  {} @ {}", w.path, w.branch)}</span></li>
                }).collect_view()}
            </ul>
            <div class="flex flex-wrap items-end gap-2 text-sm">
                <label class="grid gap-1 text-muted">"Name"<input class=input prop:value=name on:input=move |e| set_name.set(event_target_value(&e)) /></label>
                <label class="grid gap-1 text-muted">"Repository path"<input class=input prop:value=path on:input=move |e| set_path.set(event_target_value(&e)) /></label>
                <label class="grid gap-1 text-muted">"Branch"<input class=input prop:value=branch on:input=move |e| set_branch.set(event_target_value(&e)) /></label>
                <button class="rounded-lg border border-line px-3 py-1.5 hover:bg-line" on:click=add>"Add"</button>
            </div>
            <p class="m-0 text-bad" role="alert">{note}</p>
        </section>
    }
}

#[component]
pub fn SettingsPage() -> impl IntoView {
    let (profile, set_profile) = signal(None::<ActiveProfile>);
    spawn_local(async move {
        if let Ok(p) = ipc::call::<ActiveProfile, String>(command::PROFILE, None {}).await {
            set_profile.set(Some(p));
        }
    });
    let row = |label: &'static str, value: String| {
        view! { <dt class="text-muted">{label}</dt><dd class="m-0 break-all">{value}</dd> }
    };
    view! {
        <h1 class="mt-0 text-2xl font-bold">"Settings"</h1>
        <section class="grid gap-2 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Profile"</h2>
            {move || profile.get().map(|p| view! {
                <dl class="m-0 grid grid-cols-[120px_1fr] gap-x-3 gap-y-1">
                    {row("Name", p.name)}
                    {row("Kind", p.kind)}
                    {row("Engine", p.engine_url)}
                    {row("Checkout", p.checkout)}
                    {row("File", p.file)}
                </dl>
            })}
        </section>
        <FactorySettings />
        <Checkpoints />
        <SettingsHistory />
        <Workspaces />
        <Accounts />
    }
}
