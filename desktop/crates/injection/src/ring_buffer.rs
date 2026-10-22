//! Non-blocking NV12 frame handoff between the media path and the camera.
//!
//! One writer publishes decoded frames; any number of readers consume them.
//! When the buffer is full the **oldest** frame is discarded so the producer is
//! never blocked and the consumer always sees the most recent video. Every
//! eviction increments [`RingBufferStats::discarded`] — dropping frames is
//! observable, never silent.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

/// Bytes per pixel row in a packed NV12 luma plane.
const NV12_PLANE_DIVISOR: usize = 2;

/// A validated NV12 frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Assigned by [`FrameWriter::publish`]; `0` before publication.
    pub sequence: u64,
    /// Monotonic capture timestamp in nanoseconds.
    pub timestamp_ns: u64,
    pub width: u32,
    pub height: u32,
    /// Bytes per row, including any padding.
    pub stride: usize,
    /// Luma plane followed by the interleaved chroma plane.
    pub data: Vec<u8>,
}

impl Frame {
    /// Build a frame, validating the dimensions against the NV12 layout.
    pub fn new_nv12(
        width: u32,
        height: u32,
        stride: usize,
        timestamp_ns: u64,
        data: Vec<u8>,
    ) -> Result<Self, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::ZeroDimension);
        }
        if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(FrameError::OddDimension { width, height });
        }
        if stride < width as usize {
            return Err(FrameError::StrideTooSmall { stride, width });
        }
        let expected = Self::expected_len(width, height, stride)?;
        if data.len() != expected {
            return Err(FrameError::WrongLength {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            sequence: 0,
            timestamp_ns,
            width,
            height,
            stride,
            data,
        })
    }

    /// Size in bytes of an NV12 image with the given geometry.
    pub fn expected_len(width: u32, height: u32, stride: usize) -> Result<usize, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::ZeroDimension);
        }
        if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(FrameError::OddDimension { width, height });
        }
        if stride < width as usize {
            return Err(FrameError::StrideTooSmall { stride, width });
        }
        let luma = stride
            .checked_mul(height as usize)
            .ok_or(FrameError::Overflow)?;
        luma.checked_add(luma / NV12_PLANE_DIVISOR)
            .ok_or(FrameError::Overflow)
    }

    pub fn luma(&self) -> &[u8] {
        let luma_len = self.stride * self.height as usize;
        &self.data[..luma_len]
    }

    pub fn chroma(&self) -> &[u8] {
        let luma_len = self.stride * self.height as usize;
        &self.data[luma_len..]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    ZeroDimension,
    OddDimension { width: u32, height: u32 },
    StrideTooSmall { stride: usize, width: u32 },
    WrongLength { expected: usize, actual: usize },
    MarkerDoesNotFit { width: u32, height: u32 },
    Overflow,
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDimension => write!(formatter, "frame dimensions must be non-zero"),
            Self::OddDimension { width, height } => write!(
                formatter,
                "NV12 requires even dimensions, received {width}x{height}"
            ),
            Self::StrideTooSmall { stride, width } => write!(
                formatter,
                "stride {stride} is smaller than the width {width}"
            ),
            Self::WrongLength { expected, actual } => write!(
                formatter,
                "NV12 payload is {actual} bytes, expected {expected}"
            ),
            Self::MarkerDoesNotFit { width, height } => write!(
                formatter,
                "the frame number marker does not fit a {width}x{height} frame"
            ),
            Self::Overflow => write!(formatter, "frame geometry overflows the address space"),
        }
    }
}

impl std::error::Error for FrameError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingBufferError {
    ZeroCapacity,
}

/// Returned when a timed read expires without a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadTimeout;

impl fmt::Display for ReadTimeout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "no frame arrived before the read timeout")
    }
}

impl std::error::Error for ReadTimeout {}

impl fmt::Display for RingBufferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => write!(formatter, "ring buffer capacity must be positive"),
        }
    }
}

impl std::error::Error for RingBufferError {}

/// Snapshot of the buffer counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingBufferStats {
    pub capacity: usize,
    pub depth: usize,
    pub published: u64,
    pub discarded: u64,
}

/// The frame that was accepted by the writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishedFrame {
    pub sequence: u64,
    /// Whether this publication evicted the oldest pending frame.
    pub discarded_oldest: bool,
}

struct State {
    slots: VecDeque<Frame>,
    next_sequence: u64,
    published: u64,
    discarded: u64,
    shutdown: bool,
    /// One cursor per live reader, so a frame is only reclaimed after every
    /// reader has consumed it.
    reader_cursors: Vec<Weak<AtomicU64>>,
}

struct Shared {
    capacity: usize,
    state: Mutex<State>,
    available: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A panicking publisher must not take the whole camera down: the state
        // is a plain queue, so recovering the guard is safe.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Cloneable handle to the shared buffer.
#[derive(Clone)]
pub struct RingBuffer {
    shared: Arc<Shared>,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Result<Self, RingBufferError> {
        if capacity == 0 {
            return Err(RingBufferError::ZeroCapacity);
        }
        Ok(Self {
            shared: Arc::new(Shared {
                capacity,
                state: Mutex::new(State {
                    slots: VecDeque::with_capacity(capacity),
                    next_sequence: 0,
                    published: 0,
                    discarded: 0,
                    shutdown: false,
                    reader_cursors: Vec::new(),
                }),
                available: Condvar::new(),
            }),
        })
    }

