//! Ephemeral, per-session TLS certificates.
//!
//! The certificate and its private key exist in memory only. The QR payload
//! carries the SHA-256 of the DER-encoded leaf, and the mobile pins exactly that
//! value, so no certificate authority or persisted key is involved.

use std::fmt;

use rcgen::KeyPair;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

/// The subject alternative name carried by every ephemeral certificate.
pub const CERTIFICATE_SERVER_NAME: &str = "remotecam.local";

/// Failure while creating the ephemeral certificate.
#[derive(Debug)]
pub enum CertificateError {
    /// The generator refused to create the certificate or the key.
    Generation(rcgen::Error),
}

impl fmt::Display for CertificateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generation(error) => write!(formatter, "certificate generation failed: {error}"),
        }
    }
}

impl std::error::Error for CertificateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Generation(error) => Some(error),
        }
    }
}

impl From<rcgen::Error> for CertificateError {
    fn from(error: rcgen::Error) -> Self {
        Self::Generation(error)
    }
}

/// The SHA-256 digest of a DER-encoded certificate.
pub fn sha256_fingerprint(der: &[u8]) -> [u8; 32] {
    Sha256::digest(der).into()
}

/// One in-memory ECDSA P-256 certificate and its private key.
pub struct EphemeralCertificate {
    der: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    fingerprint: [u8; 32],
}

impl fmt::Debug for EphemeralCertificate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EphemeralCertificate")
            .field("fingerprint", &"[elided]")
            .field("der_bytes", &self.der.as_ref().len())
            .finish()
    }
}

impl EphemeralCertificate {
    /// Generates a fresh self-signed certificate for one session.
    pub fn generate() -> Result<Self, CertificateError> {
        let certified =
            rcgen::generate_simple_self_signed(vec![CERTIFICATE_SERVER_NAME.to_owned()])?;
        Ok(Self::from_certified_key(certified))
    }

    fn from_certified_key(certified: rcgen::CertifiedKey<KeyPair>) -> Self {
        let der = certified.cert.der().clone();
        let fingerprint = sha256_fingerprint(der.as_ref());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            certified.signing_key.serialize_der(),
        ));
        Self {
            der,
            key,
            fingerprint,
        }
    }

    /// The DER-encoded leaf certificate.
    pub fn der_bytes(&self) -> &[u8] {
        self.der.as_ref()
    }

    /// The exact SHA-256 fingerprint the QR payload carries.
    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    pub(crate) fn into_parts(self) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        (self.der, self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::{CertificateError, EphemeralCertificate, sha256_fingerprint};

    #[test]
    fn the_fingerprint_is_the_sha256_of_the_leaf() {
        let certificate = EphemeralCertificate::generate().expect("certificate");

        assert_eq!(
            certificate.fingerprint(),
            sha256_fingerprint(certificate.der_bytes())
        );
    }

    #[test]
    fn every_session_gets_a_different_certificate() {
        let first = EphemeralCertificate::generate().expect("certificate");
        let second = EphemeralCertificate::generate().expect("certificate");

        assert_ne!(first.der_bytes(), second.der_bytes());
        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn debug_elides_the_fingerprint() {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let fingerprint = format!("{:?}", certificate.fingerprint());

        let rendered = format!("{certificate:?}");

        assert!(rendered.contains("[elided]"));
        assert!(!rendered.contains(&fingerprint));
    }

    #[test]
    fn a_generation_failure_names_the_stage_and_keeps_the_cause() {
        let error = CertificateError::from(rcgen::Error::Time);

        assert_eq!(
            error.to_string(),
            "certificate generation failed: Time error"
        );
        assert!(std::error::Error::source(&error).is_some());
    }
}
