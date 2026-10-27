use std::sync::Arc;
use std::time::Duration;

use crate::nv12::Nv12Frame;
use crate::scaler;

pub const OUTPUT_FRAME_RATE: u32 = 30;
pub const MAX_REPEAT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Vga,
    Hd720,
    FullHd1080,
}

impl OutputMode {
    pub const ALL: [Self; 3] = [Self::Vga, Self::Hd720, Self::FullHd1080];

    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Vga => (640, 480),
            Self::Hd720 => (1280, 720),
            Self::FullHd1080 => (1920, 1080),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Paced {
    Fresh(Arc<Nv12Frame>),
    Repeated(Arc<Nv12Frame>),
    Absent,
}

pub struct FramePacer {
    mode: OutputMode,
    pending: Option<Arc<Nv12Frame>>,
    latest: Option<(Arc<Nv12Frame>, Duration)>,
}

impl FramePacer {
    pub fn new(mode: OutputMode) -> Self {
        Self {
            mode,
            pending: None,
            latest: None,
        }
    }

    pub fn offer(&mut self, frame: Nv12Frame, now: Duration) {
        let (width, height) = self.mode.dimensions();
        let frame = if (frame.width(), frame.height()) == (width, height) {
            frame
        } else {
            scaler::fit(&frame, width, height)
        };
        let frame = Arc::new(frame);
        self.pending = Some(Arc::clone(&frame));
        self.latest = Some((frame, now));
    }

    pub fn tick(&mut self, now: Duration) -> Paced {
        if let Some(frame) = self.pending.take() {
            return Paced::Fresh(frame);
        }
        match &self.latest {
            Some((frame, arrival)) if now.saturating_sub(*arrival) <= MAX_REPEAT => {
                Paced::Repeated(Arc::clone(frame))
            }
            _ => Paced::Absent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn frame(width: u32, height: u32, timestamp: u32) -> Nv12Frame {
        let size = width as usize * height as usize * 3 / 2;
        Nv12Frame::new(width, height, timestamp, vec![9; size]).expect("frame")
    }

    #[test]
    fn the_published_modes_are_vga_720p_and_1080p_at_thirty_fps() {
        let sizes: Vec<_> = OutputMode::ALL.iter().map(|m| m.dimensions()).collect();
        assert_eq!(sizes, [(640, 480), (1280, 720), (1920, 1080)]);
        assert_eq!(OUTPUT_FRAME_RATE, 30);
    }

    #[test]
    fn a_frame_already_at_the_mode_size_is_published_unscaled() {
        let mut pacer = FramePacer::new(OutputMode::Hd720);
        pacer.offer(frame(1280, 720, 1), ms(0));
        let Paced::Fresh(published) = pacer.tick(ms(0)) else {
            panic!("expected a fresh frame");
        };
        assert_eq!((published.width(), published.height()), (1280, 720));
        assert_eq!(published.rtp_timestamp(), 1);
    }

    #[test]
    fn a_frame_of_another_size_is_rescaled_to_the_mode() {
        let mut pacer = FramePacer::new(OutputMode::Vga);
        pacer.offer(frame(320, 240, 5), ms(0));
        let Paced::Fresh(published) = pacer.tick(ms(0)) else {
            panic!("expected a fresh frame");
        };
        assert_eq!((published.width(), published.height()), (640, 480));
    }

    #[test]
    fn the_last_frame_is_repeated_to_complete_the_rate() {
        let mut pacer = FramePacer::new(OutputMode::Vga);
        pacer.offer(frame(640, 480, 1), ms(0));
        let Paced::Fresh(first) = pacer.tick(ms(0)) else {
            panic!("expected a fresh frame");
        };
        let Paced::Repeated(again) = pacer.tick(ms(33)) else {
            panic!("expected a repeat");
        };
        assert!(
            Arc::ptr_eq(&first, &again),
            "repeating must not copy pixels"
        );
    }

    #[test]
    fn repetition_stops_after_two_seconds_and_the_absence_is_signalled() {
        let mut pacer = FramePacer::new(OutputMode::Vga);
        assert_eq!(pacer.tick(ms(0)), Paced::Absent, "nothing yet to repeat");
        pacer.offer(frame(640, 480, 1), ms(100));
        assert!(matches!(pacer.tick(ms(100)), Paced::Fresh(_)));
        assert!(matches!(pacer.tick(ms(2_100)), Paced::Repeated(_)));
        assert_eq!(pacer.tick(ms(2_101)), Paced::Absent);
        assert_eq!(pacer.tick(ms(5_000)), Paced::Absent);
    }

    #[test]
    fn a_new_frame_after_an_absence_resumes_the_video() {
        let mut pacer = FramePacer::new(OutputMode::Vga);
        pacer.offer(frame(640, 480, 1), ms(0));
        pacer.tick(ms(0));
        assert_eq!(pacer.tick(ms(3_000)), Paced::Absent);
        pacer.offer(frame(640, 480, 2), ms(3_100));
        let Paced::Fresh(resumed) = pacer.tick(ms(3_100)) else {
            panic!("expected a fresh frame");
        };
        assert_eq!(resumed.rtp_timestamp(), 2);
    }

    #[test]
    fn only_the_newest_offered_frame_is_published() {
        let mut pacer = FramePacer::new(OutputMode::Vga);
        pacer.offer(frame(640, 480, 1), ms(0));
        pacer.offer(frame(640, 480, 2), ms(1));
        let Paced::Fresh(published) = pacer.tick(ms(2)) else {
            panic!("expected a fresh frame");
        };
        assert_eq!(published.rtp_timestamp(), 2);
    }
}
