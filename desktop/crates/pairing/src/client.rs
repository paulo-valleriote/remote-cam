use std::fmt;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use remotecam_protocol::ReasonCode;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, Error as RustlsError, SignatureScheme,
};
use subtle::ConstantTimeEq;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::certificate::{CERTIFICATE_SERVER_NAME, sha256_fingerprint};
use crate::error::{PairingError, classify_io_error};
use crate::listener::ALPN_PROTOCOL;

#[derive(Debug, Default)]
struct RejectionState {
    code: Mutex<Option<ReasonCode>>,
}

impl RejectionState {
    fn record(&self, code: ReasonCode) -> RustlsError {
        let mut guard = self
            .code
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(code);
        RustlsError::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
    }

    fn take(&self) -> Option<ReasonCode> {
        self.code
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    fn clear(&self) {
        *self
            .code
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

#[derive(Debug)]
struct ExactPinVerifier {
    pin: [u8; 32],
    provider: Arc<CryptoProvider>,
    rejection: Arc<RejectionState>,
}

impl ServerCertVerifier for ExactPinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let presented = sha256_fingerprint(end_entity.as_ref());
        if bool::from(self.pin.ct_eq(&presented)) {
            return Ok(ServerCertVerified::assertion());
        }
        Err(self.rejection.record(ReasonCode::CertificatePinMismatch))
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _certificate: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Err(RustlsError::General(
            "TLS 1.2 signatures are not accepted".to_owned(),
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn client_config(
    fingerprint: [u8; 32],
    rejection: &Arc<RejectionState>,
) -> Result<ClientConfig, PairingError> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = ExactPinVerifier {
        pin: fingerprint,
        provider: Arc::clone(&provider),
        rejection: Arc::clone(rejection),
    };
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| PairingError::new(ReasonCode::TlsHandshakeFailed, error.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
    config.resumption = rustls::client::Resumption::disabled();
    Ok(config)
}

pub struct PinnedClient {
    fingerprint: [u8; 32],
    config: Arc<ClientConfig>,
    rejection: Arc<RejectionState>,
}

impl PinnedClient {
    pub fn new(fingerprint: [u8; 32]) -> Result<Self, PairingError> {
        let rejection = Arc::new(RejectionState::default());
        let config = client_config(fingerprint, &rejection)?;
        Ok(Self {
            fingerprint,
            config: Arc::new(config),
            rejection,
        })
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    pub async fn connect(&self, address: SocketAddr) -> Result<ClientChannel, PairingError> {
        self.rejection.clear();
        let stream = TcpStream::connect(address).await.map_err(|error| {
            PairingError::new(ReasonCode::DesktopUnreachable, error.to_string())
        })?;
        let server_name = ServerName::try_from(CERTIFICATE_SERVER_NAME.to_owned())
            .expect("the certificate server name is a valid DNS name");
        let connector = TlsConnector::from(Arc::clone(&self.config));
        let stream = connector
            .connect(server_name, stream)
            .await
            .map_err(|error| {
                let code = self
                    .rejection
                    .take()
                    .unwrap_or_else(|| classify_io_error(&error));
                PairingError::new(code, error.to_string())
            })?;
        Ok(ClientChannel { inner: stream })
    }
}

pub struct ClientChannel {
    inner: TlsStream<TcpStream>,
}

impl fmt::Debug for ClientChannel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientChannel")
            .field("tls13", &self.is_tls13())
            .field("alpn_protocol", &self.alpn_protocol())
            .finish()
    }
}

impl ClientChannel {
    pub fn is_tls13(&self) -> bool {
        self.inner.get_ref().1.protocol_version() == Some(rustls::ProtocolVersion::TLSv1_3)
    }

    pub fn alpn_protocol(&self) -> Option<&[u8]> {
        self.inner.get_ref().1.alpn_protocol()
    }

    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.get_ref().0.peer_addr()
    }
}

impl AsyncRead for ClientChannel {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for ClientChannel {
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
    use super::{PinnedClient, RejectionState, client_config};
    use std::sync::Arc;

    #[test]
    fn the_client_keeps_the_fingerprint_it_was_built_with() {
        let client = PinnedClient::new([0xab; 32]).expect("client");

        assert_eq!(client.fingerprint(), [0xab; 32]);
    }

    #[test]
    fn a_rejection_is_consumed_once() {
        let rejection = Arc::new(RejectionState::default());
        rejection.record(remotecam_protocol::ReasonCode::CertificatePinMismatch);

        assert_eq!(
            rejection.take(),
            Some(remotecam_protocol::ReasonCode::CertificatePinMismatch)
        );
        assert_eq!(rejection.take(), None);
    }

    #[test]
    fn a_rejection_can_be_cleared_before_a_new_attempt() {
        let rejection = Arc::new(RejectionState::default());
        rejection.record(remotecam_protocol::ReasonCode::CertificatePinMismatch);

        rejection.clear();

        assert_eq!(rejection.take(), None);
    }

    #[test]
    fn the_configuration_is_reusable_and_pins_the_capability() {
        let rejection = Arc::new(RejectionState::default());
        let config = client_config([1; 32], &rejection).expect("configuration");

        assert_eq!(config.alpn_protocols, vec![b"remotecam/1".to_vec()]);
    }
}
