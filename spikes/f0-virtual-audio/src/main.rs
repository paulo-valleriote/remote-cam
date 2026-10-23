//! CLI for the virtual audio run harness (T104).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike tooling.
//!
//! Two modes:
//!
//! ```text
//! f0-virtual-audio --render phrase_audio.wav --signature-verified
//! f0-virtual-audio --capture consumer_capture.wav --run-id aud-01 \
//!     --consumer-app obs --observer "Named Observer" --signature-verified \
//!     --out spikes/evidence/audio/aud-01/audio_run_capture.json
//! ```
//!
//! The first renders the scripted audio into `CABLE Input`; the operator records
//! `CABLE Output` in the consumer application. The second validates the closed
//! recording and writes the run index. The harness never invents a capture or a
//! signature result.

use std::path::PathBuf;
use std::process::ExitCode;

use f0_virtual_audio::{
    CAPTURE_DEVICE_NAME, DEVICE_PIN, DEVICE_VERSION_PIN, HarnessError, RunPlan, read_capture,
    render_capture,
};

struct Options {
    render: Option<PathBuf>,
    capture: Option<PathBuf>,
    out: Option<PathBuf>,
    run_id: Option<String>,
    capture_id: Option<String>,
    consumer_app: Option<String>,
    observer: Option<String>,
    script_ref: String,
    dropout_ref: Option<String>,
    intelligibility_ref: Option<String>,
    renegotiations: u32,
    signature_verified: bool,
    loops: u32,
}

impl Options {
    fn parse() -> Result<Self, HarnessError> {
        let mut options = Self {
            render: None,
            capture: None,
            out: None,
            run_id: None,
            capture_id: None,
            consumer_app: None,
            observer: None,
            script_ref: "spikes/fixtures/phrases-v1.csv".to_owned(),
            dropout_ref: None,
            intelligibility_ref: None,
            renegotiations: 0,
            signature_verified: false,
            loops: 1,
        };

        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--render" => options.render = Some(next_path(&mut arguments, "--render")?),
                "--capture" => options.capture = Some(next_path(&mut arguments, "--capture")?),
                "--out" => options.out = Some(next_path(&mut arguments, "--out")?),
                "--run-id" => {
                    options.run_id = Some(next_value(&mut arguments, "--run-id")?);
                }
                "--capture-id" => {
                    options.capture_id = Some(next_value(&mut arguments, "--capture-id")?);
                }
                "--consumer-app" => {
                    options.consumer_app = Some(next_value(&mut arguments, "--consumer-app")?);
                }
                "--observer" => {
                    options.observer = Some(next_value(&mut arguments, "--observer")?);
                }
                "--script-ref" => options.script_ref = next_value(&mut arguments, "--script-ref")?,
                "--dropout-ref" => {
                    options.dropout_ref = Some(next_value(&mut arguments, "--dropout-ref")?);
                }
                "--intelligibility-ref" => {
                    options.intelligibility_ref =
                        Some(next_value(&mut arguments, "--intelligibility-ref")?);
                }
                "--renegotiations" => {
                    options.renegotiations = next_value(&mut arguments, "--renegotiations")?
                        .parse()
                        .map_err(|_| {
                            HarnessError::Plan("--renegotiations must be an integer".into())
                        })?;
                }
                "--loops" => {
                    options.loops = next_value(&mut arguments, "--loops")?
                        .parse()
                        .map_err(|_| HarnessError::Plan("--loops must be an integer".into()))?;
                }
                "--signature-verified" => options.signature_verified = true,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    return Err(HarnessError::Plan(format!("unknown argument {other:?}")));
                }
            }
        }

        Ok(options)
    }
}

fn next_value(
    arguments: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, HarnessError> {
    arguments
        .next()
        .ok_or_else(|| HarnessError::Plan(format!("{flag} needs a value")))
}

fn next_path(
    arguments: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<PathBuf, HarnessError> {
    Ok(PathBuf::from(next_value(arguments, flag)?))
}

fn print_help() {
    println!(
        "f0-virtual-audio - NON-PRODUCTION F0 virtual audio harness\n\
         \n\
         Render mode (Windows 11 + VB-CABLE):\n\
           --render <wav> [--loops N] --signature-verified\n\
         \n\
         Index mode (any host):\n\
           --capture <wav> --run-id <id> --consumer-app <name> --observer <name>\n\
           --signature-verified --out <audio_run_capture.json>\n\
         \n\
         Optional: --capture-id, --script-ref, --dropout-ref, --intelligibility-ref,\n\
                   --renegotiations <n>"
    );
}

fn main() -> ExitCode {
    match run() {
        Ok(admissible) => {
            if admissible {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, HarnessError> {
    let options = Options::parse()?;

    if let Some(source) = &options.render {
        if !options.signature_verified {
            return Err(HarnessError::Plan(
                "refusing to render before the signature probe: pass --signature-verified only \
                 after signature_probe.ps1 reports Valid on the reference runner"
                    .to_owned(),
            ));
        }
        render_capture(source, options.loops)?;
        println!(
            "rendered {} into {DEVICE_PIN} ({DEVICE_VERSION_PIN}); start the consumer recording \
             before the next render and record CABLE Output.",
            source.display()
        );
        return Ok(true);
    }

    let capture = options
        .capture
        .as_ref()
        .ok_or_else(|| HarnessError::Plan("--capture or --render is required".to_owned()))?;
    let out = options
        .out
        .as_ref()
        .ok_or_else(|| HarnessError::Plan("--out is required in index mode".to_owned()))?;
    let run_id = options
        .run_id
        .clone()
        .ok_or_else(|| HarnessError::Plan("--run-id is required in index mode".to_owned()))?;
    let consumer_app = options
        .consumer_app
        .clone()
        .ok_or_else(|| HarnessError::Plan("--consumer-app is required in index mode".to_owned()))?;
    let observer = options
        .observer
        .clone()
        .ok_or_else(|| HarnessError::Plan("--observer is required in index mode".to_owned()))?;

    let (_, info) = read_capture(capture)?;
    let plan = RunPlan {
        run_id: run_id.clone(),
        capture_id: options.capture_id.unwrap_or_else(|| run_id.clone()),
        capture_file: capture.display().to_string(),
        device_name: CAPTURE_DEVICE_NAME.to_owned(),
        device_pin: DEVICE_PIN.to_owned(),
        device_version: DEVICE_VERSION_PIN.to_owned(),
        signature_verified: options.signature_verified,
        consumer_app,
        consumer_renegotiations: options.renegotiations,
        dropout_analysis_ref: options
            .dropout_ref
            .unwrap_or_else(|| format!("spikes/evidence/audio/{run_id}/dropouts.json")),
        intelligibility_ref: options
            .intelligibility_ref
            .unwrap_or_else(|| format!("spikes/evidence/audio/{run_id}/intelligibility.json")),
        intelligibility_observer: observer,
        phrase_script_ref: options.script_ref,
    };

    let document = plan.capture_index(&info)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, serde_json::to_string_pretty(&document).unwrap() + "\n")?;

    let admissible = document["admissible"].as_bool().unwrap_or(false);
    println!(
        "wrote {} ({}, {:.1} s)",
        out.display(),
        if admissible {
            "admissible"
        } else {
            "NOT admissible"
        },
        info.duration_s
    );
    Ok(admissible)
}
