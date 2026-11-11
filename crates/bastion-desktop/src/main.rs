//! Bastion's native client (PLAN.md D37): a new client of the Bastion server,
//! on iced. It reaches a server only through its HTTP API and its events.

mod app;
mod dock;
mod events;
mod icons;
mod pages;
mod route;
mod theme;

// The client library (D38), under the names the pages use.
use bastion_client::{client, servers, start};

use iced::{Size, window};

/// The castle, as 64 x 64 RGBA pixels, from `assets/icon/bastion.svg`.
const ICON: &[u8] = include_bytes!("../../../assets/icon/bastion-64.rgba");

/// Start the tray beside this program (F9). It shows the notices, and a
/// second copy exits, so starting it each time is safe.
fn start_tray() {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("bastion-tray")));
    let Some(tray) = beside.filter(|t| t.exists()) else {
        return;
    };
    // What it says goes to a log beside the client's files, so a tray that
    // stops at once leaves its reason.
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(bastion_client::start::config_dir().join("tray.log"));
    let mut command = std::process::Command::new(tray);
    if let Ok(log) = log {
        let err = log.try_clone();
        command.stdout(log);
        command.stderr(err.map_or_else(|_| std::process::Stdio::null(), Into::into));
    }
    if let Err(e) = command.spawn() {
        eprintln!("the tray did not start: {e}");
    }
}

fn main() -> iced::Result {
    start_tray();
    let window = window::Settings {
        size: Size::new(1280.0, 860.0),
        min_size: Some(Size::new(720.0, 480.0)),
        exit_on_close_request: false,
        icon: window::icon::from_rgba(ICON.to_vec(), 64, 64).ok(),
        ..window::Settings::default()
    };
    iced::application(app::App::new, app::App::update, app::App::view)
        .title(app::App::title)
        .subscription(app::App::subscription)
        .theme(|_: &app::App| theme::bastion())
        .default_font(theme::FONT)
        .window(window)
        .run()
}
