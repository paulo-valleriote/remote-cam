use remotecam_harness::{HarnessReport, Verdict, run};
use remotecam_session_core::SessionState;

#[test]
fn the_pairing_scenario_reaches_connected_and_passes() {
    let report = run();

    assert_eq!(report.scenario, "pairing");
    assert_eq!(report.observed_state, "connected");
    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(report.summary.scenarios, 1);
    assert_eq!(report.summary.passed, 1);
    assert_eq!(report.summary.failed, 0);
}

#[test]
fn a_session_that_does_not_reach_connected_fails_the_scenario() {
    let report = HarnessReport::for_session_state(SessionState::Ended);

    assert_eq!(report.observed_state, "ended");
    assert_eq!(report.verdict, Verdict::Fail);
    assert_eq!(report.summary.scenarios, 1);
    assert_eq!(report.summary.passed, 0);
    assert_eq!(report.summary.failed, 1);
    assert_eq!(report.reason_code, Some("internal_error"));
}

#[test]
fn the_report_serialises_to_the_documented_json_line() {
    let json = run().to_json().expect("the report is a plain JSON value");

    assert_eq!(
        json,
        r#"{"report_version":1,"scenario":"pairing","observed_state":"connected","message_sequence":["pair","paired","offer","answer"],"verdict":"pass","summary":{"scenarios":1,"passed":1,"failed":0}}"#
    );
    assert!(!json.contains("token"));
    assert!(!json.contains("fingerprint"));
}

#[test]
fn the_same_state_produces_the_same_report() {
    let first = HarnessReport::for_session_state(SessionState::Connected)
        .to_json()
        .expect("the report is a plain JSON value");
    let second = HarnessReport::for_session_state(SessionState::Connected)
        .to_json()
        .expect("the report is a plain JSON value");

    assert_eq!(first, second);
}

#[test]
fn the_pairing_run_is_reproducible() {
    let first = run().to_json().expect("the report is a plain JSON value");
    let second = run().to_json().expect("the report is a plain JSON value");

    assert_eq!(first, second);
}

#[test]
fn the_pairing_run_observes_the_mobile_message_sequence() {
    let report = run();

    assert_eq!(
        report.message_sequence,
        ["pair", "paired", "offer", "answer"]
    );
}
