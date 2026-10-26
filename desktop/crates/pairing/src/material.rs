use std::fmt;

use remotecam_protocol::{Endpoint, QrEncodeError, QrPayload};
use remotecam_session_core::{
    PAIRING_WINDOW_MILLIS, PairingMaterial as SessionPairingMaterial, SessionId,
};
use zeroize::{Zeroize, Zeroizing};

use crate::certificate::EphemeralCertificate;

const QR_VALIDITY_SECONDS: u64 = PAIRING_WINDOW_MILLIS / 1_000;

pub trait UnixClock {
    fn now_unix(&self) -> u64;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemUnixClock;

impl UnixClock for SystemUnixClock {
    fn now_unix(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    }
}

fn encode_qr_payload(payload: &mut QrPayload) -> Result<Zeroizing<Vec<u8>>, QrEncodeError> {
    let encoded = payload.encode();
    payload.pairing_token.zeroize();
    encoded.map(Zeroizing::new)
}

pub const DESKTOP_CAPABILITIES: u16 = 0x003f;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialError {
    Qr(QrEncodeError),
}

impl fmt::Display for MaterialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Qr(error) => write!(formatter, "QR payload rejected: {error:?}"),
        }
    }
}

impl std::error::Error for MaterialError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Qr(error) => Some(error),
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
    tls_fingerprint: [u8; 32],
    encoded_qr: Zeroizing<Vec<u8>>,
}

pub struct PairingMaterialRequest<'a> {
    session_material: &'a SessionPairingMaterial,
    endpoints: Vec<Endpoint>,
}

impl<'a> PairingMaterialRequest<'a> {
    pub fn new(session_material: &'a SessionPairingMaterial, endpoints: Vec<Endpoint>) -> Self {
        Self {
            session_material,
            endpoints,
        }
    }
}

impl fmt::Debug for PairingMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PairingMaterial")
            .field("session_id", &self.session_id)
            .field("epoch", &self.epoch)
            .field("expires_at_unix", &self.expires_at_unix)
            .field("endpoints", &self.endpoints)
            .field("tls_fingerprint", &"[elided]")
            .field("encoded_qr_bytes", &self.encoded_qr.len())
            .finish()
    }
}

impl Drop for PairingMaterial {
    fn drop(&mut self) {
        self.wipe();
    }
}

impl PairingMaterial {
    /// Clears what the session must not leave behind. `Drop` calls it, so a value that lived
    /// inline on a stack frame does not leave its fingerprint in the dead frame.
    fn wipe(&mut self) {
        self.tls_fingerprint.zeroize();
        self.encoded_qr.zeroize();
    }

    pub fn issue(
        request: PairingMaterialRequest<'_>,
        certificate: &EphemeralCertificate,
        clock: &impl UnixClock,
    ) -> Result<Self, MaterialError> {
        Self::issue_with_fingerprint(request, certificate.fingerprint(), clock)
    }

