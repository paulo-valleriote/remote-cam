//! Adaptive jitter-buffer policy (T068).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! `webrtc` 0.21.1 provides packet ordering and a fixed playout delay, but it
//! does not ship NetEq or any adaptive policy. This module is the policy layer
//! that sits around that mechanism: it derives a target depth from the RFC 3550
//! interarrival-jitter estimate, moves the playout delay towards the target,
//! fast-forwards after a stall, and keeps the buffer depth bounded.
//!
//! The policy is deterministic and takes the current time as an argument, so the
//! tests drive it with an injected clock and never sleep.

/// Gain of the RFC 3550 §6.4.1 interarrival-jitter estimator.
pub const RFC3550_JITTER_GAIN: f64 = 1.0 / 16.0;

/// Maximum number of frames the policy will keep pending.
pub const DEFAULT_MAX_DEPTH: usize = 120;

/// Tunables for [`JitterPolicy`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JitterPolicyConfig {
    /// Lowest target delay the policy may choose.
    pub min_target_ms: f64,
    /// Highest target delay the policy may choose.
    pub max_target_ms: f64,
    /// Target delay used before any packet has been observed.
    pub initial_target_ms: f64,
    /// Largest number of frames kept pending.
    pub max_depth: usize,
    /// A gap longer than this between packets is a stall.
    pub stall_threshold_ms: f64,
    /// Largest playout-delay change applied by one [`JitterPolicy::tick`].
    pub playout_step_ms: f64,
    /// Jitter is multiplied by this factor when deriving the target depth.
    pub jitter_safety_factor: f64,
}

impl Default for JitterPolicyConfig {
    fn default() -> Self {
        Self {
            min_target_ms: 20.0,
            max_target_ms: 250.0,
            initial_target_ms: 50.0,
            max_depth: DEFAULT_MAX_DEPTH,
            stall_threshold_ms: 500.0,
            playout_step_ms: 5.0,
            jitter_safety_factor: 4.0,
        }
    }
}

/// One received RTP packet, in the policy's own clock domain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PacketTiming {
    /// Arrival instant, milliseconds.
    pub arrival_ms: f64,
    /// RTP timestamp converted to milliseconds.
    pub rtp_timestamp_ms: f64,
}

/// What the policy did with a packet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PacketDecision {
    pub target_delay_ms: f64,
    pub playout_delay_ms: f64,
    pub buffered: usize,
    /// The buffer was full and the oldest pending frame was dropped.
    pub dropped_oldest: bool,
}

/// State of the consumer after a clock tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// Nothing is buffered.
    Idle,
    /// Frames are pending and playout is progressing.
    Playing,
    /// A stall was detected; pending frames were discarded and the delay reset.
    FastForward,
}

/// Result of advancing the policy's clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickDecision {
    pub state: PlaybackState,
    pub target_delay_ms: f64,
    pub playout_delay_ms: f64,
    pub buffered: usize,
    /// Frames discarded by this tick (stall or depth cap).
    pub discarded: usize,
}

/// Counters exposed as evidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JitterStats {
    pub received: u64,
    pub consumed: u64,
    pub discarded: u64,
    pub fast_forwards: u64,
    pub jitter_ms: f64,
    pub target_delay_ms: f64,
    pub playout_delay_ms: f64,
    pub buffered: usize,
}

/// Adaptive playout policy for one media stream.
#[derive(Debug, Clone)]
pub struct JitterPolicy {
    config: JitterPolicyConfig,
    jitter_ms: f64,
    target_delay_ms: f64,
    playout_delay_ms: f64,
    buffered: usize,
    last_arrival_ms: Option<f64>,
    last_transit_ms: Option<f64>,
    received: u64,
    consumed: u64,
    discarded: u64,
    fast_forwards: u64,
}

impl JitterPolicy {
    pub fn new(config: JitterPolicyConfig) -> Self {
        let initial = config
            .initial_target_ms
            .clamp(config.min_target_ms, config.max_target_ms);
        Self {
            config,
            jitter_ms: 0.0,
            target_delay_ms: initial,
            playout_delay_ms: initial,
            buffered: 0,
            last_arrival_ms: None,
            last_transit_ms: None,
            received: 0,
            consumed: 0,
            discarded: 0,
            fast_forwards: 0,
        }
    }

    pub fn config(&self) -> JitterPolicyConfig {
        self.config
    }

