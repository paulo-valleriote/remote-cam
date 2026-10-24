//! Device readiness for the pinned third-party virtual audio device.
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! Readiness is a pure decision over a device inventory and a test-tone sink.
//! The Windows adapter supplies the inventory through a PowerShell probe and the
//! tone through WASAPI; the logic below is exercised on Linux with fakes.
//!
//! A session is audio-ready only when **all** of the following hold:
//!
//! 1. both the render endpoint (`CABLE Input`) and the capture endpoint
//!    (`CABLE Output`) exist;
//! 2. both carry the **exact** pinned product identity (friendly name and
//!    manufacturer) — a look-alike device is rejected, not accepted by prefix;
//! 3. the driver signature status is `Valid` (FR-031);
//! 4. an audible 1 kHz test tone was rendered successfully (FR-031).
//!
//! Failure never makes the session unusable: video-only operation is always
//! reported as usable (FR-032).

use std::time::Duration;

/// The product pinned for the technical measurement (research D-12).
pub const DEVICE_PIN: &str = "VB-CABLE";
/// Driver package pinned by the recorded procedure.
pub const DEVICE_VERSION_PIN: &str = "driver pack 45";
/// Frequency of the mandatory readiness tone.
pub const TEST_TONE_HZ: f64 = 1_000.0;
/// Duration of the mandatory readiness tone.
pub const TEST_TONE_MS: u64 = 1_000;

/// Direction of one audio endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Render,
    Capture,
}

/// The identity of an audio endpoint, as Windows reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointIdentity {
    /// Friendly name, e.g. `CABLE Input (VB-Audio Virtual Cable)`.
    pub friendly_name: String,
    /// Manufacturer string, e.g. `VB-Audio`.
    pub manufacturer: String,
    /// Driver version string reported by Windows.
    pub driver_version: String,
}

/// Signature status of the driver behind an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Windows accepted the embedded or catalog signature.
    Valid,
    /// The driver is not signed.
    Unsigned,
    /// A signature is present but Windows rejected it.
    Invalid,
    /// The probe could not determine the status. Treated as not ready: an
    /// unverified signature never passes FR-031.
    Unknown,
}

/// One endpoint returned by the platform inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioEndpoint {
    pub identity: EndpointIdentity,
    pub direction: Direction,
    pub signature: SignatureStatus,
}

/// Platform inventory, injected so the decision is testable off Windows.
pub trait DeviceInventory {
    fn endpoints(&self) -> Vec<AudioEndpoint>;
}

/// The device a session expects, with the exact names the measurement pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedDevice {
    pub product: String,
    pub version: String,
    pub render_friendly_name: String,
    pub capture_friendly_name: String,
    pub manufacturer: String,
}

/// The pinned VB-CABLE device.
pub fn vb_cable_expected() -> ExpectedDevice {
    ExpectedDevice {
        product: DEVICE_PIN.to_owned(),
        version: DEVICE_VERSION_PIN.to_owned(),
        render_friendly_name: "CABLE Input (VB-Audio Virtual Cable)".to_owned(),
        capture_friendly_name: "CABLE Output (VB-Audio Virtual Cable)".to_owned(),
        manufacturer: "VB-Audio".to_owned(),
    }
}

/// Sink for the mandatory readiness tone.
pub trait TestToneSink {
    /// Render an audible tone and return only once it was actually played.
    fn play_test_tone(&mut self, frequency_hz: f64, duration: Duration) -> Result<(), String>;
}

/// Outcome of the readiness check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    /// Both endpoints matched, the signature is valid and the tone was played.
    AudioReady,
    /// No endpoint with the pinned identity exists.
    Missing,
    /// The device exists but its signature is not accepted.
    Unsigned,
    /// An endpoint exists but is not the pinned product (look-alike or renamed).
    UnexpectedIdentity,
    /// The device is valid but the 1 kHz tone could not be confirmed.
    ToneNotConfirmed,
}

