//! Certificate pinning contract tests (T043).
//!
//! The gate's security claim rests on three properties that are tested here
//! without any product code: exact DER-leaf SHA-256, no root-store fallback, and
//! a `CertificateVerify` signature that is actually verified against the pinned
//! certificate.

use std::sync::Arc;
use std::time::Duration;

use remotecam_protocol::ReasonCode;
use remotecam_signaling::certificate::test_support::chain_fixture;
use remotecam_signaling::certificate::{EphemeralCertificate, sha256_fingerprint};
use remotecam_signaling::client::PinnedCertificateVerifier;
use remotecam_signaling::client::{
    AcceptanceOnlyVerifier, ExactPinVerifier, classify_io_error, client_config_with_custom_verifier,
};
use remotecam_signaling::fingerprint_exchange::MediaFingerprints;
use remotecam_signaling::server::{PairingServer, SourcePolicy, bind_loopback};
use remotecam_signaling::token::TokenStore;
use rustls::DigitallySignedStruct;
use rustls::RootCertStore;
use rustls::internal::msgs::codec::{Codec, Reader};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::TlsConnector;

fn server_name() -> ServerName<'static> {
    ServerName::try_from("remotecam.local").expect("valid server name")
}

fn verify(
    verifier: &ExactPinVerifier,
    leaf: &CertificateDer<'static>,
    intermediates: &[CertificateDer<'static>],
) -> Result<(), ReasonCode> {
    verifier
        .rustls_verifier()
        .verify_server_cert(leaf, intermediates, &server_name(), &[], UnixTime::now())
        .map(|_| ())
        .map_err(|_| verifier.last_rejection().expect("a rejection was recorded"))
}

#[test]
fn accepts_the_exact_der_encoded_leaf_sha256() {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let verifier = ExactPinVerifier::new(certificate.fingerprint());

    verify(&verifier, certificate.der(), &[]).expect("the exact pin is accepted");
    assert_eq!(verifier.last_rejection(), None);
}

#[test]
fn rejects_a_one_byte_fingerprint_mismatch() {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let mut pin = certificate.fingerprint();
    pin[31] ^= 0x01;
    let verifier = ExactPinVerifier::new(pin);

    assert_eq!(
        verify(&verifier, certificate.der(), &[]),
        Err(ReasonCode::CertificatePinMismatch)
    );
}

#[test]
fn rejects_chain_only_trust_without_falling_back() {
    let fixture = chain_fixture().expect("chain fixture");
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(fixture.ca_der.clone()))
        .expect("CA added");
    // Pin the CA, not the leaf: the presented leaf chains to the CA, so the
    // attempt is a chain-only trust attempt, not a random wrong certificate.
    let verifier =
        ExactPinVerifier::new(sha256_fingerprint(&fixture.ca_der)).with_chain_roots(roots);

    let intermediates = vec![CertificateDer::from(fixture.ca_der.clone())];
    assert_eq!(
        verify(&verifier, fixture.leaf.der(), &intermediates),
        Err(ReasonCode::CertificateChainRejected)
    );
    assert_eq!(
        verifier.last_rejection(),
        Some(ReasonCode::CertificateChainRejected)
    );
}

#[tokio::test]
async fn tls12_downgrade_is_rejected_as_tls_version_unsupported() {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let token_store = Arc::new(TokenStore::new());
    token_store.issue([7; 32], Duration::ZERO);
    let fingerprints = MediaFingerprints::new([1; 32], [2; 32]);
    let listener = bind_loopback().await.expect("listener");
    let address = listener.local_addr().expect("bound address");
    let server = PairingServer::new(
        listener,
        &certificate,
        None,
        Arc::clone(&token_store),
        fingerprints,
        SourcePolicy {
            allow_loopback: true,
            expected_subnet: None,
        },
        [0; 16],
        1,
    )
    .expect("server");
    let server_task = tokio::spawn(async move {
        let mut server = server;
        server.accept_one().await
    });

    let config = client_config_with_custom_verifier(
        Arc::new(AcceptanceOnlyVerifier::new()),
        &[&rustls::version::TLS12],
    )
    .expect("client config");
    let connector = TlsConnector::from(Arc::new(config));
    let tcp = tokio::net::TcpStream::connect(address).await.expect("tcp");
    let error = connector
        .connect(server_name(), tcp)
        .await
        .expect_err("TLS 1.3-only server must refuse a TLS 1.2 client");
    assert_eq!(
        classify_io_error(&error),
        ReasonCode::TlsVersionUnsupported,
        "{error}"
    );

    let rejection = server_task
        .await
        .expect("server task")
        .expect_err("server must reject");
    assert_eq!(rejection.code, ReasonCode::TlsVersionUnsupported);
}

#[test]
fn verifies_the_certificate_verify_signature() {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let signing_key = rustls::crypto::aws_lc_rs::sign::any_supported_type(&certificate.key())
        .expect("the generated key is supported");
    let offered = rustls::crypto::aws_lc_rs::default_provider()
        .signature_verification_algorithms
        .supported_schemes();
    let signer = signing_key
        .choose_scheme(&offered)
        .expect("a supported scheme");
    let message = b"TLS 1.3 CertificateVerify transcript";
    let signature = signer.sign(message).expect("signature");

    let mut wire = Vec::new();
    wire.extend_from_slice(&u16::from(signer.scheme()).to_be_bytes());
    wire.extend_from_slice(
        &u16::try_from(signature.len())
            .expect("signature fits")
            .to_be_bytes(),
    );
    wire.extend_from_slice(&signature);
    let signed = DigitallySignedStruct::read(&mut Reader::init(&wire)).expect("decodes");

    let verifier = ExactPinVerifier::new(certificate.fingerprint()).rustls_verifier();
    assert!(
        verifier
            .verify_tls13_signature(message, certificate.der(), &signed)
            .is_ok(),
        "a signature by the pinned certificate's key must verify"
    );
    assert!(
        verifier
            .verify_tls13_signature(b"a different transcript", certificate.der(), &signed)
            .is_err(),
        "a signature over a different message must not verify"
    );
}
