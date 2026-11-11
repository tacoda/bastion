//! What the window does before its first page: start or attach to the
//! `local` Bastion server, and read the servers the owner added. The files
//! live in the OS's application directories under `dev.tacoda.bastion`.

use std::path::{Path, PathBuf};

use bastion_protocol::Server;

use crate::local;
use crate::servers::{self, Servers};

/// The checkout a dev build came from, which holds `worker-compose.yaml`.
const CHECKOUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
/// The server a dev build runs, from that checkout.
const DEV_SERVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/debug/bastion-server"
);
const IDENTIFIER: &str = "dev.tacoda.bastion";

/// Where the local server's pieces are (F10): its binary, the stack it runs
/// (`worker-compose.yaml`, the seed, Bastion's own workers) and its data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub server: PathBuf,
    pub stack: PathBuf,
    pub data: PathBuf,
}

/// An installed Bastion keeps its stack in `share/bastion` beside the
/// folder its binaries are in, and its data in the user's data folder; a
/// dev build runs from its checkout, with its data there too.
pub fn layout_from(exe_dir: &Path, data_home: &Path) -> Layout {
    let share = exe_dir.join("../share/bastion");
    if share.join("worker-compose.yaml").exists() {
        return Layout {
            server: exe_dir.join("bastion-server"),
            stack: share,
            data: data_home.join("server"),
        };
    }
    let checkout = PathBuf::from(CHECKOUT);
    Layout {
        server: PathBuf::from(DEV_SERVER),
        data: checkout.join("data/server"),
        stack: checkout,
    }
}

/// This program's layout, from where it runs.
pub fn layout() -> Layout {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    layout_from(&exe_dir, &dir("XDG_DATA_HOME", ".local/share"))
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Where the client's files live: Application Support on macOS, the XDG
/// directories elsewhere.
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

/// The servers this client knows, around the `local` one.
pub fn servers(local: Server) -> Servers {
    Servers::new(
        config_dir().join(servers::FILE_NAME),
        local,
        servers::vault(config_dir().join("tokens")),
    )
}

/// Start or attach to `local`. Blocks until it answers, so it runs off the
/// window's thread.
pub fn start() -> Result<Server, String> {
    let mut at = layout();
    at.stack = at
        .stack
        .canonicalize()
        .map_err(|e| format!("{}: {e}", at.stack.display()))?;
    local::start(&at)
}

/// Every server, `local` first. The added servers' tokens come from the
/// Keychain, which can take seconds, so the window shows `local` first.
pub fn every(local: Server) -> Result<Vec<Server>, String> {
    servers(local).all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installed_bastion_runs_its_shared_stack_and_a_dev_build_its_checkout() {
        let root = std::env::temp_dir().join(format!("bastion-layout-{}", std::process::id()));
        let (bin, share) = (root.join("bin"), root.join("share/bastion"));
        std::fs::create_dir_all(&bin).unwrap();
        let home = Path::new("/home/ada/.local/share/dev.tacoda.bastion");
        let dev = layout_from(&bin, home);
        assert!(
            dev.server.ends_with("target/debug/bastion-server"),
            "{dev:?}"
        );
        std::fs::create_dir_all(&share).unwrap();
        std::fs::write(share.join("worker-compose.yaml"), "workers: {}").unwrap();
        let installed = layout_from(&bin, home);
        assert_eq!(installed.server, bin.join("bastion-server"));
        assert_eq!(installed.stack, bin.join("../share/bastion"));
        assert_eq!(installed.data, home.join("server"));
        let _ = std::fs::remove_dir_all(root);
    }
}
