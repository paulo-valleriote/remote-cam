use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use remotecam_session_core::AV_DRIFT_LIMIT;

use crate::nv12::Nv12Frame;

/// The A/V Drift the presentation aims for (TDD §5.5). The hard limit that ends the
/// session is `remotecam_session_core::AV_DRIFT_LIMIT`.
pub const DRIFT_TARGET: Duration = Duration::from_millis(20);

const VIDEO_CLOCK_HZ: u64 = 90_000;
/// Half a frame at 30 fps: a frame this much ahead of the audio is closer to it than
/// the next one would be, so presenting it keeps the offset inside the target.
const EARLY_TOLERANCE: Duration = Duration::from_micros(16_667);
/// Without a publication for this long the audio is gone and the video stops waiting.
const AUDIO_STALE_AFTER: Duration = Duration::from_millis(100);
const QUEUE_CAPACITY: usize = 16;
/// About one second of 30 fps video. The drift is the median offset over this window,
/// so a frame delayed by loss recovery is jitter and only a sustained offset is drift.
const DRIFT_WINDOW: usize = 31;

#[derive(Debug, Clone, Copy)]
struct Reading {
    position: Duration,
    at: Duration,
}

/// The only thing the audio chain tells the video chain: how far the audio has played.
/// It carries no samples and no frames, so the two chains never share a buffer.
#[derive(Debug, Clone, Default)]
pub struct AudioClock(Arc<Mutex<Option<Reading>>>);

