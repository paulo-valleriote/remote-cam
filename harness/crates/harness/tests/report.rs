use remotecam_harness::{
    HarnessReport, Outcome, ScenarioConfig, ScenarioKind, Verdict, run, run_with,
};
use remotecam_session_core::{FIRST_MEDIA_FRAME_TIMEOUT_MILLIS, SessionState, Timestamp};

#[test]
fn the_pairing_scenario_reaches_connected_and_passes() {
    let report = run();

    assert_eq!(report.scenario, "pairing");
    assert_eq!(report.outcome, remotecam_harness::Outcome::Connected);
    assert_eq!(report.observed_state, "ended");
    assert!(
        report
            .state_trace
            .iter()
            .any(|transition| transition.to == "connected")
    );
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
        r#"{"report_version":2,"scenario":"pairing","outcome":"connected","observed_state":"ended","message_sequence":["pair","paired","offer","answer"],"state_trace":[{"from":"idle","to":"pairing_ready","at_millis":0},{"from":"pairing_ready","to":"handshaking","at_millis":0},{"from":"handshaking","to":"negotiating","at_millis":0},{"from":"negotiating","to":"connected","at_millis":33},{"from":"connected","to":"ended","at_millis":66}],"frames":[{"sequence":0,"at_millis":33},{"sequence":1,"at_millis":66}],"resources":{"listeners":{"created":1,"active_after_close":0},"connections":{"created":1,"active_after_close":0},"media_ports":{"created":1,"active_after_close":0},"tasks":{"created":1,"active_after_close":0}},"verdict":"pass","summary":{"scenarios":1,"passed":1,"failed":0}}"#
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

#[test]
fn the_public_timeout_scenario_reports_cleanup_and_a_discarded_late_frame() {
    let report = run_with(ScenarioConfig {
        scenario: ScenarioKind::FirstFrameTimeout,
        seed: 42,
        clock_start: Timestamp::ZERO,
    });

    assert_eq!(report.outcome, Outcome::TimedOut);
    assert_eq!(report.observed_state, "idle");
    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(
        report
            .state_trace
            .last()
            .expect("timeout is recorded in the state trace")
            .at_millis,
        FIRST_MEDIA_FRAME_TIMEOUT_MILLIS + 1
    );
    assert_eq!(
        report.late_frame_discarded,
        Some(remotecam_harness::DiscardedFrame {
            sequence: 0,
            at_millis: 3_033,
            reason: "timeout",
        })
    );
    assert_eq!(report.resources.listeners.active_after_close, 0);
    assert_eq!(report.resources.connections.active_after_close, 0);
    assert_eq!(report.resources.media_ports.active_after_close, 0);
    assert_eq!(report.resources.tasks.active_after_close, 0);
}
