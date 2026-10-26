//! The pinned handshake scenarios replayed on the Rust Session Core.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use remotecam_session_core::conformance::{
    HandshakeResult, repository_root, run_cli, run_handshake_scenarios,
};

fn vectors() -> PathBuf {
    repository_root().join("protocol/vectors")
}

fn run() -> Vec<HandshakeResult> {
    run_handshake_scenarios(&vectors()).expect("every pinned handshake scenario holds")
}

fn result<'a>(results: &'a [HandshakeResult], id: &str) -> &'a HandshakeResult {
    results
        .iter()
        .find(|result| result.id == id)
        .unwrap_or_else(|| panic!("{id} is reported"))
}

#[test]
fn every_declared_handshake_scenario_is_reported_exactly_once() {
    let text = fs::read_to_string(vectors().join("signaling/scenarios.json")).unwrap();
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    let declared: Vec<&str> = document["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|scenario| scenario["kind"] == "handshake")
        .map(|scenario| scenario["id"].as_str().unwrap())
        .collect();

    let results = run();
    let reported: Vec<&str> = results.iter().map(|result| result.id.as_str()).collect();

    assert_eq!(reported, declared);
}

#[test]
fn a_valid_pairing_ends_negotiating_on_the_baseline_epoch() {
    let results = run();
    let valid = result(&results, "hs-valid-pairing");

    assert!(valid.executed);
    assert_eq!(valid.accepted, Some(true));
    assert_eq!(valid.state.as_deref(), Some("negotiating"));
    assert_eq!(
        valid.transition.as_deref(),
        Some("handshaking->negotiating")
    );
    assert_eq!(valid.epoch, Some(1));
    assert_eq!(valid.reason_code, None);
}

#[test]
fn a_late_token_is_refused_and_a_fresh_qr_is_issued() {
    let results = run();
    let expired = result(&results, "hs-token-expired");

    assert_eq!(expired.reason_code.as_deref(), Some("token_expired"));
    assert_eq!(expired.state.as_deref(), Some("pairing_ready"));
    assert_eq!(expired.epoch, Some(2));
}

#[test]
fn a_stale_epoch_is_discarded_and_counted() {
    let results = run();
    let stale = result(&results, "hs-protocol-violation");

    assert_eq!(stale.reason_code.as_deref(), Some("protocol_violation"));
    assert_eq!(stale.protocol_violations, Some(1));
}

#[test]
fn qr_scenarios_report_the_decoder_reason() {
    let results = run();

    assert_eq!(
        result(&results, "hs-qr-unsupported-version")
            .reason_code
            .as_deref(),
        Some("unsupported_qr_version")
    );
    assert_eq!(result(&results, "hs-qr-expired").state, None);
}

#[test]
fn transport_scenarios_are_reported_as_not_executed() {
    let results = run();
    let proxy = result(&results, "hs-interception-proxy");

    assert_eq!(proxy.layer, "transport");
    assert!(!proxy.executed);
    assert_eq!(proxy.accepted, None);
    assert_eq!(proxy.reason_code, None);
}

fn scratch_vectors(scenarios: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "remotecam-hs-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let directory = root.join("signaling");
    fs::create_dir_all(&directory).unwrap();
    fs::create_dir_all(root.join("qr")).unwrap();
    fs::write(directory.join("scenarios.json"), scenarios).unwrap();
    root
}

fn scenario_with(extra: &str) -> String {
    format!(
        r#"{{"version":1,"scenarios":[{{"id":"hs-x","kind":"handshake","expected_outcome":"accept","expected_reason_code":null,{extra}}}]}}"#
    )
}

fn expect_failure(scenarios: &str) -> String {
    let root = scratch_vectors(scenarios);
    let error = run_handshake_scenarios(&root).expect_err("the scenario must fail");
    fs::remove_dir_all(&root).ok();
    error
}

#[test]
fn a_diverging_expectation_fails_the_run() {
    let error = expect_failure(&scenario_with(
        r#""layer":"core","steps":[{"action":"start_pairing"}],"expected_state":"idle","expected_transition":"idle->pairing_ready","expected_epoch":1,"expected_protocol_violations":0"#,
    ));
    assert!(error.contains("hs-x"), "{error}");
    assert!(error.contains("state"), "{error}");
}

#[test]
fn an_unknown_layer_fails_instead_of_being_skipped() {
    let error = expect_failure(&scenario_with(r#""layer":"mystery""#));
    assert!(error.contains("mystery"), "{error}");
}

#[test]
fn an_unknown_step_action_fails() {
    let error = expect_failure(&scenario_with(
        r#""layer":"core","steps":[{"action":"teleport"}]"#,
    ));
    assert!(error.contains("teleport"), "{error}");
}

#[test]
fn a_core_scenario_without_steps_fails() {
    let error = expect_failure(&scenario_with(r#""layer":"core","steps":[]"#));
    assert!(error.contains("steps"), "{error}");
}

#[test]
fn a_scenario_without_a_layer_fails() {
    let error = expect_failure(&scenario_with(r#""note":"none""#));
    assert!(error.contains("layer"), "{error}");
}

#[test]
fn a_missing_scenario_file_is_an_error() {
    let error = run_handshake_scenarios(Path::new("/nonexistent/remotecam")).unwrap_err();
    assert!(error.contains("scenarios.json"), "{error}");
}

#[test]
fn every_mismatching_field_is_reported_in_one_failure() {
    let error = expect_failure(
        r#"{"version":1,"scenarios":[{"id":"hs-x","kind":"handshake","layer":"core","steps":[{"action":"start_pairing"}],"expected_outcome":"reject","expected_reason_code":"token_invalid","expected_state":"idle","expected_transition":"idle->idle","expected_epoch":9,"expected_protocol_violations":5}]}"#,
    );
    for field in [
        "accepted",
        "reason_code",
        "state",
        "transition",
        "epoch",
        "protocol_violations",
    ] {
        assert!(error.contains(field), "{field} missing from {error}");
    }
}

#[test]
fn every_step_action_is_understood() {
    let steps = [
        "start_pairing",
        "peer_connected",
        "negotiation_completed",
        "first_media_frame",
        "end_session",
        "first_frame_timed_out",
    ]
    .map(|action| format!(r#"{{"action":"{action}"}}"#))
    .join(",");
    let error = expect_failure(&scenario_with(&format!(
        r#""layer":"core","steps":[{steps}]"#
    )));
    assert!(!error.contains("unknown step"), "{error}");
    assert!(error.contains("reason_code"), "{error}");
}

#[test]
fn a_step_missing_its_arguments_fails() {
    let clock = expect_failure(&scenario_with(
        r#""layer":"core","steps":[{"action":"advance_clock"}]"#,
    ));
    assert!(clock.contains("millis"), "{clock}");
    let epoch = expect_failure(&scenario_with(
        r#""layer":"core","steps":[{"action":"admit_epoch"}]"#,
    ));
    assert!(epoch.contains("epoch"), "{epoch}");
}

#[test]
fn a_token_reference_that_does_not_exist_fails() {
    let none = expect_failure(&scenario_with(
        r#""layer":"core","steps":[{"action":"pair","token":"first"}]"#,
    ));
    assert!(none.contains("no pairing token"), "{none}");
    let unknown = expect_failure(&scenario_with(
        r#""layer":"core","steps":[{"action":"pair","token":"nope"}]"#,
    ));
    assert!(unknown.contains("unknown token"), "{unknown}");
}

#[test]
fn a_qr_scenario_needs_a_readable_vector_with_valid_hex() {
    let missing_name = expect_failure(&scenario_with(r#""layer":"qr""#));
    assert!(missing_name.contains("qr_vector"), "{missing_name}");
    let missing_file = expect_failure(&scenario_with(r#""layer":"qr","qr_vector":"absent""#));
    assert!(missing_file.contains("cannot read"), "{missing_file}");

    let root = scratch_vectors(&scenario_with(r#""layer":"qr","qr_vector":"broken""#));
    fs::write(
        root.join("qr/broken.json"),
        r#"{"now_unix":1,"expected":{"payload_hex":"zz"}}"#,
    )
    .unwrap();
    let error = run_handshake_scenarios(&root).unwrap_err();
    fs::remove_dir_all(&root).ok();
    assert!(error.contains("invalid payload hex"), "{error}");
}

#[test]
fn a_qr_scenario_that_decodes_is_accepted() {
    let root = scratch_vectors(&scenario_with(r#""layer":"qr","qr_vector":"minimal-v1""#));
    fs::copy(
        vectors().join("qr/minimal-v1.json"),
        root.join("qr/minimal-v1.json"),
    )
    .unwrap();
    let results = run_handshake_scenarios(&root).unwrap();
    fs::remove_dir_all(&root).ok();
    assert_eq!(results[0].accepted, Some(true));
    assert_eq!(results[0].reason_code, None);
}

fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn the_command_prints_the_document_when_no_output_file_is_given() {
    let document = run_cli(&[]).unwrap().expect("a document is printed");
    let parsed: serde_json::Value = serde_json::from_str(&document).unwrap();
    assert_eq!(parsed["handshake"].as_array().unwrap().len(), 22);
}

#[test]
fn the_command_writes_the_document_to_the_requested_file() {
    let path = std::env::temp_dir().join(format!("remotecam-hs-out-{}.json", std::process::id()));
    let output = path.to_str().unwrap();
    let printed = run_cli(&arguments(&["--output", output])).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    fs::remove_file(&path).ok();
    assert_eq!(printed, None);
    assert!(written.ends_with("}\n"), "{written}");
}

#[test]
fn the_command_refuses_unknown_arguments_and_missing_values() {
    assert!(
        run_cli(&arguments(&["--nope"]))
            .unwrap_err()
            .contains("usage")
    );
    assert!(
        run_cli(&arguments(&["--vectors"]))
            .unwrap_err()
            .contains("usage")
    );
}

#[test]
fn the_command_fails_on_bad_vectors_and_unwritable_output() {
    let bad = run_cli(&arguments(&["--vectors", "/nonexistent/remotecam"])).unwrap_err();
    assert!(bad.contains("scenarios.json"), "{bad}");
    let unwritable =
        run_cli(&arguments(&["--output", "/nonexistent/remotecam/out.json"])).unwrap_err();
    assert!(unwritable.contains("cannot write"), "{unwritable}");
}
