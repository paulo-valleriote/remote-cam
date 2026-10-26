use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll};

use remotecam_protocol::ReasonCode;
use rustls::server::ServerSessionMemoryCache;
use rustls::{ProtocolVersion, ServerConfig};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use crate::certificate::EphemeralCertificate;
use crate::error::{PairingError, classify_io_error};
use crate::material::{MaterialError, PairingMaterial, PairingMaterialRequest};

pub const ALPN_PROTOCOL: &[u8] = b"remotecam/1";

#[derive(Debug)]
pub enum ListenerError {
    Tls(rustls::Error),
    Io(std::io::Error),
}

impl fmt::Display for ListenerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tls(error) => write!(formatter, "TLS configuration failed: {error}"),
            Self::Io(error) => write!(formatter, "listener socket failed: {error}"),
        }
    }
}

impl std::error::Error for ListenerError {}

fn server_config(
    certificate: EphemeralCertificate,
) -> Result<(Arc<ServerConfig>, [u8; 32]), ListenerError> {
    let fingerprint = certificate.fingerprint();
    let (der, key) = certificate.into_parts();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(ListenerError::Tls)?
        .with_no_client_auth()
        .with_single_cert(vec![der], key)
        .map_err(ListenerError::Tls)?;
    config.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
    config.session_storage = ServerSessionMemoryCache::new(0);
    config.send_tls13_tickets = 0;
    Ok((Arc::new(config), fingerprint))
}

pub struct PairingListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
    fingerprint: [u8; 32],
    local_addr: SocketAddr,
    attempts: AtomicU32,
}

impl fmt::Debug for PairingListener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PairingListener")
            .field("local_addr", &self.local_addr)
            .field("fingerprint", &"[elided]")
            .field("attempts", &self.attempts())
            .finish()
    }
}

impl PairingListener {
    pub async fn bind(
        address: SocketAddr,
        certificate: EphemeralCertificate,
    ) -> Result<Self, ListenerError> {
        let (config, fingerprint) = server_config(certificate)?;
        let listener = TcpListener::bind(address)
            .await
            .map_err(ListenerError::Io)?;
        let local_addr = listener.local_addr().map_err(ListenerError::Io)?;
        Ok(Self {
            listener,
            acceptor: TlsAcceptor::from(config),
            fingerprint,
            local_addr,
            attempts: AtomicU32::new(0),
        })
    }

    pub async fn bind_loopback(certificate: EphemeralCertificate) -> Result<Self, ListenerError> {
        Self::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), certificate).await
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    pub fn issue_material(
        &self,
        request: PairingMaterialRequest<'_>,
    ) -> Result<PairingMaterial, MaterialError> {
        PairingMaterial::issue_with_fingerprint(request, self.fingerprint)
    }

    pub fn attempts(&self) -> u32 {
        self.attempts.load(Ordering::SeqCst)
    }

    pub async fn accept(&self) -> Result<ServerChannel, PairingError> {
        let socket = self.accept_socket().await?;
        self.handshake(socket).await
    }

    /// Accepts the next TCP peer without starting TLS, so it can be refused before the handshake.
    pub async fn accept_socket(&self) -> Result<TcpStream, PairingError> {
        let (stream, _peer) = self.listener.accept().await.map_err(|error| {
            PairingError::new(ReasonCode::DesktopUnreachable, error.to_string())
        })?;
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Ok(stream)
    }

    /// Runs the TLS 1.3 handshake with the dedicated ALPN protocol on an accepted socket.
    pub async fn handshake(&self, socket: TcpStream) -> Result<ServerChannel, PairingError> {
        let stream = self
            .acceptor
            .accept(socket)
            .await
            .map_err(|error| PairingError::new(classify_io_error(&error), error.to_string()))?;
        if stream.get_ref().1.alpn_protocol() != Some(ALPN_PROTOCOL) {
            return Err(PairingError::new(
                ReasonCode::TlsHandshakeFailed,
                "the dedicated ALPN protocol was not negotiated",
            ));
        }
        Ok(ServerChannel { inner: stream })
    }
}

