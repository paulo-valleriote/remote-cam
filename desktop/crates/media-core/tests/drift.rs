//! T062 — drift regression and resampler pacing.

use remotecam_media_core::drift::{
    DriftComponents, DriftSample, ResamplerModel, SlidingRatio, drift_verdict, estimate_drift,
};

fn linear_samples(slope_ms_per_s: f64, count: usize) -> Vec<DriftSample> {
    (0..count)
        .map(|index| {
            let elapsed_s = index as f64 * 2.0;
            DriftSample {
                elapsed_s,
                source_skew_ms: slope_ms_per_s * elapsed_s,
                audio_playout_delay_ms: 0.0,
                video_playout_delay_ms: 0.0,
            }
        })
        .collect()
}

#[test]
fn regression_recovers_a_known_slope() {
    // 5 ms per hour is 5/3600 ms per second.
    let estimate = estimate_drift(&linear_samples(5.0 / 3_600.0, 60)).expect("regression");
    assert!((estimate.drift_ms_per_hour - 5.0).abs() < 0.01);
    assert!(estimate.r_squared > 0.999);
    assert_eq!(estimate.sample_count, 60);
    assert_eq!(drift_verdict(&estimate, 20.0), "pass");
    assert!(estimate.slope_ci95_ms_per_hour.0 <= 5.0);
    assert!(estimate.slope_ci95_ms_per_hour.1 >= 5.0);
}

#[test]
fn drift_above_the_threshold_fails() {
    let estimate = estimate_drift(&linear_samples(2_500.0 / 3_600.0, 60)).expect("regression");
    assert!(estimate.drift_ms_per_hour > 20.0);
    assert_eq!(drift_verdict(&estimate, 20.0), "fail");
}

#[test]
fn a_confidence_interval_that_straddles_the_threshold_is_conditional() {
    // Alternating noise produces a slope near the threshold with a wide
    // interval: the measurement cannot certify the criterion.
    let mut samples = Vec::new();
    for index in 0..8 {
        let elapsed_s = index as f64 * 60.0;
        let noise = if index % 2 == 0 { 2.0 } else { -2.0 };
        samples.push(DriftSample {
            elapsed_s,
            source_skew_ms: 19.0 * elapsed_s / 3_600.0 + noise,
            audio_playout_delay_ms: 0.0,
            video_playout_delay_ms: 0.0,
        });
    }
    let estimate = estimate_drift(&samples).expect("regression");
    assert!(
        estimate.slope_ci95_ms_per_hour.0.max(0.0) < estimate.slope_ci95_ms_per_hour.1,
        "the interval must be ordered"
    );
    assert_eq!(drift_verdict(&estimate, 20.0), "conditional");
}

#[test]
fn a_free_running_resampler_is_rejected_on_its_own() {
    let model = ResamplerModel::free_running(1e-5);
    assert!(!model.is_rtp_paced());
    assert!((model.drift_ms_per_hour() - 36.0).abs() < 0.01);
    assert!(model.drift_ms_per_hour() > 20.0);
}

#[test]
fn an_rtp_paced_resampler_converges_to_the_rtp_clock() {
    let mut model = ResamplerModel::rtp_paced(1.0);
    for _ in 0..40 {
        model.observe(1.0, 1.0);
    }
    assert!(model.is_rtp_paced());
    assert!(
        model.drift_ms_per_hour().abs() < 1.0,
        "ratio {} still drifts {} ms/h",
        model.ratio(),
        model.drift_ms_per_hour()
    );
}

#[test]
fn the_sliding_ratio_bounds_and_then_forgets_an_outlier() {
    let mut ratio = SlidingRatio::new(20);
    for _ in 0..19 {
        ratio.push(0.02, 0.02);
    }
    ratio.push(0.02, 0.06);
    assert!((ratio.ratio() - 1.1).abs() < 0.001);
    assert_eq!(ratio.sample_count(), 20);

    // Once the outlier leaves the window the estimator returns to 1.0; an
    // unbounded accumulator would carry it forever.
    for _ in 0..20 {
        ratio.push(0.02, 0.02);
    }
    assert!((ratio.ratio() - 1.0).abs() < 1e-9);
}

#[test]
fn drift_components_break_down_the_total() {
    let components = DriftComponents::from_measurement(12.0, 9.5);
    assert_eq!(components.sender_side_skew_drift_ms_per_hour, 0.0);
    assert_eq!(components.receiver_resampler_drift_ms_per_hour, 9.5);
    assert!((components.receiver_present_vs_render_drift_ms_per_hour - 2.5).abs() < 1e-9);
}

#[test]
fn a_constant_series_is_not_a_valid_drift_measurement() {
    let samples = vec![
        DriftSample {
            elapsed_s: 0.0,
            source_skew_ms: 1.0,
            audio_playout_delay_ms: 0.0,
            video_playout_delay_ms: 0.0,
        };
        10
    ];
    assert!(estimate_drift(&samples).is_err());
}
