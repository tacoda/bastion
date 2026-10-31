//! The governance pages (PLAN.md A5): Constraints and Capabilities, each on
//! its ladder with promote and demote, and the Audit Log with its check.

use bastion_protocol::{
    AuditView, CapabilityView, ConstraintView, FactoryCapabilityView, WorkspaceCharterView,
    command, event,
};
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

/// What the three governance pages read, kept current by every push.
#[derive(Clone, Copy)]
struct Governance {
    constraints: ReadSignal<Vec<ConstraintView>>,
    grants: ReadSignal<Vec<CapabilityView>>,
    audit: ReadSignal<Option<AuditView>>,
    error: ReadSignal<String>,
    set_error: WriteSignal<String>,
    reload: Callback<()>,
}

impl Governance {
    /// Report a change's error, or read the page again.
    fn done(self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.set_error.set(String::new());
                self.reload.run(());
            }
            Err(e) => self.set_error.set(e),
        }
    }
}

fn governance() -> Governance {
    let (constraints, set_constraints) = signal(Vec::<ConstraintView>::new());
    let (grants, set_grants) = signal(Vec::<CapabilityView>::new());
    let (audit, set_audit) = signal(None::<AuditView>);
    let (error, set_error) = signal(String::new());
    let reload = Callback::new(move |()| {
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
    });
    ipc::listen(event::JOB_CHANGED, move |_: String| reload.run(()));
    reload.run(());
    Governance {
        constraints,
        grants,
        audit,
        error,
        set_error,
        reload,
    }
}

fn heading(title: &'static str, g: Governance) -> impl IntoView {
    view! {
        <h1 class="m-0 mb-4 text-2xl font-bold">{title}</h1>
        <p class="text-bad" role="alert">{g.error}</p>
    }
}

/// The Constraints page (PLAN.md A5): each rule on its ladder, its history,
/// and the form to add one.
#[component]
pub fn ConstraintsPage() -> impl IntoView {
    let g = governance();
    let step = move |id: String, up: bool| {
        send(
            command::CONSTRAINT_MOVE,
            MoveConstraint { id, up },
            move |r| g.done(r),
        )
    };
    view! {
        {heading("Constraints", g)}
        <p class="m-0 mb-4 max-w-3xl text-sm text-muted">
            "Each rule climbs told, then checked (recorded), then enforced (refused). The charter's rules come from each repository; the harness binds an agent's turn; the factory binds the pipeline."
        </p>
        <CharterRules />
        <ConstraintsList layer="harness" constraints=g.constraints audit=g.audit step=step />
        <ConstraintsList layer="factory" constraints=g.constraints audit=g.audit step=step />
        <AddConstraint added=move |r| g.done(r) />
    }
}

/// The Capabilities page (PLAN.md A5): each grant on its ladder, per stage.
#[component]
pub fn CapabilitiesPage() -> impl IntoView {
    let g = governance();
    let step = move |grant: &CapabilityView, up: bool| {
        let args = MoveCapability {
            stage: grant.stage.clone(),
            function: grant.function.clone(),
            up,
        };
        send(command::CAPABILITY_MOVE, args, move |r| g.done(r))
    };
    view! {
        {heading("Capabilities", g)}
        <CharterAllows />
        <GrantsList grants=g.grants step=step />
        <FactoryCapabilities />
    }
}

/// The Audit Log page (PLAN.md D21): the keyed chain, verified as it is read.
#[component]
pub fn AuditPage() -> impl IntoView {
    let g = governance();
    view! {
        {heading("Audit Log", g)}
        <AuditList audit=g.audit />
    }
}

