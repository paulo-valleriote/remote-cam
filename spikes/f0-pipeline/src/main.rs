//! F0 desktop receive-pipeline harness.
//!
//! **NON-PRODUCTION** — risk-reduction spike (FR-039). Nothing here merges into
//! `desktop/` or `mobile/`.
//!
//! The run matrix is documented in `docs/spike/gate-3-desktop-receive-pipeline.md`.
//! Every subcommand that produces gate evidence refuses to run outside the
//! reference environment: the latency and resource gates require the verified
//! Windows 11 hardware-decode path (PC-1), and a Linux CI result is never cited
//! for a verdict.

use f0_pipeline::{drift_run, latency_capture, relay, stall, synthetic_source};

#[cfg(windows)]
use f0_pipeline::resource;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use remotecam_media_core::drift::ResamplerModel;

use relay::{ImpairmentConfig, RelayCore, relay_log_jsonl};
use synthetic_source::SyntheticSourceConfig;

const DEFAULT_EVIDENCE_ROOT: &str = "spikes/evidence/pipeline";

#[derive(Debug, Clone)]
struct Options {
    command: String,
    run_id: String,
    duration_s: u64,
    seed: u64,
    loss_pct: f64,
    added_latency_ms: f64,
    jitter_ms: f64,
    stall_s: f64,
    samples: Option<PathBuf>,
    listen: String,
    forward: Option<String>,
    evidence_root: PathBuf,
    source_id: String,
    python: String,
    verify: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            command: "all".to_owned(),
            run_id: "lat-01".to_owned(),
            duration_s: 600,
            seed: relay::SEED,
            loss_pct: relay::PROFILE_LOSS_PCT,
            added_latency_ms: relay::PROFILE_ADDED_LATENCY_MS,
            jitter_ms: relay::PROFILE_JITTER_MS,
            stall_s: 5.0,
            samples: None,
            listen: "0.0.0.0:0".to_owned(),
            forward: None,
            evidence_root: PathBuf::from(DEFAULT_EVIDENCE_ROOT),
            source_id: "src-01".to_owned(),
            python: "python3".to_owned(),
            verify: false,
        }
    }
}

