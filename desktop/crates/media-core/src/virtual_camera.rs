use std::fmt;
use std::sync::Arc;
#[cfg(not(windows))]
use std::sync::Mutex;
#[cfg(not(windows))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::frame_ring::FrameRing;
use crate::nv12::Nv12Frame;
use crate::output::{FramePacer, OutputMode, Paced};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    NotStarted,
    AlreadyStarted,
    Rejected(String),
}

impl fmt::Display for PublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStarted => write!(formatter, "virtual camera is not started"),
            Self::AlreadyStarted => write!(formatter, "virtual camera is already started"),
            Self::Rejected(reason) => write!(formatter, "virtual camera rejected: {reason}"),
        }
    }
}

impl std::error::Error for PublishError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Nv12,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub timestamp: u32,
    pub sequence: u64,
}

impl FrameHeader {
    pub fn describing(frame: &Nv12Frame, sequence: u64) -> Self {
        Self {
            width: frame.width(),
            height: frame.height(),
            stride: frame.width(),
            format: PixelFormat::Nv12,
            timestamp: frame.rtp_timestamp(),
            sequence,
        }
    }
}

pub trait FrameSource {
    fn pop_newest(&self) -> Option<Nv12Frame>;
}

impl FrameSource for FrameRing<Nv12Frame> {
    fn pop_newest(&self) -> Option<Nv12Frame> {
        std::iter::from_fn(|| self.pop()).last()
    }
}

impl FrameSource for crate::core::MediaCore {
    fn pop_newest(&self) -> Option<Nv12Frame> {
        self.ring().pop_newest()
    }
}

impl<T: FrameSource + ?Sized> FrameSource for Arc<T> {
    fn pop_newest(&self) -> Option<Nv12Frame> {
        (**self).pop_newest()
    }
}

pub trait VirtualCameraPublisher {
    fn start(&mut self, modes: &[OutputMode]) -> Result<(), PublishError>;
    fn publish(&mut self, header: FrameHeader, frame: Arc<Nv12Frame>) -> Result<(), PublishError>;
    fn stop(&mut self);
}

pub struct VirtualCameraProducer<S, P: VirtualCameraPublisher> {
    source: S,
    publisher: P,
    mode: OutputMode,
    pacer: FramePacer,
    started: bool,
    next_sequence: u64,
}

impl<S: FrameSource, P: VirtualCameraPublisher> VirtualCameraProducer<S, P> {
    pub fn new(source: S, publisher: P, mode: OutputMode) -> Self {
        Self {
            source,
            publisher,
            mode,
            pacer: FramePacer::new(mode),
            started: false,
            next_sequence: 0,
        }
    }

    pub fn start(&mut self) -> Result<(), PublishError> {
        if self.started {
            return Err(PublishError::AlreadyStarted);
        }
        self.publisher.start(&OutputMode::ALL)?;
        self.started = true;
        self.pacer = FramePacer::new(self.mode);
        self.next_sequence = 0;
        Ok(())
    }

    pub fn pump(&mut self, now: Duration) -> Result<(), PublishError> {
        if !self.started {
            return Err(PublishError::NotStarted);
        }
        if let Some(frame) = self.source.pop_newest() {
            self.pacer.offer(frame, now);
        }
        match self.pacer.tick(now) {
            Paced::Fresh(frame) | Paced::Repeated(frame) => {
                let header = FrameHeader::describing(&frame, self.next_sequence);
                self.publisher.publish(header, frame)?;
                self.next_sequence += 1;
                Ok(())
            }
            Paced::Absent => Ok(()),
        }
    }

    pub fn stop(&mut self) {
        if self.started {
            self.started = false;
            self.publisher.stop();
        }
    }
}

#[cfg(not(windows))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Started(Vec<OutputMode>),
    Frame(FrameHeader),
    Stopped,
}

#[cfg(not(windows))]
#[derive(Default, Clone)]
pub struct RecordingPublisher {
    events: Arc<Mutex<Vec<Event>>>,
    open: Arc<AtomicUsize>,
}

