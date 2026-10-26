#[allow(dead_code)]
mod common;

use common::pair_frame;
use remotecam_protocol::ReasonCode;
use remotecam_protocol::signaling::{
    AnswerSdp, CloseCode, ErrorCode, Limits, MAX_SIGNALING_MESSAGE_BYTES, OfferSdp,
    SignalingMessage,
};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{
    Effect, PairingError, SessionCore, SessionEvent, SessionState, Timestamp,
};

const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];
const SENDONLY_VIDEO: &str = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\n";

type Core = SessionCore<ManualClock, SeededRandomSource>;

fn pairing_core() -> (Core, [u8; 32]) {
    let mut core = SessionCore::new(
        ManualClock::new(Timestamp::ZERO),
        SeededRandomSource::new(7),
        DESKTOP_FINGERPRINT,
    );
    let effects = core
        .handle(SessionEvent::StartPairing)
        .expect("starting pairing is valid");
    let [Effect::PresentPairing(material)] = effects.as_slice() else {
        panic!("starting pairing presents its material");
    };
    let token = *material.token().as_bytes();
    core.handle(SessionEvent::PeerConnected)
        .expect("the peer may connect while pairing is ready");
    (core, token)
}

fn negotiating_core() -> Core {
    let (mut core, token) = pairing_core();
    core.authenticate_first_frame(&pair_frame(token))
        .expect("the matching token authenticates");
    assert_eq!(core.state(), SessionState::Negotiating);
    core
}

fn padded_ping(length: usize) -> Vec<u8> {
    let mut frame = SignalingMessage::Ping.encode().expect("ping encodes");
    frame.resize(length, b' ');
    frame
}

#[test]
fn a_frame_at_the_size_limit_is_admitted() {
    let mut core = negotiating_core();

    let message = core
        .admit_signaling_frame(&padded_ping(MAX_SIGNALING_MESSAGE_BYTES))
        .expect("the limit itself is allowed");

    assert_eq!(message, SignalingMessage::Ping);
}

#[test]
fn a_frame_above_the_size_limit_is_refused_and_ends_the_session() {
    let mut core = negotiating_core();

    let refusal = core
        .admit_signaling_frame(&padded_ping(MAX_SIGNALING_MESSAGE_BYTES + 1))
        .expect_err("one byte over the limit is refused");

    assert_eq!(refusal.code(), ReasonCode::SecurityViolation);
    assert_eq!(
        refusal.effects(),
        [
            Effect::SendClose {
                reason: CloseCode::SecurityViolation
            },
            Effect::End
        ]
    );
    assert_eq!(core.state(), SessionState::Ended);
}

#[test]
fn an_oversized_frame_ends_a_connected_session_too() {
    let mut core = negotiating_core();
    core.handle(SessionEvent::NegotiationCompleted).unwrap();
    core.handle(SessionEvent::FirstMediaFrame(
        remotecam_session_core::MediaFrame {
            sequence: 0,
            timestamp: Timestamp::ZERO,
        },
    ))
    .unwrap();
    assert_eq!(core.state(), SessionState::Connected);

    let refusal = core
        .admit_signaling_frame(&vec![b' '; MAX_SIGNALING_MESSAGE_BYTES * 2])
        .expect_err("an oversized frame is refused");

    assert_eq!(refusal.code(), ReasonCode::SecurityViolation);
    assert_eq!(core.state(), SessionState::Ended);
}

#[test]
fn a_frame_outside_the_schema_is_refused_with_one_code_and_keeps_the_session() {
    let cases: [(&[u8], ReasonCode); 4] = [
        (b"{not json", ReasonCode::InvalidMessage),
        (
            br#"{"version":1,"type":"ping","extra":1}"#,
            ReasonCode::InvalidMessage,
        ),
        (
            br#"{"version":1,"type":"hello"}"#,
            ReasonCode::InvalidMessage,
        ),
        (
            br#"{"version":2,"type":"ping"}"#,
            ReasonCode::UnsupportedVersion,
        ),
    ];

    for (frame, expected) in cases {
        let mut core = negotiating_core();

        let refusal = core
            .admit_signaling_frame(frame)
            .expect_err("the frame is outside the contract");

        assert_eq!(refusal.code(), expected);
        assert!(refusal.effects().is_empty());
        assert_eq!(core.state(), SessionState::Negotiating);
    }
}

