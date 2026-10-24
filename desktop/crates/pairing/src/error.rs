use std::fmt;

use remotecam_protocol::ReasonCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingError {
    code: ReasonCode,
    detail: String,
}

impl PairingError {
    pub fn new(code: ReasonCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    pub fn code(&self) -> ReasonCode {
        self.code
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for PairingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.detail)
    }
}

impl std::error::Error for PairingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

pub fn classify_rustls_error(error: &rustls::Error) -> ReasonCode {
    match error {
        rustls::Error::AlertReceived(alert) => match alert {
            rustls::AlertDescription::ProtocolVersion => ReasonCode::TlsVersionUnsupported,
            rustls::AlertDescription::AccessDenied
            | rustls::AlertDescription::BadCertificate
            | rustls::AlertDescription::UnknownCA
            | rustls::AlertDescription::CertificateUnknown
            | rustls::AlertDescription::UnsupportedCertificate => {
                ReasonCode::CertificatePinMismatch
            }
            _ => ReasonCode::TlsHandshakeFailed,
        },
        rustls::Error::PeerIncompatible(
            rustls::PeerIncompatible::ServerDoesNotSupportTls12Or13
            | rustls::PeerIncompatible::Tls12NotOffered
            | rustls::PeerIncompatible::Tls12NotOfferedOrEnabled
            | rustls::PeerIncompatible::Tls13RequiredForQuic,
        ) => ReasonCode::TlsVersionUnsupported,
        rustls::Error::InvalidCertificate(_) => ReasonCode::CertificatePinMismatch,
        _ => ReasonCode::TlsHandshakeFailed,
    }
}

pub fn classify_io_error(error: &std::io::Error) -> ReasonCode {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        .map_or(ReasonCode::TlsHandshakeFailed, classify_rustls_error)
}

#[cfg(test)]
mod tests {
    use super::{PairingError, classify_io_error, classify_rustls_error};
    use remotecam_protocol::ReasonCode;
    use rustls::{AlertDescription, CertificateError, Error, PeerIncompatible};

    #[test]
    fn classifies_a_protocol_version_alert_as_an_unsupported_version() {
        assert_eq!(
            classify_rustls_error(&Error::AlertReceived(AlertDescription::ProtocolVersion)),
            ReasonCode::TlsVersionUnsupported
        );
    }

    #[test]
    fn classifies_an_incompatible_version_negotiation() {
        for error in [
            Error::PeerIncompatible(PeerIncompatible::Tls12NotOffered),
            Error::PeerIncompatible(PeerIncompatible::Tls12NotOfferedOrEnabled),
            Error::PeerIncompatible(PeerIncompatible::ServerDoesNotSupportTls12Or13),
            Error::PeerIncompatible(PeerIncompatible::Tls13RequiredForQuic),
        ] {
            assert_eq!(
                classify_rustls_error(&error),
                ReasonCode::TlsVersionUnsupported
            );
        }
    }

    #[test]
    fn classifies_a_refused_certificate_as_a_pin_mismatch() {
        assert_eq!(
            classify_rustls_error(&Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure
            )),
            ReasonCode::CertificatePinMismatch
        );
        for alert in [
            AlertDescription::AccessDenied,
            AlertDescription::BadCertificate,
            AlertDescription::UnknownCA,
            AlertDescription::CertificateUnknown,
            AlertDescription::UnsupportedCertificate,
        ] {
            assert_eq!(
                classify_rustls_error(&Error::AlertReceived(alert)),
                ReasonCode::CertificatePinMismatch
            );
        }
    }

    #[test]
    fn classifies_any_other_failure_as_a_handshake_failure() {
        assert_eq!(
            classify_rustls_error(&Error::General("unexpected".into())),
            ReasonCode::TlsHandshakeFailed
        );
        assert_eq!(
            classify_rustls_error(&Error::AlertReceived(AlertDescription::DecodeError)),
            ReasonCode::TlsHandshakeFailed
        );
    }

    #[test]
    fn classifies_io_errors_with_and_without_a_wrapped_rustls_error() {
        let plain = std::io::Error::other("noise");
        assert_eq!(classify_io_error(&plain), ReasonCode::TlsHandshakeFailed);

        let wrapped =
            std::io::Error::other(Error::AlertReceived(AlertDescription::ProtocolVersion));
        assert_eq!(
            classify_io_error(&wrapped),
            ReasonCode::TlsVersionUnsupported
        );
    }

    #[test]
    fn a_pairing_error_renders_its_closed_code() {
        let error = PairingError::new(ReasonCode::CertificatePinMismatch, "certificate rejected");

        assert_eq!(error.code(), ReasonCode::CertificatePinMismatch);
        assert_eq!(error.detail(), "certificate rejected");
        assert_eq!(
            error.to_string(),
            "certificate_pin_mismatch: certificate rejected"
        );
    }
}
