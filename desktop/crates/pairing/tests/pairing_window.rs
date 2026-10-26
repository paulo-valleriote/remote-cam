use remotecam_pairing::{
    EphemeralCertificate, PairingListener, PairingMaterialRequest, PinnedClient,
};
use remotecam_protocol::signaling::{Limits, SignalingMessage};
use remotecam_protocol::{Endpoint, ReasonCode};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{
    Effect, PAIRING_WINDOW_MILLIS, PairingMaterial, SessionCore, SessionEvent, SessionState,
    Timestamp,
};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];
const EXPIRES_AT: u64 = 2_000_000_000;

type Core = SessionCore<ManualClock, SeededRandomSource>;

struct Fixture {
    core: Core,
    clock: ManualClock,
    listener: Arc<PairingListener>,
    first: PairingMaterial,
}

async fn fixture() -> Fixture {
    let clock = ManualClock::new(Timestamp::ZERO);
    let mut core = SessionCore::new(
        clock.clone(),
        SeededRandomSource::new(7),
        DESKTOP_FINGERPRINT,
    );
    let mut effects = core
        .handle(SessionEvent::StartPairing)
        .expect("starting pairing is valid");
    let Some(Effect::PresentPairing(session_material)) = effects.pop() else {
        panic!("starting pairing presents its material");
    };
    let listener = Arc::new(
        PairingListener::bind_loopback(EphemeralCertificate::generate().expect("certificate"))
            .await
            .expect("listener"),
    );
    let endpoint =
        Endpoint::from_address(4, "192.168.1.42", listener.local_addr().port()).expect("endpoint");
    listener
        .issue_material(PairingMaterialRequest::new(
            &session_material,
            EXPIRES_AT,
            vec![endpoint],
        ))
        .expect("pairing material");
    Fixture {
        core,
        clock,
        listener,
        first: session_material,
    }
}

fn pair_frame(token: &[u8; 32]) -> Vec<u8> {
    SignalingMessage::Pair {
        token: *token,
        dtls_fingerprint: [0x55; 32],
        limits: Limits::new(),
    }
    .encode()
    .expect("the pair message is valid")
}

fn refusal(effects: Vec<Effect>) -> (ReasonCode, PairingMaterial) {
    let mut reason = None;
    let mut fresh = None;
    for effect in effects {
        match effect {
            Effect::RejectPeer { reason: code } => reason = Some(code),
            Effect::PresentPairing(material) => fresh = Some(material),
            other => panic!("unexpected effect {other:?}"),
        }
    }
    (
        reason.expect("the attempt is refused"),
        fresh.expect("fresh material is presented"),
    )
}

#[tokio::test]
async fn a_peer_after_the_qr_window_is_closed_before_the_handshake() {
    let mut fixture = fixture().await;
    let address = fixture.listener.local_addr();
    fixture.clock.advance(PAIRING_WINDOW_MILLIS);

    let peer = tokio::spawn(async move {
        let mut socket = TcpStream::connect(address).await.expect("tcp connect");
        let mut received = Vec::new();
        socket
            .read_to_end(&mut received)
            .await
            .expect("the refused peer sees a close");
        received
    });

    let mut fresh = None;
    let refused = fixture
        .listener
        .accept_admitted(|| {
            let effects = fixture
                .core
                .handle(SessionEvent::PeerConnected)
                .expect("an expired qr is answered, not rejected as a transition");
            let (code, material) = refusal(effects);
            fresh = Some(material);
            Some(code)
        })
        .await;

    let error = refused.expect_err("the expired qr is refused");
    assert_eq!(error.code(), ReasonCode::QrExpired);
    assert!(peer.await.expect("peer task").is_empty());
    assert_eq!(fixture.listener.attempts(), 1);
    assert_eq!(fresh.expect("fresh material").epoch(), 2);
    assert_eq!(fixture.core.state(), SessionState::PairingReady);
}

#[tokio::test]
async fn a_pair_frame_after_the_token_window_is_closed_and_the_old_token_is_dead() {
    let mut fixture = fixture().await;
    let address = fixture.listener.local_addr();
    let certificate_fingerprint = fixture.listener.fingerprint();
    let old_token = *fixture.first.token().as_bytes();

    let peer = tokio::spawn(async move {
        let client = PinnedClient::new(certificate_fingerprint).expect("client");
        let mut channel = client.connect(address).await.expect("handshake");
        let mut received = Vec::new();
        channel
            .read_to_end(&mut received)
            .await
            .expect("the refused peer sees a close");
        received
    });

    let socket = fixture.listener.accept_socket().await.expect("tcp accept");
    fixture
        .core
        .handle(SessionEvent::PeerConnected)
        .expect("the qr is still valid when the peer connects");
    let channel = fixture.listener.handshake(socket).await.expect("handshake");

    fixture.clock.advance(PAIRING_WINDOW_MILLIS);
    let effects = fixture
        .core
        .authenticate_first_frame(&pair_frame(&old_token))
        .expect("an expired token is answered, not rejected as a pairing error");
    let (code, fresh) = refusal(effects);
    let error = channel.reject(code).await;

    assert_eq!(error.code(), ReasonCode::TokenExpired);
    assert!(peer.await.expect("peer task").is_empty());
    assert_ne!(fresh.token().as_bytes(), &old_token);
    assert_eq!(fixture.core.state(), SessionState::PairingReady);
    assert!(
        fixture
            .core
            .authenticate_first_frame(&pair_frame(&old_token))
            .is_err(),
        "the replaced material is the only one that can pair"
    );
}
