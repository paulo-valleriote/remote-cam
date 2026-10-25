//! RemoteCam Session Core: the product session state machine and vocabulary.

use std::fmt;

use remotecam_protocol::ReasonCode;
use remotecam_protocol::signaling::{Limits, SignalingMessage, parse_signaling};
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
    epoch: u64,
    token: PairingToken,
}

impl PairingMaterial {
    /// The identity of the pairing attempt.
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// The process-local session generation.
    pub const fn epoch(&self) -> u64 {
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
    /// Send the authenticated peer the `paired` frame.
    SendPaired {
        session_id: SessionId,
        epoch: u64,
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
}

impl PairingError {
    /// The stable rejection reason exposed to the transport adapter.
    pub const fn code(self) -> ReasonCode {
        match self {
            Self::ProtocolViolation => ReasonCode::ProtocolViolation,
            Self::TokenInvalid => ReasonCode::TokenInvalid,
            Self::TokenAlreadyUsed => ReasonCode::TokenAlreadyUsed,
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

/// The product session core. It owns the state machine and performs no I/O.
pub struct SessionCore<C: Clock, R: RandomSource> {
    state: SessionState,
    clock: C,
    random: R,
    session_id: Option<SessionId>,
    epoch: Option<u64>,
    pairing_token: Option<PairingToken>,
    desktop_dtls_fingerprint: [u8; 32],
    last_transition_at: Timestamp,
    first_frame_deadline: Option<Timestamp>,
}

impl<C: Clock, R: RandomSource> SessionCore<C, R> {
    /// Creates a core that rests in `Idle` until the first event arrives.
    pub fn new(clock: C, random: R, desktop_dtls_fingerprint: [u8; 32]) -> Self {
        Self {
            state: SessionState::Idle,
            clock,
            random,
            session_id: None,
            epoch: None,
            pairing_token: None,
            desktop_dtls_fingerprint,
            last_transition_at: Timestamp::ZERO,
            first_frame_deadline: None,
        }
    }

    /// The current lifecycle position.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// The identity of the pairing in progress, if one has started.
    pub fn session_id(&self) -> Option<SessionId> {
        self.session_id
    }

    /// The generation assigned to the current pairing, if one has started.
    pub fn epoch(&self) -> Option<u64> {
        self.epoch
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
    pub fn authenticate_first_frame(&mut self, frame: &[u8]) -> Result<Effect, PairingError> {
        if matches!(
            self.state,
            SessionState::Negotiating | SessionState::Connected
        ) {
            return Err(PairingError::TokenAlreadyUsed);
        }

        if self.state != SessionState::Handshaking {
            self.reject_pairing_frame();
            return Err(PairingError::ProtocolViolation);
        }

        let message = match parse_signaling(frame) {
            Ok(message) => message,
            Err(_) => {
                self.reject_pairing_frame();
                return Err(PairingError::ProtocolViolation);
            }
        };

        let SignalingMessage::Pair {
            mut token, limits, ..
        } = message
        else {
            self.reject_pairing_frame();
            return Err(PairingError::ProtocolViolation);
        };

        let Some(expected_token) = self.pairing_token.as_ref() else {
            token.zeroize();
            self.reject_pairing_frame();
            return Err(PairingError::ProtocolViolation);
        };
        let token_matches = expected_token.matches(&token);
        token.zeroize();

        if !token_matches {
            return Err(PairingError::TokenInvalid);
        }

        drop(self.pairing_token.take());
        self.state = SessionState::Negotiating;
        self.last_transition_at = self.clock.now();

        Ok(Effect::SendPaired {
            session_id: self.session_id.expect("pairing has a session identity"),
            epoch: self.epoch.expect("pairing has an epoch"),
            dtls_fingerprint: self.desktop_dtls_fingerprint,
            accepted_limits: limits,
        })
    }

    fn reject_pairing_frame(&mut self) {
        self.state = SessionState::Ended;
        drop(self.pairing_token.take());
        self.last_transition_at = self.clock.now();
    }

    fn transition(&mut self, event: SessionEvent) -> Option<(SessionState, Vec<Effect>)> {
        match (self.state, event) {
            (SessionState::Idle, SessionEvent::StartPairing) => {
                let session_id = self.assign_session_id();
                let epoch = 1;
                self.epoch = Some(epoch);
                let mut token_bytes = [0u8; 32];
                self.random.fill_bytes(&mut token_bytes);
                self.pairing_token = Some(PairingToken::from_bytes(token_bytes));
                let material = PairingMaterial {
                    session_id,
                    epoch,
                    token: PairingToken::from_bytes(token_bytes),
                };
                token_bytes.zeroize();
                Some((
                    SessionState::PairingReady,
                    vec![Effect::PresentPairing(material)],
                ))
            }
            (SessionState::PairingReady, SessionEvent::PeerConnected) => {
                Some((SessionState::Handshaking, Vec::new()))
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
                self.session_id = None;
                self.epoch = None;
                self.pairing_token = None;
                Some((SessionState::Idle, vec![Effect::End]))
            }
            (SessionState::Connected, SessionEvent::EndSession) => {
                Some((SessionState::Ended, vec![Effect::End]))
            }
            _ => None,
        }
    }

    fn assign_session_id(&mut self) -> SessionId {
        let session_id = SessionId::generate(&mut self.random);
        self.session_id = Some(session_id);
        session_id
    }
}

#[cfg(test)]
mod tests {
    use super::{PairingError, SessionState, TransitionError, TransitionErrorCode};
    use remotecam_protocol::ReasonCode;

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
