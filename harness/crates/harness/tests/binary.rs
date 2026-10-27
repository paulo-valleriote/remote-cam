use std::process::{Command, Output};

use serde_json::Value;

#[test]
fn the_binary_emits_one_reproducible_aggregate_report_line() {
    let first = run_under_watchdog(&[]);
    let second = run_under_watchdog(&[]);

    for output in [&first, &second] {
        assert!(
            output.status.success(),
            "the harness exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let first = parse_report(&first);
    let second = parse_report(&second);
    assert_eq!(first["verdict"], "pass");
    assert_eq!(first["report_version"], 7);
    assert_eq!(first["contract_version"], 1);
    assert_eq!(first["summary"]["scenarios"], 10);
    assert_eq!(first["summary"]["passed"], 10);
    assert_eq!(first["scenarios"][0]["scenario"], "pairing-happy-path");
    assert_eq!(first["scenarios"][0]["outcome"], "accepted");
    assert_eq!(first["scenarios"][0]["reason_code"], Value::Null);
    assert_eq!(first["scenarios"][1]["scenario"], "pin-mismatch-aborts");
    assert_eq!(first["scenarios"][1]["outcome"], "rejected");
    assert_eq!(
        first["scenarios"][1]["reason_code"],
        "certificate_pin_mismatch"
    );

    assert_eq!(first["commit"], second["commit"]);
    let mut first_without_environment = first;
    let mut second_without_environment = second;
    first_without_environment
        .as_object_mut()
        .unwrap()
        .remove("environment");
    second_without_environment
        .as_object_mut()
        .unwrap()
        .remove("environment");
    assert_eq!(first_without_environment, second_without_environment);
}

#[test]
fn the_binary_rejects_invalid_arguments_without_printing_a_report() {
    let output = run_under_watchdog(&["--seed", "not-a-number"]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid u64 value for --seed"));
}

#[test]
fn the_binary_rejects_unknown_duplicate_and_missing_arguments() {
    for arguments in [
        &["--unknown", "1"][..],
        &["--seed", "1", "--seed", "2"][..],
        &["--clock-start-millis"][..],
    ] {
        let output = run_under_watchdog(arguments);
        assert!(
            !output.status.success(),
            "arguments accepted: {arguments:?}"
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn the_binary_accepts_custom_reproducibility_inputs() {
    let output = run_under_watchdog(&["--seed", "42", "--clock-start-millis", "100"]);

    assert!(
        output.status.success(),
        "the harness exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse_report(&output);
    assert_eq!(report["seed"], 42);
    assert_eq!(report["clock_start_millis"], 100);
    assert_eq!(report["scenarios"][0]["frames"][0]["at_millis"], 133);
}

fn parse_report(output: &Output) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("the report is UTF-8");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "the report is a single JSON line");
    serde_json::from_str(lines[0]).expect("the report is valid JSON")
}

fn run_under_watchdog(arguments: &[&str]) -> Output {
    let mut command = Command::new("timeout");
    command.args(["60s", env!("CARGO_BIN_EXE_remotecam-harness")]);
    command.args(arguments);
    command
        .output()
        .expect("GNU timeout is available in the Linux harness environment")
}
