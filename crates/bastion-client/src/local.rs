//! The `local` Bastion server (D30): a `bastion-server` on this machine, started by the
//! client, or the one already running. It outlives the client, so jobs go on.

use std::fs::File;
use std::net::TcpStream;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use bastion_protocol::{LOCAL, Server};

use crate::start::Layout;

/// The engine the local server's stack runs.
const ENGINE: &str = "ws://127.0.0.1:49160";
/// The port the local server listens on.
pub const PORT: u16 = 49190;
/// How long a start may take before the client gives up on it.
const START_WAIT: Duration = Duration::from_secs(20);

/// Attach to the local server, starting it first if nothing listens on its
/// port. Its data folder holds its token and its log.
pub fn start(at: &Layout) -> Result<Server, String> {
    if !listening() {
        spawn(at)?;
    }
    // A server another build started keeps its token in its own folder.
    let data = &started_with().unwrap_or_else(|| at.data.clone());
    let token = data.join("token");
    let since = Instant::now();
    while !(listening() && token.exists()) {
        if since.elapsed() > START_WAIT {
            return Err(format!(
                "bastion-server did not start within {}s; see {}",
                START_WAIT.as_secs(),
                data.join("server.log").display()
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(Server {
        name: LOCAL.into(),
        url: format!("http://127.0.0.1:{PORT}"),
        token: std::fs::read_to_string(&token)
            .map_err(|e| format!("{}: {e}", token.display()))?
            .trim()
            .into(),
    })
}

fn listening() -> bool {
    TcpStream::connect(("127.0.0.1", PORT)).is_ok()
}

/// The file that says which data folder the running local server uses, so
/// a client of another build (a dev build beside an installed one) finds
/// its token. It holds the folder's path, never the token.
fn pointer() -> std::path::PathBuf {
    crate::start::config_dir().join("local-server")
}

/// The folders a server looks in for `iii` and `git`: `path`, then where
/// they are usually installed. An app opened from Finder or the Dock gets
/// only the system's folders, so `~/.local/bin` and Homebrew's are added.
fn search_path(path: String) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let usual = [
        format!("{home}/.local/bin"),
        "/opt/homebrew/bin".into(),
        "/usr/local/bin".into(),
    ];
    let mut all: Vec<String> = path
        .split(':')
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect();
    for dir in usual {
        if !all.contains(&dir) {
            all.push(dir);
        }
    }
    all.join(":")
}

// ponytail: a pointer left by a server since stopped, and a server started
// another way, mislead an attach; try its token and fall back if refused.
fn started_with() -> Option<std::path::PathBuf> {
    let kept = std::fs::read_to_string(pointer()).ok()?;
    Some(std::path::PathBuf::from(kept.trim())).filter(|p| p.join("token").exists())
}

/// Start the server in its own process group, so quitting the client leaves it running.
fn spawn(at: &Layout) -> Result<(), String> {
    let (checkout, data) = (&at.stack, &at.data);
    std::fs::create_dir_all(data).map_err(|e| format!("{}: {e}", data.display()))?;
    let log = File::create(data.join("server.log")).map_err(|e| e.to_string())?;
    let mut command = Command::new(&at.server);
    command
        .env(
            "PATH",
            search_path(std::env::var("PATH").unwrap_or_default()),
        )
        .args(["--checkout", &checkout.display().to_string()])
        .args(["--data", &data.display().to_string()])
        .args(["--port", &PORT.to_string()])
        .args(["--engine", ENGINE])
        .current_dir(checkout)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let _ = std::fs::create_dir_all(crate::start::config_dir());
    let _ = std::fs::write(pointer(), data.display().to_string());
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", at.server.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_started_from_finder_still_finds_iii() {
        let finder = search_path("/usr/bin:/bin:/usr/sbin:/sbin".into());
        assert!(finder.starts_with("/usr/bin:/bin:/usr/sbin:/sbin:"));
        assert!(finder.contains("/.local/bin") && finder.contains("/opt/homebrew/bin"));
        let shell = search_path("/opt/homebrew/bin:/usr/bin".into());
        assert_eq!(shell.matches("/opt/homebrew/bin").count(), 1, "{shell}");
    }
}
