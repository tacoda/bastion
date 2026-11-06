//! The Jobs page and New request (PLAN.md A3): every job, the ones waiting on
//! the owner first, after wipp's Jobs page and GitHub Actions' runs list.

use bastion_protocol::{IssueView, JobSummary, LOCAL, Workspace, command, event};
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

/// A job, and the Bastion server it runs on (D30).
#[derive(Clone)]
struct ServerJob {
    server: String,
    job: JobSummary,
}

/// Every server's jobs, and why any server's could not be read. A server
/// that cannot be read keeps the jobs last read from it, so they stay listed.
async fn read_all_jobs(before: &[ServerJob]) -> (Vec<ServerJob>, Vec<String>) {
    let (mut all, mut errors) = (Vec::new(), Vec::new());
    for s in ipc::servers().await {
        match ipc::call_on::<Vec<JobSummary>, String>(&s.name, command::JOBS, None {}).await {
            Ok(jobs) => all.extend(jobs.into_iter().map(|job| ServerJob {
                server: s.name.clone(),
                job,
            })),
            Err(e) => {
                errors.push(format!("{}: {}", s.name, e.text()));
                all.extend(before.iter().filter(|j| j.server == s.name).cloned());
            }
        }
    }
    all.sort_by_key(|j| std::cmp::Reverse(j.job.created_at));
    (all, errors)
}

/// The jobs page's state: every job, the servers that cannot be reached,
/// and why any reachable server's jobs could not be read.
struct Jobs {
    jobs: ReadSignal<Vec<ServerJob>>,
    down: ReadSignal<Vec<String>>,
    errors: ReadSignal<Vec<String>>,
}