#[component]
fn ConstraintsList(
    /// The layer this section shows: `harness` or `factory`.
    layer: &'static str,
    constraints: ReadSignal<Vec<ConstraintView>>,
    audit: ReadSignal<Option<AuditView>>,
    step: impl Fn(String, bool) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let move_constraint = step;
    let (open, set_open) = signal(String::new());
    // One constraint's history: every audit entry about it, newest first.
    let history = move |id: String| {
        let entries: Vec<_> = audit
            .get()
            .into_iter()
            .flat_map(|a| a.entries)
            .filter(|e| e.subject == id)
            .collect();
        view! {
            <ol class="m-0 w-full list-none p-0 pl-2 text-xs text-muted">
                {if entries.is_empty() { Some(view! { <li>"No changes recorded yet."</li> }) } else { None }}
                {entries.into_iter().rev().map(|e| view! {
                    <li>{format!("#{} · {} · {} {}", e.seq, ago(e.at), e.kind, e.detail)}</li>
                }).collect_view()}
            </ol>
        }
    };
    view! {
    <section class="mb-4 rounded-xl border border-line bg-panel">
        <h2 class="m-0 border-b border-line px-3 py-2 text-sm font-semibold text-muted">{layer_title(layer)}</h2>
        <ul class="m-0 list-none p-0">
            {move || constraints.get().into_iter().filter(|c| c.layer == layer).map(|c| {
                let (up, down, here) = (c.id.clone(), c.id.clone(), c.id.clone());
                let shown = { let id = c.id.clone(); move || open.get() == id };
                let (shown_for_list, toggle_id) = (shown.clone(), c.id.clone());
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
                        <button class=STEP on:click=move |_| set_open.update(|o| *o = if *o == toggle_id { String::new() } else { toggle_id.clone() })>"History"</button>
                        <button class=STEP disabled=bottom on:click=move |_| move_constraint(down.clone(), false)>"Demote"</button>
                        <button class=STEP disabled=top on:click=move |_| move_constraint(up.clone(), true)>"Promote"</button>
                        <Show when=shown_for_list.clone()>{let id = here.clone(); move || history(id.clone())}</Show>
                    </li>
                }
            }).collect_view()}
        </ul>
    </section>
    }
}

#[derive(Serialize)]
struct AddArgs {
    constraint: ConstraintView,
}

/// The form to add an owner's constraint.
#[component]
fn AddConstraint(
    added: impl Fn(Result<(), String>) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let field = |label: &'static str, hint: &'static str, value: RwSignal<String>| {
        view! {
            <label class="grid gap-1 text-xs text-muted">{label}
                <input class="rounded border border-line bg-bg px-2 py-1 text-sm text-ink" placeholder=hint spellcheck="false"
                    prop:value=value on:input=move |e| value.set(event_target_value(&e)) />
            </label>
        }
    };
    let (id, rule, function, contains) = (
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
    );
    let rung = RwSignal::new("told".to_string());
    let submit = move |_| {
        let constraint = ConstraintView {
            id: id.get_untracked(),
            rule: rule.get_untracked(),
            function: function.get_untracked(),
            contains: contains.get_untracked(),
            rung: rung.get_untracked(),
            source: "owner".into(),
            layer: String::new(),
        };
        send(
            command::CONSTRAINT_ADD,
            AddArgs { constraint },
            move |result| {
                if result.is_ok() {
                    for s in [id, rule, function, contains] {
                        s.set(String::new());
                    }
                }
                added(result)
            },
        );
    };
    view! {
        <details class="mb-4 rounded-xl border border-line bg-panel">
            <summary class="cursor-pointer px-3 py-2 text-sm font-semibold text-muted">"Add a constraint"</summary>
            <div class="grid gap-2 p-3 sm:grid-cols-2">
                {field("Id", "no-curl", id)}
                {field("Rule the agent is told", "Do not fetch from the network.", rule)}
                {field("Calls it binds (empty binds none)", "sandbox::exec", function)}
                {field("Words the call holds, in order", "curl", contains)}
                <label class="grid gap-1 text-xs text-muted">"Rung"
                    <select class="rounded border border-line bg-bg px-2 py-1 text-sm text-ink" on:change=move |e| rung.set(event_target_value(&e))>
                        {["told", "checked", "enforced"].map(|r| view! { <option value=r selected=move || rung.get() == r>{r}</option> })}
                    </select>
                </label>
                <div class="flex items-end"><button class=STEP on:click=submit>"Add"</button></div>
            </div>
        </details>
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
            "Harness: what an agent's calls may do per stage: denied, then ask (held for you), then allowed"
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

/// A layer's section title on the Constraints page.
fn layer_title(layer: &str) -> &'static str {
    if layer == "factory" {
        "Factory: rules on the pipeline, checked at review"
    } else {
        "Harness: rules on an agent's calls"
    }
}

const SECTION: &str = "mb-4 rounded-xl border border-line bg-panel";
const SECTION_TITLE: &str = "m-0 border-b border-line px-3 py-2 text-sm font-semibold text-muted";

/// Read a list command once and on every push.
fn listed<T: serde::de::DeserializeOwned + Send + Sync + 'static>(
    cmd: &'static str,
) -> ReadSignal<Vec<T>> {
    let (all, set_all) = signal(Vec::<T>::new());
    let load = move || {
        spawn_local(async move {
            if let Ok(v) = ipc::call::<Vec<T>, String>(cmd, None {}).await {
                set_all.set(v);
            }
        });
    };
    ipc::listen(event::JOB_CHANGED, move |_: String| load());
    load();
    all
}

