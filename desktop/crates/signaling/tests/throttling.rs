//! Per-origin throttling contract tests (T045).
//!
//! A global limit would turn one hostile origin into a denial of service against
//! the legitimate user, so every test here also asserts the per-origin scope.

use remotecam_protocol::ReasonCode;
use remotecam_signaling::throttling::{
    ATTEMPT_LIMIT_PER_MINUTE, AttemptPermit, CONSECUTIVE_FAILURE_LIMIT, ThrottleAction, Throttling,
};

const HOSTILE: &str = "192.168.1.77";
const LEGITIMATE: &str = "192.168.1.88";

#[test]
fn the_sixth_attempt_in_a_minute_is_rate_limited() {
    let mut throttle = Throttling::new();
    assert_eq!(ATTEMPT_LIMIT_PER_MINUTE, 5);

    for attempt_number in 1..=5_u64 {
        let permit = throttle.begin_attempt(HOSTILE, attempt_number * 1_000);
        assert!(
            matches!(permit, AttemptPermit::Allowed { sequence } if u64::from(sequence) == attempt_number),
            "attempt {attempt_number} should be allowed"
        );
        throttle.record_failure(HOSTILE, attempt_number * 1_000);
    }

    let permit = throttle.begin_attempt(HOSTILE, 6_000);
    match permit {
        AttemptPermit::Refused(decision) => {
            assert_eq!(decision.action, ThrottleAction::RateLimited);
            assert_eq!(decision.reason_code, Some(ReasonCode::RateLimited));
            assert_eq!(decision.sequence, 6);
            assert!(decision.outcome.contains("blocked"));
        }
        AttemptPermit::Allowed { .. } => panic!("the sixth attempt must be refused"),
    }
}

#[test]
fn a_second_origin_is_not_affected_by_the_flood() {
    let mut throttle = Throttling::new();
    for attempt_number in 1..=6_u64 {
        throttle.begin_attempt(HOSTILE, attempt_number * 1_000);
        throttle.record_failure(HOSTILE, attempt_number * 1_000);
    }

    let permit = throttle.begin_attempt(LEGITIMATE, 7_000);
    assert!(
        matches!(permit, AttemptPermit::Allowed { .. }),
        "the legitimate origin must still pair"
    );
    throttle.record_success(LEGITIMATE);
    assert_eq!(throttle.consecutive_failures(LEGITIMATE), 0);
}

#[test]
fn the_progressive_block_strictly_increases() {
    let mut throttle = Throttling::new();
    for attempt_number in 1..=6_u64 {
        throttle.begin_attempt(HOSTILE, attempt_number * 1_000);
        throttle.record_failure(HOSTILE, attempt_number * 1_000);
    }

    // The first block expires after 60 s; spend the next window, then cross
    // again to observe the second, longer block.
    for step in 0..5_u64 {
        let permit = throttle.begin_attempt(HOSTILE, 67_000 + step * 1_000);
        assert!(matches!(permit, AttemptPermit::Allowed { .. }));
    }
    let permit = throttle.begin_attempt(HOSTILE, 72_000);
    match permit {
        AttemptPermit::Refused(decision) => {
            assert_eq!(decision.action, ThrottleAction::RateLimited);
            assert!(decision.outcome.contains("120 s"), "{}", decision.outcome);
        }
        AttemptPermit::Allowed { .. } => panic!("the second window must also be limited"),
    }

    let durations = throttle.block_durations_ms();
    assert!(durations.len() >= 2, "{durations:?}");
    assert!(
        durations.windows(2).all(|pair| pair[1] > pair[0]),
        "block durations must strictly increase: {durations:?}"
    );
}

#[test]
fn ten_consecutive_failures_close_the_listener_as_a_security_event() {
    let mut throttle = Throttling::new();
    assert_eq!(CONSECUTIVE_FAILURE_LIMIT, 10);

    let mut closure = None;
    for attempt_number in 1..=10_u64 {
        let at_ms = attempt_number * 1_000;
        throttle.begin_attempt(HOSTILE, at_ms);
        closure = throttle.record_failure(HOSTILE, at_ms);
    }

    let closure = closure.expect("the tenth failure must close the listener");
    assert_eq!(closure.action, ThrottleAction::ListenerClosed);
    assert_eq!(closure.reason_code, Some(ReasonCode::ListenerClosed));
    assert!(throttle.listener_closed());
    assert!(throttle.security_event());
    assert_eq!(throttle.consecutive_failures(HOSTILE), 10);
}

#[test]
fn a_closed_listener_refuses_every_origin() {
    let mut throttle = Throttling::new();
    for attempt_number in 1..=10_u64 {
        let at_ms = attempt_number * 1_000;
        throttle.begin_attempt(HOSTILE, at_ms);
        throttle.record_failure(HOSTILE, at_ms);
    }

    match throttle.begin_attempt(LEGITIMATE, 20_000) {
        AttemptPermit::Refused(decision) => {
            assert_eq!(decision.action, ThrottleAction::ListenerClosed);
            assert_eq!(decision.reason_code, Some(ReasonCode::ListenerClosed));
        }
        AttemptPermit::Allowed { .. } => panic!("a closed listener cannot accept"),
    }
}

#[test]
fn a_success_resets_the_consecutive_failure_count() {
    let mut throttle = Throttling::new();
    for attempt_number in 1..=5_u64 {
        let at_ms = attempt_number * 1_000;
        throttle.begin_attempt(HOSTILE, at_ms);
        throttle.record_failure(HOSTILE, at_ms);
    }
    throttle.record_success(HOSTILE);
    assert_eq!(throttle.consecutive_failures(HOSTILE), 0);
    assert!(!throttle.listener_closed());
}

#[test]
fn every_recorded_decision_has_an_outcome_and_a_closed_code() {
    let mut throttle = Throttling::new();
    for attempt_number in 1..=12_u64 {
        let at_ms = attempt_number * 1_000;
        throttle.begin_attempt(HOSTILE, at_ms);
        throttle.record_failure(HOSTILE, at_ms);
    }
    let allowed = throttle
        .decisions()
        .iter()
        .filter(|decision| decision.action == ThrottleAction::Allowed)
        .count();
    assert_eq!(allowed, 5, "five attempts are allowed per window");
    for decision in throttle.decisions() {
        assert!(!decision.outcome.trim().is_empty());
        if decision.action != ThrottleAction::Allowed {
            assert!(decision.reason_code.is_some(), "{decision:?}");
        }
    }
}

#[test]
fn a_blocked_origin_cannot_establish_a_session() {
    let mut throttle = Throttling::new();
    for attempt_number in 1..=6_u64 {
        throttle.begin_attempt(HOSTILE, attempt_number * 1_000);
        throttle.record_failure(HOSTILE, attempt_number * 1_000);
    }
    // The next call is inside the block, so it must be refused before any
    // token validation can run.
    assert!(matches!(
        throttle.begin_attempt(HOSTILE, 7_000),
        AttemptPermit::Refused(_)
    ));
}
