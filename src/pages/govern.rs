//! The Governance page (PLAN.md A5): the constraints and grants on their
//! ladders, with promote and demote, and the audit chain with its check.

use bastion_protocol::{AuditView, CapabilityView, ConstraintView, command, event};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc::{self, None};
use crate::route::ago;

#[derive(Serialize)]
struct MoveConstraint {
    id: String,
    up: bool,
}

#[derive(Serialize)]
struct MoveCapability {
    stage: String,
    function: String,
    up: bool,
}

/// Call a command, then report the error or read the page again.
fn send<A: Serialize + 'static>(
    cmd: &'static str,
    args: A,
    done: impl Fn(Result<(), String>) + 'static,
) {
    spawn_local(async move {
        done(
            ipc::call::<(), String>(cmd, args)
                .await
                .map_err(|e| e.text()),
        )
    });
}

const RUNG: &str = "rounded-full border border-line px-2 py-0.5 text-xs";
const STEP: &str =
    "rounded border border-line px-2 py-0.5 text-sm hover:bg-line disabled:opacity-40";

#[component]
pub fn GovernPage() -> impl IntoView {
    let (constraints, set_constraints) = signal(Vec::<ConstraintView>::new());
    let (grants, set_grants) = signal(Vec::<CapabilityView>::new());
    let (audit, set_audit) = signal(None::<AuditView>);
    let (error, set_error) = signal(String::new());
    let reload = move || {
        spawn_local(async move {
            let answers = (
                ipc::call::<Vec<ConstraintView>, String>(command::CONSTRAINTS, None {}).await,
                ipc::call::<Vec<CapabilityView>, String>(command::CAPABILITIES, None {}).await,
                ipc::call::<AuditView, String>(command::AUDIT, None {}).await,
            );
            match answers {
                (Ok(c), Ok(g), Ok(a)) => {
                    set_constraints.set(c);
                    set_grants.set(g);
                    set_audit.set(Some(a));
                }
                (Err(e), ..) | (_, Err(e), _) | (.., Err(e)) => set_error.set(e.text()),
            }
        });
    };
    ipc::listen(event::JOB_CHANGED, move |_: String| reload());
    reload();
    let done = move |result: Result<(), String>| match result {
        Ok(()) => {
            set_error.set(String::new());
            reload();
        }
        Err(e) => set_error.set(e),
    };
    let move_constraint =
        move |id: String, up: bool| send(command::CONSTRAINT_MOVE, MoveConstraint { id, up }, done);
    let move_grant = move |g: &CapabilityView, up: bool| {
        let args = MoveCapability {
            stage: g.stage.clone(),
            function: g.function.clone(),
            up,
        };
        send(command::CAPABILITY_MOVE, args, done)
    };

    view! {
        <h1 class="m-0 mb-4 text-2xl font-bold">"Governance"</h1>
        <p class="text-bad" role="alert">{error}</p>

        <ConstraintsList constraints=constraints step=move_constraint />
        <GrantsList grants=grants step=move_grant />
        <AuditList audit=audit />
    }
}

#[component]
fn ConstraintsList(
    constraints: ReadSignal<Vec<ConstraintView>>,
    step: impl Fn(String, bool) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let move_constraint = step;
    view! {
    <section class="mb-4 rounded-xl border border-line bg-panel">
        <h2 class="m-0 border-b border-line px-3 py-2 text-sm font-semibold text-muted">
            "Constraints: told, then checked (recorded), then enforced (refused)"
        </h2>
        <ul class="m-0 list-none p-0">
            {move || constraints.get().into_iter().map(|c| {
                let (up, down) = (c.id.clone(), c.id.clone());
                let (top, bottom) = (c.rung == "enforced", c.rung == "told");
                let binds = if c.function.is_empty() {
                    "binds no call".to_string()
                } else {
                    format!("binds {} with the words {:?}", c.function, c.contains)
                };
                view! {
                    <li class="flex flex-wrap items-center gap-3 border-t border-line px-3 py-2 first:border-t-0">
                        <span class=RUNG>{c.rung.clone()}</span>
                        <span class="grid flex-1">
                            <span>{c.rule.clone()}</span>
                            <span class="text-xs text-muted">{format!("{} · {} · {binds}", c.id, c.source)}</span>
                        </span>
                        <button class=STEP disabled=bottom on:click=move |_| move_constraint(down.clone(), false)>"Demote"</button>
                        <button class=STEP disabled=top on:click=move |_| move_constraint(up.clone(), true)>"Promote"</button>
                    </li>
                }
            }).collect_view()}
        </ul>
    </section>
    }
}

