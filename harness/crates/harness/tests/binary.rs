use std::process::{Command, Output};

use serde_json::Value;

#[test]
fn the_headless_binary_emits_a_reproducible_exchange_report_under_a_watchdog() {
    let first = run_under_watchdog();
    let second = run_under_watchdog();

    for output in [&first, &second] {
        assert!(
            output.status.success(),
            "the harness exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert_eq!(first.stdout, second.stdout);
    let stdout = String::from_utf8(first.stdout).expect("the report is UTF-8");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "the report is a single JSON line");

    let report: Value = serde_json::from_str(lines[0]).expect("the report is valid JSON");
    assert_eq!(report["verdict"], "pass");
    assert_eq!(report["scenario"], "pairing");
    assert_eq!(report["observed_state"], "connected");
    assert_eq!(
        report["message_sequence"],
        serde_json::json!(["pair", "paired", "offer", "answer"])
    );
}

fn run_under_watchdog() -> Output {
    Command::new("timeout")
        .args(["10s", env!("CARGO_BIN_EXE_remotecam-harness")])
        .output()
        .expect("GNU timeout is available in the Linux harness environment")
}
