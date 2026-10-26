//! Deterministic implementations of the injected Session Core ports.
//!
//! These sources are reproducible by construction and are intended for tests
//! and the headless harness. They are not cryptographic and must never generate
//! production secrets.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Clock, RandomSource, Timestamp};

/// A clock that only moves when the caller advances it.
///
/// Clones share one position, so a handle kept outside the core advances the
/// clock the core reads.
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    now_millis: Arc<AtomicU64>,
}

impl ManualClock {
    /// Creates a manual clock positioned at `start`.
    pub fn new(start: Timestamp) -> Self {
        Self {
            now_millis: Arc::new(AtomicU64::new(start.as_millis())),
        }
    }

    /// Moves the clock forward by `millis`.
    pub fn advance(&self, millis: u64) {
        self.now_millis.fetch_add(millis, Ordering::SeqCst);
    }

    pub fn set(&self, now: Timestamp) {
        self.now_millis.store(now.as_millis(), Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.now_millis.load(Ordering::SeqCst))
    }
}

/// A non-cryptographic byte source seeded with a single `u64`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeededRandomSource {
    state: u64,
}

impl SeededRandomSource {
    /// Creates a source whose stream is fixed by `seed`.
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^ (mixed >> 31)
    }
}

impl RandomSource for SeededRandomSource {
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        for chunk in destination.chunks_mut(8) {
            let value = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&value[..chunk.len()]);
        }
    }
}
