use remotecam_pairing::{EphemeralCertificate, PairingListener, PairingMaterialRequest};
use remotecam_protocol::signaling::{Limits, SignalingMessage};
use remotecam_protocol::{Endpoint, QrPayload};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{Effect, PairingError, SessionCore, SessionEvent, Timestamp};

const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];
const EXPIRES_AT: u64 = 2_000_000_000;

#[tokio::test]
async fn qr_material_authenticates_the_session_core_pairing() {
    let mut core = SessionCore::new(
        ManualClock::new(Timestamp::ZERO),
        SeededRandomSource::new(7),
        DESKTOP_FINGERPRINT,
    );
    let effects = core
        .handle(SessionEvent::StartPairing)
        .expect("starting pairing succeeds");
    let [Effect::PresentPairing(session_material)] = effects.as_slice() else {
        panic!("starting pairing presents session material");
    };

    let listener =
        PairingListener::bind_loopback(EphemeralCertificate::generate().expect("certificate"))
            .await
            .expect("listener");
    let endpoint =
        Endpoint::from_address(4, "192.168.1.42", listener.local_addr().port()).expect("endpoint");
    let pairing_material = listener
        .issue_material(PairingMaterialRequest::new(
            session_material,
            EXPIRES_AT,
            vec![endpoint],
        ))
        .expect("pairing material");
    let payload =
        QrPayload::decode(pairing_material.encoded_qr(), 1_900_000_000).expect("QR payload");

    assert_eq!(payload.tls_fingerprint, listener.fingerprint());
    assert_eq!(payload.endpoints[0].port, listener.local_addr().port());
    assert_eq!(
        payload.session_id,
        *session_material.session_id().as_bytes()
    );
    assert_eq!(
        payload.epoch,
        u32::try_from(session_material.epoch()).expect("session epoch fits QR")
    );
    assert_eq!(payload.pairing_token, *session_material.token().as_bytes());

    core.handle(SessionEvent::PeerConnected)
        .expect("peer connects before authenticating");
    let frame = SignalingMessage::Pair {
        token: payload.pairing_token,
        dtls_fingerprint: [0x55; 32],
        limits: Limits::new(),
    }
    .encode()
    .expect("pair message is valid");

    let effect = core
        .authenticate_first_frame(&frame)
        .expect("the QR token authenticates its owning session");

    assert_eq!(
        effect,
        vec![Effect::SendPaired {
            session_id: session_material.session_id(),
            epoch: session_material.epoch(),
            dtls_fingerprint: DESKTOP_FINGERPRINT,
            accepted_limits: Limits::new(),
        }]
    );
    assert_eq!(
        core.authenticate_first_frame(&frame),
        Err(PairingError::TokenAlreadyUsed)
    );
}
