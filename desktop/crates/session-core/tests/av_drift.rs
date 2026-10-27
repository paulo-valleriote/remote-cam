#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::pair_frame;
use remotecam_protocol::signaling::CloseCode;
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{
    AV_DRIFT_LIMIT, AvDrift, Effect, MediaFrame, SessionCore, SessionEvent, SessionState,
    Timestamp, TransitionErrorCode,
};

const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];

fn connected() -> SessionCore<ManualClock, SeededRandomSource> {
    let clock = ManualClock::new(Timestamp::ZERO);
    let mut core = SessionCore::new(clock, SeededRandomSource::new(9), DESKTOP_FINGERPRINT);
    let effects = core.handle(SessionEvent::StartPairing).expect("pairing");
    let [Effect::PresentPairing(material)] = effects.as_slice() else {
        panic!("pairing material");
    };
    let frame = pair_frame(*material.token().as_bytes());
    core.handle(SessionEvent::PeerConnected).expect("peer");
    core.authenticate_first_frame(&frame).expect("auth");
    core.handle(SessionEvent::NegotiationCompleted)
        .expect("negotiated");
    core.handle(SessionEvent::FirstMediaFrame(MediaFrame {
        sequence: 0,
        timestamp: Timestamp::ZERO,
    }))
    .expect("first frame");
    assert_eq!(core.state(), SessionState::Connected);
    core
}

fn drift(millis: u64) -> SessionEvent {
    SessionEvent::AvDrift(AvDrift::from_duration(Duration::from_millis(millis)))
}

#[test]
fn the_hard_limit_is_one_hundred_milliseconds() {
    assert_eq!(AV_DRIFT_LIMIT, Duration::from_millis(100));
}

#[test]
fn a_drift_of_exactly_one_hundred_milliseconds_keeps_the_session_connected() {
    let mut core = connected();

    let effects = core.handle(drift(100)).expect("drift is a valid event");

    assert!(effects.is_empty());
    assert_eq!(core.state(), SessionState::Connected);
}

#[test]
fn a_drift_of_one_hundred_and_one_milliseconds_ends_with_media_failure() {
    let mut core = connected();

    let effects = core.handle(drift(101)).expect("drift is a valid event");

    assert_eq!(
        effects,
        vec![
            Effect::SendClose {
                reason: CloseCode::MediaFailure
            },
            Effect::End
        ],
        "the close frame goes out before the resources are released"
    );
    assert_eq!(core.state(), SessionState::Ended);
}

#[test]
fn a_drift_below_the_target_never_acts() {
    let mut core = connected();
    for millis in [0, 1, 20, 99] {
        assert!(core.handle(drift(millis)).expect("valid").is_empty());
    }
    assert_eq!(core.state(), SessionState::Connected);
}

#[test]
fn a_drift_after_the_session_ended_is_an_invalid_transition() {
    let mut core = connected();
    core.handle(drift(500)).expect("ends");

    let error = core.handle(drift(500)).expect_err("already ended");

    assert_eq!(error.code(), TransitionErrorCode::InvalidTransition);
    assert_eq!(core.state(), SessionState::Ended);
}

#[test]
fn a_drift_before_the_session_is_connected_is_rejected() {
    let clock = ManualClock::new(Timestamp::ZERO);
    let mut core = SessionCore::new(clock, SeededRandomSource::new(1), DESKTOP_FINGERPRINT);

    let error = core.handle(drift(500)).expect_err("idle");

    assert_eq!(error.from_state(), SessionState::Idle);
}
