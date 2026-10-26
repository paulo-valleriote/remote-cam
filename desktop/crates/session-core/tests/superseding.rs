use remotecam_protocol::ReasonCode;
use remotecam_protocol::signaling::{CloseCode, Limits, SignalingMessage};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{
    Effect, MediaFrame, PAIRING_WINDOW_MILLIS, PairingError, PairingMaterial, SessionCore,
    SessionEvent, SessionId, SessionState, Timestamp,
};

const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];
const PEER_FINGERPRINT: [u8; 32] = [0x55; 32];
const RIVAL_PEER_FINGERPRINT: [u8; 32] = [0x66; 32];

type Core = SessionCore<ManualClock, SeededRandomSource>;

struct Offer {
    session_id: SessionId,
    epoch: u32,
    token: [u8; 32],
}

impl Offer {
    fn of(material: &PairingMaterial) -> Self {
        Self {
            session_id: material.session_id(),
            epoch: material.epoch(),
            token: *material.token().as_bytes(),
        }
    }

    fn pair_frame(&self) -> Vec<u8> {
        pair_frame_with_token(self.token)
    }
}

fn core_at(start_millis: u64) -> (Core, ManualClock) {
    let clock = ManualClock::new(Timestamp::from_millis(start_millis));
    let handle = clock.clone();
    let core = SessionCore::new(clock, SeededRandomSource::new(7), DESKTOP_FINGERPRINT);
    (core, handle)
}

fn present_pairing(core: &mut Core) -> Offer {
    let mut effects = core
        .handle(SessionEvent::StartPairing)
        .expect("starting pairing is valid");
    let Some(Effect::PresentPairing(material)) = effects.pop() else {
        panic!("starting pairing presents its material");
    };
    Offer::of(&material)
}

fn pair_frame_with_token(token: [u8; 32]) -> Vec<u8> {
    pair_frame_from_peer(token, PEER_FINGERPRINT)
}

fn pair_frame_from_peer(token: [u8; 32], dtls_fingerprint: [u8; 32]) -> Vec<u8> {
    SignalingMessage::Pair {
        token,
        dtls_fingerprint,
        limits: Limits::new(),
    }
    .encode()
    .expect("the pair message is valid")
}

fn connected_core() -> (Core, ManualClock, Offer) {
    let (mut core, clock) = core_at(0);
    let first = present_pairing(&mut core);
    core.handle(SessionEvent::PeerConnected)
        .expect("the first peer can connect");
    core.authenticate_first_frame(&first.pair_frame())
        .expect("the first peer authenticates");
    core.handle(SessionEvent::NegotiationCompleted)
        .expect("negotiation completes");
    core.handle(SessionEvent::FirstMediaFrame(MediaFrame {
        sequence: 0,
        timestamp: Timestamp::ZERO,
    }))
    .expect("the first frame arrives on time");
    assert_eq!(core.state(), SessionState::Connected);
    (core, clock, first)
}

#[test]
fn starting_pairing_while_connected_presents_a_new_qr_and_keeps_the_predecessor() {
    let (mut core, _clock, _first) = connected_core();

    let effects = core
        .handle(SessionEvent::StartPairing)
        .expect("a connected core accepts a new pairing");

    assert!(matches!(effects.as_slice(), [Effect::PresentPairing(_)]));
    assert_eq!(core.state(), SessionState::Connected);
    assert_eq!(core.retired_state(), None);
}

#[test]
fn a_valid_successor_pair_closes_the_predecessor_before_it_is_paired() {
    let (mut core, _clock, first) = connected_core();
    let successor = present_pairing(&mut core);
    core.handle(SessionEvent::PeerConnected)
        .expect("the successor peer can connect");

    let effects = core
        .authenticate_first_frame(&successor.pair_frame())
        .expect("the successor's token authenticates");

    assert_eq!(
        effects,
        vec![
            Effect::SendClose {
                reason: CloseCode::Superseded,
            },
            Effect::End,
            Effect::SendPaired {
                session_id: successor.session_id,
                epoch: successor.epoch,
                dtls_fingerprint: DESKTOP_FINGERPRINT,
                accepted_limits: Limits::new(),
            },
        ]
    );
    assert_eq!(core.state(), SessionState::Negotiating);
    assert_eq!(core.retired_state(), Some(SessionState::Superseded));
    assert_ne!(successor.session_id, first.session_id);
    assert!(successor.epoch > first.epoch);
}

