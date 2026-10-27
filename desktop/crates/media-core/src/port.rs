//! The real Media Port: serves decoded frames from the Frame Ring to the
//! Session Core as `MediaFrame` events.

use std::sync::Arc;

use remotecam_session_core::{Clock, MediaFrame, MediaPort};

use crate::core::MediaCore;
use crate::nv12::Nv12Frame;

/// A [`MediaPort`] over a [`MediaCore`]. The `MediaFrame` is the arrival event
/// (sequence and injected-clock timestamp); the pixels of the frame just
/// delivered stay readable through [`RingMediaPort::current_frame`].
pub struct RingMediaPort<C> {
    core: Arc<MediaCore>,
    clock: C,
    started: bool,
    next_sequence: u64,
    current: Option<Nv12Frame>,
    last_dimensions: Option<(u32, u32)>,
}

impl<C: Clock> RingMediaPort<C> {
    pub fn new(core: Arc<MediaCore>, clock: C) -> Self {
        Self {
            core,
            clock,
            started: false,
            next_sequence: 0,
            current: None,
            last_dimensions: None,
        }
    }

    /// The NV12 frame behind the last `MediaFrame` returned by `next_frame`.
    pub fn current_frame(&self) -> Option<&Nv12Frame> {
        self.current.as_ref()
    }
}

impl<C> RingMediaPort<C> {
    /// Width and height of the last frame delivered; kept after `stop` so a
    /// report can still describe what the session carried.
    pub fn last_dimensions(&self) -> Option<(u32, u32)> {
        self.last_dimensions
    }
}

impl<C: Clock> MediaPort for RingMediaPort<C> {
    fn start(&mut self) {
        self.started = true;
    }

    fn next_frame(&mut self) -> Option<MediaFrame> {
        if !self.started {
            return None;
        }
        let frame = self.core.ring().pop()?;
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.checked_add(1)?;
        self.last_dimensions = Some((frame.width(), frame.height()));
        self.current = Some(frame);
        Some(MediaFrame {
            sequence,
            timestamp: self.clock.now(),
        })
    }

    fn stop(&mut self) {
        self.started = false;
        self.current = None;
        self.core.ring().clear();
    }
}
