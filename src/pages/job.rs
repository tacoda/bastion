//! The job page (PLAN.md A3): one job as a run. The request and its badges,
//! the owner's actions when it waits, then its pipeline as steps that open to
//! their artifacts and turn. Read again on each push.

use bastion_protocol::{
    ArtifactView, CharterView, HeldCallRef, HeldCallView, JobDetail, StepView, TranscriptLine,
    command, event,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc;
use crate::pages::diff::DiffView;
use crate::route::{Route, ago, mark, nav, until};

#[derive(Serialize)]
struct Id {
    id: String,
}

#[derive(Serialize)]
struct Comment {
    id: String,
    text: String,
}

#[derive(Serialize)]
struct Allow {
    held: HeldCallRef,
}

#[derive(Serialize)]
struct Refuse {
    held: HeldCallRef,
    feedback: String,
}

/// One call the gate holds (PLAN.md A5): allow exactly it once, or refuse it
/// with a note the agent reads as the call's result.
#[component]
fn HeldCall(job: String, call: HeldCallView, error: WriteSignal<String>) -> impl IntoView {
    let (feedback, set_feedback) = signal(String::new());
    let held = StoredValue::new(HeldCallRef {
        job,
        call: call.id.clone(),
    });
    let report = move |result: Result<(), ipc::Failure<String>>| match result {
        Ok(()) => error.set(String::new()),
        Err(e) => error.set(e.text()),
    };
    let allow = move |_| {
        spawn_local(async move {
            report(
                ipc::call::<(), String>(
                    command::HELD_ALLOW,
                    Allow {
                        held: held.get_value(),
                    },
                )
                .await,
            );
        });
    };
    let refuse = move |_| {
        spawn_local(async move {
            let args = Refuse {
                held: held.get_value(),
                feedback: feedback.get_untracked(),
            };
            report(ipc::call::<(), String>(command::HELD_REFUSE, args).await);
        });
    };
    view! {
        <li class="grid gap-2 border-t border-line pt-3 first:border-t-0 first:pt-0">
            <p class="m-0"><strong>{call.function}</strong>" "<code class="break-all">{call.text}</code>
                <span class="text-xs text-muted">{format!(" · held {}", ago(call.at))}</span></p>
            <textarea class="min-h-16 rounded border border-line bg-bg p-2 text-ink" spellcheck="false" aria-label="What the agent should do instead"
                placeholder="What the agent should do instead"
                prop:value=feedback on:input=move |e| set_feedback.set(event_target_value(&e))></textarea>
            <div class="flex flex-wrap gap-2">
                <button class="rounded-lg bg-primary px-3 py-2 font-semibold text-white hover:bg-primary-hover" on:click=allow>"Allow once"</button>
                <button class="rounded-lg border border-line px-3 py-2 hover:bg-line disabled:opacity-50"
                    disabled=move || feedback.get().trim().is_empty() on:click=refuse>"Refuse with this note"</button>
            </div>
        </li>
    }
}

/// What governs this job: the calls the gate holds, then its charter.
#[component]
fn Governed(detail: ReadSignal<Option<JobDetail>>, error: WriteSignal<String>) -> impl IntoView {
    let held = move || {
        detail.get().filter(|d| !d.held.is_empty()).map(|d| view! {
            <section class="mb-4 grid gap-3 rounded-xl border border-warn bg-panel p-4">
                <h2 class="m-0 text-sm font-semibold text-warn">"The gate holds a call for you"</h2>
                <ul class="m-0 grid list-none gap-3 p-0">
                    {d.held.into_iter().map(|call| view! { <HeldCall job=d.job.id.clone() call=call error=error /> }).collect_view()}
                </ul>
            </section>
        })
    };
    view! { {held} {move || detail.get().map(|d| charter(d.charter))} }
}

/// What the repository asks of this job, or that it asks nothing.
fn charter(c: Option<CharterView>) -> impl IntoView + use<> {
    let c = c?;
    let files = if c.files.is_empty() {
        "No charter: this job runs on the owner's defaults.".to_string()
    } else {
        format!("Charter: {}", c.files.join(", "))
    };
    let lines = |title: &'static str, items: Vec<String>| {
        (!items.is_empty()).then(|| view! {
            <h3 class="m-0 mt-2 text-xs font-semibold text-muted">{title}</h3>
            <ul class="m-0 pl-5 text-xs">{items.into_iter().map(|i| view! { <li class="break-all">{i}</li> }).collect_view()}</ul>
        })
    };
    Some(view! {
        <section class="mb-4 rounded-xl border border-line bg-panel p-4">
            <p class="m-0 text-sm">{files}</p>
            {lines("The repository's rules", c.rules)}
            {lines("The repository's hooks, as they ran", c.hook_runs)}
            {lines("What review found", c.findings)}
        </section>
    })
}

