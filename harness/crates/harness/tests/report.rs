use remotecam_harness::{
    Outcome, RunOptions, ScenarioConfig, ScenarioKind, Verdict, run, run_with, run_with_options,
};
use remotecam_session_core::{FIRST_MEDIA_FRAME_TIMEOUT_MILLIS, Timestamp};
use serde_json::Value;

const REPORT_SCHEMA: &str = include_str!("../../../report.schema.json");
const GOLDEN_REPORT: &str = include_str!("fixtures/report.json");

#[test]
fn the_reported_scenarios_pass_with_expected_results() {
    let report = run().expect("Git and the pinned Rust toolchain are available");

    assert_eq!(report.report_version, 7);
    assert_eq!(report.contract_version, 1);
    assert_eq!(report.scenarios.len(), 10);
    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(report.summary.scenarios, 10);
    assert_eq!(report.summary.passed, 10);
    assert_eq!(report.summary.failed, 0);

    let happy_path = &report.scenarios[0];
    assert_eq!(happy_path.scenario, "pairing-happy-path");
    assert_eq!(happy_path.outcome, Outcome::Accepted);
    assert_eq!(happy_path.observed_state, "ended");
    assert_eq!(happy_path.reason_code, None);
    assert_eq!(happy_path.verdict, Verdict::Pass);
    assert_eq!(
        happy_path.message_sequence,
        ["pair", "paired", "offer", "answer"]
    );
    assert!(
        happy_path
            .state_trace
            .iter()
            .any(|transition| transition.to == "connected")
    );

    let pin_mismatch = &report.scenarios[1];
    assert_eq!(pin_mismatch.scenario, "pin-mismatch-aborts");
    assert_eq!(pin_mismatch.outcome, Outcome::Rejected);
    assert_eq!(pin_mismatch.reason_code, Some("certificate_pin_mismatch"));
    assert_eq!(pin_mismatch.verdict, Verdict::Pass);
    assert!(pin_mismatch.message_sequence.is_empty());
    assert_eq!(pin_mismatch.resources.connections.created, 0);
    assert_eq!(pin_mismatch.resources.listeners.active_after_close, 0);
    assert_eq!(pin_mismatch.resources.tasks.active_after_close, 0);
}

#[test]
fn the_serialised_report_matches_the_versioned_golden_fixture() {
    let report = run().expect("Git and the pinned Rust toolchain are available");
    let actual: Value =
        serde_json::from_str(&report.to_json().expect("report serialises")).unwrap();
    let mut expected: Value = serde_json::from_str(GOLDEN_REPORT).unwrap();

    expected["commit"] = actual["commit"].clone();
    expected["environment"] = actual["environment"].clone();

    assert_eq!(actual, expected);
    assert!(!report.to_json().unwrap().contains("pairing_token"));
    assert!(!report.to_json().unwrap().contains("fingerprint"));
}

#[test]
fn every_emitted_report_validates_against_the_versioned_schema() {
    let report = run().expect("Git and the pinned Rust toolchain are available");
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("report schema is valid");
    let instance: Value =
        serde_json::from_str(&report.to_json().expect("report serialises")).unwrap();

    let errors: Vec<String> = validator
        .iter_errors(&instance)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "report schema errors: {errors:#?}");
}

#[test]
fn the_versioned_schema_rejects_results_that_violate_scenario_contracts() {
    let report = run().expect("Git and the pinned Rust toolchain are available");
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("report schema is valid");
    let instance: Value =
        serde_json::from_str(&report.to_json().expect("report serialises")).unwrap();

    let invalid_scenarios = [
        (0, "outcome", serde_json::json!("rejected")),
        (0, "verdict", serde_json::json!("fail")),
        (0, "reason_code", serde_json::json!("internal_error")),
        (1, "outcome", serde_json::json!("accepted")),
        (1, "verdict", serde_json::json!("fail")),
        (1, "reason_code", serde_json::Value::Null),
    ];

    for (index, field, value) in invalid_scenarios {
        let mut invalid_instance = instance.clone();
        invalid_instance["scenarios"][index][field] = value;

        assert!(
            validator.iter_errors(&invalid_instance).next().is_some(),
            "schema accepted invalid scenario {index} field {field}"
        );
    }
}

