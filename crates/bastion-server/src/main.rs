//! `bastion-server`, headless (D27): starts or attaches to the stack in
//! `--checkout`, runs the factory on it, and serves the API: on loopback by
//! default, or on `--listen` over TLS (P5).
//!
//! `bastion-server [--checkout DIR] [--data DIR] [--port N] [--engine URL] [--owner NAME]
//!                 [--listen IP --tls-cert PEM --tls-key PEM]`

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bastion_server::engine::{Engine, IiiEngine};
use bastion_server::machine::{Machine, ThisMachine};
use bastion_server::server::Server;
use bastion_server::stack::{self, Stack};
use bastion_server::tls::{self, Serving, TlsListener};
use bastion_server::{api, http, rt};

struct Args {
    /// Who started the server: its admin (D31).
    owner: Option<String>,
    checkout: PathBuf,
    data: PathBuf,
    port: u16,
    engine: String,
    serving: Serving,
}

/// Flags as `--name value`, each with its default.
// ponytail: flags read by hand; a parser crate when flags grow past these.
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
        owner: flag("--owner"),
        data: flag("--data").map_or_else(|| checkout.join("data/server"), PathBuf::from),
        port: flag("--port").map_or(Ok(49190), |p| p.parse().map_err(|_| format!("--port {p}")))?,
        engine: flag("--engine").unwrap_or_else(|| "ws://127.0.0.1:49160".into()),
        serving: tls::serving(
            flag("--listen").as_deref(),
            flag("--tls-cert").as_deref(),
            flag("--tls-key").as_deref(),
        )?,
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

/// The name in `git config user.name`, or `owner` when it is unset.
fn git_user(machine: &dyn Machine) -> String {
    machine
        .run("git", &["config", "user.name"], None)
        .ok()
        .filter(|out| out.success && !out.text.trim().is_empty())
        .map_or_else(|| "owner".into(), |out| out.text.trim().to_string())
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
        let owner = a
            .owner
            .clone()
            .unwrap_or_else(|| git_user(&*server.machine));
        server.start_factory(seed, a.data.join("bundles"), owner.clone());
        let app = http::router(server, &token, &owner);
        serve(&a, app).await
    })
}

/// Serve `app` where `--listen` says, over TLS when given a certificate.
async fn serve(a: &Args, app: axum::Router) -> Result<(), String> {
    let at = (a.serving.ip, a.port);
    let tcp = tokio::net::TcpListener::bind(at)
        .await
        .map_err(|e| format!("{}:{}: {e}", at.0, at.1))?;
    let scheme = if a.serving.tls.is_some() {
        "https"
    } else {
        "http"
    };
    println!(
        "bastion-server: serving {scheme} on {}:{}, token in {}",
        at.0,
        at.1,
        a.data.join("token").display()
    );
    let served = match &a.serving.tls {
        None => axum::serve(tcp, app).await,
        Some((cert, key)) => {
            let acceptor = tls::acceptor(cert, key)?;
            axum::serve(TlsListener { tcp, acceptor }, app).await
        }
    };
    served.map_err(|e| e.to_string())
}
