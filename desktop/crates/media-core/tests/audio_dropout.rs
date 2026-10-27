use std::time::Duration;

use remotecam_media_core::audio_scenario::{
    DeliveryGapDetector, SilenceRunDetector, run_audio_profile,
};
use remotecam_media_core::testing::{LossProfile, ProfileLabel};

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

#[test]
fn delivery_detector_flags_a_stall_of_50_ms_but_not_one_of_49() {
    let mut detector = DeliveryGapDetector::new();
    for at in [0, 20, 40, 89, 109] {
        detector.observe(ms(at));
    }
    assert_eq!(detector.dropouts(), 0);
    assert_eq!(detector.longest_gap(), ms(49));
    detector.observe(ms(159));
    assert_eq!(detector.dropouts(), 1);
    assert_eq!(detector.longest_gap(), ms(50));
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
    assert_eq!(report.delivery_dropouts, 0);
    assert_eq!(report.silence_dropouts, 0);
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
    assert!(report.silence_dropouts > 0, "the content detector sees it");
}