/// Every job on every server, kept current by `job-changed`, and the servers
/// that cannot be reached now. Only the latest read lands.
fn jobs_signal() -> Jobs {
    let (jobs, set_jobs) = signal(Vec::<ServerJob>::new());
    let (down, set_down) = signal(ipc::unreachable());
    let (errors, set_errors) = signal(Vec::<String>::new());
    let latest = StoredValue::new(0u64);
    let reload = move || {
        let n = latest.get_value() + 1;
        latest.set_value(n);
        spawn_local(async move {
            let (all, failed) = read_all_jobs(&jobs.get_untracked()).await;
            if latest.get_value() == n {
                set_jobs.set(all);
                set_errors.set(failed);
            }
        });
    };
    ipc::listen(event::JOB_CHANGED, move |_: String| reload());
    ipc::listen(ipc::REACH_CHANGED, move |_: String| {
        set_down.set(ipc::unreachable());
        reload();
    });
    reload();
    Jobs { jobs, down, errors }
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

fn row(sj: ServerJob, down: bool) -> impl IntoView {
    let go = nav();
    let ServerJob { server, job } = sj;
    let (symbol, color) = mark(&job.state);
    let dim = if down { "opacity-60" } else { "" };
    let unreachable = if down { " · unreachable" } else { "" };
    let (id, on) = (job.id.clone(), server.clone());
    view! {
        <li class={format!("border-t border-line first:border-t-0 {dim}")}>
            <button class="grid w-full grid-cols-[24px_1fr_auto] items-start gap-x-3 px-3 py-2.5 text-left hover:bg-line"
                on:click=move |_| {
                    // The job's page talks to the server it runs on.
                    ipc::choose(&on);
                    go.set(Route::Job(id.clone()))
                }>
                <span class={format!("text-lg leading-6 {color}")} aria-hidden="true">{symbol}</span>
                <span class="grid min-w-0 gap-0.5">
                    <span class="truncate font-semibold">{job.request.clone()}</span>
                    <span class="text-xs text-muted">
                        {format!("{server}{unreachable} · {} · {} · {} · {}{}", job.workspace, job.id, job.state.replace('_', " "), ago(job.created_at), asked_by(&job))}
                    </span>
                </span>
                <span class="rounded-full border border-line px-2 py-0.5 text-xs capitalize text-muted">{job.stage.clone()}</span>
            </button>
        </li>
    }
}

fn group(title: String, jobs: Vec<ServerJob>, down: Vec<String>, accent: bool) -> impl IntoView {
    let border = if accent { "border-warn" } else { "border-line" };
    view! {
        <section class={format!("mb-4 rounded-xl border {border} bg-panel")}>
            <h2 class="m-0 border-b border-line px-3 py-2 text-sm font-semibold text-muted">{title}</h2>
            {if jobs.is_empty() {
                view! { <p class="m-0 px-3 py-2.5 text-sm text-muted">"Nothing here."</p> }.into_any()
            } else {
                let rows = jobs.into_iter().map(|j| {
                    let gone = down.contains(&j.server);
                    row(j, gone)
                });
                view! { <ul class="m-0 list-none p-0">{rows.collect_view()}</ul> }.into_any()
            }}
        </section>
    }
}

/// " · by ana", for a job someone asked for.
fn asked_by(job: &JobSummary) -> String {
    job.requester
        .as_ref()
        .map_or_else(String::new, |who| format!(" · by {who}"))
}

/// What the page says about servers it cannot read: unreachable ones first,
/// then why a reachable one's jobs did not read.
fn page_note(down: &[String], errors: &[String]) -> String {
    let failed = errors
        .iter()
        .filter(|e| !down.iter().any(|d| e.starts_with(&format!("{d}: "))));
    let mut lines: Vec<String> = vec![unreachable_note(down)];
    lines.extend(failed.cloned());
    lines.retain(|l| !l.is_empty());
    lines.join(" ")
}

/// What the page says while servers cannot be reached.
fn unreachable_note(down: &[String]) -> String {
    match down {
        [] => String::new(),
        [one] => {
            format!("The Bastion server {one} cannot be reached. Its jobs show what was last read.")
        }
        many => format!(
            "The Bastion servers {} cannot be reached. Their jobs show what was last read.",
            many.join(", ")
        ),
    }
}

/// A job's Bastion server and workspace, as the filter names them.
fn place(j: &ServerJob) -> String {
    format!("{} / {}", j.server, j.job.workspace)
}

#[component]
pub fn JobsPage() -> impl IntoView {
    let go = nav();
    let Jobs { jobs, down, errors } = jobs_signal();
    // The filter's choices: each Bastion server and workspace that has jobs.
    let places = move || {
        let mut all: Vec<String> = jobs.get().iter().map(place).collect();
        all.sort();
        all.dedup();
        all
    };
    let (workspace, set_workspace) = signal(String::new());
    let (text, set_text) = signal(String::new());
    let (show_finished, set_show_finished) = signal(false);

    let shown = move || {
        let (ws, words) = (workspace.get(), text.get().to_lowercase());
        jobs.get()
            .into_iter()
            .filter(|j| ws.is_empty() || place(j) == ws)
            .filter(|j| {
                words.is_empty()
                    || j.job.request.to_lowercase().contains(&words)
                    || j.job.id.contains(&words)
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
        <p class="text-bad" role="alert">{move || page_note(&down.get(), &errors.get())}</p>
        <div class="mb-4 flex flex-wrap gap-3 text-sm" role="search">
            <label class="grid gap-1 text-muted">"Workspace"
                <select class=input on:change=move |e| set_workspace.set(event_target_value(&e))>
                    <option value="">"Every workspace"</option>
                    {move || places().into_iter().map(|p| view! { <option value=p.clone()>{p.clone()}</option> }).collect_view()}
                </select>
            </label>
            <label class="grid gap-1 text-muted">"Search"
                <input class=input type="search" placeholder="Request or id" on:input=move |e| set_text.set(event_target_value(&e)) />
            </label>
        </div>
        {move || {
            let all = shown();
            let waiting: Vec<_> = all.iter().filter(|j| j.job.state == "needs_you").cloned().collect();
            let active: Vec<_> = all.iter().filter(|j| IN_PROGRESS.contains(&j.job.state.as_str())).cloned().collect();
            let done: Vec<_> = all.iter().filter(|j| FINISHED.contains(&j.job.state.as_str())).cloned().collect();
            let done_count = done.len();
            view! {
                {(!waiting.is_empty()).then(|| group(format!("Waiting for you ({})", waiting.len()), waiting, down.get(), true))}
                {group(format!("In progress ({})", active.len()), active, down.get(), false)}
                <button class="mb-2 text-sm text-muted hover:text-ink" aria-expanded=move || show_finished.get().to_string()
                    on:click=move |_| set_show_finished.update(|s| *s = !*s)>
                    {move || format!("{} Finished ({done_count})", if show_finished.get() { "▾" } else { "▸" })}
                </button>
                <Show when=move || show_finished.get()>
                    {group(format!("Finished ({done_count})"), done.clone(), down.get(), false)}
                </Show>
            }
        }}
    }
}

#[component]
pub fn NewRequestPage() -> impl IntoView {
    let go = nav();
    // A request runs on `local` unless the owner picks another server (D30).
    ipc::choose(LOCAL);
    let servers = LocalResource::new(ipc::servers);
    let workspaces = workspaces_signal();
    let (picked, set_picked) = signal(String::new());
    let (request, set_request) = signal(String::new());
    let (error, set_error) = signal(String::new());
    let chosen = move || {
        let t = picked.get();
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
            <label class="grid gap-1 text-sm text-muted">"Bastion server"
                <select class=input on:change=move |e| {
                    set_picked.set(String::new());
                    ipc::choose(&event_target_value(&e));
                }>
                    {move || servers.get().unwrap_or_default().into_iter().map(|s| view! { <option value=s.name.clone()>{s.name.clone()}</option> }).collect_view()}
                </select>
            </label>
            <label class="grid gap-1 text-sm text-muted">"Workspace"
                <select class=input on:change=move |e| set_picked.set(event_target_value(&e))>
                    {move || workspaces.get().into_iter().map(|w| view! { <option value=w.name.clone()>{w.name.clone()}</option> }).collect_view()}
                </select>
            </label>
            <label class="grid gap-1 text-sm text-muted">"What should change?"
                <textarea class={format!("{input} min-h-32")} prop:value=request on:input=move |e| set_request.set(event_target_value(&e))></textarea>
            </label>
            <p class="m-0 text-sm text-muted">
                {move || if workspaces.get().is_empty() { "This Bastion server has no workspace yet. Add one in Settings." } else { "" }}
            </p>
            <p class="m-0 text-bad" role="alert">{error}</p>
            <button class="w-fit rounded-lg bg-primary px-3 py-2 font-semibold text-white hover:bg-primary-hover disabled:opacity-50"
                disabled=move || chosen().is_empty() || request.get().trim().is_empty() on:click=submit>"Submit"</button>
        </section>
        <FromIssue workspace=Signal::derive(chosen) />
    }
}

#[derive(Serialize)]
struct ForWorkspace {
    workspace: String,
}

#[derive(Serialize)]
struct FromIssueArgs {
    workspace: String,
    number: u64,
}

/// Start a job from one of the workspace's open GitHub issues (PLAN.md A7).
#[component]
fn FromIssue(workspace: Signal<String>) -> impl IntoView {
    let go = nav();
    let (issues, set_issues) = signal(Vec::<IssueView>::new());
    let (note, set_note) = signal(String::new());
    let load = move |_| {
        let workspace = workspace.get_untracked();
        spawn_local(async move {
            match ipc::call::<Vec<IssueView>, String>(command::ISSUES, ForWorkspace { workspace })
                .await
            {
                Ok(all) => {
                    set_note.set(if all.is_empty() {
                        "No open issues.".into()
                    } else {
                        String::new()
                    });
                    set_issues.set(all);
                }
                Err(e) => set_note.set(e.text()),
            }
        });
    };
    let start = move |number: u64| {
        let workspace = workspace.get_untracked();
        spawn_local(async move {
            match ipc::call::<String, String>(
                command::JOB_FROM_ISSUE,
                FromIssueArgs { workspace, number },
            )
            .await
            {
                Ok(id) => go.set(Route::Job(id)),
                Err(e) => set_note.set(e.text()),
            }
        });
    };
    view! {
        <section class="mt-4 grid max-w-2xl gap-2 rounded-xl border border-line bg-panel p-4">
            <div class="flex items-center justify-between">
                <h2 class="m-0 text-sm font-semibold text-muted">"Or start from a GitHub issue"</h2>
                <button class="rounded-lg border border-line px-3 py-1 text-sm hover:bg-line" on:click=load>"Load issues"</button>
            </div>
            <ul class="m-0 grid list-none gap-1 p-0 text-sm">
                {move || issues.get().into_iter().map(|i| {
                    let number = i.number;
                    view! {
                        <li><button class="w-full rounded px-2 py-1 text-left hover:bg-line" on:click=move |_| start(number)>
                            <span class="text-muted">{format!("#{} ", i.number)}</span>{i.title}
                        </button></li>
                    }
                }).collect_view()}
            </ul>
            <p class="m-0 text-sm text-muted" role="status">{note}</p>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_names_each_server_it_cannot_reach() {
        assert_eq!(unreachable_note(&[]), "");
        assert!(unreachable_note(&["Two".into()]).starts_with("The Bastion server Two cannot"));
        let both = unreachable_note(&["Two".into(), "box".into()]);
        assert!(
            both.starts_with("The Bastion servers Two, box cannot"),
            "{both}"
        );
    }

    #[test]
    fn a_reachable_server_that_fails_to_read_still_says_why() {
        let errors = vec![
            "local: no database".to_string(),
            "Two: cannot be reached".to_string(),
        ];
        let note = page_note(&["Two".into()], &errors);
        assert!(note.contains("Two cannot be reached"), "{note}");
        assert!(note.contains("local: no database"), "{note}");
        assert!(!note.contains("Two: cannot"), "{note}");
    }
}
