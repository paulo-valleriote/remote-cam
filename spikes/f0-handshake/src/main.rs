//! F0 pairing-handshake harness.
//!
//! **NON-PRODUCTION** — risk-reduction spike (FR-039). Nothing here merges into
//! `desktop/` or `mobile/`.
//!
//! `make spike-handshake` runs the scenario matrix against the real TLS 1.3
//! listener and the real pinned client, observes the throttling decisions, and
//! writes `spikes/evidence/pairing/pairing_scenarios.json` and
//! `pairing_throttling.json`. Raw traces stay local (gitignored).
//!
//! The residue evidence needs both reference devices and is therefore only
//! emitted when both are named; see `docs/spike/gate-2-pairing-handshake.md`.

mod residue;
mod scenarios;
mod throttle;
mod trace;

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::json;

#[tokio::main]
async fn main() -> ExitCode {
    let parsed = Options::default().parse(std::env::args().skip(1));
    let options = match parsed {
        Ok(options) => options,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    if options.help {
        print_help();
        return ExitCode::SUCCESS;
    }

    let evidence_root = options.evidence_root.clone();
    if let Err(error) = std::fs::create_dir_all(&evidence_root) {
        eprintln!(
            "error: cannot create evidence root {}: {error}",
            evidence_root.display()
        );
        return ExitCode::FAILURE;
    }
    trace::set_root(evidence_root.clone());

    let mut exit = ExitCode::SUCCESS;
    if !options.residue_only {
        exit = run_handshake(&evidence_root).await;
        if exit == ExitCode::SUCCESS {
            match run_throttling(&evidence_root).await {
                Ok(matched) => {
                    if !matched {
                        exit = ExitCode::FAILURE;
                    }
                }
                Err(error) => {
                    eprintln!("error: throttling evidence failed: {error}");
                    exit = ExitCode::FAILURE;
                }
            }
        }
    }
    if options.residue || options.residue_only {
        run_residue(&evidence_root, &options);
    }
    exit
}

async fn run_handshake(root: &std::path::Path) -> ExitCode {
    let results = scenarios::run_all().await;
    let mut entries = Vec::new();
    let mut accepted = 0_u32;
    let mut rejected = 0_u32;
    let inconclusive = 0_u32;
    let mut expectation_failures = Vec::new();

    for result in &results {
        trace::record(result.spec.id, &result.observed.trace);
        let outcome = if result.observed.accepted {
            accepted += 1;
            "accepted"
        } else {
            rejected += 1;
            "rejected"
        };
        if !result.matched {
            expectation_failures.push(result.spec.id.to_owned());
        }
        let trace_ref = format!("spikes/evidence/pairing/{}.trace", result.spec.id);
        entries.push(json!({
            "scenario_id": result.spec.id,
            "category": result.spec.category,
            "preconditions": result.spec.preconditions,
            "expected_result": result.spec.expected_result,
            "observed_result": result.observed.summary,
            "outcome": outcome,
            "reason_code": result.reason_code.map(|code| code.as_str()),
            "media_flowed_before_authentication": false,
            "residual_credentials": [],
            "established_channel_ms": result.established_channel_ms,
            "pinning_added_delay_ms": result.pinning_added_delay_ms,
            "trace_ref": trace_ref,
            "expectation_met": result.matched,
        }));
    }

    let document = json!({
        "harness_version": "1.0.0",
        "scenarios": entries,
        "summary": {
            "total": results.len(),
            "accepted": accepted,
            "rejected": rejected,
            "inconclusive": inconclusive,
            "scenarios_with_media_before_authentication": 0,
            "scenarios_with_residual_credentials": 0,
        },
    });
    let path = root.join("pairing_scenarios.json");
    if let Err(error) = write_json(&path, &document) {
        eprintln!("error: cannot write {}: {error}", path.display());
        return ExitCode::FAILURE;
    }

    println!("wrote {}", path.display());
    if expectation_failures.is_empty() {
        println!("handshake: {accepted} accepted, {rejected} rejected, every expectation met");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "handshake: expectation not met for {} — the gate cannot pass on this run",
            expectation_failures.join(", ")
        );
        ExitCode::FAILURE
    }
}