    pub(crate) fn issue_with_fingerprint(
        request: PairingMaterialRequest<'_>,
        tls_fingerprint: [u8; 32],
        clock: &impl UnixClock,
    ) -> Result<Self, MaterialError> {
        let PairingMaterialRequest {
            session_material,
            endpoints,
        } = request;
        let session_id = session_material.session_id();
        let expires_at_unix = clock.now_unix().saturating_add(QR_VALIDITY_SECONDS);

        let mut payload = QrPayload {
            session_id: *session_id.as_bytes(),
            epoch: session_material.epoch(),
            expires_at: expires_at_unix,
            pairing_token: *session_material.token().as_bytes(),
            tls_fingerprint,
            capabilities: DESKTOP_CAPABILITIES,
            endpoints: endpoints.clone(),
        };
        let encoded_qr = encode_qr_payload(&mut payload)?;

        Ok(Self {
            session_id,
            epoch: session_material.epoch(),
            expires_at_unix,
            endpoints,
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

    pub fn tls_fingerprint(&self) -> [u8; 32] {
        self.tls_fingerprint
    }

    pub fn encoded_qr(&self) -> &[u8] {
        &self.encoded_qr
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DESKTOP_CAPABILITIES, MaterialError, PAIRING_WINDOW_MILLIS, PairingMaterial,
        PairingMaterialRequest, UnixClock,
    };
    use crate::certificate::EphemeralCertificate;
    use remotecam_protocol::hex::encode_hex;
    use remotecam_protocol::{Endpoint, QrEncodeError, QrPayload};
    use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
    use remotecam_session_core::{Effect, SessionCore, SessionEvent, Timestamp};
    use zeroize::Zeroizing;

    const CREATED_AT_UNIX: u64 = 1_900_000_000;

    struct FixedUnixClock(u64);

    impl UnixClock for FixedUnixClock {
        fn now_unix(&self) -> u64 {
            self.0
        }
    }

    fn endpoints() -> Vec<Endpoint> {
        vec![Endpoint::from_address(4, "192.168.1.42", 51820).expect("endpoint")]
    }

    fn session_material() -> remotecam_session_core::PairingMaterial {
        let mut core = SessionCore::new(
            ManualClock::new(Timestamp::ZERO),
            SeededRandomSource::new(9),
            [0x44; 32],
        );
        let effects = core
            .handle(SessionEvent::StartPairing)
            .expect("starting pairing succeeds");
        match effects.into_iter().next() {
            Some(Effect::PresentPairing(material)) => material,
            _ => panic!("starting pairing presents its material"),
        }
    }

    fn material(certificate: &EphemeralCertificate) -> PairingMaterial {
        let session_material = session_material();
        PairingMaterial::issue(
            PairingMaterialRequest::new(&session_material, endpoints()),
            certificate,
            &FixedUnixClock(CREATED_AT_UNIX),
        )
        .expect("material")
    }

    #[test]
    fn the_qr_expires_one_pairing_window_after_its_creation() {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let material = material(&certificate);

        assert_eq!(
            material.expires_at_unix(),
            CREATED_AT_UNIX + PAIRING_WINDOW_MILLIS / 1_000
        );
        assert_eq!(material.expires_at_unix(), CREATED_AT_UNIX + 120);
    }

    #[test]
    fn the_encoded_qr_decodes_back_to_the_same_material() {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let session_material = session_material();
        let material = PairingMaterial::issue(
            PairingMaterialRequest::new(&session_material, endpoints()),
            &certificate,
            &FixedUnixClock(CREATED_AT_UNIX),
        )
        .expect("material");

        let decoded =
            QrPayload::decode(material.encoded_qr(), CREATED_AT_UNIX).expect("decoded payload");

        assert_eq!(
            decoded.session_id,
            *session_material.session_id().as_bytes()
        );
        assert_eq!(decoded.epoch, material.epoch());
        assert_eq!(decoded.expires_at, material.expires_at_unix());
        assert_eq!(decoded.pairing_token, *session_material.token().as_bytes());
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
            expires_at: CREATED_AT_UNIX + 120,
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
    fn a_payload_without_endpoints_is_refused() {
        let certificate = EphemeralCertificate::generate().expect("certificate");

        let error = PairingMaterial::issue(
            PairingMaterialRequest::new(&session_material(), Vec::new()),
            &certificate,
            &FixedUnixClock(CREATED_AT_UNIX),
        )
        .expect_err("an unreachable QR must be refused");

        assert_eq!(error, MaterialError::Qr(QrEncodeError::NoEndpoints));
        assert!(error.to_string().contains("QR payload rejected"));
    }

    #[test]
    fn wiping_clears_the_fingerprint_and_the_encoded_qr() {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let mut material = material(&certificate);

        material.wipe();

        assert_eq!(material.tls_fingerprint(), [0; 32]);
        assert!(material.encoded_qr().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn debug_elides_the_token_and_the_fingerprint() {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let material = material(&certificate);
        let payload =
            QrPayload::decode(material.encoded_qr(), CREATED_AT_UNIX).expect("decoded payload");
        let token = encode_hex(&payload.pairing_token);
        let fingerprint = encode_hex(&material.tls_fingerprint());

        let rendered = format!("{material:?}");

        assert!(rendered.contains("[elided]"));
        assert!(!rendered.contains(&token));
        assert!(!rendered.contains(&fingerprint));
    }
}
