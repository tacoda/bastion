//! The Evals pages (PLAN.md A6): every eval with its progress, a form to run
//! one, and one eval as A against B per request and in total, each refused
//! call linked to its audit entry and its job.

use bastion_protocol::{
    ConstraintView, EvalRow, EvalSummary, EvalView, NewEval, RunView, command, event,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc::{self, None};
use crate::pages::jobs::workspaces_signal;
use crate::route::{Route, ago, nav};

#[derive(Serialize)]
struct Id {
    id: String,
}

#[derive(Serialize)]
struct Create {
    eval: NewEval,
}

#[derive(Serialize)]
struct Apply {
    id: String,
    arm: String,
}

const INPUT: &str = "rounded border border-line bg-bg px-2 py-1 text-sm text-ink";
const BUTTON: &str =
    "rounded-lg border border-line px-3 py-1.5 text-sm hover:bg-line disabled:opacity-40";

/// Read `cmd` now and on every push, keeping only the latest answer.
fn live<T, A>(
    cmd: &'static str,
    args: impl Fn() -> A + Copy + 'static,
) -> (ReadSignal<Option<T>>, ReadSignal<String>)
where
    T: serde::de::DeserializeOwned + Send + Sync + 'static,
    A: Serialize + 'static,
{
    let (value, set_value) = signal(None::<T>);
    let (error, set_error) = signal(String::new());
    let latest = StoredValue::new(0u64);
    let reload = move || {
        let n = latest.get_value() + 1;
        latest.set_value(n);
        spawn_local(async move {
            let answer = ipc::call::<T, String>(cmd, args()).await;
            if latest.get_value() == n {
                match answer {
                    Ok(v) => set_value.set(Some(v)),
                    Err(e) => set_error.set(e.text()),
                }
            }
        });
    };
    ipc::listen(event::JOB_CHANGED, move |_: String| reload());
    reload();
    (value, error)
}

#[component]
pub fn EvalsPage() -> impl IntoView {
    let go = nav();
    let (evals, error) = live::<Vec<EvalSummary>, _>(command::EVALS, || None {});
    view! {
        <div class="mb-4 flex items-center justify-between">
            <h1 class="m-0 text-2xl font-bold">"Evals"</h1>
        </div>
        <p class="m-0 mb-4 max-w-3xl text-sm text-muted">
            "An eval runs the same requests from the same commit under two harness configurations, A and B, that differ in one rung. Runs approve their own plans and stop at review, so nothing lands."
        </p>
        <p class="text-bad" role="alert">{error}</p>
        <NewEvalForm />
        <section class="rounded-xl border border-line bg-panel">
            <h2 class="m-0 border-b border-line px-3 py-2 text-sm font-semibold text-muted">"Evals"</h2>
            <ul class="m-0 list-none p-0">
                {move || {
                    let all = evals.get().unwrap_or_default();
                    if all.is_empty() {
                        return view! { <li class="px-3 py-2 text-sm text-muted">"No evals yet."</li> }.into_any();
                    }
                    all.into_iter().map(|e| {
                        let id = e.id.clone();
                        view! {
                            <li class="border-t border-line first:border-t-0">
                                <button class="grid w-full grid-cols-[1fr_auto] gap-2 px-3 py-2 text-left hover:bg-line" on:click=move |_| go.set(Route::Eval(id.clone()))>
                                    <span><strong>{e.name.clone()}</strong>
                                        <span class="block text-xs text-muted">{format!("{} · {} · {} · {}", e.id, e.workspace, &e.base[..7.min(e.base.len())], ago(e.created_at))}</span>
                                    </span>
                                    <span class="text-sm text-muted">{format!("{} of {} runs done", e.done, e.runs)}</span>
                                </button>
                            </li>
                        }
                    }).collect_view().into_any()
                }}
            </ul>
        </section>
    }
}

/// A choice the owner may leave alone: its first option counts as chosen.
fn chosen(options: Vec<String>, picked: String) -> String {
    if picked.is_empty() {
        options.into_iter().next().unwrap_or_default()
    } else {
        picked
    }
}

/// A select bound to `value`, over `options` as (value, label).
fn select(
    value: RwSignal<String>,
    options: impl Fn() -> Vec<(String, String)> + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <select class=INPUT on:change=move |e| value.set(event_target_value(&e))>
            {move || options().into_iter().map(|(v, label)| {
                let here = v.clone();
                view! { <option value=v selected=move || value.get() == here>{label}</option> }
            }).collect_view()}
        </select>
    }
}

fn rung_options() -> Vec<(String, String)> {
    ["told", "checked", "enforced"]
        .map(|r| (r.to_string(), r.to_string()))
        .to_vec()
}

/// Run the eval, then open its page, or say why it did not start.
fn create(eval: NewEval, go: crate::route::Nav, error: WriteSignal<String>) {
    spawn_local(async move {
        match ipc::call::<String, String>(command::EVAL_CREATE, Create { eval }).await {
            Ok(id) => go.set(Route::Eval(id)),
            Err(e) => error.set(e.text()),
        }
    });
}