/// Carries out a `RejectPeer` effect before the handshake: drops the socket and reports `reason`.
pub fn reject_socket(socket: TcpStream, reason: ReasonCode) -> PairingError {
    drop(socket);
    PairingError::new(reason, "peer refused before the handshake")
}

pub struct ServerChannel {
    inner: TlsStream<TcpStream>,
}

impl fmt::Debug for ServerChannel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerChannel")
            .field("tls13", &self.is_tls13())
            .field("alpn_protocol", &self.alpn_protocol())
            .finish()
    }
}

impl ServerChannel {
    pub fn is_tls13(&self) -> bool {
        self.inner.get_ref().1.protocol_version() == Some(ProtocolVersion::TLSv1_3)
    }

    pub fn alpn_protocol(&self) -> Option<&[u8]> {
        self.inner.get_ref().1.alpn_protocol()
    }

    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.get_ref().0.peer_addr()
    }

    /// Carries out a `RejectPeer` effect after the handshake: closes the channel and reports `reason`.
    pub async fn reject(mut self, reason: ReasonCode) -> PairingError {
        // The peer is leaving either way, so a failed close_notify changes nothing.
        let _ = self.shutdown().await;
        PairingError::new(reason, "peer refused after the handshake")
    }
}

impl AsyncRead for ServerChannel {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for ServerChannel {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(context, buffer)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

#[cfg(test)]
mod tests {
    use super::{ListenerError, PairingListener, SocketAddr, server_config};
    use crate::certificate::EphemeralCertificate;

    #[test]
    fn listener_errors_name_the_failing_stage() {
        let io = ListenerError::Io(std::io::Error::other("port in use"));
        let tls = ListenerError::Tls(rustls::Error::General("bad configuration".into()));

        assert!(io.to_string().contains("listener socket failed"));
        assert!(tls.to_string().contains("TLS configuration failed"));
    }

    #[tokio::test]
    async fn binds_loopback_and_reports_the_served_fingerprint() {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let fingerprint = certificate.fingerprint();

        let listener = PairingListener::bind_loopback(certificate)
            .await
            .expect("listener");

        assert!(listener.local_addr().ip().is_loopback());
        assert_ne!(listener.local_addr().port(), 0);
        assert_eq!(listener.fingerprint(), fingerprint);
        assert_eq!(listener.attempts(), 0);

        let rendered = format!("{listener:?}");
        assert!(rendered.contains("[elided]"));
        assert!(!rendered.contains(&remotecam_protocol::hex::encode_hex(&fingerprint)));
    }

    #[tokio::test]
    async fn reports_a_port_that_is_already_bound() {
        let first =
            PairingListener::bind_loopback(EphemeralCertificate::generate().expect("certificate"))
                .await
                .expect("listener");
        let second_certificate = EphemeralCertificate::generate().expect("certificate");

        let error = PairingListener::bind(first.local_addr(), second_certificate)
            .await
            .expect_err("a bound port must be refused");

        assert!(error.to_string().contains("listener socket failed"));
    }

    #[tokio::test]
    async fn binds_a_wildcard_address_for_lan_peers() {
        let listener = PairingListener::bind(
            SocketAddr::from(([0, 0, 0, 0], 0)),
            EphemeralCertificate::generate().expect("certificate"),
        )
        .await
        .expect("pairing listener can bind all local interfaces");

        assert!(listener.local_addr().ip().is_unspecified());
        assert_ne!(listener.local_addr().port(), 0);
    }

    #[test]
    fn the_server_configuration_carries_the_certificate_fingerprint() {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let expected = certificate.fingerprint();
        let (_, fingerprint) = server_config(certificate).expect("configuration");

        assert_eq!(fingerprint, expected);
    }
}
