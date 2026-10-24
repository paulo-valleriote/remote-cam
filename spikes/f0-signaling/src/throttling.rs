//! Per-origin pairing throttling.
//!
//! The attempt limit protects the listener against token brute force. It is
//! scoped to the origin address on purpose: a global limit would let one hostile
//! origin deny pairing to the legitimate user, which is a denial-of-service bug
//! rather than a security control.
//!
//! The thresholds are fixed by the contract: five attempts per minute, a
//! progressive block that strictly increases, and listener shutdown after ten
//! consecutive failures.

use std::collections::{HashMap, VecDeque};
use std::fmt;

use remotecam_protocol::ReasonCode;

/// At most this many attempts are allowed per origin in one window.
pub const ATTEMPT_LIMIT_PER_MINUTE: u32 = 5;
/// The observation window for the attempt limit.
pub const ATTEMPT_WINDOW_MS: u64 = 60_000;
/// The listener closes after this many consecutive failures.
pub const CONSECUTIVE_FAILURE_LIMIT: u32 = 10;
/// The first progressive block, doubled on every further violation.
pub const BASE_BLOCK_MS: u64 = 60_000;

/// What the throttler did with one attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleAction {
    /// The attempt proceeded to token validation.
    Allowed,
    /// The attempt was refused because the per-minute limit was exceeded.
    RateLimited,
    /// The attempt was refused because the origin is inside a progressive block.
    Blocked,
    /// The pairing listener no longer accepts connections.
    ListenerClosed,
}

impl ThrottleAction {
    /// The stable spelling used by the evidence file.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::RateLimited => "rate_limited",
            Self::Blocked => "blocked",
            Self::ListenerClosed => "listener_closed",
        }
    }
}

impl fmt::Display for ThrottleAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One recorded throttling decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThrottleDecision {
    /// Order of the attempt within the scenario.
    pub sequence: u32,
    /// Milliseconds since the start of the scenario.
    pub at_ms: u64,
    /// The origin address the decision applied to.
    pub origin_id: String,
    /// What was decided.
    pub action: ThrottleAction,
    /// The closed reason code; `None` only for an allowed attempt.
    pub reason_code: Option<ReasonCode>,
    /// What was observed to happen.
    pub outcome: String,
}

/// The result of asking the throttler whether an attempt may proceed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptPermit {
    /// The attempt may proceed.
    Allowed {
        /// The attempt's sequence number.
        sequence: u32,
    },
    /// The attempt is refused; the decision carries the reason.
    Refused(ThrottleDecision),
}

#[derive(Debug, Default)]
struct OriginState {
    attempts: VecDeque<u64>,
    consecutive_failures: u32,
    block_count: u32,
    blocked_until_ms: Option<u64>,
}

impl OriginState {
    fn prune(&mut self, now_ms: u64) {
        while let Some(oldest) = self.attempts.front() {
            if now_ms.saturating_sub(*oldest) >= ATTEMPT_WINDOW_MS {
                self.attempts.pop_front();
            } else {
                break;
            }
        }
    }

    fn next_block_ms(&mut self) -> u64 {
        self.block_count = self.block_count.saturating_add(1);
        let shift = self.block_count.saturating_sub(1).min(6);
        BASE_BLOCK_MS.saturating_mul(1_u64 << shift)
    }
}

/// The per-origin throttler used by the pairing listener.
#[derive(Debug, Default)]
pub struct Throttling {
    origins: HashMap<String, OriginState>,
    decisions: Vec<ThrottleDecision>,
    block_durations: Vec<u64>,
    next_sequence: u32,
    listener_closed: bool,
    security_event: bool,
}

impl Throttling {
    /// Creates an open listener with no recorded attempts.
    pub fn new() -> Self {
        Self {
            next_sequence: 1,
            ..Self::default()
        }
    }