/// The constraint the two arms set, and the rung each sets it to.
fn arms(
    target: RwSignal<String>,
    options: impl Fn() -> Vec<(String, String)> + Send + Sync + 'static,
    a: RwSignal<String>,
    b: RwSignal<String>,
) -> impl IntoView {
    let label = "grid gap-1 text-xs text-muted";
    view! {
        <div class="grid gap-3 sm:grid-cols-3">
            <label class=label>"The constraint A and B set" {select(target, options)}</label>
            <label class=label>"A at" {select(a, rung_options)}</label>
            <label class=label>"B at" {select(b, rung_options)}</label>
        </div>
    }
}

/// The form to run an eval: requests, and the one rung A and B set.
#[component]
fn NewEvalForm() -> impl IntoView {
    let go = nav();
    let workspaces = workspaces_signal();
    let (constraints, _) = live::<Vec<ConstraintView>, _>(command::CONSTRAINTS, || None {});
    let [name, workspace, requests, target] = [(); 4].map(|_| RwSignal::new(String::new()));
    let (rung_a, rung_b) = (
        RwSignal::new("told".to_string()),
        RwSignal::new("enforced".to_string()),
    );
    let (error, set_error) = signal(String::new());
    let workspace_options = move || {
        workspaces
            .get()
            .into_iter()
            .map(|w| (w.name.clone(), w.name))
            .collect::<Vec<_>>()
    };
    let target_options = move || {
        let all = constraints.get().unwrap_or_default();
        all.into_iter()
            .map(|c| {
                (
                    format!("constraint:{}", c.id),
                    format!("{} ({})", c.id, c.rung),
                )
            })
            .collect::<Vec<_>>()
    };
    let submit = move |_| {
        let first = |options: Vec<(String, String)>, picked: RwSignal<String>| {
            chosen(
                options.into_iter().map(|(v, _)| v).collect(),
                picked.get_untracked(),
            )
        };
        let eval = NewEval {
            name: name.get_untracked(),
            workspace: first(workspace_options(), workspace),
            requests: requests.get_untracked().lines().map(String::from).collect(),
            target: first(target_options(), target),
            rung_a: rung_a.get_untracked(),
            rung_b: rung_b.get_untracked(),
        };
        create(eval, go, set_error);
    };
    let label = "grid gap-1 text-xs text-muted";
    view! {
        <details class="mb-4 rounded-xl border border-line bg-panel">
            <summary class="cursor-pointer px-3 py-2 text-sm font-semibold text-muted">"Run an eval"</summary>
            <div class="grid gap-3 p-3">
                <div class="grid gap-3 sm:grid-cols-2">
                    <label class=label>"Name"
                        <input class=INPUT spellcheck="false" prop:value=name on:input=move |e| name.set(event_target_value(&e)) />
                    </label>
                    <label class=label>"Workspace" {select(workspace, workspace_options)}</label>
                </div>
                <label class=label>"Requests, one per line"
                    <textarea class={format!("{INPUT} min-h-24")} spellcheck="false" prop:value=requests on:input=move |e| requests.set(event_target_value(&e))></textarea>
                </label>
                {arms(target, target_options, rung_a, rung_b)}
                <div class="flex items-center gap-3">
                    <button class=BUTTON on:click=submit>"Run it"</button>
                    <span class="text-xs text-muted">"Each request runs once under A and once under B: two plan and two build turns per request, plus a grade."</span>
                </div>
                <p class="m-0 text-sm text-bad" role="alert">{error}</p>
            </div>
        </details>
    }
}

/// One measure of a run, with the other arm's beside it when they differ.
fn cell(value: String, other: Option<String>) -> impl IntoView {
    let differs = other.is_some_and(|o| o != value);
    view! { <td class={if differs { "px-2 py-1 font-semibold text-warn" } else { "px-2 py-1" }}>{value}</td> }
}

