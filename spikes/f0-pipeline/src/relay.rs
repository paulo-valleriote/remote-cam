//! Seeded, RTP-aware impairment relay (T075).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The relay parses enough RTP to know what is media and what is not. ICE,
//! DTLS, STUN and RTCP always pass through untouched: impairing them would
//! break the connection and present as a product defect rather than as media
//! loss. Media packets are dropped, delayed or held by a seeded PRNG so that a
//! second engineer reproduces the exact pattern from the recorded seed.
//!
//! There is **one** profile: 3 % uniform loss, 30 ms added latency, ±10 ms
//! jitter, media only (`nominal-3pct-30ms-10ms`). Burst loss is a deviation and
//! cannot pass a gate.

use std::fmt::Write as _;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use serde::{Deserialize, Serialize};

/// The only profile admitted for a loss-and-jitter gate run.
pub const PROFILE_NAME: &str = "nominal-3pct-30ms-10ms";
pub const PROFILE_LOSS_PCT: f64 = 3.0;
pub const PROFILE_ADDED_LATENCY_MS: f64 = 30.0;
pub const PROFILE_JITTER_MS: f64 = 10.0;
pub const SEED: u64 = 42;

/// Configuration of the relay for one run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ImpairmentConfig {
    pub loss_pct: f64,
    pub loss_distribution: LossDistribution,
    pub added_latency_ms: f64,
    pub jitter_ms: f64,
    pub seed: u64,
    pub stall_at_ms: Option<f64>,
    pub stall_ms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LossDistribution {
    Uniform,
    Burst,
}

impl ImpairmentConfig {
    /// The single pinned profile, with an optional stall for the stall scenario.
    pub fn pinned(stall_at_ms: Option<f64>, stall_ms: f64) -> Self {
        Self {
            loss_pct: PROFILE_LOSS_PCT,
            loss_distribution: LossDistribution::Uniform,
            added_latency_ms: PROFILE_ADDED_LATENCY_MS,
            jitter_ms: PROFILE_JITTER_MS,
            seed: SEED,
            stall_at_ms,
            stall_ms,
        }
    }

    /// A run that deviates from the profile may not support a gate verdict.
    pub fn is_pinned_profile(&self) -> bool {
        (self.loss_pct - PROFILE_LOSS_PCT).abs() < 1e-9
            && self.loss_distribution == LossDistribution::Uniform
            && (self.added_latency_ms - PROFILE_ADDED_LATENCY_MS).abs() < 1e-9
            && (self.jitter_ms - PROFILE_JITTER_MS).abs() < 1e-9
    }

    /// Contract file for the run.
    pub fn profile_document(&self, relay_log_ref: &str) -> serde_json::Value {
        serde_json::json!({
            "profile_name": PROFILE_NAME,
            "loss_pct": self.loss_pct,
            "loss_distribution": self.loss_distribution,
            "added_latency_ms": self.added_latency_ms,
            "jitter_ms": self.jitter_ms,
            "applies_to": "media_only",
            "seed": self.seed,
            "relay_log_ref": relay_log_ref,
            "deviation_label": if self.is_pinned_profile() { serde_json::Value::Null } else { serde_json::Value::String("profile-deviation".to_owned()) },
        })
    }
}

/// A parsed RTP header, without allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpInfo {
    pub payload_type: u8,
    pub sequence: u16,
    pub ssrc: u32,
}

/// Classification of one datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Traffic {
    Media(RtpInfo),
    /// ICE/STUN, DTLS, RTCP or anything the relay does not recognise.
    Control,
}

/// Classify one UDP payload.
///
/// RTP has version 2 in the top two bits. RTCP shares the version field, so the
/// packet-type byte distinguishes it: RTCP packet types are 192–223, outside the
/// 7-bit RTP payload-type range.
pub fn classify(packet: &[u8]) -> Traffic {
    if packet.len() < 12 || packet[0] >> 6 != 2 {
        return Traffic::Control;
    }
    let byte = packet[1];
    if (192..=223).contains(&byte) {
        return Traffic::Control;
    }
    Traffic::Media(RtpInfo {
        payload_type: byte & 0x7F,
        sequence: u16::from_be_bytes([packet[2], packet[3]]),
        ssrc: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
    })
}

