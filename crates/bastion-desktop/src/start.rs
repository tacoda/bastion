//! What the window does before its first page: start or attach to the
//! `local` Bastion server, and read the servers the owner added. The files
//! sit where the Tauri app kept them, so both clients share them until G4.

use std::path::{Path, PathBuf};

use bastion_protocol::Server;

use crate::local;
use crate::profile::{self, Profiles};
use crate::servers::{self, Keychain, Servers};

/// The checkout this build came from, which holds `worker-compose.yaml`.
// ponytail: a build-time path; packaging the stack replaces it (PLAN.md A13).
const CHECKOUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const IDENTIFIER: &str = "dev.tacoda.bastion";

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Where the app's files live: Application Support on macOS, the XDG
/// directories elsewhere, as Tauri put them.
fn dir(xdg: &str, fallback: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        return home().join("Library/Application Support").join(IDENTIFIER);
    }
    let base = std::env::var_os(xdg).map_or_else(|| home().join(fallback), PathBuf::from);
    base.join(IDENTIFIER)
}

pub fn config_dir() -> PathBuf {
    dir("XDG_CONFIG_HOME", ".config")
}

fn data_dir() -> PathBuf {
    dir("XDG_DATA_HOME", ".local/share")
}

/// The servers this app knows, around the `local` one.
pub fn servers(local: Server) -> Servers {
    Servers::new(
        config_dir().join(servers::FILE_NAME),
        local,
        Box::new(Keychain),
    )
}

/// Start or attach to `local`, then list every server, `local` first.
/// Blocks until `local` answers, so it runs off the window's thread.
pub fn start() -> Result<Vec<Server>, String> {
    let checkout = Path::new(CHECKOUT)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let profiles = Profiles::load_or_create(&config_dir().join(profile::FILE_NAME), checkout)?;
    let local = local::start(profiles.active(), &data_dir().join("server"))?;
    servers(local).all()
}
