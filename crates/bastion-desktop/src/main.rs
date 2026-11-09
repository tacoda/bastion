//! Bastion's native client (PLAN.md D37): a new client of the Bastion server,
//! on iced. It reaches a server only through its HTTP API and its events.

// G2's pages use the rest of the server list, the routes and the theme.
#![allow(dead_code)]

mod app;
mod client;
mod events;
mod icons;
mod local;
mod pages;
mod profile;
mod route;
mod servers;
mod start;
mod theme;

use iced::{Size, window};

fn main() -> iced::Result {
    let window = window::Settings {
        size: Size::new(1280.0, 860.0),
        min_size: Some(Size::new(720.0, 480.0)),
        exit_on_close_request: false,
        ..window::Settings::default()
    };
    iced::application(app::App::new, app::App::update, app::App::view)
        .title("Bastion")
        .subscription(app::App::subscription)
        .theme(|_: &app::App| theme::bastion())
        .default_font(theme::FONT)
        .window(window)
        .run()
}
