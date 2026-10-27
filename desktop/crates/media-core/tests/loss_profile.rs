#![cfg(not(windows))]

use std::time::Duration;

use remotecam_media_core::loss_scenario::run_loss_profile;
use remotecam_media_core::output::MAX_REPEAT;
use remotecam_media_core::testing::{LossProfile, ProfileLabel};

#[test]
fn under_the_nominal_profile_the_video_degrades_without_freezing_for_two_seconds() {
    let report = run_loss_profile(LossProfile::NOMINAL, 30);

    assert_eq!(report.label, ProfileLabel::Nominal);
    assert!(report.fresh_frames > 700, "video keeps flowing: {report:?}");
    assert_eq!(
        report.absent_ticks, 0,
        "the absence signal never fires: {report:?}"
    );
    assert!(
        report.longest_without_fresh < MAX_REPEAT,
        "longest stretch without a fresh frame: {report:?}"
    );
    assert!(report.nack_requests > 0, "loss is NACKed: {report:?}");
    assert!(
        u64::from(report.packets_sent) > 100 * report.packets_lost,
        "retransmission brings the residual loss under 1 % (raw loss is 3 %): {report:?}"
    );
}

#[test]
fn a_lost_reference_is_repaired_by_a_keyframe_request() {
    let report = run_loss_profile(LossProfile::NOMINAL, 30);
    assert!(
        report.plis > 0,
        "a P-frame lost for good must end in a PLI: {report:?}"
    );
}

#[test]
fn a_clean_network_adds_at_most_the_twenty_millisecond_budget() {
    let clean = LossProfile {
        loss_per_mille: 0,
        jitter: Duration::ZERO,
        ..LossProfile::NOMINAL
    };
    let report = run_loss_profile(clean, 10);
    assert_eq!(report.label, ProfileLabel::OutOfProfile, "not a gate run");
    assert!(
        report.jitter_depth <= Duration::from_millis(20),
        "{report:?}"
    );
    assert_eq!(report.absent_ticks, 0);
    assert_eq!(report.packets_lost, 0);
}

#[test]
fn the_same_profile_replays_the_same_report() {
    assert_eq!(
        run_loss_profile(LossProfile::NOMINAL, 5),
        run_loss_profile(LossProfile::NOMINAL, 5)
    );
}
