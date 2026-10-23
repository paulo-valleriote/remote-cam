//! `make spike-synthetic-source` entry point.
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike. Renders the source manifest
//! and one frame, and optionally proves the QR marker survives H.264
//! encode/decode before a measurement starts.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use f0_pipeline::synthetic_source::{
    SyntheticSourceConfig, render_frame, verify_marker_roundtrip, write_pgm,
};

fn main() -> ExitCode {
    let mut config = SyntheticSourceConfig::default();
    let mut evidence_root = PathBuf::from("spikes/evidence/pipeline");
    let mut python = "python3".to_owned();
    let mut verify = false;

    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let (key, inline) = match argument.split_once('=') {
            Some((key, value)) => (key.to_owned(), Some(value.to_owned())),
            None => (argument, None),
        };
        let mut value = || inline.clone().or_else(|| arguments.next());
        match key.as_str() {
            "--source-id" => {
                if let Some(value) = value() {
                    config.source_id = value;
                }
            }
            "--duration-s" => {
                if let Some(value) = value().and_then(|value| value.parse().ok()) {
                    config.duration_s = value;
                }
            }
            "--evidence-root" => {
                if let Some(value) = value() {
                    evidence_root = PathBuf::from(value);
                }
            }
            "--python" => {
                if let Some(value) = value() {
                    python = value;
                }
            }
            "--verify" => verify = true,
            "--help" | "-h" => {
                println!(
                    "synthetic-source [--source-id ID] [--duration-s N] [--evidence-root PATH] [--verify]"
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("error: unknown option {other}");
                return ExitCode::from(2);
            }
        }
    }

    if let Err(error) = run(&config, &evidence_root, &python, verify) {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run(
    config: &SyntheticSourceConfig,
    evidence_root: &Path,
    python: &str,
    verify: bool,
) -> Result<(), String> {
    std::fs::create_dir_all(evidence_root).map_err(|error| error.to_string())?;
    let command = format!(
        "make spike-synthetic-source SOURCE_ID={} DURATION_S={}",
        config.source_id, config.duration_s
    );
    let metadata = serde_json::to_string_pretty(&config.metadata(&command))
        .map_err(|error| error.to_string())?;
    std::fs::write(
        evidence_root.join("synthetic_source.json"),
        format!("{metadata}\n"),
    )
    .map_err(|error| error.to_string())?;

    let rendered = render_frame(config, 1_700_000_000_000, 0).map_err(|error| error.to_string())?;
    write_pgm(
        &evidence_root.join("synthetic_marker.pgm"),
        config.width as usize,
        config.height as usize,
        rendered.frame.luma(),
        rendered.frame.stride,
    )?;
    if verify {
        let payload = verify_marker_roundtrip(config, python)?;
        let document = serde_json::json!({
            "payload": payload,
            "survives_encode_decode": true,
            "codec": "H.264 (OpenH264)",
            "decoder": "zxing-cpp",
        });
        std::fs::write(
            evidence_root.join("marker_roundtrip.json"),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&document).map_err(|error| error.to_string())?
            ),
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}