#[test]
fn an_unused_qr_that_expires_keeps_the_predecessor_connected() {
    let (mut core, clock, _first) = connected_core();
    present_pairing(&mut core);
    clock.advance(PAIRING_WINDOW_MILLIS);

    let effects = core
        .handle(SessionEvent::PeerConnected)
        .expect("an expired successor qr is refused");

    assert_eq!(
        effects,
        vec![Effect::RejectPeer {
            reason: ReasonCode::QrExpired,
        }]
    );
    assert_eq!(core.state(), SessionState::Connected);
    assert_eq!(core.retired_state(), None);
}

#[test]
fn a_wrong_token_for_the_successor_leaves_the_predecessor_connected() {
    let (mut core, _clock, _first) = connected_core();
    let successor = present_pairing(&mut core);
    core.handle(SessionEvent::PeerConnected)
        .expect("the successor peer can connect");
    let mut wrong = successor.token;
    wrong[0] ^= 1;

    let error = core
        .authenticate_first_frame(&pair_frame_with_token(wrong))
        .expect_err("a token that is not the successor's is refused");

    assert_eq!(error, PairingError::TokenInvalid);
    assert_eq!(core.state(), SessionState::Connected);
    assert_eq!(core.retired_state(), None);
}

#[test]
fn a_pair_frame_before_the_successor_connects_does_not_end_the_predecessor() {
    let (mut core, _clock, _first) = connected_core();
    let successor = present_pairing(&mut core);

    let error = core
        .authenticate_first_frame(&successor.pair_frame())
        .expect_err("the successor peer must connect before pairing");

    assert_eq!(error, PairingError::ProtocolViolation);
    assert_eq!(core.state(), SessionState::Connected);
}

#[test]
fn a_consumed_token_presented_while_the_successor_is_active_is_refused_as_superseded() {
    let (mut core, _clock, _first) = connected_core();
    let successor = present_pairing(&mut core);
    core.handle(SessionEvent::PeerConnected)
        .expect("the successor peer can connect");
    let frame = successor.pair_frame();
    core.authenticate_first_frame(&frame)
        .expect("the successor authenticates");

    assert_eq!(
        core.authenticate_first_frame(&frame),
        Err(PairingError::Superseded)
    );
    assert_eq!(core.state(), SessionState::Negotiating);
}

#[test]
fn two_peers_racing_for_the_same_token_let_only_the_first_valid_frame_win() {
    let (mut core, _clock, _first) = connected_core();
    let successor = present_pairing(&mut core);
    core.handle(SessionEvent::PeerConnected)
        .expect("the winning peer can connect");
    core.handle(SessionEvent::PeerConnected)
        .expect("a rival peer can connect to the same qr");
    let winner = pair_frame_from_peer(successor.token, PEER_FINGERPRINT);
    let loser = pair_frame_from_peer(successor.token, RIVAL_PEER_FINGERPRINT);

    let effects = core
        .authenticate_first_frame(&winner)
        .expect("the first valid token wins");

    assert_eq!(
        effects,
        vec![
            Effect::SendClose {
                reason: CloseCode::Superseded,
            },
            Effect::End,
            Effect::SendPaired {
                session_id: successor.session_id,
                epoch: successor.epoch,
                dtls_fingerprint: DESKTOP_FINGERPRINT,
                accepted_limits: Limits::new(),
            },
        ]
    );

    let refusal = core
        .authenticate_first_frame(&loser)
        .expect_err("the rival's consumed token is refused");

    assert_eq!(refusal, PairingError::Superseded);
    assert_eq!(refusal.code(), ReasonCode::Superseded);
    assert_eq!(core.state(), SessionState::Negotiating);
    assert_eq!(core.session_id(), Some(successor.session_id));
    assert_eq!(core.epoch(), Some(successor.epoch));
    assert_eq!(core.retired_state(), Some(SessionState::Superseded));
}

#[test]
fn the_close_effect_renders_its_reason_for_logs() {
    let effect = Effect::SendClose {
        reason: CloseCode::Superseded,
    };

    assert_eq!(format!("{effect:?}"), "SendClose { reason: Superseded }");
}