impl Options {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut arguments = arguments.peekable();
        if let Some(first) = arguments.peek()
            && !first.starts_with("--")
        {
            options.command = arguments.next().unwrap_or_default();
        }
        while let Some(argument) = arguments.next() {
            let (key, inline) = match argument.split_once('=') {
                Some((key, value)) => (key.to_owned(), Some(value.to_owned())),
                None => (argument, None),
            };
            let mut value = || {
                inline
                    .clone()
                    .or_else(|| arguments.next())
                    .ok_or_else(|| format!("{key} needs a value"))
            };
            match key.as_str() {
                "--run" | "--run-id" => options.run_id = value()?,
                "--duration-s" | "--duration" => {
                    options.duration_s = value()?.parse().map_err(|_| "bad duration".to_owned())?;
                }
                "--seed" => options.seed = value()?.parse().map_err(|_| "bad seed".to_owned())?,
                "--loss-pct" => {
                    options.loss_pct = value()?.parse().map_err(|_| "bad loss".to_owned())?
                }
                "--added-latency-ms" => {
                    options.added_latency_ms = value()?
                        .parse()
                        .map_err(|_| "bad added latency".to_owned())?;
                }
                "--jitter-ms" => {
                    options.jitter_ms = value()?.parse().map_err(|_| "bad jitter".to_owned())?
                }
                "--stall-s" => {
                    options.stall_s = value()?.parse().map_err(|_| "bad stall".to_owned())?
                }
                "--samples" => options.samples = Some(PathBuf::from(value()?)),
                "--listen" => options.listen = value()?,
                "--forward" => options.forward = Some(value()?),
                "--evidence-root" => options.evidence_root = PathBuf::from(value()?),
                "--source-id" => options.source_id = value()?,
                "--python" => options.python = value()?,
                "--verify" => options.verify = true,
                "--help" | "-h" => options.command = "help".to_owned(),
                other => return Err(format!("unknown option {other}")),
            }
        }
        Ok(options)
    }

    fn impairment(&self) -> ImpairmentConfig {
        ImpairmentConfig {
            loss_pct: self.loss_pct,
            loss_distribution: relay::LossDistribution::Uniform,
            added_latency_ms: self.added_latency_ms,
            jitter_ms: self.jitter_ms,
            seed: self.seed,
            stall_at_ms: None,
            stall_ms: 0.0,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    if options.command == "help" {
        print_help();
        return ExitCode::SUCCESS;
    }
    if let Err(error) = std::fs::create_dir_all(&options.evidence_root) {
        eprintln!("error: cannot create evidence root: {error}");
        return ExitCode::FAILURE;
    }

    let result = match options.command.as_str() {
        "all" | "run" => run_matrix(&options).await,
        "latency" => run_latency(&options),
        "loss" => run_loss(&options).await,
        "resources" => run_resources(&options),
        "drift" => run_drift(&options),
        "stall" => run_stall(&options),
        "synthetic-source" | "source" => run_source(&options),
        "verify-marker" => run_verify_marker(&options),
        other => Err(format!("unknown command {other}; run with --help")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The full matrix is gated on the reference environment.
async fn run_matrix(options: &Options) -> Result<(), String> {
    let manifest = options.evidence_root.join("run_matrix.json");
    if let Err(reason) = reference_environment() {
        write_json(
            &manifest,
            &serde_json::json!({
                "status": "blocked_by_environment",
                "generated_at": now_iso8601(),
                "reason": reason,
                "required": "Windows 11 x64 with a verified hardware H.264 decoder, FFmpeg >= 8.1 and a consumer application",
                "scenarios": planned_scenarios(options),
                "note": "This host cannot produce gate evidence. The procedure is docs/spike/gate-3-desktop-receive-pipeline.md; no latency, resource, drift or stall verdict may cite a run made here.",
            }),
        )?;
        return Err(format!("pipeline gate blocked: {reason}"));
    }

    write_json(
        &manifest,
        &serde_json::json!({
            "status": "ready",
            "generated_at": now_iso8601(),
            "scenarios": planned_scenarios(options),
            "run_order": [
                "make spike-synthetic-source SOURCE_ID=src-01 DURATION_S=600",
                "make spike-latency RUN=1; make spike-latency RUN=2; make spike-latency RUN=3",
                "make spike-loss ...",
                "make spike-resources DURATION_S=1800",
                "make spike-drift DURATION_S=1800 MARKER_PERIOD_S=2",
                "make spike-stall STALL_S=5",
            ],
            "note": "Each scenario is a separate command so an operator can inspect the capture before the next step; the procedure is docs/spike/gate-3-desktop-receive-pipeline.md.",
        }),
    )?;
    Ok(())
}

fn planned_scenarios(_options: &Options) -> serde_json::Value {
    serde_json::json!([
        { "scenario": "latency", "runs": 3, "duration_s_each": 600, "criterion": "raw p95 <= 150 ms" },
        { "scenario": "loss", "profile": relay::PROFILE_NAME, "duration_s": 600, "criterion": "zero dropouts >= 50 ms" },
        { "scenario": "loss-control", "loss_pct": 0, "duration_s": 600 },
        { "scenario": "resources", "duration_s": 1800, "criterion": "cpu p95 <= 15 %, private max <= 400 MB" },
        { "scenario": "drift", "duration_s": 1800, "marker_period_s": 2, "criterion": "<= 20 ms/h with a certifying interval" },
        { "scenario": "stall", "stall_s": 5, "criterion": "recovery <= 2 s, bounded buffer" },
    ])
}

fn run_latency(options: &Options) -> Result<(), String> {
    reference_environment()?;
    let plan = latency_capture::LatencyCapturePlan::new(
        &options.run_id,
        "consumer",
        &format!("spikes/evidence/pipeline/{}-capture.mp4", options.run_id),
        options.duration_s,
    );
    write_json(
        &options
            .evidence_root
            .join(format!("{}-capture-plan.json", options.run_id)),
        &plan.document(),
    )?;
    plan.run_calibration_capture()?;
    plan.run_capture()?;
    Err(format!(
        "capture written; run the analyser: python spikes/tools/analysis/latency.py summary --samples <decoded.csv> --floor <floor.json> --run-id {}",
        options.run_id
    ))
}

async fn run_loss(options: &Options) -> Result<(), String> {
    let forward = options
        .forward
        .clone()
        .ok_or_else(|| "--forward is required for the loss scenario".to_owned())?;
    let log_path = options.evidence_root.join("relay_log.jsonl");
    let core = RelayCore::new(options.impairment());
    let profile = core
        .config()
        .profile_document("spikes/evidence/pipeline/relay_log.jsonl");
    run_relay(&options.listen, &forward, options, log_path.clone()).await?;
    write_json(
        &options.evidence_root.join("impairment_profile.json"),
        &profile,
    )?;
    let text = std::fs::read_to_string(&log_path).map_err(|error| error.to_string())?;
    let decisions = relay::parse_relay_log(&text);
    let dropped = decisions
        .iter()
        .filter(|decision| decision.action == relay::Action::Drop)
        .count();
    write_json(
        &options.evidence_root.join("relay_summary.json"),
        &serde_json::json!({
            "decisions": decisions.len(),
            "dropped": dropped,
            "realised_loss_pct": if decisions.is_empty() { 0.0 } else { 100.0 * dropped as f64 / decisions.len() as f64 },
            "profile": profile["profile_name"],
        }),
    )
}

async fn run_relay(
    listen: &str,
    forward: &str,
    options: &Options,
    log_path: PathBuf,
) -> Result<(), String> {
    use tokio::net::UdpSocket;

    let socket = UdpSocket::bind(listen)
        .await
        .map_err(|error| format!("cannot bind {listen}: {error}"))?;
    let target: std::net::SocketAddr = forward
        .parse()
        .map_err(|_| format!("{forward} is not a socket address"))?;
    let outbound = std::sync::Arc::new(
        UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|error| error.to_string())?,
    );

    let mut core = RelayCore::new(options.impairment());
    let mut decisions = Vec::new();
    let mut held: Vec<Vec<u8>> = Vec::new();
    let started = Instant::now();
    let mut buffer = vec![0_u8; 65_535];

    while started.elapsed() < Duration::from_secs(options.duration_s) {
        let receive =
            tokio::time::timeout(Duration::from_millis(200), socket.recv_from(&mut buffer)).await;
        if let Ok(Ok((length, _source))) = receive {
            let packet = buffer[..length].to_vec();
            let now_ms = started.elapsed().as_secs_f64() * 1000.0;
            let Some(decision) = core.decide(now_ms, &packet) else {
                let _ = outbound.send_to(&packet, target).await;
                continue;
            };
            match decision.action {
                relay::Action::Drop => {}
                relay::Action::Pass => {
                    let _ = outbound.send_to(&packet, target).await;
                }
                relay::Action::Delay => {
                    let delay = decision.delay_ms.max(0.0);
                    let outbound = std::sync::Arc::clone(&outbound);
                    let packet = packet.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs_f64(delay / 1000.0)).await;
                        let _ = outbound.send_to(&packet, target).await;
                    });
                }
                relay::Action::Hold => held.push(packet),
                relay::Action::StallRelease => {
                    for packet in held.drain(..) {
                        let _ = outbound.send_to(&packet, target).await;
                    }
                    let _ = outbound.send_to(&packet, target).await;
                }
            }
            decisions.push(decision);
        }
    }

    std::fs::write(&log_path, relay_log_jsonl(&decisions)).map_err(|error| error.to_string())
}

