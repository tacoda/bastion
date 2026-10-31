//! The engine console (PLAN.md A2): containers with stop, start and logs,
//! then functions, trigger types and queues. Read again on each push.

use bastion_protocol::{
    Console, Container, EngineStatus, LogLine, LogPage, LogRequest, Queue, Registered, command,
    event,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc::{self, None};

/// The most log lines the panel keeps.
const LOG_LINES: usize = 500;
/// The most functions or trigger types a filter shows.
const SHOWN: usize = 100;

#[derive(Serialize)]
struct Name {
    name: String,
}

#[derive(Serialize)]
struct Logs {
    request: LogRequest,
}

#[component]
pub fn ConsolePanel() -> impl IntoView {
    let (console, set_console) = signal(None::<Console>);
    let (error, set_error) = signal(String::new());
    let (busy, set_busy) = signal(None::<String>);
    let (watching, set_watching) = signal(None::<String>);
    let (lines, set_lines) = signal(Vec::<LogLine>::new());
    let (filter, set_filter) = signal(String::new());

    let reload = move || {
        spawn_local(async move {
            match ipc::call::<Console, String>(command::CONSOLE, None {}).await {
                Ok(c) => {
                    set_error.set(String::new());
                    set_console.set(Some(c));
                }
                Err(e) => set_error.set(e.text()),
            }
        });
    };
    ipc::listen(event::CONSOLE_CHANGED, move |()| reload());
    ipc::listen(event::ENGINE_CHANGED, move |_: EngineStatus| reload());
    reload();

    let set_running = move |name: String, running: bool| {
        set_busy.set(Some(name.clone()));
        spawn_local(async move {
            let cmd = if running {
                command::CONTAINER_START
            } else {
                command::CONTAINER_STOP
            };
            if let Err(e) = ipc::call::<(), String>(cmd, Name { name }).await {
                set_error.set(e.text());
            }
            set_busy.set(Option::None);
            reload();
        });
    };

    // Each watch reads in a loop until another container is chosen.
    let watch = move |name: String| {
        set_watching.set(Some(name.clone()));
        set_lines.set(Vec::new());
        spawn_local(async move {
            let mut cursor = Option::None;
            while watching.get_untracked().as_deref() == Some(name.as_str()) {
                let request = LogRequest {
                    container: name.clone(),
                    cursor: cursor.clone(),
                };
                match ipc::call::<LogPage, String>(command::CONTAINER_LOGS, Logs { request }).await
                {
                    Ok(page) if watching.get_untracked().as_deref() == Some(name.as_str()) => {
                        set_lines.update(|all| {
                            all.extend(page.lines);
                            let over = all.len().saturating_sub(LOG_LINES);
                            all.drain(..over);
                        });
                        cursor = page.cursor;
                    }
                    Ok(_) => break,
                    Err(e) => {
                        set_error.set(e.text());
                        break;
                    }
                }
            }
        });
    };

    let container_row = move |c: Container| {
        let name = c.name.clone();
        let (stop_name, start_name, log_name) = (name.clone(), name.clone(), name.clone());
        let ready = c.state == "ready";
        // compose owns only a running process, so any stopped container may be started.
        let can_toggle = !ready || c.owned;
        let color = match c.state.as_str() {
            "ready" => "text-ok",
            "failed" => "text-bad",
            _ => "text-warn",
        };
        let this_busy = move || busy.get().as_deref() == Some(name.as_str());
        view! {
            <tr class="border-t border-line">
                <td class="py-1.5 pr-3">{c.name.clone()}</td>
                <td class="py-1.5 pr-3 text-muted">{c.version.unwrap_or_default()}</td>
                <td class={format!("py-1.5 pr-3 {color}")}>
                    {c.state}
                    {c.last_error.map(|e| view! { <span class="block text-xs text-muted">{e}</span> })}
                </td>
                <td class="flex gap-2 py-1.5">
                    {can_toggle.then(|| view! {
                        <button class="rounded border border-line px-2 text-sm hover:bg-line disabled:opacity-50"
                            disabled=this_busy
                            on:click={
                                let (stop_name, start_name) = (stop_name.clone(), start_name.clone());
                                move |_| if ready { set_running(stop_name.clone(), false) } else { set_running(start_name.clone(), true) }
                            }>
                            {if ready { "Stop" } else { "Start" }}
                        </button>
                    })}
                    <button class="rounded border border-line px-2 text-sm hover:bg-line"
                        on:click={ let log_name = log_name.clone(); move |_| watch(log_name.clone()) }>"Logs"</button>
                </td>
            </tr>
        }
    };

    let matching = move |rows: Vec<Registered>| {
        let needle = filter.get().to_lowercase();
        let all: Vec<Registered> = rows
            .into_iter()
            .filter(|r| needle.is_empty() || r.id.to_lowercase().contains(&needle))
            .collect();
        let total = all.len();
        (all.into_iter().take(SHOWN).collect::<Vec<_>>(), total)
    };

    view! {
        <section class="mt-4 grid gap-3 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Containers"</h2>
            <p class="m-0 text-bad" role="alert">{error}</p>
            <table class="w-full border-collapse text-left text-sm">
                <thead><tr class="text-muted"><th>"Name"</th><th>"Version"</th><th>"State"</th><th></th></tr></thead>
                <tbody>
                    {move || console.get().map(|c| c.containers.into_iter().map(container_row).collect_view())}
                </tbody>
            </table>
        </section>

        <Show when=move || watching.get().is_some()>
            <section class="mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4">
                <div class="flex items-center justify-between">
                    <h2 class="m-0 text-sm font-semibold text-muted">{move || format!("Logs: {}", watching.get().unwrap_or_default())}</h2>
                    <button class="rounded border border-line px-2 text-sm hover:bg-line" on:click=move |_| set_watching.set(Option::None)>"Close"</button>
                </div>
                <pre class="m-0 max-h-80 overflow-auto whitespace-pre-wrap rounded bg-bg p-3 text-xs" aria-live="polite">
                    {move || lines.get().into_iter().map(|l| view! {
                        <span class={if l.stream == "stderr" { "block text-warn" } else { "block" }}>{l.message}</span>
                    }).collect_view()}
                </pre>
            </section>
        </Show>

        <section class="mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4">
            <div class="flex items-center justify-between gap-3">
                <h2 class="m-0 text-sm font-semibold text-muted">"Functions and trigger types"</h2>
                <label class="text-sm text-muted">"Filter "
                    <input class="rounded border border-line bg-bg px-2 py-1 text-ink" type="search"
                        on:input=move |e| set_filter.set(event_target_value(&e)) />
                </label>
            </div>
            {move || console.get().map(|c| {
                let (functions, f_total) = matching(c.functions);
                let (triggers, t_total) = matching(c.trigger_types);
                view! {
                    <h3 class="m-0 text-sm">{format!("Functions: {f_total}")}</h3>
                    <ul class="m-0 grid list-none gap-1 p-0 text-sm">{functions.into_iter().map(registered_row).collect_view()}</ul>
                    <h3 class="m-0 text-sm">{format!("Trigger types: {t_total}")}</h3>
                    <ul class="m-0 grid list-none gap-1 p-0 text-sm">{triggers.into_iter().map(registered_row).collect_view()}</ul>
                }
            })}
        </section>

        <section class="mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Queues"</h2>
            <table class="w-full border-collapse text-left text-sm">
                <thead><tr class="text-muted"><th>"Topic"</th><th>"Depth"</th><th>"Delivered"</th><th>"Failed"</th><th>"Dead letters"</th></tr></thead>
                <tbody>{move || console.get().map(|c| c.queues.into_iter().map(queue_row).collect_view())}</tbody>
            </table>
        </section>
    }
}

fn registered_row(r: Registered) -> impl IntoView {
    view! {
        <li>
            <code class="text-link">{r.id}</code>
            <span class="text-muted">{format!("  {} · {}", r.worker, r.namespace)}</span>
            <span class="block text-xs text-muted">{r.description}</span>
        </li>
    }
}

fn queue_row(q: Queue) -> impl IntoView {
    view! {
        <tr class="border-t border-line">
            <td class="py-1">{q.name}</td>
            <td>{q.depth}</td>
            <td>{q.delivered}</td>
            <td class={if q.failed > 0 { "text-bad" } else { "" }}>{q.failed}</td>
            <td class={if q.dead_letters > 0 { "text-bad" } else { "" }}>{q.dead_letters}</td>
        </tr>
    }
}
