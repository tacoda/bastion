//! The shell (PLAN.md A1): the sidebar, the page it shows, and the question on
//! quit while the stack runs.

use bastion_protocol::{StackState, StackStatus, command, event};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::icons;
use crate::ipc::{self, None};
use crate::pages::{
    engine::EnginePage,
    evals::{EvalPage, EvalsPage},
    govern::{AuditPage, CapabilitiesPage, ConstraintsPage},
    job::JobPage,
    jobs::{JobsPage, NewRequestPage},
    settings::SettingsPage,
};
use crate::route::{Nav, Route};

/// The sidebar's pages. A job and New request belong to Jobs.
const PAGES: [(Route, &str, &str); 7] = [
    (Route::Jobs, "Jobs", icons::JOBS),
    (Route::Constraints, "Constraints", icons::CONSTRAINTS),
    (Route::Capabilities, "Capabilities", icons::CAPABILITIES),
    (Route::Audit, "Audit Log", icons::AUDIT),
    (Route::Evals, "Evals", icons::EVALS),
    (Route::Engine, "Engine", icons::ENGINE),
    (Route::Settings, "Settings", icons::SETTINGS),
];

fn section(route: &Route) -> Route {
    match route {
        Route::NewRequest | Route::Job(_) => Route::Jobs,
        Route::Eval(_) => Route::Evals,
        other => other.clone(),
    }
}

/// The stack belongs to the server (D27), so a quit asks only while it runs.
/// Returns what the dialog's buttons do: quit, stopping the stack or not.
fn ask_on_quit(set_asking: WriteSignal<bool>) -> impl Fn(bool) + Copy {
    ipc::listen(event::QUIT_REQUESTED, move |()| {
        spawn_local(async move {
            let running = ipc::call::<StackStatus, String>(command::STACK_STATUS, None {})
                .await
                .is_ok_and(|s| s.state == StackState::Running);
            if running {
                set_asking.set(true);
            } else {
                let _ = ipc::call::<(), String>(command::QUIT, None {}).await;
            }
        });
    });
    move |stop_stack: bool| {
        spawn_local(async move {
            if stop_stack {
                let _ = ipc::call::<StackStatus, String>(command::STACK_STOP, None {}).await;
            }
            let _ = ipc::call::<(), String>(command::QUIT, None {}).await;
        });
    }
}

/// A page about one Bastion server, which shows the server picker (D30).
fn about_one_server(route: &Route) -> bool {
    !matches!(route, Route::Jobs | Route::NewRequest | Route::Job(_))
}

/// Which Bastion server the page shows. Every page reads again on a change.
#[component]
fn ServerPicker() -> impl IntoView {
    let servers = LocalResource::new(ipc::servers);
    let (shown, set_shown) = signal(ipc::current());
    view! {
        <label class="mb-4 flex w-fit items-center gap-2 text-sm text-muted">"Bastion server"
            <select class="rounded border border-line bg-bg px-2 py-1 text-ink" on:change=move |e| {
                let name = event_target_value(&e);
                ipc::choose(&name);
                set_shown.set(name);
            }>
                {move || servers.get().unwrap_or_default().into_iter().map(|s| {
                    let selected = s.name == shown.get();
                    view! { <option value=s.name.clone() selected=selected>{s.name.clone()}</option> }
                }).collect_view()}
            </select>
        </label>
    }
}

#[component]
pub fn App() -> impl IntoView {
    let (route, set_route) = signal(Route::Jobs);
    provide_context::<Nav>(set_route);
    let (asking, set_asking) = signal(false);
    let quit = ask_on_quit(set_asking);

    view! {
        <a href="#main" class="sr-only focus:not-sr-only focus:absolute focus:m-2 focus:rounded focus:bg-primary focus:px-3 focus:py-2 focus:text-white">
            "Skip to content"
        </a>
        <div class="grid min-h-screen grid-cols-[200px_minmax(0,1fr)]">
            <nav aria-label="Main" class="sticky top-0 flex h-screen flex-col gap-1 border-r border-line bg-panel p-3">
                <p class="m-0 mb-3 px-2 text-lg font-extrabold">"Bastion"</p>
                {PAGES.map(|(target, label, glyph)| {
                    let here = target.clone();
                    view! {
                        <button
                            class="flex items-center gap-2 rounded-lg px-2 py-1.5 text-left hover:bg-line aria-[current=page]:bg-primary aria-[current=page]:text-white"
                            aria-current=move || (section(&route.get()) == here).then_some("page")
                            on:click=move |_| set_route.set(target.clone())
                        >{icons::icon(glyph)}{label}</button>
                    }
                })}
            </nav>
            <main id="main" tabindex="-1" class="min-w-0 p-8">
                <Show when=move || about_one_server(&route.get())>
                    <ServerPicker />
                </Show>
                {move || match route.get() {
                    Route::Jobs => view! { <JobsPage /> }.into_any(),
                    Route::NewRequest => view! { <NewRequestPage /> }.into_any(),
                    Route::Job(id) => view! { <JobPage id=id /> }.into_any(),
                    Route::Constraints => view! { <ConstraintsPage /> }.into_any(),
                    Route::Capabilities => view! { <CapabilitiesPage /> }.into_any(),
                    Route::Audit => view! { <AuditPage /> }.into_any(),
                    Route::Evals => view! { <EvalsPage /> }.into_any(),
                    Route::Eval(id) => view! { <EvalPage id=id /> }.into_any(),
                    Route::Engine => view! { <EnginePage /> }.into_any(),
                    Route::Settings => view! { <SettingsPage /> }.into_any(),
                }}
            </main>
        </div>
        <Show when=move || asking.get()>
            <div class="fixed inset-0 grid place-items-center bg-black/60">
                <div role="dialog" aria-modal="true" aria-labelledby="quit-title" class="grid max-w-sm gap-3 rounded-xl border border-line bg-panel p-5">
                    <h2 id="quit-title" class="m-0 text-lg font-bold">"The stack is running"</h2>
                    <p class="m-0 text-muted">"Stop it with the app, or keep it running for the next start to attach to?"</p>
                    <div class="flex flex-wrap gap-2">
                        <button class="rounded-lg bg-primary px-3 py-2 font-semibold text-white hover:bg-primary-hover" on:click=move |_| quit(false)>"Keep it running"</button>
                        <button class="rounded-lg border border-line px-3 py-2 hover:bg-line" on:click=move |_| quit(true)>"Stop the stack"</button>
                        <button class="rounded-lg px-3 py-2 text-muted hover:bg-line" on:click=move |_| set_asking.set(false)>"Cancel"</button>
                    </div>
                </div>
            </div>
        </Show>
    }
}
