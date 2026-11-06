//! Serving beyond loopback (PLAN.md P5). A Bastion server listens on
//! loopback by default; any other address needs a certificate and key, so
//! tokens never cross a network in the clear.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::net::{TcpListener, TcpStream};
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
pub struct TlsListener {
    pub tcp: TcpListener,
    pub acceptor: TlsAcceptor,
}

/// How long a client gets to finish its handshake.
const HANDSHAKE: Duration = Duration::from_secs(10);

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    // ponytail: handshakes run one at a time, so a slow client delays the
    // next by up to HANDSHAKE; hand them to tasks if a server gets busy.
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (tcp, addr) = axum::serve::Listener::accept(&mut self.tcp).await;
            match tokio::time::timeout(HANDSHAKE, self.acceptor.accept(tcp)).await {
                Ok(Ok(tls)) => return (tls, addr),
                Ok(Err(e)) => eprintln!("tls handshake from {addr}: {e}"),
                Err(_) => eprintln!("tls handshake from {addr}: timed out"),
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.tcp.local_addr()
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
        let Err(e) = acceptor(Path::new("/nonexistent/c.pem"), Path::new("/nonexistent/k.pem"))
        else {
            panic!("no certificate, no acceptor");
        };
        assert!(e.contains("/nonexistent/c.pem"), "{e}");
    }
}
