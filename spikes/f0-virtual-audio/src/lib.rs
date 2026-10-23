//! Virtual audio run harness for the F0 gate (T104).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike tooling (FR-039).
//!
//! The harness does not record the consumer application itself: that is a
//! deliberate human step in the gate procedure, because the criterion is what a
//! *consumer* hears from `CABLE Output`, not what the producer thinks it sent.
//! The harness supplies the other half:
//!
//! * it renders the scripted phrase audio into `CABLE Input` through the pinned
//!   VB-CABLE endpoint (Windows only);
//! * it validates the consumer's recorded WAV before any analysis: 48 kHz mono
//!   16-bit PCM, at least 10 minutes, no truncated data chunk;
//! * it emits `audio_run_capture.json` with the pinned device identity, the
//!   signature result, the consumer renegotiation count and every reference the
//!   dropout and intelligibility analyses need.
//!
//! A WAV that is short, multi-channel or not 16-bit PCM is refused instead of
//! silently analysed: a gate that accepts the wrong capture is worse than one
//! that records no capture.

use std::fmt;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

pub const NON_PRODUCTION: &str = "NON-PRODUCTION";
pub const ANALYSER_VERSION: &str = "1.0.0";
pub const DEVICE_PIN: &str = "VB-CABLE";
pub const DEVICE_VERSION_PIN: &str = "driver pack 45";
pub const CAPTURE_DEVICE_NAME: &str = "CABLE Output (VB-Audio Virtual Cable)";
pub const REQUIRED_SAMPLE_RATE_HZ: u32 = 48_000;
pub const REQUIRED_CHANNELS: u16 = 1;
pub const REQUIRED_BITS_PER_SAMPLE: u16 = 16;
pub const MINIMUM_DURATION_S: f64 = 600.0;
pub const PROFILE_LOSS_PCT: f64 = 3.0;
pub const PROFILE_ADDED_LATENCY_MS: f64 = 30.0;
pub const PROFILE_JITTER_MS: f64 = 10.0;

/// Harness failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessError {
    Io(String),
    Wav(String),
    Plan(String),
    Unsupported(String),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "{message}"),
            Self::Wav(message) => write!(formatter, "invalid capture: {message}"),
            Self::Plan(message) => write!(formatter, "invalid run plan: {message}"),
            Self::Unsupported(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for HarnessError {}

impl From<std::io::Error> for HarnessError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// The parts of a WAV header the gate cares about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WavInfo {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub data_bytes: u32,
    pub data_offset: usize,
    pub duration_s: f64,
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, HarnessError> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| HarnessError::Wav("the header ended inside a 16-bit field".to_owned()))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, HarnessError> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| HarnessError::Wav("the header ended inside a 32-bit field".to_owned()))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn fourcc(bytes: &[u8], offset: usize) -> Result<[u8; 4], HarnessError> {
    let slice = bytes.get(offset..offset + 4).ok_or_else(|| {
        HarnessError::Wav("the header ended inside a chunk identifier".to_owned())
    })?;
    Ok([slice[0], slice[1], slice[2], slice[3]])
}

