//! The pairing material the QR payload carries.
//!
//! A material value ties one session identity, one fresh pairing token and one
//! ephemeral certificate fingerprint together, then encodes them as the version
//! 1 QR byte payload. The encoding is produced once, at issue time, so the QR a
//! user sees and the values the desktop stores cannot drift apart.

use std::fmt;

use remotecam_protocol::{Endpoint, QrEncodeError, QrPayload};
use remotecam_session_core::{PairingToken, RandomSource, SessionId};

use crate::certificate::EphemeralCertificate;

/// The capabilities the desktop advertises in the QR payload: H.264, VP8,
/// Opus, and 1080p30, 720p30 and 480p30.
pub const DESKTOP_CAPABILITIES: u16 = 0x003f;

/// Failure while issuing pairing material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialError {
    /// Epoch 0 is not a valid session generation (ADR-0004).
    InvalidEpoch,
    /// The payload violates the version 1 QR contract.
    Qr(QrEncodeError),
}

impl fmt::Display for MaterialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEpoch => formatter.write_str("epoch 0 is not a valid generation"),
            Self::Qr(error) => write!(formatter, "QR payload rejected: {error:?}"),
        }
    }
}

impl std::error::Error for MaterialError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Qr(error) => Some(error),
            Self::InvalidEpoch => None,
        }
    }
}

impl From<QrEncodeError> for MaterialError {
    fn from(error: QrEncodeError) -> Self {
        Self::Qr(error)
    }
}

/// One session's pairing material, already encoded as a QR payload.
pub struct PairingMaterial {
    session_id: SessionId,
    epoch: u32,
    expires_at_unix: u64,
    endpoints: Vec<Endpoint>,
    pairing_token: PairingToken,
    tls_fingerprint: [u8; 32],
    encoded_qr: Vec<u8>,
}

impl fmt::Debug for PairingMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PairingMaterial")
            .field("session_id", &self.session_id)
            .field("epoch", &self.epoch)
            .field("expires_at_unix", &self.expires_at_unix)
            .field("endpoints", &self.endpoints)
            .field("pairing_token", &"[elided]")
            .field("tls_fingerprint", &"[elided]")
            .field("encoded_qr_bytes", &self.encoded_qr.len())
            .finish()
    }
}

impl PairingMaterial {
    /// Issues material for one session.
    ///
    /// The session identifier and the pairing token are both drawn from the
    /// injected source, in that order. The certificate fingerprint is copied
    /// from the certificate the session will serve.
    pub fn issue<R: RandomSource + ?Sized>(
        random: &mut R,
        certificate: &EphemeralCertificate,
        epoch: u32,
        expires_at_unix: u64,
        endpoints: Vec<Endpoint>,
    ) -> Result<Self, MaterialError> {
        if epoch == 0 {
            return Err(MaterialError::InvalidEpoch);
        }

        let session_id = SessionId::generate(random);
        let pairing_token = PairingToken::generate(random);
        let tls_fingerprint = certificate.fingerprint();

        let payload = QrPayload {
            session_id: *session_id.as_bytes(),
            epoch,
            expires_at: expires_at_unix,
            pairing_token: *pairing_token.as_bytes(),
            tls_fingerprint,
            capabilities: DESKTOP_CAPABILITIES,
            endpoints: endpoints.clone(),
        };
        let encoded_qr = payload.encode()?;

        Ok(Self {
            session_id,
            epoch,
            expires_at_unix,
            endpoints,
            pairing_token,
            tls_fingerprint,
            encoded_qr,
        })
    }

    /// The session the material belongs to.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// The generation assigned when the QR was created.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// The absolute expiry carried by the payload, in Unix seconds.
    pub fn expires_at_unix(&self) -> u64 {
        self.expires_at_unix
    }

    /// The endpoints advertised in the payload.
    pub fn endpoints(&self) -> &[Endpoint] {
        &self.endpoints
    }

    /// The single-use secret the mobile must present.
    pub fn pairing_token(&self) -> &PairingToken {
        &self.pairing_token
    }

    /// The exact leaf fingerprint the mobile pins.
    pub fn tls_fingerprint(&self) -> [u8; 32] {
        self.tls_fingerprint
    }

    /// The encoded version 1 QR payload.
    pub fn encoded_qr(&self) -> &[u8] {
        &self.encoded_qr
    }
}

#[cfg(test)]
mod tests {
    use super::{DESKTOP_CAPABILITIES, MaterialError, PairingMaterial};
    use crate::certificate::EphemeralCertificate;
    use remotecam_protocol::hex::encode_hex;
    use remotecam_protocol::{Endpoint, QrEncodeError, QrPayload};
    use remotecam_session_core::testing::SeededRandomSource;

    const EXPIRES_AT: u64 = 2_000_000_000;

    fn endpoints() -> Vec<Endpoint> {
        vec![Endpoint::from_address(4, "192.168.1.42", 51820).expect("endpoint")]
    }

    fn material(certificate: &EphemeralCertificate) -> PairingMaterial {
        PairingMaterial::issue(
            &mut SeededRandomSource::new(9),
            certificate,
            1,
            EXPIRES_AT,
            endpoints(),
        )
        .expect("material")
    }

    #[test]
    fn the_encoded_qr_decodes_back_to_the_same_material() {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let material = material(&certificate);

        let decoded =
            QrPayload::decode(material.encoded_qr(), 1_900_000_000).expect("decoded payload");

        assert_eq!(decoded.session_id, *material.session_id().as_bytes());
        assert_eq!(decoded.epoch, material.epoch());
        assert_eq!(decoded.expires_at, material.expires_at_unix());
        assert_eq!(decoded.pairing_token, *material.pairing_token().as_bytes());
        assert_eq!(decoded.tls_fingerprint, material.tls_fingerprint());
        assert_eq!(decoded.capabilities, DESKTOP_CAPABILITIES);
        assert_eq!(decoded.endpoints, material.endpoints());
    }

    #[test]
    fn the_fingerprint_comes_from_the_session_certificate() {
        let certificate = EphemeralCertificate::generate().expect("certificate");

        let material = material(&certificate);

        assert_eq!(material.tls_fingerprint(), certificate.fingerprint());
    }

    #[test]
    fn epoch_zero_is_not_a_valid_generation() {
        let certificate = EphemeralCertificate::generate().expect("certificate");

        let error = PairingMaterial::issue(
            &mut SeededRandomSource::new(1),
            &certificate,
            0,
            EXPIRES_AT,
            endpoints(),
        )
        .expect_err("epoch zero must be refused");

        assert_eq!(error, MaterialError::InvalidEpoch);
        assert_eq!(error.to_string(), "epoch 0 is not a valid generation");
    }

    #[test]
    fn a_payload_without_endpoints_is_refused() {
        let certificate = EphemeralCertificate::generate().expect("certificate");

        let error = PairingMaterial::issue(
            &mut SeededRandomSource::new(1),
            &certificate,
            1,
            EXPIRES_AT,
            Vec::new(),
        )
        .expect_err("an unreachable QR must be refused");

        assert_eq!(error, MaterialError::Qr(QrEncodeError::NoEndpoints));
        assert!(error.to_string().contains("QR payload rejected"));
    }

    #[test]
    fn debug_elides_the_token_and_the_fingerprint() {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let material = material(&certificate);
        let token = encode_hex(material.pairing_token().as_bytes());
        let fingerprint = encode_hex(&material.tls_fingerprint());

        let rendered = format!("{material:?}");

        assert!(rendered.contains("[elided]"));
        assert!(!rendered.contains(&token));
        assert!(!rendered.contains(&fingerprint));
    }
}
