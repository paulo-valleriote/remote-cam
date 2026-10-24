use remotecam_harness::{HarnessReport, Verdict, run};
use remotecam_session_core::SessionState;

#[test]
fn the_scaffold_scenario_starts_from_idle_and_passes() {
    let report = run();

    assert_eq!(report.scenario, "scaffold");
    assert_eq!(report.observed_state, "idle");
    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(report.summary.scenarios, 1);
    assert_eq!(report.summary.passed, 1);
    assert_eq!(report.summary.failed, 0);
}

#[test]
fn a_session_that_does_not_rest_in_idle_fails_the_scenario() {
    let report = HarnessReport::for_session_state(SessionState::Ended);

    assert_eq!(report.observed_state, "ended");
    assert_eq!(report.verdict, Verdict::Fail);
    assert_eq!(report.summary.scenarios, 1);
    assert_eq!(report.summary.passed, 0);
    assert_eq!(report.summary.failed, 1);
}

#[test]
fn the_report_serialises_to_the_documented_json_line() {
    let json = HarnessReport::for_session_state(SessionState::Idle)
        .to_json()
        .expect("the report is a plain JSON value");

    assert_eq!(
        json,
        r#"{"report_version":1,"scenario":"scaffold","observed_state":"idle","verdict":"pass","summary":{"scenarios":1,"passed":1,"failed":0}}"#
    );
}

#[test]
fn the_same_state_produces_the_same_report() {
    let first = HarnessReport::for_session_state(SessionState::Idle)
        .to_json()
        .expect("the report is a plain JSON value");
    let second = HarnessReport::for_session_state(SessionState::Idle)
        .to_json()
        .expect("the report is a plain JSON value");

    assert_eq!(first, second);
}
