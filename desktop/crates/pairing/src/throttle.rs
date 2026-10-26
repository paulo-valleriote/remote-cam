use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv6Addr};
use std::sync::{Mutex, PoisonError};

use remotecam_session_core::Timestamp;

pub const ATTEMPT_LIMIT: usize = 5;
pub const ATTEMPT_WINDOW_MILLIS: u64 = 60_000;
pub const FIRST_BLOCK_MILLIS: u64 = 60_000;
pub const MAX_BLOCK_MILLIS: u64 = 900_000;
pub const QUIET_RESET_MILLIS: u64 = 1_800_000;
pub const MAX_TRACKED_ORIGINS: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrottlePolicy {
    pub attempt_limit: usize,
    pub window_millis: u64,
    pub first_block_millis: u64,
    pub max_block_millis: u64,
    pub quiet_reset_millis: u64,
    pub max_origins: usize,
}

impl Default for ThrottlePolicy {
    fn default() -> Self {
        Self {
            attempt_limit: ATTEMPT_LIMIT,
            window_millis: ATTEMPT_WINDOW_MILLIS,
            first_block_millis: FIRST_BLOCK_MILLIS,
            max_block_millis: MAX_BLOCK_MILLIS,
            quiet_reset_millis: QUIET_RESET_MILLIS,
            max_origins: MAX_TRACKED_ORIGINS,
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
    last_seen: u64,
}

impl OriginState {
    fn is_idle(&self, now: u64, policy: &ThrottlePolicy) -> bool {
        now.saturating_sub(self.blocked_until.max(self.last_seen)) >= policy.quiet_reset_millis
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

    pub fn tracked_origins(&self) -> usize {
        self.origins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    pub fn admit(&self, origin: IpAddr, now: Timestamp) -> Admission {
        let policy = self.policy;
        let now = now.as_millis();
        let mut origins = self.origins.lock().unwrap_or_else(PoisonError::into_inner);
        let key = origin_key(origin);
        if !origins.contains_key(&key) && origins.len() >= policy.max_origins {
            make_room(&mut origins, now, &policy);
        }
        let state = origins.entry(key).or_default();
        if state.is_idle(now, &policy) {
            *state = OriginState::default();
        }
        state.last_seen = now;

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

fn origin_key(origin: IpAddr) -> IpAddr {
    match origin {
        IpAddr::V4(_) => origin,
        IpAddr::V6(address) => match address.to_ipv4_mapped() {
            Some(mapped) => IpAddr::V4(mapped),
            None => {
                let mut segments = address.segments();
                segments[4..].fill(0);
                IpAddr::V6(Ipv6Addr::from(segments))
            }
        },
    }
}

fn make_room(origins: &mut HashMap<IpAddr, OriginState>, now: u64, policy: &ThrottlePolicy) {
    origins.retain(|_, state| !state.is_idle(now, policy));
    if origins.len() < policy.max_origins {
        return;
    }
    let victim = origins
        .iter()
        .min_by_key(|(_, state)| (state.blocked_until > now, state.last_seen))
        .map(|(key, _)| *key);
    if let Some(victim) = victim {
        origins.remove(&victim);
    }
}
