use std::sync::Arc;

use remotecam_session_core::{Clock, MediaFrame, MediaPort};

use crate::core::MediaCore;
use crate::nv12::Nv12Frame;

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

    pub fn current_frame(&self) -> Option<&Nv12Frame> {
        self.current.as_ref()
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
        let frame = self.core.ring().pop()?;
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
