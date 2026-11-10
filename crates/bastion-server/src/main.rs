//! `bastion-server`, headless (D27): starts or attaches to the stack in
//! `--checkout`, runs the factory on it, and serves the API: on loopback by
//! default, or on `--listen` over TLS.
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
    ThisMachine.write_secret(&path, made.as_bytes())?;
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
        let app = http::router(server.clone(), &token, &owner);
        let served = serve(&a, app).await;
        // The stack is the server's (D13): it goes down with it.
        match api::stack::stack_stop(&server.stack, &server.machine).await {
            Ok(_) => println!("bastion-server: stopped, and its stack with it"),
            Err(e) => eprintln!("bastion-server: stopped, but the stack did not: {e}"),
        }
        served
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
    let (signalled, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        stopped().await;
        let _ = signalled.send(true);
    });
    let when = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|s| *s).await;
    };
    let shutdown = when(rx.clone());
    let serving = async {
        match &a.serving.tls {
            None => axum::serve(tcp, app).with_graceful_shutdown(shutdown).await,
            Some((cert, key)) => {
                let acceptor = tls::acceptor(cert, key).map_err(std::io::Error::other)?;
                let listener = TlsListener::start(tcp, acceptor)?;
                axum::serve(listener, app)
                    .with_graceful_shutdown(shutdown)
                    .await
            }
        }
    };
    // An event stream stays open, so a graceful stop gets a few seconds.
    let grace = async {
        when(rx).await;
        tokio::time::sleep(GRACE).await;
    };
    tokio::select! {
        served = serving => served.map_err(|e| e.to_string()),
        _ = grace => Ok(()),
    }
}

/// How long open connections get to finish once the server is told to stop.
const GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Ends on Ctrl-C or SIGTERM, whichever comes first.
async fn stopped() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("a SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_token_file_is_made_readable_only_by_its_owner_and_kept() {
        let dir = std::env::temp_dir().join(format!("bastion-token-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let made = super::token(&dir).unwrap();
        let mode = std::fs::metadata(dir.join("token"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(super::token(&dir).unwrap(), made);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