/// Parse a RIFF/WAVE header and locate the PCM payload.
///
/// Only plain PCM and `WAVE_FORMAT_EXTENSIBLE` with a PCM sub-format are
/// accepted. A streaming header with an unknown data size is rejected: the
/// consumer recording must be a closed file before it can gate.
pub fn parse_wav_header(bytes: &[u8]) -> Result<WavInfo, HarnessError> {
    if bytes.len() < 12 {
        return Err(HarnessError::Wav(
            "the file is shorter than a RIFF header".to_owned(),
        ));
    }
    if fourcc(bytes, 0)? != *b"RIFF" || fourcc(bytes, 8)? != *b"WAVE" {
        return Err(HarnessError::Wav(
            "the file is not a RIFF/WAVE file".to_owned(),
        ));
    }

    let mut offset = 12_usize;
    let mut format: Option<(u16, u16, u32)> = None;
    let mut data: Option<(u32, usize)> = None;

    while offset + 8 <= bytes.len() {
        let id = fourcc(bytes, offset)?;
        let size = read_u32(bytes, offset + 4)? as usize;
        let body = offset + 8;
        let end = body
            .checked_add(size)
            .ok_or_else(|| HarnessError::Wav("a chunk size overflowed".to_owned()))?;
        if end > bytes.len() {
            return Err(HarnessError::Wav(format!(
                "chunk {:?} claims {size} bytes but the file ends at {}",
                String::from_utf8_lossy(&id),
                bytes.len()
            )));
        }

        if id == *b"fmt " {
            if size < 16 {
                return Err(HarnessError::Wav(
                    "the fmt chunk is shorter than 16 bytes".to_owned(),
                ));
            }
            let tag = read_u16(bytes, body)?;
            let channels = read_u16(bytes, body + 2)?;
            let sample_rate = read_u32(bytes, body + 4)?;
            let bits = read_u16(bytes, body + 14)?;
            let is_pcm = tag == 1
                || (tag == 0xFFFE
                    && size >= 40
                    && read_u16(bytes, body + 24)? == 1
                    && read_u16(bytes, body + 26)? == 0);
            if !is_pcm {
                return Err(HarnessError::Wav(format!(
                    "unsupported WAV format tag 0x{tag:04x}; PCM is required"
                )));
            }
            format = Some((channels, bits, sample_rate));
        } else if id == *b"data" {
            if size == u32::MAX as usize {
                return Err(HarnessError::Wav(
                    "the data chunk has an unknown size; close the recording first".to_owned(),
                ));
            }
            data = Some((size as u32, body));
        }

        // RIFF chunks are word aligned; a pad byte follows an odd-sized chunk.
        offset = end + (size & 1);
    }

    let (channels, bits, sample_rate) =
        format.ok_or_else(|| HarnessError::Wav("the file has no fmt chunk".to_owned()))?;
    let (data_bytes, data_offset) =
        data.ok_or_else(|| HarnessError::Wav("the file has no data chunk".to_owned()))?;
    if channels == 0 || sample_rate == 0 || bits == 0 {
        return Err(HarnessError::Wav(
            "the fmt chunk carries a zero channel count, sample rate or sample width".to_owned(),
        ));
    }

    let bytes_per_frame = u64::from(channels) * u64::from(bits.div_ceil(8));
    if bytes_per_frame == 0 {
        return Err(HarnessError::Wav("the frame size is zero".to_owned()));
    }
    let frames = u64::from(data_bytes) / bytes_per_frame;
    let duration_s = frames as f64 / f64::from(sample_rate);

    Ok(WavInfo {
        sample_rate_hz: sample_rate,
        channels,
        bits_per_sample: bits,
        data_bytes,
        data_offset,
        duration_s,
    })
}

/// Decode a validated 16-bit mono PCM capture into `f32` samples.
pub fn decode_pcm16_mono(bytes: &[u8], info: &WavInfo) -> Result<Vec<f32>, HarnessError> {
    if info.bits_per_sample != 16 || info.channels != 1 {
        return Err(HarnessError::Wav(
            "decoding requires 16-bit mono PCM".to_owned(),
        ));
    }
    let payload = bytes
        .get(info.data_offset..info.data_offset + info.data_bytes as usize)
        .ok_or_else(|| HarnessError::Wav("the data chunk is truncated".to_owned()))?;
    Ok(payload
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32_768.0)
        .collect())
}

/// The recorded run plan, assembled from the gate procedure.
#[derive(Debug, Clone, PartialEq)]
pub struct RunPlan {
    pub run_id: String,
    pub capture_id: String,
    pub capture_file: String,
    pub device_name: String,
    pub device_pin: String,
    pub device_version: String,
    pub signature_verified: bool,
    pub consumer_app: String,
    pub consumer_renegotiations: u32,
    pub dropout_analysis_ref: String,
    pub intelligibility_ref: String,
    pub intelligibility_observer: String,
    pub phrase_script_ref: String,
}

impl RunPlan {
    /// Reject a plan that cannot describe an admissible run.
    pub fn validate(&self) -> Result<(), HarnessError> {
        let required = [
            ("run_id", self.run_id.as_str()),
            ("capture_id", self.capture_id.as_str()),
            ("capture_file", self.capture_file.as_str()),
            ("device_name", self.device_name.as_str()),
            ("consumer_app", self.consumer_app.as_str()),
            ("dropout_analysis_ref", self.dropout_analysis_ref.as_str()),
            ("intelligibility_ref", self.intelligibility_ref.as_str()),
            (
                "intelligibility_observer",
                self.intelligibility_observer.as_str(),
            ),
            ("phrase_script_ref", self.phrase_script_ref.as_str()),
        ];
        for (name, value) in required {
            if value.trim().is_empty() {
                return Err(HarnessError::Plan(format!("{name} must not be empty")));
            }
        }
        if self.device_pin != DEVICE_PIN {
            return Err(HarnessError::Plan(format!(
                "the measurement device is pinned to {DEVICE_PIN}, not {:?}",
                self.device_pin
            )));
        }
        if self.device_version != DEVICE_VERSION_PIN {
            return Err(HarnessError::Plan(format!(
                "the device version is pinned to {DEVICE_VERSION_PIN}, not {:?}",
                self.device_version
            )));
        }
        Ok(())
    }

