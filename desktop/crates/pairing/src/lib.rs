pub mod certificate;
pub mod client;
mod error;
pub mod listener;
pub mod material;

pub use certificate::{
    CERTIFICATE_SERVER_NAME, CertificateError, CertificateErrorStage, EphemeralCertificate,
    sha256_fingerprint,
};
pub use client::{ClientChannel, PinnedClient};
pub use error::{PairingError, classify_io_error, classify_rustls_error};
pub use listener::{ALPN_PROTOCOL, ListenerError, PairingListener, ServerChannel, reject_socket};
pub use material::{
    DESKTOP_CAPABILITIES, MaterialError, PairingMaterial, PairingMaterialRequest, SystemUnixClock,
    UnixClock,
};
