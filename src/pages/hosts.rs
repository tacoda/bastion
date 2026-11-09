//! OpenAI-compatible hosts (PLAN.md K3): each is a model provider of its
//! own. A host's key, if it needs one, is set under Model providers.

use bastion_protocol::{HostView, command};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use super::factory_settings::{BUTTON, read};
use crate::ipc;

const INPUT: &str = "rounded border border-line bg-bg px-2 py-1 text-sm text-ink";

#[derive(Serialize)]
struct Hosts {
    hosts: Vec<HostView>,
}

/// The list with `host` added, or with the host of that id replaced.
fn with_host(mut hosts: Vec<HostView>, host: HostView) -> Vec<HostView> {
    hosts.retain(|h| h.id != host.id);
    hosts.push(host);
    hosts
}

fn without_host(hosts: Vec<HostView>, id: &str) -> Vec<HostView> {
    hosts.into_iter().filter(|h| h.id != id).collect()
}

fn save(hosts: Vec<HostView>, note: WriteSignal<String>, reload: Callback<()>) {
    spawn_local(async move {
        match ipc::call::<(), String>(command::HOSTS_SET, Hosts { hosts }).await {
            Ok(()) => {
                note.set(String::new());
                reload.run(());
            }
            Err(e) => note.set(e.text()),
        }
    });
}

fn field(label: &'static str, value: RwSignal<String>) -> impl IntoView {
    view! {
        <input class=INPUT aria-label=label placeholder=label
            prop:value=value on:input=move |e| value.set(event_target_value(&e)) />
    }
}

/// The fields for a new host, and Add.
fn add_form(
    hosts: ReadSignal<Option<Vec<HostView>>>,
    note: WriteSignal<String>,
    reload: Callback<()>,
) -> impl IntoView {
    let (id, name, url) = (
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
    );
    let add = move |_| {
        let host = HostView {
            id: id.get(),
            name: name.get(),
            api_url: url.get(),
        };
        save(
            with_host(hosts.get().unwrap_or_default(), host),
            note,
            reload,
        );
        [id, name, url].iter().for_each(|s| s.set(String::new()));
    };
    view! {
        <div class="flex flex-wrap items-center gap-2 text-sm">
            {field("Id, such as ollama", id)}
            {field("Name", name)}
            {field("URL, such as http://localhost:11434/v1/chat/completions", url)}
            <button class=BUTTON on:click=add>"Add"</button>
        </div>
    }
}

#[component]
pub fn OpenAiHosts() -> impl IntoView {
    let (hosts, reload) = read::<Vec<HostView>>(command::HOSTS);
    let (note, set_note) = signal(String::new());
    let row = move |h: HostView| {
        let id = h.id.clone();
        let remove = move |_| {
            save(
                without_host(hosts.get().unwrap_or_default(), &id),
                set_note,
                reload,
            )
        };
        view! {
            <li class="flex flex-wrap items-center gap-2">
                <strong>{h.name}</strong>
                <span class="text-muted break-all">{format!("{} · {}", h.id, h.api_url)}</span>
                <button class="text-bad hover:underline" on:click=remove>"Remove"</button>
            </li>
        }
    };
    view! {
        <section class="mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4">
            <h2 class="m-0 text-sm font-semibold text-muted">"OpenAI-compatible hosts"</h2>
            <p class="m-0 text-sm text-muted">"Each host serves its models as a provider of its own. A host that needs a key gets it under Model providers."</p>
            <ul class="m-0 grid list-none gap-1 p-0 text-sm">
                {move || hosts.get().unwrap_or_default().into_iter().map(row).collect_view()}
            </ul>
            {add_form(hosts, set_note, reload)}
            <p class="m-0 text-bad" role="alert">{note}</p>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(id: &str, url: &str) -> HostView {
        HostView {
            id: id.into(),
            name: id.into(),
            api_url: url.into(),
        }
    }

    #[test]
    fn adding_a_host_with_a_listed_id_replaces_it_and_remove_drops_it() {
        let hosts = with_host(
            vec![host("a", "http://1"), host("b", "http://2")],
            host("a", "http://3"),
        );
        assert_eq!(
            hosts.iter().map(|h| h.api_url.as_str()).collect::<Vec<_>>(),
            ["http://2", "http://3"]
        );
        assert_eq!(without_host(hosts, "b"), vec![host("a", "http://3")]);
    }
}
