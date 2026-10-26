use remotecam_pairing::throttle::{OriginThrottle, ThrottlePolicy};

pub fn unlimited_attempts() -> OriginThrottle {
    OriginThrottle::with_policy(ThrottlePolicy {
        attempt_limit: usize::MAX,
        ..ThrottlePolicy::default()
    })
}
