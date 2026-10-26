mod common;

use std::net::SocketAddr;
use std::sync::Arc;

use remotecam_pairing::{
    ALPN_PROTOCOL, EphemeralCertificate, PairingError, PairingListener, PinnedClient,
    classify_io_error, sha256_fingerprint,
};
use remotecam_protocol::ReasonCode;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn server_name() -> ServerName<'static> {
    ServerName::try_from("remotecam.local").expect("valid certificate name")
}

async fn bound_listener() -> (Arc<PairingListener>, [u8; 32], SocketAddr) {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let fingerprint = certificate.fingerprint();
    let listener = PairingListener::bind_loopback(certificate)
        .await
        .expect("listener")
        .with_origin_throttle(common::unlimited_attempts());
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
async fn refuses_a_tls13_handshake_without_the_dedicated_alpn() {
    let (listener, _fingerprint, address) = bound_listener().await;
    let server = accept_one(&listener);

    let connector = TlsConnector::from(Arc::new(tls13_client_without_alpn_config()));
    let stream = TcpStream::connect(address).await.expect("tcp");
    let _client_handshake = connector
        .connect(server_name(), stream)
        .await
        .expect("TLS 1.3 itself is supported");

    let server_error = server
        .await
        .expect("server task")
        .expect_err("the listener must reject a handshake without ALPN");
    assert_eq!(server_error.code(), ReasonCode::TlsHandshakeFailed);
}

#[tokio::test]
async fn refuses_a_pinned_server_that_does_not_negotiate_alpn() {
    let certified =
        rcgen::generate_simple_self_signed(vec!["remotecam.local".to_owned()]).expect("cert");
    let fingerprint = sha256_fingerprint(certified.cert.der().as_ref());
    let certificate = certified.cert.der().clone();
    let private_key = rustls::pki_types::PrivateKeyDer::Pkcs8(
        rustls::pki_types::PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der()),
    );
    let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .expect("TLS 1.3 server configuration")
    .with_no_client_auth()
    .with_single_cert(vec![certificate], private_key)
    .expect("server certificate");
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("server listener");
    let address = listener.local_addr().expect("server address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("TCP connection");
        TlsAcceptor::from(Arc::new(server_config))
            .accept(stream)
            .await
            .expect("TLS handshake")
    });

    let client = PinnedClient::new(fingerprint).expect("client");
    let error = client
        .connect(address)
        .await
        .expect_err("a pinned server without the required ALPN must be refused");

    assert_eq!(error.code(), ReasonCode::TlsHandshakeFailed);
    assert!(
        server
            .await
            .expect("server task")
            .get_ref()
            .1
            .alpn_protocol()
            .is_none()
    );
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
async fn accepts_concurrent_clients_once_each() {
    const CONNECTIONS: usize = 8;

    let (listener, fingerprint, address) = bound_listener().await;
    let client = Arc::new(PinnedClient::new(fingerprint).expect("client"));
    let acceptors: Vec<_> = (0..CONNECTIONS)
        .map(|_| {
            let listener = Arc::clone(&listener);
            tokio::spawn(async move {
                let mut channel = listener.accept().await.expect("concurrent handshake");
                let mut marker = [0];
                channel
                    .read_exact(&mut marker)
                    .await
                    .expect("client marker");
                channel.write_all(&marker).await.expect("server echo");
                channel.flush().await.expect("server flush");
                marker[0]
            })
        })
        .collect();
    let clients: Vec<_> = (0..CONNECTIONS)
        .map(|index| {
            let client = Arc::clone(&client);
            tokio::spawn(async move {
                let marker = [index as u8];
                let mut channel = client.connect(address).await.expect("concurrent handshake");
                assert!(channel.is_tls13());
                assert_eq!(channel.alpn_protocol(), Some(ALPN_PROTOCOL));
                channel.write_all(&marker).await.expect("client marker");
                channel.flush().await.expect("client flush");
                let mut echoed = [0];
                channel.read_exact(&mut echoed).await.expect("server echo");
                assert_eq!(echoed, marker);
                marker[0]
            })
        })
        .collect();

    let mut client_markers = Vec::with_capacity(CONNECTIONS);
    for connection in clients {
        client_markers.push(connection.await.expect("client task"));
    }
    let mut server_markers = Vec::with_capacity(CONNECTIONS);
    for acceptor in acceptors {
        server_markers.push(acceptor.await.expect("listener task"));
    }
    client_markers.sort_unstable();
    server_markers.sort_unstable();
    assert_eq!(client_markers, (0..CONNECTIONS as u8).collect::<Vec<_>>());
    assert_eq!(server_markers, client_markers);
    assert_eq!(listener.attempts(), CONNECTIONS as u32);
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

fn tls13_client_without_alpn_config() -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = AcceptAnyCertificate {
        provider: Arc::clone(&provider),
    };
    rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3 client configuration")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth()
}
