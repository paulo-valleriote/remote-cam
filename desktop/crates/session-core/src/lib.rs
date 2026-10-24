//! RemoteCam Session Core: the product session state machine and vocabulary.

pub mod testing;

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

    /// The raw bytes of the identifier.
    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }
}

/// An input the Session Core decides on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionEvent {
    /// The desktop user asked to start pairing.
    StartPairing,
    /// A peer opened the pinned channel.
    PeerConnected,
    /// The peer proved it holds the pairing token.
    PairAuthenticated,
    /// Offer and answer completed negotiation.
    NegotiationCompleted,
    /// One side ended an established session.
    EndSession,
}

/// An action an adapter performs after the Session Core decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Effect {
    /// Show the pairing material for the freshly assigned session.
    PresentPairing { session_id: SessionId },
    /// Send the authenticated peer the `paired` frame.
    SendPaired,
    /// Start the media pipeline.
    StartMedia,
    /// Release the session resources.
    End,
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
    last_transition_at: Timestamp,
}

impl<C: Clock, R: RandomSource> SessionCore<C, R> {
    /// Creates a core that rests in `Idle` until the first event arrives.
    pub fn new(clock: C, random: R) -> Self {
        Self {
            state: SessionState::Idle,
            clock,
            random,
            session_id: None,
            last_transition_at: Timestamp::ZERO,
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

    fn transition(&mut self, event: SessionEvent) -> Option<(SessionState, Vec<Effect>)> {
        match (self.state, event) {
            (SessionState::Idle, SessionEvent::StartPairing) => {
                let session_id = self.assign_session_id();
                Some((
                    SessionState::PairingReady,
                    vec![Effect::PresentPairing { session_id }],
                ))
            }
            (SessionState::PairingReady, SessionEvent::PeerConnected) => {
                Some((SessionState::Handshaking, Vec::new()))
            }
            (SessionState::Handshaking, SessionEvent::PairAuthenticated) => {
                Some((SessionState::Negotiating, vec![Effect::SendPaired]))
            }
            (SessionState::Negotiating, SessionEvent::NegotiationCompleted) => {
                Some((SessionState::Connected, vec![Effect::StartMedia]))
            }
            (SessionState::Connected, SessionEvent::EndSession) => {
                Some((SessionState::Ended, vec![Effect::End]))
            }
            _ => None,
        }
    }

    fn assign_session_id(&mut self) -> SessionId {
        let mut bytes = [0u8; SessionId::LENGTH];
        self.random.fill_bytes(&mut bytes);
        let session_id = SessionId::from_bytes(bytes);
        self.session_id = Some(session_id);
        session_id
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionState, TransitionError, TransitionErrorCode};

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
