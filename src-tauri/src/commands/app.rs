//! The app's own commands: the Bastion servers it knows, and quit.

use bastion_protocol::Server;
use tauri::{AppHandle, State};

use crate::servers::Servers;

/// Every Bastion server this app knows, `local` first, with their tokens (D30).
#[tauri::command]
pub fn servers(servers: State<'_, Servers>) -> Result<Vec<Server>, String> {
    servers.all()
}

#[tauri::command]
pub fn server_add(servers: State<'_, Servers>, server: Server) -> Result<(), String> {
    servers.add(server)
}

#[tauri::command]
pub fn server_remove(servers: State<'_, Servers>, name: String) -> Result<(), String> {
    servers.remove(&name)
}

/// Quit the app. The server and its stack keep running unless the page
/// stopped the stack first (D27).
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
