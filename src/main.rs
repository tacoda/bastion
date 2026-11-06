mod app;
mod icons;
mod ipc;
mod pages;
mod route;
mod spike_n12;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
