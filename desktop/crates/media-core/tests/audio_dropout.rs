use std::time::Duration;

use remotecam_media_core::audio_engine::FrameOrigin;
use remotecam_media_core::audio_scenario::{
    SilenceRunDetector, UnderrunDetector, run_audio_profile,
};
use remotecam_media_core::testing::{LossProfile, ProfileLabel};

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

#[test]
fn underrun_detector_counts_frames_with_no_audio_and_treats_plc_as_audio() {
    let mut detector = UnderrunDetector::new();
    detector.observe(FrameOrigin::Silence);
    detector.observe(FrameOrigin::Silence);
    assert_eq!(detector.dropouts(), 0, "40 ms of silence is below the bar");
    detector.observe(FrameOrigin::Concealed);
    detector.observe(FrameOrigin::Silence);
    detector.observe(FrameOrigin::Silence);
    assert_eq!(detector.dropouts(), 0, "PLC is audio and breaks the run");
    detector.observe(FrameOrigin::Silence);
    assert_eq!(detector.dropouts(), 1, "60 ms without any audio");
    detector.observe(FrameOrigin::Silence);
    assert_eq!(detector.dropouts(), 1, "one run counts once");
    assert_eq!(detector.longest_run(), ms(80));
    detector.observe(FrameOrigin::Decoded);
    assert_eq!(detector.longest_run(), ms(80));
}

#[test]
fn silence_detector_flags_exact_zero_runs_of_50_ms_across_frames() {
    let mut detector = SilenceRunDetector::new();
    detector.observe(&[7; 960]);
    detector.observe(&[0; 960]);
    detector.observe(&[0; 960]);
    assert_eq!(detector.dropouts(), 0, "40 ms of zeros is below the bar");
    detector.observe(&[0; 960]);
    assert_eq!(detector.dropouts(), 1, "60 ms of zeros");
    detector.observe(&[0; 960]);
    assert_eq!(detector.dropouts(), 1, "one run counts once");
    detector.observe(&[7; 960]);
    detector.observe(&[0; 2_400]);
    assert_eq!(detector.dropouts(), 2, "exactly 50 ms");
    assert_eq!(detector.longest_run(), ms(80));
}

#[test]
fn ten_minutes_of_nominal_loss_have_zero_dropouts_by_both_detectors() {
    let report = run_audio_profile(LossProfile::NOMINAL, 600);
    assert_eq!(report.label, ProfileLabel::Nominal);
    assert_eq!(report.seconds, 600);
    assert_eq!(report.underrun_dropouts, 0);
    assert_eq!(report.silence_dropouts, 0);
    assert!(report.detectors_agree());
    assert!(report.passes_gate());
    assert!(report.frames_played >= 600 * 50 - 4, "{report:?}");
    assert!(report.concealed > 0, "3 % loss must exercise PLC");
    assert_eq!(report.decode_errors, 0);
    assert_eq!(report.silent, 0);
}

#[test]
fn a_heavier_profile_is_labelled_out_of_profile() {
    let lossy = LossProfile {
        loss_per_mille: 400,
        ..LossProfile::NOMINAL
    };
    let report = run_audio_profile(lossy, 20);
    assert_eq!(report.label, ProfileLabel::OutOfProfile);
    assert!(report.underrun_dropouts > 0 && report.silence_dropouts > 0);
    assert!(report.detectors_agree(), "both see the same outage");
    assert!(!report.passes_gate());
}
