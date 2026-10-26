use std::net::{IpAddr, Ipv6Addr};

use remotecam_pairing::throttle::{Admission, OriginThrottle, ThrottlePolicy};
use remotecam_session_core::Timestamp;

fn origin(last: u8) -> IpAddr {
    IpAddr::from([192, 168, 0, last])
}

fn at(millis: u64) -> Timestamp {
    Timestamp::from_millis(millis)
}

#[test]
fn the_sixth_attempt_within_a_minute_is_refused() {
    let throttle = OriginThrottle::new();

    for second in 0..5 {
        assert_eq!(
            throttle.admit(origin(10), at(second * 1_000)),
            Admission::Admitted
        );
    }

    assert!(matches!(
        throttle.admit(origin(10), at(5_000)),
        Admission::Refused { .. }
    ));
}

fn exhaust(throttle: &OriginThrottle, origin: IpAddr, start: u64) -> Admission {
    for _ in 0..5 {
        assert_eq!(throttle.admit(origin, at(start)), Admission::Admitted);
    }
    throttle.admit(origin, at(start))
}

#[test]
fn a_blocked_origin_stays_refused_until_the_block_ends() {
    let throttle = OriginThrottle::new();

    assert_eq!(
        exhaust(&throttle, origin(10), 0),
        Admission::Refused {
            blocked_for_millis: 60_000
        }
    );
    assert_eq!(
        throttle.admit(origin(10), at(59_999)),
        Admission::Refused {
            blocked_for_millis: 1
        }
    );
    assert_eq!(throttle.admit(origin(10), at(60_000)), Admission::Admitted);
}

#[test]
fn each_repeated_block_doubles_up_to_a_ceiling() {
    let throttle = OriginThrottle::new();
    let mut start = 0;
    let mut observed = Vec::new();

    for _ in 0..7 {
        let Admission::Refused { blocked_for_millis } = exhaust(&throttle, origin(10), start)
        else {
            panic!("the sixth attempt must be refused");
        };
        observed.push(blocked_for_millis);
        start += blocked_for_millis;
    }

    assert_eq!(
        observed,
        [60_000, 120_000, 240_000, 480_000, 900_000, 900_000, 900_000]
    );
}

#[test]
fn a_blocked_origin_does_not_affect_another_origin() {
    let throttle = OriginThrottle::new();
    exhaust(&throttle, origin(10), 0);

    for _ in 0..5 {
        assert_eq!(throttle.admit(origin(11), at(1_000)), Admission::Admitted);
    }
    assert!(matches!(
        throttle.admit(origin(10), at(1_000)),
        Admission::Refused { .. }
    ));
}

#[test]
fn attempts_older_than_a_minute_stop_counting() {
    let throttle = OriginThrottle::new();
    for _ in 0..4 {
        throttle.admit(origin(10), at(0));
    }

    for _ in 0..5 {
        assert_eq!(throttle.admit(origin(10), at(60_000)), Admission::Admitted);
    }
}

#[test]
fn a_quiet_origin_starts_over_at_the_first_block() {
    let throttle = OriginThrottle::new();
    exhaust(&throttle, origin(10), 0);
    exhaust(&throttle, origin(10), 60_000);

    let quiet = 180_000 + 30 * 60_000;
    assert_eq!(
        exhaust(&throttle, origin(10), quiet),
        Admission::Refused {
            blocked_for_millis: 60_000
        }
    );
}

fn v6(prefix_subnet: u16, host: u16) -> IpAddr {
    IpAddr::V6(Ipv6Addr::new(
        0x2001,
        0xdb8,
        0,
        prefix_subnet,
        0,
        0,
        0,
        host,
    ))
}

#[test]
fn addresses_inside_one_ipv6_64_prefix_share_one_budget() {
    let throttle = OriginThrottle::new();

    for host in 1..=5 {
        assert_eq!(throttle.admit(v6(1, host), at(0)), Admission::Admitted);
    }

    assert!(matches!(
        throttle.admit(v6(1, 6), at(0)),
        Admission::Refused { .. }
    ));
    assert_eq!(throttle.admit(v6(2, 1), at(0)), Admission::Admitted);
}

#[test]
fn an_ipv4_mapped_ipv6_address_is_the_same_origin_as_its_ipv4_address() {
    let throttle = OriginThrottle::new();
    let mapped = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0xc0a8, 0x000a));
    for _ in 0..5 {
        throttle.admit(origin(10), at(0));
    }

    assert!(matches!(
        throttle.admit(mapped, at(0)),
        Admission::Refused { .. }
    ));
}

fn small_table() -> OriginThrottle {
    OriginThrottle::with_policy(ThrottlePolicy {
        max_origins: 2,
        ..ThrottlePolicy::default()
    })
}

#[test]
fn a_full_table_evicts_an_unblocked_origin_before_a_blocked_one() {
    let throttle = small_table();
    exhaust(&throttle, origin(10), 0);
    throttle.admit(origin(11), at(1_000));

    throttle.admit(origin(12), at(2_000));

    assert_eq!(throttle.tracked_origins(), 2);
    assert!(matches!(
        throttle.admit(origin(10), at(3_000)),
        Admission::Refused { .. }
    ));
}

#[test]
fn a_full_table_releases_origins_that_stayed_quiet() {
    let throttle = small_table();
    throttle.admit(origin(10), at(0));
    throttle.admit(origin(11), at(0));

    throttle.admit(origin(12), at(30 * 60_000));

    assert_eq!(throttle.tracked_origins(), 1);
}
