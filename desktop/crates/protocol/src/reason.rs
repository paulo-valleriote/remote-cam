//! The closed reason-code set of `protocol/reason-codes.json`.
//!
//! A code is a stable identifier. Its spelling is part of the wire contract and
//! changing it requires a protocol version bump. The human-readable text lives
//! in the presentation layer and is deliberately absent here.

/// The protocol context in which a code may appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReasonCategory {
    /// A pairing attempt was refused.
    PairingRejection,
    /// An established session terminated.
    SessionClose,
    /// A malformed or unsupported signaling message was refused.
    SignalingError,
}

impl ReasonCategory {
    /// The wire spelling used by the pinned enumeration vector.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PairingRejection => "pairing_rejection",
            Self::SessionClose => "session_close",
            Self::SignalingError => "signaling_error",
        }
    }
}

/// One code from the closed enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReasonCode {
    InvalidQrPayload,
    UnsupportedQrVersion,
    QrExpired,
    NoPrivateEndpoint,
    DesktopUnreachable,
    TlsHandshakeFailed,
    CertificatePinMismatch,
    CertificateChainRejected,
    TlsVersionUnsupported,
    TokenInvalid,
    TokenExpired,
    TokenAlreadyUsed,
    SourceAddressNotPrivate,
    SourceSubnetMismatch,
    RateLimited,
    ListenerClosed,
    Superseded,
    ProtocolViolation,
    InternalError,
    EndedByMobile,
    EndedByDesktop,
    Timeout,
    MediaFailure,
    SecurityViolation,
    InvalidMessage,
    UnsupportedVersion,
}

impl ReasonCode {
    /// Every code in the closed set, in the order of the canonical document.
    pub const ALL: [Self; 26] = [
        Self::InvalidQrPayload,
        Self::UnsupportedQrVersion,
        Self::QrExpired,
        Self::NoPrivateEndpoint,
        Self::DesktopUnreachable,
        Self::TlsHandshakeFailed,
        Self::CertificatePinMismatch,
        Self::CertificateChainRejected,
        Self::TlsVersionUnsupported,
        Self::TokenInvalid,
        Self::TokenExpired,
        Self::TokenAlreadyUsed,
        Self::SourceAddressNotPrivate,
        Self::SourceSubnetMismatch,
        Self::RateLimited,
        Self::ListenerClosed,
        Self::Superseded,
        Self::ProtocolViolation,
        Self::InternalError,
        Self::EndedByMobile,
        Self::EndedByDesktop,
        Self::Timeout,
        Self::MediaFailure,
        Self::SecurityViolation,
        Self::InvalidMessage,
        Self::UnsupportedVersion,
    ];