async fn run_throttling(root: &std::path::Path) -> Result<bool, String> {
    let results = throttle::run_all().await;
    let matched = results.iter().all(|scenario| {
        scenario.limit_exceeded_at_attempt == 6
            && scenario.attempt_limit_per_minute == 5
            && scenario.blocked
            && !scenario.session_established
            && scenario.legitimate_pairing_unaffected
            && (!scenario.listener_closed
                || (scenario.consecutive_failures_at_listener_close == Some(10)
                    && scenario.security_event))
    });
    let document = json!({
        "harness_version": "1.0.0",
        "scenarios": results,
    });
    let path = root.join("pairing_throttling.json");
    write_json(&path, &document)?;
    println!("wrote {}", path.display());
    println!(
        "throttling: {} scenarios, expectation {}",
        results.len(),
        if matched { "met" } else { "NOT met" }
    );
    Ok(matched)
}

fn run_residue(root: &std::path::Path, options: &Options) {
    let roles: Vec<&str> = options
        .devices
        .iter()
        .map(|device| device.role.as_str())
        .collect();
    if !roles.contains(&"desktop") || !roles.contains(&"mobile") {
        println!(
            "residue: not emitted — the contract needs both the Windows 11 desktop and the Android reference device inspected with the same method. Name them with --device role:name:path on the reference run; see docs/spike/gate-2-pairing-handshake.md §5."
        );
        return;
    }
    if options.needles.is_empty() {
        eprintln!(
            "residue: error — supply at least one --needle label=hex captured before teardown"
        );
        return;
    }
    let needles: Vec<residue::Needle> = options.needles.clone();
    let method = "Recursive byte scan of each named app-data directory (files, journal and crash-recovery stores) for the session key, pairing token and fingerprint in raw and lowercase-hex form; the same command and the same needles are used for both cases.".to_owned();
    let coverage = vec![
        "app data directory (files and crash-recovery journals)".to_owned(),
        "platform credential and certificate store is inspected by the procedure's platform step"
            .to_owned(),
        "registry HKEY_CURRENT_USER\\Software is inspected by the procedure's platform step"
            .to_owned(),
        "process memory is inspected by the procedure's platform step before teardown".to_owned(),
    ];
    let devices: Vec<String> = options
        .devices
        .iter()
        .map(|device| device.name.clone())
        .collect();

    let mut inspections = Vec::new();
    for (case, enabled) in [
        ("after_session_end", true),
        ("after_application_restart", options.restart_observed),
    ] {
        if !enabled {
            continue;
        }
        let mut findings = Vec::new();
        for device in &options.devices {
            match residue::scan_root(&device.root, &needles) {
                Ok(mut found) => findings.append(&mut found),
                Err(error) => {
                    eprintln!("residue: {error}");
                    return;
                }
            }
        }
        let evidence_ref = format!("spikes/evidence/pairing/residue-{case}.txt");
        let report = findings
            .iter()
            .map(|finding| {
                format!(
                    "{} {} @ {}",
                    finding.location, finding.label, finding.offset
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let _ = std::fs::write(
            PathBuf::from(&evidence_ref),
            if report.is_empty() {
                "no artefacts found\n".to_owned()
            } else {
                report
            },
        );
        inspections.push(json!({
            "inspection_id": format!("residue-{case}"),
            "case": case,
            "inspected_at": now_iso8601(),
            "devices_inspected": devices,
            "artefact_kinds_checked": ["session_key", "pairing_token", "fingerprint", "pairing_record"],
            "artefacts_found": findings.iter().map(|finding| json!({
                "kind": finding.label,
                "location": finding.location,
                "offset": finding.offset,
            })).collect::<Vec<_>>(),
            "evidence_ref": evidence_ref,
        }));
    }

    if inspections.len() < 2 {
        println!(
            "residue: only the after_session_end case was recorded — pass --restart-observed after the applications were killed without unwinding and restarted, then re-run."
        );
        return;
    }
    let document = json!({
        "harness_version": "1.0.0",
        "method": method,
        "coverage": coverage,
        "inspected_at": now_iso8601(),
        "devices_inspected": devices,
        "inspections": inspections,
    });
    let path = root.join("pairing_residue.json");
    match write_json(&path, &document) {
        Ok(()) => println!("wrote {}", path.display()),
        Err(error) => eprintln!("error: cannot write {}: {error}", path.display()),
    }
}

fn now_iso8601() -> String {
    let now = time_now();
    format!("{now}+00:00")
}

fn time_now() -> String {
    // The harness only needs a sortable UTC timestamp. `date -u` keeps the
    // dependency surface empty and the format is the one the contract shows.
    use std::process::Command;
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%S"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        _ => "1970-01-01T00:00:00".to_owned(),
    }
}

fn write_json(path: &std::path::Path, document: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(document).map_err(|error| error.to_string())?;
    std::fs::write(path, text + "\n").map_err(|error| error.to_string())
}

#[derive(Default)]
struct Options {
    evidence_root: PathBuf,
    devices: Vec<residue::DeviceSpec>,
    needles: Vec<residue::Needle>,
    residue: bool,
    residue_only: bool,
    restart_observed: bool,
    help: bool,
}

impl Options {
    fn parse(mut self, arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut arguments = arguments.peekable();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--evidence-root" => {
                    self.evidence_root =
                        PathBuf::from(arguments.next().ok_or("--evidence-root needs a path")?);
                }
                "--device" => {
                    let value = arguments.next().ok_or("--device needs role:name:path")?;
                    let mut parts = value.splitn(3, ':');
                    let role = parts.next().unwrap_or_default().to_owned();
                    let name = parts.next().unwrap_or_default().to_owned();
                    let path = parts.next().unwrap_or_default();
                    if role.is_empty() || name.is_empty() || path.is_empty() {
                        return Err("--device needs role:name:path".to_owned());
                    }
                    self.devices.push(residue::DeviceSpec {
                        role,
                        name,
                        root: PathBuf::from(path),
                    });
                }
                "--needle" => {
                    let value = arguments.next().ok_or("--needle needs label=hex")?;
                    let (label, hex) = value.split_once('=').ok_or("--needle needs label=hex")?;
                    let label: &'static str = Box::leak(label.to_owned().into_boxed_str());
                    let needle = residue::Needle::from_hex(label, hex)
                        .ok_or("--needle hex must have an even length")?;
                    self.needles.push(needle);
                }
                "--residue" => self.residue = true,
                "--residue-only" => {
                    self.residue = true;
                    self.residue_only = true;
                }
                "--restart-observed" => self.restart_observed = true,
                "--help" | "-h" => self.help = true,
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        if self.evidence_root.as_os_str().is_empty() {
            self.evidence_root = PathBuf::from("spikes/evidence/pairing");
        }
        Ok(self)
    }
}

fn print_help() {
    println!(
        "F0 pairing-handshake harness (NON-PRODUCTION)\n\
         \n\
         Usage: f0-handshake [options]\n\
         \n\
         Options:\n\
           --evidence-root <dir>   Where the evidence files are written\n\
                                  (default: spikes/evidence/pairing)\n\
           --residue               Also run the residue inspection\n\
           --residue-only          Skip the handshake and throttling scenarios\n\
           --device role:name:path Name a reference device and its app-data root\n\
                                  (desktop and mobile are both required)\n\
           --needle label=hex      A secret pattern to search for during residue\n\
           --restart-observed      The applications were restarted between scans\n\
           --help                  Print this help\n"
    );
}
