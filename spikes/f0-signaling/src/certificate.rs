//! Ephemeral, per-session TLS certificates.
//!
//! The certificate is generated in memory when the pairing listener starts and
//! is destroyed when the session ends. Nothing is written to disk: the QR code
//! carries the SHA-256 of the DER-encoded leaf, and the mobile pins exactly that
//! value.

use std::fmt;

use rcgen::KeyPair;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

/// Subject alternative name carried by every ephemeral certificate.
///
/// The pinned verifier does not consult the name, but the TLS stack requires a
/// well-formed certificate and the signature binding is easier to reason about
/// with a stable subject.
pub const CERTIFICATE_SERVER_NAME: &str = "remotecam.local";

/// Failure while creating the ephemeral certificate.
#[derive(Debug)]
pub enum CertificateError {
    /// `rcgen` refused to generate the certificate or the key.
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
    /// Generates a fresh self-signed ECDSA P-256 certificate for one session.
    pub fn generate() -> Result<Self, CertificateError> {
        Self::from_certified_key(rcgen::generate_simple_self_signed(vec![
            CERTIFICATE_SERVER_NAME.to_owned(),
        ])?)
    }

    /// Wraps a `rcgen` key pair as an ephemeral certificate.
    pub fn from_certified_key(
        certified: rcgen::CertifiedKey<KeyPair>,
    ) -> Result<Self, CertificateError> {
        let der = certified.cert.der().clone();
        let fingerprint = sha256_fingerprint(der.as_ref());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            certified.signing_key.serialize_der(),
        ));
        Ok(Self {
            der,
            key,
            fingerprint,
        })
    }

    /// The DER-encoded leaf certificate.
    pub fn der(&self) -> &CertificateDer<'static> {
        &self.der
    }

    /// The private key in PKCS#8 DER form.
    pub fn key(&self) -> PrivateKeyDer<'static> {
        self.key.clone_key()
    }

    /// The exact SHA-256 fingerprint the mobile pins.
    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// The fingerprint as lowercase hexadecimal, exactly 64 characters.
    pub fn fingerprint_hex(&self) -> String {
        remotecam_protocol::hex::encode_hex(&self.fingerprint)
    }
}

/// Creates a CA certificate and a leaf certificate signed by it.
///
/// This exists for the adversarial *chain-only trust* scenario: the client is
/// given the CA as a trust anchor, so it can tell a genuine public-CA-style
/// chain attempt apart from a random wrong certificate. It is never used by the
/// product path.
#[doc(hidden)]
pub mod test_support {
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};

    use super::{CertificateError, EphemeralCertificate};

    /// A CA certificate, its key, and one leaf signed by it.
    pub struct ChainFixture {
        /// The DER-encoded CA certificate, usable as a trust anchor.
        pub ca_der: Vec<u8>,
        /// The DER-encoded leaf certificate and its key.
        pub leaf: EphemeralCertificate,
        /// The chain the server presents: leaf first, then the CA.
        pub presented_chain: Vec<Vec<u8>>,
    }

    /// Builds a CA and a leaf signed by it.
    pub fn chain_fixture() -> Result<ChainFixture, CertificateError> {
        let ca_key = KeyPair::generate()?;
        let mut ca_params = CertificateParams::new(Vec::new())?;
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "RemoteCam test CA");
        let ca_cert = ca_params.self_signed(&ca_key)?;

        let leaf_key = KeyPair::generate()?;
        let leaf_params = CertificateParams::new(vec![super::CERTIFICATE_SERVER_NAME.to_owned()])?;
        let issuer = Issuer::new(ca_params, ca_key);
        let leaf_cert = leaf_params.signed_by(&leaf_key, &issuer)?;

        let leaf = EphemeralCertificate::from_certified_key(rcgen::CertifiedKey {
            cert: leaf_cert,
            signing_key: leaf_key,
        })?;
        let presented_chain = vec![
            leaf.der().as_ref().to_vec(),
            ca_cert.der().as_ref().to_vec(),
        ];
        Ok(ChainFixture {
            ca_der: ca_cert.der().as_ref().to_vec(),
            leaf,
            presented_chain,
        })
    }
}
