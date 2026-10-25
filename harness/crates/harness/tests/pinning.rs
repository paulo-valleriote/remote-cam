use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use remotecam_pairing::{EphemeralCertificate, PairingListener, PinnedClient};
use remotecam_protocol::{Endpoint, QrPayload, ReasonCode};
use tokio::runtime::Builder;
use zeroize::{Zeroize, Zeroizing};

#[test]
fn the_synthetic_mobile_rejects_a_divergent_qr_pin_without_retry() {
    let runtime = Builder::new_current_thread()
        .enable_io()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let listener = Arc::new(
            PairingListener::bind_loopback(certificate)
                .await
                .expect("listener"),
        );
        let mut divergent_fingerprint = listener.fingerprint();
        divergent_fingerprint[0] ^= 0xff;
        let endpoint = Endpoint::from_address(4, "192.168.1.42", listener.local_addr().port())
            .expect("private endpoint");
        let mut qr_payload = QrPayload {
            session_id: [0x11; 16],
            epoch: 1,
            expires_at: 2_000_000_000,
            pairing_token: [0x22; 32],
            tls_fingerprint: divergent_fingerprint,
            capabilities: remotecam_pairing::DESKTOP_CAPABILITIES,
            endpoints: vec![endpoint],
        };
        let encoded_qr = qr_payload.encode().expect("QR payload");
        qr_payload.pairing_token.zeroize();
        let encoded_qr = Zeroizing::new(encoded_qr);
        let mut decoded_qr = QrPayload::decode(&encoded_qr, 1_900_000_000).expect("decoded QR");
        decoded_qr.pairing_token.zeroize();

        let Some(endpoint) = decoded_qr.endpoints.first() else {
            panic!("the QR contains an endpoint");
        };
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
        let client = PinnedClient::new(decoded_qr.tls_fingerprint).expect("pinned client");
        let server_listener = Arc::clone(&listener);
        let server = tokio::spawn(async move { server_listener.accept().await });

        let error = client
            .connect(address)
            .await
            .expect_err("a divergent pin must abort the TLS handshake");
        assert_eq!(error.code(), ReasonCode::CertificatePinMismatch);

        let server_error = server
            .await
            .expect("server task")
            .expect_err("the listener must reject the handshake");
        assert_eq!(server_error.code(), ReasonCode::CertificatePinMismatch);
        assert_eq!(listener.attempts(), 1);
    });
}
