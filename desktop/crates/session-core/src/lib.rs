//! RemoteCam Session Core: the product session state machine and vocabulary.

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

/// The product session core. It owns the state machine and performs no I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCore {
    state: SessionState,
}

impl SessionCore {
    /// Creates a core that rests in `Idle` until the first event arrives.
    pub fn new() -> Self {
        Self {
            state: SessionState::Idle,
        }
    }

    /// The current lifecycle position.
    pub fn state(&self) -> SessionState {
        self.state
    }
}

impl Default for SessionCore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionCore, SessionState};

    #[test]
    fn a_new_session_core_rests_in_idle() {
        assert_eq!(SessionCore::new().state(), SessionState::Idle);
    }

    #[test]
    fn the_default_core_is_the_idle_core() {
        assert_eq!(SessionCore::default(), SessionCore::new());
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
}
