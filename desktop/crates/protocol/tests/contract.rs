//! Contract conformance against the pinned vectors in `protocol/vectors/`.

use remotecam_protocol::conformance::{repository_root, run_vectors};

#[test]
fn every_pinned_vector_produces_its_expected_outcome() {
    let report = run_vectors(&repository_root().join("protocol/vectors"))
        .expect("conformance vectors must execute");
    assert!(
        report.passed(),
        "conformance failures: {:?}",
        report.failures()
    );
    assert_eq!(report.qr.len(), 11, "every QR vector must be executed");
    assert_eq!(
        report.signaling.len(),
        43,
        "every message scenario must be executed"
    );
    assert_eq!(report.reason_codes.count, 26);
    assert!(report.reason_codes.ok);
    assert_eq!(
        report.encoding.len(),
        13,
        "every encoding vector must be executed"
    );
}

#[test]
fn the_repository_contract_documents_are_present() {
    let root = repository_root().join("protocol");
    for name in [
        "signaling.schema.json",
        "reason-codes.json",
        "qr-payload.md",
        "vectors/run_conformance.sh",
    ] {
        assert!(root.join(name).is_file(), "missing protocol/{name}");
    }
}
