//! The tray's link to the local Bastion server, on a thread of its own:
//! it follows the server's pushes, reads a snapshot after each change,
//! shows each notice, and carries out what the menu asks.

use bastion_client::events::{self, Feed};
use bastion_protocol::api;
use bastion_protocol::{Change, Server};
use futures::StreamExt;
use tao::event_loop::EventLoopProxy;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::menu::{Action, Snapshot};
use crate::{UserEvent, notify};

/// Run the link on a thread of its own until the tray stops.
pub fn spawn(proxy: EventLoopProxy<UserEvent>, actions: UnboundedReceiver<Action>) {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("a tokio runtime");
        runtime.block_on(run(proxy, actions));
    });
}

async fn run(proxy: EventLoopProxy<UserEvent>, mut actions: UnboundedReceiver<Action>) {
    let server = attach(&proxy).await;
    let (out, mut feed) = futures::channel::mpsc::channel(64);
    tokio::spawn(events::follow(server.clone(), out));
    let http = bastion_client::client::http();
    loop {
        tokio::select! {
            Some((_, push)) = feed.next() => on_push(&http, &server, &proxy, push).await,
            Some(action) = actions.recv() => on_action(&http, &server, &proxy, action).await,
            else => return,
        }
    }
}

/// The local server, attached to or started as the window would, tried
/// again every five seconds until it answers.
async fn attach(proxy: &EventLoopProxy<UserEvent>) -> Server {
    loop {
        let started = tokio::task::spawn_blocking(bastion_client::start::start).await;
        if let Ok(Ok(server)) = started {
            return server;
        }
        tell(proxy, Snapshot::default());
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn on_action(
    http: &reqwest::Client,
    server: &Server,
    proxy: &EventLoopProxy<UserEvent>,
    action: Action,
) {
    if let Err(e) = act(http, server, action).await {
        notify::show(&format!("Bastion could not do that: {e}"));
    }
    tell(proxy, read(http, server).await);
}

async fn on_push(
    http: &reqwest::Client,
    server: &Server,
    proxy: &EventLoopProxy<UserEvent>,
    push: Feed,
) {
    match push {
        Feed::Down => tell(proxy, Snapshot::default()),
        Feed::Change(Change::Notice { text, .. }) => {
            notify::show(&text);
            tell(proxy, read(http, server).await);
        }
        Feed::Change(Change::Engine { .. } | Change::Console) => {}
        _ => tell(proxy, read(http, server).await),
    }
}

fn tell(proxy: &EventLoopProxy<UserEvent>, snapshot: Snapshot) {
    let _ = proxy.send_event(UserEvent::Snapshot(snapshot));
}

/// What the server is doing now; a server that does not answer is down.
async fn read(http: &reqwest::Client, server: &Server) -> Snapshot {
    use bastion_client::client::send;
    let (Ok(jobs), Ok(settings)) = (
        send(http, server, &api::Jobs).await,
        send(http, server, &api::Settings).await,
    ) else {
        return Snapshot::default();
    };
    let waiting = jobs
        .iter()
        .filter(|j| j.state == "needs_you")
        .cloned()
        .collect();
    let running = jobs.iter().filter(|j| j.state == "running").count();
    Snapshot {
        up: true,
        waiting,
        running,
        intake_paused: settings.intake_paused,
    }
}

/// Carry out what the menu asked of the server.
async fn act(http: &reqwest::Client, server: &Server, action: Action) -> Result<(), String> {
    use bastion_client::client::send;
    match action {
        Action::Approve(id) => send(http, server, &api::JobApprove { id }).await,
        Action::Intake { paused } => {
            let mut settings = send(http, server, &api::Settings).await?;
            settings.intake_paused = paused;
            send(http, server, &api::SettingsSet(settings)).await
        }
        // The window and quitting are the tray's own, not the server's.
        Action::OpenApp | Action::Quit => Ok(()),
    }
}
