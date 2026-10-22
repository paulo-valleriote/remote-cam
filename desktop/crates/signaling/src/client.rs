//! The pinned pairing client.
//!
//! TLS 1.3 only, no root store in the product path, no session resumption, and
//! certificate validation by exact SHA-256 over the DER-encoded leaf. The
//! verifier sits behind the project-owned [`PinnedCertificateVerifier`] trait so
//! that a future `rustls` upgrade changes one file, not the protocol code.

use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use remotecam_protocol::ReasonCode;
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, DigitallySignedStruct, Error as RustlsError, RootCertStore, SignatureScheme,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::client_async;

const WEBSOCKET_URL: &str = "wss://remotecam.local/pairing";

/// The project-owned seam around certificate pinning.
pub trait PinnedCertificateVerifier: fmt::Debug + Send + Sync {
    /// The exact SHA-256 the client requires on the presented leaf.
    fn pinned_certificate_sha256(&self) -> [u8; 32];

    /// The `rustls` verifier backing this policy.
    fn rustls_verifier(&self) -> Arc<dyn ServerCertVerifier>;

    /// The reason the last handshake was rejected by this verifier.
    fn rejection_reason(&self) -> Option<ReasonCode>;

    /// Clears the rejection slot before a new attempt.
    fn clear_rejection(&self);
}

/// The reason the pinning verifier rejected a peer, if it did.
#[derive(Debug, Default)]
struct RejectionState {
    reason: Mutex<Option<ReasonCode>>,
}

impl RejectionState {
    fn record(&self, code: ReasonCode) -> RustlsError {
        let mut guard = self
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(code);
        RustlsError::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure)
    }

    fn reason(&self) -> Option<ReasonCode> {
        *self
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn clear(&self) {
        let mut guard = self
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = None;
    }
}

/// Exact SHA-256 pinning over the DER-encoded leaf certificate.
#[derive(Debug)]
pub struct ExactPinVerifier {
    pin: [u8; 32],
    provider: Arc<CryptoProvider>,
    chain_verifier: Option<Arc<WebPkiServerVerifier>>,
    rejection: Arc<RejectionState>,
}

impl ExactPinVerifier {
    /// Creates a verifier that trusts only `pin`. No root store is consulted.
    pub fn new(pin: [u8; 32]) -> Self {
        Self {
            pin,
            provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
            chain_verifier: None,
            rejection: Arc::new(RejectionState::default()),
        }
    }

    /// Adds a root store used **only to classify a chain-only trust attempt**.
    ///
    /// A peer that presents a certificate that is valid under the store, but is
    /// not the pinned leaf, is rejected with `certificate_chain_rejected` rather
    /// than `certificate_pin_mismatch`. The store never makes the peer trusted.
    pub fn with_chain_roots(mut self, roots: RootCertStore) -> Self {
        self.chain_verifier =
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots), self.provider.clone())
                .build()
                .ok();
        self
    }

    /// The last rejection code the verifier recorded.
    pub fn last_rejection(&self) -> Option<ReasonCode> {
        self.rejection.reason()
    }
}

impl PinnedCertificateVerifier for ExactPinVerifier {
    fn pinned_certificate_sha256(&self) -> [u8; 32] {
        self.pin
    }

    fn rustls_verifier(&self) -> Arc<dyn ServerCertVerifier> {
        // The verifier is cheap and stateless apart from the rejection slot.
        Arc::new(PinnedServerVerifier {
            pin: self.pin,
            provider: Arc::clone(&self.provider),
            chain_verifier: self.chain_verifier.clone(),
            rejection: Arc::clone(&self.rejection),
        })
    }

    fn rejection_reason(&self) -> Option<ReasonCode> {
        self.rejection.reason()
    }

    fn clear_rejection(&self) {
        self.rejection.clear();
    }
}

#[derive(Debug)]
struct PinnedServerVerifier {
    pin: [u8; 32],
    provider: Arc<CryptoProvider>,
    chain_verifier: Option<Arc<WebPkiServerVerifier>>,
    rejection: Arc<RejectionState>,
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let presented: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        if bool::from(self.pin.ct_eq(&presented)) {
            return Ok(ServerCertVerified::assertion());
        }

        if let Some(chain) = &self.chain_verifier {
            let chain_is_valid = chain
                .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
                .is_ok();
            if chain_is_valid {
                return Err(self.rejection.record(ReasonCode::CertificateChainRejected));
            }
        }
        Err(self.rejection.record(ReasonCode::CertificatePinMismatch))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        // A TLS 1.2 handshake is refused before this point; keeping the
        // verification strict means a configuration mistake cannot silently
        // weaken signature binding.
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        match verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        ) {
            Ok(valid) => Ok(valid),
            Err(error) => {
                self.rejection.record(ReasonCode::TlsHandshakeFailed);
                Err(error)
            }
        }
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Failure while connecting or pairing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingError {
    /// Closed reason code that must be reported.
    pub code: ReasonCode,
    /// English, diagnostics-only detail. Never sent to the peer and never
    /// includes host data, paths or user names.
    pub detail: String,
}

impl PairingError {
    fn new(code: ReasonCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for PairingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.detail)
    }
}

impl std::error::Error for PairingError {}

/// A connected pinned client.
#[derive(Debug)]
pub struct PinnedClient {
    config: Arc<ClientConfig>,
    verifier: Arc<dyn PinnedCertificateVerifier>,
}