    /// Emit the contract `audio_run_capture.json` document.
    pub fn capture_index(&self, info: &WavInfo) -> Result<Value, HarnessError> {
        self.validate()?;

        let mut errors: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();

        if info.sample_rate_hz != REQUIRED_SAMPLE_RATE_HZ {
            errors.push(format!(
                "sample rate is {} Hz; the gate requires {REQUIRED_SAMPLE_RATE_HZ} Hz",
                info.sample_rate_hz
            ));
        }
        if info.channels != REQUIRED_CHANNELS {
            errors.push(format!(
                "capture has {} channels; the gate requires mono",
                info.channels
            ));
        }
        if info.bits_per_sample != REQUIRED_BITS_PER_SAMPLE {
            errors.push(format!(
                "capture is {}-bit; the gate requires 16-bit PCM",
                info.bits_per_sample
            ));
        }
        if info.duration_s < MINIMUM_DURATION_S {
            errors.push(format!(
                "capture is {:.3} s; the gate requires at least {MINIMUM_DURATION_S:.0} s",
                info.duration_s
            ));
        }
        if !self.signature_verified {
            errors.push(
                "the driver signature was not verified; run signature_probe.ps1 on the \
                 Secure-Boot Windows 11 runner before recording"
                    .to_owned(),
            );
        }
        if self.consumer_renegotiations > 0 {
            warnings.push(format!(
                "the consumer renegotiated {} time(s); the gate requires zero",
                self.consumer_renegotiations
            ));
        }

        let admissible = errors.is_empty();
        let gate_ready = admissible && warnings.is_empty();

        Ok(json!({
            "capture_id": self.capture_id,
            "run_id": self.run_id,
            "capture_file": self.capture_file,
            "capture_format": "wav",
            "sample_rate_hz": info.sample_rate_hz,
            "channels": info.channels,
            "bits_per_sample": info.bits_per_sample,
            "duration_s": round3(info.duration_s),
            "device_name": self.device_name,
            "device_pin": self.device_pin,
            "device_version": self.device_version,
            "signature_verified": self.signature_verified,
            "consumer_app": self.consumer_app,
            "consumer_renegotiations": self.consumer_renegotiations,
            "dropout_analysis_ref": self.dropout_analysis_ref,
            "intelligibility_ref": self.intelligibility_ref,
            "intelligibility_observer": self.intelligibility_observer,
            "phrase_script_ref": self.phrase_script_ref,
            "analyser_version": ANALYSER_VERSION,
            "profile": {
                "loss_pct": PROFILE_LOSS_PCT,
                "added_latency_ms": PROFILE_ADDED_LATENCY_MS,
                "jitter_ms": PROFILE_JITTER_MS,
            },
            "admissible": admissible,
            "gate_ready": gate_ready,
            "findings": errors.iter().chain(warnings.iter()).cloned().collect::<Vec<_>>(),
            "non_production": NON_PRODUCTION,
        }))
    }
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Read and parse a capture file from disk.
pub fn read_capture(path: &Path) -> Result<(Vec<u8>, WavInfo), HarnessError> {
    let bytes = fs::read(path)?;
    let info = parse_wav_header(&bytes)?;
    Ok((bytes, info))
}

/// Render a WAV into the pinned `CABLE Input` endpoint under the 1 kHz tone.
///
/// Only available on Windows: the endpoint is opened through WASAPI. The caller
/// must already have run the signature probe; `signature_verified` is checked
/// by the CLI before this function is reached.
#[cfg(windows)]
pub fn render_capture(path: &Path, loops: u32) -> Result<(), HarnessError> {
    use std::time::Duration;

    use remotecam_audio_bridge::{CableRenderer, vb_cable_expected};

    let (bytes, info) = read_capture(path)?;
    if info.sample_rate_hz != REQUIRED_SAMPLE_RATE_HZ
        || info.channels != REQUIRED_CHANNELS
        || info.bits_per_sample != REQUIRED_BITS_PER_SAMPLE
    {
        return Err(HarnessError::Wav(format!(
            "the render source must be 48 kHz mono 16-bit PCM, not {} Hz / {} ch / {}-bit",
            info.sample_rate_hz, info.channels, info.bits_per_sample
        )));
    }
    let samples = decode_pcm16_mono(&bytes, &info)?;

    let expected = vb_cable_expected();
    let mut renderer = CableRenderer::open(&expected)
        .map_err(|error| HarnessError::Unsupported(error.to_string()))?;
    renderer
        .play_test_tone(1_000.0, Duration::from_millis(1_000))
        .map_err(|error| HarnessError::Unsupported(error.to_string()))?;

    let mut timestamp = 0_u64;
    for _ in 0..loops.max(1) {
        for chunk in samples.chunks(480) {
            renderer
                .write_mono(chunk, timestamp)
                .map_err(|error| HarnessError::Unsupported(error.to_string()))?;
            timestamp += chunk.len() as u64;
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}

/// Rendering is Windows-only; the Linux host refuses rather than fabricating it.
#[cfg(not(windows))]
pub fn render_capture(_path: &Path, _loops: u32) -> Result<(), HarnessError> {
    Err(HarnessError::Unsupported(
        "live rendering requires Windows 11 with the pinned VB-CABLE driver; run \
         `make spike-audio` on the reference runner"
            .to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_capture_is_refused() {
        let info = WavInfo {
            sample_rate_hz: 48_000,
            channels: 1,
            bits_per_sample: 16,
            data_bytes: 96_000,
            data_offset: 44,
            duration_s: 1.0,
        };
        let plan = plan();
        let document = plan.capture_index(&info).expect("the index must be built");

        assert_eq!(document["admissible"], false);
        assert!(
            document["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value.as_str().unwrap().contains("at least 600"))
        );
    }

    fn plan() -> RunPlan {
        RunPlan {
            run_id: "aud-01".to_owned(),
            capture_id: "aud-01".to_owned(),
            capture_file: "spikes/evidence/audio/aud-01/consumer_capture.wav".to_owned(),
            device_name: CAPTURE_DEVICE_NAME.to_owned(),
            device_pin: DEVICE_PIN.to_owned(),
            device_version: DEVICE_VERSION_PIN.to_owned(),
            signature_verified: true,
            consumer_app: "obs".to_owned(),
            consumer_renegotiations: 0,
            dropout_analysis_ref: "spikes/evidence/audio/aud-01/dropouts.json".to_owned(),
            intelligibility_ref: "spikes/evidence/audio/aud-01/intelligibility.json".to_owned(),
            intelligibility_observer: "Observer A".to_owned(),
            phrase_script_ref: "spikes/fixtures/phrases-v1.csv".to_owned(),
        }
    }

    #[test]
    fn a_clean_plan_is_admissible() {
        let info = WavInfo {
            sample_rate_hz: 48_000,
            channels: 1,
            bits_per_sample: 16,
            data_bytes: 48_000 * 2 * 610,
            data_offset: 44,
            duration_s: 610.0,
        };
        let document = plan()
            .capture_index(&info)
            .expect("the index must be built");

        assert_eq!(document["admissible"], true);
        assert_eq!(document["gate_ready"], true);
        assert_eq!(document["duration_s"], 610.0);
        assert_eq!(document["device_pin"], "VB-CABLE");
        assert_eq!(document["device_version"], "driver pack 45");
    }

    #[test]
    fn an_unverified_signature_blocks_admissibility() {
        let info = WavInfo {
            sample_rate_hz: 48_000,
            channels: 1,
            bits_per_sample: 16,
            data_bytes: 48_000 * 2 * 610,
            data_offset: 44,
            duration_s: 610.0,
        };
        let mut plan = plan();
        plan.signature_verified = false;
        let document = plan.capture_index(&info).expect("the index must be built");

        assert_eq!(document["admissible"], false);
        assert_eq!(document["signature_verified"], false);
    }

    #[test]
    fn a_renegotiation_is_recorded_and_never_hidden() {
        let info = WavInfo {
            sample_rate_hz: 48_000,
            channels: 1,
            bits_per_sample: 16,
            data_bytes: 48_000 * 2 * 610,
            data_offset: 44,
            duration_s: 610.0,
        };
        let mut plan = plan();
        plan.consumer_renegotiations = 2;
        let document = plan.capture_index(&info).expect("the index must be built");

        assert_eq!(document["admissible"], true);
        assert_eq!(document["gate_ready"], false);
        assert_eq!(document["consumer_renegotiations"], 2);
    }
}
