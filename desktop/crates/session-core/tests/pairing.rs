use remotecam_protocol::signaling::{Limits, SignalingMessage};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{
    Effect, PairingError, SessionCore, SessionEvent, SessionState, Timestamp, TransitionErrorCode,
};

const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];

fn core_in_handshaking() -> (
    SessionCore<ManualClock, SeededRandomSource>,
    [u8; 16],
    [u8; 32],
) {
    let mut core = SessionCore::new(
        ManualClock::new(Timestamp::ZERO),
        SeededRandomSource::new(7),
        DESKTOP_FINGERPRINT,
    );

    let effects = core
        .handle(SessionEvent::StartPairing)
        .expect("starting pairing succeeds");
    let [Effect::PresentPairing(material)] = effects.as_slice() else {
        panic!("starting pairing presents its material");
    };
    let token = *material.token().as_bytes();
    let session_id = *material.session_id().as_bytes();

    core.handle(SessionEvent::PeerConnected)
        .expect("the peer can connect for pairing");

    (core, session_id, token)
}

fn pair_frame(token: [u8; 32]) -> Vec<u8> {
    SignalingMessage::Pair {
        token,
        dtls_fingerprint: [0x55; 32],
        limits: Limits::new(),
    }
    .encode()
    .expect("the pair message is valid")
}

#[test]
fn a_non_pair_first_frame_ends_the_session_with_protocol_violation() {
    let (mut core, _session_id, _token) = core_in_handshaking();

    let error = core
        .authenticate_first_frame(&SignalingMessage::Ping.encode().unwrap())
        .expect_err("pair must be the first frame");

    assert_eq!(
        error.code(),
        remotecam_protocol::ReasonCode::ProtocolViolation
    );
    assert_eq!(core.state(), SessionState::Ended);
}

#[test]
fn a_malformed_first_frame_ends_the_session_with_protocol_violation() {
    let (mut core, _session_id, _token) = core_in_handshaking();

    let error = core
        .authenticate_first_frame(b"{")
        .expect_err("a malformed first frame violates the pairing protocol");

    assert_eq!(
        error.code(),
        remotecam_protocol::ReasonCode::ProtocolViolation
    );
    assert_eq!(core.state(), SessionState::Ended);
}

#[test]
fn a_pair_frame_received_before_the_peer_connects_is_a_protocol_violation() {
    let mut core = SessionCore::new(
        ManualClock::new(Timestamp::ZERO),
        SeededRandomSource::new(7),
        DESKTOP_FINGERPRINT,
    );
    let effects = core
        .handle(SessionEvent::StartPairing)
        .expect("starting pairing succeeds");
    let [Effect::PresentPairing(material)] = effects.as_slice() else {
        panic!("starting pairing presents its material");
    };
    let frame = pair_frame(*material.token().as_bytes());

    let error = core
        .authenticate_first_frame(&frame)
        .expect_err("the peer must connect before sending the pair frame");

    assert_eq!(
        error.code(),
        remotecam_protocol::ReasonCode::ProtocolViolation
    );
    assert_eq!(core.state(), SessionState::Ended);
}

#[test]
fn different_token_mismatches_have_the_same_rejection_code() {
    let (mut core, _session_id, token) = core_in_handshaking();
    let mut first_byte_mismatch = token;
    first_byte_mismatch[0] ^= 1;
    let mut last_byte_mismatch = token;
    last_byte_mismatch[31] ^= 1;

    let first_error = core
        .authenticate_first_frame(&pair_frame(first_byte_mismatch))
        .expect_err("a mismatched token is rejected");
    let last_error = core
        .authenticate_first_frame(&pair_frame(last_byte_mismatch))
        .expect_err("a mismatched token is rejected without revealing its position");

    assert_eq!(first_error, PairingError::TokenInvalid);
    assert_eq!(last_error, PairingError::TokenInvalid);
    assert_eq!(core.state(), SessionState::Handshaking);
    core.authenticate_first_frame(&pair_frame(token))
        .expect("a rejected token does not consume the expected token");
}

#[test]
fn a_valid_pair_returns_the_desktop_identity_and_consumes_its_token() {
    let (mut core, session_id, token) = core_in_handshaking();
    let frame = pair_frame(token);

    let effect = core
        .authenticate_first_frame(&frame)
        .expect("the matching token authenticates");

    assert_eq!(
        effect,
        vec![Effect::SendPaired {
            session_id: remotecam_session_core::SessionId::from_bytes(session_id),
            epoch: 1,
            dtls_fingerprint: DESKTOP_FINGERPRINT,
            accepted_limits: Limits::new(),
        }]
    );
    assert_eq!(core.state(), SessionState::Negotiating);
    assert_eq!(
        core.authenticate_first_frame(&frame),
        Err(PairingError::Superseded)
    );
}

#[test]
fn pairing_debug_output_redacts_the_token_and_fingerprint() {
    let mut core = SessionCore::new(
        ManualClock::new(Timestamp::ZERO),
        SeededRandomSource::new(7),
        DESKTOP_FINGERPRINT,
    );
    let effects = core
        .handle(SessionEvent::StartPairing)
        .expect("starting pairing succeeds");
    let [Effect::PresentPairing(material)] = effects.as_slice() else {
        panic!("starting pairing presents its material");
    };
    let pair_material_debug = format!("{material:?}");
    let token_debug = format!("{:?}", material.token().as_bytes());
    let paired_debug = format!(
        "{:?}",
        Effect::SendPaired {
            session_id: remotecam_session_core::SessionId::from_bytes([0x33; 16]),
            epoch: 1,
            dtls_fingerprint: DESKTOP_FINGERPRINT,
            accepted_limits: Limits::new(),
        }
    );

    assert!(pair_material_debug.contains("[REDACTED]"));
    assert!(!pair_material_debug.contains(&token_debug));
    assert!(paired_debug.contains("[REDACTED]"));
    assert!(!paired_debug.contains("444444"));
}

#[test]
fn media_cannot_start_before_the_first_pair_frame_is_authenticated() {
    let (mut core, _session_id, token) = core_in_handshaking();

    assert_eq!(
        core.handle(SessionEvent::NegotiationCompleted)
            .expect_err("negotiation cannot complete before authentication")
            .code(),
        TransitionErrorCode::InvalidTransition
    );

    let frame = pair_frame(token);
    core.authenticate_first_frame(&frame)
        .expect("the matching token authenticates");
    assert_eq!(
        core.handle(SessionEvent::NegotiationCompleted)
            .expect("negotiation completes after authentication"),
        vec![Effect::StartMedia]
    );
    core.handle(SessionEvent::FirstMediaFrame(
        remotecam_session_core::MediaFrame {
            sequence: 0,
            timestamp: remotecam_session_core::Timestamp::ZERO,
        },
    ))
    .expect("the first frame completes connection establishment");
    assert_eq!(core.state(), SessionState::Connected);
}