impl PinnedClient {
    /// Builds a client that pins exactly `verifier`'s certificate.
    pub fn new(verifier: Arc<dyn PinnedCertificateVerifier>) -> Result<Self, PairingError> {
        let config = client_config(verifier.as_ref())?;
        Ok(Self {
            config: Arc::new(config),
            verifier,
        })
    }

    /// The certificate fingerprint this client requires.
    pub fn pin(&self) -> [u8; 32] {
        self.verifier.pinned_certificate_sha256()
    }

    /// The reason the last handshake was rejected, if the verifier rejected it.
    pub fn last_rejection(&self) -> Option<ReasonCode> {
        self.verifier.rejection_reason()
    }

    /// Connects to `addr` and completes the WebSocket upgrade over TLS 1.3.
    ///
    /// A handshake failure is mapped to a closed reason code. The pinning
    /// verifier's own code wins when it recorded one; otherwise a protocol
    /// version alert or incompatibility is `tls_version_unsupported` and any
    /// other fault is `tls_handshake_failed`.
    pub async fn connect(
        &self,
        addr: SocketAddr,
    ) -> Result<WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>, PairingError> {
        let tcp = TcpStream::connect(addr).await.map_err(|error| {
            PairingError::new(ReasonCode::DesktopUnreachable, error.to_string())
        })?;
        self.connect_stream(tcp).await
    }

    /// Completes TLS 1.3 and the WebSocket upgrade over an existing TCP stream.
    ///
    /// The gate uses this to choose a distinct loopback source address for the
    /// legitimate origin, which is what makes the per-origin scope observable.
    pub async fn connect_stream(
        &self,
        tcp: TcpStream,
    ) -> Result<WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>, PairingError> {
        self.verifier.clear_rejection();
        let server_name = ServerName::try_from(
            crate::certificate::CERTIFICATE_SERVER_NAME.to_owned(),
        )
        .map_err(|_| PairingError::new(ReasonCode::TlsHandshakeFailed, "invalid server name"))?;
        let connector = TlsConnector::from(Arc::clone(&self.config));
        let tls = connector.connect(server_name, tcp).await.map_err(|error| {
            if let Some(code) = self.verifier.rejection_reason() {
                PairingError::new(code, error.to_string())
            } else {
                PairingError::new(classify_io_error(&error), error.to_string())
            }
        })?;
        let (socket, _response) = client_async(WEBSOCKET_URL, tls).await.map_err(|error| {
            PairingError::new(ReasonCode::TlsHandshakeFailed, error.to_string())
        })?;
        Ok(socket)
    }
}

/// Builds a TLS 1.3-only client configuration for a pinned verifier.
pub fn client_config(
    verifier: &dyn PinnedCertificateVerifier,
) -> Result<ClientConfig, PairingError> {
    client_config_with_custom_verifier(verifier.rustls_verifier(), &[&rustls::version::TLS13])
}

/// Builds a client configuration with an explicit verifier and protocol set.
///
/// `#[doc(hidden)]`: the TLS 1.2 control exists so the gate can observe that the
/// server refuses a downgrade. It is never used on a product path.
#[doc(hidden)]
pub fn client_config_with_custom_verifier(
    verifier: Arc<dyn ServerCertVerifier>,
    versions: &[&'static rustls::SupportedProtocolVersion],
) -> Result<ClientConfig, PairingError> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    Ok(ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(versions)
        .map_err(|error| PairingError::new(ReasonCode::TlsHandshakeFailed, error.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

/// A deliberately unpinned verifier used only as the latency control.
///
/// It accepts any certificate and verifies the handshake signature normally. It
/// exists to measure how much delay exact pinning adds; it is never selectable
/// on a product path.
#[doc(hidden)]
#[derive(Debug)]
pub struct AcceptanceOnlyVerifier {
    provider: Arc<CryptoProvider>,
}

impl AcceptanceOnlyVerifier {
    /// Creates the control verifier.
    pub fn new() -> Self {
        Self {
            provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
        }
    }
}

impl Default for AcceptanceOnlyVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerCertVerifier for AcceptanceOnlyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Maps a `rustls` error to the closed reason code set.
pub fn classify_rustls_error(error: &RustlsError) -> ReasonCode {
    match error {
        RustlsError::AlertReceived(alert) => match alert {
            rustls::AlertDescription::ProtocolVersion => ReasonCode::TlsVersionUnsupported,
            rustls::AlertDescription::BadCertificate
            | rustls::AlertDescription::UnknownCA
            | rustls::AlertDescription::CertificateExpired
            | rustls::AlertDescription::CertificateUnknown
            | rustls::AlertDescription::UnsupportedCertificate => {
                ReasonCode::CertificateChainRejected
            }
            _ => ReasonCode::TlsHandshakeFailed,
        },
        RustlsError::PeerIncompatible(
            rustls::PeerIncompatible::ServerDoesNotSupportTls12Or13
            | rustls::PeerIncompatible::Tls12NotOffered
            | rustls::PeerIncompatible::Tls12NotOfferedOrEnabled
            | rustls::PeerIncompatible::Tls13RequiredForQuic,
        ) => ReasonCode::TlsVersionUnsupported,
        RustlsError::InvalidCertificate(_) => ReasonCode::CertificatePinMismatch,
        _ => ReasonCode::TlsHandshakeFailed,
    }
}

/// Maps a `tokio`/`std` I/O error wrapping a `rustls` error to a reason code.
pub fn classify_io_error(error: &std::io::Error) -> ReasonCode {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<RustlsError>())
        .map_or(ReasonCode::TlsHandshakeFailed, classify_rustls_error)
}
