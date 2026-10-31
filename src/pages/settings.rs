//! The Settings page (PLAN.md A1): the active profile. A7 adds the rest.

use bastion_protocol::{ActiveProfile, command};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::ipc::{self, None};

#[component]
pub fn SettingsPage() -> impl IntoView {
    let (profile, set_profile) = signal(None::<ActiveProfile>);
    spawn_local(async move {
        if let Ok(p) = ipc::call::<ActiveProfile, String>(command::PROFILE, None {}).await {
            set_profile.set(Some(p));
        }
    });
    let row = |label: &'static str, value: String| {
        view! { <dt class="text-muted">{label}</dt><dd class="m-0 break-all">{value}</dd> }
    };
    view! {
        <h1 class="mt-0 text-2xl font-bold">"Settings"</h1>
        <section class="grid gap-2 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Profile"</h2>
            {move || profile.get().map(|p| view! {
                <dl class="m-0 grid grid-cols-[120px_1fr] gap-x-3 gap-y-1">
                    {row("Name", p.name)}
                    {row("Kind", p.kind)}
                    {row("Engine", p.engine_url)}
                    {row("Checkout", p.checkout)}
                    {row("File", p.file)}
                </dl>
            })}
        </section>
    }
}
