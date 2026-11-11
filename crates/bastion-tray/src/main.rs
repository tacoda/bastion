//! Bastion's tray icon (F9): what the local Bastion server is doing while
//! the window is closed, and a few things to do about it: approve a job
//! that waits, pause or open intake, open the window. One runs at a time;
//! the window starts it, and it outlives the window.

mod menu;
mod notify;
mod worker;

use std::collections::HashMap;
use std::fs::File;

use menu::{Action, Snapshot};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// The castle, white, as 36 x 36 RGBA pixels.
const ICON: &[u8] = include_bytes!("../../../assets/icon/tray-36.rgba");

pub enum UserEvent {
    Snapshot(Snapshot),
    Menu(MenuEvent),
}

fn main() {
    // One tray at a time: a second exits, so the window can start it freely.
    let Some(_lock) = single() else {
        return;
    };
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Menu(e));
    }));
    let (to_worker, actions) = tokio::sync::mpsc::unbounded_channel();
    worker::spawn(event_loop.create_proxy(), actions);
    let mut tray: Option<TrayIcon> = None;
    let mut picks: HashMap<MenuId, Action> = HashMap::new();
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            // macOS wants the icon made once the loop runs.
            Event::NewEvents(StartCause::Init) => {
                let (built, ids) = build_menu(&Snapshot::default());
                picks = ids;
                tray = make_tray(built);
            }
            Event::UserEvent(UserEvent::Snapshot(s)) => {
                let (built, ids) = build_menu(&s);
                picks = ids;
                if let Some(t) = &tray {
                    t.set_menu(Some(Box::new(built)));
                    t.set_title(menu::badge(&s));
                }
            }
            Event::UserEvent(UserEvent::Menu(e)) => match picks.get(e.id()).cloned() {
                Some(Action::Quit) => *flow = ControlFlow::Exit,
                Some(Action::OpenApp) => open_app(),
                Some(action) => drop(to_worker.send(action)),
                None => {}
            },
            _ => {}
        }
    });
}

/// The lock that keeps a second tray from running, held while this one runs.
fn single() -> Option<File> {
    let dir = bastion_client::start::config_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let lock = File::create(dir.join("tray.lock")).ok()?;
    lock.try_lock().ok()?;
    Some(lock)
}

fn make_tray(menu: Menu) -> Option<TrayIcon> {
    let icon = Icon::from_rgba(ICON.to_vec(), 36, 36).ok()?;
    let built = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("Bastion")
        .with_menu(Box::new(menu))
        .build();
    built.map_err(|e| eprintln!("no tray icon: {e}")).ok()
}

/// The native menu for a snapshot, and what each item's id does.
fn build_menu(s: &Snapshot) -> (Menu, HashMap<MenuId, Action>) {
    let built = Menu::new();
    let mut picks = HashMap::new();
    for (n, entry) in menu::entries(s).into_iter().enumerate() {
        let added = match entry {
            None => built.append(&PredefinedMenuItem::separator()),
            Some(e) => {
                let id = MenuId::new(format!("item-{n}"));
                let item = MenuItem::with_id(id.clone(), &e.text, e.action.is_some(), None);
                picks.extend(e.action.map(|a| (id, a)));
                built.append(&item)
            }
        };
        if let Err(e) = added {
            eprintln!("a menu line left out: {e}");
        }
    }
    (built, picks)
}

/// The window, from beside this program.
// ponytail: starts another window if one is open; ask the open one to show
// itself when the window keeps a single instance.
fn open_app() {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("bastion-desktop")));
    let Some(app) = beside else {
        return;
    };
    if let Err(e) = std::process::Command::new(&app).spawn() {
        notify::show(&format!("Bastion could not open {}: {e}", app.display()));
    }
}
