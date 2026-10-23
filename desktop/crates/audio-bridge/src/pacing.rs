//! RTP-paced ratio estimation and mono resampling for the audio bridge.
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The estimator is the same algorithm the desktop receive pipeline uses
//! (`media-core::drift::SlidingRatio`, `media-core::resample::MonoResampler`).
//! It is kept local because `media-core` links the WebRTC stack, which the
//! standalone audio bridge does not need; the RTP timestamps are still produced
//! by `media-core` and handed to [`crate::wasapi_render::CableRenderer::write_mono`].
//!
//! The ratio is derived from the RTP clock: a free-running resampler with a
//! `1e-5` relative error drifts about 36 ms/h and would fail the 20 ms/h
//! criterion on its own.

use std::collections::VecDeque;

/// Sliding estimate of rendered/expected time.
#[derive(Debug, Clone)]
pub struct SlidingRatio {
    capacity: usize,
    samples: VecDeque<(f64, f64)>,
}

impl SlidingRatio {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            samples: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    /// Push one (RTP elapsed, rendered elapsed) observation in seconds.
    pub fn push(&mut self, rtp_delta: f64, rendered_delta: f64) {
        if !rtp_delta.is_finite() || !rendered_delta.is_finite() || rtp_delta <= 0.0 {
            return;
        }
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back((rtp_delta, rendered_delta));
    }

    /// Ratio applied to the resampler. Above one slows the stream down.
    pub fn ratio(&self) -> f64 {
        if self.samples.is_empty() {
            return 1.0;
        }
        let expected: f64 = self.samples.iter().map(|(rtp, _)| rtp).sum();
        let rendered: f64 = self.samples.iter().map(|(_, rendered)| rendered).sum();
        if rendered <= 0.0 {
            return 1.0;
        }
        (expected / rendered).clamp(0.5, 2.0)
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
}

/// Streaming mono linear-interpolation resampler.
#[derive(Debug, Clone)]
pub struct MonoResampler {
    previous: f32,
    phase: f64,
    has_previous: bool,
}

impl Default for MonoResampler {
    fn default() -> Self {
        Self::new()
    }
}

impl MonoResampler {
    pub fn new() -> Self {
        Self {
            previous: 0.0,
            phase: 0.0,
            has_previous: false,
        }
    }

    /// Resample `input` at `ratio` output samples per input sample.
    pub fn process(&mut self, input: &[f32], ratio: f64) -> Vec<f32> {
        if input.is_empty() || !ratio.is_finite() || ratio <= 0.0 {
            return Vec::new();
        }
        let ratio = ratio.clamp(0.5, 2.0);
        let mut output = Vec::with_capacity((input.len() as f64 * ratio).ceil() as usize + 2);

        if !self.has_previous {
            self.previous = input[0];
            self.has_previous = true;
        }

        let mut position = self.phase;
        while position < input.len() as f64 {
            let index = position.floor() as usize;
            let fraction = position - index as f64;
            let current = input.get(index).copied().unwrap_or(self.previous);
            let next = input.get(index + 1).copied().unwrap_or(current);
            let sample = current as f64 + (next as f64 - current as f64) * fraction;
            output.push(sample as f32);
            position += 1.0 / ratio;
        }

        self.phase = position - input.len() as f64;
        self.previous = *input.last().unwrap_or(&self.previous);
        output
    }

    pub fn reset(&mut self) {
        self.previous = 0.0;
        self.phase = 0.0;
        self.has_previous = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_estimate_is_neutral() {
        assert_eq!(SlidingRatio::new(100).ratio(), 1.0);
    }

    #[test]
    fn a_rendered_side_that_lags_slows_the_resampler_down() {
        let mut ratio = SlidingRatio::new(10);
        ratio.push(1.0, 0.99);
        assert!(ratio.ratio() > 1.0);
    }

    #[test]
    fn resampling_at_unity_preserves_length_within_one_sample() {
        let mut resampler = MonoResampler::new();
        let input: Vec<f32> = (0..480).map(|index| index as f32 / 480.0).collect();
        let output = resampler.process(&input, 1.0);
        assert!((output.len() as i64 - input.len() as i64).abs() <= 1);
    }
}
