//! The pairing listener: TLS 1.3 only, ephemeral certificate, single-use token.
//!
//! The listener owns the throttling state and the token store, so a scenario
//! cannot accept an unauthenticated connection by bypassing a layer: the
//! session handle returned by [`PairingServer::accept_one`] is the only way to
//! start media.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use remotecam_protocol::{ReasonCode, SignalingMessage, parse_signaling};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::ServerConfig;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::Message;

use crate::certificate::EphemeralCertificate;
use crate::client::classify_io_error;
use crate::fingerprint_exchange::{AuthenticatedSession, MediaFingerprints, verify_fingerprints};
use crate::throttling::{AttemptPermit, CONSECUTIVE_FAILURE_LIMIT, ThrottleDecision, Throttling};
use crate::token::TokenStore;

/// How long a TLS or WebSocket handshake may take.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// Which source addresses may reach the pairing listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourcePolicy {
    /// Whether loopback is accepted. The reference run uses private LAN
    /// addresses; the harness uses loopback for the valid scenario and turns
    /// this off for the non-private-source scenario.
    pub allow_loopback: bool,
    /// When set, the source must fall inside this sub-network.
    pub expected_subnet: Option<Subnet>,
}

/// An IPv4 or IPv6 sub-network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subnet {
    network: IpAddr,
    prefix: u8,
}

impl Subnet {
    /// Builds a sub-network from an address and prefix length.
    pub fn new(network: IpAddr, prefix: u8) -> Option<Self> {
        let limit = match network {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        (prefix <= limit).then_some(Self { network, prefix })
    }

    /// Whether `address` belongs to this sub-network.
    pub fn contains(&self, address: IpAddr) -> bool {
        match (self.network, address) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - u32::from(self.prefix))
                };
                (u32::from(network) & mask) == (u32::from(address) & mask)
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u128::MAX << (128 - u32::from(self.prefix))
                };
                (u128::from(network) & mask) == (u128::from(address) & mask)
            }
            _ => false,
        }
    }
}

/// Whether an address is inside a permitted local range.
pub fn is_permitted_source(address: IpAddr, policy: &SourcePolicy) -> Result<(), ReasonCode> {
    let private = match address {
        IpAddr::V4(address) => address.is_private() || address.is_link_local(),
        IpAddr::V6(address) => address.is_unique_local() || address.is_unicast_link_local(),
    };
    let loopback = policy.allow_loopback && address.is_loopback();
    if !private && !loopback {
        return Err(ReasonCode::SourceAddressNotPrivate);
    }
    if let Some(subnet) = policy.expected_subnet
        && !subnet.contains(address)
    {
        return Err(ReasonCode::SourceSubnetMismatch);
    }
    Ok(())
}

/// Why an attempt did not produce an authenticated session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingRejection {
    /// Closed reason code to report.
    pub code: ReasonCode,
    /// The origin address, when the connection got that far.
    pub origin: Option<String>,
    /// The stage that rejected the attempt, for the trace.
    pub stage: &'static str,
    /// English detail for the local diagnostic.
    pub detail: String,
}

impl PairingRejection {
    fn new(code: ReasonCode, stage: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            origin: None,
            stage,
            detail: detail.into(),
        }
    }

    fn at_origin(mut self, origin: &str) -> Self {
        self.origin = Some(origin.to_owned());
        self
    }
}

impl fmt::Display for PairingRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at {}", self.code.as_str(), self.stage)
    }
}

impl std::error::Error for PairingRejection {}

/// One accepted pairing.
#[derive(Debug)]
pub struct AcceptedPairing {
    /// The authenticated session, which carries the media fingerprints.
    pub session: AuthenticatedSession,
    /// Time from the pairing start to the authenticated channel.
    pub established_channel_ms: u64,
    /// The throttling decision that allowed this attempt.
    pub permit_sequence: u32,
}

/// Failure while building the listener.
#[derive(Debug)]
pub enum ServerError {
    /// The TLS configuration was rejected.
    Tls(rustls::Error),
}

impl fmt::Display for ServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tls(error) => write!(formatter, "TLS configuration failed: {error}"),
        }
    }
}

impl std::error::Error for ServerError {}

impl From<rustls::Error> for ServerError {
    fn from(error: rustls::Error) -> Self {
        Self::Tls(error)
    }
}

/// Builds a TLS 1.3-only server configuration.
///
/// Session resumption is disabled and no client certificate is requested: the
/// only trust decision is the pinned server certificate on the mobile side.
pub fn server_config(
    certificate: &EphemeralCertificate,
    chain: Option<Vec<CertificateDer<'static>>>,
) -> Result<ServerConfig, ServerError> {
    let mut certificates = vec![certificate.der().clone()];
    if let Some(chain) = chain {
        certificates.extend(chain);
    }
    server_config_with_credentials(certificates, certificate.key())
}

