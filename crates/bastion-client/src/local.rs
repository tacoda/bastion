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
    let data = &at.data;
    if !listening() {
        spawn(at)?;
    }
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

/// Start the server in its own process group, so quitting the client leaves it running.
fn spawn(at: &Layout) -> Result<(), String> {
    let (checkout, data) = (&at.stack, &at.data);
    std::fs::create_dir_all(data).map_err(|e| format!("{}: {e}", data.display()))?;
    let log = File::create(data.join("server.log")).map_err(|e| e.to_string())?;
    let mut command = Command::new(&at.server);
    command
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
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", at.server.display()))
}
