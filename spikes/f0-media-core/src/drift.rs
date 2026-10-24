//! A/V drift measurement and receiver resampler pacing (T073, T081).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! Drift is measured on the receiver: the sender's audio and video RTP clocks
//! come from the same monotonic clock, so sender-side skew is zero. The two
//! receiver-side causes are the resampler ratio and the difference between the
//! present and render clocks, and they have different remediations, so the
//! summary reports them separately.
//!
//! The resampler must be paced from the RTP timestamps. A free-running
//! fixed-point resampler with a relative frequency error of `1e-5` drifts
//! `36 ms/h` on its own, which fails the `20 ms/h` criterion before the rest of
//! the pipeline is considered.

use std::fmt;

/// Drift gate threshold, milliseconds per hour.
pub const DRIFT_THRESHOLD_MS_PER_HOUR: f64 = 20.0;

/// Relative frequency error that a free-running fixed-point resampler
/// typically carries (`1e-5`), expressed in milliseconds per hour.
pub const FREE_RUNNING_ERROR_MS_PER_HOUR: f64 = 1e-5 * 3_600_000.0;

/// One drift observation from the pipeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftSample {
    pub elapsed_s: f64,
    pub source_skew_ms: f64,
    pub audio_playout_delay_ms: f64,
    pub video_playout_delay_ms: f64,
}

impl DriftSample {
    pub fn playout_skew_ms(&self) -> f64 {
        self.source_skew_ms + self.audio_playout_delay_ms - self.video_playout_delay_ms
    }
}

/// Regression result for one drift run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftEstimate {
    pub sample_count: usize,
    pub slope_ms_per_s: f64,
    pub drift_ms_per_hour: f64,
    pub intercept_ms: f64,
    pub r_squared: f64,
    /// 95 % confidence interval on the slope, in milliseconds per hour.
    pub slope_ci95_ms_per_hour: (f64, f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftError {
    TooFewSamples { count: usize },
    NonFiniteSample,
    NoElapsedSpread,
}

impl fmt::Display for DriftError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewSamples { count } => {
                write!(
                    formatter,
                    "drift regression needs at least 3 samples, got {count}"
                )
            }
            Self::NonFiniteSample => write!(formatter, "drift samples must be finite"),
            Self::NoElapsedSpread => {
                write!(
                    formatter,
                    "drift samples do not cover a non-zero elapsed interval"
                )
            }
        }
    }
}

impl std::error::Error for DriftError {}

/// Two-sided 95 % Student-t critical value, for the small sample counts the
/// gate procedure produces (one marker every two seconds).
fn t_critical_95(degrees_of_freedom: usize) -> f64 {
    const TABLE: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
        2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
        2.052, 2.048, 2.045, 2.042,
    ];
    if degrees_of_freedom == 0 {
        return f64::INFINITY;
    }
    TABLE.get(degrees_of_freedom - 1).copied().unwrap_or(1.96)
}

/// Ordinary least-squares regression of playout skew against elapsed time.
pub fn estimate_drift(samples: &[DriftSample]) -> Result<DriftEstimate, DriftError> {
    if samples.len() < 3 {
        return Err(DriftError::TooFewSamples {
            count: samples.len(),
        });
    }
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xx = 0.0;
    let mut sum_xy = 0.0;
    for sample in samples {
        if !sample.elapsed_s.is_finite()
            || !sample.source_skew_ms.is_finite()
            || !sample.audio_playout_delay_ms.is_finite()
            || !sample.video_playout_delay_ms.is_finite()
        {
            return Err(DriftError::NonFiniteSample);
        }
        let x = sample.elapsed_s;
        let y = sample.playout_skew_ms();
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
    }

    let count = samples.len() as f64;
    let mean_x = sum_x / count;
    let mean_y = sum_y / count;
    let sxx = sum_xx - count * mean_x * mean_x;
    if !sxx.is_finite() || sxx.abs() < f64::EPSILON {
        return Err(DriftError::NoElapsedSpread);
    }
    let sxy = sum_xy - count * mean_x * mean_y;
    let slope = sxy / sxx;
    let intercept = mean_y - slope * mean_x;

    let mut sse = 0.0;
    let mut sst = 0.0;
    for sample in samples {
        let predicted = intercept + slope * sample.elapsed_s;
        let residual = sample.playout_skew_ms() - predicted;
        sse += residual * residual;
        let centred = sample.playout_skew_ms() - mean_y;
        sst += centred * centred;
    }
    let r_squared = if sst.abs() < f64::EPSILON {
        1.0
    } else {
        1.0 - sse / sst
    };

    let degrees_of_freedom = samples.len() - 2;
    let variance = sse / degrees_of_freedom as f64;
    let standard_error = (variance / sxx).sqrt();
    let t = t_critical_95(degrees_of_freedom);
    let half_width_ms_per_s = t * standard_error;

    Ok(DriftEstimate {
        sample_count: samples.len(),
        slope_ms_per_s: slope,
        drift_ms_per_hour: slope * 3_600.0,
        intercept_ms: intercept,
        r_squared,
        slope_ci95_ms_per_hour: (
            (slope - half_width_ms_per_s) * 3_600.0,
            (slope + half_width_ms_per_s) * 3_600.0,
        ),
    })
}

