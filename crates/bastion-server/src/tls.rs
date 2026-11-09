//! Serving beyond loopback. A Bastion server listens on
//! loopback by default; any other address needs a certificate and key, so
//! tokens never cross a network in the clear.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

/// Where to serve, and the certificate and key when serving TLS.
#[derive(Debug, PartialEq, Eq)]
pub struct Serving {
    pub ip: IpAddr,
    pub tls: Option<(PathBuf, PathBuf)>,
}

/// What `--listen`, `--tls-cert` and `--tls-key` ask for, refused when it
/// would serve tokens in the clear beyond loopback.
pub fn serving(
    listen: Option<&str>,
    cert: Option<&str>,
    key: Option<&str>,
) -> Result<Serving, String> {
    let ip: IpAddr = listen
        .unwrap_or("127.0.0.1")
        .parse()
        .map_err(|_| format!("--listen {}: not an IP address", listen.unwrap_or_default()))?;
    let tls = match (cert, key) {
        (Some(c), Some(k)) => Some((c.into(), k.into())),
        (None, None) => None,
        _ => return Err("--tls-cert and --tls-key go together".into()),
    };
    if tls.is_none() && !ip.is_loopback() {
        return Err(format!(
            "--listen {ip} is beyond loopback: give --tls-cert and --tls-key"
        ));
    }
    Ok(Serving { ip, tls })
}

/// A TLS acceptor for the PEM certificate chain and private key.
pub fn acceptor(cert: &Path, key: &Path) -> Result<TlsAcceptor, String> {
    let chain = CertificateDer::pem_file_iter(cert)
        .and_then(|certs| certs.collect::<Result<Vec<_>, _>>())
        .map_err(|e| format!("{}: {e}", cert.display()))?;
    let key = PrivateKeyDer::from_pem_file(key).map_err(|e| format!("{}: {e}", key.display()))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|b| b.with_no_client_auth().with_single_cert(chain, key))
        .map_err(|e| e.to_string())?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// A listener that hands axum only connections that finished a handshake.
/// Each handshake runs in its own task, so a slow client delays no other.
pub struct TlsListener {
    ready: mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    addr: SocketAddr,
}

/// How long a client gets to finish its handshake.
const HANDSHAKE: Duration = Duration::from_secs(10);

impl TlsListener {
    pub fn start(mut tcp: TcpListener, acceptor: TlsAcceptor) -> std::io::Result<Self> {
        let addr = tcp.local_addr()?;
        let (done, ready) = mpsc::channel(64);
        tokio::spawn(async move {
            loop {
                let (stream, from) = axum::serve::Listener::accept(&mut tcp).await;
                let (acceptor, done) = (acceptor.clone(), done.clone());
                tokio::spawn(async move {
                    match tokio::time::timeout(HANDSHAKE, acceptor.accept(stream)).await {
                        Ok(Ok(tls)) => drop(done.send((tls, from)).await),
                        Ok(Err(e)) => eprintln!("tls handshake from {from}: {e}"),
                        Err(_) => eprintln!("tls handshake from {from}: timed out"),
                    }
                });
            }
        });
        Ok(Self { ready, addr })
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(connection) => connection,
            // The accept task holds a sender for as long as it runs.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_is_the_default_and_needs_no_certificate() {
        let s = serving(None, None, None).unwrap();
        assert_eq!(s.ip, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(s.tls, None);
        assert!(serving(Some("::1"), None, None).is_ok());
    }

    #[test]
    fn beyond_loopback_needs_a_certificate_and_key() {
        let refused = serving(Some("0.0.0.0"), None, None).unwrap_err();
        assert!(refused.contains("beyond loopback"), "{refused}");
        assert!(serving(Some("0.0.0.0"), Some("c.pem"), None).is_err());
        let s = serving(Some("10.0.0.5"), Some("c.pem"), Some("k.pem")).unwrap();
        assert_eq!(s.tls, Some(("c.pem".into(), "k.pem".into())));
        assert!(serving(Some("example.com"), None, None).is_err());
    }

    #[test]
    fn a_missing_certificate_is_named() {
        let Err(e) = acceptor(
            Path::new("/nonexistent/c.pem"),
            Path::new("/nonexistent/k.pem"),
        ) else {
            panic!("no certificate, no acceptor");
        };
        assert!(e.contains("/nonexistent/c.pem"), "{e}");
    }

    /// A self-signed certificate for `localhost`, made with openssl.
    fn certificate(dir: &Path) -> (PathBuf, PathBuf) {
        let (cert, key) = (dir.join("c.pem"), dir.join("k.pem"));
        let made = std::process::Command::new("openssl")
            .args(["req", "-x509", "-newkey", "ec", "-pkeyopt"])
            .args(["ec_paramgen_curve:prime256v1", "-nodes", "-days", "1"])
            .args([
                "-subj",
                "/CN=localhost",
                "-addext",
                "subjectAltName=DNS:localhost",
            ])
            .args(["-addext", "basicConstraints=critical,CA:FALSE"])
            .arg("-keyout")
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .output()
            .unwrap();
        assert!(made.status.success(), "openssl: {made:?}");
        (cert, key)
    }

    #[test]
    fn a_client_that_never_finishes_its_handshake_holds_up_no_other() {
        let dir = std::env::temp_dir().join(format!("bastion-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (cert, key) = certificate(&dir);
        crate::rt::block_on(async {
            let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = tcp.local_addr().unwrap();
            let mut listener = TlsListener::start(tcp, acceptor(&cert, &key).unwrap()).unwrap();
            // Connects and sends nothing.
            let _stalled = TcpStream::connect(addr).await.unwrap();
            let mut roots = rustls::RootCertStore::empty();
            for c in CertificateDer::pem_file_iter(&cert).unwrap() {
                roots.add(c.unwrap()).unwrap();
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth();
            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
            let client = tokio::spawn(async move {
                let tcp = TcpStream::connect(addr).await.unwrap();
                connector.connect(name, tcp).await.unwrap()
            });
            let accepted = axum::serve::Listener::accept(&mut listener);
            tokio::time::timeout(Duration::from_secs(2), accepted)
                .await
                .expect("the second client got through while the first stalled");
            client.await.unwrap();
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
