//! One job: its artifacts, its question, the actions the core allows, and the
//! latest turn's transcript (PLAN.md A3). Read again on each push.

use bastion_protocol::{JobDetail, command, event};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc;

#[derive(Serialize)]
struct Id {
    id: String,
}

#[derive(Serialize)]
struct Comment {
    id: String,
    text: String,
}

#[component]
pub fn JobPanel(id: String) -> impl IntoView {
    let (detail, set_detail) = signal(None::<JobDetail>);
    let (text, set_text) = signal(String::new());
    let (error, set_error) = signal(String::new());
    let job_id = StoredValue::new(id);

    // Only the latest read may land: reads overlap when pushes come fast.
    let latest = StoredValue::new(0u64);
    let reload = move || {
        let n = latest.get_value() + 1;
        latest.set_value(n);
        spawn_local(async move {
            let id = job_id.get_value();
            let answer = ipc::call::<JobDetail, String>(command::JOB, Id { id }).await;
            if latest.get_value() != n {
                return;
            }
            match answer {
                Ok(d) => set_detail.set(Some(d)),
                Err(e) => set_error.set(e.text()),
            }
        });
    };
    ipc::listen(event::JOB_CHANGED, move |changed: String| {
        if changed == job_id.get_value() {
            reload();
        }
    });
    reload();

    let act = move |cmd: &'static str| {
        spawn_local(async move {
            let id = job_id.get_value();
            let result = if cmd == command::JOB_COMMENT {
                ipc::call::<(), String>(
                    cmd,
                    Comment {
                        id,
                        text: text.get_untracked(),
                    },
                )
                .await
            } else {
                ipc::call::<(), String>(cmd, Id { id }).await
            };
            match result {
                Ok(()) => {
                    set_text.set(String::new());
                    set_error.set(String::new());
                }
                Err(e) => set_error.set(e.text()),
            }
        });
    };
    let allows = move |action: &str| {
        detail
            .get()
            .is_some_and(|d| d.job.actions.iter().any(|a| a == action))
    };

    view! {
        <section class="grid gap-3 rounded-xl border border-line bg-panel p-4">
            {move || detail.get().map(|d| view! {
                <div class="flex flex-wrap items-baseline justify-between gap-2">
                    <h2 class="m-0 text-lg font-bold">{d.job.request.clone()}</h2>
                    <span class="text-sm text-muted">
                        {format!("{} · {} · {} · {}", d.job.id, d.job.workspace, d.job.stage, d.job.state.replace('_', " "))}
                    </span>
                </div>
                {d.job.error.clone().map(|e| view! { <p class="m-0 text-bad">{e}</p> })}
                {d.job.question.clone().map(|q| view! {
                    <p class="m-0 rounded border border-warn p-2"><strong>"The agent asks: "</strong>{q}</p>
                })}
                {d.artifacts.iter().rev().map(|a| view! {
                    <article class="grid gap-1">
                        <h3 class="m-0 text-sm font-semibold text-muted">{format!("{} artifact", a.stage)}</h3>
                        <pre class="m-0 max-h-96 overflow-auto whitespace-pre-wrap rounded bg-bg p-3 text-sm">{a.body.clone()}</pre>
                    </article>
                }).collect_view()}
                <details>
                    <summary class="cursor-pointer text-sm text-muted">{format!("Latest turn transcript ({} messages)", d.transcript.len())}</summary>
                    <ol class="m-0 grid gap-2 p-0 pl-5 text-xs">
                        {d.transcript.iter().map(|line| view! {
                            <li><strong>{line.role.clone()}</strong><pre class="m-0 whitespace-pre-wrap">{line.text.clone()}</pre></li>
                        }).collect_view()}
                    </ol>
                </details>
            })}
            <Show when=move || allows("comment") || allows("answer")>
                <label class="grid gap-1 text-sm text-muted">
                    {move || if allows("answer") { "Your answer" } else { "Comment: sends the work back with your note" }}
                    <textarea class="min-h-20 rounded border border-line bg-bg p-2 text-ink"
                        prop:value=text on:input=move |e| set_text.set(event_target_value(&e))></textarea>
                </label>
            </Show>
            <div class="flex flex-wrap gap-2">
                <Show when=move || allows("approve")>
                    <button class="rounded-lg bg-primary px-3 py-2 font-semibold text-white hover:bg-primary-hover" on:click=move |_| act(command::JOB_APPROVE)>"Approve"</button>
                </Show>
                <Show when=move || allows("comment") || allows("answer")>
                    <button class="rounded-lg border border-line px-3 py-2 hover:bg-line disabled:opacity-50"
                        disabled=move || text.get().trim().is_empty() on:click=move |_| act(command::JOB_COMMENT)>
                        {move || if allows("answer") { "Answer" } else { "Send back" }}
                    </button>
                </Show>
                <Show when=move || allows("stop")>
                    <button class="rounded-lg border border-line px-3 py-2 text-bad hover:bg-line" on:click=move |_| act(command::JOB_STOP)>"Stop"</button>
                </Show>
            </div>
            <p class="m-0 text-bad" role="alert">{error}</p>
        </section>
    }
}