impl DeviceState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AudioReady => "audio_ready",
            Self::Missing => "missing",
            Self::Unsigned => "unsigned",
            Self::UnexpectedIdentity => "unexpected_identity",
            Self::ToneNotConfirmed => "tone_not_confirmed",
        }
    }

    /// Whether the session may present audio.
    pub fn is_audio_ready(&self) -> bool {
        matches!(self, Self::AudioReady)
    }
}

/// The full readiness result, including the user-facing next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioReadiness {
    pub state: DeviceState,
    /// Actionable sentence naming what the user must do next (FR-032).
    pub action: String,
    /// Always `true`: a failed audio device never blocks the video session.
    pub video_only_usable: bool,
    /// The matched endpoint, when one was found.
    pub identity: Option<EndpointIdentity>,
    /// Whether the 1 kHz tone was audibly rendered.
    pub tone_confirmed: bool,
}

impl AudioReadiness {
    pub fn is_audio_ready(&self) -> bool {
        self.state.is_audio_ready()
    }

    fn failed(state: DeviceState, action: impl Into<String>) -> Self {
        Self {
            state,
            action: action.into(),
            video_only_usable: true,
            identity: None,
            tone_confirmed: false,
        }
    }
}

/// Assess readiness.
///
/// The tone sink is called **at most once** and only after the device identity
/// and signature checks pass. A session is never reported audio-ready without
/// a successful tone.
pub fn assess(
    inventory: &dyn DeviceInventory,
    expected: &ExpectedDevice,
    tone: &mut dyn TestToneSink,
) -> AudioReadiness {
    let endpoints = inventory.endpoints();

    let render = endpoints
        .iter()
        .find(|endpoint| endpoint.direction == Direction::Render);
    let capture = endpoints
        .iter()
        .find(|endpoint| endpoint.direction == Direction::Capture);

    let (Some(render), Some(capture)) = (render, capture) else {
        return AudioReadiness::failed(
            DeviceState::Missing,
            format!(
                "Install the pinned {} {} from the official vendor download, then re-run the readiness check. Until then the session stays video-only.",
                expected.product, expected.version
            ),
        );
    };

    if !matches(
        &render.identity,
        &expected.render_friendly_name,
        &expected.manufacturer,
    ) || !matches(
        &capture.identity,
        &expected.capture_friendly_name,
        &expected.manufacturer,
    ) {
        return AudioReadiness::failed(
            DeviceState::UnexpectedIdentity,
            format!(
                "The audio endpoints are not the pinned {} device. Remove or disable the look-alike device and install the pinned {} {} from the official vendor download; the session stays video-only.",
                expected.product, expected.product, expected.version
            ),
        );
    }

    let signature = match (render.signature, capture.signature) {
        (SignatureStatus::Valid, SignatureStatus::Valid) => SignatureStatus::Valid,
        (SignatureStatus::Invalid, _) | (_, SignatureStatus::Invalid) => SignatureStatus::Invalid,
        (SignatureStatus::Unsigned, _) | (_, SignatureStatus::Unsigned) => {
            SignatureStatus::Unsigned
        }
        _ => SignatureStatus::Unknown,
    };

    if signature != SignatureStatus::Valid {
        let detail = match signature {
            SignatureStatus::Unsigned => "the driver is not signed",
            SignatureStatus::Invalid => "Windows rejected the driver signature",
            _ => "the driver signature could not be verified",
        };
        return AudioReadiness::failed(
            DeviceState::Unsigned,
            format!(
                "Do not use the audio device: {detail}. Re-run the driver signature check (`Get-AuthenticodeSignature` and `signtool verify /pa /v`) on a Secure-Boot-enabled machine, reinstall the signed official package, or keep the session video-only."
            ),
        );
    }

    let identity = Some(render.identity.clone());
    if let Err(error) = tone.play_test_tone(TEST_TONE_HZ, Duration::from_millis(TEST_TONE_MS)) {
        return AudioReadiness {
            state: DeviceState::ToneNotConfirmed,
            action: format!(
                "The 1 kHz readiness tone could not be played ({error}). Check that the `{}` endpoint is enabled and not muted, then re-run the readiness check; the session stays video-only.",
                expected.render_friendly_name
            ),
            video_only_usable: true,
            identity,
            tone_confirmed: false,
        };
    }

    AudioReadiness {
        state: DeviceState::AudioReady,
        action: format!(
            "Audio device `{}` is ready ({} {}, signature valid, 1 kHz tone confirmed).",
            expected.render_friendly_name, expected.product, expected.version
        ),
        video_only_usable: true,
        identity,
        tone_confirmed: true,
    }
}