/// The request and its badges: state, workspace, id and age.
fn header(d: &JobDetail) -> impl IntoView + use<> {
    let (symbol, color) = mark(&d.job.state);
    let badge = "rounded-full border border-line px-2 py-0.5 text-muted";
    view! {
        <div class="mb-4 grid gap-2">
            <h1 class="m-0 text-2xl font-bold">{d.job.request.clone()}</h1>
            <div class="flex flex-wrap gap-2 text-xs">
                <span class={format!("rounded-full border border-line px-2 py-0.5 {color}")}>{format!("{symbol} {}", d.job.state.replace('_', " "))}</span>
                <span class=badge>{d.job.workspace.clone()}</span>
                <span class=badge>{d.job.id.clone()}</span>
                <span class=badge>{ago(d.job.created_at)}</span>
            </div>
        </div>
    }
}

/// Why the job failed, or why its land did.
fn failure(d: &JobDetail) -> impl IntoView + use<> {
    let title = if d.job.state == "failed" {
        "This job failed"
    } else {
        "The land failed; the VM and its commits are kept"
    };
    d.job.error.clone().map(|e| {
        view! {
            <section class="mb-4 rounded-xl border border-bad bg-panel p-4">
                <h2 class="m-0 mb-2 text-sm font-semibold text-bad">{title}</h2>
                <pre class="m-0 whitespace-pre-wrap text-sm">{e}</pre>
            </section>
        }
    })
}