/// Builds a TLS 1.3-only server configuration from raw credentials.
///
/// `#[doc(hidden)]`: the gate uses a deliberately mismatched key to prove that
/// the client verifies the `CertificateVerify` signature instead of merely
/// accepting the pinned certificate. It is never used on a product path.
#[doc(hidden)]
pub fn server_config_with_credentials(
    certificates: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<ServerConfig, ServerError> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(certificates, key)?;
    config.session_storage = rustls::server::ServerSessionMemoryCache::new(0);
    config.send_half_rtt_data = false;
    Ok(config)
}

/// The pairing listener.
pub struct PairingServer {
    listener: TcpListener,
    acceptor: TlsAcceptor,
    token_store: Arc<TokenStore>,
    expected_fingerprints: MediaFingerprints,
    source_policy: SourcePolicy,
    throttling: Throttling,
    session_id: [u8; 16],
    epoch: u32,
    started: Instant,
    last_origin: Option<SocketAddr>,
}

impl fmt::Debug for PairingServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PairingServer")
            .field("local_addr", &self.local_addr().ok())
            .field("listener_closed", &self.throttling.listener_closed())
            .finish()
    }
}

impl PairingServer {
    /// Creates a listener bound to `listener`.
    ///
    // Eight arguments is the honest shape here: each one is a distinct trust
    // input (certificate, chain, token store, expected fingerprints, source
    // policy, session identity) and bundling them into a struct would only move
    // the same list one line down.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        listener: TcpListener,
        certificate: &EphemeralCertificate,
        chain: Option<Vec<CertificateDer<'static>>>,
        token_store: Arc<TokenStore>,
        expected_fingerprints: MediaFingerprints,
        source_policy: SourcePolicy,
        session_id: [u8; 16],
        epoch: u32,
    ) -> Result<Self, ServerError> {
        let config = server_config(certificate, chain)?;
        Ok(Self {
            listener,
            acceptor: TlsAcceptor::from(Arc::new(config)),
            token_store,
            expected_fingerprints,
            source_policy,
            throttling: Throttling::new(),
            session_id,
            epoch,
            started: Instant::now(),
            last_origin: None,
        })
    }

    /// The bound address, useful when the listener was created on port 0.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// The throttling decisions recorded so far.
    pub fn decisions(&self) -> &[ThrottleDecision] {
        self.throttling.decisions()
    }

    /// The throttling state, for evidence assembly.
    pub fn throttling(&self) -> &Throttling {
        &self.throttling
    }

    /// Whether the listener has closed itself.
    pub fn listener_closed(&self) -> bool {
        self.throttling.listener_closed()
    }

    /// The token store, so a scenario can issue, expire or replay tokens.
    pub fn token_store(&self) -> &Arc<TokenStore> {
        &self.token_store
    }

    /// The source address of the last accepted TCP connection.
    pub fn last_origin(&self) -> Option<SocketAddr> {
        self.last_origin
    }

    /// Milliseconds since the listener started, on the same clock the token
    /// store uses for expiry.
    pub fn now(&self) -> Duration {
        self.started.elapsed()
    }

    /// Accepts one connection and runs it through every authentication stage.
    pub async fn accept_one(&mut self) -> Result<AcceptedPairing, PairingRejection> {
        let started = Instant::now();
        let (stream, peer) = self.listener.accept().await.map_err(|error| {
            PairingRejection::new(ReasonCode::DesktopUnreachable, "accept", error.to_string())
        })?;
        self.last_origin = Some(peer);
        let origin = peer.ip().to_string();
        let now_ms = elapsed_ms(self.started);

        match self.throttling.begin_attempt(&origin, now_ms) {
            AttemptPermit::Refused(decision) => {
                let closure = self.throttling.record_failure(&origin, now_ms);
                let code = closure.as_ref().map_or(
                    decision.reason_code.unwrap_or(ReasonCode::RateLimited),
                    |_| ReasonCode::ListenerClosed,
                );
                Err(PairingRejection::new(
                    code,
                    "throttle",
                    format!(
                        "attempt {} refused by the per-origin limit",
                        decision.sequence
                    ),
                )
                .at_origin(&origin))
            }
            AttemptPermit::Allowed { sequence } => {
                if let Err(code) = is_permitted_source(peer.ip(), &self.source_policy) {
                    self.throttling.record_failure(&origin, now_ms);
                    return Err(PairingRejection::new(
                        code,
                        "source",
                        "source address is outside the permitted range",
                    )
                    .at_origin(&origin));
                }

                let tls = match timeout(HANDSHAKE_TIMEOUT, self.acceptor.accept(stream)).await {
                    Ok(Ok(tls)) => tls,
                    Ok(Err(error)) => {
                        let code = classify_io_error(&error);
                        self.throttling.record_failure(&origin, now_ms);
                        return Err(PairingRejection::new(code, "tls", error.to_string())
                            .at_origin(&origin));
                    }
                    Err(_) => {
                        self.throttling.record_failure(&origin, now_ms);
                        return Err(PairingRejection::new(
                            ReasonCode::TlsHandshakeFailed,
                            "tls",
                            "TLS handshake timed out",
                        )
                        .at_origin(&origin));
                    }
                };

                let mut socket =
                    match timeout(HANDSHAKE_TIMEOUT, tokio_tungstenite::accept_async(tls)).await {
                        Ok(Ok(socket)) => socket,
                        Ok(Err(error)) => {
                            self.throttling.record_failure(&origin, now_ms);
                            return Err(PairingRejection::new(
                                ReasonCode::TlsHandshakeFailed,
                                "websocket",
                                error.to_string(),
                            )
                            .at_origin(&origin));
                        }
                        Err(_) => {
                            self.throttling.record_failure(&origin, now_ms);
                            return Err(PairingRejection::new(
                                ReasonCode::TlsHandshakeFailed,
                                "websocket",
                                "WebSocket handshake timed out",
                            )
                            .at_origin(&origin));
                        }
                    };

                let frame = match timeout(HANDSHAKE_TIMEOUT, socket.next()).await {
                    Ok(Some(Ok(Message::Text(text)))) => text,
                    Ok(Some(Ok(_))) => {
                        self.throttling.record_failure(&origin, now_ms);
                        return Err(PairingRejection::new(
                            ReasonCode::ProtocolViolation,
                            "pairing",
                            "the first pairing frame was not a text message",
                        )
                        .at_origin(&origin));
                    }
                    Ok(Some(Err(error))) => {
                        self.throttling.record_failure(&origin, now_ms);
                        return Err(PairingRejection::new(
                            ReasonCode::ProtocolViolation,
                            "pairing",
                            error.to_string(),
                        )
                        .at_origin(&origin));
                    }
                    Ok(None) | Err(_) => {
                        self.throttling.record_failure(&origin, now_ms);
                        return Err(PairingRejection::new(
                            ReasonCode::TlsHandshakeFailed,
                            "pairing",
                            "the peer closed before sending the pairing message",
                        )
                        .at_origin(&origin));
                    }
                };

                let (token, fingerprints) = match parse_signaling(frame.as_bytes()) {
                    Ok(SignalingMessage::Pair {
                        token,
                        dtls_fingerprint,
                        ..
                    }) => (
                        token,
                        MediaFingerprints::new(dtls_fingerprint, dtls_fingerprint),
                    ),
                    Ok(_) => {
                        self.throttling.record_failure(&origin, now_ms);
                        return Err(PairingRejection::new(
                            ReasonCode::ProtocolViolation,
                            "pairing",
                            "the first message was not a pair message",
                        )
                        .at_origin(&origin));
                    }
                    Err(error) => {
                        self.throttling.record_failure(&origin, now_ms);
                        return Err(PairingRejection::new(
                            error.code,
                            "pairing",
                            "the pairing message did not satisfy the contract",
                        )
                        .at_origin(&origin));
                    }
                };

                let token_now = Duration::from_millis(now_ms);
                if let Err(code) = self.token_store.take(&token, token_now) {
                    self.throttling.record_failure(&origin, now_ms);
                    return Err(PairingRejection::new(
                        code,
                        "token",
                        "the pairing token was not accepted",
                    )
                    .at_origin(&origin));
                }

                if let Err(error) = verify_fingerprints(&fingerprints, &self.expected_fingerprints)
                {
                    self.throttling.record_failure(&origin, now_ms);
                    return Err(PairingRejection::new(
                        error.code,
                        "fingerprint",
                        "the media fingerprint did not match the expected value",
                    )
                    .at_origin(&origin));
                }

                let paired = serde_json::json!({
                    "version": 1,
                    "type": "paired",
                    "session_id": remotecam_protocol::hex::encode_hex(&self.session_id),
                    "epoch": self.epoch,
                    "dtls_fingerprint": remotecam_protocol::hex::encode_hex(&fingerprints.video),
                    "accepted_limits": {},
                });
                if let Err(error) = socket.send(Message::Text(paired.to_string().into())).await {
                    self.throttling.record_failure(&origin, now_ms);
                    return Err(PairingRejection::new(
                        ReasonCode::TlsHandshakeFailed,
                        "paired",
                        error.to_string(),
                    )
                    .at_origin(&origin));
                }

                self.throttling.record_success(&origin);
                let session = AuthenticatedSession::establish(
                    self.session_id,
                    self.epoch,
                    origin,
                    fingerprints,
                );
                Ok(AcceptedPairing {
                    session,
                    established_channel_ms: u64::try_from(started.elapsed().as_millis())
                        .unwrap_or(u64::MAX),
                    permit_sequence: sequence,
                })
            }
        }
    }

    /// The consecutive-failure threshold, exposed so the evidence names it.
    pub fn failure_threshold() -> u32 {
        CONSECUTIVE_FAILURE_LIMIT
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// A loopback address used by the harness when a test needs a permitted origin.
pub fn loopback_ipv4() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

/// An IPv6 loopback address used by the harness.
pub fn loopback_ipv6() -> IpAddr {
    IpAddr::V6(Ipv6Addr::LOCALHOST)
}

/// Binds a listener on loopback and returns it.
pub async fn bind_loopback() -> std::io::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await
}

/// Connects a plain TCP stream, used by the interception proxy scenario.
pub async fn connect_tcp(address: SocketAddr) -> std::io::Result<TcpStream> {
    TcpStream::connect(address).await
}