    pub fn stats(&self) -> JitterStats {
        JitterStats {
            received: self.received,
            consumed: self.consumed,
            discarded: self.discarded,
            fast_forwards: self.fast_forwards,
            jitter_ms: self.jitter_ms,
            target_delay_ms: self.target_delay_ms,
            playout_delay_ms: self.playout_delay_ms,
            buffered: self.buffered,
        }
    }

    /// Account for one received packet and adapt the target delay.
    ///
    /// Adapting upwards is fast (one half of the remaining distance) and
    /// adapting downwards is slow (5 %), so a transient jitter spike raises the
    /// delay immediately while a quiet period does not make the buffer chatter.
    pub fn on_packet(&mut self, timing: PacketTiming) -> PacketDecision {
        self.received += 1;

        // RFC 3550 interarrival jitter: D(i-1,i) is the difference between two
        // transit times, and the estimate follows it with a 1/16 gain.
        let transit = timing.arrival_ms - timing.rtp_timestamp_ms;
        if let Some(previous) = self.last_transit_ms {
            let delta = (transit - previous).abs();
            self.jitter_ms += (delta - self.jitter_ms) * RFC3550_JITTER_GAIN;
        }
        self.last_transit_ms = Some(transit);
        self.last_arrival_ms = Some(timing.arrival_ms);

        let desired = (self.config.initial_target_ms
            + self.jitter_ms * self.config.jitter_safety_factor)
            .clamp(self.config.min_target_ms, self.config.max_target_ms);
        if desired > self.target_delay_ms {
            self.target_delay_ms += (desired - self.target_delay_ms) * 0.5;
        } else {
            self.target_delay_ms += (desired - self.target_delay_ms) * 0.05;
        }
        self.target_delay_ms = self
            .target_delay_ms
            .clamp(self.config.min_target_ms, self.config.max_target_ms);

        self.buffered += 1;
        let mut dropped_oldest = false;
        if self.buffered > self.config.max_depth {
            self.buffered = self.config.max_depth;
            self.discarded += 1;
            dropped_oldest = true;
        }

        PacketDecision {
            target_delay_ms: self.target_delay_ms,
            playout_delay_ms: self.playout_delay_ms,
            buffered: self.buffered,
            dropped_oldest,
        }
    }

    /// Advance the playout clock.
    ///
    /// A stall is a gap longer than `stall_threshold_ms` while frames are
    /// pending. The policy then discards the backlog and resets the playout
    /// delay to the target, which is what lets the session return to baseline
    /// latency instead of playing out stale frames for seconds.
    pub fn tick(&mut self, now_ms: f64) -> TickDecision {
        if self.buffered == 0 {
            self.playout_delay_ms = self.target_delay_ms;
            return TickDecision {
                state: PlaybackState::Idle,
                target_delay_ms: self.target_delay_ms,
                playout_delay_ms: self.playout_delay_ms,
                buffered: 0,
                discarded: 0,
            };
        }

        if let Some(last) = self.last_arrival_ms
            && now_ms - last > self.config.stall_threshold_ms
        {
            let discarded = self.buffered;
            self.buffered = 0;
            self.discarded += discarded as u64;
            self.fast_forwards += 1;
            self.playout_delay_ms = self.target_delay_ms;
            return TickDecision {
                state: PlaybackState::FastForward,
                target_delay_ms: self.target_delay_ms,
                playout_delay_ms: self.playout_delay_ms,
                buffered: 0,
                discarded,
            };
        }

        let step = (self.target_delay_ms - self.playout_delay_ms)
            .clamp(-self.config.playout_step_ms, self.config.playout_step_ms);
        self.playout_delay_ms = (self.playout_delay_ms + step).max(0.0);

        TickDecision {
            state: PlaybackState::Playing,
            target_delay_ms: self.target_delay_ms,
            playout_delay_ms: self.playout_delay_ms,
            buffered: self.buffered,
            discarded: 0,
        }
    }

    /// Remove one frame from the buffer for presentation.
    pub fn consume(&mut self) -> bool {
        if self.buffered == 0 {
            return false;
        }
        self.buffered -= 1;
        self.consumed += 1;
        if self.buffered == 0 {
            self.playout_delay_ms = self.target_delay_ms;
        }
        true
    }
}

impl Default for JitterPolicy {
    fn default() -> Self {
        Self::new(JitterPolicyConfig::default())
    }
}
