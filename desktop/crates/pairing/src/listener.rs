use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use remotecam_protocol::ReasonCode;
use remotecam_session_core::{Clock, MonotonicClock};
use rustls::server::ServerSessionMemoryCache;
use rustls::{ProtocolVersion, ServerConfig};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use crate::active::{SUPERSEDE_BUDGET, close_code_for, send_close};
use crate::certificate::EphemeralCertificate;
use crate::diagnostics::{DiagnosticEntry, DiagnosticLog};
use crate::error::{PairingError, classify_io_error};
use crate::material::{MaterialError, PairingMaterial, PairingMaterialRequest, UnixClock};
use crate::throttle::{Admission, OriginThrottle};

pub const ALPN_PROTOCOL: &[u8] = b"remotecam/1";

const CONSECUTIVE_FAILURE_LIMIT: u32 = 10;

#[derive(Debug)]
pub struct AdmittedSocket(TcpStream);

impl AdmittedSocket {
    fn origin(&self) -> Option<IpAddr> {
        self.0.peer_addr().ok().map(|address| address.ip())
    }
}

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
    consecutive_failures: AtomicU32,
    closed: AtomicBool,
    throttle: OriginThrottle,
    clock: Box<dyn Clock + Send + Sync>,
    diagnostics: Mutex<DiagnosticLog>,
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
            consecutive_failures: AtomicU32::new(0),
            closed: AtomicBool::new(false),
            throttle: OriginThrottle::new(),
            clock: Box::new(MonotonicClock::new()),
            diagnostics: Mutex::new(DiagnosticLog::default()),
        })
    }

    pub async fn bind_loopback(certificate: EphemeralCertificate) -> Result<Self, ListenerError> {
        Self::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), certificate).await
    }

    #[must_use]
    pub fn with_clock(mut self, clock: impl Clock + Send + Sync + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    #[must_use]
    pub fn with_origin_throttle(mut self, throttle: OriginThrottle) -> Self {
        self.throttle = throttle;
        self
    }

    pub fn drain_diagnostics(&self) -> Vec<DiagnosticEntry> {
        self.diagnostics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
    }

    pub fn dropped_diagnostics(&self) -> u64 {
        self.diagnostics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .dropped()
    }

    fn record(&self, entry: DiagnosticEntry) {
        self.diagnostics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
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
        clock: &impl UnixClock,
    ) -> Result<PairingMaterial, MaterialError> {
        PairingMaterial::issue_with_fingerprint(request, self.fingerprint, clock)
    }

    pub fn close(&self) {
        self.close_after(None);
    }

    fn close_after(&self, origin: Option<IpAddr>) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.record(DiagnosticEntry::refusal(ReasonCode::ListenerClosed, origin));
        }
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn refuse_if_closed(
        &self,
        socket: TcpStream,
        origin: Option<IpAddr>,
    ) -> Result<TcpStream, PairingError> {
        if self.is_closed() {
            self.record(DiagnosticEntry::refusal(ReasonCode::ListenerClosed, origin));
            return Err(reject_socket(socket, ReasonCode::ListenerClosed));
        }
        Ok(socket)
    }

    pub fn attempts(&self) -> u32 {
        self.attempts.load(Ordering::SeqCst)
    }

    pub async fn accept(&self) -> Result<ServerChannel, PairingError> {
        self.accept_admitted(|| None).await
    }

    pub async fn accept_admitted(
        &self,
        refusal: impl FnOnce() -> Option<ReasonCode>,
    ) -> Result<ServerChannel, PairingError> {
        let socket = self.accept_socket().await?;
        match refusal() {
            Some(reason) => {
                self.record(DiagnosticEntry::refusal(reason, socket.origin()));
                Err(reject_socket(socket.0, reason))
            }
            None => self.handshake(socket).await,
        }
    }

    pub async fn accept_socket(&self) -> Result<AdmittedSocket, PairingError> {
        let (stream, peer) = self.listener.accept().await.map_err(|error| {
            PairingError::new(ReasonCode::DesktopUnreachable, error.to_string())
        })?;
        self.attempts.fetch_add(1, Ordering::SeqCst);
        let origin = peer.ip();
        let stream = self.refuse_if_closed(stream, Some(origin))?;
        match self.throttle.admit(origin, self.clock.now()) {
            Admission::Admitted => Ok(AdmittedSocket(stream)),
            Admission::Refused { blocked_for_millis } => {
                self.record(DiagnosticEntry::throttled(origin, blocked_for_millis));
                Err(reject_socket(stream, ReasonCode::RateLimited))
            }
        }
    }

    pub async fn handshake(&self, socket: AdmittedSocket) -> Result<ServerChannel, PairingError> {
        let origin = socket.origin();
        let socket = self.refuse_if_closed(socket.0, origin)?;
        match self.negotiate(socket).await {
            Ok(channel) => {
                self.consecutive_failures.store(0, Ordering::SeqCst);
                Ok(channel)
            }
            Err(error) => {
                self.record(DiagnosticEntry::refusal(error.code(), origin));
                self.record_failure(origin);
                Err(error)
            }
        }
    }

    async fn negotiate(&self, socket: TcpStream) -> Result<ServerChannel, PairingError> {
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

    fn record_failure(&self, origin: Option<IpAddr>) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        if failures >= CONSECUTIVE_FAILURE_LIMIT {
            self.close_after(origin);
        }
    }
}

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

    pub async fn reject(mut self, reason: ReasonCode) -> PairingError {
        if let Some(code) = close_code_for(reason) {
            let _ = timeout(SUPERSEDE_BUDGET, send_close(&mut self, code)).await;
        }
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