    pub fn writer(&self) -> FrameWriter {
        FrameWriter {
            shared: Arc::clone(&self.shared),
        }
    }

    pub fn reader(&self) -> FrameReader {
        let cursor = Arc::new(AtomicU64::new(0));
        self.shared
            .lock()
            .reader_cursors
            .push(Arc::downgrade(&cursor));
        FrameReader {
            shared: Arc::clone(&self.shared),
            cursor,
        }
    }

    pub fn stats(&self) -> RingBufferStats {
        let state = self.shared.lock();
        RingBufferStats {
            capacity: self.shared.capacity,
            depth: state.slots.len(),
            published: state.published,
            discarded: state.discarded,
        }
    }

    /// Close the buffer: release every queued frame and wake all readers.
    /// Idempotent, and safe to call concurrently with the producer.
    pub fn shutdown(&self) {
        let mut state = self.shared.lock();
        state.shutdown = true;
        state.slots.clear();
        drop(state);
        self.shared.available.notify_all();
    }

    pub fn is_shutdown(&self) -> bool {
        self.shared.lock().shutdown
    }
}

/// The single producer.
pub struct FrameWriter {
    shared: Arc<Shared>,
}

impl FrameWriter {
    /// Publish a frame without ever waiting for a consumer.
    ///
    /// Returns `None` once the buffer has been shut down.
    pub fn publish(&self, mut frame: Frame) -> Option<PublishedFrame> {
        let mut state = self.shared.lock();
        if state.shutdown {
            return None;
        }

        state.next_sequence += 1;
        let sequence = state.next_sequence;
        frame.sequence = sequence;

        let discarded_oldest = state.slots.len() == self.shared.capacity;
        if discarded_oldest {
            evict_one(&mut state);
        }
        state.slots.push_back(frame);
        state.published += 1;

        drop(state);
        self.shared.available.notify_all();

        Some(PublishedFrame {
            sequence,
            discarded_oldest,
        })
    }

    pub fn is_shutdown(&self) -> bool {
        self.shared.lock().shutdown
    }
}

/// An independent consumer cursor.
pub struct FrameReader {
    shared: Arc<Shared>,
    cursor: Arc<AtomicU64>,
}

impl FrameReader {
    /// Block until a frame newer than this reader's cursor is available.
    /// Returns `None` once the buffer is closed and drained.
    #[allow(clippy::should_implement_trait)] // a frame stream, not an Iterator
    pub fn next(&mut self) -> Option<Frame> {
        let mut state = self.shared.lock();
        loop {
            if let Some(frame) = take_next(&mut state, self.cursor()) {
                self.cursor.store(frame.sequence, Ordering::Release);
                return Some(frame);
            }
            if state.shutdown {
                return None;
            }
            state = self
                .shared
                .available
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// Wait for a frame for at most ``timeout``.
    ///
    /// Returns ``Ok(None)`` when the buffer closed, and ``Err(())`` when the
    /// timeout expired without a frame. The media source uses this to bound the
    /// time it spends inside ``RequestSample``.
    pub fn next_timeout(&mut self, timeout: Duration) -> Result<Option<Frame>, ReadTimeout> {
        let deadline = Instant::now() + timeout;
        let mut state = self.shared.lock();
        loop {
            if let Some(frame) = take_next(&mut state, self.cursor()) {
                self.cursor.store(frame.sequence, Ordering::Release);
                return Ok(Some(frame));
            }
            if state.shutdown {
                return Ok(None);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ReadTimeout);
            }
            let (next, _) = self
                .shared
                .available
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
        }
    }

    /// Take a frame if one is pending, without waiting.
    pub fn try_next(&mut self) -> Option<Frame> {
        let mut state = self.shared.lock();
        let frame = take_next(&mut state, self.cursor())?;
        self.cursor.store(frame.sequence, Ordering::Release);
        Some(frame)
    }

    /// Take the newest pending frame, skipping any older pending frames.
    pub fn latest(&mut self) -> Option<Frame> {
        let state = self.shared.lock();
        let frame = state.slots.back()?.clone();
        if frame.sequence <= self.cursor() {
            return None;
        }
        self.cursor.store(frame.sequence, Ordering::Release);
        Some(frame)
    }

    /// Highest sequence this reader has consumed.
    pub fn cursor(&self) -> u64 {
        self.cursor.load(Ordering::Acquire)
    }
}

/// Remove the oldest frame that every reader has already consumed.
///
/// When no such frame exists the consumer is behind: the oldest frame is
/// dropped and counted, which is what "drop-oldest" means under backpressure.
fn evict_one(state: &mut State) {
    state
        .reader_cursors
        .retain(|cursor| cursor.strong_count() > 0);
    let consumed_through = state
        .reader_cursors
        .iter()
        .filter_map(Weak::upgrade)
        .map(|cursor| cursor.load(Ordering::Acquire))
        .min()
        .unwrap_or(0);

    let removable = state
        .slots
        .iter()
        .position(|frame| frame.sequence <= consumed_through);
    match removable {
        Some(index) => {
            state.slots.remove(index);
        }
        None => {
            state.slots.pop_front();
            state.discarded += 1;
        }
    }
}

fn take_next(state: &mut State, cursor: u64) -> Option<Frame> {
    // Frames stay in the queue until an eviction or shutdown, so every reader
    // keeps its own cursor and no reader can steal a frame from another.
    state
        .slots
        .iter()
        .find(|frame| frame.sequence > cursor)
        .cloned()
}
