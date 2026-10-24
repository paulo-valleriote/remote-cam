//! Decoded-frame handoff into the injection ring buffer (T071).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The network path must never wait for the camera. This module publishes a
//! decoded NV12 frame into the non-blocking ring buffer owned by the injection
//! crate, counts every discarded frame, and drops frames that belong to a
//! session that has already been replaced. A late frame from the previous
//! session is a stale frame, not a frame for the new session.

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use remotecam_injection::{
    Frame, FrameReader, PublishedFrame, RingBuffer, RingBufferError, RingBufferStats,
};

/// Why a frame was not published.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffError {
    /// The frame carries the epoch of a session that is no longer current.
    StaleEpoch { epoch: u64, current: u64 },
    /// The handoff has been shut down.
    ShutDown,
    /// The ring buffer rejected its configuration.
    RingBuffer(RingBufferError),
    /// The ring buffer closed between the epoch check and the publication.
    BufferClosed,
}

impl std::fmt::Display for HandoffError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleEpoch { epoch, current } => {
                write!(
                    formatter,
                    "frame epoch {epoch} is stale; current epoch is {current}"
                )
            }
            Self::ShutDown => write!(formatter, "the frame handoff has been shut down"),
            Self::RingBuffer(error) => write!(formatter, "ring buffer error: {error}"),
            Self::BufferClosed => write!(formatter, "the ring buffer closed during publication"),
        }
    }
}

impl std::error::Error for HandoffError {}

/// Handoff counters used by the resource and stall evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffStats {
    pub published: u64,
    pub discarded: u64,
    pub stale_discarded: u64,
    pub session_replacements: u64,
    pub current_epoch: u64,
    pub buffer: RingBufferStats,
}

/// What changed when a session was replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub previous_epoch: u64,
    pub current_epoch: u64,
    /// Frames that were still pending and are therefore discarded.
    pub discarded_pending: usize,
}

struct HandoffState {
    epoch: u64,
    buffer: RingBuffer,
    shutdown: bool,
}

/// One writer, many readers, with a session epoch guard.
pub struct FrameHandoff {
    capacity: usize,
    state: Mutex<HandoffState>,
    published: AtomicU64,
    discarded: AtomicU64,
    stale_discarded: AtomicU64,
    session_replacements: AtomicU64,
}

impl FrameHandoff {
    pub fn new(capacity: usize) -> Result<Self, HandoffError> {
        let buffer = RingBuffer::new(capacity).map_err(HandoffError::RingBuffer)?;
        Ok(Self {
            capacity,
            state: Mutex::new(HandoffState {
                epoch: 1,
                buffer,
                shutdown: false,
            }),
            published: AtomicU64::new(0),
            discarded: AtomicU64::new(0),
            stale_discarded: AtomicU64::new(0),
            session_replacements: AtomicU64::new(0),
        })
    }

    pub fn epoch(&self) -> u64 {
        self.lock().epoch
    }

    /// Publish a frame without ever blocking the network path.
    pub fn publish(&self, epoch: u64, frame: Frame) -> Result<PublishedFrame, HandoffError> {
        let state = self.lock();
        if state.shutdown {
            return Err(HandoffError::ShutDown);
        }
        if epoch != state.epoch {
            drop(state);
            self.stale_discarded.fetch_add(1, Ordering::Relaxed);
            return Err(HandoffError::StaleEpoch {
                epoch,
                current: self.epoch(),
            });
        }
        let writer = state.buffer.writer();
        drop(state);

        match writer.publish(frame) {
            Some(published) => {
                self.published.fetch_add(1, Ordering::Relaxed);
                if published.discarded_oldest {
                    self.discarded.fetch_add(1, Ordering::Relaxed);
                }
                Ok(published)
            }
            None => Err(HandoffError::BufferClosed),
        }
    }

    /// Reader bound to the current session epoch.
    pub fn reader(&self) -> (u64, FrameReader) {
        let state = self.lock();
        (state.epoch, state.buffer.reader())
    }

    /// Install a fresh ring buffer and invalidate frames from the previous one.
    pub fn replace_session(&self) -> SessionSnapshot {
        let mut state = self.lock();
        let previous_epoch = state.epoch;
        let discarded_pending = state.buffer.stats().depth;
        state.buffer.shutdown();
        state.buffer = RingBuffer::new(self.capacity).expect("validated capacity");
        state.epoch += 1;
        let current_epoch = state.epoch;
        drop(state);

        self.discarded
            .fetch_add(discarded_pending as u64, Ordering::Relaxed);
        self.session_replacements.fetch_add(1, Ordering::Relaxed);

        SessionSnapshot {
            previous_epoch,
            current_epoch,
            discarded_pending,
        }
    }

    pub fn stats(&self) -> HandoffStats {
        let state = self.lock();
        HandoffStats {
            published: self.published.load(Ordering::Relaxed),
            discarded: self.discarded.load(Ordering::Relaxed),
            stale_discarded: self.stale_discarded.load(Ordering::Relaxed),
            session_replacements: self.session_replacements.load(Ordering::Relaxed),
            current_epoch: state.epoch,
            buffer: state.buffer.stats(),
        }
    }

    /// Release pending frames and wake every reader. Idempotent.
    pub fn shutdown(&self) {
        let mut state = self.lock();
        state.shutdown = true;
        state.buffer.shutdown();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HandoffState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Session lifecycle events that must be idempotent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionEvent {
    /// The remote peer ended the session.
    Ended { epoch: u64 },
    /// The local pipeline released the session's resources.
    Released { epoch: u64 },
}

/// Applies lifecycle events exactly once per epoch.
///
/// A duplicated `Ended` event is common when both the RTCP BYE and the peer
/// connection state change arrive; it must not release the same session twice.
#[derive(Debug, Default)]
pub struct SessionEvents {
    applied: Mutex<HashSet<SessionEvent>>,
}

impl SessionEvents {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when this is the first time the event was applied.
    pub fn apply(&self, event: SessionEvent) -> bool {
        let mut applied = self
            .applied
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        applied.insert(event)
    }

    pub fn applied_count(&self) -> usize {
        self.applied
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}
