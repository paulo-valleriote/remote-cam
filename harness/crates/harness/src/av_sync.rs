use std::time::Duration;

use remotecam_media_core::av_scenario::{
    AvConfig, AvReport, run_av_profile, run_av_profile_observed,
};
use remotecam_media_core::av_sync::{DRIFT_TARGET, SyncMode};
use remotecam_media_core::testing::ProfileLabel;
use remotecam_protocol::signaling::{CloseCode, Limits, SignalingMessage};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{
    AV_DRIFT_LIMIT, AvDrift, Effect, MediaFrame, SessionCore, SessionEvent, SessionState, Timestamp,
};
use serde::Serialize;

const NOMINAL_SECONDS: u64 = 3_600;
const ABSENT_AUDIO_SECONDS: u64 = 10;
const DRIFT_LIMIT_SECONDS: u64 = 60;
const INJECTED_VIDEO_DELAY: Duration = Duration::from_millis(150);
const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];
const PEER_FINGERPRINT: [u8; 32] = [0x55; 32];

/// A/V synchronisation (REC-54): the video follows the audio clock. One logical hour
/// under the nominal profile, the free-running mode without audio, and the session
/// ending when the drift passes the hard limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AvSyncObservation {
    pub nominal: AvRun,
    pub audio_absent: AvRun,
    pub drift_limit: DriftLimitRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AvRun {
    pub profile: &'static str,
    pub duration_seconds: u64,
    pub mode: &'static str,
    pub video_frames_sent: u32,
    pub presented: u64,
    pub measured: u64,
    pub skipped: u64,
    pub over_target: u64,
    pub max_drift_micros: u64,
    /// Largest unfiltered offset of a single frame; loss recovery shows up here.
    pub max_frame_offset_micros: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DriftLimitRun {
    pub injected_video_delay_millis: u64,
    pub stopped_at_millis: u64,
    pub max_drift_micros: u64,
    pub session_state: &'static str,
    pub close_reason: &'static str,
    pub close_before_release: bool,
}

impl AvRun {
    fn from_report(report: &AvReport) -> Self {
        Self {
            profile: profile_name(report.label),
            duration_seconds: report.seconds,
            mode: match report.mode {
                SyncMode::AudioMaster => "audio_master",
                SyncMode::VideoFree => "video_free",
            },
            video_frames_sent: report.video_frames_sent,
            presented: report.presented,
            measured: report.measured,
            skipped: report.skipped,
            over_target: report.over_target,
            max_drift_micros: saturating_micros(report.max_drift),
            max_frame_offset_micros: saturating_micros(report.max_frame_offset),
        }
    }
}

pub(crate) fn profile_name(label: ProfileLabel) -> &'static str {
    match label {
        ProfileLabel::Nominal => "nominal",
        ProfileLabel::OutOfProfile => "out_of_profile",
    }
}

pub(crate) fn saturating_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub(crate) fn saturating_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn connected_core() -> Result<SessionCore<ManualClock, SeededRandomSource>, &'static str> {
    let mut core = SessionCore::new(
        ManualClock::new(Timestamp::ZERO),
        SeededRandomSource::new(0),
        DESKTOP_FINGERPRINT,
    );
    let effects = core
        .handle(SessionEvent::StartPairing)
        .map_err(|_| "internal_error")?;
    let Ok([Effect::PresentPairing(material)]) = <[Effect; 1]>::try_from(effects) else {
        return Err("internal_error");
    };
    let frame = SignalingMessage::Pair {
        token: *material.token().as_bytes(),
        dtls_fingerprint: PEER_FINGERPRINT,
        limits: Limits::new(),
    }
    .encode()
    .map_err(|_| "internal_error")?;
    core.handle(SessionEvent::PeerConnected)
        .map_err(|_| "internal_error")?;
    core.authenticate_first_frame(&frame)
        .map_err(|_| "internal_error")?;
    core.handle(SessionEvent::NegotiationCompleted)
        .map_err(|_| "internal_error")?;
    core.handle(SessionEvent::FirstMediaFrame(MediaFrame {
        sequence: 0,
        timestamp: Timestamp::ZERO,
    }))
    .map_err(|_| "internal_error")?;
    Ok(core)
}

/// Feeds each measured drift to a connected Session Core until it ends the session.
fn drive_session(run_seconds: u64) -> Result<DriftLimitRun, &'static str> {
    let mut core = connected_core()?;
    let mut ending_effects = None;
    let mut transition_failed = false;
    let config = AvConfig {
        video_delay: INJECTED_VIDEO_DELAY,
        ..AvConfig::nominal(run_seconds)
    };
    let report = run_av_profile_observed(config, &mut |_, drift| match core
        .handle(SessionEvent::AvDrift(AvDrift::from_duration(drift)))
    {
        Ok(effects) if core.state() == SessionState::Ended => {
            ending_effects = Some(effects);
            false
        }
        Ok(_) => true,
        Err(_) => {
            transition_failed = true;
            false
        }
    });
    if transition_failed {
        return Err("internal_error");
    }
    let effects = ending_effects.ok_or("media_failure")?;
    let stopped_at = report.stopped_at.ok_or("internal_error")?;
    Ok(DriftLimitRun {
        injected_video_delay_millis: saturating_millis(INJECTED_VIDEO_DELAY),
        stopped_at_millis: saturating_millis(stopped_at),
        max_drift_micros: saturating_micros(report.max_drift),
        session_state: core.state().as_str(),
        close_reason: "media_failure",
        close_before_release: effects
            == [
                Effect::SendClose {
                    reason: CloseCode::MediaFailure,
                },
                Effect::End,
            ],
    })
}

pub(crate) fn observe() -> Result<AvSyncObservation, &'static str> {
    let nominal = run_av_profile(AvConfig::nominal(NOMINAL_SECONDS));
    let absent = run_av_profile(AvConfig {
        audio: false,
        ..AvConfig::nominal(ABSENT_AUDIO_SECONDS)
    });
    let drift_limit = drive_session(DRIFT_LIMIT_SECONDS)?;
    let passes = nominal.label == ProfileLabel::Nominal
        && nominal.mode == SyncMode::AudioMaster
        && nominal.max_drift <= DRIFT_TARGET
        && nominal.over_target == 0
        && absent.mode == SyncMode::VideoFree
        && absent.measured == 0
        && absent.presented > 0
        && drift_limit.max_drift_micros > saturating_micros(AV_DRIFT_LIMIT)
        && drift_limit.close_before_release
        && drift_limit.session_state == SessionState::Ended.as_str();
    if !passes {
        return Err("media_failure");
    }
    Ok(AvSyncObservation {
        nominal: AvRun::from_report(&nominal),
        audio_absent: AvRun::from_report(&absent),
        drift_limit,
    })
}
