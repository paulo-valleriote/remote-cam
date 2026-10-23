//! Tests for the audio run harness boundary (T104).

use f0_virtual_audio::{
    CAPTURE_DEVICE_NAME, DEVICE_PIN, DEVICE_VERSION_PIN, HarnessError, RunPlan, WavInfo,
    parse_wav_header, read_capture,
};

fn wav_bytes(sample_rate: u32, channels: u16, bits: u16, frames: u32) -> Vec<u8> {
    let block_align = channels * bits.div_ceil(8);
    let data_len = frames * u32::from(block_align);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&bits.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    bytes.resize(bytes.len() + data_len as usize, 0);
    bytes
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

fn ten_minute_info() -> WavInfo {
    WavInfo {
        sample_rate_hz: 48_000,
        channels: 1,
        bits_per_sample: 16,
        data_bytes: 48_000 * 2 * 610,
        data_offset: 44,
        duration_s: 610.0,
    }
}

#[test]
fn a_48k_mono_16_bit_header_is_read_exactly() {
    let bytes = wav_bytes(48_000, 1, 16, 48_000);

    let info = parse_wav_header(&bytes).expect("the header must parse");

    assert_eq!(info.sample_rate_hz, 48_000);
    assert_eq!(info.channels, 1);
    assert_eq!(info.bits_per_sample, 16);
    assert_eq!(info.data_bytes, 96_000);
    assert!((info.duration_s - 1.0).abs() < 1e-9);
}

#[test]
fn a_truncated_data_chunk_is_refused() {
    let mut bytes = wav_bytes(48_000, 1, 16, 48_000);
    bytes.truncate(bytes.len() - 10);

    let error = parse_wav_header(&bytes).expect_err("truncation must be refused");

    assert!(matches!(error, HarnessError::Wav(message) if message.contains("claims")));
}

#[test]
fn a_non_pcm_header_is_refused() {
    let mut bytes = wav_bytes(48_000, 1, 16, 100);
    bytes[20..22].copy_from_slice(&3u16.to_le_bytes());

    let error = parse_wav_header(&bytes).expect_err("float PCM must be refused");

    assert!(matches!(error, HarnessError::Wav(message) if message.contains("PCM is required")));
}

#[test]
fn a_file_that_is_not_riff_wave_is_refused() {
    let error = parse_wav_header(b"not a wav file").expect_err("garbage must be refused");

    assert!(matches!(error, HarnessError::Wav(_)));
}

#[test]
fn a_wrong_sample_rate_is_not_admissible() {
    let mut info = ten_minute_info();
    info.sample_rate_hz = 44_100;

    let document = plan().capture_index(&info).expect("the index is built");

    assert_eq!(document["admissible"], false);
    assert!(
        document["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str().unwrap().contains("48000"))
    );
}

#[test]
fn a_stereo_capture_is_not_admissible() {
    let mut info = ten_minute_info();
    info.channels = 2;

    let document = plan().capture_index(&info).expect("the index is built");

    assert_eq!(document["admissible"], false);
    assert!(
        document["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str().unwrap().contains("mono"))
    );
}

#[test]
fn a_24_bit_capture_is_not_admissible() {
    let mut info = ten_minute_info();
    info.bits_per_sample = 24;

    let document = plan().capture_index(&info).expect("the index is built");

    assert_eq!(document["admissible"], false);
}

#[test]
fn the_plan_refuses_a_device_other_than_the_pin() {
    let mut plan = plan();
    plan.device_pin = "VoiceMeeter".to_owned();

    let error = plan
        .validate()
        .expect_err("a non-pinned device must be refused");

    assert!(matches!(error, HarnessError::Plan(message) if message.contains("pinned")));
}

#[test]
fn the_plan_refuses_a_different_driver_pack() {
    let mut plan = plan();
    plan.device_version = "driver pack 43".to_owned();

    let error = plan
        .validate()
        .expect_err("a different driver pack must be refused");

    assert!(matches!(error, HarnessError::Plan(message) if message.contains("driver pack 45")));
}

#[test]
fn an_unnamed_observer_makes_the_index_invalid() {
    let mut plan = plan();
    plan.intelligibility_observer = "  ".to_owned();

    let error = plan
        .validate()
        .expect_err("an unnamed observer must be refused");

    assert!(matches!(error, HarnessError::Plan(message) if message.contains("observer")));
}

#[test]
fn a_missing_phrase_script_makes_the_index_invalid() {
    let mut plan = plan();
    plan.phrase_script_ref.clear();

    let error = plan
        .validate()
        .expect_err("a missing script must be refused");

    assert!(matches!(error, HarnessError::Plan(message) if message.contains("phrase_script_ref")));
}

#[test]
fn a_clean_ten_minute_capture_is_gate_ready() {
    let document = plan()
        .capture_index(&ten_minute_info())
        .expect("the index is built");

    assert_eq!(document["admissible"], true);
    assert_eq!(document["gate_ready"], true);
    assert_eq!(document["duration_s"], 610.0);
    for field in [
        "capture_id",
        "run_id",
        "capture_file",
        "capture_format",
        "sample_rate_hz",
        "channels",
        "duration_s",
        "device_name",
        "device_pin",
        "device_version",
        "signature_verified",
        "consumer_app",
        "consumer_renegotiations",
        "dropout_analysis_ref",
        "intelligibility_ref",
        "intelligibility_observer",
        "phrase_script_ref",
        "analyser_version",
    ] {
        assert!(!document[field].is_null(), "missing field {field}");
    }
}

#[test]
fn exactly_ten_minutes_is_enough() {
    let mut info = ten_minute_info();
    info.duration_s = 600.0;

    let document = plan().capture_index(&info).expect("the index is built");

    assert_eq!(document["admissible"], true);
}

#[test]
fn a_one_second_short_capture_is_refused() {
    let mut info = ten_minute_info();
    info.duration_s = 599.0;

    let document = plan().capture_index(&info).expect("the index is built");

    assert_eq!(document["admissible"], false);
}

#[test]
fn read_capture_reports_a_missing_file() {
    let error = read_capture(std::path::Path::new("/nonexistent/capture.wav"))
        .expect_err("a missing file must fail");

    assert!(matches!(error, HarnessError::Io(_)));
}
