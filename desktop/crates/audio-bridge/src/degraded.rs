//! Degraded operation when the virtual audio device cannot be used.
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! FR-032 requires an actionable state naming the user's next step **and** a
//! fully usable video-only session. This module turns a readiness failure into
//! that shape; it never decides that the session cannot start.

use crate::readiness::{AudioReadiness, DeviceState};

/// What the session can still do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    /// The pinned device is ready and audio is available.
    AudioAndVideo,
    /// Audio is unavailable; video continues and the user is told why.
    VideoOnly,
}

impl SessionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AudioAndVideo => "audio_and_video",
            Self::VideoOnly => "video_only",
        }
    }
}

/// The degraded-operation record shown in diagnostics and written to evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DegradedReport {
    pub mode: SessionMode,
    pub state: DeviceState,
    /// Why audio is unavailable, in user terms.
    pub reason: String,
    /// The user's next step.
    pub next_step: String,
    /// Always `true`: video remains usable (FR-032).
    pub video_only_usable: bool,
}

/// Build the record from a readiness result.
pub fn degraded_report(readiness: &AudioReadiness) -> DegradedReport {
    if readiness.is_audio_ready() {
        return DegradedReport {
            mode: SessionMode::AudioAndVideo,
            state: readiness.state.clone(),
            reason: "The pinned virtual audio device is ready.".to_owned(),
            next_step: String::new(),
            video_only_usable: true,
        };
    }

    DegradedReport {
        mode: SessionMode::VideoOnly,
        state: readiness.state.clone(),
        reason: reason_for(&readiness.state),
        next_step: readiness.action.clone(),
        video_only_usable: readiness.video_only_usable,
    }
}

fn reason_for(state: &DeviceState) -> String {
    match state {
        DeviceState::AudioReady => "The pinned virtual audio device is ready.".to_owned(),
        DeviceState::Missing => {
            "No virtual audio device with the pinned identity is installed.".to_owned()
        }
        DeviceState::Unsigned => {
            "The virtual audio driver is not accepted by Windows Secure Boot.".to_owned()
        }
        DeviceState::UnexpectedIdentity => {
            "A different audio device is present where the pinned device was expected.".to_owned()
        }
        DeviceState::ToneNotConfirmed => {
            "The readiness tone could not be played, so audio is not confirmed.".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readiness::{
        AudioEndpoint, DeviceInventory, Direction, EndpointIdentity, ExpectedDevice,
        SignatureStatus, TestToneSink, assess, vb_cable_expected,
    };
    use std::time::Duration;

    struct Empty;
    impl DeviceInventory for Empty {
        fn endpoints(&self) -> Vec<AudioEndpoint> {
            Vec::new()
        }
    }

    struct Silent;
    impl TestToneSink for Silent {
        fn play_test_tone(&mut self, _: f64, _: Duration) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn a_missing_device_degrades_to_a_usable_video_only_session() {
        let readiness = assess(&Empty, &vb_cable_expected(), &mut Silent);
        let report = degraded_report(&readiness);

        assert_eq!(report.mode, SessionMode::VideoOnly);
        assert_eq!(report.state, DeviceState::Missing);
        assert!(report.video_only_usable);
        assert!(!report.reason.is_empty());
        assert!(report.next_step.contains("Install"));
    }

    #[test]
    fn a_ready_device_keeps_audio_enabled() {
        struct Pinned;
        impl DeviceInventory for Pinned {
            fn endpoints(&self) -> Vec<AudioEndpoint> {
                let identity = EndpointIdentity {
                    friendly_name: "CABLE Input (VB-Audio Virtual Cable)".to_owned(),
                    manufacturer: "VB-Audio".to_owned(),
                    driver_version: "45".to_owned(),
                };
                let capture = EndpointIdentity {
                    friendly_name: "CABLE Output (VB-Audio Virtual Cable)".to_owned(),
                    ..identity.clone()
                };
                vec![
                    AudioEndpoint {
                        identity,
                        direction: Direction::Render,
                        signature: SignatureStatus::Valid,
                    },
                    AudioEndpoint {
                        identity: capture,
                        direction: Direction::Capture,
                        signature: SignatureStatus::Valid,
                    },
                ]
            }
        }
        let _: ExpectedDevice = vb_cable_expected();
        let readiness = assess(&Pinned, &vb_cable_expected(), &mut Silent);
        let report = degraded_report(&readiness);

        assert_eq!(report.mode, SessionMode::AudioAndVideo);
        assert!(report.next_step.is_empty());
        assert!(report.video_only_usable);
    }
}
