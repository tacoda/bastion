//! The Settings page: the active profile (A1) and the workspaces (A3). A7
//! adds the rest.

use bastion_protocol::{
    DialsView, LOCAL, PersonView, PipelineDials, Server, TokenView, Workspace, command, event,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc::{self, None};
use crate::pages::factory_settings::{
    Accounts, BUTTON, FactorySettings, ModelProviders, SettingsHistory, read, send,
};

#[derive(Serialize)]
struct AddWorkspace {
    workspace: Workspace,
}

#[derive(Clone, Serialize)]
struct SetDial {
    stage: String,
    dial: String,
}

/// Each step's dial, by pipeline, and the level each pipeline runs at
/// (PLAN.md D33). A step's button switches it between lights-on and dark.
#[component]
fn Oversight() -> impl IntoView {
    let (dials, reload) = read::<DialsView>(command::DIALS);
    let (note, set_note) = signal(String::new());
    let pipeline = move |title: &'static str, p: PipelineDials| {
        let steps = p.steps.into_iter().map(move |s| {
            let on = s.dial == "lights_on";
            let args = SetDial {
                stage: s.stage.clone(),
                dial: if on { "dark" } else { "lights_on" }.into(),
            };
            let look = if on { "text-ink" } else { "text-muted" };
            view! {
                <button class=format!("{BUTTON} {look}") aria-pressed=on.to_string()
                    on:click=move |_| send(command::DIALS_SET, args.clone(), set_note, reload)>
                    {format!("{} · {}", s.stage.replace('_', " "), if on { "lights-on" } else { "dark" })}
                </button>
            }
        });
        view! {
            <div class="grid gap-2">
                <p class="m-0 text-sm text-ink">{format!("{title}: {}", p.level.replace('_', "-"))}</p>
                <div class="flex flex-wrap gap-2">{steps.collect_view()}</div>
            </div>
        }
    };
    view! {
        <section class="mt-4 grid gap-3 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Oversight"</h2>
            <p class="m-0 text-sm text-muted">
                "A lights-on step waits for you to approve its result or send it back. A dark step goes on at once. A pipeline is lights-on when every step is, dim when a step up to the merge is, and dark when none is."
            </p>
            {move || dials.get().map(|d| view! {
                {pipeline("Short pipeline", d.short)}
                {pipeline("Full pipeline", d.full)}
            })}
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
    let (checks, set_checks) = signal(String::new());
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
            checks: checks.get(),
        };
        spawn_local(async move {
            match ipc::call::<(), String>(command::WORKSPACE_ADD, AddWorkspace { workspace }).await
            {
                Ok(()) => {
                    set_name.set(String::new());
                    set_path.set(String::new());
                    set_checks.set(String::new());
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
            <p class="m-0 text-sm text-muted">"The repositories jobs run against on this Bastion server. Each needs its branch; its checks command runs its tests at the checks step."</p>
            <ul class="m-0 grid list-none gap-1 p-0 text-sm">
                {move || workspaces.get().into_iter().map(|w| view! {
                    <li>{w.name}<span class="text-muted">{format!("  {} @ {}", w.path, w.branch)}</span>
                        {(!w.checks.is_empty()).then(|| view! { <code class="ml-2 text-muted">{w.checks.clone()}</code> })}
                    </li>
                }).collect_view()}
            </ul>
            <div class="flex flex-wrap items-end gap-2 text-sm">
                <label class="grid gap-1 text-muted">"Name"<input class=input prop:value=name on:input=move |e| set_name.set(event_target_value(&e)) /></label>
                <label class="grid gap-1 text-muted">"Repository path"<input class=input prop:value=path on:input=move |e| set_path.set(event_target_value(&e)) /></label>
                <label class="grid gap-1 text-muted">"Branch"<input class=input prop:value=branch on:input=move |e| set_branch.set(event_target_value(&e)) /></label>
                <label class="grid gap-1 text-muted">"Checks"<input class=input placeholder="pytest -q" prop:value=checks on:input=move |e| set_checks.set(event_target_value(&e)) /></label>
                <button class="rounded-lg border border-line px-3 py-1.5 hover:bg-line" on:click=add>"Add"</button>
            </div>
            <p class="m-0 text-bad" role="alert">{note}</p>
        </section>
    }
}

#[derive(Serialize)]
struct AddServer {
    server: Server,
}

#[derive(Serialize)]
struct Named {
    name: String,
}

/// The Bastion servers this app knows (D30): `local`, and the ones added
/// here. A token is kept in the Keychain.
#[component]
fn BastionServers() -> impl IntoView {
    let servers = LocalResource::new(ipc::servers);
    let (name, set_name) = signal(String::new());
    let (url, set_url) = signal(String::new());
    let (token, set_token) = signal(String::new());
    let (note, set_note) = signal(String::new());
    let after = move |answer: Result<(), ipc::Failure<String>>| async move {
        match answer {
            Ok(()) => {
                set_note.set(String::new());
                ipc::refresh_servers().await;
                servers.refetch();
            }
            Err(e) => set_note.set(e.text()),
        }
    };
    let add = move |_| {
        let server = Server {
            name: name.get(),
            url: url.get(),
            token: token.get(),
        };
        spawn_local(async move {
            after(ipc::call::<(), String>(command::SERVER_ADD, AddServer { server }).await).await;
        });
    };
    let remove = move |name: String| {
        spawn_local(async move {
            after(ipc::call::<(), String>(command::SERVER_REMOVE, Named { name }).await).await;
        });
    };
    let input = "rounded border border-line bg-bg px-2 py-1 text-ink";
    view! {
        <section class="mb-4 grid gap-3 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Bastion servers"</h2>
            <ul class="m-0 grid list-none gap-1 p-0 text-sm">
                {move || servers.get().unwrap_or_default().into_iter().map(|s| {
                    let n = s.name.clone();
                    view! {
                        <li class="flex items-center gap-3">
                            <span class="font-semibold">{s.name.clone()}</span>
                            <span class="text-muted">{s.url.clone()}</span>
                            {(s.name != LOCAL).then(|| view! {
                                <button class="text-bad hover:underline" on:click=move |_| remove(n.clone())>"Remove"</button>
                            })}
                        </li>
                    }
                }).collect_view()}
            </ul>
            <div class="flex flex-wrap items-end gap-2 text-sm">
                <label class="grid gap-1 text-muted">"Name"<input class=input prop:value=name on:input=move |e| set_name.set(event_target_value(&e)) /></label>
                <label class="grid gap-1 text-muted">"URL"<input class=input placeholder="http://127.0.0.1:49191" prop:value=url on:input=move |e| set_url.set(event_target_value(&e)) /></label>
                <label class="grid gap-1 text-muted">"Token"<input class=input type="password" prop:value=token on:input=move |e| set_token.set(event_target_value(&e)) /></label>
                <button class="rounded-lg border border-line px-3 py-1.5 hover:bg-line" on:click=add>"Add"</button>
            </div>
            <p class="m-0 text-bad" role="alert">{note}</p>
        </section>
    }
}

#[derive(Serialize)]
struct NewPerson {
    person: PersonView,
}

#[derive(Serialize)]
struct RoleChange {
    name: String,
    role: String,
}

#[derive(Serialize)]
struct Issue {
    name: String,
    label: String,
}

#[derive(Serialize)]
struct TokenId {
    id: String,
}

const ROLES: [&str; 3] = ["admin", "member", "viewer"];
const INPUT: &str = "rounded border border-line bg-bg px-2 py-1 text-ink";

/// Who you are here, and, for an admin, the people and their tokens.
/// Whether `me` manages people here: an admin, on a remote Bastion server.
/// `local` listens only on this machine, so nobody else can sign in to it.
fn manages_people(me: Option<&PersonView>) -> bool {
    ipc::current() != LOCAL && me.is_some_and(|p| p.role == "admin")
}

async fn read_people() -> (Option<PersonView>, Vec<PersonView>, Vec<TokenView>) {
    let who = ipc::call::<PersonView, String>(command::ME, None {})
        .await
        .ok();
    if !manages_people(who.as_ref()) {
        return (who, Vec::new(), Vec::new());
    }
    let all = ipc::call::<Vec<PersonView>, String>(command::PEOPLE, None {}).await;
    let issued = ipc::call::<Vec<TokenView>, String>(command::TOKENS, None {}).await;
    (who, all.unwrap_or_default(), issued.unwrap_or_default())
}

/// Issue `name` a token, and say it the one time it can be seen.
async fn issue_token(name: &str) -> Result<String, String> {
    let args = Issue {
        name: name.into(),
        label: "issued in Settings".into(),
    };
    let token = ipc::call::<String, String>(command::TOKEN_ISSUE, args)
        .await
        .map_err(ipc::Failure::text)?;
    Ok(format!("{name}'s new token, shown only now: {token}"))
}

/// A role select, starting at `current`.
fn role_picker(current: String, on_pick: impl Fn(String) + 'static) -> impl IntoView {
    view! {
        <select class=INPUT on:change=move |e| on_pick(event_target_value(&e))>
            {ROLES.map(|r| view! { <option value=r selected={r == current}>{r}</option> })}
        </select>
    }
}

/// One person, their role, and a way to issue them a token.
fn person_row(
    p: PersonView,
    change: impl Fn(String, String) + Copy + 'static,
    issue: impl Fn(String) + Copy + 'static,
) -> impl IntoView {
    let (to_role, for_token) = (p.name.clone(), p.name.clone());
    view! {
        <li class="flex flex-wrap items-center gap-3">
            <span class="font-semibold">{p.name.clone()}</span>
            {role_picker(p.role.clone(), move |r| change(to_role.clone(), r))}
            <button class="text-link hover:underline" on:click=move |_| issue(for_token.clone())>"Issue a token"</button>
        </li>
    }
}

/// One token, by its id, with a way to revoke it.
fn token_row(t: TokenView, revoke: impl Fn(String) + Copy + 'static) -> impl IntoView {
    let id = t.id.clone();
    view! {
        <li class="flex flex-wrap items-center gap-3">
            <code>{t.id}</code>
            <span>{t.person}</span>
            <span class="text-muted">{t.label}</span>
            <button class="text-bad hover:underline" on:click=move |_| revoke(id.clone())>"Revoke"</button>
        </li>
    }
}

/// The people on this Bastion server and their tokens (D31). Only an admin
/// sees it; a token is shown once, when it is issued.
/// The form that adds a person; `done` hears how it went.
#[component]
fn AddPerson(done: Callback<Result<(), ipc::Failure<String>>>) -> impl IntoView {
    let (name, set_name) = signal(String::new());
    let (role, set_role) = signal(String::from("member"));
    let add = move |_| {
        let person = PersonView {
            name: name.get(),
            role: role.get(),
        };
        spawn_local(async move {
            done.run(ipc::call::<(), String>(command::PERSON_ADD, NewPerson { person }).await);
            set_name.set(String::new());
        });
    };
    view! {
        <div class="flex flex-wrap items-end gap-2 text-sm">
            <label class="grid gap-1 text-muted">"Name"<input class=INPUT prop:value=name on:input=move |e| set_name.set(event_target_value(&e)) /></label>
            <label class="grid gap-1 text-muted">"Role"{role_picker("member".into(), move |r| set_role.set(r))}</label>
            <button class="rounded-lg border border-line px-3 py-1.5 hover:bg-line" on:click=add>"Add"</button>
        </div>
    }
}

#[component]
fn People() -> impl IntoView {
    let (me, set_me) = signal(None::<PersonView>);
    let (people, set_people) = signal(Vec::<PersonView>::new());
    let (tokens, set_tokens) = signal(Vec::<TokenView>::new());
    let (note, set_note) = signal(String::new());
    let (issued, set_issued) = signal(String::new());
    let load = move || {
        spawn_local(async move {
            let (who, all, issued) = read_people().await;
            set_me.set(who);
            set_people.set(all);
            set_tokens.set(issued);
        });
    };
    ipc::listen(event::JOB_CHANGED, move |id: String| {
        if id.is_empty() {
            load();
        }
    });
    load();
    let done = move |answer: Result<(), ipc::Failure<String>>| match answer {
        Ok(()) => {
            set_note.set(String::new());
            load();
        }
        Err(e) => set_note.set(e.text()),
    };
    let change = move |name: String, role: String| {
        spawn_local(async move {
            done(ipc::call::<(), String>(command::PERSON_ROLE, RoleChange { name, role }).await);
        });
    };
    let issue = move |name: String| {
        spawn_local(async move {
            match issue_token(&name).await {
                Ok(shown) => {
                    set_issued.set(shown);
                    load();
                }
                Err(e) => set_note.set(e),
            }
        });
    };
    let revoke = move |id: String| {
        spawn_local(async move {
            done(ipc::call::<(), String>(command::TOKEN_REVOKE, TokenId { id }).await);
        });
    };
    view! {
        <section class="mb-4 grid gap-3 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"People"</h2>
            <p class="m-0 text-sm text-muted">{move || me.get().map(|p| format!("You are {}, a {} on this Bastion server.", p.name, p.role))}</p>
            <Show when=move || manages_people(me.get().as_ref())>
                <ul class="m-0 grid list-none gap-1 p-0 text-sm">
                    {move || people.get().into_iter().map(|p| person_row(p, change, issue)).collect_view()}
                </ul>
                <AddPerson done=Callback::new(done) />
                <p class="m-0 break-all text-sm text-ok" role="status">{issued}</p>
                <h3 class="m-0 text-sm font-semibold text-muted">"Tokens"</h3>
                <ul class="m-0 grid list-none gap-1 p-0 text-sm">
                    {move || tokens.get().into_iter().map(|t| token_row(t, revoke)).collect_view()}
                </ul>
            </Show>
            <p class="m-0 text-bad" role="alert">{note}</p>
        </section>
    }
}

#[component]
pub fn SettingsPage() -> impl IntoView {
    view! {
        <h1 class="mt-0 text-2xl font-bold">"Settings"</h1>
        <BastionServers />
        <People />
        <ModelProviders />
        <super::hosts::OpenAiHosts />
        <FactorySettings />
        <Oversight />
        <SettingsHistory />
        <Workspaces />
        <Accounts />
    }
}
