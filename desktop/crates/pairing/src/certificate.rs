use std::fmt;

use p256::ecdsa::signature::Signer;
use p256::elliptic_curve::rand_core::{CryptoRng, RngCore};
use p256::pkcs8::EncodePrivateKey;
use remotecam_session_core::RandomSource;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const CERTIFICATE_SERVER_NAME: &str = "remotecam.local";

#[derive(Debug)]
pub struct CertificateError {
    stage: CertificateErrorStage,
    diagnostic_message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificateErrorStage {
    Generation,
    PrivateKeyEncoding,
}

impl CertificateError {
    fn generation(error: impl fmt::Display) -> Self {
        Self {
            stage: CertificateErrorStage::Generation,
            diagnostic_message: error.to_string(),
        }
    }

    fn private_key_encoding(error: impl fmt::Display) -> Self {
        Self {
            stage: CertificateErrorStage::PrivateKeyEncoding,
            diagnostic_message: error.to_string(),
        }
    }

    pub fn stage(&self) -> CertificateErrorStage {
        self.stage
    }

    pub fn diagnostic_message(&self) -> &str {
        &self.diagnostic_message
    }
}

impl fmt::Display for CertificateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.stage {
            CertificateErrorStage::Generation => {
                formatter.write_str("certificate generation failed")
            }
            CertificateErrorStage::PrivateKeyEncoding => {
                formatter.write_str("certificate private key encoding failed")
            }
        }
    }
}

impl std::error::Error for CertificateError {}

pub fn sha256_fingerprint(der: &[u8]) -> [u8; 32] {
    Sha256::digest(der).into()
}

pub trait CertificateRandomSource: RandomSource {}

struct RandomSourceAdapter<'a, R>(&'a mut R);

impl<R: CertificateRandomSource> RngCore for RandomSourceAdapter<'_, R> {
    fn next_u32(&mut self) -> u32 {
        let mut bytes = [0; 4];
        self.fill_bytes(&mut bytes);
        u32::from_le_bytes(bytes)
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0; 8];
        self.fill_bytes(&mut bytes);
        u64::from_le_bytes(bytes)
    }

    fn fill_bytes(&mut self, destination: &mut [u8]) {
        self.0.fill_bytes(destination);
    }

    fn try_fill_bytes(
        &mut self,
        destination: &mut [u8],
    ) -> Result<(), p256::elliptic_curve::rand_core::Error> {
        self.fill_bytes(destination);
        Ok(())
    }
}

impl<R: CertificateRandomSource> CryptoRng for RandomSourceAdapter<'_, R> {}

struct InjectedSigningKey {
    key: p256::ecdsa::SigningKey,
    public_key: Vec<u8>,
    private_key: Zeroizing<Vec<u8>>,
}

impl InjectedSigningKey {
    fn generate<R: CertificateRandomSource>(random: &mut R) -> Result<Self, CertificateError> {
        let key = p256::ecdsa::SigningKey::random(&mut RandomSourceAdapter(random));
        let public_key = key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        let private_key = Zeroizing::new(
            key.to_pkcs8_der()
                .map_err(CertificateError::private_key_encoding)?
                .as_bytes()
                .to_vec(),
        );

        Ok(Self {
            key,
            public_key,
            private_key,
        })
    }
}

impl rcgen::PublicKeyData for InjectedSigningKey {
    fn der_bytes(&self) -> &[u8] {
        &self.public_key
    }

    fn algorithm(&self) -> &'static rcgen::SignatureAlgorithm {
        &rcgen::PKCS_ECDSA_P256_SHA256
    }
}

impl rcgen::SigningKey for InjectedSigningKey {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rcgen::Error> {
        let signature: p256::ecdsa::Signature = self.key.sign(message);
        Ok(signature.to_der().as_bytes().to_vec())
    }
}

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
    pub fn generate() -> Result<Self, CertificateError> {
        let certified =
            rcgen::generate_simple_self_signed(vec![CERTIFICATE_SERVER_NAME.to_owned()])
                .map_err(CertificateError::generation)?;
        Ok(Self::from_certified_key(certified))
    }

    pub fn generate_with_random_source<R: CertificateRandomSource>(
        random: &mut R,
    ) -> Result<Self, CertificateError> {
        let signing_key = InjectedSigningKey::generate(random)?;
        let params = rcgen::CertificateParams::new(vec![CERTIFICATE_SERVER_NAME.to_owned()])
            .map_err(CertificateError::generation)?;
        let certificate = params
            .self_signed(&signing_key)
            .map_err(CertificateError::generation)?;
        let der = certificate.der().clone();
        let private_key = signing_key.private_key.to_vec();
        Ok(Self::from_parts(der, private_key))
    }

    fn from_certified_key(certified: rcgen::CertifiedKey<rcgen::KeyPair>) -> Self {
        Self::from_parts(
            certified.cert.der().clone(),
            certified.signing_key.serialize_der(),
        )
    }

    fn from_parts(der: CertificateDer<'static>, private_key: Vec<u8>) -> Self {
        let fingerprint = sha256_fingerprint(der.as_ref());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private_key));
        Self {
            der,
            key,
            fingerprint,
        }
    }

    pub fn der_bytes(&self) -> &[u8] {
        self.der.as_ref()
    }

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
    fn a_generation_failure_exposes_a_stable_stage_and_text_diagnostic() {
        let error = CertificateError::generation(rcgen::Error::Time);

        assert_eq!(error.stage(), super::CertificateErrorStage::Generation);
        assert_eq!(error.to_string(), "certificate generation failed");
        assert_eq!(error.diagnostic_message(), "Time error");
        assert!(std::error::Error::source(&error).is_none());
    }

    #[test]
    fn a_private_key_encoding_failure_exposes_a_stable_stage_and_text_diagnostic() {
        let error = CertificateError::private_key_encoding("invalid encoding");

        assert_eq!(
            error.stage(),
            super::CertificateErrorStage::PrivateKeyEncoding
        );
        assert_eq!(error.to_string(), "certificate private key encoding failed");
        assert_eq!(error.diagnostic_message(), "invalid encoding");
        assert!(std::error::Error::source(&error).is_none());
    }
}
