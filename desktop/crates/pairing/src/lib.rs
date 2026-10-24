//! The pairing adapter: ephemeral certificate, pairing material, TLS 1.3
//! listener and pinned client.
//!
//! The crate isolates `rustls`, `rcgen` and `sha2` from the session domain. It
//! implements the transport half of pairing: a TLS 1.3 channel that only the
//! holder of the exact leaf fingerprint can reach. The token is owned by
//! `remotecam-session-core`; this crate carries it in the QR material and never
//! prints it.

pub mod certificate;
pub mod client;
mod error;
pub mod listener;
pub mod material;

pub use certificate::{
    CERTIFICATE_SERVER_NAME, CertificateError, EphemeralCertificate, sha256_fingerprint,
};
pub use client::{ClientChannel, PinnedClient};
pub use error::{PairingError, classify_io_error, classify_rustls_error};
pub use listener::{ALPN_PROTOCOL, ListenerError, PairingListener, ServerChannel};
pub use material::{DESKTOP_CAPABILITIES, MaterialError, PairingMaterial};