fn measures(r: &RunView) -> [(&'static str, String); 8] {
    [
        (
            "Outcome",
            if r.job.is_empty() {
                String::new()
            } else if r.reviewed {
                "reached review".into()
            } else {
                r.state.clone()
            },
        ),
        ("Refused", r.refused.to_string()),
        ("Held", r.held.to_string()),
        ("Flagged", r.flagged.to_string()),
        ("Calls", r.passed.to_string()),
        ("Turns", r.turns.to_string()),
        ("Cost", format!("${:.4}", r.cost_micros as f64 / 1e6)),
        ("Grade", r.grade.map_or("-".into(), |g| g.to_string())),
    ]
}

/// A against B for one request or for the totals.
fn table(label: String, a: Option<RunView>, b: Option<RunView>) -> impl IntoView {
    let (ma, mb) = (a.as_ref().map(measures), b.as_ref().map(measures));
    let row = |arm: &'static str,
               m: Option<[(&'static str, String); 8]>,
               other: Option<[(&'static str, String); 8]>| {
        view! {
            <tr class="border-t border-line">
                <th class="px-2 py-1 text-left font-semibold">{arm}</th>
                {match m {
                    Some(m) => m.into_iter().enumerate().map(|(i, (_, v))| cell(v, other.as_ref().map(|o| o[i].1.clone()))).collect_view().into_any(),
                    None => view! { <td class="px-2 py-1 text-muted" colspan="8">"not started"</td> }.into_any(),
                }}
            </tr>
        }
    };
    view! {
        <table class="w-full text-sm">
            <caption class="pb-1 text-left text-sm font-semibold">{label}</caption>
            <thead><tr class="text-xs text-muted">
                <th></th>
                {measures(&RunView { job: "x".into(), ..RunView::default() }).map(|(name, _)| view! { <th class="px-2 py-1 text-left font-normal">{name}</th> })}
            </tr></thead>
            <tbody>{row("A", ma.clone(), mb.clone())}{row("B", mb, ma)}</tbody>
        </table>
    }
}

/// A run's refused calls, each with its audit entry, and a link to its job.
fn refusals(arm: &'static str, r: Option<RunView>) -> impl IntoView + use<> {
    let go = nav();
    r.filter(|r| !r.refused_calls.is_empty() || !r.job.is_empty()).map(|r| {
        let job = r.job.clone();
        view! {
            <div class="grid gap-1 text-xs">
                <p class="m-0">
                    <strong>{format!("{arm}: {} refused", r.refused_calls.len())}</strong>" · "
                    <button class="text-link underline" on:click=move |_| go.set(Route::Job(job.clone()))>{format!("job {}", r.job)}</button>
                </p>
                <ul class="m-0 pl-5">{r.refused_calls.into_iter().map(|c| view! { <li class="break-all">{c}</li> }).collect_view()}</ul>
            </div>
        }
    })
}

fn request_row(row: EvalRow) -> impl IntoView {
    let label = format!("{}. {}", row.idx + 1, row.request);
    view! {
        <li class="grid gap-2 border-t border-line p-3 first:border-t-0">
            {table(label, row.a.clone(), row.b.clone())}
            <p class="m-0 text-xs text-muted">{if row.differences.is_empty() { "No difference.".to_string() } else { format!("Differs: {}", row.differences.join("; ")) }}</p>
            <div class="grid gap-2 sm:grid-cols-2">{refusals("A", row.a)}{refusals("B", row.b)}</div>
        </li>
    }
}

#[component]
pub fn EvalPage(id: String) -> impl IntoView {
    let go = nav();
    let key = StoredValue::new(id);
    let (view_, error) = live::<EvalView, _>(command::EVAL, move || Id {
        id: key.get_value(),
    });
    let (note, set_note) = signal(String::new());
    let apply = move |arm: &'static str| {
        spawn_local(async move {
            let args = Apply {
                id: key.get_value(),
                arm: arm.into(),
            };
            set_note.set(
                match ipc::call::<(), String>(command::EVAL_APPLY, args).await {
                    Ok(()) => format!(
                        "Arm {} is now the live policy. The audit records this eval.",
                        arm.to_uppercase()
                    ),
                    Err(e) => e.text(),
                },
            );
        });
    };
    view! {
        <button class="mb-2 text-sm text-muted hover:text-ink" on:click=move |_| go.set(Route::Evals)>"← All evals"</button>
        <p class="text-bad" role="alert">{error}</p>
        {move || view_.get().map(|v| {
            let done = v.summary.done == v.summary.runs;
            view! {
                <h1 class="m-0 mb-1 text-2xl font-bold">{v.summary.name.clone()}</h1>
                <p class="m-0 mb-4 text-sm text-muted">{format!(
                    "{} · {} from {} · {} of {} runs done",
                    v.summary.id, v.summary.workspace, &v.summary.base[..7.min(v.summary.base.len())], v.summary.done, v.summary.runs
                )}</p>
                <section class="mb-4 grid gap-1 rounded-xl border border-line bg-panel p-3 text-sm">
                    <p class="m-0"><strong>"A: "</strong>{v.arm_a.join(", ")}</p>
                    <p class="m-0"><strong>"B: "</strong>{v.arm_b.join(", ")}</p>
                </section>
                <section class="mb-4 grid gap-2 rounded-xl border border-line bg-panel p-3">
                    {table("In total".to_string(), Some(v.total_a.clone()), Some(v.total_b.clone()))}
                    <p class="m-0 text-sm">{if v.differences.is_empty() { "No difference in total.".to_string() } else { format!("B differs from A: {}", v.differences.join("; ")) }}</p>
                    <div class="flex flex-wrap items-center gap-2">
                        <button class=BUTTON disabled=!done on:click=move |_| apply("a")>"Keep A"</button>
                        <button class=BUTTON disabled=!done on:click=move |_| apply("b")>"Apply B"</button>
                        <span class="text-xs text-muted">{if done { "Sets the live policy to that arm's rungs." } else { "Wait for every run to finish." }}</span>
                    </div>
                    <p class="m-0 text-sm" role="status">{note}</p>
                </section>
                <ol class="m-0 list-none rounded-xl border border-line bg-panel p-0">
                    {v.rows.into_iter().map(request_row).collect_view()}
                </ol>
            }
        })}
    }
}
