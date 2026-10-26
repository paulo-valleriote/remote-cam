//! RemoteCam Session Core: the product session state machine and vocabulary.

use std::fmt;
use std::sync::Arc;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use remotecam_protocol::ReasonCode;
use remotecam_protocol::signaling::{CloseCode, Limits, SignalingMessage, parse_signaling};
use zeroize::Zeroize;

pub mod testing;
pub mod token;

pub use token::PairingToken;

/// The lifecycle position of the single session a desktop may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    Idle,
    PairingReady,
    Handshaking,
    Negotiating,
    Connected,
    Ended,
    Superseded,
}

impl SessionState {
    /// The stable lowercase label used by reports and structured logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::PairingReady => "pairing_ready",
            Self::Handshaking => "handshaking",
            Self::Negotiating => "negotiating",
            Self::Connected => "connected",
            Self::Ended => "ended",
            Self::Superseded => "superseded",
        }
    }
}

/// A point on the injected monotonic clock, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

impl Timestamp {
    /// The clock's zero point.
    pub const ZERO: Self = Self(0);

    /// Builds a timestamp from a millisecond count.
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// The millisecond count carried by the timestamp.
    pub const fn as_millis(self) -> u64 {
        self.0
    }
}

pub const FIRST_MEDIA_FRAME_TIMEOUT_MILLIS: u64 = 3_000;

pub const PAIRING_WINDOW_MILLIS: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaFrame {
    pub sequence: u64,
    pub timestamp: Timestamp,
}

pub trait MediaPort {
    fn start(&mut self);

    fn next_frame(&mut self) -> Option<MediaFrame>;

    fn stop(&mut self);
}

/// The time source the Session Core reads. It is injected, never global.
pub trait Clock {
    /// The current position of the clock.
    fn now(&self) -> Timestamp;
}

#[derive(Debug, Clone, Copy)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now(&self) -> Timestamp {
        let elapsed = u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX);
        Timestamp::from_millis(elapsed)
    }
}

#[derive(Debug, Clone)]
pub struct EpochSource(Arc<Mutex<Option<u32>>>);

impl EpochSource {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Some(1))))
    }

    fn issue(&self) -> Option<u32> {
        let mut next = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let epoch = (*next)?;
        *next = epoch.checked_add(1);
        Some(epoch)
    }
}

impl Default for EpochSource {
    fn default() -> Self {
        Self::new()
    }
}

/// The randomness source the Session Core draws from. It is injected, never global.
pub trait RandomSource {
    /// Fills `destination` with the source's next bytes.
    fn fill_bytes(&mut self, destination: &mut [u8]);
}

/// The identity assigned to one pairing attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId([u8; 16]);

impl SessionId {
    /// The number of bytes in a session identifier.
    pub const LENGTH: usize = 16;

    /// Builds an identifier from its raw bytes.
    pub const fn from_bytes(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    pub fn generate<R: RandomSource + ?Sized>(random: &mut R) -> Self {
        let mut bytes = [0_u8; Self::LENGTH];
        random.fill_bytes(&mut bytes);
        Self::from_bytes(bytes)
    }

    /// The raw bytes of the identifier.
    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }
}

/// The short-lived material the desktop presents for one pairing attempt.
#[derive(Debug, PartialEq, Eq)]
pub struct PairingMaterial {
    session_id: SessionId,
    epoch: u32,
    token: PairingToken,
}

impl PairingMaterial {
    /// The identity of the pairing attempt.
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// The process-local session generation.
    pub const fn epoch(&self) -> u32 {
        self.epoch
    }

    /// The single-use token to include in the QR payload.
    pub const fn token(&self) -> &PairingToken {
        &self.token
    }
}

/// An input the Session Core decides on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionEvent {
    /// The desktop user asked to start pairing.
    StartPairing,
    /// A peer opened the pinned channel.
    PeerConnected,
    /// Offer and answer completed negotiation.
    NegotiationCompleted,
    FirstMediaFrame(MediaFrame),
    FirstFrameTimedOut,
    /// One side ended an established session.
    EndSession,
}

