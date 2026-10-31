//! The shell (PLAN.md A1): the sidebar, the page it shows, and the question on
//! quit while the stack runs.

use bastion_protocol::{ActiveProfile, command, event};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc::{self, None};
use crate::pages::{
    engine::EnginePage,
    floor::{FloorPage, InboxPage},
    settings::SettingsPage,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Floor,
    Inbox,
    Engine,
    Settings,
}

const PAGES: [(Page, &str); 4] = [
    (Page::Floor, "Floor"),
    (Page::Inbox, "Inbox"),
    (Page::Engine, "Engine"),
    (Page::Settings, "Settings"),
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Quit {
    stop_stack: bool,
}

#[component]
pub fn App() -> impl IntoView {
    let (page, set_page) = signal(Page::Floor);
    let (profile, set_profile) = signal(String::new());
    let (asking, set_asking) = signal(false);
    spawn_local(async move {
        if let Ok(p) = ipc::call::<ActiveProfile, String>(command::PROFILE, None {}).await {
            set_profile.set(p.name);
        }
    });
    ipc::listen(event::QUIT_REQUESTED, move |()| set_asking.set(true));
    let quit = move |stop_stack: bool| {
        spawn_local(async move {
            let _ = ipc::call::<(), String>(command::QUIT, Quit { stop_stack }).await;
        });
    };

    view! {
        <a href="#main" class="sr-only focus:not-sr-only focus:absolute focus:m-2 focus:rounded focus:bg-primary focus:px-3 focus:py-2 focus:text-white">
            "Skip to content"
        </a>
        <div class="grid min-h-screen grid-cols-[200px_1fr]">
            <nav aria-label="Main" class="sticky top-0 flex h-screen flex-col gap-1 border-r border-line bg-panel p-3">
                <p class="m-0 mb-3 px-2 text-lg font-extrabold">"Bastion"</p>
                {PAGES.map(|(p, label)| view! {
                    <button
                        class="rounded-lg px-2 py-1.5 text-left hover:bg-line aria-[current=page]:bg-primary aria-[current=page]:text-white"
                        aria-current=move || (page.get() == p).then_some("page")
                        on:click=move |_| set_page.set(p)
                    >{label}</button>
                })}
                <p class="mt-auto px-2 text-xs text-muted">{profile}</p>
            </nav>
            <main id="main" tabindex="-1" class="p-8">
                {move || match page.get() {
                    Page::Floor => view! { <FloorPage /> }.into_any(),
                    Page::Inbox => view! { <InboxPage /> }.into_any(),
                    Page::Engine => view! { <EnginePage /> }.into_any(),
                    Page::Settings => view! { <SettingsPage /> }.into_any(),
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