#[allow(clippy::needless_return)] // the two cfg blocks each end the function on their target
fn run_resources(options: &Options) -> Result<(), String> {
    reference_environment()?;
    #[cfg(not(windows))]
    {
        let _ = options;
        return Err(
            "resource sampling uses GetProcessTimes/GetProcessMemoryInfo and requires Windows"
                .to_owned(),
        );
    }
    #[cfg(windows)]
    {
        let mut samples = Vec::new();
        let started = Instant::now();
        let mut previous = None;
        let mut index = 0_u64;
        while started.elapsed() < Duration::from_secs(options.duration_s) {
            if let Some(sample) =
                resource::windows_sampler::sample(index, &options.run_id, started, &mut previous, 1)
            {
                samples.push(sample);
            }
            index += 1;
            std::thread::sleep(Duration::from_millis(1000));
        }
        let logical_cores = std::thread::available_parallelism()
            .map(|value| value.get() as u32)
            .unwrap_or(1);
        let summary = resource::ResourceSummary::from_samples(
            &options.run_id,
            options.duration_s as f64,
            logical_cores,
            &samples,
        );
        resource::write_resource_csv(
            &options.evidence_root.join("resource_samples.csv"),
            &samples,
        )?;
        write_json(
            &options.evidence_root.join("resource_summary.json"),
            &summary.document(serde_json::json!({ "tool": "typeperf", "status": "see procedure" })),
        )?;
        Ok(())
    }
}

