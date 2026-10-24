//! T061 — adaptive jitter policy under an injected clock.

use remotecam_media_core::jitter_policy::{
    JitterPolicy, JitterPolicyConfig, PacketTiming, PlaybackState,
};

fn config() -> JitterPolicyConfig {
    JitterPolicyConfig {
        min_target_ms: 20.0,
        max_target_ms: 200.0,
        initial_target_ms: 40.0,
        max_depth: 10,
        stall_threshold_ms: 500.0,
        playout_step_ms: 5.0,
        jitter_safety_factor: 4.0,
    }
}

/// Feed packets whose RTP clock advances by 33 ms but whose arrival times grow
/// increasingly erratic.
fn feed_with_jitter(policy: &mut JitterPolicy, packets: usize, jitter_amplitude_ms: f64) {
    for index in 0..packets {
        let rtp = index as f64 * 33.0;
        let wobble = if index % 2 == 0 {
            jitter_amplitude_ms
        } else {
            -jitter_amplitude_ms
        };
        policy.on_packet(PacketTiming {
            arrival_ms: rtp + wobble,
            rtp_timestamp_ms: rtp,
        });
    }
}

#[test]
fn target_depth_increases_when_jitter_increases() {
    let mut policy = JitterPolicy::new(config());
    feed_with_jitter(&mut policy, 2, 0.0);
    let calm = policy.stats().target_delay_ms;

    for _ in 0..40 {
        feed_with_jitter(&mut policy, 2, 8.0);
    }
    let jittered = policy.stats().target_delay_ms;

    assert!(
        jittered > calm,
        "target depth must rise with measured jitter ({calm} -> {jittered})"
    );
    assert!(jittered <= config().max_target_ms);
}

#[test]
fn quiet_period_lowers_the_target_slowly() {
    let mut policy = JitterPolicy::new(config());
    for _ in 0..40 {
        feed_with_jitter(&mut policy, 2, 8.0);
    }
    let elevated = policy.stats().target_delay_ms;

    for _ in 0..40 {
        feed_with_jitter(&mut policy, 2, 0.0);
    }
    let recovered = policy.stats().target_delay_ms;

    assert!(recovered < elevated, "the target must adapt back down");
    assert!(
        recovered >= config().min_target_ms,
        "the target must never go below the minimum"
    );
}

#[test]
fn playout_delay_adjusts_in_bounded_steps() {
    let mut policy = JitterPolicy::new(config());
    for _ in 0..40 {
        feed_with_jitter(&mut policy, 2, 8.0);
    }
    let before = policy.stats().playout_delay_ms;
    // The helper restarts its two-packet sequence each call, so the last
    // arrival is the odd packet at 33 ms with the -8 ms wobble.
    let tick = policy.tick(30.0);
    assert_eq!(tick.state, PlaybackState::Playing);
    let delta = (tick.playout_delay_ms - before).abs();
    assert!(
        delta <= config().playout_step_ms + f64::EPSILON,
        "one tick must not move playout delay by {delta} ms"
    );
    assert!(tick.playout_delay_ms >= 0.0);
}

#[test]
fn a_stall_fast_forwards_and_returns_to_the_target() {
    let mut policy = JitterPolicy::new(config());
    for index in 0..5 {
        policy.on_packet(PacketTiming {
            arrival_ms: index as f64 * 33.0,
            rtp_timestamp_ms: index as f64 * 33.0,
        });
    }
    let target = policy.stats().target_delay_ms;
    assert_eq!(policy.stats().buffered, 5);

    let tick = policy.tick(2_000.0);
    assert_eq!(tick.state, PlaybackState::FastForward);
    assert_eq!(tick.buffered, 0);
    assert_eq!(tick.discarded, 5);
    assert_eq!(tick.playout_delay_ms, target);
    assert_eq!(policy.stats().fast_forwards, 1);

    let after = policy.tick(2_100.0);
    assert_eq!(after.state, PlaybackState::Idle);
}

#[test]
fn buffer_depth_is_bounded_under_backpressure() {
    let mut policy = JitterPolicy::new(config());
    let mut drops = 0;
    for index in 0..1_000 {
        let decision = policy.on_packet(PacketTiming {
            arrival_ms: index as f64 * 33.0,
            rtp_timestamp_ms: index as f64 * 33.0,
        });
        if decision.dropped_oldest {
            drops += 1;
        }
        assert!(decision.buffered <= config().max_depth);
    }
    let stats = policy.stats();
    assert_eq!(stats.buffered, config().max_depth);
    assert_eq!(drops, 1_000 - config().max_depth);
    assert_eq!(stats.discarded, drops as u64);
}

#[test]
fn consuming_a_frame_reduces_the_depth_once() {
    let mut policy = JitterPolicy::new(config());
    policy.on_packet(PacketTiming {
        arrival_ms: 0.0,
        rtp_timestamp_ms: 0.0,
    });
    assert!(policy.consume());
    assert!(!policy.consume());
    assert_eq!(policy.stats().consumed, 1);
    assert_eq!(policy.stats().buffered, 0);
}