#[test]
fn the_versioned_schema_rejects_a_loss_recovery_that_cannot_pass_the_gate() {
    let report = run().expect("Git and the pinned Rust toolchain are available");
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("report schema is valid");
    let instance: Value =
        serde_json::from_str(&report.to_json().expect("report serialises")).unwrap();

    let breaches = [
        ("profile", serde_json::json!("out_of_profile")),
        ("absent_ticks", serde_json::json!(1)),
        ("longest_without_fresh_millis", serde_json::json!(2_000)),
        ("nack_requests", serde_json::json!(0)),
    ];
    for (field, value) in breaches {
        let mut invalid_instance = instance.clone();
        invalid_instance["scenarios"][9]["real_media"]["loss_recovery"][field] = value;
        assert!(
            validator.iter_errors(&invalid_instance).next().is_some(),
            "schema accepted a loss_recovery breaching {field}"
        );
    }
}

#[test]
fn the_versioned_schema_rejects_an_audio_continuity_that_cannot_pass_the_gate() {
    let report = run().expect("Git and the pinned Rust toolchain are available");
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("report schema is valid");
    let instance: Value =
        serde_json::from_str(&report.to_json().expect("report serialises")).unwrap();

    let breaches = [
        ("profile", serde_json::json!("out_of_profile")),
        ("delivery_dropouts", serde_json::json!(1)),
        ("silence_dropouts", serde_json::json!(1)),
        ("duration_seconds", serde_json::json!(599)),
    ];
    for (field, value) in breaches {
        let mut invalid_instance = instance.clone();
        invalid_instance["scenarios"][9]["real_media"]["audio_continuity"][field] = value;
        assert!(
            validator.iter_errors(&invalid_instance).next().is_some(),
            "schema accepted an audio_continuity breaching {field}"
        );
    }
}

