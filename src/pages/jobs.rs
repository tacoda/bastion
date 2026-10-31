//! The Jobs page and New request (PLAN.md A3): every job, the ones waiting on
//! the owner first, after wipp's Jobs page and GitHub Actions' runs list.

use bastion_protocol::{JobSummary, Workspace, command, event};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc::{self, None};
use crate::route::{Route, ago, mark, nav};

const IN_PROGRESS: [&str; 3] = ["queued", "provisioning", "running"];
const FINISHED: [&str; 3] = ["landed", "failed", "stopped"];

#[derive(Serialize)]
struct Submit {
    workspace: String,
    request: String,
}

/// Every job, kept current by `job-changed`. Only the latest read lands.
fn jobs_signal() -> (ReadSignal<Vec<JobSummary>>, ReadSignal<String>) {
    let (jobs, set_jobs) = signal(Vec::<JobSummary>::new());
    let (error, set_error) = signal(String::new());
    let latest = StoredValue::new(0u64);
    let reload = move || {
        let n = latest.get_value() + 1;
        latest.set_value(n);
        spawn_local(async move {
            let answer = ipc::call::<Vec<JobSummary>, String>(command::JOBS, None {}).await;
            if latest.get_value() != n {
                return;
            }
            match answer {
                Ok(all) => {
                    set_error.set(String::new());
                    set_jobs.set(all);
                }
                Err(e) => set_error.set(e.text()),
            }
        });
    };
    ipc::listen(event::JOB_CHANGED, move |_: String| reload());
    reload();
    (jobs, error)
}

pub fn workspaces_signal() -> ReadSignal<Vec<Workspace>> {
    let (workspaces, set_workspaces) = signal(Vec::<Workspace>::new());
    let load = move || {
        spawn_local(async move {
            if let Ok(all) = ipc::call::<Vec<Workspace>, String>(command::WORKSPACES, None {}).await
            {
                set_workspaces.set(all);
            }
        });
    };
    ipc::listen(event::JOB_CHANGED, move |id: String| {
        if id.is_empty() {
            load();
        }
    });
    load();
    workspaces
}

fn row(job: JobSummary) -> impl IntoView {
    let go = nav();
    let (symbol, color) = mark(&job.state);
    let id = job.id.clone();
    view! {
        <li class="border-t border-line first:border-t-0">
            <button class="grid w-full grid-cols-[24px_1fr_auto] items-start gap-x-3 px-3 py-2.5 text-left hover:bg-line"
                on:click=move |_| go.set(Route::Job(id.clone()))>
                <span class={format!("text-lg leading-6 {color}")} aria-hidden="true">{symbol}</span>
                <span class="grid min-w-0 gap-0.5">
                    <span class="truncate font-semibold">{job.request.clone()}</span>
                    <span class="text-xs text-muted">
                        {format!("{} · {} · {} · {}", job.workspace, job.id, job.state.replace('_', " "), ago(job.created_at))}
                    </span>
                </span>
                <span class="rounded-full border border-line px-2 py-0.5 text-xs capitalize text-muted">{job.stage.clone()}</span>
            </button>
        </li>
    }
}

fn group(title: String, jobs: Vec<JobSummary>, accent: bool) -> impl IntoView {
    let border = if accent { "border-warn" } else { "border-line" };
    view! {
        <section class={format!("mb-4 rounded-xl border {border} bg-panel")}>
            <h2 class="m-0 border-b border-line px-3 py-2 text-sm font-semibold text-muted">{title}</h2>
            {if jobs.is_empty() {
                view! { <p class="m-0 px-3 py-2.5 text-sm text-muted">"Nothing here."</p> }.into_any()
            } else {
                view! { <ul class="m-0 list-none p-0">{jobs.into_iter().map(row).collect_view()}</ul> }.into_any()
            }}
        </section>
    }
}

