use std::sync::Arc;
use std::time::Duration;

use remotecam_session_core::{Clock, MediaFrame, MediaPort};

use crate::core::MediaCore;
use crate::nv12::Nv12Frame;
use crate::output::{FramePacer, OutputMode, Paced};

pub struct RingMediaPort<C> {
    core: Arc<MediaCore>,
    clock: C,
    started: bool,
    next_sequence: u64,
    current: Option<Arc<Nv12Frame>>,
    last_dimensions: Option<(u32, u32)>,
    pacer: Option<FramePacer>,
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
            pacer: None,
        }
    }

    /// Publishes `mode` at its frame rate: every `next_frame` call is one consumer
    /// tick, which yields the newest frame rescaled to the mode, the previous one
    /// repeated, or nothing once the video has been absent for too long.
    #[must_use]
    pub fn with_output_mode(mut self, mode: OutputMode) -> Self {
        self.pacer = Some(FramePacer::new(mode));
        self
    }

    pub fn current_frame(&self) -> Option<&Nv12Frame> {
        self.current.as_deref()
    }

    fn pop_for_consumer(&mut self) -> Option<Arc<Nv12Frame>> {
        let Some(pacer) = self.pacer.as_mut() else {
            return self.core.ring().pop().map(Arc::new);
        };
        let now = Duration::from_millis(self.clock.now().as_millis());
        // A consumer that fell behind jumps to the newest picture.
        let newest = std::iter::from_fn(|| self.core.ring().pop()).last();
        if let Some(frame) = newest {
            pacer.offer(frame, now);
        }
        match pacer.tick(now) {
            Paced::Fresh(frame) | Paced::Repeated(frame) => Some(frame),
            Paced::Absent => None,
        }
    }
}

impl<C> RingMediaPort<C> {
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
        let following_sequence = self.next_sequence.checked_add(1)?;
        let Some(frame) = self.pop_for_consumer() else {
            if self.pacer.is_some() {
                // Absent video: no stale picture stays exposed.
                self.current = None;
            }
            return None;
        };
        let sequence = self.next_sequence;
        self.next_sequence = following_sequence;
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