fn matches(identity: &EndpointIdentity, name: &str, manufacturer: &str) -> bool {
    identity
        .friendly_name
        .trim()
        .eq_ignore_ascii_case(name.trim())
        && identity
            .manufacturer
            .trim()
            .eq_ignore_ascii_case(manufacturer.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeInventory(Vec<AudioEndpoint>);
    impl DeviceInventory for FakeInventory {
        fn endpoints(&self) -> Vec<AudioEndpoint> {
            self.0.clone()
        }
    }

    struct FakeTone {
        calls: usize,
        result: Result<(), String>,
    }
    impl TestToneSink for FakeTone {
        fn play_test_tone(&mut self, _: f64, _: Duration) -> Result<(), String> {
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

    fn pinned(signature: SignatureStatus) -> FakeInventory {
        FakeInventory(vec![
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

    fn working_tone() -> FakeTone {
        FakeTone {
            calls: 0,
            result: Ok(()),
        }
    }

    #[test]
    fn pinned_signed_device_with_a_tone_is_audio_ready() {
        let mut tone = working_tone();
        let readiness = assess(
            &pinned(SignatureStatus::Valid),
            &vb_cable_expected(),
            &mut tone,
        );

        assert_eq!(readiness.state, DeviceState::AudioReady);
        assert!(readiness.tone_confirmed);
        assert_eq!(tone.calls, 1);
        assert!(readiness.video_only_usable);
    }

    #[test]
    fn a_missing_device_is_actionable_and_keeps_video_usable() {
        let mut tone = working_tone();
        let readiness = assess(&FakeInventory(Vec::new()), &vb_cable_expected(), &mut tone);

        assert_eq!(readiness.state, DeviceState::Missing);
        assert!(readiness.action.contains("Install the pinned"));
        assert!(readiness.video_only_usable);
        assert_eq!(tone.calls, 0);
    }

    #[test]
    fn an_unsigned_device_is_rejected() {
        let mut tone = working_tone();
        let readiness = assess(
            &pinned(SignatureStatus::Unsigned),
            &vb_cable_expected(),
            &mut tone,
        );

        assert_eq!(readiness.state, DeviceState::Unsigned);
        assert!(readiness.action.contains("Get-AuthenticodeSignature"));
        assert!(readiness.video_only_usable);
        assert_eq!(tone.calls, 0);
    }

    #[test]
    fn an_unknown_signature_is_not_accepted() {
        let mut tone = working_tone();
        let readiness = assess(
            &pinned(SignatureStatus::Unknown),
            &vb_cable_expected(),
            &mut tone,
        );

        assert_eq!(readiness.state, DeviceState::Unsigned);
        assert!(!readiness.is_audio_ready());
    }

    #[test]
    fn a_look_alike_device_is_not_accepted_by_prefix() {
        let inventory = FakeInventory(vec![
            endpoint(
                "CABLE Input (VB-Audio Virtual Cable) Copy",
                Direction::Render,
                SignatureStatus::Valid,
            ),
            endpoint(
                "CABLE Output (VB-Audio Virtual Cable)",
                Direction::Capture,
                SignatureStatus::Valid,
            ),
        ]);
        let mut tone = working_tone();
        let readiness = assess(&inventory, &vb_cable_expected(), &mut tone);

        assert_eq!(readiness.state, DeviceState::UnexpectedIdentity);
        assert!(readiness.video_only_usable);
        assert_eq!(tone.calls, 0);
    }
}
