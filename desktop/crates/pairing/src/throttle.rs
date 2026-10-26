use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Mutex, PoisonError};

use remotecam_session_core::Timestamp;

pub const ATTEMPT_LIMIT: usize = 5;
pub const ATTEMPT_WINDOW_MILLIS: u64 = 60_000;
pub const FIRST_BLOCK_MILLIS: u64 = 60_000;
pub const MAX_BLOCK_MILLIS: u64 = 900_000;
pub const QUIET_RESET_MILLIS: u64 = 1_800_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrottlePolicy {
    pub attempt_limit: usize,
    pub window_millis: u64,
    pub first_block_millis: u64,
    pub max_block_millis: u64,
    pub quiet_reset_millis: u64,
}

impl Default for ThrottlePolicy {
    fn default() -> Self {
        Self {
            attempt_limit: ATTEMPT_LIMIT,
            window_millis: ATTEMPT_WINDOW_MILLIS,
            first_block_millis: FIRST_BLOCK_MILLIS,
            max_block_millis: MAX_BLOCK_MILLIS,
            quiet_reset_millis: QUIET_RESET_MILLIS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Admitted,
    Refused { blocked_for_millis: u64 },
}

#[derive(Debug, Default)]
struct OriginState {
    attempts: VecDeque<u64>,
    blocked_until: u64,
    blocks: u32,
}

impl OriginState {
    fn is_idle(&self, now: u64, policy: &ThrottlePolicy) -> bool {
        let last_attempt = self.attempts.back().copied().unwrap_or(0);
        now.saturating_sub(self.blocked_until.max(last_attempt)) >= policy.quiet_reset_millis
    }

    fn next_block_millis(&self, policy: &ThrottlePolicy) -> u64 {
        policy
            .first_block_millis
            .saturating_mul(1_u64.checked_shl(self.blocks).unwrap_or(u64::MAX))
            .min(policy.max_block_millis)
    }
}

#[derive(Debug, Default)]
pub struct OriginThrottle {
    policy: ThrottlePolicy,
    origins: Mutex<HashMap<IpAddr, OriginState>>,
}

impl OriginThrottle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_policy(policy: ThrottlePolicy) -> Self {
        Self {
            policy,
            origins: Mutex::default(),
        }
    }

    pub fn admit(&self, origin: IpAddr, now: Timestamp) -> Admission {
        let policy = self.policy;
        let now = now.as_millis();
        let mut origins = self.origins.lock().unwrap_or_else(PoisonError::into_inner);
        origins.retain(|key, state| *key == origin || !state.is_idle(now, &policy));
        let state = origins.entry(origin).or_default();
        if state.is_idle(now, &policy) {
            *state = OriginState::default();
        }

        if now < state.blocked_until {
            return Admission::Refused {
                blocked_for_millis: state.blocked_until - now,
            };
        }
        while state
            .attempts
            .front()
            .is_some_and(|&at| now.saturating_sub(at) >= policy.window_millis)
        {
            state.attempts.pop_front();
        }
        if state.attempts.len() >= policy.attempt_limit {
            state.attempts.clear();
            let blocked_for_millis = state.next_block_millis(&policy);
            state.blocks = state.blocks.saturating_add(1);
            state.blocked_until = now.saturating_add(blocked_for_millis);
            return Admission::Refused { blocked_for_millis };
        }
        state.attempts.push_back(now);
        Admission::Admitted
    }
}