/// An action an adapter performs after the Session Core decides.
#[derive(PartialEq, Eq)]
pub enum Effect {
    /// Show the pairing material for the freshly assigned session.
    PresentPairing(PairingMaterial),
    RejectPeer {
        reason: ReasonCode,
    },
    SendClose {
        reason: CloseCode,
    },
    /// Send the authenticated peer the `paired` frame.
    SendPaired {
        session_id: SessionId,
        epoch: u32,
        dtls_fingerprint: [u8; 32],
        accepted_limits: Limits,
    },
    StartMedia,
    /// Release the session resources.
    End,
}

impl fmt::Debug for Effect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PresentPairing(material) => formatter
                .debug_tuple("PresentPairing")
                .field(material)
                .finish(),
            Self::SendPaired {
                session_id,
                epoch,
                accepted_limits,
                ..
            } => formatter
                .debug_struct("SendPaired")
                .field("session_id", session_id)
                .field("epoch", epoch)
                .field("dtls_fingerprint", &"[REDACTED]")
                .field("accepted_limits", accepted_limits)
                .finish(),
            Self::RejectPeer { reason } => formatter
                .debug_struct("RejectPeer")
                .field("reason", reason)
                .finish(),
            Self::SendClose { reason } => formatter
                .debug_struct("SendClose")
                .field("reason", reason)
                .finish(),
            Self::StartMedia => formatter.write_str("StartMedia"),
            Self::End => formatter.write_str("End"),
        }
    }
}

/// A rejected first-frame pairing attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingError {
    ProtocolViolation,
    TokenInvalid,
    TokenAlreadyUsed,
    Superseded,
}

impl PairingError {
    /// The stable rejection reason exposed to the transport adapter.
    pub const fn code(self) -> ReasonCode {
        match self {
            Self::ProtocolViolation => ReasonCode::ProtocolViolation,
            Self::TokenInvalid => ReasonCode::TokenInvalid,
            Self::TokenAlreadyUsed => ReasonCode::TokenAlreadyUsed,
            Self::Superseded => ReasonCode::Superseded,
        }
    }
}

/// The closed code set for a rejected event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionErrorCode {
    /// The event does not apply in the current state.
    InvalidTransition,
}

impl TransitionErrorCode {
    /// The stable lowercase spelling used in reports and logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidTransition => "invalid_transition",
        }
    }
}

/// Why the Session Core rejected an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    /// The event does not apply in the current state.
    InvalidTransition {
        from: SessionState,
        event: SessionEvent,
    },
}

impl TransitionError {
    /// The closed code carried by the rejection.
    pub const fn code(self) -> TransitionErrorCode {
        match self {
            Self::InvalidTransition { .. } => TransitionErrorCode::InvalidTransition,
        }
    }

    /// The state the rejection was observed in.
    pub const fn from_state(self) -> SessionState {
        match self {
            Self::InvalidTransition { from, .. } => from,
        }
    }