#[cfg(not(windows))]
impl RecordingPublisher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Arc<Mutex<Vec<Event>>> {
        Arc::clone(&self.events)
    }

    pub fn headers(&self) -> Vec<FrameHeader> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::Frame(header) => Some(*header),
                _ => None,
            })
            .collect()
    }

    pub fn frames(&self) -> Vec<u32> {
        self.headers()
            .iter()
            .map(|header| header.timestamp)
            .collect()
    }

    pub fn open_resources(&self) -> usize {
        self.open.load(Ordering::SeqCst)
    }
}

#[cfg(not(windows))]
impl VirtualCameraPublisher for RecordingPublisher {
    fn start(&mut self, modes: &[OutputMode]) -> Result<(), PublishError> {
        if self
            .open
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(PublishError::AlreadyStarted);
        }
        self.events
            .lock()
            .unwrap()
            .push(Event::Started(modes.to_vec()));
        Ok(())
    }

    fn publish(&mut self, header: FrameHeader, _frame: Arc<Nv12Frame>) -> Result<(), PublishError> {
        if self.open_resources() == 0 {
            return Err(PublishError::NotStarted);
        }
        self.events.lock().unwrap().push(Event::Frame(header));
        Ok(())
    }

    fn stop(&mut self) {
        if self.open.swap(0, Ordering::SeqCst) == 1 {
            self.events.lock().unwrap().push(Event::Stopped);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::frame_ring::FrameRing;
    use crate::nv12::Nv12Frame;
    use crate::output::OutputMode;

    fn frame(width: u32, height: u32, timestamp: u32) -> Nv12Frame {
        let size = width as usize * height as usize * 3 / 2;
        Nv12Frame::new(width, height, timestamp, vec![9; size]).expect("frame")
    }

    #[test]
    fn the_camera_is_started_before_frames_and_stopped_last() {
        let ring = Arc::new(FrameRing::new());
        let publisher = RecordingPublisher::new();
        let events = publisher.events();
        let mut producer = VirtualCameraProducer::new(ring.clone(), publisher, OutputMode::Vga);
        producer.start().expect("start");
        ring.push(frame(640, 480, 1));
        producer.pump(Duration::ZERO).expect("pump");
        producer.stop();
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                Event::Started(OutputMode::ALL.to_vec()),
                Event::Frame(FrameHeader {
                    width: 640,
                    height: 480,
                    stride: 640,
                    format: PixelFormat::Nv12,
                    timestamp: 1,
                    sequence: 0
                }),
                Event::Stopped
            ]
        );
    }

    type Ring = Arc<FrameRing<Nv12Frame>>;
    type Producer = VirtualCameraProducer<Ring, RecordingPublisher>;

    fn producer_with(mode: OutputMode) -> (Ring, Producer, RecordingPublisher) {
        let ring = Arc::new(FrameRing::new());
        let publisher = RecordingPublisher::new();
        let probe = publisher.clone();
        (
            ring.clone(),
            VirtualCameraProducer::new(ring, publisher, mode),
            probe,
        )
    }

    #[test]
    fn frames_are_refused_before_start() {
        let (ring, mut producer, _) = producer_with(OutputMode::Vga);
        ring.push(frame(640, 480, 1));
        assert_eq!(producer.pump(Duration::ZERO), Err(PublishError::NotStarted));
    }

    #[test]
    fn only_the_newest_frame_of_a_backlog_is_published() {
        let (ring, mut producer, probe) = producer_with(OutputMode::Vga);
        producer.start().expect("start");
        for timestamp in 1..=3 {
            ring.push(frame(640, 480, timestamp));
        }
        producer.pump(Duration::ZERO).expect("pump");
        assert_eq!(probe.frames(), [3]);
    }

    #[test]
    fn nothing_is_published_while_the_ring_has_never_had_a_frame() {
        let (_, mut producer, probe) = producer_with(OutputMode::Vga);
        producer.start().expect("start");
        producer.pump(Duration::ZERO).expect("pump");
        assert!(probe.frames().is_empty());
    }

    #[test]
    fn frames_of_another_size_are_rescaled_to_the_selected_mode_before_publishing() {
        let (ring, mut producer, probe) = producer_with(OutputMode::Hd720);
        producer.start().expect("start");
        ring.push(frame(640, 480, 1));
        producer.pump(Duration::ZERO).expect("pump");
        assert_eq!(probe.headers()[0].width, 1280);
        assert_eq!(probe.headers()[0].height, 720);
    }

    #[test]
    fn stopping_releases_every_resource_and_is_idempotent() {
        let (ring, mut producer, probe) = producer_with(OutputMode::Vga);
        producer.start().expect("start");
        assert_eq!(probe.open_resources(), 1);
        producer.stop();
        producer.stop();
        assert_eq!(probe.open_resources(), 0);
        assert_eq!(probe.events().lock().unwrap().last(), Some(&Event::Stopped));
        assert_eq!(
            probe
                .events()
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == Event::Stopped)
                .count(),
            1
        );
        ring.push(frame(640, 480, 1));
        assert_eq!(producer.pump(Duration::ZERO), Err(PublishError::NotStarted));
    }

    #[test]
    fn starting_twice_is_refused_and_a_restart_after_stop_leaks_nothing() {
        let (_, mut producer, probe) = producer_with(OutputMode::Vga);
        producer.start().expect("start");
        assert_eq!(producer.start(), Err(PublishError::AlreadyStarted));
        producer.stop();
        producer.start().expect("restart");
        assert_eq!(probe.open_resources(), 1);
    }

    #[test]
    fn a_refused_frame_stays_in_the_ring_and_is_not_lost() {
        let (ring, mut producer, _) = producer_with(OutputMode::Vga);
        ring.push(frame(640, 480, 1));
        assert!(producer.pump(Duration::ZERO).is_err());
        assert_eq!(ring.len(), 1);
    }

    #[test]
    fn a_restart_never_republishes_a_frame_from_before_the_stop() {
        let (ring, mut producer, probe) = producer_with(OutputMode::Vga);
        producer.start().expect("start");
        ring.push(frame(640, 480, 1));
        producer.pump(Duration::ZERO).expect("pump");
        producer.stop();
        producer.start().expect("restart");
        producer.pump(Duration::from_millis(33)).expect("pump");
        assert_eq!(probe.frames(), [1]);
    }

    #[test]
    fn the_header_carries_a_sequence_that_grows_with_every_publication() {
        let (ring, mut producer, probe) = producer_with(OutputMode::Vga);
        producer.start().expect("start");
        ring.push(frame(640, 480, 7));
        producer.pump(Duration::ZERO).expect("pump");
        producer.pump(Duration::from_millis(33)).expect("repeat");
        let sequences: Vec<_> = probe.headers().iter().map(|h| h.sequence).collect();
        assert_eq!(sequences, [0, 1]);
        assert_eq!(probe.headers()[1].timestamp, 7);
    }

    #[test]
    fn a_slow_reader_never_blocks_the_network_side_and_sees_newest_first() {
        let (ring, mut producer, probe) = producer_with(OutputMode::Vga);
        producer.start().expect("start");
        let network = Arc::clone(&ring);
        let writer = std::thread::spawn(move || {
            let begin = std::time::Instant::now();
            for timestamp in 0..2_000_u32 {
                network.push(frame(640, 480, timestamp));
            }
            begin.elapsed()
        });
        for tick in 0..20_u64 {
            producer
                .pump(Duration::from_millis(tick * 33))
                .expect("pump");
            std::thread::sleep(Duration::from_millis(1));
        }
        let elapsed = writer.join().expect("network thread");
        assert!(
            elapsed < Duration::from_secs(5),
            "push stalled: {elapsed:?}"
        );
        assert!(
            ring.dropped() > 0,
            "a lagging reader must cost the oldest frames"
        );
        let frames = probe.frames();
        assert!(frames.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