/// Lines under a label, or nothing when there are none.
fn lines(label: &'static str, items: Vec<String>) -> impl IntoView {
    (!items.is_empty()).then(|| view! {
        <p class="m-0 mt-1 text-xs text-muted">{label}</p>
        <ul class="m-0 pl-5 text-sm">{items.into_iter().map(|i| view! { <li class="break-all">{i}</li> }).collect_view()}</ul>
    })
}

/// Each workspace's charter rules and hooks, read from its repository.
#[component]
fn CharterRules() -> impl IntoView {
    let charters = listed::<WorkspaceCharterView>(command::CHARTERS);
    view! {
        <section class=SECTION>
            <h2 class=SECTION_TITLE>"Charter: each repository's own rules, from .claude/settings.json"</h2>
            <ul class="m-0 list-none p-0">
                {move || charters.get().into_iter().map(|c| view! {
                    <li class="border-t border-line px-3 py-2 first:border-t-0">
                        <strong>{c.workspace.clone()}</strong>
                        {c.files.is_empty().then(|| view! { <span class="text-sm text-muted">" · no charter: the owner's rules apply"</span> })}
                        {lines("Rules (enforced)", c.rules)}
                        {lines("Hooks (exit 2 refuses the call)", c.hooks)}
                    </li>
                }).collect_view()}
            </ul>
        </section>
    }
}

/// Each workspace's charter allow rules.
#[component]
fn CharterAllows() -> impl IntoView {
    let charters = listed::<WorkspaceCharterView>(command::CHARTERS);
    view! {
        <section class=SECTION>
            <h2 class=SECTION_TITLE>"Charter: what each repository allows"</h2>
            <ul class="m-0 list-none p-0">
                {move || charters.get().into_iter().map(|c| {
                    let none = c.allow.is_empty();
                    view! {
                        <li class="border-t border-line px-3 py-2 first:border-t-0">
                            <strong>{c.workspace.clone()}</strong>
                            {none.then(|| view! { <span class="text-sm text-muted">" · allows nothing beyond the harness"</span> })}
                            {lines("Allowed by the repository; the harness's grants still decide", c.allow)}
                        </li>
                    }
                }).collect_view()}
            </ul>
        </section>
    }
}

/// What the factory itself may do, from the settings.
#[component]
fn FactoryCapabilities() -> impl IntoView {
    let all = listed::<FactoryCapabilityView>(command::FACTORY_CAPABILITIES);
    view! {
        <section class=SECTION>
            <h2 class=SECTION_TITLE>"Factory: what the factory may do, as Settings set it"</h2>
            <ul class="m-0 list-none p-0">
                {move || all.get().into_iter().map(|f| view! {
                    <li class="flex flex-wrap items-center gap-3 border-t border-line px-3 py-2 first:border-t-0">
                        <span class=RUNG>{f.rung}</span>
                        <span class="grid flex-1"><span>{f.name}</span><span class="text-xs text-muted">{f.how}</span></span>
                    </li>
                }).collect_view()}
            </ul>
        </section>
    }
}
