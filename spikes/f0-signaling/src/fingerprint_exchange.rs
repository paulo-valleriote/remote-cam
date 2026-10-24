//! Media transport fingerprint exchange inside the authenticated channel.
//!
//! The DTLS-SRTP fingerprints are the only thing binding the authenticated
//! signaling channel to the media plane. They therefore travel **after** the
//! token and certificate checks succeed, and only through the session handle a
//! successful pairing produces. A tampered fingerprint aborts before any media
//! is started.

use std::fmt;

use remotecam_protocol::ReasonCode;
use subtle::ConstantTimeEq;

/// One SHA-256 fingerprint per media stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFingerprints {
    /// The audio stream's DTLS fingerprint.
    pub audio: [u8; 32],
    /// The video stream's DTLS fingerprint.
    pub video: [u8; 32],
}

impl MediaFingerprints {
    /// Creates a fingerprint pair.
    pub fn new(audio: [u8; 32], video: [u8; 32]) -> Self {
        Self { audio, video }
    }
}

/// Proof that pairing completed and the channel is authenticated.
///
/// The type is deliberately not constructible outside this module except through
/// [`AuthenticatedSession::establish`], so the fingerprint exchange cannot be
/// called on an unauthenticated connection.
#[derive(Debug)]
pub struct AuthenticatedSession {
    session_id: [u8; 16],
    epoch: u32,
    origin: String,
    exchange: MediaFingerprints,
}

impl AuthenticatedSession {
    /// Establishes the session after the token and certificate checks passed.
    ///
    /// This is the *only* constructor. It is called by the listener once the
    /// pairing message has been authenticated, and it is what makes media
    /// startable.
    pub fn establish(
        session_id: [u8; 16],
        epoch: u32,
        origin: impl Into<String>,
        exchange: MediaFingerprints,
    ) -> Self {
        Self {
            session_id,
            epoch,
            origin: origin.into(),
            exchange,
        }
    }

    /// The session identifier announced in the QR payload.
    pub fn session_id(&self) -> [u8; 16] {
        self.session_id
    }

    /// The session generation.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// The origin that completed pairing.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The fingerprints received inside the authenticated channel.
    pub fn fingerprints(&self) -> &MediaFingerprints {
        &self.exchange
    }
}

/// A mismatch between the offered and the expected media fingerprints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FingerprintMismatch {
    /// Always [`ReasonCode::ProtocolViolation`]; the session must not start.
    pub code: ReasonCode,
}

impl fmt::Display for FingerprintMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("media fingerprint did not match the expected value")
    }
}

impl std::error::Error for FingerprintMismatch {}

/// Compares the offered fingerprints with the expected values in constant time.
pub fn verify_fingerprints(
    offered: &MediaFingerprints,
    expected: &MediaFingerprints,
) -> Result<(), FingerprintMismatch> {
    let audio = bool::from(offered.audio.ct_eq(&expected.audio));
    let video = bool::from(offered.video.ct_eq(&expected.video));
    if audio && video {
        Ok(())
    } else {
        Err(FingerprintMismatch {
            code: ReasonCode::ProtocolViolation,
        })
    }
}

/// Accepts a fingerprint exchange on an authenticated session.
///
/// Anything other than an exact match aborts before media starts.
pub fn accept_exchange(
    session: &AuthenticatedSession,
    expected: &MediaFingerprints,
) -> Result<(), FingerprintMismatch> {
    verify_fingerprints(session.fingerprints(), expected)
}
