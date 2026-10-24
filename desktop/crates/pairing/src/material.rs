use std::fmt;

use remotecam_protocol::{Endpoint, QrEncodeError, QrPayload};
use remotecam_session_core::{PairingToken, RandomSource, SessionId};
use zeroize::{Zeroize, Zeroizing};

use crate::certificate::EphemeralCertificate;

fn encode_qr_payload(payload: &mut QrPayload) -> Result<Zeroizing<Vec<u8>>, QrEncodeError> {
    let encoded = payload.encode();
    payload.pairing_token.zeroize();
    encoded.map(Zeroizing::new)
}

pub const DESKTOP_CAPABILITIES: u16 = 0x003f;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialError {
    InvalidEpoch,
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

pub struct PairingMaterial {
    session_id: SessionId,
    epoch: u32,
    expires_at_unix: u64,
    endpoints: Vec<Endpoint>,
    pairing_token: PairingToken,
    tls_fingerprint: [u8; 32],
    encoded_qr: Zeroizing<Vec<u8>>,
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

        let mut payload = QrPayload {
            session_id: *session_id.as_bytes(),
            epoch,
            expires_at: expires_at_unix,
            pairing_token: *pairing_token.as_bytes(),
            tls_fingerprint,
            capabilities: DESKTOP_CAPABILITIES,
            endpoints: endpoints.clone(),
        };
        let encoded_qr = encode_qr_payload(&mut payload)?;

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

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    pub fn expires_at_unix(&self) -> u64 {
        self.expires_at_unix
    }

    pub fn endpoints(&self) -> &[Endpoint] {
        &self.endpoints
    }

    pub fn pairing_token(&self) -> &PairingToken {
        &self.pairing_token
    }

    pub fn tls_fingerprint(&self) -> [u8; 32] {
        self.tls_fingerprint
    }

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
    use zeroize::Zeroizing;

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
    fn clears_the_temporary_qr_token_after_encoding() {
        let mut payload = QrPayload {
            session_id: [1; 16],
            epoch: 1,
            expires_at: EXPIRES_AT,
            pairing_token: [0xab; 32],
            tls_fingerprint: [2; 32],
            capabilities: DESKTOP_CAPABILITIES,
            endpoints: endpoints(),
        };

        let _encoded = super::encode_qr_payload(&mut payload).expect("encoded QR");

        assert_eq!(payload.pairing_token, [0; 32]);
    }

    #[test]
    fn owns_encoded_qr_bytes_in_a_zeroizing_buffer() {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let material = material(&certificate);

        let _zeroizing_buffer: &Zeroizing<Vec<u8>> = &material.encoded_qr;
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
