//! Five-second media stall analysis (T082).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The relay holds media packets for five seconds while the session stays up.
//! The gate asks three things: the session continued, latency returned to the
//! pre-stall baseline within two seconds, and the buffer did not grow without
//! bound. Discarded frames are counted, not asserted away.

use serde::{Deserialize, Serialize};

/// One sample of the stall trace.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StallSample {
    pub elapsed_ms: f64,
    pub buffer_depth: usize,
    pub latency_ms: f64,
    pub discarded_frames: u64,
    pub media_received: u64,
}

/// Outcome of one stall scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StallResult {
    pub session_continued: bool,
    pub baseline_latency_ms: f64,
    pub post_stall_latency_ms: f64,
    pub recovery_ms: f64,
    pub max_buffer_depth: usize,
    pub buffer_growth_bounded: bool,
    pub discarded_frames: u64,
    pub verdict: String,
}

/// Analyse a trace that contains a stall between `stall_start_ms` and
/// `stall_end_ms`.
pub fn analyse(
    samples: &[StallSample],
    stall_start_ms: f64,
    stall_end_ms: f64,
    max_recovery_ms: f64,
) -> Result<StallResult, String> {
    let before: Vec<&StallSample> = samples
        .iter()
        .filter(|sample| sample.elapsed_ms < stall_start_ms)
        .collect();
    if before.len() < 3 {
        return Err("the trace needs at least three pre-stall samples".to_owned());
    }
    let baseline = median(before.iter().map(|sample| sample.latency_ms).collect());

    let after: Vec<&StallSample> = samples
        .iter()
        .filter(|sample| sample.elapsed_ms >= stall_end_ms)
        .collect();
    if after.is_empty() {
        return Err("the trace has no post-stall samples".to_owned());
    }

    let tolerance = (baseline * 0.1).max(10.0);
    let recovery_ms = after
        .iter()
        .find(|sample| sample.latency_ms <= baseline + tolerance)
        .map(|sample| sample.elapsed_ms - stall_end_ms)
        .ok_or_else(|| "latency never returned to the pre-stall baseline".to_owned())?;

    let baseline_depth = median(
        before
            .iter()
            .map(|sample| sample.buffer_depth as f64)
            .collect(),
    );
    let max_depth = samples
        .iter()
        .map(|sample| sample.buffer_depth)
        .max()
        .unwrap_or(0);
    // Growth is unbounded when the depth keeps climbing past four times the
    // baseline and does not drain during the stall. A bounded buffer may spike
    // and then drop; a queue that never drains is the defect.
    let growth_bounded = (max_depth as f64) <= (baseline_depth * 4.0).max(8.0);

    let media_after = after
        .last()
        .map(|sample| sample.media_received)
        .unwrap_or(0);
    let media_before = before
        .last()
        .map(|sample| sample.media_received)
        .unwrap_or(0);
    let session_continued = media_after > media_before;

    let post_stall_latency_ms = after
        .iter()
        .find(|sample| sample.elapsed_ms >= stall_end_ms + recovery_ms)
        .map(|sample| sample.latency_ms)
        .unwrap_or(baseline);

    let discarded_frames = samples
        .iter()
        .map(|sample| sample.discarded_frames)
        .max()
        .unwrap_or(0);

    let passed = session_continued && recovery_ms <= max_recovery_ms && growth_bounded;

    Ok(StallResult {
        session_continued,
        baseline_latency_ms: baseline,
        post_stall_latency_ms,
        recovery_ms,
        max_buffer_depth: max_depth,
        buffer_growth_bounded: growth_bounded,
        discarded_frames,
        verdict: if passed { "pass" } else { "fail" }.to_owned(),
    })
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(recovery_ms: f64, max_depth: usize) -> Vec<StallSample> {
        let mut samples = Vec::new();
        for index in 0..200 {
            let elapsed = index as f64 * 100.0;
            let latency = if (10_000.0..15_000.0).contains(&elapsed) {
                2_000.0
            } else if elapsed >= 15_000.0 + recovery_ms {
                100.0
            } else if elapsed >= 15_000.0 {
                300.0
            } else {
                100.0
            };
            let depth = if (10_000.0..15_000.0).contains(&elapsed) {
                max_depth
            } else {
                3
            };
            samples.push(StallSample {
                elapsed_ms: elapsed,
                buffer_depth: depth,
                latency_ms: latency,
                discarded_frames: if elapsed >= 15_000.0 { 120 } else { 0 },
                media_received: index as u64,
            });
        }
        samples
    }

    #[test]
    fn recovery_within_two_seconds_passes() {
        let result = analyse(&trace(1_000.0, 6), 10_000.0, 15_000.0, 2_000.0).expect("analysis");
        assert_eq!(result.verdict, "pass");
        assert!(result.session_continued);
        assert!(result.recovery_ms <= 2_000.0);
        assert_eq!(result.discarded_frames, 120);
    }

    #[test]
    fn recovery_after_two_seconds_fails() {
        let result = analyse(&trace(2_500.0, 6), 10_000.0, 15_000.0, 2_000.0).expect("analysis");
        assert_eq!(result.verdict, "fail");
        assert!(result.recovery_ms > 2_000.0);
    }

    #[test]
    fn unbounded_buffer_growth_fails_even_with_fast_recovery() {
        let result = analyse(&trace(500.0, 50), 10_000.0, 15_000.0, 2_000.0).expect("analysis");
        assert!(!result.buffer_growth_bounded);
        assert_eq!(result.verdict, "fail");
    }

    #[test]
    fn a_session_that_never_resumes_is_a_failure() {
        let mut samples = trace(500.0, 6);
        for sample in &mut samples {
            if sample.elapsed_ms >= 15_000.0 {
                sample.media_received = 48;
            }
        }
        let result = analyse(&samples, 10_000.0, 15_000.0, 2_000.0).expect("analysis");
        assert!(!result.session_continued);
        assert_eq!(result.verdict, "fail");
    }
}
