use std::process::Command;

use serde_json::Value;

#[test]
fn the_headless_binary_emits_one_json_line_and_exits_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_remotecam-harness"))
        .output()
        .expect("the harness binary is built for the test run");

    assert!(
        output.status.success(),
        "the harness exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("the report is UTF-8");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "the report is a single JSON line");

    let report: Value = serde_json::from_str(lines[0]).expect("the report is valid JSON");
    assert_eq!(report["verdict"], "pass");
    assert_eq!(report["scenario"], "scaffold");
}
