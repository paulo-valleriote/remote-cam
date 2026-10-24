use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{
    Clock, Effect, RandomSource, SessionCore, SessionEvent, SessionId, SessionState, Timestamp,
    TransitionErrorCode,
};

const HAPPY_PATH: [SessionEvent; 5] = [
    SessionEvent::StartPairing,
    SessionEvent::PeerConnected,
    SessionEvent::PairAuthenticated,
    SessionEvent::NegotiationCompleted,
    SessionEvent::EndSession,
];

fn core_at(
    seed: u64,
    start_millis: u64,
) -> (SessionCore<ManualClock, SeededRandomSource>, ManualClock) {
    let clock = ManualClock::new(Timestamp::from_millis(start_millis));
    let handle = clock.clone();
    let core = SessionCore::new(clock, SeededRandomSource::new(seed));
    (core, handle)
}

fn core_in(
    state: SessionState,
    seed: u64,
) -> (SessionCore<ManualClock, SeededRandomSource>, ManualClock) {
    let (mut core, clock) = core_at(seed, 0);
    let events: &[SessionEvent] = match state {
        SessionState::Idle => &[],
        SessionState::PairingReady => &HAPPY_PATH[..1],
        SessionState::Handshaking => &HAPPY_PATH[..2],
        SessionState::Negotiating => &HAPPY_PATH[..3],
        SessionState::Connected => &HAPPY_PATH[..4],
        SessionState::Ended => &HAPPY_PATH[..5],
    };

    for event in events {
        core.handle(*event).expect("the setup path is valid");
    }

    (core, clock)
}

#[test]
fn a_new_core_rests_in_idle_without_a_session() {
    let (core, _clock) = core_at(7, 0);

    assert_eq!(core.state(), SessionState::Idle);
    assert_eq!(core.session_id(), None);
    assert_eq!(core.last_transition_at(), Timestamp::from_millis(0));
}

#[test]
fn starting_a_pairing_moves_to_pairing_ready_and_presents_the_material() {
    let (mut core, _clock) = core_at(7, 1_000);

    let effects = core
        .handle(SessionEvent::StartPairing)
        .expect("starting from idle is a valid transition");
    let session_id = core
        .session_id()
        .expect("starting a pairing assigns a session id");

    assert_eq!(core.state(), SessionState::PairingReady);
    assert_eq!(effects, vec![Effect::PresentPairing { session_id }]);
    assert_eq!(core.last_transition_at(), Timestamp::from_millis(1_000));
}

#[test]
fn the_same_seed_and_clock_assign_the_same_session_id() {
    let (mut first, _) = core_at(42, 0);
    let (mut second, _) = core_at(42, 0);
    let (mut other, _) = core_at(43, 0);

    let first_effects = first
        .handle(SessionEvent::StartPairing)
        .expect("starting from idle is valid");
    let second_effects = second
        .handle(SessionEvent::StartPairing)
        .expect("starting from idle is valid");
    other
        .handle(SessionEvent::StartPairing)
        .expect("starting from idle is valid");

    assert_eq!(first.session_id(), second.session_id());
    assert_eq!(first_effects, second_effects);
    assert_ne!(first.session_id(), other.session_id());
}

#[test]
fn a_peer_connection_moves_to_handshaking_without_an_effect() {
    let (mut core, clock) = core_in(SessionState::PairingReady, 7);
    clock.advance(250);

    let effects = core
        .handle(SessionEvent::PeerConnected)
        .expect("a peer may connect while pairing is ready");

    assert_eq!(core.state(), SessionState::Handshaking);
    assert_eq!(effects, Vec::new());
    assert_eq!(core.last_transition_at(), Timestamp::from_millis(250));
}

#[test]
fn an_authenticated_pair_sends_paired_and_moves_to_negotiating() {
    let (mut core, clock) = core_in(SessionState::Handshaking, 7);
    clock.advance(15);

    let effects = core
        .handle(SessionEvent::PairAuthenticated)
        .expect("authentication is valid during the handshake");

    assert_eq!(core.state(), SessionState::Negotiating);
    assert_eq!(effects, vec![Effect::SendPaired]);
    assert_eq!(core.last_transition_at(), Timestamp::from_millis(15));
}

#[test]
fn a_completed_negotiation_starts_media_and_connects() {
    let (mut core, clock) = core_in(SessionState::Negotiating, 7);
    clock.advance(40);

    let effects = core
        .handle(SessionEvent::NegotiationCompleted)
        .expect("negotiation completes after offer and answer");

    assert_eq!(core.state(), SessionState::Connected);
    assert_eq!(effects, vec![Effect::StartMedia]);
    assert_eq!(core.last_transition_at(), Timestamp::from_millis(40));
}

#[test]
fn ending_a_connected_session_moves_to_ended() {
    let (mut core, clock) = core_in(SessionState::Connected, 7);
    clock.advance(5);

    let effects = core
        .handle(SessionEvent::EndSession)
        .expect("a connected session can be ended");

    assert_eq!(core.state(), SessionState::Ended);
    assert_eq!(effects, vec![Effect::End]);
    assert_eq!(core.last_transition_at(), Timestamp::from_millis(5));
}