#[component]
pub fn JobPage(id: String) -> impl IntoView {
    let go = nav();
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
        if changed.is_empty() || changed == job_id.get_value() {
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
    let waits = move || allows("approve") || allows("answer");

    view! {
        // Stop sits outside the part that re-renders on each push, so a busy
        // turn's pushes cannot replace it between press and release.
        <div class="mb-2 flex items-center justify-between gap-3">
            <button class="text-sm text-muted hover:text-ink" on:click=move |_| go.set(Route::Jobs)>"← All jobs"</button>
            <Show when=move || allows("stop")>
                <button class="rounded-lg border border-line px-3 py-2 text-bad hover:bg-line" on:click=move |_| act(command::JOB_STOP)>"Stop"</button>
            </Show>
        </div>
        {move || detail.get().map(|d| view! { {header(&d)} {failure(&d)} })}

        <Show when=waits>
            <section class="mb-4 grid gap-3 rounded-xl border border-warn bg-panel p-4">
                <h2 class="m-0 text-sm font-semibold text-warn">"Waiting for you"</h2>
                {move || detail.get().and_then(|d| d.job.hold_until).map(|at| view! {
                    <p class="m-0 text-sm text-warn">{format!(
                        "Held: this approves itself {} unless you act first.",
                        until(at)
                    )}</p>
                })}
                {move || detail.get().and_then(|d| d.job.question).map(|q| view! {
                    <p class="m-0"><strong>"The agent asks: "</strong>{q}</p>
                })}
                <Show when=move || allows("approve")>
                    <p class="m-0 text-sm text-muted">{move || detail.get().map(|d| match d.job.stage.as_str() {
                        "land" => "Approve retries the land, or stop the job.".to_string(),
                        "review" => "Approve the build artifact to move on, or send it back with a note.".to_string(),
                        _ => "Approve the plan artifact to move on, or send it back with a note.".to_string(),
                    })}</p>
                </Show>
                {move || detail.get().filter(|_| allows("approve")).map(|d| {
                    // What the checkpoint judges: the plan at approve; the build's report and its diff at review.
                    let latest = |stage: &str| d.artifacts.iter().rev().find(|a| a.stage == stage).map(|a| a.body.clone());
                    let reviewing = d.job.stage == "review";
                    let report = match d.job.stage.as_str() {
                        "review" => latest("build"),
                        "approve" => latest("plan"),
                        _ => None,
                    };
                    let diff = if reviewing { latest("review") } else { None };
                    view! {
                        {report.map(|body| view! {
                            <pre class="m-0 max-h-96 overflow-auto whitespace-pre-wrap wrap-anywhere rounded bg-bg p-3 text-sm">{body}</pre>
                        })}
                        {diff.map(|diff| view! {
                            <h3 class="m-0 text-sm font-semibold text-muted">"The change (click a line to comment on it)"</h3>
                            <DiffView diff=diff comment=set_text />
                        })}
                    }
                })}
                <Show when=move || allows("comment") || allows("answer")>
                <label class="grid gap-1 text-sm text-muted">
                    {move || if allows("answer") { "Your answer" } else { "A note for the agent (sends the work back)" }}
                    // Spellcheck off: macOS text checking measures the whole page through
                    // accessibility after each comma, and a large diff froze the webview.
                    <textarea class="min-h-20 rounded border border-line bg-bg p-2 text-ink" spellcheck="false"
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
                </div>
            </section>
        </Show>
        <Governed detail=detail error=set_error />
        <p class="text-bad" role="alert">{error}</p>

        <section class="rounded-xl border border-line bg-panel">
            <h2 class="m-0 border-b border-line px-3 py-2 text-sm font-semibold text-muted">"Steps"</h2>
            <ol class="m-0 list-none p-0">
                {move || detail.get().map(|d| {
                    // The latest turn belongs to the last agent step that ran.
                    let turn_step = d.steps.iter().rev()
                        .find(|s| matches!(s.stage.as_str(), "plan" | "build") && s.state != "pending")
                        .map(|s| s.stage.clone());
                    d.steps.clone().into_iter().map(|step| {
                        let artifacts: Vec<ArtifactView> = d.artifacts.iter().filter(|a| a.stage == step.stage).cloned().collect();
                        let transcript = (turn_step.as_deref() == Some(step.stage.as_str())).then(|| d.transcript.clone());
                        step_row(step, artifacts, transcript)
                    }).collect_view()
                })}
            </ol>
        </section>
    }
}

fn step_row(
    step: StepView,
    artifacts: Vec<ArtifactView>,
    transcript: Option<Vec<TranscriptLine>>,
) -> impl IntoView {
    let (symbol, color) = mark(&step.state);
    let has_detail = !artifacts.is_empty() || transcript.as_ref().is_some_and(|t| !t.is_empty());
    let open = matches!(step.state.as_str(), "running" | "waiting" | "failed");
    view! {
        <li class="border-t border-line first:border-t-0">
            <details open=open>
                <summary class="flex cursor-pointer items-center gap-3 px-3 py-2.5 hover:bg-line">
                    <span class={format!("w-4 text-center {color}")} aria-hidden="true">{symbol}</span>
                    <span class="font-semibold capitalize">{step.stage.clone()}</span>
                    <span class="text-xs text-muted">{step.state.clone()}</span>
                </summary>
                {has_detail.then(|| view! {
                    <div class="grid gap-3 px-3 pb-3 pl-10">
                        {artifacts.into_iter().rev().map(|a| view! {
                            <article class="grid gap-1">
                                <h3 class="m-0 text-xs font-semibold text-muted">{format!("{} artifact · {}", a.stage, ago(a.created_at))}</h3>
                                <pre class="m-0 max-h-96 overflow-auto whitespace-pre-wrap wrap-anywhere rounded bg-bg p-3 text-sm">{a.body}</pre>
                            </article>
                        }).collect_view()}
                        {transcript.filter(|t| !t.is_empty()).map(|t| view! {
                            <details>
                                <summary class="cursor-pointer text-xs text-muted">{format!("Latest turn ({} messages)", t.len())}</summary>
                                <ol class="m-0 grid gap-2 p-0 pl-5 text-xs">
                                    {t.into_iter().map(|line| view! {
                                        <li><strong>{line.role}</strong><pre class="m-0 whitespace-pre-wrap">{line.text}</pre></li>
                                    }).collect_view()}
                                </ol>
                            </details>
                        })}
                    </div>
                })}
            </details>
        </li>
    }
}
