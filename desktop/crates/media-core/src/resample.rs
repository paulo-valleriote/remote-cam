//! Mono linear-interpolation resampler with a variable ratio.
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The ratio is supplied by the RTP-paced estimator, so the resampler never
//! free-runs. The state carries the last input sample and the fractional phase
//! across calls, which is what keeps the output continuous when a buffer is
//! split.

/// Streaming mono resampler.
#[derive(Debug, Clone)]
pub struct MonoResampler {
    previous: f32,
    phase: f64,
    has_previous: bool,
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
    ///
    /// A ratio above one slows the stream down (more output samples), a ratio
    /// below one speeds it up. The phase is carried across calls.
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
            let current = input[index.min(input.len() - 1)];
            let previous = if index == 0 {
                self.previous
            } else {
                input[index - 1]
            };
            output.push(previous + (current - previous) * fraction as f32);
            position += ratio;
        }

        // Save the phase relative to the next buffer and the last input sample
        // so interpolation is continuous across the boundary.
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

impl Default for MonoResampler {
    fn default() -> Self {
        Self::new()
    }
}