#[test]
fn repeated_runs_with_identical_inputs_differ_only_in_environment() {
    let options = RunOptions {
        seed: 42,
        clock_start_millis: 100,
    };
    let first = run_with_options(options);
    let second = run_with_options(options);
    let first = serde_json::to_value(first.expect("first run succeeds")).unwrap();
    let second = serde_json::to_value(second.expect("second run succeeds")).unwrap();

    assert_eq!(first["commit"], second["commit"]);
    let mut normalized_first = first;
    let mut normalized_second = second;
    normalized_first
        .as_object_mut()
        .unwrap()
        .remove("environment");
    normalized_second
        .as_object_mut()
        .unwrap()
        .remove("environment");
    assert_eq!(normalized_first, normalized_second);
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

#[test]
fn report_metadata_and_environment_have_the_approved_shape() {
    let report = run().expect("Git and the pinned Rust toolchain are available");

    assert_eq!(report.seed, 0);
    assert_eq!(report.clock_start_millis, 0);
    assert!(matches!(report.commit.len(), 40 | 64));
    assert!(report.commit.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(!report.environment.os.is_empty());
    assert!(!report.environment.architecture.is_empty());
    assert!(!report.environment.rustc_version.is_empty());
    let environment = serde_json::to_value(report.environment).unwrap();
    let mut environment_keys: Vec<&str> = environment
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    environment_keys.sort_unstable();
    assert_eq!(environment_keys, ["architecture", "os", "rustc_version"]);
}

#[test]
fn every_rec_10_acceptance_scenario_reports_its_refusal_contract() {
    let report = run().expect("Git and the pinned Rust toolchain are available");
    type Expectation = (
        &'static str,
        Outcome,
        Option<&'static str>,
        &'static [&'static str],
        &'static [u64],
    );
    let expected: [Expectation; 7] = [
        (
            "qr-expiry-refused",
            Outcome::Rejected,
            Some("qr_expired"),
            &[],
            &[1, 2],
        ),
        (
            "token-expiry-refused",
            Outcome::Rejected,
            Some("token_expired"),
            &["pair"],
            &[1, 2],
        ),
        (
            "single-use-refused",
            Outcome::Rejected,
            Some("superseded"),
            &[],
            &[1],
        ),
        (
            "replay-refused",
            Outcome::Rejected,
            Some("token_already_used"),
            &["pair"],
            &[1, 2],
        ),
        (
            "stale-epoch-discarded",
            Outcome::Accepted,
            None,
            &["pair", "paired", "offer", "answer"],
            &[1, 2],
        ),
        (
            "clock-backwards-window",
            Outcome::Rejected,
            Some("token_expired"),
            &["pair"],
            &[1, 2],
        ),
        (
            "regeneration-after-refusal",
            Outcome::Accepted,
            None,
            &["pair", "paired", "offer", "answer"],
            &[1, 2],
        ),
    ];

    assert_eq!(report.scenarios.len(), 10);
    for (scenario, (name, outcome, reason, messages, epochs)) in
        report.scenarios[2..].iter().zip(expected)
    {
        assert_eq!(scenario.scenario, name);
        assert_eq!(scenario.outcome, outcome, "{name}");
        assert_eq!(scenario.reason_code, reason, "{name}");
        assert_eq!(scenario.message_sequence, messages, "{name}");
        assert_eq!(scenario.issued_epochs, epochs, "{name}");
        assert_eq!(scenario.verdict, Verdict::Pass, "{name}");
    }
    assert_eq!(report.scenarios[6].protocol_violations, Some(1));
}

#[test]
fn real_media_reaches_connected_with_a_decoded_nv12_frame() {
    let report = run_with(ScenarioConfig {
        scenario: ScenarioKind::RealMediaFrame,
        seed: 0,
        clock_start: Timestamp::ZERO,
    });

    assert_eq!(report.scenario, "real-media-frame");
    assert_eq!(report.outcome, Outcome::Accepted);
    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(report.observed_state, "ended");
    assert!(
        report
            .state_trace
            .iter()
            .any(|transition| transition.to == "connected" && transition.at_millis == 33)
    );
    assert_eq!(report.frames.len(), 2);

    let media = report.real_media.expect("the real Media Core observation");
    assert_eq!(media.pixel_format, "nv12");
    assert_eq!((media.width, media.height), (64, 48));
    assert_eq!(media.frames_decoded, 2);
    assert_eq!(media.frames_dropped, 0);
    assert_eq!(media.ring_slots, 3);
    assert!(media.receive_only);
    assert_eq!(report.resources.media_ports.created, 1);
    assert_eq!(report.resources.media_ports.active_after_close, 0);
}

#[test]
fn real_media_reports_the_nominal_loss_profile_with_nack_and_pli_observable() {
    let report = run_with(ScenarioConfig {
        scenario: ScenarioKind::RealMediaFrame,
        seed: 0,
        clock_start: Timestamp::ZERO,
    });
    let loss = report.real_media.expect("real media").loss_recovery;

    assert_eq!(loss.profile, "nominal");
    assert_eq!(loss.duration_seconds, 10);
    assert!(loss.nack_requests > 0 && loss.nacked_packets >= loss.nack_requests);
    assert_eq!(loss.absent_ticks, 0, "frozen video is forbidden");
    assert!(
        loss.longest_without_fresh_millis < 2_000,
        "the video never froze for 2 s: {loss:?}"
    );
    assert!(loss.fresh_frames > 0 && loss.packets_sent > 0);
}

#[test]
fn real_media_reports_ten_minutes_of_continuous_audio_with_two_agreeing_detectors() {
    let report = run_with(ScenarioConfig {
        scenario: ScenarioKind::RealMediaFrame,
        seed: 0,
        clock_start: Timestamp::ZERO,
    });
    let audio = report.real_media.expect("real media").audio_continuity;

    assert_eq!(audio.profile, "nominal");
    assert_eq!(audio.duration_seconds, 600);
    assert_eq!(audio.delivery_dropouts, 0);
    assert_eq!(audio.silence_dropouts, 0);
    assert!(audio.concealed_frames > 0, "3 % loss exercises PLC");
    assert!(audio.frames_played >= 600 * 50 - 4);
}

#[test]
fn real_media_runs_are_byte_identical_for_the_same_seed_and_clock() {
    let config = ScenarioConfig {
        scenario: ScenarioKind::RealMediaFrame,
        seed: 7,
        clock_start: Timestamp::from_millis(500),
    };
    assert_eq!(run_with(config), run_with(config));
}
