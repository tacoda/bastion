//! Bastion's native client (PLAN.md D37): a new client of the Bastion server,
//! on iced. It reaches a server only through its HTTP API and its events.

mod app;
mod events;
mod icons;
mod notify;
mod pages;
mod route;
mod theme;

// The client library (D38), under the names the pages use.
use bastion_client::{client, servers, start};

use iced::{Size, window};

/// The castle, as 64 x 64 RGBA pixels, from `assets/icon/bastion.svg`.
const ICON: &[u8] = include_bytes!("../../../assets/icon/bastion-64.rgba");

fn main() -> iced::Result {
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