#[component]
fn GrantsList(
    grants: ReadSignal<Vec<CapabilityView>>,
    step: impl Fn(&CapabilityView, bool) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let move_grant = step;
    view! {
    <section class="mb-4 rounded-xl border border-line bg-panel">
        <h2 class="m-0 border-b border-line px-3 py-2 text-sm font-semibold text-muted">
            "Capabilities: denied, then ask (held for you), then allowed, per stage"
        </h2>
        <ul class="m-0 list-none p-0">
            {move || grants.get().into_iter().map(|g| {
                let (top, bottom) = (g.rung == "allowed", g.rung == "denied");
                let (up, down) = (g.clone(), g.clone());
                view! {
                    <li class="flex flex-wrap items-center gap-3 border-t border-line px-3 py-2 first:border-t-0">
                        <span class=RUNG>{g.rung.clone()}</span>
                        <span class="flex-1"><span class="capitalize">{g.stage.clone()}</span>" · "{g.function.clone()}</span>
                        <button class=STEP disabled=bottom on:click=move |_| move_grant(&down, false)>"Demote"</button>
                        <button class=STEP disabled=top on:click=move |_| move_grant(&up, true)>"Promote"</button>
                    </li>
                }
            }).collect_view()}
        </ul>
    </section>
    }
}

#[component]
fn AuditList(audit: ReadSignal<Option<AuditView>>) -> impl IntoView {
    let (exported, set_exported) = signal(String::new());
    let export = move |_| {
        spawn_local(async move {
            let answer = ipc::call::<String, String>(command::AUDIT_EXPORT, None {}).await;
            set_exported.set(match answer {
                Ok(path) => format!("Written to {path}"),
                Err(e) => e.text(),
            });
        });
    };
    view! {
    <section class="rounded-xl border border-line bg-panel">
        <div class="flex items-center justify-between border-b border-line px-3 py-2">
            <h2 class="m-0 text-sm font-semibold text-muted">"Audit"</h2>
            <button class=STEP on:click=export>"Export"</button>
        </div>
        <p class="m-0 px-3 pt-2 text-xs text-muted break-all" role="status">{exported}</p>
        {move || audit.get().map(|a| {
            let (status, color) = match a.broken_at {
                None => (format!("Verified: all {} entries chain from the first.", a.entries.len()), "text-ok"),
                Some(seq) => (format!("Broken: entry {seq} does not follow from the one before it."), "text-bad"),
            };
            let writer = match (a.pending, a.last_error) {
                (_, Some(e)) => Some((format!("The audit writer is failing: {e}"), "text-bad")),
                (0, None) => None,
                (n, None) => Some((format!("{n} records waiting to be written."), "text-warn")),
            };
            view! {
                <p class={format!("m-0 px-3 py-2 text-sm font-semibold {color}")} role="status">{status}</p>
                {writer.map(|(text, color)| view! { <p class={format!("m-0 px-3 pb-2 text-sm {color}")} role="alert">{text}</p> })}
                <ol class="m-0 max-h-[32rem] list-none overflow-auto p-0 text-sm">
                    {a.entries.into_iter().rev().map(|e| view! {
                        <li class="grid grid-cols-[4rem_6rem_7rem_1fr] gap-2 border-t border-line px-3 py-1.5">
                            <span class="text-muted">{format!("#{}", e.seq)}</span>
                            <span class="text-muted">{ago(e.at)}</span>
                            <span>{e.kind}</span>
                            <span class="break-all">{format!("{} · {}", e.subject, e.detail)}
                                <span class="block text-xs text-muted">{e.hash[..12].to_string()}</span>
                            </span>
                        </li>
                    }).collect_view()}
                </ol>
            }
        })}
    </section>
    }
}