fn run_drift(options: &Options) -> Result<(), String> {
    let path = options
        .samples
        .clone()
        .ok_or_else(|| "--samples <drift.csv> is required".to_owned())?;
    let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let observations = parse_drift_csv(&text)?;
    let resampler = ResamplerModel::rtp_paced(1.0);
    drift_run::write_drift_csv(
        &options.evidence_root.join("drift_samples.csv"),
        &observations,
    )?;
    write_json(
        &options.evidence_root.join("drift_summary.json"),
        &drift_run::summary_document(&options.run_id, &observations, &resampler),
    )
}

fn run_stall(options: &Options) -> Result<(), String> {
    let path = options
        .samples
        .clone()
        .ok_or_else(|| "--samples <stall.csv> is required".to_owned())?;
    let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let samples = parse_stall_csv(&text)?;
    let last = samples
        .last()
        .map(|sample| sample.elapsed_ms)
        .unwrap_or(0.0);
    let stall_start = (last - options.stall_s * 1000.0).max(0.0);
    let result = stall::analyse(&samples, stall_start, last, 2_000.0)?;
    write_json(
        &options.evidence_root.join("stall.json"),
        &serde_json::json!(result),
    )
}

fn run_source(options: &Options) -> Result<(), String> {
    let config = SyntheticSourceConfig {
        source_id: options.source_id.clone(),
        duration_s: options.duration_s,
        ..SyntheticSourceConfig::default()
    };
    let command = format!(
        "make spike-synthetic-source SOURCE_ID={} DURATION_S={}",
        config.source_id, config.duration_s
    );
    write_json(
        &options.evidence_root.join("synthetic_source.json"),
        &config.metadata(&command),
    )?;
    let rendered = synthetic_source::render_frame(&config, 1_700_000_000_000, 0)
        .map_err(|error| error.to_string())?;
    let pgm = options.evidence_root.join("synthetic_marker.pgm");
    synthetic_source::write_pgm(
        &pgm,
        config.width as usize,
        config.height as usize,
        rendered.frame.luma(),
        rendered.frame.stride,
    )?;
    if options.verify {
        let payload = synthetic_source::verify_marker_roundtrip(&config, &options.python)?;
        write_json(
            &options.evidence_root.join("marker_roundtrip.json"),
            &serde_json::json!({
                "payload": payload,
                "survives_encode_decode": true,
                "codec": "H.264 (OpenH264)",
                "decoder": "zxing-cpp",
            }),
        )?;
    }
    Ok(())
}

fn run_verify_marker(options: &Options) -> Result<(), String> {
    let config = SyntheticSourceConfig::default();
    let payload = synthetic_source::verify_marker_roundtrip(&config, &options.python)?;
    println!("{payload}");
    Ok(())
}

fn parse_drift_csv(text: &str) -> Result<Vec<drift_run::DriftObservation>, String> {
    let mut lines = text.lines();
    let _header = lines.next().ok_or_else(|| "empty drift CSV".to_owned())?;
    let mut observations = Vec::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 6 {
            return Err(format!("malformed drift row: {line}"));
        }
        observations.push(drift_run::DriftObservation {
            elapsed_s: fields[2].parse().map_err(|_| "bad elapsed_s".to_owned())?,
            source_skew_ms: fields[3]
                .parse()
                .map_err(|_| "bad source_skew_ms".to_owned())?,
            audio_playout_delay_ms: fields[4]
                .parse()
                .map_err(|_| "bad audio_playout_delay_ms".to_owned())?,
            video_playout_delay_ms: fields[5]
                .parse()
                .map_err(|_| "bad video_playout_delay_ms".to_owned())?,
        });
    }
    Ok(observations)
}

