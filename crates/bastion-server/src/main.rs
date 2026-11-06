//! `bastion-server`, headless (D27): starts or attaches to the stack in
//! `--checkout`, runs the factory on it, and serves the API on loopback.
//!
//! `bastion-server [--checkout DIR] [--data DIR] [--port N] [--engine URL]`

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bastion_server::engine::{Engine, IiiEngine};
use bastion_server::machine::{Machine, ThisMachine};
use bastion_server::server::Server;
use bastion_server::stack::{self, Stack};
use bastion_server::{api, http, rt};

struct Args {
    checkout: PathBuf,
    data: PathBuf,
    port: u16,
    engine: String,
}

/// Flags as `--name value`, each with its default.
// ponytail: four flags read by hand; a parser crate when flags grow past these.
fn args() -> Result<Args, String> {
    let given: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        given
            .iter()
            .position(|a| a == name)
            .and_then(|i| given.get(i + 1).cloned())
    };
    let checkout = flag("--checkout").map_or_else(std::env::current_dir, |d| Ok(d.into()));
    let checkout = checkout.map_err(|e| e.to_string())?;
    Ok(Args {
        data: flag("--data").map_or_else(|| checkout.join("data/server"), PathBuf::from),
        port: flag("--port").map_or(Ok(49190), |p| p.parse().map_err(|_| format!("--port {p}")))?,
        engine: flag("--engine").unwrap_or_else(|| "ws://127.0.0.1:49160".into()),
        checkout,
    })
}

/// The token every request must carry, made on first start and kept in
/// `data/token`, readable by this user only.
fn token(data: &Path) -> Result<String, String> {
    let path = data.join("token");
    if let Ok(kept) = std::fs::read_to_string(&path) {
        return Ok(kept.trim().to_string());
    }
    let made = uuid::Uuid::new_v4().simple().to_string();
    std::fs::write(&path, &made).map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let only_me = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, only_me).map_err(|e| e.to_string())?;
    }
    Ok(made)
}

fn main() -> Result<(), String> {
    let a = args()?;
    std::fs::create_dir_all(&a.data).map_err(|e| format!("{}: {e}", a.data.display()))?;
    let token = token(&a.data)?;
    let seed = std::fs::read_to_string(a.checkout.join("seed/governance.sql"))
        .map_err(|e| format!("seed/governance.sql in {}: {e}", a.checkout.display()))?;
    let stack = Stack {
        checkout: a.checkout.clone(),
        port: stack::port(&a.engine).ok_or(format!("no port in {}", a.engine))?,
        log: a.data.join("compose.log"),
    };
    rt::block_on(async move {
        let engine: Arc<dyn Engine> = Arc::new(IiiEngine::connect(&a.engine));
        let machine: Arc<dyn Machine> = Arc::new(ThisMachine);
        let server = Server::new(engine, machine, stack, a.data.clone())?;
        // Start the stack, or attach to the one already running.
        match api::stack::stack_start(&server.stack, &server.machine, &server.engine).await {
            Ok(status) => println!("stack: {:?}", status.state),
            Err(refused) => eprintln!("stack did not start: {refused:?}"),
        }
        server.start_factory(seed, a.data.join("bundles"));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", a.port))
            .await
            .map_err(|e| format!("port {}: {e}", a.port))?;
        println!(
            "bastion-server: serving on 127.0.0.1:{}, token in {}",
            a.port,
            a.data.join("token").display()
        );
        axum::serve(listener, http::router(server, &token))
            .await
            .map_err(|e| e.to_string())
    })
}