impl AudioClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// The audio chain reports that media time `position` began playing at `at`.
    pub fn publish(&self, position: Duration, at: Duration) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(Reading { position, at });
    }

    /// The last publication as `(position, at)`, without interpolation or staleness.
    pub fn latest(&self) -> Option<(Duration, Duration)> {
        let reading = (*self.0.lock().unwrap_or_else(PoisonError::into_inner))?;
        Some((reading.position, reading.at))
    }

    /// The audio position at `now`, or `None` when nothing is playing or the last
    /// publication is stale.
    pub fn read(&self, now: Duration) -> Option<Duration> {
        let reading = (*self.0.lock().unwrap_or_else(PoisonError::into_inner))?;
        let elapsed = now.saturating_sub(reading.at);
        (elapsed <= AUDIO_STALE_AFTER).then(|| reading.position + elapsed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// The audio clock leads and the video follows it.
    AudioMaster,
    /// No audio clock: the video is presented as it arrives.
    VideoFree,
}

#[derive(Debug)]
pub struct SyncTick {
    pub mode: SyncMode,
    pub frame: Option<Nv12Frame>,
    /// The absolute offset between the audio clock and the video, when there is one
    /// to measure: a frame just presented, or a held frame far ahead of the audio.
    pub drift: Option<Duration>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncStats {
    pub master_presented: u64,
    pub free_presented: u64,
    pub skipped: u64,
    /// Largest windowed drift: the sustained offset the hard limit judges.
    pub max_drift: Duration,
    /// Largest raw offset of any single presented frame, unfiltered. Loss recovery
    /// shows up here as jitter, where `max_drift` ignores it.
    pub max_frame_offset: Duration,
}

struct Queued {
    media_time: Duration,
    frame: Nv12Frame,
}

/// Presents decoded video against the audio clock. Its queue holds only video frames.
pub struct VideoSync {
    clock: AudioClock,
    queue: VecDeque<Queued>,
    origin: Option<u64>,
    last_extended: u64,
    offsets: VecDeque<i64>,
    stats: SyncStats,
}

impl VideoSync {
    pub fn new(clock: AudioClock) -> Self {
        Self {
            clock,
            queue: VecDeque::new(),
            origin: None,
            last_extended: 0,
            offsets: VecDeque::with_capacity(DRIFT_WINDOW),
            stats: SyncStats::default(),
        }
    }

    pub fn stats(&self) -> SyncStats {
        self.stats
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Lifts the 32-bit RTP timestamp onto a 64-bit line; the wrapping signed
    /// difference is the distance across a wrap.
    fn extend(&mut self, timestamp: u32) -> u64 {
        let delta = i64::from(timestamp.wrapping_sub(self.last_extended as u32) as i32);
        self.last_extended = self.last_extended.saturating_add_signed(delta);
        self.last_extended
    }

    /// Queues a decoded frame. Its media time is measured from the first frame, as the
    /// audio's is from its own first packet: both streams start at the same capture
    /// instant on the v1 phone.
    pub fn offer(&mut self, frame: Nv12Frame) {
        let extended = match self.origin {
            Some(_) => self.extend(frame.rtp_timestamp()),
            None => {
                // The high bit leaves room below the first timestamp for a frame that
                // arrives out of order.
                self.last_extended = u64::from(frame.rtp_timestamp()) | (1 << 32);
                self.origin = Some(self.last_extended);
                self.last_extended
            }
        };
        let ticks = extended.saturating_sub(self.origin.unwrap_or(extended));
        let media_time = Duration::from_micros(ticks * 1_000_000 / VIDEO_CLOCK_HZ);
        if self.queue.len() == QUEUE_CAPACITY {
            self.queue.pop_front();
            self.stats.skipped += 1;
        }
        self.queue.push_back(Queued { media_time, frame });
    }

    pub fn tick(&mut self, now: Duration) -> SyncTick {
        match self.clock.read(now) {
            Some(audio) => self.follow(audio),
            None => self.run_free(),
        }
    }

    fn run_free(&mut self) -> SyncTick {
        self.offsets.clear();
        let newest = self.queue.pop_back();
        self.stats.skipped += self.queue.len() as u64;
        self.queue.clear();
        let frame = newest.map(|queued| queued.frame);
        if frame.is_some() {
            self.stats.free_presented += 1;
        }
        SyncTick {
            mode: SyncMode::VideoFree,
            frame,
            drift: None,
        }
    }

    /// Adds the signed offset of a presented frame and returns the windowed drift.
    fn record_offset(&mut self, audio: Duration, media_time: Duration) -> Duration {
        let micros = |duration: Duration| i64::try_from(duration.as_micros()).unwrap_or(i64::MAX);
        if self.offsets.len() == DRIFT_WINDOW {
            self.offsets.pop_front();
        }
        self.offsets.push_back(micros(audio) - micros(media_time));
        let mut sorted: Vec<i64> = self.offsets.iter().copied().collect();
        sorted.sort_unstable();
        Duration::from_micros(sorted[sorted.len() / 2].unsigned_abs())
    }

    fn follow(&mut self, audio: Duration) -> SyncTick {
        let due = self
            .queue
            .iter()
            .take_while(|queued| queued.media_time <= audio + EARLY_TOLERANCE)
            .count();
        if due == 0 {
            let ahead = self
                .queue
                .front()
                .map(|queued| queued.media_time - audio)
                .filter(|ahead| *ahead > AV_DRIFT_LIMIT);
            if let Some(ahead) = ahead {
                self.stats.max_drift = self.stats.max_drift.max(ahead);
            }
            return SyncTick {
                mode: SyncMode::AudioMaster,
                frame: None,
                drift: ahead,
            };
        }
        self.stats.skipped += (due - 1) as u64;
        self.queue.drain(..due - 1);
        let presented = self.queue.pop_front().expect("a due frame");
        let drift = self.record_offset(audio, presented.media_time);
        self.stats.max_frame_offset = self
            .stats
            .max_frame_offset
            .max(presented.media_time.abs_diff(audio));
        self.stats.master_presented += 1;
        self.stats.max_drift = self.stats.max_drift.max(drift);
        SyncTick {
            mode: SyncMode::AudioMaster,
            frame: Some(presented.frame),
            drift: Some(drift),
        }
    }
}
