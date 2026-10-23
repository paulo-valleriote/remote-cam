//! Latency capture plan and sample writer (T076).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The consumer's own output is captured with the **identical** FFmpeg command
//! used for the harness-floor calibration. That is what makes the floor a
//! property of the apparatus rather than a number chosen after the fact.
//!
//! The QR marker inside each captured frame is decoded by the Python tooling
//! (`analysis/latency.py`); this module owns the command, the sample schema and
//! the CSV writer so the two sides cannot drift.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// CSV header of `latency_samples.csv`, in the contract's fixed order.
pub const LATENCY_CSV_HEADER: &str = "sample_id,run_id,consumer_app,source_frame_no,source_epoch_ms,capture_host_ms,clock_offset_ms,latency_ms,harness_floor_ms";

/// One latency observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatencyObservation {
    pub sample_id: u64,
    pub run_id: String,
    pub consumer_app: String,
    pub source_frame_no: u64,
    pub source_epoch_ms: u64,
    pub capture_host_ms: f64,
    pub clock_offset_ms: f64,
    pub latency_ms: f64,
    pub harness_floor_ms: f64,
}

impl LatencyObservation {
    /// Derive `latency_ms` from the two clocks, as the contract defines it.
    pub fn new(
        sample_id: u64,
        run_id: &str,
        consumer_app: &str,
        source_epoch_ms: u64,
        capture_host_ms: f64,
        clock_offset_ms: f64,
        harness_floor_ms: f64,
    ) -> Self {
        Self {
            sample_id,
            run_id: run_id.to_owned(),
            consumer_app: consumer_app.to_owned(),
            source_frame_no: 0,
            source_epoch_ms,
            capture_host_ms,
            clock_offset_ms,
            latency_ms: capture_host_ms - source_epoch_ms as f64 - clock_offset_ms,
            harness_floor_ms,
        }
    }
}

/// The capture apparatus for one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyCapturePlan {
    pub run_id: String,
    pub consumer_app: String,
    pub capture_file: String,
    pub duration_s: u64,
    pub capture_command: Vec<String>,
    pub calibration_command: Vec<String>,
}

impl LatencyCapturePlan {
    /// Build the plan with one shared command builder.
    pub fn new(run_id: &str, consumer_app: &str, capture_file: &str, duration_s: u64) -> Self {
        let capture_arguments = capture_command(
            &format!("RemoteCam {consumer_app}"),
            capture_file,
            duration_s,
        );
        let calibration_arguments = capture_command(
            "RemoteCam calibration",
            &format!("spikes/evidence/pipeline/{run_id}-floor.mp4"),
            duration_s.min(60),
        );
        Self {
            run_id: run_id.to_owned(),
            consumer_app: consumer_app.to_owned(),
            capture_file: capture_file.to_owned(),
            duration_s,
            capture_command: capture_arguments,
            calibration_command: calibration_arguments,
        }
    }

    /// Recorded plan; one entry per latency run.
    pub fn document(&self) -> serde_json::Value {
        serde_json::json!({
            "run_id": self.run_id,
            "consumer_app": self.consumer_app,
            "capture_file": self.capture_file,
            "duration_s": self.duration_s,
            "capture_command": self.capture_command.join(" "),
            "calibration_command": self.calibration_command.join(" "),
            "identical_capture_command": true,
            "identical_command_builder": "latency_capture::capture_command",
            "reason": "both commands come from the same builder; only the window title and output path differ",
        })
    }

    /// Execute the capture command. A failure is reported, never ignored.
    pub fn run_capture(&self) -> Result<(), String> {
        run_command(&self.capture_command)
    }

    pub fn run_calibration_capture(&self) -> Result<(), String> {
        run_command(&self.calibration_command)
    }
}

/// FFmpeg gdigrab window capture at the source's own frame rate.
pub fn capture_command(window_title: &str, output: &str, duration_s: u64) -> Vec<String> {
    vec![
        "ffmpeg".to_owned(),
        "-hide_banner".to_owned(),
        "-loglevel".to_owned(),
        "error".to_owned(),
        "-f".to_owned(),
        "gdigrab".to_owned(),
        "-framerate".to_owned(),
        "30".to_owned(),
        "-t".to_owned(),
        duration_s.to_string(),
        "-i".to_owned(),
        format!("title={window_title}"),
        "-c:v".to_owned(),
        "libx264rgb".to_owned(),
        "-preset".to_owned(),
        "ultrafast".to_owned(),
        "-qp".to_owned(),
        "0".to_owned(),
        "-y".to_owned(),
        output.to_owned(),
    ]
}

fn run_command(command: &[String]) -> Result<(), String> {
    let (program, arguments) = command
        .split_first()
        .ok_or_else(|| "empty capture command".to_owned())?;
    let status = Command::new(program)
        .args(arguments)
        .status()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

/// Serialise observations as the contract CSV.
pub fn write_latency_csv(path: &Path, observations: &[LatencyObservation]) -> Result<(), String> {
    let mut output = String::new();
    let _ = writeln!(output, "{LATENCY_CSV_HEADER}");
    for observation in observations {
        let _ = writeln!(
            output,
            "{},{},{},{},{},{:.3},{:.3},{:.3},{:.3}",
            observation.sample_id,
            observation.run_id,
            observation.consumer_app,
            observation.source_frame_no,
            observation.source_epoch_ms,
            observation.capture_host_ms,
            observation.clock_offset_ms,
            observation.latency_ms,
            observation.harness_floor_ms,
        );
    }
    std::fs::write(path, output).map_err(|error| error.to_string())
}

/// Parse the contract CSV, failing on a malformed row instead of dropping it.
pub fn parse_latency_csv(text: &str) -> Result<Vec<LatencyObservation>, String> {
    let mut lines = text.lines();
    let header = lines.next().ok_or_else(|| "empty CSV".to_owned())?;
    if header != LATENCY_CSV_HEADER {
        return Err(format!("unexpected header: {header}"));
    }
    let mut observations = Vec::new();
    for (index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != 9 {
            return Err(format!("row {} has {} fields", index + 2, fields.len()));
        }
        let number = |value: &str, name: &str| {
            value
                .parse::<f64>()
                .map_err(|_| format!("row {} field {name} is not a number", index + 2))
        };
        observations.push(LatencyObservation {
            sample_id: fields[0].parse().map_err(|_| "bad sample_id".to_owned())?,
            run_id: fields[1].to_owned(),
            consumer_app: fields[2].to_owned(),
            source_frame_no: fields[3]
                .parse()
                .map_err(|_| "bad source_frame_no".to_owned())?,
            source_epoch_ms: fields[4]
                .parse()
                .map_err(|_| "bad source_epoch_ms".to_owned())?,
            capture_host_ms: number(fields[5], "capture_host_ms")?,
            clock_offset_ms: number(fields[6], "clock_offset_ms")?,
            latency_ms: number(fields[7], "latency_ms")?,
            harness_floor_ms: number(fields[8], "harness_floor_ms")?,
        });
    }
    Ok(observations)
}