    /// Decides whether an attempt from `origin_id` may proceed at `now_ms`.
    pub fn begin_attempt(&mut self, origin_id: &str, now_ms: u64) -> AttemptPermit {
        if self.listener_closed {
            let sequence = self.next_sequence;
            let decision = ThrottleDecision {
                sequence,
                at_ms: now_ms,
                origin_id: origin_id.to_owned(),
                action: ThrottleAction::ListenerClosed,
                reason_code: Some(ReasonCode::ListenerClosed),
                outcome: "connection refused because the pairing listener is closed".to_owned(),
            };
            self.next_sequence = self.next_sequence.saturating_add(1);
            self.decisions.push(decision.clone());
            return AttemptPermit::Refused(decision);
        }

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let state = self.origins.entry(origin_id.to_owned()).or_default();
        state.prune(now_ms);

        if state.blocked_until_ms.is_some_and(|until| now_ms < until) {
            let until = state.blocked_until_ms.unwrap_or(now_ms);
            let decision = ThrottleDecision {
                sequence,
                at_ms: now_ms,
                origin_id: origin_id.to_owned(),
                action: ThrottleAction::Blocked,
                reason_code: Some(ReasonCode::RateLimited),
                outcome: format!(
                    "origin blocked for another {} s",
                    until.saturating_sub(now_ms) / 1000
                ),
            };
            self.decisions.push(decision.clone());
            return AttemptPermit::Refused(decision);
        }

        if state.attempts.len() >= ATTEMPT_LIMIT_PER_MINUTE as usize {
            let block_ms = state.next_block_ms();
            let blocked_until = now_ms.saturating_add(block_ms);
            state.blocked_until_ms = Some(blocked_until);
            self.block_durations.push(block_ms);
            let decision = ThrottleDecision {
                sequence,
                at_ms: now_ms,
                origin_id: origin_id.to_owned(),
                action: ThrottleAction::RateLimited,
                reason_code: Some(ReasonCode::RateLimited),
                outcome: format!(
                    "attempt {} refused; origin blocked for {} s",
                    sequence,
                    block_ms / 1000
                ),
            };
            self.decisions.push(decision.clone());
            return AttemptPermit::Refused(decision);
        }

        state.attempts.push_back(now_ms);
        let decision = ThrottleDecision {
            sequence,
            at_ms: now_ms,
            origin_id: origin_id.to_owned(),
            action: ThrottleAction::Allowed,
            reason_code: None,
            outcome: "attempt allowed".to_owned(),
        };
        self.decisions.push(decision);
        AttemptPermit::Allowed { sequence }
    }

    /// Records a failed attempt. Returns the closure decision at the threshold.
    pub fn record_failure(&mut self, origin_id: &str, now_ms: u64) -> Option<ThrottleDecision> {
        let state = self.origins.entry(origin_id.to_owned()).or_default();
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        if state.consecutive_failures < CONSECUTIVE_FAILURE_LIMIT {
            return None;
        }
        self.listener_closed = true;
        self.security_event = true;
        let decision = ThrottleDecision {
            sequence: self.next_sequence.saturating_sub(1),
            at_ms: now_ms,
            origin_id: origin_id.to_owned(),
            action: ThrottleAction::ListenerClosed,
            reason_code: Some(ReasonCode::ListenerClosed),
            outcome: format!(
                "pairing listener closed after {CONSECUTIVE_FAILURE_LIMIT} consecutive failures"
            ),
        };
        self.decisions.push(decision.clone());
        Some(decision)
    }

    /// Records a successful pairing. Success resets the consecutive count but
    /// does not undo a progressive block.
    pub fn record_success(&mut self, origin_id: &str) {
        if let Some(state) = self.origins.get_mut(origin_id) {
            state.consecutive_failures = 0;
        }
    }

    /// Whether the listener has been closed by the consecutive-failure rule.
    pub fn listener_closed(&self) -> bool {
        self.listener_closed
    }

    /// Whether the closure was recorded as a security event.
    pub fn security_event(&self) -> bool {
        self.security_event
    }

    /// Every recorded decision, in attempt order.
    pub fn decisions(&self) -> &[ThrottleDecision] {
        &self.decisions
    }

    /// The progressive block durations observed, in order.
    pub fn block_durations_ms(&self) -> Vec<u64> {
        self.block_durations.clone()
    }

    /// The consecutive failure count recorded for `origin_id`.
    pub fn consecutive_failures(&self, origin_id: &str) -> u32 {
        self.origins
            .get(origin_id)
            .map_or(0, |state| state.consecutive_failures)
    }
}