    /// The stable wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidQrPayload => "invalid_qr_payload",
            Self::UnsupportedQrVersion => "unsupported_qr_version",
            Self::QrExpired => "qr_expired",
            Self::NoPrivateEndpoint => "no_private_endpoint",
            Self::DesktopUnreachable => "desktop_unreachable",
            Self::TlsHandshakeFailed => "tls_handshake_failed",
            Self::CertificatePinMismatch => "certificate_pin_mismatch",
            Self::CertificateChainRejected => "certificate_chain_rejected",
            Self::TlsVersionUnsupported => "tls_version_unsupported",
            Self::TokenInvalid => "token_invalid",
            Self::TokenExpired => "token_expired",
            Self::TokenAlreadyUsed => "token_already_used",
            Self::SourceAddressNotPrivate => "source_address_not_private",
            Self::SourceSubnetMismatch => "source_subnet_mismatch",
            Self::RateLimited => "rate_limited",
            Self::ListenerClosed => "listener_closed",
            Self::Superseded => "superseded",
            Self::ProtocolViolation => "protocol_violation",
            Self::InternalError => "internal_error",
            Self::EndedByMobile => "ended_by_mobile",
            Self::EndedByDesktop => "ended_by_desktop",
            Self::Timeout => "timeout",
            Self::MediaFailure => "media_failure",
            Self::SecurityViolation => "security_violation",
            Self::InvalidMessage => "invalid_message",
            Self::UnsupportedVersion => "unsupported_version",
        }
    }

    /// The contexts in which this code may appear.
    pub fn categories(self) -> &'static [ReasonCategory] {
        match self {
            Self::InvalidQrPayload
            | Self::UnsupportedQrVersion
            | Self::QrExpired
            | Self::NoPrivateEndpoint
            | Self::DesktopUnreachable
            | Self::TlsHandshakeFailed
            | Self::CertificatePinMismatch
            | Self::CertificateChainRejected
            | Self::TlsVersionUnsupported
            | Self::TokenInvalid
            | Self::TokenExpired
            | Self::TokenAlreadyUsed
            | Self::SourceAddressNotPrivate
            | Self::SourceSubnetMismatch
            | Self::ListenerClosed
            | Self::ProtocolViolation => &[ReasonCategory::PairingRejection],
            Self::RateLimited => &[
                ReasonCategory::PairingRejection,
                ReasonCategory::SignalingError,
            ],
            Self::Superseded => &[
                ReasonCategory::PairingRejection,
                ReasonCategory::SessionClose,
            ],
            Self::InternalError => &[
                ReasonCategory::PairingRejection,
                ReasonCategory::SessionClose,
                ReasonCategory::SignalingError,
            ],
            Self::EndedByMobile | Self::EndedByDesktop | Self::Timeout | Self::MediaFailure => {
                &[ReasonCategory::SessionClose]
            }
            Self::SecurityViolation => &[ReasonCategory::SessionClose],
            Self::InvalidMessage | Self::UnsupportedVersion => &[ReasonCategory::SignalingError],
        }
    }

    /// True for the codes that must reach the security section of the local
    /// diagnostic. `rate_limited` is explicitly not one of them.
    pub fn is_security_event(self) -> bool {
        matches!(
            self,
            Self::CertificatePinMismatch
                | Self::CertificateChainRejected
                | Self::TlsVersionUnsupported
                | Self::TokenAlreadyUsed
                | Self::SourceAddressNotPrivate
                | Self::SourceSubnetMismatch
                | Self::ListenerClosed
                | Self::SecurityViolation
        )
    }

    /// True when the presentation layer may offer the user a next step.
    pub fn is_user_actionable(self) -> bool {
        matches!(
            self,
            Self::InvalidQrPayload
                | Self::UnsupportedQrVersion
                | Self::QrExpired
                | Self::NoPrivateEndpoint
                | Self::DesktopUnreachable
                | Self::RateLimited
                | Self::Superseded
                | Self::InternalError
                | Self::EndedByMobile
                | Self::EndedByDesktop
                | Self::Timeout
                | Self::MediaFailure
        )
    }

    /// Parses a wire spelling. Unknown spellings are a contract break, not a
    /// free-text reason.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_round_trips_through_its_wire_spelling() {
        for code in ReasonCode::ALL {
            assert_eq!(ReasonCode::parse(code.as_str()), Some(code));
        }
    }

    #[test]
    fn the_set_is_closed() {
        assert_eq!(ReasonCode::ALL.len(), 26);
        assert_eq!(ReasonCode::parse("not_a_code"), None);
    }

    #[test]
    fn only_the_declared_codes_are_security_events() {
        let escalated: Vec<&str> = ReasonCode::ALL
            .into_iter()
            .filter(|code| code.is_security_event())
            .map(ReasonCode::as_str)
            .collect();
        assert_eq!(
            escalated,
            vec![
                "certificate_pin_mismatch",
                "certificate_chain_rejected",
                "tls_version_unsupported",
                "token_already_used",
                "source_address_not_private",
                "source_subnet_mismatch",
                "listener_closed",
                "security_violation",
            ]
        );
        assert!(!ReasonCode::RateLimited.is_security_event());
    }

    #[test]
    fn shared_codes_carry_every_context() {
        assert_eq!(
            ReasonCode::InternalError.categories(),
            &[
                ReasonCategory::PairingRejection,
                ReasonCategory::SessionClose,
                ReasonCategory::SignalingError,
            ]
        );
        assert_eq!(
            ReasonCode::Timeout.categories(),
            &[ReasonCategory::SessionClose]
        );
        assert_eq!(
            ReasonCode::InvalidMessage.categories(),
            &[ReasonCategory::SignalingError]
        );
    }

    #[test]
    fn user_actionable_flags_are_explicit() {
        assert!(ReasonCode::QrExpired.is_user_actionable());
        assert!(!ReasonCode::CertificatePinMismatch.is_user_actionable());
    }
}
