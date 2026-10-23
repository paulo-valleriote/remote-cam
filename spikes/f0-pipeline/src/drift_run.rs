//! Drift run evidence (T081).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! One marker pair every two seconds feeds the regression in
//! `remotecam-media-core`, and the result is written as the contract's
//! `drift_samples.csv` and `drift_summary.json`. The three receiver-side
//! components are reported separately because each has a different remediation.

use std::fmt::Write as _;
use std::path::Path;

use remotecam_media_core::drift::{
    DriftComponents, DriftSample, ResamplerModel, drift_verdict, estimate_drift,
};
use serde::{Deserialize, Serialize};

pub const MARKER_PERIOD_S: f64 = 2.0;

/// One marker pair, before regression.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DriftObservation {
    pub elapsed_s: f64,
    pub source_skew_ms: f64,
    pub audio_playout_delay_ms: f64,
    pub video_playout_delay_ms: f64,
}

impl DriftObservation {
    pub fn playout_skew_ms(&self) -> f64 {
        self.source_skew_ms + self.audio_playout_delay_ms - self.video_playout_delay_ms
    }
}

impl From<DriftObservation> for DriftSample {
    fn from(observation: DriftObservation) -> Self {
        Self {
            elapsed_s: observation.elapsed_s,
            source_skew_ms: observation.source_skew_ms,
            audio_playout_delay_ms: observation.audio_playout_delay_ms,
            video_playout_delay_ms: observation.video_playout_delay_ms,
        }
    }
}

pub fn write_drift_csv(path: &Path, observations: &[DriftObservation]) -> Result<(), String> {
    let mut output = String::from(
        "sample_id,run_id,elapsed_s,source_skew_ms,audio_playout_delay_ms,video_playout_delay_ms,playout_skew_ms\n",
    );
    for (index, observation) in observations.iter().enumerate() {
        let _ = writeln!(
            output,
            "{},drift-01,{:.3},{:.4},{:.4},{:.4},{:.4}",
            index + 1,
            observation.elapsed_s,
            observation.source_skew_ms,
            observation.audio_playout_delay_ms,
            observation.video_playout_delay_ms,
            observation.playout_skew_ms(),
        );
    }
    std::fs::write(path, output).map_err(|error| error.to_string())
}

/// Build the `drift_summary.json` document.
pub fn summary_document(
    run_id: &str,
    observations: &[DriftObservation],
    resampler: &ResamplerModel,
) -> serde_json::Value {
    let samples: Vec<DriftSample> = observations
        .iter()
        .copied()
        .map(DriftSample::from)
        .collect();
    match estimate_drift(&samples) {
        Ok(estimate) => {
            let resampler_drift = resampler.drift_ms_per_hour();
            let components =
                DriftComponents::from_measurement(estimate.drift_ms_per_hour, resampler_drift);
            serde_json::json!({
                "run_id": run_id,
                "duration_s": observations.last().map(|sample| sample.elapsed_s).unwrap_or(0.0),
                "sample_count": observations.len(),
                "marker_period_s": MARKER_PERIOD_S,
                "regression": {
                    "slope_ms_per_s": estimate.slope_ms_per_s,
                    "drift_ms_per_hour": estimate.drift_ms_per_hour,
                    "intercept_ms": estimate.intercept_ms,
                    "r_squared": estimate.r_squared,
                    "slope_ci95_ms_per_hour": [
                        estimate.slope_ci95_ms_per_hour.0,
                        estimate.slope_ci95_ms_per_hour.1,
                    ],
                },
                "criterion": { "metric": "drift_ms_per_hour", "threshold": 20, "comparison": "<=" },
                "verdict": drift_verdict(&estimate, 20.0),
                "components": {
                    "sender_side_skew_drift_ms_per_hour": components.sender_side_skew_drift_ms_per_hour,
                    "receiver_resampler_drift_ms_per_hour": components.receiver_resampler_drift_ms_per_hour,
                    "receiver_present_vs_render_drift_ms_per_hour": components.receiver_present_vs_render_drift_ms_per_hour,
                },
                "resampler": {
                    "rtp_paced": resampler.is_rtp_paced(),
                    "ratio": resampler.ratio(),
                    "drift_ms_per_hour": resampler_drift,
                },
                "note": "A free-running resampler at 1e-5 relative error drifts about 36 ms/h and would fail this criterion on its own; the receiver is paced from the RTP timestamps.",
            })
        }
        Err(error) => serde_json::json!({
            "run_id": run_id,
            "sample_count": observations.len(),
            "verdict": "incomplete",
            "error": error.to_string(),
        }),
    }
}