    /// The event the rejection was observed for.
    pub const fn event(self) -> SessionEvent {
        match self {
            Self::InvalidTransition { event, .. } => event,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionIdentity {
    session_id: SessionId,
    epoch: u32,
}

#[derive(Debug)]
struct PendingOffer {
    identity: SessionIdentity,
    token: PairingToken,
    expires_at: Timestamp,
    peer_connected: bool,
}

/// The product session core. It owns the state machine and performs no I/O.
pub struct SessionCore<C: Clock, R: RandomSource> {
    state: SessionState,
    clock: C,
    random: R,
    active: Option<SessionIdentity>,
    offer: Option<PendingOffer>,
    retired: bool,
    consumed_tokens: Vec<PairingToken>,
    desktop_dtls_fingerprint: [u8; 32],
    last_transition_at: Timestamp,
    first_frame_deadline: Option<Timestamp>,
    epochs: EpochSource,
    protocol_violations: u64,
}

impl<C: Clock, R: RandomSource> SessionCore<C, R> {
    pub fn new(clock: C, random: R, desktop_dtls_fingerprint: [u8; 32]) -> Self {
        Self::with_epoch_source(clock, random, desktop_dtls_fingerprint, EpochSource::new())
    }

    pub fn with_epoch_source(
        clock: C,
        random: R,
        desktop_dtls_fingerprint: [u8; 32],
        epochs: EpochSource,
    ) -> Self {
        Self {
            state: SessionState::Idle,
            clock,
            random,
            active: None,
            offer: None,
            retired: false,
            consumed_tokens: Vec::new(),
            desktop_dtls_fingerprint,
            last_transition_at: Timestamp::ZERO,
            first_frame_deadline: None,
            epochs,
            protocol_violations: 0,
        }
    }

    pub fn protocol_violations(&self) -> u64 {
        self.protocol_violations
    }

    pub fn admit_frame_epoch(&mut self, frame_epoch: u32) -> bool {
        match self.current_identity() {
            Some(current) if frame_epoch < current.epoch => {
                self.protocol_violations += 1;
                false
            }
            _ => true,
        }
    }

    /// The current lifecycle position.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// The identity of the pairing in progress, if one has started.
    pub fn session_id(&self) -> Option<SessionId> {
        self.current_identity().map(|identity| identity.session_id)
    }

    /// The generation assigned to the current pairing, if one has started.
    pub fn epoch(&self) -> Option<u32> {
        self.current_identity().map(|identity| identity.epoch)
    }

    /// The retired state of the predecessor replaced by the current session, if any.
    pub fn retired_state(&self) -> Option<SessionState> {
        self.retired.then_some(SessionState::Superseded)
    }

    /// The injected clock reading captured at the last accepted transition.
    pub fn last_transition_at(&self) -> Timestamp {
        self.last_transition_at
    }

    /// Decides on `event` and returns the effects an adapter must perform.
    pub fn handle(&mut self, event: SessionEvent) -> Result<Vec<Effect>, TransitionError> {
        let (next_state, effects) =
            self.transition(event)
                .ok_or(TransitionError::InvalidTransition {
                    from: self.state,
                    event,
                })?;

        self.state = next_state;
        self.last_transition_at = self.clock.now();

        Ok(effects)
    }

    /// Authenticates the required first signaling frame from a connected peer.
    pub fn authenticate_first_frame(&mut self, frame: &[u8]) -> Result<Vec<Effect>, PairingError> {
        let presented = match parse_signaling(frame) {
            Ok(SignalingMessage::Pair {
                mut token, limits, ..
            }) => {
                let presented = PairingToken::from_bytes(token);
                token.zeroize();
                Some((presented, limits))
            }
            _ => None,
        };

        if presented
            .as_ref()
            .is_some_and(|(token, _)| self.is_consumed(token))
        {
            return Err(self.reuse_refusal());
        }

        if self.state == SessionState::Negotiating
            || (self.state == SessionState::Connected && self.offer.is_none())
        {
            return Err(PairingError::TokenAlreadyUsed);
        }

        if !self.awaits_pair_frame() {
            self.refuse_offer();
            return Err(PairingError::ProtocolViolation);
        }

        let Some((token, limits)) = presented else {
            self.refuse_offer();
            return Err(PairingError::ProtocolViolation);
        };

        if self.pairing_window_closed() {
            let (state, effects) = self.retire_expired_pairing(ReasonCode::TokenExpired);
            self.state = state;
            self.last_transition_at = self.clock.now();
            return Ok(effects);
        }

        let Some(offer) = self
            .offer
            .take_if(|offer| offer.token.matches(token.as_bytes()))
        else {
            if self.state == SessionState::Connected {
                self.refuse_offer();
            }
            return Err(PairingError::TokenInvalid);
        };

        Ok(self.activate_offer(offer, limits))
    }

    fn current_identity(&self) -> Option<SessionIdentity> {
        self.active
            .or_else(|| self.offer.as_ref().map(|offer| offer.identity))
    }

    fn awaits_pair_frame(&self) -> bool {
        matches!(
            self.state,
            SessionState::Handshaking | SessionState::Connected
        ) && self
            .offer
            .as_ref()
            .is_some_and(|offer| offer.peer_connected)
    }

    fn activate_offer(&mut self, offer: PendingOffer, accepted_limits: Limits) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.retired = self.active.is_some();
        if self.retired {
            effects.push(Effect::SendClose {
                reason: CloseCode::Superseded,
            });
            effects.push(Effect::End);
        }
        self.consumed_tokens.push(offer.token);
        self.active = Some(offer.identity);
        self.state = SessionState::Negotiating;
        self.last_transition_at = self.clock.now();
        effects.push(Effect::SendPaired {
            session_id: offer.identity.session_id,
            epoch: offer.identity.epoch,
            dtls_fingerprint: self.desktop_dtls_fingerprint,
            accepted_limits,
        });
        effects
    }

    fn is_consumed(&self, token: &PairingToken) -> bool {
        self.consumed_tokens
            .iter()
            .fold(false, |found, used| used.matches(token.as_bytes()) | found)
    }

    fn reuse_refusal(&self) -> PairingError {
        if matches!(
            self.state,
            SessionState::Negotiating | SessionState::Connected
        ) {
            PairingError::Superseded
        } else {
            PairingError::TokenAlreadyUsed
        }
    }

    fn refuse_offer(&mut self) {
        if self.state != SessionState::Connected {
            self.state = SessionState::Ended;
        }
        self.offer = None;
        self.last_transition_at = self.clock.now();
    }

    fn pairing_window_closed(&self) -> bool {
        self.offer
            .as_ref()
            .is_some_and(|offer| self.clock.now() >= offer.expires_at)
    }

    fn mark_peer_connected(&mut self) {
        if let Some(offer) = self.offer.as_mut() {
            offer.peer_connected = true;
        }
    }

    fn retire_expired_pairing(&mut self, reason: ReasonCode) -> (SessionState, Vec<Effect>) {
        self.offer = None;
        if self.active.is_some() {
            return (SessionState::Connected, vec![Effect::RejectPeer { reason }]);
        }
        match self.begin_pairing() {
            Some(material) => (
                SessionState::PairingReady,
                vec![
                    Effect::RejectPeer { reason },
                    Effect::PresentPairing(material),
                ],
            ),
            None => {
                self.clear_pairing();
                (SessionState::Idle, vec![Effect::RejectPeer { reason }])
            }
        }
    }

    fn clear_pairing(&mut self) {
        self.offer = None;
        self.active = None;
        self.retired = false;
    }

    fn begin_pairing(&mut self) -> Option<PairingMaterial> {
        let epoch = self.epochs.issue()?;
        let session_id = SessionId::generate(&mut self.random);
        let mut token_bytes = [0u8; 32];
        self.random.fill_bytes(&mut token_bytes);
        let material = PairingMaterial {
            session_id,
            epoch,
            token: PairingToken::from_bytes(token_bytes),
        };
        self.offer = Some(PendingOffer {
            identity: SessionIdentity { session_id, epoch },
            token: PairingToken::from_bytes(token_bytes),
            expires_at: Timestamp::from_millis(
                self.clock
                    .now()
                    .as_millis()
                    .saturating_add(PAIRING_WINDOW_MILLIS),
            ),
            peer_connected: false,
        });
        token_bytes.zeroize();
        Some(material)
    }

    fn transition(&mut self, event: SessionEvent) -> Option<(SessionState, Vec<Effect>)> {
        match (self.state, event) {
            (SessionState::Idle | SessionState::Connected, SessionEvent::StartPairing) => {
                let material = self.begin_pairing()?;
                let next = match self.state {
                    SessionState::Idle => SessionState::PairingReady,
                    _ => SessionState::Connected,
                };
                Some((next, vec![Effect::PresentPairing(material)]))
            }
            (SessionState::PairingReady | SessionState::Connected, SessionEvent::PeerConnected)
                if self.state == SessionState::PairingReady || self.offer.is_some() =>
            {
                if self.pairing_window_closed() {
                    return Some(self.retire_expired_pairing(ReasonCode::QrExpired));
                }
                self.mark_peer_connected();
                let next = match self.state {
                    SessionState::PairingReady => SessionState::Handshaking,
                    _ => SessionState::Connected,
                };
                Some((next, Vec::new()))
            }
            (SessionState::Negotiating, SessionEvent::NegotiationCompleted)
                if self.first_frame_deadline.is_none() =>
            {
                let now = self.clock.now();
                self.first_frame_deadline = Some(Timestamp::from_millis(
                    now.as_millis()
                        .saturating_add(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS),
                ));
                Some((SessionState::Negotiating, vec![Effect::StartMedia]))
            }
            (SessionState::Negotiating, SessionEvent::FirstMediaFrame(frame)) => {
                let deadline = self.first_frame_deadline?;
                if frame.timestamp <= deadline && self.clock.now() <= deadline {
                    self.first_frame_deadline = None;
                    Some((SessionState::Connected, Vec::new()))
                } else {
                    None
                }
            }
            (SessionState::Negotiating, SessionEvent::FirstFrameTimedOut)
                if self
                    .first_frame_deadline
                    .is_some_and(|deadline| self.clock.now() > deadline) =>
            {
                self.first_frame_deadline = None;
                self.clear_pairing();
                Some((SessionState::Idle, vec![Effect::End]))
            }
            (SessionState::Connected, SessionEvent::EndSession) => {
                self.offer = None;
                Some((SessionState::Ended, vec![Effect::End]))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Effect, MediaFrame, PairingError, SessionCore, SessionEvent, SessionState, Timestamp,
        TransitionError, TransitionErrorCode,
    };
    use crate::testing::{ManualClock, SeededRandomSource};
    use remotecam_protocol::ReasonCode;
    use remotecam_protocol::signaling::{Limits, SignalingMessage};

    const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];
    const PEER_FINGERPRINT: [u8; 32] = [0x55; 32];

    fn connected_with_pending_successor() -> SessionCore<ManualClock, SeededRandomSource> {
        let mut core = SessionCore::new(
            ManualClock::new(Timestamp::ZERO),
            SeededRandomSource::new(7),
            DESKTOP_FINGERPRINT,
        );
        let Some(Effect::PresentPairing(material)) = core
            .handle(SessionEvent::StartPairing)
            .expect("starting pairing is valid")
            .pop()
        else {
            panic!("starting pairing presents its material");
        };
        core.handle(SessionEvent::PeerConnected)
            .expect("the first peer can connect");
        let frame = SignalingMessage::Pair {
            token: *material.token().as_bytes(),
            dtls_fingerprint: PEER_FINGERPRINT,
            limits: Limits::new(),
        }
        .encode()
        .expect("the pair message is valid");
        core.authenticate_first_frame(&frame)
            .expect("the first peer authenticates");
        core.handle(SessionEvent::NegotiationCompleted)
            .expect("negotiation completes");
        core.handle(SessionEvent::FirstMediaFrame(MediaFrame {
            sequence: 0,
            timestamp: Timestamp::ZERO,
        }))
        .expect("the first frame arrives on time");
        core.handle(SessionEvent::StartPairing)
            .expect("a connected core accepts a successor pairing");
        core
    }

    #[test]
    fn ending_a_connected_session_discards_the_pending_successor_offer() {
        let mut core = connected_with_pending_successor();
        assert!(core.offer.is_some());

        let effects = core
            .handle(SessionEvent::EndSession)
            .expect("a connected session can end");

        assert_eq!(effects, vec![Effect::End]);
        assert_eq!(core.state(), SessionState::Ended);
        assert!(core.offer.is_none());
    }

    #[test]
    fn pairing_failures_map_to_the_closed_reason_codes() {
        assert_eq!(
            PairingError::ProtocolViolation.code(),
            ReasonCode::ProtocolViolation
        );
        assert_eq!(PairingError::TokenInvalid.code(), ReasonCode::TokenInvalid);
        assert_eq!(
            PairingError::TokenAlreadyUsed.code(),
            ReasonCode::TokenAlreadyUsed
        );
    }

    #[test]
    fn every_state_has_a_stable_report_label() {
        let labels: Vec<&str> = [
            SessionState::Idle,
            SessionState::PairingReady,
            SessionState::Handshaking,
            SessionState::Negotiating,
            SessionState::Connected,
            SessionState::Ended,
            SessionState::Superseded,
        ]
        .into_iter()
        .map(SessionState::as_str)
        .collect();

        assert_eq!(
            labels,
            [
                "idle",
                "pairing_ready",
                "handshaking",
                "negotiating",
                "connected",
                "ended",
                "superseded",
            ]
        );
    }

    #[test]
    fn the_invalid_transition_code_has_a_stable_spelling() {
        assert_eq!(
            TransitionErrorCode::InvalidTransition.as_str(),
            "invalid_transition"
        );
    }

    #[test]
    fn a_rejection_keeps_the_state_and_event_it_saw() {
        let error = TransitionError::InvalidTransition {
            from: SessionState::Idle,
            event: super::SessionEvent::PeerConnected,
        };

        assert_eq!(error.code(), TransitionErrorCode::InvalidTransition);
        assert_eq!(error.from_state(), SessionState::Idle);
        assert_eq!(error.event(), super::SessionEvent::PeerConnected);
    }
}