/// What the relay did with a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Pass,
    Drop,
    Delay,
    Hold,
    StallRelease,
}

/// One line of `relay_log.jsonl`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RelayDecision {
    pub t_ms: f64,
    pub ssrc: u32,
    pub payload_type: u8,
    pub seq: u16,
    pub action: Action,
    pub delay_ms: f64,
}

/// Deterministic decision core, independent of sockets and wall clock.
pub struct RelayCore {
    config: ImpairmentConfig,
    rng: StdRng,
    held: Vec<RelayDecision>,
    media_seen: u64,
    media_dropped: u64,
}

impl RelayCore {
    pub fn new(config: ImpairmentConfig) -> Self {
        Self {
            rng: StdRng::seed_from_u64(config.seed),
            config,
            held: Vec::new(),
            media_seen: 0,
            media_dropped: 0,
        }
    }

    pub fn config(&self) -> ImpairmentConfig {
        self.config
    }

    /// Decide what happens to one datagram at `now_ms`.
    ///
    /// Control traffic returns `None` and is never logged, which is what keeps
    /// the log free of anything but media SSRCs.
    pub fn decide(&mut self, now_ms: f64, packet: &[u8]) -> Option<RelayDecision> {
        let Traffic::Media(info) = classify(packet) else {
            return None;
        };
        self.media_seen += 1;

        let mut decision = RelayDecision {
            t_ms: now_ms,
            ssrc: info.ssrc,
            payload_type: info.payload_type,
            seq: info.sequence,
            action: Action::Pass,
            delay_ms: 0.0,
        };

        if let Some(stall_at) = self.config.stall_at_ms
            && now_ms >= stall_at
            && now_ms < stall_at + self.config.stall_ms
        {
            decision.action = Action::Hold;
            self.held.push(decision);
            return Some(decision);
        }

        if let Some(stall_at) = self.config.stall_at_ms
            && now_ms >= stall_at + self.config.stall_ms
            && !self.held.is_empty()
        {
            // The relay forwards every datagram it held when it sees this
            // decision; the log records one release, not one line per packet.
            self.held.clear();
            decision.action = Action::StallRelease;
            decision.delay_ms = self.config.stall_ms;
            return Some(decision);
        }

        if self
            .rng
            .random_bool((self.config.loss_pct / 100.0).clamp(0.0, 1.0))
        {
            decision.action = Action::Drop;
            self.media_dropped += 1;
            return Some(decision);
        }

        if self.config.added_latency_ms > 0.0 || self.config.jitter_ms > 0.0 {
            let jitter = if self.config.jitter_ms > 0.0 {
                self.rng
                    .random_range(-self.config.jitter_ms..=self.config.jitter_ms)
            } else {
                0.0
            };
            decision.action = Action::Delay;
            decision.delay_ms = (self.config.added_latency_ms + jitter).max(0.0);
        }

        Some(decision)
    }

    pub fn media_seen(&self) -> u64 {
        self.media_seen
    }

    pub fn media_dropped(&self) -> u64 {
        self.media_dropped
    }

    /// Realised loss from the relay's own decisions, never from configuration.
    pub fn realised_loss_pct(&self) -> f64 {
        if self.media_seen == 0 {
            0.0
        } else {
            100.0 * self.media_dropped as f64 / self.media_seen as f64
        }
    }
}

/// Render decisions as the JSONL contract.
pub fn relay_log_jsonl(decisions: &[RelayDecision]) -> String {
    let mut output = String::new();
    for decision in decisions {
        let line = serde_json::to_string(decision).unwrap_or_else(|_| "{}".to_owned());
        let _ = writeln!(output, "{line}");
    }
    output
}

/// Read the media decisions from a JSONL log.
pub fn parse_relay_log(jsonl: &str) -> Vec<RelayDecision> {
    jsonl
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}
