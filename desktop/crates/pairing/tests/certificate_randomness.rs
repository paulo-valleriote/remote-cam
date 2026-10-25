use remotecam_pairing::EphemeralCertificate;
use remotecam_session_core::RandomSource;
use remotecam_session_core::testing::SeededRandomSource;

struct DeterministicRandomSource(SeededRandomSource);

impl RandomSource for DeterministicRandomSource {
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        self.0.fill_bytes(destination);
    }
}

impl remotecam_pairing::certificate::CertificateRandomSource for DeterministicRandomSource {}

#[test]
fn an_injected_random_source_reproduces_the_ephemeral_certificate() {
    let mut first_random = DeterministicRandomSource(SeededRandomSource::new(7));
    let mut second_random = DeterministicRandomSource(SeededRandomSource::new(7));
    let first =
        EphemeralCertificate::generate_with_random_source(&mut first_random).expect("certificate");
    let second =
        EphemeralCertificate::generate_with_random_source(&mut second_random).expect("certificate");

    assert_eq!(first.der_bytes(), second.der_bytes());
    assert_eq!(first.fingerprint(), second.fingerprint());
}

#[test]
fn different_injected_random_sources_produce_different_certificates() {
    let mut first_random = DeterministicRandomSource(SeededRandomSource::new(7));
    let mut second_random = DeterministicRandomSource(SeededRandomSource::new(8));
    let first =
        EphemeralCertificate::generate_with_random_source(&mut first_random).expect("certificate");
    let second =
        EphemeralCertificate::generate_with_random_source(&mut second_random).expect("certificate");

    assert_ne!(first.der_bytes(), second.der_bytes());
    assert_ne!(first.fingerprint(), second.fingerprint());
}