fn parse_stall_csv(text: &str) -> Result<Vec<stall::StallSample>, String> {
    let mut lines = text.lines();
    let _header = lines.next().ok_or_else(|| "empty stall CSV".to_owned())?;
    let mut samples = Vec::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 5 {
            return Err(format!("malformed stall row: {line}"));
        }
        samples.push(stall::StallSample {
            elapsed_ms: fields[0].parse().map_err(|_| "bad elapsed_ms".to_owned())?,
            buffer_depth: fields[1]
                .parse()
                .map_err(|_| "bad buffer_depth".to_owned())?,
            latency_ms: fields[2].parse().map_err(|_| "bad latency_ms".to_owned())?,
            discarded_frames: fields[3]
                .parse()
                .map_err(|_| "bad discarded_frames".to_owned())?,
            media_received: fields[4]
                .parse()
                .map_err(|_| "bad media_received".to_owned())?,
        });
    }
    Ok(samples)
}

fn reference_environment() -> Result<(), String> {
    if !cfg!(windows) {
        return Err(format!(
            "this host is {}, not the Windows 11 reference machine; the pipeline gate cannot run here",
            std::env::consts::OS
        ));
    }
    #[cfg(windows)]
    {
        use remotecam_media_core::adapters::mf_decoder::verify_h264_hardware_decode;
        let verification = verify_h264_hardware_decode()
            .map_err(|error| format!("hardware-decode verification failed: {error}"))?;
        if !verification.is_admissible() {
            return Err(format!(
                "PC-1 failed: hardware_accelerated={}, output_format={}",
                verification.hardware_accelerated, verification.output_format_supported
            ));
        }
    }
    Ok(())
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    std::fs::write(path, format!("{text}\n")).map_err(|error| error.to_string())
}

fn now_iso8601() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    iso8601_utc(seconds)
}

/// Convert Unix seconds to an ISO 8601 UTC timestamp with an explicit offset.
///
/// The harness deliberately has no date-time dependency; the civil-date
/// algorithm is Howard Hinnant's `civil_from_days`.
fn iso8601_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let remainder = seconds % 86_400;
    let (hour, minute, second) = (remainder / 3_600, (remainder % 3_600) / 60, remainder % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}+00:00")
}

fn print_help() {
    println!(
        "f0-pipeline — NON-PRODUCTION F0 desktop receive-pipeline harness\n\n\
         Usage: f0-pipeline <command> [options]\n\n\
         Commands:\n\
           all                 Run the full matrix (requires the Windows reference machine)\n\
           latency             Plan and capture one latency run\n\
           loss                Run the seeded RTP-aware impairment relay\n\
           resources           Sample CPU and private working set\n\
           drift               Summarise a drift CSV\n\
           stall               Analyse a stall trace CSV\n\
           synthetic-source    Render the timestamped source manifest and one frame\n\
           verify-marker       Prove the QR marker survives encode/decode\n\n\
         Options: --run ID --duration-s N --seed N --loss-pct N --added-latency-ms N\n\
                  --jitter-ms N --stall-s N --samples PATH --listen ADDR --forward ADDR\n\
                  --evidence-root PATH --source-id ID --python PATH --verify"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timestamp_is_iso8601_with_an_offset() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(iso8601_utc(1_700_000_000), "2023-11-14T22:13:20+00:00");
    }

    #[test]
    fn the_pinned_profile_is_recognised_and_a_deviation_is_labelled() {
        let pinned = ImpairmentConfig::pinned(None, 0.0);
        assert!(pinned.is_pinned_profile());
        assert!(
            pinned
                .profile_document("log")
                .get("deviation_label")
                .is_some()
        );

        let mut deviation = pinned;
        deviation.loss_pct = 8.0;
        assert!(!deviation.is_pinned_profile());
    }
}