#[component]
pub fn JobsPage() -> impl IntoView {
    let go = nav();
    let (jobs, error) = jobs_signal();
    let workspaces = workspaces_signal();
    let (workspace, set_workspace) = signal(String::new());
    let (text, set_text) = signal(String::new());
    let (show_finished, set_show_finished) = signal(false);

    let shown = move || {
        let (ws, words) = (workspace.get(), text.get().to_lowercase());
        jobs.get()
            .into_iter()
            .filter(|j| ws.is_empty() || j.workspace == ws)
            .filter(|j| {
                words.is_empty()
                    || j.request.to_lowercase().contains(&words)
                    || j.id.contains(&words)
            })
            .collect::<Vec<_>>()
    };
    let input = "rounded border border-line bg-bg px-2 py-1 text-ink";

    view! {
        <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
            <h1 class="m-0 text-2xl font-bold">"Jobs"</h1>
            <button class="rounded-lg bg-primary px-3 py-2 font-semibold text-white hover:bg-primary-hover"
                on:click=move |_| go.set(Route::NewRequest)>"New request"</button>
        </div>
        <p class="text-bad" role="alert">{error}</p>
        <div class="mb-4 flex flex-wrap gap-3 text-sm" role="search">
            <label class="grid gap-1 text-muted">"Workspace"
                <select class=input on:change=move |e| set_workspace.set(event_target_value(&e))>
                    <option value="">"Every workspace"</option>
                    {move || workspaces.get().into_iter().map(|w| view! { <option value=w.name.clone()>{w.name.clone()}</option> }).collect_view()}
                </select>
            </label>
            <label class="grid gap-1 text-muted">"Search"
                <input class=input type="search" placeholder="Request or id" on:input=move |e| set_text.set(event_target_value(&e)) />
            </label>
        </div>
        {move || {
            let all = shown();
            let waiting: Vec<_> = all.iter().filter(|j| j.state == "needs_you").cloned().collect();
            let active: Vec<_> = all.iter().filter(|j| IN_PROGRESS.contains(&j.state.as_str())).cloned().collect();
            let done: Vec<_> = all.iter().filter(|j| FINISHED.contains(&j.state.as_str())).cloned().collect();
            let done_count = done.len();
            view! {
                {(!waiting.is_empty()).then(|| group(format!("Waiting for you ({})", waiting.len()), waiting, true))}
                {group(format!("In progress ({})", active.len()), active, false)}
                <button class="mb-2 text-sm text-muted hover:text-ink" aria-expanded=move || show_finished.get().to_string()
                    on:click=move |_| set_show_finished.update(|s| *s = !*s)>
                    {move || format!("{} Finished ({done_count})", if show_finished.get() { "▾" } else { "▸" })}
                </button>
                <Show when=move || show_finished.get()>
                    {group(format!("Finished ({done_count})"), done.clone(), false)}
                </Show>
            }
        }}
    }
}

#[component]
pub fn NewRequestPage() -> impl IntoView {
    let go = nav();
    let workspaces = workspaces_signal();
    let (target, set_target) = signal(String::new());
    let (request, set_request) = signal(String::new());
    let (error, set_error) = signal(String::new());
    let chosen = move || {
        let t = target.get();
        if t.is_empty() {
            workspaces
                .get()
                .first()
                .map(|w| w.name.clone())
                .unwrap_or_default()
        } else {
            t
        }
    };
    let submit = move |_| {
        let body = Submit {
            workspace: chosen(),
            request: request.get(),
        };
        spawn_local(async move {
            match ipc::call::<String, String>(command::JOB_SUBMIT, body).await {
                Ok(id) => go.set(Route::Job(id)),
                Err(e) => set_error.set(e.text()),
            }
        });
    };
    let input = "rounded border border-line bg-bg px-2 py-1 text-ink";
    view! {
        <button class="mb-2 text-sm text-muted hover:text-ink" on:click=move |_| go.set(Route::Jobs)>"← All jobs"</button>
        <h1 class="mt-0 text-2xl font-bold">"New request"</h1>
        <section class="grid max-w-2xl gap-3 rounded-xl border border-line bg-panel p-4">
            <label class="grid gap-1 text-sm text-muted">"Workspace"
                <select class=input on:change=move |e| set_target.set(event_target_value(&e))>
                    {move || workspaces.get().into_iter().map(|w| view! { <option value=w.name.clone()>{w.name.clone()}</option> }).collect_view()}
                </select>
            </label>
            <label class="grid gap-1 text-sm text-muted">"What should change?"
                <textarea class={format!("{input} min-h-32")} prop:value=request on:input=move |e| set_request.set(event_target_value(&e))></textarea>
            </label>
            <p class="m-0 text-sm text-muted">
                {move || if workspaces.get().is_empty() { "Add a workspace in Settings first." } else { "" }}
            </p>
            <p class="m-0 text-bad" role="alert">{error}</p>
            <button class="w-fit rounded-lg bg-primary px-3 py-2 font-semibold text-white hover:bg-primary-hover disabled:opacity-50"
                disabled=move || chosen().is_empty() || request.get().trim().is_empty() on:click=submit>"Submit"</button>
        </section>
    }
}
