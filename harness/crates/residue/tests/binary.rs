use std::process::{Command, Output};

use serde_json::Value;

fn residue(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_remotecam-residue"))
        .args(args)
        .output()
        .expect("residue binary runs")
}

fn report(output: &Output) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf-8 stdout");
    assert_eq!(stdout.lines().count(), 1, "one JSON line: {stdout}");
    serde_json::from_str(&stdout).expect("the report is JSON")
}

fn inspection<'a>(report: &'a Value, case: &str) -> &'a Value {
    report["inspections"]
        .as_array()
        .expect("inspections")
        .iter()
        .find(|inspection| inspection["case"] == case)
        .unwrap_or_else(|| panic!("inspection {case} missing"))
}

fn scope<'a>(inspection: &'a Value, name: &str) -> &'a Value {
    inspection["scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .find(|scope| scope["scope"] == name)
        .unwrap_or_else(|| panic!("scope {name} missing"))
}

fn findings(scope: &Value) -> Vec<String> {
    scope["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .map(|label| label.as_str().expect("label").to_owned())
        .collect()
}

#[test]
fn a_complete_session_and_a_restart_leave_no_residue() {
    let output = residue(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = report(&output);

    assert_eq!(report["summary"]["passed"], true);
    let cases: Vec<_> = report["inspections"]
        .as_array()
        .expect("inspections")
        .iter()
        .map(|inspection| inspection["case"].as_str().expect("case"))
        .collect();
    assert_eq!(
        cases,
        ["live-control", "session-end", "process-exit", "restart"]
    );

    for case in ["session-end", "process-exit", "restart"] {
        let inspection = inspection(&report, case);
        assert_eq!(inspection["expectation"], "clean");
        assert_eq!(inspection["verdict"], "pass", "{case}");
        for scope in inspection["scopes"].as_array().expect("scopes") {
            assert!(findings(scope).is_empty(), "{case}: {scope}");
        }
    }
}

#[test]
fn the_method_sees_the_secrets_while_the_session_is_live() {
    let report = report(&residue(&[]));

    let live = inspection(&report, "live-control");
    assert_eq!(live["expectation"], "found");
    assert_eq!(live["verdict"], "pass");
    let memory = findings(scope(live, "process_memory"));
    for label in [
        "certificate_der",
        "private_key_limbs",
        "pairing_token",
        "pairing_token_hex",
        "qr_payload",
        "tls_fingerprint",
    ] {
        assert!(
            memory.iter().any(|found| found == label),
            "{label} not seen live: {memory:?}"
        );
    }
}

#[test]
fn every_inspection_declares_what_it_swept() {
    let report = report(&residue(&[]));

    for case in ["live-control", "session-end", "restart"] {
        let inspection = inspection(&report, case);
        for name in [
            "process_memory",
            "state_directory",
            "credential_stores",
            "tls_ticket_state",
        ] {
            assert!(
                scope(inspection, name)["swept"].is_boolean(),
                "{case}/{name}"
            );
        }
        assert_eq!(scope(inspection, "process_memory")["swept"], true, "{case}");
    }
    let exited = inspection(&report, "process-exit");
    assert_eq!(scope(exited, "process_memory")["swept"], false);
    assert_eq!(scope(exited, "state_directory")["swept"], true);
}

#[test]
fn a_deliberate_leak_is_detected_in_memory_and_in_state() {
    let report = report(&residue(&[]));

    assert_eq!(report["canary"]["memory_detected"], true);
    assert_eq!(report["canary"]["state_detected"], true);
}

#[test]
fn the_report_is_deterministic_for_one_seed() {
    let first = residue(&["--seed", "7"]);
    let second = residue(&["--seed", "7"]);

    assert_eq!(first.stdout, second.stdout);
    assert_eq!(report(&first)["seed"], 7);
}

#[test]
fn the_report_is_also_written_to_the_output_file() {
    let path =
        std::env::temp_dir().join(format!("remotecam-residue-out-{}.json", std::process::id()));
    let output = residue(&["--out", path.to_str().expect("utf-8 path")]);

    let written = std::fs::read(&path).expect("evidence file");
    assert_eq!(written, output.stdout);
    std::fs::remove_file(path).expect("cleanup");
}

#[test]
fn an_unknown_argument_exits_with_usage_status() {
    let output = residue(&["--bogus"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn the_report_states_the_platform_and_that_the_device_run_is_open() {
    let report = report(&residue(&[]));

    assert_eq!(report["platform"], "linux");
    assert_eq!(report["device_run"], "open");
}

#[test]
fn the_credential_store_sweep_proves_it_can_find_a_planted_secret() {
    let report = report(&residue(&[]));

    for case in ["live-control", "session-end", "process-exit", "restart"] {
        let stores = scope(inspection(&report, case), "credential_stores");
        assert_eq!(stores["control_detected"], true, "{case}");
        assert_eq!(stores["unreadable_stores"], 0, "{case}");
    }
}

#[test]
fn the_ticket_scope_is_declared_as_a_unit_test_basis_not_a_sweep() {
    let report = report(&residue(&[]));

    for case in ["live-control", "session-end", "process-exit", "restart"] {
        let tickets = scope(inspection(&report, case), "tls_ticket_state");
        assert_eq!(tickets["swept"], false, "{case}");
        assert_eq!(tickets["method"], "unit_test", "{case}");
        assert_eq!(
            tickets["basis"],
            "remotecam-pairing::listener::tests::server_config_issues_no_tickets_and_keeps_no_resumable_session",
            "{case}"
        );
    }
}

#[test]
fn the_memory_sweep_reports_that_every_region_was_readable() {
    let report = report(&residue(&[]));

    for case in ["live-control", "session-end", "restart"] {
        let memory = scope(inspection(&report, case), "process_memory");
        assert_eq!(memory["unreadable_regions"], 0, "{case}");
    }
}
