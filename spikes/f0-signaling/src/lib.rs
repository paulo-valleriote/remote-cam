//! Pairing handshake for RemoteCam.
//!
//! This crate is the F0 risk-reduction implementation of the trust boundary:
//! a TLS 1.3 listener with an ephemeral certificate, exact SHA-256 certificate
//! pinning on the client, a single-use pairing token, a fingerprint exchange
//! that only happens inside the authenticated channel, and per-origin
//! throttling with listener shutdown.
//!
//! **NON-PRODUCTION**: the F0 spike harness in `spikes/f0-handshake/` drives
//! these modules and produces the pairing-gate evidence. The product protocol
//! phase starts from the recorded design, not from this code being merged.

pub mod certificate;
pub mod client;
pub mod fingerprint_exchange;
pub mod qr;
pub mod server;
pub mod throttling;
pub mod token;

pub use certificate::{CERTIFICATE_SERVER_NAME, CertificateError, EphemeralCertificate};
pub use client::{ExactPinVerifier, PairingError, PinnedCertificateVerifier, PinnedClient};
pub use fingerprint_exchange::{
    AuthenticatedSession, FingerprintMismatch, MediaFingerprints, accept_exchange,
    verify_fingerprints,
};
pub use qr::{QR_VALIDITY, QrDisplay, QrRenderError, build_display, build_payload, render};
pub use server::{
    AcceptedPairing, PairingRejection, PairingServer, ServerError, SourcePolicy, Subnet,
    is_permitted_source,
};
pub use throttling::{
    ATTEMPT_LIMIT_PER_MINUTE, ATTEMPT_WINDOW_MS, CONSECUTIVE_FAILURE_LIMIT, ThrottleAction,
    ThrottleDecision, Throttling,
};
pub use token::{TOKEN_BYTES, TOKEN_VALIDITY, TokenStore};