/// Gate verdict for a drift estimate.
///
/// A confidence interval that reaches above the threshold cannot certify the
/// criterion, even when the point estimate is inside it: the verdict is
/// `conditional` and the procedure requires more markers.
pub fn drift_verdict(estimate: &DriftEstimate, threshold_ms_per_hour: f64) -> &'static str {
    if estimate.drift_ms_per_hour.abs() > threshold_ms_per_hour {
        "fail"
    } else if estimate.slope_ci95_ms_per_hour.0.abs() > threshold_ms_per_hour
        || estimate.slope_ci95_ms_per_hour.1.abs() > threshold_ms_per_hour
    {
        "conditional"
    } else {
        "pass"
    }
}

/// How the audio renderer derives its resampling ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResamplerPacing {
    /// A fixed ratio derived from the nominal clock, with no RTP feedback.
    FreeRunning { relative_error: f64 },
    /// A ratio steered towards the RTP timestamp cadence.
    RtpPaced { target_ratio: f64 },
}

/// Small proportional-integral loop that paces the resampler from RTP time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResamplerModel {
    pacing: ResamplerPacing,
    ratio: f64,
    /// Proportional gain per observation.
    gain: f64,
}

impl ResamplerModel {
    pub fn new(pacing: ResamplerPacing) -> Self {
        let ratio = match pacing {
            ResamplerPacing::FreeRunning { relative_error } => 1.0 + relative_error,
            ResamplerPacing::RtpPaced { target_ratio } => target_ratio,
        };
        Self {
            pacing,
            ratio,
            gain: 0.2,
        }
    }

    pub fn free_running(relative_error: f64) -> Self {
        Self::new(ResamplerPacing::FreeRunning { relative_error })
    }

    pub fn rtp_paced(target_ratio: f64) -> Self {
        Self::new(ResamplerPacing::RtpPaced { target_ratio })
    }

    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    /// Feed one observation: the RTP-time interval and the rendered interval.
    ///
    /// The loop moves the ratio a fifth of the way towards the ratio implied by
    /// the observation. A free-running resampler ignores the observation,
    /// which is the behaviour the drift gate rejects.
    pub fn observe(&mut self, rtp_elapsed_s: f64, output_elapsed_s: f64) -> f64 {
        if let ResamplerPacing::RtpPaced { .. } = self.pacing
            && rtp_elapsed_s > 0.0
            && output_elapsed_s.is_finite()
        {
            let implied = output_elapsed_s / rtp_elapsed_s;
            self.ratio += (implied - self.ratio) * self.gain;
        }
        self.ratio
    }

    /// Drift this resampler contributes, in milliseconds per hour.
    pub fn drift_ms_per_hour(&self) -> f64 {
        match self.pacing {
            ResamplerPacing::FreeRunning { relative_error } => relative_error * 3_600_000.0,
            ResamplerPacing::RtpPaced { target_ratio } => (self.ratio - target_ratio) * 3_600_000.0,
        }
    }

    pub fn is_rtp_paced(&self) -> bool {
        matches!(self.pacing, ResamplerPacing::RtpPaced { .. })
    }
}

/// Sliding-window ratio estimator used by the audio renderer.
///
/// The ratio is `rendered time / RTP time` over a bounded window, so a single
/// late buffer cannot steer the resampler. A free-running ratio is rejected by
/// [`ResamplerModel`] before it reaches the renderer.
#[derive(Debug, Clone)]
pub struct SlidingRatio {
    capacity: usize,
    samples: std::collections::VecDeque<(f64, f64)>,
    rendered_total: f64,
    rtp_total: f64,
}

impl SlidingRatio {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            samples: std::collections::VecDeque::with_capacity(capacity.max(1)),
            rendered_total: 0.0,
            rtp_total: 0.0,
        }
    }

    /// Add one observation pair (both in seconds or both in samples).
    pub fn push(&mut self, rtp_delta: f64, rendered_delta: f64) {
        if !rtp_delta.is_finite() || !rendered_delta.is_finite() || rtp_delta <= 0.0 {
            return;
        }
        if self.samples.len() == self.capacity
            && let Some((old_rtp, old_rendered)) = self.samples.pop_front()
        {
            self.rtp_total -= old_rtp;
            self.rendered_total -= old_rendered;
        }
        self.samples.push_back((rtp_delta, rendered_delta));
        self.rtp_total += rtp_delta;
        self.rendered_total += rendered_delta;
    }

    pub fn ratio(&self) -> f64 {
        if self.rtp_total <= 0.0 {
            1.0
        } else {
            self.rendered_total / self.rtp_total
        }
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
}

/// Break the measured drift into its three receiver-side causes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftComponents {
    pub sender_side_skew_drift_ms_per_hour: f64,
    pub receiver_resampler_drift_ms_per_hour: f64,
    pub receiver_present_vs_render_drift_ms_per_hour: f64,
}

impl DriftComponents {
    /// Sender skew is zero by construction (both streams share the sender's
    /// monotonic clock); the residual is attributed to the receiver.
    pub fn from_measurement(
        total_drift_ms_per_hour: f64,
        resampler_drift_ms_per_hour: f64,
    ) -> Self {
        Self {
            sender_side_skew_drift_ms_per_hour: 0.0,
            receiver_resampler_drift_ms_per_hour: resampler_drift_ms_per_hour,
            receiver_present_vs_render_drift_ms_per_hour: total_drift_ms_per_hour
                - resampler_drift_ms_per_hour,
        }
    }
}