#[test]
fn an_offer_with_forbidden_media_is_refused_before_it_reaches_the_session() {
    let forbidden = [
        "v=0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n",
        "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendrecv\r\n",
        "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=recvonly\r\n",
        "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\na=candidate:1 1 udp 1 8.8.8.8 5000 typ host\r\n",
        "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\na=candidate:1 1 udp 1 192.168.1.2 5000 typ srflx\r\n",
    ];

    for sdp in forbidden {
        let mut core = negotiating_core();
        let frame = format!(r#"{{"version":1,"type":"offer","epoch":1,"sdp":{sdp:?}}}"#);

        let refusal = core
            .admit_signaling_frame(frame.as_bytes())
            .expect_err("the offer asks for something forbidden");

        assert_eq!(refusal.code(), ReasonCode::InvalidMessage, "{sdp}");
        assert_eq!(core.state(), SessionState::Negotiating);
    }
}

#[test]
fn a_conforming_offer_is_admitted() {
    let mut core = negotiating_core();
    let frame = SignalingMessage::Offer {
        epoch: 1,
        sdp: OfferSdp::new(SENDONLY_VIDEO).unwrap(),
    }
    .encode()
    .unwrap();

    let message = core.admit_signaling_frame(&frame).expect("a valid offer");

    assert!(matches!(message, SignalingMessage::Offer { epoch: 1, .. }));
}

#[test]
fn messages_that_only_the_desktop_sends_are_refused_when_received() {
    let desktop_to_mobile = [
        SignalingMessage::Answer {
            epoch: 1,
            sdp: AnswerSdp::new("v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=recvonly\r\n").unwrap(),
        },
        SignalingMessage::Paired {
            session_id: [1; 16],
            epoch: 1,
            dtls_fingerprint: [2; 32],
            accepted_limits: Limits::new(),
        },
        SignalingMessage::Error {
            code: ErrorCode::InvalidMessage,
            message: None,
        },
    ];

    for message in desktop_to_mobile {
        let mut core = negotiating_core();

        let refusal = core
            .admit_signaling_frame(&message.encode().unwrap())
            .expect_err("the mobile never sends this message");

        assert_eq!(refusal.code(), ReasonCode::InvalidMessage, "{message:?}");
    }
}

#[test]
fn a_second_pair_frame_on_an_established_session_is_a_protocol_violation() {
    let mut core = negotiating_core();

    let refusal = core
        .admit_signaling_frame(&pair_frame([9; 32]))
        .expect_err("pair is only valid as the first frame");

    assert_eq!(refusal.code(), ReasonCode::ProtocolViolation);
    assert_eq!(core.state(), SessionState::Negotiating);
}

#[test]
fn a_signaling_frame_without_an_established_session_is_a_protocol_violation() {
    let (mut core, _token) = pairing_core();

    let refusal = core
        .admit_signaling_frame(&padded_ping(0))
        .expect_err("the peer has not authenticated yet");

    assert_eq!(refusal.code(), ReasonCode::ProtocolViolation);
    assert_eq!(core.state(), SessionState::Handshaking);
}

#[test]
fn an_oversized_first_frame_is_refused_with_security_violation() {
    let (mut core, _token) = pairing_core();

    let error = core
        .authenticate_first_frame(&vec![b' '; MAX_SIGNALING_MESSAGE_BYTES + 1])
        .expect_err("an oversized first frame cannot authenticate");

    assert_eq!(error, PairingError::SecurityViolation);
    assert_eq!(error.code(), ReasonCode::SecurityViolation);
    assert_eq!(core.state(), SessionState::Ended);
}

#[test]
fn a_first_frame_other_than_pair_is_a_protocol_violation() {
    let (mut core, _token) = pairing_core();

    let error = core
        .authenticate_first_frame(&SignalingMessage::Ping.encode().unwrap())
        .expect_err("pair must come first");

    assert_eq!(error, PairingError::ProtocolViolation);
    assert_eq!(core.state(), SessionState::Ended);
}

#[test]
fn an_oversized_frame_is_a_security_violation_whatever_the_state() {
    let (mut core, _token) = pairing_core();

    let refusal = core
        .admit_signaling_frame(&vec![b' '; MAX_SIGNALING_MESSAGE_BYTES + 1])
        .expect_err("an oversized frame is refused before any state check");

    assert_eq!(refusal.code(), ReasonCode::SecurityViolation);
    assert_eq!(core.state(), SessionState::Handshaking);
}
