//! One deterministic pairing cycle. The probe runs it live; the inspector replays only its
//! derivation, so the secrets it searches for never exist in the process under inspection
//! beyond the session itself.

use std::fmt;
use std::sync::Arc;

use remotecam_pairing::certificate::CertificateRandomSource;
use remotecam_pairing::{
    ClientChannel, EphemeralCertificate, PairingListener, PairingMaterial, PairingMaterialRequest,
    PinnedClient, ServerChannel, UnixClock, read_frame, write_frame,
};
use remotecam_protocol::hex::encode_hex;
use remotecam_protocol::signaling::{Limits, SignalingMessage};
use remotecam_protocol::{Endpoint, QrPayload, ReasonCode};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{Effect, RandomSource, SessionCore, SessionEvent, Timestamp};

use zeroize::Zeroize;

use crate::scan::Needle;

const CERTIFICATE_SEED: u64 = 1;
const ADVERTISED_ADDRESS: &str = "192.168.1.42";
const ADVERTISED_PORT: u16 = 51_820;
const UNIX_NOW: u64 = 1_900_000_000;
const DESKTOP_DTLS_FINGERPRINT: [u8; 32] = [0x44; 32];
const MOBILE_DTLS_FINGERPRINT: [u8; 32] = [0x55; 32];

#[derive(Debug)]
pub struct CycleError(&'static str);

impl fmt::Display for CycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for CycleError {}

struct CertificateRandom(SeededRandomSource);

impl RandomSource for CertificateRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        self.0.fill_bytes(destination);
    }
}

impl CertificateRandomSource for CertificateRandom {}

struct FixedUnixClock;

impl UnixClock for FixedUnixClock {
    fn now_unix(&self) -> u64 {
        UNIX_NOW
    }
}

fn certificate_random(seed: u64) -> CertificateRandom {
    CertificateRandom(SeededRandomSource::new(seed ^ CERTIFICATE_SEED))
}

fn endpoint() -> Result<Endpoint, CycleError> {
    Endpoint::from_address(4, ADVERTISED_ADDRESS, ADVERTISED_PORT)
        .ok_or(CycleError("the advertised endpoint is invalid"))
}

fn start_pairing(
    seed: u64,
) -> Result<
    (
        SessionCore<ManualClock, SeededRandomSource>,
        remotecam_session_core::PairingMaterial,
    ),
    CycleError,
> {
    let mut core = SessionCore::new(
        ManualClock::new(Timestamp::ZERO),
        SeededRandomSource::new(seed),
        DESKTOP_DTLS_FINGERPRINT,
    );
    let effects = core
        .handle(SessionEvent::StartPairing)
        .map_err(|_| CycleError("the session core refused to start pairing"))?;
    let material = effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::PresentPairing(material) => Some(material),
            _ => None,
        })
        .ok_or(CycleError("pairing produced no material"))?;
    Ok((core, material))
}

fn certificate(seed: u64) -> Result<EphemeralCertificate, CycleError> {
    EphemeralCertificate::generate_with_random_source(&mut certificate_random(seed))
        .map_err(|_| CycleError("certificate generation failed"))
}

/// The secrets of one cycle, derived without running it.
pub fn derive_needles(seed: u64) -> Result<Vec<Needle>, CycleError> {
    let (_core, session_material) = start_pairing(seed)?;
    let certificate = certificate(seed)?;
    let token = *session_material.token().as_bytes();
    let fingerprint = certificate.fingerprint();
    let qr = PairingMaterial::issue(
        PairingMaterialRequest::new(&session_material, vec![endpoint()?]),
        &certificate,
        &FixedUnixClock,
    )
    .map_err(|_| CycleError("pairing material was refused"))?;

    // `SigningKey::random` draws the scalar as the first 32 bytes of the certificate stream.
    let mut private_scalar = [0_u8; 32];
    certificate_random(seed).fill_bytes(&mut private_scalar);

    Ok(vec![
        Needle::new("pairing_token", token.to_vec()),
        Needle::new("pairing_token_hex", encode_hex(&token).into_bytes()),
        Needle::new("certificate_der", certificate.der_bytes().to_vec()),
        Needle::new("tls_fingerprint", fingerprint.to_vec()),
        Needle::new("tls_fingerprint_hex", encode_hex(&fingerprint).into_bytes()),
        Needle::new("private_key", private_scalar.to_vec()),
        // The crypto library keeps the scalar as little-endian 64-bit limbs.
        Needle::new(
            "private_key_limbs",
            private_scalar.iter().rev().copied().collect(),
        ),
        Needle::new("qr_payload", qr.encoded_qr().to_vec()),
    ])
}

/// A paired session: everything it owns drops with it.
pub struct LiveSession {
    _core: SessionCore<ManualClock, SeededRandomSource>,
    _listener: Arc<PairingListener>,
    material: PairingMaterial,
    _client: ClientChannel,
    _server: ServerChannel,
}

impl LiveSession {
    pub fn material(&self) -> &PairingMaterial {
        &self.material
    }
}

/// Runs a complete pairing over loopback TLS 1.3 until the session is established.
pub async fn open_session(seed: u64) -> Result<Box<LiveSession>, CycleError> {
    let (mut core, session_material) = start_pairing(seed)?;
    let listener = Arc::new(
        PairingListener::bind_loopback(certificate(seed)?)
            .await
            .map_err(|_| CycleError("the listener could not bind"))?,
    );
    let material = listener
        .issue_material(
            PairingMaterialRequest::new(&session_material, vec![endpoint()?]),
            &FixedUnixClock,
        )
        .map_err(|_| CycleError("pairing material was refused"))?;
    let mut payload = QrPayload::decode(material.encoded_qr(), UNIX_NOW)
        .map_err(|_| CycleError("the QR payload did not decode"))?;

    let mut pair = SignalingMessage::Pair {
        token: payload.pairing_token,
        dtls_fingerprint: MOBILE_DTLS_FINGERPRINT,
        limits: Limits::new(),
    };
    let frame = pair.encode();
    if let SignalingMessage::Pair { token, .. } = &mut pair {
        token.zeroize();
    }
    payload.pairing_token.zeroize();
    let frame = frame.map_err(|_| CycleError("the pair frame could not be encoded"))?;

    let desktop = {
        let listener = Arc::clone(&listener);
        tokio::spawn(async move {
            let mut server = listener
                .accept_admitted(|| {
                    core.handle(SessionEvent::PeerConnected)
                        .err()
                        .map(|_| ReasonCode::InternalError)
                })
                .await
                .map_err(|_| CycleError("the desktop refused the peer"))?;
            let first = read_frame(&mut server)
                .await
                .map_err(|_| CycleError("the first frame was not read"))?;
            core.authenticate_first_frame(&first)
                .map_err(|_| CycleError("the token was refused"))?;
            Ok::<_, CycleError>((core, server))
        })
    };

    let client = PinnedClient::new(payload.tls_fingerprint)
        .map_err(|_| CycleError("the client pin was rejected"))?;
    let mut client_channel = client
        .connect(listener.local_addr())
        .await
        .map_err(|_| CycleError("the pinned handshake failed"))?;
    write_frame(&mut client_channel, &frame)
        .await
        .map_err(|_| CycleError("the pair frame was not sent"))?;
    let (core, server) = desktop
        .await
        .map_err(|_| CycleError("the desktop task failed"))??;

    // Boxed: a move of the session copies a pointer, not the fingerprint inside it, so no
    // caller frame keeps a stale inline copy.
    Ok(Box::new(LiveSession {
        _core: core,
        _listener: listener,
        material,
        _client: client_channel,
        _server: server,
    }))
}
