//! The Engine page (PLAN.md A1): the link, the stack with Start and Stop, and
//! Doctor, then the console (A2).

use bastion_protocol::{
    Check, CheckState, DoctorReport, EngineStatus, Link, StackState, StackStatus, StartRefused,
    command, event,
};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::ipc::{self, Failure, None};
use crate::pages::console::ConsolePanel;

fn describe_link(status: &EngineStatus) -> String {
    match (status.link, status.functions) {
        (Link::Up, Some(n)) => format!("Up at {}, {n} functions.", status.url),
        (Link::Up, None) => format!("Up at {}.", status.url),
        (Link::Connecting, _) => format!("Connecting to {}.", status.url),
        (Link::Reconnecting, _) => format!("Reconnecting to {}.", status.url),
        (Link::Down, _) => format!("Down: {} does not answer.", status.url),
    }
}

fn describe_stack(status: &StackStatus) -> String {
    match &status.state {
        StackState::Stopped => "Stopped.".into(),
        StackState::Running => format!("Running from {}.", status.compose_file),
        StackState::Taken { pid, compose_file } => format!(
            "Port taken by engine pid {pid} ({}).",
            compose_file.as_deref().unwrap_or("not a compose project")
        ),
    }
}

fn describe_refusal(refused: StartRefused) -> String {
    match refused {
        StartRefused::Doctor { .. } => {
            "Doctor found a required check that failed. See below.".into()
        }
        StartRefused::Taken { pid, compose_file } => format!(
            "Not started: engine pid {pid} already holds the port ({}).",
            compose_file.as_deref().unwrap_or("not a compose project")
        ),
        StartRefused::Failed { message } => format!("Not started: {message}"),
    }
}

#[component]
pub fn EnginePage() -> impl IntoView {
    let (link, set_link) = signal(String::from("…"));
    let (stack, set_stack) = signal(None::<StackStatus>);
    let (note, set_note) = signal(String::new());
    let (busy, set_busy) = signal(false);
    let (report, set_report) = signal(None::<DoctorReport>);

    let refresh_stack = move || {
        spawn_local(async move {
            match ipc::call::<StackStatus, String>(command::STACK_STATUS, None {}).await {
                Ok(s) => set_stack.set(Some(s)),
                Err(e) => set_note.set(e.text()),
            }
        });
    };
    let run_doctor = move || {
        spawn_local(async move {
            if let Ok(r) = ipc::call::<DoctorReport, String>(command::DOCTOR, None {}).await {
                set_report.set(Some(r));
            }
        });
    };
    spawn_local(async move {
        if let Ok(s) = ipc::call::<EngineStatus, String>(command::ENGINE_STATUS, None {}).await {
            set_link.set(describe_link(&s));
        }
    });
    ipc::listen(event::ENGINE_CHANGED, move |s: EngineStatus| {
        set_link.set(describe_link(&s));
        refresh_stack();
        run_doctor();
    });
    refresh_stack();
    run_doctor();

    let start = move |_| {
        set_busy.set(true);
        set_note.set("Starting: Doctor first, then the stack.".into());
        spawn_local(async move {
            match ipc::call::<StackStatus, StartRefused>(command::STACK_START, None {}).await {
                Ok(s) => {
                    set_note.set(String::new());
                    set_stack.set(Some(s));
                }
                Err(Failure::Refused(StartRefused::Doctor { report })) => {
                    set_report.set(Some(report.clone()));
                    set_note.set(describe_refusal(StartRefused::Doctor { report }));
                }
                Err(Failure::Refused(r)) => set_note.set(describe_refusal(r)),
                Err(Failure::Broken(e)) => set_note.set(e),
            }
            set_busy.set(false);
        });
    };
    let stop = move |_| {
        set_busy.set(true);
        spawn_local(async move {
            match ipc::call::<StackStatus, String>(command::STACK_STOP, None {}).await {
                Ok(s) => {
                    set_note.set(String::new());
                    set_stack.set(Some(s));
                }
                Err(e) => set_note.set(e.text()),
            }
            set_busy.set(false);
        });
    };
    let running = move || matches!(stack.get().map(|s| s.state), Some(StackState::Running));

    view! {
        <h1 class="mt-0 text-2xl font-bold">"Engine"</h1>
        <section class="mb-4 grid gap-2 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Link"</h2>
            <p class="m-0" data-testid="link">{link}</p>
        </section>
        <section class="mb-4 grid gap-2 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"Stack"</h2>
            <p class="m-0" data-testid="stack">{move || stack.get().map(|s| describe_stack(&s)).unwrap_or_else(|| "…".into())}</p>
            <p class="m-0 text-muted" role="status">{note}</p>
            <div class="flex gap-2">
                <button class="rounded-lg bg-primary px-3 py-2 font-semibold text-white hover:bg-primary-hover disabled:opacity-50"
                    disabled=move || busy.get() || running() on:click=start>"Start"</button>
                <button class="rounded-lg border border-line px-3 py-2 hover:bg-line disabled:opacity-50"
                    disabled=move || busy.get() || !running() on:click=stop>"Stop"</button>
            </div>
        </section>
        <section class="grid gap-2 rounded-xl border border-line bg-panel p-4">
            <div class="flex items-center justify-between">
                <h2 class="m-0 text-sm font-semibold text-muted">"Doctor"</h2>
                <button class="rounded-lg border border-line px-3 py-1 text-sm hover:bg-line" on:click=move |_| run_doctor()>"Run again"</button>
            </div>
            <ul class="m-0 grid list-none gap-2 p-0">
                {move || report.get().map(|r| r.checks.into_iter().map(check_row).collect_view())}
            </ul>
        </section>
        <ConsolePanel />
    }
}

fn check_row(check: Check) -> impl IntoView {
    let (mark, color) = match check.state {
        CheckState::Pass => ("Pass", "text-ok"),
        CheckState::Warn => ("Warn", "text-warn"),
        CheckState::Fail => ("Fail", "text-bad"),
        CheckState::Skipped => ("Skipped", "text-muted"),
    };
    view! {
        <li class="grid grid-cols-[72px_1fr] gap-x-3">
            <span class={format!("font-semibold {color}")}>{mark}</span>
            <span><strong>{check.name}</strong>": "{check.detail}</span>
            {check.fix.map(|fix| view! { <span></span><span class="text-muted">{fix}</span> })}
        </li>
    }
}
