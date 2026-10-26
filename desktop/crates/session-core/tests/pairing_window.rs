use remotecam_protocol::ReasonCode;
use remotecam_protocol::signaling::{Limits, SignalingMessage};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource, epoch_source_starting_at};
use remotecam_session_core::{
    Effect, EpochSource, FIRST_MEDIA_FRAME_TIMEOUT_MILLIS, PAIRING_WINDOW_MILLIS, PairingError,
    PairingMaterial, SessionCore, SessionEvent, SessionState, Timestamp,
};

const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];

fn core_at(start_millis: u64) -> (SessionCore<ManualClock, SeededRandomSource>, ManualClock) {
    let clock = ManualClock::new(Timestamp::from_millis(start_millis));
    let handle = clock.clone();
    let core = SessionCore::new(clock, SeededRandomSource::new(7), DESKTOP_FINGERPRINT);
    (core, handle)
}

fn start(core: &mut SessionCore<ManualClock, SeededRandomSource>) -> PairingMaterial {
    let mut effects = core
        .handle(SessionEvent::StartPairing)
        .expect("starting pairing is valid");
    let Some(Effect::PresentPairing(material)) = effects.pop() else {
        panic!("starting pairing presents its material");
    };
    material
}

fn pair_frame(material: &PairingMaterial) -> Vec<u8> {
    SignalingMessage::Pair {
        token: *material.token().as_bytes(),
        dtls_fingerprint: [0x55; 32],
        limits: Limits::new(),
    }
    .encode()
    .expect("the pair message is valid")
}

fn assert_refused_and_replaced(
    effects: Vec<Effect>,
    reason: ReasonCode,
    epoch: u32,
) -> PairingMaterial {
    let [refusal, presented]: [Effect; 2] = effects.try_into().expect("two effects");
    let Effect::PresentPairing(fresh) = presented else {
        panic!("a refused attempt is followed by fresh material");
    };
    assert_eq!(refusal, Effect::RejectPeer { reason });
    assert_eq!(fresh.epoch(), epoch);
    fresh
}

#[test]
fn a_qr_is_accepted_just_before_its_window_closes() {
    let (mut core, clock) = core_at(0);
    start(&mut core);
    clock.advance(PAIRING_WINDOW_MILLIS - 1);

    core.handle(SessionEvent::PeerConnected)
        .expect("the window is still open");

    assert_eq!(core.state(), SessionState::Handshaking);
}

#[test]
fn an_expired_qr_is_refused_before_the_handshake_and_replaced() {
    let (mut core, clock) = core_at(0);
    let first = start(&mut core);
    clock.advance(PAIRING_WINDOW_MILLIS);

    let effects = core
        .handle(SessionEvent::PeerConnected)
        .expect("an expired qr is answered, not rejected as a transition");
    let fresh = assert_refused_and_replaced(effects, ReasonCode::QrExpired, 2);

    assert_eq!(core.state(), SessionState::PairingReady);
    assert_eq!(core.epoch(), Some(2));
    assert_ne!(fresh.token().as_bytes(), first.token().as_bytes());
    assert_ne!(fresh.session_id(), first.session_id());
}

#[test]
fn an_expired_token_is_refused_at_authentication_and_replaced() {
    let (mut core, clock) = core_at(0);
    let first = start(&mut core);
    core.handle(SessionEvent::PeerConnected)
        .expect("the qr is still valid when the peer connects");
    clock.advance(PAIRING_WINDOW_MILLIS);

    let effects = core
        .authenticate_first_frame(&pair_frame(&first))
        .expect("an expired token is answered, not rejected as a pairing error");

    assert_refused_and_replaced(effects, ReasonCode::TokenExpired, 2);
    assert_eq!(core.state(), SessionState::PairingReady);
}

#[test]
fn a_clock_that_moves_backwards_does_not_invalidate_a_new_session() {
    let (mut core, clock) = core_at(1_000);
    let material = start(&mut core);
    clock.set(Timestamp::from_millis(500));

    core.handle(SessionEvent::PeerConnected)
        .expect("a backwards clock keeps the window open");
    let effects = core
        .authenticate_first_frame(&pair_frame(&material))
        .expect("the matching token authenticates");

    assert_eq!(core.state(), SessionState::Negotiating);
    assert!(matches!(effects.as_slice(), [Effect::SendPaired { .. }]));
}

#[test]
fn a_frame_with_a_stale_epoch_is_dropped_and_counted() {
    let (mut core, clock) = core_at(0);
    start(&mut core);

    assert!(core.admit_frame_epoch(1));
    assert_eq!(core.protocol_violations(), 0);

    clock.advance(PAIRING_WINDOW_MILLIS);
    core.handle(SessionEvent::PeerConnected)
        .expect("an expired qr is answered with fresh material");

    assert!(!core.admit_frame_epoch(1));
    assert_eq!(core.protocol_violations(), 1);
    assert!(core.admit_frame_epoch(2));
    assert_eq!(core.protocol_violations(), 1);
}

#[test]
fn a_new_core_starts_its_epoch_sequence_at_one() {
    let (mut first, _) = core_at(0);
    let (mut second, _) = core_at(0);

    assert_eq!(start(&mut first).epoch(), 1);
    assert_eq!(start(&mut second).epoch(), 1);
}

#[test]
fn a_consumed_token_is_refused_with_token_already_used_after_the_session_ends() {
    let (mut core, clock) = core_at(0);
    let material = start(&mut core);
    core.handle(SessionEvent::PeerConnected)
        .expect("the qr is still valid when the peer connects");
    core.authenticate_first_frame(&pair_frame(&material))
        .expect("the matching token authenticates");
    core.handle(SessionEvent::NegotiationCompleted)
        .expect("negotiation starts the first-frame timer");
    clock.advance(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS + 1);
    core.handle(SessionEvent::FirstFrameTimedOut)
        .expect("the first frame never arrived");

    let refusal = core.authenticate_first_frame(&pair_frame(&material));

    assert_eq!(refusal, Err(PairingError::TokenAlreadyUsed));
    assert_eq!(core.state(), SessionState::Idle);
}

#[test]
fn cores_sharing_one_epoch_source_never_reuse_an_epoch() {
    let epochs = EpochSource::new();
    let clock = ManualClock::new(Timestamp::ZERO);
    let mut first = SessionCore::with_epoch_source(
        clock.clone(),
        SeededRandomSource::new(7),
        DESKTOP_FINGERPRINT,
        epochs.clone(),
    );
    let mut second = SessionCore::with_epoch_source(
        clock,
        SeededRandomSource::new(8),
        DESKTOP_FINGERPRINT,
        epochs,
    );

    assert_eq!(start(&mut first).epoch(), 1);
    assert_eq!(start(&mut second).epoch(), 2);
}

#[test]
fn a_spent_epoch_range_refuses_new_material_without_leaving_idle() {
    let epochs = epoch_source_starting_at(u32::MAX - 1);
    let clock = ManualClock::new(Timestamp::ZERO);
    let mut last_issued = SessionCore::with_epoch_source(
        clock.clone(),
        SeededRandomSource::new(7),
        DESKTOP_FINGERPRINT,
        epochs.clone(),
    );
    let mut refused = SessionCore::with_epoch_source(
        clock,
        SeededRandomSource::new(8),
        DESKTOP_FINGERPRINT,
        epochs,
    );

    assert_eq!(start(&mut last_issued).epoch(), u32::MAX - 1);
    assert!(refused.handle(SessionEvent::StartPairing).is_err());
    assert_eq!(refused.state(), SessionState::Idle);
}
