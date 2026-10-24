//! Loopback integration tests for the pairing channel.
//!
//! Every scenario uses a real TCP socket on IPv4 loopback. The certificate, the
//! TLS handshake and the pin comparison are the production ones.

use std::net::SocketAddr;
use std::sync::Arc;

use remotecam_pairing::{
    ALPN_PROTOCOL, EphemeralCertificate, PairingError, PairingListener, PinnedClient,
    classify_io_error,
};
use remotecam_protocol::ReasonCode;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

fn server_name() -> ServerName<'static> {
    ServerName::try_from("remotecam.local").expect("valid certificate name")
}

async fn bound_listener() -> (Arc<PairingListener>, [u8; 32], SocketAddr) {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let fingerprint = certificate.fingerprint();
    let listener = PairingListener::bind_loopback(certificate)
        .await
        .expect("listener");
    let address = listener.local_addr();
    (Arc::new(listener), fingerprint, address)
}

fn accept_one(
    listener: &Arc<PairingListener>,
) -> tokio::task::JoinHandle<Result<remotecam_pairing::ServerChannel, PairingError>> {
    let listener = Arc::clone(listener);
    tokio::spawn(async move { listener.accept().await })
}

#[tokio::test]
async fn accepts_a_loopback_handshake_with_the_exact_pin() {
    let (listener, fingerprint, address) = bound_listener().await;
    let server = accept_one(&listener);

    let client = PinnedClient::new(fingerprint).expect("client");
    let channel = client.connect(address).await.expect("handshake");

    assert!(channel.is_tls13());
    assert_eq!(channel.alpn_protocol(), Some(ALPN_PROTOCOL));
    assert!(channel.peer_addr().expect("peer").ip().is_loopback());

    let server_channel = server.await.expect("server task").expect("handshake");
    assert!(server_channel.is_tls13());
    assert_eq!(server_channel.alpn_protocol(), Some(ALPN_PROTOCOL));
    assert!(server_channel.peer_addr().expect("peer").ip().is_loopback());
    assert_eq!(listener.attempts(), 1);
}

#[tokio::test]
async fn aborts_a_divergent_pin_without_a_second_attempt() {
    let (listener, fingerprint, address) = bound_listener().await;
    let server = accept_one(&listener);

    let mut divergent = fingerprint;
    divergent[0] ^= 0xff;
    let client = PinnedClient::new(divergent).expect("client");
    let error = client
        .connect(address)
        .await
        .expect_err("the divergent pin must abort");

    assert_eq!(error.code(), ReasonCode::CertificatePinMismatch);
    assert_eq!(listener.attempts(), 1);

    let server_error = server
        .await
        .expect("server task")
        .expect_err("the listener must not accept the session");
    assert_eq!(server_error.code(), ReasonCode::CertificatePinMismatch);
}

#[tokio::test]
async fn refuses_a_tls12_client_with_tls_version_unsupported() {
    let (listener, _fingerprint, address) = bound_listener().await;
    let server = accept_one(&listener);

    let connector = TlsConnector::from(Arc::new(tls12_client_config()));
    let stream = TcpStream::connect(address).await.expect("tcp");
    let error = connector
        .connect(server_name(), stream)
        .await
        .expect_err("a TLS 1.3-only listener must refuse TLS 1.2");

    assert_eq!(
        classify_io_error(&error),
        ReasonCode::TlsVersionUnsupported,
        "{error}"
    );

    let server_error = server.await.expect("server task").expect_err("refused");
    assert_eq!(server_error.code(), ReasonCode::TlsVersionUnsupported);
}

#[tokio::test]
async fn carries_bytes_both_ways_after_the_handshake() {
    let (listener, fingerprint, address) = bound_listener().await;
    let server = accept_one(&listener);

    let client = PinnedClient::new(fingerprint).expect("client");
    let mut client_channel = client.connect(address).await.expect("handshake");
    let mut server_channel = server.await.expect("server task").expect("handshake");

    client_channel
        .write_all(b"pairing")
        .await
        .expect("client write");
    client_channel.flush().await.expect("client flush");

    let mut received = [0_u8; 7];
    server_channel
        .read_exact(&mut received)
        .await
        .expect("server read");
    assert_eq!(&received, b"pairing");

    server_channel
        .write_all(&received)
        .await
        .expect("server write");
    server_channel.flush().await.expect("server flush");
    server_channel.shutdown().await.expect("server shutdown");

    let mut echoed = Vec::new();
    client_channel
        .read_to_end(&mut echoed)
        .await
        .expect("client read");
    assert_eq!(echoed, b"pairing");
    client_channel.shutdown().await.expect("client shutdown");

    let rendered = format!("{client_channel:?} {server_channel:?}");
    assert!(rendered.contains("ClientChannel"));
    assert!(rendered.contains("ServerChannel"));
    assert_eq!(rendered.matches("tls13: true").count(), 2);
    assert_eq!(rendered.matches("alpn_protocol: Some(").count(), 2);
}

#[tokio::test]
async fn reports_an_unreachable_listener_on_a_closed_loopback_port() {
    let (listener, fingerprint, address) = bound_listener().await;
    drop(listener);

    let client = PinnedClient::new(fingerprint).expect("client");
    let error = client
        .connect(address)
        .await
        .expect_err("the closed port must not connect");

    assert_eq!(error.code(), ReasonCode::DesktopUnreachable);
}

#[test]
fn a_pin_failure_never_renders_the_fingerprint() {
    let fingerprint = [0xab; 32];
    let rendered_fingerprint = remotecam_protocol::hex::encode_hex(&fingerprint);
    let error = PairingError::new(
        ReasonCode::CertificatePinMismatch,
        "the client rejected the certificate",
    );

    let rendered = format!("{error} {error:?}");

    assert!(!rendered.contains(&rendered_fingerprint));
    assert!(rendered.contains("certificate_pin_mismatch"));
}

#[derive(Debug)]
struct AcceptAnyCertificate {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for AcceptAnyCertificate {
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
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
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

fn tls12_client_config() -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = AcceptAnyCertificate {
        provider: Arc::clone(&provider),
    };
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS12])
        .expect("TLS 1.2 client configuration")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
    config
}
