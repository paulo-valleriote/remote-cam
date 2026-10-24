//! The in-memory, single-use pairing token store.
//!
//! A token is 256 bits read from the QR payload. It lives in memory only, is
//! valid for 120 seconds and is invalidated by the first successful pairing.
//! The comparison is constant time: a timing difference is a defect, never an
//! optimisation opportunity.

use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use remotecam_protocol::ReasonCode;
use subtle::ConstantTimeEq;

/// Length of a pairing token in bytes.
pub const TOKEN_BYTES: usize = 32;
/// How long an issued token stays valid.
pub const TOKEN_VALIDITY: Duration = Duration::from_secs(120);

struct StoredToken {
    bytes: [u8; TOKEN_BYTES],
    issued_at: Duration,
    expires_at: Duration,
    consumed: bool,
}

impl Drop for StoredToken {
    fn drop(&mut self) {
        zeroize(&mut self.bytes);
    }
}

impl fmt::Debug for StoredToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredToken")
            .field("bytes", &"[elided]")
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .field("consumed", &self.consumed)
            .finish()
    }
}

impl StoredToken {
    fn is_expired(&self, now: Duration) -> bool {
        now >= self.expires_at
    }

    fn matches(&self, candidate: &[u8; TOKEN_BYTES]) -> bool {
        bool::from(self.bytes.ct_eq(candidate))
    }
}

/// The single token a pairing session may consume.
#[derive(Debug)]
pub struct TokenStore {
    token: Mutex<Option<StoredToken>>,
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenStore {
    /// Creates an empty store. A pairing cannot start until `issue` is called.
    pub fn new() -> Self {
        Self {
            token: Mutex::new(None),
        }
    }

    /// Creates a store holding a fresh random token issued at `now`.
    pub fn with_random_token(now: Duration) -> (Self, [u8; TOKEN_BYTES]) {
        let store = Self::new();
        let token = store.issue_random(now);
        (store, token)
    }

    /// Issues a fresh random token, replacing any previous one.
    pub fn issue_random(&self, now: Duration) -> [u8; TOKEN_BYTES] {
        let mut bytes = [0_u8; TOKEN_BYTES];
        rand::fill(&mut bytes);
        self.issue(bytes, now);
        bytes
    }

    /// Issues a caller-supplied token. Exists so a test or the evidence harness
    /// can replay a previously captured token.
    pub fn issue(&self, bytes: [u8; TOKEN_BYTES], now: Duration) {
        let mut guard = self.lock();
        *guard = Some(StoredToken {
            bytes,
            issued_at: now,
            expires_at: now + TOKEN_VALIDITY,
            consumed: false,
        });
    }

    /// Whether a token is currently present (issued and not yet cleared).
    pub fn is_issued(&self) -> bool {
        self.lock().is_some()
    }

    /// Whether the present token has been consumed.
    pub fn is_consumed(&self) -> bool {
        self.lock().as_ref().is_some_and(|token| token.consumed)
    }

    /// Validates and consumes a token.
    ///
    /// The checks run in a deterministic order so the reported code is stable:
    /// a mismatch is `token_invalid`, then an already-consumed token is
    /// `token_already_used`, then an expired token is `token_expired`.
    pub fn take(&self, candidate: &[u8; TOKEN_BYTES], now: Duration) -> Result<(), ReasonCode> {
        let mut guard = self.lock();
        let Some(token) = guard.as_mut() else {
            return Err(ReasonCode::TokenInvalid);
        };
        if !token.matches(candidate) {
            return Err(ReasonCode::TokenInvalid);
        }
        if token.consumed {
            return Err(ReasonCode::TokenAlreadyUsed);
        }
        if token.is_expired(now) {
            return Err(ReasonCode::TokenExpired);
        }
        token.consumed = true;
        Ok(())
    }

    /// Expires the present token immediately.
    ///
    /// `#[doc(hidden)]`: the pairing gate uses this to observe the expired-token
    /// rejection without waiting two minutes. It is not a product operation.
    #[doc(hidden)]
    pub fn expire(&self) {
        let mut guard = self.lock();
        if let Some(token) = guard.as_mut() {
            token.expires_at = Duration::ZERO;
        }
    }

    /// Invalidates the present token without consuming it.
    pub fn invalidate(&self) {
        let mut guard = self.lock();
        if let Some(token) = guard.as_mut() {
            token.consumed = true;
        }
    }

    /// Removes and zeroes the token. Called on session teardown.
    pub fn clear(&self) {
        let mut guard = self.lock();
        if let Some(token) = guard.as_mut() {
            zeroize(&mut token.bytes);
        }
        *guard = None;
    }

    /// When the present token expires, if one is issued.
    pub fn expires_at(&self) -> Option<Duration> {
        self.lock().as_ref().map(|token| token.expires_at)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<StoredToken>> {
        self.token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for TokenStore {
    fn drop(&mut self) {
        let guard = self
            .token
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(token) = guard.as_mut() {
            zeroize(&mut token.bytes);
        }
    }
}

/// Overwrites secret bytes without letting the compiler optimise the write away.
fn zeroize(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        // SAFETY: `byte` is a valid, aligned, exclusive reference for the write.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}
