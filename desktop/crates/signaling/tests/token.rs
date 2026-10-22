//! Single-use pairing token contract tests (T044).

use std::hint::black_box;
use std::time::{Duration, Instant};

use remotecam_protocol::ReasonCode;
use remotecam_signaling::token::{TOKEN_BYTES, TOKEN_VALIDITY, TokenStore};

const TOKEN: [u8; TOKEN_BYTES] = [0x5a; TOKEN_BYTES];

#[test]
fn a_token_is_256_bits_and_single_use() {
    let store = TokenStore::new();
    store.issue(TOKEN, Duration::ZERO);
    assert_eq!(TOKEN.len() * 8, 256);
    assert!(store.is_issued());
    assert!(!store.is_consumed());

    assert_eq!(store.take(&TOKEN, Duration::from_millis(1)), Ok(()));
    assert!(store.is_consumed());
    assert_eq!(
        store.take(&TOKEN, Duration::from_millis(2)),
        Err(ReasonCode::TokenAlreadyUsed)
    );
}

#[test]
fn replay_of_a_consumed_token_is_token_already_used() {
    let store = TokenStore::new();
    store.issue(TOKEN, Duration::ZERO);
    store.take(&TOKEN, Duration::ZERO).expect("first use");
    assert_eq!(
        store.take(&TOKEN, Duration::from_secs(1)),
        Err(ReasonCode::TokenAlreadyUsed)
    );
}

#[test]
fn a_token_expires_after_120_seconds() {
    let store = TokenStore::new();
    store.issue(TOKEN, Duration::ZERO);
    assert_eq!(TOKEN_VALIDITY, Duration::from_secs(120));
    assert_eq!(
        store.take(&TOKEN, TOKEN_VALIDITY),
        Err(ReasonCode::TokenExpired)
    );
    assert!(!store.is_consumed(), "an expired token is not consumed");
}

#[test]
fn a_token_is_valid_just_before_the_boundary() {
    let store = TokenStore::new();
    store.issue(TOKEN, Duration::ZERO);
    assert_eq!(
        store.take(&TOKEN, TOKEN_VALIDITY - Duration::from_millis(1)),
        Ok(())
    );
}

#[test]
fn a_wrong_token_is_token_invalid() {
    let store = TokenStore::new();
    store.issue(TOKEN, Duration::ZERO);
    let mut wrong = TOKEN;
    wrong[0] ^= 0xff;
    assert_eq!(
        store.take(&wrong, Duration::ZERO),
        Err(ReasonCode::TokenInvalid)
    );
    assert!(!store.is_consumed());
}

#[test]
fn no_token_is_token_invalid() {
    let store = TokenStore::new();
    assert_eq!(
        store.take(&TOKEN, Duration::ZERO),
        Err(ReasonCode::TokenInvalid)
    );
}

#[test]
fn teardown_removes_the_token_and_invalidation_blocks_reuse() {
    let store = TokenStore::new();
    store.issue(TOKEN, Duration::ZERO);
    store.invalidate();
    assert_eq!(
        store.take(&TOKEN, Duration::ZERO),
        Err(ReasonCode::TokenAlreadyUsed),
        "an invalidated token cannot be consumed"
    );

    store.clear();
    assert!(!store.is_issued());
    assert_eq!(
        store.take(&TOKEN, Duration::ZERO),
        Err(ReasonCode::TokenInvalid)
    );
}

#[test]
fn comparison_is_constant_time_across_matching_prefixes() {
    // A short-circuiting comparison returns sooner when the first byte differs.
    // The bound is deliberately generous: this test exists to catch an accidental
    // `==`, not to measure the CPU.
    let store = TokenStore::new();
    store.issue(TOKEN, Duration::ZERO);
    let mut first_byte_wrong = TOKEN;
    first_byte_wrong[0] ^= 0xff;
    let mut last_byte_wrong = TOKEN;
    last_byte_wrong[31] ^= 0xff;

    let iterations = 20_000;
    let mut samples = Vec::new();
    for candidate in [first_byte_wrong, last_byte_wrong] {
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = black_box(store.take(black_box(&candidate), Duration::ZERO));
        }
        samples.push(start.elapsed());
    }

    let faster = samples[0].min(samples[1]);
    let slower = samples[0].max(samples[1]);
    if faster >= Duration::from_millis(1) {
        assert!(
            slower.as_secs_f64() < faster.as_secs_f64() * 5.0,
            "the comparison appears to short-circuit: {samples:?}"
        );
    }
}
