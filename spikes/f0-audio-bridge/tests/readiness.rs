//! Readiness contract for the virtual audio device (T100, FR-031/FR-032).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! These tests run on Linux CI: the Windows inventory and the WASAPI tone are
//! replaced by fakes, so every branch of the readiness decision is exercised
//! without a sound card.

use std::time::Duration;

use remotecam_audio_bridge::{
    AudioEndpoint, DeviceInventory, DeviceState, Direction, EndpointIdentity, SignatureStatus,
    TEST_TONE_HZ, TestToneSink, assess, degraded_report, vb_cable_expected,
};

struct Inventory(Vec<AudioEndpoint>);

impl DeviceInventory for Inventory {
    fn endpoints(&self) -> Vec<AudioEndpoint> {
        self.0.clone()
    }
}

struct Tone {
    calls: usize,
    result: Result<(), String>,
}

impl Tone {
    fn audible() -> Self {
        Self {
            calls: 0,
            result: Ok(()),
        }
    }

    fn failing() -> Self {
        Self {
            calls: 0,
            result: Err("endpoint is muted".to_owned()),
        }
    }
}

impl TestToneSink for Tone {
    fn play_test_tone(&mut self, frequency_hz: f64, duration: Duration) -> Result<(), String> {
        assert_eq!(frequency_hz, TEST_TONE_HZ);
        assert!(duration >= Duration::from_millis(500));
        self.calls += 1;
        self.result.clone()
    }
}

fn endpoint(name: &str, direction: Direction, signature: SignatureStatus) -> AudioEndpoint {
    AudioEndpoint {
        identity: EndpointIdentity {
            friendly_name: name.to_owned(),
            manufacturer: "VB-Audio".to_owned(),
            driver_version: "45".to_owned(),
        },
        direction,
        signature,
    }
}

fn pinned(signature: SignatureStatus) -> Inventory {
    Inventory(vec![
        endpoint(
            "CABLE Input (VB-Audio Virtual Cable)",
            Direction::Render,
            signature,
        ),
        endpoint(
            "CABLE Output (VB-Audio Virtual Cable)",
            Direction::Capture,
            signature,
        ),
    ])
}

#[test]
fn missing_device_is_actionable_and_video_stays_usable() {
    let mut tone = Tone::audible();
    let readiness = assess(&Inventory(Vec::new()), &vb_cable_expected(), &mut tone);
    let degraded = degraded_report(&readiness);

    assert_eq!(readiness.state, DeviceState::Missing);
    assert!(readiness.action.to_lowercase().contains("install"));
    assert!(readiness.video_only_usable);
    assert!(degraded.video_only_usable);
    assert!(degraded.next_step.to_lowercase().contains("install"));
    assert_eq!(tone.calls, 0, "a missing device must not play a tone");
}

#[test]
fn unsigned_device_is_rejected_before_the_tone() {
    let mut tone = Tone::audible();
    let readiness = assess(
        &pinned(SignatureStatus::Unsigned),
        &vb_cable_expected(),
        &mut tone,
    );
    let degraded = degraded_report(&readiness);

    assert_eq!(readiness.state, DeviceState::Unsigned);
    assert!(readiness.action.contains("Get-AuthenticodeSignature"));
    assert!(readiness.action.contains("signtool verify /pa /v"));
    assert!(degraded.video_only_usable);
    assert_eq!(tone.calls, 0);
}

#[test]
fn unexpected_identity_is_rejected() {
    let inventory = Inventory(vec![
        endpoint(
            "Line 1 (Virtual Audio Cable)",
            Direction::Render,
            SignatureStatus::Valid,
        ),
        endpoint(
            "Line 1 (Virtual Audio Cable)",
            Direction::Capture,
            SignatureStatus::Valid,
        ),
    ]);
    let mut tone = Tone::audible();
    let readiness = assess(&inventory, &vb_cable_expected(), &mut tone);

    assert_eq!(readiness.state, DeviceState::UnexpectedIdentity);
    assert!(readiness.action.contains("not the pinned"));
    assert!(readiness.video_only_usable);
    assert_eq!(tone.calls, 0);
}

#[test]
fn a_tone_is_required_before_audio_ready() {
    let mut tone = Tone::audible();
    let readiness = assess(
        &pinned(SignatureStatus::Valid),
        &vb_cable_expected(),
        &mut tone,
    );

    assert_eq!(readiness.state, DeviceState::AudioReady);
    assert_eq!(
        tone.calls, 1,
        "readiness must play the 1 kHz tone exactly once"
    );
    assert!(readiness.tone_confirmed);
}

#[test]
fn a_failed_tone_is_not_audio_ready() {
    let mut tone = Tone::failing();
    let readiness = assess(
        &pinned(SignatureStatus::Valid),
        &vb_cable_expected(),
        &mut tone,
    );
    let degraded = degraded_report(&readiness);

    assert_eq!(readiness.state, DeviceState::ToneNotConfirmed);
    assert!(!readiness.tone_confirmed);
    assert!(readiness.action.contains("1 kHz"));
    assert!(degraded.video_only_usable);
}

#[test]
fn every_failure_state_keeps_video_only_usable() {
    let cases = [
        (Inventory(Vec::new()), DeviceState::Missing),
        (pinned(SignatureStatus::Unsigned), DeviceState::Unsigned),
        (pinned(SignatureStatus::Invalid), DeviceState::Unsigned),
        (pinned(SignatureStatus::Unknown), DeviceState::Unsigned),
    ];

    for (inventory, expected) in cases {
        let mut tone = Tone::audible();
        let readiness = assess(&inventory, &vb_cable_expected(), &mut tone);
        assert_eq!(readiness.state, expected);
        assert!(readiness.video_only_usable);
        assert!(degraded_report(&readiness).video_only_usable);
        assert!(!degraded_report(&readiness).next_step.is_empty());
    }
}
