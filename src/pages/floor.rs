//! The floor (PLAN.md A3): workspaces, a new request, and every job by stage.
//! The inbox: every job that waits on the owner. Both read again on each push.

use bastion_protocol::{JobSummary, Workspace, command, event};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc::{self, None};
use crate::pages::job::JobPanel;

const STAGES: [&str; 5] = ["plan", "approve", "build", "review", "land"];

#[derive(Serialize)]
struct AddWorkspace {
    workspace: Workspace,
}

#[derive(Serialize)]
struct Submit {
    workspace: String,
    request: String,
}

/// Jobs, kept current by `job-changed`.
fn jobs_signal() -> (ReadSignal<Vec<JobSummary>>, ReadSignal<String>) {
    let (jobs, set_jobs) = signal(Vec::<JobSummary>::new());
    let (error, set_error) = signal(String::new());
    let reload = move || {
        spawn_local(async move {
            match ipc::call::<Vec<JobSummary>, String>(command::JOBS, None {}).await {
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

fn card(job: JobSummary, open: WriteSignal<Option<String>>) -> impl IntoView {
    let id = job.id.clone();
    let color = match job.state.as_str() {
        "needs_you" => "text-warn",
        "failed" => "text-bad",
        "landed" => "text-ok",
        _ => "text-muted",
    };
    view! {
        <li>
            <button class="grid w-full gap-0.5 rounded-lg border border-line bg-bg p-2 text-left hover:border-primary"
                on:click=move |_| open.set(Some(id.clone()))>
                <span class="line-clamp-2 text-sm">{job.request}</span>
                <span class={format!("text-xs {color}")}>{format!("{} · {}", job.workspace, job.state.replace('_', " "))}</span>
            </button>
        </li>
    }
}

#[component]
pub fn FloorPage() -> impl IntoView {
    let (jobs, jobs_error) = jobs_signal();
    let (open, set_open) = signal(None::<String>);
    let (workspaces, set_workspaces) = signal(Vec::<Workspace>::new());
    let (note, set_note) = signal(String::new());
    let (ws_name, set_ws_name) = signal(String::new());
    let (ws_path, set_ws_path) = signal(String::new());
    let (ws_branch, set_ws_branch) = signal(String::from("main"));
    let (target, set_target) = signal(String::new());
    let (request, set_request) = signal(String::new());

    let load_workspaces = move || {
        spawn_local(async move {
            if let Ok(all) = ipc::call::<Vec<Workspace>, String>(command::WORKSPACES, None {}).await
            {
                if target.get_untracked().is_empty()
                    && let Some(first) = all.first()
                {
                    set_target.set(first.name.clone());
                }
                set_workspaces.set(all);
            }
        });
    };
    load_workspaces();

    let add_workspace = move |_| {
        let workspace = Workspace {
            name: ws_name.get(),
            path: ws_path.get(),
            branch: ws_branch.get(),
        };
        spawn_local(async move {
            match ipc::call::<(), String>(command::WORKSPACE_ADD, AddWorkspace { workspace }).await
            {
                Ok(()) => {
                    set_note.set(String::new());
                    set_ws_name.set(String::new());
                    set_ws_path.set(String::new());
                    load_workspaces();
                }
                Err(e) => set_note.set(e.text()),
            }
        });
    };
    let submit = move |_| {
        let body = Submit {
            workspace: target.get(),
            request: request.get(),
        };
        spawn_local(async move {
            match ipc::call::<String, String>(command::JOB_SUBMIT, body).await {
                Ok(id) => {
                    set_note.set(String::new());
                    set_request.set(String::new());
                    set_open.set(Some(id));
                }
                Err(e) => set_note.set(e.text()),
            }
        });
    };
    let input = "rounded border border-line bg-bg px-2 py-1 text-ink";

    view! {
        <h1 class="mt-0 text-2xl font-bold">"Floor"</h1>
        <p class="text-bad" role="alert">{jobs_error}{note}</p>
        <section class="mb-4 grid gap-3 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"New request"</h2>
            <label class="grid gap-1 text-sm text-muted">"Workspace"
                <select class=input on:change=move |e| set_target.set(event_target_value(&e))>
                    {move || workspaces.get().into_iter().map(|w| {
                        let selected = w.name == target.get();
                        view! { <option value=w.name.clone() selected=selected>{w.name.clone()}</option> }
                    }).collect_view()}
                </select>
            </label>
            <label class="grid gap-1 text-sm text-muted">"What should change?"
                <textarea class={format!("{input} min-h-20")} prop:value=request on:input=move |e| set_request.set(event_target_value(&e))></textarea>
            </label>
            <button class="w-fit rounded-lg bg-primary px-3 py-2 font-semibold text-white hover:bg-primary-hover disabled:opacity-50"
                disabled=move || target.get().is_empty() || request.get().trim().is_empty() on:click=submit>"Submit"</button>
        </section>

        <section class="mb-4 grid grid-cols-5 gap-2">
            {STAGES.map(|stage| view! {
                <div class="grid content-start gap-2 rounded-xl border border-line bg-panel p-2">
                    <h2 class="m-0 text-sm font-semibold capitalize text-muted">{stage}</h2>
                    <ul class="m-0 grid list-none gap-2 p-0">
                        {move || jobs.get().into_iter()
                            .filter(|j| j.stage == stage && !matches!(j.state.as_str(), "landed" | "failed" | "stopped"))
                            .map(|j| card(j, set_open)).collect_view()}
                    </ul>
                </div>
            })}
        </section>

        {move || open.get().map(|id| view! { <JobPanel id=id /> })}

        <section class="mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Finished"</h2>
            <ul class="m-0 grid list-none gap-2 p-0">
                {move || jobs.get().into_iter()
                    .filter(|j| matches!(j.state.as_str(), "landed" | "failed" | "stopped"))
                    .map(|j| card(j, set_open)).collect_view()}
            </ul>
        </section>

        <section class="mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Workspaces"</h2>
            <ul class="m-0 grid list-none gap-1 p-0 text-sm">
                {move || workspaces.get().into_iter().map(|w| view! {
                    <li>{w.name}<span class="text-muted">{format!("  {} @ {}", w.path, w.branch)}</span></li>
                }).collect_view()}
            </ul>
            <div class="flex flex-wrap items-end gap-2 text-sm">
                <label class="grid gap-1 text-muted">"Name"<input class=input prop:value=ws_name on:input=move |e| set_ws_name.set(event_target_value(&e)) /></label>
                <label class="grid gap-1 text-muted">"Repository path"<input class=input prop:value=ws_path on:input=move |e| set_ws_path.set(event_target_value(&e)) /></label>
                <label class="grid gap-1 text-muted">"Branch"<input class=input prop:value=ws_branch on:input=move |e| set_ws_branch.set(event_target_value(&e)) /></label>
                <button class="rounded-lg border border-line px-3 py-1.5 hover:bg-line" on:click=add_workspace>"Add"</button>
            </div>
        </section>
    }
}

#[component]
pub fn InboxPage() -> impl IntoView {
    let (jobs, error) = jobs_signal();
    view! {
        <h1 class="mt-0 text-2xl font-bold">"Inbox"</h1>
        <p class="text-bad" role="alert">{error}</p>
        {move || {
            let waiting: Vec<JobSummary> = jobs.get().into_iter().filter(|j| j.state == "needs_you").collect();
            if waiting.is_empty() {
                view! { <p class="text-muted">"Nothing waits on you."</p> }.into_any()
            } else {
                view! {
                    <div class="grid gap-4">
                        {waiting.into_iter().map(|j| view! { <JobPanel id=j.id /> }).collect_view()}
                    </div>
                }.into_any()
            }
        }}
    }
}