#[test]
fn the_happy_path_walks_every_state_in_order() {
    let (mut core, _clock) = core_at(7, 0);
    let mut states = vec![core.state()];
    let mut effects = Vec::new();

    for event in HAPPY_PATH {
        effects.push(core.handle(event).expect("the happy path is valid"));
        states.push(core.state());
    }

    assert_eq!(
        states,
        [
            SessionState::Idle,
            SessionState::PairingReady,
            SessionState::Handshaking,
            SessionState::Negotiating,
            SessionState::Connected,
            SessionState::Ended,
        ]
    );

    let session_id = core.session_id().expect("the path assigned a session id");
    assert_eq!(effects[0], vec![Effect::PresentPairing { session_id }]);
    assert_eq!(effects[1], Vec::new());
    assert_eq!(effects[2], vec![Effect::SendPaired]);
    assert_eq!(effects[3], vec![Effect::StartMedia]);
    assert_eq!(effects[4], vec![Effect::End]);
}

#[test]
fn an_out_of_order_event_is_rejected_with_a_closed_code_and_keeps_the_state() {
    let cases = [
        (SessionState::Idle, SessionEvent::PeerConnected),
        (SessionState::Idle, SessionEvent::EndSession),
        (SessionState::PairingReady, SessionEvent::StartPairing),
        (SessionState::PairingReady, SessionEvent::PairAuthenticated),
        (
            SessionState::Handshaking,
            SessionEvent::NegotiationCompleted,
        ),
        (SessionState::Negotiating, SessionEvent::EndSession),
        (SessionState::Connected, SessionEvent::StartPairing),
        (SessionState::Ended, SessionEvent::StartPairing),
        (SessionState::Ended, SessionEvent::EndSession),
    ];

    for (state, event) in cases {
        let (mut core, clock) = core_in(state, 7);
        let session_id = core.session_id();
        clock.advance(100);

        let error = core
            .handle(event)
            .expect_err("an out-of-order event is rejected");

        assert_eq!(error.code(), TransitionErrorCode::InvalidTransition);
        assert_eq!(error.from_state(), state);
        assert_eq!(error.event(), event);
        assert_eq!(core.state(), state);
        assert_eq!(core.session_id(), session_id);
        assert_eq!(core.last_transition_at(), Timestamp::from_millis(0));
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Trace {
    session_id: Option<SessionId>,
    effects: Vec<Vec<Effect>>,
    timestamps: Vec<Timestamp>,
}

fn run_path(seed: u64, start_millis: u64) -> Trace {
    let (mut core, clock) = core_at(seed, start_millis);
    let mut effects = Vec::new();
    let mut timestamps = Vec::new();

    for event in HAPPY_PATH {
        clock.advance(10);
        effects.push(core.handle(event).expect("the happy path is valid"));
        timestamps.push(core.last_transition_at());
    }

    Trace {
        session_id: core.session_id(),
        effects,
        timestamps,
    }
}

#[test]
fn the_same_seed_and_clock_produce_the_same_effects_and_timestamps() {
    assert_eq!(run_path(42, 0), run_path(42, 0));
}

#[test]
fn a_different_seed_changes_the_session_id_but_not_the_transition_timestamps() {
    let first = run_path(42, 0);
    let other = run_path(43, 0);

    assert_ne!(first.session_id, other.session_id);
    assert_eq!(first.timestamps, other.timestamps);
}

#[test]
fn the_injected_clock_start_shifts_every_transition_timestamp() {
    let trace = run_path(42, 1_000);

    assert_eq!(
        trace.timestamps,
        [
            Timestamp::from_millis(1_010),
            Timestamp::from_millis(1_020),
            Timestamp::from_millis(1_030),
            Timestamp::from_millis(1_040),
            Timestamp::from_millis(1_050),
        ]
    );
}

#[test]
fn the_manual_clock_only_moves_when_advanced() {
    let clock = ManualClock::new(Timestamp::from_millis(1_000));

    assert_eq!(clock.now(), Timestamp::from_millis(1_000));

    clock.advance(250);

    assert_eq!(clock.now(), Timestamp::from_millis(1_250));
}

#[test]
fn the_seeded_source_repeats_its_stream_and_mixes_the_whole_buffer() {
    let mut first = SeededRandomSource::new(42);
    let mut second = SeededRandomSource::new(42);
    let mut different = SeededRandomSource::new(43);
    let mut first_bytes = [0u8; 32];
    let mut second_bytes = [0u8; 32];
    let mut different_bytes = [0u8; 32];

    first.fill_bytes(&mut first_bytes);
    second.fill_bytes(&mut second_bytes);
    different.fill_bytes(&mut different_bytes);

    assert_eq!(first_bytes, second_bytes);
    assert_ne!(first_bytes, different_bytes);
    assert_eq!(
        u64::from_le_bytes(first_bytes[0..8].try_into().expect("eight bytes")),
        0xbdd7_3226_2feb_6e95
    );

    let mut tail = [0u8; 19];
    first.fill_bytes(&mut tail);

    assert_ne!(tail, [0u8; 19]);
}

#[test]
fn a_session_id_round_trips_its_bytes() {
    let bytes = [7u8; SessionId::LENGTH];
    let session_id = SessionId::from_bytes(bytes);

    assert_eq!(session_id.as_bytes(), &bytes);
}

#[test]
fn a_timestamp_exposes_its_milliseconds_and_zero_point() {
    assert_eq!(Timestamp::ZERO.as_millis(), 0);
    assert_eq!(Timestamp::from_millis(1_234).as_millis(), 1_234);
}
